pub(crate) fn is_per_character_effect(raw: &str) -> bool {
    let mut rest = raw;
    let mut seen_text = false;
    let mut inline_tags = 0;
    let mut short_segments = 0;
    while let Some(start) = rest.find('{') {
        let before = &rest[..start];
        if !before.is_empty() {
            seen_text = true;
            let length = before.replace("\\N", "").replace("\\n", "").chars().count();
            if (1..=3).contains(&length) {
                short_segments += 1;
            }
        }
        let after = &rest[start + 1..];
        let Some(end) = after.find('}') else {
            return false;
        };
        if seen_text {
            inline_tags += 1;
        }
        rest = &after[end + 1..];
    }
    let length = rest.replace("\\N", "").replace("\\n", "").chars().count();
    if (1..=3).contains(&length) {
        short_segments += 1;
    }
    inline_tags >= 3 && short_segments >= 3
}

use std::collections::{BTreeMap, BTreeSet};

const NUMERIC_TAGS: &[&str] = &[
    "xshad", "yshad", "xbord", "ybord", "fscx", "fscy", "blur", "bord", "shad", "frz", "frx",
    "fry", "fsp", "fax", "fay", "fs", "ko",
];
const COLOR_TAGS: &[&str] = &["1c", "2c", "3c", "4c", "c"];
const ALPHA_TAGS: &[&str] = &["alpha", "1a", "2a", "3a", "4a"];

#[derive(Clone)]
struct Tag {
    name: String,
    value: String,
    starred: bool,
}

#[derive(Clone)]
struct Stop {
    position: usize,
    value: String,
    keyframe: bool,
}

struct ParsedTransform {
    prefix: String,
    values: BTreeMap<String, String>,
    order: Vec<String>,
}

struct TransformPlan {
    prefix: String,
    stops: BTreeMap<String, Vec<Stop>>,
    order: Vec<String>,
}

pub(crate) struct GradientPlan {
    header: String,
    visible_text: String,
    stops: BTreeMap<String, Vec<Stop>>,
    header_keys: BTreeSet<String>,
    transforms: Vec<TransformPlan>,
    original_tokens: usize,
}

fn tag_kind(name: &str) -> Option<&'static str> {
    if NUMERIC_TAGS.contains(&name) {
        Some("numeric")
    } else if COLOR_TAGS.contains(&name) {
        Some("color")
    } else if ALPHA_TAGS.contains(&name) {
        Some("alpha")
    } else {
        None
    }
}

fn parse_color(value: &str) -> Option<[u8; 3]> {
    let hex = value.strip_prefix("&H")?.strip_suffix('&')?;
    if hex.len() != 6 {
        return None;
    }
    Some([
        u8::from_str_radix(&hex[0..2], 16).ok()?,
        u8::from_str_radix(&hex[2..4], 16).ok()?,
        u8::from_str_radix(&hex[4..6], 16).ok()?,
    ])
}

fn parse_alpha(value: &str) -> Option<u8> {
    let hex = value.strip_prefix("&H")?.strip_suffix('&')?;
    if hex.len() != 2 {
        return None;
    }
    u8::from_str_radix(hex, 16).ok()
}

fn valid_value(name: &str, value: &str) -> bool {
    match tag_kind(name) {
        Some("numeric") => value.parse::<f64>().is_ok_and(f64::is_finite),
        Some("color") => parse_color(value).is_some(),
        Some("alpha") => parse_alpha(value).is_some(),
        _ => false,
    }
}

fn parse_tags(block: &str, strict: bool) -> Option<Vec<Tag>> {
    let inner = block.strip_prefix('{')?.strip_suffix('}')?;
    let mut parts = inner.split('\\');
    let mut prefix = parts.next()?;
    if strict && !prefix.is_empty() && prefix != "*" {
        return None;
    }
    let mut tags = Vec::new();
    for part in parts {
        let starred = prefix == "*";
        let (part, next_prefix) = if let Some(value) = part.strip_suffix('*') {
            (value, "*")
        } else {
            (part, "")
        };
        prefix = next_prefix;
        let name = NUMERIC_TAGS
            .iter()
            .chain(COLOR_TAGS)
            .chain(ALPHA_TAGS)
            .find(|name| part.starts_with(**name));
        if let Some(name) = name {
            let value = &part[name.len()..];
            if valid_value(name, value) {
                tags.push(Tag {
                    name: (*name).to_owned(),
                    value: value.to_owned(),
                    starred,
                });
            } else if strict {
                return None;
            }
        } else if strict {
            return None;
        }
    }
    if strict && tags.is_empty() {
        None
    } else {
        Some(tags)
    }
}

fn block_parts(block: &str) -> Option<Vec<&str>> {
    let inner = block.strip_prefix('{')?.strip_suffix('}')?;
    let mut starts = Vec::new();
    let mut depth = 0usize;
    for (index, byte) in inner.bytes().enumerate() {
        match byte {
            b'(' => depth += 1,
            b')' => depth = depth.checked_sub(1)?,
            b'\\' if depth == 0 => starts.push(index),
            _ => {}
        }
    }
    if depth != 0 || starts.is_empty() || !matches!(&inner[..starts[0]], "" | "*") {
        return None;
    }
    Some(
        starts
            .iter()
            .enumerate()
            .map(|(index, start)| &inner[*start..*starts.get(index + 1).unwrap_or(&inner.len())])
            .collect(),
    )
}

fn parse_transform(part: &str) -> Option<ParsedTransform> {
    let inner = part.strip_prefix("\\t(")?.strip_suffix(')')?;
    let payload_at = inner.find('\\')?;
    let timing = &inner[..payload_at];
    let fields: Vec<&str> = timing
        .strip_suffix(',')
        .unwrap_or(timing)
        .split(',')
        .collect();
    if !timing.is_empty()
        && (!timing.ends_with(',')
            || fields.len() > 3
            || fields.iter().any(|field| {
                !field
                    .trim()
                    .parse::<f64>()
                    .is_ok_and(|value| value.is_finite())
            }))
    {
        return None;
    }
    let payload = &inner[payload_at..];
    let tags = parse_tags(&format!("{{{payload}}}"), true)?;
    let mut values = BTreeMap::new();
    let mut order = Vec::new();
    for tag in tags {
        order.push(tag.name.clone());
        if tag.starred || values.insert(tag.name, tag.value).is_some() {
            return None;
        }
    }
    Some(ParsedTransform {
        prefix: part[..3 + payload_at].to_owned(),
        values,
        order,
    })
}

fn parse_inline_block(block: &str) -> Option<(Vec<Tag>, Vec<ParsedTransform>)> {
    let mut static_tags = Vec::new();
    let mut transforms = Vec::new();
    let mut starred = block.strip_prefix("{*").is_some();
    for part in block_parts(block)? {
        if part.starts_with("\\t(") {
            if starred {
                return None;
            }
            transforms.push(parse_transform(part)?);
        } else {
            let prefix = if starred { "*" } else { "" };
            static_tags.extend(parse_tags(&format!("{{{prefix}{part}}}"), true)?);
            starred = part.ends_with('*');
        }
    }
    if static_tags.is_empty() && transforms.is_empty() {
        return None;
    }
    Some((static_tags, transforms))
}

fn header_transform_parts(header: &str) -> Option<(String, Vec<ParsedTransform>)> {
    let mut cleaned = String::new();
    let mut transforms = Vec::new();
    let mut rest = header;
    while let Some(start) = rest.find('{') {
        cleaned.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let end = after.find('}')?;
        let block = &rest[start..start + end + 2];
        let mut retained = String::new();
        for part in block_parts(block)? {
            if part.starts_with("\\t(") {
                transforms.push(parse_transform(part)?);
            } else {
                retained.push_str(part);
            }
        }
        if !retained.is_empty() {
            cleaned.push('{');
            cleaned.push_str(&retained);
            cleaned.push('}');
        }
        rest = &after[end + 1..];
    }
    cleaned.push_str(rest);
    Some((cleaned, transforms))
}

fn tokens(text: &str) -> Vec<&str> {
    let mut result = Vec::new();
    let mut offset = 0;
    while offset < text.len() {
        let rest = &text[offset..];
        let width = if rest.starts_with("\\N") || rest.starts_with("\\n") || rest.starts_with("\\h")
        {
            2
        } else {
            rest.chars().next().expect("nonempty").len_utf8()
        };
        result.push(&text[offset..offset + width]);
        offset += width;
    }
    result
}

impl GradientPlan {
    #[cfg(test)]
    pub(crate) fn parse(raw: &str) -> Option<Self> {
        Self::parse_with_defaults(raw, None)
    }

    pub(crate) fn parse_with_defaults(
        raw: &str,
        style_defaults: Option<&BTreeMap<String, String>>,
    ) -> Option<Self> {
        if !is_per_character_effect(raw) {
            return None;
        }
        let mut rest = raw;
        let mut header = String::new();
        let mut visible_text = String::new();
        let mut original_tokens = 0;
        let mut seen_text = false;
        let mut trailing_inline_block = false;
        let mut all_stops: BTreeMap<String, Vec<Stop>> = BTreeMap::new();
        let mut text_segment_lengths = Vec::new();
        let mut inline_transform_groups = Vec::new();

        while let Some(start) = rest.find('{') {
            let before = &rest[..start];
            if !before.is_empty() {
                seen_text = true;
                trailing_inline_block = false;
                visible_text.push_str(before);
                let length = tokens(before).len();
                original_tokens += length;
                text_segment_lengths.push(length);
            }
            let after = &rest[start + 1..];
            let end = after.find('}')?;
            let block = &rest[start..start + end + 2];
            if !seen_text {
                header.push_str(block);
            } else {
                trailing_inline_block = true;
                let (tags, transforms) = parse_inline_block(block)?;
                inline_transform_groups.push((original_tokens, transforms));
                for tag in tags {
                    all_stops.entry(tag.name).or_default().push(Stop {
                        position: original_tokens,
                        value: tag.value,
                        keyframe: !tag.starred,
                    });
                }
            }
            rest = &after[end + 1..];
        }
        if !rest.is_empty() {
            trailing_inline_block = false;
            visible_text.push_str(rest);
            let length = tokens(rest).len();
            original_tokens += length;
            text_segment_lengths.push(length);
        }
        if header.is_empty()
            || trailing_inline_block
            || original_tokens == 0
            || (all_stops.is_empty()
                && inline_transform_groups
                    .iter()
                    .all(|(_, group)| group.is_empty()))
        {
            return None;
        }
        let mut initial = BTreeMap::new();
        let mut reset_in_header = false;
        let mut header_rest = header.as_str();
        while let Some(start) = header_rest.find('{') {
            let after = &header_rest[start + 1..];
            let end = after.find('}')?;
            for part in block_parts(&header_rest[start..start + end + 2])? {
                if part.starts_with("\\r") {
                    initial.clear();
                    reset_in_header = true;
                } else if !part.starts_with("\\t(") {
                    for tag in parse_tags(&format!("{{{part}}}"), false)? {
                        if tag.name == "c" || tag.name == "1c" {
                            initial.insert("c".to_owned(), tag.value.clone());
                            initial.insert("1c".to_owned(), tag.value);
                        } else {
                            initial.insert(tag.name, tag.value);
                        }
                    }
                }
            }
            header_rest = &after[end + 1..];
        }
        let header_keys = initial.keys().cloned().collect();
        let has_transforms = inline_transform_groups
            .iter()
            .any(|(_, group)| !group.is_empty());
        let (header, header_transforms) = if has_transforms {
            header_transform_parts(&header)?
        } else {
            (header, Vec::new())
        };
        let mut transforms = Vec::new();
        if has_transforms {
            if text_segment_lengths.len() != original_tokens || inline_transform_groups.is_empty() {
                return None;
            }
            let count = inline_transform_groups[0].1.len();
            if count == 0
                || inline_transform_groups
                    .iter()
                    .any(|(_, group)| group.len() != count)
                || header_transforms.len() != count
            {
                return None;
            }
            for slot in 0..count {
                let template = &inline_transform_groups[0].1[slot];
                let header_transform = &header_transforms[slot];
                if header_transform.prefix != template.prefix
                    || header_transform.order != template.order
                {
                    return None;
                }
                let mut transform_stops: BTreeMap<String, Vec<Stop>> = BTreeMap::new();
                for (name, value) in &header_transform.values {
                    transform_stops.entry(name.clone()).or_default().push(Stop {
                        position: 0,
                        value: value.clone(),
                        keyframe: true,
                    });
                }
                for (position, group) in &inline_transform_groups {
                    let item = &group[slot];
                    if item.prefix != template.prefix || item.order != template.order {
                        return None;
                    }
                    for (name, value) in &item.values {
                        transform_stops.entry(name.clone()).or_default().push(Stop {
                            position: *position,
                            value: value.clone(),
                            keyframe: true,
                        });
                    }
                }
                transforms.push(TransformPlan {
                    prefix: template.prefix.clone(),
                    stops: transform_stops,
                    order: template.order.clone(),
                });
            }
        }
        let mut stops = BTreeMap::new();
        for (name, inline) in all_stops {
            if !initial.contains_key(&name) {
                if reset_in_header {
                    return None;
                }
                let inherited = style_defaults?.get(&name)?;
                initial.insert(name.clone(), inherited.clone());
            }
            let mut keyframes = Vec::new();
            if let Some(value) = initial.get(&name) {
                keyframes.push(Stop {
                    position: 0,
                    value: value.clone(),
                    keyframe: true,
                });
            }
            keyframes.extend(inline.iter().filter(|stop| stop.keyframe).cloned());
            if keyframes.len() < 2 {
                keyframes.clear();
                if let Some(value) = initial.get(&name) {
                    keyframes.push(Stop {
                        position: 0,
                        value: value.clone(),
                        keyframe: true,
                    });
                }
                keyframes.extend(inline);
            }
            keyframes.sort_by_key(|stop| stop.position);
            if keyframes.is_empty() {
                return None;
            }
            stops.insert(name, keyframes);
        }
        Some(Self {
            header,
            visible_text,
            stops,
            header_keys,
            transforms,
            original_tokens,
        })
    }

    pub(crate) fn visible_text(&self) -> &str {
        &self.visible_text
    }

    pub(crate) fn same_structure_as(&self, other: &Self) -> bool {
        self.header == other.header
            && self.header_keys == other.header_keys
            && self.stops.keys().eq(other.stops.keys())
            && self.transforms.len() == other.transforms.len()
            && self
                .transforms
                .iter()
                .zip(&other.transforms)
                .all(|(left, right)| {
                    left.prefix == right.prefix
                        && left.order == right.order
                        && left.stops.keys().eq(right.stops.keys())
                })
    }

    pub(crate) fn rebuild(&self, replacement: &str) -> Option<String> {
        if replacement.contains(['{', '}', '\r', '\n']) {
            return None;
        }
        let units = tokens(replacement);
        if units.is_empty() {
            return None;
        }
        let mut result = self.header.clone();
        for (index, unit) in units.iter().enumerate() {
            let position = if units.len() <= 1 {
                0.0
            } else {
                index as f64 / (units.len() - 1) as f64
            };
            let mut block = String::new();
            for (name, stops) in &self.stops {
                if index == 0 && self.header_keys.contains(name) {
                    continue;
                }
                block.push('\\');
                block.push_str(name);
                block.push_str(&interpolate(name, stops, self.original_tokens, position)?);
            }
            for transform in &self.transforms {
                block.push_str(&transform.prefix);
                for name in &transform.order {
                    let stops = transform.stops.get(name)?;
                    block.push('\\');
                    block.push_str(name);
                    block.push_str(&interpolate(name, stops, self.original_tokens, position)?);
                }
                block.push(')');
            }
            if !block.is_empty() {
                result.push('{');
                result.push_str(&block);
                result.push('}');
            }
            result.push_str(unit);
        }
        Some(result)
    }
}

fn interpolate(name: &str, stops: &[Stop], original_tokens: usize, target: f64) -> Option<String> {
    let denominator = (original_tokens.saturating_sub(1)).max(1) as f64;
    let first = stops.first()?;
    let first_pos = first.position as f64 / denominator;
    if target <= first_pos {
        return Some(first.value.clone());
    }
    for pair in stops.windows(2) {
        let (left, right) = (&pair[0], &pair[1]);
        let left_pos = left.position as f64 / denominator;
        let right_pos = right.position as f64 / denominator;
        if target <= right_pos {
            let fraction = if right_pos <= left_pos {
                0.0
            } else {
                (target - left_pos) / (right_pos - left_pos)
            };
            return interpolate_value(name, &left.value, &right.value, fraction);
        }
    }
    Some(stops.last()?.value.clone())
}

fn interpolate_value(name: &str, left: &str, right: &str, fraction: f64) -> Option<String> {
    match tag_kind(name)? {
        "numeric" => {
            let a = left.parse::<f64>().ok()?;
            let b = right.parse::<f64>().ok()?;
            let value = a + (b - a) * fraction;
            if !value.is_finite() {
                return None;
            }
            let mut formatted = format!("{value:.2}");
            while formatted.contains('.') && formatted.ends_with('0') {
                formatted.pop();
            }
            if formatted.ends_with('.') {
                formatted.pop();
            }
            if formatted == "-0" {
                formatted = "0".into();
            }
            Some(formatted)
        }
        "color" => {
            let a = parse_color(left)?;
            let b = parse_color(right)?;
            let channel =
                |i: usize| (f64::from(a[i]) + (f64::from(b[i]) - f64::from(a[i])) * fraction) as u8;
            Some(format!(
                "&H{:02X}{:02X}{:02X}&",
                channel(0),
                channel(1),
                channel(2)
            ))
        }
        "alpha" => {
            let a = parse_alpha(left)?;
            let b = parse_alpha(right)?;
            let value = (f64::from(a) + (f64::from(b) - f64::from(a)) * fraction) as u8;
            Some(format!("&H{value:02X}&"))
        }
        _ => None,
    }
}
