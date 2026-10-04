use crate::non_empty_string::NonEmptyString;
use clap::{ArgAction, Args, Parser};

/// Session-control flags that query or abort state.
#[derive(Args)]
struct SessionQueryFlags {
    /// Abort the current factor session and restore the repository.
    #[arg(long = "abort", help_heading = "Session Control")]
    abort: bool,

    /// Show status for the current factor session.
    ///
    /// Prints session details when active, otherwise reports no active session.
    #[arg(long = "status", help_heading = "Session Control")]
    status: bool,
}

/// Session-control flags that advance an active split flow.
#[derive(Args)]
struct SessionProgressFlags {
    /// Resume replay and recovery, or submit staged changes with --message.
    ///
    /// Without a message, resumes the current replay, captures its completed
    /// checkpoint, and opens the remaining change for selection. With a
    /// message, validates the staged atom and captures a new checkpoint.
    #[arg(long = "continue", help_heading = "Session Control")]
    r#continue: bool,

    /// Validate and capture all remaining changes, then finish the session.
    ///
    /// Uses the original selected tip's message when --message is omitted.
    #[arg(long = "finish", help_heading = "Session Control")]
    finish: bool,

    /// Unstage the current candidate in an open selection.
    ///
    /// Preserves every previously completed checkpoint.
    #[arg(long = "retry", help_heading = "Session Control")]
    retry: bool,
}

/// Split one git commit or contiguous commit span into smaller atomic commits.
#[derive(Parser)]
#[expect(
    clippy::arbitrary_source_item_ordering,
    reason = "keep --version grouped with help in CLI output"
)]
#[command(
    bin_name = "git-factor",
    version,
    disable_version_flag = true,
    disable_help_subcommand = true,
    after_long_help = "\
WORKFLOW:
  1. Start a session:     git factor --gate test 'cargo test' HEAD
  2. Select an atom:     git add --patch -- <path>
  3. Capture the atom:   git factor --message 'Add login'
  4. Repeat steps 2-3 on the automatically exposed remainder.
  5. Finish remaining:  git factor --finish

  Each successful split finishes its rebase before opening the next one.
  Gates validate each tree independently of the unstaged remainder.
  Passing command/tree proofs are reused; commit hooks still validate messages.
  Gates must be deterministic tree checks and must not depend on commit metadata, history, or messages.
  Resolve replay conflicts or gate failures, then run git factor --continue.
  Use --retry only in an open selection to unstage its current candidate.
  During replay, use --continue or --abort to return to the latest checkpoint.
  Earlier successful splits remain captured.
  Legacy sessions must be completed with their originating version.

EXAMPLES:
  Split a commit using ordered named and legacy gates:
    git factor --gate test 'cargo test' --exec 'cargo fmt --check' HEAD

  Split an inclusive contiguous span:
    git factor --gate check 'make check' HEAD~2 HEAD

  Use git-native exclusive-start range syntax:
    git factor --exec 'npm test' HEAD~3..HEAD

  Use git-native inclusive-start range syntax:
    git factor --exec 'npm test' HEAD~3^..HEAD

  Submit a multi-paragraph message (also accepts --continue):
    git factor --message 'Add login' --message 'Support OAuth2 sessions.'

  Resume replay after a conflict or gate failure:
    git factor --continue

  Finish with the original selected tip's message:
    git factor --finish

  Show active-session status:
    git factor --status"
)]
pub(in crate::git_factor) struct Cli {
    /// Commit or span to split (e.g. SHA, A B, A..B, A^..B).
    ///
    /// Accepts full or short SHAs, branch names, and git revision syntax.
    /// `<rev>` splits one commit. `<start> <end>` splits one inclusive span.
    /// `<start>..<end>` uses git's exclusive-start range semantics, and
    /// `<start>^..<end>` is the git-native inclusive form. Symmetric diff
    /// (`...`) is not supported.
    #[arg(value_name = "COMMIT")]
    commits: Vec<NonEmptyString>,

    /// Print version information and exit.
    #[arg(
        short = 'v',
        long = "version",
        action = ArgAction::Version
    )]
    version: Option<bool>,

    /// Shell command(s) to run as the deterministic validation gate.
    ///
    /// Multiple --exec and --gate flags run individually in supplied order.
    /// Passing command/tree proofs are reused. Commands must have valid bash
    /// syntax, inspect only the tree, and preserve its bytes and commit metadata.
    #[arg(long = "exec", value_name = "COMMAND", help_heading = "Start Options")]
    exec: Vec<NonEmptyString>,

    /// Ordered named tree checks, recorded in Gate-<name> trailers.
    ///
    /// Names start with an ASCII letter and contain ASCII letters, digits, or
    /// hyphens. Names are unique without regard to case. Commands obey the
    /// same tree-only contract as --exec.
    #[arg(long = "gate", num_args = 2, value_names = ["NAME", "COMMAND"], action = ArgAction::Append, help_heading = "Start Options")]
    gate: Vec<NonEmptyString>,

    /// Commit message for the split commit.
    ///
    /// Submitting a message validates and captures the staged atom. Without a
    /// message, --continue resumes replay. Optional with --finish (defaults to
    /// the original selected tip message). Multiple --message flags produce separate
    /// paragraphs, matching git commit behavior.
    #[arg(
        long = "message",
        short = 'm',
        value_name = "MSG",
        help_heading = "Commit Options"
    )]
    message: Vec<NonEmptyString>,

    /// Flags that progress an active session.
    #[command(flatten)]
    session_progress: SessionProgressFlags,

    /// Flags for aborting and inspecting session state.
    #[command(flatten)]
    session_query: SessionQueryFlags,
}

impl Cli {
    /// Returns whether `--abort` was requested.
    pub(in crate::git_factor) const fn abort(&self) -> bool {
        self.session_query.abort
    }

    /// Returns requested commit refs/ranges.
    pub(in crate::git_factor) fn commits(&self) -> &[NonEmptyString] {
        &self.commits
    }

    /// Returns whether `--continue` was requested.
    pub(in crate::git_factor) const fn continue_flag(&self) -> bool {
        self.session_progress.r#continue
    }

    /// Returns requested `--exec` commands.
    pub(in crate::git_factor) fn exec(&self) -> &[NonEmptyString] {
        &self.exec
    }

    /// Returns whether `--finish` was requested.
    pub(in crate::git_factor) const fn finish(&self) -> bool {
        self.session_progress.finish
    }

    /// Returns paired named gate values in CLI occurrence order.
    pub(in crate::git_factor) fn gate(&self) -> &[NonEmptyString] {
        &self.gate
    }

    /// Returns requested commit messages.
    pub(in crate::git_factor) fn message(&self) -> &[NonEmptyString] {
        &self.message
    }

    /// Returns whether `--retry` was requested.
    pub(in crate::git_factor) const fn retry(&self) -> bool {
        self.session_progress.retry
    }

    /// Returns whether `--status` was requested.
    pub(in crate::git_factor) const fn status(&self) -> bool {
        self.session_query.status
    }
}

#[cfg(test)]
mod proptests {
    mod cli {
        mod gate {
            use crate::non_empty_string::NonEmptyString;
            use proptest::collection::vec;
            use proptest::prelude::*;
            proptest! {
                #[test]
                fn preserves_generated_named_pair_order(pairs in vec(("[a-z]{1,12}", "[A-Za-z0-9 $;]{1,24}"), 0..5)) {
                    use clap::Parser as _;
                    use crate::test_support::OrAbort as _;
                    let mut args = vec!["git-factor".to_owned()];
                    let mut expected = Vec::new();
                    for (name, command) in pairs {
                        args.extend(["--gate".to_owned(), name.clone(), command.clone()]);
                        expected.extend([name, command]);
                    }
                    let parsed = super::super::super::Cli::try_parse_from(args).or_abort("CLI");
                    let actual = parsed.gate();
                    prop_assert_eq!(actual.iter().map(NonEmptyString::as_str).collect::<Vec<_>>(), expected.iter().map(String::as_str).collect::<Vec<_>>());
                }
            }
        }
    }
    use clap::Parser as _;
    use proptest::collection::vec;
    use proptest::prelude::*;
    use proptest::sample::select;

    use crate::non_empty_string::NonEmptyString;

    use super::Cli;

    fn token() -> impl Strategy<Value = String> {
        let alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789._/:"
            .chars()
            .collect::<Vec<_>>();
        vec(select(alphabet), 1..17).prop_map(|chars| chars.into_iter().collect())
    }

    proptest! {
        #[test]
        fn proptest_cli_accessors_round_trip(
            abort in any::<bool>(),
            status in any::<bool>(),
            continue_flag in any::<bool>(),
            retry in any::<bool>(),
            finish in any::<bool>(),
            commits in vec(token(), 0..4),
            execs in vec(token(), 0..4),
            messages in vec(token(), 0..4),
        ) {
            let mut args = vec!["git-factor".to_owned()];

            if abort {
                args.push("--abort".to_owned());
            }
            if status {
                args.push("--status".to_owned());
            }
            if continue_flag {
                args.push("--continue".to_owned());
            }
            if retry {
                args.push("--retry".to_owned());
            }
            if finish {
                args.push("--finish".to_owned());
            }
            for exec in &execs {
                args.push("--exec".to_owned());
                args.push(exec.clone());
            }
            for message in &messages {
                args.push("--message".to_owned());
                args.push(message.clone());
            }
            args.extend(commits.iter().cloned());

            let cli = Cli::parse_from(args);

            prop_assert_eq!(cli.abort(), abort);
            prop_assert_eq!(cli.status(), status);
            prop_assert_eq!(cli.continue_flag(), continue_flag);
            prop_assert_eq!(cli.retry(), retry);
            prop_assert_eq!(cli.finish(), finish);
            prop_assert_eq!(
                cli.commits()
                    .iter()
                    .map(NonEmptyString::as_str)
                    .collect::<Vec<_>>(),
                commits.iter().map(String::as_str).collect::<Vec<_>>()
            );
            prop_assert_eq!(
                cli.exec()
                    .iter()
                    .map(NonEmptyString::as_str)
                    .collect::<Vec<_>>(),
                execs.iter().map(String::as_str).collect::<Vec<_>>()
            );
            prop_assert_eq!(
                cli.message()
                    .iter()
                    .map(NonEmptyString::as_str)
                    .collect::<Vec<_>>(),
                messages.iter().map(String::as_str).collect::<Vec<_>>()
            );
        }
    }
}

#[cfg(test)]
mod tests {
    mod cli {
        mod gate {
            #[test]
            fn preserves_named_pairs_in_occurrence_order() {
                use crate::non_empty_string::NonEmptyString;
                use crate::test_support::OrAbort as _;
                use clap::Parser as _;
                let parsed = super::super::super::Cli::try_parse_from([
                    "git-factor",
                    "--gate",
                    "test",
                    "printf one",
                    "--exec",
                    "printf middle",
                    "--gate",
                    "lint",
                    "printf two",
                ])
                .or_abort("CLI");
                let actual = parsed.gate();
                assert_eq!(
                    actual
                        .iter()
                        .map(NonEmptyString::as_str)
                        .collect::<Vec<_>>(),
                    ["test", "printf one", "lint", "printf two"]
                );
            }
        }
    }
}
