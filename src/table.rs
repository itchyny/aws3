//! The tables of the outputs, ported from `awscli.table` of the AWS CLI, which
//! render the sections of the titles, the column headers and the rows, where
//! the nested sections are indented.

use crate::api::shape;
use crate::east_asian_width::WIDE_RANGES;
use crate::value::Value;

/// Builds the table of the value, whose title is the name of the operation,
/// like `TableFormatter` of the AWS CLI. Returns `None` for the false values,
/// which are not rendered.
pub fn build(title: &str, value: &Value) -> Result<Option<MultiTable>, String> {
    let mut table = MultiTable::default();
    Ok(table.build(Some(title), value, 0)?.then_some(table))
}

/// The tables of the sections, like `MultiTable` of the AWS CLI.
#[derive(Debug, Default)]
pub struct MultiTable {
    sections: Vec<Section>,
}

#[derive(Debug, Default)]
struct Section {
    title: String,
    headers: Vec<String>,
    rows: Vec<Vec<String>>,
    indent_level: usize,
    num_cols: Option<usize>,
    max_widths: Vec<usize>,
}

impl MultiTable {
    pub fn new_section(&mut self, title: &str, indent_level: usize) {
        self.sections.push(Section {
            title: title.to_owned(),
            indent_level,
            ..Section::default()
        });
    }

    pub fn add_row_header(&mut self, headers: Vec<String>) {
        let section = self.current();
        section.update_max_widths(&headers);
        section.num_cols.get_or_insert(headers.len());
        section.headers = headers;
    }

    /// Adds the row, which fails if the number of the columns differs.
    pub fn add_row(&mut self, row: Vec<String>) -> Result<(), String> {
        let section = self.current();
        let num_cols = *section.num_cols.get_or_insert(row.len());
        if row.len() != num_cols {
            return Err(format!(
                "Row should have {num_cols} elements, instead it has {}",
                row.len()
            ));
        }
        section.update_max_widths(&row);
        section.rows.push(row);
        Ok(())
    }

    fn build(
        &mut self,
        title: Option<&str>,
        value: &Value,
        indent_level: usize,
    ) -> Result<bool, String> {
        if is_false(value) {
            return Ok(false);
        }
        if let Some(title) = title {
            self.new_section(title, indent_level);
        }
        match value {
            Value::List(values) if matches!(values.first(), Some(Value::Map(_))) => {
                self.build_from_list(values, indent_level, title)?;
            }
            Value::List(values) => {
                for value in values {
                    match value {
                        Value::List(elements) if elements.iter().all(is_scalar) => {
                            self.add_row(elements.iter().map(shape::str).collect())?;
                        }
                        // The keys of the maps are scalars like Python.
                        Value::Map(pairs) => {
                            self.add_row(pairs.iter().map(|(key, _)| key.clone()).collect())?;
                        }
                        Value::List(_) => {
                            self.build(None, value, 0)?;
                        }
                        value => self.add_row(vec![shape::str(value)])?,
                    }
                }
            }
            Value::Map(pairs) => self.build_from_map(pairs, indent_level)?,
            _ => {}
        }
        Ok(true)
    }

    fn build_from_map(
        &mut self,
        pairs: &[(String, Value)],
        indent_level: usize,
    ) -> Result<(), String> {
        let (headers, more) = group_scalar_keys(pairs);
        if let [header] = headers.as_slice() {
            let value = pairs
                .iter()
                .find(|(key, _)| key == header)
                .map(|(_, value)| value);
            self.add_row(vec![
                header.clone(),
                value.map(shape::str).unwrap_or_default(),
            ])?;
        } else if !headers.is_empty() {
            let row = headers
                .iter()
                .map(|header| {
                    pairs
                        .iter()
                        .find(|(key, _)| key == header)
                        .map_or_else(String::new, |(_, value)| shape::str(value))
                })
                .collect();
            self.add_row_header(headers);
            self.add_row(row)?;
        }
        for key in more {
            let value = pairs
                .iter()
                .find(|(k, _)| *k == key)
                .map(|(_, value)| value);
            if let Some(value) = value {
                self.build(Some(&key), value, indent_level + 1)?;
            }
        }
        Ok(())
    }

    fn build_from_list(
        &mut self,
        values: &[Value],
        indent_level: usize,
        title: Option<&str>,
    ) -> Result<(), String> {
        let mut headers = Vec::new();
        let mut more = Vec::new();
        for value in values {
            let Value::Map(pairs) = value else {
                return Err(format!(
                    "{} indices must be integers or slices, not str",
                    if matches!(value, Value::Str(_)) {
                        "string"
                    } else {
                        shape::type_name(value)
                    }
                ));
            };
            let (h, m) = group_scalar_keys(pairs);
            headers.extend(h);
            more.extend(m);
        }
        headers.sort();
        headers.dedup();
        more.sort();
        more.dedup();
        self.add_row_header(headers.clone());
        for (i, value) in values.iter().enumerate() {
            let Value::Map(pairs) = value else {
                continue;
            };
            if i > 0 && !more.is_empty() {
                self.new_section(title.unwrap_or_default(), indent_level);
                self.add_row_header(headers.clone());
            }
            let row = headers
                .iter()
                .map(|header| {
                    pairs
                        .iter()
                        .find(|(key, _)| key == header)
                        .map_or_else(String::new, |(_, value)| shape::str(value))
                })
                .collect();
            self.add_row(row)?;
            for key in &more {
                if let Some((_, value)) = pairs.iter().find(|(k, _)| k == key) {
                    self.build(Some(key), value, indent_level + 1)?;
                }
            }
        }
        Ok(())
    }

    fn current(&mut self) -> &mut Section {
        if self.sections.is_empty() {
            self.sections.push(Section::default());
        }
        self.sections.last_mut().expect("a section")
    }

    /// Renders the sections, which are converted to the vertical tables if
    /// they do not fit in the width of the terminal.
    pub fn render(mut self, terminal_width: usize, colored: bool) -> String {
        let mut max_width = self.max_width();
        if max_width > terminal_width {
            self.convert_to_vertical();
            max_width = self.max_width();
        }
        let mut output = format!("{}\n", "-".repeat(max_width));
        for section in &self.sections {
            section.render(max_width, colored, &mut output);
        }
        output
    }

    fn max_width(&self) -> usize {
        self.sections
            .iter()
            .map(Section::total_width)
            .max()
            .unwrap_or_default()
    }

    /// Converts the sections of a row with the headers to the rows of the
    /// pairs of the headers and the elements.
    fn convert_to_vertical(&mut self) {
        for section in &mut self.sections {
            if section.rows.len() == 1 && !section.headers.is_empty() {
                let headers = std::mem::take(&mut section.headers);
                let row = section.rows.remove(0);
                let mut converted = Section {
                    title: std::mem::take(&mut section.title),
                    indent_level: section.indent_level,
                    ..Section::default()
                };
                for (header, element) in headers.into_iter().zip(row) {
                    let _ = converted.add_row(vec![header, element]);
                }
                *section = converted;
            }
        }
    }
}

impl Section {
    fn add_row(&mut self, row: Vec<String>) -> Result<(), String> {
        self.num_cols.get_or_insert(row.len());
        self.update_max_widths(&row);
        self.rows.push(row);
        Ok(())
    }

    fn update_max_widths(&mut self, row: &[String]) {
        if self.max_widths.is_empty() {
            self.max_widths = row.iter().map(|element| text_length(element)).collect();
        } else {
            for (width, element) in self.max_widths.iter_mut().zip(row) {
                *width = (*width).max(text_length(element));
            }
        }
    }

    /// Returns the widths of the columns scaled to the maximum width.
    fn column_widths(&self, padding: usize, max_width: Option<usize>) -> Vec<usize> {
        let unscaled: Vec<_> = self
            .max_widths
            .iter()
            .map(|width| width + padding)
            .collect();
        let Some(max_width) = max_width else {
            return unscaled;
        };
        if unscaled.is_empty() {
            return unscaled;
        }
        let total: usize = unscaled.iter().sum();
        let scale = max_width as f64 / total as f64;
        let mut scaled: Vec<i64> = unscaled
            .iter()
            .map(|&width| (scale * width as f64).round_ties_even() as i64)
            .collect();
        let mut off_by = scaled.iter().sum::<i64>() - max_width as i64;
        while off_by != 0 {
            let order: Vec<usize> = if off_by < 0 {
                (0..scaled.len()).rev().collect()
            } else {
                (0..scaled.len()).collect()
            };
            for i in order {
                if off_by > 0 {
                    scaled[i] -= 1;
                    off_by -= 1;
                } else {
                    scaled[i] += 1;
                    off_by += 1;
                }
                if off_by == 0 {
                    break;
                }
            }
        }
        scaled
            .into_iter()
            .map(|width| width.max(0) as usize)
            .collect()
    }

    /// Returns the width with the borders and the indentation.
    fn total_width(&self) -> usize {
        let outer = 2 * self.indent_level;
        let total = self
            .column_widths(0, None)
            .iter()
            .map(|width| width + 4)
            .sum::<usize>()
            + 2
            + outer;
        total.max(text_length(&self.title) + 2 + outer)
    }

    fn render(&self, max_width: usize, colored: bool, output: &mut String) {
        let indent = if colored {
            "\x1b[2m\x1b[33m|\x1b[0m"
        } else {
            "|"
        }
        .repeat(self.indent_level);
        let mut write_line = |line: &str| {
            output.push_str(&indent);
            output.push_str(line);
            output.push_str(&indent);
            output.push('\n');
        };
        let max_width = max_width - 2 * self.indent_level;
        if !self.title.is_empty() {
            write_line(&center_text(&self.title, max_width, "|", "|"));
            if self.headers.is_empty() && self.rows.is_empty() {
                write_line(&format!("+{}+", "-".repeat(max_width.saturating_sub(2))));
            }
        }
        if !self.headers.is_empty() {
            let widths = self.column_widths(4, Some(max_width));
            let mut line = String::new();
            for (i, (width, header)) in widths.iter().zip(&self.headers).enumerate() {
                let left_edge = if i == 0 { "|" } else { "" };
                line += &center_text(header, *width, left_edge, "|");
            }
            write_line(&line_break(&widths));
            write_line(&line);
        }
        if !self.rows.is_empty() {
            let widths = self.column_widths(4, Some(max_width));
            if widths.is_empty() {
                return;
            }
            write_line(&line_break(&widths));
            for row in &self.rows {
                let mut line = String::new();
                for (i, (width, element)) in widths.iter().zip(row).enumerate() {
                    let left_edge = if i == 0 { "|" } else { "" };
                    line += &align_left(element, *width, left_edge, "|", colored);
                }
                write_line(&line);
            }
            write_line(&line_break(&widths));
        }
    }
}

/// Returns whether the value is false in Python.
fn is_false(value: &Value) -> bool {
    match value {
        Value::Str(string) => string.is_empty(),
        Value::Bytes(bytes) => bytes.is_empty(),
        Value::Int(digits) => digits
            .trim_start_matches('-')
            .trim_start_matches('0')
            .is_empty(),
        Value::Float(value) => *value == 0.0,
        Value::Bool(value) => !value,
        Value::Null => true,
        Value::List(values) => values.is_empty(),
        Value::Map(pairs) => pairs.is_empty(),
    }
}

fn is_scalar(value: &Value) -> bool {
    !matches!(value, Value::List(_) | Value::Map(_))
}

/// Returns the sorted keys of the scalars and of the others.
fn group_scalar_keys(pairs: &[(String, Value)]) -> (Vec<String>, Vec<String>) {
    let (mut headers, mut more): (Vec<String>, Vec<String>) = (Vec::new(), Vec::new());
    for (key, value) in pairs {
        if is_scalar(value) {
            headers.push(key.clone());
        } else {
            more.push(key.clone());
        }
    }
    headers.sort();
    more.sort();
    (headers, more)
}

fn line_break(widths: &[usize]) -> String {
    let mut line = String::new();
    for (i, width) in widths.iter().enumerate() {
        if i == 0 {
            line += &format!("+{}+", "-".repeat(width.saturating_sub(2)));
        } else {
            line += &format!("{}+", "-".repeat(width.saturating_sub(1)));
        }
    }
    line
}

fn center_text(text: &str, length: usize, left_edge: &str, right_edge: &str) -> String {
    let text_length = text_length(text) as i64;
    let char_start = length as i64 / 2 - text_length / 2 - 1;
    let so_far = left_edge.len() as i64 + char_start + text_length;
    let right = length as i64 - right_edge.len() as i64 - so_far;
    format!(
        "{left_edge}{}{text}{}{right_edge}",
        " ".repeat(char_start.max(0) as usize),
        " ".repeat(right.max(0) as usize)
    )
}

fn align_left(
    text: &str,
    length: usize,
    left_edge: &str,
    right_edge: &str,
    colored: bool,
) -> String {
    let text_length = text_length(text);
    let computed = text_length + 2 + left_edge.len() + right_edge.len();
    let padding = if length >= computed { 2 } else { 0 };
    let right = length.saturating_sub(left_edge.len() + padding + text_length + right_edge.len());
    // The colors of the row elements of `ColorizedStyler` of the AWS CLI.
    let text = if colored {
        format!("\x1b[1m\x1b[34m{text}\x1b[0m")
    } else {
        text.to_owned()
    };
    format!(
        "{left_edge}{}{text}{}{right_edge}",
        " ".repeat(padding),
        " ".repeat(right)
    )
}

/// Returns the width of the text, where the wide, fullwidth and ambiguous
/// characters are counted as two like `get_text_length` of the AWS CLI.
pub fn text_length(text: &str) -> usize {
    text.chars()
        .map(|ch| {
            let code = ch as u32;
            let wide = WIDE_RANGES
                .binary_search_by(|&(start, end)| {
                    if end < code {
                        std::cmp::Ordering::Less
                    } else if start > code {
                        std::cmp::Ordering::Greater
                    } else {
                        std::cmp::Ordering::Equal
                    }
                })
                .is_ok();
            if wide { 2 } else { 1 }
        })
        .sum()
}

/// Returns the width of the terminal of the standard output, which is 80 if
/// unavailable like `determine_terminal_width` of the AWS CLI.
pub fn terminal_width() -> usize {
    #[cfg(unix)]
    {
        let mut size = libc::winsize {
            ws_row: 0,
            ws_col: 0,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        // SAFETY: TIOCGWINSZ writes the size to the struct of the argument.
        if unsafe { libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, &mut size) } == 0 {
            return usize::from(size.ws_col);
        }
    }
    80
}
