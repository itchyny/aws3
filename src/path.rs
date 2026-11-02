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

#[cfg(test)]
mod tests {
    use super::*;

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
