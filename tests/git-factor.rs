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
Unstaged changes after reset:
M\tfile.txt
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
Unstaged changes after reset:
M\tfile.txt
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
Unstaged changes after reset:
M\tfile.txt
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
