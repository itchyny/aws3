/// Splits an S3 path into the bucket and the key, with or without the `s3://`
/// scheme, in the same way as `find_bucket_key` of the AWS CLI. The access
/// point ARNs are treated as buckets.
pub fn split_bucket_key(path: &str) -> (&str, &str) {
    let path = path.strip_prefix("s3://").unwrap_or(path);
    if let Some((end, start)) = access_point_arn(path).or_else(|| outposts_access_point_arn(path)) {
        return (&path[..end], &path[start..]);
    }
    match path.find('/') {
        Some(index) => (&path[..index], &path[index + 1..]),
        None => (path, ""),
    }
}

/// Returns the error message of the S3 path of the resources unsupported by
/// the s3 commands, in the same way as `block_unsupported_resources` of the
/// AWS CLI.
pub fn check_supported_resource(path: &str) -> Result<(), String> {
    let path = path.strip_prefix("s3://").unwrap_or(path);
    let is_object_lambda = arn_resources(path, "s3-object-lambda")
        .any(|mut arn| arn.region(1).is_some() && arn.access_point(Cursor::outpost_name).is_some());
    if is_object_lambda {
        return Err("s3 commands do not support S3 Object Lambda resources. \
                    Use s3api commands instead."
            .to_owned());
    }
    let is_outposts_bucket = arn_resources(path, "s3-outposts").any(|mut arn| {
        arn.outpost()
            .and_then(|()| arn.tag("bucket"))
            .and_then(|()| arn.separator())
            .and_then(|()| arn.outpost_name())
            .is_some()
    });
    if is_outposts_bucket {
        return Err("s3 commands do not support Outpost Bucket ARNs. \
                    Use s3control commands instead."
            .to_owned());
    }
    Ok(())
}

/// Returns the end of the bucket and the start of the key of the access point
/// ARN, e.g. `arn:aws:s3:us-east-1:123456789012:accesspoint/name/key`, which
/// matches `arn:(aws).*:s3:[a-z\-0-9]*:[0-9]{12}:accesspoint[:/][^/]+/?`.
fn access_point_arn(path: &str) -> Option<(usize, usize)> {
    arn_resources(path, "s3").find_map(|mut arn| {
        arn.region(0)?;
        arn.access_point(|arn| arn.take(1, usize::MAX, |byte| byte != b'/'))?;
        let end = path.len() - arn.0.len();
        Some((end, end + usize::from(arn.0.starts_with('/'))))
    })
}

/// Returns the end of the bucket and the start of the key of the access point
/// ARN of S3 on Outposts, which matches `arn:(aws).*:s3-outposts:[a-z\-0-9]+:
/// [0-9]{12}:outpost[/:][a-zA-Z0-9\-]{1,63}[/:]accesspoint[/:][a-zA-Z0-9\-]{1,63}[/:]?`.
fn outposts_access_point_arn(path: &str) -> Option<(usize, usize)> {
    arn_resources(path, "s3-outposts").find_map(|mut arn| {
        arn.outpost()?;
        arn.tag("accesspoint")?;
        arn.separator()?;
        arn.outpost_name()?;
        let end = path.len() - arn.0.len();
        Some((end, end + usize::from(arn.0.starts_with(['/', ':']))))
    })
}

/// Returns the cursors after `:{service}:` of the ARN starting with `arn:aws`,
/// from the last occurrence like the greedy `arn:(aws).*:{service}:`.
fn arn_resources<'a>(path: &'a str, service: &str) -> impl Iterator<Item = Cursor<'a>> {
    let pattern = format!(":{service}:");
    let rest = path.strip_prefix("arn:aws").unwrap_or_default();
    let offset = path.len() - rest.len();
    let indices: Vec<_> = if path.starts_with("arn:aws") {
        rest.match_indices(&pattern)
            .map(|(index, _)| index)
            .collect()
    } else {
        Vec::new()
    };
    indices
        .into_iter()
        .rev()
        .map(move |index| Cursor(&path[offset + index + pattern.len()..]))
}

/// The cursor of the ARN patterns of the AWS CLI, which are parsed by hand.
struct Cursor<'a>(&'a str);

impl Cursor<'_> {
    fn tag(&mut self, tag: &str) -> Option<()> {
        self.0 = self.0.strip_prefix(tag)?;
        Some(())
    }

    fn separator(&mut self) -> Option<()> {
        self.0 = self.0.strip_prefix(['/', ':'])?;
        Some(())
    }

    /// Consumes the bytes allowed greedily, up to the maximum length.
    fn take(&mut self, min: usize, max: usize, allowed: impl Fn(u8) -> bool) -> Option<()> {
        let length = self
            .0
            .bytes()
            .take(max)
            .take_while(|&byte| allowed(byte))
            .count();
        (length >= min).then(|| self.0 = &self.0[length..])
    }

    /// Consumes `[a-z\-0-9]*:[0-9]{12}:`, with the minimum length of the region.
    fn region(&mut self, min: usize) -> Option<()> {
        self.take(min, usize::MAX, |byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-'
        })?;
        self.tag(":")?;
        self.take(12, 12, |byte| byte.is_ascii_digit())?;
        self.tag(":")
    }

    /// Consumes `accesspoint[/:]` and the name.
    fn access_point(&mut self, name: impl Fn(&mut Self) -> Option<()>) -> Option<()> {
        self.tag("accesspoint")?;
        self.separator()?;
        name(self)
    }

    /// Consumes `[a-zA-Z0-9\-]{1,63}`.
    fn outpost_name(&mut self) -> Option<()> {
        self.take(1, 63, |byte| byte.is_ascii_alphanumeric() || byte == b'-')
    }

    /// Consumes the region, the account and `outpost[/:]{id}[/:]`.
    fn outpost(&mut self) -> Option<()> {
        self.region(1)?;
        self.tag("outpost")?;
        self.separator()?;
        self.outpost_name()?;
        self.separator()
    }
}

/// Decodes the URL-encoded key of a listing in the same way as
/// `urllib.parse.unquote_plus` of Python, which the AWS CLI uses. The keys are
/// requested to be encoded since XML cannot contain some characters.
pub fn url_decode(value: &str) -> String {
    let hex = |byte: u8| (byte as char).to_digit(16).map(|digit| digit as u8);
    let mut bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    while let [byte, rest @ ..] = bytes {
        bytes = rest;
        match (byte, rest) {
            (b'+', _) => decoded.push(b' '),
            (b'%', [high, low, rest @ ..])
                if let (Some(high), Some(low)) = (hex(*high), hex(*low)) =>
            {
                decoded.push(high << 4 | low);
                bytes = rest;
            }
            _ => decoded.push(*byte),
        }
    }
    String::from_utf8_lossy(&decoded).into_owned()
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
    fn decode() {
        assert_eq!(url_decode("plain.txt"), "plain.txt");
        assert_eq!(url_decode("a%2Bb+c.txt"), "a+b c.txt");
        assert_eq!(url_decode("100%2525.txt"), "100%25.txt");
        assert_eq!(url_decode("%C3%BCn%C3%AF.txt"), "ünï.txt");
        assert_eq!(url_decode("ctrl%01char"), "ctrl\u{1}char");
        assert_eq!(url_decode("dir%2Fsub%20dir%2F"), "dir/sub dir/");
        assert_eq!(url_decode("bad%zz%4"), "bad%zz%4");
        assert_eq!(url_decode("%"), "%");
        assert_eq!(url_decode("%FF"), "\u{FFFD}");
    }

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
        // The expected values are of find_bucket_key of the AWS CLI.
        let ap = "arn:aws:s3:us-east-1:123456789012:accesspoint";
        let op = "arn:aws:s3-outposts:us-east-1:123456789012:outpost";
        let long = "a".repeat(70);
        let cases = [
            (format!("s3://{ap}/ap/a/b"), format!("{ap}/ap"), "a/b"),
            (format!("{ap}:ap"), format!("{ap}:ap"), ""),
            (format!("{ap}:ap:x/key"), format!("{ap}:ap:x"), "key"),
            (
                "arn:aws:s3::123456789012:accesspoint/mfzwi23gnjvgw.mrap/key".to_owned(),
                "arn:aws:s3::123456789012:accesspoint/mfzwi23gnjvgw.mrap".to_owned(),
                "key",
            ),
            (
                "arn:aws-cn:s3:cn-north-1:123456789012:accesspoint/ap/key".to_owned(),
                "arn:aws-cn:s3:cn-north-1:123456789012:accesspoint/ap".to_owned(),
                "key",
            ),
            (
                "arn:aws:x:s3:us-east-1:123456789012:accesspoint/ap/k".to_owned(),
                "arn:aws:x:s3:us-east-1:123456789012:accesspoint/ap".to_owned(),
                "k",
            ),
            (
                format!("{op}/op-1/accesspoint/ap1/dir/key"),
                format!("{op}/op-1/accesspoint/ap1"),
                "dir/key",
            ),
            (
                format!("{op}/op-1/accesspoint/ap1:dir/key"),
                format!("{op}/op-1/accesspoint/ap1"),
                "dir/key",
            ),
            (
                format!("{op}/op-1/accesspoint/ap1"),
                format!("{op}/op-1/accesspoint/ap1"),
                "",
            ),
            (
                format!("{op}/op-1/accesspoint/ap1/"),
                format!("{op}/op-1/accesspoint/ap1"),
                "",
            ),
            (
                format!("{op}:op-1:accesspoint:ap1:k"),
                format!("{op}:op-1:accesspoint:ap1"),
                "k",
            ),
            (
                format!("{op}/op-1/accesspoint/ap.x/k"),
                format!("{op}/op-1/accesspoint/ap"),
                ".x/k",
            ),
            (
                format!("{op}/op-1/accesspoint/{long}/k"),
                format!("{op}/op-1/accesspoint/{}", &long[..63]),
                "aaaaaaa/k",
            ),
            (
                "arn:aws:s3-outposts::123456789012:outpost/op-1/accesspoint/ap/k".to_owned(),
                "arn:aws:s3-outposts::123456789012:outpost".to_owned(),
                "op-1/accesspoint/ap/k",
            ),
            (
                "arn:aws:s3:us-east-1:12345:accesspoint/ap/k".to_owned(),
                "arn:aws:s3:us-east-1:12345:accesspoint".to_owned(),
                "ap/k",
            ),
            (
                format!("{}/ap/k", ap.replace("accesspoint", "bucket")),
                ap.replace("accesspoint", "bucket"),
                "ap/k",
            ),
            (
                format!("{}/ap/k", ap.replace("aws", "gcp")),
                ap.replace("aws", "gcp"),
                "ap/k",
            ),
        ];
        for (path, bucket, key) in &cases {
            assert_eq!(split_bucket_key(path), (bucket.as_str(), *key), "{path}");
        }
    }

    #[test]
    fn supported_resource() {
        let op = "arn:aws:s3-outposts:us-east-1:123456789012:outpost/op-1";
        assert_eq!(check_supported_resource("s3://bucket/key"), Ok(()));
        assert_eq!(
            check_supported_resource(&format!("s3://{op}/accesspoint/ap/k")),
            Ok(())
        );
        assert_eq!(
            check_supported_resource(
                "s3://arn:aws:s3-object-lambda:us-east-1:123456789012:accesspoint/olap/key"
            ),
            Err("s3 commands do not support S3 Object Lambda resources. \
                 Use s3api commands instead."
                .to_owned())
        );
        assert_eq!(
            check_supported_resource(&format!("{op}/bucket/b1")),
            Err("s3 commands do not support Outpost Bucket ARNs. \
                 Use s3control commands instead."
                .to_owned())
        );
    }
}
