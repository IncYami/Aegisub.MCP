use super::ass::{is_drawing, split_tags, strip_blocks};
use super::automation::is_per_character_effect;
use super::query::view;
use super::{
    BatchResult, Document, Event, Result, SubtitleFormat, TextEdit, TextGroupMember, TextGroupView,
    TextGroupsResult, error, timed_text,
};
use std::{collections::BTreeMap, path::Path};

fn source_text(event: &Event) -> Option<String> {
    match event.format {
        SubtitleFormat::Ass if is_drawing(&event.raw_text) => None,
        SubtitleFormat::Ass => strip_blocks(&event.raw_text),
        SubtitleFormat::Srt | SubtitleFormat::Vtt => timed_text::plain_visible(&event.raw_text),
    }
    .filter(|text| !text.is_empty())
}

fn automatic_template(event: &Event, translation: &str) -> Result<String> {
    if !view(event, false).editable {
        return Err(error("Event is read-only"));
    }
    if event.format != SubtitleFormat::Ass || is_per_character_effect(&event.raw_text) {
        return Ok(translation.to_owned());
    }
    let (visible, tags) = split_tags(&event.raw_text)?;
    let plain =
        strip_blocks(&event.raw_text).ok_or_else(|| error("Malformed ASS override block"))?;
    let markers = (1..=tags.len())
        .map(|index| format!("{{{index}}}"))
        .collect::<String>();
    if visible != format!("{plain}{markers}") {
        return Err(error(
            "Internal ASS tags need event-specific placement in the translation",
        ));
    }
    Ok(format!("{translation}{markers}"))
}

fn group_member(event: &Event) -> TextGroupMember {
    let event_view = view(event, false);
    let group_apply_reason = if !event_view.editable {
        event_view.read_only_reason
    } else if automatic_template(event, "example").is_err() {
        Some("internal_override_tags")
    } else {
        None
    };
    TextGroupMember {
        id: event.id,
        format: event.format,
        start: event.start.clone(),
        end: event.end.clone(),
        style: event.style.clone(),
        editable: event_view.editable,
        automatic_apply: group_apply_reason.is_none(),
        read_only_reason: event_view.read_only_reason,
        group_apply_reason,
    }
}

pub fn list_text_groups(
    path: &str,
    offset: usize,
    limit: usize,
    query: Option<&str>,
) -> Result<TextGroupsResult> {
    let doc = Document::open(Path::new(path))?;
    let mut index = BTreeMap::<String, usize>::new();
    let mut groups = Vec::<TextGroupView>::new();
    for event in &doc.events {
        let Some(text) = source_text(event) else {
            continue;
        };
        let member = group_member(event);
        if let Some(position) = index.get(&text).copied() {
            let group = &mut groups[position];
            group.automatic_apply &= member.automatic_apply;
            group.occurrences += 1;
            group.members.push(member);
        } else {
            index.insert(text.clone(), groups.len());
            groups.push(TextGroupView {
                group_id: event.id,
                source_text: text,
                occurrences: 1,
                automatic_apply: member.automatic_apply,
                members: vec![member],
            });
        }
    }
    let total_groups = groups.len();
    let query = query.map(str::to_lowercase);
    let matching = groups
        .into_iter()
        .filter(|group| {
            query
                .as_ref()
                .is_none_or(|query| group.source_text.to_lowercase().contains(query))
        })
        .collect::<Vec<_>>();
    let matched_groups = matching.len();
    Ok(TextGroupsResult {
        revision: doc.revision,
        total_events: doc.events.len(),
        total_groups,
        matched_groups,
        offset,
        groups: matching.into_iter().skip(offset).take(limit).collect(),
    })
}

pub fn replace_text_group(
    path: &str,
    group_id: usize,
    expected_revision: &str,
    source: &str,
    translation: &str,
) -> Result<BatchResult> {
    if source.is_empty() || translation.is_empty() {
        return Err(error("Group source and translation must be nonempty"));
    }
    let doc = Document::open(Path::new(path))?;
    if doc.revision != expected_revision {
        return Err(error(
            "Revision conflict: file changed; list text groups again",
        ));
    }
    let group_event = doc
        .events
        .get(
            group_id
                .checked_sub(1)
                .ok_or_else(|| error("Group ID must be >= 1"))?,
        )
        .ok_or_else(|| error(format!("Group event {group_id} not found")))?;
    if source_text(group_event).as_deref() != Some(source) {
        return Err(error(
            "Group source text no longer matches its representative event",
        ));
    }
    let mut edits = Vec::new();
    for event in &doc.events {
        if source_text(event).as_deref() == Some(source) {
            let visible_text = automatic_template(event, translation)
                .map_err(|e| error(format!("Event {}: {e}", event.id)))?;
            edits.push(TextEdit {
                id: event.id,
                visible_text,
            });
        }
    }
    super::edit::replace_text_batch(path, expected_revision, &edits)
}
