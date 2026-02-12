use clap::Parser;
use non_empty_string::NonEmptyString;

/// Split a large git commit into smaller atomic commits.
#[derive(Parser)]
#[command(
    bin_name = "git-factor",
    disable_help_subcommand = true,
    after_long_help = "\
WORKFLOW:
  1. Start a session:    git factor --exec 'make test' HEAD
  2. Stage changes:      git add --patch -- <path>
  3. Commit a slice:     git factor --continue --message 'type: description'
  4. Repeat steps 2-3 for each atomic commit.
  5. Finish remaining:   git factor --finish

  Each split commit must pass the exec gate independently.
  Use --finish without --message to reuse the original commit message.

EXAMPLES:
  Split the latest commit, running tests after each split:
    git factor --exec 'cargo test' HEAD

  Split three commits in a range:
    git factor --exec 'make check' HEAD~3..HEAD

  Split two specific commits:
    git factor --exec 'npm test' abc1234 def5678

  Continue with a multi-paragraph commit message:
    git factor --continue --message 'feat: add login' --message 'Implements OAuth2 flow.'

  Finish with the original commit message:
    git factor --finish

  Abort and restore the repository:
    git factor --abort"
)]
pub(super) struct Cli {
    /// Abort the current factor session and restore the repository.
    #[arg(long = "abort", help_heading = "Session Control")]
    abort: bool,

    /// Commit(s) or ranges to split (e.g. SHA, A..B, main..HEAD).
    ///
    /// Accepts full or short SHAs, branch names, and revision ranges.
    /// Ranges are expanded via git rev-list in chronological order.
    /// Multiple refs can be specified and are deduplicated automatically.
    #[arg(value_name = "COMMIT")]
    commits: Vec<NonEmptyString>,

    /// Continue by committing the currently staged changes.
    ///
    /// Stages must contain changes and the exec gate must pass.
    /// After committing, remaining changes are restored as unstaged changes.
    #[arg(long = "continue", help_heading = "Session Control")]
    r#continue: bool,

    /// Shell command(s) to run as a validation gate after each split commit.
    ///
    /// Multiple --exec flags are joined with && and also passed to
    /// git rebase --exec. The command must have valid bash syntax.
    #[arg(long = "exec", value_name = "COMMAND", help_heading = "Start Options")]
    exec: Vec<NonEmptyString>,

    /// Commit all remaining changes and finish the current commit.
    ///
    /// Cherry-picks the original commit to restore all remaining changes,
    /// verifies the tree hash matches, then runs the exec gate.
    /// When no --message is given, reuses the original commit message.
    #[arg(long = "finish", help_heading = "Session Control")]
    finish: bool,

    /// Commit message for the split commit.
    ///
    /// Required with --continue. Optional with --finish (defaults to
    /// the original commit message). Multiple --message flags produce separate
    /// paragraphs, matching git commit behavior.
    #[arg(
        long = "message",
        short = 'm',
        value_name = "MSG",
        help_heading = "Commit Options"
    )]
    message: Vec<NonEmptyString>,
}

impl Cli {
    /// Returns whether `--abort` was requested.
    pub(super) const fn abort(&self) -> bool {
        self.abort
    }

    /// Returns requested commit refs/ranges.
    pub(super) fn commits(&self) -> &[NonEmptyString] {
        &self.commits
    }

    /// Returns whether `--continue` was requested.
    pub(super) const fn continue_flag(&self) -> bool {
        self.r#continue
    }

    /// Returns requested `--exec` commands.
    pub(super) fn exec(&self) -> &[NonEmptyString] {
        &self.exec
    }

    /// Returns whether `--finish` was requested.
    pub(super) const fn finish(&self) -> bool {
        self.finish
    }

    /// Returns requested commit messages.
    pub(super) fn message(&self) -> &[NonEmptyString] {
        &self.message
    }
}
