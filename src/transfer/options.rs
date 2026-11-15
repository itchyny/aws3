//! The options of the transfer commands.

use crate::cli::{CaseConflict, CopyProps, Filter, TransferArgs};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandName {
    Cp,
    Mv,
    Rm,
    Sync,
}

impl CommandName {
    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Cp => "cp",
            Self::Mv => "mv",
            Self::Rm => "rm",
            Self::Sync => "sync",
        }
    }

    pub(super) fn usage(self) -> &'static str {
        match self {
            Self::Rm => "<S3Uri>",
            _ => "<LocalPath> <S3Uri> or <S3Uri> <LocalPath> or <S3Uri> <S3Uri>",
        }
    }
}

/// The options of the transfer commands.
#[derive(Debug, Clone)]
pub struct Options {
    pub command: CommandName,
    pub paths: Vec<String>,
    pub dir_op: bool,
    pub dryrun: bool,
    pub quiet: bool,
    pub only_show_errors: bool,
    pub progress: bool,
    pub progress_frequency: u64,
    pub progress_multiline: bool,
    pub follow_symlinks: bool,
    pub guess_mime_type: bool,
    pub page_size: Option<i32>,
    pub filters: Vec<Filter>,
    pub source_region: Option<String>,
    /// Whether the underlying buckets of the access points are resolved to
    /// validate that `mv` does not move an object onto itself.
    pub validate_same_s3_paths: bool,
    pub ignore_glacier_warnings: bool,
    pub force_glacier_transfer: bool,
    pub copy_props: CopyProps,
    pub case_conflict: CaseConflict,
    pub expected_size: Option<u64>,
    pub no_overwrite: bool,
    pub size_only: bool,
    pub exact_timestamps: bool,
    pub delete: bool,
    pub(super) has_checksum_algorithm: bool,
    pub(super) has_checksum_mode: bool,
    pub(super) has_tags: bool,
    pub(super) has_sse_c: bool,
    pub(super) has_sse_c_key: bool,
    pub(super) has_sse_c_copy_source: bool,
    pub(super) has_sse_c_copy_source_key: bool,
}

impl Options {
    pub fn new(command: CommandName, paths: Vec<String>, dir_op: bool) -> Self {
        Self {
            command,
            paths,
            dir_op,
            dryrun: false,
            quiet: false,
            only_show_errors: false,
            progress: true,
            progress_frequency: 0,
            progress_multiline: false,
            follow_symlinks: true,
            guess_mime_type: true,
            page_size: None,
            filters: Vec::new(),
            source_region: None,
            validate_same_s3_paths: false,
            ignore_glacier_warnings: false,
            force_glacier_transfer: false,
            copy_props: CopyProps::Default,
            case_conflict: CaseConflict::Ignore,
            expected_size: None,
            no_overwrite: false,
            size_only: false,
            exact_timestamps: false,
            delete: false,
            has_checksum_algorithm: false,
            has_checksum_mode: false,
            has_tags: false,
            has_sse_c: false,
            has_sse_c_key: false,
            has_sse_c_copy_source: false,
            has_sse_c_copy_source_key: false,
        }
    }

    pub fn with_transfer_args(self, args: &TransferArgs) -> Self {
        Self {
            dryrun: args.dryrun,
            quiet: args.quiet,
            only_show_errors: args.only_show_errors,
            progress: !args.no_progress,
            progress_frequency: args.progress_frequency.unwrap_or(0),
            progress_multiline: args.progress_multiline,
            follow_symlinks: !args.no_follow_symlinks,
            guess_mime_type: !args.no_guess_mime_type,
            page_size: args.page_size,
            filters: args.filter.filters.clone(),
            source_region: args.source_region.clone(),
            ignore_glacier_warnings: args.ignore_glacier_warnings,
            force_glacier_transfer: args.force_glacier_transfer,
            copy_props: args.copy_props,
            case_conflict: args.case_conflict,
            no_overwrite: args.no_overwrite,
            has_checksum_algorithm: args.checksum_algorithm.is_some(),
            has_checksum_mode: args.checksum_mode.is_some(),
            has_tags: !args.tags.is_empty(),
            has_sse_c: args.sse_c.is_some(),
            has_sse_c_key: args.sse_c_key.is_some(),
            has_sse_c_copy_source: args.sse_c_copy_source.is_some(),
            has_sse_c_copy_source_key: args.sse_c_copy_source_key.is_some(),
            ..self
        }
    }
}
