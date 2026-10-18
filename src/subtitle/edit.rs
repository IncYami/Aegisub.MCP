use super::ass::{hash, is_drawing, split_tags};
use super::automation::{GradientPlan, is_per_character_effect};
use super::query::view;
use super::{
    BatchResult, Document, ReplaceResult, Result, SubtitleFormat, TextEdit, error, timed_text,
};
use std::{collections::HashSet, fs, io::Write, path::Path, sync::Mutex};

pub(super) static EDIT_LOCK: Mutex<()> = Mutex::new(());
fn count_escape(text: &str, escaped: &str) -> usize {
    text.match_indices(escaped).count()
}

fn validate_escapes(source: &str, replacement: &str) -> Result<()> {
    for escape in ["\\N", "\\n", "\\h"] {
        if count_escape(source, escape) != count_escape(replacement, escape) {
            return Err(error(format!("Escape {escape} count changed")));
        }
    }
    Ok(())
}

fn rebuild(raw: &str, replacement: &str) -> Result<String> {
    if replacement.contains(['\r', '\n']) {
        return Err(error("Use ASS escape \\N instead of a physical newline"));
    }
    let (visible, tags) = split_tags(raw)?;
    validate_escapes(&visible, replacement)?;
    let mut leading = String::new();
    let mut tail = raw;
    while tail.starts_with('{') {
        let Some(end) = tail.find('}') else {
            return Err(error("Unclosed ASS override block"));
        };
        leading.push_str(&tail[..=end]);
        tail = &tail[end + 1..];
    }
    let mut rebuilt = leading;
    let mut rest = replacement;
    for (index, tag) in tags.iter().enumerate() {
        let marker = format!("{{{}}}", index + 1);
        let Some(position) = rest.find(&marker) else {
            return Err(error(format!("Missing tag marker {marker}")));
        };
        let text = &rest[..position];
        if text.contains(['{', '}']) {
            return Err(error("Unexpected braces in visible text"));
        }
        rebuilt.push_str(text);
        rebuilt.push_str(tag);
        rest = &rest[position + marker.len()..];
    }
    if rest.contains(['{', '}']) {
        return Err(error("Unexpected braces or repeated tag marker"));
    }
    rebuilt.push_str(rest);
    Ok(rebuilt)
}

fn plain_line_ending(text: &str) -> &'static str {
    if text.contains("\r\n") {
        "\r\n"
    } else if text.contains('\n') {
        "\n"
    } else if text.contains('\r') {
        "\r"
    } else {
        "\n"
    }
}

fn rebuild_plain(
    raw: &str,
    document: &str,
    replacement: &str,
    format: SubtitleFormat,
) -> Result<String> {
    if !timed_text::editable(raw) {
        return Err(error("Formatted SRT/WebVTT cues are read-only"));
    }
    if replacement.contains(['\r', '<', '>'])
        || replacement.split('\n').any(|line| line.trim().is_empty())
        || replacement.is_empty()
        || (format == SubtitleFormat::Vtt && replacement.contains("-->"))
    {
        return Err(error("Invalid plain cue text or unsupported markup"));
    }
    let line_ending = if raw.contains(['\r', '\n']) {
        plain_line_ending(raw)
    } else {
        plain_line_ending(document)
    };
    Ok(replacement.replace('\n', line_ending))
}

fn match_offsets(text: &str, needle: &str) -> Vec<usize> {
    text.char_indices()
        .filter_map(|(offset, _)| text[offset..].starts_with(needle).then_some(offset))
        .collect()
}

fn unique_offset(offsets: &[usize]) -> Result<usize> {
    match offsets {
        [offset] => Ok(*offset),
        [] => Err(error("Text was not found within one visible segment")),
        _ => Err(error("Text occurs more than once in this event")),
    }
}

fn ass_segment_offsets(raw: &str, needle: &str) -> Result<Vec<usize>> {
    let mut offsets = Vec::new();
    let mut rest = raw;
    let mut base = 0;
    while let Some(start) = rest.find('{') {
        offsets.extend(
            match_offsets(&rest[..start], needle)
                .into_iter()
                .map(|at| base + at),
        );
        let after = &rest[start + 1..];
        let end = after
            .find('}')
            .ok_or_else(|| error("Unclosed ASS override block"))?;
        base += start + end + 2;
        rest = &after[end + 1..];
    }
    offsets.extend(match_offsets(rest, needle).into_iter().map(|at| base + at));
    Ok(offsets)
}

pub fn replace_segment(
    path: &str,
    id: usize,
    expected_revision: &str,
    old_text: &str,
    new_text: &str,
) -> Result<ReplaceResult> {
    if old_text.is_empty() || old_text.contains(['\r', '\n']) || new_text.contains(['\r', '\n']) {
        return Err(error(
            "Search text must be nonempty; segment text cannot contain physical line breaks",
        ));
    }
    let doc = Document::open(Path::new(path))?;
    if doc.revision != expected_revision {
        return Err(error(
            "Revision conflict: file changed; fetch the event again",
        ));
    }
    let event = doc
        .events
        .get(id.checked_sub(1).ok_or_else(|| error("ID must be >= 1"))?)
        .ok_or_else(|| error(format!("Event {id} not found")))?;
    if !view(event, false).editable {
        return Err(error(format!("Event {id} is read-only")));
    }
    let replacement = if event.format != SubtitleFormat::Ass {
        let visible = timed_text::plain_visible(&event.raw_text)
            .ok_or_else(|| error("Malformed cue text"))?;
        let offset = unique_offset(&match_offsets(&visible, old_text))?;
        format!(
            "{}{}{}",
            &visible[..offset],
            new_text,
            &visible[offset + old_text.len()..]
        )
    } else {
        if old_text.contains(['{', '}']) || new_text.contains(['{', '}']) {
            return Err(error("Segment text cannot contain ASS tag markers"));
        }
        if is_per_character_effect(&event.raw_text) {
            let plan =
                GradientPlan::parse_with_defaults(&event.raw_text, event.style_defaults.as_deref())
                    .ok_or_else(|| error("Unsupported per-character effect"))?;
            let visible = plan.visible_text();
            let offset = unique_offset(&match_offsets(visible, old_text))?;
            format!(
                "{}{}{}",
                &visible[..offset],
                new_text,
                &visible[offset + old_text.len()..]
            )
        } else {
            let offset = unique_offset(&ass_segment_offsets(&event.raw_text, old_text)?)?;
            let candidate = format!(
                "{}{}{}",
                &event.raw_text[..offset],
                new_text,
                &event.raw_text[offset + old_text.len()..]
            );
            split_tags(&candidate)?.0
        }
    };
    replace_text(path, id, expected_revision, &replacement)
}

pub fn replace_text(
    path: &str,
    id: usize,
    expected_revision: &str,
    visible_text: &str,
) -> Result<ReplaceResult> {
    let batch = replace_text_batch(
        path,
        expected_revision,
        &[TextEdit {
            id,
            visible_text: visible_text.to_owned(),
        }],
    )?;
    let event = batch.events.into_iter().next().expect("one edit returned");
    Ok(ReplaceResult {
        previous_revision: batch.previous_revision,
        revision: batch.revision,
        event,
    })
}

pub fn replace_text_batch(
    path: &str,
    expected_revision: &str,
    edits: &[TextEdit],
) -> Result<BatchResult> {
    let _edit_guard = EDIT_LOCK
        .lock()
        .map_err(|_| error("Edit lock is unavailable"))?;
    if edits.is_empty() {
        return Err(error("Batch must contain at least one edit"));
    }
    let path = Path::new(path);
    let doc = Document::open(path)?;
    if doc.revision != expected_revision {
        return Err(error(
            "Revision conflict: file changed; fetch the event again",
        ));
    }
    let mut ids = HashSet::new();
    let mut replacements = Vec::with_capacity(edits.len());
    for edit in edits {
        if !ids.insert(edit.id) {
            return Err(error(format!("Duplicate event ID {}", edit.id)));
        }
        let event = doc
            .events
            .get(
                edit.id
                    .checked_sub(1)
                    .ok_or_else(|| error("ID must be >= 1"))?,
            )
            .ok_or_else(|| error(format!("Event {} not found", edit.id)))?;
        if event.format == SubtitleFormat::Ass && is_drawing(&event.raw_text) {
            return Err(error(format!("Event {} is a drawing", edit.id)));
        }
        let new_text = if event.format != SubtitleFormat::Ass {
            rebuild_plain(
                &event.raw_text,
                &doc.content,
                &edit.visible_text,
                event.format,
            )
        } else if is_per_character_effect(&event.raw_text) {
            let plan =
                GradientPlan::parse_with_defaults(&event.raw_text, event.style_defaults.as_deref())
                    .ok_or_else(|| error("Unsupported per-character effect"))?;
            validate_escapes(plan.visible_text(), &edit.visible_text)?;
            plan.rebuild(&edit.visible_text)
                .ok_or_else(|| error("Cannot rebuild per-character effect"))
        } else {
            rebuild(&event.raw_text, &edit.visible_text)
        }
        .map_err(|e| error(format!("Event {}: {e}", edit.id)))?;
        replacements.push((event.text_range.clone(), new_text));
    }
    replacements.sort_by_key(|(range, _)| range.start);
    let mut output = String::with_capacity(doc.content.len());
    let mut cursor = 0;
    for (range, text) in replacements {
        output.push_str(&doc.content[cursor..range.start]);
        output.push_str(&text);
        cursor = range.end;
    }
    output.push_str(&doc.content[cursor..]);

    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut temporary = tempfile::NamedTempFile::new_in(parent)
        .map_err(|e| error(format!("Cannot create temporary file: {e}")))?;
    temporary
        .as_file()
        .set_permissions(
            fs::metadata(path)
                .map_err(|e| error(e.to_string()))?
                .permissions(),
        )
        .map_err(|e| error(format!("Cannot preserve permissions: {e}")))?;
    temporary
        .write_all(output.as_bytes())
        .map_err(|e| error(format!("Write failed: {e}")))?;
    temporary
        .as_file()
        .sync_all()
        .map_err(|e| error(format!("Sync failed: {e}")))?;
    let current = fs::read(path).map_err(|e| error(format!("Cannot recheck file: {e}")))?;
    if hash(&current) != doc.revision {
        return Err(error("Revision conflict: file changed during edit"));
    }
    temporary
        .persist(path)
        .map_err(|e| error(format!("Replace failed: {e}")))?;
    let revision = hash(output.as_bytes());
    let updated = Document::open(path)?;
    let events = edits
        .iter()
        .map(|edit| {
            updated
                .events
                .get(edit.id - 1)
                .map(|event| view(event, false))
                .ok_or_else(|| error(format!("Updated event {} disappeared", edit.id)))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(BatchResult {
        previous_revision: doc.revision,
        revision,
        events,
    })
}
