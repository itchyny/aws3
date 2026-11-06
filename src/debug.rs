//! The debug logging of `--debug`, which writes the events of the SDK to the
//! standard error in the same line format as the AWS CLI, e.g.
//! `2026-01-01 00:00:00,000 - main - aws3 - DEBUG - CLI version: aws3/0.1.0`.

use std::fmt::{self, Write as _};
use std::io::Write as _;

use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Level, Metadata, Subscriber};

/// Starts the debug logging, and logs the version and the arguments.
pub fn init() {
    if tracing::subscriber::set_global_default(DebugSubscriber).is_ok() {
        let args: Vec<_> = std::env::args().skip(1).collect();
        tracing::debug!(
            target: "aws3",
            "CLI version: aws3/{} {}/{}",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS,
            std::env::consts::ARCH,
        );
        tracing::debug!(target: "aws3", "Arguments entered to CLI: {args:?}");
    }
}

/// The subscriber writing the events, without tracking the spans. The SDK
/// logs the canonical requests, the requests, and the responses in the trace
/// level, which the AWS CLI logs in the debug level.
struct DebugSubscriber;

impl Subscriber for DebugSubscriber {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        *metadata.level() <= Level::DEBUG || metadata.target().starts_with("aws_")
    }

    fn new_span(&self, _: &Attributes<'_>) -> Id {
        Id::from_u64(1)
    }

    fn record(&self, _: &Id, _: &Record<'_>) {}

    fn record_follows_from(&self, _: &Id, _: &Id) {}

    fn event(&self, event: &Event<'_>) {
        let metadata = event.metadata();
        let mut message = Message::default();
        event.record(&mut message);
        let thread = std::thread::current();
        let line = format!(
            "{} - {} - {} - {} - {}\n",
            chrono::Local::now().format("%Y-%m-%d %H:%M:%S,%3f"),
            thread.name().unwrap_or("thread"),
            metadata.target(),
            metadata.level(),
            message.0,
        );
        let _ = std::io::stderr().lock().write_all(line.as_bytes());
    }

    fn enter(&self, _: &Id) {}

    fn exit(&self, _: &Id) {}
}

/// The message of an event, followed by the other fields as `name=value`.
#[derive(Default)]
struct Message(String);

impl Visit for Message {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        if field.name() == "message" {
            let fields = std::mem::take(&mut self.0);
            let _ = write!(self.0, "{value:?}");
            if !fields.is_empty() {
                self.0.push(' ');
                self.0.push_str(&fields);
            }
        } else {
            if !self.0.is_empty() {
                self.0.push(' ');
            }
            let _ = write!(self.0, "{}={value:?}", field.name());
        }
    }
}
