use super::ass::hash;
use super::backup::{utc_timestamp, versioned_filename};
use super::{Result, error};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::SystemTime,
};

fn autosave_dir(path: &Path) -> Result<PathBuf> {
    let parent = path
        .parent()
        .ok_or_else(|| error("Subtitle path has no parent"))?;
    let directory = parent.join(".autosave");
    match fs::symlink_metadata(&directory) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            return Err(error("Autosave path must be a real directory"));
        }
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => match fs::create_dir(&directory) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                let metadata = fs::symlink_metadata(&directory)
                    .map_err(|e| error(format!("Cannot inspect autosave directory: {e}")))?;
                if metadata.file_type().is_symlink() || !metadata.is_dir() {
                    return Err(error("Autosave path must be a real directory"));
                }
            }
            Err(e) => return Err(error(format!("Cannot create autosave directory: {e}"))),
        },
        Err(e) => return Err(error(format!("Cannot inspect autosave directory: {e}"))),
    }
    Ok(directory)
}

pub(super) fn lock_for_edit(path: &Path) -> Result<fs::File> {
    let directory = autosave_dir(path)?;
    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| error("Subtitle filename is not UTF-8"))?;
    let lock_path = directory.join(format!(".{filename}.lock"));
    let lock = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(lock_path)
        .map_err(|e| error(format!("Cannot open subtitle edit lock: {e}")))?;
    lock.try_lock()
        .map_err(|e| error(format!("Subtitle is already locked for editing: {e}")))?;
    Ok(lock)
}

#[cfg(target_os = "linux")]
fn exchange(left: &Path, right: &Path) -> std::io::Result<()> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};

    unsafe extern "C" {
        fn renameat2(
            olddirfd: i32,
            oldpath: *const std::ffi::c_char,
            newdirfd: i32,
            newpath: *const std::ffi::c_char,
            flags: u32,
        ) -> i32;
    }

    let left = CString::new(left.as_os_str().as_bytes())
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "Invalid path"))?;
    let right = CString::new(right.as_os_str().as_bytes())
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "Invalid path"))?;
    const AT_FDCWD: i32 = -100;
    const RENAME_EXCHANGE: u32 = 2;
    // SAFETY: Both paths are valid NUL-terminated C strings and remain alive for the call.
    let result = unsafe {
        renameat2(
            AT_FDCWD,
            left.as_ptr(),
            AT_FDCWD,
            right.as_ptr(),
            RENAME_EXCHANGE,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(not(target_os = "linux"))]
fn exchange(_left: &Path, _right: &Path) -> std::io::Result<()> {
    Err(std::io::Error::other(
        "Atomic subtitle exchange requires Linux",
    ))
}

fn sync_directories(path: &Path, autosave_dir: &Path) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| error("Subtitle path has no parent"))?;
    fs::File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|e| error(format!("Cannot sync subtitle directory: {e}")))?;
    fs::File::open(autosave_dir)
        .and_then(|directory| directory.sync_all())
        .map_err(|e| error(format!("Cannot sync autosave directory: {e}")))?;
    Ok(())
}

fn exchange_and_validate(
    path: &Path,
    autosave_path: &Path,
    expected_revision: &str,
    output: &str,
) -> Result<()> {
    exchange(path, autosave_path).map_err(|e| {
        error(format!(
            "Cannot exchange subtitle with autosave at {}: {e}",
            autosave_path.display()
        ))
    })?;

    let directory = autosave_path
        .parent()
        .ok_or_else(|| error("Autosave path has no parent"))?;
    let displaced = fs::read(autosave_path).map_err(|e| {
        error(format!(
            "Subtitle was exchanged but displaced version at {} cannot be read: {e}",
            autosave_path.display()
        ))
    })?;
    if hash(&displaced) != expected_revision {
        let output_is_active = fs::read(path)
            .map(|current| hash(&current) == hash(output.as_bytes()))
            .unwrap_or(false);
        if output_is_active && exchange(path, autosave_path).is_ok() {
            sync_directories(path, directory).map_err(|e| {
                error(format!(
                    "Subtitle was restored but directory sync failed; inspect {}: {e}",
                    autosave_path.display()
                ))
            })?;
            return Err(error(format!(
                "Revision conflict: subtitle changed during edit; original restored and MCP candidate retained at {}",
                autosave_path.display()
            )));
        }
        sync_directories(path, directory).map_err(|e| {
            error(format!(
                "Subtitle versions were retained but directory sync failed; inspect {}: {e}",
                autosave_path.display()
            ))
        })?;
        return Err(error(format!(
            "Revision conflict: displaced subtitle retained at {}; inspect both versions",
            autosave_path.display()
        )));
    }
    sync_directories(path, directory).map_err(|e| {
        error(format!(
            "Subtitle was exchanged but directory sync failed; inspect {}: {e}",
            autosave_path.display()
        ))
    })?;
    Ok(())
}

pub(super) fn write_with_autosave(
    path: &Path,
    expected_revision: &str,
    output: &str,
) -> Result<PathBuf> {
    let directory = autosave_dir(path)?;
    let mut candidate = tempfile::NamedTempFile::new_in(&directory)
        .map_err(|e| error(format!("Cannot create autosave candidate: {e}")))?;
    candidate
        .as_file()
        .set_permissions(
            fs::metadata(path)
                .map_err(|e| error(format!("Cannot read subtitle permissions: {e}")))?
                .permissions(),
        )
        .map_err(|e| error(format!("Cannot preserve subtitle permissions: {e}")))?;
    candidate
        .write_all(output.as_bytes())
        .map_err(|e| error(format!("Cannot write autosave candidate: {e}")))?;
    candidate
        .as_file()
        .sync_all()
        .map_err(|e| error(format!("Cannot sync autosave candidate: {e}")))?;

    let current = fs::read(path).map_err(|e| error(format!("Cannot recheck subtitle: {e}")))?;
    if hash(&current) != expected_revision {
        return Err(error("Revision conflict: subtitle changed during edit"));
    }

    let stamp = utc_timestamp(SystemTime::now())?;
    let mut collision = 0;
    let autosave_path = loop {
        let destination = directory.join(versioned_filename(path, &stamp, "AUTOSAVE", collision));
        match candidate.persist_noclobber(&destination) {
            Ok(_) => break destination,
            Err(e) if e.error.kind() == std::io::ErrorKind::AlreadyExists => {
                candidate = e.file;
                collision += 1;
            }
            Err(e) => {
                return Err(error(format!(
                    "Cannot reserve autosave version: {}",
                    e.error
                )));
            }
        }
    };
    if let Err(e) = sync_directories(path, &directory) {
        let _ = fs::remove_file(&autosave_path);
        return Err(e);
    }
    exchange_and_validate(path, &autosave_path, expected_revision, output)?;
    Ok(autosave_path)
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn edit_lock_rejects_a_second_writer() {
        let directory = tempfile::tempdir().unwrap();
        let subtitle = directory.path().join("episode.ass");
        fs::write(&subtitle, "Original").unwrap();
        let first = lock_for_edit(&subtitle).unwrap();
        assert!(lock_for_edit(&subtitle).is_err());
        drop(first);
        assert!(lock_for_edit(&subtitle).is_ok());
    }

    #[test]
    fn exchange_keeps_external_edit_when_revision_changes_at_commit() {
        let directory = tempfile::tempdir().unwrap();
        let subtitle = directory.path().join("episode.ass");
        let autosave = directory.path().join("episode.AUTOSAVE.ass");
        fs::write(&subtitle, "Original").unwrap();
        fs::write(&autosave, "MCP edit").unwrap();
        let expected_revision = hash(b"Original");
        fs::write(&subtitle, "External edit").unwrap();

        let result = exchange_and_validate(&subtitle, &autosave, &expected_revision, "MCP edit");
        assert!(result.is_err());
        assert_eq!(fs::read_to_string(&subtitle).unwrap(), "External edit");
        assert_eq!(fs::read_to_string(&autosave).unwrap(), "MCP edit");
    }
}
