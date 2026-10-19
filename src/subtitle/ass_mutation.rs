use super::ass_lines::{AssLine, dialogue_lines};
use super::autosave::{lock_for_edit, write_with_autosave};
use super::direct_events::{defined_styles, valid_text, validate_time};
use super::edit::EDIT_LOCK;
use super::query::view;
use super::{Document, EventView, Result, error};
use serde::Serialize;
use std::{
    collections::HashSet,
    fs,
    ops::Range,
    path::{Path, PathBuf},
};

pub struct AssEventPatch {
    pub id: usize,
    pub start: Option<String>,
    pub end: Option<String>,
    pub style: Option<String>,
    pub layer: Option<String>,
    pub name: Option<String>,
    pub margin_l: Option<String>,
    pub margin_r: Option<String>,
    pub margin_v: Option<String>,
    pub effect: Option<String>,
    pub raw_text: Option<String>,
    pub allow_escape_changes: bool,
}

impl AssEventPatch {
    fn values(&self) -> [(&'static str, Option<&str>); 10] {
        [
            ("Start", self.start.as_deref()),
            ("End", self.end.as_deref()),
            ("Style", self.style.as_deref()),
            ("Layer", self.layer.as_deref()),
            ("Name", self.name.as_deref()),
            ("MarginL", self.margin_l.as_deref()),
            ("MarginR", self.margin_r.as_deref()),
            ("MarginV", self.margin_v.as_deref()),
            ("Effect", self.effect.as_deref()),
            ("Text", self.raw_text.as_deref()),
        ]
    }
}

#[derive(Serialize)]
pub struct AssFieldChange {
    id: usize,
    field: &'static str,
    before: String,
    after: String,
}

#[derive(Serialize)]
pub struct AssUpdateResult {
    previous_revision: String,
    revision: String,
    autosave_path: String,
    changes: Vec<AssFieldChange>,
    events: Vec<EventView>,
}

#[derive(Serialize)]
pub struct AssDeleteResult {
    previous_revision: String,
    revision: String,
    autosave_path: String,
    removed_ids: Vec<usize>,
    remaining_events: usize,
}

#[derive(Serialize)]
pub struct AssMoveResult {
    previous_revision: String,
    revision: String,
    autosave_path: String,
    moved_old_id: usize,
    moved_new_id: usize,
    total_events: usize,
}

#[derive(Serialize)]
pub struct AssRestoreResult {
    previous_revision: String,
    revision: String,
    restored_from: String,
    autosave_path: String,
}

fn destination(path: &str) -> Result<PathBuf> {
    let path = fs::canonicalize(path).map_err(|e| error(format!("Cannot resolve ASS: {e}")))?;
    if path
        .components()
        .any(|part| part.as_os_str() == ".backup" || part.as_os_str() == ".autosave")
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
    Ok(path)
}

fn loaded(path: &Path, expected_revision: &str) -> Result<(Document, Vec<AssLine>)> {
    let doc = Document::open(path)?;
    if doc.revision != expected_revision {
        return Err(error("Revision conflict: subtitle changed; fetch it again"));
    }
    let lines = dialogue_lines(&doc.content)?;
    if lines.len() != doc.events.len() {
        return Err(error("ASS Dialogue parsing is inconsistent"));
    }
    Ok((doc, lines))
}

fn validate_field(name: &str, value: &str, styles: &HashSet<String>) -> Result<()> {
    if value.chars().any(char::is_control) || (name != "Text" && value.contains(',')) {
        return Err(error(format!(
            "ASS {name} contains a control character or comma"
        )));
    }
    match name {
        "Start" | "End" => {
            validate_time(value, name)?;
        }
        "Style" if !styles.contains(&value.to_ascii_lowercase()) => {
            return Err(error(format!("ASS style is not defined: {value}")));
        }
        "Layer"
            if !value
                .parse::<u32>()
                .is_ok_and(|layer| layer <= i32::MAX as u32) =>
        {
            return Err(error("ASS Layer must be an integer from 0 to 2147483647"));
        }
        "MarginL" | "MarginR" | "MarginV" if value.parse::<u32>().is_err() => {
            return Err(error(format!("ASS {name} must be a nonnegative integer")));
        }
        "Text" => valid_text(value)?,
        _ => {}
    }
    Ok(())
}

fn patched_value<'a>(line: &'a AssLine, patch: &'a AssEventPatch, name: &str) -> Result<&'a str> {
    let chosen = patch
        .values()
        .into_iter()
        .find(|(field, _)| *field == name)
        .and_then(|(_, value)| value);
    chosen
        .or_else(|| line.value(name))
        .ok_or_else(|| error(format!("ASS Events Format lacks {name}")))
}

fn render_changes(
    doc: &Document,
    lines: &[AssLine],
    patches: &[AssEventPatch],
) -> Result<(String, Vec<AssFieldChange>)> {
    let styles = defined_styles(&doc.content);
    let mut seen = HashSet::new();
    let mut edits = Vec::<(Range<usize>, String)>::new();
    let mut changes = Vec::new();
    for patch in patches {
        if !seen.insert(patch.id) {
            return Err(error(format!("Duplicate event ID {}", patch.id)));
        }
        let line = lines
            .get(
                patch
                    .id
                    .checked_sub(1)
                    .ok_or_else(|| error("ID must be >= 1"))?,
            )
            .ok_or_else(|| error(format!("Event {} not found", patch.id)))?;
        if patch.values().iter().all(|(_, value)| value.is_none()) {
            return Err(error(format!("Event {} has no changes", patch.id)));
        }
        for required in ["Start", "End", "Style", "Text"] {
            if line.index(required).is_none() {
                return Err(error(format!("ASS Events Format lacks {required}")));
            }
        }
        let start = if let Some(value) = patch.start.as_deref() {
            validate_time(value, "Start")?
        } else {
            super::context::parse_ass_time(patched_value(line, patch, "Start")?)
                .ok_or_else(|| error(format!("Event {} has an invalid existing start", patch.id)))?
        };
        let end = if let Some(value) = patch.end.as_deref() {
            validate_time(value, "End")?
        } else {
            super::context::parse_ass_time(patched_value(line, patch, "End")?)
                .ok_or_else(|| error(format!("Event {} has an invalid existing end", patch.id)))?
        };
        if end <= start {
            return Err(error(format!(
                "Event {} end must be later than start",
                patch.id
            )));
        }
        if let Some(new_text) = patch.raw_text.as_deref() {
            let old_text = line
                .value("Text")
                .ok_or_else(|| error("ASS Events Format lacks Text"))?;
            if !patch.allow_escape_changes
                && ["\\N", "\\n", "\\h"].iter().any(|escape| {
                    old_text.matches(escape).count() != new_text.matches(escape).count()
                })
            {
                return Err(error(format!(
                    "Event {} changes ASS escape counts; set allow_escape_changes to confirm",
                    patch.id
                )));
            }
        }
        for (name, supplied) in patch.values() {
            let Some(value) = supplied else { continue };
            let index = line
                .index(name)
                .ok_or_else(|| error(format!("ASS Events Format lacks {name}")))?;
            validate_field(name, value, &styles)?;
            let before = line.values[index].trim();
            if before == value {
                continue;
            }
            let old = &line.values[index];
            let leading = old.len() - old.trim_start().len();
            let trailing = old.len() - old.trim_end().len();
            let replacement = format!(
                "{}{}{}",
                &old[..leading],
                value,
                &old[old.len() - trailing..]
            );
            edits.push((line.value_ranges[index].clone(), replacement));
            changes.push(AssFieldChange {
                id: patch.id,
                field: name,
                before: before.to_owned(),
                after: value.to_owned(),
            });
        }
    }
    if changes.is_empty() {
        return Err(error("No event values changed"));
    }
    edits.sort_by_key(|(range, _)| range.start);
    let mut output = String::with_capacity(doc.content.len());
    let mut cursor = 0;
    for (range, replacement) in edits {
        output.push_str(&doc.content[cursor..range.start]);
        output.push_str(&replacement);
        cursor = range.end;
    }
    output.push_str(&doc.content[cursor..]);
    Ok((output, changes))
}

pub fn update_ass_events(
    path: &str,
    expected_revision: &str,
    patches: &[AssEventPatch],
) -> Result<AssUpdateResult> {
    if patches.is_empty() {
        return Err(error("At least one event patch is required"));
    }
    let path = destination(path)?;
    let _guard = EDIT_LOCK
        .lock()
        .map_err(|_| error("Edit lock is unavailable"))?;
    let _file_lock = lock_for_edit(&path)?;
    let (doc, lines) = loaded(&path, expected_revision)?;
    let (output, changes) = render_changes(&doc, &lines, patches)?;
    let autosave_path = write_with_autosave(&path, &doc.revision, &output)?;
    let updated = Document::open(&path)?;
    let events = patches
        .iter()
        .map(|patch| {
            updated
                .events
                .get(patch.id - 1)
                .map(|event| view(event, true))
                .ok_or_else(|| error(format!("Updated event {} is missing", patch.id)))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(AssUpdateResult {
        previous_revision: doc.revision,
        revision: super::ass::hash(output.as_bytes()),
        autosave_path: autosave_path.display().to_string(),
        changes,
        events,
    })
}

pub fn delete_ass_events(
    path: &str,
    expected_revision: &str,
    ids: &[usize],
) -> Result<AssDeleteResult> {
    if ids.is_empty() {
        return Err(error("At least one event ID is required"));
    }
    let path = destination(path)?;
    let _guard = EDIT_LOCK
        .lock()
        .map_err(|_| error("Edit lock is unavailable"))?;
    let _file_lock = lock_for_edit(&path)?;
    let (doc, lines) = loaded(&path, expected_revision)?;
    let mut unique = HashSet::new();
    for &id in ids {
        if !unique.insert(id) {
            return Err(error(format!("Duplicate event ID {id}")));
        }
        if id == 0 || id > lines.len() {
            return Err(error(format!("Event {id} not found")));
        }
    }
    let mut output = String::with_capacity(doc.content.len());
    let mut cursor = 0;
    for line in &lines {
        if unique.contains(&line.id) {
            output.push_str(&doc.content[cursor..line.full_range.start]);
            cursor = line.full_range.end;
        }
    }
    output.push_str(&doc.content[cursor..]);
    let autosave_path = write_with_autosave(&path, &doc.revision, &output)?;
    let mut removed_ids = ids.to_vec();
    removed_ids.sort_unstable();
    Ok(AssDeleteResult {
        previous_revision: doc.revision,
        revision: super::ass::hash(output.as_bytes()),
        autosave_path: autosave_path.display().to_string(),
        removed_ids,
        remaining_events: lines.len() - ids.len(),
    })
}

pub fn move_ass_event(
    path: &str,
    expected_revision: &str,
    id: usize,
    before_id: Option<usize>,
) -> Result<AssMoveResult> {
    let path = destination(path)?;
    let _guard = EDIT_LOCK
        .lock()
        .map_err(|_| error("Edit lock is unavailable"))?;
    let _file_lock = lock_for_edit(&path)?;
    let (doc, lines) = loaded(&path, expected_revision)?;
    if id == 0 || id > lines.len() {
        return Err(error(format!("Event {id} not found")));
    }
    if before_id.is_some_and(|target| target == 0 || target > lines.len()) {
        return Err(error("Move target event was not found"));
    }
    if before_id == Some(id) {
        return Err(error("An event cannot move before itself"));
    }
    let mut order = (1..=lines.len()).collect::<Vec<_>>();
    order.remove(id - 1);
    let target = before_id
        .map(|before| {
            order
                .iter()
                .position(|current| *current == before)
                .expect("target exists")
        })
        .unwrap_or(order.len());
    order.insert(target, id);
    let new_id = target + 1;
    if new_id == id {
        return Err(error("Event is already at the requested position"));
    }
    let low = id.min(new_id) - 1;
    let high = id.max(new_id);
    let expected_format = &lines[low].fields;
    if lines[low..high]
        .iter()
        .any(|line| &line.fields != expected_format)
    {
        return Err(error(
            "Cannot move across differing ASS Events Format lines",
        ));
    }
    let mut output = String::with_capacity(doc.content.len());
    let mut cursor = 0;
    for slot in low..high {
        let line = &lines[slot];
        output.push_str(&doc.content[cursor..line.line_range.start]);
        output.push_str(lines[order[slot] - 1].raw_line(&doc.content));
        cursor = line.line_range.end;
    }
    output.push_str(&doc.content[cursor..]);
    let autosave_path = write_with_autosave(&path, &doc.revision, &output)?;
    Ok(AssMoveResult {
        previous_revision: doc.revision,
        revision: super::ass::hash(output.as_bytes()),
        autosave_path: autosave_path.display().to_string(),
        moved_old_id: id,
        moved_new_id: new_id,
        total_events: lines.len(),
    })
}

pub fn restore_ass_version(
    path: &str,
    expected_revision: &str,
    version_path: &str,
) -> Result<AssRestoreResult> {
    let destination = destination(path)?;
    let source = fs::canonicalize(version_path)
        .map_err(|e| error(format!("Cannot resolve version: {e}")))?;
    let parent = destination
        .parent()
        .ok_or_else(|| error("Destination has no parent"))?;
    let source_parent = source
        .parent()
        .ok_or_else(|| error("Version has no parent"))?;
    if source_parent != parent.join(".backup") && source_parent != parent.join(".autosave") {
        return Err(error(
            "Restore source must be a sibling .backup or .autosave version",
        ));
    }
    let stem = destination
        .file_stem()
        .and_then(|stem| stem.to_str())
        .ok_or_else(|| error("Invalid subtitle filename"))?;
    let name = source
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| error("Invalid version filename"))?;
    if !name.starts_with(&format!("{stem}."))
        || !(name.ends_with(".BACKUP.ass") || name.ends_with(".AUTOSAVE.ass"))
    {
        return Err(error(
            "Restore source does not belong to the destination subtitle",
        ));
    }
    let _guard = EDIT_LOCK
        .lock()
        .map_err(|_| error("Edit lock is unavailable"))?;
    let _file_lock = lock_for_edit(&destination)?;
    let current = Document::open(&destination)?;
    if current.revision != expected_revision {
        return Err(error("Revision conflict: subtitle changed; fetch it again"));
    }
    let version = Document::open(&source)?;
    if current.revision == version.revision {
        return Err(error("Selected version is already active"));
    }
    let reread = fs::read(&source).map_err(|e| error(format!("Cannot recheck version: {e}")))?;
    if super::ass::hash(&reread) != version.revision {
        return Err(error("Restore source changed during operation"));
    }
    let autosave_path = write_with_autosave(&destination, &current.revision, &version.content)?;
    Ok(AssRestoreResult {
        previous_revision: current.revision,
        revision: version.revision,
        restored_from: source.display().to_string(),
        autosave_path: autosave_path.display().to_string(),
    })
}
