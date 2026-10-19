use super::ass::{is_drawing, split_tags, strip_blocks};
use super::automation::{GradientPlan, is_per_character_effect};
use super::context::{align_tracks, diagnose_tracks, load_contexts, parse_ass_time};
use super::{
    Document, Event, EventView, GetResult, ListResult, Result, SubtitleFormat, error, timed_text,
};
use std::path::Path;

#[derive(Default)]
pub struct EventFilters {
    pub start_ms: Option<u64>,
    pub end_ms: Option<u64>,
    pub style: Option<String>,
    pub kind: Option<String>,
    pub layer: Option<String>,
    pub include_raw: bool,
    pub diagnose_context: bool,
    pub changed_since_path: Option<String>,
    pub changed_since_revision: Option<String>,
}

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
            context_diagnostics: None,
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
        context_diagnostics: None,
    }
}

#[cfg(test)]
pub fn list_events_with_context(
    path: &str,
    offset: usize,
    limit: usize,
    query: Option<&str>,
    contexts: &[super::ContextTrack],
    proximity_ms: u32,
) -> Result<ListResult> {
    list_events_filtered(
        path,
        offset,
        limit,
        query,
        contexts,
        proximity_ms,
        &EventFilters::default(),
    )
}

pub fn list_events_filtered(
    path: &str,
    offset: usize,
    limit: usize,
    query: Option<&str>,
    contexts: &[super::ContextTrack],
    proximity_ms: u32,
    filters: &EventFilters,
) -> Result<ListResult> {
    if filters
        .start_ms
        .zip(filters.end_ms)
        .is_some_and(|(start, end)| end <= start)
    {
        return Err(error("Time filter end must be later than start"));
    }
    if filters
        .kind
        .as_deref()
        .is_some_and(|kind| kind != "text" && kind != "drawing")
    {
        return Err(error("Event kind filter must be text or drawing"));
    }
    let doc = Document::open(Path::new(path))?;
    let total_events = doc.events.len();
    let query = query.map(str::to_lowercase);
    let changed = match (&filters.changed_since_path, &filters.changed_since_revision) {
        (None, None) => None,
        (Some(source), Some(revision)) => {
            let baseline = Document::open(Path::new(source))?;
            if &baseline.revision != revision || baseline.events.len() != doc.events.len() {
                return Err(error(
                    "Changed-since baseline revision or event count differs; use audit_subtitle for structural changes",
                ));
            }
            let changed = if path.to_ascii_lowercase().ends_with(".ass")
                && source.to_ascii_lowercase().ends_with(".ass")
            {
                let current_lines = super::ass_lines::dialogue_lines(&doc.content)?;
                let old_lines = super::ass_lines::dialogue_lines(&baseline.content)?;
                if super::audit::exact_reorder(&old_lines, &current_lines, &baseline, &doc) {
                    return Err(error(
                        "Changed-since does not track event identity after reordering; use audit_subtitle",
                    ));
                }
                current_lines
                    .iter()
                    .zip(&old_lines)
                    .map(|(new, old)| new.raw_line(&doc.content) != old.raw_line(&baseline.content))
                    .collect::<Vec<_>>()
            } else {
                doc.events
                    .iter()
                    .zip(&baseline.events)
                    .map(|(new, old)| {
                        new.start != old.start
                            || new.end != old.end
                            || new.style != old.style
                            || new.raw_text != old.raw_text
                    })
                    .collect::<Vec<_>>()
            };
            Some(changed)
        }
        _ => {
            return Err(error(
                "changed_since_path and changed_since_revision are required together",
            ));
        }
    };
    let ass_lines = if filters.layer.is_some() {
        Some(super::ass_lines::dialogue_lines(&doc.content)?)
    } else {
        None
    };
    if filters.layer.is_some() && !path.to_ascii_lowercase().ends_with(".ass") {
        return Err(error("Layer filter is available only for ASS"));
    }
    let matching: Vec<EventView> = doc
        .events
        .iter()
        .filter(|event| {
            let time_ms = if event.format == SubtitleFormat::Ass {
                parse_ass_time(&event.start).zip(parse_ass_time(&event.end))
            } else {
                timed_text::timestamp_ms(&event.start, event.format)
                    .zip(timed_text::timestamp_ms(&event.end, event.format))
            };
            let time_ok = if filters.start_ms.is_none() && filters.end_ms.is_none() {
                true
            } else {
                time_ms.is_some_and(|(start, end)| {
                    end > filters.start_ms.unwrap_or(0)
                        && start < filters.end_ms.unwrap_or(u64::MAX)
                })
            };
            let kind_ok = filters.kind.as_deref().is_none_or(|kind| {
                (kind == "drawing")
                    == (event.format == SubtitleFormat::Ass && is_drawing(&event.raw_text))
            });
            let style_ok = filters
                .style
                .as_deref()
                .is_none_or(|style| event.style.eq_ignore_ascii_case(style));
            let layer_ok = filters.layer.as_deref().is_none_or(|layer| {
                ass_lines
                    .as_ref()
                    .and_then(|lines| lines.get(event.id - 1))
                    .and_then(|line| line.value("Layer"))
                    == Some(layer)
            });
            let changed_ok = changed.as_ref().is_none_or(|items| items[event.id - 1]);
            time_ok && kind_ok && style_ok && layer_ok && changed_ok
        })
        .map(|event| view(event, filters.include_raw))
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
            if filters.diagnose_context {
                event.context_diagnostics = Some(diagnose_tracks(master, &tracks, proximity_ms));
            }
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

#[cfg(test)]
pub fn get_event_with_context(
    path: &str,
    id: usize,
    include_raw: bool,
    contexts: &[super::ContextTrack],
    proximity_ms: u32,
) -> Result<GetResult> {
    get_event_with_diagnostics(path, id, include_raw, contexts, proximity_ms, false)
}

pub fn get_event_with_diagnostics(
    path: &str,
    id: usize,
    include_raw: bool,
    contexts: &[super::ContextTrack],
    proximity_ms: u32,
    diagnose_context: bool,
) -> Result<GetResult> {
    let doc = Document::open(Path::new(path))?;
    let event = doc
        .events
        .get(id.checked_sub(1).ok_or_else(|| error("ID must be >= 1"))?)
        .ok_or_else(|| error(format!("Event {id} not found")))?;
    let mut event_view = view(event, include_raw);
    if !contexts.is_empty() {
        let tracks = load_contexts(contexts)?;
        event_view.contexts = Some(align_tracks(event, &tracks, proximity_ms));
        if diagnose_context {
            event_view.context_diagnostics = Some(diagnose_tracks(event, &tracks, proximity_ms));
        }
    }
    Ok(GetResult {
        revision: doc.revision,
        event: event_view,
    })
}
