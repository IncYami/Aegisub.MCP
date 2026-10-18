use super::{Event, Result, SubtitleFormat, error};

struct Line<'a> {
    text: &'a str,
    start: usize,
    end: usize,
}

fn lines(content: &str) -> Vec<Line<'_>> {
    let mut result = Vec::new();
    let bytes = content.as_bytes();
    let mut start = 0;
    while start < bytes.len() {
        let mut end = start;
        while end < bytes.len() && bytes[end] != b'\r' && bytes[end] != b'\n' {
            end += 1;
        }
        result.push(Line {
            text: &content[start..end],
            start,
            end,
        });
        start = if bytes.get(end) == Some(&b'\r') && bytes.get(end + 1) == Some(&b'\n') {
            end + 2
        } else if end < bytes.len() {
            end + 1
        } else {
            end
        };
    }
    result
}

pub(super) fn timestamp_ms(value: &str, format: SubtitleFormat) -> Option<u64> {
    let separator = if format == SubtitleFormat::Srt {
        ','
    } else {
        '.'
    };
    let (clock, milliseconds) = value.split_once(separator)?;
    if milliseconds.len() != 3 || !milliseconds.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let parts: Vec<&str> = clock.split(':').collect();
    if !(parts.len() == 3 || (format == SubtitleFormat::Vtt && parts.len() == 2)) {
        return None;
    }
    if parts
        .iter()
        .any(|part| !part.bytes().all(|byte| byte.is_ascii_digit()))
    {
        return None;
    }
    let (hours, minutes, seconds) = if parts.len() == 3 {
        if parts[0].len() < 2 || parts[1].len() != 2 || parts[2].len() != 2 {
            return None;
        }
        (parts[0].parse::<u64>().ok()?, parts[1], parts[2])
    } else {
        if parts[0].len() != 2 || parts[1].len() != 2 {
            return None;
        }
        (0, parts[0], parts[1])
    };
    let minutes = minutes.parse::<u64>().ok()?;
    let seconds = seconds.parse::<u64>().ok()?;
    if minutes >= 60 || seconds >= 60 {
        return None;
    }
    hours
        .checked_mul(3_600_000)?
        .checked_add(minutes.checked_mul(60_000)?)?
        .checked_add(seconds.checked_mul(1_000)?)?
        .checked_add(milliseconds.parse::<u64>().ok()?)
}

fn timing(line: &str, format: SubtitleFormat) -> Option<(&str, &str)> {
    let (start, right) = line.split_once("-->")?;
    let start = start.trim();
    let end = if format == SubtitleFormat::Vtt {
        right.split_whitespace().next()?
    } else {
        right.trim()
    };
    let start_ms = timestamp_ms(start, format)?;
    let end_ms = timestamp_ms(end, format)?;
    (end_ms > start_ms).then_some((start, end))
}

pub(super) fn parse(content: &str, format: SubtitleFormat) -> Result<Vec<Event>> {
    let lines = lines(content);
    let mut blocks = Vec::new();
    let mut cursor = 0;
    while cursor < lines.len() {
        while cursor < lines.len() && lines[cursor].text.trim().is_empty() {
            cursor += 1;
        }
        if cursor == lines.len() {
            break;
        }
        let start = cursor;
        while cursor < lines.len() && !lines[cursor].text.trim().is_empty() {
            cursor += 1;
        }
        blocks.push(&lines[start..cursor]);
    }
    if format == SubtitleFormat::Vtt {
        let Some(first) = blocks.first() else {
            return Err(error("WebVTT header is missing"));
        };
        let header = first[0].text.trim_start_matches('\u{feff}');
        if header != "WEBVTT" && !header.starts_with("WEBVTT ") && !header.starts_with("WEBVTT\t") {
            return Err(error("WebVTT header is missing"));
        }
        if first.iter().any(|line| line.text.contains("-->")) {
            return Err(error("WebVTT header must be separated from cues"));
        }
        if first
            .iter()
            .any(|line| line.text.starts_with("X-TIMESTAMP-MAP"))
        {
            return Err(error("WebVTT X-TIMESTAMP-MAP is unsupported"));
        }
    }
    let mut events = Vec::new();
    for (block_index, block) in blocks.iter().enumerate() {
        if format == SubtitleFormat::Vtt {
            if block_index == 0 {
                continue;
            }
            let label = block[0].text;
            if label == "NOTE"
                || label.starts_with("NOTE ")
                || label.starts_with("NOTE\t")
                || matches!(label, "STYLE" | "REGION")
            {
                continue;
            }
        }
        let first = block[0].text.trim_start_matches('\u{feff}');
        let timing_index = if first.contains("-->") { 0 } else { 1 };
        if format == SubtitleFormat::Srt
            && timing_index == 1
            && !first.bytes().all(|b| b.is_ascii_digit())
        {
            return Err(error(format!("Invalid SRT cue at byte {}", block[0].start)));
        }
        let timing_line = block
            .get(timing_index)
            .ok_or_else(|| error(format!("Cue timing is missing at byte {}", block[0].start)))?;
        let (start, end) = timing(timing_line.text, format)
            .ok_or_else(|| error(format!("Invalid cue timing at byte {}", timing_line.start)))?;
        let first_text = block
            .get(timing_index + 1)
            .ok_or_else(|| error(format!("Cue text is missing at byte {}", timing_line.start)))?;
        let last_text = block.last().expect("nonempty block");
        let text_range = first_text.start..last_text.end;
        events.push(Event {
            id: events.len() + 1,
            format,
            start: start.to_owned(),
            end: end.to_owned(),
            style: "Default".to_owned(),
            style_defaults: None,
            raw_text: content[text_range.clone()].to_owned(),
            text_range,
        });
    }
    Ok(events)
}

pub(super) fn plain_visible(text: &str) -> Option<String> {
    let mut visible = String::new();
    let mut rest = text;
    while let Some(start) = rest.find('<') {
        visible.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let end = after.find('>')?;
        rest = &after[end + 1..];
    }
    visible.push_str(rest);
    Some(
        visible
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&nbsp;", "\u{00a0}")
            .replace("&lrm;", "\u{200e}")
            .replace("&rlm;", "\u{200f}")
            .replace("&amp;", "&")
            .replace("\r\n", "\n")
            .replace('\r', "\n"),
    )
}

pub(super) fn editable(text: &str) -> bool {
    if text.contains(['<', '>']) {
        return false;
    }
    let mut rest = text;
    while let Some(start) = rest.find('&') {
        rest = &rest[start + 1..];
        if let Some(end) = rest.find(';')
            && end <= 12
            && rest[..end]
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'#')
        {
            return false;
        }
    }
    true
}
