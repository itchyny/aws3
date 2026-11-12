//! The restore-object operation, which restores the archived objects. The
//! restores of S3 Select are not supported.

use aws_sdk_s3::Client;
use aws_sdk_s3::types::{GlacierJobParameters, RequestPayer, RestoreRequest, Tier};

use super::output::Response;
use super::shape::{Shape, Structure, parse_structure};
use super::{members, required_checksum_algorithm};
use crate::cli::RestoreObjectArgs;
use crate::error::Error;
use crate::redirect::Redirector;
use crate::value::Value;

static GLACIER_JOB_PARAMETERS: Structure = Structure {
    members: &[("Tier", Shape::String { min: 0 })],
    required: &["Tier"],
};

/// The members of the restores of S3 Select, which are validated only to be
/// rejected.
const SELECT_MEMBERS: [&str; 4] = ["Type", "Tier", "SelectParameters", "OutputLocation"];

static RESTORE_REQUEST: Structure = Structure {
    members: &[
        ("Days", Shape::Long),
        (
            "GlacierJobParameters",
            Shape::Structure(&GLACIER_JOB_PARAMETERS),
        ),
        ("Type", Shape::Any),
        ("Tier", Shape::Any),
        ("Description", Shape::String { min: 0 }),
        ("SelectParameters", Shape::Any),
        ("OutputLocation", Shape::Any),
    ],
    required: &[],
};

pub async fn run(client: &Client, args: &RestoreObjectArgs) -> Result<Value, Error> {
    let restore_request = args
        .restore_request
        .as_deref()
        .map(|value| {
            let value = parse_structure(
                "--restore-request",
                "RestoreRequest",
                value,
                &RESTORE_REQUEST,
            )?;
            restore_request(&value)
        })
        .transpose()?;
    let output = Redirector::default()
        .send(client, &args.bucket, "RestoreObject", |client| {
            client
                .restore_object()
                .bucket(&args.bucket)
                .key(&args.key)
                .set_version_id(args.version_id.clone())
                .set_restore_request(restore_request.clone())
                .set_request_payer(args.request_payer.as_deref().map(RequestPayer::from))
                .checksum_algorithm(required_checksum_algorithm(
                    args.checksum_algorithm.as_deref(),
                ))
                .set_expected_bucket_owner(args.expected_bucket_owner.clone())
                .send()
        })
        .await
        .map_err(|err| Error::sdk("RestoreObject", err))?;
    let mut response = Response::default();
    response.str("RequestCharged", output.request_charged());
    response.str("RestoreOutputPath", output.restore_output_path());
    Ok(response.build())
}

/// Builds the restore request from the validated argument, rejecting the
/// members of the restores of S3 Select.
fn restore_request(value: &Value) -> Result<RestoreRequest, Error> {
    if let Some(member) = SELECT_MEMBERS
        .iter()
        .find(|member| value.get(member).is_some())
    {
        return Err(Error::new(format!(
            "RestoreRequest.{member} of the restores of S3 Select is not supported by the api \
             command, use aws s3api restore-object instead"
        )));
    }
    let glacier_job_parameters = value
        .get("GlacierJobParameters")
        .map(|parameters| {
            GlacierJobParameters::builder()
                .set_tier(members::str(parameters, "Tier").as_deref().map(Tier::from))
                .build()
                .map_err(|err| Error::new(err.to_string()))
        })
        .transpose()?;
    Ok(RestoreRequest::builder()
        .set_days(members::int(value, "Days"))
        .set_glacier_job_parameters(glacier_job_parameters)
        .set_description(members::str(value, "Description"))
        .build())
}
