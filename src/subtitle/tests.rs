use super::ass::{hash, is_drawing};
use super::automation::{GradientPlan, is_per_character_effect};
use super::*;
use std::fs;

fn ass_fixture() -> std::path::PathBuf {
    std::path::PathBuf::from("tests/fixtures/sample.ass")
}

fn direct_event(start: &str, end: &str, text: &str) -> NewAssEvent {
    NewAssEvent {
        start: start.to_owned(),
        end: end.to_owned(),
        text: text.to_owned(),
        style: None,
        layer: None,
        name: None,
        margin_l: None,
        margin_r: None,
        margin_v: None,
        effect: None,
    }
}

fn ass_patch(id: usize) -> AssEventPatch {
    AssEventPatch {
        id,
        start: None,
        end: None,
        style: None,
        layer: None,
        name: None,
        margin_l: None,
        margin_r: None,
        margin_v: None,
        effect: None,
        raw_text: None,
        allow_escape_changes: false,
    }
}

fn small_ass() -> &'static str {
    "[Script Info]\nTitle: Test\n[V4+ Styles]\nFormat: Name, Fontname\nStyle: Default,Lato\nStyle: Sign,Lato\n[Events]\nFormat: Start, End, Style, Layer, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0:00:01.00,0:00:02.00,Default,0,,0,0,0,,Hello\\Nworld\nComment: keep this\nDialogue: 0:00:03.00,0:00:04.00,Sign,1,,0,0,0,,Second\nDialogue: 0:00:05.00,0:00:06.00,Default,0,,0,0,0,,Third\n"
}

#[test]
fn ass_field_and_tag_updates_are_atomic_and_auditable() {
    let dir = tempfile::tempdir().unwrap();
    let baseline = dir.path().join("baseline.ass");
    let active = dir.path().join("active.ass");
    fs::write(&baseline, small_ass()).unwrap();
    fs::write(&active, small_ass()).unwrap();
    let revision = super::ass::hash(small_ass().as_bytes());
    let mut valid = ass_patch(1);
    valid.start = Some("0:00:01.20".into());
    valid.end = Some("0:00:02.40".into());
    valid.raw_text = Some("{\\pos(100,200)\\fnLato\\c&HFFFFFF&}Olá\\Nmundo".into());
    let mut invalid = ass_patch(2);
    invalid.layer = Some("-1".into());
    assert!(update_ass_events(active.to_str().unwrap(), &revision, &[valid, invalid]).is_err());
    assert_eq!(fs::read_to_string(&active).unwrap(), small_ass());
    let mut invalid_tag = ass_patch(1);
    invalid_tag.raw_text = Some("{\\pos(unknown,200)}Hello\\Nworld".into());
    assert!(update_ass_events(active.to_str().unwrap(), &revision, &[invalid_tag]).is_err());
    assert_eq!(fs::read_to_string(&active).unwrap(), small_ass());
    let mut valid = ass_patch(1);
    valid.start = Some("0:00:01.20".into());
    valid.end = Some("0:00:02.40".into());
    valid.raw_text = Some("{\\pos(100,200)\\fnLato\\c&HFFFFFF&}Olá\\Nmundo".into());
    let updated = update_ass_events(active.to_str().unwrap(), &revision, &[valid]).unwrap();
    let value = serde_json::to_value(&updated).unwrap();
    assert_eq!(value["changes"].as_array().unwrap().len(), 3);
    assert!(
        fs::read_to_string(&active)
            .unwrap()
            .contains("Comment: keep this")
    );
    let policy = AuditPolicy {
        allowed_fields: vec!["Text".into()],
        allowed_event_ids: vec![],
        allow_non_dialogue_changes: false,
        allow_event_count_changes: false,
        allow_reordering: false,
        allow_gradient_rebuild: false,
    };
    let audit = audit_subtitle(
        baseline.to_str().unwrap(),
        active.to_str().unwrap(),
        &revision,
        value["revision"].as_str().unwrap(),
        &policy,
        0,
        50,
    )
    .unwrap();
    let report = serde_json::to_value(audit).unwrap();
    assert_eq!(report["total_changed_events"], 1);
    assert_eq!(report["event_order_status"], "indeterminate");
    assert!(!report["policy_violations"].as_array().unwrap().is_empty());
    assert_eq!(report["differences"][0]["baseline_id"], 1);
    assert_eq!(report["differences"][0]["tags_changed"], true);
    assert!(
        audit_subtitle(
            baseline.to_str().unwrap(),
            active.to_str().unwrap(),
            &revision,
            "stale",
            &policy,
            0,
            50
        )
        .is_err()
    );
}

#[test]
fn ass_move_delete_and_restore_preserve_other_lines() {
    let dir = tempfile::tempdir().unwrap();
    let active = dir.path().join("active.ass");
    fs::write(&active, small_ass()).unwrap();
    let revision = super::ass::hash(small_ass().as_bytes());
    let moved = move_ass_event(active.to_str().unwrap(), &revision, 3, Some(1)).unwrap();
    let moved_value = serde_json::to_value(&moved).unwrap();
    let moved_revision = moved_value["revision"].as_str().unwrap();
    let moved_text = fs::read_to_string(&active).unwrap();
    assert!(moved_text.find("Third").unwrap() < moved_text.find("Hello").unwrap());
    assert!(moved_text.contains("Comment: keep this"));
    let baseline = dir.path().join("baseline.ass");
    fs::write(&baseline, small_ass()).unwrap();
    let policy = AuditPolicy {
        allowed_fields: vec!["Text".into()],
        allowed_event_ids: vec![],
        allow_non_dialogue_changes: false,
        allow_event_count_changes: false,
        allow_reordering: false,
        allow_gradient_rebuild: false,
    };
    let order_report = serde_json::to_value(
        audit_subtitle(
            baseline.to_str().unwrap(),
            active.to_str().unwrap(),
            &revision,
            moved_revision,
            &policy,
            0,
            50,
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(order_report["event_order_status"], "changed");
    assert_eq!(order_report["moved_events"][0]["baseline_id"], 1);
    assert_eq!(order_report["added_ids"].as_array().unwrap().len(), 0);
    assert_eq!(order_report["removed_ids"].as_array().unwrap().len(), 0);
    assert_eq!(
        order_report["unchanged_text_ids"].as_array().unwrap().len(),
        3
    );
    let reordered_filter = EventFilters {
        changed_since_path: Some(baseline.display().to_string()),
        changed_since_revision: Some(revision.clone()),
        ..EventFilters::default()
    };
    assert!(
        list_events_filtered(
            active.to_str().unwrap(),
            0,
            20,
            None,
            &[],
            500,
            &reordered_filter
        )
        .is_err()
    );
    let deleted = delete_ass_events(active.to_str().unwrap(), moved_revision, &[2]).unwrap();
    let deleted_value = serde_json::to_value(&deleted).unwrap();
    assert_eq!(deleted_value["remaining_events"], 2);
    assert!(
        !fs::read_to_string(&active)
            .unwrap()
            .contains("Hello\\Nworld")
    );
    let moved_baseline = dir.path().join("moved-baseline.ass");
    fs::write(&moved_baseline, &moved_text).unwrap();
    let deletion_report = serde_json::to_value(
        audit_subtitle(
            moved_baseline.to_str().unwrap(),
            active.to_str().unwrap(),
            moved_revision,
            deleted_value["revision"].as_str().unwrap(),
            &policy,
            0,
            50,
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(deletion_report["removed_ids"], serde_json::json!([2]));
    assert_eq!(deletion_report["added_ids"], serde_json::json!([]));
    assert!(
        !deletion_report["policy_violations"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let restored = restore_ass_version(
        active.to_str().unwrap(),
        deleted_value["revision"].as_str().unwrap(),
        deleted_value["autosave_path"].as_str().unwrap(),
    )
    .unwrap();
    let restored_value = serde_json::to_value(restored).unwrap();
    assert_eq!(restored_value["revision"], moved_revision);
    assert_eq!(fs::read_to_string(&active).unwrap(), moved_text);
}

#[test]
fn filtered_windows_and_context_diagnostics_report_candidates() {
    let dir = tempfile::tempdir().unwrap();
    let active = dir.path().join("active.ass");
    let context = dir.path().join("context.ass");
    fs::write(&active, small_ass()).unwrap();
    fs::write(&context, small_ass()).unwrap();
    let filters = EventFilters {
        start_ms: Some(2500),
        end_ms: Some(4500),
        style: Some("Sign".into()),
        include_raw: true,
        diagnose_context: true,
        ..EventFilters::default()
    };
    let track = ContextTrack {
        name: "reference".into(),
        path: context.display().to_string(),
    };
    let result = list_events_filtered(
        active.to_str().unwrap(),
        0,
        20,
        None,
        &[track],
        500,
        &filters,
    )
    .unwrap();
    let value = serde_json::to_value(result).unwrap();
    assert_eq!(value["matched_events"], 1);
    assert_eq!(value["events"][0]["id"], 2);
    assert_eq!(value["events"][0]["raw_text"], "Second");
    assert_eq!(
        value["events"][0]["context_diagnostics"]["reference"]["status"],
        "exact"
    );
    let changed = EventFilters {
        changed_since_path: Some(context.display().to_string()),
        changed_since_revision: Some(super::ass::hash(small_ass().as_bytes())),
        ..EventFilters::default()
    };
    assert_eq!(
        list_events_filtered(active.to_str().unwrap(), 0, 20, None, &[], 500, &changed)
            .unwrap()
            .matched_events,
        0
    );
}

#[test]
fn audit_reports_other_sections_and_rejects_stale_revisions() {
    let dir = tempfile::tempdir().unwrap();
    let baseline = dir.path().join("baseline.ass");
    let edited = dir.path().join("edited.ass");
    fs::write(&baseline, small_ass()).unwrap();
    let changed = small_ass().replace("Title: Test", "Title: Changed");
    fs::write(&edited, &changed).unwrap();
    let policy = AuditPolicy {
        allowed_fields: vec!["Text".into()],
        allowed_event_ids: vec![],
        allow_non_dialogue_changes: false,
        allow_event_count_changes: false,
        allow_reordering: false,
        allow_gradient_rebuild: false,
    };
    let report = serde_json::to_value(
        audit_subtitle(
            baseline.to_str().unwrap(),
            edited.to_str().unwrap(),
            &super::ass::hash(small_ass().as_bytes()),
            &super::ass::hash(changed.as_bytes()),
            &policy,
            0,
            20,
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(report["non_dialogue_changed"], true);
    assert_eq!(
        report["non_dialogue_sections"][0]["section"],
        "[Script Info]"
    );
    assert_eq!(report["total_changed_events"], 0);
    assert!(!report["policy_violations"].as_array().unwrap().is_empty());
    assert!(
        audit_subtitle(
            baseline.to_str().unwrap(),
            edited.to_str().unwrap(),
            "stale",
            &super::ass::hash(changed.as_bytes()),
            &policy,
            0,
            20
        )
        .is_err()
    );
}

#[test]
fn direct_ass_event_creation_uses_reordered_format_and_preserves_tags() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("direct.ass");
    let original = "\u{feff}[Script Info]\r\nTitle: direct\r\n[V4+ Styles]\r\nFormat: Name, Fontname, Fontsize\r\nStyle: Default,Arial,20\r\n[Events]\r\nFormat: Start, End, Style, Layer, Name, MarginL, MarginR, MarginV, Effect, Text\r\nComment: untouched\r\n";
    fs::write(&path, original).unwrap();
    let revision = super::ass::hash(original.as_bytes());
    let mut event = direct_event(
        "0:00:01.20",
        "0:00:02.30",
        "{\\unknownTag7}Hello, world\\Nnext",
    );
    event.layer = Some("2".to_owned());
    event.name = Some("actor".to_owned());
    event.margin_l = Some("10".to_owned());
    event.margin_r = Some("20".to_owned());
    event.margin_v = Some("30".to_owned());
    event.effect = Some("Banner;3;0;100".to_owned());
    let result = create_ass_events(path.to_str().unwrap(), &revision, &[event]).unwrap();
    assert_eq!(result.events.len(), 1);
    assert_eq!(result.events[0].id, 1);
    assert_eq!(
        result.events[0].raw_text.as_deref(),
        Some("{\\unknownTag7}Hello, world\\Nnext")
    );
    let contents = fs::read_to_string(path).unwrap();
    assert!(contents.contains("Dialogue: 0:00:01.20,0:00:02.30,Default,2,actor,10,20,30,Banner;3;0;100,{\\unknownTag7}Hello, world\\Nnext\r\n"));
    assert!(contents.starts_with('\u{feff}'));
    assert!(contents.contains("Comment: untouched\r\n"));
}

#[test]
fn direct_ass_event_creation_rejects_invalid_batch_and_stale_revision_without_writing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("direct.ass");
    let original = "[V4+ Styles]\nFormat: Name, Fontname\nStyle: Default,Arial\n[Events]\nFormat: Layer, Start, End, Style, Text\n";
    fs::write(&path, original).unwrap();
    let revision = super::ass::hash(original.as_bytes());
    let valid = direct_event("0:00:01.00", "0:00:02.00", "Valid");
    let invalid = direct_event("0:00:02.00", "0:00:03.00", "nested {a{b}} and unmatched }");
    assert!(create_ass_events(path.to_str().unwrap(), &revision, &[valid, invalid]).is_err());
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    let event = direct_event("0:00:01.00", "0:00:02.00", "Valid");
    assert!(create_ass_events(path.to_str().unwrap(), "stale", &[event]).is_err());
    assert_eq!(fs::read_to_string(&path).unwrap(), original);

    let mut negative_layer = direct_event("0:00:01.00", "0:00:02.00", "Negative layer");
    negative_layer.layer = Some("-1".to_owned());
    assert!(create_ass_events(path.to_str().unwrap(), &revision, &[negative_layer]).is_err());
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
}

#[test]
fn direct_ass_event_creation_rejects_supplied_field_missing_from_format() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("direct.ass");
    let original = "[V4+ Styles]\nFormat: Name, Fontname\nStyle: Default,Arial\n[Events]\nFormat: Start, End, Style, Text\n";
    fs::write(&path, original).unwrap();
    let revision = super::ass::hash(original.as_bytes());
    let mut event = direct_event("0:00:01.00", "0:00:02.00", "Has name");
    event.name = Some("Actor".to_owned());
    assert!(create_ass_events(path.to_str().unwrap(), &revision, &[event]).is_err());
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
}

#[test]
fn edit_preserves_unrelated_bytes_and_tags() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.ass");
    let original = "\u{feff}[Script Info]\r\nTitle: sample\r\n[Events]\r\nFormat: Layer, Start, End, Style, Text\r\nDialogue: 0,0:00:01.00,0:00:02.00,Default,{\\i1}Hello {\\i0}world\\Nagain\r\nComment: 0,0:00:01.00,0:00:02.00,Default,ignore\r\n";
    fs::write(&path, original).unwrap();
    let name = path.to_str().unwrap();
    let before = get_event_with_context(name, 1, false, &[], 500).unwrap();
    assert_eq!(before.event.visible_text, "Hello {1}world\\Nagain");
    replace_text(name, 1, &before.revision, "Olá {1}mundo\\Nnovamente").unwrap();
    let result = fs::read_to_string(&path).unwrap();
    assert_eq!(
        result,
        original.replace("Hello {\\i0}world\\Nagain", "Olá {\\i0}mundo\\Nnovamente")
    );
    assert!(replace_text(name, 1, &before.revision, "Outro {1}texto\\Nfim").is_err());
}

#[test]
fn preparation_preserves_timestamped_backups_and_edits_original() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source.ass");
    let original = "\u{feff}[Events]\r\nFormat: Start, End, Style, Text\r\nDialogue: 0:00:01.00,0:00:02.00,Default,Original\r\n";
    fs::write(&source, original).unwrap();
    let prepared = prepare_subtitle(source.to_str().unwrap()).unwrap();
    assert_eq!(prepared.editable_path, source.to_str().unwrap());
    assert_eq!(prepared.total_events, 1);
    let backup = std::path::Path::new(&prepared.backup_path);
    assert_eq!(backup.parent().unwrap(), dir.path().join(".backup"));
    let name = backup.file_name().unwrap().to_str().unwrap();
    assert!(name.starts_with("source.20"));
    assert!(name.ends_with("Z.BACKUP.ass"));
    assert_eq!(fs::read_to_string(backup).unwrap(), original);
    assert!(prepare_subtitle(backup.to_str().unwrap()).is_err());
    let changed =
        replace_text(&prepared.editable_path, 1, &prepared.revision, "Translated").unwrap();
    assert!(fs::read_to_string(&source).unwrap().contains("Translated"));
    assert_eq!(fs::read_to_string(backup).unwrap(), original);
    let autosave = std::path::Path::new(&changed.autosave_path);
    assert_eq!(autosave.parent().unwrap(), dir.path().join(".autosave"));
    assert!(
        autosave
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .ends_with("Z.AUTOSAVE.ass")
    );
    assert_eq!(fs::read_to_string(autosave).unwrap(), original);
    assert!(prepare_subtitle(autosave.to_str().unwrap()).is_err());
    let second = prepare_subtitle(source.to_str().unwrap()).unwrap();
    assert_ne!(second.backup_path, prepared.backup_path);
    assert!(
        fs::read_to_string(second.backup_path)
            .unwrap()
            .contains("Translated")
    );
}

#[test]
fn each_edit_keeps_the_previous_version_in_autosave() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("episode.ass");
    let original = "[Events]\nFormat: Start, End, Style, Text\nDialogue: 0:00:01.00,0:00:02.00,Default,Original\n";
    fs::write(&source, original).unwrap();
    let revision = Document::open(&source).unwrap().revision;
    let first = replace_text(source.to_str().unwrap(), 1, &revision, "First").unwrap();
    let first_content = fs::read_to_string(&source).unwrap();
    let second = replace_text(source.to_str().unwrap(), 1, &first.revision, "Second").unwrap();

    assert_eq!(fs::read_to_string(&first.autosave_path).unwrap(), original);
    assert_eq!(
        fs::read_to_string(&second.autosave_path).unwrap(),
        first_content
    );
    assert_ne!(first.autosave_path, second.autosave_path);
    assert!(fs::read_to_string(&source).unwrap().contains("Second"));
    assert!(replace_text(source.to_str().unwrap(), 1, &revision, "Stale").is_err());
    let count = fs::read_dir(dir.path().join(".autosave"))
        .unwrap()
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .ends_with(".AUTOSAVE.ass")
        })
        .count();
    assert_eq!(count, 2);
}

#[test]
fn editing_fails_without_a_safe_autosave_directory() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("episode.ass");
    let original = "[Events]\nFormat: Start, End, Style, Text\nDialogue: 0:00:01.00,0:00:02.00,Default,Original\n";
    fs::write(&source, original).unwrap();
    fs::write(dir.path().join(".autosave"), "not a directory").unwrap();
    let revision = Document::open(&source).unwrap().revision;

    assert!(replace_text(source.to_str().unwrap(), 1, &revision, "Changed").is_err());
    assert_eq!(fs::read_to_string(&source).unwrap(), original);
}

#[test]
fn creates_empty_ass_with_configured_base_without_overwriting() {
    let dir = tempfile::tempdir().unwrap();
    let destination = dir.path().join("translation.ass");
    let options = AssTemplateOptions {
        title: "Monogatari Translation".into(),
        play_res_x: 1280,
        play_res_y: 720,
        font_name: "Noto Sans CJK JP".into(),
        font_size: None,
    };
    let created = create_ass(destination.to_str().unwrap(), &options).unwrap();
    assert_eq!(created.total_events, 0);
    assert_eq!(created.working_path, destination.to_str().unwrap());
    let output = fs::read_to_string(&destination).unwrap();
    assert_eq!(created.revision, hash(output.as_bytes()));
    assert!(output.contains("[Script Info]\n"));
    assert!(output.contains("Title: Monogatari Translation\n"));
    assert!(output.contains("ScriptType: v4.00+\n"));
    assert!(output.contains("PlayResX: 1280\nPlayResY: 720\n"));
    assert!(output.contains("[V4+ Styles]\n"));
    assert!(output.contains("Style: Default,Noto Sans CJK JP,53,&H00F8F8F8,&H000000FF,&H00000000,&H96000000,-1,0,0,0,100,100,0,0,1,2.50,1.50,2,80,80,40,1"));
    assert!(output.contains("[Events]\n"));
    assert!(!output.contains("Dialogue:"));
    let listed = list_events_with_context(&created.working_path, 0, 50, None, &[], 500).unwrap();
    assert_eq!(listed.total_events, 0);
    assert!(create_ass(destination.to_str().unwrap(), &options).is_err());
    assert_eq!(fs::read_to_string(&destination).unwrap(), output);
}

#[test]
fn ass_creation_rejects_invalid_options_without_creating_output() {
    let dir = tempfile::tempdir().unwrap();
    let destination = dir.path().join("translation.ass");
    let mut options = AssTemplateOptions {
        title: "Untitled".into(),
        play_res_x: 1920,
        play_res_y: 1080,
        font_name: "Noto Sans CJK JP".into(),
        font_size: None,
    };
    options.play_res_x = 0;
    assert!(create_ass(destination.to_str().unwrap(), &options).is_err());
    options.play_res_x = 1920;
    options.font_name = "Bad,Font".into();
    assert!(create_ass(destination.to_str().unwrap(), &options).is_err());
    options.font_name = "Noto Sans CJK JP".into();
    options.title = "Bad\nTitle".into();
    assert!(create_ass(destination.to_str().unwrap(), &options).is_err());
    assert!(!destination.exists());
    options.title = "Untitled".into();
    let autosave = dir.path().join(".autosave");
    fs::create_dir(&autosave).unwrap();
    let inside_autosave = autosave.join("translation.ass");
    assert!(create_ass(inside_autosave.to_str().unwrap(), &options).is_err());
    assert!(!inside_autosave.exists());
}

#[test]
fn inserts_srt_cues_into_created_ass_and_then_translates_them() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("japanese.srt");
    let destination = dir.path().join("translation.ass");
    let original = "\u{feff}1\r\n00:00:01,234 --> 00:00:02,345\r\nこんにちは、世界。\r\n二行目\r\n\r\n2\r\n00:00:03,000 --> 00:00:04,000\r\n次の字幕\r\n";
    fs::write(&source, original).unwrap();
    let created = create_ass(
        destination.to_str().unwrap(),
        &AssTemplateOptions {
            title: "Translation".into(),
            play_res_x: 1920,
            play_res_y: 1080,
            font_name: "Noto Sans CJK JP".into(),
            font_size: None,
        },
    )
    .unwrap();
    let inserted = insert_events_from_source(
        &created.working_path,
        &created.revision,
        source.to_str().unwrap(),
        &[1, 2],
    )
    .unwrap();
    assert_eq!(inserted.events.len(), 2);
    assert_eq!(inserted.events[0].id, 1);
    assert_eq!(
        inserted.events[0].visible_text,
        "こんにちは、世界。\\N二行目"
    );
    assert_eq!(inserted.source_revision, hash(original.as_bytes()));
    assert_eq!(
        hash(&fs::read(&inserted.autosave_path).unwrap()),
        created.revision
    );
    assert_eq!(fs::read_to_string(&source).unwrap(), original);
    let output = fs::read_to_string(&destination).unwrap();
    assert!(
        output.contains(
            "Dialogue: 0,0:00:01.23,0:00:02.35,Default,,0,0,0,,こんにちは、世界。\\N二行目"
        )
    );
    assert!(output.contains("Dialogue: 0,0:00:03.00,0:00:04.00,Default,,0,0,0,,次の字幕"));
    assert!(
        insert_events_from_source(
            &created.working_path,
            &created.revision,
            source.to_str().unwrap(),
            &[1]
        )
        .is_err()
    );
    replace_text(
        &created.working_path,
        1,
        &inserted.revision,
        "Olá, mundo.\\NSegunda linha",
    )
    .unwrap();
    assert!(
        fs::read_to_string(&destination)
            .unwrap()
            .contains("Olá, mundo.\\NSegunda linha")
    );
    assert_eq!(fs::read_to_string(&source).unwrap(), original);
}

#[test]
fn insertion_rejects_invalid_batch_without_changing_destination() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("styled.srt");
    let destination = dir.path().join("translation.ass");
    fs::write(&source, "1\n00:00:01,000 --> 00:00:02,000\nPlain\n\n2\n00:00:03,000 --> 00:00:04,000\n<i>Styled</i>\n").unwrap();
    let created = create_ass(
        destination.to_str().unwrap(),
        &AssTemplateOptions {
            title: "Untitled".into(),
            play_res_x: 1920,
            play_res_y: 1080,
            font_name: "Noto Sans CJK JP".into(),
            font_size: None,
        },
    )
    .unwrap();
    let before = fs::read_to_string(&destination).unwrap();
    assert!(
        insert_events_from_source(
            &created.working_path,
            &created.revision,
            source.to_str().unwrap(),
            &[1, 2]
        )
        .is_err()
    );
    assert!(
        insert_events_from_source(
            &created.working_path,
            &created.revision,
            source.to_str().unwrap(),
            &[1, 1]
        )
        .is_err()
    );
    assert_eq!(fs::read_to_string(&destination).unwrap(), before);
}

#[test]
fn inserts_vtt_and_ass_events_without_changing_sources() {
    let dir = tempfile::tempdir().unwrap();
    let vtt = dir.path().join("source.vtt");
    let ass = dir.path().join("source.ass");
    let destination = dir.path().join("translation.ass");
    let vtt_text = "WEBVTT\n\n00:01.005 --> 00:02.010\nFirst line\nSecond line\n";
    let ass_text = "[Events]\nFormat: Layer, Start, End, Style, Text\nDialogue: 0,0:00:03.00,0:00:04.00,Other,{\\i1}Tagged{\\i0} text\n";
    fs::write(&vtt, vtt_text).unwrap();
    fs::write(&ass, ass_text).unwrap();
    let created = create_ass(
        destination.to_str().unwrap(),
        &AssTemplateOptions {
            title: "Untitled".into(),
            play_res_x: 1920,
            play_res_y: 1080,
            font_name: "Noto Sans CJK JP".into(),
            font_size: None,
        },
    )
    .unwrap();
    let first = insert_events_from_source(
        &created.working_path,
        &created.revision,
        vtt.to_str().unwrap(),
        &[1],
    )
    .unwrap();
    assert_eq!(first.events[0].visible_text, "First line\\NSecond line");
    let second = insert_events_from_source(
        &created.working_path,
        &first.revision,
        ass.to_str().unwrap(),
        &[1],
    )
    .unwrap();
    assert_eq!(second.events[0].id, 2);
    assert_eq!(second.events[0].visible_text, "Tagged{1} text");
    let output = fs::read_to_string(&destination).unwrap();
    assert!(
        output
            .contains("Dialogue: 0,0:00:01.00,0:00:02.01,Default,,0,0,0,,First line\\NSecond line")
    );
    assert!(
        output
            .contains("Dialogue: 0,0:00:03.00,0:00:04.00,Default,,0,0,0,,{\\i1}Tagged{\\i0} text")
    );
    assert_eq!(fs::read_to_string(vtt).unwrap(), vtt_text);
    assert_eq!(fs::read_to_string(ass).unwrap(), ass_text);
}

#[test]
fn replace_segment_changes_only_one_visible_ass_span() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("segment.ass");
    let original = "[Events]\nFormat: Start, End, Style, Text\nDialogue: 0:00:01.00,0:00:02.00,Default,{\\i1}First {\\i0}target and tail\nDialogue: 0:00:03.00,0:00:04.00,Default,Other\n";
    fs::write(&path, original).unwrap();
    let name = path.to_str().unwrap();
    let before = get_event_with_context(name, 1, false, &[], 500).unwrap();
    assert_eq!(before.event.visible_text, "First {1}target and tail");
    replace_segment(name, 1, &before.revision, "target", "result").unwrap();
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        original.replace("{\\i0}target", "{\\i0}result")
    );
    let after = get_event_with_context(name, 1, false, &[], 500).unwrap();
    assert!(replace_segment(name, 1, &before.revision, "result", "again").is_err());
    assert!(replace_segment(name, 1, &after.revision, "First result", "Broken").is_err());
    assert!(replace_segment(name, 1, &after.revision, "i0", "i1").is_err());
    assert_eq!(after.event.visible_text, "First {1}result and tail");
}

#[test]
fn replace_segment_rejects_repeated_and_overlapping_matches() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("repeated.ass");
    let original = "[Events]\nFormat: Start, End, Style, Text\nDialogue: 0:00:01.00,0:00:02.00,Default,banana\n";
    fs::write(&path, original).unwrap();
    let name = path.to_str().unwrap();
    let before = get_event_with_context(name, 1, false, &[], 500).unwrap();
    assert!(replace_segment(name, 1, &before.revision, "ana", "x").is_err());
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
}

#[test]
fn replace_segment_rebuilds_supported_gradient() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("gradient_segment.ass");
    fs::write(&path, "[Events]\nFormat: Start, End, Style, Text\nDialogue: 0:00:01.00,0:00:02.00,Default,{\\c&HFFFFFF&}A{\\c&HAAAAAA&}B{\\c&H555555&}C{\\c&H000000&}D\n").unwrap();
    let name = path.to_str().unwrap();
    let before = get_event_with_context(name, 1, false, &[], 500).unwrap();
    replace_segment(name, 1, &before.revision, "BC", "XYZ").unwrap();
    let after = get_event_with_context(name, 1, true, &[], 500).unwrap();
    assert_eq!(after.event.visible_text, "AXYZD");
    assert!(after.event.raw_text.unwrap().ends_with("{\\c&H000000&}D"));
}

#[test]
fn replace_segment_edits_plain_srt_cue() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("segment.srt");
    let original = "1\r\n00:00:01,000 --> 00:00:02,000\r\nFirst line\r\nSecond line\r\n";
    fs::write(&path, original).unwrap();
    let name = path.to_str().unwrap();
    let before = get_event_with_context(name, 1, false, &[], 500).unwrap();
    replace_segment(name, 1, &before.revision, "Second", "Final").unwrap();
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        original.replace("Second line", "Final line")
    );
}

#[test]
fn batch_is_atomic_and_uses_one_revision() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("batch.ass");
    let original = "[Events]\r\nFormat: Layer, Start, End, Style, Text\r\nDialogue: 0,0:00:01.00,0:00:02.00,Default,First\r\nDialogue: 0,0:00:02.00,0:00:03.00,Default,{\\i1}Hello {\\i0}world\\Nagain\r\nDialogue: 0,0:00:03.00,0:00:04.00,Default,Last\r\n";
    fs::write(&path, original).unwrap();
    let name = path.to_str().unwrap();
    let revision = get_event_with_context(name, 1, false, &[], 500)
        .unwrap()
        .revision;
    let invalid = [
        TextEdit {
            id: 1,
            visible_text: "Primeira".into(),
        },
        TextEdit {
            id: 2,
            visible_text: "Olá mundo\\Nnovamente".into(),
        },
    ];
    assert!(replace_text_batch(name, &revision, &invalid).is_err());
    assert_eq!(fs::read_to_string(&path).unwrap(), original);

    let valid = [
        TextEdit {
            id: 2,
            visible_text: "Olá {1}mundo\\Nnovamente".into(),
        },
        TextEdit {
            id: 1,
            visible_text: "Primeira".into(),
        },
    ];
    let result = replace_text_batch(name, &revision, &valid).unwrap();
    assert_eq!(
        result
            .events
            .iter()
            .map(|event| event.id)
            .collect::<Vec<_>>(),
        vec![2, 1]
    );
    let expected = original
        .replace("First", "Primeira")
        .replace("Hello {\\i0}world\\Nagain", "Olá {\\i0}mundo\\Nnovamente");
    assert_eq!(fs::read_to_string(&path).unwrap(), expected);
    assert!(replace_text_batch(name, &revision, &valid).is_err());
    let duplicate = [
        TextEdit {
            id: 1,
            visible_text: "A".into(),
        },
        TextEdit {
            id: 1,
            visible_text: "B".into(),
        },
    ];
    assert!(replace_text_batch(name, &result.revision, &duplicate).is_err());
    assert_eq!(fs::read_to_string(&path).unwrap(), expected);
}

#[test]
fn simultaneous_edits_from_one_revision_cannot_overwrite_each_other() {
    use std::sync::{Arc, Barrier};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("concurrent.ass");
    fs::write(&path, "[Events]\nFormat: Start, End, Style, Text\nDialogue: 0:00:01.00,0:00:02.00,Default,Original\n").unwrap();
    let name = path.to_str().unwrap().to_owned();
    let revision = get_event_with_context(&name, 1, false, &[], 500)
        .unwrap()
        .revision;
    let barrier = Arc::new(Barrier::new(3));
    let handles = ["First", "Second"].map(|text| {
        let barrier = Arc::clone(&barrier);
        let name = name.clone();
        let revision = revision.clone();
        std::thread::spawn(move || {
            barrier.wait();
            replace_text(&name, 1, &revision, text).is_ok()
        })
    });
    barrier.wait();
    let results = handles.map(|handle| handle.join().unwrap());
    assert_eq!(results.into_iter().filter(|success| *success).count(), 1);
}

#[test]
fn context_alignment_is_many_to_many_and_conservative() {
    let dir = tempfile::tempdir().unwrap();
    let master = dir.path().join("master.ass");
    let context = dir.path().join("context.ass");
    fs::write(&master, "[Events]\nFormat: Start, End, Style, Text\nDialogue: 0:00:01.00,0:00:02.00,Default,First\nDialogue: 0:00:02.00,0:00:03.00,Default,Second\nDialogue: 0:00:04.00,0:00:05.00,Default,Third\n").unwrap();
    fs::write(&context, "[Events]\nFormat: Start, End, Style, Text\nDialogue: 0:00:01.00,0:00:03.00,Default,{\\i1}Whole\\Nsentence\nDialogue: 0:00:03.95,0:00:04.05,Default,Weak overlap\nDialogue: 0:00:05.05,0:00:05.15,Default,Nearby\n").unwrap();
    let tracks = [ContextTrack {
        name: "ja-JP".into(),
        path: context.to_str().unwrap().into(),
    }];
    let master_path = master.to_str().unwrap();
    for id in [1, 2] {
        let result = get_event_with_context(master_path, id, false, &tracks, 500).unwrap();
        let matches = &result.event.contexts.unwrap()["ja-JP"];
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].source_id, 1);
        assert_eq!(matches[0].text, "Whole sentence");
    }
    let third = get_event_with_context(master_path, 3, false, &tracks, 500).unwrap();
    assert!(third.event.contexts.unwrap()["ja-JP"].is_empty());
}

#[test]
fn static_color_gradient_is_rebuilt() {
    assert!(is_drawing("{\\p01}m 0 0 l 2 2{\\p0}Caption"));
    assert!(!is_drawing("{\\pos(10,20)}Caption"));
    let gradient = "{\\c&HFFFFFF&}A{\\c&HAAAAAA&}B{\\c&H555555&}C{\\c&H000000&}D";
    assert!(is_per_character_effect(gradient));
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("effect.ass");
    let original = format!(
        "[Events]\nFormat: Start, End, Style, Text\nDialogue: 0:00:01.00,0:00:02.00,Default,{gradient}\n"
    );
    fs::write(&path, &original).unwrap();
    let name = path.to_str().unwrap();
    let event = get_event_with_context(name, 1, false, &[], 500).unwrap();
    assert!(event.event.editable);
    assert_eq!(event.event.visible_text, "ABCD");
    assert_eq!(event.event.read_only_reason, None);
    replace_text(name, 1, &event.revision, "WXYZQ").unwrap();
    let updated = get_event_with_context(name, 1, true, &[], 500).unwrap();
    assert_eq!(updated.event.visible_text, "WXYZQ");
    let raw = updated.event.raw_text.unwrap();
    assert_eq!(raw.matches("\\c").count(), 5);
    assert!(raw.starts_with("{\\c&HFFFFFF&}W"));
    assert!(raw.ends_with("{\\c&H000000&}Q"));
    replace_text(name, 1, &updated.revision, "Novo").unwrap();
    let updated_again = get_event_with_context(name, 1, true, &[], 500).unwrap();
    assert_eq!(updated_again.event.visible_text, "Novo");
    let raw_again = updated_again.event.raw_text.unwrap();
    assert_eq!(raw_again.matches("\\c").count(), 4);
    assert!(raw_again.starts_with("{\\c&HFFFFFF&}N"));
    assert!(
        fs::read_to_string(path)
            .unwrap()
            .starts_with("[Events]\nFormat:")
    );
}

#[test]
fn gradient_inherits_first_character_color_from_event_style() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("inherited_color.ass");
    let original = concat!(
        "[V4+ Styles]\n",
        "Format: Fontname, Name, PrimaryColour\n",
        "Style: Lato,Sign,&H00F8F8F8\n",
        "[Events]\n",
        "Format: Start, End, Style, Text\n",
        "Dialogue: 0:00:01.00,0:00:02.00,Sign,",
        "{\\pos(960,500)}A",
        "{\\c&H000000&}B{\\c&H555555&}C",
        "{\\c&HAAAAAA&}D{\\c&HFFFFFF&}E\n"
    );
    fs::write(&path, original).unwrap();
    let name = path.to_str().unwrap();
    let before = get_event_with_context(name, 1, false, &[], 500).unwrap();
    assert!(before.event.editable);
    replace_text(name, 1, &before.revision, "VWXYZQ").unwrap();
    let after = get_event_with_context(name, 1, true, &[], 500).unwrap();
    let raw = after.event.raw_text.unwrap();
    assert!(raw.starts_with("{\\pos(960,500)}{\\c&HF8F8F8&}V"));
    assert!(!raw.starts_with("{\\pos(960,500)}{\\c&H000000&}"));
    assert!(raw.ends_with("{\\c&HFFFFFF&}Q"));
    assert!(after.event.editable);
}

#[test]
fn gradient_with_unknown_inherited_color_is_read_only() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("missing_style.ass");
    let original = concat!(
        "[Events]\nFormat: Start, End, Style, Text\n",
        "Dialogue: 0:00:01.00,0:00:02.00,Unknown,",
        "{\\pos(960,500)}A{\\c&H000000&}B",
        "{\\c&H555555&}C{\\c&HAAAAAA&}D{\\c&HFFFFFF&}E\n"
    );
    fs::write(&path, original).unwrap();
    let name = path.to_str().unwrap();
    let event = get_event_with_context(name, 1, false, &[], 500).unwrap();
    assert!(!event.event.editable);
    assert_eq!(
        event.event.read_only_reason,
        Some("unsupported_per_character_effect")
    );
    assert!(replace_text(name, 1, &event.revision, "VWXYZQ").is_err());
    assert_eq!(fs::read_to_string(path).unwrap(), original);
}

#[test]
fn gradient_uses_explicit_primary_color_alias_before_style() {
    let original = concat!(
        "{\\1c&HF8F8F8&}A{\\c&H000000&}B",
        "{\\c&H555555&}C{\\c&HAAAAAA&}D{\\c&HFFFFFF&}E"
    );
    let rebuilt = GradientPlan::parse(original)
        .unwrap()
        .rebuild("VWXYZ")
        .unwrap();
    assert!(rebuilt.starts_with("{\\1c&HF8F8F8&}V"));
    assert!(rebuilt.contains("{\\c&H000000&}W"));
}

#[test]
fn numeric_gradient_uses_keyframes_and_unsupported_transforms_stay_read_only() {
    let original = concat!(
        "{\\pos(960,411)\\xshad1}H",
        "{*\\xshad0.8}o{*\\xshad0.6}n{*\\xshad0.4}e",
        "{*\\xshad0.2}d {*\\xshad-0.2}S{*\\xshad-0.4}t",
        "{*\\xshad-0.6}o{*\\xshad-0.8}r{\\xshad-1}y"
    );
    let plan = GradientPlan::parse(original).unwrap();
    assert_eq!(plan.visible_text(), "Honed Story");
    let rebuilt = plan.rebuild("Wazamonogatari").unwrap();
    assert_eq!(
        rebuilt.matches("\\xshad").count(),
        "Wazamonogatari".chars().count()
    );
    assert!(rebuilt.contains("{\\xshad0.85}"));
    assert!(rebuilt.ends_with("{\\xshad-1}i"));
    assert!(!rebuilt.contains("{*\\xshad"));
    let transformed = "{\\frz307.9\\t(24,983,\\frz387.98)}I{\\frz303.23\\t(24,983,\\frz383.31)}t{\\frz299.69}s{\\frz296.29} ";
    assert!(is_per_character_effect(transformed));
    assert!(GradientPlan::parse(transformed).is_none());
    let generated = "{\\frz0}A{*\\frz100}B{*\\frz100}C{\\frz30}D";
    let rebuilt = GradientPlan::parse(generated)
        .unwrap()
        .rebuild("VWXYZ")
        .unwrap();
    assert!(rebuilt.contains("{\\frz7.5}W"));
}

#[test]
fn animated_gradient_preserves_timing_and_rebuilds_nested_values() {
    let original = concat!(
        "{\\frz0\\t(0,100,\\frz90)}A",
        "{\\frz10\\t(0,100,\\frz100)}B",
        "{\\frz20\\t(0,100,\\frz110)}C",
        "{\\frz30\\t(0,100,\\frz120)}D"
    );
    let plan = GradientPlan::parse(original).unwrap();
    assert_eq!(plan.visible_text(), "ABCD");
    let rebuilt = plan.rebuild("WXYZQ").unwrap();
    assert!(rebuilt.starts_with("{\\frz0}{\\t(0,100,\\frz90)}W"));
    assert!(rebuilt.contains("{\\frz7.5\\t(0,100,\\frz97.5)}X"));
    assert!(rebuilt.ends_with("{\\frz30\\t(0,100,\\frz120)}Q"));
    assert_eq!(rebuilt.matches("\\t(0,100,").count(), 5);
    let repeated = GradientPlan::parse(&rebuilt)
        .unwrap()
        .rebuild("Novo")
        .unwrap();
    assert_eq!(repeated.matches("\\t(0,100,").count(), 4);
    assert!(repeated.ends_with("{\\frz30\\t(0,100,\\frz120)}o"));
}

#[test]
fn header_transform_values_do_not_replace_static_gradient_stops() {
    let original = concat!(
        "{\\frz307.9\\t(24,983,\\frz387.98\\bord5)}A",
        "{\\frz327.9}B{\\frz347.9}C{\\frz367.9}D"
    );
    let plan = GradientPlan::parse(original).unwrap();
    let rebuilt = plan.rebuild("WXYZQ").unwrap();
    assert!(rebuilt.starts_with("{\\frz307.9\\t(24,983,\\frz387.98\\bord5)}W"));
    assert!(rebuilt.contains("{\\frz322.9}X"));
    assert!(rebuilt.ends_with("{\\frz367.9}Q"));
    assert_eq!(rebuilt.matches("\\t(24,983,").count(), 1);
}

#[test]
fn animated_gradient_is_editable_through_file_api() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("animated.ass");
    let original = concat!(
        "[Events]\nFormat: Start, End, Style, Text\n",
        "Dialogue: 0:00:01.00,0:00:02.00,Default,",
        "{\\frz0\\t(0,100,\\frz90)}A",
        "{\\frz10\\t(0,100,\\frz100)}B",
        "{\\frz20\\t(0,100,\\frz110)}C",
        "{\\frz30\\t(0,100,\\frz120)}D\n"
    );
    fs::write(&path, original).unwrap();
    let name = path.to_str().unwrap();
    let before = get_event_with_context(name, 1, false, &[], 500).unwrap();
    assert!(before.event.editable);
    assert_eq!(before.event.visible_text, "ABCD");
    replace_text(name, 1, &before.revision, "WXYZQ").unwrap();
    let after = get_event_with_context(name, 1, true, &[], 500).unwrap();
    assert!(after.event.editable);
    assert_eq!(after.event.visible_text, "WXYZQ");
    assert!(
        after
            .event
            .raw_text
            .unwrap()
            .contains("\\t(0,100,\\frz97.5)")
    );
}

#[test]
fn srt_multiline_edit_preserves_timing_bom_and_line_endings() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sample.srt");
    let original = "\u{feff}1\r\n00:00:01,000 --> 00:00:02,500\r\nFirst line\r\nSecond line\r\n\r\n2\r\n00:00:03,000 --> 00:00:04,000\r\nLater\r\n";
    fs::write(&path, original).unwrap();
    let name = path.to_str().unwrap();
    let before = get_event_with_context(name, 1, false, &[], 500).unwrap();
    assert_eq!(before.event.visible_text, "First line\nSecond line");
    assert!(before.event.editable);
    assert_eq!(
        serde_json::to_value(&before.event).unwrap()["format"],
        "srt"
    );
    assert!(replace_text(name, 1, &before.revision, "Bad\n\ncue").is_err());
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    let edited = replace_text(name, 1, &before.revision, "New\nWords").unwrap();
    assert!(edited.autosave_path.ends_with("Z.AUTOSAVE.srt"));
    assert_eq!(fs::read_to_string(&edited.autosave_path).unwrap(), original);
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        original.replace("First line\r\nSecond line", "New\r\nWords")
    );
}

#[test]
fn vtt_cues_and_mixed_format_contexts() {
    let dir = tempfile::tempdir().unwrap();
    let master = dir.path().join("master.ass");
    let srt = dir.path().join("context.srt");
    let vtt = dir.path().join("context.vtt");
    fs::write(&master, "[Events]\nFormat: Start, End, Style, Text\nDialogue: 0:00:01.00,0:00:02.00,Default,Master\n").unwrap();
    fs::write(&srt, "1\n00:00:01,000 --> 00:00:02,000\nSRT context\n").unwrap();
    let original_vtt = "WEBVTT - Sample\n\nNOTE internal note\nignored\n\nSTYLE\n::cue { color: white; }\n\ncue-1\n00:01.000 --> 00:02.000 align:start\nVTT context\n\n00:03.000 --> 00:04.000\n<i>Styled &amp; cue</i>\n";
    fs::write(&vtt, original_vtt).unwrap();
    let tracks = [
        ContextTrack {
            name: "srt".into(),
            path: srt.to_str().unwrap().into(),
        },
        ContextTrack {
            name: "vtt".into(),
            path: vtt.to_str().unwrap().into(),
        },
    ];
    let result = get_event_with_context(master.to_str().unwrap(), 1, false, &tracks, 500).unwrap();
    let contexts = result.event.contexts.unwrap();
    assert_eq!(contexts["srt"][0].text, "SRT context");
    assert_eq!(contexts["vtt"][0].text, "VTT context");
    let name = vtt.to_str().unwrap();
    let first = get_event_with_context(name, 1, false, &[], 500).unwrap();
    assert_eq!(serde_json::to_value(&first.event).unwrap()["format"], "vtt");
    assert!(first.event.editable);
    replace_text(name, 1, &first.revision, "Updated VTT").unwrap();
    assert_eq!(
        fs::read_to_string(&vtt).unwrap(),
        original_vtt.replace("VTT context", "Updated VTT")
    );
    let srt_track = [ContextTrack {
        name: "srt".into(),
        path: srt.to_str().unwrap().into(),
    }];
    let vtt_master = get_event_with_context(name, 1, false, &srt_track, 500).unwrap();
    assert_eq!(
        vtt_master.event.contexts.unwrap()["srt"][0].text,
        "SRT context"
    );
    let styled = get_event_with_context(name, 2, false, &[], 500).unwrap();
    assert!(!styled.event.editable);
    assert_eq!(styled.event.visible_text, "Styled & cue");
    assert!(replace_text(name, 2, &styled.revision, "Changed").is_err());
    let malformed = dir.path().join("missing_separator.vtt");
    fs::write(&malformed, "WEBVTT\n00:01.000 --> 00:02.000\nText\n").unwrap();
    assert!(Document::open(&malformed).is_err());
}

#[test]
fn inconsistent_animated_gradient_is_read_only() {
    let missing_transform = concat!(
        "{\\frz0\\t(0,100,\\frz90)}A",
        "{\\frz10\\t(0,100,\\frz100)}B",
        "{\\frz20}C",
        "{\\frz30\\t(0,100,\\frz120)}D"
    );
    assert!(is_per_character_effect(missing_transform));
    assert!(GradientPlan::parse(missing_transform).is_none());
    let changed_timing = missing_transform.replace("{\\frz20}C", "{\\frz20\\t(0,200,\\frz110)}C");
    assert!(GradientPlan::parse(&changed_timing).is_none());
}

#[test]
fn bom_before_events_section_is_parsed() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bom.ass");
    fs::write(&path, "\u{feff}[Events]\r\nFormat: Start, End, Style, Text\r\nDialogue: 0:00:01.00,0:00:02.00,Default,Visible\r\n").unwrap();
    let document = Document::open(&path).unwrap();
    assert_eq!(document.events.len(), 1);
    assert_eq!(document.events[0].raw_text, "Visible");
}

#[test]
fn real_fixture_is_readable() {
    let path = ass_fixture();
    let doc = Document::open(&path).unwrap();
    assert!(doc.events.len() > 100);
    assert!(!doc.events[0].raw_text.is_empty());
}

#[test]
fn edit_supported_gradient_in_copy_of_real_fixture() {
    let source = ass_fixture();
    let source_doc = Document::open(&source).unwrap();
    let event = source_doc
        .events
        .iter()
        .find(|event| {
            GradientPlan::parse(&event.raw_text)
                .is_some_and(|plan| !plan.visible_text().contains("\\N"))
        })
        .expect("supported gradient in real fixture");
    let dir = tempfile::tempdir().unwrap();
    let copy = dir.path().join("gradient.ass");
    fs::copy(&source, &copy).unwrap();
    let name = copy.to_str().unwrap();
    let before = get_event_with_context(name, event.id, false, &[], 500).unwrap();
    assert!(before.event.editable);
    replace_text(name, event.id, &before.revision, "Teste").unwrap();
    let after = get_event_with_context(name, event.id, false, &[], 500).unwrap();
    assert_eq!(after.event.visible_text, "Teste");
    assert_eq!(fs::read(&source).unwrap(), source_doc.content.as_bytes());
    let edited = fs::read_to_string(&copy).unwrap();
    assert_eq!(
        &edited[..event.text_range.start],
        &source_doc.content[..event.text_range.start]
    );
    assert!(edited.ends_with(&source_doc.content[event.text_range.end..]));
    let policy = AuditPolicy {
        allowed_fields: vec!["Text".into()],
        allowed_event_ids: vec![event.id],
        allow_non_dialogue_changes: false,
        allow_event_count_changes: false,
        allow_reordering: false,
        allow_gradient_rebuild: true,
    };
    let report = serde_json::to_value(
        audit_subtitle(
            source.to_str().unwrap(),
            name,
            &source_doc.revision,
            &hash(edited.as_bytes()),
            &policy,
            0,
            20,
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(report["total_changed_events"], 1);
    assert_eq!(report["differences"][0]["gradient_rebuilt"], true);
    assert!(report["policy_violations"].as_array().unwrap().is_empty());
}

#[test]
fn edit_copy_of_real_fixture() {
    let source = ass_fixture();
    let original = fs::read(&source).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let copy = dir.path().join("copy.ass");
    fs::write(&copy, &original).unwrap();
    let name = copy.to_str().unwrap();
    let before = get_event_with_context(name, 1, false, &[], 500).unwrap();
    let replacement = before.event.visible_text.replacen("Higasa", "Test", 1);
    assert_ne!(replacement, before.event.visible_text);
    replace_text(name, 1, &before.revision, &replacement).unwrap();
    let edited = fs::read(&copy).unwrap();
    assert_eq!(fs::read(&source).unwrap(), original);
    let source_doc = Document::open(&source).unwrap();
    let first = &source_doc.events[0];
    let mut expected = source_doc.content[..first.text_range.start].to_owned();
    expected.push_str(&first.raw_text.replacen("Higasa", "Test", 1));
    expected.push_str(&source_doc.content[first.text_range.end..]);
    assert_eq!(hash(&edited), hash(expected.as_bytes()));
    let policy = AuditPolicy {
        allowed_fields: vec!["Text".into()],
        allowed_event_ids: vec![1],
        allow_non_dialogue_changes: false,
        allow_event_count_changes: false,
        allow_reordering: false,
        allow_gradient_rebuild: false,
    };
    let report = serde_json::to_value(
        audit_subtitle(
            source.to_str().unwrap(),
            name,
            &source_doc.revision,
            &hash(&edited),
            &policy,
            0,
            20,
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(report["total_changed_events"], 1);
    assert_eq!(report["non_dialogue_changed"], false);
    assert_eq!(report["event_order_status"], "unchanged_by_non_text_fields");
    assert!(report["policy_violations"].as_array().unwrap().is_empty());
}

#[test]
fn groups_identical_text_across_styles_and_applies_once() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("groups.ass");
    let original = "[Events]\nFormat: Start, End, Style, Text\nDialogue: 0:00:01.00,0:00:02.00,Default,Hello\nDialogue: 0:00:02.00,0:00:03.00,Default,Other\nDialogue: 0:00:03.00,0:00:04.00,Sign,{\\fnMarker}Hello\nDialogue: 0:00:04.00,0:00:05.00,Sign,{\\i1}Hello{\\i0}\n";
    fs::write(&path, original).unwrap();
    let name = path.to_str().unwrap();
    let groups = list_text_groups(name, 0, 50, None).unwrap();
    assert_eq!(groups.total_events, 4);
    assert_eq!(groups.total_groups, 2);
    assert_eq!(groups.groups[0].source_text, "Hello");
    assert_eq!(groups.groups[0].occurrences, 3);
    assert!(groups.groups[0].automatic_apply);
    assert_eq!(
        groups.groups[0]
            .members
            .iter()
            .map(|member| member.id)
            .collect::<Vec<_>>(),
        vec![1, 3, 4]
    );
    let changed = replace_text_group(name, 1, &groups.revision, "Hello", "Olá").unwrap();
    assert_eq!(changed.events.len(), 3);
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        original
            .replace(",Hello\n", ",Olá\n")
            .replace("}Hello", "}Olá")
    );
    assert!(replace_text_group(name, 1, &groups.revision, "Hello", "Again").is_err());
}

#[test]
fn group_edit_rejects_ambiguous_internal_tags_without_writing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("groups.ass");
    let original = "[Events]\nFormat: Start, End, Style, Text\nDialogue: 0:00:01.00,0:00:02.00,Default,Hello\nDialogue: 0:00:03.00,0:00:04.00,Sign,Hel{\\c&HFFFFFF&}lo\n";
    fs::write(&path, original).unwrap();
    let name = path.to_str().unwrap();
    let groups = list_text_groups(name, 0, 50, None).unwrap();
    assert_eq!(groups.groups.len(), 1);
    assert!(!groups.groups[0].automatic_apply);
    assert_eq!(
        groups.groups[0].members[1].group_apply_reason,
        Some("internal_override_tags")
    );
    assert!(replace_text_group(name, 1, &groups.revision, "Hello", "Olá").is_err());
    assert_eq!(fs::read_to_string(path).unwrap(), original);
}
