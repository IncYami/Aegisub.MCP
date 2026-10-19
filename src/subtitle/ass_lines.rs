use super::{Result, error};
use std::ops::Range;

#[derive(Clone, Debug)]
pub(super) struct AssLine {
    pub id: usize,
    pub line_range: Range<usize>,
    pub full_range: Range<usize>,
    pub fields: Vec<String>,
    pub values: Vec<String>,
    pub value_ranges: Vec<Range<usize>>,
}

impl AssLine {
    pub fn index(&self, name: &str) -> Option<usize> {
        self.fields
            .iter()
            .position(|field| field.eq_ignore_ascii_case(name))
    }

    pub fn value(&self, name: &str) -> Option<&str> {
        self.index(name).map(|index| self.values[index].trim())
    }

    pub fn raw_line<'a>(&self, content: &'a str) -> &'a str {
        &content[self.line_range.clone()]
    }
}

pub(super) fn dialogue_lines(content: &str) -> Result<Vec<AssLine>> {
    let mut records = Vec::new();
    let mut in_events = false;
    let mut format = Vec::<String>::new();
    let mut offset = 0;
    for full in content.split_inclusive('\n') {
        let line = full.trim_end_matches(['\r', '\n']);
        let trimmed = line.trim().trim_start_matches('\u{feff}');
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_events = trimmed.eq_ignore_ascii_case("[Events]");
            format.clear();
        } else if in_events && let Some((kind, data)) = line.split_once(':') {
            if kind.trim().eq_ignore_ascii_case("Format") {
                format = data
                    .split(',')
                    .map(|field| field.trim().to_owned())
                    .collect();
            } else if kind.trim().eq_ignore_ascii_case("Dialogue") && !format.is_empty() {
                if !format
                    .last()
                    .is_some_and(|field| field.eq_ignore_ascii_case("Text"))
                {
                    return Err(error("ASS Events Format must end with Text"));
                }
                let mut ranges = Vec::with_capacity(format.len());
                let mut values = Vec::with_capacity(format.len());
                let mut field_start = offset + kind.len() + 1;
                let mut rest = data;
                for _ in 0..format.len() - 1 {
                    let comma = rest.find(',').ok_or_else(|| {
                        error(format!("Malformed Dialogue event at byte {offset}"))
                    })?;
                    let field_end = field_start + comma;
                    ranges.push(field_start..field_end);
                    values.push(content[field_start..field_end].to_owned());
                    field_start = field_end + 1;
                    rest = &rest[comma + 1..];
                }
                let end = offset + line.len();
                ranges.push(field_start..end);
                values.push(content[field_start..end].to_owned());
                records.push(AssLine {
                    id: records.len() + 1,
                    line_range: offset..end,
                    full_range: offset..offset + full.len(),
                    fields: format.clone(),
                    values,
                    value_ranges: ranges,
                });
            }
        }
        offset += full.len();
    }
    Ok(records)
}

pub(super) fn non_dialogue_content(content: &str, lines: &[AssLine]) -> String {
    let mut result = String::new();
    let mut cursor = 0;
    for line in lines {
        result.push_str(&content[cursor..line.full_range.start]);
        cursor = line.full_range.end;
    }
    result.push_str(&content[cursor..]);
    result
}
