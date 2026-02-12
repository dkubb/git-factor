use std::path::{Path, PathBuf};

use clap::Parser;
use non_empty_string::NonEmptyString;

/// Strict sequence editor for git rebase todo lists.
#[derive(Parser)]
#[command(bin_name = "git-sequence-editor")]
pub(super) struct Cli {
    /// Short SHA(s) to set to `drop` in the todo list.
    #[arg(long = "drop", value_name = "SHA")]
    drop: Vec<NonEmptyString>,

    /// Short SHA(s) to set to `edit` in the todo list.
    #[arg(long = "edit", value_name = "SHA")]
    edit: Vec<NonEmptyString>,

    /// Path to the rebase todo file (provided by git).
    #[arg(value_name = "FILE")]
    file: PathBuf,

    /// Short SHA(s) to set to `pick` in the todo list.
    #[arg(long = "pick", value_name = "SHA")]
    pick: Vec<NonEmptyString>,
}

impl Cli {
    /// Returns requested `drop` SHAs.
    pub(super) fn drop(&self) -> &[NonEmptyString] {
        &self.drop
    }

    /// Returns requested `edit` SHAs.
    pub(super) fn edit(&self) -> &[NonEmptyString] {
        &self.edit
    }

    /// Returns the todo file path.
    pub(super) fn file(&self) -> &Path {
        self.file.as_path()
    }

    /// Constructs a `Cli` value for unit tests.
    #[cfg(test)]
    #[expect(
        clippy::missing_const_for_fn,
        reason = "test helper constructs heap-backed vectors"
    )]
    pub(super) fn for_tests(
        drop: Vec<NonEmptyString>,
        edit: Vec<NonEmptyString>,
        file: PathBuf,
        pick: Vec<NonEmptyString>,
    ) -> Self {
        Self {
            drop,
            edit,
            file,
            pick,
        }
    }

    /// Returns requested `pick` SHAs.
    pub(super) fn pick(&self) -> &[NonEmptyString] {
        &self.pick
    }
}
