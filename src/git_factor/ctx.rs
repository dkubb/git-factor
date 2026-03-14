use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Shared filesystem implementation for the real CLI.
pub(in crate::git_factor) static REAL_FS: RealFs = RealFs;

/// Filesystem access used by state helpers.
pub(in crate::git_factor) trait Fs {
    /// Canonicalizes a path.
    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf>;

    /// Creates a directory and all missing parent components.
    fn create_dir_all(&self, path: &Path) -> io::Result<()>;

    /// Returns true when the path exists.
    fn exists(&self, path: &Path) -> bool;

    /// Returns true when the path exists and is a directory.
    fn is_dir(&self, path: &Path) -> bool;

    /// Reads a UTF-8 text file into a string.
    fn read_to_string(&self, path: &Path) -> io::Result<String>;

    /// Removes a directory tree.
    fn remove_dir_all(&self, path: &Path) -> io::Result<()>;

    /// Removes a file.
    fn remove_file(&self, path: &Path) -> io::Result<()>;

    /// Writes a UTF-8 text file from a string.
    fn write_string(&self, path: &Path, content: &str) -> io::Result<()>;
}

/// Production [`Fs`] implementation.
pub(in crate::git_factor) struct RealFs;

impl Fs for RealFs {
    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        fs::canonicalize(path)
    }

    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        fs::create_dir_all(path)
    }

    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn is_dir(&self, path: &Path) -> bool {
        path.is_dir()
    }

    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        fs::read_to_string(path)
    }

    fn remove_dir_all(&self, path: &Path) -> io::Result<()> {
        fs::remove_dir_all(path)
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        fs::remove_file(path)
    }

    fn write_string(&self, path: &Path, content: &str) -> io::Result<()> {
        fs::write(path, content)
    }
}

/// Execution context for state operations.
#[expect(
    clippy::field_scoped_visibility_modifiers,
    reason = "field must be visible to sibling modules within git_factor"
)]
pub(in crate::git_factor) struct Ctx<'fs> {
    /// Filesystem access.
    pub(in crate::git_factor) fs: &'fs dyn Fs,
}

#[cfg(test)]
mod tests {
    use super::{Ctx, REAL_FS};
    use crate::git_factor::ctx::Fs as _;
    use crate::test_support::OrAbort as _;
    use std::path::Path;
    use tempfile::TempDir;

    #[test]
    fn real_fs_supports_directory_and_file_lifecycle() {
        let dir = TempDir::new().or_abort("tempdir");
        let nested = dir.path().join("nested");
        let file = nested.join("note.txt");

        REAL_FS
            .create_dir_all(&nested)
            .or_abort("create dir should succeed");
        assert!(REAL_FS.exists(&nested));
        assert!(REAL_FS.is_dir(&nested));

        REAL_FS
            .write_string(&file, "hello\n")
            .or_abort("write should succeed");
        assert_eq!(
            REAL_FS
                .read_to_string(&file)
                .or_abort("read should succeed"),
            "hello\n"
        );

        let canonical = REAL_FS
            .canonicalize(&file)
            .or_abort("canonicalize should succeed");
        assert!(canonical.ends_with(Path::new("note.txt")));

        REAL_FS
            .remove_file(&file)
            .or_abort("remove file should succeed");
        assert!(!REAL_FS.exists(&file));

        REAL_FS
            .remove_dir_all(&nested)
            .or_abort("remove dir should succeed");
        assert!(!REAL_FS.exists(&nested));
    }

    #[test]
    fn ctx_holds_fs_reference() {
        let ctx = Ctx { fs: &REAL_FS };
        let dir = TempDir::new().or_abort("tempdir");
        let path = dir.path().join("ctx.txt");

        ctx.fs
            .write_string(&path, "ctx\n")
            .or_abort("ctx write should succeed");

        let content = ctx
            .fs
            .read_to_string(&path)
            .or_abort("ctx read should succeed");
        assert_eq!(content, "ctx\n");
    }
}
