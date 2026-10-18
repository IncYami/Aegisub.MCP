# Aegisub MCP

A local MCP server written in Rust for reading and editing ASS `Dialogue` events and simple SRT/WebVTT cues. ASS remains the primary format. The server shows subtitle text to the AI while hiding ASS override blocks. Before editing an existing subtitle in place, it saves the original bytes in a timestamped `.backup` file. No persistent intermediate JSON files are needed.

## Code layout

| Module | Responsibility |
| --- | --- |
| `src/subtitle/ass.rs` | Parse ASS, locate events, and separate visible text from override blocks while preserving the original bytes. |
| `src/subtitle/query.rs` | Build event views, search results, and paginated responses. |
| `src/subtitle/edit.rs` | Validate replacements and atomically write individual or batch edits. |
| `src/subtitle/automation/gradient.rs` | Detect per-character effects and rebuild supported static and animated gradients. |
| `src/subtitle/context.rs` | Align external context tracks with the master track by time. |
| `src/subtitle/group.rs` | Group exact visible text across a subtitle and apply one translation to every safe occurrence. |
| `src/subtitle/timed_text.rs` | Parse simple SRT and WebVTT cues while retaining their byte ranges. |
| `src/subtitle/backup.rs` | Create a timestamped backup before registering an existing subtitle for in-place edits. |
| `src/subtitle/ass_creation.rs` | Create an empty ASS script with a configurable base style. |
| `src/subtitle/event_import.rs` | Copy source subtitle cues into ASS Dialogue events. |
| `src/subtitle/font_collect.rs` | Find installed ASS fonts, check glyph and style availability, and copy original files with organized names. |
| `src/subtitle.rs` | Shared types and interfaces between subtitle modules. |
| `src/mcp.rs` | Expose operations as MCP tools. |
| `src/main.rs` | Start the MCP server over `stdio`. |

The ASS core does not depend on MCP. Typesetting operations act on ASS events, while project-specific features such as context alignment stay separate.

## Run

```sh
nix-shell
cargo build
cargo run
```

`cargo run` starts the MCP server over `stdio`. Configure an MCP client to run `target/debug/aegisub-mcp` or `nix-shell --run 'cargo run --quiet'` from this directory. The server reserves stdout for the MCP protocol.

## Version control

The `main` branch tracks the Rust source, Nix shell, Cargo manifests, documentation, project instructions, and the neutral-named ASS test fixture in `tests/fixtures/sample.ass`. Build output, Nix results, backup directories, personal `rev.txt`, and root-level subtitle files are ignored. Subtitle files are excluded from Git line-ending normalization so the fixture keeps its original bytes.

Before committing Rust changes, run `nix-shell --run 'cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test'`.

## Available tools

- `prepare_subtitle`: back up an existing ASS, SSA, SRT, or WebVTT subtitle before editing it in place. Pass `source_path`. The tool creates `.backup/` beside the subtitle and saves a file named like `episode.2026-10-08T14-30-15.123456789Z.ass`. It returns `editable_path`, `backup_path`, and the current revision. Existing backups are never overwritten.
- `create_ass`: create an empty `.ass` with a complete base script and style. Pass a new `destination_path` and optionally `title`, `play_res_x`, `play_res_y`, `font_name`, and `font_size`. Existing files are never overwritten. The result includes `working_path` and its initial revision.
- `get_style_guide`: read the project style guide through MCP before choosing a font or planning typesetting. It is read-only and returns the versioned guide in `docs/subtitle-style-guide.md`.
- `collect_fonts`: inspect fonts used by an ASS subtitle and copy their installed files. Pass `ass_path` and optionally `destination_path`. By default, it creates a `fonts/` directory beside the ASS. It reads style fonts and inline `\\fn`, `\\b`, and `\\i` overrides, checks characters against fontconfig coverage, and reports missing fonts, glyphs, or requested bold/italic faces in structured warnings. It copies each selected TTF, OTF, TTC, or OTC file unchanged, including variable fonts and collections. Files use organized names such as `Lato-Bold.ttf` or `Inter-Variable.ttf`. Existing files are never overwritten; a content hash is added to the name if a different file already uses it.
- `insert_events_from_source`: insert source events into an ASS working file. Pass `destination_path`, its current `expected_revision`, `source_path`, and one or more `source_ids`. The source may be ASS, SRT, or WebVTT. The source is never changed; all requested events are inserted together or none are inserted.
- `list_events`: paginate ASS events or SRT/WebVTT cues and search visible text.
- `list_text_groups`: paginate exact visible-text groups across an entire ASS, SRT, or WebVTT file. Each distinct text appears once, including single occurrences. Members retain their IDs, times, and styles; grouping ignores ASS override tags and does not require nearby lines. The result indicates whether one translation can be applied automatically to every member.
- `get_event`: read an event or cue by its 1-based ID. Source text is returned only with `include_raw: true`.
- `replace_text_group`: apply one translation to every occurrence returned in a text group. Pass `path`, `group_id`, `source_text`, `translated_text`, and `expected_revision` from `list_text_groups`. All edits are validated and written atomically to a registered editable subtitle. Leading and trailing ASS tags are preserved; supported per-character gradients are rebuilt. Groups with internal tags that need a translated position are returned for review but cannot be applied automatically.
- `replace_text`: replace visible text using the revision returned by a read. In ASS, internal override blocks appear as `{1}`, `{2}`, etc.; keep each marker once and in the same order. For supported per-character gradients, the tool interpolates tags automatically. Plain SRT/WebVTT cues can contain multiple text lines.
- `replace_segment`: replace one unique piece of text inside an event or cue without resending its full text. It requires `path`, `id`, `expected_revision`, `old_text`, and `new_text`. In regular ASS events, the matched text must stay within one visible text segment and cannot cross an override block. Supported gradients are rebuilt after replacement. Repeated matches are rejected.
- `replace_text_batch`: validate and replace several events or cues in one file under one revision. If any edit fails, the file is left untouched. Duplicate IDs are rejected.

To edit an existing subtitle, call `prepare_subtitle` and use its `editable_path` for subsequent operations. The original path remains the active subtitle; its pre-edit contents are preserved in `backup_path`. To start a blank ASS project, call `create_ass` and use its `working_path`. After a server restart, call `prepare_subtitle` again before editing an existing file; this creates another dated backup of its current contents. Files inside `.backup` cannot be prepared for editing.

## Creating an ASS base

Call `create_ass` with a new output path. No source subtitle is required:

```json
{
  "destination_path": "/path/to/translation.pt-BR.ass",
  "title": "Translation",
  "play_res_x": 1920,
  "play_res_y": 1080
}
```

The base ASS contains `[Script Info]`, `[V4+ Styles]`, and `[Events]`, with no `Dialogue` events. Its default title is `Untitled` and default script resolution is 1920×1080; set both resolution fields to the video's native dimensions when known. At 1920×1080, the exact `Default` style fields are:

```ass
Style: Default,Lato,80,&H00F8F8F8,&H000000FF,&H00000000,&H96000000,-1,0,0,0,100,100,0,0,1,3.75,2.25,2,120,120,60,1
```

This gives near-white bold text, a black 3.75-pixel outline, a partially transparent black shadow offset by 2.25 pixels, bottom-center alignment, and 120/120/60 margins. Font size, outline, shadow, and margins scale with script resolution; `font_name` and `font_size` can be overridden during creation. `Outline` and `Shadow` remain decimal values in the generated file. The script declares `ScaledBorderAndShadow: yes` and `YCbCr Matrix: None`; adjust the matrix when the video's color matrix is known.

Lato is the default for the translated subtitle. It is available from [Google Fonts](https://fonts.google.com/specimen/Lato) under the SIL Open Font License. Its [published character sets](https://github.com/google/fonts/blob/main/ofl/lato/METADATA.pb) cover Latin, Greek, and Cyrillic, but not Japanese. Japanese text imported during translation may be shown through the renderer's installed CJK font fallback, such as [Noto Sans CJK JP](https://github.com/notofonts/noto-cjk); this depends on the viewer's fonts and is not encoded as a second style in the ASS. Install Lato for predictable final rendering and a CJK font if you need to preview untranslated Japanese cues. Use `font_name` to choose a different primary font for another target language. Creating the ASS does not download or embed font files.

## Recommended styling and typesetting

Keep every Dialogue event on `Default`. Use local override tags in the event text when a sign, note, title, or other on-screen element needs a different font or appearance. The [ASS subtitle style guide](docs/subtitle-style-guide.md) suggests Google Fonts such as Permanent Marker for Latin-script marker lettering, Tinos for a Times-like serif, and Japanese-capable fonts for Japanese text. It also explains font coverage, rendering checks, and the current MCP tag-editing limitation. MCP clients can retrieve the same guide with `get_style_guide`; no extra styles are added to generated ASS files.

## Collecting fonts for a project

After the ASS has its final fonts, call `collect_fonts`:

```json
{
  "ass_path": "/path/to/translation.pt-BR.ass",
  "destination_path": "/path/to/project/fonts"
}
```

The destination is optional; without it, the tool uses `fonts/` beside the ASS. On NixOS, it uses fontconfig's installed-font index, so fonts in the Nix store can be copied into the project. The result lists required family names, each copied or already-present file, and warnings about missing fonts, glyphs, or requested bold/italic faces. The tool names files from font metadata and copies original bytes. Collections and variable fonts remain collections and variable fonts. It matches installed family or full names exactly instead of silently accepting fontconfig substitutions. The operation does not convert, subset, or embed fonts in an MKV; the project's muxing workflow can handle those steps. Font licenses still apply when distributing copied files.

`create_ass` creates the base document. To pull lines from a Japanese SRT into it, call `list_events` on the SRT, then `insert_events_from_source` with the returned cue IDs and the new ASS revision:

```json
{
  "destination_path": "/path/to/translation.pt-BR.ass",
  "expected_revision": "<revision from create_ass>",
  "source_path": "/path/to/source.ja-JP.srt",
  "source_ids": [1, 2, 3]
}
```

The tool copies each cue's timing and text to a new `Dialogue` event using the destination's `Default` style. It converts physical SRT/WebVTT line breaks to ASS `\N` and rounds start times down and end times up to centiseconds. Plain timed-text cues are supported; inline markup and entities are rejected so they are not silently lost. ASS override text is retained, but source styles and other sections are not copied. Events are appended in the requested order. Use the returned revision with `replace_text` or `replace_text_batch` to translate them. The original subtitle stays unchanged and may also be used as a context track.

`list_events` and `get_event` accept optional ASS, SRT, or WebVTT context tracks. The main file may use any of these formats. Example `get_event` arguments:

```json
{
  "path": "/path/to/master.ass",
  "id": 1,
  "contexts": [
    { "name": "ja-JP", "path": "/path/to/japanese.ass" },
    { "name": "old-pt-BR", "path": "/path/to/previous.ass" }
  ],
  "proximity_ms": 500
}
```

Alignment accepts matching intervals or substantial overlaps. Without overlap, it uses proximity only when one candidate is clearly closer. Context occurrences remain separate even when their text is identical.

The server preserves all unrelated file bytes, including BOM, line endings, styles, comments, cue identifiers, and attachments. Writes use a temporary file in the same directory. Edit tools reject unregistered files; an existing subtitle becomes editable only after its backup succeeds. Backups are stored in UTC with nanosecond timestamps and cannot be edited through the MCP. Non-UTF-8 files, ASS `\p` drawings, unsupported per-character effects, and ASS edits that remove `\N`, `\n`, or `\h` escapes are rejected. SRT/WebVTT cues with inline markup or character entities are read-only in this initial support; plain cue text is editable. WebVTT files with `X-TIMESTAMP-MAP` are rejected because their cue times require an additional mapping.

Supported gradients use numeric, color, or alpha tags. Animated gradients are editable only when every character has a matching `\t(...)` transform with consistent timing and tag names. Other transform patterns remain read-only.

Gradient reconstruction uses the event style color when the first character has no explicit color tag. If that inherited value cannot be resolved safely, the event is read-only.

## Project status

Event reading, individual and batch editing, exact repeated-text grouping, empty ASS creation, source-event insertion, mixed-format temporal context alignment, and supported per-character gradient reconstruction are implemented. Remaining work includes creating events without a source subtitle, more typesetting effects, inline markup editing for SRT/WebVTT, and visual rendering checks. The real-world ASS fixture in `tests/fixtures/sample.ass` is used only through temporary copies in editing tests.

Syntax references: [libass ASS format guide](https://github.com/libass/libass/wiki/ASS-File-Format-Guide) and [Aegisub override tag manual](https://aegisub.org/docs/latest/ass_tags/).
