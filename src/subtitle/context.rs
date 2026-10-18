use super::ass::{is_drawing, strip_blocks};
use super::{
    ContextMatch, ContextTrack, Document, Event, Result, SubtitleFormat, error, timed_text,
};
use std::{
    collections::{BTreeMap, HashSet},
    path::Path,
};

struct ContextEvent {
    id: usize,
    start: String,
    end: String,
    start_ms: u64,
    end_ms: u64,
    text: String,
}

pub(super) struct LoadedTrack {
    name: String,
    events: Vec<ContextEvent>,
}

pub(super) fn parse_ass_time(value: &str) -> Option<u64> {
    let parts: Vec<&str> = value.split(':').collect();
    let (hours, minutes, seconds_part) = match parts.as_slice() {
        [minutes, seconds] => (0, *minutes, *seconds),
        [hours, minutes, seconds] => (hours.parse::<u64>().ok()?, *minutes, *seconds),
        _ => return None,
    };
    let minutes = minutes.parse::<u64>().ok()?;
    if minutes >= 60 {
        return None;
    }
    let (seconds, fraction) = seconds_part
        .split_once(['.', ','])
        .unwrap_or((seconds_part, ""));
    let seconds = seconds.parse::<u64>().ok()?;
    if seconds >= 60 || fraction.len() > 3 || !fraction.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let fractional_ms = match fraction.len() {
        0 => 0,
        1 => fraction.parse::<u64>().ok()?.checked_mul(100)?,
        2 => fraction.parse::<u64>().ok()?.checked_mul(10)?,
        _ => fraction.parse::<u64>().ok()?,
    };
    hours
        .checked_mul(3_600_000)?
        .checked_add(minutes.checked_mul(60_000)?)?
        .checked_add(seconds.checked_mul(1_000)?)?
        .checked_add(fractional_ms)
}

fn plain_context(event: &Event) -> Option<String> {
    if event.format == SubtitleFormat::Ass && is_drawing(&event.raw_text) {
        return None;
    }
    let text = if event.format == SubtitleFormat::Ass {
        strip_blocks(&event.raw_text)?
    } else {
        timed_text::plain_visible(&event.raw_text)?
    };
    let text = text
        .replace("\\N", " ")
        .replace("\\n", " ")
        .replace("\\h", " ");
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    (!normalized.is_empty()).then_some(normalized)
}

pub(super) fn load_contexts(tracks: &[ContextTrack]) -> Result<Vec<LoadedTrack>> {
    if tracks.len() > 8 {
        return Err(error("At most 8 context tracks are supported per request"));
    }
    let mut names = HashSet::new();
    let mut loaded = Vec::with_capacity(tracks.len());
    for track in tracks {
        if track.name.trim().is_empty() || !names.insert(track.name.clone()) {
            return Err(error(format!(
                "Context track name is empty or duplicated: {}",
                track.name
            )));
        }
        let document = Document::open(Path::new(&track.path))?;
        let mut events = document
            .events
            .iter()
            .filter_map(|event| {
                let start_ms = parse_ass_time(&event.start)?;
                let end_ms = parse_ass_time(&event.end)?;
                if end_ms <= start_ms {
                    return None;
                }
                Some(ContextEvent {
                    id: event.id,
                    start: event.start.clone(),
                    end: event.end.clone(),
                    start_ms,
                    end_ms,
                    text: plain_context(event)?,
                })
            })
            .collect::<Vec<_>>();
        events.sort_by_key(|event| (event.start_ms, event.end_ms, event.id));
        loaded.push(LoadedTrack {
            name: track.name.clone(),
            events,
        });
    }
    Ok(loaded)
}

fn context_match(
    event: &ContextEvent,
    method: &'static str,
    overlap_ms: Option<u64>,
    distance_ms: Option<u64>,
) -> ContextMatch {
    ContextMatch {
        source_id: event.id,
        start: event.start.clone(),
        end: event.end.clone(),
        text: event.text.clone(),
        method,
        overlap_ms,
        distance_ms,
    }
}

pub(super) fn align_tracks(
    master: &Event,
    tracks: &[LoadedTrack],
    proximity_ms: u32,
) -> BTreeMap<String, Vec<ContextMatch>> {
    let mut result = BTreeMap::new();
    let Some(master_start) = parse_ass_time(&master.start) else {
        return result;
    };
    let Some(master_end) = parse_ass_time(&master.end) else {
        return result;
    };
    if master_end <= master_start {
        return result;
    }
    let master_duration = master_end - master_start;
    for track in tracks {
        let mut accepted = Vec::new();
        let mut any_overlap = false;
        for event in &track.events {
            let overlap = master_end
                .min(event.end_ms)
                .saturating_sub(master_start.max(event.start_ms));
            if overlap == 0 {
                continue;
            }
            any_overlap = true;
            let shorter = master_duration.min(event.end_ms - event.start_ms);
            let exact = master_start == event.start_ms && master_end == event.end_ms;
            let enough = if shorter < 100 {
                overlap == shorter
            } else {
                overlap >= 100 && overlap.saturating_mul(2) >= shorter
            };
            if exact || enough {
                accepted.push(context_match(
                    event,
                    if exact { "exact" } else { "overlap" },
                    Some(overlap),
                    None,
                ));
            }
        }
        if accepted.is_empty() && !any_overlap && proximity_ms > 0 {
            let mut candidates = track
                .events
                .iter()
                .filter_map(|event| {
                    let distance = if event.end_ms <= master_start {
                        master_start - event.end_ms
                    } else {
                        event.start_ms.saturating_sub(master_end)
                    };
                    (distance <= u64::from(proximity_ms)).then_some((distance, event))
                })
                .collect::<Vec<_>>();
            candidates.sort_by_key(|(distance, event)| {
                (*distance, event.start_ms, event.end_ms, event.id)
            });
            if let Some((best, event)) = candidates.first() {
                let clear_winner = candidates
                    .get(1)
                    .is_none_or(|(second, _)| second.saturating_sub(*best) >= 100);
                if clear_winner {
                    accepted.push(context_match(event, "proximity", None, Some(*best)));
                }
            }
        }
        result.insert(track.name.clone(), accepted);
    }
    result
}
