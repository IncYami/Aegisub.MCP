use super::ass::{is_drawing, split_tags, strip_blocks};
use super::automation::{GradientPlan, is_per_character_effect};
use super::context::{align_tracks, load_contexts};
use super::{
    Document, Event, EventView, GetResult, ListResult, Result, SubtitleFormat, error, timed_text,
};
use std::path::Path;

pub(super) fn view(event: &Event, include_raw: bool) -> EventView {
    if event.format != SubtitleFormat::Ass {
        let editable = timed_text::editable(&event.raw_text);
        return EventView {
            id: event.id,
            format: event.format,
            start: event.start.clone(),
            end: event.end.clone(),
            style: event.style.clone(),
            visible_text: timed_text::plain_visible(&event.raw_text)
                .unwrap_or_else(|| event.raw_text.clone()),
            raw_text: include_raw.then(|| event.raw_text.clone()),
            editable,
            read_only_reason: (!editable).then_some("formatted_timed_text"),
            contexts: None,
        };
    }
    let parsed = split_tags(&event.raw_text);
    let drawing = is_drawing(&event.raw_text);
    let per_character = is_per_character_effect(&event.raw_text);
    let gradient = per_character
        .then(|| {
            GradientPlan::parse_with_defaults(&event.raw_text, event.style_defaults.as_deref())
        })
        .flatten();
    let editable = parsed.is_ok() && !drawing && (!per_character || gradient.is_some());
    let read_only_reason = if drawing {
        Some("drawing")
    } else if per_character && gradient.is_none() {
        Some("unsupported_per_character_effect")
    } else if parsed.is_err() {
        Some("malformed_override_block")
    } else {
        None
    };
    let visible_text = if drawing {
        String::new()
    } else if per_character {
        gradient
            .as_ref()
            .map(|plan| plan.visible_text().to_owned())
            .unwrap_or_else(|| strip_blocks(&event.raw_text).unwrap_or_default())
    } else {
        parsed.map(|(text, _)| text).unwrap_or_default()
    };
    EventView {
        id: event.id,
        format: event.format,
        start: event.start.clone(),
        end: event.end.clone(),
        style: event.style.clone(),
        visible_text,
        raw_text: include_raw.then(|| event.raw_text.clone()),
        editable,
        read_only_reason,
        contexts: None,
    }
}

pub fn list_events_with_context(
    path: &str,
    offset: usize,
    limit: usize,
    query: Option<&str>,
    contexts: &[super::ContextTrack],
    proximity_ms: u32,
) -> Result<ListResult> {
    let doc = Document::open(Path::new(path))?;
    let total_events = doc.events.len();
    let query = query.map(str::to_lowercase);
    let matching: Vec<EventView> = doc
        .events
        .iter()
        .map(|event| view(event, false))
        .filter(|event| {
            query
                .as_ref()
                .is_none_or(|q| event.visible_text.to_lowercase().contains(q))
        })
        .collect();
    let matched_events = matching.len();
    let mut events = matching
        .into_iter()
        .skip(offset)
        .take(limit)
        .collect::<Vec<_>>();
    if !contexts.is_empty() {
        let tracks = load_contexts(contexts)?;
        for event in &mut events {
            let master = &doc.events[event.id - 1];
            event.contexts = Some(align_tracks(master, &tracks, proximity_ms));
        }
    }
    Ok(ListResult {
        revision: doc.revision,
        total_events,
        matched_events,
        offset,
        events,
    })
}

pub fn get_event_with_context(
    path: &str,
    id: usize,
    include_raw: bool,
    contexts: &[super::ContextTrack],
    proximity_ms: u32,
) -> Result<GetResult> {
    let doc = Document::open(Path::new(path))?;
    let event = doc
        .events
        .get(id.checked_sub(1).ok_or_else(|| error("ID must be >= 1"))?)
        .ok_or_else(|| error(format!("Event {id} not found")))?;
    let mut event_view = view(event, include_raw);
    if !contexts.is_empty() {
        event_view.contexts = Some(align_tracks(event, &load_contexts(contexts)?, proximity_ms));
    }
    Ok(GetResult {
        revision: doc.revision,
        event: event_view,
    })
}
