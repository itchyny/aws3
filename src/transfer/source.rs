//! The sources and the destinations of the transfers, listed by the
//! generators and filtered by the filters.

use std::path::Path;
use std::sync::Arc;

use tokio::sync::mpsc;

use super::context::Context;
use super::filter::Filters;
use super::generator::{self, Item, ListOptions};
use super::paths::{self, Endpoint};
use super::plan::PathsType;
use crate::path::split_bucket_key;

/// The maximum number of the ranges of the keys listed in parallel.
const MAX_LIST_RANGES: usize = 16;

/// Creates the filters rooted at the source and the destination.
pub(super) fn create_filters(ctx: &Context) -> Filters {
    let dir_op = ctx.options.dir_op;
    let root = |path: &str| {
        if let Some(path) = path.strip_prefix("s3://") {
            let (bucket, key) = split_bucket_key(path);
            let key = if !dir_op && !key.ends_with('/') {
                key.rsplit_once('/').map_or("", |(dir, _)| dir)
            } else {
                key
            };
            format!("{bucket}/{key}")
        } else if dir_op {
            paths::abspath(path)
        } else {
            let parent = Path::new(path)
                .parent()
                .map(|parent| parent.to_string_lossy().into_owned());
            paths::abspath(
                parent
                    .as_deref()
                    .filter(|parent| !parent.is_empty())
                    .unwrap_or("."),
            )
        }
    };
    Filters::new(
        &ctx.options.filters,
        &root(&ctx.plan.src),
        &root(&ctx.plan.dest),
    )
}

/// Spawns the generator of the files or objects at the endpoint, where the
/// objects are listed in parallel out of order unless `ordered`.
pub(super) fn spawn_generator(
    ctx: &Arc<Context>,
    endpoint: &Endpoint,
    is_delete: bool,
    ordered: bool,
) -> mpsc::Receiver<Item> {
    let (tx, rx) = mpsc::channel(ctx.config.max_queue_size);
    let path = endpoint.path.clone();
    let dir_op = ctx.options.dir_op;
    if endpoint.is_s3 {
        let ctx = ctx.clone();
        tokio::spawn(async move {
            let options = ListOptions {
                page_size: ctx.options.page_size,
                request_payer: ctx.params.request_payer.clone(),
                sse_c: if ctx.plan.paths_type == PathsType::S3S3 {
                    ctx.params.sse_c_copy_source.clone()
                } else {
                    ctx.params.sse_c.clone()
                },
                checksum_mode: ctx.params.checksum_mode.clone(),
                is_delete,
                ranges: if ordered {
                    1
                } else {
                    MAX_LIST_RANGES.min(ctx.config.max_concurrent_requests)
                },
            };
            let client = &ctx.source_client;
            generator::list_s3(client, &ctx.redirector, &path, dir_op, &options, &tx).await;
        });
    } else {
        let follow_symlinks = ctx.options.follow_symlinks;
        tokio::task::spawn_blocking(move || {
            generator::list_local(&path, dir_op, follow_symlinks, &tx)
        });
    }
    rx
}
