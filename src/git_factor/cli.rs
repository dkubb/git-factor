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
    /// Continue by committing the currently staged changes.
    ///
    /// Staged changes must contain the next atomic split and the exec gate
    /// must pass. After committing, remaining changes are restored as
    /// unstaged changes from the green baseline state.
    #[arg(long = "continue", help_heading = "Session Control")]
    r#continue: bool,

    /// Commit all remaining changes and finish the current factor session.
    ///
    /// Restores all remaining changes, verifies the tree hash matches the
    /// recorded green baseline, and commits the final split. When no
    /// --message is given, reuses the original tip commit message.
    #[arg(long = "finish", help_heading = "Session Control")]
    finish: bool,

    /// Discard the current split attempt and restore the remaining pool.
    ///
    /// Restores the green baseline commit into the index and working tree,
    /// then unstages everything so the session returns to the normal
    /// "remaining changes are unstaged" state.
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
  1. Start a session:    git factor --exec 'make test' HEAD
  2. If start gate fails: fix, stage, amend, then run git rebase --continue
  3. When paused at the factor break: git factor --continue
  4. Stage changes:      git add --patch -- <path>
  5. Commit a slice:     git factor --continue --message 'type: description'
  6. Discard bad staging: git factor --retry
  7. Repeat steps 4-6 for each atomic commit.
  8. Finish remaining:   git factor --finish

  The start gate must pass on a clean repository state.
  Each split commit must pass the exec gate independently.
  Ranges refactor one contiguous, merge-free ancestry span into a new series.
  Use --finish without --message to reuse the original commit message.

EXAMPLES:
  Split the latest commit, first proving the full commit is green:
    git factor --exec 'cargo test' HEAD

  Refactor an inclusive span into a new commit series:
    git factor --exec 'make check' HEAD~2 HEAD

  Use git-native exclusive-start range syntax:
    git factor --exec 'npm test' HEAD~3..HEAD

  Use git-native inclusive-start range syntax:
    git factor --exec 'npm test' HEAD~3^..HEAD

  Continue with a multi-paragraph commit message:
    git factor --continue --message 'feat: add login' --message 'Implements OAuth2 flow.'

  Discard the current split attempt and restore the remaining pool:
    git factor --retry

  Finish with the original commit message:
    git factor --finish

  Abort and restore the repository:
    git factor --abort

  Show active-session status or whether a start is pending:
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
    /// Multiple --exec flags are joined with &&. Git-factor runs the combined
    /// gate before the session starts and before each split commit. The
    /// command must have valid bash syntax and must leave the repository
    /// clean.
    #[arg(long = "exec", value_name = "COMMAND", help_heading = "Start Options")]
    exec: Vec<NonEmptyString>,

    /// Commit message for the split commit.
    ///
    /// Submits staged changes with or without --continue. Optional with --finish
    /// (defaults to the original commit message). Multiple --message flags produce
    /// separate paragraphs, matching git commit behavior.
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
