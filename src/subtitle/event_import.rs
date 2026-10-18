use super::ass::{hash, is_drawing, split_tags};
use super::autosave::{lock_for_edit, write_with_autosave};
use super::context::parse_ass_time;
use super::edit::EDIT_LOCK;
use super::query::view;
use super::{Document, InsertEventsResult, Result, SubtitleFormat, error, timed_text};
use std::{collections::HashSet, fs, path::Path};

const EVENT_FORMAT: &str =
    "Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text";

fn format_ass_time(milliseconds: u64, end: bool) -> Result<String> {
    let centiseconds = if end {
        milliseconds.div_ceil(10)
    } else {
        milliseconds / 10
    };
    let hours = centiseconds / 360_000;
    if hours > 9 {
        return Err(error("Cue time exceeds the ASS one-digit hour range"));
    }
    Ok(format!(
        "{hours}:{:02}:{:02}.{:02}",
        centiseconds / 6_000 % 60,
        centiseconds / 100 % 60,
        centiseconds % 100
    ))
}

fn event_line(event: &super::Event) -> Result<String> {
    let (start_ms, end_ms) = if event.format == SubtitleFormat::Ass {
        (parse_ass_time(&event.start), parse_ass_time(&event.end))
    } else {
        (
            timed_text::timestamp_ms(&event.start, event.format),
            timed_text::timestamp_ms(&event.end, event.format),
        )
    };
    let start_ms =
        start_ms.ok_or_else(|| error(format!("Invalid source event {} start", event.id)))?;
    let end_ms = end_ms.ok_or_else(|| error(format!("Invalid source event {} end", event.id)))?;
    if end_ms <= start_ms {
        return Err(error(format!("Invalid source event {} duration", event.id)));
    }
    let text = if event.format == SubtitleFormat::Ass {
        if is_drawing(&event.raw_text) || split_tags(&event.raw_text).is_err() {
            return Err(error(format!(
                "Source event {} has a drawing or malformed ASS tags",
                event.id
            )));
        }
        event.raw_text.clone()
    } else {
        if !timed_text::editable(&event.raw_text)
            || event.raw_text.contains(['{', '}', '\\'])
            || event
                .raw_text
                .chars()
                .any(|ch| ch.is_control() && ch != '\r' && ch != '\n')
        {
            return Err(error(format!(
                "Source cue {} has markup, entities, or ASS control characters",
                event.id
            )));
        }
        event
            .raw_text
            .replace("\r\n", "\n")
            .replace('\r', "\n")
            .replace('\n', "\\N")
    };
    Ok(format!(
        "Dialogue: 0,{},{},Default,,0,0,0,,{text}",
        format_ass_time(start_ms, false)?,
        format_ass_time(end_ms, true)?
    ))
}

fn events_insert_offset(content: &str) -> Result<usize> {
    let mut in_events = false;
    let mut saw_events = false;
    let mut formats = 0;
    let mut offset = 0;
    let mut insertion = content.len();
    for full_line in content.split_inclusive('\n') {
        let line = full_line.trim_end_matches(['\r', '\n']);
        let trimmed = line.trim().trim_start_matches('\u{feff}');
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            if in_events {
                insertion = offset;
                break;
            }
            in_events = trimmed.eq_ignore_ascii_case("[Events]");
            saw_events |= in_events;
        } else if in_events
            && let Some((name, value)) = line.split_once(':')
            && name.trim().eq_ignore_ascii_case("Format")
        {
            formats += 1;
            let fields = value.split(',').map(str::trim).collect::<Vec<_>>();
            let expected = EVENT_FORMAT.split(", ").collect::<Vec<_>>();
            if fields != expected {
                return Err(error(
                    "Destination ASS Events format is not the base format",
                ));
            }
        }
        offset += full_line.len();
    }
    if !saw_events || formats != 1 {
        return Err(error("Destination ASS needs one base Events Format line"));
    }
    Ok(insertion)
}

pub fn insert_events_from_source(
    destination: &str,
    expected_revision: &str,
    source: &str,
    source_ids: &[usize],
) -> Result<InsertEventsResult> {
    if source_ids.is_empty() {
        return Err(error("At least one source event ID is required"));
    }
    let destination_path = fs::canonicalize(destination)
        .map_err(|e| error(format!("Cannot resolve destination ASS: {e}")))?;
    let source_path = fs::canonicalize(source)
        .map_err(|e| error(format!("Cannot resolve source subtitle: {e}")))?;
    if destination_path == source_path {
        return Err(error("Source and destination subtitles must differ"));
    }
    if !destination_path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("ass"))
    {
        return Err(error("Destination must be an .ass file"));
    }
    let source_doc = Document::open(&source_path)?;
    let mut unique = HashSet::new();
    let mut lines = Vec::with_capacity(source_ids.len());
    for &id in source_ids {
        if !unique.insert(id) {
            return Err(error(format!("Duplicate source event ID {id}")));
        }
        let event = source_doc
            .events
            .get(id.checked_sub(1).ok_or_else(|| error("ID must be >= 1"))?)
            .ok_or_else(|| error(format!("Source event {id} not found")))?;
        lines.push(event_line(event)?);
    }

    let _guard = EDIT_LOCK
        .lock()
        .map_err(|_| error("Edit lock is unavailable"))?;
    let _file_lock = lock_for_edit(&destination_path)?;
    let destination_doc = Document::open(&destination_path)?;
    if destination_doc.revision != expected_revision {
        return Err(error(
            "Revision conflict: destination changed; fetch it again",
        ));
    }
    let insertion = events_insert_offset(&destination_doc.content)?;
    let newline = if destination_doc.content.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let mut inserted = String::new();
    if insertion > 0 && !destination_doc.content[..insertion].ends_with(['\r', '\n']) {
        inserted.push_str(newline);
    }
    for line in &lines {
        inserted.push_str(line);
        inserted.push_str(newline);
    }
    let mut output = String::with_capacity(destination_doc.content.len() + inserted.len());
    output.push_str(&destination_doc.content[..insertion]);
    output.push_str(&inserted);
    output.push_str(&destination_doc.content[insertion..]);

    let current_source = fs::read(&source_path)
        .map_err(|e| error(format!("Cannot recheck source subtitle: {e}")))?;
    if hash(&current_source) != source_doc.revision {
        return Err(error("Source changed during insertion"));
    }
    let autosave_path = write_with_autosave(&destination_path, &destination_doc.revision, &output)?;
    let updated = Document::open(Path::new(&destination_path))?;
    let first_id = destination_doc.events.len() + 1;
    let events = (first_id..first_id + lines.len())
        .map(|id| {
            updated
                .events
                .get(id - 1)
                .map(|event| view(event, false))
                .ok_or_else(|| error(format!("Inserted event {id} is missing")))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(InsertEventsResult {
        previous_revision: destination_doc.revision,
        revision: hash(output.as_bytes()),
        autosave_path: autosave_path.display().to_string(),
        source_revision: source_doc.revision,
        events,
    })
}
