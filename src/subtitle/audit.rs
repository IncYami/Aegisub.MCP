use super::ass::{hash, split_tags, strip_blocks};
use super::ass_lines::{AssLine, dialogue_lines, non_dialogue_content};
use super::automation::{GradientPlan, is_per_character_effect};
use super::{Document, Result, error};
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

pub struct AuditPolicy {
    pub allowed_fields: Vec<String>,
    pub allowed_event_ids: Vec<usize>,
    pub allow_non_dialogue_changes: bool,
    pub allow_event_count_changes: bool,
    pub allow_reordering: bool,
    pub allow_gradient_rebuild: bool,
}

#[derive(Serialize)]
pub struct AuditFieldDifference {
    field: String,
    before: String,
    after: String,
    allowed: bool,
}

#[derive(Serialize)]
pub struct AuditEventDifference {
    baseline_id: usize,
    edited_id: usize,
    fields: Vec<AuditFieldDifference>,
    tags_changed: bool,
    gradient_rebuilt: bool,
    allowed: bool,
}

#[derive(Serialize)]
pub struct AuditSectionDifference {
    section: String,
    baseline_hash: Option<String>,
    edited_hash: Option<String>,
}

#[derive(Serialize)]
pub struct AuditMovedEvent {
    baseline_id: usize,
    edited_id: usize,
}

#[derive(Serialize)]
pub struct AuditResult {
    baseline_revision: String,
    edited_revision: String,
    baseline_events: usize,
    edited_events: usize,
    non_dialogue_changed: bool,
    non_dialogue_sections: Vec<AuditSectionDifference>,
    event_order_status: &'static str,
    moved_events: Vec<AuditMovedEvent>,
    alignment: &'static str,
    added_ids: Vec<usize>,
    removed_ids: Vec<usize>,
    unchanged_text_ids: Vec<usize>,
    total_changed_events: usize,
    policy_violations: Vec<String>,
    offset: usize,
    differences: Vec<AuditEventDifference>,
}

fn tags(raw: &str) -> Option<Vec<&str>> {
    let mut blocks = Vec::new();
    let mut rest = raw;
    while let Some(start) = rest.find('{') {
        let after = &rest[start + 1..];
        let end = after.find('}')?;
        blocks.push(&rest[start..start + end + 2]);
        rest = &after[end + 1..];
    }
    Some(blocks)
}

fn text_without_tags(raw: &str) -> String {
    strip_blocks(raw).unwrap_or_else(|| raw.to_owned())
}

fn section_blocks(content: &str) -> Vec<(String, String)> {
    let mut blocks = Vec::new();
    let mut name = "<preamble>".to_owned();
    let mut text = String::new();
    for full in content.split_inclusive('\n') {
        let trimmed = full.trim().trim_start_matches('\u{feff}');
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            if !text.is_empty() {
                blocks.push((name, text));
                text = String::new();
            }
            name = trimmed.to_owned();
        }
        text.push_str(full);
    }
    if !text.is_empty() {
        blocks.push((name, text));
    }
    blocks
}

fn changed_sections(before: &str, after: &str) -> Vec<AuditSectionDifference> {
    let old = section_blocks(before);
    let new = section_blocks(after);
    let mut changes = Vec::new();
    for index in 0..old.len().max(new.len()) {
        let left = old.get(index);
        let right = new.get(index);
        if left == right {
            continue;
        }
        changes.push(AuditSectionDifference {
            section: right
                .or(left)
                .map(|(name, _)| name.clone())
                .unwrap_or_default(),
            baseline_hash: left.map(|(_, content)| hash(content.as_bytes())),
            edited_hash: right.map(|(_, content)| hash(content.as_bytes())),
        });
    }
    changes
}

pub(super) fn exact_reorder(
    before: &[AssLine],
    after: &[AssLine],
    before_doc: &Document,
    after_doc: &Document,
) -> bool {
    if before.len() != after.len() {
        return false;
    }
    let mut a = BTreeMap::<&str, usize>::new();
    let mut b = BTreeMap::<&str, usize>::new();
    for line in before {
        *a.entry(line.raw_line(&before_doc.content)).or_default() += 1;
    }
    for line in after {
        *b.entry(line.raw_line(&after_doc.content)).or_default() += 1;
    }
    a == b
        && before.iter().zip(after).any(|(left, right)| {
            left.raw_line(&before_doc.content) != right.raw_line(&after_doc.content)
        })
}

fn moved_exact_lines(
    before: &[AssLine],
    after: &[AssLine],
    old: &str,
    new: &str,
) -> Vec<AuditMovedEvent> {
    let mut positions = BTreeMap::<&str, Vec<usize>>::new();
    for line in after {
        positions
            .entry(line.raw_line(new))
            .or_default()
            .push(line.id);
    }
    before
        .iter()
        .filter_map(|line| {
            let match_ids = positions.get(line.raw_line(old))?;
            (match_ids.len() == 1 && match_ids[0] != line.id).then_some(AuditMovedEvent {
                baseline_id: line.id,
                edited_id: match_ids[0],
            })
        })
        .collect()
}

fn non_text_signature(line: &AssLine) -> Vec<(&str, &str)> {
    line.fields
        .iter()
        .filter(|field| !field.eq_ignore_ascii_case("Text"))
        .map(|field| (field.as_str(), line.value(field).unwrap_or("")))
        .collect()
}

fn unchanged_alignment(
    before: &[AssLine],
    after: &[AssLine],
    old: &str,
    new: &str,
) -> Vec<(usize, usize)> {
    let n = before.len();
    let m = after.len();
    let old_hashes = before
        .iter()
        .map(|line| hash(line.raw_line(old).as_bytes()))
        .collect::<Vec<_>>();
    let new_hashes = after
        .iter()
        .map(|line| hash(line.raw_line(new).as_bytes()))
        .collect::<Vec<_>>();
    let mut lengths = vec![vec![0usize; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lengths[i][j] = if old_hashes[i] == new_hashes[j] {
                lengths[i + 1][j + 1] + 1
            } else {
                lengths[i + 1][j].max(lengths[i][j + 1])
            };
        }
    }
    let (mut i, mut j) = (0, 0);
    let mut pairs = Vec::new();
    while i < n && j < m {
        if old_hashes[i] == new_hashes[j] {
            pairs.push((i, j));
            i += 1;
            j += 1;
        } else if lengths[i + 1][j] >= lengths[i][j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    pairs
}

fn compare_event(
    before: &AssLine,
    after: &AssLine,
    before_doc: &Document,
    after_doc: &Document,
    allowed_fields: &BTreeSet<String>,
    allowed_ids: &BTreeSet<usize>,
    allow_gradient_rebuild: bool,
) -> AuditEventDifference {
    let mut fields = Vec::new();
    let id_allowed = allowed_ids.is_empty() || allowed_ids.contains(&before.id);
    let all_names = before
        .fields
        .iter()
        .chain(&after.fields)
        .map(|name| name.to_ascii_lowercase())
        .collect::<BTreeSet<_>>();
    for name in all_names {
        let old = before.value(&name).unwrap_or("");
        let new = after.value(&name).unwrap_or("");
        if old != new {
            fields.push(AuditFieldDifference {
                field: name.clone(),
                before: old.to_owned(),
                after: new.to_owned(),
                allowed: id_allowed && allowed_fields.contains(&name),
            });
        }
    }
    let old_raw = before.value("Text").unwrap_or("");
    let new_raw = after.value("Text").unwrap_or("");
    let tags_changed = tags(old_raw) != tags(new_raw)
        || split_tags(old_raw).is_err()
        || split_tags(new_raw).is_err();
    let old_event = &before_doc.events[before.id - 1];
    let new_event = &after_doc.events[after.id - 1];
    let gradient_rebuilt = tags_changed
        && is_per_character_effect(old_raw)
        && is_per_character_effect(new_raw)
        && GradientPlan::parse_with_defaults(old_raw, old_event.style_defaults.as_deref())
            .zip(GradientPlan::parse_with_defaults(
                new_raw,
                new_event.style_defaults.as_deref(),
            ))
            .is_some_and(|(old, new)| old.same_structure_as(&new));
    let text_changed = text_without_tags(old_raw) != text_without_tags(new_raw);
    if old_raw != new_raw && !text_changed && !tags_changed {
        fields.push(AuditFieldDifference {
            field: "text_bytes".to_owned(),
            before: old_raw.to_owned(),
            after: new_raw.to_owned(),
            allowed: false,
        });
    }
    if tags_changed {
        fields.push(AuditFieldDifference {
            field: "tags".to_owned(),
            before: tags(old_raw).map(|v| v.join("")).unwrap_or_default(),
            after: tags(new_raw).map(|v| v.join("")).unwrap_or_default(),
            allowed: id_allowed
                && (allowed_fields.contains("tags")
                    || (allow_gradient_rebuild && gradient_rebuilt)),
        });
    }
    let escape_counts = |raw: &str| ["\\N", "\\n", "\\h"].map(|escape| raw.matches(escape).count());
    if escape_counts(old_raw) != escape_counts(new_raw) {
        fields.push(AuditFieldDifference {
            field: "escapes".to_owned(),
            before: format!("{:?}", escape_counts(old_raw)),
            after: format!("{:?}", escape_counts(new_raw)),
            allowed: id_allowed && allowed_fields.contains("escapes"),
        });
    }
    let line_ending_before = &before_doc.content[before.line_range.end..before.full_range.end];
    let line_ending_after = &after_doc.content[after.line_range.end..after.full_range.end];
    if line_ending_before != line_ending_after {
        fields.push(AuditFieldDifference {
            field: "line_ending".to_owned(),
            before: line_ending_before.to_owned(),
            after: line_ending_after.to_owned(),
            allowed: false,
        });
    }
    let allowed = fields.iter().all(|field| field.allowed);
    AuditEventDifference {
        baseline_id: before.id,
        edited_id: after.id,
        fields,
        tags_changed,
        gradient_rebuilt,
        allowed,
    }
}

pub fn audit_subtitle(
    baseline_path: &str,
    edited_path: &str,
    expected_baseline_revision: &str,
    expected_edited_revision: &str,
    policy: &AuditPolicy,
    offset: usize,
    limit: usize,
) -> Result<AuditResult> {
    let baseline = Document::open(Path::new(baseline_path))?;
    let edited = Document::open(Path::new(edited_path))?;
    if baseline.revision != expected_baseline_revision
        || edited.revision != expected_edited_revision
    {
        return Err(error("Revision conflict: an audited subtitle changed"));
    }
    if !baseline_path.to_ascii_lowercase().ends_with(".ass")
        || !edited_path.to_ascii_lowercase().ends_with(".ass")
    {
        return Err(error("ASS audit requires two .ass files"));
    }
    let old_lines = dialogue_lines(&baseline.content)?;
    let new_lines = dialogue_lines(&edited.content)?;
    if old_lines.len() != baseline.events.len() || new_lines.len() != edited.events.len() {
        return Err(error("ASS Dialogue parsing is inconsistent"));
    }
    let old_other = non_dialogue_content(&baseline.content, &old_lines);
    let new_other = non_dialogue_content(&edited.content, &new_lines);
    let non_dialogue_changed = old_other != new_other;
    let non_dialogue_sections = if non_dialogue_changed {
        changed_sections(&old_other, &new_other)
    } else {
        Vec::new()
    };
    let exact_order_change = exact_reorder(&old_lines, &new_lines, &baseline, &edited);
    let moved_events = if exact_order_change {
        moved_exact_lines(&old_lines, &new_lines, &baseline.content, &edited.content)
    } else {
        Vec::new()
    };
    let event_order_status = if exact_order_change {
        "changed"
    } else if old_lines.len() == new_lines.len()
        && old_lines
            .iter()
            .zip(&new_lines)
            .all(|(left, right)| non_text_signature(left) == non_text_signature(right))
    {
        "unchanged_by_non_text_fields"
    } else {
        "indeterminate"
    };
    let allowed_fields = policy
        .allowed_fields
        .iter()
        .map(|field| field.to_ascii_lowercase())
        .collect::<BTreeSet<_>>();
    let allowed_ids = policy
        .allowed_event_ids
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    let mut differences = Vec::new();
    let mut added_ids = Vec::new();
    let mut removed_ids = Vec::new();
    let mut unchanged_text_ids = Vec::new();
    let alignment;
    if exact_order_change {
        alignment = "exact_line_multiset";
        unchanged_text_ids.extend(old_lines.iter().map(|line| line.id));
    } else if old_lines.len() == new_lines.len() {
        alignment = "position";
        for (old, new) in old_lines.iter().zip(&new_lines) {
            let diff = compare_event(
                old,
                new,
                &baseline,
                &edited,
                &allowed_fields,
                &allowed_ids,
                policy.allow_gradient_rebuild,
            );
            if old.value("Text") == new.value("Text") {
                unchanged_text_ids.push(old.id);
            }
            if !diff.fields.is_empty() {
                differences.push(diff);
            }
        }
    } else {
        alignment = "unchanged_line_sequence";
        let matches =
            unchanged_alignment(&old_lines, &new_lines, &baseline.content, &edited.content);
        let old_matched = matches.iter().map(|(old, _)| *old).collect::<BTreeSet<_>>();
        let new_matched = matches.iter().map(|(_, new)| *new).collect::<BTreeSet<_>>();
        unchanged_text_ids.extend(matches.iter().map(|(old, _)| old_lines[*old].id));
        removed_ids.extend(
            old_lines
                .iter()
                .enumerate()
                .filter(|(index, _)| !old_matched.contains(index))
                .map(|(_, line)| line.id),
        );
        added_ids.extend(
            new_lines
                .iter()
                .enumerate()
                .filter(|(index, _)| !new_matched.contains(index))
                .map(|(_, line)| line.id),
        );
    }
    let mut policy_violations = Vec::new();
    if non_dialogue_changed && !policy.allow_non_dialogue_changes {
        policy_violations.push("Non-Dialogue content changed".to_owned());
    }
    if (!added_ids.is_empty() || !removed_ids.is_empty()) && !policy.allow_event_count_changes {
        policy_violations.push(
            "Events were added, removed, or could not be aligned after reordering".to_owned(),
        );
    }
    if event_order_status == "changed" && !policy.allow_reordering {
        policy_violations.push("Dialogue event order changed".to_owned());
    }
    if event_order_status == "indeterminate" && !policy.allow_reordering {
        policy_violations.push(
            "Dialogue event identity/order cannot be verified from this comparison".to_owned(),
        );
    }
    policy_violations.extend(differences.iter().filter(|diff| !diff.allowed).map(|diff| {
        format!(
            "Event {} changed outside the allowed fields",
            diff.baseline_id
        )
    }));
    let old_current =
        fs::read(baseline_path).map_err(|e| error(format!("Cannot recheck baseline: {e}")))?;
    let new_current =
        fs::read(edited_path).map_err(|e| error(format!("Cannot recheck edited subtitle: {e}")))?;
    if hash(&old_current) != baseline.revision || hash(&new_current) != edited.revision {
        return Err(error("An audited subtitle changed during comparison"));
    }
    let total_changed_events = differences.len();
    Ok(AuditResult {
        baseline_revision: baseline.revision,
        edited_revision: edited.revision,
        baseline_events: old_lines.len(),
        edited_events: new_lines.len(),
        non_dialogue_changed,
        non_dialogue_sections,
        event_order_status,
        moved_events,
        alignment,
        added_ids,
        removed_ids,
        unchanged_text_ids,
        total_changed_events,
        policy_violations,
        offset,
        differences: differences.into_iter().skip(offset).take(limit).collect(),
    })
}
