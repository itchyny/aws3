//! The tasks of the transfers, with the operations, the sources and the
//! destinations, and the outcomes.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use super::generator::ObjectProps;

/// The kind of a transfer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Operation {
    Upload,
    Download,
    Copy,
    Delete,
    LocalDelete,
}

impl Operation {
    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Upload => "upload",
            Self::Download => "download",
            Self::Copy => "copy",
            Self::Delete | Self::LocalDelete => "delete",
        }
    }
}

/// A transfer of a file or an object.
#[derive(Debug, Clone)]
pub(super) struct Task {
    pub(super) operation: Operation,
    /// The absolute local path, or `bucket/key` of S3.
    pub(super) src: String,
    pub(super) dest: String,
    pub(super) compare_key: String,
    pub(super) size: Option<u64>,
    pub(super) last_modified: Option<SystemTime>,
    pub(super) object: Option<ObjectProps>,
    /// The keys of the downloads to remove this key from on completion.
    pub(super) case_conflict: Option<(CaseConflictKeys, String)>,
}

/// The result of a transfer which did not fail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Outcome {
    Done,
    /// The transfer is skipped because the destination already exists.
    Skipped,
}

/// The keys of the downloads, in lowercase, to detect case conflicts.
pub(super) type CaseConflictKeys = Arc<Mutex<HashSet<String>>>;
