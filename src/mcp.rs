use crate::subtitle;
use rmcp::{
    ServerHandler, handler::server::wrapper::Parameters, model::CallToolResult, schemars, tool,
    tool_handler, tool_router,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
};

#[derive(Clone, Default)]
pub(crate) struct SubtitleServer {
    editable_files: Arc<Mutex<HashSet<PathBuf>>>,
}

fn tool_error(message: impl Into<String>) -> CallToolResult {
    CallToolResult::structured_error(serde_json::json!({ "error": message.into() }))
}

impl SubtitleServer {
    fn register_editable_file(&self, path: &str) -> Result<(), String> {
        let path =
            fs::canonicalize(path).map_err(|e| format!("Cannot resolve editable subtitle: {e}"))?;
        if path.components().any(|component| {
            component.as_os_str() == ".backup" || component.as_os_str() == ".autosave"
        }) {
            return Err("Files inside .backup or .autosave cannot be edited".to_owned());
        }
        self.editable_files
            .lock()
            .map_err(|_| "Editable file registry is unavailable".to_owned())?
            .insert(path);
        Ok(())
    }

    fn require_editable_file(&self, path: &str) -> Result<(), String> {
        let canonical =
            fs::canonicalize(path).map_err(|e| format!("Cannot resolve editable file: {e}"))?;
        if canonical.components().any(|component| {
            component.as_os_str() == ".backup" || component.as_os_str() == ".autosave"
        }) {
            return Err("Files inside .backup or .autosave cannot be edited".to_owned());
        }
        let registered = self
            .editable_files
            .lock()
            .map_err(|_| "Editable file registry is unavailable".to_owned())?
            .contains(&canonical);
        if registered {
            Ok(())
        } else {
            Err("Edits require a subtitle prepared by prepare_subtitle or created by create_ass in this server session".to_owned())
        }
    }
}

fn tool_result<T: Serialize>(result: Result<T, subtitle::SubtitleError>) -> CallToolResult {
    match result {
        Ok(value) => CallToolResult::structured(
            serde_json::to_value(value).expect("serializable tool response"),
        ),
        Err(error) => {
            CallToolResult::structured_error(serde_json::json!({ "error": error.to_string() }))
        }
    }
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ListArgs {
    path: String,
    #[serde(default)]
    offset: usize,
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    contexts: Vec<ContextArgs>,
    #[serde(default)]
    proximity_ms: Option<u32>,
    #[serde(default)]
    start_ms: Option<u64>,
    #[serde(default)]
    end_ms: Option<u64>,
    #[serde(default)]
    style: Option<String>,
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    layer: Option<String>,
    #[serde(default)]
    include_raw: bool,
    #[serde(default)]
    diagnose_context: bool,
    #[serde(default)]
    changed_since_path: Option<String>,
    #[serde(default)]
    changed_since_revision: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ListGroupsArgs {
    path: String,
    #[serde(default)]
    offset: usize,
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    query: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct GetArgs {
    path: String,
    id: usize,
    #[serde(default)]
    include_raw: bool,
    #[serde(default)]
    contexts: Vec<ContextArgs>,
    #[serde(default)]
    proximity_ms: Option<u32>,
    #[serde(default)]
    diagnose_context: bool,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ContextArgs {
    name: String,
    path: String,
}

fn context_tracks(args: Vec<ContextArgs>) -> Vec<subtitle::ContextTrack> {
    args.into_iter()
        .map(|context| subtitle::ContextTrack {
            name: context.name,
            path: context.path,
        })
        .collect()
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ReplaceArgs {
    path: String,
    id: usize,
    expected_revision: String,
    visible_text: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ReplaceGroupArgs {
    path: String,
    group_id: usize,
    expected_revision: String,
    source_text: String,
    translated_text: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct PrepareSubtitleArgs {
    source_path: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct CreateAssArgs {
    destination_path: String,
    #[serde(default = "default_title")]
    title: String,
    #[serde(default = "default_play_res_x")]
    play_res_x: u32,
    #[serde(default = "default_play_res_y")]
    play_res_y: u32,
    #[serde(default = "default_font_name")]
    font_name: String,
    #[serde(default)]
    font_size: Option<u32>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct InsertEventsArgs {
    destination_path: String,
    expected_revision: String,
    source_path: String,
    source_ids: Vec<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct CreateAssEventArgs {
    start: String,
    end: String,
    text: String,
    #[serde(default)]
    style: Option<String>,
    #[serde(default)]
    layer: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    margin_l: Option<String>,
    #[serde(default)]
    margin_r: Option<String>,
    #[serde(default)]
    margin_v: Option<String>,
    #[serde(default)]
    effect: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct CreateAssEventsArgs {
    destination_path: String,
    expected_revision: String,
    events: Vec<CreateAssEventArgs>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AssEventPatchArgs {
    id: usize,
    #[serde(default)]
    start: Option<String>,
    #[serde(default)]
    end: Option<String>,
    #[serde(default)]
    style: Option<String>,
    #[serde(default)]
    layer: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    margin_l: Option<String>,
    #[serde(default)]
    margin_r: Option<String>,
    #[serde(default)]
    margin_v: Option<String>,
    #[serde(default)]
    effect: Option<String>,
    #[serde(default)]
    raw_text: Option<String>,
    #[serde(default)]
    allow_escape_changes: bool,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct UpdateAssEventsArgs {
    path: String,
    expected_revision: String,
    patches: Vec<AssEventPatchArgs>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct DeleteAssEventsArgs {
    path: String,
    expected_revision: String,
    ids: Vec<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct MoveAssEventArgs {
    path: String,
    expected_revision: String,
    id: usize,
    #[serde(default)]
    before_id: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct RestoreAssVersionArgs {
    path: String,
    expected_revision: String,
    version_path: String,
}

fn default_audit_fields() -> Vec<String> {
    vec!["Text".to_owned()]
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AuditPolicyArgs {
    #[serde(default = "default_audit_fields")]
    allowed_fields: Vec<String>,
    #[serde(default)]
    allowed_event_ids: Vec<usize>,
    #[serde(default)]
    allow_non_dialogue_changes: bool,
    #[serde(default)]
    allow_event_count_changes: bool,
    #[serde(default)]
    allow_reordering: bool,
    #[serde(default)]
    allow_gradient_rebuild: bool,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct AuditArgs {
    baseline_path: String,
    edited_path: String,
    expected_baseline_revision: String,
    expected_edited_revision: String,
    #[serde(default)]
    policy: Option<AuditPolicyArgs>,
    #[serde(default)]
    offset: usize,
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct CollectFontsArgs {
    ass_path: String,
    #[serde(default)]
    destination_path: Option<String>,
}

fn default_title() -> String {
    "Untitled".to_owned()
}

fn default_play_res_x() -> u32 {
    1920
}
fn default_play_res_y() -> u32 {
    1080
}
fn default_font_name() -> String {
    "Lato".to_owned()
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct SegmentArgs {
    path: String,
    id: usize,
    expected_revision: String,
    old_text: String,
    new_text: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct BatchEditArgs {
    id: usize,
    visible_text: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct BatchArgs {
    path: String,
    expected_revision: String,
    edits: Vec<BatchEditArgs>,
}

#[tool_router]
impl SubtitleServer {
    #[tool(
        description = "Read the project's ASS subtitle style guide. Use it before choosing fonts or typesetting tags. It explains the Lato Default style for new scripts, preservation of existing master styles, Google Fonts suggestions, and local override tags. Read-only; no subtitle file is changed."
    )]
    fn get_style_guide(&self) -> CallToolResult {
        CallToolResult::structured(serde_json::json!({
            "guide": include_str!("../docs/subtitle-style-guide.md")
        }))
    }

    #[tool(
        description = "Inspect fonts used by an ASS subtitle and copy the matching installed font files without changing their contents. Reads style fonts and inline font overrides, checks used glyphs against fontconfig character coverage, and warns when a font or requested bold/italic face is missing. Copies original TTF, OTF, TTC, or OTC files once each with organized filenames; variable fonts and collections remain intact. Defaults to a fonts directory beside ass_path; destination_path may select another directory. Never edits the subtitle or overwrites existing font files. Returns copied files and structured warnings."
    )]
    fn collect_fonts(&self, Parameters(args): Parameters<CollectFontsArgs>) -> CallToolResult {
        tool_result(subtitle::collect_fonts(
            &args.ass_path,
            args.destination_path.as_deref(),
        ))
    }

    #[tool(
        description = "Back up an existing ASS, SSA, SRT or WebVTT subtitle to a timestamped .BACKUP file in a sibling .backup directory, then authorize edits to the original file in this server session. Edits save the displaced version in .autosave with an .AUTOSAVE suffix. The backup preserves the original bytes and is never edited by MCP tools. Returns editable_path, backup_path and the current revision."
    )]
    fn prepare_subtitle(
        &self,
        Parameters(args): Parameters<PrepareSubtitleArgs>,
    ) -> CallToolResult {
        match subtitle::prepare_subtitle(&args.source_path) {
            Ok(prepared) => {
                if let Err(message) = self.register_editable_file(&prepared.editable_path) {
                    return tool_error(message);
                }
                tool_result(Ok(prepared))
            }
            Err(error) => tool_result::<subtitle::PreparedSubtitleResult>(Err(error)),
        }
    }

    #[tool(
        description = "Create a new empty ASS subtitle with a complete base script, one Default style and an Events format. Requires a destination_path ending in .ass; existing files are never overwritten. Optional title, play_res_x, play_res_y, font_name and font_size customize the base. At 1920x1080 the default style uses Lato 80, bold, near-white text, 3.75 outline, 2.25 shadow, and 120/120/60 margins. Style dimensions scale with script resolution. The returned working_path is registered for edits in this server session."
    )]
    fn create_ass(&self, Parameters(args): Parameters<CreateAssArgs>) -> CallToolResult {
        if std::path::Path::new(&args.destination_path)
            .components()
            .any(|component| {
                component.as_os_str() == ".backup" || component.as_os_str() == ".autosave"
            })
        {
            return tool_error("Cannot create an editable subtitle inside .backup or .autosave");
        }
        let options = subtitle::AssTemplateOptions {
            title: args.title,
            play_res_x: args.play_res_x,
            play_res_y: args.play_res_y,
            font_name: args.font_name,
            font_size: args.font_size,
        };
        match subtitle::create_ass(&args.destination_path, &options) {
            Ok(created) => {
                if let Err(message) = self.register_editable_file(&created.working_path) {
                    return tool_error(message);
                }
                tool_result(Ok(created))
            }
            Err(error) => tool_result::<subtitle::AssCreationResult>(Err(error)),
        }
    }

    #[tool(
        description = "Insert one or more source cues into an editable ASS subtitle prepared by prepare_subtitle or created by create_ass. Pass source_ids such as [1] or [1,2,3], source_path in ASS, SRT or WebVTT, destination_path, and expected_revision. Copies timing and text into new Default-style Dialogue events in the requested order. The imported source is never edited. Plain SRT/WebVTT cues only; ASS override text is retained. The batch is atomic and returns the new revision and autosave_path for the displaced subtitle."
    )]
    fn insert_events_from_source(
        &self,
        Parameters(args): Parameters<InsertEventsArgs>,
    ) -> CallToolResult {
        if let Err(message) = self.require_editable_file(&args.destination_path) {
            return tool_error(message);
        }
        tool_result(subtitle::insert_events_from_source(
            &args.destination_path,
            &args.expected_revision,
            &args.source_path,
            &args.source_ids,
        ))
    }

    #[tool(
        description = "Create one or more Dialogue events directly in an editable ASS subtitle prepared by prepare_subtitle or created by create_ass. Pass destination_path, expected_revision, and an events array with start and end in h:mm:ss.cc format plus raw ASS text. Style defaults to Default; layer, name, margins, and effect are optional. Text may contain commas and valid ASS override tags, including unknown tags, but must remain on one physical line. All events are validated and written atomically using the destination Events Format field order. Returns created events, the new revision, and autosave_path."
    )]
    fn create_ass_events(
        &self,
        Parameters(args): Parameters<CreateAssEventsArgs>,
    ) -> CallToolResult {
        if let Err(message) = self.require_editable_file(&args.destination_path) {
            return tool_error(message);
        }
        let events = args
            .events
            .into_iter()
            .map(|event| subtitle::NewAssEvent {
                start: event.start,
                end: event.end,
                text: event.text,
                style: event.style,
                layer: event.layer,
                name: event.name,
                margin_l: event.margin_l,
                margin_r: event.margin_r,
                margin_v: event.margin_v,
                effect: event.effect,
            })
            .collect::<Vec<_>>();
        tool_result(subtitle::create_ass_events(
            &args.destination_path,
            &args.expected_revision,
            &events,
        ))
    }

    #[tool(
        description = "Update fields or raw ASS Text in existing Dialogue events. Only registered .ass files are editable. Pass patches with IDs and selected fields; raw_text replaces the complete Text field, including override tags. ASS escape counts are preserved unless allow_escape_changes is explicitly true. Validates timing, style, fields, override-block structure, and supported positional tags before one atomic write. Requires expected_revision and returns field diffs, new revision, and autosave_path."
    )]
    fn update_ass_events(
        &self,
        Parameters(args): Parameters<UpdateAssEventsArgs>,
    ) -> CallToolResult {
        if let Err(message) = self.require_editable_file(&args.path) {
            return tool_error(message);
        }
        let patches = args
            .patches
            .into_iter()
            .map(|patch| subtitle::AssEventPatch {
                id: patch.id,
                start: patch.start,
                end: patch.end,
                style: patch.style,
                layer: patch.layer,
                name: patch.name,
                margin_l: patch.margin_l,
                margin_r: patch.margin_r,
                margin_v: patch.margin_v,
                effect: patch.effect,
                raw_text: patch.raw_text,
                allow_escape_changes: patch.allow_escape_changes,
            })
            .collect::<Vec<_>>();
        tool_result(subtitle::update_ass_events(
            &args.path,
            &args.expected_revision,
            &patches,
        ))
    }

    #[tool(
        description = "Delete selected Dialogue events from a registered .ass file in one atomic write. Requires current expected_revision and one or more unique IDs. Preserves all remaining bytes, creates an autosave, and returns removed IDs. Event IDs after a deletion are reassigned by file order; fetch a new revision and IDs before further edits."
    )]
    fn delete_ass_events(
        &self,
        Parameters(args): Parameters<DeleteAssEventsArgs>,
    ) -> CallToolResult {
        if let Err(message) = self.require_editable_file(&args.path) {
            return tool_error(message);
        }
        tool_result(subtitle::delete_ass_events(
            &args.path,
            &args.expected_revision,
            &args.ids,
        ))
    }

    #[tool(
        description = "Move one Dialogue event before another Dialogue ID in a registered .ass file; omit before_id to move to the end. Preserves non-Dialogue lines in place and rejects moves across differing Events Format definitions. Requires current expected_revision, creates an autosave, and returns the moved event's new position. IDs change with order; fetch them again afterward."
    )]
    fn move_ass_event(&self, Parameters(args): Parameters<MoveAssEventArgs>) -> CallToolResult {
        if let Err(message) = self.require_editable_file(&args.path) {
            return tool_error(message);
        }
        tool_result(subtitle::move_ass_event(
            &args.path,
            &args.expected_revision,
            args.id,
            args.before_id,
        ))
    }

    #[tool(
        description = "Restore a registered .ass subtitle from its own sibling .backup or .autosave version. Requires current expected_revision and version_path. The currently active file is preserved as a new autosave before restoring the selected version. This restores the entire file; use update_ass_events to undo only selected event fields."
    )]
    fn restore_ass_version(
        &self,
        Parameters(args): Parameters<RestoreAssVersionArgs>,
    ) -> CallToolResult {
        if let Err(message) = self.require_editable_file(&args.path) {
            return tool_error(message);
        }
        tool_result(subtitle::restore_ass_version(
            &args.path,
            &args.expected_revision,
            &args.version_path,
        ))
    }

    #[tool(
        description = "Compare two exact ASS revisions without editing either file. Supply baseline_path and edited_path with both expected revision hashes. Reports changed event fields and tags, unchanged Text IDs, additions/removals, order and other-section changes, plus violations of an optional policy. Default policy allows Text edits only. Results are paginated; a report applies only to its returned revisions."
    )]
    fn audit_subtitle(&self, Parameters(args): Parameters<AuditArgs>) -> CallToolResult {
        let policy = args.policy.unwrap_or(AuditPolicyArgs {
            allowed_fields: default_audit_fields(),
            allowed_event_ids: Vec::new(),
            allow_non_dialogue_changes: false,
            allow_event_count_changes: false,
            allow_reordering: false,
            allow_gradient_rebuild: false,
        });
        tool_result(subtitle::audit_subtitle(
            &args.baseline_path,
            &args.edited_path,
            &args.expected_baseline_revision,
            &args.expected_edited_revision,
            &subtitle::AuditPolicy {
                allowed_fields: policy.allowed_fields,
                allowed_event_ids: policy.allowed_event_ids,
                allow_non_dialogue_changes: policy.allow_non_dialogue_changes,
                allow_event_count_changes: policy.allow_event_count_changes,
                allow_reordering: policy.allow_reordering,
                allow_gradient_rebuild: policy.allow_gradient_rebuild,
            },
            args.offset,
            args.limit.unwrap_or(50).min(200),
        ))
    }

    #[tool(
        description = "List ASS Dialogue events or SRT/WebVTT cues. Paginate with offset and limit (max 200); filter by visible-text query, overlapping [start_ms,end_ms), style, ASS layer, or kind=text|drawing. include_raw returns ASS Text fields in a window. changed_since_path and changed_since_revision compare event positions when counts are unchanged; exact reorders are rejected. Use audit_subtitle for structural changes. Optional context tracks align by time; diagnose_context adds candidates withheld by ambiguous or insufficient matching. Returns current revision."
    )]
    fn list_events(&self, Parameters(args): Parameters<ListArgs>) -> CallToolResult {
        let contexts = context_tracks(args.contexts);
        tool_result(subtitle::list_events_filtered(
            &args.path,
            args.offset,
            args.limit.unwrap_or(50).min(200),
            args.query.as_deref(),
            &contexts,
            args.proximity_ms.unwrap_or(500),
            &subtitle::EventFilters {
                start_ms: args.start_ms,
                end_ms: args.end_ms,
                style: args.style,
                kind: args.kind,
                layer: args.layer,
                include_raw: args.include_raw,
                diagnose_context: args.diagnose_context,
                changed_since_path: args.changed_since_path,
                changed_since_revision: args.changed_since_revision,
            },
        ))
    }

    #[tool(
        description = "List subtitle text grouped by exact visible text across the entire ASS, SRT, or WebVTT file. Grouping ignores ASS override tags and does not depend on style, timing, or adjacency. Returns one group per distinct text, including single occurrences, with member IDs, timing, styles, revision, and whether one translation can be applied automatically to every occurrence. Internal ASS tags that need event-specific placement are flagged. Read-only."
    )]
    fn list_text_groups(&self, Parameters(args): Parameters<ListGroupsArgs>) -> CallToolResult {
        tool_result(subtitle::list_text_groups(
            &args.path,
            args.offset,
            args.limit.unwrap_or(50).min(200),
            args.query.as_deref(),
        ))
    }

    #[tool(
        description = "Get one ASS Dialogue event or SRT/WebVTT cue by its one-based ID. ASS tags are hidden by default; set include_raw to inspect source text. Optional context tracks align by time; diagnose_context also reports rejected or ambiguous candidates. Returns the file revision required for editing."
    )]
    fn get_event(&self, Parameters(args): Parameters<GetArgs>) -> CallToolResult {
        let contexts = context_tracks(args.contexts);
        tool_result(subtitle::get_event_with_diagnostics(
            &args.path,
            args.id,
            args.include_raw,
            &contexts,
            args.proximity_ms.unwrap_or(500),
            args.diagnose_context,
        ))
    }

    #[tool(
        description = "Replace visible text in an editable subtitle prepared by prepare_subtitle or created by create_ass. Requires the revision from get_event/list_events; fails if the file changed. For ASS, keep every {1}, {2}, etc. tag marker exactly once and in order. Preserves the rest of the file and returns autosave_path for the displaced version."
    )]
    fn replace_text(&self, Parameters(args): Parameters<ReplaceArgs>) -> CallToolResult {
        if let Err(message) = self.require_editable_file(&args.path) {
            return tool_error(message);
        }
        tool_result(subtitle::replace_text(
            &args.path,
            args.id,
            &args.expected_revision,
            &args.visible_text,
        ))
    }

    #[tool(
        description = "Apply one translation to every occurrence of an exact visible-text group in a registered editable subtitle. Use group_id, source_text, and expected_revision from list_text_groups. Includes dialogue and signs regardless of style or distance in the file. Validates every event before one atomic write; rejects groups containing read-only events or internal ASS tags whose translated placement is ambiguous. Preserves leading and trailing ASS tags, rebuilds supported per-character gradients, and returns autosave_path."
    )]
    fn replace_text_group(&self, Parameters(args): Parameters<ReplaceGroupArgs>) -> CallToolResult {
        if let Err(message) = self.require_editable_file(&args.path) {
            return tool_error(message);
        }
        tool_result(subtitle::replace_text_group(
            &args.path,
            args.group_id,
            &args.expected_revision,
            &args.source_text,
            &args.translated_text,
        ))
    }

    #[tool(
        description = "Replace one unique text segment in an editable subtitle prepared by prepare_subtitle or created by create_ass. Requires the current file revision. The search must occur exactly once; in ASS it cannot cross override tags. Returns autosave_path for the displaced version."
    )]
    fn replace_segment(&self, Parameters(args): Parameters<SegmentArgs>) -> CallToolResult {
        if let Err(message) = self.require_editable_file(&args.path) {
            return tool_error(message);
        }
        tool_result(subtitle::replace_segment(
            &args.path,
            args.id,
            &args.expected_revision,
            &args.old_text,
            &args.new_text,
        ))
    }

    #[tool(
        description = "Atomically replace visible text in multiple events or cues of one editable subtitle prepared by prepare_subtitle or created by create_ass. Every edit must pass validation; duplicate IDs, stale revisions and invalid tag markers reject the entire batch without writing. Returns autosave_path for the displaced version."
    )]
    fn replace_text_batch(&self, Parameters(args): Parameters<BatchArgs>) -> CallToolResult {
        if let Err(message) = self.require_editable_file(&args.path) {
            return tool_error(message);
        }
        let edits = args
            .edits
            .into_iter()
            .map(|edit| subtitle::TextEdit {
                id: edit.id,
                visible_text: edit.visible_text,
            })
            .collect::<Vec<_>>();
        tool_result(subtitle::replace_text_batch(
            &args.path,
            &args.expected_revision,
            &edits,
        ))
    }
}

#[tool_handler]
impl ServerHandler for SubtitleServer {
    fn get_info(&self) -> rmcp::model::ServerConfig {
        rmcp::model::ServerConfig::new(rmcp::model::ServerCapabilities::builder().enable_tools().build())
            .with_server_info(rmcp::model::Implementation::new(
                env!("CARGO_PKG_NAME"),
                env!("CARGO_PKG_VERSION"),
            ))
            .with_instructions("Before editing an existing ASS, SRT, or WebVTT subtitle, call prepare_subtitle; it backs up and registers the original. Fetch the current revision before every edit. New ASS files start with create_ass, then create_ass_events or insert_events_from_source. Preserve existing master styles, layers, timing, and tags unless an edit explicitly requests changes. New scripts use Default with local override tags. Each edit saves an autosave. Use audit_subtitle against a known baseline before delivery. Read get_style_guide for font and typesetting guidance. Context matches are timing candidates; diagnose_context exposes ambiguity. collect_fonts reports installed font coverage.")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_identity_matches_package_metadata() {
        let info = SubtitleServer::default().get_info();
        assert_eq!(info.server_info.name, env!("CARGO_PKG_NAME"));
        assert_eq!(info.server_info.version, env!("CARGO_PKG_VERSION"));
        assert!(info.capabilities.tools.is_some());
        assert!(info.instructions.is_some());
    }

    #[test]
    fn style_guide_tool_exposes_project_convention() {
        let result = SubtitleServer::default().get_style_guide();
        assert_ne!(result.is_error, Some(true));
        let guide = result.structured_content.unwrap()["guide"]
            .as_str()
            .unwrap()
            .to_owned();
        assert!(guide.contains("For a new script created with `create_ass`"));
        assert!(guide.contains("When editing an existing master"));
        assert!(guide.contains("Permanent Marker"));
        assert!(guide.contains("Tinos"));
        assert!(guide.contains("Use `update_ass_events`"));
    }

    #[test]
    fn mcp_edits_require_a_backup_before_editing_original() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("source.ass");
        let original = "[Events]\nFormat: Start, End, Style, Text\nDialogue: 0:00:01.00,0:00:02.00,Default,Original\n";
        fs::write(&source, original).unwrap();
        let source_path = source.to_str().unwrap().to_owned();
        let revision = serde_json::to_value(
            subtitle::get_event_with_context(&source_path, 1, false, &[], 500).unwrap(),
        )
        .unwrap()["revision"]
            .as_str()
            .unwrap()
            .to_owned();
        let server = SubtitleServer::default();
        let denied = server.replace_text(Parameters(ReplaceArgs {
            path: source_path.clone(),
            id: 1,
            expected_revision: revision.clone(),
            visible_text: "Changed".into(),
        }));
        assert_eq!(denied.is_error, Some(true));
        let denied_segment = server.replace_segment(Parameters(SegmentArgs {
            path: source_path.clone(),
            id: 1,
            expected_revision: revision.clone(),
            old_text: "Original".into(),
            new_text: "Changed".into(),
        }));
        assert_eq!(denied_segment.is_error, Some(true));
        let denied_batch = server.replace_text_batch(Parameters(BatchArgs {
            path: source_path.clone(),
            expected_revision: revision.clone(),
            edits: vec![BatchEditArgs {
                id: 1,
                visible_text: "Changed".into(),
            }],
        }));
        assert_eq!(denied_batch.is_error, Some(true));
        assert_eq!(fs::read_to_string(&source).unwrap(), original);

        let created = server.prepare_subtitle(Parameters(PrepareSubtitleArgs {
            source_path: source_path.clone(),
        }));
        assert_ne!(created.is_error, Some(true));
        let data = created.structured_content.unwrap();
        let editable_path = data["editable_path"].as_str().unwrap().to_owned();
        let backup_path = data["backup_path"].as_str().unwrap().to_owned();
        let prepared_revision = data["revision"].as_str().unwrap().to_owned();
        assert_eq!(editable_path, source_path);
        assert_eq!(fs::read_to_string(&backup_path).unwrap(), original);
        assert!(server.require_editable_file(&backup_path).is_err());
        let edited = server.clone().replace_text(Parameters(ReplaceArgs {
            path: editable_path.clone(),
            id: 1,
            expected_revision: prepared_revision,
            visible_text: "Changed".into(),
        }));
        assert_ne!(edited.is_error, Some(true));
        let autosave_path = edited.structured_content.unwrap()["autosave_path"]
            .as_str()
            .unwrap()
            .to_owned();
        assert_eq!(fs::read_to_string(&autosave_path).unwrap(), original);
        assert!(server.require_editable_file(&autosave_path).is_err());
        assert!(fs::read_to_string(&source).unwrap().contains("Changed"));
        assert_eq!(fs::read_to_string(&backup_path).unwrap(), original);
        let next = server.prepare_subtitle(Parameters(PrepareSubtitleArgs {
            source_path: editable_path.clone(),
        }));
        assert_ne!(next.is_error, Some(true));
        let next_backup = next.structured_content.unwrap()["backup_path"]
            .as_str()
            .unwrap()
            .to_owned();
        assert_ne!(next_backup, backup_path);
        assert!(fs::read_to_string(next_backup).unwrap().contains("Changed"));
        assert!(server.require_editable_file(&editable_path).is_ok());
    }

    #[test]
    fn mcp_group_edit_requires_backup_and_updates_all_occurrences() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("source.ass");
        let original = "[Events]\nFormat: Start, End, Style, Text\nDialogue: 0:00:01.00,0:00:02.00,Default,Hello\nDialogue: 0:00:03.00,0:00:04.00,Sign,{\\fnMarker}Hello\n";
        fs::write(&source, original).unwrap();
        let source_path = source.to_str().unwrap().to_owned();
        let server = SubtitleServer::default();
        let listed = server.list_text_groups(Parameters(ListGroupsArgs {
            path: source_path.clone(),
            offset: 0,
            limit: None,
            query: None,
        }));
        assert_ne!(listed.is_error, Some(true));
        let listing = listed.structured_content.unwrap();
        assert_eq!(listing["groups"][0]["occurrences"], 2);
        let denied = server.replace_text_group(Parameters(ReplaceGroupArgs {
            path: source_path.clone(),
            group_id: 1,
            expected_revision: listing["revision"].as_str().unwrap().to_owned(),
            source_text: "Hello".into(),
            translated_text: "Olá".into(),
        }));
        assert_eq!(denied.is_error, Some(true));
        let prepared = server.prepare_subtitle(Parameters(PrepareSubtitleArgs {
            source_path: source_path.clone(),
        }));
        let backup = prepared.structured_content.unwrap();
        let editable = backup["editable_path"].as_str().unwrap();
        let changed = server.replace_text_group(Parameters(ReplaceGroupArgs {
            path: editable.to_owned(),
            group_id: 1,
            expected_revision: backup["revision"].as_str().unwrap().to_owned(),
            source_text: "Hello".into(),
            translated_text: "Olá".into(),
        }));
        assert_ne!(changed.is_error, Some(true));
        assert_eq!(
            fs::read_to_string(backup["backup_path"].as_str().unwrap()).unwrap(),
            original
        );
        assert_eq!(
            fs::read_to_string(editable).unwrap().matches("Olá").count(),
            2
        );
    }

    #[test]
    fn empty_ass_is_registered_and_existing_destination_is_preserved() {
        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("new.ass");
        let server = SubtitleServer::default();
        let args = CreateAssArgs {
            destination_path: destination.display().to_string(),
            title: "Untitled".into(),
            play_res_x: 1920,
            play_res_y: 1080,
            font_name: "Noto Sans CJK JP".into(),
            font_size: None,
        };
        let created = server.create_ass(Parameters(args));
        assert_ne!(created.is_error, Some(true));
        let result = created.structured_content.unwrap();
        let working = result["working_path"].as_str().unwrap();
        assert_eq!(result["total_events"], 0);
        assert!(server.require_editable_file(working).is_ok());
        let original = fs::read_to_string(working).unwrap();
        let repeated = server.create_ass(Parameters(CreateAssArgs {
            destination_path: destination.display().to_string(),
            title: "Different".into(),
            play_res_x: 1920,
            play_res_y: 1080,
            font_name: "Noto Sans CJK JP".into(),
            font_size: None,
        }));
        assert_eq!(repeated.is_error, Some(true));
        assert_eq!(fs::read_to_string(working).unwrap(), original);
    }

    #[test]
    fn create_ass_uses_lato_when_font_is_omitted() {
        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("default.ass");
        let args: CreateAssArgs = serde_json::from_value(serde_json::json!({
            "destination_path": destination.display().to_string()
        }))
        .unwrap();
        let server = SubtitleServer::default();
        let result = server.create_ass(Parameters(args));
        assert_ne!(result.is_error, Some(true));
        let content = fs::read_to_string(destination).unwrap();
        assert!(content.contains("Style: Default,Lato,80,&H00F8F8F8,&H000000FF,&H00000000,&H96000000,-1,0,0,0,100,100,0,0,1,3.75,2.25,2,120,120,60,1"));
    }

    #[test]
    fn mcp_inserts_from_srt_only_into_registered_ass() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("source.srt");
        let destination = directory.path().join("new.ass");
        fs::write(&source, "1\n00:00:01,000 --> 00:00:02,000\nこんにちは\n").unwrap();
        let created = subtitle::create_ass(
            destination.to_str().unwrap(),
            &subtitle::AssTemplateOptions {
                title: "Untitled".into(),
                play_res_x: 1920,
                play_res_y: 1080,
                font_name: "Noto Sans CJK JP".into(),
                font_size: None,
            },
        )
        .unwrap();
        let server = SubtitleServer::default();
        let args = InsertEventsArgs {
            destination_path: created.working_path.clone(),
            expected_revision: created.revision.clone(),
            source_path: source.display().to_string(),
            source_ids: vec![1],
        };
        let denied = server.insert_events_from_source(Parameters(args));
        assert_eq!(denied.is_error, Some(true));
        assert_eq!(
            fs::read_to_string(&destination)
                .unwrap()
                .matches("Dialogue:")
                .count(),
            0
        );
        server
            .register_editable_file(&created.working_path)
            .unwrap();
        let allowed = server.insert_events_from_source(Parameters(InsertEventsArgs {
            destination_path: created.working_path,
            expected_revision: created.revision,
            source_path: source.display().to_string(),
            source_ids: vec![1],
        }));
        assert_ne!(allowed.is_error, Some(true));
        assert!(
            fs::read_to_string(&destination)
                .unwrap()
                .contains("こんにちは")
        );
    }

    #[test]
    fn mcp_creates_ass_events_only_in_registered_files() {
        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("new.ass");
        let server = SubtitleServer::default();
        let created = server.create_ass(Parameters(CreateAssArgs {
            destination_path: destination.display().to_string(),
            title: "Untitled".into(),
            play_res_x: 1920,
            play_res_y: 1080,
            font_name: "Lato".into(),
            font_size: None,
        }));
        assert_ne!(created.is_error, Some(true));
        let created = created.structured_content.unwrap();
        let working = created["working_path"].as_str().unwrap().to_owned();
        let revision = created["revision"].as_str().unwrap().to_owned();
        let unregistered = directory.path().join("unregistered.ass");
        fs::write(&unregistered, "[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n").unwrap();
        let denied = server.create_ass_events(Parameters(CreateAssEventsArgs {
            destination_path: unregistered.display().to_string(),
            expected_revision: revision.clone(),
            events: vec![CreateAssEventArgs {
                start: "0:00:01.00".into(),
                end: "0:00:02.00".into(),
                text: "Hello".into(),
                style: None,
                layer: None,
                name: None,
                margin_l: None,
                margin_r: None,
                margin_v: None,
                effect: None,
            }],
        }));
        assert_eq!(denied.is_error, Some(true));
        let allowed = server.create_ass_events(Parameters(CreateAssEventsArgs {
            destination_path: working.clone(),
            expected_revision: revision,
            events: vec![CreateAssEventArgs {
                start: "0:00:01.00".into(),
                end: "0:00:02.00".into(),
                text: "Hello".into(),
                style: None,
                layer: None,
                name: None,
                margin_l: None,
                margin_r: None,
                margin_v: None,
                effect: None,
            }],
        }));
        assert_ne!(allowed.is_error, Some(true));
        let result = allowed.structured_content.unwrap();
        assert_eq!(result["events"][0]["id"], 1);
        assert!(result["autosave_path"].as_str().is_some());
        assert!(
            fs::read_to_string(working)
                .unwrap()
                .contains("Dialogue: 0,0:00:01.00,0:00:02.00,Default,,0,0,0,,Hello")
        );
    }

    #[test]
    fn mcp_ass_event_updates_require_preparation() {
        use sha2::Digest;
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("existing.ass");
        let original = "[V4+ Styles]\nFormat: Name, Fontname\nStyle: Default,Lato\n[Events]\nFormat: Start, End, Style, Text\nDialogue: 0:00:01.00,0:00:02.00,Default,Hello\n";
        fs::write(&path, original).unwrap();
        let server = SubtitleServer::default();
        let args = serde_json::json!({
            "path": path.display().to_string(),
            "expected_revision": format!("{:x}", sha2::Sha256::digest(original.as_bytes())),
            "patches": [{"id": 1, "start": "0:00:01.20"}]
        });
        let denied =
            server.update_ass_events(Parameters(serde_json::from_value(args.clone()).unwrap()));
        assert_eq!(denied.is_error, Some(true));
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
        let prepared = server.prepare_subtitle(Parameters(PrepareSubtitleArgs {
            source_path: path.display().to_string(),
        }));
        assert_ne!(prepared.is_error, Some(true));
        let allowed = server.update_ass_events(Parameters(serde_json::from_value(args).unwrap()));
        assert_ne!(allowed.is_error, Some(true));
        assert!(fs::read_to_string(path).unwrap().contains("0:00:01.20"));
    }
}
