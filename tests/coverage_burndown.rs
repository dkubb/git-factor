//! Coverage burn-down integration tests for `git-factor`.

#![forbid(unsafe_code)]
#![expect(
    clippy::implicit_return,
    reason = "integration tests favor concise tail expressions"
)]

extern crate alloc;

mod support;

#[cfg(test)]
#[expect(
    clippy::inline_modules,
    reason = "preserve the established inline test layout"
)]
mod tests {

    use alloc::collections::BTreeMap;
    use core::fmt::Write as _;
    use core::panic::AssertUnwindSafe;
    use core::time::Duration;
    use std::env;
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt as _;
    use std::panic::{catch_unwind, resume_unwind};
    use std::path::{Path, PathBuf};
    use std::process::{self, Command};
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
    use crate::support::native_git_bin;
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

    trait OrAbort<T> {
        fn or_abort(self) -> T;
    }
    impl<T, E> OrAbort<T> for Result<T, E> {
        fn or_abort(self) -> T {
            self.unwrap_or_else(|_| process::abort())
        }
    }
    impl<T> OrAbort<T> for Option<T> {
        fn or_abort(self) -> T {
            self.unwrap_or_else(|| process::abort())
        }
    }

    /// Read a required saved journal field, refusing absent or wrongly shaped objects.
    fn journal_field<'journal>(
        journal: &'journal serde_json::Value,
        pointer: &str,
    ) -> &'journal serde_json::Value {
        journal.pointer(pointer).or_abort()
    }

    /// Independently address the passing literal command and saved target tree.
    fn final_proof_key(repo: &Path, tree: &str) -> String {
        let external = TempDir::new().or_abort();
        let command = external.path().join("command");
        fs::write(&command, b"true").or_abort();
        let hash = git(
            repo,
            &["hash-object", "--no-filters", command.to_str().or_abort()],
        );
        format!("refs/factor/gates/{hash}/{tree}")
    }

    /// Count exact native proof-reference records before the terminal action.
    fn proof_key_count(before: &str, key: &str) -> usize {
        before
            .lines()
            .filter(|line| {
                line.split_once(' ')
                    .is_some_and(|(reference, _)| reference == key)
            })
            .count()
    }

    /// Preserve every existing proof except the newly accepted commit for the final tree.
    fn completed_proofs(before: &str, key: &str, atom: &str) -> String {
        before
            .lines()
            .map(|line| {
                let (reference, _object) = line.split_once(' ').or_abort();
                if reference == key {
                    format!("{reference} {atom}")
                } else {
                    line.to_owned()
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Observe every path and byte, excluding only the explicitly owned diagnostic output.
    fn repository_frame(repo: &Path) -> BTreeMap<PathBuf, Option<Vec<u8>>> {
        let mut frame = BTreeMap::new();
        let mut pending = vec![repo.to_path_buf()];
        while let Some(directory) = pending.pop() {
            for entry in fs::read_dir(directory).or_abort() {
                let path = entry.or_abort().path();
                let relative = path.strip_prefix(repo).or_abort();
                if relative == Path::new(".git/factor/error.log") {
                    continue;
                }
                let metadata = fs::symlink_metadata(&path).or_abort();
                if metadata.is_dir() {
                    frame.insert(relative.to_path_buf(), None);
                    pending.push(path);
                } else {
                    frame.insert(relative.to_path_buf(), Some(fs::read(path).or_abort()));
                }
            }
        }
        frame
    }

    /// Observe protected worktree bytes independently of Git's mutable metadata.
    fn user_frame(repo: &Path) -> BTreeMap<PathBuf, Option<Vec<u8>>> {
        repository_frame(repo)
            .into_iter()
            .filter(|entry| !entry.0.starts_with(".git"))
            .collect()
    }

    /// Expect only the known native terminal sequence and, when planned, the next opening.
    fn terminal_pattern(message: &str, next: Option<&str>) -> String {
        let mut callback = String::new();
        for character in fs::canonicalize(git_factor_bin())
            .or_abort()
            .display()
            .to_string()
            .chars()
        {
            if "\\.^$|?*+()[]{}".contains(character) {
                callback.push('\\');
            }
            callback.push(character);
        }
        let mut pattern = format!(
            concat!(
                r"\AHEAD is now at [0-9a-f]{{7,40}} {}\n",
                r"Rebasing \(3/4\)\rExecuting: '{}' checkpoint-gate-remainder\n",
                r"Rebasing \(4/4\)\rExecuting: '{}' checkpoint-terminal\n",
                r"Successfully rebased and updated refs/heads/main\.\n"
            ),
            message, callback, callback,
        );
        if let Some(next_message) = next {
            write!(pattern,
                r"Rebasing \(1/4\)\rRebasing \(2/4\)\rStopped at [0-9a-f]{{7,40}} \({next_message}\)\n"
            ).or_abort();
        }
        pattern.push_str(r"\z");
        pattern
    }

    /// Build the complete initial response from independently saved input facts.
    fn started_json(commit: &str, short: &str, message: &str, untracked: &[&str]) -> String {
        let encoded_commit = serde_json::to_string(commit).or_abort();
        let encoded_short = serde_json::to_string(short).or_abort();
        let encoded_message = serde_json::to_string(message).or_abort();
        let encoded_untracked = serde_json::to_string(untracked).or_abort();
        format!(
            concat!(
                "{{\"actions\":{{\"abort\":[\"git\",\"factor\",\"--abort\"],",
                "\"submit\":[\"git\",\"factor\",\"--continue\",\"--message\",\"<message>\"]}},",
                "\"changes\":{{\"unstaged\":[],\"untracked\":{untracked}}},",
                "\"guidance\":[\"Stage one independently valid atomic change.\",",
                "\"Use one concrete action in the commit message.\",",
                "\"Submit each atom through git factor so its gates run.\"],",
                "\"operation\":\"start\",\"references\":[],",
                "\"target\":{{\"commit\":{commit},\"commit_count\":1,\"message\":{message},\"short_commit\":{short}}}}}\n"
            ),
            commit = encoded_commit,
            short = encoded_short,
            message = encoded_message,
            untracked = encoded_untracked
        )
    }

    /// Keep every field and the final LF of the committed response explicit.
    fn committed_json(unstaged: &str, untracked: &str) -> String {
        format!(
            concat!(
                "{{\"operation\":\"continue\",\"result\":\"committed\",",
                "\"actions\":{{\"abort\":[\"git\",\"factor\",\"--abort\"],",
                "\"submit\":[\"git\",\"factor\",\"--continue\",\"--message\",\"<message>\"]}},",
                "\"changes\":{{\"unstaged\":{unstaged},\"untracked\":{untracked}}},",
                "\"guidance\":[\"Stage one independently valid atomic change.\",",
                "\"Use one concrete action in the commit message.\",",
                "\"Submit each atom through git factor so its gates run.\"],",
                "\"references\":[],\"split_count\":1}}\n"
            ),
            unstaged = unstaged,
            untracked = untracked,
        )
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
    fn continue_refuses_missing_required_final_tree_in_binary_path() {
        let dir = init_repo();
        let repo = dir.path();
        commit_file(repo, "tracked.txt", "v1\n", "feat: initial");
        commit_file(repo, "tracked.txt", "v2\n", "feat: second");

        Command::new(git_factor_bin())
            .current_dir(repo)
            .args(["--exec", "true", "HEAD"])
            .assert()
            .success();

        let journal_path = git_dir(repo).join("factor-journal.json");
        let saved = assert_ok!(fs::read(&journal_path), "save canonical journal");
        let mut journal: serde_json::Value =
            assert_ok!(serde_json::from_slice(&saved), "parse fixture journal");
        let removed = journal
            .as_object_mut()
            .and_then(|fields| fields.remove("final_tree"));
        assert!(
            removed.is_some(),
            "fixture has required final_tree authority"
        );
        let corrupt = format!("{journal}\n");
        assert_ok!(
            fs::write(&journal_path, &corrupt),
            "remove required final_tree authority"
        );
        let before = repository_frame(repo);
        let expected_error = format!(
            concat!(
                "git command failed: unsupported or corrupt checkpoint journal; ",
                "legacy sessions require their originating version: ",
                "missing field `final_tree` at line 1 column {}\n"
            ),
            corrupt.len().checked_sub(1).or_abort(),
        );

        Command::new(git_factor_bin())
            .current_dir(repo)
            .args(["--continue", "--message", "test: split"])
            .assert()
            .code(EXIT_SOFTWARE)
            .stdout(predicate::eq(""))
            .stderr(predicate::eq(expected_error));

        assert_eq!(repository_frame(repo), before);
        assert!(!git_dir(repo).join("factor/error.log").exists());
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
    fn abort_refuses_nonregular_canonical_journal_in_binary_path() {
        let dir = init_repo();
        let repo = dir.path();
        commit_file(repo, "tracked.txt", "v1\n", "feat: initial");
        start_session(repo);

        let journal = git_dir(repo).join("factor-journal.json");
        let saved = assert_ok!(fs::read(&journal), "save canonical journal");
        let archive = assert_ok!(TempDir::new(), "journal archive");
        assert_ok!(
            fs::write(archive.path().join("journal"), &saved),
            "archive journal"
        );
        assert_ok!(fs::remove_file(&journal), "remove canonical journal");
        assert_ok!(
            fs::create_dir_all(&journal),
            "replace canonical journal with directory"
        );
        let before = repository_frame(repo);

        Command::new(git_factor_bin())
            .current_dir(repo)
            .args(["--abort"])
            .assert()
            .code(EXIT_SOFTWARE)
            .stdout(predicate::eq(""))
            .stderr(predicate::eq(
                "git command failed: canonical checkpoint journal is not an owned regular file\n",
            ));

        let after = repository_frame(repo);
        assert_eq!(after, before);
        assert!(!git_dir(repo).join("factor/error.log").exists());
    }

    #[test]
    fn start_rejects_empty_short_sha_in_binary_path() {
        let dir = init_repo();
        let repo = dir.path();
        commit_file(repo, "tracked.txt", "v1\n", "feat: initial");

        let target = git(repo, &["rev-parse", "HEAD"]);
        let expected_tree = git(
            repo,
            &["show", "--format=%T", "--no-patch", "refs/heads/main"],
        );
        let protected = user_frame(repo);
        let receipts = assert_ok!(TempDir::new(), "fault receipts");
        let receipt = receipts.path().join("request");
        let journal_path = git_dir(repo).join("factor-journal.json");
        let (_wrapper_dir, wrapper_bin) = make_git_wrapper_named(
            "git",
            r#"
if [ "$#" -eq 3 ] && [ "$1" = "rev-parse" ] && [ "$2" = "--short" ] &&
   [ "$3" = "$GIT_FACTOR_TARGET" ] && [ ! -e "$GIT_FACTOR_FAULT_RECEIPT" ]; then
  grep -q '"phase":"selecting"' "$GIT_FACTOR_JOURNAL"
  printf '%s\n' "$PWD" "$@" > "$GIT_FACTOR_FAULT_RECEIPT"
  cp "$GIT_FACTOR_JOURNAL" "$GIT_FACTOR_FAULT_RECEIPT.journal"
  exit 0
fi
"#,
        );

        Command::new(git_factor_bin())
            .current_dir(repo)
            .env("PATH", prefixed_path!(&wrapper_bin))
            .env("GIT_FACTOR_FAULT_RECEIPT", &receipt)
            .env("GIT_FACTOR_TARGET", &target)
            .env("GIT_FACTOR_JOURNAL", &journal_path)
            .args(["--exec", "true", "HEAD"])
            .assert()
            .code(EXIT_SOFTWARE)
            .stdout(predicate::str::is_empty())
            .stderr(assert_ok!(
                predicate::str::is_match(concat!(
                    r"\ARebasing \(1/4\)\rRebasing \(2/4\)\rStopped at [0-9a-f]{7,40} ",
                    r"\(feat: initial\)\ngit command failed: empty short SHA\n\z"
                )),
                "opening then empty-short refusal grammar"
            ));
        assert_eq!(
            assert_ok!(fs::read_to_string(&receipt), "read exact request"),
            format!(
                "{}\nrev-parse\n--short\n{target}\n",
                assert_ok!(fs::canonicalize(repo), "canonical fixture cwd").display()
            )
        );
        assert_eq!(user_frame(repo), protected);
        let journal: serde_json::Value = assert_ok!(
            serde_json::from_slice(&assert_ok!(fs::read(&journal_path), "read journal")),
            "parse journal"
        );
        assert_eq!((*journal_field(&journal, "/state/phase")), "selecting");
        assert_eq!((*journal_field(&journal, "/original_tip")), target);
        assert!(git_dir(repo).join("rebase-merge").is_dir());
        assert_eq!(
            git(repo, &["rev-parse", "refs/factor/session-lease"]),
            assert_ok!(
                (*journal_field(&journal, "/state/lease"))
                    .as_str()
                    .ok_or("missing lease"),
                "lease"
            )
        );
        assert_eq!(git(repo, &["rev-parse", "refs/heads/main"]), target);
        let observed_journal = assert_ok!(
            fs::read(receipt.with_extension("journal")),
            "receipt journal"
        );
        assert_eq!(
            observed_journal,
            assert_ok!(fs::read(&journal_path), "durable journal")
        );
        let diagnostic = assert_ok!(
            fs::read_to_string(git_dir(repo).join("factor/error.log")),
            "owned diagnostic"
        );
        assert!(
            diagnostic
                .lines()
                .any(|line| line == "error=git command failed: empty short SHA")
        );
        assert_eq!((*journal_field(&journal, "/final_tree")), expected_tree);
        assert_eq!(git(repo, &["diff", "--cached", "--name-only"]), "");
    }

    #[test]
    fn start_reports_bash_spawn_error_when_shell_unavailable() {
        let dir = init_repo();
        let repo = dir.path();
        commit_file(repo, "tracked.txt", "v1\n", "feat: initial");

        let path_dir = assert_ok!(TempDir::new(), "tempdir");
        let git_shim = path_dir.path().join("git");
        assert_ok!(
            fs::write(
                &git_shim,
                "#!/bin/sh\nexec \"$GIT_FACTOR_NATIVE_GIT\" \"$@\"\n"
            ),
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
            .env("GIT_FACTOR_NATIVE_GIT", native_git_bin())
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
    "$GIT_FACTOR_NATIVE_GIT" "$@"
    status="$?"
    printf '%s\n' '#!/nonexistent/interpreter' > "$0"
    exit "$status"
  fi
fi

exec "$GIT_FACTOR_NATIVE_GIT" "$@"
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
            .env("GIT_FACTOR_NATIVE_GIT", native_git_bin())
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
    fn start_refuses_foreign_factor_path_in_binary_path() {
        let dir = init_repo();
        let repo = dir.path();
        commit_file(repo, "tracked.txt", "v1\n", "feat: initial");

        let state_path = git_dir(repo).join("factor");
        assert_ok!(fs::write(&state_path, "occupied\n"), "write factor file");

        let before = repository_frame(repo);

        Command::new(git_factor_bin())
            .current_dir(repo)
            .args(["--exec", "true", "HEAD"])
            .assert()
            .code(EXIT_SOFTWARE)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::eq(concat!(
                "git command failed: existing legacy or active session must be finished ",
                "or aborted with its originating version\n"
            )));

        assert_eq!(repository_frame(repo), before);
        assert!(!git_dir(repo).join("factor/error.log").exists());
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
    fn abort_refuses_blank_canonical_journal_in_binary_path() {
        let dir = init_repo();
        let repo = dir.path();
        commit_file(repo, "tracked.txt", "v1\n", "feat: initial");
        start_session(repo);

        let journal = git_dir(repo).join("factor-journal.json");
        let saved = assert_ok!(fs::read(&journal), "save canonical journal");
        let archive = assert_ok!(TempDir::new(), "journal archive");
        assert_ok!(
            fs::write(archive.path().join("journal"), &saved),
            "archive journal"
        );
        assert_ok!(
            fs::write(&journal, "\n"),
            "replace canonical journal with blank input"
        );
        let before = repository_frame(repo);

        Command::new(git_factor_bin())
            .current_dir(repo)
            .args(["--abort"])
            .assert()
            .code(EXIT_SOFTWARE)
            .stdout(predicate::eq(""))
            .stderr(predicate::eq(concat!(
                "git command failed: unsupported or corrupt checkpoint journal; ",
                "legacy sessions require their originating version: ",
                "EOF while parsing a value at line 2 column 0\n"
            )));

        let after = repository_frame(repo);
        assert_eq!(after, before);
        assert!(!git_dir(repo).join("factor/error.log").exists());
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
        let expected_tree = git(repo, &["show", "--format=%T", "--no-patch", "HEAD"]);

        start_session(repo);

        git(repo, &["add", "a.txt"]);

        let protected = user_frame(repo);
        Command::new(git_factor_bin())
            .current_dir(repo)
            .env_remove("CLAUDECODE")
            .args(["--continue", "--message", "feat: split a"])
            .assert()
            .code(EXIT_OK)
            .stdout(predicate::eq(committed_json(
                r#"[{"path":"b.txt","kind":"text","added":1,"deleted":1}]"#,
                "[]",
            )))
            .stderr(assert_ok!(
                predicate::str::is_match(terminal_pattern(
                    "feat: update both",
                    Some("feat: update both")
                )),
                "native terminal grammar"
            ));

        assert_eq!(user_frame(repo), protected);
        let journal: serde_json::Value = assert_ok!(
            serde_json::from_slice(&assert_ok!(
                fs::read(git_dir(repo).join("factor-journal.json")),
                "read Selecting journal"
            )),
            "parse Selecting journal"
        );
        assert_eq!((*journal_field(&journal, "/state/phase")), "selecting");
        let checkpoint = assert_ok!(
            (*journal_field(&journal, "/checkpoint"))
                .as_str()
                .ok_or("missing checkpoint"),
            "checkpoint"
        );
        assert_eq!(git(repo, &["rev-parse", "refs/heads/main"]), checkpoint);
        let atom = git(repo, &["rev-parse", "refs/heads/main^"]);
        assert_eq!((*journal_field(&journal, "/state/base")), atom);
        assert_eq!(
            git(repo, &["show", "--format=%s", "--no-patch", &atom]),
            "feat: split a"
        );
        assert_eq!(
            git(repo, &["rev-parse", "refs/factor/session-lease"]),
            assert_ok!(
                (*journal_field(&journal, "/state/lease"))
                    .as_str()
                    .ok_or("missing lease"),
                "lease"
            )
        );
        assert!(git_dir(repo).join("rebase-merge").is_dir());
        let source = assert_ok!(
            (*journal_field(&journal, "/state/source"))
                .as_str()
                .ok_or("missing Selecting source"),
            "Selecting source"
        );
        assert_eq!(
            git(repo, &["show", "--format=%T", "--no-patch", source]),
            expected_tree
        );
        assert_eq!(
            git(repo, &["rev-parse", "HEAD"]),
            assert_ok!(
                (*journal_field(&journal, "/state/head"))
                    .as_str()
                    .ok_or("missing head"),
                "Selecting head"
            )
        );
        assert_eq!(git(repo, &["diff", "--cached", "--name-only"]), "");
    }

    #[test]
    fn continue_completes_fully_staged_span_in_one_step_during_rebase() {
        let dir = init_repo();
        let repo = dir.path();
        commit_file(repo, "base.txt", "base\n", "feat: base");
        commit_file(repo, "a.txt", "a\n", "feat: add a");
        commit_file(repo, "b.txt", "b\n", "feat: add b");

        let expected_tree = git(repo, &["show", "--format=%T", "--no-patch", "HEAD"]);

        Command::new(git_factor_bin())
            .current_dir(repo)
            .env_remove("CLAUDECODE")
            .args(["--exec", "true", "HEAD~1", "HEAD"])
            .assert()
            .success();
        git(repo, &["add", "--all"]);

        let protected = user_frame(repo);
        let proof_key = final_proof_key(repo, &expected_tree);
        let proofs = git(
            repo,
            &[
                "for-each-ref",
                "--format=%(refname) %(objectname)",
                "refs/factor/gates",
            ],
        );
        assert_eq!(proof_key_count(&proofs, &proof_key), 1);

        Command::new(git_factor_bin())
            .current_dir(repo)
            .env_remove("CLAUDECODE")
            .args(["--continue", "--message", "feat: split first"])
            .assert()
            .code(EXIT_OK)
            .stdout(predicate::eq(
                "{\"operation\":\"continue\",\"result\":\"complete\",\"split_count\":1}\n",
            ))
            .stderr(assert_ok!(
                predicate::str::is_match(terminal_pattern("feat: split first", None)),
                "native terminal grammar"
            ));

        assert_eq!(user_frame(repo), protected);
        assert!(!git_dir(repo).join("factor-journal.json").exists());
        assert!(!git_dir(repo).join("factor").exists());
        assert!(!git_dir(repo).join("rebase-merge").exists());
        assert_eq!(git(repo, &["symbolic-ref", "HEAD"]), "refs/heads/main");
        assert_eq!(
            git(
                repo,
                &[
                    "for-each-ref",
                    "--format=%(refname) %(objectname)",
                    "refs/factor"
                ]
            ),
            completed_proofs(&proofs, &proof_key, &git(repo, &["rev-parse", "HEAD"]))
        );
        assert_eq!(
            git(repo, &["log", "-1", "--format=%s"]),
            "feat: split first"
        );
        assert_eq!(
            git(repo, &["show", "--format=%T", "--no-patch", "HEAD"]),
            expected_tree
        );
        assert_eq!(git(repo, &["diff", "--name-only"]), "");
        assert_eq!(git(repo, &["diff", "--cached", "--name-only"]), "");
    }

    #[test]
    fn continue_propagates_advance_error_after_converged_tree_in_binary_path() {
        let dir = init_repo();
        let repo = dir.path();
        commit_file(repo, "tracked.txt", "v1\n", "feat: initial");
        start_session(repo);
        git(repo, &["add", "--all"]);

        let expected_tree = git(
            repo,
            &["show", "--format=%T", "--no-patch", "refs/heads/main"],
        );
        let protected = user_frame(repo);
        let receipts = assert_ok!(TempDir::new(), "fault receipts");
        let receipt = receipts.path().join("request");
        let journal_path = git_dir(repo).join("factor-journal.json");
        let (_wrapper_dir, wrapper_bin) = make_git_wrapper_named(
            "git",
            r#"
if [ "$#" -eq 3 ] && [ "$1" = "rev-list" ] && [ "$2" = "--count" ] &&
   [ -f "$GIT_FACTOR_JOURNAL" ] && grep -q '"phase":"closing"' "$GIT_FACTOR_JOURNAL" &&
   [ "$3" = "$(/usr/bin/git rev-parse HEAD)" ] && [ ! -e "$GIT_FACTOR_FAULT_RECEIPT" ]; then
  printf '%s\n' "$PWD" "$@" > "$GIT_FACTOR_FAULT_RECEIPT"
  cp "$GIT_FACTOR_JOURNAL" "$GIT_FACTOR_FAULT_RECEIPT.journal"
  echo "forced rev-list failure" >&2
  exit 1
fi
"#,
        );
        let native_then_error = terminal_pattern("feat: split", None)
            .replace(r"\z", r"git command failed: forced rev-list failure\n\z");

        Command::new(git_factor_bin())
            .current_dir(repo)
            .env("PATH", prefixed_path!(&wrapper_bin))
            .env("GIT_FACTOR_FAULT_RECEIPT", &receipt)
            .env("GIT_FACTOR_JOURNAL", &journal_path)
            .args(["--continue", "--message", "feat: split"])
            .assert()
            .code(EXIT_SOFTWARE)
            .stdout(predicate::eq(""))
            .stderr(assert_ok!(
                predicate::str::is_match(native_then_error),
                "terminal then count refusal"
            ));

        let after = repository_frame(repo);
        assert_eq!(user_frame(repo), protected);
        let saved = assert_ok!(fs::read(&journal_path), "read Closing journal");
        let journal: serde_json::Value =
            assert_ok!(serde_json::from_slice(&saved), "parse Closing journal");
        let atom = git(repo, &["rev-parse", "refs/heads/main"]);
        assert_eq!((*journal_field(&journal, "/checkpoint")), atom);
        assert_eq!((*journal_field(&journal, "/state/phase")), "closing");
        assert_eq!((*journal_field(&journal, "/state/outcome/atom")), atom);
        assert_eq!(
            git(repo, &["show", "--format=%s", "--no-patch", &atom]),
            "feat: split"
        );
        assert_eq!(git(repo, &["rev-list", "--count", &atom]), "1");
        assert_eq!(
            assert_ok!(fs::read_to_string(&receipt), "read exact count request"),
            format!(
                "{}\nrev-list\n--count\n{atom}\n",
                assert_ok!(fs::canonicalize(repo), "canonical fixture cwd").display()
            )
        );
        assert_eq!(
            assert_ok!(
                fs::read(receipt.with_extension("journal")),
                "receipt journal"
            ),
            saved
        );
        assert!(!git_dir(repo).join("rebase-merge").exists());
        assert_eq!(
            git(repo, &["rev-parse", "refs/factor/session-lease"]),
            assert_ok!(
                (*journal_field(&journal, "/state/lease"))
                    .as_str()
                    .ok_or("missing lease"),
                "lease"
            )
        );
        assert!(after.contains_key(Path::new(".git/factor-journal.json")));
        let diagnostic = assert_ok!(
            fs::read_to_string(git_dir(repo).join("factor/error.log")),
            "owned diagnostic"
        );
        assert!(
            diagnostic
                .lines()
                .any(|line| line == "error=git command failed: forced rev-list failure")
        );
        assert_eq!((*journal_field(&journal, "/final_tree")), expected_tree);
        assert_eq!(
            git(repo, &["show", "--format=%T", "--no-patch", &atom]),
            expected_tree
        );
        assert_eq!(git(repo, &["diff", "--name-only"]), "");
        assert_eq!(git(repo, &["diff", "--cached", "--name-only"]), "");
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

        let expected_tree = git(repo, &["show", "--format=%T", "--no-patch", "HEAD"]);

        start_session(repo);
        git(repo, &["add", "a.txt"]);

        let protected = user_frame(repo);
        Command::new(git_factor_bin())
            .current_dir(repo)
            .env_remove("CLAUDECODE")
            .args(["--continue", "--message", "feat: split tracked"])
            .assert()
            .code(EXIT_OK)
            .stdout(predicate::eq(committed_json("[]", r#"["new.txt"]"#)))
            .stderr(assert_ok!(
                predicate::str::is_match(terminal_pattern(
                    "feat: tracked and new file",
                    Some("feat: tracked and new file")
                )),
                "native terminal grammar"
            ));

        assert_eq!(user_frame(repo), protected);
        let journal: serde_json::Value = assert_ok!(
            serde_json::from_slice(&assert_ok!(
                fs::read(git_dir(repo).join("factor-journal.json")),
                "read Selecting journal"
            )),
            "parse Selecting journal"
        );
        assert_eq!((*journal_field(&journal, "/state/phase")), "selecting");
        let checkpoint = assert_ok!(
            (*journal_field(&journal, "/checkpoint"))
                .as_str()
                .ok_or("missing checkpoint"),
            "checkpoint"
        );
        assert_eq!(git(repo, &["rev-parse", "refs/heads/main"]), checkpoint);
        let atom = git(repo, &["rev-parse", "refs/heads/main^"]);
        assert_eq!((*journal_field(&journal, "/state/base")), atom);
        assert_eq!(
            git(repo, &["show", "--format=%s", "--no-patch", &atom]),
            "feat: split tracked"
        );
        assert_eq!(
            git(repo, &["rev-parse", "refs/factor/session-lease"]),
            assert_ok!(
                (*journal_field(&journal, "/state/lease"))
                    .as_str()
                    .ok_or("missing lease"),
                "lease"
            )
        );
        assert!(git_dir(repo).join("rebase-merge").is_dir());
        let source = assert_ok!(
            (*journal_field(&journal, "/state/source"))
                .as_str()
                .ok_or("missing Selecting source"),
            "Selecting source"
        );
        assert_eq!(
            git(repo, &["show", "--format=%T", "--no-patch", source]),
            expected_tree
        );
        assert_eq!(
            git(repo, &["rev-parse", "HEAD"]),
            assert_ok!(
                (*journal_field(&journal, "/state/head"))
                    .as_str()
                    .ok_or("missing head"),
                "Selecting head"
            )
        );
        assert_eq!(git(repo, &["diff", "--cached", "--name-only"]), "");
    }

    #[test]
    fn continue_preserves_capture_after_selection_output_query_failure() {
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

        let expected_tree = git(
            repo,
            &["show", "--format=%T", "--no-patch", "refs/heads/main"],
        );
        let protected = user_frame(repo);
        let receipts = (TempDir::new()).or_abort();
        let receipt = receipts.path().join("request");
        let journal_path = git_dir(repo).join("factor-journal.json");
        let original_journal: serde_json::Value =
            (serde_json::from_slice(&(fs::read(&journal_path)).or_abort())).or_abort();
        let old_checkpoint = journal_field(&original_journal, "/checkpoint")
            .as_str()
            .or_abort();
        let (_wrapper_dir, wrapper_bin) = make_git_wrapper_named(
            "git",
            r#"
if [ "$#" -eq 2 ] && [ "$1" = "rev-parse" ] && [ "$2" = "--show-toplevel" ] &&
   [ -f "$GIT_FACTOR_JOURNAL" ] && grep -q '"phase":"selecting"' "$GIT_FACTOR_JOURNAL" &&
   ! grep -q "\"checkpoint\":\"$GIT_FACTOR_OLD_CHECKPOINT\"" "$GIT_FACTOR_JOURNAL" &&
   [ ! -e "$GIT_FACTOR_FAULT_RECEIPT" ]; then
  printf '%s\n' "$PWD" "$@" > "$GIT_FACTOR_FAULT_RECEIPT"
  cp "$GIT_FACTOR_JOURNAL" "$GIT_FACTOR_FAULT_RECEIPT.journal"
  echo "forced show-toplevel failure" >&2
  exit 1
fi
"#,
        );
        let native_then_error = terminal_pattern("feat: add b", Some("feat: add b")).replace(
            r"\z",
            r"git command failed: Git change query failed: forced show-toplevel failure\n\z",
        );

        Command::new(git_factor_bin())
            .current_dir(repo)
            .env("PATH", prefixed_path!(&wrapper_bin))
            .env("GIT_FACTOR_FAULT_RECEIPT", &receipt)
            .env("GIT_FACTOR_JOURNAL", &journal_path)
            .env("GIT_FACTOR_OLD_CHECKPOINT", old_checkpoint)
            .args(["--continue", "--message", "feat: split first"])
            .assert()
            .code(EXIT_SOFTWARE)
            .stdout(predicate::eq(""))
            .stderr((predicate::str::is_match(native_then_error)).or_abort());

        let after = repository_frame(repo);
        assert!(after.contains_key(Path::new(".git/index")));
        assert!(after.contains_key(Path::new(".git/HEAD")));
        assert!(after.contains_key(Path::new(".git/refs/heads/main")));
        assert!(after.contains_key(Path::new(".git/factor-journal.json")));
        assert_eq!(user_frame(repo), protected);
        let saved = (fs::read(&journal_path)).or_abort();
        let journal: serde_json::Value = (serde_json::from_slice(&saved)).or_abort();
        let checkpoint = git(repo, &["rev-parse", "refs/heads/main"]);
        assert_ne!(checkpoint, old_checkpoint);
        assert_eq!((*journal_field(&journal, "/checkpoint")), checkpoint);
        assert_eq!((*journal_field(&journal, "/state/phase")), "selecting");
        let atom = git(repo, &["rev-parse", "refs/heads/main^"]);
        assert_eq!((*journal_field(&journal, "/state/base")), atom);
        assert_eq!(
            git(repo, &["show", "--format=%s", "--no-patch", &atom]),
            "feat: split first"
        );
        assert_eq!(
            (fs::read_to_string(&receipt)).or_abort(),
            format!(
                "{}\nrev-parse\n--show-toplevel\n",
                (fs::canonicalize(repo)).or_abort().display()
            )
        );
        assert_eq!(
            (fs::read(receipt.with_extension("journal"))).or_abort(),
            saved
        );
        assert!(git_dir(repo).join("rebase-merge").is_dir());
        assert_eq!(
            git(repo, &["rev-parse", "HEAD"]),
            journal_field(&journal, "/state/head").as_str().or_abort()
        );
        assert_eq!(
            git(repo, &["rev-parse", "refs/factor/session-lease"]),
            journal_field(&journal, "/state/lease").as_str().or_abort()
        );
        let diagnostic = (fs::read_to_string(git_dir(repo).join("factor/error.log"))).or_abort();
        assert!(diagnostic.lines().any(|line| line
            == "error=git command failed: Git change query failed: forced show-toplevel failure"));
        assert_eq!((*journal_field(&journal, "/final_tree")), expected_tree);
        assert_eq!(git(repo, &["diff", "--cached", "--name-only"]), "");
    }

    #[test]
    fn start_refuses_protected_root_observation_failure_before_session() {
        let dir = init_repo();
        let repo = dir.path();
        commit_file(repo, "tracked.txt", "v1\n", "feat: initial");

        let before = repository_frame(repo);
        let receipts = assert_ok!(TempDir::new(), "fault receipts");
        let receipt = receipts.path().join("request");
        let (_wrapper_dir, wrapper_bin) = make_git_wrapper_named(
            "git",
            r#"
if [ "$#" -eq 2 ] && [ "$1" = "rev-parse" ] && [ "$2" = "--show-toplevel" ] &&
   [ ! -e "$GIT_FACTOR_FAULT_RECEIPT" ]; then
  printf '%s\n' "$PWD" "$@" > "$GIT_FACTOR_FAULT_RECEIPT"
  echo "forced show-toplevel failure" >&2
  exit 1
fi
"#,
        );

        Command::new(git_factor_bin())
            .current_dir(repo)
            .env("PATH", prefixed_path!(&wrapper_bin))
            .env("GIT_FACTOR_FAULT_RECEIPT", &receipt)
            .args(["--exec", "true", "HEAD"])
            .assert()
            .code(EXIT_SOFTWARE)
            .stdout(predicate::eq(""))
            .stderr(predicate::eq(
                "git command failed: cannot observe protected worktree root\n",
            ));

        assert_eq!(repository_frame(repo), before);
        assert_eq!(
            assert_ok!(fs::read_to_string(&receipt), "read exact root request"),
            format!(
                "{}\nrev-parse\n--show-toplevel\n",
                assert_ok!(fs::canonicalize(repo), "canonical fixture cwd").display()
            )
        );
        assert!(!git_dir(repo).join("factor-journal.json").exists());
    }

    #[test]
    fn start_preserves_selecting_after_selection_output_query_failure() {
        let dir = init_repo();
        let repo = dir.path();
        commit_file(repo, "tracked.txt", "v1\n", "feat: initial");
        let target = git(repo, &["rev-parse", "HEAD"]);
        let target_tree = git(repo, &["show", "--format=%T", "--no-patch", "HEAD"]);
        let protected = user_frame(repo);
        let receipts = assert_ok!(TempDir::new(), "fault receipts");
        let receipt = receipts.path().join("request");
        let journal_path = git_dir(repo).join("factor-journal.json");
        let (_wrapper_dir, wrapper_bin) = make_git_wrapper_named(
            "git",
            r#"
if [ "$#" -eq 2 ] && [ "$1" = "rev-parse" ] && [ "$2" = "--show-toplevel" ] &&
   [ -f "$GIT_FACTOR_JOURNAL" ] && grep -q '"phase":"selecting"' "$GIT_FACTOR_JOURNAL" &&
   [ ! -e "$GIT_FACTOR_FAULT_RECEIPT" ]; then
  printf '%s\n' "$PWD" "$@" > "$GIT_FACTOR_FAULT_RECEIPT"
  cp "$GIT_FACTOR_JOURNAL" "$GIT_FACTOR_FAULT_RECEIPT.journal"
  echo "forced show-toplevel failure" >&2
  exit 1
fi
"#,
        );

        Command::new(git_factor_bin())
            .current_dir(repo)
            .env("PATH", prefixed_path!(&wrapper_bin))
            .env("GIT_FACTOR_FAULT_RECEIPT", &receipt)
            .env("GIT_FACTOR_JOURNAL", &journal_path)
            .args(["--exec", "true", "HEAD"])
            .assert()
            .code(EXIT_SOFTWARE)
            .stdout(predicate::eq(""))
            .stderr(assert_ok!(predicate::str::is_match(concat!(
                r"\ARebasing \(1/4\)\rRebasing \(2/4\)\rStopped at [0-9a-f]{7,40} ",
                r"\(feat: initial\)\ngit command failed: Git change query failed: forced show-toplevel failure\n\z"
            )), "Opening completes before selection output failure"));

        let after = repository_frame(repo);
        assert!(after.contains_key(Path::new(".git/index")));
        assert!(after.contains_key(Path::new(".git/HEAD")));
        assert!(after.contains_key(Path::new(".git/refs/heads/main")));
        assert!(after.contains_key(Path::new(".git/factor-journal.json")));
        assert_eq!(user_frame(repo), protected);
        let saved = assert_ok!(fs::read(&journal_path), "read Selecting journal");
        let journal: serde_json::Value =
            assert_ok!(serde_json::from_slice(&saved), "parse Selecting journal");
        assert_eq!((*journal_field(&journal, "/original_tip")), target);
        assert_eq!((*journal_field(&journal, "/checkpoint")), target);
        assert_eq!((*journal_field(&journal, "/final_tree")), target_tree);
        assert_eq!((*journal_field(&journal, "/state/phase")), "selecting");
        assert_eq!(
            assert_ok!(fs::read_to_string(&receipt), "read exact output request"),
            format!(
                "{}\nrev-parse\n--show-toplevel\n",
                assert_ok!(fs::canonicalize(repo), "canonical fixture cwd").display()
            )
        );
        assert_eq!(
            assert_ok!(
                fs::read(receipt.with_extension("journal")),
                "receipt journal"
            ),
            saved
        );
        assert!(git_dir(repo).join("rebase-merge").is_dir());
        assert_eq!(git(repo, &["rev-parse", "refs/heads/main"]), target);
        assert_eq!(
            git(repo, &["rev-parse", "HEAD"]),
            assert_ok!(
                (*journal_field(&journal, "/state/head"))
                    .as_str()
                    .ok_or("missing head"),
                "Selecting head"
            )
        );
        assert_eq!(
            git(repo, &["rev-parse", "refs/factor/session-lease"]),
            assert_ok!(
                (*journal_field(&journal, "/state/lease"))
                    .as_str()
                    .ok_or("missing lease"),
                "lease"
            )
        );
        let diagnostic = assert_ok!(
            fs::read_to_string(git_dir(repo).join("factor/error.log")),
            "owned diagnostic"
        );
        assert!(diagnostic.lines().any(|line| line
            == "error=git command failed: Git change query failed: forced show-toplevel failure"));
    }

    #[test]
    fn status_refuses_blank_initial_range_count_in_binary_path() {
        let dir = init_repo();
        let repo = dir.path();
        commit_file(repo, "tracked.txt", "v1\n", "feat: initial");
        start_session(repo);

        let target = git(repo, &["rev-parse", "refs/heads/main"]);
        let receipts = assert_ok!(TempDir::new(), "fault receipts");
        let receipt = receipts.path().join("request");
        let before = repository_frame(repo);
        let (_wrapper_dir, wrapper_bin) = make_git_wrapper_named(
            "git",
            r#"
if [ "$#" -eq 3 ] && [ "$1" = "rev-list" ] && [ "$2" = "--count" ] &&
   [ "$3" = "$GIT_FACTOR_TARGET" ]; then
  printf '%s\n' "$PWD" "$@" > "$GIT_FACTOR_FAULT_RECEIPT"
  printf '\n'
  exit 0
fi
"#,
        );

        Command::new(git_factor_bin())
            .current_dir(repo)
            .env("PATH", prefixed_path!(&wrapper_bin))
            .env("GIT_FACTOR_FAULT_RECEIPT", &receipt)
            .env("GIT_FACTOR_TARGET", &target)
            .arg("--status")
            .assert()
            .code(EXIT_SOFTWARE)
            .stdout(predicate::eq(""))
            .stderr(predicate::eq(concat!(
                "git command failed: initial range count is invalid: ",
                "cannot parse integer from empty string\n"
            )));

        assert_eq!(repository_frame(repo), before);
        assert_eq!(
            assert_ok!(fs::read_to_string(&receipt), "read exact count request"),
            format!(
                "{}\nrev-list\n--count\n{target}\n",
                assert_ok!(fs::canonicalize(repo), "canonical fixture cwd").display()
            )
        );
        assert!(!git_dir(repo).join("factor/error.log").exists());
    }

    #[test]
    fn continue_completes_rebase_after_final_span_step_in_binary_path() {
        let dir = init_repo();
        let repo = dir.path();
        commit_file(repo, "base.txt", "base\n", "feat: base");
        commit_file(repo, "a.txt", "a1\n", "feat: add a");
        commit_file(repo, "b.txt", "b1\n", "feat: add b");

        let expected_tree = git(repo, &["show", "--format=%T", "--no-patch", "HEAD"]);

        Command::new(git_factor_bin())
            .current_dir(repo)
            .env_remove("CLAUDECODE")
            .args(["--exec", "true", "HEAD~1", "HEAD"])
            .assert()
            .success();

        let protected = user_frame(repo);
        git(repo, &["add", "a.txt"]);
        Command::new(git_factor_bin())
            .current_dir(repo)
            .env_remove("CLAUDECODE")
            .args(["--continue", "--message", "feat: split first"])
            .assert()
            .code(EXIT_OK)
            .stdout(predicate::eq(committed_json("[]", r#"["b.txt"]"#)))
            .stderr(assert_ok!(
                predicate::str::is_match(terminal_pattern("feat: add b", Some("feat: add b"))),
                "first capture native grammar"
            ));
        let captured = git(repo, &["rev-parse", "refs/heads/main"]);
        let first = git(repo, &["rev-parse", "refs/heads/main^"]);
        let first_journal = fs::read(git_dir(repo).join("factor-journal.json")).or_abort();
        let selecting: serde_json::Value = serde_json::from_slice(&first_journal).or_abort();
        assert_eq!((*journal_field(&selecting, "/checkpoint")), captured);
        assert_eq!((*journal_field(&selecting, "/state/base")), first);
        assert_eq!(
            git(repo, &["show", "--format=%s", "--no-patch", &first]),
            "feat: split first"
        );
        let original_base = assert_ok!(
            (*journal_field(&selecting, "/original_base"))
                .as_str()
                .ok_or("missing original base"),
            "original base"
        );
        assert_eq!(
            git(
                repo,
                &["rev-list", "--count", &format!("{original_base}..{first}")]
            ),
            "1"
        );

        git(repo, &["add", "--all"]);
        let proof_key = final_proof_key(repo, &expected_tree);
        let proofs = git(
            repo,
            &[
                "for-each-ref",
                "--format=%(refname) %(objectname)",
                "refs/factor/gates",
            ],
        );
        assert_eq!(proof_key_count(&proofs, &proof_key), 1);

        Command::new(git_factor_bin())
            .current_dir(repo)
            .env_remove("CLAUDECODE")
            .args(["--continue", "--message", "feat: split second"])
            .assert()
            .code(EXIT_OK)
            .stdout(predicate::eq(
                "{\"operation\":\"continue\",\"result\":\"complete\",\"split_count\":2}\n",
            ))
            .stderr(assert_ok!(
                predicate::str::is_match(terminal_pattern("feat: split second", None)),
                "final capture native grammar"
            ));

        assert_eq!(user_frame(repo), protected);
        assert_eq!(
            git(repo, &["log", "-2", "--format=%s"]),
            "feat: split second\nfeat: split first"
        );
        assert_eq!(git(repo, &["rev-parse", "HEAD~1"]), first);
        assert!(!git_dir(repo).join("factor-journal.json").exists());
        assert!(!git_dir(repo).join("factor").exists());
        assert!(!git_dir(repo).join("rebase-merge").exists());
        assert_eq!(
            git(
                repo,
                &[
                    "for-each-ref",
                    "--format=%(refname) %(objectname)",
                    "refs/factor"
                ]
            ),
            completed_proofs(&proofs, &proof_key, &git(repo, &["rev-parse", "HEAD"]))
        );
        assert_eq!(
            git(repo, &["show", "--format=%T", "--no-patch", "HEAD"]),
            expected_tree
        );
        assert_eq!(git(repo, &["diff", "--name-only"]), "");
        assert_eq!(git(repo, &["diff", "--cached", "--name-only"]), "");
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

        let expected_tree = git(repo, &["show", "--format=%T", "--no-patch", "HEAD"]);

        Command::new(git_factor_bin())
            .current_dir(repo)
            .env_remove("CLAUDECODE")
            .args(["--exec", "true", "HEAD~1", "HEAD"])
            .assert()
            .success();

        git(repo, &["add", "a.txt"]);
        let protected = user_frame(repo);
        Command::new(git_factor_bin())
            .current_dir(repo)
            .env_remove("CLAUDECODE")
            .args(["--continue", "--message", "feat: split a only"])
            .assert()
            .code(EXIT_OK)
            .stdout(predicate::eq(committed_json(
                r#"[{"path":"b.txt","kind":"text","added":1,"deleted":1}]"#,
                "[]",
            )))
            .stderr(assert_ok!(
                predicate::str::is_match(terminal_pattern(
                    "feat: refine b",
                    Some("feat: refine b")
                )),
                "native terminal grammar"
            ));

        assert_eq!(user_frame(repo), protected);
        let journal: serde_json::Value = assert_ok!(
            serde_json::from_slice(&assert_ok!(
                fs::read(git_dir(repo).join("factor-journal.json")),
                "read Selecting journal"
            )),
            "parse Selecting journal"
        );
        assert_eq!((*journal_field(&journal, "/state/phase")), "selecting");
        let checkpoint = assert_ok!(
            (*journal_field(&journal, "/checkpoint"))
                .as_str()
                .ok_or("missing checkpoint"),
            "checkpoint"
        );
        assert_eq!(git(repo, &["rev-parse", "refs/heads/main"]), checkpoint);
        let atom = git(repo, &["rev-parse", "refs/heads/main^"]);
        assert_eq!((*journal_field(&journal, "/state/base")), atom);
        assert_eq!(
            git(repo, &["show", "--format=%s", "--no-patch", &atom]),
            "feat: split a only"
        );
        assert_eq!(
            git(repo, &["rev-parse", "refs/factor/session-lease"]),
            assert_ok!(
                (*journal_field(&journal, "/state/lease"))
                    .as_str()
                    .ok_or("missing lease"),
                "lease"
            )
        );
        assert!(git_dir(repo).join("rebase-merge").is_dir());
        let source = assert_ok!(
            (*journal_field(&journal, "/state/source"))
                .as_str()
                .ok_or("missing Selecting source"),
            "Selecting source"
        );
        assert_eq!(
            git(repo, &["show", "--format=%T", "--no-patch", source]),
            expected_tree
        );
        assert_eq!(
            git(repo, &["rev-parse", "HEAD"]),
            assert_ok!(
                (*journal_field(&journal, "/state/head"))
                    .as_str()
                    .ok_or("missing head"),
                "Selecting head"
            )
        );
        assert_eq!(git(repo, &["diff", "--cached", "--name-only"]), "");
    }

    #[test]
    fn start_reports_empty_tracked_summary_with_untracked_pool_in_binary_path() {
        let dir = init_repo();
        let repo = dir.path();
        commit_file(repo, "tracked.txt", "v1\n", "feat: initial");

        let target = git(repo, &["rev-parse", "HEAD"]);
        let short = git(repo, &["rev-parse", "--short", "HEAD"]);
        let protected = user_frame(repo);
        let receipts = assert_ok!(TempDir::new(), "fault receipts");
        let receipt = receipts.path().join("request");
        let journal_path = git_dir(repo).join("factor-journal.json");
        let (_wrapper_dir, wrapper_bin) = make_git_wrapper_named(
            "git",
            r#"
if [ "$#" -eq 5 ] && [ "$1" = "diff" ] && [ "$2" = "--numstat" ] &&
   [ "$3" = "-z" ] && [ "$4" = "--no-renames" ] && [ "$5" = "--no-relative" ]; then
  printf '%s\n' "$PWD" "$@" > "$GIT_FACTOR_FAULT_RECEIPT"
  cp "$GIT_FACTOR_JOURNAL" "$GIT_FACTOR_FAULT_RECEIPT.journal"
  exit 0
fi
"#,
        );

        Command::new(git_factor_bin())
            .current_dir(repo)
            .env_remove("CLAUDECODE")
            .env("PATH", prefixed_path!(&wrapper_bin))
            .env("GIT_FACTOR_FAULT_RECEIPT", &receipt)
            .env("GIT_FACTOR_TARGET", &target)
            .env("GIT_FACTOR_JOURNAL", &journal_path)
            .args(["--exec", "true", "HEAD"])
            .assert()
            .code(EXIT_OK)
            .stdout(predicate::eq(started_json(
                &target,
                &short,
                "feat: initial",
                &["tracked.txt"],
            )))
            .stderr(assert_ok!(
                predicate::str::is_match(concat!(
                    r"\ARebasing \(1/4\)\rRebasing \(2/4\)\rStopped at [0-9a-f]{7,40} ",
                    r"\(feat: initial\)\n\z"
                )),
                "owned opening grammar"
            ));

        assert_eq!(
            assert_ok!(fs::read_to_string(&receipt), "read exact request"),
            format!(
                "{}\ndiff\n--numstat\n-z\n--no-renames\n--no-relative\n",
                assert_ok!(fs::canonicalize(repo), "canonical fixture cwd").display()
            )
        );
        assert_eq!(user_frame(repo), protected);
        let journal: serde_json::Value = assert_ok!(
            serde_json::from_slice(&assert_ok!(fs::read(&journal_path), "read journal")),
            "parse journal"
        );
        assert_eq!((*journal_field(&journal, "/state/phase")), "selecting");
        assert_eq!((*journal_field(&journal, "/original_tip")), target);
        assert!(git_dir(repo).join("rebase-merge").is_dir());
        assert_eq!(
            git(repo, &["rev-parse", "refs/factor/session-lease"]),
            assert_ok!(
                (*journal_field(&journal, "/state/lease"))
                    .as_str()
                    .ok_or("missing lease"),
                "lease"
            )
        );
        assert_eq!(git(repo, &["rev-parse", "refs/heads/main"]), target);
        let observed_journal = assert_ok!(
            fs::read(receipt.with_extension("journal")),
            "receipt journal"
        );
        assert_eq!(
            observed_journal,
            assert_ok!(fs::read(&journal_path), "durable journal")
        );
    }

    #[test]
    fn abort_finishes_canonical_session_after_native_rebase_abort() {
        let dir = init_repo();
        let repo = dir.path();
        commit_file(repo, "tracked.txt", "v1\n", "feat: initial");
        commit_file(repo, "tracked.txt", "v2\n", "feat: update");
        let checkpoint = git(repo, &["rev-parse", "HEAD"]);
        let expected_tree = git(repo, &["rev-parse", "HEAD^{tree}"]);
        start_session(repo);
        assert!(git_dir(repo).join("rebase-merge").is_dir());
        let journal_path = git_dir(repo).join("factor-journal.json");
        let selecting = assert_ok!(fs::read(&journal_path), "save canonical Selecting journal");
        let journal: serde_json::Value = assert_ok!(
            serde_json::from_slice(&selecting),
            "parse Selecting journal"
        );
        assert_eq!((*journal_field(&journal, "/state/phase")), "selecting");
        let lease = git(repo, &["rev-parse", "refs/factor/session-lease"]);
        assert_eq!((*journal_field(&journal, "/state/lease")), lease);
        assert_eq!((*journal_field(&journal, "/checkpoint")), checkpoint);
        git(repo, &["rebase", "--abort"]);
        assert!(!git_dir(repo).join("rebase-merge").exists());
        assert_eq!(
            assert_ok!(
                fs::read(&journal_path),
                "observe surviving Selecting journal"
            ),
            selecting
        );
        assert_eq!(
            git(repo, &["rev-parse", "refs/factor/session-lease"]),
            lease
        );
        let proofs = git(
            repo,
            &[
                "for-each-ref",
                "--format=%(refname) %(objectname)",
                "refs/factor/gates",
            ],
        );
        assert!(
            !proofs.is_empty(),
            "baseline acceptance retains durable gate proofs"
        );
        let protected = user_frame(repo);
        let index = fs::read(git_dir(repo).join("index")).or_abort();
        let receipts = assert_ok!(TempDir::new(), "native refusal receipts");
        let receipt = receipts.path().join("request");
        let (_wrapper_dir, wrapper_bin) = make_git_wrapper_named(
            "git",
            r#"
if [ "$#" -eq 2 ] && [ "$1" = "rebase" ] && [ "$2" = "--abort" ]; then
  printf '%s\n' "$PWD" "$@" > "$GIT_FACTOR_FAULT_RECEIPT"
  echo "unexpected second native rebase abort" >&2
  exit 99
fi
"#,
        );

        Command::new(git_factor_bin())
            .current_dir(repo)
            .env("PATH", prefixed_path!(&wrapper_bin))
            .env("GIT_FACTOR_FAULT_RECEIPT", &receipt)
            .arg("--abort")
            .assert()
            .code(EXIT_OK)
            .stdout(predicate::eq(
                "{\"operation\":\"abort\",\"rebase\":{\"in_progress\":false},\"actions\":{}}\n",
            ))
            .stderr(predicate::str::is_empty());

        assert!(
            !receipt.exists(),
            "native rebase abort must be skipped after native recovery"
        );
        assert_eq!(user_frame(repo), protected);
        assert_eq!(
            assert_ok!(
                fs::read(git_dir(repo).join("index")),
                "observe recovered index"
            ),
            index
        );
        assert_eq!(git(repo, &["rev-parse", "HEAD"]), checkpoint);
        assert_eq!(git(repo, &["rev-parse", "refs/heads/main"]), checkpoint);
        assert_eq!(git(repo, &["rev-parse", "HEAD^{tree}"]), expected_tree);
        assert_eq!(git(repo, &["symbolic-ref", "HEAD"]), "refs/heads/main");
        assert!(!git_dir(repo).join("factor-journal.json").exists());
        assert!(!git_dir(repo).join("factor").exists());
        assert!(!git_dir(repo).join("rebase-merge").exists());
        assert_eq!(
            git(
                repo,
                &[
                    "for-each-ref",
                    "--format=%(refname) %(objectname)",
                    "refs/factor"
                ]
            ),
            proofs
        );
    }

    #[test]
    fn finish_completes_canonical_session_during_native_rebase() {
        let dir = init_repo();
        let repo = dir.path();
        commit_file(repo, "base.txt", "base\n", "feat: base");
        commit_file(repo, "a.txt", "a1\n", "feat: add a");
        commit_file(repo, "b.txt", "b1\n", "feat: add b");

        let expected_tree = git(repo, &["show", "--format=%T", "--no-patch", "HEAD"]);

        Command::new(git_factor_bin())
            .current_dir(repo)
            .env_remove("CLAUDECODE")
            .args(["--exec", "true", "HEAD~1", "HEAD"])
            .assert()
            .success();

        git(repo, &["add", "--all"]);
        let protected = user_frame(repo);
        let proof_key = final_proof_key(repo, &expected_tree);
        let proofs = git(
            repo,
            &[
                "for-each-ref",
                "--format=%(refname) %(objectname)",
                "refs/factor/gates",
            ],
        );
        assert_eq!(proof_key_count(&proofs, &proof_key), 1);

        Command::new(git_factor_bin())
            .current_dir(repo)
            .env_remove("CLAUDECODE")
            .args(["--finish", "--message", "feat: finish remaining"])
            .assert()
            .code(EXIT_OK)
            .stdout(predicate::eq(
                "{\"operation\":\"finish\",\"result\":\"complete\",\"split_count\":1}\n",
            ))
            .stderr(assert_ok!(
                predicate::str::is_match(terminal_pattern("feat: finish remaining", None)),
                "native terminal grammar"
            ));

        assert_eq!(user_frame(repo), protected);
        assert!(!git_dir(repo).join("factor-journal.json").exists());
        assert!(!git_dir(repo).join("factor").exists());
        assert!(!git_dir(repo).join("rebase-merge").exists());
        assert_eq!(git(repo, &["symbolic-ref", "HEAD"]), "refs/heads/main");
        assert_eq!(
            git(
                repo,
                &[
                    "for-each-ref",
                    "--format=%(refname) %(objectname)",
                    "refs/factor"
                ]
            ),
            completed_proofs(&proofs, &proof_key, &git(repo, &["rev-parse", "HEAD"]))
        );
        assert_eq!(
            git(repo, &["log", "-1", "--format=%s"]),
            "feat: finish remaining"
        );
        assert_eq!(
            git(repo, &["show", "--format=%T", "--no-patch", "HEAD"]),
            expected_tree
        );
        assert_eq!(git(repo, &["diff", "--name-only"]), "");
        assert_eq!(git(repo, &["diff", "--cached", "--name-only"]), "");
    }

    #[test]
    fn start_ignores_invalid_rev_list_lines_when_resolving_range_refs() {
        let dir = init_repo();
        let repo = dir.path();
        commit_file(repo, "base.txt", "base\n", "feat: base");
        commit_file(repo, "next.txt", "next\n", "feat: next");

        let target = git(repo, &["rev-parse", "HEAD"]);
        let short = git(repo, &["rev-parse", "--short", "HEAD"]);
        let protected = user_frame(repo);
        let receipts = assert_ok!(TempDir::new(), "fault receipts");
        let receipt = receipts.path().join("request");
        let journal_path = git_dir(repo).join("factor-journal.json");
        let (_wrapper_dir, wrapper_bin) = make_git_wrapper_named(
            "git",
            r#"
if [ "$#" -eq 4 ] && [ "$1" = "rev-list" ] && [ "$2" = "--reverse" ] &&
   [ "$3" = "--ancestry-path" ] && [ "$4" = "HEAD~1..HEAD" ]; then
  printf '%s\n' "$PWD" "$@" > "$GIT_FACTOR_FAULT_RECEIPT"
  /usr/bin/git "$@"
  printf '%s\n' 'not-a-commit-sha'
  exit 0
fi
"#,
        );

        Command::new(git_factor_bin())
            .current_dir(repo)
            .env_remove("CLAUDECODE")
            .env("PATH", prefixed_path!(&wrapper_bin))
            .env("GIT_FACTOR_FAULT_RECEIPT", &receipt)
            .env("GIT_FACTOR_TARGET", &target)
            .env("GIT_FACTOR_JOURNAL", &journal_path)
            .args(["--exec", "true", "HEAD~1..HEAD"])
            .assert()
            .code(EXIT_OK)
            .stdout(predicate::eq(started_json(
                &target,
                &short,
                "feat: next",
                &["next.txt"],
            )))
            .stderr(assert_ok!(
                predicate::str::is_match(concat!(
                    r"\ARebasing \(1/4\)\rRebasing \(2/4\)\rStopped at [0-9a-f]{7,40} ",
                    r"\(feat: next\)\n\z"
                )),
                "owned opening grammar"
            ));

        assert_eq!(
            assert_ok!(fs::read_to_string(&receipt), "read exact request"),
            format!(
                "{}\nrev-list\n--reverse\n--ancestry-path\nHEAD~1..HEAD\n",
                assert_ok!(fs::canonicalize(repo), "canonical fixture cwd").display()
            )
        );
        assert_eq!(user_frame(repo), protected);
        let journal: serde_json::Value = assert_ok!(
            serde_json::from_slice(&assert_ok!(fs::read(&journal_path), "read journal")),
            "parse journal"
        );
        assert_eq!((*journal_field(&journal, "/state/phase")), "selecting");
        assert_eq!((*journal_field(&journal, "/original_tip")), target);
        assert!(git_dir(repo).join("rebase-merge").is_dir());
        assert_eq!(
            git(repo, &["rev-parse", "refs/factor/session-lease"]),
            assert_ok!(
                (*journal_field(&journal, "/state/lease"))
                    .as_str()
                    .ok_or("missing lease"),
                "lease"
            )
        );
        assert_eq!(git(repo, &["rev-parse", "refs/heads/main"]), target);
    }

    #[test]
    fn proptest_run_integration_coverage_suite() {
        const MAX_ATTEMPTS: usize = 3;
        for attempt in 1..=MAX_ATTEMPTS {
            let result = catch_unwind(AssertUnwindSafe(|| {
                abort_refuses_nonregular_canonical_journal_in_binary_path();
                abort_finishes_canonical_session_after_native_rebase_abort();
                continue_completes_fully_staged_span_in_one_step_during_rebase();
                continue_completes_rebase_after_final_span_step_in_binary_path();
                continue_propagates_advance_error_after_converged_tree_in_binary_path();
                continue_preserves_capture_after_selection_output_query_failure();
                continue_reports_remaining_tracked_changes_without_untracked_section();
                continue_reports_tracked_remaining_without_untracked_during_rebase();
                continue_reports_untracked_section_when_remaining_pool_has_untracked_files();
                continue_refuses_missing_required_final_tree_in_binary_path();
                finish_completes_canonical_session_during_native_rebase();
                start_ignores_invalid_rev_list_lines_when_resolving_range_refs();
                start_reports_empty_tracked_summary_with_untracked_pool_in_binary_path();
                start_propagates_commit_message_error_in_binary_path();
                start_refuses_protected_root_observation_failure_before_session();
                start_preserves_selecting_after_selection_output_query_failure();
                start_rejects_empty_short_sha_in_binary_path();
                start_rejects_invalid_exec_syntax_in_binary_path();
                start_rejects_merge_commit_in_binary_path();
                start_rejects_non_ancestor_commit_in_binary_path();
                start_reports_bash_spawn_error_when_shell_unavailable();
                start_reports_cannot_resolve_cwd_when_process_cwd_was_deleted();
                start_reports_git_spawn_error_during_ancestor_validation();
                start_refuses_foreign_factor_path_in_binary_path();
                status_accepts_absolute_git_dir_output_in_binary_path();
                status_refuses_blank_initial_range_count_in_binary_path();
                abort_refuses_blank_canonical_journal_in_binary_path();
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
