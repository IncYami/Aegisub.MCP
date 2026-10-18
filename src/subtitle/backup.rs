use super::ass::hash;
use super::{Document, PreparedSubtitleResult, Result, error};
use std::{
    fs,
    io::Write,
    time::{SystemTime, UNIX_EPOCH},
};

pub(super) fn utc_timestamp(now: SystemTime) -> Result<String> {
    let elapsed = now
        .duration_since(UNIX_EPOCH)
        .map_err(|_| error("System clock predates the Unix epoch"))?;
    let seconds = elapsed.as_secs();
    let days =
        i64::try_from(seconds / 86_400).map_err(|_| error("System clock date is out of range"))?;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_part = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_part + 2) / 5 + 1;
    let month = month_part + if month_part < 10 { 3 } else { -9 };
    if month <= 2 {
        year += 1;
    }
    let hour = seconds % 86_400 / 3_600;
    let minute = seconds % 3_600 / 60;
    let second = seconds % 60;
    Ok(format!(
        "{year:04}-{month:02}-{day:02}T{hour:02}-{minute:02}-{second:02}.{:09}Z",
        elapsed.subsec_nanos()
    ))
}

pub fn prepare_subtitle(source: &str) -> Result<PreparedSubtitleResult> {
    let source_path = fs::canonicalize(source)
        .map_err(|e| error(format!("Cannot resolve source subtitle: {e}")))?;
    if source_path
        .components()
        .any(|component| component.as_os_str() == ".backup" || component.as_os_str() == ".autosave")
    {
        return Err(error(
            "Files inside .backup or .autosave cannot be prepared for editing",
        ));
    }
    let document = Document::open(&source_path)?;
    let parent = source_path.parent().expect("canonical path has parent");
    let backup_dir = parent.join(".backup");
    match fs::symlink_metadata(&backup_dir) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            return Err(error("Backup path must be a real directory"));
        }
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir(&backup_dir)
                .map_err(|e| error(format!("Cannot create backup directory: {e}")))?;
        }
        Err(e) => return Err(error(format!("Cannot inspect backup directory: {e}"))),
    }
    let stamp = utc_timestamp(SystemTime::now())?;
    let mut temporary = tempfile::Builder::new()
        .prefix(".subtitle-backup-")
        .tempfile_in(&backup_dir)
        .map_err(|e| error(format!("Cannot create temporary backup: {e}")))?;
    temporary
        .write_all(document.content.as_bytes())
        .map_err(|e| error(format!("Cannot write subtitle backup: {e}")))?;
    temporary
        .as_file()
        .set_permissions(
            fs::metadata(&source_path)
                .map_err(|e| error(format!("Cannot read subtitle permissions: {e}")))?
                .permissions(),
        )
        .map_err(|e| error(format!("Cannot preserve subtitle permissions: {e}")))?;
    temporary
        .as_file()
        .sync_all()
        .map_err(|e| error(format!("Cannot sync subtitle backup: {e}")))?;
    let current_source = fs::read(&source_path)
        .map_err(|e| error(format!("Cannot recheck source subtitle: {e}")))?;
    if hash(&current_source) != document.revision {
        return Err(error("Source subtitle changed while creating the backup"));
    }
    let backup_path = {
        let mut collision = 0;
        loop {
            let path = backup_dir.join(versioned_filename(
                &source_path,
                &stamp,
                "BACKUP",
                collision,
            ));
            match temporary.persist_noclobber(&path) {
                Ok(_) => break path,
                Err(e) if e.error.kind() == std::io::ErrorKind::AlreadyExists => {
                    temporary = e.file;
                    collision += 1;
                }
                Err(e) => return Err(error(format!("Cannot save subtitle backup: {}", e.error))),
            }
        }
    };
    fs::File::open(&backup_dir)
        .and_then(|directory| directory.sync_all())
        .map_err(|e| error(format!("Cannot sync backup directory: {e}")))?;
    Ok(PreparedSubtitleResult {
        editable_path: source_path.display().to_string(),
        backup_path: backup_path.display().to_string(),
        revision: document.revision,
        total_events: document.events.len(),
    })
}

pub(super) fn versioned_filename(
    source_path: &std::path::Path,
    stamp: &str,
    marker: &str,
    collision: usize,
) -> String {
    let stem = source_path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("subtitle");
    let extension = source_path
        .extension()
        .and_then(|value| value.to_str())
        .expect("Subtitle extension was validated");
    let suffix = if collision == 0 {
        String::new()
    } else {
        format!("-{collision}")
    };
    format!("{stem}.{stamp}{suffix}.{marker}.{extension}")
}

#[cfg(test)]
mod tests {
    use super::utc_timestamp;
    use std::time::{Duration, UNIX_EPOCH};

    #[test]
    fn backup_timestamp_is_utc_and_filename_safe() {
        let date = UNIX_EPOCH + Duration::from_secs(1_728_000_000) + Duration::from_nanos(42);
        assert_eq!(
            utc_timestamp(date).unwrap(),
            "2024-10-04T00-00-00.000000042Z"
        );
    }
}
