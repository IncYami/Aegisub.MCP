use super::ass::{hash, is_drawing, split_tags};
use super::autosave::{lock_for_edit, write_with_autosave};
use super::context::parse_ass_time;
use super::edit::EDIT_LOCK;
use super::query::view;
use super::{BatchResult, Document, Result, error};
use std::{collections::HashSet, fs};

pub struct NewAssEvent {
    pub start: String,
    pub end: String,
    pub text: String,
    pub style: Option<String>,
    pub layer: Option<String>,
    pub name: Option<String>,
    pub margin_l: Option<String>,
    pub margin_r: Option<String>,
    pub margin_v: Option<String>,
    pub effect: Option<String>,
}

fn validate_time(value: &str, name: &str) -> Result<u64> {
    let valid_shape = value.split_once(':').is_some_and(|(hours, rest)| {
        !hours.is_empty()
            && hours.bytes().all(|byte| byte.is_ascii_digit())
            && hours.parse::<u16>().is_ok_and(|hours| hours <= 595)
            && rest.len() == 8
            && rest.as_bytes()[2] == b':'
            && rest.as_bytes()[5] == b'.'
            && rest
                .bytes()
                .enumerate()
                .all(|(index, byte)| matches!(index, 2 | 5) || byte.is_ascii_digit())
    });
    if !valid_shape {
        return Err(error(format!("{name} must use ASS h:mm:ss.cc format")));
    }
    parse_ass_time(value).ok_or_else(|| error(format!("Invalid ASS {name} time")))
}

fn valid_text(text: &str) -> Result<()> {
    if text.contains(['\r', '\n']) || text.chars().any(char::is_control) {
        return Err(error(
            "ASS text must not contain physical line breaks or control characters",
        ));
    }
    let mut in_block = false;
    for ch in text.chars() {
        match ch {
            '{' if in_block => return Err(error("ASS text contains nested override braces")),
            '{' => in_block = true,
            '}' if !in_block => return Err(error("ASS text contains an unmatched closing brace")),
            '}' => in_block = false,
            _ => {}
        }
    }
    if in_block {
        return Err(error("ASS text contains an unclosed override block"));
    }
    if is_drawing(text) || split_tags(text).is_err() {
        return Err(error(
            "ASS text contains drawing commands or malformed override tags",
        ));
    }
    Ok(())
}

fn event_format(content: &str) -> Result<(usize, Vec<String>)> {
    let mut in_events = false;
    let mut found = None;
    let mut saw_events = false;
    let mut offset = 0;
    let mut insertion = content.len();
    for line in content.split_inclusive('\n') {
        let bare = line.trim_end_matches(['\r', '\n']);
        let trimmed = bare.trim().trim_start_matches('\u{feff}');
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            if in_events {
                insertion = offset;
            }
            in_events = trimmed.eq_ignore_ascii_case("[Events]");
            if in_events {
                if saw_events {
                    return Err(error("ASS must contain only one Events section"));
                }
                saw_events = true;
            }
        } else if in_events
            && let Some((name, value)) = bare.split_once(':')
            && name.trim().eq_ignore_ascii_case("Format")
        {
            if found.is_some() {
                return Err(error(
                    "ASS Events section must have one unambiguous Format line",
                ));
            }
            let fields = value
                .split(',')
                .map(str::trim)
                .map(str::to_owned)
                .collect::<Vec<_>>();
            let mut names = HashSet::new();
            if fields.is_empty()
                || fields
                    .iter()
                    .any(|field| field.is_empty() || !names.insert(field.to_ascii_lowercase()))
                || !fields
                    .last()
                    .is_some_and(|field| field.eq_ignore_ascii_case("Text"))
            {
                return Err(error(
                    "ASS Events Format has duplicate, empty, or unsupported field order",
                ));
            }
            for field in &fields {
                if ![
                    "layer", "start", "end", "style", "name", "marginl", "marginr", "marginv",
                    "effect", "text",
                ]
                .contains(&field.to_ascii_lowercase().as_str())
                {
                    return Err(error(format!("Unsupported ASS Events field: {field}")));
                }
            }
            for required in ["Start", "End", "Style", "Text"] {
                if !fields
                    .iter()
                    .any(|field| field.eq_ignore_ascii_case(required))
                {
                    return Err(error(format!("ASS Events Format must contain {required}")));
                }
            }
            found = Some(fields);
        }
        offset += line.len();
    }
    found
        .map(|fields| (insertion, fields))
        .ok_or_else(|| error("Destination ASS needs one Events section with a Format line"))
}

fn defined_styles(content: &str) -> HashSet<String> {
    let mut styles = HashSet::new();
    let mut in_styles = false;
    let mut fields = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim().trim_start_matches('\u{feff}');
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_styles = trimmed.eq_ignore_ascii_case("[V4+ Styles]")
                || trimmed.eq_ignore_ascii_case("[V4 Styles]");
            fields.clear();
            continue;
        }
        if !in_styles {
            continue;
        }
        let Some((kind, value)) = line.split_once(':') else {
            continue;
        };
        if kind.trim().eq_ignore_ascii_case("Format") {
            fields = value.split(',').map(str::trim).map(str::to_owned).collect();
        } else if kind.trim().eq_ignore_ascii_case("Style")
            && !fields.is_empty()
            && let Some(index) = fields
                .iter()
                .position(|field| field.eq_ignore_ascii_case("Name"))
            && let Some(name) = value
                .split(',')
                .nth(index)
                .map(str::trim)
                .filter(|name| !name.is_empty())
        {
            styles.insert(name.to_ascii_lowercase());
        }
    }
    styles
}

fn field_value<'a>(event: &'a NewAssEvent, field: &str) -> &'a str {
    if field.eq_ignore_ascii_case("Start") {
        &event.start
    } else if field.eq_ignore_ascii_case("End") {
        &event.end
    } else if field.eq_ignore_ascii_case("Text") {
        &event.text
    } else if field.eq_ignore_ascii_case("Style") {
        event.style.as_deref().unwrap_or("Default")
    } else if field.eq_ignore_ascii_case("Layer") {
        event.layer.as_deref().unwrap_or("0")
    } else if field.eq_ignore_ascii_case("Name") {
        event.name.as_deref().unwrap_or("")
    } else if field.eq_ignore_ascii_case("MarginL") {
        event.margin_l.as_deref().unwrap_or("0")
    } else if field.eq_ignore_ascii_case("MarginR") {
        event.margin_r.as_deref().unwrap_or("0")
    } else if field.eq_ignore_ascii_case("MarginV") {
        event.margin_v.as_deref().unwrap_or("0")
    } else {
        event.effect.as_deref().unwrap_or("")
    }
}

fn render_event(
    event: &NewAssEvent,
    fields: &[String],
    styles: &HashSet<String>,
) -> Result<String> {
    let start = validate_time(&event.start, "start")?;
    let end = validate_time(&event.end, "end")?;
    if end <= start {
        return Err(error("ASS event end must be later than start"));
    }
    valid_text(&event.text)?;
    let style = event.style.as_deref().unwrap_or("Default");
    if style.contains(',')
        || style.chars().any(char::is_control)
        || !styles.contains(&style.to_ascii_lowercase())
    {
        return Err(error(format!(
            "ASS style is not defined in the destination: {style}"
        )));
    }
    for (field, value) in [
        ("Layer", event.layer.as_deref()),
        ("Name", event.name.as_deref()),
        ("MarginL", event.margin_l.as_deref()),
        ("MarginR", event.margin_r.as_deref()),
        ("MarginV", event.margin_v.as_deref()),
        ("Effect", event.effect.as_deref()),
    ] {
        if let Some(value) = value {
            if !fields
                .iter()
                .any(|format_field| format_field.eq_ignore_ascii_case(field))
            {
                return Err(error(format!(
                    "ASS Events Format does not include supplied {field} field"
                )));
            }
            if value.contains([',', '\r', '\n']) || value.chars().any(char::is_control) {
                return Err(error(format!(
                    "ASS {field} must not contain commas, line breaks, or control characters"
                )));
            }
            if field == "Layer"
                && !value
                    .parse::<u32>()
                    .is_ok_and(|layer| layer <= i32::MAX as u32)
            {
                return Err(error("ASS Layer must be an integer from 0 to 2147483647"));
            }
            if field.starts_with("Margin") && value.parse::<u32>().is_err() {
                return Err(error(format!("ASS {field} must be a nonnegative integer")));
            }
        }
    }
    Ok(format!(
        "Dialogue: {}",
        fields
            .iter()
            .map(|field| field_value(event, field))
            .collect::<Vec<_>>()
            .join(",")
    ))
}

pub fn create_ass_events(
    destination: &str,
    expected_revision: &str,
    events: &[NewAssEvent],
) -> Result<BatchResult> {
    if events.is_empty() {
        return Err(error("At least one ASS event is required"));
    }
    let path = fs::canonicalize(destination)
        .map_err(|e| error(format!("Cannot resolve destination ASS: {e}")))?;
    if path
        .components()
        .any(|component| component.as_os_str() == ".backup" || component.as_os_str() == ".autosave")
    {
        return Err(error("Files inside .backup or .autosave cannot be edited"));
    }
    if !path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("ass"))
    {
        return Err(error("Destination must be an .ass file"));
    }
    let _guard = EDIT_LOCK
        .lock()
        .map_err(|_| error("Edit lock is unavailable"))?;
    let _file_lock = lock_for_edit(&path)?;
    let doc = Document::open(&path)?;
    if doc.revision != expected_revision {
        return Err(error(
            "Revision conflict: destination changed; fetch it again",
        ));
    }
    let (insertion, fields) = event_format(&doc.content)?;
    let styles = defined_styles(&doc.content);
    let lines = events
        .iter()
        .map(|event| render_event(event, &fields, &styles))
        .collect::<Result<Vec<_>>>()?;
    let newline = if doc.content.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let mut inserted = String::new();
    if insertion > 0 && !doc.content[..insertion].ends_with(['\r', '\n']) {
        inserted.push_str(newline);
    }
    for line in &lines {
        inserted.push_str(line);
        inserted.push_str(newline);
    }
    let mut output = String::with_capacity(doc.content.len() + inserted.len());
    output.push_str(&doc.content[..insertion]);
    output.push_str(&inserted);
    output.push_str(&doc.content[insertion..]);
    let current =
        fs::read(&path).map_err(|e| error(format!("Cannot recheck destination ASS: {e}")))?;
    if hash(&current) != doc.revision {
        return Err(error("Destination changed during event creation"));
    }
    let autosave_path = write_with_autosave(&path, &doc.revision, &output)?;
    let updated = Document::open(&path)?;
    let first_id = doc.events.len() + 1;
    let created = (first_id..first_id + lines.len())
        .map(|id| {
            updated
                .events
                .get(id - 1)
                .map(|event| view(event, true))
                .ok_or_else(|| error(format!("Created event {id} is missing")))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(BatchResult {
        previous_revision: doc.revision,
        revision: hash(output.as_bytes()),
        autosave_path: autosave_path.display().to_string(),
        events: created,
    })
}
