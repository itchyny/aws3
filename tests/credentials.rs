// The credential processes are shell scripts made executable with the Unix
// permissions.
#![cfg(unix)]

mod common;

use std::process::Command;

use common::{ACCESS_KEY, Env, Request, SECRET_KEY, xml_response};

/// Returns the command without the credentials in the environment variables.
fn command(env: &Env, args: &[&str]) -> Command {
    let mut command = env.command(args);
    command
        .env_remove("AWS_ACCESS_KEY_ID")
        .env_remove("AWS_SECRET_ACCESS_KEY");
    command
}

fn run(mut command: Command) -> common::Result {
    common::Result::new(command.output().unwrap())
}

/// Returns the requests to the bucket, excluding those to the credential
/// providers handled by the interceptor.
fn bucket_requests(env: &Env) -> Vec<Request> {
    let requests = env.requests();
    requests
        .into_iter()
        .filter(|request| request.bucket() == "bucket")
        .collect()
}

/// Asserts that the requests to the bucket are signed with the credentials
/// of the session token.
fn assert_signed(env: &Env, token: &str) {
    let requests = bucket_requests(env);
    assert!(!requests.is_empty());
    for request in requests {
        assert_eq!(request.access_key(), ACCESS_KEY);
        assert_eq!(request.header("x-amz-security-token"), token);
    }
}

/// Returns the response of STS with the credentials of the session token.
fn sts_response(action: &str, token: &str) -> http::Response<s3s::Body> {
    xml_response(
        200,
        &format!(
            "<{action}Response xmlns=\"https://sts.amazonaws.com/doc/2011-06-15/\">\
             <{action}Result><Credentials><AccessKeyId>{ACCESS_KEY}</AccessKeyId>\
             <SecretAccessKey>{SECRET_KEY}</SecretAccessKey><SessionToken>{token}</SessionToken>\
             <Expiration>2099-01-01T00:00:00Z</Expiration></Credentials>\
             <AssumedRoleUser><AssumedRoleId>AROAEXAMPLE:session</AssumedRoleId>\
             <Arn>arn:aws:sts::123456789012:assumed-role/role/session</Arn></AssumedRoleUser>\
             </{action}Result></{action}Response>"
        ),
    )
}

/// Returns the response of the credentials in JSON.
fn json_response(body: &str) -> http::Response<s3s::Body> {
    http::Response::builder()
        .header("content-type", "application/json")
        .body(s3s::Body::from(body.to_owned()))
        .unwrap()
}

/// Returns the credentials in JSON of the container and the instance metadata.
fn credentials_json(token: &str) -> String {
    format!(
        r#"{{"Code":"Success","Type":"AWS-HMAC","AccessKeyId":"{ACCESS_KEY}","SecretAccessKey":"{SECRET_KEY}","Token":"{token}","Expiration":"2099-01-01T00:00:00Z"}}"#
    )
}

fn form_param<'a>(request: &'a Request, name: &str) -> Option<&'a str> {
    std::str::from_utf8(&request.body)
        .ok()?
        .split('&')
        .find_map(|param| param.strip_prefix(name)?.strip_prefix('='))
}

#[test]
fn credentials_shared_file() {
    let env = Env::new();
    env.bucket("bucket");
    env.write(
        ".aws-credentials",
        format!(
            "[default]\naws_access_key_id = {ACCESS_KEY}\naws_secret_access_key = {SECRET_KEY}\n\
             aws_session_token = default-token\n\n\
             [other]\naws_access_key_id = {ACCESS_KEY}\naws_secret_access_key = {SECRET_KEY}\n\
             aws_session_token = other-token\n"
        ),
    );
    let credentials = env.path(".aws-credentials");
    let mut cmd = command(&env, &["ls", "s3://bucket/"]);
    cmd.env("AWS_SHARED_CREDENTIALS_FILE", &credentials);
    run(cmd).success();
    assert_signed(&env, "default-token");

    for (args, profile) in [
        (&["ls", "s3://bucket/", "--profile", "other"][..], None),
        (&["ls", "s3://bucket/"][..], Some("other")),
    ] {
        let start = env.requests().len();
        let mut cmd = command(&env, args);
        cmd.env("AWS_SHARED_CREDENTIALS_FILE", &credentials);
        if let Some(profile) = profile {
            cmd.env("AWS_PROFILE", profile);
        }
        run(cmd).success();
        for request in &env.requests()[start..] {
            assert_eq!(request.header("x-amz-security-token"), "other-token");
        }
    }
}

#[test]
fn credentials_process() {
    let env = Env::new();
    env.bucket("bucket");
    env.write(
        "credentials.sh",
        format!(
            "#!/bin/sh\necho '{{\"Version\":1,\"AccessKeyId\":\"{ACCESS_KEY}\",\
             \"SecretAccessKey\":\"{SECRET_KEY}\",\"SessionToken\":\"process-token\",\
             \"Expiration\":\"2099-01-01T00:00:00Z\"}}'\n"
        ),
    );
    env.write(
        "failure.sh",
        "#!/bin/sh\necho 'access denied' >&2\nexit 1\n",
    );
    for script in ["credentials.sh", "failure.sh"] {
        let path = env.path(script);
        std::fs::set_permissions(&path, std::os::unix::fs::PermissionsExt::from_mode(0o755))
            .unwrap();
    }
    let config = env.config(&format!(
        "[default]\ncredential_process = {}\n\n[profile failure]\ncredential_process = {}\n",
        env.path("credentials.sh").display(),
        env.path("failure.sh").display(),
    ));
    let mut cmd = command(&env, &["ls", "s3://bucket/"]);
    cmd.env("AWS_CONFIG_FILE", &config);
    run(cmd).success();
    assert_signed(&env, "process-token");

    let start = env.requests().len();
    let mut cmd = command(&env, &["ls", "s3://bucket/", "--profile", "failure"]);
    cmd.env("AWS_CONFIG_FILE", &config);
    let result = run(cmd);
    assert_eq!(result.code, 255, "{}", result.stderr);
    assert!(result.stderr.contains("access denied"), "{}", result.stderr);
    assert_eq!(env.requests().len(), start);
}

#[test]
fn credentials_assume_role() {
    let env = Env::new();
    env.bucket("bucket");
    env.intercept(|request| {
        (form_param(request, "Action") == Some("AssumeRole"))
            .then(|| sts_response("AssumeRole", "role-token"))
    });
    let config = env.config(
        "[default]\nrole_arn = arn:aws:iam::123456789012:role/role\n\
         source_profile = base\nrole_session_name = session\n\n\
         [profile base]\naws_access_key_id = AKIABASEEXAMPLE\n\
         aws_secret_access_key = base-secret\n",
    );
    let mut cmd = command(&env, &["ls", "s3://bucket/"]);
    cmd.env("AWS_CONFIG_FILE", &config);
    run(cmd).success();
    assert_signed(&env, "role-token");
    // The role is assumed with the credentials of the source profile.
    let requests = env.requests();
    let assume: Vec<_> = requests
        .iter()
        .filter(|request| form_param(request, "Action") == Some("AssumeRole"))
        .collect();
    assert_eq!(assume.len(), 1);
    assert_eq!(assume[0].access_key(), "AKIABASEEXAMPLE");
    assert_eq!(
        form_param(assume[0], "RoleArn"),
        Some("arn%3Aaws%3Aiam%3A%3A123456789012%3Arole%2Frole")
    );
    assert_eq!(form_param(assume[0], "RoleSessionName"), Some("session"));
}

#[test]
fn credentials_web_identity() {
    let env = Env::new();
    env.bucket("bucket");
    env.intercept(|request| {
        (form_param(request, "Action") == Some("AssumeRoleWithWebIdentity"))
            .then(|| sts_response("AssumeRoleWithWebIdentity", "web-token"))
    });
    env.write("token", "oidc-token");
    let mut cmd = command(&env, &["ls", "s3://bucket/"]);
    cmd.env("AWS_WEB_IDENTITY_TOKEN_FILE", env.path("token"))
        .env("AWS_ROLE_ARN", "arn:aws:iam::123456789012:role/role")
        .env("AWS_ROLE_SESSION_NAME", "session");
    run(cmd).success();
    assert_signed(&env, "web-token");

    // The token file is also configured in the profile.
    let config = env.config(&format!(
        "[default]\nrole_arn = arn:aws:iam::123456789012:role/role\n\
         web_identity_token_file = {}\n",
        env.path("token").display()
    ));
    let mut cmd = command(&env, &["ls", "s3://bucket/"]);
    cmd.env("AWS_CONFIG_FILE", &config);
    run(cmd).success();
    assert_signed(&env, "web-token");

    let requests = env.requests();
    let assume: Vec<_> = requests
        .iter()
        .filter(|request| form_param(request, "Action") == Some("AssumeRoleWithWebIdentity"))
        .collect();
    assert_eq!(assume.len(), 2);
    for request in assume {
        assert_eq!(form_param(request, "WebIdentityToken"), Some("oidc-token"));
        // The request is not signed.
        assert_eq!(request.header("authorization"), "");
    }
}

#[test]
fn credentials_container() {
    let env = Env::new();
    env.bucket("bucket");
    env.intercept(|request| {
        (request.uri.path() == "/credentials").then(|| {
            if request.header("authorization") == "container-auth" {
                json_response(&credentials_json("container-token"))
            } else {
                xml_response(403, "")
            }
        })
    });
    let mut cmd = command(&env, &["ls", "s3://bucket/"]);
    cmd.env(
        "AWS_CONTAINER_CREDENTIALS_FULL_URI",
        format!("{}/credentials", env.endpoint()),
    )
    .env("AWS_CONTAINER_AUTHORIZATION_TOKEN", "container-auth");
    run(cmd).success();
    assert_signed(&env, "container-token");

    // The authorization token is also read from the file, for EKS Pod Identity.
    env.write("auth-token", "container-auth");
    let start = env.requests().len();
    let mut cmd = command(&env, &["ls", "s3://bucket/"]);
    cmd.env(
        "AWS_CONTAINER_CREDENTIALS_FULL_URI",
        format!("{}/credentials", env.endpoint()),
    )
    .env(
        "AWS_CONTAINER_AUTHORIZATION_TOKEN_FILE",
        env.path("auth-token"),
    );
    run(cmd).success();
    assert!(env.requests().len() > start);
    assert_signed(&env, "container-token");
}

#[test]
fn credentials_instance_metadata() {
    let env = Env::new();
    env.bucket("bucket");
    env.intercept(|request| {
        let path = request.uri.path();
        if path == "/latest/api/token" {
            return Some(
                http::Response::builder()
                    .header("x-aws-ec2-metadata-token-ttl-seconds", "21600")
                    .body(s3s::Body::from("imds-session".to_owned()))
                    .unwrap(),
            );
        }
        let rest = path
            .strip_prefix("/latest/meta-data/iam/security-credentials/")
            .or_else(|| {
                path.strip_prefix("/latest/meta-data/iam/security-credentials-extended/")
            })?;
        if request.header("x-aws-ec2-metadata-token") != "imds-session" {
            return Some(xml_response(401, ""));
        }
        Some(if rest.is_empty() {
            json_response("instance-role")
        } else {
            json_response(&credentials_json("instance-token"))
        })
    });
    let mut cmd = command(&env, &["ls", "s3://bucket/"]);
    cmd.env_remove("AWS_EC2_METADATA_DISABLED")
        .env("AWS_EC2_METADATA_SERVICE_ENDPOINT", env.endpoint());
    run(cmd).success();
    assert_signed(&env, "instance-token");
}

#[test]
fn credentials_sso() {
    let env = Env::new();
    env.bucket("bucket");
    env.intercept(|request| {
        (request.uri.path() == "/federation/credentials").then(|| {
            if request.header("x-amz-sso_bearer_token") == "sso-access-token" {
                json_response(&format!(
                    r#"{{"roleCredentials":{{"accessKeyId":"{ACCESS_KEY}","secretAccessKey":"{SECRET_KEY}","sessionToken":"sso-token","expiration":4070908800000}}}}"#
                ))
            } else {
                xml_response(401, "")
            }
        })
    });
    // The token cache is named by the SHA-1 of the session name.
    env.write(
        ".aws/sso/cache/0ad374308c5a4e22f723adf10145eafad7c4031c.json",
        r#"{"accessToken":"sso-access-token","expiresAt":"2099-01-01T00:00:00Z","region":"us-east-1","startUrl":"https://example.awsapps.com/start"}"#,
    );
    let config = env.config(
        "[default]\nsso_session = my-sso\nsso_account_id = 123456789012\n\
         sso_role_name = role\n\n\
         [sso-session my-sso]\nsso_start_url = https://example.awsapps.com/start\n\
         sso_region = us-east-1\n",
    );
    let mut cmd = command(&env, &["ls", "s3://bucket/"]);
    cmd.env("AWS_CONFIG_FILE", &config);
    run(cmd).success();
    assert_signed(&env, "sso-token");
    let requests = env.requests();
    let sso = requests
        .iter()
        .find(|request| request.uri.path() == "/federation/credentials")
        .unwrap();
    assert_eq!(
        sso.uri.query(),
        Some("role_name=role&account_id=123456789012")
    );
}
