//! Saving over an existing file with `ReplaceFileW`, so the file keeps its identity.
//!
//! A plain rename puts the temporary file in place of the target, and the result has the
//! temporary file's attributes and permissions. `ReplaceFileW` instead keeps the target's
//! attributes (hidden, system...), ACL, creation time and short name, and gives it the new
//! content. It is the documented way to save a document atomically on Windows.

use std::io;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use bracket_core::document::FileReplacer;
use windows_sys::Win32::Storage::FileSystem::{
    REPLACEFILE_IGNORE_ACL_ERRORS, REPLACEFILE_IGNORE_MERGE_ERRORS, ReplaceFileW,
};

pub struct WindowsReplacer;

impl FileReplacer for WindowsReplacer {
    fn replace(&self, temp: &Path, target: &Path) -> io::Result<()> {
        // ReplaceFileW needs an existing target; a new file is simply renamed into place.
        if !target.exists() {
            return std::fs::rename(temp, target);
        }
        replace_file(temp, target)
    }
}

/// Calls `ReplaceFileW`. Paths are canonicalized (`\\?\` form), so long paths work too.
#[allow(unsafe_code)]
fn replace_file(temp: &Path, target: &Path) -> io::Result<()> {
    let target = wide(&std::fs::canonicalize(target)?);
    let temp = wide(&std::fs::canonicalize(temp)?);
    // If merging attributes or the ACL fails, the content is still replaced: losing an
    // attribute is better than failing to save. Without a backup file name nothing else is
    // left behind.
    let flags = REPLACEFILE_IGNORE_MERGE_ERRORS | REPLACEFILE_IGNORE_ACL_ERRORS;
    // SAFETY: `target` and `temp` are NUL-terminated UTF-16 strings that live until the call
    // returns. The backup name, exclude and reserved arguments may be null, as documented for
    // ReplaceFileW. No other memory is shared with the call.
    let ok = unsafe {
        ReplaceFileW(
            target.as_ptr(),
            temp.as_ptr(),
            std::ptr::null(),
            flags,
            std::ptr::null(),
            std::ptr::null(),
        )
    };
    if ok == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn wide(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use bracket_core::document::{RenameReplacer, write_bytes_atomic};
    use std::fs;
    use std::os::windows::fs::MetadataExt;
    use std::path::PathBuf;
    use std::process::Command;

    const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("bracket-replace-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn hidden_file(dir: &Path) -> PathBuf {
        let path = dir.join("hidden.txt");
        fs::write(&path, "old").unwrap();
        let status = Command::new("attrib")
            .arg("+h")
            .arg(&path)
            .status()
            .unwrap();
        assert!(status.success());
        assert_ne!(
            fs::metadata(&path).unwrap().file_attributes() & FILE_ATTRIBUTE_HIDDEN,
            0
        );
        path
    }

    #[test]
    fn replace_keeps_attributes_and_creation_time() {
        let dir = temp_dir("keeps");
        let path = hidden_file(&dir);
        let created = fs::metadata(&path).unwrap().creation_time();
        std::thread::sleep(std::time::Duration::from_millis(50));

        write_bytes_atomic(&path, b"new", &WindowsReplacer).unwrap();

        let meta = fs::metadata(&path).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"new");
        assert_ne!(
            meta.file_attributes() & FILE_ATTRIBUTE_HIDDEN,
            0,
            "still hidden"
        );
        assert_eq!(meta.creation_time(), created, "same creation time");
        assert_eq!(
            fs::read_dir(&dir).unwrap().count(),
            1,
            "no temporary file left"
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn rename_loses_the_attributes_that_replace_keeps() {
        // Documents why WindowsReplacer exists.
        let dir = temp_dir("rename");
        let path = hidden_file(&dir);
        write_bytes_atomic(&path, b"new", &RenameReplacer).unwrap();
        assert_eq!(
            fs::metadata(&path).unwrap().file_attributes() & FILE_ATTRIBUTE_HIDDEN,
            0
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn new_files_are_created() {
        let dir = temp_dir("new");
        let path = dir.join("new.txt");
        write_bytes_atomic(&path, b"hello", &WindowsReplacer).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"hello");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn failure_leaves_the_target_untouched() {
        let dir = temp_dir("readonly");
        let path = dir.join("locked.txt");
        fs::write(&path, "keep me").unwrap();
        let mut perms = fs::metadata(&path).unwrap().permissions();
        perms.set_readonly(true);
        fs::set_permissions(&path, perms).unwrap();

        assert!(write_bytes_atomic(&path, b"lost", &WindowsReplacer).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"keep me");
        assert_eq!(
            fs::read_dir(&dir).unwrap().count(),
            1,
            "temporary file removed"
        );

        let status = Command::new("attrib")
            .arg("-r")
            .arg(&path)
            .status()
            .unwrap();
        assert!(status.success());
        fs::remove_dir_all(&dir).unwrap();
    }
}
