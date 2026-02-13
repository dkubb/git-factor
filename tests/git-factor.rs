//! Contract integration tests for `git-factor`.

#![forbid(unsafe_code)]

#[cfg(test)]
#[path = "support/mod.rs"]
mod support;

#[cfg(test)]
mod tests {
    use std::env;
    use std::ffi::{OsStr, OsString};
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    use tempfile::TempDir;

    use super::support::*;

    #[derive(Clone, Debug, Eq, PartialEq)]
    enum StreamExpectation {
        Exact(String),
        Suffix(String),
    }

    impl StreamExpectation {
        #[expect(
            clippy::pattern_type_mismatch,
            reason = "matching enum variants through &self in test-only expectations"
        )]
        fn assert_matches(&self, stream_name: &str, actual: &str) {
            match self {
                Self::Exact(expected_value) => {
                    assert_eq!(
                        actual, expected_value,
                        "{stream_name} mismatch\nexpected:\n{expected_value}\nactual:\n{actual}"
                    );
                }
                Self::Suffix(expected_value) => {
                    assert!(
                        actual.ends_with(expected_value),
                        "{stream_name} suffix mismatch\nexpected suffix:\n{expected_value}\nactual:\n{actual}"
                    );
                }
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
            self.stderr = Some(StreamExpectation::Exact(expected_stderr));
            self
        }

        fn stderr_suffix(mut self, stderr_suffix: impl Into<String>) -> Self {
            let expected_stderr = stderr_suffix.into();
            assert!(
                !expected_stderr.is_empty(),
                "stderr_suffix called with default empty value; omit it from the expectation"
            );
            self.stderr = Some(StreamExpectation::Suffix(expected_stderr));
            self
        }

        fn stdout(mut self, stdout: impl Into<String>) -> Self {
            let expected_stdout = stdout.into();
            assert!(
                !expected_stdout.is_empty(),
                "stdout called with default empty value; omit it from the expectation"
            );
            self.stdout = Some(StreamExpectation::Exact(expected_stdout));
            self
        }

        fn stdout_suffix(mut self, stdout_suffix: impl Into<String>) -> Self {
            let expected_stdout = stdout_suffix.into();
            assert!(
                !expected_stdout.is_empty(),
                "stdout_suffix called with default empty value; omit it from the expectation"
            );
            self.stdout = Some(StreamExpectation::Suffix(expected_stdout));
            self
        }
    }

    impl CommandExpectation for GitFactorExpectation {
        fn expected_code(&self) -> i32 {
            self.code
        }

        fn expected_stderr(&self) -> &str {
            #[expect(
                clippy::pattern_type_mismatch,
                reason = "matching enum variants through Option<&T> in test-only expectation type"
            )]
            if let Some(StreamExpectation::Exact(stderr)) = self.stderr.as_ref() {
                stderr.as_str()
            } else {
                ""
            }
        }

        fn expected_stdout(&self) -> &str {
            #[expect(
                clippy::pattern_type_mismatch,
                reason = "matching enum variants through Option<&T> in test-only expectation type"
            )]
            if let Some(StreamExpectation::Exact(stdout)) = self.stdout.as_ref() {
                stdout.as_str()
            } else {
                ""
            }
        }
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

    fn assert_git_factor_command(command: &mut Command, expected: GitFactorExpectation) {
        let output = command
            .output()
            .expect("git-factor command should start and produce output");
        let code = output.status.code().unwrap_or(EXIT_FAILURE);
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        assert_eq!(
            code, expected.code,
            "exit code mismatch: expected {}, got {}\nstdout:\n{}\nstderr:\n{}",
            expected.code, code, stdout, stderr
        );
        assert_stream_expectation("stdout", &stdout, expected.stdout.as_ref());
        assert_stream_expectation("stderr", &stderr, expected.stderr.as_ref());

        let repo = expected.repo.clone();
        if let Some(repo_path) = repo.as_ref() {
            assert_git_factor_postconditions(repo_path, expected)
                .expect("git-factor postcondition assertions should pass");
        }
    }

    #[expect(
        clippy::too_many_lines,
        reason = "postcondition contract checks intentionally aggregate all shared assertions"
    )]
    fn assert_git_factor_postconditions(
        repo: &Path,
        expected: GitFactorExpectation,
    ) -> Result<(), String> {
        let GitFactorExpectation {
            factor_state_exists,
            git_outputs,
            git_outputs_non_empty,
            git_status_porcelain: expected_git_status_porcelain,
            git_status_porcelain_non_empty,
            head_sha: expected_head_sha,
            path_exists,
            rebase_apply_exists,
            rebase_merge_exists,
            requires_rebase,
            ..
        } = expected;

        if let Some(should_exist) = factor_state_exists {
            let exists = git_dir(repo).join("factor").is_dir();
            if exists != should_exist {
                return Err(format!(
                    "factor state dir existence mismatch: expected {should_exist}, got {exists}"
                ));
            }
        }

        if let Some(expected_status) = expected_git_status_porcelain.as_ref() {
            let actual = git_status_porcelain(repo);
            if actual != *expected_status {
                return Err(format!(
                    "git status --porcelain mismatch:\nexpected:\n{expected_status}\nactual:\n{actual}"
                ));
            }
        }
        if git_status_porcelain_non_empty == Some(true) {
            let actual = git_status_porcelain(repo);
            if actual.is_empty() {
                return Err(
                    "git status --porcelain expected non-empty output, got empty".to_owned(),
                );
            }
        }

        if let Some(expected_head) = expected_head_sha.as_ref() {
            let actual = git(repo, &["rev-parse", "HEAD"]);
            if actual != *expected_head {
                return Err(format!(
                    "HEAD mismatch: expected {expected_head}, got {actual}"
                ));
            }
        }

        for (args, expected_output) in git_outputs {
            let args_refs: Vec<&str> = args.iter().map(String::as_str).collect();
            let actual = git(repo, &args_refs);
            if actual != expected_output {
                return Err(format!(
                    "git output mismatch for {args_refs:?}: expected {expected_output:?}, got {actual:?}"
                ));
            }
        }

        for args in git_outputs_non_empty {
            let args_refs: Vec<&str> = args.iter().map(String::as_str).collect();
            let actual = git(repo, &args_refs);
            if actual.is_empty() {
                return Err(format!(
                    "git output expected non-empty for {args_refs:?}, got empty"
                ));
            }
        }

        for (path, expected_exists) in path_exists {
            let actual_exists = repo.join(path.as_str()).exists();
            if actual_exists != expected_exists {
                return Err(format!(
                    "path existence mismatch for {path:?}: expected {expected_exists}, got {actual_exists}"
                ));
            }
        }

        if let Some(expected_rebase_merge_exists) = rebase_merge_exists {
            let rebase_merge_exists_actual = git_dir(repo).join("rebase-merge").is_dir();
            if rebase_merge_exists_actual != expected_rebase_merge_exists {
                return Err(format!(
                    "rebase-merge existence mismatch: expected {expected_rebase_merge_exists}, got {rebase_merge_exists_actual}",
                ));
            }
        }

        if let Some(expected_rebase_apply_exists) = rebase_apply_exists {
            let rebase_apply_exists_actual = git_dir(repo).join("rebase-apply").is_dir();
            if rebase_apply_exists_actual != expected_rebase_apply_exists {
                return Err(format!(
                    "rebase-apply existence mismatch: expected {expected_rebase_apply_exists}, got {rebase_apply_exists_actual}",
                ));
            }
        }

        if let Some(expected_requires_rebase) = requires_rebase {
            let requires_rebase_path = git_dir(repo).join("factor/requires_rebase");
            let actual = fs::read_to_string(&requires_rebase_path).map_err(|err| {
                format!(
                    "failed to read requires_rebase file `{}`: {err}",
                    requires_rebase_path.display()
                )
            })?;
            let expected_content = if expected_requires_rebase {
                "true\n"
            } else {
                "false\n"
            };
            if actual != expected_content {
                return Err(format!(
                    "requires_rebase mismatch: expected {expected_content:?}, got {actual:?}"
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

    fn expected_single_commit_start_with_untracked_stdout(
        short_sha: &str,
        message: &str,
        untracked: &str,
    ) -> String {
        format!(
            "\
FACTOR: Split session started for {short_sha}.
ORIGINAL MESSAGE: {message}
UNSTAGED:
UNTRACKED:
  {untracked}

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

    fn expected_now_splitting_suffix_with_untracked(
        short_sha: &str,
        original_message: &str,
    ) -> String {
        format!(
            "\
FACTOR: Previous commit split into 1 commits.
FACTOR: Now splitting {short_sha}.
ORIGINAL MESSAGE: {original_message}
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
"
        )
    }

    fn expected_continue_remaining_suffix() -> &'static str {
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
"
    }

    fn expected_single_commit_start_with_reference_and_claude_stdout(
        short_sha: &str,
        message: &str,
        reference_path: &Path,
    ) -> String {
        format!(
            "\
FACTOR: Split session started for {short_sha}.
ORIGINAL MESSAGE: {message}
UNSTAGED:
  file.txt | 1 +
   1 file changed, 1 insertion(+)
UNTRACKED:
  references/rust.md

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
            reference_path.display()
        )
    }

    fn expected_single_commit_start_with_claude_no_reference_stdout(
        short_sha: &str,
        message: &str,
    ) -> String {
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
<claude>
- If context is above 50%, pause and ask the user to /compact.
- Do NOT stop early. Keep committing until \"Complete\".
- Do NOT use git commit directly. ONLY use git-factor --continue.
- Each commit MUST pass the exec gate. No shortcuts.
</claude>
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

Session Control:
      --abort
          Abort the current factor session and restore the repository

      --continue
          Continue by committing the currently staged changes.
          
          Stages must contain changes and the exec gate must pass. After committing, remaining changes are restored as unstaged changes.

      --finish
          Commit all remaining changes and finish the current commit.
          
          Cherry-picks the original commit to restore all remaining changes, verifies the tree hash matches, then runs the exec gate. When no --message is given, reuses the original commit message.

Start Options:
      --exec <COMMAND>
          Shell command(s) to run as a validation gate after each split commit.
          
          Multiple --exec flags are joined with && and also passed to git rebase --exec. The command must have valid bash syntax.

Commit Options:
  -m, --message <MSG>
          Commit message for the split commit.
          
          Required with --continue. Optional with --finish (defaults to the original commit message). Multiple --message flags produce separate paragraphs, matching git commit behavior.

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
    git factor --abort
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
        for entry in &expectation.envs {
            command.env(&entry.0, &entry.1);
        }
        assert_git_factor_command(&mut command, expectation);
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
        fs::write(factor_dir.join("exec"), format!("{exec}\n")).expect("write exec state");
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
            #[expect(
                clippy::literal_string_with_formatting_args,
                reason = "shell script contains ${VAR:-default} expansions"
            )]
            r#"if [ "${1:-}" = "rev-parse" ] && [ "${2:-}" = "--verify" ] && [ "${3:-}" = "definitely-not-a-commit" ]; then
  printf "%040s\n" "g" | tr ' ' 'g'
  exit 0
fi
"#,
        );

        let original_path = env::var_os("PATH").expect("PATH");
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
            #[expect(
                clippy::literal_string_with_formatting_args,
                reason = "shell script contains ${VAR:-default} expansions"
            )]
            r#"if [ "${1:-}" = "rev-parse" ] && [ "${2:-}" = "--verify" ] && [ "${3:-}" = "definitely-not-a-commit" ]; then
  echo "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
  exit 0
fi
"#,
        );

        let original_path = env::var_os("PATH").expect("PATH");
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
        let head_short_sha = git(repo, &["rev-parse", "--short", "HEAD"]);

        run_git_factor(
            repo,
            &["--exec", "true"],
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
            #[expect(
                clippy::literal_string_with_formatting_args,
                reason = "shell script contains ${VAR:-default} expansions"
            )]
            r#"if [ "${1:-}" = "rev-list" ] && [ "${2:-}" = "HEAD~1..HEAD" ]; then
  exit 0
fi
"#,
        );

        let original_path = env::var_os("PATH").expect("PATH");
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
        let dir = TempDir::new().expect("tempdir");
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

        let root_sha = git(repo, &["rev-list", "--max-parents=0", "HEAD"]);

        run_git_factor(
            repo,
            &["--exec", "true", root_sha.as_str()],
            GitFactorExpectation::default()
                .factor_state_exists(true)
                .requires_rebase(true)
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
            #[expect(
                clippy::literal_string_with_formatting_args,
                reason = "shell script contains ${1:-} expansions"
            )]
            r#"if [ "${1:-}" = "rebase" ]; then
  exit 1
fi
"#,
        );
        let _keep_alive = wrap_dir;

        let original_path = env::var_os("PATH").expect("PATH");
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
            #[expect(
                clippy::literal_string_with_formatting_args,
                reason = "shell script contains ${1:-} expansions"
            )]
            r#"if [ "${1:-}" = "show" ] && [ "${2:-}" = "--format=%B" ] && [ "${3:-}" = "--no-patch" ]; then
  echo "mock show failure" >&2
  exit 1
fi
"#,
        );
        let _keep_alive = wrap_dir;

        let original_path = env::var_os("PATH").expect("PATH");
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

        let original_path = env::var_os("PATH").expect("PATH");
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
            #[expect(
                clippy::literal_string_with_formatting_args,
                reason = "shell script contains ${VAR:-default} expansions"
            )]
            r#"if [ "${1:-}" = "diff" ] && [ "${2:-}" = "--stat" ]; then
  count_file="$(dirname "$0")/.diff_stat_count"
  count=0
  if [ -f "$count_file" ]; then
    count="$(cat "$count_file")"
  fi
  count="$((count + 1))"
  printf "%s" "$count" > "$count_file"
  if [ "$count" -ge 2 ]; then
    echo "mock diff stat failure" >&2
    exit 1
  fi
fi
"#,
        );
        let _keep_alive = wrap_dir;

        let original_path = env::var_os("PATH").expect("PATH");
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
            #[expect(
                clippy::literal_string_with_formatting_args,
                reason = "shell script contains ${1:-} expansions"
            )]
            r#"if [ "${1:-}" = "rev-parse" ] && [ "${2:-}" = "--show-toplevel" ]; then
  echo "mock show-toplevel failure" >&2
  exit 1
fi
"#,
        );
        let _keep_alive = wrap_dir;

        let original_path = env::var_os("PATH").expect("PATH");
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
            #[expect(
                clippy::literal_string_with_formatting_args,
                reason = "shell script contains ${1:-} expansions"
            )]
            r#"if [ "${1:-}" = "rev-parse" ] && [ "${2:-}" = "--git-dir" ]; then
  echo "/dev/null"
  exit 0
fi
"#,
        );
        let _keep_alive = wrap_dir;

        let original_path = env::var_os("PATH").expect("PATH");
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
        fs::create_dir_all(&factor_dir).expect("create factor dir");

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
        fs::create_dir_all(&rebase_merge).expect("create rebase-merge dir");

        run_git_factor(
            repo,
            &["--exec", "true", "HEAD"],
            GitFactorExpectation::default()
                .code(EXIT_USAGE)
                .stderr("a rebase is already in progress\n"),
        );
    }

    #[test]
    fn abort_resets_repo_to_current_target_commit() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");
        commit_file(repo, "file.txt", "one\ntwo\n", "feat: change");
        commit_file(repo, "file.txt", "one\ntwo\nthree\n", "feat: latest");
        let target_commit = git(repo, &["rev-parse", "HEAD~1"]);

        run_git_factor(
            repo,
            &["--exec", "true", "HEAD~1"],
            GitFactorExpectation::default().factor_state_exists(true),
        );

        run_git_factor(
            repo,
            &["--abort"],
            GitFactorExpectation::default()
                .head_sha(target_commit)
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
                .factor_state_exists(false),
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
                .stderr("no staged changes to commit\n")
                .git_output(&["diff", "--stat"], diff_before),
        );
    }

    #[test]
    fn continue_reports_error_when_rebase_disappears_after_exec_gate() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");
        commit_file(repo, "file.txt", "one\ntwo\n", "feat: change");

        run_git_factor(
            repo,
            &["--exec", "rm -rf .git/rebase-merge && true", "HEAD~1"],
            GitFactorExpectation::default(),
        );

        git(repo, &["add", "--all"]);
        run_git_factor(
            repo,
            &["--continue", "--message", "test: split"],
            GitFactorExpectation::default()
                .code(EXIT_SOFTWARE)
                .stderr("git command failed: no rebase in progress\n"),
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
                .stdout(expected_continue_remaining_suffix())
                .git_output(&["ls-files", "--others", "--exclude-standard"], "b.txt"),
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
            #[expect(
                clippy::literal_string_with_formatting_args,
                reason = "shell script contains ${1:-} expansions"
            )]
            r#"if [ "${1:-}" = "commit" ]; then
  for arg in "$@"; do
    if [ "$arg" = "--allow-empty" ]; then
      exit 1
    fi
  done
fi
"#,
        );
        let _keep_alive = wrap_dir;

        let original_path = env::var_os("PATH").expect("PATH");
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
    fn finish_exec_failure_returns_tempfail_and_keeps_session_active() {
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
                .code(EXIT_TEMPFAIL)
                .stderr("exec gate failed: false (exit code 1)\n")
                .factor_state_exists(true)
                .git_status_porcelain_non_empty(),
        );
    }

    #[test]
    fn start_invokes_sequence_editor_when_bin_path_has_spaces() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");
        commit_file(repo, "file.txt", "one\ntwo\n", "feat: change");

        let bin_root = TempDir::new().expect("tempdir");
        let spaced = bin_root.path().join("with spaces");
        fs::create_dir_all(&spaced).expect("create spaced dir");
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

        let bin_root = TempDir::new().expect("tempdir");
        let quoted = bin_root.path().join("with'quote");
        fs::create_dir_all(&quoted).expect("create quoted dir");
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
            #[expect(
                clippy::literal_string_with_formatting_args,
                reason = "shell script contains braces like ${1:-}"
            )]
            r#"if [ "${1:-}" = "rebase" ]; then
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
            format!("{}:{}", wrap_bin.display(), env::var("PATH").expect("PATH")),
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
            #[expect(
                clippy::literal_string_with_formatting_args,
                reason = "shell script contains braces like ${1:-}"
            )]
            r#"if [ "${1:-}" = "rebase" ]; then
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
            format!("{}:{}", wrap_bin.display(), env::var("PATH").expect("PATH")),
        );
    }

    #[test]
    fn prints_claude_hints_and_reference_path_when_present() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");
        commit_file(repo, "file.txt", "one\ntwo\n", "feat: change");
        let head_short_sha = git(repo, &["rev-parse", "--short", "HEAD"]);
        fs::create_dir_all(repo.join("references")).expect("create references dir");
        write_file(repo, "references/rust.md", "# rust\n");
        let reference_path = repo
            .join("references/rust.md")
            .canonicalize()
            .expect("canonical reference path");

        run_git_factor_with_env(
            repo,
            &["--exec", "true", "HEAD"],
            GitFactorExpectation::default().stdout(
                expected_single_commit_start_with_reference_and_claude_stdout(
                    head_short_sha.as_str(),
                    "feat: change",
                    reference_path.as_path(),
                ),
            ),
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
            GitFactorExpectation::default().stdout(
                expected_single_commit_start_with_claude_no_reference_stdout(
                    head_short_sha.as_str(),
                    "feat: change",
                ),
            ),
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
        commit_file(repo, "file.txt", "one\ntwo\n", "feat: change");
        commit_file(repo, "file.txt", "one\ntwo\nthree\n", "feat: latest");

        run_git_factor(
            repo,
            &["--exec", "true", "HEAD~1"],
            GitFactorExpectation::default().rebase_merge_exists(true),
        );

        let git_dir_path = git_dir(repo);
        fs::remove_dir_all(git_dir_path.join("rebase-merge")).expect("remove rebase-merge");
        fs::create_dir_all(git_dir_path.join("rebase-apply")).expect("create rebase-apply");

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

        run_git_factor(
            repo,
            &["--abort"],
            GitFactorExpectation::default()
                .stdout_suffix(
                    "FACTOR: Rebase still active. To abort full rebase, run: git rebase --abort\n",
                )
                .factor_state_exists(false),
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
            GitFactorExpectation::default().stdout_suffix(
                expected_now_splitting_suffix_with_untracked(
                    second_short_sha.as_str(),
                    "chore: add new",
                ),
            ),
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
            GitFactorExpectation::default().stdout(
                expected_single_commit_start_with_untracked_stdout(
                    head_short_sha.as_str(),
                    "feat: add file",
                    "new.txt",
                ),
            ),
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
    fn finish_skips_root_rebase_when_root_tree_is_not_empty() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "feat: root");

        start_session(repo);
        git(repo, &["add", "--all"]);

        let (wrap_dir, wrap_bin) = make_git_wrapper_named(
            "git",
            #[expect(
                clippy::literal_string_with_formatting_args,
                reason = "shell script contains ${1:-} expansions"
            )]
            r#"if [ "${1:-}" = "ls-tree" ]; then
  echo "100644 blob deadbeefdeadbeefdeadbeefdeadbeefdeadbeef	file.txt"
  exit 0
fi
if [ "${1:-}" = "rebase" ] && [ "${2:-}" = "--root" ] && [ "${3:-}" = "--interactive" ]; then
  echo "UNEXPECTED_ROOT_REBASE" >&2
  exit 1
fi
"#,
        );
        let _keep_alive = wrap_dir;

        let original_path = env::var_os("PATH").expect("PATH");
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
        fs::create_dir_all(&factor_dir).expect("create factor dir");
        fs::write(
            factor_dir.join("commits"),
            format!("{}\n", git(repo, &["rev-parse", "HEAD"])),
        )
        .expect("write commits");
        fs::write(factor_dir.join("current_index"), "0\n").expect("write current_index");
        fs::write(factor_dir.join("exec"), "true\n").expect("write exec");
        fs::write(factor_dir.join("split_count"), "0\n").expect("write split_count");

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
        fs::create_dir_all(&factor_dir).expect("create factor dir");
        fs::write(
            factor_dir.join("commits"),
            format!("{}\n", git(repo, &["rev-parse", "HEAD"])),
        )
        .expect("write commits");
        fs::write(factor_dir.join("current_index"), "0\n").expect("write current_index");
        fs::write(factor_dir.join("exec"), "true\n").expect("write exec");
        fs::write(factor_dir.join("split_count"), "0\n").expect("write split_count");

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
        fs::create_dir_all(&factor_dir).expect("create factor dir");
        fs::create_dir_all(git_dir(repo).join("rebase-merge")).expect("create rebase-merge");

        // Empty commits file should be rejected.
        fs::write(factor_dir.join("commits"), "\n").expect("write commits");
        fs::write(factor_dir.join("current_index"), "0\n").expect("write current_index");
        fs::write(factor_dir.join("exec"), "true\n").expect("write exec");
        fs::write(factor_dir.join("split_count"), "0\n").expect("write split_count");

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
        .expect("write corrupted requires_rebase state");

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
        .expect("write corrupted requires_rebase state");

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
        fs::remove_file(&unreadable_path).expect("remove requires_rebase state file");
        fs::create_dir_all(&unreadable_path).expect("replace requires_rebase with directory");

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
        fs::remove_file(&unreadable_path).expect("remove requires_rebase state file");
        fs::create_dir_all(&unreadable_path).expect("replace requires_rebase with directory");

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
        fs::create_dir_all(&factor_dir).expect("create factor dir");
        fs::create_dir_all(git_dir(repo).join("rebase-merge")).expect("create rebase-merge");

        fs::write(
            factor_dir.join("commits"),
            format!("{}\n", git(repo, &["rev-parse", "HEAD"])),
        )
        .expect("write commits");
        fs::write(factor_dir.join("current_index"), "not-a-number\n").expect("write current_index");
        fs::write(factor_dir.join("exec"), "true\n").expect("write exec");
        fs::write(factor_dir.join("split_count"), "0\n").expect("write split_count");

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
        fs::create_dir_all(&factor_dir).expect("create factor dir");
        fs::create_dir_all(git_dir(repo).join("rebase-merge")).expect("create rebase-merge");

        fs::write(
            factor_dir.join("commits"),
            format!("{}\n", git(repo, &["rev-parse", "HEAD"])),
        )
        .expect("write commits");
        fs::write(factor_dir.join("current_index"), "1\n").expect("write current_index");
        fs::write(factor_dir.join("exec"), "true\n").expect("write exec");
        fs::write(factor_dir.join("split_count"), "0\n").expect("write split_count");

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
        let bin_dir = TempDir::new().expect("tempdir");
        let (factor, _editor_unused) = copy_bins_to(bin_dir.path());
        let bad_editor = bin_dir.path().join("git-sequence-editor");
        write_executable(&bad_editor, "#!/bin/sh\nexit 1\n");
        let bad_editor_canonical =
            fs::canonicalize(&bad_editor).expect("canonical path for git-sequence-editor wrapper");
        let edited_short_sha = git(repo, &["rev-parse", "--short", "HEAD~1"]);
        let expected_stderr = format!(
            "error: there was a problem with the editor ''{}' '--edit' '{}''\n\
                 git command failed: git rebase failed (exit 1)\n",
            bad_editor_canonical.display(),
            edited_short_sha,
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
            #[expect(
                clippy::literal_string_with_formatting_args,
                reason = "shell script contains braces like ${1:-}"
            )]
            r#"if [ "${1:-}" = "commit" ]; then
  exit 1
fi
"#,
        );

        let original_path = env::var_os("PATH").expect("PATH");
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
            #[expect(
                clippy::literal_string_with_formatting_args,
                reason = "shell script contains braces like ${1:-}"
            )]
            r#"if [ "${1:-}" = "rev-parse" ] && [ "${2:-}" = "--short" ]; then
  echo "fatal: short lookup failed" 1>&2
  exit 42
fi
"#,
        );
        let _keep_alive = wrap_dir;
        let wrapped_path = format!("{}:{}", wrap_bin.display(), env::var("PATH").expect("PATH"));

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
        fs::remove_file(repo.join("conflict")).expect("remove conflict file");
        fs::create_dir_all(repo.join("conflict")).expect("create conflict dir");
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

        fs::remove_file(repo.join("conflict")).expect("remove conflict file");
        fs::create_dir_all(repo.join("conflict")).expect("create conflict dir");
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
            #[expect(
                clippy::literal_string_with_formatting_args,
                reason = "shell script contains braces like ${1:-}"
            )]
            r#"if [ "${1:-}" = "cherry-pick" ] && [ "${2:-}" = "--quit" ]; then
  exit 1
fi
"#,
        );
        let _keep_alive = wrap_dir;

        let wrapped_path = format!("{}:{}", wrap_bin.display(), env::var("PATH").expect("PATH"));
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
                .stderr("git command failed: git cherry-pick --quit failed (exit 1)\n"),
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
            #[expect(
                clippy::literal_string_with_formatting_args,
                reason = "shell script contains braces like ${1:-}"
            )]
            r#"if [ "${1:-}" = "read-tree" ]; then
  exit 1
fi
"#,
        );
        let _keep_alive = wrap_dir;

        let wrapped_path = format!("{}:{}", wrap_bin.display(), env::var("PATH").expect("PATH"));
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
            #[expect(
                clippy::literal_string_with_formatting_args,
                reason = "shell script contains braces like ${1:-}"
            )]
            r#"if [ "${1:-}" = "cherry-pick" ] && [ "${2:-}" = "--no-commit" ]; then
  exit 1
fi
if [ "${1:-}" = "diff" ] && [ "${2:-}" = "--name-only" ] && [ "${3:-}" = "--diff-filter=U" ]; then
  exit 0
fi
"#,
        );
        let _keep_alive = wrap_dir;

        run_git_factor_with_env(
            repo,
            &["--continue", "--message", "test: slice"],
            GitFactorExpectation::default()
                .code(EXIT_TEMPFAIL)
                .stderr("exec gate failed: false (exit code 1)\n"),
            "PATH",
            format!("{}:{}", wrap_bin.display(), env::var("PATH").expect("PATH")),
        );
    }

    #[test]
    fn finish_handles_cherry_pick_failure_without_conflicts() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");
        commit_file(repo, "file.txt", "one\ntwo\n", "feat: change");

        start_session(repo);

        // Wrapper: fail `cherry-pick --no-commit`, report no conflicts, and force
        // `cherry-pick --quit` to "succeed" so cmd_finish continues into tree checks.
        let (wrap_dir, wrap_bin) = make_git_wrapper_named(
            "git",
            #[expect(
                clippy::literal_string_with_formatting_args,
                reason = "shell script contains braces like ${1:-}"
            )]
            r#"if [ "${1:-}" = "cherry-pick" ] && [ "${2:-}" = "--no-commit" ]; then
  exit 1
fi
if [ "${1:-}" = "diff" ] && [ "${2:-}" = "--name-only" ] && [ "${3:-}" = "--diff-filter=U" ]; then
  exit 0
fi
if [ "${1:-}" = "cherry-pick" ] && [ "${2:-}" = "--quit" ]; then
  exit 0
fi
"#,
        );
        let _keep_alive = wrap_dir;
        let original_commit = fs::read_to_string(git_dir(repo).join("factor/commits"))
            .expect("read commits")
            .lines()
            .next()
            .expect("first commit in session state")
            .to_owned();
        let expected_tree = git(repo, &["rev-parse", &format!("{original_commit}^{{tree}}")]);
        let actual_tree = git(repo, &["write-tree"]);

        run_git_factor_with_env(
            repo,
            &["--finish", "--message", "test: finish"],
            GitFactorExpectation::default()
                .code(EXIT_TEMPFAIL)
                .stderr(format!(
                    "tree hash mismatch: expected {expected_tree}, got {actual_tree}\n"
                )),
            "PATH",
            format!("{}:{}", wrap_bin.display(), env::var("PATH").expect("PATH")),
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
    fn finish_reports_conflicts_from_wrapper_injection() {
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

        // Wrapper: force cherry-pick --no-commit to fail, and make the unmerged query
        // return a non-empty list so cmd_finish takes the conflict error path.
        let (wrap_dir, wrap_bin) = make_git_wrapper_named(
            "git",
            #[expect(
                clippy::literal_string_with_formatting_args,
                reason = "shell script contains braces like ${1:-}"
            )]
            r#"if [ "${1:-}" = "cherry-pick" ] && [ "${2:-}" = "--no-commit" ]; then
  exit 1
fi
if [ "${1:-}" = "diff" ] && [ "${2:-}" = "--name-only" ] && [ "${3:-}" = "--diff-filter=U" ]; then
  echo "conflict.txt"
  exit 0
fi
"#,
        );
        let _keep_alive = wrap_dir;

        let wrapped_path = format!("{}:{}", wrap_bin.display(), env::var("PATH").expect("PATH"));
        run_git_factor_with_env(
            repo,
            &["--exec", "true", "HEAD"],
            GitFactorExpectation::default(),
            "PATH",
            wrapped_path.clone(),
        );

        // Finish with the wrapper-enabled PATH so the conflict path triggers.
        run_git_factor_with_env(
            repo,
            &["--finish"],
            GitFactorExpectation::default()
                .code(EXIT_SOFTWARE)
                .stderr("git command failed: final cherry-pick left conflicts:\nconflict.txt\n"),
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
            #[expect(
                clippy::literal_string_with_formatting_args,
                reason = "shell script contains braces like ${1:-}"
            )]
            r#"if [ "${1:-}" = "write-tree" ]; then
  echo "0000000000000000000000000000000000000000"
  exit 0
fi
"#,
        );
        let _keep_alive = wrap_dir;

        let wrapped_path = format!("{}:{}", wrap_bin.display(), env::var("PATH").expect("PATH"));
        run_git_factor_with_env(
            repo,
            &["--exec", "true", "HEAD"],
            GitFactorExpectation::default(),
            "PATH",
            wrapped_path.clone(),
        );
        let original_commit = fs::read_to_string(git_dir(repo).join("factor/commits"))
            .expect("read commits")
            .lines()
            .next()
            .expect("first commit in session state")
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
            #[expect(
                clippy::literal_string_with_formatting_args,
                reason = "shell script contains braces like ${1:-}"
            )]
            r#"if [ "${1:-}" = "rev-parse" ]; then
  case "${2:-}" in
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
            format!("{}:{}", wrap_bin.display(), env::var("PATH").expect("PATH")),
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
            #[expect(
                clippy::literal_string_with_formatting_args,
                reason = "shell script contains braces like ${1:-}"
            )]
            r#"if [ "${1:-}" = "rev-parse" ]; then
  case "${2:-}" in
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
            format!("{}:{}", wrap_bin.display(), env::var("PATH").expect("PATH")),
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
            #[expect(
                clippy::literal_string_with_formatting_args,
                reason = "shell script contains braces like ${1:-}"
            )]
            r#"if [ "${1:-}" = "rev-parse" ] && [ "${2:-}" = "HEAD^{tree}" ]; then
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
            format!("{}:{}", wrap_bin.display(), env::var("PATH").expect("PATH")),
        );
    }

    #[test]
    fn finish_reports_error_when_expected_tree_lookup_fails() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");
        commit_file(repo, "file.txt", "one\ntwo\n", "feat: change");

        start_session(repo);
        fs::remove_file(git_dir(repo).join("factor/expected_tree"))
            .expect("remove expected_tree to force fallback lookup");

        let (wrap_dir, wrap_bin) = make_git_wrapper_named(
            "git",
            #[expect(
                clippy::literal_string_with_formatting_args,
                reason = "shell script contains braces like ${1:-}"
            )]
            r#"if [ "${1:-}" = "rev-parse" ]; then
  case "${2:-}" in
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
            format!("{}:{}", wrap_bin.display(), env::var("PATH").expect("PATH")),
        );
    }

    #[test]
    fn abort_does_not_attempt_git_rebase_abort() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");
        commit_file(repo, "file.txt", "one\ntwo\n", "feat: change");
        commit_file(repo, "file.txt", "one\ntwo\nthree\n", "feat: latest");
        run_git_factor(
            repo,
            &["--exec", "true", "HEAD~1"],
            GitFactorExpectation::default().factor_state_exists(true),
        );

        // If git-factor calls `git rebase --abort`, this wrapper forces a failure.
        let (wrap_dir, wrap_bin) = make_git_wrapper_named(
            "git",
            #[expect(
                clippy::literal_string_with_formatting_args,
                reason = "shell script contains braces like ${1:-}"
            )]
            r#"if [ "${1:-}" = "rebase" ] && [ "${2:-}" = "--abort" ]; then
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
                .factor_state_exists(false),
            "PATH",
            format!("{}:{}", wrap_bin.display(), env::var("PATH").expect("PATH")),
        );
    }
}
