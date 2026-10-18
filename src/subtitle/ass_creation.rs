use super::ass::hash;
use super::{AssCreationResult, AssTemplateOptions, Result, error};
use std::{fs, io::Write, path::Path};

fn render_ass(options: &AssTemplateOptions) -> Result<String> {
    if options.play_res_x == 0
        || options.play_res_y == 0
        || options.play_res_x > 16_384
        || options.play_res_y > 16_384
    {
        return Err(error(
            "ASS resolution must be between 1 and 16384 pixels per axis",
        ));
    }
    if options.title.is_empty()
        || options.title.trim() != options.title
        || options.title.contains(['\r', '\n'])
    {
        return Err(error(
            "ASS title must be nonempty and contain no line breaks",
        ));
    }
    if options.font_name.is_empty()
        || options.font_name.trim() != options.font_name
        || options.font_name.contains([',', '\r', '\n'])
    {
        return Err(error(
            "ASS font name must be nonempty and contain no commas or line breaks",
        ));
    }
    let font_size = options
        .font_size
        .unwrap_or_else(|| ((80.0 * f64::from(options.play_res_y) / 1080.0).round() as u32).max(1));
    if !(1..=1000).contains(&font_size) {
        return Err(error("ASS font size must be between 1 and 1000"));
    }
    let margin_h = ((120.0 * f64::from(options.play_res_x) / 1920.0).round() as u32).max(1);
    let margin_v = ((60.0 * f64::from(options.play_res_y) / 1080.0).round() as u32).max(1);
    let outline = 3.75 * f64::from(options.play_res_y) / 1080.0;
    let shadow = 2.25 * f64::from(options.play_res_y) / 1080.0;
    Ok(format!(
        "[Script Info]\nTitle: {}\nScriptType: v4.00+\nWrapStyle: 0\nScaledBorderAndShadow: yes\nYCbCr Matrix: None\nPlayResX: {}\nPlayResY: {}\nLayoutResX: {}\nLayoutResY: {}\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,{},{},&H00F8F8F8,&H000000FF,&H00000000,&H96000000,-1,0,0,0,100,100,0,0,1,{:.2},{:.2},2,{},{},{},1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n",
        options.title,
        options.play_res_x,
        options.play_res_y,
        options.play_res_x,
        options.play_res_y,
        options.font_name,
        font_size,
        outline,
        shadow,
        margin_h,
        margin_h,
        margin_v
    ))
}

pub fn create_ass(destination: &str, options: &AssTemplateOptions) -> Result<AssCreationResult> {
    let destination = Path::new(destination);
    if !destination
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case("ass"))
    {
        return Err(error("ASS destination must have an .ass extension"));
    }
    let name = destination
        .file_name()
        .ok_or_else(|| error("ASS destination must include a file name"))?;
    let content = render_ass(options)?;
    let parent = destination
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let parent = fs::canonicalize(parent)
        .map_err(|e| error(format!("Cannot resolve ASS destination directory: {e}")))?;
    if parent
        .components()
        .any(|component| component.as_os_str() == ".backup" || component.as_os_str() == ".autosave")
    {
        return Err(error(
            "Cannot create an editable subtitle inside .backup or .autosave",
        ));
    }
    let mut temporary = tempfile::Builder::new()
        .prefix(".aegisub-mcp-")
        .suffix(".ass")
        .tempfile_in(&parent)
        .map_err(|e| error(format!("Cannot create ASS file: {e}")))?;
    temporary
        .write_all(content.as_bytes())
        .map_err(|e| error(format!("Cannot write ASS file: {e}")))?;
    temporary
        .as_file()
        .sync_all()
        .map_err(|e| error(format!("Cannot sync ASS file: {e}")))?;
    let path = parent.join(name);
    temporary
        .persist_noclobber(&path)
        .map_err(|e| error(format!("Cannot save ASS file without overwriting: {e}")))?;
    Ok(AssCreationResult {
        working_path: path.display().to_string(),
        revision: hash(content.as_bytes()),
        total_events: 0,
        play_res_x: options.play_res_x,
        play_res_y: options.play_res_y,
    })
}
