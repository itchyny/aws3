use std::io::{self, BufWriter, Stdout, Write};
use std::process::ExitCode;

use aws_config::SdkConfig;
use aws_sdk_s3::Client;
use aws_sdk_s3::config::http::HttpResponse;
use aws_sdk_s3::error::SdkError;
use aws_sdk_s3::operation::list_objects_v2::{ListObjectsV2Error, ListObjectsV2Output};
use aws_sdk_s3::primitives::DateTime;
use aws_sdk_s3::types::RequestPayer;
use chrono::Local;

use crate::cli::{self, GlobalArgs, LsArgs};
use crate::client;
use crate::error::Error;
use crate::path::split_bucket_key;

pub async fn run(args: &LsArgs, global: &GlobalArgs) -> Result<ExitCode, Error> {
    let config = client::load_config(global).await?;
    let (bucket, key) = split_bucket_key(&args.path);
    let mut lister = Lister::new(args.human_readable);
    if bucket.is_empty() {
        lister.list_buckets(&config, args).await?;
    } else {
        lister.list_objects(&config, bucket, key, args).await?;
    }
    if args.summarize {
        lister.print_summary()?;
    }
    lister.out.flush()?;
    // Listing a key reports failure when nothing matches.
    Ok(if !key.is_empty() && !lister.found {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}

struct Lister {
    out: BufWriter<Stdout>,
    human_readable: bool,
    found: bool,
    total_objects: u64,
    total_size: i64,
}

impl Lister {
    fn new(human_readable: bool) -> Self {
        Self {
            out: BufWriter::new(io::stdout()),
            human_readable,
            found: false,
            total_objects: 0,
            total_size: 0,
        }
    }

    async fn list_buckets(&mut self, config: &SdkConfig, args: &LsArgs) -> Result<(), Error> {
        let client = client::new_client(config, None);
        let mut paginator = client
            .list_buckets()
            .set_prefix(args.bucket_name_prefix.clone())
            .set_bucket_region(args.bucket_region.clone())
            .into_paginator();
        if let Some(page_size) = args.page_size {
            paginator = paginator.page_size(page_size);
        }
        let mut pages = paginator.send();
        while let Some(page) = pages.next().await {
            let page = page.map_err(|err| Error::sdk("ListBuckets", err))?;
            for bucket in page.buckets() {
                writeln!(
                    self.out,
                    "{} {}",
                    format_date_time(bucket.creation_date()),
                    bucket.name().unwrap_or_default(),
                )?;
            }
            self.out.flush()?;
        }
        Ok(())
    }

    async fn list_objects(
        &mut self,
        config: &SdkConfig,
        bucket: &str,
        key: &str,
        args: &LsArgs,
    ) -> Result<(), Error> {
        let send = |client: &Client| {
            let mut paginator = client
                .list_objects_v2()
                .bucket(bucket)
                .prefix(key)
                .set_delimiter((!args.recursive).then(|| "/".to_owned()))
                .set_request_payer(
                    args.request_payer
                        .request_payer
                        .map(|cli::RequestPayer::Requester| RequestPayer::Requester),
                )
                .into_paginator();
            if let Some(page_size) = args.page_size {
                paginator = paginator.page_size(page_size);
            }
            paginator.send()
        };
        let mut pages = send(&client::new_client(config, None));
        let mut page = pages.next().await;
        // Retry in the region of the bucket if it differs from the configured one.
        if let Some(Err(err)) = &page
            && let Some(region) = bucket_region(err)
            && Some(region) != config.region().map(|region| region.as_ref())
        {
            pages = send(&client::new_client(config, Some(region)));
            page = pages.next().await;
        }
        while let Some(result) = page {
            let output = result.map_err(|err| Error::sdk("ListObjectsV2", err))?;
            self.print_page(&output, !args.recursive)?;
            page = pages.next().await;
        }
        Ok(())
    }

    fn print_page(&mut self, output: &ListObjectsV2Output, use_basename: bool) -> io::Result<()> {
        for prefix in output.common_prefixes() {
            let prefix = prefix.prefix().unwrap_or_default();
            let name = prefix.rsplit('/').nth(1).unwrap_or_default();
            writeln!(self.out, "{:>30} {name}/", "PRE")?;
            self.found = true;
        }
        for object in output.contents() {
            let size = object.size().unwrap_or_default();
            self.total_size += size;
            self.total_objects += 1;
            let key = object.key().unwrap_or_default();
            let name = if use_basename {
                key.rsplit('/').next().unwrap_or_default()
            } else {
                key
            };
            writeln!(
                self.out,
                "{} {:>10} {name}",
                format_date_time(object.last_modified()),
                self.format_size(size),
            )?;
            self.found = true;
        }
        self.out.flush()
    }

    fn print_summary(&mut self) -> io::Result<()> {
        writeln!(self.out, "\nTotal Objects: {}", self.total_objects)?;
        writeln!(
            self.out,
            "   Total Size: {}",
            self.format_size(self.total_size)
        )
    }

    fn format_size(&self, size: i64) -> String {
        if self.human_readable {
            human_readable_size(size)
        } else {
            size.to_string()
        }
    }
}

/// Returns the region of the bucket from the error response, as the AWS CLI
/// follows the region of the bucket.
fn bucket_region(err: &SdkError<ListObjectsV2Error, HttpResponse>) -> Option<&str> {
    err.raw_response()?.headers().get("x-amz-bucket-region")
}

/// Formats the date time in the local time zone.
fn format_date_time(date_time: Option<&DateTime>) -> String {
    date_time
        .and_then(|date_time| chrono::DateTime::from_timestamp(date_time.secs(), 0))
        .map(|date_time| {
            date_time
                .with_timezone(&Local)
                .format("%Y-%m-%d %H:%M:%S")
                .to_string()
        })
        .unwrap_or_default()
}

/// Formats the size in base-2 units, in the same way as the AWS CLI.
fn human_readable_size(size: i64) -> String {
    const BASE: f64 = 1024.0;
    const SUFFIXES: [&str; 6] = ["KiB", "MiB", "GiB", "TiB", "PiB", "EiB"];
    let bytes = size as f64;
    if size == 1 {
        return "1 Byte".to_owned();
    } else if bytes < BASE {
        return format!("{size} Bytes");
    }
    for (i, suffix) in SUFFIXES.into_iter().enumerate() {
        let unit = BASE.powi(i as i32 + 2);
        if (bytes / unit * BASE).round_ties_even() < BASE || i == SUFFIXES.len() - 1 {
            return format!("{:.1} {suffix}", BASE * bytes / unit);
        }
    }
    unreachable!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_readable() {
        assert_eq!(human_readable_size(0), "0 Bytes");
        assert_eq!(human_readable_size(1), "1 Byte");
        assert_eq!(human_readable_size(1023), "1023 Bytes");
        assert_eq!(human_readable_size(1024), "1.0 KiB");
        assert_eq!(human_readable_size(1536000), "1.5 MiB");
        assert_eq!(human_readable_size(1024 * 1024 - 1), "1.0 MiB");
        assert_eq!(human_readable_size(1024 * 1024 - 512), "1.0 MiB");
        assert_eq!(human_readable_size(1024 * 1024 - 513), "1023.5 KiB");
        assert_eq!(human_readable_size(5 << 40), "5.0 TiB");
        assert_eq!(human_readable_size(i64::MAX), "8.0 EiB");
    }
}
