use std::fmt;
use std::process::ExitCode;

use aws_sdk_s3::config::http::HttpResponse;
use aws_sdk_s3::error::{ProvideErrorMetadata, SdkError};

use crate::cli::ErrorFormat;
use crate::errorformat;

/// Exit code of the AWS CLI on parameter validation errors.
const EXIT_PARAM_VALIDATION: u8 = 252;
/// Exit code of the AWS CLI when credentials are not found.
const EXIT_NO_CREDENTIALS: u8 = 253;
/// Exit code of the AWS CLI on service errors.
const EXIT_SERVICE: u8 = 254;
/// Exit code of the AWS CLI on other errors.
const EXIT_GENERAL: u8 = 255;

#[derive(Debug)]
pub struct Error {
    code: u8,
    message: String,
    /// The code and the message of the error for the structured formats of
    /// `--cli-error-format`, which other errors are not formatted in.
    info: Option<ErrorInfo>,
}

#[derive(Debug)]
struct ErrorInfo {
    kind: ErrorKind,
    code: String,
    message: String,
}

/// The kinds of the errors of the handlers of the AWS CLI, which format the
/// errors in `--cli-error-format`.
#[derive(Debug, PartialEq, Eq)]
enum ErrorKind {
    ParamValidation,
    Client,
    NoCredentials,
}

impl Error {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            code: EXIT_GENERAL,
            message: message.into(),
            info: None,
        }
    }

    /// Creates a parameter validation error, which the AWS CLI reports before
    /// sending any requests.
    pub fn param_validation(message: impl fmt::Display) -> Self {
        Self {
            code: EXIT_PARAM_VALIDATION,
            message: format!("An error occurred (ParamValidation): {message}"),
            info: Some(ErrorInfo {
                kind: ErrorKind::ParamValidation,
                code: "ParamValidation".to_owned(),
                message: message.to_string(),
            }),
        }
    }

    /// Creates an error from an SDK error of the operation, using the same
    /// messages and exit codes as the AWS CLI.
    pub fn sdk<E>(operation: &str, err: SdkError<E, HttpResponse>) -> Self
    where
        E: ProvideErrorMetadata + std::error::Error + 'static,
    {
        match &err {
            SdkError::ServiceError(service_err) => {
                let status = service_err.raw().status().as_u16();
                let inner = service_err.err();
                // Responses of HEAD requests have no body, so the AWS CLI
                // reports the status code and the reason phrase.
                let (code, message) = if operation.starts_with("Head") {
                    (status.to_string(), reason_phrase(status).to_owned())
                } else {
                    (
                        inner
                            .code()
                            .map_or_else(|| status.to_string(), str::to_owned),
                        inner.message().unwrap_or("Unknown").to_owned(),
                    )
                };
                Self {
                    code: EXIT_SERVICE,
                    message: format!(
                        "An error occurred ({code}) when calling the {operation} operation: {message}"
                    ),
                    info: Some(ErrorInfo {
                        kind: ErrorKind::Client,
                        code,
                        message,
                    }),
                }
            }
            // The message of the transfers is extended by the error handler of
            // the commands.
            _ if is_no_credentials(&err) => Self {
                code: EXIT_NO_CREDENTIALS,
                message: "Unable to locate credentials".to_owned(),
                info: Some(ErrorInfo {
                    kind: ErrorKind::NoCredentials,
                    code: "NoCredentials".to_owned(),
                    message: "Unable to locate credentials. \
                              You can configure credentials by running \"aws login\"."
                        .to_owned(),
                }),
            },
            SdkError::DispatchFailure(failure) if failure.is_io() || failure.is_timeout() => {
                Self::new(format!(
                    "Could not connect to the endpoint URL: {}",
                    Chain(&err)
                ))
            }
            // The innermost error describes the cause, e.g. an invalid
            // combination of the configurations to resolve the endpoint.
            _ => endpoint_error(&root_cause(&err).to_string()),
        }
    }

    /// Creates an error of resolving the credentials of `aws login`, using the
    /// same messages as `LoginCredentialFetcher` of the AWS CLI where the error
    /// types of the SDK are private.
    pub fn login(session: &str, err: &(dyn std::error::Error + 'static)) -> Self {
        let mut source = Some(err);
        let mut messages = Vec::new();
        let mut not_found = false;
        while let Some(err) = source {
            if let Some(err) = err.downcast_ref::<std::io::Error>() {
                not_found |= err.kind() == std::io::ErrorKind::NotFound;
            }
            messages.push(err.to_string());
            source = err.source();
        }
        let has = |prefix: &str| messages.iter().any(|message| message.starts_with(prefix));
        let message = if not_found {
            format!(
                "Error loading login session token: Unable to load a existing login session \
                 for session {session}, Please reauthenticate with 'aws login'."
            )
        } else if has("missing field") || has("Expected field") {
            "Error loading login session token: Failed to load access token from token cache, \
             missing one or more required fields."
                .to_owned()
        } else if has("invalid JSON in cached Login token file") {
            // The AWS CLI reports the cache key on the invalid JSON.
            format!("'{}'", hex_sha256(session))
        } else {
            messages.pop().unwrap_or_default()
        };
        Self::new(message)
    }

    /// Renders the error for the standard error in the format, in the same way
    /// as the error handlers of the AWS CLI.
    pub fn render(&self, format: ErrorFormat, colored: bool) -> String {
        let plain = |message: &str| {
            let message = message.trim();
            if message.is_empty() {
                "\naws3: [ERROR]:\n".to_owned()
            } else {
                format!("\naws3: [ERROR]: {message}\n")
            }
        };
        let Some(info) = &self.info else {
            return plain(&self.message);
        };
        let (code, message) = (info.code.as_str(), info.message.as_str());
        match format {
            ErrorFormat::Enhanced if info.kind == ErrorKind::NoCredentials => {
                plain(&format!("An error occurred ({code}): {message}"))
            }
            ErrorFormat::Enhanced => plain(&self.message),
            // The legacy format omits the code except for the client errors.
            ErrorFormat::Legacy if info.kind == ErrorKind::Client => plain(&self.message),
            ErrorFormat::Legacy => plain(message),
            ErrorFormat::Json => errorformat::json(code, message),
            ErrorFormat::Yaml => errorformat::yaml(code, message),
            ErrorFormat::Text => errorformat::text(code, message),
            ErrorFormat::Table => {
                errorformat::table(code, message, errorformat::terminal_width(), colored)
            }
        }
    }

    pub fn exit_code(&self) -> ExitCode {
        ExitCode::from(self.code)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Self::new(err.to_string())
    }
}

/// Returns whether the SDK error is a service error with the error code.
pub fn has_error_code<E, R>(err: &SdkError<E, R>, code: &str) -> bool
where
    E: ProvideErrorMetadata,
{
    matches!(err, SdkError::ServiceError(err) if err.err().code() == Some(code))
}

/// Returns whether the SDK error is a service error with the status code.
pub fn has_status_code<E>(err: &SdkError<E, HttpResponse>, status: u16) -> bool {
    matches!(err, SdkError::ServiceError(err) if err.raw().status().as_u16() == status)
}

fn reason_phrase(status: u16) -> &'static str {
    match status {
        301 => "Moved Permanently",
        304 => "Not Modified",
        307 => "Temporary Redirect",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        412 => "Precondition Failed",
        416 => "Requested Range Not Satisfiable",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        503 => "Service Unavailable",
        _ => "Unknown",
    }
}

fn hex_sha256(value: &str) -> String {
    use sha2::{Digest, Sha256};
    crate::presign::hex(&Sha256::digest(value))
}

fn is_no_credentials(err: &(dyn std::error::Error + 'static)) -> bool {
    let mut source = Some(err);
    while let Some(err) = source {
        if let Some(err) =
            err.downcast_ref::<aws_credential_types::provider::error::CredentialsError>()
            && matches!(
                err,
                aws_credential_types::provider::error::CredentialsError::CredentialsNotLoaded(_)
            )
        {
            return true;
        }
        source = err.source();
    }
    false
}

/// Translates the error of resolving the endpoint in the same way as
/// `ruleset_error_to_botocore_exception` of the AWS CLI.
fn endpoint_error(message: &str) -> Error {
    if let Some(rest) = message.strip_prefix("Invalid region in ARN: ") {
        let label = rest.split('`').nth(1).unwrap_or(message);
        return Error::new(format!(
            "Invalid host label to be bound to the hostname of the endpoint: \"{label}\"."
        ));
    }
    if message == "S3 Object Lambda does not support S3 Accelerate"
        || message == "Accelerate cannot be used with FIPS"
    {
        return Error::new(format!(
            "Unsupported configuration when using S3: {message}"
        ));
    }
    if [
        "S3 Outposts does not support",
        "S3 MRAP does not support",
        "S3 Object Lambda does not support",
        "Access Points do not support",
        "Invalid configuration:",
        "Client was configured for partition",
    ]
    .iter()
    .any(|prefix| message.starts_with(prefix))
    {
        return Error::new(format!(
            "Unsupported configuration when using S3 access-points: {message}"
        ));
    }
    if message.to_lowercase().starts_with("invalid arn:") {
        return Error::param_validation(format!("Parameter validation failed:\n{message}"));
    }
    Error::new(message)
}

fn root_cause<'a>(
    err: &'a (dyn std::error::Error + 'static),
) -> &'a (dyn std::error::Error + 'static) {
    let mut err = err;
    while let Some(source) = err.source() {
        err = source;
    }
    err
}

/// Displays an error with its sources, since SDK errors are terse at the top level.
struct Chain<'a>(&'a (dyn std::error::Error + 'static));

impl fmt::Display for Chain<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)?;
        let mut source = self.0.source();
        while let Some(err) = source {
            write!(f, ": {err}")?;
            source = err.source();
        }
        Ok(())
    }
}
