use std::fmt;
use std::process::ExitCode;

use aws_sdk_s3::config::http::HttpResponse;
use aws_sdk_s3::error::{ProvideErrorMetadata, SdkError};

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
}

impl Error {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            code: EXIT_GENERAL,
            message: message.into(),
        }
    }

    /// Creates a parameter validation error, which the AWS CLI reports before
    /// sending any requests.
    pub fn param_validation(message: impl fmt::Display) -> Self {
        Self {
            code: EXIT_PARAM_VALIDATION,
            message: format!("An error occurred (ParamValidation): {message}"),
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
                }
            }
            _ if is_no_credentials(&err) => Self {
                code: EXIT_NO_CREDENTIALS,
                message: "An error occurred (NoCredentials): Unable to locate credentials. \
                          You can configure credentials by running \"aws configure\"."
                    .into(),
            },
            SdkError::DispatchFailure(failure) if failure.is_io() || failure.is_timeout() => Self {
                code: EXIT_GENERAL,
                message: format!("Could not connect to the endpoint URL: {}", Chain(&err)),
            },
            _ => Self {
                code: EXIT_GENERAL,
                message: Chain(&err).to_string(),
            },
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
        Self {
            code: EXIT_GENERAL,
            message: err.to_string(),
        }
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
