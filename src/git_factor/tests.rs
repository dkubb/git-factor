use alloc::collections::VecDeque;
use core::cell::RefCell;
use std::collections::HashMap;
use std::env;
use std::ffi::OsString;
use std::panic::resume_unwind;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Mutex;
use std::thread;

use super::*;
use crate::git_factor::validation::validate_not_merge;
use tempfile::TempDir;

/// Generates an `Fs` trait method that delegates to `REAL_FS`.
macro_rules! fs_delegate {
    (canonicalize) => {
        fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
            REAL_FS.canonicalize(path)
        }
    };
    (create_dir_all) => {
        fn create_dir_all(&self, path: &Path) -> io::Result<()> {
            REAL_FS.create_dir_all(path)
        }
    };
    (exists) => {
        fn exists(&self, path: &Path) -> bool {
            REAL_FS.exists(path)
        }
    };
    (is_dir) => {
        fn is_dir(&self, path: &Path) -> bool {
            REAL_FS.is_dir(path)
        }
    };
    (read_to_string) => {
        fn read_to_string(&self, path: &Path) -> io::Result<String> {
            REAL_FS.read_to_string(path)
        }
    };
    (remove_dir_all) => {
        fn remove_dir_all(&self, path: &Path) -> io::Result<()> {
            REAL_FS.remove_dir_all(path)
        }
    };
    (remove_file) => {
        fn remove_file(&self, path: &Path) -> io::Result<()> {
            REAL_FS.remove_file(path)
        }
    };
    (write_string) => {
        fn write_string(&self, path: &Path, content: &str) -> io::Result<()> {
            REAL_FS.write_string(path, content)
        }
    };
}

const TEST_COMMIT_META: &str = "a\0b\0c\0d\0e\0f";
const TEST_COMMIT_ENVS: [(&str, &str); 6] = [
    ("GIT_AUTHOR_NAME", "a"),
    ("GIT_AUTHOR_EMAIL", "b"),
    ("GIT_AUTHOR_DATE", "c"),
    ("GIT_COMMITTER_NAME", "d"),
    ("GIT_COMMITTER_EMAIL", "e"),
    ("GIT_COMMITTER_DATE", "f"),
];
const SHA_LEN: usize = COMMIT_SHA_HEX_LEN;

/// Valid 40-char hex tree hash for the "expected" / "same" tree in tests.
const TREE_EXPECTED: &str = "dddddddddddddddddddddddddddddddddddddddd";

/// The same tree hash with trailing newline (matching git output format).
const TREE_EXPECTED_NL: &str = "dddddddddddddddddddddddddddddddddddddddd\n";

/// Valid 40-char hex tree hash for a "different" / "actual" / "restored" tree.
const TREE_DIFFERENT: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";

/// The different tree hash with trailing newline.
const TREE_DIFFERENT_NL: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee\n";

/// Valid 40-char hex tree hash for rehydrate test scenarios.
const TREE_REHYDRATE: &str = "ffffffffffffffffffffffffffffffffffffffff";

/// The rehydrate tree hash with trailing newline.
const TREE_REHYDRATE_NL: &str = "ffffffffffffffffffffffffffffffffffffffff\n";

#[cfg(unix)]
use std::os::unix::ffi::OsStringExt as _;

#[cfg(unix)]
struct NonUtf8Fs;

#[cfg(unix)]
impl Fs for NonUtf8Fs {
    fn canonicalize(&self, _path: &Path) -> io::Result<PathBuf> {
        let mut bytes = b"/tmp/".to_vec();
        bytes.push(0xff);
        bytes.extend_from_slice(b"/bin/git-factor");
        Ok(PathBuf::from(OsString::from_vec(bytes)))
    }

    fs_delegate!(create_dir_all);
    fs_delegate!(exists);
    fs_delegate!(is_dir);
    fs_delegate!(read_to_string);
    fs_delegate!(remove_dir_all);
    fs_delegate!(remove_file);
    fs_delegate!(write_string);
}

struct FailingRequiresRebaseWriteFs;

impl Fs for FailingRequiresRebaseWriteFs {
    fs_delegate!(canonicalize);
    fs_delegate!(create_dir_all);
    fs_delegate!(exists);
    fs_delegate!(is_dir);
    fs_delegate!(read_to_string);
    fs_delegate!(remove_dir_all);
    fs_delegate!(remove_file);

    fn write_string(&self, path: &Path, content: &str) -> io::Result<()> {
        if path
            .file_name()
            .is_some_and(|name| name == "requires_rebase")
        {
            return Err(io::Error::other("requires_rebase write failed"));
        }
        REAL_FS.write_string(path, content)
    }
}

struct FailingIsRootWriteFs;

impl Fs for FailingIsRootWriteFs {
    fs_delegate!(canonicalize);
    fs_delegate!(create_dir_all);
    fs_delegate!(exists);
    fs_delegate!(is_dir);
    fs_delegate!(read_to_string);
    fs_delegate!(remove_dir_all);
    fs_delegate!(remove_file);

    fn write_string(&self, path: &Path, content: &str) -> io::Result<()> {
        if path.file_name().is_some_and(|name| name == "is_root") {
            return Err(io::Error::other("is_root write failed"));
        }
        REAL_FS.write_string(path, content)
    }
}

struct FailingWriteForFileFs {
    file_name: &'static str,
    message: &'static str,
}

impl Fs for FailingWriteForFileFs {
    fs_delegate!(canonicalize);
    fs_delegate!(create_dir_all);
    fs_delegate!(exists);
    fs_delegate!(is_dir);
    fs_delegate!(read_to_string);
    fs_delegate!(remove_dir_all);
    fs_delegate!(remove_file);

    fn write_string(&self, path: &Path, content: &str) -> io::Result<()> {
        if path.file_name().is_some_and(|name| name == self.file_name) {
            return Err(io::Error::other(self.message));
        }
        REAL_FS.write_string(path, content)
    }
}

struct CorruptSplitCountWriteFs;

impl Fs for CorruptSplitCountWriteFs {
    fs_delegate!(canonicalize);
    fs_delegate!(create_dir_all);
    fs_delegate!(exists);
    fs_delegate!(is_dir);
    fs_delegate!(read_to_string);
    fs_delegate!(remove_dir_all);
    fs_delegate!(remove_file);

    fn write_string(&self, path: &Path, content: &str) -> io::Result<()> {
        if path.file_name().is_some_and(|name| name == "split_count") {
            return REAL_FS.write_string(path, "not-a-number\n");
        }
        REAL_FS.write_string(path, content)
    }
}

struct NthReadFailureFs {
    fail_at: usize,
    file_name: &'static str,
    message: &'static str,
    reads: Mutex<usize>,
}

impl NthReadFailureFs {
    fn new(file_name: &'static str, fail_at: usize, message: &'static str) -> Self {
        Self {
            file_name,
            fail_at,
            message,
            reads: Mutex::new(0),
        }
    }
}

impl Fs for NthReadFailureFs {
    fs_delegate!(canonicalize);
    fs_delegate!(create_dir_all);
    fs_delegate!(exists);
    fs_delegate!(is_dir);

    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        if path.file_name().is_some_and(|name| name == self.file_name) {
            let mut reads = self
                .reads
                .lock()
                .map_err(|error| io::Error::other(format!("nth-read lock: {error}")))?;
            *reads = reads.checked_add(1).or_abort("counter should not overflow");
            if *reads == self.fail_at {
                return Err(io::Error::other(self.message));
            }
        }
        REAL_FS.read_to_string(path)
    }

    fs_delegate!(remove_dir_all);
    fs_delegate!(remove_file);
    fs_delegate!(write_string);
}
