/// Splits an S3 path into the bucket and the key, with or without the `s3://`
/// scheme. Access point ARNs are treated as buckets.
pub fn split_bucket_key(path: &str) -> (&str, &str) {
    let path = path.strip_prefix("s3://").unwrap_or(path);
    let start = access_point_name_start(path).unwrap_or(0);
    match path[start..].find('/') {
        Some(index) => (&path[..start + index], &path[start + index + 1..]),
        None => (path, ""),
    }
}

/// Returns the start index of the access point name in an access point ARN,
/// e.g. `arn:aws:s3:us-east-1:123456789012:accesspoint/name`.
fn access_point_name_start(path: &str) -> Option<usize> {
    if !path.starts_with("arn:") {
        return None;
    }
    let index = path.find(":accesspoint")? + ":accesspoint".len();
    matches!(path[index..].chars().next(), Some(':' | '/')).then_some(index + 1)
}

/// The pattern of bucket names in the error message of the AWS CLI.
const VALID_BUCKET: &str = r"^[a-zA-Z0-9.\-_]{1,255}$";

/// The pattern of access point ARNs in the error message of the AWS CLI.
const VALID_S3_ARN: &str = r"^arn:(aws).*:(s3|s3-object-lambda):[a-z\-0-9]*:[0-9]{12}:accesspoint[/:][a-zA-Z0-9\-.]{1,63}$|^arn:(aws).*:s3-outposts:[a-z\-0-9]+:[0-9]{12}:outpost[/:][a-zA-Z0-9\-]{1,63}[/:]accesspoint[/:][a-zA-Z0-9\-]{1,63}$";

/// Validates the bucket name in the same way as `validate_bucket_name` of the
/// AWS CLI, which is applied to the requests, and returns the error message.
pub fn validate_bucket_name(bucket: &str) -> Result<(), String> {
    if is_valid_bucket(bucket) || is_access_point_arn(bucket) || is_outpost_arn(bucket) {
        return Ok(());
    }
    Err(format!(
        "Parameter validation failed:\nInvalid bucket name \"{bucket}\": Bucket name must match \
         the regex \"{VALID_BUCKET}\" or be an ARN matching the regex \"{VALID_S3_ARN}\""
    ))
}

fn is_valid_bucket(bucket: &str) -> bool {
    (1..=255).contains(&bucket.len())
        && bucket
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
}

/// Returns whether the string consists of 1 to 63 of the allowed characters.
fn is_name(name: &str, allowed: fn(u8) -> bool) -> bool {
    (1..=63).contains(&name.len()) && name.bytes().all(allowed)
}

fn is_region(region: &str) -> bool {
    region
        .bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn is_account(account: &str) -> bool {
    account.len() == 12 && account.bytes().all(|b| b.is_ascii_digit())
}

/// Returns whether the string matches the access point ARN pattern, which is
/// parsed from the end since no part after the prefix contains colons.
fn is_access_point_arn(arn: &str) -> bool {
    let Some((head, name)) = arn.rsplit_once(['/', ':']) else {
        return false;
    };
    let Some(head) = head.strip_suffix(":accesspoint") else {
        return false;
    };
    let Some((head, account)) = head.rsplit_once(':') else {
        return false;
    };
    let Some((head, region)) = head.rsplit_once(':') else {
        return false;
    };
    let Some((prefix, service)) = head.rsplit_once(':') else {
        return false;
    };
    is_name(name, |b| {
        b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.')
    }) && is_account(account)
        && is_region(region)
        && matches!(service, "s3" | "s3-object-lambda")
        && prefix.starts_with("arn:aws")
}

/// Returns whether the string matches the S3 on Outposts access point ARN
/// pattern, e.g. `arn:aws:s3-outposts:<region>:<account>:outpost/<id>/accesspoint/<name>`.
fn is_outpost_arn(arn: &str) -> bool {
    let is_id = |name: &str| is_name(name, |b| b.is_ascii_alphanumeric() || b == b'-');
    let Some((head, name)) = arn.rsplit_once(['/', ':']) else {
        return false;
    };
    let Some(head) = head.strip_suffix("accesspoint") else {
        return false;
    };
    let Some(head) = head.strip_suffix(['/', ':']) else {
        return false;
    };
    let Some((head, id)) = head.rsplit_once(['/', ':']) else {
        return false;
    };
    let Some(head) = head.strip_suffix(":outpost") else {
        return false;
    };
    let Some((head, account)) = head.rsplit_once(':') else {
        return false;
    };
    let Some((head, region)) = head.rsplit_once(':') else {
        return false;
    };
    let Some(prefix) = head.strip_suffix(":s3-outposts") else {
        return false;
    };
    is_id(name)
        && is_id(id)
        && is_account(account)
        && !region.is_empty()
        && is_region(region)
        && prefix.starts_with("arn:aws")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bucket_name() {
        let valid = [
            "bucket",
            "my.bucket_name-1",
            "UPPER",
            &"a".repeat(255),
            "arn:aws:s3:us-east-1:123456789012:accesspoint/my-ap",
            "arn:aws:s3:us-east-1:123456789012:accesspoint:my.ap",
            "arn:aws-cn:s3:cn-north-1:123456789012:accesspoint/ap",
            "arn:aws:s3::123456789012:accesspoint/mfzwi23gnjvgw.mrap",
            "arn:aws:s3-object-lambda:us-east-1:123456789012:accesspoint/olap",
            "arn:aws:s3-outposts:us-east-1:123456789012:outpost/op-01234567890123456/accesspoint/ap",
            "arn:aws:s3-outposts:us-east-1:123456789012:outpost:op-1:accesspoint:ap",
        ];
        for bucket in valid {
            assert_eq!(validate_bucket_name(bucket), Ok(()), "{bucket}");
        }
        let invalid = [
            "",
            "bad~name",
            "bad/name",
            &"a".repeat(256),
            "arn:aws:s3:us-we$t-2:123456789012:accesspoint/ap",
            "arn:aws:s3:us-east-1:123456789012:accesspoint:",
            "arn:aws:s3:us-east-1:12345:accesspoint/ap",
            "arn:aws:s3:us-east-1:123456789012:bucket/ap",
            "arn:aws:s3:us-east-1:123456789012:accesspoint/a_p",
            "arn:gcp:s3:us-east-1:123456789012:accesspoint/ap",
            "arn:aws:s3-outposts::123456789012:outpost/op-1/accesspoint/ap",
            "arn:aws:s3-outposts:us-east-1:123456789012:outpost/op.1/accesspoint/ap",
        ];
        for bucket in invalid {
            assert!(validate_bucket_name(bucket).is_err(), "{bucket}");
        }
        assert_eq!(
            validate_bucket_name("bad~name").unwrap_err(),
            r#"Parameter validation failed:
Invalid bucket name "bad~name": Bucket name must match the regex "^[a-zA-Z0-9.\-_]{1,255}$" or be an ARN matching the regex "^arn:(aws).*:(s3|s3-object-lambda):[a-z\-0-9]*:[0-9]{12}:accesspoint[/:][a-zA-Z0-9\-.]{1,63}$|^arn:(aws).*:s3-outposts:[a-z\-0-9]+:[0-9]{12}:outpost[/:][a-zA-Z0-9\-]{1,63}[/:]accesspoint[/:][a-zA-Z0-9\-]{1,63}$""#
        );
    }

    #[test]
    fn split() {
        assert_eq!(split_bucket_key("s3://"), ("", ""));
        assert_eq!(split_bucket_key("s3://bucket"), ("bucket", ""));
        assert_eq!(split_bucket_key("s3://bucket/"), ("bucket", ""));
        assert_eq!(split_bucket_key("s3://bucket/a/b/"), ("bucket", "a/b/"));
        assert_eq!(split_bucket_key("bucket/a"), ("bucket", "a"));
        assert_eq!(
            split_bucket_key("s3://arn:aws:s3:us-east-1:123456789012:accesspoint/ap/a/b"),
            ("arn:aws:s3:us-east-1:123456789012:accesspoint/ap", "a/b")
        );
        assert_eq!(
            split_bucket_key("arn:aws:s3:us-east-1:123456789012:accesspoint:ap"),
            ("arn:aws:s3:us-east-1:123456789012:accesspoint:ap", "")
        );
    }
}
