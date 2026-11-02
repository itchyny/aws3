use std::collections::HashMap;
use std::io::{self, Write};
use std::process::ExitCode;
use std::time::Instant;

use crate::ls::human_readable_size;

/// How the results of transfers are printed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrintMode {
    /// Nothing is printed, including errors.
    Quiet,
    /// Only warnings and errors are printed.
    OnlyShowErrors,
    /// Results are printed without progress.
    NoProgress,
    /// Results are printed with progress, every `frequency` seconds.
    Progress { frequency: u64, oneline: bool },
}

/// Records the statistics of transfers and prints the results in the same
/// format as the AWS CLI.
pub struct Printer {
    mode: PrintMode,
    bytes_transferred: u64,
    bytes_failed_to_transfer: u64,
    files_transferred: u64,
    files_failed: u64,
    files_warned: u64,
    files_skipped: u64,
    errors: u64,
    expected_bytes_transferred: u64,
    expected_files_transferred: u64,
    final_expected_files_transferred: Option<u64>,
    start_time: Option<Instant>,
    ongoing: HashMap<u64, (u64, u64)>,
    progress_length: usize,
    first_progress: bool,
    last_progress: Instant,
}

/// The identifier of an ongoing transfer.
pub type TransferId = u64;

impl Printer {
    pub fn new(mode: PrintMode) -> Self {
        Self {
            mode,
            bytes_transferred: 0,
            bytes_failed_to_transfer: 0,
            files_transferred: 0,
            files_failed: 0,
            files_warned: 0,
            files_skipped: 0,
            errors: 0,
            expected_bytes_transferred: 0,
            expected_files_transferred: 0,
            final_expected_files_transferred: None,
            start_time: None,
            ongoing: HashMap::new(),
            progress_length: 0,
            first_progress: true,
            last_progress: Instant::now(),
        }
    }

    /// Records a transfer of the size is queued.
    pub fn queued(&mut self, id: TransferId, size: u64) {
        self.start_time.get_or_insert_with(Instant::now);
        self.ongoing.insert(id, (0, size));
        self.expected_bytes_transferred += size;
        self.expected_files_transferred += 1;
    }

    /// Records the bytes of the transfer are transferred.
    pub fn progress(&mut self, id: TransferId, bytes: u64) {
        if let Some((progress, _)) = self.ongoing.get_mut(&id) {
            *progress += bytes;
        }
        self.bytes_transferred += bytes;
        self.print_progress();
    }

    pub fn success(&mut self, id: TransferId, transfer_type: &str, src: &str, dest: Option<&str>) {
        self.ongoing.remove(&id);
        self.files_transferred += 1;
        if matches!(
            self.mode,
            PrintMode::NoProgress | PrintMode::Progress { .. }
        ) {
            let statement = format!("{transfer_type}: {}", location(src, dest));
            self.print_out(&self.pad(&statement, '\n'));
        }
        self.redisplay_progress();
    }

    pub fn failure(
        &mut self,
        id: TransferId,
        transfer_type: &str,
        src: &str,
        dest: Option<&str>,
        message: &str,
    ) {
        if let Some((progress, size)) = self.ongoing.remove(&id) {
            self.bytes_failed_to_transfer += size.saturating_sub(progress);
        }
        self.files_failed += 1;
        self.files_transferred += 1;
        if self.mode != PrintMode::Quiet {
            let statement = format!("{transfer_type} failed: {} {message}", location(src, dest));
            self.print_err(&self.pad(&statement, '\n'));
        }
        self.redisplay_progress();
    }

    /// Records the transfer is skipped, e.g. the destination already exists.
    pub fn skipped(&mut self, id: TransferId) {
        self.ongoing.remove(&id);
        self.files_skipped += 1;
        self.add_progress_if_needed();
    }

    pub fn warning(&mut self, message: &str) {
        self.files_warned += 1;
        if self.mode != PrintMode::Quiet {
            self.print_err(&self.pad(message, '\n'));
        }
        self.redisplay_progress();
    }

    /// Prints the message to the standard error, which is not counted as a
    /// warning and printed even with `--quiet`.
    pub fn notice(&mut self, message: &str) {
        self.print_err(&self.pad(message, '\n'));
        self.redisplay_progress();
    }

    pub fn dry_run(&mut self, transfer_type: &str, src: &str, dest: Option<&str>) {
        if self.mode != PrintMode::Quiet {
            let statement = format!("(dryrun) {transfer_type}: {}", location(src, dest));
            self.print_out(&self.pad(&statement, '\n'));
        }
    }

    /// Records a fatal error, which aborts the command.
    pub fn error(&mut self, message: &str) {
        self.errors += 1;
        if self.mode != PrintMode::Quiet {
            self.print_err(&self.pad(&format!("fatal error: {message}"), '\n'));
        }
    }

    /// Records the command is cancelled by Ctrl-C.
    pub fn ctrl_c(&mut self) {
        self.errors += 1;
        if self.mode != PrintMode::Quiet {
            self.print_err(&self.pad("cancelled: ctrl-c received", '\n'));
        }
    }

    /// Records the number of transfers is final.
    pub fn final_total(&mut self, total: u64) {
        self.final_expected_files_transferred = Some(total);
        self.clear_progress_if_done('\n');
    }

    pub fn exit_code(&self) -> ExitCode {
        if self.files_failed + self.errors > 0 {
            ExitCode::from(1)
        } else if self.files_warned > 0 {
            ExitCode::from(2)
        } else {
            ExitCode::SUCCESS
        }
    }

    fn expected_totals_are_final(&self) -> bool {
        self.final_expected_files_transferred == Some(self.expected_files_transferred)
    }

    fn has_remaining_progress(&self) -> bool {
        !self.expected_totals_are_final()
            || self.files_transferred + self.files_skipped != self.expected_files_transferred
    }

    fn redisplay_progress(&mut self) {
        self.progress_length = 0;
        self.add_progress_if_needed();
    }

    fn add_progress_if_needed(&mut self) {
        if self.has_remaining_progress() {
            self.print_progress();
        } else {
            self.clear_progress_if_done('\r');
        }
    }

    fn print_progress(&mut self) {
        let PrintMode::Progress { frequency, oneline } = self.mode else {
            return;
        };
        let estimated = |total: String| {
            if self.expected_totals_are_final() {
                total
            } else {
                format!("~{total}")
            }
        };
        let remaining_files = estimated(
            (self.expected_files_transferred as i64
                - (self.files_transferred + self.files_skipped) as i64)
                .to_string(),
        );
        let mut statement = if self.expected_bytes_transferred > 0 {
            let elapsed = self
                .start_time
                .map_or(0.0, |start| start.elapsed().as_secs_f64());
            let speed = if elapsed > 0.0 {
                (self.bytes_transferred as f64 / elapsed) as i64
            } else {
                0
            };
            format!(
                "Completed {}/{} ({}/s) with {remaining_files} file(s) remaining",
                human_readable_size(
                    (self.bytes_transferred + self.bytes_failed_to_transfer) as i64
                ),
                estimated(human_readable_size(self.expected_bytes_transferred as i64)),
                human_readable_size(speed),
            )
        } else {
            format!(
                "Completed {} file(s) with {remaining_files} file(s) remaining",
                self.files_transferred
            )
        };
        if !self.expected_totals_are_final() {
            statement.push_str(" (calculating...)");
        }
        let statement = if oneline {
            let statement = self.pad(&statement, '\r');
            self.progress_length = statement.len() - 1;
            statement
        } else {
            self.pad(&statement, '\n')
        };
        let now = Instant::now();
        if self.first_progress
            || frequency == 0
            || now.duration_since(self.last_progress).as_secs() >= frequency
        {
            self.first_progress = false;
            self.last_progress = now;
            self.print_out(&statement);
        }
    }

    fn clear_progress_if_done(&mut self, ending: char) {
        if self.progress_length > 0 && !self.has_remaining_progress() {
            let statement = self.pad("", ending);
            self.print_out(&statement);
        }
    }

    /// Pads the statement to overwrite the previous progress.
    fn pad(&self, statement: &str, ending: char) -> String {
        let width = statement.chars().count();
        let padding = self.progress_length.saturating_sub(width);
        format!("{statement}{}{ending}", " ".repeat(padding))
    }

    fn print_out(&self, statement: &str) {
        let mut stdout = io::stdout().lock();
        let _ = stdout.write_all(statement.as_bytes());
        let _ = stdout.flush();
    }

    fn print_err(&self, statement: &str) {
        let mut stderr = io::stderr().lock();
        let _ = stderr.write_all(statement.as_bytes());
        let _ = stderr.flush();
    }
}

fn location(src: &str, dest: Option<&str>) -> String {
    match dest {
        Some(dest) => format!("{src} to {dest}"),
        None => src.to_owned(),
    }
}
