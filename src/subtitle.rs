use serde::Serialize;
use std::{collections::BTreeMap, fmt, ops::Range, sync::Arc};

mod ass;
mod ass_creation;
mod ass_lines;
mod ass_mutation;
mod audit;
mod automation;
mod autosave;
mod backup;
mod context;
mod direct_events;
mod edit;
mod event_import;
mod font_collect;
mod group;
mod query;
mod timed_text;

pub use ass_creation::create_ass;
pub use ass_mutation::{
    AssEventPatch, delete_ass_events, move_ass_event, restore_ass_version, update_ass_events,
};
pub use audit::{AuditPolicy, audit_subtitle};
pub use backup::prepare_subtitle;
pub use direct_events::{NewAssEvent, create_ass_events};
pub use edit::{replace_segment, replace_text, replace_text_batch};
pub use event_import::insert_events_from_source;
pub use font_collect::collect_fonts;
pub use group::{list_text_groups, replace_text_group};
pub use query::{EventFilters, get_event_with_diagnostics, list_events_filtered};
#[cfg(test)]
pub use query::{get_event_with_context, list_events_with_context};

#[derive(Debug)]
pub struct SubtitleError(String);

impl fmt::Display for SubtitleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for SubtitleError {}

type Result<T> = std::result::Result<T, SubtitleError>;
fn error(message: impl Into<String>) -> SubtitleError {
    SubtitleError(message.into())
}

#[derive(Debug)]
struct Event {
    id: usize,
    format: SubtitleFormat,
    start: String,
    end: String,
    style: String,
    style_defaults: Option<Arc<BTreeMap<String, String>>>,
    raw_text: String,
    text_range: Range<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
enum SubtitleFormat {
    Ass,
    Srt,
    Vtt,
}

#[derive(Serialize)]
pub struct EventView {
    id: usize,
    format: SubtitleFormat,
    start: String,
    end: String,
    style: String,
    visible_text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    raw_text: Option<String>,
    editable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    read_only_reason: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    contexts: Option<BTreeMap<String, Vec<ContextMatch>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    context_diagnostics: Option<BTreeMap<String, ContextDiagnostic>>,
}

pub struct ContextTrack {
    pub name: String,
    pub path: String,
}

#[derive(Serialize)]
pub struct ContextMatch {
    source_id: usize,
    start: String,
    end: String,
    text: String,
    method: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    overlap_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    distance_ms: Option<u64>,
}

#[derive(Serialize)]
pub struct ContextDiagnostic {
    status: &'static str,
    candidate_ids: Vec<usize>,
    withheld_ids: Vec<usize>,
}

#[derive(Serialize)]
pub struct ListResult {
    revision: String,
    total_events: usize,
    matched_events: usize,
    offset: usize,
    events: Vec<EventView>,
}

#[derive(Serialize)]
pub struct TextGroupMember {
    id: usize,
    format: SubtitleFormat,
    start: String,
    end: String,
    style: String,
    editable: bool,
    automatic_apply: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    read_only_reason: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    group_apply_reason: Option<&'static str>,
}

#[derive(Serialize)]
pub struct TextGroupView {
    group_id: usize,
    source_text: String,
    occurrences: usize,
    automatic_apply: bool,
    members: Vec<TextGroupMember>,
}

#[derive(Serialize)]
pub struct TextGroupsResult {
    revision: String,
    total_events: usize,
    total_groups: usize,
    matched_groups: usize,
    offset: usize,
    groups: Vec<TextGroupView>,
}

#[derive(Serialize)]
pub struct GetResult {
    revision: String,
    event: EventView,
}

#[derive(Serialize)]
pub struct ReplaceResult {
    previous_revision: String,
    revision: String,
    autosave_path: String,
    event: EventView,
}

pub struct TextEdit {
    pub id: usize,
    pub visible_text: String,
}

#[derive(Serialize)]
pub struct BatchResult {
    previous_revision: String,
    revision: String,
    autosave_path: String,
    events: Vec<EventView>,
}

#[derive(Serialize)]
pub struct PreparedSubtitleResult {
    pub(crate) editable_path: String,
    pub(crate) backup_path: String,
    pub(crate) revision: String,
    pub(crate) total_events: usize,
}

pub struct AssTemplateOptions {
    pub title: String,
    pub play_res_x: u32,
    pub play_res_y: u32,
    pub font_name: String,
    pub font_size: Option<u32>,
}

#[derive(Serialize)]
pub struct AssCreationResult {
    pub(crate) working_path: String,
    pub(crate) revision: String,
    pub(crate) total_events: usize,
    pub(crate) play_res_x: u32,
    pub(crate) play_res_y: u32,
}

#[derive(Serialize)]
pub struct InsertEventsResult {
    previous_revision: String,
    revision: String,
    autosave_path: String,
    source_revision: String,
    events: Vec<EventView>,
}

#[derive(Serialize)]
pub struct CollectedFontFile {
    source_path: String,
    destination_path: String,
    already_present: bool,
}

#[derive(Serialize)]
pub struct FontCollectionResult {
    subtitle_path: String,
    destination_path: String,
    required_fonts: Vec<String>,
    files: Vec<CollectedFontFile>,
    warnings: Vec<FontWarning>,
}

#[derive(Serialize)]
pub struct FontWarning {
    kind: &'static str,
    font_name: String,
    message: String,
}

struct Document {
    content: String,
    events: Vec<Event>,
    revision: String,
}

#[cfg(test)]
mod tests;
