use super::{Document, Event, Result, SubtitleFormat, error, timed_text};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs, path::Path, sync::Arc};

fn style_color(value: &str) -> Option<(String, String)> {
    let hex = value
        .strip_prefix("&H")
        .or_else(|| value.strip_prefix("&h"))?
        .trim_end_matches('&');
    if hex.len() != 8 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    Some((
        format!("&H{}&", hex[2..].to_ascii_uppercase()),
        format!("&H{}&", hex[..2].to_ascii_uppercase()),
    ))
}

fn style_defaults(content: &str) -> BTreeMap<String, Arc<BTreeMap<String, String>>> {
    let mut styles = BTreeMap::new();
    let mut in_styles = false;
    let mut format = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim().trim_start_matches('\u{feff}');
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_styles = trimmed.eq_ignore_ascii_case("[V4+ Styles]");
            format.clear();
            continue;
        }
        if !in_styles {
            continue;
        }
        let Some((kind, value)) = line.split_once(':') else {
            continue;
        };
        if kind.trim().eq_ignore_ascii_case("Format") {
            format = value
                .split(',')
                .map(|field| field.trim().to_owned())
                .collect();
        } else if kind.trim().eq_ignore_ascii_case("Style") && !format.is_empty() {
            let fields: Vec<&str> = value.split(',').map(str::trim).collect();
            if fields.len() != format.len() {
                continue;
            }
            let field = |wanted: &str| -> Option<&str> {
                let index = format
                    .iter()
                    .position(|name| name.eq_ignore_ascii_case(wanted))?;
                fields.get(index).copied()
            };
            let Some(name) = field("Name").filter(|name| !name.is_empty()) else {
                continue;
            };
            let mut defaults = BTreeMap::new();
            for (field_name, color_tag, alpha_tag) in [
                ("PrimaryColour", "1c", "1a"),
                ("SecondaryColour", "2c", "2a"),
                ("OutlineColour", "3c", "3a"),
                ("BackColour", "4c", "4a"),
            ] {
                if let Some((color, alpha)) = field(field_name).and_then(style_color) {
                    defaults.insert(color_tag.to_owned(), color.clone());
                    defaults.insert(alpha_tag.to_owned(), alpha);
                    if color_tag == "1c" {
                        defaults.insert("c".to_owned(), color);
                    }
                }
            }
            styles.insert(name.to_ascii_lowercase(), Arc::new(defaults));
        }
    }
    styles
}

pub(super) fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

impl Document {
    pub(super) fn open(path: &Path) -> Result<Self> {
        let bytes =
            fs::read(path).map_err(|e| error(format!("Cannot read {}: {e}", path.display())))?;
        let revision = hash(&bytes);
        let content = String::from_utf8(bytes)
            .map_err(|_| error("File is not UTF-8; editing unsupported"))?;
        let extension = path
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if extension == "srt" || extension == "vtt" {
            let format = if extension == "srt" {
                SubtitleFormat::Srt
            } else {
                SubtitleFormat::Vtt
            };
            let events = timed_text::parse(&content, format)?;
            return Ok(Self {
                content,
                events,
                revision,
            });
        }
        if !matches!(extension.as_str(), "ass" | "ssa") {
            return Err(error(
                "Supported subtitle extensions: .ass, .ssa, .srt, .vtt",
            ));
        }
        let mut events = Vec::new();
        let styles = style_defaults(&content);
        let mut in_events = false;
        let mut format: Vec<String> = Vec::new();
        let mut offset = 0;

        for full_line in content.split_inclusive('\n') {
            let line = full_line.trim_end_matches(['\r', '\n']);
            let trimmed = line.trim().trim_start_matches('\u{feff}');
            if trimmed.starts_with('[') && trimmed.ends_with(']') {
                in_events = trimmed.eq_ignore_ascii_case("[Events]");
                format.clear();
            } else if in_events && let Some((name, data)) = line.split_once(':') {
                if name.trim().eq_ignore_ascii_case("Format") {
                    format = data
                        .split(',')
                        .map(|field| field.trim().to_owned())
                        .collect();
                } else if name.trim().eq_ignore_ascii_case("Dialogue") && !format.is_empty() {
                    if !format
                        .last()
                        .is_some_and(|field| field.eq_ignore_ascii_case("Text"))
                    {
                        return Err(error("ASS Events Format must end with Text"));
                    }
                    let fields: Vec<&str> = data.splitn(format.len(), ',').collect();
                    if fields.len() != format.len() {
                        return Err(error(format!("Malformed Dialogue event at byte {offset}")));
                    }
                    let field = |wanted: &str| -> String {
                        format
                            .iter()
                            .position(|name| name.eq_ignore_ascii_case(wanted))
                            .map(|i| fields[i].trim().to_owned())
                            .unwrap_or_default()
                    };
                    let mut commas = 0;
                    let mut text_start = None;
                    for (index, ch) in data.char_indices() {
                        if ch == ',' {
                            commas += 1;
                            if commas == format.len() - 1 {
                                text_start = Some(offset + name.len() + 1 + index + 1);
                                break;
                            }
                        }
                    }
                    let start =
                        text_start.ok_or_else(|| error("Malformed Dialogue field boundaries"))?;
                    let end = offset + line.len();
                    let style = field("Style");
                    events.push(Event {
                        id: events.len() + 1,
                        format: SubtitleFormat::Ass,
                        start: field("Start"),
                        end: field("End"),
                        style_defaults: styles.get(&style.to_ascii_lowercase()).cloned(),
                        style,
                        raw_text: content[start..end].to_owned(),
                        text_range: start..end,
                    });
                }
            }
            offset += full_line.len();
        }
        Ok(Self {
            content,
            events,
            revision,
        })
    }
}

pub(super) fn is_drawing(raw: &str) -> bool {
    let mut rest = raw;
    while let Some(start) = rest.find('{') {
        let after = &rest[start + 1..];
        let Some(end) = after.find('}') else { break };
        let block = &after[..end];
        let mut tags = block;
        while let Some(position) = tags.find("\\p") {
            let value = &tags[position + 2..];
            let digits = value.bytes().take_while(u8::is_ascii_digit).count();
            if digits > 0 && value[..digits].bytes().any(|digit| digit != b'0') {
                return true;
            }
            tags = value;
        }
        rest = &after[end + 1..];
    }
    false
}

pub(super) fn split_tags(raw: &str) -> Result<(String, Vec<String>)> {
    let mut visible = String::new();
    let mut tags = Vec::new();
    let mut rest = raw;
    let mut seen_text = false;
    while let Some(start) = rest.find('{') {
        let before = &rest[..start];
        visible.push_str(before);
        if !before.is_empty() {
            seen_text = true;
        }
        let after = &rest[start + 1..];
        let Some(end) = after.find('}') else {
            return Err(error("Unclosed ASS override block"));
        };
        let block = &rest[start..start + end + 2];
        if seen_text {
            tags.push(block.to_owned());
            visible.push_str(&format!("{{{}}}", tags.len()));
        }
        rest = &after[end + 1..];
    }
    visible.push_str(rest);
    Ok((visible, tags))
}

pub(super) fn strip_blocks(raw: &str) -> Option<String> {
    let mut text = String::new();
    let mut rest = raw;
    while let Some(start) = rest.find('{') {
        text.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let end = after.find('}')?;
        rest = &after[end + 1..];
    }
    text.push_str(rest);
    Some(text)
}
