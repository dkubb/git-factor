use crate::non_empty_string::NonEmptyString;
use std::path::{Path, PathBuf};

use clap::Parser;

use super::todo::TodoSha;

/// Strict sequence editor for git rebase todo lists.
#[derive(Parser)]
#[command(bin_name = "git-sequence-editor", version)]
pub(in crate::git_sequence_editor) struct Cli {
    /// Short SHA(s) to set to `drop` in the todo list.
    #[arg(long = "drop", value_name = "SHA")]
    drop: Vec<TodoSha>,

    /// Short SHA(s) to set to `edit` in the todo list.
    #[arg(long = "edit", value_name = "SHA")]
    edit: Vec<TodoSha>,

    /// Command to insert as `exec` after the targeted factor span tip.
    #[arg(long = "factor-begin", value_name = "COMMAND")]
    factor_begin: Option<NonEmptyString>,

    /// Command to insert as `exec` before the factor span begins.
    #[arg(long = "factor-preflight", value_name = "COMMAND")]
    factor_preflight: Option<NonEmptyString>,

    /// Short SHA after which the factor-session exec/break lines are inserted.
    #[arg(long = "factor-target", value_name = "SHA")]
    factor_target: Option<TodoSha>,

    /// Path to the rebase todo file (provided by git).
    #[arg(value_name = "FILE")]
    file: PathBuf,

    /// Short SHA(s) to set to `pick` in the todo list.
    #[arg(long = "pick", value_name = "SHA")]
    pick: Vec<TodoSha>,
}

impl Cli {
    /// Returns requested `drop` SHAs.
    pub(in crate::git_sequence_editor) fn drop(&self) -> &[TodoSha] {
        &self.drop
    }

    /// Returns requested `edit` SHAs.
    pub(in crate::git_sequence_editor) fn edit(&self) -> &[TodoSha] {
        &self.edit
    }

    /// Returns the requested factor begin command.
    pub(in crate::git_sequence_editor) const fn factor_begin(&self) -> Option<&NonEmptyString> {
        self.factor_begin.as_ref()
    }

    /// Returns the requested factor preflight command.
    pub(in crate::git_sequence_editor) const fn factor_preflight(&self) -> Option<&NonEmptyString> {
        self.factor_preflight.as_ref()
    }

    /// Returns the requested factor target SHA.
    pub(in crate::git_sequence_editor) const fn factor_target(&self) -> Option<&TodoSha> {
        self.factor_target.as_ref()
    }

    /// Returns the todo file path.
    pub(in crate::git_sequence_editor) fn file(&self) -> &Path {
        self.file.as_path()
    }

    /// Constructs a `Cli` value for unit tests.
    #[cfg(test)]
    pub(in crate::git_sequence_editor) const fn for_tests(
        drop: Vec<TodoSha>,
        edit: Vec<TodoSha>,
        file: PathBuf,
        pick: Vec<TodoSha>,
    ) -> Self {
        Self {
            factor_begin: None,
            factor_preflight: None,
            factor_target: None,
            drop,
            edit,
            file,
            pick,
        }
    }

    /// Constructs a `Cli` value with factor-mode fields for unit tests.
    #[cfg(test)]
    pub(in crate::git_sequence_editor) const fn for_tests_with_factor(
        factor_begin: Option<NonEmptyString>,
        factor_preflight: Option<NonEmptyString>,
        factor_target: Option<TodoSha>,
        drop: Vec<TodoSha>,
        edit: Vec<TodoSha>,
        file: PathBuf,
        pick: Vec<TodoSha>,
    ) -> Self {
        Self {
            drop,
            edit,
            factor_begin,
            factor_preflight,
            factor_target,
            file,
            pick,
        }
    }

    /// Returns requested `pick` SHAs.
    pub(in crate::git_sequence_editor) fn pick(&self) -> &[TodoSha] {
        &self.pick
    }
}

#[cfg(test)]
mod proptests {
    use std::path::PathBuf;

    use super::super::todo::FULL_HEX_SHA_LEN;
    use clap::Parser as _;
    use proptest::collection::vec;
    use proptest::prelude::*;
    use proptest::sample::select;

    use super::{Cli, TodoSha};
    const HEX_ALPHABET: &str = "0123456789abcdefABCDEF";

    fn token() -> impl Strategy<Value = String> {
        let alphabet = HEX_ALPHABET.chars().collect::<Vec<_>>();
        vec(select(alphabet), 1..(FULL_HEX_SHA_LEN + 1))
            .prop_map(|chars| chars.into_iter().collect())
    }

    fn push_flag_values(args: &mut Vec<String>, flag: &str, values: &[String]) {
        for value in values {
            args.push(flag.to_owned());
            args.push(value.clone());
        }
    }

    proptest! {
        #[test]
        fn proptest_cli_accessors_round_trip(
            drop in vec(token(), 0..4),
            edit in vec(token(), 0..4),
            pick in vec(token(), 0..4),
            file in token(),
        ) {
            let mut args = vec!["git-sequence-editor".to_owned()];

            push_flag_values(&mut args, "--drop", &drop);
            push_flag_values(&mut args, "--edit", &edit);
            push_flag_values(&mut args, "--pick", &pick);
            args.push(file.clone());

            let cli = Cli::parse_from(args);

            prop_assert_eq!(
                cli.drop()
                    .iter()
                    .map(TodoSha::as_str)
                    .collect::<Vec<_>>(),
                drop.iter().map(String::as_str).collect::<Vec<_>>()
            );
            prop_assert_eq!(
                cli.edit()
                    .iter()
                    .map(TodoSha::as_str)
                    .collect::<Vec<_>>(),
                edit.iter().map(String::as_str).collect::<Vec<_>>()
            );
            prop_assert_eq!(
                cli.pick()
                    .iter()
                    .map(TodoSha::as_str)
                    .collect::<Vec<_>>(),
                pick.iter().map(String::as_str).collect::<Vec<_>>()
            );
            let expected_file = PathBuf::from(file);
            prop_assert_eq!(cli.file(), expected_file.as_path());
        }
    }
}
