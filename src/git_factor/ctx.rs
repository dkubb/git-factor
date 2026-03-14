use std::fs;
use std::io;
use std::path::Path;

/// Shared filesystem implementation for the real CLI.
pub(in crate::git_factor) static REAL_FS: RealFs = RealFs;

/// Filesystem access used by state helpers.
pub(in crate::git_factor) trait Fs {
    /// Reads a UTF-8 text file into a string.
    fn read_to_string(&self, path: &Path) -> io::Result<String>;

    /// Writes a UTF-8 text file from a string.
    fn write_string(&self, path: &Path, content: &str) -> io::Result<()>;
}

/// Production [`Fs`] implementation.
pub(in crate::git_factor) struct RealFs;

impl Fs for RealFs {
    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        fs::read_to_string(path)
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
    use tempfile::TempDir;

    #[test]
    fn real_fs_round_trips_file_contents() {
        let dir = TempDir::new().or_abort("tempdir");
        let path = dir.path().join("note.txt");

        REAL_FS
            .write_string(&path, "hello\n")
            .or_abort("write should succeed");

        let content = REAL_FS
            .read_to_string(&path)
            .or_abort("read should succeed");
        assert_eq!(content, "hello\n");
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
