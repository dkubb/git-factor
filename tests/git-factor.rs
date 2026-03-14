#![expect(
    clippy::implicit_return,
    reason = "integration tests favor concise tail expressions"
)]
//! Contract integration tests for `git-factor`.

#![forbid(unsafe_code)]

#[cfg(test)]
#[path = "support/mod.rs"]
mod support;

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;
    use std::process::{self, Command};

    use assert_cmd::assert::OutputAssertExt as _;
    use predicates::prelude::*;

    use super::support::*;

    fn must_ok<T, E>(result: Result<T, E>) -> T {
        result.unwrap_or_else(|_| process::abort())
    }

    fn write_factor_state(repo: &Path, entries: &[(&str, &str)]) {
        let factor_dir = git_dir(repo).join("factor");
        must_ok(fs::create_dir_all(&factor_dir));
        for &(name, content) in entries {
            must_ok(fs::write(factor_dir.join(name), format!("{content}\n")));
        }
    }

    fn expected_help_stdout() -> &'static str {
        "\
Split a large git commit into smaller atomic commits

Usage: git-factor [OPTIONS] [COMMIT]...

Arguments:
  [COMMIT]...
          Commit(s) or ranges to split (e.g. SHA, A..B, main..HEAD).
          
          Accepts full or short SHAs, branch names, and revision ranges. Ranges are expanded via git rev-list in chronological order. Multiple refs can be specified and are deduplicated automatically.

Options:
  -h, --help
          Print help (see a summary with '-h')

Start Options:
      --exec <COMMAND>
          Shell command(s) to run as the deterministic validation gate.
          
          Multiple --exec flags are joined with &&. Git-factor runs the combined gate when a target commit becomes active and before each split commit. The command must have valid bash syntax and must leave the repository clean.

Commit Options:
  -m, --message <MSG>
          Commit message for the split commit.
          
          Required with --continue. Optional with --finish (defaults to the original commit message). Multiple --message flags produce separate paragraphs, matching git commit behavior.

Session Control:
      --continue
          Continue by committing the currently staged changes.
          
          Staged changes must contain the next atomic split and the exec gate must pass. After committing, remaining changes are restored as unstaged changes from the green baseline commit.

      --finish
          Commit all remaining changes and finish the current commit.
          
          Cherry-picks the original commit to restore all remaining changes, verifies the tree hash matches the recorded green baseline, and commits the final split. When no --message is given, reuses the original commit message.

      --abort
          Abort the current factor session and restore the repository

      --status
          Show status for the current factor session.
          
          Prints session details when active, otherwise reports no active session.

WORKFLOW:
  1. Start a session:    git factor --exec 'make test' HEAD
  2. If start gate fails: fix, stage, amend, then run git rebase --continue
  3. When paused at the factor break: git factor --continue
  4. Stage changes:      git add --patch -- <path>
  5. Commit a slice:     git factor --continue --message 'type: description'
  6. Repeat steps 4-5 for each atomic commit.
  7. Finish remaining:   git factor --finish

  The start gate must pass on a clean repository state.
  Each split commit must pass the exec gate independently.
  Use --finish without --message to reuse the original commit message.

EXAMPLES:
  Split the latest commit, first proving the full commit is green:
    git factor --exec 'cargo test' HEAD

  Split three commits in a range, pausing before each factor session:
    git factor --exec 'make check' HEAD~3..HEAD

  Split two specific commits:
    git factor --exec 'npm test' abc1234 def5678

  Continue with a multi-paragraph commit message:
    git factor --continue --message 'feat: add login' --message 'Implements OAuth2 flow.'

  Finish with the original commit message:
    git factor --finish

  Abort and restore the repository:
    git factor --abort

  Show active-session status or whether a start is pending:
    git factor --status
"
    }

    #[test]
    fn cli_no_args_prints_help() {
        Command::new(git_factor_bin())
            .assert()
            .code(EXIT_OK)
            .stdout(predicate::str::diff(expected_help_stdout().to_owned()))
            .stderr(predicate::str::is_empty());
    }

    #[test]
    fn cli_help_flag_prints_help_to_stdout_and_exits_ok() {
        Command::new(git_factor_bin())
            .arg("--help")
            .assert()
            .code(EXIT_OK)
            .stdout(predicate::str::diff(expected_help_stdout().to_owned()))
            .stderr(predicate::str::is_empty());
    }

    #[test]
    fn start_requires_exec_command() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");

        Command::new(git_factor_bin())
            .current_dir(repo)
            .arg("HEAD")
            .assert()
            .code(EXIT_USAGE)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::diff(
                "--exec <COMMAND> is required when starting a factor session\n".to_owned(),
            ));
    }

    #[test]
    fn start_requires_exec_command_when_only_message_is_provided() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");

        Command::new(git_factor_bin())
            .current_dir(repo)
            .args(["--message", "test: msg"])
            .assert()
            .code(EXIT_USAGE)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::diff(
                "--message can only be used with --continue or --finish\n".to_owned(),
            ));
    }

    #[test]
    fn rejects_finish_when_combined_with_exec_or_commit() {
        let dir = init_repo();
        let repo = dir.path();

        Command::new(git_factor_bin())
            .current_dir(repo)
            .args(["--finish", "--exec", "true", "HEAD"])
            .assert()
            .code(EXIT_USAGE)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::diff(
                "--finish cannot be combined with --continue, --exec, or COMMIT\n".to_owned(),
            ));
    }

    #[test]
    fn rejects_finish_when_combined_with_continue() {
        let dir = init_repo();
        let repo = dir.path();

        Command::new(git_factor_bin())
            .current_dir(repo)
            .args(["--finish", "--continue", "--message", "test: msg"])
            .assert()
            .code(EXIT_USAGE)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::diff(
                "--finish cannot be combined with --continue, --exec, or COMMIT\n".to_owned(),
            ));
    }

    #[test]
    fn rejects_finish_when_combined_with_commit() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");

        Command::new(git_factor_bin())
            .current_dir(repo)
            .args(["--finish", "HEAD"])
            .assert()
            .code(EXIT_USAGE)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::diff(
                "--finish cannot be combined with --continue, --exec, or COMMIT\n".to_owned(),
            ));
    }

    #[test]
    fn rejects_continue_when_combined_with_exec_or_commit() {
        let dir = init_repo();
        let repo = dir.path();

        Command::new(git_factor_bin())
            .current_dir(repo)
            .args(["--continue", "--exec", "true", "--message", "test: msg"])
            .assert()
            .code(EXIT_USAGE)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::diff(
                "--continue cannot be combined with --exec or COMMIT\n".to_owned(),
            ));
    }

    #[test]
    fn rejects_continue_when_combined_with_commit() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");

        Command::new(git_factor_bin())
            .current_dir(repo)
            .args(["--continue", "--message", "test: msg", "HEAD"])
            .assert()
            .code(EXIT_USAGE)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::diff(
                "--continue cannot be combined with --exec or COMMIT\n".to_owned(),
            ));
    }

    #[test]
    fn rejects_start_when_message_is_provided() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");

        Command::new(git_factor_bin())
            .current_dir(repo)
            .args(["--exec", "true", "--message", "test: msg", "HEAD"])
            .assert()
            .code(EXIT_USAGE)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::diff(
                "--message can only be used with --continue or --finish\n".to_owned(),
            ));
    }

    #[test]
    fn rejects_continue_without_message() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");

        Command::new(git_factor_bin())
            .current_dir(repo)
            .arg("--continue")
            .assert()
            .code(EXIT_USAGE)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::diff(
                "--continue requires --message <MSG>\n".to_owned(),
            ));
    }

    #[test]
    fn cli_invalid_flag_triggers_clap_error_path() {
        Command::new(git_factor_bin())
            .arg("--definitely-not-a-real-flag")
            .assert()
            .code(EXIT_USAGE)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::contains(
                "unexpected argument '--definitely-not-a-real-flag'",
            ));
    }

    #[test]
    fn start_placeholder_reports_later_workflow() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "base\n", "chore: base");

        Command::new(git_factor_bin())
            .current_dir(repo)
            .args(["--exec", "true", "HEAD"])
            .assert()
            .code(EXIT_USAGE)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::diff(
                "start workflow lands in later commits\n".to_owned(),
            ));
    }

    #[test]
    fn continue_placeholder_reports_later_workflow() {
        let dir = init_repo();
        let repo = dir.path();

        Command::new(git_factor_bin())
            .current_dir(repo)
            .args(["--continue", "--message", "test: slice"])
            .assert()
            .code(EXIT_USAGE)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::diff(
                "continue workflow lands in later commits\n".to_owned(),
            ));
    }

    #[test]
    fn abort_placeholder_reports_later_workflow() {
        let dir = init_repo();
        let repo = dir.path();

        Command::new(git_factor_bin())
            .current_dir(repo)
            .arg("--abort")
            .assert()
            .code(EXIT_USAGE)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::diff(
                "abort workflow lands in later commits\n".to_owned(),
            ));
    }

    #[test]
    fn status_reports_no_active_session() {
        let dir = init_repo();
        let repo = dir.path();

        Command::new(git_factor_bin())
            .current_dir(repo)
            .arg("--status")
            .assert()
            .code(EXIT_OK)
            .stdout(predicate::str::diff(
                "FACTOR: No active session.\n".to_owned(),
            ))
            .stderr(predicate::str::is_empty());
    }

    #[test]
    fn status_reports_active_session_details() {
        let dir = init_repo();
        let repo = dir.path();
        commit_file(repo, "file.txt", "one\n", "chore: base");
        let current_commit = git(repo, &["rev-parse", "HEAD"]);

        write_factor_state(
            repo,
            &[
                ("commits", current_commit.as_str()),
                ("current_index", "0"),
                ("split_count", "2"),
                ("phase", "splitting"),
                ("requires_rebase", "false"),
                ("is_root", "false"),
            ],
        );

        Command::new(git_factor_bin())
            .current_dir(repo)
            .arg("--status")
            .assert()
            .code(EXIT_OK)
            .stdout(predicate::str::diff(format!(
                "FACTOR: Active session.\nCURRENT_COMMIT: {current_commit}\nCURRENT_INDEX: 0\nSPLIT_COUNT: 2\nPHASE: splitting\nREQUIRES_REBASE: false\nREBASE_IN_PROGRESS: false\nIS_ROOT: false\n"
            )))
            .stderr(predicate::str::is_empty());
    }

    #[test]
    fn status_reports_pending_start_during_rebase() {
        let dir = init_repo();
        let repo = dir.path();
        commit_file(repo, "file.txt", "one\n", "chore: base");
        let current_commit = git(repo, &["rev-parse", "HEAD"]);
        let git_dir = git_dir(repo);
        must_ok(fs::create_dir_all(git_dir.join("rebase-merge")));

        write_factor_state(
            repo,
            &[
                ("commits", current_commit.as_str()),
                ("current_index", "0"),
                ("split_count", "0"),
                ("phase", "pending_start"),
                ("requires_rebase", "true"),
                ("is_root", "true"),
            ],
        );

        Command::new(git_factor_bin())
            .current_dir(repo)
            .arg("--status")
            .assert()
            .code(EXIT_OK)
            .stdout(predicate::str::diff(format!(
                "FACTOR: Active session.\nCURRENT_COMMIT: {current_commit}\nCURRENT_INDEX: 0\nSPLIT_COUNT: 0\nPHASE: pending_start\nREQUIRES_REBASE: true\nREBASE_IN_PROGRESS: true\nIS_ROOT: true\n"
            )))
            .stderr(predicate::str::is_empty());
    }

    #[test]
    fn finish_placeholder_reports_later_workflow() {
        let dir = init_repo();
        let repo = dir.path();

        Command::new(git_factor_bin())
            .current_dir(repo)
            .arg("--finish")
            .assert()
            .code(EXIT_USAGE)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::diff(
                "finish workflow lands in later commits\n".to_owned(),
            ));
    }

    #[test]
    fn rejects_abort_when_combined_with_finish() {
        let dir = init_repo();
        let repo = dir.path();

        Command::new(git_factor_bin())
            .current_dir(repo)
            .args(["--abort", "--finish"])
            .assert()
            .code(EXIT_USAGE)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::diff(
                "--abort cannot be combined with other options\n".to_owned(),
            ));
    }

    #[test]
    fn rejects_abort_when_combined_with_continue() {
        let dir = init_repo();
        let repo = dir.path();

        Command::new(git_factor_bin())
            .current_dir(repo)
            .args(["--abort", "--continue", "--message", "test: msg"])
            .assert()
            .code(EXIT_USAGE)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::diff(
                "--abort cannot be combined with other options\n".to_owned(),
            ));
    }

    #[test]
    fn rejects_abort_when_combined_with_exec() {
        let dir = init_repo();
        let repo = dir.path();

        Command::new(git_factor_bin())
            .current_dir(repo)
            .args(["--abort", "--exec", "true"])
            .assert()
            .code(EXIT_USAGE)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::diff(
                "--abort cannot be combined with other options\n".to_owned(),
            ));
    }

    #[test]
    fn rejects_abort_when_combined_with_commit() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");

        Command::new(git_factor_bin())
            .current_dir(repo)
            .args(["--abort", "HEAD"])
            .assert()
            .code(EXIT_USAGE)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::diff(
                "--abort cannot be combined with other options\n".to_owned(),
            ));
    }

    #[test]
    fn rejects_abort_when_combined_with_status() {
        let dir = init_repo();
        let repo = dir.path();

        Command::new(git_factor_bin())
            .current_dir(repo)
            .args(["--abort", "--status"])
            .assert()
            .code(EXIT_USAGE)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::diff(
                "--abort cannot be combined with other options\n".to_owned(),
            ));
    }

    #[test]
    fn rejects_status_when_combined_with_exec() {
        let dir = init_repo();
        let repo = dir.path();

        Command::new(git_factor_bin())
            .current_dir(repo)
            .args(["--status", "--exec", "true"])
            .assert()
            .code(EXIT_USAGE)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::diff(
                "--status cannot be combined with other options\n".to_owned(),
            ));
    }

    #[test]
    fn rejects_status_when_combined_with_continue() {
        let dir = init_repo();
        let repo = dir.path();

        Command::new(git_factor_bin())
            .current_dir(repo)
            .args(["--status", "--continue", "--message", "test: msg"])
            .assert()
            .code(EXIT_USAGE)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::diff(
                "--status cannot be combined with other options\n".to_owned(),
            ));
    }

    #[test]
    fn rejects_status_when_combined_with_finish() {
        let dir = init_repo();
        let repo = dir.path();

        Command::new(git_factor_bin())
            .current_dir(repo)
            .args(["--status", "--finish"])
            .assert()
            .code(EXIT_USAGE)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::diff(
                "--status cannot be combined with other options\n".to_owned(),
            ));
    }

    #[test]
    fn rejects_status_when_combined_with_commit() {
        let dir = init_repo();
        let repo = dir.path();

        Command::new(git_factor_bin())
            .current_dir(repo)
            .args(["--status", "HEAD"])
            .assert()
            .code(EXIT_USAGE)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::diff(
                "--status cannot be combined with other options\n".to_owned(),
            ));
    }

    #[test]
    fn rejects_status_when_combined_with_message() {
        let dir = init_repo();
        let repo = dir.path();

        Command::new(git_factor_bin())
            .current_dir(repo)
            .args(["--status", "--message", "test: message"])
            .assert()
            .code(EXIT_USAGE)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::diff(
                "--status cannot be combined with other options\n".to_owned(),
            ));
    }
}
