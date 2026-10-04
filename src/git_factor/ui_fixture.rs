use super::*;
use crate::test_support::OrAbort as _;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

#[derive(Clone, Copy, Debug)]
pub(in crate::git_factor::ui) enum Marker {
    Apply,
    Both,
    File,
    Merge,
    Missing,
    NotRepository,
}
impl Marker {
    pub(in crate::git_factor::ui) const fn expected(self) -> bool {
        matches!(self, Self::Apply | Self::Both | Self::Merge)
    }
}
pub(in crate::git_factor::ui) struct Fixture(TempDir);
impl Fixture {
    pub(in crate::git_factor::ui) fn context(&self) -> Ctx<'_> {
        Ctx {
            cwd: self.0.path().to_path_buf(),
            env: &REAL_ENV,
            fs: &REAL_FS,
            io: &REAL_IO,
            runner: &REAL_RUNNER,
        }
    }
    pub(in crate::git_factor::ui) fn directory(&self) -> &Path {
        self.0.path()
    }
    pub(in crate::git_factor::ui) fn new(marker: Marker) -> Self {
        let fixture = Self(TempDir::new().or_abort("rebase marker resources"));
        if !matches!(marker, Marker::NotRepository) {
            let initialized = REAL_RUNNER
                .output("git", &["init", "--quiet"], &[], fixture.directory())
                .or_abort("native init");
            assert!(initialized.status.success(), "native init: {initialized:?}");
        }
        let admin = fixture.directory().join(".git");
        match marker {
            Marker::Apply => {
                fs::create_dir_all(admin.join("rebase-apply")).or_abort("apply marker");
            }
            Marker::Merge => {
                fs::create_dir_all(admin.join("rebase-merge")).or_abort("merge marker");
            }
            Marker::Both => {
                fs::create_dir_all(admin.join("rebase-apply")).or_abort("apply marker");
                fs::create_dir_all(admin.join("rebase-merge")).or_abort("merge marker");
            }
            Marker::File => fs::write(admin.join("rebase-merge"), b"ordinary file")
                .or_abort("non-directory marker"),
            Marker::Missing | Marker::NotRepository => {}
        }
        fs::write(fixture.directory().join("user"), b"protected user bytes")
            .or_abort("protected bytes");
        fixture
    }
}
