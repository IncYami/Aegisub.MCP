# Aegisub.MCP

This project implements a Rust MCP server for reading and editing subtitles directly, without persistent intermediate JSON files. ASS is the primary format; SRT and WebVTT have simple cue support.

## Environment

- Enter the environment with `nix-shell` before building or testing.
- Run `cargo fmt`, `cargo clippy`, and `cargo test` after Rust code changes.
- Keep ASS parsing and editing logic separate from the MCP layer.
- Keep code comments, documentation, tool descriptions, and project messages in en-US. Chat with the user in pt-BR.

## Git and versioning

- After completing any project change, commit it and push the updated `main` branch to `origin`. Do not leave completed changes only in the local working tree.
- Every commit that changes Rust application code must also increase the package version in `Cargo.toml` and update `Cargo.lock` in the same commit. Use semantic versioning: increase the patch number for fixes or internal changes, the minor number for new functionality while the project is in `0.x`, and the major number for incompatible changes after `1.0.0`.
- Documentation-only, instruction-only, and Nix packaging-only changes do not require an application version increase.

## Layout

- `src/subtitle/ass.rs` handles ASS syntax and preservation of the original structure.
- `src/subtitle/query.rs` presents events for reading; `edit.rs` validates and writes changes.
- `src/subtitle/automation/` contains typesetting transformations, including static and animated per-character gradient reconstruction.
- `src/subtitle/context.rs` aligns external tracks by time.
- `src/subtitle/group.rs` groups exact visible text across all events, independent of style or timing, and applies one translation atomically to safe group members.
- `src/subtitle/timed_text.rs` parses SRT and WebVTT cues.
- `src/subtitle/backup.rs` creates timestamped backups in a sibling `.backup` directory before the MCP layer authorizes edits to an existing subtitle.
- `src/subtitle/ass_creation.rs` creates empty ASS working files with a configurable base script and Default style.
- `src/subtitle/event_import.rs` inserts source subtitle events into an ASS working file without changing the source.
- `src/subtitle/font_collect.rs` tracks font usage in ASS styles and inline tags, inspects installed faces, reports missing glyphs and bold/italic variants, and copies original font files with organized names without overwriting existing files. Do not convert, subset, or split font files in the MCP.
- `src/mcp.rs` defines the tool interface and adapts its arguments to the ASS core.
- `docs/subtitle-style-guide.md` is the versioned styling guidance returned by the `get_style_guide` MCP tool. Keep generated ASS events on `Default`; use local override tags for typesetting.

## Subtitle integrity

- Preserve unrelated sections, order, fields, styles, comments, attachments, encoding, BOM, and line endings.
- Read field order from `Format:`; do not assume fixed positions.
- Separate visible text from ASS tags, escapes, and drawing segments. Do not discard unknown tags.
- If an edit cannot preserve the structure safely, return an explicit error instead of writing an approximation.
- Re-read the file and detect concurrent changes before writing. Validate an entire batch before writing any edit.
- Do not overwrite `tests/fixtures/sample.ass`. Use temporary copies in tests.
- Require a successful backup before registering an existing subtitle for in-place MCP edits. Edit tools must accept only files registered in the current server session. Never edit a file inside `.backup`.

## References

- `tests/fixtures/sample.ass` is a real-world test fixture committed under a neutral filename.
- Consult the libass ASS format guide and Aegisub override tag manual when making syntax or rendering decisions.
