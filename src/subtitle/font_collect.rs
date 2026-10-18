use super::{CollectedFontFile, Document, FontCollectionResult, FontWarning, Result, error};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
    process::Command,
    sync::Mutex,
};

static FONT_LOCK: Mutex<()> = Mutex::new(());

#[derive(Clone)]
struct FontFace {
    names: Vec<String>,
    family: String,
    output_name: String,
    weight: u32,
    italic: bool,
    charset: String,
    path: PathBuf,
}

fn normalized_name(name: &str) -> String {
    name.trim().trim_start_matches('@').to_lowercase()
}

fn installed_fonts() -> Result<Vec<FontFace>> {
    let output = Command::new("fc-list")
        .args([
            "-f",
            "%{family}\t%{style}\t%{fullname}\t%{weight}\t%{slant}\t%{charset}\t%{file}\n",
        ])
        .output()
        .map_err(|e| error(format!("Cannot run fontconfig fc-list: {e}")))?;
    if !output.status.success() {
        return Err(error(format!(
            "fontconfig fc-list failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    let listing = String::from_utf8(output.stdout)
        .map_err(|_| error("fontconfig returned non-UTF-8 output"))?;
    let mut faces = Vec::new();
    for line in listing.lines() {
        let fields = line.splitn(7, '\t').collect::<Vec<_>>();
        if fields.len() != 7 {
            continue;
        }
        let path = PathBuf::from(fields[6]);
        let extension = path
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if !matches!(extension.as_str(), "ttf" | "otf" | "ttc" | "otc") {
            continue;
        }
        let names = fields[0]
            .split(',')
            .chain(fields[2].split(','))
            .map(normalized_name)
            .filter(|name| !name.is_empty())
            .collect::<Vec<_>>();
        if names.is_empty() {
            continue;
        }
        let family = fields[0].split(',').next().unwrap_or_default().trim();
        let style = fields[1].split(',').next().unwrap_or_default().trim();
        faces.push(FontFace {
            names,
            family: family.to_owned(),
            output_name: standardized_font_name(family, style),
            weight: fields[3].parse().unwrap_or(80),
            italic: fields[4].parse::<u32>().is_ok_and(|slant| slant >= 100),
            charset: fields[5].to_owned(),
            path,
        });
    }
    Ok(faces)
}

fn filename_component(value: &str) -> String {
    let mut result = String::new();
    let mut separator = false;
    for character in value.chars() {
        if character.is_alphanumeric() {
            if separator && !result.is_empty() {
                result.push('-');
            }
            result.push(character);
            separator = false;
        } else {
            separator = true;
        }
    }
    result
}

fn standardized_font_name(family: &str, style: &str) -> String {
    let family = filename_component(family);
    let style = filename_component(style);
    match (family.is_empty(), style.is_empty()) {
        (false, false) => format!("{family}-{style}"),
        (false, true) => family,
        (true, false) => style,
        (true, true) => "font".to_owned(),
    }
}

fn hash_file(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path)
        .map_err(|e| error(format!("Cannot read font {}: {e}", path.display())))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|e| error(format!("Cannot hash font {}: {e}", path.display())))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn is_variable_font(path: &Path) -> Result<bool> {
    let mut file = fs::File::open(path)
        .map_err(|e| error(format!("Cannot inspect font {}: {e}", path.display())))?;
    let mut header = [0_u8; 12];
    file.read_exact(&mut header)
        .map_err(|e| error(format!("Cannot read font header {}: {e}", path.display())))?;
    let table_count = u16::from_be_bytes([header[4], header[5]]);
    for _ in 0..table_count {
        let mut record = [0_u8; 16];
        file.read_exact(&mut record)
            .map_err(|e| error(format!("Cannot read font table record: {e}")))?;
        if &record[..4] == b"fvar" {
            return Ok(true);
        }
    }
    Ok(false)
}

fn charset_contains(charset: &str, codepoint: u32) -> Option<bool> {
    if charset.is_empty() {
        return None;
    }
    for span in charset.split_whitespace() {
        let (first, last) = span.split_once('-').unwrap_or((span, span));
        let (Ok(first), Ok(last)) = (
            u32::from_str_radix(first, 16),
            u32::from_str_radix(last, 16),
        ) else {
            return None;
        };
        if (first..=last).contains(&codepoint) {
            return Some(true);
        }
    }
    Some(false)
}

fn warning(kind: &'static str, font: &str, message: String) -> FontWarning {
    FontWarning {
        kind,
        font_name: font.to_owned(),
        message,
    }
}

fn select_face<'a>(request: &FontRequest, matches: &[&'a FontFace]) -> &'a FontFace {
    let target_weight = if request.bold { 200 } else { 80 };
    matches
        .iter()
        .copied()
        .min_by_key(|face| {
            (
                u32::from(face.italic != request.italic) * 1000
                    + face.weight.abs_diff(target_weight),
                &face.path,
                &face.output_name,
            )
        })
        .expect("matches are nonempty")
}

fn missing_glyphs(request: &FontRequest, face: &FontFace) -> Option<Vec<char>> {
    let mut missing = Vec::new();
    for ch in &request.chars {
        match charset_contains(&face.charset, *ch as u32) {
            Some(true) => {}
            Some(false) => missing.push(*ch),
            None => return None,
        }
    }
    Some(missing)
}

fn destination_for(
    directory: &Path,
    output_name: &str,
    extension: &str,
    content_hash: &str,
) -> Result<(PathBuf, bool)> {
    let original = directory.join(format!("{output_name}.{extension}"));
    if !original.exists() {
        return Ok((original, false));
    }
    if hash_file(&original)? == content_hash {
        return Ok((original, true));
    }
    let alternate = directory.join(format!(
        "{output_name}-{}.{}",
        &content_hash[..12],
        extension
    ));
    if alternate.exists() {
        if hash_file(&alternate)? == content_hash {
            return Ok((alternate, true));
        }
        return Err(error(format!(
            "Font destination collision: {}",
            alternate.display()
        )));
    }
    Ok((alternate, false))
}

fn collect_with_faces(
    subtitle: &Path,
    destination: Option<&str>,
    faces: &[FontFace],
) -> Result<FontCollectionResult> {
    let subtitle = fs::canonicalize(subtitle)
        .map_err(|e| error(format!("Cannot resolve ASS subtitle: {e}")))?;
    if !subtitle
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("ass"))
    {
        return Err(error("Font collection requires an .ass subtitle"));
    }
    let document = Document::open(&subtitle)?;
    let requests = requests(&document)?;
    if requests.is_empty() {
        return Err(error("ASS subtitle does not declare or use any fonts"));
    }
    let required_fonts = requests
        .iter()
        .map(|request| request.font.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let mut selected = BTreeMap::<PathBuf, (String, String)>::new();
    let mut warnings = Vec::new();
    let mut missing = BTreeSet::new();
    for request in &requests {
        let wanted = normalized_name(&request.font);
        let matches = faces
            .iter()
            .filter(|face| face.names.iter().any(|name| name == &wanted))
            .collect::<Vec<_>>();
        if matches.is_empty() {
            if missing.insert(wanted) {
                warnings.push(warning(
                    "missing_font",
                    &request.font,
                    "The font is not installed; no file can be copied for it".to_owned(),
                ));
            }
            continue;
        }
        let exact_style = matches
            .iter()
            .any(|face| (face.weight >= 200) == request.bold && face.italic == request.italic);
        if !exact_style {
            let kind = match (request.bold, request.italic) {
                (true, true) => "missing_bold_italic",
                (true, false) => "missing_bold",
                (false, true) => "missing_italic",
                (false, false) => "missing_regular",
            };
            warnings.push(warning(
                kind,
                &request.font,
                "No installed face matches the requested bold/italic combination; the renderer may synthesize it or use another face".to_owned(),
            ));
        }
        let chosen = select_face(request, &matches);
        if !request.chars.is_empty() {
            match missing_glyphs(request, chosen) {
                Some(glyphs) if !glyphs.is_empty() => {
                    let examples = glyphs
                        .iter()
                        .take(12)
                        .map(|ch| format!("U+{:04X}", *ch as u32))
                        .collect::<Vec<_>>()
                        .join(", ");
                    warnings.push(warning(
                        "missing_glyphs",
                        &request.font,
                        format!(
                            "Selected face {} lacks {} used glyph(s): {examples}",
                            chosen.output_name,
                            glyphs.len()
                        ),
                    ));
                }
                None => warnings.push(warning(
                    "coverage_unavailable",
                    &request.font,
                    format!(
                        "fontconfig did not provide usable character coverage for {}",
                        chosen.output_name
                    ),
                )),
                _ => {}
            }
        }
        let path = fs::canonicalize(&chosen.path).map_err(|e| {
            error(format!(
                "Cannot resolve font {}: {e}",
                chosen.path.display()
            ))
        })?;
        selected
            .entry(path)
            .or_insert_with(|| (chosen.family.clone(), chosen.output_name.clone()));
    }
    let _guard = FONT_LOCK
        .lock()
        .map_err(|_| error("Font collection lock is unavailable"))?;
    let destination = destination.map(PathBuf::from).unwrap_or_else(|| {
        subtitle
            .parent()
            .expect("canonical path has parent")
            .join("fonts")
    });
    fs::create_dir_all(&destination)
        .map_err(|e| error(format!("Cannot create font destination: {e}")))?;
    let destination = fs::canonicalize(&destination)
        .map_err(|e| error(format!("Cannot resolve font destination: {e}")))?;
    let mut files = Vec::new();
    for (source, (family, face_name)) in selected {
        let extension = source
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("ttf")
            .to_ascii_lowercase();
        let is_collection = matches!(extension.as_str(), "ttc" | "otc");
        let output_name = if is_collection {
            let stem = source
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or("font");
            let stem = stem
                .strip_suffix(".otf")
                .or_else(|| stem.strip_suffix(".ttf"))
                .unwrap_or(stem);
            standardized_font_name(stem, "Collection")
        } else if is_variable_font(&source)? {
            if normalized_name(&family).ends_with("variable") {
                filename_component(&family)
            } else {
                standardized_font_name(&family, "Variable")
            }
        } else {
            face_name
        };
        let content_hash = hash_file(&source)?;
        let (target, already_present) =
            destination_for(&destination, &output_name, &extension, &content_hash)?;
        if !already_present {
            let mut temporary = tempfile::NamedTempFile::new_in(&destination)
                .map_err(|e| error(format!("Cannot create temporary font file: {e}")))?;
            let mut input = fs::File::open(&source)
                .map_err(|e| error(format!("Cannot open font {}: {e}", source.display())))?;
            io::copy(&mut input, temporary.as_file_mut())
                .map_err(|e| error(format!("Cannot copy font {}: {e}", source.display())))?;
            temporary
                .as_file()
                .set_permissions(
                    fs::metadata(&source)
                        .map_err(|e| error(format!("Cannot read font permissions: {e}")))?
                        .permissions(),
                )
                .map_err(|e| error(format!("Cannot preserve font permissions: {e}")))?;
            temporary
                .as_file()
                .sync_all()
                .map_err(|e| error(format!("Cannot sync copied font: {e}")))?;
            temporary.persist_noclobber(&target).map_err(|e| {
                error(format!(
                    "Cannot save font without overwriting {}: {e}",
                    target.display()
                ))
            })?;
        }
        files.push(CollectedFontFile {
            source_path: source.display().to_string(),
            destination_path: target.display().to_string(),
            already_present,
        });
    }
    Ok(FontCollectionResult {
        subtitle_path: subtitle.display().to_string(),
        destination_path: destination.display().to_string(),
        required_fonts,
        files,
        warnings,
    })
}

pub fn collect_fonts(subtitle: &str, destination: Option<&str>) -> Result<FontCollectionResult> {
    let faces = installed_fonts()?;
    collect_with_faces(Path::new(subtitle), destination, &faces)
}

#[derive(Clone, Default)]
struct Style {
    font: String,
    bold: bool,
    italic: bool,
}

#[derive(Clone)]
struct FontRequest {
    font: String,
    bold: bool,
    italic: bool,
    chars: BTreeSet<char>,
}

fn enabled(value: &str) -> bool {
    match value.trim().parse::<i32>() {
        Ok(0 | 400) => false,
        Ok(1) => true,
        Ok(weight) => !(0..=500).contains(&weight),
        Err(_) => false,
    }
}

fn styles(content: &str) -> Result<BTreeMap<String, Style>> {
    let mut result = BTreeMap::new();
    let mut in_styles = false;
    let mut format = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim().trim_start_matches('\u{feff}');
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_styles = trimmed.eq_ignore_ascii_case("[V4+ Styles]")
                || trimmed.eq_ignore_ascii_case("[V4 Styles]");
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
                .map(|part| part.trim().to_owned())
                .collect();
        } else if kind.trim().eq_ignore_ascii_case("Style") {
            let field = |name: &str| -> Option<&str> {
                let index = format
                    .iter()
                    .position(|item| item.eq_ignore_ascii_case(name))?;
                value.split(',').nth(index).map(str::trim)
            };
            let name = field("Name").ok_or_else(|| error("ASS style is missing Name"))?;
            let font = field("Fontname")
                .filter(|name| !name.is_empty())
                .ok_or_else(|| error("ASS style is missing Fontname"))?;
            result.insert(
                name.to_lowercase(),
                Style {
                    font: font.to_owned(),
                    bold: field("Bold").is_some_and(enabled),
                    italic: field("Italic").is_some_and(enabled),
                },
            );
        }
    }
    Ok(result)
}

fn insert_request(requests: &mut BTreeMap<(String, bool, bool), FontRequest>, style: &Style) {
    if style.font.is_empty() {
        return;
    }
    requests
        .entry((normalized_name(&style.font), style.bold, style.italic))
        .or_insert_with(|| FontRequest {
            font: style.font.clone(),
            bold: style.bold,
            italic: style.italic,
            chars: BTreeSet::new(),
        });
}

fn add_text(
    requests: &mut BTreeMap<(String, bool, bool), FontRequest>,
    style: &Style,
    drawing: bool,
    text: &str,
) {
    if drawing || style.font.is_empty() {
        return;
    }
    insert_request(requests, style);
    let request = requests
        .get_mut(&(normalized_name(&style.font), style.bold, style.italic))
        .expect("font request was inserted");
    let mut chars = text.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some('h') => {
                    request.chars.insert('\u{a0}');
                }
                Some('N' | 'n') => {}
                Some(other) => {
                    request.chars.insert(other);
                }
                None => {}
            }
        } else if !ch.is_control() && !ch.is_whitespace() {
            request.chars.insert(ch);
        }
    }
}

fn apply_tags(
    tags: &str,
    current: &mut Style,
    base: &Style,
    styles: &BTreeMap<String, Style>,
    drawing: &mut bool,
) {
    for tag in tags.split('\\').skip(1) {
        if let Some(name) = tag.strip_prefix("fn") {
            current.font = if name.trim().is_empty() {
                base.font.clone()
            } else {
                name.trim().to_owned()
            };
        } else if let Some(value) = tag.strip_prefix('b')
            && (value.is_empty() || value.starts_with(|ch: char| ch.is_ascii_digit() || ch == '-'))
        {
            current.bold = if value.trim().is_empty() {
                base.bold
            } else {
                enabled(value)
            };
        } else if let Some(value) = tag.strip_prefix('i')
            && (value.is_empty() || value.starts_with(|ch: char| ch.is_ascii_digit() || ch == '-'))
        {
            current.italic = if value.trim().is_empty() {
                base.italic
            } else {
                enabled(value)
            };
        } else if let Some(value) = tag.strip_prefix('r') {
            *current = if value.trim().is_empty() {
                base.clone()
            } else {
                styles
                    .get(&value.trim().to_lowercase())
                    .cloned()
                    .unwrap_or_else(|| base.clone())
            };
            *drawing = false;
        } else if let Some(value) = tag.strip_prefix('p')
            && value.starts_with(|ch: char| ch.is_ascii_digit())
        {
            *drawing = value.bytes().next().is_some_and(|digit| digit != b'0');
        }
    }
}

fn requests(document: &Document) -> Result<Vec<FontRequest>> {
    let styles = styles(&document.content)?;
    let mut requests = BTreeMap::new();
    for style in styles.values() {
        insert_request(&mut requests, style);
    }
    for event in &document.events {
        let base = styles
            .get(&event.style.to_lowercase())
            .cloned()
            .or_else(|| styles.get("default").cloned())
            .unwrap_or_default();
        let mut current = base.clone();
        let mut drawing = false;
        let mut remaining = event.raw_text.as_str();
        loop {
            let Some(start) = remaining.find('{') else {
                add_text(&mut requests, &current, drawing, remaining);
                break;
            };
            add_text(&mut requests, &current, drawing, &remaining[..start]);
            let after = &remaining[start + 1..];
            let end = after
                .find('}')
                .ok_or_else(|| error(format!("Unclosed override block in event {}", event.id)))?;
            apply_tags(&after[..end], &mut current, &base, &styles, &mut drawing);
            remaining = &after[end + 1..];
        }
    }
    Ok(requests.into_values().collect())
}

#[cfg(test)]
mod usage_tests {
    use super::*;
    use std::fs;

    #[test]
    fn attributes_visible_characters_to_inline_font_and_reset() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("usage.ass");
        fs::write(&path, "[V4+ Styles]\nFormat: Name, Fontname, Bold, Italic\nStyle: Default,Lato,-1,0\n[Events]\nFormat: Start, End, Style, Text\nDialogue: 0:00:01.00,0:00:02.00,Default,A{\\fnSign Font\\i1\\bord3}字{\\rDefault}B\n").unwrap();
        let document = Document::open(&path).unwrap();
        let uses = requests(&document).unwrap();
        let lato = uses.iter().find(|item| item.font == "Lato").unwrap();
        let sign = uses.iter().find(|item| item.font == "Sign Font").unwrap();
        assert_eq!(lato.chars, ['A', 'B'].into_iter().collect());
        assert!(lato.bold);
        assert!(!lato.italic);
        assert_eq!(sign.chars, ['字'].into_iter().collect());
        assert!(sign.bold);
        assert!(sign.italic);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copies_original_font_and_reports_coverage_and_missing_variants() {
        let dir = tempfile::tempdir().unwrap();
        let subtitle = dir.path().join("working.ass");
        let lato = dir.path().join("system-lato.ttf");
        fs::write(&lato, b"\0\x01\0\0\0\0\0\0\0\0\0\0lato-face").unwrap();
        fs::write(&subtitle, "[V4+ Styles]\nFormat: Name, Fontname, Bold, Italic\nStyle: Default,Lato,-1,0\n[Events]\nFormat: Start, End, Style, Text\nDialogue: 0:00:01.00,0:00:02.00,Default,A{\\i1}é{\\i0}B\n").unwrap();
        let faces = vec![FontFace {
            names: vec!["lato".into()],
            family: "Lato".into(),
            output_name: "Lato-Regular".into(),
            weight: 80,
            italic: false,
            charset: "41-42".into(),
            path: lato.clone(),
        }];
        let first = collect_with_faces(&subtitle, None, &faces).unwrap();
        assert_eq!(first.required_fonts, vec!["Lato"]);
        assert_eq!(first.files.len(), 1);
        assert_eq!(
            fs::read(&first.files[0].destination_path).unwrap(),
            fs::read(&lato).unwrap()
        );
        assert!(
            first
                .warnings
                .iter()
                .any(|item| item.kind == "missing_bold")
        );
        assert!(
            first
                .warnings
                .iter()
                .any(|item| item.kind == "missing_bold_italic")
        );
        assert!(
            first
                .warnings
                .iter()
                .any(|item| item.kind == "missing_glyphs" && item.message.contains("U+00E9"))
        );
        let second = collect_with_faces(&subtitle, None, &faces).unwrap();
        assert!(second.files[0].already_present);
    }

    #[test]
    fn missing_font_is_warning_and_other_fonts_are_copied() {
        let dir = tempfile::tempdir().unwrap();
        let subtitle = dir.path().join("working.ass");
        let font = dir.path().join("font.otf");
        fs::write(&font, b"OTTO\0\0\0\0\0\0\0\0font-face").unwrap();
        fs::write(&subtitle, "[V4+ Styles]\nFormat: Name, Fontname\nStyle: Default,Installed\nStyle: Sign,Missing\n[Events]\nFormat: Start, End, Style, Text\nDialogue: 0:00:01.00,0:00:02.00,Default,Hello\n").unwrap();
        let faces = vec![FontFace {
            names: vec!["installed".into()],
            family: "Installed".into(),
            output_name: "Installed-Regular".into(),
            weight: 80,
            italic: false,
            charset: "20-7e".into(),
            path: font,
        }];
        let result = collect_with_faces(&subtitle, None, &faces).unwrap();
        assert_eq!(result.files.len(), 1);
        assert!(
            result
                .warnings
                .iter()
                .any(|item| item.kind == "missing_font" && item.font_name == "Missing")
        );
    }

    #[test]
    fn collects_real_installed_lato_when_requested() {
        if std::env::var_os("AEGISUB_TEST_INSTALLED_FONTS").is_none() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let subtitle = dir.path().join("working.ass");
        fs::write(&subtitle, "[V4+ Styles]\nFormat: Name, Fontname, Bold, Italic\nStyle: Default,Lato,-1,0\n[Events]\nFormat: Start, End, Style, Text\nDialogue: 0:00:01.00,0:00:02.00,Default,Hello é\n").unwrap();
        let result = collect_fonts(subtitle.to_str().unwrap(), None).unwrap();
        assert_eq!(result.required_fonts, vec!["Lato"]);
        assert_eq!(result.files.len(), 1);
        assert!(result.warnings.is_empty(), "unexpected font warnings");
        assert_eq!(
            fs::read(&result.files[0].source_path).unwrap(),
            fs::read(&result.files[0].destination_path).unwrap()
        );
    }

    #[test]
    fn preserves_variable_and_collection_files() {
        let dir = tempfile::tempdir().unwrap();
        let subtitle = dir.path().join("working.ass");
        let variable = dir.path().join("original.ttf");
        let collection = dir.path().join("NotoSansCJK-VF.otf.ttc");
        let mut sfnt = vec![0_u8; 28];
        sfnt[..4].copy_from_slice(b"\0\x01\0\0");
        sfnt[4..6].copy_from_slice(&1_u16.to_be_bytes());
        sfnt[12..16].copy_from_slice(b"fvar");
        fs::write(&variable, &sfnt).unwrap();
        fs::write(&collection, b"ttcf-original-collection").unwrap();
        fs::write(&subtitle, "[V4+ Styles]\nFormat: Name, Fontname\nStyle: Default,Variable Family\nStyle: Sign,CJK Family\n").unwrap();
        let faces = vec![
            FontFace {
                names: vec!["variable family".into()],
                family: "Variable Family".into(),
                output_name: "Variable-Family-Regular".into(),
                weight: 80,
                italic: false,
                charset: "20-7e".into(),
                path: variable.clone(),
            },
            FontFace {
                names: vec!["cjk family".into()],
                family: "CJK Family".into(),
                output_name: "CJK-Family-Regular".into(),
                weight: 80,
                italic: false,
                charset: "20-7e".into(),
                path: collection.clone(),
            },
        ];
        let result = collect_with_faces(&subtitle, None, &faces).unwrap();
        assert_eq!(result.files.len(), 2);
        for file in result.files {
            assert_eq!(
                fs::read(file.source_path).unwrap(),
                fs::read(file.destination_path).unwrap()
            );
        }
    }
}
