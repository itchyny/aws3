//! The outputs of the responses in the formats of `--output`, which are
//! filtered by `--query`.

use std::io::{self, Write};

use clap::ValueEnum;

use super::jmespath::Expression;
use super::response::Responses;
use crate::cli::{ApiOperation, GlobalArgs};
use crate::config::ProfileConfig;
use crate::error::Error;
use crate::output::{table, text, yaml};
use crate::value::Value;

/// The formats of the outputs of `--output`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Json,
    Text,
    Table,
    Yaml,
    YamlStream,
    Off,
}

/// Returns the name of the operation, e.g. `HeadObject`, which is the name of
/// the variant of the subcommand.
fn operation_name(operation: &ApiOperation) -> String {
    let debug = format!("{operation:?}");
    debug
        .split(['(', ' '])
        .next()
        .unwrap_or_default()
        .to_owned()
}

/// Returns the format of the outputs of the option, the environment variable
/// or the config file, like the AWS CLI.
pub fn output_format(global: &GlobalArgs) -> Result<Format, Error> {
    let name = match global.output {
        Some(output) => output
            .to_possible_value()
            .map(|value| value.get_name().to_owned())
            .unwrap_or_default(),
        None => std::env::var("AWS_DEFAULT_OUTPUT")
            .ok()
            .or_else(|| ProfileConfig::load(global.profile.as_deref()).output)
            .unwrap_or_else(|| "json".to_owned()),
    };
    match name.as_str() {
        "json" => Ok(Format::Json),
        "text" => Ok(Format::Text),
        "table" => Ok(Format::Table),
        "yaml" => Ok(Format::Yaml),
        "yaml-stream" => Ok(Format::YamlStream),
        "off" => Ok(Format::Off),
        _ => Err(Error::new(format!("Unknown output type: {name}"))),
    }
}

/// Prints the responses in the format after applying the query, ignoring the
/// errors of writing like the other outputs of the commands. The empty objects
/// are not printed in JSON and YAML, and the pages are printed one by one in
/// text and the streamed YAML.
pub fn print(
    operation: &ApiOperation,
    responses: Responses,
    format: Format,
    query: Option<&Expression>,
    colored: bool,
) -> Result<(), Error> {
    let search = |value: Value| match query {
        Some(query) => query
            .search(&value)
            .map_err(|err| Error::new(err.to_string())),
        None => Ok(value),
    };
    match format {
        Format::Json => {
            let response = search(match responses {
                Responses::Value(value) => value,
                Responses::Pages(pages) => pages.merge(),
            })?;
            if !matches!(&response, Value::Map(pairs) if pairs.is_empty()) {
                let _ = writeln!(io::stdout(), "{}", response.to_json_pretty());
            }
        }
        Format::Text => {
            let responses = match responses {
                Responses::Value(value) => vec![value],
                Responses::Pages(pages) => pages.into_pages(),
            };
            for response in responses {
                let _ = io::stdout().write_all(
                    text::format(&search(response)?)
                        .map_err(Error::new)?
                        .as_bytes(),
                );
            }
        }
        Format::Table => {
            let response = search(match responses {
                Responses::Value(value) => value,
                Responses::Pages(pages) => pages.merge(),
            })?;
            if let Some(table) =
                table::build(&operation_name(operation), &response).map_err(Error::new)?
            {
                let _ = io::stdout().write_all(
                    table
                        .render(table::terminal_width(), colored, true)
                        .as_bytes(),
                );
            }
        }
        Format::Yaml => {
            let response = search(match responses {
                Responses::Value(value) => value,
                Responses::Pages(pages) => pages.merge(),
            })?;
            if !matches!(&response, Value::Map(pairs) if pairs.is_empty()) {
                let _ = io::stdout().write_all(yaml(&response).as_bytes());
            }
        }
        Format::YamlStream => {
            let responses = match responses {
                Responses::Value(value) => {
                    let response = search(value)?;
                    if matches!(&response, Value::Map(pairs) if pairs.is_empty()) {
                        return Ok(());
                    }
                    vec![response]
                }
                Responses::Pages(pages) => pages.into_responses(),
            };
            for response in responses {
                let response = Value::List(vec![search(response)?]);
                let _ = io::stdout().write_all(yaml(&response).as_bytes());
            }
        }
        Format::Off => {}
    }
    Ok(())
}

/// Formats the response in YAML, where the scalars are formatted in JSON like
/// `YAMLDumper` of the AWS CLI.
fn yaml(response: &Value) -> String {
    match response {
        Value::List(_) | Value::Map(_) => yaml::dump(response),
        _ => format!("{}\n", response.to_json_pretty()),
    }
}
