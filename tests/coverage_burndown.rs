//! Coverage burn-down integration tests for `git-factor`.

#![forbid(unsafe_code)]

mod support;

#[cfg(test)]
#[expect(
    clippy::inline_modules,
    reason = "preserve the established inline test layout"
)]
mod tests {

    use core::panic::AssertUnwindSafe;
    use core::time::Duration;
    use std::env;
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt as _;
    use std::panic::{catch_unwind, resume_unwind};
    use std::process::Command;
    use std::thread::sleep;

    use assert_cmd::prelude::*;
    use predicates::prelude::*;
    use tempfile::TempDir;

    use crate::support::EXIT_DATAERR;
    use crate::support::EXIT_OK;
    use crate::support::EXIT_SOFTWARE;
    use crate::support::commit_file;
    use crate::support::git;
    use crate::support::git_dir;
    use crate::support::git_factor_bin;
    use crate::support::init_repo;
    use crate::support::make_git_wrapper_named;
    use crate::support::start_session;

    macro_rules! prefixed_path {
        ($prefix:expr) => {{
            match env::var("PATH") {
                Ok(existing) => format!("{}:{existing}", $prefix.display()),
                Err(_err) => format!("{}:", $prefix.display()),
            }
        }};
    }

    macro_rules! assert_ok {
        ($expr:expr, $context:literal $(,)?) => {{
            let result = $expr;
            assert!(result.is_ok(), "{}: {:?}", $context, result.as_ref().err());
            let Ok(value) = result else {
                return;
            };
            value
        }};
    }

    #[test]
    fn start_rejects_invalid_exec_syntax_in_binary_path() {
        let dir = init_repo();
        let repo = dir.path();
        commit_file(repo, "tracked.txt", "v1\n", "feat: initial");

        Command::new(git_factor_bin())
            .current_dir(repo)
            .args(["--exec", "if )", "HEAD"])
            .assert()
            .code(EXIT_DATAERR)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::contains("invalid exec syntax: if )"));
    }

    #[test]
    fn start_rejects_non_ancestor_commit_in_binary_path() {
        let dir = init_repo();
        let repo = dir.path();
        commit_file(repo, "tracked.txt", "base\n", "feat: base");

        git(repo, &["checkout", "--quiet", "-b", "side"]);
        commit_file(repo, "tracked.txt", "side\n", "feat: side");
        let side_tip = git(repo, &["rev-parse", "HEAD"]);
        git(repo, &["checkout", "--quiet", "-"]);

        Command::new(git_factor_bin())
            .current_dir(repo)
            .args(["--exec", "true", side_tip.as_str()])
            .assert()
            .code(EXIT_DATAERR)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::contains("is not an ancestor of HEAD"));
    }

    #[test]
    fn continue_uses_original_commit_tree_when_expected_tree_state_missing() {
        let dir = init_repo();
        let repo = dir.path();
        commit_file(repo, "tracked.txt", "v1\n", "feat: initial");
        commit_file(repo, "tracked.txt", "v2\n", "feat: second");

        Command::new(git_factor_bin())
            .current_dir(repo)
            .args(["--exec", "true", "HEAD"])
            .assert()
            .success();

        let state_dir = git_dir(repo).join("factor");
        assert_ok!(
            fs::remove_file(state_dir.join("expected_tree")),
            "remove expected_tree state file"
        );
        git(repo, &["add", "--all"]);

        Command::new(git_factor_bin())
            .current_dir(repo)
            .args(["--continue", "--message", "test: split"])
            .assert()
            .code(EXIT_OK)
            .stdout(predicate::str::contains(
                "{\"operation\":\"continue\",\"split_count\":1}",
            ))
            .stderr(predicate::str::is_empty());
    }

    #[test]
    fn start_reports_cannot_resolve_cwd_when_process_cwd_was_deleted() {
        let dir = init_repo();
        let repo = dir.path();
        commit_file(repo, "tracked.txt", "v1\n", "feat: initial");
        let bin = git_factor_bin();

        Command::new("bash")
            .args([
                "-ceu",
                r#"repo="$1"; bin="$2"; cd "$repo"; rm -rf "$repo"; exec "$bin" --exec true HEAD"#,
                "--",
            ])
            .arg(repo)
            .arg(bin)
            .assert()
            .failure()
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::contains("cannot resolve cwd"));
    }

    #[test]
    fn abort_propagates_start_head_state_read_error_in_binary_path() {
        let dir = init_repo();
        let repo = dir.path();
        commit_file(repo, "tracked.txt", "v1\n", "feat: initial");
        start_session(repo);

        let state_dir = git_dir(repo).join("factor");
        let start_head = state_dir.join("start_head");
        assert_ok!(fs::remove_file(&start_head), "remove start_head file");
        assert_ok!(
            fs::create_dir_all(&start_head),
            "replace start_head with directory"
        );

        Command::new(git_factor_bin())
            .current_dir(repo)
            .arg("--abort")
            .assert()
            .failure()
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::contains("directory"));
    }

    #[test]
    fn start_rejects_empty_short_sha_in_binary_path() {
        let dir = init_repo();
        let repo = dir.path();
        commit_file(repo, "tracked.txt", "v1\n", "feat: initial");

        let (_wrapper_dir, wrapper_bin) = make_git_wrapper_named(
            "git",
            r#"
if [ "$1" = "rev-parse" ] && [ "$#" -ge 2 ] && [ "$2" = "--short" ]; then
  exit 0
fi
"#,
        );

        Command::new(git_factor_bin())
            .current_dir(repo)
            .env("PATH", prefixed_path!(&wrapper_bin))
            .args(["--exec", "true", "HEAD"])
            .assert()
            .code(EXIT_SOFTWARE)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::contains("empty short SHA"));
    }

    #[test]
    fn start_reports_bash_spawn_error_when_shell_unavailable() {
        let dir = init_repo();
        let repo = dir.path();
        commit_file(repo, "tracked.txt", "v1\n", "feat: initial");

        let path_dir = assert_ok!(TempDir::new(), "tempdir");
        let git_shim = path_dir.path().join("git");
        assert_ok!(
            fs::write(&git_shim, "#!/bin/sh\nexec /usr/bin/git \"$@\"\n"),
            "write git shim"
        );
        #[cfg(unix)]
        {
            let metadata = assert_ok!(fs::metadata(&git_shim), "metadata");
            let mut perms = metadata.permissions();
            perms.set_mode(0o755);
            assert_ok!(fs::set_permissions(&git_shim, perms), "set permissions");
        };

        Command::new(git_factor_bin())
            .current_dir(repo)
            .env("PATH", path_dir.path())
            .args(["--exec", "true", "HEAD"])
            .assert()
            .failure()
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::contains("bash syntax check:"));
    }

    #[test]
    fn start_reports_git_spawn_error_during_ancestor_validation() {
        let dir = init_repo();
        let repo = dir.path();
        commit_file(repo, "tracked.txt", "v1\n", "feat: initial");

        let wrapper_dir = assert_ok!(TempDir::new(), "tempdir");
        let wrapper_bin = wrapper_dir.path();
        let git_wrapper = wrapper_bin.join("git");
        assert_ok!(
            fs::write(
                &git_wrapper,
                r#"#!/bin/sh
set -eu

if [ "$1" = "rev-parse" ] && [ "$#" -ge 3 ] && [ "$2" = "--verify" ] && [ "$3" = "HEAD" ]; then
  count_file="${0%/*}/.verify_head_count"
  count=0
  if [ -f "$count_file" ]; then
    IFS= read -r count < "$count_file"
  fi
  count=$((count + 1))
  printf '%s\n' "$count" > "$count_file"
  if [ "$count" -eq 2 ]; then
    /usr/bin/git "$@"
    status="$?"
    printf '%s\n' '#!/nonexistent/interpreter' > "$0"
    exit "$status"
  fi
fi

exec /usr/bin/git "$@"
"#,
            ),
            "write git wrapper"
        );
        #[cfg(unix)]
        {
            let metadata = assert_ok!(fs::metadata(&git_wrapper), "metadata");
            let mut perms = metadata.permissions();
            perms.set_mode(0o755);
            assert_ok!(fs::set_permissions(&git_wrapper, perms), "set permissions");
        };

        Command::new(git_factor_bin())
            .current_dir(repo)
            .env("PATH", wrapper_bin)
            .args(["--exec", "true", "HEAD"])
            .assert()
            .failure()
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::contains("No such file or directory"));
    }

    #[test]
    fn start_rejects_merge_commit_in_binary_path() {
        let dir = init_repo();
        let repo = dir.path();
        commit_file(repo, "base.txt", "base\n", "feat: base");
        git(repo, &["checkout", "--quiet", "-b", "side"]);
        commit_file(repo, "side.txt", "side\n", "feat: side");
        git(repo, &["checkout", "--quiet", "-"]);
        commit_file(repo, "main.txt", "main\n", "feat: main");
        git(repo, &["merge", "--quiet", "--no-ff", "--no-edit", "side"]);

        Command::new(git_factor_bin())
            .current_dir(repo)
            .args(["--exec", "true", "HEAD"])
            .assert()
            .code(EXIT_DATAERR)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::contains("is a merge commit"));
    }

    #[test]
    fn start_propagates_commit_message_error_in_binary_path() {
        let dir = init_repo();
        let repo = dir.path();
        commit_file(repo, "tracked.txt", "v1\n", "feat: initial");

        let (_wrapper_dir, wrapper_bin) = make_git_wrapper_named(
            "git",
            r#"
if [ "$1" = "show" ] && [ "$#" -ge 2 ] && [ "$2" = "--format=%B" ]; then
  echo "forced commit-message failure" >&2
  exit 1
fi
"#,
        );

        Command::new(git_factor_bin())
            .current_dir(repo)
            .env("PATH", prefixed_path!(&wrapper_bin))
            .args(["--exec", "true", "HEAD"])
            .assert()
            .code(EXIT_SOFTWARE)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::contains("forced commit-message failure"));
    }

    #[test]
    fn start_reports_state_write_error_when_factor_path_is_file() {
        let dir = init_repo();
        let repo = dir.path();
        commit_file(repo, "tracked.txt", "v1\n", "feat: initial");

        let state_path = git_dir(repo).join("factor");
        assert_ok!(fs::write(&state_path, "occupied\n"), "write factor file");

        Command::new(git_factor_bin())
            .current_dir(repo)
            .args(["--exec", "true", "HEAD"])
            .assert()
            .code(EXIT_SOFTWARE)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::contains("failed to write state"));
    }

    #[test]
    fn status_accepts_absolute_git_dir_output_in_binary_path() {
        let dir = init_repo();
        let repo = dir.path();

        let (_wrapper_dir, wrapper_bin) = make_git_wrapper_named(
            "git",
            r#"
if [ "$1" = "rev-parse" ] && [ "$#" -ge 2 ] && [ "$2" = "--git-dir" ]; then
  out=$(/usr/bin/git "$@")
  case "$out" in
    /*) printf '%s\n' "$out" ;;
    *) printf '%s\n' "$(pwd)/$out" ;;
  esac
  exit 0
fi
"#,
        );

        Command::new(git_factor_bin())
            .current_dir(repo)
            .env("PATH", prefixed_path!(&wrapper_bin))
            .arg("--status")
            .assert()
            .code(EXIT_OK)
            .stdout(predicate::eq(
                "{\"operation\":\"status\",\"session\":null}\n",
            ))
            .stderr(predicate::str::is_empty());
    }

    #[test]
    fn abort_reports_empty_start_head_state_file_in_binary_path() {
        let dir = init_repo();
        let repo = dir.path();
        commit_file(repo, "tracked.txt", "v1\n", "feat: initial");
        start_session(repo);

        let state_dir = git_dir(repo).join("factor");
        assert_ok!(
            fs::write(state_dir.join("start_head"), "\n"),
            "overwrite start_head with empty value"
        );

        Command::new(git_factor_bin())
            .current_dir(repo)
            .arg("--abort")
            .assert()
            .code(EXIT_SOFTWARE)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::contains(
                "corrupted state file 'start_head': file is empty",
            ));
    }

    #[test]
    fn continue_reports_remaining_tracked_changes_without_untracked_section() {
        let dir = init_repo();
        let repo = dir.path();
        commit_file(repo, "a.txt", "a1\n", "feat: base a");
        commit_file(repo, "b.txt", "b1\n", "feat: base b");
        assert_ok!(fs::write(repo.join("a.txt"), "a2\n"), "update a");
        assert_ok!(fs::write(repo.join("b.txt"), "b2\n"), "update b");
        git(repo, &["add", "a.txt", "b.txt"]);
        git(repo, &["commit", "--message", "feat: update both"]);
        start_session(repo);

        git(repo, &["add", "a.txt"]);

        Command::new(git_factor_bin())
            .current_dir(repo)
            .args(["--continue", "--message", "feat: split a"])
            .assert()
            .code(EXIT_OK)
            .stdout(
                predicate::str::contains("STATE: Remaining changes are unstaged.")
                    .and(predicate::str::contains("UNTRACKED:").not()),
            )
            .stderr(predicate::str::is_empty());
    }

    #[test]
    fn continue_completes_fully_staged_span_in_one_step_during_rebase() {
        let dir = init_repo();
        let repo = dir.path();
        commit_file(repo, "base.txt", "base\n", "feat: base");
        commit_file(repo, "a.txt", "a\n", "feat: add a");
        commit_file(repo, "b.txt", "b\n", "feat: add b");

        Command::new(git_factor_bin())
            .current_dir(repo)
            .args(["--exec", "true", "HEAD~1", "HEAD"])
            .assert()
            .success();
        git(repo, &["add", "--all"]);

        Command::new(git_factor_bin())
            .current_dir(repo)
            .args(["--continue", "--message", "feat: split first"])
            .assert()
            .code(EXIT_OK)
            .stdout(predicate::str::contains(
                "{\"operation\":\"continue\",\"split_count\":1}",
            ))
            .stderr(predicate::str::is_empty());
    }

    #[test]
    fn continue_propagates_advance_error_after_converged_tree_in_binary_path() {
        let dir = init_repo();
        let repo = dir.path();
        commit_file(repo, "tracked.txt", "v1\n", "feat: initial");
        start_session(repo);
        git(repo, &["add", "--all"]);

        let (_wrapper_dir, wrapper_bin) = make_git_wrapper_named(
            "git",
            r#"
if [ "$1" = "rev-list" ] && [ "$#" -ge 3 ] && [ "$2" = "--max-parents=0" ] && [ "$3" = "HEAD" ]; then
  echo "forced rev-list failure" >&2
  exit 1
fi
"#,
        );

        Command::new(git_factor_bin())
            .current_dir(repo)
            .env("PATH", prefixed_path!(&wrapper_bin))
            .args(["--continue", "--message", "feat: split"])
            .assert()
            .code(EXIT_SOFTWARE)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::contains("forced rev-list failure"));
    }

    #[test]
    fn continue_reports_untracked_section_when_remaining_pool_has_untracked_files() {
        let dir = init_repo();
        let repo = dir.path();
        commit_file(repo, "a.txt", "a1\n", "feat: base a");

        assert_ok!(fs::write(repo.join("a.txt"), "a2\n"), "update tracked file");
        assert_ok!(fs::write(repo.join("new.txt"), "new\n"), "create new file");
        git(repo, &["add", "a.txt", "new.txt"]);
        git(repo, &["commit", "--message", "feat: tracked and new file"]);

        start_session(repo);
        git(repo, &["add", "a.txt"]);

        Command::new(git_factor_bin())
            .current_dir(repo)
            .args(["--continue", "--message", "feat: split tracked"])
            .assert()
            .code(EXIT_OK)
            .stdout(
                predicate::str::contains("STATE: Remaining changes are unstaged.")
                    .and(predicate::str::contains("UNTRACKED:"))
                    .and(predicate::str::contains("new.txt")),
            )
            .stderr(predicate::str::is_empty());
    }

    #[test]
    fn continue_propagates_print_hints_failure_with_remaining_span_changes() {
        let dir = init_repo();
        let repo = dir.path();
        commit_file(repo, "base.txt", "base\n", "feat: base");
        commit_file(repo, "a.txt", "a\n", "feat: add a");
        commit_file(repo, "b.txt", "b\n", "feat: add b");

        Command::new(git_factor_bin())
            .current_dir(repo)
            .args(["--exec", "true", "HEAD~1", "HEAD"])
            .assert()
            .success();

        git(repo, &["add", "a.txt"]);

        let (_wrapper_dir, wrapper_bin) = make_git_wrapper_named(
            "git",
            r#"
if [ "$1" = "rev-parse" ] && [ "$#" -ge 2 ] && [ "$2" = "--show-toplevel" ]; then
  echo "forced show-toplevel failure" >&2
  exit 1
fi
"#,
        );

        Command::new(git_factor_bin())
            .current_dir(repo)
            .env("PATH", prefixed_path!(&wrapper_bin))
            .args(["--continue", "--message", "feat: split first"])
            .assert()
            .code(EXIT_SOFTWARE)
            .stdout(predicate::str::contains("FACTOR: Split 1 committed."))
            .stderr(predicate::str::contains("forced show-toplevel failure"));
    }

    #[test]
    fn start_propagates_print_hints_failure_after_session_starts() {
        let dir = init_repo();
        let repo = dir.path();
        commit_file(repo, "tracked.txt", "v1\n", "feat: initial");

        let (_wrapper_dir, wrapper_bin) = make_git_wrapper_named(
            "git",
            r#"
if [ "$1" = "rev-parse" ] && [ "$#" -ge 2 ] && [ "$2" = "--show-toplevel" ]; then
  echo "forced show-toplevel failure" >&2
  exit 1
fi
"#,
        );

        Command::new(git_factor_bin())
            .current_dir(repo)
            .env("PATH", prefixed_path!(&wrapper_bin))
            .args(["--exec", "true", "HEAD"])
            .assert()
            .code(EXIT_SOFTWARE)
            .stdout(predicate::str::contains("FACTOR: Split session started"))
            .stderr(predicate::str::contains("forced show-toplevel failure"));
    }

    #[test]
    fn status_reports_empty_current_index_state_file_in_binary_path() {
        let dir = init_repo();
        let repo = dir.path();
        commit_file(repo, "tracked.txt", "v1\n", "feat: initial");
        start_session(repo);

        let state_dir = git_dir(repo).join("factor");
        assert_ok!(
            fs::write(state_dir.join("current_index"), "\n"),
            "overwrite current_index with empty"
        );

        Command::new(git_factor_bin())
            .current_dir(repo)
            .arg("--status")
            .assert()
            .code(EXIT_SOFTWARE)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::contains(
                "corrupted state file 'current_index': file is empty",
            ));
    }

    #[test]
    fn continue_completes_rebase_after_final_span_step_in_binary_path() {
        let dir = init_repo();
        let repo = dir.path();
        commit_file(repo, "base.txt", "base\n", "feat: base");
        commit_file(repo, "a.txt", "a1\n", "feat: add a");
        commit_file(repo, "b.txt", "b1\n", "feat: add b");

        Command::new(git_factor_bin())
            .current_dir(repo)
            .args(["--exec", "true", "HEAD~1", "HEAD"])
            .assert()
            .success();

        git(repo, &["add", "a.txt"]);
        Command::new(git_factor_bin())
            .current_dir(repo)
            .args(["--continue", "--message", "feat: split first"])
            .assert()
            .code(EXIT_OK)
            .stdout(predicate::str::contains("FACTOR: Split 1 committed.").and(
                predicate::str::contains("STATE: Remaining changes are unstaged."),
            ));

        git(repo, &["add", "--all"]);
        Command::new(git_factor_bin())
            .current_dir(repo)
            .args(["--continue", "--message", "feat: split second"])
            .assert()
            .code(EXIT_OK)
            .stdout(predicate::str::contains(
                "{\"operation\":\"continue\",\"split_count\":2}",
            ))
            .stderr(predicate::str::is_empty());
    }

    #[test]
    fn continue_reports_tracked_remaining_without_untracked_during_rebase() {
        let dir = init_repo();
        let repo = dir.path();
        assert_ok!(fs::write(repo.join("a.txt"), "a0\n"), "write base a");
        assert_ok!(fs::write(repo.join("b.txt"), "b0\n"), "write base b");
        git(repo, &["add", "a.txt", "b.txt"]);
        git(repo, &["commit", "--message", "feat: base files"]);
        assert_ok!(fs::write(repo.join("a.txt"), "a1\n"), "update a");
        assert_ok!(fs::write(repo.join("b.txt"), "b1\n"), "update b");
        git(repo, &["add", "a.txt", "b.txt"]);
        git(repo, &["commit", "--message", "feat: update a and b"]);
        assert_ok!(fs::write(repo.join("b.txt"), "b2\n"), "refine b");
        git(repo, &["add", "b.txt"]);
        git(repo, &["commit", "--message", "feat: refine b"]);

        Command::new(git_factor_bin())
            .current_dir(repo)
            .args(["--exec", "true", "HEAD~1", "HEAD"])
            .assert()
            .success();

        git(repo, &["add", "a.txt"]);
        Command::new(git_factor_bin())
            .current_dir(repo)
            .args(["--continue", "--message", "feat: split a only"])
            .assert()
            .code(EXIT_OK)
            .stdout(
                predicate::str::contains("STATE: Remaining changes are unstaged.")
                    .and(predicate::str::contains("UNTRACKED:").not()),
            )
            .stderr(predicate::str::is_empty());
    }

    #[test]
    fn start_omits_remaining_hint_when_diff_stat_is_empty_in_binary_path() {
        let dir = init_repo();
        let repo = dir.path();
        commit_file(repo, "tracked.txt", "v1\n", "feat: initial");

        let (_wrapper_dir, wrapper_bin) = make_git_wrapper_named(
            "git",
            r#"
if [ "$1" = "diff" ] && [ "$#" -ge 2 ] && [ "$2" = "--stat" ]; then
  exit 0
fi
"#,
        );

        Command::new(git_factor_bin())
            .current_dir(repo)
            .env("PATH", prefixed_path!(&wrapper_bin))
            .args(["--exec", "true", "HEAD"])
            .assert()
            .code(EXIT_OK)
            .stdout(
                predicate::str::contains("FACTOR: Split session started")
                    .and(predicate::str::contains("  REMAINING:").not()),
            )
            .stderr(predicate::str::is_empty());
    }

    #[test]
    fn abort_skips_rebase_abort_when_started_rebase_true_but_not_mid_rebase() {
        let dir = init_repo();
        let repo = dir.path();
        commit_file(repo, "tracked.txt", "v1\n", "feat: initial");
        start_session(repo);

        let state_dir = git_dir(repo).join("factor");
        assert_ok!(
            fs::write(state_dir.join("started_rebase"), "true\n"),
            "write started_rebase"
        );
        assert_ok!(
            fs::write(state_dir.join("requires_rebase"), "true\n"),
            "write requires_rebase"
        );

        Command::new(git_factor_bin())
            .current_dir(repo)
            .arg("--abort")
            .assert()
            .code(EXIT_OK)
            .stdout(predicate::eq(
                "{\"operation\":\"abort\",\"rebase\":{\"in_progress\":false},\"actions\":{}}\n",
            ))
            .stderr(predicate::str::is_empty());
    }

    #[test]
    fn finish_runs_while_mid_rebase_when_requires_rebase_is_true() {
        let dir = init_repo();
        let repo = dir.path();
        commit_file(repo, "base.txt", "base\n", "feat: base");
        commit_file(repo, "a.txt", "a1\n", "feat: add a");
        commit_file(repo, "b.txt", "b1\n", "feat: add b");

        Command::new(git_factor_bin())
            .current_dir(repo)
            .args(["--exec", "true", "HEAD~1", "HEAD"])
            .assert()
            .success();

        git(repo, &["add", "--all"]);
        Command::new(git_factor_bin())
            .current_dir(repo)
            .args(["--finish", "--message", "feat: finish remaining"])
            .assert()
            .code(EXIT_OK)
            .stdout(predicate::str::contains(
                "{\"operation\":\"finish\",\"split_count\":1}",
            ))
            .stderr(predicate::str::is_empty());
    }

    #[test]
    fn start_ignores_invalid_rev_list_lines_when_resolving_range_refs() {
        let dir = init_repo();
        let repo = dir.path();
        commit_file(repo, "base.txt", "base\n", "feat: base");
        commit_file(repo, "next.txt", "next\n", "feat: next");

        let (_wrapper_dir, wrapper_bin) = make_git_wrapper_named(
            "git",
            r#"
if [ "$1" = "rev-list" ] && [ "$#" -ge 2 ] && [ "$2" = "HEAD~1..HEAD" ]; then
  /usr/bin/git "$@"
  printf '%s\n' 'not-a-commit-sha'
  exit 0
fi
"#,
        );

        Command::new(git_factor_bin())
            .current_dir(repo)
            .env("PATH", prefixed_path!(&wrapper_bin))
            .args(["--exec", "true", "HEAD~1..HEAD"])
            .assert()
            .code(EXIT_OK)
            .stdout(predicate::str::contains("FACTOR: Split session started"))
            .stderr(predicate::str::is_empty());
    }

    #[test]
    fn proptest_run_integration_coverage_suite() {
        const MAX_ATTEMPTS: usize = 3;
        for attempt in 1..=MAX_ATTEMPTS {
            let result = catch_unwind(AssertUnwindSafe(|| {
                abort_propagates_start_head_state_read_error_in_binary_path();
                abort_skips_rebase_abort_when_started_rebase_true_but_not_mid_rebase();
                continue_completes_fully_staged_span_in_one_step_during_rebase();
                continue_completes_rebase_after_final_span_step_in_binary_path();
                continue_propagates_advance_error_after_converged_tree_in_binary_path();
                continue_propagates_print_hints_failure_with_remaining_span_changes();
                continue_reports_remaining_tracked_changes_without_untracked_section();
                continue_reports_tracked_remaining_without_untracked_during_rebase();
                continue_reports_untracked_section_when_remaining_pool_has_untracked_files();
                continue_uses_original_commit_tree_when_expected_tree_state_missing();
                finish_runs_while_mid_rebase_when_requires_rebase_is_true();
                start_ignores_invalid_rev_list_lines_when_resolving_range_refs();
                start_omits_remaining_hint_when_diff_stat_is_empty_in_binary_path();
                start_propagates_commit_message_error_in_binary_path();
                start_propagates_print_hints_failure_after_session_starts();
                start_rejects_empty_short_sha_in_binary_path();
                start_rejects_invalid_exec_syntax_in_binary_path();
                start_rejects_merge_commit_in_binary_path();
                start_rejects_non_ancestor_commit_in_binary_path();
                start_reports_bash_spawn_error_when_shell_unavailable();
                start_reports_cannot_resolve_cwd_when_process_cwd_was_deleted();
                start_reports_git_spawn_error_during_ancestor_validation();
                start_reports_state_write_error_when_factor_path_is_file();
                status_accepts_absolute_git_dir_output_in_binary_path();
                status_reports_empty_current_index_state_file_in_binary_path();
                abort_reports_empty_start_head_state_file_in_binary_path();
            }));

            match result {
                Ok(()) => return,
                Err(payload) => {
                    let transient = match (
                        payload.downcast_ref::<&str>(),
                        payload.downcast_ref::<String>(),
                    ) {
                        (Some(message), _) => {
                            message.contains("Resource temporarily unavailable (os error 35)")
                        }
                        (None, Some(message)) => {
                            message.contains("Resource temporarily unavailable (os error 35)")
                        }
                        (None, None) => false,
                    };
                    if attempt < MAX_ATTEMPTS && transient {
                        sleep(Duration::from_millis(50));
                        continue;
                    }
                    resume_unwind(payload);
                }
            }
        }
    }
}
