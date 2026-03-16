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
    use core::panic::AssertUnwindSafe;
    use core::time::Duration;
    use std::env;
    use std::ffi::{OsStr, OsString};
    use std::fs;
    use std::io::ErrorKind;
    use std::panic::{catch_unwind, resume_unwind};
    use std::path::{Path, PathBuf};
    use std::process::{self, Command};
    use std::thread::sleep;

    use git_factor::non_empty_string::NonEmptyString;
    use tempfile::TempDir;

    use super::support::*;

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

    #[derive(Clone, Debug, Eq, PartialEq)]
    struct StreamExpectation {
        exact: Option<String>,
        suffix: Option<String>,
    }

    impl StreamExpectation {
        fn assert_matches(&self, stream_name: &str, actual: &str) {
            if let Some(expected_value) = self.exact.as_deref() {
                assert_eq!(
                    actual, expected_value,
                    "{stream_name} mismatch\nexpected:\n{expected_value}\nactual:\n{actual}"
                );
                return;
            }

            if let Some(expected_value) = self.suffix.as_deref() {
                assert!(
                    actual.ends_with(expected_value),
                    "{stream_name} suffix mismatch\nexpected suffix:\n{expected_value}\nactual:\n{actual}"
                );
            }
        }

        fn exact(&self) -> Option<&str> {
            self.exact.as_deref()
        }

        fn new_exact(expected_value: String) -> Self {
            Self {
                exact: Some(expected_value),
                suffix: None,
            }
        }

        fn new_suffix(expected_value: String) -> Self {
            Self {
                exact: None,
                suffix: Some(expected_value),
            }
        }
    }

    #[derive(Clone, Debug, Eq, PartialEq)]
    struct GitFactorExpectation {
        args: Vec<OsString>,
        bin_path: Option<PathBuf>,
        code: i32,
        envs: Vec<(OsString, OsString)>,
        factor_state_exists: Option<bool>,
        git_outputs: Vec<(Vec<String>, String)>,
        git_outputs_non_empty: Vec<Vec<String>>,
        git_status_porcelain: Option<String>,
        git_status_porcelain_non_empty: Option<bool>,
        head_sha: Option<String>,
        path_contents: Vec<(String, String)>,
        path_env: Option<OsString>,
        path_exists: Vec<(String, bool)>,
        rebase_apply_exists: Option<bool>,
        rebase_merge_exists: Option<bool>,
        repo: Option<PathBuf>,
        requires_rebase: Option<bool>,
        stderr: Option<StreamExpectation>,
        stdout: Option<StreamExpectation>,
    }

    impl Default for GitFactorExpectation {
        fn default() -> Self {
            Self {
                args: Vec::new(),
                bin_path: None,
                code: EXIT_OK,
                envs: Vec::new(),
                factor_state_exists: None,
                git_outputs: Vec::new(),
                git_outputs_non_empty: Vec::new(),
                git_status_porcelain: None,
                git_status_porcelain_non_empty: None,
                head_sha: None,
                path_contents: Vec::new(),
                path_env: None,
                path_exists: Vec::new(),
                rebase_apply_exists: None,
                rebase_merge_exists: None,
                repo: None,
                requires_rebase: None,
                stderr: None,
                stdout: None,
            }
        }
    }

    impl GitFactorExpectation {
        fn code(mut self, code: i32) -> Self {
            assert_ne!(
                code, EXIT_OK,
                "code called with default value EXIT_OK; omit it from the expectation"
            );
            self.code = code;
            self
        }

        fn factor_state_exists(mut self, factor_state_exists: bool) -> Self {
            self.factor_state_exists = Some(factor_state_exists);
            self
        }

        fn git_output(mut self, args: &[&str], expected_output: impl Into<String>) -> Self {
            self.git_outputs.push((
                args.iter().map(|arg| (*arg).to_owned()).collect(),
                expected_output.into(),
            ));
            self
        }

        fn git_output_non_empty(mut self, args: &[&str]) -> Self {
            self.git_outputs_non_empty
                .push(args.iter().map(|arg| (*arg).to_owned()).collect());
            self
        }

        fn git_status_porcelain(mut self, git_status_porcelain: impl Into<String>) -> Self {
            self.git_status_porcelain = Some(git_status_porcelain.into());
            self
        }

        fn git_status_porcelain_non_empty(mut self) -> Self {
            self.git_status_porcelain_non_empty = Some(true);
            self
        }

        fn head_sha(mut self, head_sha: impl Into<String>) -> Self {
            self.head_sha = Some(head_sha.into());
            self
        }

        fn path_content(
            mut self,
            path: impl Into<String>,
            expected_content: impl Into<String>,
        ) -> Self {
            self.path_contents
                .push((path.into(), expected_content.into()));
            self
        }

        fn path_exists(mut self, path: impl Into<String>, exists: bool) -> Self {
            self.path_exists.push((path.into(), exists));
            self
        }

        fn rebase_apply_exists(mut self, rebase_apply_exists: bool) -> Self {
            self.rebase_apply_exists = Some(rebase_apply_exists);
            self
        }

        fn rebase_merge_exists(mut self, rebase_merge_exists: bool) -> Self {
            self.rebase_merge_exists = Some(rebase_merge_exists);
            self
        }

        fn requires_rebase(mut self, requires_rebase: bool) -> Self {
            self.requires_rebase = Some(requires_rebase);
            self
        }

        fn stderr(mut self, stderr: impl Into<String>) -> Self {
            let expected_stderr = stderr.into();
            assert!(
                !expected_stderr.is_empty(),
                "stderr called with default empty value; omit it from the expectation"
            );
            self.stderr = Some(StreamExpectation::new_exact(expected_stderr));
            self
        }

        fn stderr_suffix(mut self, stderr_suffix: impl Into<String>) -> Self {
            let expected_stderr = stderr_suffix.into();
            assert!(
                !expected_stderr.is_empty(),
                "stderr_suffix called with default empty value; omit it from the expectation"
            );
            self.stderr = Some(StreamExpectation::new_suffix(expected_stderr));
            self
        }

        fn stdout(mut self, stdout: impl Into<String>) -> Self {
            let expected_stdout = stdout.into();
            assert!(
                !expected_stdout.is_empty(),
                "stdout called with default empty value; omit it from the expectation"
            );
            self.stdout = Some(StreamExpectation::new_exact(expected_stdout));
            self
        }

        fn stdout_suffix(mut self, stdout_suffix: impl Into<String>) -> Self {
            let expected_stdout = stdout_suffix.into();
            assert!(
                !expected_stdout.is_empty(),
                "stdout_suffix called with default empty value; omit it from the expectation"
            );
            self.stdout = Some(StreamExpectation::new_suffix(expected_stdout));
            self
        }
    }

    impl CommandExpectation for GitFactorExpectation {
        fn expected_code(&self) -> i32 {
            self.code
        }

        fn expected_stderr(&self) -> &str {
            self.stderr
                .as_ref()
                .and_then(StreamExpectation::exact)
                .unwrap_or_default()
        }

        fn expected_stdout(&self) -> &str {
            self.stdout
                .as_ref()
                .and_then(StreamExpectation::exact)
                .unwrap_or_default()
        }
    }

    fn shell_quote(arg: &str) -> String {
        let mut out = String::with_capacity(arg.len());
        out.push('\'');
        for ch in arg.chars() {
            if ch == '\'' {
                out.push_str("'\\''");
            } else {
                out.push(ch);
            }
        }
        out.push('\'');
        out
    }

    fn assert_stream_expectation(
        stream_name: &str,
        actual: &str,
        expected: Option<&StreamExpectation>,
    ) {
        if let Some(stream) = expected {
            stream.assert_matches(stream_name, actual);
        }
    }

    fn assert_git_factor_postconditions(repo_path: &Path, expected: GitFactorExpectation) {
        let GitFactorExpectation {
            factor_state_exists,
            git_outputs,
            git_outputs_non_empty,
            git_status_porcelain: expected_git_status_porcelain,
            git_status_porcelain_non_empty,
            head_sha,
            path_contents,
            path_exists,
            rebase_apply_exists,
            rebase_merge_exists,
            requires_rebase,
            ..
        } = expected;

        if let Some(expected_exists_value) = factor_state_exists {
            let actual_exists = git_dir(repo_path).join("factor").is_dir();
            assert_eq!(
                actual_exists, expected_exists_value,
                "factor state dir existence mismatch: expected {expected_exists_value}, got {actual_exists}"
            );
        }

        if let Some(expected_status_text) = expected_git_status_porcelain.as_deref() {
            let actual = git_status_porcelain(repo_path);
            assert_eq!(
                actual, expected_status_text,
                "git status --porcelain mismatch:\nexpected:\n{expected_status_text}\nactual:\n{actual}"
            );
        }

        if git_status_porcelain_non_empty == Some(true) {
            let actual = git_status_porcelain(repo_path);
            assert!(
                !actual.is_empty(),
                "git status --porcelain expected non-empty output, got empty"
            );
        }

        if let Some(expected_head_sha_text) = head_sha.as_deref() {
            let actual = git(repo_path, &["rev-parse", "HEAD"]);
            assert_eq!(
                actual, expected_head_sha_text,
                "HEAD mismatch: expected {expected_head_sha_text}, got {actual}"
            );
        }

        for entry in &git_outputs {
            let args = &entry.0;
            let expected_output_text = &entry.1;
            let args_refs: Vec<&str> = args.iter().map(String::as_str).collect();
            let actual = git(repo_path, &args_refs);
            assert_eq!(
                actual, *expected_output_text,
                "git output mismatch for {args_refs:?}: expected {expected_output_text:?}, got {actual:?}"
            );
        }

        for args in &git_outputs_non_empty {
            let args_refs: Vec<&str> = args.iter().map(String::as_str).collect();
            let actual = git(repo_path, &args_refs);
            assert!(
                !actual.is_empty(),
                "git output expected non-empty for {args_refs:?}, got empty"
            );
        }

        for entry in &path_exists {
            let path = &entry.0;
            let expected_exists = entry.1;
            let actual_exists = repo_path.join(path.as_str()).exists();
            assert_eq!(
                actual_exists, expected_exists,
                "path existence mismatch for {path:?}: expected {expected_exists}, got {actual_exists}"
            );
        }

        for entry in &path_contents {
            let path = &entry.0;
            let expected_content = &entry.1;
            let full_path = repo_path.join(path.as_str());
            let actual_content = fs::read_to_string(&full_path).or_abort();
            assert_eq!(
                actual_content, *expected_content,
                "path content mismatch for {path:?}: expected {expected_content:?}, got {actual_content:?}"
            );
        }

        assert_rebase_dir_exists(repo_path, "rebase-merge", rebase_merge_exists).or_abort();
        assert_rebase_dir_exists(repo_path, "rebase-apply", rebase_apply_exists).or_abort();

        if let Some(expected_requires_rebase) = requires_rebase {
            let requires_rebase_path = git_dir(repo_path).join("factor/requires_rebase");
            let actual = fs::read_to_string(&requires_rebase_path).or_abort();
            let expected_content = if expected_requires_rebase {
                "true\n"
            } else {
                "false\n"
            };
            assert_eq!(
                actual, expected_content,
                "requires_rebase mismatch: expected {expected_content:?}, got {actual:?}"
            );
        }
    }

    fn assert_rebase_dir_exists(
        repo: &Path,
        rebase_dir: &str,
        expected_rebase_dir_exists: Option<bool>,
    ) -> Result<(), String> {
        if let Some(expected_exists_value) = expected_rebase_dir_exists {
            let actual_exists = git_dir(repo).join(rebase_dir).is_dir();
            if actual_exists != expected_exists_value {
                return Err(format!(
                    "{rebase_dir} existence mismatch: expected {expected_exists_value}, got {actual_exists}"
                ));
            }
        }
        Ok(())
    }

    fn expected_single_commit_start_stdout(short_sha: &str, message: &str) -> String {
        format!(
            "\
FACTOR: Split session started for {short_sha}.
ORIGINAL MESSAGE: {message}
UNSTAGED:
  file.txt | 1 +
   1 file changed, 1 insertion(+)

NEXT: Stage changes for the first atomic commit, then run:
  git factor --continue --message \"type: description\"

Run git factor --help for the full workflow guide.

HINTS:
  - Find the ONE smallest addition nothing depends on
  - Target 15-30 lines (50 max)
  - Message: single concrete action, no \"and\"/\"or\"
  - Verify: git log --oneline | wc -l
  - NEVER use git commit. ONLY use git factor --continue.
  REMAINING:  1 file changed, 1 insertion(+)
  RECOVERY: git factor --abort
"
        )
    }

    fn expected_multi_commit_start_suffix(first_short_sha: &str) -> String {
        format!(
            "\
FACTOR: Split session started for 2 commits (first: {first_short_sha}).
ORIGINAL MESSAGE: feat: a
UNSTAGED:
  base.txt | 1 +
   1 file changed, 1 insertion(+)

NEXT: Stage changes for the first atomic commit, then run:
  git factor --continue --message \"type: description\"

Run git factor --help for the full workflow guide.

HINTS:
  - Find the ONE smallest addition nothing depends on
  - Target 15-30 lines (50 max)
  - Message: single concrete action, no \"and\"/\"or\"
  - Verify: git log --oneline | wc -l
  - NEVER use git commit. ONLY use git factor --continue.
  REMAINING:  1 file changed, 1 insertion(+)
  RECOVERY: git factor --abort
"
        )
    }

    fn expected_now_splitting_suffix_with_remaining(
        short_sha: &str,
        original_message: &str,
        unstaged_summary: &str,
    ) -> String {
        format!(
            "\
FACTOR: Previous commit split into 1 commits.
FACTOR: Now splitting {short_sha}.
ORIGINAL MESSAGE: {original_message}
UNSTAGED:
{unstaged_summary}

NEXT: Stage changes for the next commit, then run:
  git factor --continue --message \"type: description\"

HINTS:
  - Find the ONE smallest addition nothing depends on
  - Target 15-30 lines (50 max)
  - Message: single concrete action, no \"and\"/\"or\"
  - Verify: git log --oneline | wc -l
  - NEVER use git commit. ONLY use git factor --continue.
  REMAINING:  1 file changed, 1 insertion(+)
  RECOVERY: git factor --abort
"
        )
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

      --retry
          Discard the current split attempt and restore the remaining pool.
          
          Restores the green baseline commit into the index and working tree, then unstages everything so the session returns to the normal \"remaining changes are unstaged\" state.

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
  6. Discard bad staging: git factor --retry
  7. Repeat steps 4-6 for each atomic commit.
  8. Finish remaining:   git factor --finish

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

  Discard the current split attempt and restore the remaining pool:
    git factor --retry

  Finish with the original commit message:
    git factor --finish

  Abort and restore the repository:
    git factor --abort

  Show active-session status or whether a start is pending:
    git factor --status
"
    }

    fn expected_completion_stdout_suffix(split_count: u32) -> String {
        format!("FACTOR: Complete. Final commit split into {split_count} commits.\n")
    }

    fn execute_expectation(expectation: GitFactorExpectation) {
        let bin_path = expectation.bin_path.clone().unwrap_or_else(git_factor_bin);
        let mut command = Command::new(&bin_path);
        if let Some(repo) = expectation.repo.as_ref() {
            command.current_dir(repo);
        }
        command.args(&expectation.args);
        if let Some(path_env) = expectation.path_env.as_ref() {
            command.env("PATH", path_env);
        }
        command.env_remove("CLAUDECODE");
        for entry in &expectation.envs {
            command.env(&entry.0, &entry.1);
        }
        let output = match command.output() {
            Ok(output) => output,
            Err(err) if err.kind() == ErrorKind::WouldBlock => {
                resume_unwind(Box::new(format!("retryable spawn error: {err}")));
            }
            Err(err) => {
                resume_unwind(Box::new(format!("command spawn error: {err}")));
            }
        };
        let code = output.status.code().unwrap_or(EXIT_FAILURE);
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        assert_eq!(
            code, expectation.code,
            "exit code mismatch: expected {}, got {}\nstdout:\n{}\nstderr:\n{}",
            expectation.code, code, stdout, stderr
        );
        assert_stream_expectation("stdout", &stdout, expectation.stdout.as_ref());
        assert_stream_expectation("stderr", &stderr, expectation.stderr.as_ref());

        let repo = expectation.repo.clone();
        if let Some(repo_path) = repo.as_ref() {
            assert_git_factor_postconditions(repo_path, expectation);
        }
    }

    fn run_git_factor(repo: &Path, args: &[&str], expected: GitFactorExpectation) {
        let mut expectation = expected;
        expectation.args = args.iter().map(OsString::from).collect();
        expectation.repo = Some(repo.to_path_buf());
        execute_expectation(expectation);
    }

    fn run_git_factor_in_dir(dir: &Path, args: &[&str], expected: GitFactorExpectation) {
        let mut expectation = expected;
        expectation.args = args.iter().map(OsString::from).collect();
        expectation.repo = Some(dir.to_path_buf());
        execute_expectation(expectation);
    }

    fn run_git_factor_no_repo(args: &[&str], expected: GitFactorExpectation) {
        let mut expectation = expected;
        expectation.args = args.iter().map(OsString::from).collect();
        execute_expectation(expectation);
    }

    fn run_git_factor_with_prefixed_path(
        repo: &Path,
        args: &[&str],
        expected: GitFactorExpectation,
        path_env: OsString,
    ) {
        let mut expectation = expected;
        expectation.args = args.iter().map(OsString::from).collect();
        expectation.path_env = Some(path_env);
        expectation.repo = Some(repo.to_path_buf());
        execute_expectation(expectation);
    }

    fn run_git_factor_with_prefixed_path_and_env(
        repo: &Path,
        args: &[&str],
        expected: GitFactorExpectation,
        path_env: OsString,
        key: impl Into<OsString>,
        value: impl Into<OsString>,
    ) {
        let mut expectation = expected;
        expectation.args = args.iter().map(OsString::from).collect();
        expectation.path_env = Some(path_env);
        expectation.envs.push((key.into(), value.into()));
        expectation.repo = Some(repo.to_path_buf());
        execute_expectation(expectation);
    }

    fn run_git_factor_with_env(
        repo: &Path,
        args: &[&str],
        expected: GitFactorExpectation,
        key: impl Into<OsString>,
        value: impl Into<OsString>,
    ) {
        let mut expectation = expected;
        expectation.args = args.iter().map(OsString::from).collect();
        expectation.envs.push((key.into(), value.into()));
        expectation.repo = Some(repo.to_path_buf());
        execute_expectation(expectation);
    }

    fn run_git_factor_with_bin(
        repo: &Path,
        bin_path: &Path,
        args: &[&str],
        expected: GitFactorExpectation,
    ) {
        let mut expectation = expected;
        expectation.args = args.iter().map(OsString::from).collect();
        expectation.bin_path = Some(bin_path.to_path_buf());
        expectation.repo = Some(repo.to_path_buf());
        execute_expectation(expectation);
    }

    fn overwrite_session_exec(repo: &Path, exec: &str) {
        let factor_dir = git_dir(repo).join("factor");
        fs::write(factor_dir.join("exec"), format!("{exec}\n")).or_abort();
    }

    #[test]
    fn rejects_commit_ref_when_git_returns_non_hex_40_char_sha() {
        let dir = init_repo();
        let repo = dir.path();
        commit_file(repo, "file.txt", "one\n", "chore: base");

        // Intercept `git rev-parse --verify <ref>` and return a non-hex 40-char SHA to
        // ensure we exercise CommitSha::new's non-hex branch in a non-test build.
        let (wrap_dir, wrap_bin) = make_git_wrapper_named(
            "git",
            r#"if [ "${1-}" = "rev-parse" ] && [ "${2-}" = "--verify" ] && [ "${3-}" = "definitely-not-a-commit" ]; then
  printf "%040s\n" "g" | tr ' ' 'g'
  exit 0
fi
"#,
        );

        let original_path = env::var_os("PATH").or_abort();
        let prefixed_path = {
            let mut joined = OsString::new();
            joined.push(wrap_bin.as_os_str());
            joined.push(OsStr::new(":"));
            joined.push(original_path);
            joined
        };

        // Wrapper tempdir must live through command execution.
        let _keep_alive = wrap_dir;
        run_git_factor_with_prefixed_path(
            repo,
            &["--exec", "true", "definitely-not-a-commit"],
            GitFactorExpectation::default()
                .code(EXIT_DATAERR)
                .stderr("invalid commit: 000000000000000000000000000000000000000g\n"),
            prefixed_path,
        );
    }

    #[test]
    fn rejects_commit_ref_when_git_returns_non_40_char_sha() {
        let dir = init_repo();
        let repo = dir.path();
        commit_file(repo, "file.txt", "one\n", "chore: base");

        // Intercept `git rev-parse --verify <ref>` and return a short SHA to
        // exercise CommitSha::new's length-validation branch.
        let (wrap_dir, wrap_bin) = make_git_wrapper_named(
            "git",
            r#"if [ "${1-}" = "rev-parse" ] && [ "${2-}" = "--verify" ] && [ "${3-}" = "definitely-not-a-commit" ]; then
  echo "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
  exit 0
fi
"#,
        );

        let original_path = env::var_os("PATH").or_abort();
        let prefixed_path = {
            let mut joined = OsString::new();
            joined.push(wrap_bin.as_os_str());
            joined.push(OsStr::new(":"));
            joined.push(original_path);
            joined
        };

        // Wrapper tempdir must live through command execution.
        let _keep_alive = wrap_dir;
        run_git_factor_with_prefixed_path(
            repo,
            &["--exec", "true", "definitely-not-a-commit"],
            GitFactorExpectation::default()
                .code(EXIT_DATAERR)
                .stderr("invalid commit: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n"),
            prefixed_path,
        );
    }

    #[test]
    fn rejects_default_head_when_repository_has_no_commits() {
        let dir = init_repo();
        let repo = dir.path();

        run_git_factor(
            repo,
            &["--exec", "true"],
            GitFactorExpectation::default()
                .code(EXIT_DATAERR)
                .stderr("invalid commit: HEAD\n"),
        );
    }

    #[test]
    fn rejects_start_when_git_returns_non_hex_40_char_tree_hash() {
        let dir = init_repo();
        let repo = dir.path();
        commit_file(repo, "file.txt", "one\n", "chore: base");

        // Intercept `git rev-parse HEAD^{tree}` and return a non-hex 40-char
        // value to exercise tree-hash validation in a non-test build.
        let (wrap_dir, wrap_bin) = make_git_wrapper_named(
            "git",
            r#"if [ "${1-}" = "rev-parse" ] && [ "${2-}" = "HEAD^{tree}" ]; then
  printf "%040s\n" "g" | tr ' ' 'g'
  exit 0
fi
"#,
        );

        let original_path = env::var_os("PATH").or_abort();
        let prefixed_path = {
            let mut joined = OsString::new();
            joined.push(wrap_bin.as_os_str());
            joined.push(OsStr::new(":"));
            joined.push(original_path);
            joined
        };

        let _keep_alive = wrap_dir;
        run_git_factor_with_prefixed_path(
            repo,
            &["--exec", "true", "HEAD"],
            GitFactorExpectation::default().code(EXIT_SOFTWARE).stderr(
                "git command failed: invalid tree hash: '000000000000000000000000000000000000000g'\n",
            ),
            prefixed_path,
        );
    }

    #[test]
    fn cli_no_args_prints_help() {
        run_git_factor_no_repo(
            &[],
            GitFactorExpectation::default().stdout(expected_help_stdout()),
        );
    }

    #[test]
    fn cli_help_flag_prints_help_to_stdout_and_exits_ok() {
        run_git_factor_no_repo(
            &["--help"],
            GitFactorExpectation::default().stdout(expected_help_stdout()),
        );
    }

    #[test]
    fn start_requires_exec_command() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");

        run_git_factor(
            repo,
            &["HEAD"],
            GitFactorExpectation::default()
                .code(EXIT_USAGE)
                .stderr("--exec <COMMAND> is required when starting a factor session\n"),
        );
    }

    #[test]
    fn start_requires_exec_command_when_only_message_is_provided() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");

        run_git_factor(
            repo,
            &["--message", "test: msg"],
            GitFactorExpectation::default()
                .code(EXIT_USAGE)
                .stderr("--exec <COMMAND> is required when starting a factor session\n"),
        );
    }

    #[test]
    fn start_head_runs_exec_gate_before_session_starts() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");
        commit_file(repo, "file.txt", "one\ntwo\n", "feat: change");
        let head_before = git(repo, &["rev-parse", "HEAD"]);

        run_git_factor(
            repo,
            &["--exec", "false", "HEAD"],
            GitFactorExpectation::default()
                .code(EXIT_TEMPFAIL)
                .stderr("exec gate failed: false (exit code 1)\n")
                .head_sha(head_before)
                .git_status_porcelain("")
                .factor_state_exists(false),
        );
    }

    #[test]
    fn start_defaults_to_head_when_commit_is_omitted() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");
        commit_file(repo, "file.txt", "one\ntwo\n", "feat: change");
        let head_sha = git(repo, &["rev-parse", "HEAD"]);
        let head_short_sha = git(repo, &["rev-parse", "--short", "HEAD"]);

        run_git_factor(
            repo,
            &["--exec", "true"],
            GitFactorExpectation::default()
                .rebase_apply_exists(false)
                .rebase_merge_exists(false)
                .requires_rebase(false)
                .path_content(".git/factor/started_rebase", "false\n")
                .path_content(".git/factor/start_head", format!("{head_sha}\n"))
                .stdout(expected_single_commit_start_stdout(
                    head_short_sha.as_str(),
                    "feat: change",
                )),
        );

        git(repo, &["add", "--all"]);
        run_git_factor(
            repo,
            &["--continue", "--message", "test: split"],
            GitFactorExpectation::default().stdout_suffix(expected_completion_stdout_suffix(1)),
        );
    }

    #[test]
    fn start_treats_explicit_head_sha_as_head_mode() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");
        commit_file(repo, "file.txt", "one\ntwo\n", "feat: change");
        let head_sha = git(repo, &["rev-parse", "HEAD"]);
        let head_short_sha = git(repo, &["rev-parse", "--short", "HEAD"]);

        run_git_factor(
            repo,
            &["--exec", "true", head_sha.as_str()],
            GitFactorExpectation::default()
                .requires_rebase(false)
                .stdout(expected_single_commit_start_stdout(
                    head_short_sha.as_str(),
                    "feat: change",
                )),
        );
    }

    #[test]
    fn start_runs_with_absolute_git_dir() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");
        commit_file(repo, "file.txt", "one\ntwo\n", "feat: change");

        let git_dir_abs = git_dir(repo);
        run_git_factor_with_env(
            repo,
            &["--exec", "true", "HEAD"],
            GitFactorExpectation::default(),
            "GIT_DIR",
            git_dir_abs.into_os_string(),
        );
    }

    #[test]
    fn rejects_invalid_exec_syntax() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "base\n", "chore: base");

        run_git_factor(
            repo,
            &["--exec", "true &&", "HEAD"],
            GitFactorExpectation::default()
                .code(EXIT_DATAERR)
                .stderr("invalid exec syntax: true &&\n"),
        );
    }

    #[test]
    fn rejects_ranges_when_rev_list_returns_no_commits() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");
        commit_file(repo, "file.txt", "one\ntwo\n", "feat: change");

        // Intercept rev-list and return success with no output so the commit set
        // is empty before topological sorting.
        let (wrap_dir, wrap_bin) = make_git_wrapper_named(
            "git",
            r#"if [ "${1-}" = "rev-list" ] && [ "${2-}" = "HEAD~1..HEAD" ]; then
  exit 0
fi
"#,
        );

        let original_path = env::var_os("PATH").or_abort();
        let prefixed_path = {
            let mut joined = OsString::new();
            joined.push(wrap_bin.as_os_str());
            joined.push(OsStr::new(":"));
            joined.push(original_path);
            joined
        };

        let _keep_alive = wrap_dir;
        run_git_factor_with_prefixed_path(
            repo,
            &["--exec", "true", "HEAD~1..HEAD"],
            GitFactorExpectation::default()
                .code(EXIT_SOFTWARE)
                .stderr("git command failed: no commits resolved from the given refs\n"),
            prefixed_path,
        );
    }

    #[test]
    fn rejects_empty_commit_range_when_rev_list_returns_no_commits() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");
        commit_file(repo, "file.txt", "one\ntwo\n", "feat: change");

        run_git_factor(
            repo,
            &["--exec", "true", "HEAD..HEAD"],
            GitFactorExpectation::default()
                .code(EXIT_SOFTWARE)
                .stderr("git command failed: no commits resolved from the given refs\n"),
        );
    }

    #[test]
    fn rejects_abort_when_combined_with_other_options() {
        let dir = init_repo();
        let repo = dir.path();

        run_git_factor(
            repo,
            &["--abort", "--finish"],
            GitFactorExpectation::default()
                .code(EXIT_USAGE)
                .stderr("--abort cannot be combined with other options\n"),
        );
    }

    #[test]
    fn rejects_abort_when_combined_with_continue() {
        let dir = init_repo();
        let repo = dir.path();

        run_git_factor(
            repo,
            &["--abort", "--continue", "--message", "test: msg"],
            GitFactorExpectation::default()
                .code(EXIT_USAGE)
                .stderr("--abort cannot be combined with other options\n"),
        );
    }

    #[test]
    fn rejects_abort_when_combined_with_exec() {
        let dir = init_repo();
        let repo = dir.path();

        run_git_factor(
            repo,
            &["--abort", "--exec", "true"],
            GitFactorExpectation::default()
                .code(EXIT_USAGE)
                .stderr("--abort cannot be combined with other options\n"),
        );
    }

    #[test]
    fn rejects_abort_when_combined_with_commit() {
        let dir = init_repo();
        let repo = dir.path();
        commit_file(repo, "file.txt", "one\n", "chore: base");

        run_git_factor(
            repo,
            &["--abort", "HEAD"],
            GitFactorExpectation::default()
                .code(EXIT_USAGE)
                .stderr("--abort cannot be combined with other options\n"),
        );
    }

    #[test]
    fn rejects_abort_when_combined_with_status() {
        let dir = init_repo();
        let repo = dir.path();

        run_git_factor(
            repo,
            &["--abort", "--status"],
            GitFactorExpectation::default()
                .code(EXIT_USAGE)
                .stderr("--abort cannot be combined with other options\n"),
        );
    }

    #[test]
    fn rejects_finish_when_combined_with_exec_or_commit() {
        let dir = init_repo();
        let repo = dir.path();

        run_git_factor(
            repo,
            &["--finish", "--exec", "true", "HEAD"],
            GitFactorExpectation::default()
                .code(EXIT_USAGE)
                .stderr("--finish cannot be combined with --continue, --exec, or COMMIT\n"),
        );
    }

    #[test]
    fn rejects_finish_when_combined_with_continue() {
        let dir = init_repo();
        let repo = dir.path();

        run_git_factor(
            repo,
            &["--finish", "--continue", "--message", "test: msg"],
            GitFactorExpectation::default()
                .code(EXIT_USAGE)
                .stderr("--finish cannot be combined with --continue, --exec, or COMMIT\n"),
        );
    }

    #[test]
    fn rejects_finish_when_combined_with_commit() {
        let dir = init_repo();
        let repo = dir.path();
        commit_file(repo, "file.txt", "one\n", "chore: base");

        run_git_factor(
            repo,
            &["--finish", "HEAD"],
            GitFactorExpectation::default()
                .code(EXIT_USAGE)
                .stderr("--finish cannot be combined with --continue, --exec, or COMMIT\n"),
        );
    }

    #[test]
    fn rejects_continue_when_combined_with_exec_or_commit() {
        let dir = init_repo();
        let repo = dir.path();

        run_git_factor(
            repo,
            &["--continue", "--exec", "true", "--message", "test: msg"],
            GitFactorExpectation::default()
                .code(EXIT_USAGE)
                .stderr("--continue cannot be combined with --exec or COMMIT\n"),
        );
    }

    #[test]
    fn rejects_continue_when_combined_with_commit() {
        let dir = init_repo();
        let repo = dir.path();
        commit_file(repo, "file.txt", "one\n", "chore: base");

        run_git_factor(
            repo,
            &["--continue", "--message", "test: msg", "HEAD"],
            GitFactorExpectation::default()
                .code(EXIT_USAGE)
                .stderr("--continue cannot be combined with --exec or COMMIT\n"),
        );
    }

    #[test]
    fn rejects_start_when_message_is_provided() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");

        run_git_factor(
            repo,
            &["--exec", "true", "--message", "test: msg", "HEAD"],
            GitFactorExpectation::default()
                .code(EXIT_USAGE)
                .stderr("--message can only be used with --continue or --finish\n"),
        );
    }

    #[test]
    fn rejects_continue_without_message() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");
        commit_file(repo, "file.txt", "one\ntwo\n", "feat: change");
        run_git_factor(
            repo,
            &["--exec", "true", "HEAD~1"],
            GitFactorExpectation::default(),
        );

        run_git_factor(
            repo,
            &["--continue"],
            GitFactorExpectation::default()
                .code(EXIT_USAGE)
                .stderr("--continue requires --message <MSG>\n"),
        );
    }

    #[test]
    fn cli_rejects_running_outside_a_git_repo() {
        let dir = TempDir::new().or_abort();
        run_git_factor_in_dir(
            dir.path(),
            &["--exec", "true", "HEAD"],
            GitFactorExpectation::default()
                .code(EXIT_DATAERR)
                .stderr("not a git repository\n"),
        );
    }

    #[test]
    fn abort_rejects_without_active_session() {
        let dir = init_repo();
        run_git_factor_in_dir(
            dir.path(),
            &["--abort"],
            GitFactorExpectation::default()
                .code(EXIT_USAGE)
                .stderr("no active factor session\n"),
        );
    }

    #[test]
    fn status_reports_no_active_session() {
        let dir = init_repo();
        run_git_factor_in_dir(
            dir.path(),
            &["--status"],
            GitFactorExpectation::default().stdout("FACTOR: No active session.\n"),
        );
    }

    #[test]
    fn status_reports_active_session_details() {
        let dir = init_repo();
        let repo = dir.path();
        commit_file(repo, "file.txt", "one\n", "chore: base");
        commit_file(repo, "file.txt", "one\ntwo\n", "feat: change");

        start_session(repo);
        let current_commit = fs::read_to_string(git_dir(repo).join("factor/commits"))
            .or_abort()
            .trim()
            .to_owned();

        run_git_factor(
            repo,
            &["--status"],
            GitFactorExpectation::default().stdout(format!(
                "FACTOR: Active session.\nCURRENT_COMMIT: {current_commit}\nCURRENT_INDEX: 0\nSPLIT_COUNT: 0\nPHASE: splitting\nREQUIRES_REBASE: false\nREBASE_IN_PROGRESS: false\nIS_ROOT: false\n"
            )),
        );
    }

    #[test]
    fn rejects_status_when_combined_with_other_options() {
        let dir = init_repo();
        let repo = dir.path();
        run_git_factor(
            repo,
            &["--status", "--exec", "true"],
            GitFactorExpectation::default()
                .code(EXIT_USAGE)
                .stderr("--status cannot be combined with other options\n"),
        );
    }

    #[test]
    fn rejects_status_when_combined_with_continue() {
        let dir = init_repo();
        let repo = dir.path();

        run_git_factor(
            repo,
            &["--status", "--continue", "--message", "test: msg"],
            GitFactorExpectation::default()
                .code(EXIT_USAGE)
                .stderr("--status cannot be combined with other options\n"),
        );
    }

    #[test]
    fn rejects_status_when_combined_with_finish() {
        let dir = init_repo();
        let repo = dir.path();

        run_git_factor(
            repo,
            &["--status", "--finish"],
            GitFactorExpectation::default()
                .code(EXIT_USAGE)
                .stderr("--status cannot be combined with other options\n"),
        );
    }

    #[test]
    fn rejects_status_when_combined_with_commit() {
        let dir = init_repo();
        let repo = dir.path();

        run_git_factor(
            repo,
            &["--status", "HEAD"],
            GitFactorExpectation::default()
                .code(EXIT_USAGE)
                .stderr("--status cannot be combined with other options\n"),
        );
    }

    #[test]
    fn rejects_status_when_combined_with_message() {
        let dir = init_repo();
        let repo = dir.path();

        run_git_factor(
            repo,
            &["--status", "--message", "test: message"],
            GitFactorExpectation::default()
                .code(EXIT_USAGE)
                .stderr("--status cannot be combined with other options\n"),
        );
    }

    #[test]
    fn abort_succeeds_when_session_dir_exists_but_no_rebase_is_active() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");

        start_session(repo);

        run_git_factor(
            repo,
            &["--abort"],
            GitFactorExpectation::default()
                .stdout("FACTOR: Session aborted for current commit step.\n"),
        );
    }

    #[test]
    fn continue_rejects_without_active_session() {
        let dir = init_repo();
        run_git_factor_in_dir(
            dir.path(),
            &["--continue", "--message", "test: slice"],
            GitFactorExpectation::default()
                .code(EXIT_USAGE)
                .stderr("no active factor session\n"),
        );
    }

    #[test]
    fn finish_rejects_without_active_session() {
        let dir = init_repo();
        let repo = dir.path();

        run_git_factor(
            repo,
            &["--finish", "--message", "test: msg"],
            GitFactorExpectation::default()
                .code(EXIT_USAGE)
                .stderr("no active factor session\n"),
        );
    }

    #[test]
    fn cli_invalid_flag_triggers_clap_error_path() {
        run_git_factor_no_repo(
            &["--definitely-not-a-real-flag"],
            GitFactorExpectation::default().code(EXIT_USAGE),
        );
    }

    #[test]
    fn rejects_symmetric_diff_ranges() {
        let dir = init_repo();
        let repo = dir.path();
        commit_file(repo, "file.txt", "one\n", "chore: base");
        commit_file(repo, "file.txt", "one\ntwo\n", "feat: change");

        run_git_factor(
            repo,
            &["--exec", "true", "HEAD~1...HEAD"],
            GitFactorExpectation::default().code(EXIT_DATAERR).stderr(
                "invalid commit: HEAD~1...HEAD (symmetric diff '...' is not supported, use '..')\n",
            ),
        );
    }

    #[test]
    fn rejects_invalid_commit_ref() {
        let dir = init_repo();
        let repo = dir.path();
        commit_file(repo, "file.txt", "one\n", "chore: base");

        run_git_factor(
            repo,
            &["--exec", "true", "definitely-not-a-commit"],
            GitFactorExpectation::default()
                .code(EXIT_DATAERR)
                .stderr("invalid commit: definitely-not-a-commit\n"),
        );
    }

    #[test]
    fn rejects_merge_commits() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");

        // Create a merge commit.
        git(repo, &["checkout", "-b", "left"]);
        commit_file(repo, "left.txt", "left\n", "feat: left");
        git(repo, &["checkout", "-b", "right", "HEAD~1"]);
        commit_file(repo, "right.txt", "right\n", "feat: right");
        git(repo, &["checkout", "left"]);
        git(repo, &["merge", "--no-ff", "right", "--no-edit"]);

        let merge_sha = git(repo, &["rev-parse", "HEAD"]);

        run_git_factor(
            repo,
            &["--exec", "true", merge_sha.as_str()],
            GitFactorExpectation::default()
                .code(EXIT_DATAERR)
                .stderr(format!(
                    "commit {merge_sha} is a merge commit and cannot be split\n"
                )),
        );
    }

    #[test]
    fn rejects_commits_that_are_not_ancestors_of_head() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");
        let main_branch = git(repo, &["branch", "--show-current"]);
        git(repo, &["checkout", "-b", "other"]);
        commit_file(repo, "other.txt", "other\n", "feat: other");
        let other_sha = git(repo, &["rev-parse", "HEAD"]);
        git(repo, &["checkout", main_branch.as_str()]);
        commit_file(repo, "file.txt", "one\ntwo\n", "feat: on master");

        run_git_factor(
            repo,
            &["--exec", "true", other_sha.as_str()],
            GitFactorExpectation::default()
                .code(EXIT_DATAERR)
                .stderr(format!("commit {other_sha} is not an ancestor of HEAD\n")),
        );
    }

    #[test]
    fn start_accepts_root_commit_ref_in_non_head_mode() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "feat: root");
        commit_file(repo, "file.txt", "one\ntwo\n", "feat: head");
        let start_head = git(repo, &["rev-parse", "HEAD"]);

        let root_sha = git(repo, &["rev-list", "--max-parents=0", "HEAD"]);

        run_git_factor(
            repo,
            &["--exec", "true", root_sha.as_str()],
            GitFactorExpectation::default()
                .factor_state_exists(true)
                .requires_rebase(true)
                .path_content(".git/factor/started_rebase", "true\n")
                .path_content(".git/factor/start_head", format!("{start_head}\n"))
                .rebase_merge_exists(true),
        );
    }

    #[test]
    fn start_cleans_state_when_git_rebase_fails() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");
        commit_file(repo, "file.txt", "one\ntwo\n", "feat: change");

        let (wrap_dir, wrap_bin) = make_git_wrapper_named(
            "git",
            r#"if [ "${1-}" = "rebase" ]; then
  exit 1
fi
"#,
        );
        let _keep_alive = wrap_dir;

        let original_path = env::var_os("PATH").or_abort();
        let prefixed_path = {
            let mut joined = OsString::new();
            joined.push(wrap_bin.as_os_str());
            joined.push(OsStr::new(":"));
            joined.push(original_path);
            joined
        };

        run_git_factor_with_prefixed_path(
            repo,
            &["--exec", "true", "HEAD~1"],
            GitFactorExpectation::default()
                .code(EXIT_SOFTWARE)
                .stderr("git command failed: git rebase failed (exit 1)\n")
                .factor_state_exists(false),
            prefixed_path,
        );
    }

    #[test]
    fn start_reports_error_when_commit_message_lookup_fails() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");
        commit_file(repo, "file.txt", "one\ntwo\n", "feat: change");

        let (wrap_dir, wrap_bin) = make_git_wrapper_named(
            "git",
            r#"if [ "${1-}" = "show" ] && [ "${2-}" = "--format=%B" ] && [ "${3-}" = "--no-patch" ]; then
  echo "mock show failure" >&2
  exit 1
fi
"#,
        );
        let _keep_alive = wrap_dir;

        let original_path = env::var_os("PATH").or_abort();
        let prefixed_path = {
            let mut joined = OsString::new();
            joined.push(wrap_bin.as_os_str());
            joined.push(OsStr::new(":"));
            joined.push(original_path);
            joined
        };

        run_git_factor_with_prefixed_path(
            repo,
            &["--exec", "true", "HEAD"],
            GitFactorExpectation::default()
                .code(EXIT_SOFTWARE)
                .stderr("git command failed: mock show failure\n"),
            prefixed_path,
        );
    }

    #[test]
    fn start_reports_error_when_short_sha_lookup_for_sequence_editor_fails() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "feat: one");
        commit_file(repo, "file.txt", "one\ntwo\n", "feat: two");
        let head_sha = git(repo, &["rev-parse", "HEAD"]);

        let (wrap_dir, wrap_bin) = make_git_wrapper_named(
            "git",
            format!(
                r#"if [ "${{1:-}}" = "rev-parse" ] && [ "${{2:-}}" = "--short" ] && [ "${{3:-}}" = "{head_sha}" ]; then
  echo "mock short failure" >&2
  exit 1
fi
"#
            )
            .as_str(),
        );
        let _keep_alive = wrap_dir;

        let original_path = env::var_os("PATH").or_abort();
        let prefixed_path = {
            let mut joined = OsString::new();
            joined.push(wrap_bin.as_os_str());
            joined.push(OsStr::new(":"));
            joined.push(original_path);
            joined
        };

        run_git_factor_with_prefixed_path(
            repo,
            &["--exec", "true", "HEAD~1..HEAD"],
            GitFactorExpectation::default()
                .code(EXIT_SOFTWARE)
                .stderr("git command failed: mock short failure\n"),
            prefixed_path,
        );
    }

    #[test]
    fn start_reports_error_when_hint_diff_stat_fails() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");
        commit_file(repo, "file.txt", "one\ntwo\n", "feat: change");

        let (wrap_dir, wrap_bin) = make_git_wrapper_named(
            "git",
            r#"if [ "${1-}" = "diff" ] && [ "${2-}" = "--stat" ]; then
  echo "mock diff stat failure" >&2
  exit 1
fi
"#,
        );
        let _keep_alive = wrap_dir;

        let original_path = env::var_os("PATH").or_abort();
        let prefixed_path = {
            let mut joined = OsString::new();
            joined.push(wrap_bin.as_os_str());
            joined.push(OsStr::new(":"));
            joined.push(original_path);
            joined
        };

        run_git_factor_with_prefixed_path(
            repo,
            &["--exec", "true", "HEAD"],
            GitFactorExpectation::default()
                .code(EXIT_SOFTWARE)
                .stderr("git command failed: mock diff stat failure\n"),
            prefixed_path,
        );
    }

    #[test]
    fn start_reports_error_when_hint_show_toplevel_fails() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");
        commit_file(repo, "file.txt", "one\ntwo\n", "feat: change");

        let (wrap_dir, wrap_bin) = make_git_wrapper_named(
            "git",
            r#"if [ "${1-}" = "rev-parse" ] && [ "${2-}" = "--show-toplevel" ]; then
  echo "mock show-toplevel failure" >&2
  exit 1
fi
"#,
        );
        let _keep_alive = wrap_dir;

        let original_path = env::var_os("PATH").or_abort();
        let prefixed_path = {
            let mut joined = OsString::new();
            joined.push(wrap_bin.as_os_str());
            joined.push(OsStr::new(":"));
            joined.push(original_path);
            joined
        };

        run_git_factor_with_prefixed_path(
            repo,
            &["--exec", "true", "HEAD"],
            GitFactorExpectation::default()
                .code(EXIT_SOFTWARE)
                .stderr("git command failed: mock show-toplevel failure\n"),
            prefixed_path,
        );
    }

    #[test]
    fn start_reports_state_write_error_when_git_dir_is_not_a_directory() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");
        commit_file(repo, "file.txt", "one\ntwo\n", "feat: change");

        let (wrap_dir, wrap_bin) = make_git_wrapper_named(
            "git",
            r#"if [ "${1-}" = "rev-parse" ] && [ "${2-}" = "--git-dir" ]; then
  echo "/dev/null"
  exit 0
fi
"#,
        );
        let _keep_alive = wrap_dir;

        let original_path = env::var_os("PATH").or_abort();
        let prefixed_path = {
            let mut joined = OsString::new();
            joined.push(wrap_bin.as_os_str());
            joined.push(OsStr::new(":"));
            joined.push(original_path);
            joined
        };

        run_git_factor_with_prefixed_path(
            repo,
            &["--exec", "true", "HEAD"],
            GitFactorExpectation::default()
                .code(EXIT_SOFTWARE)
                .stderr("failed to write state: Not a directory (os error 20)\n"),
            prefixed_path,
        );
    }

    #[test]
    fn rejects_start_when_session_dir_exists() {
        let dir = init_repo();
        let repo = dir.path();
        commit_file(repo, "file.txt", "one\n", "chore: base");

        let factor_dir = git_dir(repo).join("factor");
        fs::create_dir_all(&factor_dir).or_abort();

        run_git_factor(
            repo,
            &["--exec", "true", "HEAD"],
            GitFactorExpectation::default()
                .code(EXIT_USAGE)
                .stderr("a factor session is already active (use --abort to cancel)\n"),
        );
    }

    #[test]
    fn rejects_start_during_an_existing_rebase() {
        let dir = init_repo();
        let repo = dir.path();
        commit_file(repo, "file.txt", "one\n", "chore: base");

        // Simulate an in-progress rebase (is_mid_rebase checks for these dirs).
        let rebase_merge = git_dir(repo).join("rebase-merge");
        fs::create_dir_all(&rebase_merge).or_abort();

        run_git_factor(
            repo,
            &["--exec", "true", "HEAD"],
            GitFactorExpectation::default()
                .code(EXIT_USAGE)
                .stderr("a rebase is already in progress\n"),
        );
    }

    #[test]
    fn abort_resets_repo_to_pre_start_head() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");
        commit_file(repo, "file.txt", "one\ntwo\n", "feat: change");
        commit_file(repo, "file.txt", "one\ntwo\nthree\n", "feat: latest");
        let start_head = git(repo, &["rev-parse", "HEAD"]);

        run_git_factor(
            repo,
            &["--exec", "true", "HEAD~1"],
            GitFactorExpectation::default().factor_state_exists(true),
        );

        run_git_factor(
            repo,
            &["--abort"],
            GitFactorExpectation::default()
                .head_sha(start_head)
                .git_status_porcelain("")
                .factor_state_exists(false),
        );
    }

    #[test]
    fn continue_completes_single_commit_split_and_preserves_tree() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");
        commit_file(repo, "file.txt", "one\ntwo\n", "feat: original");

        let expected_tree = git(repo, &["rev-parse", "HEAD^{tree}"]);

        start_session(repo);

        // Stage all remaining changes and commit the slice.
        git(repo, &["add", "--all"]);
        run_git_factor(
            repo,
            &["--continue", "--message", "test: split"],
            GitFactorExpectation::default()
                .git_output(&["rev-parse", "HEAD^{tree}"], expected_tree)
                .git_status_porcelain("")
                .factor_state_exists(false)
                .path_exists(".git/factor", false),
        );
    }

    #[test]
    fn continue_requires_staged_changes_and_does_not_wipe_pool() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");
        commit_file(repo, "file.txt", "one\ntwo\n", "feat: change");

        start_session(repo);

        let diff_before = git(repo, &["diff", "--stat"]);

        run_git_factor(
            repo,
            &["--continue", "--message", "test: no staging"],
            GitFactorExpectation::default()
                .code(EXIT_USAGE)
                .stderr(
                    "no staged changes to commit\nNEXT: stage exactly one atomic change, then rerun:\n  git factor --continue --message \"type: description\"\n",
                )
                .git_output(&["diff", "--stat"], diff_before),
        );
    }

    #[test]
    fn start_persists_factor_state_files_in_git_dir() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "base\n", "chore: base");
        commit_file(repo, "file.txt", "base\nchange\n", "feat: change");

        let original_commit = git(repo, &["rev-parse", "HEAD"]);
        let expected_tree = git(repo, &["rev-parse", "HEAD^{tree}"]);

        run_git_factor(
            repo,
            &["--exec", "true", "HEAD"],
            GitFactorExpectation::default()
                .factor_state_exists(true)
                .path_content(".git/factor/commits", format!("{original_commit}\n"))
                .path_content(".git/factor/current_index", "0\n")
                .path_content(".git/factor/exec", "true\n")
                .path_content(".git/factor/split_count", "0\n")
                .path_content(".git/factor/requires_rebase", "false\n")
                .path_content(".git/factor/is_root", "false\n")
                .path_content(".git/factor/expected_tree", format!("{expected_tree}\n")),
        );
    }

    #[test]
    fn proptest_start_with_multiple_exec_flags_persists_joined_exec_command() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "base\n", "chore: base");
        commit_file(repo, "file.txt", "base\nchange\n", "feat: change");

        run_git_factor(
            repo,
            &["--exec", "true", "--exec", "true", "HEAD"],
            GitFactorExpectation::default()
                .factor_state_exists(true)
                .path_content(".git/factor/exec", "true && true\n"),
        );
    }

    #[test]
    fn proptest_non_empty_string_public_api_is_instantiated() {
        use core::borrow::Borrow as _;

        let mut value = NonEmptyString::new("alpha".to_owned()).or_abort();
        value.push_str("-beta");
        assert_eq!(value.as_str(), "alpha-beta");
        assert_eq!(value.as_ref(), "alpha-beta");
        let borrowed: &str = value.borrow();
        assert_eq!(borrowed, "alpha-beta");
        let deref_value: &str = &value;
        assert_eq!(deref_value, "alpha-beta");
        assert_eq!(value.to_string(), "alpha-beta");

        let parsed = "gamma".parse::<NonEmptyString>().or_abort();
        assert_eq!(parsed.as_str(), "gamma");
        let from_str = NonEmptyString::try_from("delta").or_abort();
        assert_eq!(from_str.as_str(), "delta");
        let from_string = NonEmptyString::try_from("epsilon".to_owned()).or_abort();
        assert_eq!(from_string.as_str(), "epsilon");
    }

    #[test]
    fn start_writes_trace_log_with_process_and_state_fields() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "base\n", "chore: base");
        commit_file(repo, "file.txt", "base\nchange\n", "feat: change");

        let trace_path = git_dir(repo).join("trace/git-factor.jsonl");
        run_git_factor_with_env(
            repo,
            &["--exec", "true", "HEAD"],
            GitFactorExpectation::default().factor_state_exists(true),
            "GIT_FACTOR_TRACE_LOG",
            trace_path.as_os_str(),
        );

        let trace_content = fs::read_to_string(&trace_path).or_abort();

        assert!(
            trace_content.contains("\"event\":\"factor_cmd_start\""),
            "trace log should include factor start note event\n{trace_content}"
        );
        assert!(
            trace_content.contains("\"event\":\"process\""),
            "trace log should include process events\n{trace_content}"
        );
        assert!(
            trace_content.contains("\"after_factor_split_count\":\"0\""),
            "trace log should snapshot factor split count side effect\n{trace_content}"
        );
        assert!(
            trace_content.contains("\"after_factor_current_index\":\"0\""),
            "trace log should snapshot factor current index side effect\n{trace_content}"
        );
        assert!(
            trace_content.contains("\"after_factor_expected_tree\":\""),
            "trace log should snapshot expected tree side effect\n{trace_content}"
        );
    }

    #[test]
    fn status_with_empty_trace_log_env_disables_trace_logging() {
        let dir = init_repo();
        let repo = dir.path();
        commit_file(repo, "file.txt", "base\n", "chore: base");

        run_git_factor_with_env(
            repo,
            &["--status"],
            GitFactorExpectation::default().stdout("FACTOR: No active session.\n"),
            "GIT_FACTOR_TRACE_LOG",
            "",
        );

        assert!(
            !repo.join("trace").exists(),
            "trace directory should not be created when trace env is empty"
        );
    }

    #[test]
    fn trace_log_truncates_large_stderr_and_escapes_control_chars() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "base\n", "chore: base");

        let (wrap_dir, wrap_bin) = make_git_wrapper_named(
            "git",
            r#"if [ "${1-}" = "rev-parse" ] && [ "${2-}" = "--short" ]; then
  printf 'fatal: short lookup \\ quote " tab \t cr \r ctrl \001' >&2
  head -c 8400 < /dev/zero | tr '\0' 'x' >&2
  printf 'TAILMARK\n' >&2
  exit 42
fi
"#,
        );
        let _keep_alive = wrap_dir;

        let trace_path = git_dir(repo).join("trace/trimmed.jsonl");
        run_git_factor_with_prefixed_path_and_env(
            repo,
            &["--exec", "true", "HEAD"],
            GitFactorExpectation::default().code(EXIT_SOFTWARE),
            format!("{}:{}", wrap_bin.display(), env::var("PATH").or_abort()).into(),
            "GIT_FACTOR_TRACE_LOG",
            trace_path.as_os_str().to_os_string(),
        );

        let trace_content = fs::read_to_string(&trace_path).or_abort();
        assert!(
            trace_content.contains("\"event\":\"process\""),
            "trace log should include process events\n{trace_content}"
        );
        assert!(
            trace_content.contains("\\u0001"),
            "trace log should escape control characters\n{trace_content}"
        );
        assert!(
            trace_content.contains("\\\\"),
            "trace log should escape backslashes\n{trace_content}"
        );
        assert!(
            trace_content.contains("\\\""),
            "trace log should escape quotes\n{trace_content}"
        );
        assert!(
            trace_content.contains("\\t"),
            "trace log should escape tabs\n{trace_content}"
        );
        assert!(
            trace_content.contains("\\r"),
            "trace log should escape carriage returns\n{trace_content}"
        );
        assert!(
            !trace_content.contains("TAILMARK"),
            "trace log should truncate long stderr payloads\n{trace_content}"
        );
    }

    #[test]
    fn status_trace_records_rebase_merge_snapshot_fields() {
        let dir = init_repo();
        let repo = dir.path();
        commit_file(repo, "file.txt", "base\n", "chore: base");

        let rebase_merge = git_dir(repo).join("rebase-merge");
        fs::create_dir_all(&rebase_merge).or_abort();
        fs::write(rebase_merge.join("msgnum"), "2\n").or_abort();
        fs::write(rebase_merge.join("end"), "5\n").or_abort();
        fs::write(
            rebase_merge.join("git-rebase-todo"),
            "# comment\n\npick deadbeef step\n",
        )
        .or_abort();
        fs::write(rebase_merge.join("done"), "pick a\n\npick done\n").or_abort();

        let trace_path = repo.join("trace/rebase-merge.jsonl");
        run_git_factor_with_env(
            repo,
            &["--status"],
            GitFactorExpectation::default().stdout("FACTOR: No active session.\n"),
            "GIT_FACTOR_TRACE_LOG",
            trace_path.as_os_str().to_os_string(),
        );

        let trace_content = fs::read_to_string(&trace_path).or_abort();
        assert!(
            trace_content.contains("\"state_rebase_state\":\"rebase-merge\""),
            "trace should record rebase-merge state\n{trace_content}"
        );
        assert!(
            trace_content.contains("\"state_rebase_todo_head\":\"pick deadbeef step\""),
            "trace should skip todo comments and blanks\n{trace_content}"
        );
        assert!(
            trace_content.contains("\"state_rebase_done_tail\":\"pick done\""),
            "trace should record done tail\n{trace_content}"
        );
    }

    #[test]
    fn status_trace_records_rebase_apply_snapshot_fields() {
        let dir = init_repo();
        let repo = dir.path();
        commit_file(repo, "file.txt", "base\n", "chore: base");

        let rebase_apply = git_dir(repo).join("rebase-apply");
        fs::create_dir_all(&rebase_apply).or_abort();
        fs::write(rebase_apply.join("next"), "3\n").or_abort();
        fs::write(rebase_apply.join("last"), "7\n").or_abort();
        fs::write(rebase_apply.join("patch"), "dummy patch\n").or_abort();

        let trace_path = repo.join("trace/rebase-apply.jsonl");
        run_git_factor_with_env(
            repo,
            &["--status"],
            GitFactorExpectation::default().stdout("FACTOR: No active session.\n"),
            "GIT_FACTOR_TRACE_LOG",
            trace_path.as_os_str().to_os_string(),
        );

        let trace_content = fs::read_to_string(&trace_path).or_abort();
        assert!(
            trace_content.contains("\"state_rebase_state\":\"rebase-apply\""),
            "trace should record rebase-apply state\n{trace_content}"
        );
        assert!(
            trace_content.contains("\"state_rebase_todo_head\":\"patch\""),
            "trace should record synthetic patch todo marker\n{trace_content}"
        );
    }

    #[test]
    fn status_trace_collects_paths_and_ignores_short_status_lines() {
        let dir = init_repo();
        let repo = dir.path();
        commit_file(repo, "file.txt", "base\n", "chore: base");

        let (wrap_dir, wrap_bin) = make_git_wrapper_named(
            "git",
            r#"if [ "${1-}" = "status" ] && [ "${2-}" = "--porcelain=v1" ] && [ "${3-}" = "--untracked-files=all" ]; then
  printf 'M\n'
  printf 'A  staged.txt\n'
  printf ' M unstaged.txt\n'
  printf '?? untracked.txt\n'
  exit 0
fi
"#,
        );
        let _keep_alive = wrap_dir;

        let trace_path = repo.join("trace/status-paths.jsonl");
        run_git_factor_with_prefixed_path_and_env(
            repo,
            &["--status"],
            GitFactorExpectation::default().stdout("FACTOR: No active session.\n"),
            format!("{}:{}", wrap_bin.display(), env::var("PATH").or_abort()).into(),
            "GIT_FACTOR_TRACE_LOG",
            trace_path.as_os_str().to_os_string(),
        );

        let trace_content = fs::read_to_string(&trace_path).or_abort();
        assert!(
            trace_content.contains("\"state_staged_paths\":[\"staged.txt\"]"),
            "trace should capture staged paths from porcelain output\n{trace_content}"
        );
        assert!(
            trace_content.contains("\"state_unstaged_paths\":[\"unstaged.txt\"]"),
            "trace should capture unstaged paths from porcelain output\n{trace_content}"
        );
        assert!(
            trace_content.contains("\"state_untracked_paths\":[\"untracked.txt\"]"),
            "trace should capture untracked paths from porcelain output\n{trace_content}"
        );
    }

    #[test]
    fn continue_trace_logs_spawn_error_when_bash_is_unavailable() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");
        commit_file(repo, "file.txt", "one\ntwo\n", "feat: change");

        start_session(repo);
        git(repo, &["add", "--all"]);

        let wrapper_root = TempDir::new().or_abort();
        let wrapper_bin = wrapper_root.path().join("bin");
        fs::create_dir_all(&wrapper_bin).or_abort();
        let git_wrapper = wrapper_bin.join("git");
        write_executable(&git_wrapper, "#!/bin/sh\nexec /usr/bin/git \"$@\"\n");

        let trace_root = TempDir::new().or_abort();
        let trace_path = trace_root.path().join("missing-bash.jsonl");
        run_git_factor_with_prefixed_path_and_env(
            repo,
            &["--continue", "--message", "test: split"],
            GitFactorExpectation::default().code(EXIT_SOFTWARE),
            wrapper_bin.as_os_str().to_os_string(),
            "GIT_FACTOR_TRACE_LOG",
            trace_path.as_os_str().to_os_string(),
        );

        let trace_content = fs::read_to_string(&trace_path).or_abort();
        assert!(
            trace_content.contains("\"bin\":\"bash\""),
            "trace should identify missing bash spawn source\n{trace_content}"
        );
        assert!(
            trace_content.contains("\"spawned\":false"),
            "trace should record failed spawn details\n{trace_content}"
        );
    }

    #[test]
    fn continue_without_trace_log_reports_missing_bash_and_skips_trace_output() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");
        commit_file(repo, "file.txt", "one\ntwo\n", "feat: change");

        start_session(repo);
        git(repo, &["add", "--all"]);

        let wrapper_root = TempDir::new().or_abort();
        let wrapper_bin = wrapper_root.path().join("bin");
        fs::create_dir_all(&wrapper_bin).or_abort();
        let git_wrapper = wrapper_bin.join("git");
        write_executable(&git_wrapper, "#!/bin/sh\nexec /usr/bin/git \"$@\"\n");

        run_git_factor_with_prefixed_path(
            repo,
            &["--continue", "--message", "test: split"],
            GitFactorExpectation::default()
                .code(EXIT_SOFTWARE)
                .stderr("git command failed: bash -c: No such file or directory (os error 2)\n"),
            wrapper_bin.as_os_str().to_os_string(),
        );

        assert!(
            !repo.join("trace").exists(),
            "trace directory should not be created when trace env is unset"
        );
    }

    #[test]
    fn continue_reports_rehydrate_write_tree_failure_after_exec_spawn_error() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");
        commit_file(repo, "file.txt", "one\ntwo\n", "feat: change");

        start_session(repo);
        git(repo, &["add", "--all"]);

        let wrapper_root = TempDir::new().or_abort();
        let wrapper_bin = wrapper_root.path().join("bin");
        fs::create_dir_all(&wrapper_bin).or_abort();
        let git_wrapper = wrapper_bin.join("git");
        write_executable(
            &git_wrapper,
            r#"#!/bin/sh
if [ "${1-}" = "write-tree" ]; then
  echo "forced write-tree failure" 1>&2
  exit 1
fi
exec /usr/bin/git "$@"
"#,
        );
        let _keep_alive = wrapper_root;

        run_git_factor_with_prefixed_path(
            repo,
            &["--continue", "--message", "test: split"],
            GitFactorExpectation::default()
                .code(EXIT_SOFTWARE)
                .stderr("git command failed: forced write-tree failure\n"),
            wrapper_bin.as_os_str().to_os_string(),
        );
    }

    #[test]
    fn start_writes_is_root_false_when_base_commit_is_not_root() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");
        commit_file(repo, "file.txt", "one\ntwo\n", "feat: middle");
        commit_file(repo, "file.txt", "one\ntwo\nthree\n", "feat: change");

        run_git_factor(
            repo,
            &["--exec", "true", "HEAD"],
            GitFactorExpectation::default().path_content(".git/factor/is_root", "false\n"),
        );
    }

    #[test]
    fn continue_updates_split_count_side_effect_in_git_dir() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "base\n", "chore: base");
        commit_file(repo, "file.txt", "base\na\nb\n", "feat: change");

        let expected_tree = git(repo, &["rev-parse", "HEAD^{tree}"]);
        start_session(repo);

        write_file(repo, "file.txt", "base\na\n");
        git(repo, &["add", "file.txt"]);

        run_git_factor(
            repo,
            &["--continue", "--message", "test: split slice"],
            GitFactorExpectation::default()
                .factor_state_exists(true)
                .path_content(".git/factor/current_index", "0\n")
                .path_content(".git/factor/split_count", "1\n")
                .path_content(".git/factor/expected_tree", format!("{expected_tree}\n"))
                .git_status_porcelain_non_empty(),
        );
    }

    #[test]
    fn continue_reports_split_count_overflow() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "base\n", "chore: base");
        commit_file(repo, "file.txt", "base\na\nb\n", "feat: change");

        start_session(repo);
        fs::write(
            git_dir(repo).join("factor/split_count"),
            format!("{}\n", u8::MAX),
        )
        .or_abort();

        write_file(repo, "file.txt", "base\na\n");
        git(repo, &["add", "file.txt"]);

        run_git_factor(
            repo,
            &["--continue", "--message", "test: split slice"],
            GitFactorExpectation::default()
                .code(EXIT_SOFTWARE)
                .stderr("git command failed: split_count overflow\n"),
        );
    }

    #[test]
    fn start_reports_error_when_rebase_disappears_after_start_gate() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");
        commit_file(repo, "file.txt", "one\ntwo\n", "feat: change");

        run_git_factor(
            repo,
            &["--exec", "rm -rf .git/rebase-merge && true", "HEAD~1"],
            GitFactorExpectation::default()
                .code(EXIT_SOFTWARE)
                .stderr_suffix("git command failed: git rebase failed (exit 1)\n"),
        );
    }

    #[test]
    fn continue_preserves_index_and_rehydrates_pool_when_exec_gate_fails() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "tracked.txt", "one\n", "chore: base");

        // Original commit both modifies a tracked file and adds a new file.
        write_file(repo, "tracked.txt", "one\ntwo\n");
        write_file(repo, "new.txt", "new\n");
        git(repo, &["add", "--all"]);
        git(repo, &["commit", "--message", "feat: original"]);

        // Start session, then force a failing exec gate for continue.
        start_session(repo);
        overwrite_session_exec(repo, "false");

        // Stage only tracked.txt from the pool; leave new.txt unstaged.
        git(repo, &["add", "tracked.txt"]);

        run_git_factor(
            repo,
            &["--continue", "--message", "test: staged slice"],
            GitFactorExpectation::default()
                .code(EXIT_TEMPFAIL)
                .stderr("exec gate failed: false (exit code 1)\n")
                .git_output(&["diff", "--name-only", "--staged"], "tracked.txt")
                .git_status_porcelain("M  tracked.txt\n?? new.txt")
                .path_exists("new.txt", true),
        );
    }

    #[test]
    fn continue_leaves_remaining_changes_unstaged_after_partial_split() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "base.txt", "base\n", "chore: base");

        // Original commit touches two files so we can split into at least 2 slices.
        write_file(repo, "a.txt", "a\n");
        write_file(repo, "b.txt", "b\n");
        git(repo, &["add", "--all"]);
        git(repo, &["commit", "--message", "feat: original"]);

        start_session(repo);

        // Commit only a.txt as the first slice.
        git(repo, &["add", "a.txt"]);
        run_git_factor(
            repo,
            &["--continue", "--message", "test: slice a"],
            GitFactorExpectation::default()
                .stdout(
                    "\
FACTOR: Split 1 committed.
STATE: Remaining changes are unstaged.
UNSTAGED:
UNTRACKED:
  b.txt

NEXT: Stage changes for the next commit, then run:
  git factor --continue --message \"type: description\"

HINTS:
  - Find the ONE smallest addition nothing depends on
  - Target 15-30 lines (50 max)
  - Message: single concrete action, no \"and\"/\"or\"
  - Verify: git log --oneline | wc -l
  - NEVER use git commit. ONLY use git factor --continue.
  RECOVERY: git factor --abort
",
                )
                .git_output(&["ls-files", "--others", "--exclude-standard"], "b.txt"),
        );
    }

    #[test]
    fn continue_leaves_remaining_changes_unstaged_without_untracked_after_partial_split() {
        let dir = init_repo();
        let repo = dir.path();

        write_file(repo, "a.txt", "a0\n");
        write_file(repo, "b.txt", "b0\n");
        git(repo, &["add", "--all"]);
        git(repo, &["commit", "--message", "chore: base"]);

        write_file(repo, "a.txt", "a0\na1\n");
        write_file(repo, "b.txt", "b0\nb1\n");
        git(repo, &["add", "--all"]);
        git(repo, &["commit", "--message", "feat: original"]);

        start_session(repo);

        git(repo, &["add", "a.txt"]);
        run_git_factor(
            repo,
            &["--continue", "--message", "test: slice a"],
            GitFactorExpectation::default()
                .stdout(
                    "\
FACTOR: Split 1 committed.
STATE: Remaining changes are unstaged.
UNSTAGED:
  b.txt | 1 +
   1 file changed, 1 insertion(+)

NEXT: Stage changes for the next commit, then run:
  git factor --continue --message \"type: description\"

HINTS:
  - Find the ONE smallest addition nothing depends on
  - Target 15-30 lines (50 max)
  - Message: single concrete action, no \"and\"/\"or\"
  - Verify: git log --oneline | wc -l
  - NEVER use git commit. ONLY use git factor --continue.
  REMAINING:  1 file changed, 1 insertion(+)
  RECOVERY: git factor --abort
",
                )
                .git_output(&["ls-files", "--others", "--exclude-standard"], ""),
        );
    }

    #[test]
    fn finish_reuses_original_commit_message_when_none_provided() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");
        commit_file(repo, "file.txt", "one\ntwo\n", "feat: original message");

        let original_message = git(repo, &["log", "--format=%B", "--max-count=1"]);

        start_session(repo);

        run_git_factor(
            repo,
            &["--finish"],
            GitFactorExpectation::default().git_output(
                &["log", "--format=%B", "--max-count=1"],
                original_message.trim_end().to_owned(),
            ),
        );
    }

    #[test]
    fn finish_succeeds_without_rebase_when_target_is_head() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");
        commit_file(repo, "file.txt", "one\ntwo\n", "feat: change");
        let head_short_sha = git(repo, &["rev-parse", "--short", "HEAD"]);

        run_git_factor(
            repo,
            &["--exec", "true", "HEAD"],
            GitFactorExpectation::default()
                .rebase_apply_exists(false)
                .rebase_merge_exists(false)
                .requires_rebase(false)
                .stdout(expected_single_commit_start_stdout(
                    head_short_sha.as_str(),
                    "feat: change",
                )),
        );

        git(repo, &["add", "--all"]);
        run_git_factor(
            repo,
            &["--finish", "--message", "test: done"],
            GitFactorExpectation::default().stdout_suffix(expected_completion_stdout_suffix(1)),
        );
    }

    #[test]
    fn finish_preserves_empty_commits() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");
        git(
            repo,
            &["commit", "--allow-empty", "--message", "feat: placeholder"],
        );

        let expected_tree = git(repo, &["rev-parse", "HEAD^{tree}"]);
        let expected_subject = git(repo, &["log", "--format=%s", "--max-count=1"]);

        start_session(repo);

        run_git_factor(
            repo,
            &["--finish"],
            GitFactorExpectation::default()
                .git_output(&["rev-parse", "HEAD^{tree}"], expected_tree)
                .git_output(&["log", "--format=%s", "--max-count=1"], expected_subject),
        );
    }

    #[test]
    fn finish_uses_allow_empty_when_original_commit_is_empty() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");
        git(
            repo,
            &[
                "commit",
                "--allow-empty",
                "--message",
                "feat: empty original",
            ],
        );

        start_session(repo);

        let (wrap_dir, wrap_bin) = make_git_wrapper_named(
            "git",
            r#"if [ "${1-}" = "commit" ]; then
  for arg in "$@"; do
    if [ "$arg" = "--allow-empty" ]; then
      exit 1
    fi
  done
fi
"#,
        );
        let _keep_alive = wrap_dir;

        let original_path = env::var_os("PATH").or_abort();
        let prefixed_path = {
            let mut joined = OsString::new();
            joined.push(wrap_bin.as_os_str());
            joined.push(OsStr::new(":"));
            joined.push(original_path);
            joined
        };

        run_git_factor_with_prefixed_path(
            repo,
            &["--finish"],
            GitFactorExpectation::default()
                .code(EXIT_SOFTWARE)
                .stderr("git command failed: git commit failed (exit 1)\n"),
            prefixed_path,
        );
    }

    #[test]
    fn finish_ignores_exec_gate_and_completes_session() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");
        commit_file(repo, "file.txt", "one\ntwo\n", "feat: original");

        start_session(repo);
        overwrite_session_exec(repo, "false");

        run_git_factor(
            repo,
            &["--finish"],
            GitFactorExpectation::default()
                .stdout_suffix(expected_completion_stdout_suffix(1))
                .factor_state_exists(false)
                .path_exists(".git/factor", false),
        );
    }

    #[test]
    fn start_invokes_sequence_editor_when_bin_path_has_spaces() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");
        commit_file(repo, "file.txt", "one\ntwo\n", "feat: change");

        let bin_root = TempDir::new().or_abort();
        let spaced = bin_root.path().join("with spaces");
        fs::create_dir_all(&spaced).or_abort();
        let (factor, _editor) = copy_bins_to(&spaced);

        run_git_factor_with_bin(
            repo,
            factor.as_path(),
            &["--exec", "true", "HEAD"],
            GitFactorExpectation::default(),
        );
    }

    #[test]
    fn start_invokes_sequence_editor_when_bin_path_has_single_quote() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");
        commit_file(repo, "file.txt", "one\ntwo\n", "feat: change");

        let bin_root = TempDir::new().or_abort();
        let quoted = bin_root.path().join("with'quote");
        fs::create_dir_all(&quoted).or_abort();
        let (factor, _editor) = copy_bins_to(&quoted);

        run_git_factor_with_bin(
            repo,
            factor.as_path(),
            &["--exec", "true", "HEAD~1"],
            GitFactorExpectation::default(),
        );
    }

    #[test]
    fn start_uses_rebase_root_for_multi_commit_session_starting_at_root() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "root\n", "feat: root");
        commit_file(repo, "file.txt", "root\nnext\n", "feat: next");
        let root_sha = git(repo, &["rev-list", "--max-parents=0", "HEAD"]);

        let (wrap_dir, wrap_bin) = make_git_wrapper_named(
            "git",
            r#"if [ "${1-}" = "rebase" ]; then
  saw_root=0
  for arg in "$@"; do
    if [ "$arg" = "--root" ]; then
      saw_root=1
      break
    fi
  done
  if [ "$saw_root" -eq 1 ]; then
    exit 42
  fi
  exit 43
fi
"#,
        );
        let _keep_alive = wrap_dir;

        run_git_factor_with_env(
            repo,
            &["--exec", "true", root_sha.as_str(), "HEAD"],
            GitFactorExpectation::default()
                .code(EXIT_SOFTWARE)
                .stderr("git command failed: git rebase failed (exit 42)\n"),
            "PATH",
            format!("{}:{}", wrap_bin.display(), env::var("PATH").or_abort()),
        );
    }

    #[test]
    fn start_does_not_use_rebase_root_for_non_root_multi_commit_session() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "feat: one");
        commit_file(repo, "file.txt", "one\ntwo\n", "feat: two");
        commit_file(repo, "file.txt", "one\ntwo\nthree\n", "feat: three");

        let (wrap_dir, wrap_bin) = make_git_wrapper_named(
            "git",
            r#"if [ "${1-}" = "rebase" ]; then
  saw_root=0
  for arg in "$@"; do
    if [ "$arg" = "--root" ]; then
      saw_root=1
      break
    fi
  done
  if [ "$saw_root" -eq 1 ]; then
    exit 42
  fi
  exit 43
fi
"#,
        );
        let _keep_alive = wrap_dir;

        run_git_factor_with_env(
            repo,
            &["--exec", "true", "HEAD~1", "HEAD"],
            GitFactorExpectation::default()
                .code(EXIT_SOFTWARE)
                .stderr("git command failed: git rebase failed (exit 43)\n"),
            "PATH",
            format!("{}:{}", wrap_bin.display(), env::var("PATH").or_abort()),
        );
    }

    #[test]
    fn prints_claude_hints_and_reference_path_when_present() {
        let dir = init_repo();
        let repo = dir.path();

        write_file(repo, "file.txt", "one\n");
        fs::create_dir_all(repo.join("references")).or_abort();
        write_file(repo, "references/rust.md", "# rust\n");
        git(repo, &["add", "file.txt", "references/rust.md"]);
        git(repo, &["commit", "--message", "chore: base"]);
        commit_file(repo, "file.txt", "one\ntwo\n", "feat: change");
        let head_short_sha = git(repo, &["rev-parse", "--short", "HEAD"]);
        let reference_path = repo.join("references/rust.md").canonicalize().or_abort();

        run_git_factor_with_env(
            repo,
            &["--exec", "true", "HEAD"],
            GitFactorExpectation::default().stdout(format!(
                "\
FACTOR: Split session started for {}.
ORIGINAL MESSAGE: feat: change
UNSTAGED:
  file.txt | 1 +
   1 file changed, 1 insertion(+)

NEXT: Stage changes for the first atomic commit, then run:
  git factor --continue --message \"type: description\"

Run git factor --help for the full workflow guide.

HINTS:
  - Find the ONE smallest addition nothing depends on
  - Target 15-30 lines (50 max)
  - Message: single concrete action, no \"and\"/\"or\"
  - Verify: git log --oneline | wc -l
  - NEVER use git commit. ONLY use git factor --continue.
  REMAINING:  1 file changed, 1 insertion(+)
  REFERENCE: {}
  RECOVERY: git factor --abort
<claude>
- If context is above 50%, pause and ask the user to /compact.
- Do NOT stop early. Keep committing until \"Complete\".
- Do NOT use git commit directly. ONLY use git-factor --continue.
- Each commit MUST pass the exec gate. No shortcuts.
</claude>
",
                head_short_sha.as_str(),
                reference_path.display()
            )),
            "CLAUDECODE",
            "1",
        );
    }

    #[test]
    fn prints_claude_hints_without_reference_path_when_missing() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");
        commit_file(repo, "file.txt", "one\ntwo\n", "feat: change");
        let head_short_sha = git(repo, &["rev-parse", "--short", "HEAD"]);

        run_git_factor_with_env(
            repo,
            &["--exec", "true", "HEAD"],
            GitFactorExpectation::default().stdout(format!(
                "\
FACTOR: Split session started for {}.
ORIGINAL MESSAGE: feat: change
UNSTAGED:
  file.txt | 1 +
   1 file changed, 1 insertion(+)

NEXT: Stage changes for the first atomic commit, then run:
  git factor --continue --message \"type: description\"

Run git factor --help for the full workflow guide.

HINTS:
  - Find the ONE smallest addition nothing depends on
  - Target 15-30 lines (50 max)
  - Message: single concrete action, no \"and\"/\"or\"
  - Verify: git log --oneline | wc -l
  - NEVER use git commit. ONLY use git factor --continue.
  REMAINING:  1 file changed, 1 insertion(+)
  RECOVERY: git factor --abort
<claude>
- If context is above 50%, pause and ask the user to /compact.
- Do NOT stop early. Keep committing until \"Complete\".
- Do NOT use git commit directly. ONLY use git-factor --continue.
- Each commit MUST pass the exec gate. No shortcuts.
</claude>
",
                head_short_sha.as_str()
            )),
            "CLAUDECODE",
            "1",
        );
    }

    #[test]
    fn abort_mid_rebase_cleans_untracked_files() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");
        commit_file(repo, "file.txt", "one\ntwo\n", "feat: change");
        commit_file(repo, "file.txt", "one\ntwo\nthree\n", "feat: latest");

        run_git_factor(
            repo,
            &["--exec", "true", "HEAD~1"],
            GitFactorExpectation::default()
                .rebase_merge_exists(true)
                .rebase_apply_exists(false),
        );

        write_file(repo, "untracked.txt", "hello\n");

        run_git_factor(
            repo,
            &["--abort"],
            GitFactorExpectation::default()
                .path_exists("untracked.txt", false)
                .git_status_porcelain(""),
        );
    }

    #[test]
    fn abort_succeeds_when_rebase_apply_is_active() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");
        start_session(repo);
        fs::create_dir_all(git_dir(repo).join("rebase-apply")).or_abort();

        run_git_factor(
            repo,
            &["--abort"],
            GitFactorExpectation::default()
                .stdout(
                    "FACTOR: Session aborted for current commit step.\nFACTOR: Rebase still active. To abort full rebase, run: git rebase --abort\n",
                )
                .rebase_apply_exists(true)
                .rebase_merge_exists(false),
        );
    }

    #[test]
    fn abort_reports_rebase_abort_failure() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");
        commit_file(repo, "file.txt", "one\ntwo\n", "feat: change");
        commit_file(repo, "file.txt", "one\ntwo\nthree\n", "feat: latest");

        run_git_factor(
            repo,
            &["--exec", "true", "HEAD~1"],
            GitFactorExpectation::default(),
        );

        // If git-factor calls `git rebase --abort`, this wrapper forces a failure.
        let (wrap_dir, wrap_bin) = make_git_wrapper_named(
            "git",
            r#"if [ "${1-}" = "rebase" ] && [ "${2-}" = "--abort" ]; then
  exit 1
fi
"#,
        );
        let _keep_alive = wrap_dir;

        run_git_factor_with_env(
            repo,
            &["--abort"],
            GitFactorExpectation::default()
                .code(EXIT_SOFTWARE)
                .stderr("git command failed: git rebase failed (exit 1)\n")
                .factor_state_exists(true),
            "PATH",
            format!("{}:{}", wrap_bin.display(), env::var("PATH").or_abort()),
        );
    }

    #[test]
    fn continue_advances_to_next_commit_for_multi_commit_range() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "base.txt", "base\n", "chore: base");
        commit_file(repo, "base.txt", "base\na\n", "feat: a");
        // Second commit introduces an untracked file when reset --mixed runs, so
        // advance_to_next_commit prints the UNTRACKED section.
        write_file(repo, "base.txt", "base\na\nb\n");
        write_file(repo, "new.txt", "new\n");
        git(repo, &["add", "--all"]);
        git(repo, &["commit", "--message", "feat: b"]);
        let first_short_sha = git(repo, &["rev-parse", "--short", "HEAD~1"]);
        let second_short_sha = git(repo, &["rev-parse", "--short", "HEAD"]);

        run_git_factor(
            repo,
            &["--exec", "true", "HEAD~2..HEAD"],
            GitFactorExpectation::default()
                .stdout_suffix(expected_multi_commit_start_suffix(first_short_sha.as_str())),
        );

        // First commit: stage everything and continue. This should advance to the next commit.
        git(repo, &["add", "--all"]);
        run_git_factor(
            repo,
            &["--continue", "--message", "test: split a"],
            GitFactorExpectation::default().stdout_suffix(
                expected_now_splitting_suffix_with_remaining(
                    second_short_sha.as_str(),
                    "feat: b",
                    "  base.txt | 1 +\n   1 file changed, 1 insertion(+)\nUNTRACKED:\n  new.txt",
                ),
            ),
        );

        // Second commit: finish quickly.
        git(repo, &["add", "--all"]);
        run_git_factor(
            repo,
            &["--continue", "--message", "test: split b"],
            GitFactorExpectation::default().stdout_suffix(expected_completion_stdout_suffix(1)),
        );
    }

    #[test]
    fn continue_advances_to_next_commit_for_multiple_explicit_refs() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "base.txt", "base\n", "chore: base");
        commit_file(repo, "base.txt", "base\na\n", "feat: a");
        write_file(repo, "base.txt", "base\na\nb\n");
        git(repo, &["add", "--all"]);
        git(repo, &["commit", "--message", "feat: b"]);
        let first_short_sha = git(repo, &["rev-parse", "--short", "HEAD~1"]);
        let second_short_sha = git(repo, &["rev-parse", "--short", "HEAD"]);

        run_git_factor(
            repo,
            &["--exec", "true", "HEAD~1", "HEAD"],
            GitFactorExpectation::default()
                .stdout_suffix(expected_multi_commit_start_suffix(first_short_sha.as_str())),
        );

        git(repo, &["add", "--all"]);
        run_git_factor(
            repo,
            &["--continue", "--message", "test: split a"],
            GitFactorExpectation::default().stdout_suffix(
                expected_now_splitting_suffix_with_remaining(
                    second_short_sha.as_str(),
                    "feat: b",
                    "  base.txt | 1 +\n   1 file changed, 1 insertion(+)",
                ),
            ),
        );

        git(repo, &["add", "--all"]);
        run_git_factor(
            repo,
            &["--continue", "--message", "test: split b"],
            GitFactorExpectation::default().stdout_suffix(expected_completion_stdout_suffix(1)),
        );
    }

    #[test]
    fn continue_completes_rebase_when_last_commit_has_no_next_edit_stop() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "base.txt", "base\n", "chore: base");
        commit_file(repo, "base.txt", "base\na\n", "feat: a");
        commit_file(repo, "base.txt", "base\na\nb\n", "feat: b");

        run_git_factor(
            repo,
            &["--exec", "true", "HEAD~2..HEAD"],
            GitFactorExpectation::default(),
        );

        git(repo, &["add", "--all"]);
        run_git_factor(
            repo,
            &["--continue", "--message", "test: split a"],
            GitFactorExpectation::default(),
        );

        git(repo, &["add", "--all"]);
        run_git_factor(
            repo,
            &["--continue", "--message", "test: split b"],
            GitFactorExpectation::default()
                .stdout_suffix(expected_completion_stdout_suffix(1))
                .rebase_merge_exists(false),
        );
    }

    #[test]
    fn continue_prints_untracked_when_next_commit_adds_files() {
        let dir = init_repo();
        let repo = dir.path();

        // Commit A modifies an existing file.
        commit_file(repo, "file.txt", "one\n", "chore: one");
        write_file(repo, "file.txt", "two\n");
        git(repo, &["add", "file.txt"]);
        git(repo, &["commit", "--message", "chore: two"]);

        // Commit B adds a new file, which will become untracked after the mixed reset.
        write_file(repo, "new.txt", "new\n");
        git(repo, &["add", "new.txt"]);
        git(repo, &["commit", "--message", "chore: add new"]);
        let second_short_sha = git(repo, &["rev-parse", "--short", "HEAD"]);

        // Start factoring both commits.
        run_git_factor(
            repo,
            &["--exec", "true", "HEAD~2..HEAD"],
            GitFactorExpectation::default(),
        );

        // Stage the entire first commit's change so we advance to the next commit.
        git(repo, &["add", "file.txt"]);

        run_git_factor(
            repo,
            &["--continue", "--message", "test: first"],
            GitFactorExpectation::default().stdout_suffix(format!(
                "\
FACTOR: Previous commit split into 1 commits.
FACTOR: Now splitting {}.
ORIGINAL MESSAGE: chore: add new
UNSTAGED:
UNTRACKED:
  new.txt

NEXT: Stage changes for the next commit, then run:
  git factor --continue --message \"type: description\"

HINTS:
  - Find the ONE smallest addition nothing depends on
  - Target 15-30 lines (50 max)
  - Message: single concrete action, no \"and\"/\"or\"
  - Verify: git log --oneline | wc -l
  - NEVER use git commit. ONLY use git factor --continue.
  RECOVERY: git factor --abort
",
                second_short_sha.as_str()
            )),
        );
    }

    #[test]
    fn start_prints_untracked_when_target_commit_adds_file() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "base.txt", "one\n", "chore: base");
        commit_file(repo, "new.txt", "new\n", "feat: add file");
        let head_short_sha = git(repo, &["rev-parse", "--short", "HEAD"]);

        run_git_factor(
            repo,
            &["--exec", "true", "HEAD"],
            GitFactorExpectation::default().stdout(format!(
                "\
FACTOR: Split session started for {}.
ORIGINAL MESSAGE: feat: add file
UNSTAGED:
UNTRACKED:
  new.txt

NEXT: Stage changes for the first atomic commit, then run:
  git factor --continue --message \"type: description\"

Run git factor --help for the full workflow guide.

HINTS:
  - Find the ONE smallest addition nothing depends on
  - Target 15-30 lines (50 max)
  - Message: single concrete action, no \"and\"/\"or\"
  - Verify: git log --oneline | wc -l
  - NEVER use git commit. ONLY use git factor --continue.
  RECOVERY: git factor --abort
",
                head_short_sha.as_str()
            )),
        );
    }

    #[test]
    fn continue_drops_empty_root_commit_session() {
        let dir = init_repo();
        let repo = dir.path();

        // Single root commit.
        commit_file(repo, "file.txt", "one\n", "feat: root");

        let expected_tree = git(repo, &["rev-parse", "HEAD^{tree}"]);
        let root = git(repo, &["rev-list", "--max-parents=0", "HEAD"]);

        start_session(repo);
        git(repo, &["add", "--all"]);
        run_git_factor(
            repo,
            &["--continue", "--message", "test: root split"],
            GitFactorExpectation::default()
                .stdout_suffix(expected_completion_stdout_suffix(1))
                .git_output(&["rev-parse", "HEAD^{tree}"], expected_tree)
                .git_output_non_empty(&["ls-tree", root.as_str()]),
        );
    }

    #[test]
    fn continue_trace_logs_rebase_editor_env_pairs_for_empty_root_cleanup() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "feat: root");

        start_session(repo);
        git(repo, &["add", "--all"]);

        let trace_root = TempDir::new().or_abort();
        let trace_path = trace_root.path().join("rebase-env.jsonl");
        run_git_factor_with_env(
            repo,
            &["--continue", "--message", "test: root split"],
            GitFactorExpectation::default().stdout_suffix(expected_completion_stdout_suffix(1)),
            "GIT_FACTOR_TRACE_LOG",
            trace_path.as_os_str().to_os_string(),
        );

        let trace_content = fs::read_to_string(&trace_path).or_abort();
        assert!(
            trace_content.contains("GIT_EDITOR=false"),
            "trace should include GIT_EDITOR env entry\n{trace_content}"
        );
        assert!(
            trace_content.contains("GIT_SEQUENCE_EDITOR="),
            "trace should include GIT_SEQUENCE_EDITOR env entry\n{trace_content}"
        );
    }

    #[test]
    fn continue_reports_multiple_root_history_during_empty_root_cleanup() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "feat: root");

        start_session(repo);
        git(repo, &["add", "--all"]);

        let (wrap_dir, wrap_bin) = make_git_wrapper_named(
            "git",
            r#"if [ "${1-}" = "rev-list" ] && [ "${2-}" = "--max-parents=0" ] && [ "${3-}" = "HEAD" ]; then
  printf '%s\n%s\n' "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
  exit 0
fi
"#,
        );
        let _keep_alive = wrap_dir;

        let original_path = env::var_os("PATH").or_abort();
        let prefixed_path = {
            let mut joined = OsString::new();
            joined.push(wrap_bin.as_os_str());
            joined.push(OsStr::new(":"));
            joined.push(original_path);
            joined
        };

        run_git_factor_with_prefixed_path(
            repo,
            &["--continue", "--message", "test: root split"],
            GitFactorExpectation::default()
                .code(EXIT_SOFTWARE)
                .stderr(
                    "git command failed: multiple root commits found; empty-root cleanup requires a single-root history\n",
                ),
            prefixed_path,
        );
    }

    #[test]
    fn continue_reports_empty_root_cleanup_rebase_failure() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "feat: root");

        start_session(repo);
        git(repo, &["add", "--all"]);

        let (wrap_dir, wrap_bin) = make_git_wrapper_named(
            "git",
            r#"if [ "${1-}" = "rebase" ] && [ "${2-}" = "--empty" ] && [ "${3-}" = "drop" ] && [ "${4-}" = "--interactive" ] && [ "${5-}" = "--quiet" ] && [ "${6-}" = "--root" ]; then
  exit 77
fi
"#,
        );
        let _keep_alive = wrap_dir;

        let original_path = env::var_os("PATH").or_abort();
        let prefixed_path = {
            let mut joined = OsString::new();
            joined.push(wrap_bin.as_os_str());
            joined.push(OsStr::new(":"));
            joined.push(original_path);
            joined
        };

        run_git_factor_with_prefixed_path(
            repo,
            &["--continue", "--message", "test: root split"],
            GitFactorExpectation::default()
                .code(EXIT_SOFTWARE)
                .stderr("git command failed: rebase to remove empty root failed (exit 77)\n"),
            prefixed_path,
        );
    }

    #[test]
    fn finish_skips_root_rebase_when_root_tree_is_not_empty() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "feat: root");

        start_session(repo);
        git(repo, &["add", "--all"]);

        let (wrap_dir, wrap_bin) = make_git_wrapper_named(
            "git",
            r#"if [ "${1-}" = "ls-tree" ]; then
  echo "100644 blob deadbeefdeadbeefdeadbeefdeadbeefdeadbeef	file.txt"
  exit 0
fi
if [ "${1-}" = "rebase" ] && [ "${2-}" = "--root" ] && [ "${3-}" = "--interactive" ]; then
  echo "UNEXPECTED_ROOT_REBASE" >&2
  exit 1
fi
"#,
        );
        let _keep_alive = wrap_dir;

        let original_path = env::var_os("PATH").or_abort();
        let prefixed_path = {
            let mut joined = OsString::new();
            joined.push(wrap_bin.as_os_str());
            joined.push(OsStr::new(":"));
            joined.push(original_path);
            joined
        };

        run_git_factor_with_prefixed_path(
            repo,
            &["--continue", "--message", "test: root split"],
            GitFactorExpectation::default().stdout_suffix(expected_completion_stdout_suffix(1)),
            prefixed_path,
        );
    }

    #[test]
    fn continue_errors_when_session_exists_but_no_rebase_is_active() {
        let dir = init_repo();
        let repo = dir.path();
        commit_file(repo, "file.txt", "one\n", "chore: base");

        let factor_dir = git_dir(repo).join("factor");
        fs::create_dir_all(&factor_dir).or_abort();
        fs::write(
            factor_dir.join("commits"),
            format!("{}\n", git(repo, &["rev-parse", "HEAD"])),
        )
        .or_abort();
        fs::write(factor_dir.join("current_index"), "0\n").or_abort();
        fs::write(factor_dir.join("exec"), "true\n").or_abort();
        fs::write(factor_dir.join("split_count"), "0\n").or_abort();

        run_git_factor(
            repo,
            &["--continue", "--message", "test: msg"],
            GitFactorExpectation::default()
                .code(EXIT_SOFTWARE)
                .stderr("git command failed: no rebase in progress\n"),
        );
    }

    #[test]
    fn finish_errors_when_session_exists_but_no_rebase_is_active() {
        let dir = init_repo();
        let repo = dir.path();
        commit_file(repo, "file.txt", "one\n", "chore: base");

        let factor_dir = git_dir(repo).join("factor");
        fs::create_dir_all(&factor_dir).or_abort();
        fs::write(
            factor_dir.join("commits"),
            format!("{}\n", git(repo, &["rev-parse", "HEAD"])),
        )
        .or_abort();
        fs::write(factor_dir.join("current_index"), "0\n").or_abort();
        fs::write(factor_dir.join("exec"), "true\n").or_abort();
        fs::write(factor_dir.join("split_count"), "0\n").or_abort();

        run_git_factor(
            repo,
            &["--finish"],
            GitFactorExpectation::default()
                .code(EXIT_SOFTWARE)
                .stderr("git command failed: no rebase in progress\n"),
        );
    }

    #[test]
    fn continue_reports_corrupted_commits_state_file() {
        let dir = init_repo();
        let repo = dir.path();
        commit_file(repo, "file.txt", "one\n", "chore: base");

        // Fake an active session + rebase so cmd_continue attempts to read state.
        let factor_dir = git_dir(repo).join("factor");
        fs::create_dir_all(&factor_dir).or_abort();
        fs::create_dir_all(git_dir(repo).join("rebase-merge")).or_abort();

        // Empty commits file should be rejected.
        fs::write(factor_dir.join("commits"), "\n").or_abort();
        fs::write(factor_dir.join("current_index"), "0\n").or_abort();
        fs::write(factor_dir.join("exec"), "true\n").or_abort();
        fs::write(factor_dir.join("split_count"), "0\n").or_abort();

        run_git_factor(
            repo,
            &["--continue", "--message", "test: msg"],
            GitFactorExpectation::default()
                .code(EXIT_SOFTWARE)
                .stderr("git command failed: corrupted state file 'commits': file is empty\n"),
        );
    }

    #[test]
    fn continue_reports_corrupted_requires_rebase_state() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");
        commit_file(repo, "file.txt", "one\ntwo\n", "feat: change");

        run_git_factor(repo, &["--exec", "true"], GitFactorExpectation::default());

        fs::write(
            git_dir(repo).join("factor/requires_rebase"),
            "definitely-not-a-bool\n",
        )
        .or_abort();

        git(repo, &["add", "--all"]);
        run_git_factor(
                repo,
                &["--continue", "--message", "test: split"],
                GitFactorExpectation::default()
                    .code(EXIT_SOFTWARE)
                    .stderr("git command failed: corrupted state file 'requires_rebase': invalid value 'definitely-not-a-bool'\n"),
            );
    }

    #[test]
    fn finish_reports_corrupted_requires_rebase_state() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");
        commit_file(repo, "file.txt", "one\ntwo\n", "feat: change");

        run_git_factor(repo, &["--exec", "true"], GitFactorExpectation::default());

        fs::write(
            git_dir(repo).join("factor/requires_rebase"),
            "definitely-not-a-bool\n",
        )
        .or_abort();

        run_git_factor(
                repo,
                &["--finish", "--message", "test: final"],
                GitFactorExpectation::default()
                    .code(EXIT_SOFTWARE)
                    .stderr("git command failed: corrupted state file 'requires_rebase': invalid value 'definitely-not-a-bool'\n"),
            );
    }

    #[test]
    fn continue_reports_unreadable_requires_rebase_state() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");
        commit_file(repo, "file.txt", "one\ntwo\n", "feat: change");

        run_git_factor(repo, &["--exec", "true"], GitFactorExpectation::default());

        let unreadable_path = git_dir(repo).join("factor/requires_rebase");
        fs::remove_file(&unreadable_path).or_abort();
        fs::create_dir_all(&unreadable_path).or_abort();

        git(repo, &["add", "--all"]);
        run_git_factor(
            repo,
            &["--continue", "--message", "test: split"],
            GitFactorExpectation::default()
                .code(EXIT_SOFTWARE)
                .stderr("failed to read state: Is a directory (os error 21)\n"),
        );
    }

    #[test]
    fn finish_reports_unreadable_requires_rebase_state() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");
        commit_file(repo, "file.txt", "one\ntwo\n", "feat: change");

        run_git_factor(repo, &["--exec", "true"], GitFactorExpectation::default());

        let unreadable_path = git_dir(repo).join("factor/requires_rebase");
        fs::remove_file(&unreadable_path).or_abort();
        fs::create_dir_all(&unreadable_path).or_abort();

        run_git_factor(
            repo,
            &["--finish", "--message", "test: final"],
            GitFactorExpectation::default()
                .code(EXIT_SOFTWARE)
                .stderr("failed to read state: Is a directory (os error 21)\n"),
        );
    }

    #[test]
    fn reports_invalid_numeric_state_files() {
        let dir = init_repo();
        let repo = dir.path();
        commit_file(repo, "file.txt", "one\n", "chore: base");

        // Fake an active session + rebase so cmd_continue attempts to read state.
        let factor_dir = git_dir(repo).join("factor");
        fs::create_dir_all(&factor_dir).or_abort();
        fs::create_dir_all(git_dir(repo).join("rebase-merge")).or_abort();

        fs::write(
            factor_dir.join("commits"),
            format!("{}\n", git(repo, &["rev-parse", "HEAD"])),
        )
        .or_abort();
        fs::write(factor_dir.join("current_index"), "not-a-number\n").or_abort();
        fs::write(factor_dir.join("exec"), "true\n").or_abort();
        fs::write(factor_dir.join("split_count"), "0\n").or_abort();

        run_git_factor(
                repo,
                &["--continue", "--message", "test: msg"],
                GitFactorExpectation::default()
                    .code(EXIT_SOFTWARE)
                    .stderr("git command failed: corrupted state file 'current_index': invalid value 'not-a-number'\n"),
            );
    }

    #[test]
    fn reports_out_of_range_commit_index() {
        let dir = init_repo();
        let repo = dir.path();
        commit_file(repo, "file.txt", "one\n", "chore: base");

        // Fake an active session + rebase so cmd_continue attempts to read state.
        let factor_dir = git_dir(repo).join("factor");
        fs::create_dir_all(&factor_dir).or_abort();
        fs::create_dir_all(git_dir(repo).join("rebase-merge")).or_abort();

        fs::write(
            factor_dir.join("commits"),
            format!("{}\n", git(repo, &["rev-parse", "HEAD"])),
        )
        .or_abort();
        fs::write(factor_dir.join("current_index"), "1\n").or_abort();
        fs::write(factor_dir.join("exec"), "true\n").or_abort();
        fs::write(factor_dir.join("split_count"), "0\n").or_abort();

        run_git_factor(
            repo,
            &["--continue", "--message", "test: msg"],
            GitFactorExpectation::default()
                .code(EXIT_SOFTWARE)
                .stderr("git command failed: commit index 1 out of range (have 1 commits)\n"),
        );
    }

    #[test]
    fn continue_validates_exec_gate_against_staged_index_state() {
        let dir = init_repo();
        let repo = dir.path();

        // Original commit has a file that does NOT satisfy the exec gate.
        commit_file(repo, "file.txt", "bad\n", "chore: base");
        commit_file(repo, "file.txt", "bad\nstill bad\n", "feat: change");

        // Start session, then set a gate that validates staged-index materialization.
        // Note: command substitution strips trailing newlines, so compare to "good".
        start_session(repo);
        overwrite_session_exec(repo, "test \"$(cat file.txt)\" = \"good\"");

        // Stage the gated content; the worktree will be cleaned back to HEAD
        // inside --continue, so the tool must materialize the index into the
        // filesystem before running the gate.
        write_file(repo, "file.txt", "good\n");
        git(repo, &["add", "file.txt"]);

        run_git_factor(
            repo,
            &["--continue", "--message", "test: slice"],
            GitFactorExpectation::default(),
        );
    }

    #[test]
    fn continue_materializes_index_by_removing_staged_deletions_from_worktree() {
        let dir = init_repo();
        let repo = dir.path();

        // Stage a deletion in the slice by splitting a commit that *removes*
        // a tracked file (so `git add -u` can stage the removal).
        commit_file(repo, "keep.txt", "keep\n", "chore: base");
        commit_file(repo, "delete-me.txt", "gone\n", "chore: add delete-me");
        git(repo, &["rm", "--quiet", "delete-me.txt"]);
        git(repo, &["commit", "--message", "feat: delete delete-me"]);

        start_session(repo);

        // Stage a deletion as the slice.
        git(repo, &["add", "--update", "--", "delete-me.txt"]);

        run_git_factor(
            repo,
            &["--continue", "--message", "test: delete"],
            GitFactorExpectation::default(),
        );
    }

    #[test]
    fn start_reports_rebase_failure_when_sequence_editor_fails() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");
        commit_file(repo, "file.txt", "one\ntwo\n", "feat: change");

        // Copy git-factor into a new directory and provide a sibling editor that always fails.
        let bin_dir = TempDir::new().or_abort();
        let (factor, _editor_unused) = copy_bins_to(bin_dir.path());
        let bad_editor = bin_dir.path().join("git-sequence-editor");
        write_executable(&bad_editor, "#!/bin/sh\nexit 1\n");
        let bad_editor_canonical = fs::canonicalize(&bad_editor).or_abort();
        let edited_short_sha = git(repo, &["rev-parse", "--short", "HEAD~1"]);
        let factor_str = factor.to_str().or_abort();
        let preflight = format!(
            "{} {} {}",
            shell_quote(factor_str),
            shell_quote("rebase-exec-preflight"),
            shell_quote("0")
        );
        let begin = format!(
            "{} {} {}",
            shell_quote(factor_str),
            shell_quote("rebase-exec-begin"),
            shell_quote("0")
        );
        let sequence_editor = [
            shell_quote(bad_editor_canonical.to_str().or_abort()),
            shell_quote("--factor-target"),
            shell_quote(edited_short_sha.as_str()),
            shell_quote("--factor-preflight"),
            shell_quote(preflight.as_str()),
            shell_quote("--factor-begin"),
            shell_quote(begin.as_str()),
        ]
        .join(" ");
        let expected_stderr = format!(
            "error: there was a problem with the editor '{sequence_editor}'\n\
                 git command failed: git rebase failed (exit 1)\n"
        );

        run_git_factor_with_bin(
            repo,
            factor.as_path(),
            &["--exec", "true", "HEAD~1"],
            GitFactorExpectation::default()
                .code(EXIT_SOFTWARE)
                .stderr(expected_stderr),
        );
    }

    #[test]
    fn continue_reports_git_commit_failure() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");
        commit_file(repo, "file.txt", "one\ntwo\n", "feat: change");

        run_git_factor(repo, &["--exec", "true"], GitFactorExpectation::default());

        let (wrap_dir, wrap_bin) = make_git_wrapper_named(
            "git",
            r#"if [ "${1-}" = "commit" ]; then
  exit 1
fi
"#,
        );

        let original_path = env::var_os("PATH").or_abort();
        let prefixed_path = {
            let mut joined = OsString::new();
            joined.push(wrap_bin.as_os_str());
            joined.push(OsStr::new(":"));
            joined.push(original_path);
            joined
        };

        git(repo, &["add", "--all"]);

        let _keep_alive = wrap_dir;
        run_git_factor_with_prefixed_path(
            repo,
            &["--continue", "--message", "test: split"],
            GitFactorExpectation::default()
                .code(EXIT_SOFTWARE)
                .stderr("git command failed: git commit failed (exit 1)\n"),
            prefixed_path,
        );
    }

    #[test]
    fn start_reports_missing_git_binary_from_git_output() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");

        run_git_factor_with_env(
            repo,
            &["--exec", "true", "HEAD"],
            GitFactorExpectation::default().code(EXIT_DATAERR).stderr(
                "failed to determine git directory: No such file or directory (os error 2)\n",
            ),
            "PATH",
            "",
        );
    }

    #[test]
    fn start_reports_nonzero_git_output_status_with_stderr_message() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");

        let (wrap_dir, wrap_bin) = make_git_wrapper_named(
            "git",
            r#"if [ "${1-}" = "rev-parse" ] && [ "${2-}" = "--short" ]; then
  echo "fatal: short lookup failed" 1>&2
  exit 42
fi
"#,
        );
        let _keep_alive = wrap_dir;
        let wrapped_path = format!("{}:{}", wrap_bin.display(), env::var("PATH").or_abort());

        run_git_factor_with_env(
            repo,
            &["--exec", "true", "HEAD"],
            GitFactorExpectation::default()
                .code(EXIT_SOFTWARE)
                .stderr("git command failed: fatal: short lookup failed\n"),
            "PATH",
            wrapped_path,
        );
    }

    #[test]
    fn continue_restore_handles_file_directory_replacement() {
        let dir = init_repo();
        let repo = dir.path();

        // Base repo state.
        commit_file(repo, "file.txt", "base\n", "chore: base");

        // Original commit adds `conflict` file.
        commit_file(repo, "conflict", "theirs\n", "feat: add conflict file");
        start_session(repo);

        // Stage a slice that introduces a directory where the original commit has a file.
        fs::remove_file(repo.join("conflict")).or_abort();
        fs::create_dir_all(repo.join("conflict")).or_abort();
        write_file(repo, "conflict/nested.txt", "ours\n");
        git(repo, &["add", "--all"]);

        run_git_factor(
            repo,
            &["--continue", "--message", "test: dir conflict"],
            GitFactorExpectation::default()
                .git_output(&["ls-files", "--others", "--exclude-standard"], "conflict"),
        );
    }

    #[test]
    fn continue_reports_rehydrate_conflicts_when_exec_gate_fails() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "base\n", "chore: base");
        commit_file(repo, "conflict", "theirs\n", "feat: add conflict file");
        let commit_short_sha = git(repo, &["rev-parse", "--short", "HEAD"]);

        // Start session, then force exec failure so rehydrate runs.
        start_session(repo);
        overwrite_session_exec(repo, "false");

        fs::remove_file(repo.join("conflict")).or_abort();
        fs::create_dir_all(repo.join("conflict")).or_abort();
        write_file(repo, "conflict/nested.txt", "ours\n");
        git(repo, &["add", "--all"]);

        run_git_factor(
            repo,
            &["--continue", "--message", "test: slice"],
            GitFactorExpectation::default()
                .code(EXIT_SOFTWARE)
                .stderr_suffix(format!(
                    "git command failed: rehydrate cherry-pick left conflicts:\nconflict~{commit_short_sha} (feat: add conflict file)\n"
                )),
        );
    }

    #[test]
    fn continue_reports_rehydrate_quit_failure() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "base\n", "chore: base");
        commit_file(repo, "file.txt", "base\nchange\n", "feat: change");

        // Wrapper that fails only for `git cherry-pick --quit`.
        let (wrap_dir, wrap_bin) = make_git_wrapper_named(
            "git",
            r#"if [ "${1-}" = "cherry-pick" ] && [ "${2-}" = "--quit" ]; then
  exit 1
fi
"#,
        );
        let _keep_alive = wrap_dir;

        let wrapped_path = format!("{}:{}", wrap_bin.display(), env::var("PATH").or_abort());
        run_git_factor_with_env(
            repo,
            &["--exec", "true", "HEAD"],
            GitFactorExpectation::default(),
            "PATH",
            wrapped_path.clone(),
        );
        overwrite_session_exec(repo, "false");

        // Stage a slice so --continue proceeds into rehydrate.
        write_file(repo, "file.txt", "base\nslice\n");
        git(repo, &["add", "file.txt"]);

        run_git_factor_with_env(
            repo,
            &["--continue", "--message", "test: slice"],
            GitFactorExpectation::default()
                .code(EXIT_SOFTWARE)
                .stderr_suffix("git command failed: git cherry-pick --quit failed (exit 1)\n"),
            "PATH",
            wrapped_path,
        );
    }

    #[test]
    fn continue_reports_rehydrate_read_tree_failure() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "base\n", "chore: base");
        commit_file(repo, "file.txt", "base\nchange\n", "feat: change");

        // Wrapper that fails only for `git read-tree`.
        let (wrap_dir, wrap_bin) = make_git_wrapper_named(
            "git",
            r#"if [ "${1-}" = "read-tree" ]; then
  exit 1
fi
"#,
        );
        let _keep_alive = wrap_dir;

        let wrapped_path = format!("{}:{}", wrap_bin.display(), env::var("PATH").or_abort());
        run_git_factor_with_env(
            repo,
            &["--exec", "true", "HEAD"],
            GitFactorExpectation::default(),
            "PATH",
            wrapped_path.clone(),
        );
        overwrite_session_exec(repo, "false");

        // Stage a slice so --continue proceeds into rehydrate.
        write_file(repo, "file.txt", "base\nslice\n");
        git(repo, &["add", "file.txt"]);

        run_git_factor_with_env(
            repo,
            &["--continue", "--message", "test: slice"],
            GitFactorExpectation::default()
                .code(EXIT_SOFTWARE)
                .stderr("git command failed: git read-tree failed (exit 1)\n"),
            "PATH",
            wrapped_path,
        );
    }

    #[test]
    fn continue_handles_rehydrate_cherry_pick_failure_without_conflicts() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "base\n", "chore: base");
        commit_file(repo, "file.txt", "base\nchange\n", "feat: change");

        start_session(repo);
        overwrite_session_exec(repo, "false");

        // Stage a slice so --continue proceeds into rehydrate.
        write_file(repo, "file.txt", "base\nslice\n");
        git(repo, &["add", "file.txt"]);

        // Wrapper: make `git cherry-pick --no-commit` fail, but report no unmerged files.
        let (wrap_dir, wrap_bin) = make_git_wrapper_named(
            "git",
            r#"if [ "${1-}" = "cherry-pick" ] && [ "${2-}" = "--no-commit" ]; then
  exit 1
fi
if [ "${1-}" = "diff" ] && [ "${2-}" = "--name-only" ] && [ "${3-}" = "--diff-filter=U" ]; then
  exit 0
fi
"#,
        );
        let _keep_alive = wrap_dir;

        run_git_factor_with_env(
            repo,
            &["--continue", "--message", "test: slice"],
            GitFactorExpectation::default().code(EXIT_SOFTWARE),
            "PATH",
            format!("{}:{}", wrap_bin.display(), env::var("PATH").or_abort()),
        );
    }

    #[test]
    fn finish_reports_restore_failure_from_wrapper_injection() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");
        commit_file(repo, "file.txt", "one\ntwo\n", "feat: change");

        start_session(repo);

        // Wrapper: fail `git restore` so finish surfaces a restore failure.
        let (wrap_dir, wrap_bin) = make_git_wrapper_named(
            "git",
            r#"if [ "${1-}" = "restore" ] && [ "${2-}" = "--source" ]; then
  exit 1
fi
if [ "${1-}" = "cherry-pick" ] && [ "${2-}" = "--quit" ]; then
  exit 0
fi
"#,
        );
        let _keep_alive = wrap_dir;

        run_git_factor_with_env(
            repo,
            &["--finish", "--message", "test: finish"],
            GitFactorExpectation::default()
                .code(EXIT_SOFTWARE)
                .stderr("git command failed: git restore failed (exit 1)\n"),
            "PATH",
            format!("{}:{}", wrap_bin.display(), env::var("PATH").or_abort()),
        );
    }

    #[test]
    fn finish_accepts_multi_paragraph_messages() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");
        commit_file(repo, "file.txt", "one\ntwo\n", "feat: change");

        start_session(repo);
        git(repo, &["add", "--all"]);

        run_git_factor(
            repo,
            &[
                "--finish",
                "--message",
                "test: first paragraph",
                "--message",
                "second paragraph",
            ],
            GitFactorExpectation::default(),
        );
    }

    #[test]
    fn finish_reports_restore_failure_before_message_resolution() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");

        // Create an "empty message" commit we can later finish.
        git(
            repo,
            &[
                "commit",
                "--allow-empty",
                "--allow-empty-message",
                "--message",
                "",
            ],
        );

        // Wrapper: force restore to fail so finish exits before message lookup.
        let (wrap_dir, wrap_bin) = make_git_wrapper_named(
            "git",
            r#"if [ "${1-}" = "restore" ] && [ "${2-}" = "--source" ]; then
  exit 1
fi
"#,
        );
        let _keep_alive = wrap_dir;

        let wrapped_path = format!("{}:{}", wrap_bin.display(), env::var("PATH").or_abort());
        run_git_factor_with_env(
            repo,
            &["--exec", "true", "HEAD"],
            GitFactorExpectation::default(),
            "PATH",
            wrapped_path.clone(),
        );

        // Finish with the wrapper-enabled PATH so restore failure triggers.
        run_git_factor_with_env(
            repo,
            &["--finish"],
            GitFactorExpectation::default()
                .code(EXIT_SOFTWARE)
                .stderr("git command failed: git restore failed (exit 1)\n"),
            "PATH",
            wrapped_path,
        );
    }

    #[test]
    fn finish_rejects_empty_original_message_when_no_message_provided() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");
        git(
            repo,
            &[
                "commit",
                "--allow-empty",
                "--allow-empty-message",
                "--message",
                "",
            ],
        );

        start_session(repo);

        // Stage everything and attempt to finish with no message; tool should
        // refuse to reuse an empty original message.
        git(repo, &["add", "--all"]);
        run_git_factor(
            repo,
            &["--finish"],
            GitFactorExpectation::default()
                .code(EXIT_SOFTWARE)
                .stderr("git command failed: original commit has empty message\n"),
        );
    }

    #[test]
    fn finish_reports_tree_hash_mismatch_via_write_tree_wrapper() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");
        commit_file(repo, "file.txt", "one\ntwo\n", "feat: change");

        // Wrapper: force `git write-tree` (used for actual tree) to return a bogus value.
        let (wrap_dir, wrap_bin) = make_git_wrapper_named(
            "git",
            r#"if [ "${1-}" = "write-tree" ]; then
  echo "0000000000000000000000000000000000000000"
  exit 0
fi
"#,
        );
        let _keep_alive = wrap_dir;

        let wrapped_path = format!("{}:{}", wrap_bin.display(), env::var("PATH").or_abort());
        run_git_factor_with_env(
            repo,
            &["--exec", "true", "HEAD"],
            GitFactorExpectation::default(),
            "PATH",
            wrapped_path.clone(),
        );
        let original_commit = fs::read_to_string(git_dir(repo).join("factor/commits"))
            .or_abort()
            .lines()
            .next()
            .or_abort()
            .to_owned();
        let expected_tree = git(repo, &["rev-parse", &format!("{original_commit}^{{tree}}")]);
        let actual_tree = "0000000000000000000000000000000000000000";

        run_git_factor_with_env(
            repo,
            &["--finish", "--message", "test: finish"],
            GitFactorExpectation::default()
                .code(EXIT_TEMPFAIL)
                .stderr(format!(
                    "tree hash mismatch: expected {expected_tree}, got {actual_tree}\n"
                )),
            "PATH",
            wrapped_path,
        );
    }

    #[test]
    fn continue_reports_error_when_expected_tree_lookup_fails() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "base\n", "chore: base");
        commit_file(repo, "file.txt", "base\nchange\n", "feat: change");

        start_session(repo);
        write_file(repo, "file.txt", "base\nslice\n");
        git(repo, &["add", "file.txt"]);

        let (wrap_dir, wrap_bin) = make_git_wrapper_named(
            "git",
            r#"if [ "${1-}" = "rev-parse" ]; then
  case "${2-}" in
    *"^{tree}")
      echo "fatal: expected tree failed" >&2
      exit 1
      ;;
  esac
fi
"#,
        );
        let _keep_alive = wrap_dir;

        run_git_factor_with_env(
            repo,
            &["--continue", "--message", "test: slice"],
            GitFactorExpectation::default()
                .code(EXIT_SOFTWARE)
                .stderr("git command failed: fatal: expected tree failed\n"),
            "PATH",
            format!("{}:{}", wrap_bin.display(), env::var("PATH").or_abort()),
        );
    }

    #[test]
    fn continue_uses_expected_tree_state_without_original_tree_lookup() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "base\n", "chore: base");
        commit_file(repo, "file.txt", "base\na\nb\n", "feat: change");

        start_session(repo);
        write_file(repo, "file.txt", "base\na\n");
        git(repo, &["add", "file.txt"]);

        let (wrap_dir, wrap_bin) = make_git_wrapper_named(
            "git",
            r#"if [ "${1-}" = "rev-parse" ]; then
  case "${2-}" in
    HEAD^{tree}) ;;
    *"^{tree}")
      echo "fatal: unexpected original tree lookup" >&2
      exit 1
      ;;
  esac
fi
"#,
        );
        let _keep_alive = wrap_dir;

        run_git_factor_with_env(
            repo,
            &["--continue", "--message", "test: slice"],
            GitFactorExpectation::default(),
            "PATH",
            format!("{}:{}", wrap_bin.display(), env::var("PATH").or_abort()),
        );
    }

    #[test]
    fn continue_reports_error_when_actual_tree_lookup_fails() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "base\n", "chore: base");
        commit_file(repo, "file.txt", "base\nchange\n", "feat: change");

        start_session(repo);
        write_file(repo, "file.txt", "base\nslice\n");
        git(repo, &["add", "file.txt"]);

        let (wrap_dir, wrap_bin) = make_git_wrapper_named(
            "git",
            r#"if [ "${1-}" = "rev-parse" ] && [ "${2-}" = "HEAD^{tree}" ]; then
  echo "fatal: actual tree failed" >&2
  exit 1
fi
"#,
        );
        let _keep_alive = wrap_dir;

        run_git_factor_with_env(
            repo,
            &["--continue", "--message", "test: slice"],
            GitFactorExpectation::default()
                .code(EXIT_SOFTWARE)
                .stderr("git command failed: fatal: actual tree failed\n"),
            "PATH",
            format!("{}:{}", wrap_bin.display(), env::var("PATH").or_abort()),
        );
    }

    #[test]
    fn continue_reports_tree_hash_mismatch_after_restore() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "base\n", "chore: base");
        commit_file(repo, "file.txt", "base\nchange\n", "feat: change");

        start_session(repo);
        write_file(repo, "file.txt", "base\nslice\n");
        git(repo, &["add", "file.txt"]);

        let (wrap_dir, wrap_bin) = make_git_wrapper_named(
            "git",
            r#"if [ "${1-}" = "write-tree" ]; then
  echo "0000000000000000000000000000000000000000"
  exit 0
fi
"#,
        );
        let _keep_alive = wrap_dir;

        let expected_tree = fs::read_to_string(git_dir(repo).join("factor/expected_tree"))
            .or_abort()
            .trim()
            .to_owned();
        let actual_tree = "0000000000000000000000000000000000000000";

        run_git_factor_with_env(
            repo,
            &["--continue", "--message", "test: partial split"],
            GitFactorExpectation::default()
                .code(EXIT_TEMPFAIL)
                .stderr(format!(
                    "tree hash mismatch: expected {expected_tree}, got {actual_tree}\n"
                )),
            "PATH",
            format!("{}:{}", wrap_bin.display(), env::var("PATH").or_abort()),
        );
    }

    #[test]
    fn finish_reports_error_when_expected_tree_lookup_fails() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");
        commit_file(repo, "file.txt", "one\ntwo\n", "feat: change");

        start_session(repo);
        fs::remove_file(git_dir(repo).join("factor/expected_tree")).or_abort();

        let (wrap_dir, wrap_bin) = make_git_wrapper_named(
            "git",
            r#"if [ "${1-}" = "rev-parse" ]; then
  case "${2-}" in
    *"^{tree}")
      echo "fatal: expected tree failed in finish" >&2
      exit 1
      ;;
  esac
fi
"#,
        );
        let _keep_alive = wrap_dir;

        run_git_factor_with_env(
            repo,
            &["--finish", "--message", "test: finish"],
            GitFactorExpectation::default()
                .code(EXIT_SOFTWARE)
                .stderr("git command failed: fatal: expected tree failed in finish\n"),
            "PATH",
            format!("{}:{}", wrap_bin.display(), env::var("PATH").or_abort()),
        );
    }

    #[test]
    fn abort_does_not_attempt_git_rebase_abort_for_external_rebase() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");
        start_session(repo);
        fs::create_dir_all(git_dir(repo).join("rebase-merge")).or_abort();

        // If git-factor calls `git rebase --abort`, this wrapper forces a failure.
        let (wrap_dir, wrap_bin) = make_git_wrapper_named(
            "git",
            r#"if [ "${1-}" = "rebase" ] && [ "${2-}" = "--abort" ]; then
  exit 1
fi
"#,
        );
        let _keep_alive = wrap_dir;

        run_git_factor_with_env(
            repo,
            &["--abort"],
            GitFactorExpectation::default()
                .stdout_suffix(
                    "FACTOR: Rebase still active. To abort full rebase, run: git rebase --abort\n",
                )
                .rebase_merge_exists(true)
                .factor_state_exists(false),
            "PATH",
            format!("{}:{}", wrap_bin.display(), env::var("PATH").or_abort()),
        );
    }

    #[test]
    fn start_rejects_dirty_worktree_with_actionable_error() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");
        write_file(repo, "file.txt", "one\ndirty\n");

        run_git_factor(
            repo,
            &["--exec", "true"],
            GitFactorExpectation::default().code(EXIT_SOFTWARE).stderr(
                "git command failed: working tree must be clean before starting; stash, commit, or remove local changes\nSTATUS:\n M file.txt\n",
            ),
        );
    }

    #[test]
    fn abort_falls_back_to_current_commit_when_start_head_state_is_missing() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");
        commit_file(repo, "file.txt", "one\ntwo\n", "feat: change");

        start_session(repo);
        fs::remove_file(git_dir(repo).join("factor/start_head")).or_abort();

        run_git_factor(
            repo,
            &["--abort"],
            GitFactorExpectation::default()
                .stdout("FACTOR: Session aborted for current commit step.\n")
                .factor_state_exists(false),
        );
    }

    #[test]
    fn continue_reports_truncated_commit_metadata_from_git_show() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");
        commit_file(repo, "file.txt", "one\ntwo\n", "feat: change");
        let original_commit = git(repo, &["rev-parse", "HEAD"]);

        start_session(repo);
        git(repo, &["add", "--all"]);

        let (wrap_dir, wrap_bin) = make_git_wrapper_named(
            "git",
            r#"if [ "${1-}" = "show" ] && [ "${2-}" = "--format=%an%x00%ae%x00%aI%x00%cn%x00%ce%x00%cI" ] && [ "${3-}" = "--no-patch" ]; then
  printf "only-one-field\n"
  exit 0
fi
"#,
        );
        let _keep_alive = wrap_dir;

        run_git_factor_with_env(
            repo,
            &["--continue", "--message", "test: split"],
            GitFactorExpectation::default().code(EXIT_SOFTWARE).stderr(format!(
                "git command failed: truncated commit metadata: expected 6 fields, got 1 for {original_commit}\n"
            )),
            "PATH",
            format!("{}:{}", wrap_bin.display(), env::var("PATH").or_abort()),
        );
    }

    #[test]
    fn start_removes_state_path_when_rebase_failure_rewrites_state_dir_as_file() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");
        commit_file(repo, "file.txt", "one\ntwo\n", "feat: change");

        let (wrap_dir, wrap_bin) = make_git_wrapper_named(
            "git",
            r#"if [ "${1-}" = "rebase" ]; then
  rm -rf .git/factor
  : > .git/factor
  exit 1
fi
"#,
        );
        let _keep_alive = wrap_dir;

        run_git_factor_with_env(
            repo,
            &["--exec", "true", "HEAD~1"],
            GitFactorExpectation::default()
                .code(EXIT_SOFTWARE)
                .stderr_suffix("git command failed: git rebase failed (exit 1)\n")
                .factor_state_exists(false)
                .path_exists(".git/factor", false),
            "PATH",
            format!("{}:{}", wrap_bin.display(), env::var("PATH").or_abort()),
        );
    }

    #[test]
    fn abort_removes_state_path_when_clean_rewrites_state_dir_as_file() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");
        commit_file(repo, "file.txt", "one\ntwo\n", "feat: change");
        start_session(repo);

        let (wrap_dir, wrap_bin) = make_git_wrapper_named(
            "git",
            r#"if [ "${1-}" = "clean" ]; then
  rm -rf .git/factor
  : > .git/factor
fi
"#,
        );
        let _keep_alive = wrap_dir;

        run_git_factor_with_env(
            repo,
            &["--abort"],
            GitFactorExpectation::default()
                .stdout("FACTOR: Session aborted for current commit step.\n")
                .factor_state_exists(false)
                .path_exists(".git/factor", false),
            "PATH",
            format!("{}:{}", wrap_bin.display(), env::var("PATH").or_abort()),
        );
    }

    #[test]
    fn rejects_start_when_git_returns_non_40_char_tree_hash() {
        let dir = init_repo();
        let repo = dir.path();
        commit_file(repo, "file.txt", "one\n", "chore: base");

        let (wrap_dir, wrap_bin) = make_git_wrapper_named(
            "git",
            r#"if [ "${1-}" = "rev-parse" ] && [ "${2-}" = "HEAD^{tree}" ]; then
  printf "abc\n"
  exit 0
fi
"#,
        );

        let original_path = env::var_os("PATH").or_abort();
        let prefixed_path = {
            let mut joined = OsString::new();
            joined.push(wrap_bin.as_os_str());
            joined.push(OsStr::new(":"));
            joined.push(original_path);
            joined
        };

        let _keep_alive = wrap_dir;
        run_git_factor_with_prefixed_path(
            repo,
            &["--exec", "true", "HEAD"],
            GitFactorExpectation::default()
                .code(EXIT_SOFTWARE)
                .stderr("git command failed: invalid tree hash: 'abc'\n"),
            prefixed_path,
        );
    }

    #[test]
    fn continue_removes_state_path_when_completion_rewrites_state_dir_as_file() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "base.txt", "base\n", "chore: base");
        commit_file(repo, "base.txt", "base\na\n", "feat: a");
        commit_file(repo, "base.txt", "base\na\nb\n", "feat: b");

        run_git_factor(
            repo,
            &["--exec", "true", "HEAD~1", "HEAD"],
            GitFactorExpectation::default(),
        );

        git(repo, &["add", "--all"]);
        run_git_factor(
            repo,
            &["--continue", "--message", "test: first"],
            GitFactorExpectation::default().stdout_suffix("RECOVERY: git factor --abort\n"),
        );

        git(repo, &["add", "--all"]);
        let (wrap_dir, wrap_bin) = make_git_wrapper_named(
            "git",
            r#"if [ "${1-}" = "rebase" ] && [ "${2-}" = "--continue" ]; then
  rm -rf .git/factor
  : > .git/factor
fi
"#,
        );
        let _keep_alive = wrap_dir;

        run_git_factor_with_env(
            repo,
            &["--continue", "--message", "test: second"],
            GitFactorExpectation::default()
                .stdout_suffix(expected_completion_stdout_suffix(1))
                .factor_state_exists(false)
                .path_exists(".git/factor", false),
            "PATH",
            format!("{}:{}", wrap_bin.display(), env::var("PATH").or_abort()),
        );
    }

    #[test]
    fn continue_reports_rebase_continue_failure_with_recovery_hint() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "base.txt", "base\n", "chore: base");
        commit_file(repo, "base.txt", "base\na\n", "feat: a");
        commit_file(repo, "base.txt", "base\na\nb\n", "feat: b");

        run_git_factor(
            repo,
            &["--exec", "true", "HEAD~1", "HEAD"],
            GitFactorExpectation::default(),
        );

        git(repo, &["add", "--all"]);
        run_git_factor(
            repo,
            &["--continue", "--message", "test: first"],
            GitFactorExpectation::default().stdout_suffix("RECOVERY: git factor --abort\n"),
        );

        git(repo, &["add", "--all"]);
        let (wrap_dir, wrap_bin) = make_git_wrapper_named(
            "git",
            r#"if [ "${1-}" = "rebase" ] && [ "${2-}" = "--continue" ]; then
  exit 1
fi
"#,
        );
        let _keep_alive = wrap_dir;

        run_git_factor_with_env(
            repo,
            &["--continue", "--message", "test: second"],
            GitFactorExpectation::default().code(EXIT_SOFTWARE).stderr(
                "git command failed: git command failed: git rebase failed (exit 1)\n\nResolve the rebase issue, then rerun 'git rebase --continue'.\nTo abandon the factor session, run 'git factor --abort'\n",
            ),
            "PATH",
            format!("{}:{}", wrap_bin.display(), env::var("PATH").or_abort()),
        );
    }

    #[test]
    fn continue_materializes_staged_deletions_into_worktree() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");
        write_file(repo, "remove.txt", "remove me\n");
        git(repo, &["add", "remove.txt"]);
        git(repo, &["commit", "-m", "chore: add removable file"]);

        write_file(repo, "file.txt", "one\ntwo\n");
        git(repo, &["add", "file.txt"]);
        git(repo, &["rm", "--quiet", "remove.txt"]);
        git(repo, &["commit", "-m", "feat: update and remove"]);

        start_session(repo);
        git(repo, &["add", "--all"]);

        run_git_factor(
            repo,
            &["--continue", "--message", "test: split with delete"],
            GitFactorExpectation::default(),
        );

        assert!(
            !repo.join("remove.txt").exists(),
            "staged deletion should be materialized into the worktree"
        );
    }

    #[test]
    fn continue_materializes_deleted_paths_reported_by_git_diff() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");
        commit_file(repo, "file.txt", "one\ntwo\n", "feat: change");

        start_session(repo);
        git(repo, &["add", "--all"]);
        write_file(repo, "deleted-path.txt", "ephemeral\n");

        let (wrap_dir, wrap_bin) = make_git_wrapper_named(
            "git",
            r#"if [ "${1-}" = "diff" ] && [ "${2-}" = "--diff-filter=D" ] && [ "${3-}" = "--name-only" ] && [ "${4-}" = "--staged" ]; then
  printf "deleted-path.txt\n \n"
  exit 0
fi
"#,
        );
        let _keep_alive = wrap_dir;

        run_git_factor_with_env(
            repo,
            &[
                "--continue",
                "--message",
                "test: split with forced delete list",
            ],
            GitFactorExpectation::default(),
            "PATH",
            format!("{}:{}", wrap_bin.display(), env::var("PATH").or_abort()),
        );

        assert!(
            !repo.join("deleted-path.txt").exists(),
            "materialization should remove paths returned by git diff --diff-filter=D"
        );
    }

    #[test]
    fn abort_skips_rebase_abort_when_started_rebase_is_true_without_mid_rebase() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");
        commit_file(repo, "file.txt", "one\ntwo\n", "feat: change");
        start_session(repo);

        let state_dir = git_dir(repo).join("factor");
        fs::write(state_dir.join("started_rebase"), "true\n").or_abort();
        fs::write(state_dir.join("requires_rebase"), "true\n").or_abort();

        let (wrap_dir, wrap_bin) = make_git_wrapper_named(
            "git",
            r#"if [ "${1-}" = "rebase" ] && [ "${2-}" = "--abort" ]; then
  exit 1
fi
"#,
        );
        let _keep_alive = wrap_dir;

        run_git_factor_with_env(
            repo,
            &["--abort"],
            GitFactorExpectation::default()
                .stdout("FACTOR: Session aborted for current commit step.\n")
                .factor_state_exists(false),
            "PATH",
            format!("{}:{}", wrap_bin.display(), env::var("PATH").or_abort()),
        );
    }

    #[test]
    fn start_ignores_invalid_rev_list_lines_in_range_expansion() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "base.txt", "base\n", "feat: base");
        commit_file(repo, "next.txt", "next\n", "feat: next");

        let (wrap_dir, wrap_bin) = make_git_wrapper_named(
            "git",
            r#"if [ "${1-}" = "rev-list" ] && [ "${2-}" = "HEAD~1..HEAD" ]; then
  /usr/bin/git "$@"
  printf '%s\n' 'not-a-commit-sha'
  exit 0
fi
"#,
        );
        let _keep_alive = wrap_dir;

        run_git_factor_with_env(
            repo,
            &["--exec", "true", "HEAD~1..HEAD"],
            GitFactorExpectation::default().factor_state_exists(true),
            "PATH",
            format!("{}:{}", wrap_bin.display(), env::var("PATH").or_abort()),
        );
    }

    #[test]
    fn start_rejects_invalid_range_ref() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "base.txt", "base\n", "feat: base");
        commit_file(repo, "next.txt", "next\n", "feat: next");

        run_git_factor(
            repo,
            &["--exec", "true", "bad..range"],
            GitFactorExpectation::default()
                .code(EXIT_DATAERR)
                .stderr("invalid commit: bad..range\n"),
        );
    }

    #[test]
    fn abort_propagates_git_dir_lookup_failure_after_active_check() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");
        commit_file(repo, "file.txt", "one\ntwo\n", "feat: change");
        start_session(repo);

        let (wrap_dir, wrap_bin) = make_git_wrapper_named(
            "git",
            r#"if [ "${1-}" = "rev-parse" ] && [ "${2-}" = "--git-dir" ]; then
  count_file="$(dirname "$0")/.git_dir_count"
  count=0
  if [ -f "$count_file" ]; then
    count="$(cat "$count_file")"
  fi
  count="$((count + 1))"
  printf "%s" "$count" > "$count_file"
  if [ "$count" -eq 2 ]; then
    echo "mock git-dir failure" >&2
    exit 1
  fi
fi
"#,
        );
        let _keep_alive = wrap_dir;

        run_git_factor_with_env(
            repo,
            &["--abort"],
            GitFactorExpectation::default()
                .code(EXIT_DATAERR)
                .stderr("not a git repository\n"),
            "PATH",
            format!("{}:{}", wrap_bin.display(), env::var("PATH").or_abort()),
        );
    }

    #[test]
    fn abort_reports_unreadable_requires_rebase_state() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");
        commit_file(repo, "file.txt", "one\ntwo\n", "feat: change");
        start_session(repo);

        let unreadable_path = git_dir(repo).join("factor/requires_rebase");
        fs::remove_file(&unreadable_path).or_abort();
        fs::create_dir_all(&unreadable_path).or_abort();

        run_git_factor(
            repo,
            &["--abort"],
            GitFactorExpectation::default()
                .code(EXIT_SOFTWARE)
                .stderr("failed to read state: Is a directory (os error 21)\n"),
        );
    }

    #[test]
    fn abort_reports_unreadable_started_rebase_state() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");
        commit_file(repo, "file.txt", "one\ntwo\n", "feat: change");
        start_session(repo);

        let unreadable_path = git_dir(repo).join("factor/started_rebase");
        fs::remove_file(&unreadable_path).or_abort();
        fs::create_dir_all(&unreadable_path).or_abort();

        run_git_factor(
            repo,
            &["--abort"],
            GitFactorExpectation::default()
                .code(EXIT_SOFTWARE)
                .stderr("failed to read state: Is a directory (os error 21)\n"),
        );
    }

    #[test]
    fn abort_reports_missing_current_commit_when_start_head_is_missing() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");
        commit_file(repo, "file.txt", "one\ntwo\n", "feat: change");
        start_session(repo);

        let state_dir = git_dir(repo).join("factor");
        fs::remove_file(state_dir.join("start_head")).or_abort();
        fs::remove_file(state_dir.join("commits")).or_abort();

        run_git_factor(
            repo,
            &["--abort"],
            GitFactorExpectation::default()
                .code(EXIT_SOFTWARE)
                .stderr("failed to read state: No such file or directory (os error 2)\n"),
        );
    }

    #[test]
    fn abort_reports_reset_failure() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");
        commit_file(repo, "file.txt", "one\ntwo\n", "feat: change");
        start_session(repo);

        let (wrap_dir, wrap_bin) = make_git_wrapper_named(
            "git",
            r#"if [ "${1-}" = "reset" ] && [ "${2-}" = "--hard" ]; then
  exit 1
fi
"#,
        );
        let _keep_alive = wrap_dir;

        run_git_factor_with_env(
            repo,
            &["--abort"],
            GitFactorExpectation::default()
                .code(EXIT_SOFTWARE)
                .stderr("git command failed: git reset failed (exit 1)\n"),
            "PATH",
            format!("{}:{}", wrap_bin.display(), env::var("PATH").or_abort()),
        );
    }

    #[test]
    fn abort_reports_clean_failure() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");
        commit_file(repo, "file.txt", "one\ntwo\n", "feat: change");
        start_session(repo);

        let (wrap_dir, wrap_bin) = make_git_wrapper_named(
            "git",
            r#"if [ "${1-}" = "clean" ] && [ "${2-}" = "--force" ]; then
  exit 1
fi
"#,
        );
        let _keep_alive = wrap_dir;

        run_git_factor_with_env(
            repo,
            &["--abort"],
            GitFactorExpectation::default()
                .code(EXIT_SOFTWARE)
                .stderr("git command failed: git clean failed (exit 1)\n"),
            "PATH",
            format!("{}:{}", wrap_bin.display(), env::var("PATH").or_abort()),
        );
    }

    #[test]
    fn postconditions_helper_accepts_default_expectation() {
        let dir = init_repo();
        assert_git_factor_postconditions(dir.path(), GitFactorExpectation::default());
    }

    #[test]
    fn proptest_run_integration_suite() {
        const MAX_ATTEMPTS: usize = 3;
        for attempt in 1..=MAX_ATTEMPTS {
            let result = catch_unwind(AssertUnwindSafe(|| {
                cli_help_flag_prints_help_to_stdout_and_exits_ok();
                start_requires_exec_command();
                start_requires_exec_command_when_only_message_is_provided();
                start_defaults_to_head_when_commit_is_omitted();
                rejects_default_head_when_repository_has_no_commits();
                start_accepts_root_commit_ref_in_non_head_mode();
                start_runs_with_absolute_git_dir();
                start_reports_error_when_hint_show_toplevel_fails();
                start_reports_missing_git_binary_from_git_output();
                rejects_invalid_commit_ref();
                status_reports_active_session_details();
                status_trace_records_rebase_merge_snapshot_fields();
                status_trace_records_rebase_apply_snapshot_fields();
                start_cleans_state_when_git_rebase_fails();
                continue_completes_single_commit_split_and_preserves_tree();
                rejects_continue_without_message();
                continue_advances_to_next_commit_for_multi_commit_range();
                continue_advances_to_next_commit_for_multiple_explicit_refs();
                continue_preserves_index_and_rehydrates_pool_when_exec_gate_fails();
                continue_materializes_staged_deletions_into_worktree();
                continue_reports_rehydrate_read_tree_failure();
                continue_reports_rehydrate_conflicts_when_exec_gate_fails();
                continue_drops_empty_root_commit_session();
                finish_succeeds_without_rebase_when_target_is_head();
                finish_reuses_original_commit_message_when_none_provided();
                finish_rejects_empty_original_message_when_no_message_provided();
                finish_reports_tree_hash_mismatch_via_write_tree_wrapper();
                finish_ignores_exec_gate_and_completes_session();
                continue_reports_rebase_continue_failure_with_recovery_hint();
                continue_trace_logs_spawn_error_when_bash_is_unavailable();
                start_reports_nonzero_git_output_status_with_stderr_message();
                abort_succeeds_when_session_dir_exists_but_no_rebase_is_active();
                abort_resets_repo_to_pre_start_head();
                abort_propagates_git_dir_lookup_failure_after_active_check();
                start_ignores_invalid_rev_list_lines_in_range_expansion();
            }));

            match result {
                Ok(()) => return,
                Err(payload) => {
                    let transient = match (
                        payload.downcast_ref::<&str>(),
                        payload.downcast_ref::<String>(),
                    ) {
                        (Some(message), _) => {
                            message.contains("retryable spawn error:")
                                || message.contains("Resource temporarily unavailable")
                                || message.contains("os error 11")
                                || message.contains("os error 35")
                        }
                        (None, Some(message)) => {
                            message.contains("retryable spawn error:")
                                || message.contains("Resource temporarily unavailable")
                                || message.contains("os error 11")
                                || message.contains("os error 35")
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

    #[test]
    fn proptest_invalid_flag_reports_usage_and_stderr() {
        let dir = init_repo();
        run_git_factor(
            dir.path(),
            &["--unknown-flag"],
            GitFactorExpectation::default().code(EXIT_USAGE),
        );
    }
}
