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
#[expect(
    clippy::inline_modules,
    reason = "preserve the established inline test layout"
)]
mod tests {
    #[cfg(unix)]
    use core::time::Duration;
    use std::env;
    use std::ffi::{OsStr, OsString};
    use std::fs::{self, DirEntry};
    use std::io;
    use std::panic::resume_unwind;
    use std::path::{Path, PathBuf};
    use std::process::{self, Command};
    #[cfg(unix)]
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

    struct NativeOriginalPool {
        base: String,
        deletion_tree: String,
        directory: TempDir,
        final_tree: String,
        foreign_refs: String,
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

    #[derive(Debug, Eq, PartialEq)]
    struct NativeSelectionSnapshot {
        done: Vec<u8>,
        head: String,
        index: Vec<u8>,
        journal: Vec<u8>,
        refs: String,
        todo: Vec<u8>,
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
            let actual_exists = git_dir(repo_path).join("factor-journal.json").is_file();
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
            let observed = Command::new(git_factor_bin())
                .current_dir(repo_path)
                .arg("--status")
                .output()
                .or_abort();
            assert_eq!(observed.status.code(), Some(EXIT_OK));
            let status: serde_json::Value = serde_json::from_slice(&observed.stdout).or_abort();
            assert_eq!(
                status
                    .pointer("/session/rebase/required")
                    .and_then(serde_json::Value::as_bool),
                Some(expected_requires_rebase),
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

    fn expected_single_commit_start_stdout(commit: &str, short_sha: &str, message: &str) -> String {
        let encoded_commit = serde_json::to_string(commit).or_abort();
        let encoded_short_sha = serde_json::to_string(short_sha).or_abort();
        let encoded_message = serde_json::to_string(message).or_abort();
        format!(
            "{{\"actions\":{{\"abort\":[\"git\",\"factor\",\"--abort\"],\"submit\":[\"git\",\"factor\",\"--continue\",\"--message\",\"<message>\"]}},\"changes\":{{\"unstaged\":[{{\"path\":\"file.txt\",\"kind\":\"text\",\"added\":1,\"deleted\":0}}],\"untracked\":[]}},\"guidance\":[\"Stage one independently valid atomic change.\",\"Use one concrete action in the commit message.\",\"Submit each atom through git factor so its gates run.\"],\"operation\":\"start\",\"references\":[],\"target\":{{\"commit\":{encoded_commit},\"commit_count\":1,\"message\":{encoded_message},\"short_commit\":{encoded_short_sha}}}}}\n"
        )
    }

    fn expected_multi_commit_start_suffix(
        tip_sha: &str,
        tip_short_sha: &str,
        untracked: &str,
    ) -> String {
        let encoded_tip_sha = serde_json::to_string(tip_sha).or_abort();
        let encoded_tip_short_sha = serde_json::to_string(tip_short_sha).or_abort();
        format!(
            "{{\"actions\":{{\"abort\":[\"git\",\"factor\",\"--abort\"],\"submit\":[\"git\",\"factor\",\"--continue\",\"--message\",\"<message>\"]}},\"changes\":{{\"unstaged\":[{{\"path\":\"base.txt\",\"kind\":\"text\",\"added\":2,\"deleted\":0}}],\"untracked\":{untracked}}},\"guidance\":[\"Stage one independently valid atomic change.\",\"Use one concrete action in the commit message.\",\"Submit each atom through git factor so its gates run.\"],\"operation\":\"start\",\"references\":[],\"target\":{{\"commit\":{encoded_tip_sha},\"commit_count\":2,\"message\":\"feat: b\",\"short_commit\":{encoded_tip_short_sha}}}}}\n"
        )
    }

    fn expected_help_stdout() -> &'static str {
        "Split one git commit or contiguous commit span into smaller atomic commits\n\nUsage: git-factor [OPTIONS] [COMMIT]...\n\nArguments:\n  [COMMIT]...\n          Commit or span to split (e.g. SHA, A B, A..B, A^..B).\n          \n          Accepts full or short SHAs, branch names, and git revision syntax. `<rev>` splits one commit. `<start> <end>` splits one inclusive span. `<start>..<end>` uses git's exclusive-start range semantics, and `<start>^..<end>` is the git-native inclusive form. Symmetric diff (`...`) is not supported.\n\nOptions:\n  -v, --version\n          Print version information and exit\n\n  -h, --help\n          Print help (see a summary with '-h')\n\nStart Options:\n      --exec <COMMAND>\n          Shell command(s) to run as the deterministic validation gate.\n          \n          Multiple --exec and --gate flags run individually in supplied order. Passing command/tree proofs are reused. Commands must have valid bash syntax, inspect only the tree, and preserve its bytes and commit metadata.\n\n      --gate <NAME> <COMMAND>\n          Ordered named tree checks, recorded in Gate-<name> trailers.\n          \n          Names start with an ASCII letter and contain ASCII letters, digits, or hyphens. Names are unique without regard to case. Commands obey the same tree-only contract as --exec.\n\nCommit Options:\n  -m, --message <MSG>\n          Commit message for the split commit.\n          \n          Submitting a message validates and captures the staged atom. Without a message, --continue resumes replay. Optional with --finish (defaults to the original selected tip message). Multiple --message flags produce separate paragraphs, matching git commit behavior.\n\nSession Control:\n      --continue\n          Resume replay and recovery, or submit staged changes with --message.\n          \n          Without a message, resumes the current replay, captures its completed checkpoint, and opens the remaining change for selection. With a message, validates the staged atom and captures a new checkpoint.\n\n      --finish\n          Validate and capture all remaining changes, then finish the session.\n          \n          Uses the original selected tip's message when --message is omitted.\n\n      --retry\n          Unstage the current candidate in an open selection.\n          \n          Preserves every previously completed checkpoint.\n\n      --abort\n          Abort the current factor session and restore the repository\n\n      --status\n          Show status for the current factor session.\n          \n          Prints session details when active, otherwise reports no active session.\n\nWORKFLOW:\n  1. Start a session:     git factor --gate test 'cargo test' HEAD\n  2. Select an atom:     git add --patch -- <path>\n  3. Capture the atom:   git factor --message 'Add login'\n  4. Repeat steps 2-3 on the automatically exposed remainder.\n  5. Finish remaining:  git factor --finish\n\n  Each successful split finishes its rebase before opening the next one.\n  Gates validate each tree independently of the unstaged remainder.\n  Passing command/tree proofs are reused; commit hooks still validate messages.\n  Gates must be deterministic tree checks and must not depend on commit metadata, history, or messages.\n  Resolve replay conflicts or gate failures, then run git factor --continue.\n  Use --retry only in an open selection to unstage its current candidate.\n  During replay, use --continue or --abort to return to the latest checkpoint.\n  Earlier successful splits remain captured.\n  Legacy sessions must be completed with their originating version.\n\nEXAMPLES:\n  Split a commit using ordered named and legacy gates:\n    git factor --gate test 'cargo test' --exec 'cargo fmt --check' HEAD\n\n  Split an inclusive contiguous span:\n    git factor --gate check 'make check' HEAD~2 HEAD\n\n  Use git-native exclusive-start range syntax:\n    git factor --exec 'npm test' HEAD~3..HEAD\n\n  Use git-native inclusive-start range syntax:\n    git factor --exec 'npm test' HEAD~3^..HEAD\n\n  Submit a multi-paragraph message (also accepts --continue):\n    git factor --message 'Add login' --message 'Support OAuth2 sessions.'\n\n  Resume replay after a conflict or gate failure:\n    git factor --continue\n\n  Finish with the original selected tip's message:\n    git factor --finish\n\n  Show active-session status:\n    git factor --status\n"
    }

    fn expected_completion_stdout_suffix(operation: &str, split_count: u32) -> String {
        format!(
            "{{\"operation\":\"{operation}\",\"result\":\"complete\",\"split_count\":{split_count}}}\n"
        )
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
            Err(err) if err.kind() == io::ErrorKind::WouldBlock => {
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

    fn checkpoint_journal(repo: &Path) -> PathBuf {
        git_dir(repo).join("factor-journal.json")
    }

    fn read_journal(repo: &Path) -> serde_json::Value {
        serde_json::from_slice(&fs::read(checkpoint_journal(repo)).or_abort()).or_abort()
    }

    fn assert_true_gate_message(repo: &Path, commit: &str, body: &str, tree: &str) {
        let command_hash = "f32a5804e292d30bedf68f62d32fb75d87e99fd9";
        assert_eq!(
            git(repo, &["show", "--format=%B", "--no-patch", commit]),
            format!("{body}\n\nGate-exec-{command_hash}:\n {command_hash}\n {tree}"),
        );
    }

    #[expect(
        clippy::single_call_fn,
        reason = "This named abort observer checks exact refs and the independently validated surviving gate proof"
    )]
    fn assert_original_refs_and_true_proof(repo: &Path, original: &str, tree: &str) {
        let command_hash = "f32a5804e292d30bedf68f62d32fb75d87e99fd9";
        let proof_ref = format!("refs/factor/gates/{command_hash}/{tree}");
        let proof = git(repo, &["rev-parse", proof_ref.as_str()]);
        assert_eq!(
            git(repo, &["rev-parse", &format!("{proof}^{{tree}}")]),
            tree
        );
        assert!(
            git(repo, &["show", "--format=%B", "--no-patch", &proof]).ends_with(&format!(
                "Gate-exec-{command_hash}:\n {command_hash}\n {tree}"
            ))
        );
        let mut expected = original.lines().map(str::to_owned).collect::<Vec<_>>();
        expected.push(format!("{proof} {proof_ref}"));
        expected.sort();
        let mut actual = git(repo, &["show-ref"])
            .lines()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        actual.sort();
        assert_eq!(actual, expected);
    }

    fn snapshot_selection(repo: &Path) -> NativeSelectionSnapshot {
        let admin = git_dir(repo);
        NativeSelectionSnapshot {
            head: git(repo, &["rev-parse", "HEAD"]),
            refs: git(repo, &["show-ref"]),
            index: fs::read(admin.join("index")).or_abort(),
            journal: fs::read(checkpoint_journal(repo)).or_abort(),
            done: fs::read(admin.join("rebase-merge/done")).or_abort(),
            todo: fs::read(admin.join("rebase-merge/git-rebase-todo")).or_abort(),
        }
    }

    #[expect(
        clippy::filetype_is_file,
        reason = "The fixture inventory rejects special files before attempting a regular-file read"
    )]
    fn legacy_inventory(directory: &Path) -> Vec<(PathBuf, Option<Vec<u8>>)> {
        let mut entries: Vec<_> = fs::read_dir(directory)
            .or_abort()
            .map(OrAbort::or_abort)
            .collect();
        entries.sort_by_key(DirEntry::path);
        let mut snapshot = Vec::new();
        for entry in entries {
            let kind = entry.file_type().or_abort();
            if kind.is_dir() {
                snapshot.push((entry.path(), None));
                snapshot.extend(legacy_inventory(&entry.path()));
            } else {
                assert!(
                    kind.is_file(),
                    "legacy fixture contains only regular files and directories"
                );
                snapshot.push((entry.path(), Some(fs::read(entry.path()).or_abort())));
            }
        }
        snapshot
    }

    fn verify_legacy_session_refusal(repo: &Path, args: &[&str]) {
        let admin = git_dir(repo);
        let head = git(repo, &["rev-parse", "HEAD"]);
        let refs = git(repo, &["show-ref"]);
        let index = fs::read(admin.join("index")).or_abort();
        let foreign_state = legacy_inventory(&admin.join("factor"));
        let original_file = fs::read(repo.join("file.txt")).or_abort();
        let rebase_paths: Vec<_> = ["rebase-merge", "rebase-apply"]
            .into_iter()
            .map(|name| {
                let path = admin.join(name);
                (
                    path.is_dir(),
                    if path.is_dir() {
                        legacy_inventory(&path)
                    } else {
                        Vec::new()
                    },
                )
            })
            .collect();
        fs::write(repo.join("unrelated.tmp"), b"unrelated legacy user bytes\n").or_abort();
        assert!(!checkpoint_journal(repo).exists());
        run_git_factor(repo, args, GitFactorExpectation::default().code(EXIT_SOFTWARE).stderr(
            "git command failed: existing legacy session must be finished or aborted with its originating version\n"
        ));
        assert_eq!(git(repo, &["rev-parse", "HEAD"]), head);
        assert_eq!(git(repo, &["show-ref"]), refs);
        assert_eq!(fs::read(admin.join("index")).or_abort(), index);
        assert_eq!(legacy_inventory(&admin.join("factor")), foreign_state);
        assert_eq!(fs::read(repo.join("file.txt")).or_abort(), original_file);
        assert_eq!(
            fs::read(repo.join("unrelated.tmp")).or_abort(),
            b"unrelated legacy user bytes\n"
        );
        for (name, before) in ["rebase-merge", "rebase-apply"]
            .into_iter()
            .zip(rebase_paths)
        {
            let path = admin.join(name);
            assert_eq!(path.is_dir(), before.0);
            if before.0 {
                assert_eq!(legacy_inventory(&path), before.1);
            }
        }
        assert!(!checkpoint_journal(repo).exists());
    }

    fn overwrite_session_exec(repo: &Path, exec: &str) {
        let mut journal = read_journal(repo);
        let gates = journal
            .get_mut("gates")
            .and_then(serde_json::Value::as_array_mut)
            .or_abort();
        let command = gates
            .first_mut()
            .and_then(|gate| gate.get_mut("command"))
            .or_abort();
        *command = serde_json::Value::String(exec.to_owned());
        fs::write(
            checkpoint_journal(repo),
            format!("{}\n", serde_json::to_string(&journal).or_abort()),
        )
        .or_abort();
    }

    fn expected_recovery_stdout(operation: &str) -> String {
        format!(
            "{{\"actions\":{{\"amend\":[\"git\",\"commit\",\"--amend\",\"--no-edit\"],\"continue_factor\":[\"git\",\"factor\",\"--continue\"],\"stage\":[\"git\",\"add\",\"<paths>\"]}},\"operation\":\"{operation}\",\"result\":\"recovery_required\"}}\n"
        )
    }

    fn expected_remaining_stdout(unstaged: &str, untracked: &str) -> String {
        format!(
            "{{\"operation\":\"continue\",\"result\":\"committed\",\"actions\":{{\"abort\":[\"git\",\"factor\",\"--abort\"],\"submit\":[\"git\",\"factor\",\"--continue\",\"--message\",\"<message>\"]}},\"changes\":{{\"unstaged\":{unstaged},\"untracked\":{untracked}}},\"guidance\":[\"Stage one independently valid atomic change.\",\"Use one concrete action in the commit message.\",\"Submit each atom through git factor so its gates run.\"],\"references\":[],\"split_count\":1}}\n"
        )
    }

    #[test]
    #[expect(
        clippy::literal_string_with_formatting_args,
        reason = "Git's tree revision syntax is literal input"
    )]
    fn failed_parent_queries_cannot_turn_existing_ancestry_into_a_root() {
        let dir = init_repo();
        let repo = dir.path();
        commit_file(repo, "file.txt", "one\n", "Add initial content");
        commit_file(repo, "file.txt", "two\n", "Change content");
        let head = git(repo, &["rev-parse", "HEAD"]);
        let tree = git(repo, &["rev-parse", "HEAD^{tree}"]);
        let contents = fs::read(repo.join("file.txt")).or_abort();
        let index = fs::read(git_dir(repo).join("index")).or_abort();
        let references = git(repo, &["show-ref"]);
        let (wrapper, bin) = make_git_wrapper_named(
            "git",
            r#"if [ "${1-}" = "rev-parse" ] && [ "${2-}" = "--quiet" ] && [ "${3-}" = "--verify" ]; then
  case "${4-}" in
    *^) printf 'parent query refused\n' >&2; exit 128 ;;
  esac
fi
if [ "${1-}" = "cat-file" ] && [ "${2-}" = "commit" ]; then
  printf 'parent object query refused\n' >&2
  exit 128
fi
"#,
        );
        let mut path = OsString::from(bin.as_os_str());
        path.push(OsStr::new(":"));
        path.push(env::var_os("PATH").or_abort());
        let mut command = Command::new(git_factor_bin());
        command
            .current_dir(repo)
            .args(["--exec", "printf ran > gate-ran", "HEAD"])
            .env("PATH", path)
            .env_remove("CLAUDECODE")
            .env_remove("GIT_FACTOR_TRACE_LOG");

        let output = command.output().or_abort();

        assert_eq!(output.status.code(), Some(EXIT_SOFTWARE));
        assert_eq!(output.stdout.as_slice(), b"");
        assert_eq!(
            output.stderr.as_slice(),
            b"git command failed: parent object query refused\n"
        );
        assert_eq!(git(repo, &["rev-parse", "HEAD"]), head);
        assert_eq!(git(repo, &["rev-parse", "HEAD^{tree}"]), tree);
        assert_eq!(fs::read(repo.join("file.txt")).or_abort(), contents);
        assert_eq!(git(repo, &["show-ref"]), references);
        assert_eq!(fs::read(git_dir(repo).join("index")).or_abort(), index);
        assert!(!repo.join("gate-ran").exists());
        assert!(!git_dir(repo).join("factor").exists());
        assert!(!git_dir(repo).join("rebase-merge").exists());
        drop(wrapper);
    }

    #[test]
    #[expect(
        clippy::literal_string_with_formatting_args,
        reason = "Git's tree revision syntax is literal input"
    )]
    fn failed_later_merge_queries_refuse_before_running_gates() {
        let dir = init_repo();
        let repo = dir.path();
        commit_file(repo, "file.txt", "one\n", "Add initial content");
        let branch = git(repo, &["branch", "--show-current"]);
        git(repo, &["checkout", "-b", "side"]);
        commit_file(repo, "side.txt", "side\n", "Add side content");
        git(repo, &["checkout", branch.trim()]);
        commit_file(repo, "file.txt", "two\n", "Change content");
        let ordinary = git(repo, &["rev-parse", "HEAD"]);
        git(
            repo,
            &["merge", "--no-ff", "-m", "Merge side content", "side"],
        );
        let head = git(repo, &["rev-parse", "HEAD"]);
        let tree = git(repo, &["rev-parse", "HEAD^{tree}"]);
        let contents = fs::read(repo.join("file.txt")).or_abort();
        let side_contents = fs::read(repo.join("side.txt")).or_abort();
        let index = fs::read(git_dir(repo).join("index")).or_abort();
        let references = git(repo, &["show-ref"]);
        let interception = format!(
            r#"if [ "${{1-}}" = "rev-parse" ] && [ "${{2-}}" = "--quiet" ] && [ "${{3-}}" = "--verify" ]; then
  case "${{4-}}" in
    *^2) printf 'merge query refused\n' >&2; exit 128 ;;
  esac
fi
if [ "${{1-}}" = "cat-file" ] && [ "${{2-}}" = "commit" ] && [ "${{3-}}" = "{}" ]; then
  printf 'later merge object query refused\n' >&2
  exit 128
fi
"#,
            head.trim(),
        );
        let (wrapper, bin) = make_git_wrapper_named("git", &interception);
        let mut path = OsString::from(bin.as_os_str());
        path.push(OsStr::new(":"));
        path.push(env::var_os("PATH").or_abort());
        let mut command = Command::new(git_factor_bin());
        command
            .current_dir(repo)
            .args([
                "--exec",
                "printf ran > gate-ran",
                ordinary.trim(),
                head.trim(),
            ])
            .env("PATH", path)
            .env_remove("CLAUDECODE")
            .env_remove("GIT_FACTOR_TRACE_LOG");

        let output = command.output().or_abort();

        assert_eq!(output.status.code(), Some(EXIT_SOFTWARE));
        assert_eq!(output.stdout.as_slice(), b"");
        assert_eq!(
            output.stderr.as_slice(),
            b"git command failed: later merge object query refused\n"
        );
        assert_eq!(git(repo, &["rev-parse", "HEAD"]), head);
        assert_eq!(git(repo, &["rev-parse", "HEAD^{tree}"]), tree);
        assert_eq!(fs::read(repo.join("file.txt")).or_abort(), contents);
        assert_eq!(fs::read(repo.join("side.txt")).or_abort(), side_contents);
        assert_eq!(git(repo, &["show-ref"]), references);
        assert_eq!(fs::read(git_dir(repo).join("index")).or_abort(), index);
        assert!(!repo.join("gate-ran").exists());
        assert!(!git_dir(repo).join("factor").exists());
        assert!(!git_dir(repo).join("rebase-merge").exists());
        drop(wrapper);
    }

    #[test]
    fn rejects_commit_ref_when_git_returns_non_hex_40_char_sha() {
        let dir = init_repo();
        let repo = dir.path();
        commit_file(repo, "file.txt", "one\n", "chore: base");

        write_file(repo, "unrelated.txt", "preserve parser user bytes\n");
        let head = git(repo, &["rev-parse", "HEAD"]);
        let refs = git(repo, &["show-ref"]);
        let index = fs::read(git_dir(repo).join("index")).or_abort();
        // Intercept `git rev-parse --verify <ref>` and return a non-hex 40-char SHA to
        // ensure we exercise CommitSha::new's non-hex branch in a non-test build.
        let (wrap_dir, wrap_bin) = make_git_wrapper_named(
            "git",
            r#"if [ "${1-}" = "rev-parse" ] && [ "${2-}" = "--verify" ] && [ "${3-}" = "definitely-not-a-commit" ]; then
  printf 'invalid-hash\n' >> "$(dirname "$0")/observed-hash"
  printf '%s\n' '000000000000000000000000000000000000000g'
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
        let mut expectation = GitFactorExpectation::default()
            .code(EXIT_DATAERR)
            .stderr("invalid commit: 000000000000000000000000000000000000000g\n");
        expectation.stdout = Some(StreamExpectation::new_exact(String::new()));

        run_git_factor_with_prefixed_path(
            repo,
            &["--exec", "true", "definitely-not-a-commit"],
            expectation,
            prefixed_path,
        );
        assert_eq!(
            fs::read(wrap_bin.join("observed-hash")).or_abort(),
            b"invalid-hash\n"
        );
        assert_eq!(git(repo, &["rev-parse", "HEAD"]), head);
        assert_eq!(git(repo, &["show-ref"]), refs);
        assert_eq!(fs::read(git_dir(repo).join("index")).or_abort(), index);
        assert_eq!(fs::read(repo.join("file.txt")).or_abort(), b"one\n");
        assert_eq!(
            fs::read(repo.join("unrelated.txt")).or_abort(),
            b"preserve parser user bytes\n"
        );
        assert!(!checkpoint_journal(repo).exists());
        assert!(!git_dir(repo).join("factor").exists());
        assert!(!git_dir(repo).join("rebase-merge").exists());
        assert!(!git_dir(repo).join("rebase-apply").exists());
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

        write_file(repo, "unrelated.txt", "preserve parser user bytes\n");
        let head = git(repo, &["rev-parse", "HEAD"]);
        let refs = git(repo, &["show-ref"]);
        let index = fs::read(git_dir(repo).join("index")).or_abort();
        // Intercept `git rev-parse HEAD^{tree}` and return a non-hex 40-char
        // value to exercise tree-hash validation in a non-test build.
        let (wrap_dir, wrap_bin) = make_git_wrapper_named(
            "git",
            r#"if [ "${1-}" = "rev-parse" ] && [ "${2-}" = "HEAD^{tree}" ]; then
  printf 'invalid-hash\n' >> "$(dirname "$0")/observed-hash"
  printf '%s\n' '000000000000000000000000000000000000000g'
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
        let mut expectation = GitFactorExpectation::default().code(EXIT_SOFTWARE).stderr(
            "git command failed: invalid tree hash: '000000000000000000000000000000000000000g'\n",
        );
        expectation.stdout = Some(StreamExpectation::new_exact(String::new()));

        run_git_factor_with_prefixed_path(
            repo,
            &["--exec", "true", "HEAD"],
            expectation,
            prefixed_path,
        );
        assert_eq!(
            fs::read(wrap_bin.join("observed-hash")).or_abort(),
            b"invalid-hash\n"
        );
        assert_eq!(git(repo, &["rev-parse", "HEAD"]), head);
        assert_eq!(git(repo, &["show-ref"]), refs);
        assert_eq!(fs::read(git_dir(repo).join("index")).or_abort(), index);
        assert_eq!(fs::read(repo.join("file.txt")).or_abort(), b"one\n");
        assert_eq!(
            fs::read(repo.join("unrelated.txt")).or_abort(),
            b"preserve parser user bytes\n"
        );
        assert!(!checkpoint_journal(repo).exists());
        assert!(!git_dir(repo).join("factor").exists());
        assert!(!git_dir(repo).join("rebase-merge").exists());
        assert!(!git_dir(repo).join("rebase-apply").exists());
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
    fn cli_version_flag_prints_version_to_stdout_and_exits_ok() {
        run_git_factor_no_repo(
            &["--version"],
            GitFactorExpectation::default()
                .stdout(format!("git-factor {}\n", env!("CARGO_PKG_VERSION"))),
        );
    }

    #[test]
    fn cli_short_version_flag_prints_version_to_stdout_and_exits_ok() {
        run_git_factor_no_repo(
            &["-v"],
            GitFactorExpectation::default()
                .stdout(format!("git-factor {}\n", env!("CARGO_PKG_VERSION"))),
        );
    }

    #[test]
    fn start_requires_exec_command() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");

        let head = git(repo, &["rev-parse", "HEAD"]);
        let refs = git(repo, &["show-ref"]);
        let index = fs::read(git_dir(repo).join("index")).or_abort();

        let mut expectation = GitFactorExpectation::default()
            .code(EXIT_USAGE)
            .stderr("--exec <COMMAND> is required when starting a factor session\n")
            .factor_state_exists(false)
            .rebase_merge_exists(false)
            .rebase_apply_exists(false);
        expectation.stdout = Some(StreamExpectation::new_exact(String::new()));

        run_git_factor(repo, &["HEAD"], expectation);
        assert_eq!(git(repo, &["rev-parse", "HEAD"]), head);
        assert_eq!(git(repo, &["show-ref"]), refs);
        assert_eq!(fs::read(git_dir(repo).join("index")).or_abort(), index);
        assert_eq!(fs::read(repo.join("file.txt")).or_abort(), b"one\n");
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
                .stderr("no active factor session\n"),
        );
    }

    #[test]
    fn start_head_runs_exec_gate_before_session_starts() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");
        commit_file(repo, "file.txt", "one\ntwo\n", "feat: change");
        let head_before = git(repo, &["rev-parse", "HEAD"]);
        let branch = git(repo, &["symbolic-ref", "HEAD"]);

        run_git_factor(
            repo,
            &["--exec", "false", "HEAD"],
            GitFactorExpectation::default()
                .code(EXIT_TEMPFAIL)
                .stderr_suffix("exec gate failed: false (exit code 1)\n")
                .git_output(&["rev-parse", branch.as_str()], head_before.clone())
                .git_status_porcelain("")
                .factor_state_exists(true),
        );
        assert_eq!(
            read_journal(repo)
                .get("checkpoint")
                .and_then(serde_json::Value::as_str),
            Some(head_before.as_str())
        );
        assert_eq!(
            read_journal(repo)
                .pointer("/state/phase")
                .and_then(serde_json::Value::as_str),
            Some("opening")
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
                .rebase_merge_exists(true)
                .requires_rebase(true)
                .stdout(expected_single_commit_start_stdout(
                    head_sha.as_str(),
                    head_short_sha.as_str(),
                    "feat: change",
                )),
        );
        assert_eq!(
            read_journal(repo)
                .get("checkpoint")
                .and_then(serde_json::Value::as_str),
            Some(head_sha.as_str())
        );

        git(repo, &["add", "--all"]);
        run_git_factor(
            repo,
            &["--continue", "--message", "test: split"],
            GitFactorExpectation::default()
                .stdout_suffix(expected_completion_stdout_suffix("continue", 1)),
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
                .requires_rebase(true)
                .stdout(expected_single_commit_start_stdout(
                    head_sha.as_str(),
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

        // Intercept range expansion and return success with no output so the
        // ancestry span resolver sees an empty dotted range.
        let (wrap_dir, wrap_bin) = make_git_wrapper_named(
            "git",
            r#"if [ "${1-}" = "rev-list" ] && [ "${2-}" = "--reverse" ] && [ "${3-}" = "--ancestry-path" ] && [ "${4-}" = "HEAD~1..HEAD" ]; then
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
                .code(EXIT_DATAERR)
                .stderr("invalid commit: HEAD~1..HEAD\n"),
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
                .code(EXIT_DATAERR)
                .stderr("invalid commit: HEAD..HEAD\n"),
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
                .stderr("--message cannot be combined with --exec or COMMIT\n"),
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

        let original_head = git(repo, &["rev-parse", "HEAD"]);
        let original_index = fs::read(git_dir(repo).join("index")).or_abort();
        let original_journal = fs::read(checkpoint_journal(repo)).or_abort();
        let original_refs = git(repo, &["show-ref"]);
        let resumed = Command::new(git_factor_bin())
            .current_dir(repo)
            .arg("--continue")
            .output()
            .or_abort();
        assert!(
            resumed.status.success(),
            "{}",
            String::from_utf8_lossy(&resumed.stderr)
        );
        let selection: serde_json::Value = serde_json::from_slice(&resumed.stdout).or_abort();
        assert_eq!(
            selection
                .get("operation")
                .and_then(serde_json::Value::as_str),
            Some("continue")
        );
        assert_eq!(
            selection.pointer("/changes/untracked"),
            Some(&serde_json::from_str::<serde_json::Value>("[\"file.txt\"]").or_abort())
        );
        assert_eq!(git(repo, &["rev-parse", "HEAD"]), original_head);
        assert_eq!(git(repo, &["show-ref"]), original_refs);
        assert_eq!(
            fs::read(git_dir(repo).join("index")).or_abort(),
            original_index
        );
        assert_eq!(
            fs::read(checkpoint_journal(repo)).or_abort(),
            original_journal
        );
        assert_eq!(fs::read(repo.join("file.txt")).or_abort(), b"one\n");
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
            GitFactorExpectation::default().stdout("{\"operation\":\"status\",\"session\":null}\n"),
        );
    }

    #[test]
    fn status_reports_active_session_details() {
        let dir = init_repo();
        let repo = dir.path();
        commit_file(repo, "file.txt", "one\n", "chore: base");
        commit_file(repo, "file.txt", "one\ntwo\n", "feat: change");

        let current_commit = git(repo, &["rev-parse", "HEAD"]);
        start_session(repo);

        run_git_factor(
            repo,
            &["--status"],
            GitFactorExpectation::default().stdout(format!(
                "{{\"operation\":\"status\",\"session\":{{\"checkpoint\":\"{current_commit}\",\"phase\":\"selecting\",\"rebase\":{{\"in_progress\":true,\"required\":true}},\"split_count\":0,\"target\":{{\"commit\":\"{current_commit}\",\"commit_count\":1,\"span_starts_at_root\":false}}}}}}\n"
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
            GitFactorExpectation::default().stdout(
                "{\"operation\":\"abort\",\"rebase\":{\"in_progress\":false},\"actions\":{}}\n",
            ),
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
    fn rejects_non_contiguous_two_ref_spans() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");
        let main_branch = git(repo, &["branch", "--show-current"]);
        git(repo, &["checkout", "-b", "other"]);
        commit_file(repo, "other.txt", "other\n", "feat: other");
        let other_sha = git(repo, &["rev-parse", "HEAD"]);
        git(repo, &["checkout", main_branch.as_str()]);
        commit_file(repo, "file.txt", "one\ntwo\n", "feat: on main");

        let output = Command::new(git_factor_bin())
            .current_dir(repo)
            .args(["--exec", "true", other_sha.as_str(), "HEAD"])
            .output()
            .or_abort();
        assert_eq!(output.status.code(), Some(EXIT_DATAERR));
        assert!(
            output.stdout.is_empty(),
            "stdout should be empty: {output:?}"
        );
        let stderr = String::from_utf8(output.stderr).or_abort();
        assert!(
            stderr.contains("contiguous ancestry span"),
            "unexpected stderr: {stderr}"
        );
    }

    #[test]
    fn rejects_spans_containing_merge_commits() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "base.txt", "base\n", "feat: base");
        git(repo, &["checkout", "--quiet", "-b", "side"]);
        commit_file(repo, "side.txt", "side\n", "feat: side");
        git(repo, &["checkout", "--quiet", "-"]);
        commit_file(repo, "main.txt", "main\n", "feat: main");
        git(repo, &["merge", "--quiet", "--no-ff", "--no-edit", "side"]);
        commit_file(repo, "after.txt", "after\n", "feat: after");

        let output = Command::new(git_factor_bin())
            .current_dir(repo)
            .args(["--exec", "true", "HEAD~1^..HEAD"])
            .output()
            .or_abort();
        assert_eq!(output.status.code(), Some(EXIT_DATAERR));
        assert!(
            output.stdout.is_empty(),
            "stdout should be empty: {output:?}"
        );
        let stderr = String::from_utf8(output.stderr).or_abort();
        assert!(
            stderr.contains("is a merge commit"),
            "unexpected stderr: {stderr}"
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
                .rebase_merge_exists(true),
        );
        assert_eq!(
            read_journal(repo)
                .get("checkpoint")
                .and_then(serde_json::Value::as_str),
            Some(start_head.as_str())
        );
        assert_eq!(
            read_journal(repo)
                .get("original_tip")
                .and_then(serde_json::Value::as_str),
            Some(root_sha.as_str())
        );
        assert!(read_journal(repo).get("original_base").or_abort().is_null());
    }

    #[test]
    fn start_preserves_opening_journal_when_git_rebase_fails() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");
        commit_file(repo, "file.txt", "one\ntwo\n", "feat: change");

        write_file(repo, "unrelated.txt", "preserve opening user bytes\n");
        let before_head = git(repo, &["rev-parse", "HEAD"]);
        let before_index = fs::read(git_dir(repo).join("index")).or_abort();
        let before_refs = git(repo, &["show-ref", "--heads", "--tags"]);
        let before_file = fs::read(repo.join("file.txt")).or_abort();
        let (wrap_dir, wrap_bin) = make_git_wrapper_named(
            "git",
            r#"if [ "$#" -eq 13 ] && [ "${1-}" = "-c" ] && [ "${2-}" = "rebase.missingCommitsCheck=ignore" ] && [ "${3-}" = "rebase" ] && [ "${4-}" = "--interactive" ]; then
  printf '%s\n' "$@" >> "$(dirname "$0")/observed-opening"
  exit 1
fi
"#,
        );
        let expected_base = "--root".to_owned();
        let _keep_alive = wrap_dir;
        let observation = wrap_bin.join("observed-opening");

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
                .code(EXIT_TEMPFAIL)
                .stdout(expected_recovery_stdout("start"))
                .factor_state_exists(true),
            prefixed_path,
        );
        assert_eq!(
            fs::read_to_string(observation).or_abort(),
            format!(
                "-c\nrebase.missingCommitsCheck=ignore\nrebase\n--interactive\n--no-ff\n--reschedule-failed-exec\n--no-update-refs\n--no-autostash\n--no-autosquash\n--no-rebase-merges\n--empty=keep\n--keep-empty\n{expected_base}\n"
            )
        );
        assert_eq!(git(repo, &["rev-parse", "HEAD"]), before_head);
        assert_eq!(
            fs::read(git_dir(repo).join("index")).or_abort(),
            before_index
        );
        assert_eq!(git(repo, &["show-ref", "--heads", "--tags"]), before_refs);
        assert_eq!(fs::read(repo.join("file.txt")).or_abort(), before_file);
        assert_eq!(
            fs::read(repo.join("unrelated.txt")).or_abort(),
            b"preserve opening user bytes\n"
        );
        assert_eq!(
            read_journal(repo)
                .pointer("/state/phase")
                .and_then(serde_json::Value::as_str),
            Some("opening")
        );
        assert!(!git_dir(repo).join("rebase-merge").exists());
        assert!(!git_dir(repo).join("rebase-apply").exists());
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
                .stderr_suffix("git command failed: mock short failure\n"),
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
            r#"if [ "${1-}" = "diff" ] && [ "${2-}" = "--numstat" ]; then
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
                .stderr_suffix(
                    "git command failed: Git change query failed: mock diff stat failure\n",
                ),
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
                .stderr("git command failed: cannot observe protected worktree root\n"),
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
                .stderr("failed to read state: Not a directory (os error 20)\n"),
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
                .code(EXIT_SOFTWARE)
                .stderr("git command failed: existing legacy or active session must be finished or aborted with its originating version\n"),
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
                .path_content("file.txt", "one\ntwo\n")
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
    fn retry_discards_attempt_and_restores_remaining_pool() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");
        commit_file(repo, "file.txt", "one\ntwo\nthree\n", "feat: change");

        start_session(repo);

        let status_before = git(repo, &["status", "--porcelain=v1"]);
        let diff_before = git(repo, &["diff", "--stat"]);

        fs::remove_file(repo.join("file.txt")).or_abort();
        fs::write(repo.join("scratch.tmp"), "temporary\n").or_abort();

        let original_head = git(repo, &["rev-parse", "HEAD"]);
        let original_index = fs::read(git_dir(repo).join("index")).or_abort();
        let original_journal = fs::read(checkpoint_journal(repo)).or_abort();
        let original_refs = git(repo, &["show-ref"]);
        let expected_tree = read_journal(repo)
            .get("final_tree")
            .and_then(serde_json::Value::as_str)
            .or_abort()
            .to_owned();
        run_git_factor(repo, &["--retry"], GitFactorExpectation::default().code(EXIT_TEMPFAIL).stderr(format!(
            "tree hash mismatch: expected {expected_tree}, got 4b825dc642cb6eb9a060e54bf8d69288fbee4904\n"
        )));
        assert_eq!(git(repo, &["rev-parse", "HEAD"]), original_head);
        assert_eq!(git(repo, &["show-ref"]), original_refs);
        assert_eq!(
            fs::read(git_dir(repo).join("index")).or_abort(),
            original_index
        );
        assert_eq!(
            fs::read(checkpoint_journal(repo)).or_abort(),
            original_journal
        );
        assert!(!repo.join("file.txt").exists());
        assert_eq!(
            fs::read(repo.join("scratch.tmp")).or_abort(),
            b"temporary\n"
        );
        write_file(repo, "file.txt", "one\ntwo\nthree\n");
        git(repo, &["add", "file.txt"]);
        run_git_factor(
            repo,
            &["--retry"],
            GitFactorExpectation::default()
                .git_output(&["diff", "--stat"], diff_before)
                .git_status_porcelain(format!("{status_before}\n?? scratch.tmp"))
                .path_exists("scratch.tmp", true)
                .factor_state_exists(true),
        );
        assert_eq!(
            fs::read(repo.join("scratch.tmp")).or_abort(),
            b"temporary\n"
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
        let original_base = git(repo, &["rev-parse", "HEAD~1"]);
        run_git_factor(
            repo,
            &["--exec", "true", "HEAD"],
            GitFactorExpectation::default()
                .factor_state_exists(true)
                .rebase_merge_exists(true),
        );
        let journal = read_journal(repo);
        assert_eq!(
            journal.get("format").and_then(serde_json::Value::as_str),
            Some("checkpoint_v2")
        );
        assert_eq!(
            journal
                .get("original_tip")
                .and_then(serde_json::Value::as_str),
            Some(original_commit.as_str())
        );
        assert_eq!(
            journal
                .get("checkpoint")
                .and_then(serde_json::Value::as_str),
            Some(original_commit.as_str())
        );
        assert_eq!(
            journal
                .get("original_base")
                .and_then(serde_json::Value::as_str),
            Some(original_base.as_str())
        );
        assert_eq!(
            journal
                .get("final_tree")
                .and_then(serde_json::Value::as_str),
            Some(expected_tree.as_str())
        );
        assert_eq!(
            journal
                .pointer("/state/phase")
                .and_then(serde_json::Value::as_str),
            Some("selecting")
        );
        assert_eq!(
            journal
                .pointer("/gates/0/command")
                .and_then(serde_json::Value::as_str),
            Some("true")
        );
        assert!(!git_dir(repo).join("factor/commits").exists());
    }

    #[test]
    fn proptest_start_with_multiple_exec_flags_persists_joined_exec_command() {
        let dir = init_repo();
        let repo = dir.path();
        commit_file(repo, "file.txt", "base\n", "chore: base");
        commit_file(repo, "file.txt", "base\nchange\n", "feat: change");
        run_git_factor(
            repo,
            &["--exec", "true", "--exec", ":", "HEAD"],
            GitFactorExpectation::default().factor_state_exists(true),
        );
        let journal = read_journal(repo);
        let gates = journal
            .get("gates")
            .and_then(serde_json::Value::as_array)
            .or_abort();
        assert_eq!(gates.len(), 2);
        assert_eq!(
            gates
                .first()
                .and_then(|gate| gate.get("command"))
                .and_then(serde_json::Value::as_str),
            Some("true")
        );
        assert_eq!(
            gates
                .get(1)
                .and_then(|gate| gate.get("command"))
                .and_then(serde_json::Value::as_str),
            Some(":")
        );
        assert_ne!(
            gates.first().and_then(|gate| gate.get("name")),
            gates.get(1).and_then(|gate| gate.get("name"))
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
            trace_content.contains("\"after_factor_phase\":\"selecting\""),
            "trace log should observe the current selecting phase\n{trace_content}"
        );
        assert!(
            trace_content.contains("\"after_factor_checkpoint\":"),
            "trace log should observe the completed checkpoint\n{trace_content}"
        );
        assert!(
            trace_content.contains("\"after_factor_final_tree\":"),
            "trace log should observe the fixed final tree\n{trace_content}"
        );
        let status = Command::new(git_factor_bin())
            .arg("--status")
            .current_dir(repo)
            .output()
            .or_abort();
        assert!(status.status.success());
        let observed: serde_json::Value = serde_json::from_slice(&status.stdout).or_abort();
        assert_eq!(
            observed
                .pointer("/session/phase")
                .and_then(serde_json::Value::as_str),
            Some("selecting")
        );
        assert_eq!(
            observed
                .pointer("/session/split_count")
                .and_then(serde_json::Value::as_u64),
            Some(0)
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
            GitFactorExpectation::default().stdout("{\"operation\":\"status\",\"session\":null}\n"),
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
            GitFactorExpectation::default().stdout("{\"operation\":\"status\",\"session\":null}\n"),
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
            GitFactorExpectation::default().stdout("{\"operation\":\"status\",\"session\":null}\n"),
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
    #[cfg(unix)]
    #[expect(
        clippy::literal_string_with_formatting_args,
        reason = "Git's tree revision syntax is literal input"
    )]
    fn status_trace_preserves_raw_index_after_ignored_hardlink_creation() {
        use std::os::unix::fs::MetadataExt as _;

        let dir = init_repo();
        let repo = dir.path();
        git(repo, &["config", "core.trustctime", "true"]);
        commit_file(
            repo,
            ".gitignore",
            "ignored-link\nunrelated.txt\n",
            "Add ignores",
        );
        commit_file(repo, "tracked.txt", "tracked bytes\n", "Add tracked file");
        // Cache a clean stat entry before changing only the tracked inode's ctime.
        git(repo, &["status", "--porcelain=v1", "--untracked-files=all"]);
        fs::write(repo.join("unrelated.txt"), b"unrelated user bytes\n").or_abort();
        let original = fs::metadata(repo.join("tracked.txt")).or_abort();
        // Git builds may compare ctime only at whole-second resolution.
        sleep(Duration::from_millis(1100));
        fs::hard_link(repo.join("tracked.txt"), repo.join("ignored-link")).or_abort();
        let linked = fs::metadata(repo.join("tracked.txt")).or_abort();
        let alias = fs::metadata(repo.join("ignored-link")).or_abort();
        assert_eq!((linked.dev(), linked.ino()), (alias.dev(), alias.ino()));
        assert_eq!(original.modified().or_abort(), linked.modified().or_abort());
        assert_ne!(original.ctime(), linked.ctime());
        let index_path = git_dir(repo).join("index");
        let index_before = fs::read(&index_path).or_abort();
        let head = git(repo, &["rev-parse", "HEAD"]);
        let tree = git(repo, &["rev-parse", "HEAD^{tree}"]);
        let refs = git(repo, &["show-ref"]);
        let snapshot_git_dir = git(repo, &["rev-parse", "--absolute-git-dir"]);
        let snapshot_toplevel = git(repo, &["rev-parse", "--show-toplevel"]);
        let (wrapper_dir, wrapper_bin) = make_git_wrapper_named("git", "");
        let path = format!("{}:{}", wrapper_bin.display(), env::var("PATH").or_abort());
        let trace_dir = TempDir::new().or_abort();
        let trace_path = trace_dir.path().join("status.jsonl");

        let output = Command::new(git_factor_bin())
            .current_dir(repo)
            .env_remove("CLAUDECODE")
            .env("PATH", path)
            .env("GIT_FACTOR_TRACE_LOG", &trace_path)
            .args(["--status"])
            .output()
            .or_abort();

        assert_eq!(output.status.code(), Some(EXIT_OK));
        assert_eq!(
            output.stdout.as_slice(),
            b"{\"operation\":\"status\",\"session\":null}\n"
        );
        assert_eq!(output.stderr.as_slice(), b"");
        assert_eq!(fs::read(&index_path).or_abort(), index_before);
        assert_eq!(
            (
                git(repo, &["rev-parse", "HEAD"]),
                git(repo, &["rev-parse", "HEAD^{tree}"]),
                git(repo, &["show-ref"])
            ),
            (head.clone(), tree.clone(), refs)
        );
        assert_eq!(
            fs::read(repo.join("tracked.txt")).or_abort(),
            b"tracked bytes\n"
        );
        assert_eq!(
            fs::read(repo.join("ignored-link")).or_abort(),
            b"tracked bytes\n"
        );
        assert_eq!(
            fs::read(repo.join("unrelated.txt")).or_abort(),
            b"unrelated user bytes\n"
        );
        assert!(!git_dir(repo).join("factor").exists());
        let trace = fs::read_to_string(trace_path).or_abort();
        let notes = trace
            .lines()
            .filter(|line| line.contains("\"event\":\"factor_cmd_status\""))
            .collect::<Vec<_>>();
        assert_eq!(notes.len(), 1);
        let note = notes.first().or_abort();
        let without_prefix = note.strip_prefix("{\"ts_unix_ms\":").or_abort();
        let (timestamp, fields) = without_prefix.split_once(',').or_abort();
        assert!(timestamp.parse::<u64>().or_abort() > 0);
        let expected = format!(
            concat!(
                "\"event\":\"factor_cmd_status\",",
                "\"state_head\":\"{head}\",\"state_head_tree\":\"{tree}\",",
                "\"state_git_dir\":\"{git_dir}\",\"state_toplevel\":\"{toplevel}\",",
                "\"state_staged_paths\":[],\"state_unstaged_paths\":[],\"state_untracked_paths\":[],",
                "\"state_factor_checkpoint\":null,\"state_factor_final_tree\":null,",
                "\"state_factor_phase\":null,\"state_factor_source\":null,",
                "\"state_rebase_state\":null,",
                "\"state_rebase_msgnum\":null,\"state_rebase_end\":null,",
                "\"state_rebase_todo_head\":null,\"state_rebase_done_tail\":null}}",
            ),
            head = head,
            tree = tree,
            git_dir = snapshot_git_dir,
            toplevel = snapshot_toplevel
        );
        assert_eq!(fields, expected);
        drop(wrapper_dir);
    }

    #[test]
    fn status_trace_collects_paths_and_ignores_short_status_lines() {
        let dir = init_repo();
        let repo = dir.path();
        commit_file(repo, "file.txt", "base\n", "chore: base");

        let (wrap_dir, wrap_bin) = make_git_wrapper_named(
            "git",
            r#"if [ "${1-}" = "--no-optional-locks" ] && [ "${2-}" = "status" ] && [ "${3-}" = "--porcelain=v1" ] && [ "${4-}" = "--untracked-files=all" ]; then
  printf ' M unstaged.txt\n'
  printf 'M\n'
  printf 'A  staged.txt\n'
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
            GitFactorExpectation::default().stdout("{\"operation\":\"status\",\"session\":null}\n"),
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
    fn status_trace_ignores_empty_paths_between_valid_records() {
        let dir = init_repo();
        let repo = dir.path();
        commit_file(repo, "file.txt", "base\n", "chore: base");

        let (wrap_dir, wrap_bin) = make_git_wrapper_named(
            "git",
            r#"if [ "${1-}" = "--no-optional-locks" ] && [ "${2-}" = "status" ] && [ "${3-}" = "--porcelain=v1" ] && [ "${4-}" = "--untracked-files=all" ]; then
  printf 'MM\n??\n M\nMM \n'
  printf ' M unstaged.txt\n'
  printf '?? \n'
  printf 'M\n'
  printf 'A  staged.txt\n'
  printf '?? untracked.txt\n'
  printf ' M '
  exit 0
fi
"#,
        );
        let _keep_alive = wrap_dir;

        let trace_path = repo.join("trace/status-paths.jsonl");
        run_git_factor_with_prefixed_path_and_env(
            repo,
            &["--status"],
            GitFactorExpectation::default().stdout("{\"operation\":\"status\",\"session\":null}\n"),
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
        write_executable(
            &git_wrapper,
            &format!(
                "#!/bin/sh\nexec {} \"$@\"\n",
                shell_quote(native_git_bin().to_str().or_abort())
            ),
        );

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
        write_executable(
            &git_wrapper,
            &format!(
                "#!/bin/sh\nexec {} \"$@\"\n",
                shell_quote(native_git_bin().to_str().or_abort())
            ),
        );

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
  : > .git/unexpected-write-tree
  echo "forced write-tree failure" 1>&2
  exit 1
fi
exec /usr/bin/git "$@"
"#
            .replace(
                "/usr/bin/git",
                &shell_quote(native_git_bin().to_str().or_abort()),
            )
            .as_str(),
        );
        let _keep_alive = wrapper_root;

        let before = snapshot_selection(repo);
        run_git_factor_with_prefixed_path(
            repo,
            &["--continue", "--message", "test: split"],
            GitFactorExpectation::default()
                .code(EXIT_SOFTWARE)
                .stderr("git command failed: bash -c: No such file or directory (os error 2)\n"),
            wrapper_bin.as_os_str().to_os_string(),
        );
        assert_eq!(snapshot_selection(repo), before);
        assert_eq!(fs::read(repo.join("file.txt")).or_abort(), b"one\ntwo\n");
        assert!(!git_dir(repo).join("unexpected-write-tree").exists());
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
            GitFactorExpectation::default().requires_rebase(true),
        );
        assert!(
            read_journal(repo)
                .get("original_base")
                .and_then(serde_json::Value::as_str)
                .is_some()
        );
        let observed = Command::new(git_factor_bin())
            .current_dir(repo)
            .arg("--status")
            .output()
            .or_abort();
        assert!(observed.status.success());
        let status: serde_json::Value = serde_json::from_slice(&observed.stdout).or_abort();
        assert_eq!(
            status
                .pointer("/session/target/span_starts_at_root")
                .and_then(serde_json::Value::as_bool),
            Some(false)
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
        write_file(repo, "file.txt", "base\na\nb\n");

        run_git_factor(
            repo,
            &["--continue", "--message", "test: split slice"],
            GitFactorExpectation::default()
                .factor_state_exists(true)
                .rebase_merge_exists(true)
                .git_status_porcelain_non_empty(),
        );
        let journal = read_journal(repo);
        assert_eq!(
            journal
                .get("final_tree")
                .and_then(serde_json::Value::as_str),
            Some(expected_tree.as_str())
        );
        let checkpoint = journal
            .get("checkpoint")
            .and_then(serde_json::Value::as_str)
            .or_abort();
        let atom = journal
            .pointer("/state/base")
            .and_then(serde_json::Value::as_str)
            .or_abort();
        assert_eq!(
            git(repo, &["rev-parse", &format!("{checkpoint}^{{tree}}")]),
            expected_tree
        );
        assert_eq!(
            git(repo, &["log", "-1", "--format=%s", atom]),
            "test: split slice"
        );
        let status = Command::new(git_factor_bin())
            .current_dir(repo)
            .arg("--status")
            .output()
            .or_abort();
        assert!(status.status.success());
        let observed_status: serde_json::Value = serde_json::from_slice(&status.stdout).or_abort();
        assert_eq!(
            observed_status
                .pointer("/session/split_count")
                .and_then(serde_json::Value::as_u64),
            Some(1)
        );
    }

    #[test]
    #[expect(
        clippy::create_dir,
        reason = "Owned legacy fixture paths must be fresh; an existing path is a setup error"
    )]
    fn continue_reports_split_count_overflow() {
        let directory = init_repo();
        let repo = directory.path();
        commit_file(repo, "file.txt", "one\n", "chore: base");
        let state = git_dir(repo).join("factor");
        fs::create_dir(&state).or_abort();
        fs::write(
            state.join("commits"),
            format!("{}\n", git(repo, &["rev-parse", "HEAD"])),
        )
        .or_abort();
        fs::write(state.join("current_index"), b"0\n").or_abort();
        fs::write(state.join("exec"), b"true\n").or_abort();
        fs::write(state.join("split_count"), b"0\n").or_abort();
        fs::write(state.join("requires_rebase"), b"false\n").or_abort();
        fs::write(state.join("split_count"), format!("{}\n", u8::MAX)).or_abort();
        verify_legacy_session_refusal(repo, &["--continue", "--message", "test: legacy split"]);
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
                .factor_state_exists(true)
                .rebase_merge_exists(true)
                .requires_rebase(true),
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
                .stdout(expected_remaining_stdout("[]", "[\"b.txt\"]"))
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
                .stdout(expected_remaining_stdout(
                    "[{\"path\":\"b.txt\",\"kind\":\"text\",\"added\":1,\"deleted\":0}]",
                    "[]",
                ))
                .git_output(&["ls-files", "--others", "--exclude-standard"], ""),
        );
    }

    #[test]
    fn finish_reuses_original_commit_message_when_none_provided() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");
        commit_file(repo, "file.txt", "one\ntwo\n", "feat: original message");

        let expected_tree = git(repo, &["rev-parse", "HEAD^{tree}"]);
        let original_message = git(repo, &["log", "--format=%B", "--max-count=1"]);

        start_session(repo);

        run_git_factor(
            repo,
            &["--finish"],
            GitFactorExpectation::default().stdout(expected_completion_stdout_suffix("finish", 1)),
        );
        assert_true_gate_message(
            repo,
            "HEAD",
            original_message.as_str(),
            expected_tree.as_str(),
        );
        assert_eq!(git(repo, &["rev-parse", "HEAD^{tree}"]), expected_tree);
    }

    #[test]
    fn finish_succeeds_without_rebase_when_target_is_head() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");
        commit_file(repo, "file.txt", "one\ntwo\n", "feat: change");
        let head_sha = git(repo, &["rev-parse", "HEAD"]);
        let head_short_sha = git(repo, &["rev-parse", "--short", "HEAD"]);

        run_git_factor(
            repo,
            &["--exec", "true", "HEAD"],
            GitFactorExpectation::default()
                .rebase_apply_exists(false)
                .rebase_merge_exists(true)
                .requires_rebase(true)
                .stdout(expected_single_commit_start_stdout(
                    head_sha.as_str(),
                    head_short_sha.as_str(),
                    "feat: change",
                )),
        );

        git(repo, &["add", "--all"]);
        run_git_factor(
            repo,
            &["--finish", "--message", "test: done"],
            GitFactorExpectation::default()
                .stdout_suffix(expected_completion_stdout_suffix("finish", 1)),
        );
    }

    #[test]
    fn finish_preserves_empty_commits() {
        let directory = init_repo();
        let repo = directory.path();
        commit_file(repo, "file.txt", "one\n", "chore: base");
        git(
            repo,
            &["commit", "--allow-empty", "--message", "feat: placeholder"],
        );
        let original_head = git(repo, &["rev-parse", "HEAD"]);
        let original_tree = git(repo, &["rev-parse", "HEAD^{tree}"]);
        let original_message = git(repo, &["show", "--format=%B", "--no-patch", "HEAD"]);
        let original_refs = git(repo, &["show-ref"]);
        let original_index = fs::read(git_dir(repo).join("index")).or_abort();
        fs::write(
            repo.join("unrelated.tmp"),
            b"unrelated empty-selection bytes\n",
        )
        .or_abort();
        run_git_factor(repo, &["--exec", "false", "HEAD"], GitFactorExpectation::default()
            .code(EXIT_DATAERR)
            .stdout("{\"operation\":\"start\",\"reason\":\"empty_change\",\"result\":\"refused\"}\n")
            .factor_state_exists(false).rebase_merge_exists(false));
        assert_eq!(git(repo, &["rev-parse", "HEAD"]), original_head);
        assert_eq!(git(repo, &["rev-parse", "HEAD^{tree}"]), original_tree);
        assert_eq!(
            git(repo, &["show", "--format=%B", "--no-patch", "HEAD"]),
            original_message
        );
        assert_eq!(git(repo, &["show-ref"]), original_refs);
        assert_eq!(
            fs::read(git_dir(repo).join("index")).or_abort(),
            original_index
        );
        assert_eq!(fs::read(repo.join("file.txt")).or_abort(), b"one\n");
        assert_eq!(
            fs::read(repo.join("unrelated.tmp")).or_abort(),
            b"unrelated empty-selection bytes\n"
        );
        run_git_factor(
            repo,
            &["--finish"],
            GitFactorExpectation::default()
                .code(EXIT_USAGE)
                .stderr("no active factor session\n"),
        );
    }

    #[test]
    fn finish_uses_allow_empty_when_original_commit_is_empty() {
        let directory = init_repo();
        let repo = directory.path();
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
        let original_head = git(repo, &["rev-parse", "HEAD"]);
        let original_tree = git(repo, &["rev-parse", "HEAD^{tree}"]);
        let original_message = git(repo, &["show", "--format=%B", "--no-patch", "HEAD"]);
        let original_refs = git(repo, &["show-ref"]);
        let original_index = fs::read(git_dir(repo).join("index")).or_abort();
        fs::write(
            repo.join("unrelated.tmp"),
            b"unrelated empty-selection bytes\n",
        )
        .or_abort();
        run_git_factor(repo, &["--exec", "false", "HEAD"], GitFactorExpectation::default()
            .code(EXIT_DATAERR)
            .stdout("{\"operation\":\"start\",\"reason\":\"empty_change\",\"result\":\"refused\"}\n")
            .factor_state_exists(false).rebase_merge_exists(false));
        assert_eq!(git(repo, &["rev-parse", "HEAD"]), original_head);
        assert_eq!(git(repo, &["rev-parse", "HEAD^{tree}"]), original_tree);
        assert_eq!(
            git(repo, &["show", "--format=%B", "--no-patch", "HEAD"]),
            original_message
        );
        assert_eq!(git(repo, &["show-ref"]), original_refs);
        assert_eq!(
            fs::read(git_dir(repo).join("index")).or_abort(),
            original_index
        );
        assert_eq!(fs::read(repo.join("file.txt")).or_abort(), b"one\n");
        assert_eq!(
            fs::read(repo.join("unrelated.tmp")).or_abort(),
            b"unrelated empty-selection bytes\n"
        );
        run_git_factor(
            repo,
            &["--finish"],
            GitFactorExpectation::default()
                .code(EXIT_USAGE)
                .stderr("no active factor session\n"),
        );
    }

    #[test]
    fn finish_enforces_exec_gate_before_completing_session() {
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
                .stdout("{\"actions\":{\"submit\":[\"git\",\"factor\",\"--continue\",\"--message\",\"<message>\"]},\"gate\":{\"command\":\"false\",\"exit_code\":1},\"guidance\":[\"Adjust staged changes so the gate passes, then submit the atom again.\"],\"operation\":\"finish\",\"result\":\"gate_failed\"}\n")
                .factor_state_exists(true)
                .path_exists(".git/factor", true),
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
        commit_file(repo, "file.txt", "root\nnext\ntail\n", "feat: tail");
        let root_sha = git(repo, &["rev-list", "--max-parents=0", "HEAD"]);

        write_file(repo, "unrelated.txt", "preserve opening user bytes\n");
        let before_head = git(repo, &["rev-parse", "HEAD"]);
        let before_index = fs::read(git_dir(repo).join("index")).or_abort();
        let before_refs = git(repo, &["show-ref", "--heads", "--tags"]);
        let before_file = fs::read(repo.join("file.txt")).or_abort();
        let (wrap_dir, wrap_bin) = make_git_wrapper_named(
            "git",
            r#"if [ "$#" -eq 13 ] && [ "${1-}" = "-c" ] && [ "${2-}" = "rebase.missingCommitsCheck=ignore" ] && [ "${3-}" = "rebase" ] && [ "${4-}" = "--interactive" ]; then
  printf '%s\n' "$@" >> "$(dirname "$0")/observed-opening"
  saw_root=0
  for arg in "$@"; do
    if [ "$arg" = "--root" ]; then
      saw_root=1
      break
    fi
  done
  if [ "$saw_root" -eq 1 ]; then
    echo 'root rebase selected (42)' >&2
    exit 42
  fi
  echo 'nonroot rebase selected (43)' >&2
  exit 43
fi
"#,
        );
        let expected_base = "--root".to_owned();
        let _keep_alive = wrap_dir;
        let observation = wrap_bin.join("observed-opening");

        run_git_factor_with_env(
            repo,
            &["--exec", "true", root_sha.as_str(), "HEAD~1"],
            GitFactorExpectation::default()
                .code(EXIT_TEMPFAIL)
                .stderr("root rebase selected (42)\n")
                .stdout(expected_recovery_stdout("start"))
                .factor_state_exists(true),
            "PATH",
            format!("{}:{}", wrap_bin.display(), env::var("PATH").or_abort()),
        );
        assert_eq!(
            fs::read_to_string(observation).or_abort(),
            format!(
                "-c\nrebase.missingCommitsCheck=ignore\nrebase\n--interactive\n--no-ff\n--reschedule-failed-exec\n--no-update-refs\n--no-autostash\n--no-autosquash\n--no-rebase-merges\n--empty=keep\n--keep-empty\n{expected_base}\n"
            )
        );
        assert_eq!(git(repo, &["rev-parse", "HEAD"]), before_head);
        assert_eq!(
            fs::read(git_dir(repo).join("index")).or_abort(),
            before_index
        );
        assert_eq!(git(repo, &["show-ref", "--heads", "--tags"]), before_refs);
        assert_eq!(fs::read(repo.join("file.txt")).or_abort(), before_file);
        assert_eq!(
            fs::read(repo.join("unrelated.txt")).or_abort(),
            b"preserve opening user bytes\n"
        );
        assert_eq!(
            read_journal(repo)
                .pointer("/state/phase")
                .and_then(serde_json::Value::as_str),
            Some("opening")
        );
        assert!(!git_dir(repo).join("rebase-merge").exists());
        assert!(!git_dir(repo).join("rebase-apply").exists());
    }

    #[test]
    fn start_does_not_use_rebase_root_for_non_root_multi_commit_session() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "feat: one");
        commit_file(repo, "file.txt", "one\ntwo\n", "feat: two");
        commit_file(repo, "file.txt", "one\ntwo\nthree\n", "feat: three");
        commit_file(repo, "file.txt", "one\ntwo\nthree\nfour\n", "feat: four");

        write_file(repo, "unrelated.txt", "preserve opening user bytes\n");
        let before_head = git(repo, &["rev-parse", "HEAD"]);
        let before_index = fs::read(git_dir(repo).join("index")).or_abort();
        let before_refs = git(repo, &["show-ref", "--heads", "--tags"]);
        let before_file = fs::read(repo.join("file.txt")).or_abort();
        let (wrap_dir, wrap_bin) = make_git_wrapper_named(
            "git",
            r#"if [ "$#" -eq 13 ] && [ "${1-}" = "-c" ] && [ "${2-}" = "rebase.missingCommitsCheck=ignore" ] && [ "${3-}" = "rebase" ] && [ "${4-}" = "--interactive" ]; then
  printf '%s\n' "$@" >> "$(dirname "$0")/observed-opening"
  saw_root=0
  for arg in "$@"; do
    if [ "$arg" = "--root" ]; then
      saw_root=1
      break
    fi
  done
  if [ "$saw_root" -eq 1 ]; then
    echo 'root rebase selected (42)' >&2
    exit 42
  fi
  echo 'nonroot rebase selected (43)' >&2
  exit 43
fi
"#,
        );
        let expected_base = git(repo, &["rev-parse", "HEAD~3"]);
        let _keep_alive = wrap_dir;
        let observation = wrap_bin.join("observed-opening");

        run_git_factor_with_env(
            repo,
            &["--exec", "true", "HEAD~2", "HEAD~1"],
            GitFactorExpectation::default()
                .code(EXIT_TEMPFAIL)
                .stderr("nonroot rebase selected (43)\n")
                .stdout(expected_recovery_stdout("start"))
                .factor_state_exists(true),
            "PATH",
            format!("{}:{}", wrap_bin.display(), env::var("PATH").or_abort()),
        );
        assert_eq!(
            fs::read_to_string(observation).or_abort(),
            format!(
                "-c\nrebase.missingCommitsCheck=ignore\nrebase\n--interactive\n--no-ff\n--reschedule-failed-exec\n--no-update-refs\n--no-autostash\n--no-autosquash\n--no-rebase-merges\n--empty=keep\n--keep-empty\n{expected_base}\n"
            )
        );
        assert_eq!(git(repo, &["rev-parse", "HEAD"]), before_head);
        assert_eq!(
            fs::read(git_dir(repo).join("index")).or_abort(),
            before_index
        );
        assert_eq!(git(repo, &["show-ref", "--heads", "--tags"]), before_refs);
        assert_eq!(fs::read(repo.join("file.txt")).or_abort(), before_file);
        assert_eq!(
            fs::read(repo.join("unrelated.txt")).or_abort(),
            b"preserve opening user bytes\n"
        );
        assert_eq!(
            read_journal(repo)
                .pointer("/state/phase")
                .and_then(serde_json::Value::as_str),
            Some("opening")
        );
        assert!(!git_dir(repo).join("rebase-merge").exists());
        assert!(!git_dir(repo).join("rebase-apply").exists());
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
        let original = git(repo, &["rev-parse", "HEAD"]);
        let head_short_sha = git(repo, &["rev-parse", "--short", "HEAD"]);
        let reference_path = repo.join("references/rust.md").canonicalize().or_abort();

        let expected = expected_single_commit_start_stdout(&original, &head_short_sha, "feat: change")
            .replace("\"Submit each atom through git factor so its gates run.\"]", "\"Submit each atom through git factor so its gates run.\",\"Above 50% context, pause and ask the user to /compact.\",\"Continue splitting until the session is complete.\"]");
        let expected_with_reference = expected.replace(
            "\"references\":[]",
            &format!(
                "\"references\":[{}]",
                serde_json::to_string(reference_path.to_str().or_abort()).or_abort()
            ),
        );
        run_git_factor_with_env(
            repo,
            &["--exec", "true", "HEAD"],
            GitFactorExpectation::default().stdout(expected_with_reference),
            "CLAUDECODE",
            "1",
        );
        assert_eq!(
            read_journal(repo)
                .get("original_tip")
                .and_then(serde_json::Value::as_str),
            Some(original.as_str())
        );
        assert_eq!(
            fs::read(repo.join("references/rust.md")).or_abort(),
            b"# rust\n"
        );
    }

    #[test]
    fn prints_claude_hints_without_reference_path_when_missing() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");
        commit_file(repo, "file.txt", "one\ntwo\n", "feat: change");
        let original = git(repo, &["rev-parse", "HEAD"]);
        let head_short_sha = git(repo, &["rev-parse", "--short", "HEAD"]);

        let expected = expected_single_commit_start_stdout(&original, &head_short_sha, "feat: change")
            .replace("\"Submit each atom through git factor so its gates run.\"]", "\"Submit each atom through git factor so its gates run.\",\"Above 50% context, pause and ask the user to /compact.\",\"Continue splitting until the session is complete.\"]");
        run_git_factor_with_env(
            repo,
            &["--exec", "true", "HEAD"],
            GitFactorExpectation::default().stdout(expected),
            "CLAUDECODE",
            "1",
        );
        assert_eq!(
            read_journal(repo)
                .get("original_tip")
                .and_then(serde_json::Value::as_str),
            Some(original.as_str())
        );
        assert!(!repo.join("references/rust.md").exists());
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
                .path_exists("untracked.txt", true)
                .git_status_porcelain("?? untracked.txt"),
        );
    }

    #[test]
    fn abort_succeeds_when_rebase_apply_is_active() {
        let dir = init_repo();
        let repo = dir.path();
        commit_file(repo, "file.txt", "one\n", "chore: base");
        let base = git(repo, &["rev-parse", "HEAD"]);
        commit_file(repo, "file.txt", "two\n", "feat: patch");
        let patch = Command::new(native_git_bin())
            .args(["format-patch", "-1", "--stdout", "HEAD"])
            .current_dir(repo)
            .output()
            .or_abort();
        assert!(patch.status.success());
        let patch_dir = TempDir::new().or_abort();
        let patch_path = patch_dir.path().join("owned.patch");
        fs::write(&patch_path, patch.stdout).or_abort();
        git(repo, &["reset", "--hard", &base]);
        commit_file(repo, "file.txt", "foreign\n", "Independent conflict");
        let paused = Command::new(native_git_bin())
            .args(["am", "--3way"])
            .arg(&patch_path)
            .current_dir(repo)
            .output()
            .or_abort();
        assert!(!paused.status.success());
        assert!(git_dir(repo).join("rebase-apply").is_dir());
        let head = git(repo, &["rev-parse", "HEAD"]);
        let refs = git(repo, &["show-ref"]);
        let index = fs::read(git_dir(repo).join("index")).or_abort();
        let native = legacy_inventory(&git_dir(repo).join("rebase-apply"));
        let physical = fs::read(repo.join("file.txt")).or_abort();
        fs::write(repo.join("unrelated.txt"), "preserve external apply\n").or_abort();
        run_git_factor(
            repo,
            &["--abort"],
            GitFactorExpectation::default()
                .code(EXIT_USAGE)
                .stderr("no active factor session\n")
                .rebase_apply_exists(true)
                .rebase_merge_exists(false)
                .factor_state_exists(false),
        );
        assert_eq!(git(repo, &["rev-parse", "HEAD"]), head);
        assert_eq!(git(repo, &["show-ref"]), refs);
        assert_eq!(fs::read(git_dir(repo).join("index")).or_abort(), index);
        assert_eq!(
            legacy_inventory(&git_dir(repo).join("rebase-apply")),
            native
        );
        assert_eq!(fs::read(repo.join("file.txt")).or_abort(), physical);
        assert_eq!(
            fs::read(repo.join("unrelated.txt")).or_abort(),
            b"preserve external apply\n"
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
    fn continue_splits_combined_span_for_multi_commit_range() {
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
        let second_sha = git(repo, &["rev-parse", "HEAD"]);
        let second_short_sha = git(repo, &["rev-parse", "--short", "HEAD"]);
        let expected_tree = git(repo, &["rev-parse", "HEAD^{tree}"]);

        run_git_factor(
            repo,
            &["--exec", "true", "HEAD~2..HEAD"],
            GitFactorExpectation::default().stdout_suffix(expected_multi_commit_start_suffix(
                second_sha.as_str(),
                second_short_sha.as_str(),
                "[\"new.txt\"]",
            )),
        );

        // First split: stage the tracked span portion, leaving the new file for later.
        git(repo, &["add", "base.txt"]);
        run_git_factor(
            repo,
            &["--continue", "--message", "test: split a"],
            GitFactorExpectation::default()
                .stdout(expected_remaining_stdout("[]", "[\"new.txt\"]")),
        );

        // Final split: add the remaining untracked file to converge to the tip tree.
        git(repo, &["add", "new.txt"]);
        run_git_factor(
            repo,
            &["--continue", "--message", "test: split b"],
            GitFactorExpectation::default()
                .stdout_suffix(expected_completion_stdout_suffix("continue", 2))
                .git_output(&["rev-parse", "HEAD^{tree}"], expected_tree),
        );
    }

    #[test]
    fn continue_completes_combined_span_for_multiple_explicit_refs() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "base.txt", "base\n", "chore: base");
        commit_file(repo, "base.txt", "base\na\n", "feat: a");
        write_file(repo, "base.txt", "base\na\nb\n");
        git(repo, &["add", "--all"]);
        git(repo, &["commit", "--message", "feat: b"]);
        let second_sha = git(repo, &["rev-parse", "HEAD"]);
        let second_short_sha = git(repo, &["rev-parse", "--short", "HEAD"]);

        run_git_factor(
            repo,
            &["--exec", "true", "HEAD~1", "HEAD"],
            GitFactorExpectation::default().stdout_suffix(expected_multi_commit_start_suffix(
                second_sha.as_str(),
                second_short_sha.as_str(),
                "[]",
            )),
        );

        git(repo, &["add", "--all"]);
        run_git_factor(
            repo,
            &["--continue", "--message", "test: split a"],
            GitFactorExpectation::default()
                .stdout_suffix(expected_completion_stdout_suffix("continue", 1)),
        );
    }

    #[test]
    fn start_accepts_git_native_inclusive_dotted_ranges() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "base.txt", "base\n", "chore: base");
        commit_file(repo, "base.txt", "base\na\n", "feat: a");
        write_file(repo, "base.txt", "base\na\nb\n");
        git(repo, &["add", "--all"]);
        git(repo, &["commit", "--message", "feat: b"]);
        let second_sha = git(repo, &["rev-parse", "HEAD"]);
        let second_short_sha = git(repo, &["rev-parse", "--short", "HEAD"]);

        run_git_factor(
            repo,
            &["--exec", "true", "HEAD~1^..HEAD"],
            GitFactorExpectation::default().stdout_suffix(expected_multi_commit_start_suffix(
                second_sha.as_str(),
                second_short_sha.as_str(),
                "[]",
            )),
        );
    }

    #[test]
    fn continue_completes_fully_staged_span_in_one_step() {
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
            GitFactorExpectation::default()
                .stdout_suffix(expected_completion_stdout_suffix("continue", 1))
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
            GitFactorExpectation::default()
                .stdout(expected_remaining_stdout("[]", "[\"new.txt\"]")),
        );
    }

    #[test]
    fn start_prints_untracked_when_target_commit_adds_file() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "base.txt", "one\n", "chore: base");
        commit_file(repo, "new.txt", "new\n", "feat: add file");
        let original = git(repo, &["rev-parse", "HEAD"]);
        let head_short_sha = git(repo, &["rev-parse", "--short", "HEAD"]);

        let expected = expected_single_commit_start_stdout(
            &original,
            &head_short_sha,
            "feat: add file",
        )
        .replace(
            "\"unstaged\":[{\"path\":\"file.txt\",\"kind\":\"text\",\"added\":1,\"deleted\":0}]",
            "\"unstaged\":[]",
        )
        .replace("\"untracked\":[]", "\"untracked\":[\"new.txt\"]");
        run_git_factor(
            repo,
            &["--exec", "true", "HEAD"],
            GitFactorExpectation::default().stdout(expected),
        );
        assert_eq!(
            read_journal(repo)
                .get("original_tip")
                .and_then(serde_json::Value::as_str),
            Some(original.as_str())
        );
        assert_eq!(fs::read(repo.join("new.txt")).or_abort(), b"new\n");
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
                .stdout_suffix(expected_completion_stdout_suffix("continue", 1))
                .git_output(&["rev-parse", "HEAD^{tree}"], expected_tree)
                .git_output_non_empty(&["ls-tree", root.as_str()]),
        );
    }

    #[test]
    fn continue_trace_logs_rebase_editor_env_pairs_for_empty_root_cleanup() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "feat: root");

        let trace_root = TempDir::new().or_abort();
        let trace_path = trace_root.path().join("rebase-env.jsonl");
        run_git_factor_with_env(
            repo,
            &["--exec", "true", "HEAD"],
            GitFactorExpectation::default(),
            "GIT_FACTOR_TRACE_LOG",
            trace_path.as_os_str(),
        );
        git(repo, &["add", "--all"]);
        run_git_factor_with_env(
            repo,
            &["--continue", "--message", "test: root split"],
            GitFactorExpectation::default()
                .stdout_suffix(expected_completion_stdout_suffix("continue", 1)),
            "GIT_FACTOR_TRACE_LOG",
            trace_path.as_os_str().to_os_string(),
        );

        let trace_content = fs::read_to_string(&trace_path).or_abort();
        assert!(
            trace_content.contains("GIT_EDITOR=true"),
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

        let original_tree = git(repo, &["rev-parse", "HEAD^{tree}"]);
        let unrelated_root = git(
            repo,
            &[
                "commit-tree",
                original_tree.as_str(),
                "-m",
                "Independent root",
            ],
        );
        git(
            repo,
            &[
                "update-ref",
                "refs/heads/independent",
                unrelated_root.as_str(),
            ],
        );
        start_session(repo);
        git(repo, &["add", "--all"]);

        let (wrap_dir, wrap_bin) = make_git_wrapper_named(
            "git",
            r#"if [ "${1-}" = "rev-list" ] && [ "${2-}" = "--max-parents=0" ] && [ "${3-}" = "HEAD" ]; then
  : > .git/unexpected-root-cleanup-query
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
                .stdout(expected_completion_stdout_suffix("continue", 1))
                .factor_state_exists(false)
                .rebase_merge_exists(false),
            prefixed_path,
        );
        assert_eq!(
            git(repo, &["rev-parse", "refs/heads/independent"]),
            unrelated_root
        );
        assert_eq!(git(repo, &["rev-parse", "HEAD^{tree}"]), original_tree);
        assert_eq!(git(repo, &["rev-list", "--count", "HEAD"]), "1");
        assert_eq!(
            git(repo, &["cat-file", "-p", "HEAD"])
                .lines()
                .filter(|line| line.starts_with("parent "))
                .count(),
            0
        );
        assert!(!git_dir(repo).join("unexpected-root-cleanup-query").exists());
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
            r#"if [ "${1-}" = "rebase" ] && [ "${2-}" = "--continue" ]; then
  echo "owned root replay refuses exit 77" >&2
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
                .code(EXIT_TEMPFAIL)
                .stdout(expected_recovery_stdout("continue"))
                .stderr_suffix("owned root replay refuses exit 77\n")
                .factor_state_exists(true)
                .rebase_merge_exists(true),
            prefixed_path,
        );
        assert_eq!(fs::read(repo.join("file.txt")).or_abort(), b"one\n");
        assert_eq!(
            read_journal(repo)
                .pointer("/state/phase")
                .and_then(serde_json::Value::as_str),
            Some("replaying")
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
            GitFactorExpectation::default()
                .stdout_suffix(expected_completion_stdout_suffix("continue", 1)),
            prefixed_path,
        );
    }

    #[test]
    #[expect(
        clippy::create_dir,
        reason = "Owned legacy fixture paths must be fresh; an existing path is a setup error"
    )]
    fn continue_errors_when_session_exists_but_no_rebase_is_active() {
        let directory = init_repo();
        let repo = directory.path();
        commit_file(repo, "file.txt", "one\n", "chore: base");
        let state = git_dir(repo).join("factor");
        fs::create_dir(&state).or_abort();
        fs::write(
            state.join("commits"),
            format!("{}\n", git(repo, &["rev-parse", "HEAD"])),
        )
        .or_abort();
        fs::write(state.join("current_index"), b"0\n").or_abort();
        fs::write(state.join("exec"), b"true\n").or_abort();
        fs::write(state.join("split_count"), b"0\n").or_abort();
        fs::write(state.join("requires_rebase"), b"false\n").or_abort();
        verify_legacy_session_refusal(repo, &["--continue", "--message", "test: legacy split"]);
    }

    #[test]
    #[expect(
        clippy::create_dir,
        reason = "Owned legacy fixture paths must be fresh; an existing path is a setup error"
    )]
    fn finish_errors_when_session_exists_but_no_rebase_is_active() {
        let directory = init_repo();
        let repo = directory.path();
        commit_file(repo, "file.txt", "one\n", "chore: base");
        let state = git_dir(repo).join("factor");
        fs::create_dir(&state).or_abort();
        fs::write(
            state.join("commits"),
            format!("{}\n", git(repo, &["rev-parse", "HEAD"])),
        )
        .or_abort();
        fs::write(state.join("current_index"), b"0\n").or_abort();
        fs::write(state.join("exec"), b"true\n").or_abort();
        fs::write(state.join("split_count"), b"0\n").or_abort();
        fs::write(state.join("requires_rebase"), b"false\n").or_abort();
        verify_legacy_session_refusal(repo, &["--finish", "--message", "test: legacy finish"]);
    }

    #[test]
    #[expect(
        clippy::create_dir,
        reason = "Owned legacy fixture paths must be fresh; an existing path is a setup error"
    )]
    fn continue_reports_corrupted_commits_state_file() {
        let directory = init_repo();
        let repo = directory.path();
        commit_file(repo, "file.txt", "one\n", "chore: base");
        let state = git_dir(repo).join("factor");
        fs::create_dir(&state).or_abort();
        fs::write(
            state.join("commits"),
            format!("{}\n", git(repo, &["rev-parse", "HEAD"])),
        )
        .or_abort();
        fs::write(state.join("current_index"), b"0\n").or_abort();
        fs::write(state.join("exec"), b"true\n").or_abort();
        fs::write(state.join("split_count"), b"0\n").or_abort();
        fs::write(state.join("requires_rebase"), b"false\n").or_abort();
        fs::write(state.join("commits"), b"\n").or_abort();
        fs::create_dir(git_dir(repo).join("rebase-merge")).or_abort();
        verify_legacy_session_refusal(repo, &["--continue", "--message", "test: legacy split"]);
    }

    #[test]
    #[expect(
        clippy::create_dir,
        reason = "Owned legacy fixture paths must be fresh; an existing path is a setup error"
    )]
    fn continue_reports_corrupted_requires_rebase_state() {
        let directory = init_repo();
        let repo = directory.path();
        commit_file(repo, "file.txt", "one\n", "chore: base");
        let state = git_dir(repo).join("factor");
        fs::create_dir(&state).or_abort();
        fs::write(
            state.join("commits"),
            format!("{}\n", git(repo, &["rev-parse", "HEAD"])),
        )
        .or_abort();
        fs::write(state.join("current_index"), b"0\n").or_abort();
        fs::write(state.join("exec"), b"true\n").or_abort();
        fs::write(state.join("split_count"), b"0\n").or_abort();
        fs::write(state.join("requires_rebase"), b"false\n").or_abort();
        fs::write(state.join("requires_rebase"), b"definitely-not-a-bool\n").or_abort();
        verify_legacy_session_refusal(repo, &["--continue", "--message", "test: legacy split"]);
    }

    #[test]
    #[expect(
        clippy::create_dir,
        reason = "Owned legacy fixture paths must be fresh; an existing path is a setup error"
    )]
    fn finish_reports_corrupted_requires_rebase_state() {
        let directory = init_repo();
        let repo = directory.path();
        commit_file(repo, "file.txt", "one\n", "chore: base");
        let state = git_dir(repo).join("factor");
        fs::create_dir(&state).or_abort();
        fs::write(
            state.join("commits"),
            format!("{}\n", git(repo, &["rev-parse", "HEAD"])),
        )
        .or_abort();
        fs::write(state.join("current_index"), b"0\n").or_abort();
        fs::write(state.join("exec"), b"true\n").or_abort();
        fs::write(state.join("split_count"), b"0\n").or_abort();
        fs::write(state.join("requires_rebase"), b"false\n").or_abort();
        fs::write(state.join("requires_rebase"), b"definitely-not-a-bool\n").or_abort();
        verify_legacy_session_refusal(repo, &["--finish", "--message", "test: legacy finish"]);
    }

    #[test]
    #[expect(
        clippy::create_dir,
        reason = "Owned legacy fixture paths must be fresh; an existing path is a setup error"
    )]
    fn continue_reports_unreadable_requires_rebase_state() {
        let directory = init_repo();
        let repo = directory.path();
        commit_file(repo, "file.txt", "one\n", "chore: base");
        let state = git_dir(repo).join("factor");
        fs::create_dir(&state).or_abort();
        fs::write(
            state.join("commits"),
            format!("{}\n", git(repo, &["rev-parse", "HEAD"])),
        )
        .or_abort();
        fs::write(state.join("current_index"), b"0\n").or_abort();
        fs::write(state.join("exec"), b"true\n").or_abort();
        fs::write(state.join("split_count"), b"0\n").or_abort();
        fs::write(state.join("requires_rebase"), b"false\n").or_abort();
        fs::remove_file(state.join("requires_rebase")).or_abort();
        fs::create_dir(state.join("requires_rebase")).or_abort();
        verify_legacy_session_refusal(repo, &["--continue", "--message", "test: legacy split"]);
    }

    #[test]
    #[expect(
        clippy::create_dir,
        reason = "Owned legacy fixture paths must be fresh; an existing path is a setup error"
    )]
    fn finish_reports_unreadable_requires_rebase_state() {
        let directory = init_repo();
        let repo = directory.path();
        commit_file(repo, "file.txt", "one\n", "chore: base");
        let state = git_dir(repo).join("factor");
        fs::create_dir(&state).or_abort();
        fs::write(
            state.join("commits"),
            format!("{}\n", git(repo, &["rev-parse", "HEAD"])),
        )
        .or_abort();
        fs::write(state.join("current_index"), b"0\n").or_abort();
        fs::write(state.join("exec"), b"true\n").or_abort();
        fs::write(state.join("split_count"), b"0\n").or_abort();
        fs::write(state.join("requires_rebase"), b"false\n").or_abort();
        fs::remove_file(state.join("requires_rebase")).or_abort();
        fs::create_dir(state.join("requires_rebase")).or_abort();
        verify_legacy_session_refusal(repo, &["--finish", "--message", "test: legacy finish"]);
    }

    #[test]
    #[expect(
        clippy::create_dir,
        reason = "Owned legacy fixture paths must be fresh; an existing path is a setup error"
    )]
    fn reports_invalid_numeric_state_files() {
        let directory = init_repo();
        let repo = directory.path();
        commit_file(repo, "file.txt", "one\n", "chore: base");
        let state = git_dir(repo).join("factor");
        fs::create_dir(&state).or_abort();
        fs::write(
            state.join("commits"),
            format!("{}\n", git(repo, &["rev-parse", "HEAD"])),
        )
        .or_abort();
        fs::write(state.join("current_index"), b"0\n").or_abort();
        fs::write(state.join("exec"), b"true\n").or_abort();
        fs::write(state.join("split_count"), b"0\n").or_abort();
        fs::write(state.join("requires_rebase"), b"false\n").or_abort();
        fs::write(state.join("current_index"), b"not-a-number\n").or_abort();
        fs::create_dir(git_dir(repo).join("rebase-merge")).or_abort();
        verify_legacy_session_refusal(repo, &["--continue", "--message", "test: legacy split"]);
    }

    #[test]
    #[expect(
        clippy::create_dir,
        reason = "Owned legacy fixture paths must be fresh; an existing path is a setup error"
    )]
    fn reports_out_of_range_commit_index() {
        let directory = init_repo();
        let repo = directory.path();
        commit_file(repo, "file.txt", "one\n", "chore: base");
        let state = git_dir(repo).join("factor");
        fs::create_dir(&state).or_abort();
        fs::write(
            state.join("commits"),
            format!("{}\n", git(repo, &["rev-parse", "HEAD"])),
        )
        .or_abort();
        fs::write(state.join("current_index"), b"0\n").or_abort();
        fs::write(state.join("exec"), b"true\n").or_abort();
        fs::write(state.join("split_count"), b"0\n").or_abort();
        fs::write(state.join("requires_rebase"), b"false\n").or_abort();
        fs::write(state.join("current_index"), b"1\n").or_abort();
        fs::create_dir(git_dir(repo).join("rebase-merge")).or_abort();
        verify_legacy_session_refusal(repo, &["--continue", "--message", "test: legacy split"]);
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
        write_file(repo, "file.txt", "bad\nstill bad\n");

        run_git_factor(
            repo,
            &["--continue", "--message", "test: slice"],
            GitFactorExpectation::default()
                .code(EXIT_TEMPFAIL)
                .stdout(expected_recovery_stdout("continue")),
        );
        let atom = read_journal(repo)
            .pointer("/state/atom")
            .and_then(serde_json::Value::as_str)
            .or_abort()
            .to_owned();
        assert_eq!(git(repo, &["show", &format!("{atom}:file.txt")]), "good");
        assert_eq!(
            fs::read(repo.join("file.txt")).or_abort(),
            b"bad\nstill bad\n"
        );
        assert_eq!(
            read_journal(repo)
                .pointer("/state/phase")
                .and_then(serde_json::Value::as_str),
            Some("replaying")
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
        let head = git(repo, &["rev-parse", "HEAD"]);
        let bin_dir = TempDir::new().or_abort();
        let (factor, _) = copy_bins_to(bin_dir.path());
        let bad_editor = bin_dir.path().join("git-sequence-editor");
        write_executable(
            &bad_editor,
            "#!/bin/sh\nprintf 'editor\\n' >> \"$(dirname \"$0\")/observed-editor\"\nprintf 'owned checkpoint editor refusal\\n' >&2\nexit 1\n",
        );
        let canonical_bad_editor = bad_editor.canonicalize().or_abort();
        let (wrapper_dir, wrapper_bin) = make_git_wrapper_named(
            "git",
            &format!(
                "if [ \"${{1-}}\" = \"-c\" ] && [ \"${{2-}}\" = \"rebase.missingCommitsCheck=ignore\" ] && [ \"${{3-}}\" = \"rebase\" ] && [ \"${{4-}}\" = \"--interactive\" ]; then\n  printf '%s\\n' \"$@\" >> \"$(dirname \"$0\")/observed-opening\"\n  export GIT_SEQUENCE_EDITOR={}\nfi\n",
                shell_quote(canonical_bad_editor.to_str().or_abort())
            ),
        );
        let _keep_alive = wrapper_dir;
        let observation = wrapper_bin.join("observed-opening");
        let mut expected = GitFactorExpectation::default()
            .code(EXIT_TEMPFAIL)
            .stdout(expected_recovery_stdout("start"))
            .stderr_suffix(format!(
                "error: there was a problem with the editor '{}'\n",
                canonical_bad_editor.display()
            ))
            .factor_state_exists(true)
            .rebase_merge_exists(false);
        expected.envs.push((
            "PATH".into(),
            format!("{}:{}", wrapper_bin.display(), env::var("PATH").or_abort()).into(),
        ));
        run_git_factor_with_bin(repo, &factor, &["--exec", "true", "HEAD~1"], expected);
        assert_eq!(git(repo, &["rev-parse", "HEAD"]), head);
        assert_eq!(fs::read(repo.join("file.txt")).or_abort(), b"one\ntwo\n");
        assert_eq!(
            read_journal(repo)
                .pointer("/state/phase")
                .and_then(serde_json::Value::as_str),
            Some("opening")
        );
        assert_eq!(
            fs::read(bin_dir.path().join("observed-editor")).or_abort(),
            b"editor\n"
        );
        assert_eq!(
            fs::read_to_string(observation).or_abort(),
            "-c\nrebase.missingCommitsCheck=ignore\nrebase\n--interactive\n--no-ff\n--reschedule-failed-exec\n--no-update-refs\n--no-autostash\n--no-autosquash\n--no-rebase-merges\n--empty=keep\n--keep-empty\n--root\n"
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
            r#"if [ "${1-}" = "-c" ] && [ "${3-}" = "commit" ]; then
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
                .stderr("git command failed: git -c failed (exit 1)\n"),
            prefixed_path,
        );
    }

    #[test]
    fn nonquiet_native_commit_stdout_is_forwarded_to_stderr() {
        let directory = init_repo();
        let repo = directory.path();
        commit_file(repo, "file.txt", "one\n", "chore: base");
        commit_file(repo, "file.txt", "one\ntwo\n", "feat: change");
        start_session(repo);
        git(repo, &["add", "file.txt"]);
        write_executable(
            &git_dir(repo).join("hooks/commit-msg"),
            "#!/bin/sh\nexit 1\n",
        );
        let (wrapper, bin) = make_git_wrapper_named(
            "git",
            "if [ \"${1-}\" = -c ] && [ \"${3-}\" = commit ]; then\n  printf 'native stdout\n'\n  printf 'native stderr\n' >&2\nfi\n",
        );
        let mut path = OsString::new();
        path.push(bin.as_os_str());
        path.push(OsStr::new(":"));
        path.push(env::var_os("PATH").or_abort());

        let mut expectation = GitFactorExpectation::default()
            .code(EXIT_SOFTWARE)
            .stderr("native stdout\nnative stderr\ngit command failed: git -c failed (exit 1)\n");
        expectation.stdout = Some(StreamExpectation::new_exact(String::new()));
        run_git_factor_with_prefixed_path(
            repo,
            &["--continue", "--message", "test: split"],
            expectation,
            path,
        );

        assert!(
            wrapper.path().exists(),
            "native wrapper remains alive for the Act"
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
            GitFactorExpectation::default().code(EXIT_SOFTWARE).stderr(
                "git command failed: git version: No such file or directory (os error 2)\n",
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
                .stderr_suffix("git command failed: fatal: short lookup failed\n"),
            "PATH",
            wrapped_path,
        );
    }

    #[test]
    fn start_reports_silent_git_output_exit_status() {
        let dir = init_repo();
        let repo = dir.path();
        commit_file(repo, "file.txt", "one\n", "chore: base");
        let (wrap_dir, wrap_bin) = make_git_wrapper_named(
            "git",
            r#"if [ "${1-}" = "rev-parse" ] && [ "${2-}" = "--short" ]; then
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
                .stderr_suffix("git command failed: exit status: 42\n"),
            "PATH",
            wrapped_path,
        );
    }

    #[test]
    fn start_reports_silent_git_output_signal() {
        let dir = init_repo();
        let repo = dir.path();
        commit_file(repo, "file.txt", "one\n", "chore: base");
        let (wrap_dir, wrap_bin) = make_git_wrapper_named(
            "git",
            r#"if [ "${1-}" = "rev-parse" ] && [ "${2-}" = "--short" ]; then
  kill -KILL "$$"
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
                .stderr_suffix("git command failed: signal: 9 (SIGKILL)\n"),
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

        let before = snapshot_selection(repo);
        run_git_factor(
            repo,
            &["--continue", "--message", "test: dir conflict"],
            GitFactorExpectation::default().code(EXIT_SOFTWARE).stderr(
                "git command failed: staged paths must belong to the remaining selected change\n",
            ),
        );
        assert_eq!(snapshot_selection(repo), before);
        assert!(repo.join("conflict").is_dir());
        assert_eq!(
            fs::read(repo.join("conflict/nested.txt")).or_abort(),
            b"ours\n"
        );
        assert_eq!(fs::read(repo.join("file.txt")).or_abort(), b"base\n");
    }

    #[test]
    fn continue_reports_rehydrate_conflicts_when_exec_gate_fails() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "base\n", "chore: base");
        commit_file(repo, "conflict", "theirs\n", "feat: add conflict file");

        // Start session, then force exec failure so rehydrate runs.
        start_session(repo);
        overwrite_session_exec(repo, "false");

        fs::remove_file(repo.join("conflict")).or_abort();
        fs::create_dir_all(repo.join("conflict")).or_abort();
        write_file(repo, "conflict/nested.txt", "ours\n");
        git(repo, &["add", "--all"]);

        let before = snapshot_selection(repo);
        run_git_factor(
            repo,
            &["--continue", "--message", "test: slice"],
            GitFactorExpectation::default().code(EXIT_SOFTWARE).stderr(
                "git command failed: staged paths must belong to the remaining selected change\n",
            ),
        );
        assert_eq!(snapshot_selection(repo), before);
        assert!(repo.join("conflict").is_dir());
        assert_eq!(
            fs::read(repo.join("conflict/nested.txt")).or_abort(),
            b"ours\n"
        );
        assert_eq!(fs::read(repo.join("file.txt")).or_abort(), b"base\n");
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
  : > .git/unexpected-cherry-pick
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
        write_file(repo, "file.txt", "base\nchange\n");
        let before = snapshot_selection(repo);

        run_git_factor_with_env(
            repo,
            &["--continue", "--message", "test: slice"],
            GitFactorExpectation::default()
                .code(EXIT_TEMPFAIL)
                .stderr_suffix("exec gate failed: false (exit code 1)\n"),
            "PATH",
            wrapped_path,
        );
        assert_eq!(snapshot_selection(repo), before);
        assert_eq!(
            fs::read(repo.join("file.txt")).or_abort(),
            b"base\nchange\n"
        );
        assert!(!git_dir(repo).join("unexpected-cherry-pick").exists());
    }

    #[test]
    fn continue_reports_remaining_selection_read_tree_failure() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "base\n", "chore: base");
        commit_file(repo, "file.txt", "base\nchange\n", "feat: change");

        // Refuse only the private remaining-selection observation, before gates or replay.
        let (wrap_dir, wrap_bin) = make_git_wrapper_named(
            "git",
            r#"if [ "$#" -eq 8 ] && [ "${1-}" = "-c" ] && [ "${2-}" = "core.ignorestat=false" ] && [ "${3-}" = "-c" ] && [ "${4-}" = "core.splitIndex=false" ] && [ "${5-}" = "-c" ] && [ "${6-}" = "core.sparseCheckout=false" ] && [ "${7-}" = "read-tree" ]; then
  case "${GIT_INDEX_FILE-}" in
    */selection-index)
      printf '%s\n' "$@" 'selection-index' >> "$(dirname "$0")/observed-selection"
      exit 1
      ;;
  esac
fi
"#,
        );
        let _keep_alive = wrap_dir;
        let observation = wrap_bin.join("observed-selection");

        let wrapped_path = format!("{}:{}", wrap_bin.display(), env::var("PATH").or_abort());
        start_session(repo);
        let source = read_journal(repo)
            .pointer("/state/source")
            .and_then(serde_json::Value::as_str)
            .or_abort()
            .to_owned();

        // Stage a slice while leaving the complete original source in the worktree.
        write_file(repo, "file.txt", "base\nslice\n");
        git(repo, &["add", "file.txt"]);
        write_file(repo, "file.txt", "base\nchange\n");
        let before = snapshot_selection(repo);

        let mut expectation = GitFactorExpectation::default()
            .code(EXIT_SOFTWARE)
            .stderr("git command failed: cannot observe the remaining selection without changing its index\n");
        expectation.stdout = Some(StreamExpectation::new_exact(String::new()));

        run_git_factor_with_env(
            repo,
            &["--continue", "--message", "test: slice"],
            expectation,
            "PATH",
            wrapped_path,
        );
        assert_eq!(
            fs::read_to_string(observation).or_abort(),
            format!(
                "-c\ncore.ignorestat=false\n-c\ncore.splitIndex=false\n-c\ncore.sparseCheckout=false\nread-tree\n{source}\nselection-index\n"
            )
        );
        assert_eq!(snapshot_selection(repo), before);
        assert_eq!(
            fs::read(repo.join("file.txt")).or_abort(),
            b"base\nchange\n"
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
        write_file(repo, "file.txt", "base\nchange\n");
        let before = snapshot_selection(repo);

        // Wrapper: make `git cherry-pick --no-commit` fail, but report no unmerged files.
        let (wrap_dir, wrap_bin) = make_git_wrapper_named(
            "git",
            r#"if [ "${1-}" = "cherry-pick" ] && [ "${2-}" = "--no-commit" ]; then
  : > .git/unexpected-cherry-pick
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
            GitFactorExpectation::default()
                .code(EXIT_TEMPFAIL)
                .stderr_suffix("exec gate failed: false (exit code 1)\n"),
            "PATH",
            format!("{}:{}", wrap_bin.display(), env::var("PATH").or_abort()),
        );
        assert_eq!(snapshot_selection(repo), before);
        assert_eq!(
            fs::read(repo.join("file.txt")).or_abort(),
            b"base\nchange\n"
        );
        assert!(!git_dir(repo).join("unexpected-cherry-pick").exists());
    }

    #[test]
    fn finish_reports_restore_failure_from_wrapper_injection() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");
        commit_file(repo, "file.txt", "one\ntwo\n", "feat: change");

        start_session(repo);

        // Refuse the real index priming step only after private selection admission succeeds.
        let (wrap_dir, wrap_bin) = make_git_wrapper_named(
            "git",
            r#"if [ "$#" -eq 2 ] && [ "${1-}" = "read-tree" ] && [ -z "${GIT_INDEX_FILE-}" ]; then
  printf '%s\n' "$@" 'real-index' >> "$(dirname "$0")/observed-restore"
  exit 1
fi
"#,
        );
        let _keep_alive = wrap_dir;
        let observation = wrap_bin.join("observed-restore");
        let source = read_journal(repo)
            .pointer("/state/source")
            .and_then(serde_json::Value::as_str)
            .or_abort()
            .to_owned();

        let before = snapshot_selection(repo);
        let mut expectation = GitFactorExpectation::default()
            .code(EXIT_SOFTWARE)
            .stderr("git command failed: git read-tree failed (exit 1)\n");
        expectation.stdout = Some(StreamExpectation::new_exact(String::new()));

        run_git_factor_with_env(
            repo,
            &["--finish", "--message", "test: finish"],
            expectation,
            "PATH",
            format!("{}:{}", wrap_bin.display(), env::var("PATH").or_abort()),
        );
        assert_eq!(
            fs::read_to_string(observation).or_abort(),
            format!("read-tree\n{source}\nreal-index\n")
        );
        assert_eq!(snapshot_selection(repo), before);
        assert_eq!(fs::read(repo.join("file.txt")).or_abort(), b"one\ntwo\n");
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
        let head = git(repo, &["rev-parse", "HEAD"]);
        let refs = git(repo, &["show-ref"]);
        let index = fs::read(git_dir(repo).join("index")).or_abort();
        let (wrap_dir, wrap_bin) = make_git_wrapper_named(
            "git",
            r#"if [ "${1-}" = "restore" ] && [ "${2-}" = "--source" ]; then
  : > .git/unexpected-restore
  exit 1
fi
"#,
        );
        let _keep_alive = wrap_dir;
        let path = format!("{}:{}", wrap_bin.display(), env::var("PATH").or_abort());
        run_git_factor_with_env(repo, &["--exec", "true", "HEAD"], GitFactorExpectation::default().code(EXIT_DATAERR).stdout("{\"operation\":\"start\",\"reason\":\"empty_change\",\"result\":\"refused\"}\n").factor_state_exists(false).rebase_merge_exists(false), "PATH", path.clone());
        run_git_factor_with_env(
            repo,
            &["--finish"],
            GitFactorExpectation::default()
                .code(EXIT_USAGE)
                .stderr("no active factor session\n"),
            "PATH",
            path,
        );
        assert_eq!(git(repo, &["rev-parse", "HEAD"]), head);
        assert_eq!(git(repo, &["show-ref"]), refs);
        assert_eq!(fs::read(git_dir(repo).join("index")).or_abort(), index);
        assert_eq!(fs::read(repo.join("file.txt")).or_abort(), b"one\n");
        assert_eq!(
            git(repo, &["show", "--format=%B", "--no-patch", "HEAD"]),
            ""
        );
        assert!(!git_dir(repo).join("unexpected-restore").exists());
    }

    #[test]
    fn start_rejects_empty_original_message_before_session_publication() {
        let dir = init_repo();
        let repo = dir.path();
        commit_file(repo, "file.txt", "one\n", "chore: base");
        write_file(repo, "file.txt", "one\ntwo\n");
        git(repo, &["add", "file.txt"]);
        git(repo, &["commit", "--allow-empty-message", "--message", ""]);
        write_file(repo, "unrelated.txt", "preserve user bytes\n");
        let head = git(repo, &["rev-parse", "HEAD"]);
        let index = fs::read(git_dir(repo).join("index")).or_abort();
        let refs = git(repo, &["show-ref"]);

        let mut expectation = GitFactorExpectation::default()
            .code(EXIT_SOFTWARE)
            .stderr("git command failed: empty candidate source message: value must not be empty\n")
            .factor_state_exists(false)
            .rebase_merge_exists(false)
            .rebase_apply_exists(false);
        expectation.stdout = Some(StreamExpectation::new_exact(String::new()));

        run_git_factor(repo, &["--exec", "true", "HEAD"], expectation);

        assert_eq!(git(repo, &["rev-parse", "HEAD"]), head);
        assert_eq!(fs::read(git_dir(repo).join("index")).or_abort(), index);
        assert_eq!(git(repo, &["show-ref"]), refs);
        assert_eq!(
            git(repo, &["show", "--format=%B", "--no-patch", "HEAD"]),
            ""
        );
        assert_eq!(fs::read(repo.join("file.txt")).or_abort(), b"one\ntwo\n");
        assert_eq!(
            fs::read(repo.join("unrelated.txt")).or_abort(),
            b"preserve user bytes\n"
        );
        assert!(!git_dir(repo).join("factor").exists());
    }

    #[test]
    fn finish_reports_tree_hash_mismatch_via_write_tree_wrapper() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");
        commit_file(repo, "file.txt", "one\ntwo\n", "feat: change");

        let actual_tree = git(repo, &["rev-parse", "HEAD~1^{tree}"]);
        start_session(repo);
        let expected_tree = read_journal(repo)
            .get("final_tree")
            .and_then(serde_json::Value::as_str)
            .or_abort()
            .to_owned();
        assert_ne!(expected_tree, actual_tree);
        git(repo, &["read-tree", expected_tree.as_str()]);
        // Falsify only the remaining selection's private observed tree with a real base tree.
        let (wrap_dir, wrap_bin) = make_git_wrapper_named(
            "git",
            &format!(
                r#"if [ "$#" -eq 3 ] && [ "${{1-}}" = "-c" ] && [ "${{2-}}" = "core.splitIndex=false" ] && [ "${{3-}}" = "write-tree" ]; then
  case "${{GIT_INDEX_FILE-}}" in
    */selection-index)
      printf '%s\n' "$@" 'selection-index' >> "$(dirname "$0")/observed-tree"
      printf '%s\n' '{actual_tree}'
      exit 0
      ;;
  esac
fi
"#
            ),
        );
        let _keep_alive = wrap_dir;
        let observation = wrap_bin.join("observed-tree");
        let wrapped_path = format!("{}:{}", wrap_bin.display(), env::var("PATH").or_abort());
        let diagnostic =
            format!("tree hash mismatch: expected {expected_tree}, got {actual_tree}\n");
        let before = snapshot_selection(repo);

        let mut expectation = GitFactorExpectation::default()
            .code(EXIT_TEMPFAIL)
            .stderr(diagnostic);
        expectation.stdout = Some(StreamExpectation::new_exact(String::new()));

        run_git_factor_with_env(
            repo,
            &["--finish", "--message", "test: finish"],
            expectation,
            "PATH",
            wrapped_path,
        );
        assert_eq!(
            fs::read_to_string(observation).or_abort(),
            "-c\ncore.splitIndex=false\nwrite-tree\nselection-index\n"
        );
        assert_eq!(snapshot_selection(repo), before);
        assert_eq!(fs::read(repo.join("file.txt")).or_abort(), b"one\ntwo\n");
        assert_eq!(
            read_journal(repo)
                .get("final_tree")
                .and_then(serde_json::Value::as_str),
            Some(expected_tree.as_str())
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
            GitFactorExpectation::default()
                .code(EXIT_SOFTWARE)
                .stderr("git command failed: fatal: unexpected original tree lookup\n"),
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
        write_file(repo, "file.txt", "base\nchange\n");

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
    fn continue_reports_remaining_selection_tree_hash_mismatch() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "base\n", "chore: base");
        commit_file(repo, "file.txt", "base\nchange\n", "feat: change");

        let actual_tree = git(repo, &["rev-parse", "HEAD~1^{tree}"]);
        start_session(repo);
        write_file(repo, "file.txt", "base\nslice\n");
        git(repo, &["add", "file.txt"]);
        write_file(repo, "file.txt", "base\nchange\n");

        let expected_tree = read_journal(repo)
            .get("final_tree")
            .and_then(serde_json::Value::as_str)
            .or_abort()
            .to_owned();
        assert_ne!(expected_tree, actual_tree);
        // Falsify only the remaining selection's private observed tree with a real base tree.
        let (wrap_dir, wrap_bin) = make_git_wrapper_named(
            "git",
            &format!(
                r#"if [ "$#" -eq 3 ] && [ "${{1-}}" = "-c" ] && [ "${{2-}}" = "core.splitIndex=false" ] && [ "${{3-}}" = "write-tree" ]; then
  case "${{GIT_INDEX_FILE-}}" in
    */selection-index)
      printf '%s\n' "$@" 'selection-index' >> "$(dirname "$0")/observed-tree"
      printf '%s\n' '{actual_tree}'
      exit 0
      ;;
  esac
fi
"#
            ),
        );
        let _keep_alive = wrap_dir;
        let observation = wrap_bin.join("observed-tree");
        let wrapped_path = format!("{}:{}", wrap_bin.display(), env::var("PATH").or_abort());
        let diagnostic =
            format!("tree hash mismatch: expected {expected_tree}, got {actual_tree}\n");
        let before = snapshot_selection(repo);

        let mut expectation = GitFactorExpectation::default()
            .code(EXIT_TEMPFAIL)
            .stderr(diagnostic);
        expectation.stdout = Some(StreamExpectation::new_exact(String::new()));

        run_git_factor_with_env(
            repo,
            &["--continue", "--message", "test: partial split"],
            expectation,
            "PATH",
            wrapped_path,
        );
        assert_eq!(
            fs::read_to_string(observation).or_abort(),
            "-c\ncore.splitIndex=false\nwrite-tree\nselection-index\n"
        );
        assert_eq!(snapshot_selection(repo), before);
        assert_eq!(
            fs::read(repo.join("file.txt")).or_abort(),
            b"base\nchange\n"
        );
        assert_eq!(
            read_journal(repo)
                .get("final_tree")
                .and_then(serde_json::Value::as_str),
            Some(expected_tree.as_str())
        );
    }

    #[test]
    fn finish_reports_error_when_expected_tree_lookup_fails() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");
        commit_file(repo, "file.txt", "one\ntwo\n", "feat: change");

        start_session(repo);

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
        let paused = Command::new(native_git_bin())
            .args(["rebase", "--root", "--exec", "false"])
            .current_dir(repo)
            .output()
            .or_abort();
        assert!(!paused.status.success());
        let head = git(repo, &["rev-parse", "HEAD"]);
        let refs = git(repo, &["show-ref"]);
        let index = fs::read(git_dir(repo).join("index")).or_abort();
        let native = legacy_inventory(&git_dir(repo).join("rebase-merge"));
        fs::write(repo.join("unrelated.txt"), "preserve external work\n").or_abort();
        let (wrap_dir, wrap_bin) = make_git_wrapper_named(
            "git",
            r#"if [ "${1-}" = "rebase" ] && [ "${2-}" = "--abort" ]; then
  : > .git/unexpected-factor-abort
  exit 1
fi
"#,
        );
        let _keep_alive = wrap_dir;
        run_git_factor_with_env(
            repo,
            &["--abort"],
            GitFactorExpectation::default()
                .code(EXIT_USAGE)
                .stderr("no active factor session\n")
                .rebase_merge_exists(true)
                .factor_state_exists(false),
            "PATH",
            format!("{}:{}", wrap_bin.display(), env::var("PATH").or_abort()),
        );
        assert_eq!(git(repo, &["rev-parse", "HEAD"]), head);
        assert_eq!(git(repo, &["show-ref"]), refs);
        assert_eq!(fs::read(git_dir(repo).join("index")).or_abort(), index);
        assert_eq!(
            legacy_inventory(&git_dir(repo).join("rebase-merge")),
            native
        );
        assert_eq!(fs::read(repo.join("file.txt")).or_abort(), b"one\n");
        assert_eq!(
            fs::read(repo.join("unrelated.txt")).or_abort(),
            b"preserve external work\n"
        );
        assert!(!git_dir(repo).join("unexpected-factor-abort").exists());
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
                "git command failed: tracked working tree and staged tree must match HEAD\n",
            ),
        );
    }

    #[test]
    #[expect(
        clippy::create_dir,
        reason = "Owned legacy fixture paths must be fresh; an existing path is a setup error"
    )]
    fn abort_falls_back_to_current_commit_when_start_head_state_is_missing() {
        let directory = init_repo();
        let repo = directory.path();
        commit_file(repo, "file.txt", "one\n", "chore: base");
        let state = git_dir(repo).join("factor");
        fs::create_dir(&state).or_abort();
        fs::write(
            state.join("commits"),
            format!("{}\n", git(repo, &["rev-parse", "HEAD"])),
        )
        .or_abort();
        fs::write(state.join("current_index"), b"0\n").or_abort();
        fs::write(state.join("exec"), b"true\n").or_abort();
        fs::write(state.join("split_count"), b"0\n").or_abort();
        fs::write(state.join("requires_rebase"), b"false\n").or_abort();
        verify_legacy_session_refusal(repo, &["--abort"]);
    }

    #[test]
    fn continue_reports_truncated_commit_metadata_from_raw_commit() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");
        commit_file(repo, "file.txt", "one\ntwo\n", "feat: change");

        start_session(repo);
        git(repo, &["add", "--all"]);

        let (wrap_dir, wrap_bin) = make_git_wrapper_named(
            "git",
            r#"if [ "${1-}" = "cat-file" ] && [ "${2-}" = "commit" ]; then
  printf "only-one-field\n"
  exit 0
fi
"#,
        );
        let _keep_alive = wrap_dir;

        run_git_factor_with_env(
            repo,
            &["--continue", "--message", "test: split"],
            GitFactorExpectation::default()
                .code(EXIT_SOFTWARE)
                .stderr("git command failed: truncated candidate author metadata\n"),
            "PATH",
            format!("{}:{}", wrap_bin.display(), env::var("PATH").or_abort()),
        );
    }

    #[test]
    fn start_preserves_foreign_scratch_file_when_rebase_launch_fails() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");
        commit_file(repo, "file.txt", "one\ntwo\n", "feat: change");

        write_file(repo, "unrelated.txt", "preserve opening user bytes\n");
        let before_head = git(repo, &["rev-parse", "HEAD"]);
        let before_index = fs::read(git_dir(repo).join("index")).or_abort();
        let before_refs = git(repo, &["show-ref", "--heads", "--tags"]);
        let before_file = fs::read(repo.join("file.txt")).or_abort();
        let (wrap_dir, wrap_bin) = make_git_wrapper_named(
            "git",
            r#"if [ "$#" -eq 13 ] && [ "${1-}" = "-c" ] && [ "${2-}" = "rebase.missingCommitsCheck=ignore" ] && [ "${3-}" = "rebase" ] && [ "${4-}" = "--interactive" ]; then
  printf '%s\n' "$@" >> "$(dirname "$0")/observed-opening"
  rm -rf .git/factor
  : > .git/factor
  exit 1
fi
"#,
        );
        let expected_base = "--root".to_owned();
        let _keep_alive = wrap_dir;
        let observation = wrap_bin.join("observed-opening");

        run_git_factor_with_env(
            repo,
            &["--exec", "true", "HEAD~1"],
            GitFactorExpectation::default()
                .code(EXIT_TEMPFAIL)
                .stdout(expected_recovery_stdout("start"))
                .factor_state_exists(true)
                .path_exists(".git/factor", true),
            "PATH",
            format!("{}:{}", wrap_bin.display(), env::var("PATH").or_abort()),
        );
        assert!(git_dir(repo).join("factor").is_file());
        assert_eq!(fs::read(git_dir(repo).join("factor")).or_abort(), b"");
        assert!(checkpoint_journal(repo).is_file());
        assert_eq!(
            fs::read_to_string(observation).or_abort(),
            format!(
                "-c\nrebase.missingCommitsCheck=ignore\nrebase\n--interactive\n--no-ff\n--reschedule-failed-exec\n--no-update-refs\n--no-autostash\n--no-autosquash\n--no-rebase-merges\n--empty=keep\n--keep-empty\n{expected_base}\n"
            )
        );
        assert_eq!(git(repo, &["rev-parse", "HEAD"]), before_head);
        assert_eq!(
            fs::read(git_dir(repo).join("index")).or_abort(),
            before_index
        );
        assert_eq!(git(repo, &["show-ref", "--heads", "--tags"]), before_refs);
        assert_eq!(fs::read(repo.join("file.txt")).or_abort(), before_file);
        assert_eq!(
            fs::read(repo.join("unrelated.txt")).or_abort(),
            b"preserve opening user bytes\n"
        );
        assert_eq!(
            read_journal(repo)
                .pointer("/state/phase")
                .and_then(serde_json::Value::as_str),
            Some("opening")
        );
        assert!(!git_dir(repo).join("rebase-merge").exists());
        assert!(!git_dir(repo).join("rebase-apply").exists());
    }

    #[test]
    fn abort_removes_state_path_when_clean_rewrites_state_dir_as_file() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");
        commit_file(repo, "file.txt", "one\ntwo\n", "feat: change");
        start_session(repo);
        let checkpoint = read_journal(repo)
            .get("checkpoint")
            .and_then(serde_json::Value::as_str)
            .or_abort()
            .to_owned();
        let original_tree = git(repo, &["rev-parse", &format!("{checkpoint}^{{tree}}")]);
        fs::write(repo.join("unrelated.txt"), "preserve abort scratch\n").or_abort();

        let (wrap_dir, wrap_bin) = make_git_wrapper_named(
            "git",
            r#"if [ "${1-}" = "clean" ]; then
  : > .git/unexpected-clean
  rm -rf .git/factor
  : > .git/factor
fi
if [ "${1-}" = "reset" ] && [ "${2-}" = "--hard" ]; then
  : > .git/unexpected-manual-reset
  exit 1
fi
"#,
        );
        let _keep_alive = wrap_dir;

        run_git_factor_with_env(
            repo,
            &["--abort"],
            GitFactorExpectation::default()
                .stdout(
                    "{\"operation\":\"abort\",\"rebase\":{\"in_progress\":false},\"actions\":{}}\n",
                )
                .factor_state_exists(false)
                .path_exists(".git/factor", false),
            "PATH",
            format!("{}:{}", wrap_bin.display(), env::var("PATH").or_abort()),
        );
        assert_eq!(git(repo, &["rev-parse", "HEAD"]), checkpoint);
        assert_eq!(git(repo, &["rev-parse", "HEAD^{tree}"]), original_tree);
        assert_eq!(fs::read(repo.join("file.txt")).or_abort(), b"one\ntwo\n");
        assert_eq!(
            fs::read(repo.join("unrelated.txt")).or_abort(),
            b"preserve abort scratch\n"
        );
        assert!(!git_dir(repo).join("unexpected-clean").exists());
        assert!(!git_dir(repo).join("unexpected-manual-reset").exists());
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
        commit_file(repo, "base.txt", "base\na\nb\nc\n", "feat: c");

        run_git_factor(
            repo,
            &["--exec", "true", "HEAD~2", "HEAD~1"],
            GitFactorExpectation::default(),
        );

        let (wrap_dir, wrap_bin) = make_git_wrapper_named(
            "git",
            r#"if [ "${1-}" = "rebase" ] && [ "${2-}" = "--continue" ]; then
  rm -rf .git/factor
  : > .git/factor
fi
"#,
        );
        let _keep_alive = wrap_dir;

        git(repo, &["add", "--all"]);
        run_git_factor_with_env(
            repo,
            &["--continue", "--message", "test: first"],
            GitFactorExpectation::default()
                .code(EXIT_TEMPFAIL)
                .stdout(expected_recovery_stdout("continue"))
                .factor_state_exists(true)
                .path_exists(".git/factor", true),
            "PATH",
            format!("{}:{}", wrap_bin.display(), env::var("PATH").or_abort()),
        );
        assert!(git_dir(repo).join("factor").is_file());
        assert_eq!(fs::read(git_dir(repo).join("factor")).or_abort(), b"");
        assert!(checkpoint_journal(repo).is_file());
    }

    #[test]
    fn continue_reports_rebase_continue_failure_with_recovery_hint() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "base.txt", "base\n", "chore: base");
        commit_file(repo, "base.txt", "base\na\n", "feat: a");
        commit_file(repo, "base.txt", "base\na\nb\n", "feat: b");
        commit_file(repo, "base.txt", "base\na\nb\nc\n", "feat: c");

        run_git_factor(
            repo,
            &["--exec", "true", "HEAD~2", "HEAD~1"],
            GitFactorExpectation::default(),
        );

        let (wrap_dir, wrap_bin) = make_git_wrapper_named(
            "git",
            r#"if [ "${1-}" = "rebase" ] && [ "${2-}" = "--continue" ]; then
  echo 'owned native replay failure' >&2
  exit 1
fi
"#,
        );
        let _keep_alive = wrap_dir;

        git(repo, &["add", "--all"]);
        run_git_factor_with_env(
            repo,
            &["--continue", "--message", "test: first"],
            GitFactorExpectation::default()
                .code(EXIT_TEMPFAIL)
                .stderr_suffix("owned native replay failure\n")
                .stdout(expected_recovery_stdout("continue"))
                .factor_state_exists(true)
                .rebase_merge_exists(true),
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
  : > .git/unexpected-deletion-query
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
            repo.join("deleted-path.txt").exists(),
            "unrelated deletion-list bytes must remain outside selection ownership"
        );
        assert_eq!(
            fs::read(repo.join("deleted-path.txt")).or_abort(),
            b"ephemeral\n"
        );
        assert!(!git_dir(repo).join("unexpected-deletion-query").exists());
    }

    #[test]
    #[expect(
        clippy::create_dir,
        reason = "Owned legacy fixture paths must be fresh; an existing path is a setup error"
    )]
    fn abort_skips_rebase_abort_when_started_rebase_is_true_without_mid_rebase() {
        let directory = init_repo();
        let repo = directory.path();
        commit_file(repo, "file.txt", "one\n", "chore: base");
        let state = git_dir(repo).join("factor");
        fs::create_dir(&state).or_abort();
        fs::write(
            state.join("commits"),
            format!("{}\n", git(repo, &["rev-parse", "HEAD"])),
        )
        .or_abort();
        fs::write(state.join("current_index"), b"0\n").or_abort();
        fs::write(state.join("exec"), b"true\n").or_abort();
        fs::write(state.join("split_count"), b"0\n").or_abort();
        fs::write(state.join("requires_rebase"), b"false\n").or_abort();
        fs::write(state.join("started_rebase"), b"true\n").or_abort();
        fs::write(state.join("requires_rebase"), b"true\n").or_abort();
        verify_legacy_session_refusal(repo, &["--abort"]);
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
    #[expect(
        clippy::create_dir,
        reason = "Owned legacy fixture paths must be fresh; an existing path is a setup error"
    )]
    fn abort_reports_unreadable_requires_rebase_state() {
        let directory = init_repo();
        let repo = directory.path();
        commit_file(repo, "file.txt", "one\n", "chore: base");
        let state = git_dir(repo).join("factor");
        fs::create_dir(&state).or_abort();
        fs::write(
            state.join("commits"),
            format!("{}\n", git(repo, &["rev-parse", "HEAD"])),
        )
        .or_abort();
        fs::write(state.join("current_index"), b"0\n").or_abort();
        fs::write(state.join("exec"), b"true\n").or_abort();
        fs::write(state.join("split_count"), b"0\n").or_abort();
        fs::write(state.join("requires_rebase"), b"false\n").or_abort();
        fs::remove_file(state.join("requires_rebase")).or_abort();
        fs::create_dir(state.join("requires_rebase")).or_abort();
        verify_legacy_session_refusal(repo, &["--abort"]);
    }

    #[test]
    #[expect(
        clippy::create_dir,
        reason = "Owned legacy fixture paths must be fresh; an existing path is a setup error"
    )]
    fn abort_reports_unreadable_started_rebase_state() {
        let directory = init_repo();
        let repo = directory.path();
        commit_file(repo, "file.txt", "one\n", "chore: base");
        let state = git_dir(repo).join("factor");
        fs::create_dir(&state).or_abort();
        fs::write(
            state.join("commits"),
            format!("{}\n", git(repo, &["rev-parse", "HEAD"])),
        )
        .or_abort();
        fs::write(state.join("current_index"), b"0\n").or_abort();
        fs::write(state.join("exec"), b"true\n").or_abort();
        fs::write(state.join("split_count"), b"0\n").or_abort();
        fs::write(state.join("requires_rebase"), b"false\n").or_abort();
        fs::create_dir(state.join("started_rebase")).or_abort();
        verify_legacy_session_refusal(repo, &["--abort"]);
    }

    #[test]
    #[expect(
        clippy::create_dir,
        reason = "Owned legacy fixture paths must be fresh; an existing path is a setup error"
    )]
    fn abort_reports_missing_current_commit_when_start_head_is_missing() {
        let directory = init_repo();
        let repo = directory.path();
        commit_file(repo, "file.txt", "one\n", "chore: base");
        let state = git_dir(repo).join("factor");
        fs::create_dir(&state).or_abort();
        fs::write(
            state.join("commits"),
            format!("{}\n", git(repo, &["rev-parse", "HEAD"])),
        )
        .or_abort();
        fs::write(state.join("current_index"), b"0\n").or_abort();
        fs::write(state.join("exec"), b"true\n").or_abort();
        fs::write(state.join("split_count"), b"0\n").or_abort();
        fs::write(state.join("requires_rebase"), b"false\n").or_abort();
        fs::remove_file(state.join("commits")).or_abort();
        verify_legacy_session_refusal(repo, &["--abort"]);
    }

    #[test]
    fn abort_reports_reset_failure() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");
        commit_file(repo, "file.txt", "one\ntwo\n", "feat: change");
        start_session(repo);
        let checkpoint = read_journal(repo)
            .get("checkpoint")
            .and_then(serde_json::Value::as_str)
            .or_abort()
            .to_owned();
        let original_tree = git(repo, &["rev-parse", &format!("{checkpoint}^{{tree}}")]);
        fs::write(repo.join("unrelated.txt"), "preserve abort scratch\n").or_abort();

        let (wrap_dir, wrap_bin) = make_git_wrapper_named(
            "git",
            r#"if [ "${1-}" = "reset" ] && [ "${2-}" = "--hard" ]; then
  : > .git/unexpected-manual-reset
  exit 1
fi
if [ "${1-}" = "clean" ]; then
  : > .git/unexpected-clean
  exit 1
fi
"#,
        );
        let _keep_alive = wrap_dir;

        run_git_factor_with_env(
            repo,
            &["--abort"],
            GitFactorExpectation::default().stdout(
                "{\"operation\":\"abort\",\"rebase\":{\"in_progress\":false},\"actions\":{}}\n",
            ),
            "PATH",
            format!("{}:{}", wrap_bin.display(), env::var("PATH").or_abort()),
        );
        assert_eq!(git(repo, &["rev-parse", "HEAD"]), checkpoint);
        assert_eq!(git(repo, &["rev-parse", "HEAD^{tree}"]), original_tree);
        assert_eq!(fs::read(repo.join("file.txt")).or_abort(), b"one\ntwo\n");
        assert_eq!(
            fs::read(repo.join("unrelated.txt")).or_abort(),
            b"preserve abort scratch\n"
        );
        assert!(!git_dir(repo).join("unexpected-clean").exists());
        assert!(!git_dir(repo).join("unexpected-manual-reset").exists());
    }

    #[test]
    fn abort_reports_clean_failure() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");
        commit_file(repo, "file.txt", "one\ntwo\n", "feat: change");
        start_session(repo);
        let checkpoint = read_journal(repo)
            .get("checkpoint")
            .and_then(serde_json::Value::as_str)
            .or_abort()
            .to_owned();
        let original_tree = git(repo, &["rev-parse", &format!("{checkpoint}^{{tree}}")]);
        fs::write(repo.join("unrelated.txt"), "preserve abort scratch\n").or_abort();

        let (wrap_dir, wrap_bin) = make_git_wrapper_named(
            "git",
            r#"if [ "${1-}" = "clean" ] && [ "${2-}" = "--force" ]; then
  : > .git/unexpected-clean
  exit 1
fi
if [ "${1-}" = "reset" ] && [ "${2-}" = "--hard" ]; then
  : > .git/unexpected-manual-reset
  exit 1
fi
"#,
        );
        let _keep_alive = wrap_dir;

        run_git_factor_with_env(
            repo,
            &["--abort"],
            GitFactorExpectation::default().stdout(
                "{\"operation\":\"abort\",\"rebase\":{\"in_progress\":false},\"actions\":{}}\n",
            ),
            "PATH",
            format!("{}:{}", wrap_bin.display(), env::var("PATH").or_abort()),
        );
        assert_eq!(git(repo, &["rev-parse", "HEAD"]), checkpoint);
        assert_eq!(git(repo, &["rev-parse", "HEAD^{tree}"]), original_tree);
        assert_eq!(fs::read(repo.join("file.txt")).or_abort(), b"one\ntwo\n");
        assert_eq!(
            fs::read(repo.join("unrelated.txt")).or_abort(),
            b"preserve abort scratch\n"
        );
        assert!(!git_dir(repo).join("unexpected-clean").exists());
        assert!(!git_dir(repo).join("unexpected-manual-reset").exists());
    }

    #[test]
    fn postconditions_helper_accepts_default_expectation() {
        let dir = init_repo();
        assert_git_factor_postconditions(dir.path(), GitFactorExpectation::default());
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
    #[test]
    fn message_only_requires_an_active_session() {
        let dir = init_repo();
        let repo = dir.path();

        commit_file(repo, "file.txt", "one\n", "chore: base");

        run_git_factor(
            repo,
            &["--message", "test: msg"],
            GitFactorExpectation::default()
                .code(EXIT_USAGE)
                .stderr("no active factor session\n"),
        );
    }

    #[test]
    fn finish_preserves_unrelated_branch_during_descendant_replay() {
        let dir = init_repo();
        let repo = dir.path();
        commit_file(repo, "base.txt", "base\n", "Add base");
        write_file(repo, "first.txt", "first\n");
        write_file(repo, "second.txt", "second\n");
        git(repo, &["add", "first.txt", "second.txt"]);
        git(repo, &["commit", "--message", "Add selected changes"]);
        let selected = git(repo, &["rev-parse", "HEAD"]);
        git(repo, &["branch", "other", selected.as_str()]);
        commit_file(repo, "later.txt", "later\n", "Add descendant");
        let original_tree = git(repo, &["rev-parse", "HEAD^{tree}"]);
        let original_branch = git(repo, &["symbolic-ref", "HEAD"]);
        git(repo, &["config", "rebase.updateRefs", "true"]);

        // Arrange a partially split session with a descendant still awaiting replay.
        let started = Command::new(git_factor_bin())
            .current_dir(repo)
            .args(["--exec", "true", selected.as_str()])
            .output()
            .or_abort();
        assert_eq!(started.status.code(), Some(EXIT_OK));
        git(repo, &["add", "first.txt"]);
        let submitted = Command::new(git_factor_bin())
            .current_dir(repo)
            .args(["--continue", "--message", "Add first change"])
            .output()
            .or_abort();
        assert_eq!(submitted.status.code(), Some(EXIT_OK));
        assert!(git_dir(repo).join("rebase-merge").exists());
        assert_eq!(git(repo, &["rev-parse", "refs/heads/other"]), selected);

        let finished = Command::new(git_factor_bin())
            .current_dir(repo)
            .args(["--finish"])
            .output()
            .or_abort();

        assert_eq!(finished.status.code(), Some(EXIT_OK));
        assert_eq!(
            finished.stdout,
            b"{\"operation\":\"finish\",\"result\":\"complete\",\"split_count\":2}\n",
        );
        assert!(
            String::from_utf8_lossy(&finished.stderr).contains("Successfully rebased and updated")
        );
        assert_eq!(git(repo, &["rev-parse", "refs/heads/other"]), selected);
        assert_eq!(git(repo, &["rev-parse", "HEAD^{tree}"]), original_tree);
        assert_eq!(git(repo, &["write-tree"]), original_tree);
        assert_eq!(git(repo, &["symbolic-ref", "HEAD"]), original_branch);
        assert_eq!(git(repo, &["status", "--porcelain"]), "");
        assert_eq!(git(repo, &["rev-list", "--count", "HEAD"]), "4");
        assert_eq!(git(repo, &["log", "-1", "--format=%s"]), "Add descendant");
        assert!(!git_dir(repo).join("factor").exists());
        assert!(!git_dir(repo).join("rebase-merge").exists());
        assert!(!git_dir(repo).join("rebase-apply").exists());
    }

    #[test]
    fn finish_preserves_branch_created_before_empty_root_cleanup() {
        let dir = init_repo();
        let repo = dir.path();
        write_file(repo, "first.txt", "first\n");
        write_file(repo, "second.txt", "second\n");
        git(repo, &["add", "first.txt", "second.txt"]);
        git(repo, &["commit", "--message", "Add root changes"]);
        let original_tree = git(repo, &["rev-parse", "HEAD^{tree}"]);
        let original_branch = git(repo, &["symbolic-ref", "HEAD"]);
        git(repo, &["config", "rebase.updateRefs", "true"]);

        // A completed root atom is parentless; the remainder opens a fresh native round.
        start_session(repo);
        git(repo, &["add", "first.txt"]);
        let submitted = Command::new(git_factor_bin())
            .current_dir(repo)
            .args(["--continue", "--message", "Add first change"])
            .output()
            .or_abort();
        assert_eq!(submitted.status.code(), Some(EXIT_OK));
        let accepted = git(repo, &["rev-parse", "HEAD"]);
        git(repo, &["branch", "other", accepted.as_str()]);
        assert!(git_dir(repo).join("rebase-merge").exists());
        let first_atom = git(repo, &["rev-list", "--max-parents=0", "HEAD"]);
        assert_eq!(
            git(repo, &["ls-tree", "--name-only", first_atom.as_str()]),
            "first.txt"
        );

        let finished = Command::new(git_factor_bin())
            .current_dir(repo)
            .args(["--finish"])
            .output()
            .or_abort();

        assert_eq!(finished.status.code(), Some(EXIT_OK));
        assert_eq!(
            finished.stdout,
            b"{\"operation\":\"finish\",\"result\":\"complete\",\"split_count\":2}\n",
        );
        assert!(
            String::from_utf8_lossy(&finished.stderr).contains("Successfully rebased and updated")
        );
        assert_eq!(git(repo, &["rev-parse", "refs/heads/other"]), accepted);
        assert_eq!(git(repo, &["rev-parse", "HEAD^{tree}"]), original_tree);
        assert_eq!(git(repo, &["write-tree"]), original_tree);
        assert_eq!(git(repo, &["symbolic-ref", "HEAD"]), original_branch);
        assert_eq!(git(repo, &["status", "--porcelain"]), "");
        assert_eq!(git(repo, &["rev-list", "--count", "HEAD"]), "2");
        assert_eq!(git(repo, &["log", "-1", "--format=%s"]), "Add root changes");
        assert!(!git_dir(repo).join("factor").exists());
        assert!(!git_dir(repo).join("rebase-merge").exists());
        assert!(!git_dir(repo).join("rebase-apply").exists());
    }

    #[test]
    fn finish_preserves_fixup_descendant_during_empty_root_cleanup() {
        let dir = init_repo();
        let repo = dir.path();
        write_file(repo, "first.txt", "first\n");
        write_file(repo, "second.txt", "second\n");
        git(repo, &["add", "first.txt", "second.txt"]);
        git(repo, &["commit", "--message", "Add root changes"]);
        let selected = git(repo, &["rev-parse", "HEAD"]);
        commit_file(repo, "later.txt", "later\n", "fixup! Add first change");
        let original_tree = git(repo, &["rev-parse", "HEAD^{tree}"]);
        let original_branch = git(repo, &["symbolic-ref", "HEAD"]);
        git(repo, &["config", "rebase.autoSquash", "true"]);
        let started = Command::new(git_factor_bin())
            .current_dir(repo)
            .args(["--exec", "true", selected.as_str()])
            .output()
            .or_abort();
        assert_eq!(started.status.code(), Some(EXIT_OK));
        git(repo, &["add", "first.txt"]);
        let submitted = Command::new(git_factor_bin())
            .current_dir(repo)
            .args(["--continue", "--message", "Add first change"])
            .output()
            .or_abort();
        assert_eq!(submitted.status.code(), Some(EXIT_OK));
        let first_atom = git(repo, &["rev-list", "--max-parents=0", "HEAD"]);
        assert_eq!(
            git(repo, &["ls-tree", "--name-only", first_atom.as_str()]),
            "first.txt"
        );

        let finished = Command::new(git_factor_bin())
            .current_dir(repo)
            .args(["--finish"])
            .output()
            .or_abort();

        assert_eq!(finished.status.code(), Some(EXIT_OK));
        assert_eq!(
            finished.stdout,
            b"{\"operation\":\"finish\",\"result\":\"complete\",\"split_count\":2}\n"
        );
        assert!(
            String::from_utf8_lossy(&finished.stderr).contains("Successfully rebased and updated")
        );
        assert_eq!(git(repo, &["rev-parse", "HEAD^{tree}"]), original_tree);
        assert_eq!(git(repo, &["write-tree"]), original_tree);
        assert_eq!(git(repo, &["symbolic-ref", "HEAD"]), original_branch);
        assert_eq!(git(repo, &["status", "--porcelain"]), "");
        assert_eq!(fs::read(repo.join("first.txt")).or_abort(), b"first\n");
        assert_eq!(fs::read(repo.join("second.txt")).or_abort(), b"second\n");
        assert_eq!(fs::read(repo.join("later.txt")).or_abort(), b"later\n");
        assert!(!git_dir(repo).join("factor").exists());
        assert!(!git_dir(repo).join("rebase-merge").exists());
        assert!(!git_dir(repo).join("rebase-apply").exists());
        assert_eq!(
            git(repo, &["log", "--reverse", "--format=%s"]),
            "Add first change\nAdd root changes\nfixup! Add first change"
        );
        assert_eq!(git(repo, &["rev-list", "--count", "HEAD"]), "3");
    }

    #[test]
    fn terminal_continue_emits_only_json_with_native_gate_output_on_stderr() {
        let directory = init_repo();
        let repo = directory.path();
        commit_file(repo, "file.txt", "base\n", "Base");
        commit_file(repo, "file.txt", "base\nselected\n", "Selected source");
        let original_tree = git(repo, &["rev-parse", "HEAD^{tree}"]);
        git(repo, &["branch", "unrelated", "HEAD~1"]);
        let unrelated = git(repo, &["rev-parse", "refs/heads/unrelated"]);
        let gate_directory = TempDir::new().or_abort();
        let gate_counter = gate_directory.path().join("gate-count");
        let command = format!(
            "printf 'completion gate output\\n'; printf 'pass\\n' >> {}",
            shell_quote(gate_counter.to_str().or_abort())
        );
        let command_file = gate_directory.path().join("command");
        fs::write(&command_file, command.as_bytes()).or_abort();
        let command_hash = git(repo, &["hash-object", command_file.to_str().or_abort()]);
        let started = Command::new(git_factor_bin())
            .current_dir(repo)
            .env_remove("CLAUDECODE")
            .args(["--exec", command.as_str(), "HEAD"])
            .output()
            .or_abort();
        assert!(
            started.status.success(),
            "{}",
            String::from_utf8_lossy(&started.stderr)
        );
        assert!(String::from_utf8_lossy(&started.stderr).contains("completion gate output\n"));
        let selection: serde_json::Value = serde_json::from_slice(&started.stdout).or_abort();
        assert_eq!(
            selection.get("operation"),
            Some(&serde_json::Value::String("start".to_owned()))
        );
        assert_eq!(fs::read(&gate_counter).or_abort(), b"pass\n");
        git(repo, &["add", "file.txt"]);

        run_git_factor(
            repo,
            &["--continue", "--message", "Complete selected source"],
            GitFactorExpectation::default()
                .stdout("{\"operation\":\"continue\",\"result\":\"complete\",\"split_count\":1}\n")
                .stderr_suffix(format!(
                    "Successfully rebased and updated {}.\n",
                    read_journal(repo)
                        .get("branch")
                        .and_then(serde_json::Value::as_str)
                        .or_abort()
                ))
                .factor_state_exists(false)
                .rebase_merge_exists(false)
                .rebase_apply_exists(false)
                .git_status_porcelain(""),
        );

        assert_eq!(git(repo, &["rev-parse", "HEAD^{tree}"]), original_tree);
        assert_eq!(git(repo, &["write-tree"]), original_tree);
        assert_eq!(
            fs::read(repo.join("file.txt")).or_abort(),
            b"base\nselected\n"
        );
        assert_eq!(git(repo, &["rev-parse", "refs/heads/unrelated"]), unrelated);
        assert_eq!(
            git(repo, &["show", "--format=%B", "--no-patch", "HEAD"]),
            format!(
                "Complete selected source\n\nGate-exec-{command_hash}:\n {command_hash}\n {original_tree}"
            )
        );
        assert_eq!(fs::read(&gate_counter).or_abort(), b"pass\n");
    }

    #[test]
    #[expect(
        clippy::literal_string_with_formatting_args,
        reason = "HEAD^{tree} is native Git revision syntax, not a Rust formatting placeholder"
    )]
    fn json_abort_retains_existing_reset_and_cleanup_contract() {
        let directory = init_repo();
        let repo = directory.path();
        commit_file(repo, "base", "base\n", "Base");
        commit_file(repo, "atom", "atom\n", "Selected source");
        let head = git(repo, &["rev-parse", "HEAD"]);
        let tree = git(repo, &["rev-parse", "HEAD^{tree}"]);
        let refs = git(repo, &["show-ref"]);
        run_git_factor(
            repo,
            &["--exec", "true", "HEAD"],
            GitFactorExpectation::default(),
        );
        write_file(repo, "base", "attempt bytes\n");
        git(repo, &["add", "base"]);
        write_file(repo, "scratch", "attempt scratch\n");
        let selecting_head = git(repo, &["rev-parse", "HEAD"]);
        let selecting_refs = git(repo, &["show-ref"]);
        let index = fs::read(repo.join(".git/index")).or_abort();
        let journal = fs::read(checkpoint_journal(repo)).or_abort();
        run_git_factor(
            repo,
            &["--abort"],
            GitFactorExpectation::default()
                .code(EXIT_SOFTWARE)
                .stderr("git command failed: staged paths must belong to the remaining selected change\n")
                .factor_state_exists(true),
        );
        assert_eq!(git(repo, &["rev-parse", "HEAD"]), selecting_head);
        assert_eq!(git(repo, &["show-ref"]), selecting_refs);
        assert_eq!(fs::read(repo.join(".git/index")).or_abort(), index);
        assert_eq!(fs::read(checkpoint_journal(repo)).or_abort(), journal);
        assert_eq!(fs::read(repo.join("base")).or_abort(), b"attempt bytes\n");
        assert_eq!(
            fs::read(repo.join("scratch")).or_abort(),
            b"attempt scratch\n"
        );
        // Restore only fixture-owned foreign staging, then request the real abort.
        git(
            repo,
            &["restore", "--source=HEAD", "--staged", "--worktree", "base"],
        );
        run_git_factor(
            repo,
            &["--abort"],
            GitFactorExpectation::default()
                .stdout(
                    "{\"operation\":\"abort\",\"rebase\":{\"in_progress\":false},\"actions\":{}}\n",
                )
                .factor_state_exists(false)
                .rebase_merge_exists(false)
                .rebase_apply_exists(false)
                .git_status_porcelain("?? scratch")
                .path_exists("scratch", true),
        );
        assert_eq!(git(repo, &["rev-parse", "HEAD"]), head);
        assert_eq!(git(repo, &["rev-parse", "HEAD^{tree}"]), tree);
        assert_eq!(git(repo, &["write-tree"]), tree);
        assert_original_refs_and_true_proof(repo, &refs, &tree);
        assert_eq!(fs::read(repo.join("base")).or_abort(), b"base\n");
        assert_eq!(fs::read(repo.join("atom")).or_abort(), b"atom\n");
        assert_eq!(
            fs::read(repo.join("scratch")).or_abort(),
            b"attempt scratch\n"
        );
    }

    #[test]
    fn json_status_preserves_native_tip_session() {
        verify_json_status_preservation(false, false);
    }

    #[test]
    fn json_status_preserves_native_root_session() {
        verify_json_status_preservation(true, false);
    }

    #[test]
    fn json_status_preserves_native_pending_start_session() {
        verify_json_status_preservation(false, true);
    }

    fn verify_json_status_preservation(root: bool, pending: bool) {
        let directory = init_repo();
        let repo = directory.path();
        if !root {
            commit_file(repo, "base", "base\n", "Base");
        }
        commit_file(repo, "atom", "atom\n", "Selected source");
        let selected = git(repo, &["rev-parse", "HEAD"]);
        let selected_tree = git(repo, &["rev-parse", "HEAD^{tree}"]);
        let parent = (!root).then(|| git(repo, &["rev-parse", "HEAD^"]));
        if pending {
            commit_file(repo, "descendant", "descendant\n", "Later history");
        }
        let checkpoint = git(repo, &["rev-parse", "HEAD"]);
        if pending {
            let reset_parent = parent.as_ref().or_abort();
            let (wrapper, bin) = make_git_wrapper_named(
                "git",
                &format!(
                    "if [ \"$#\" -eq 4 ] && [ \"$1\" = reset ] && [ \"$2\" = --mixed ] && [ \"$3\" = --quiet ] && [ \"$4\" = '{reset_parent}' ]; then\n  exit 1\nfi\n"
                ),
            );
            let mut path = OsString::from(bin.as_os_str());
            path.push(OsStr::new(":"));
            path.push(env::var_os("PATH").or_abort());
            run_git_factor_with_prefixed_path(
                repo,
                &["--exec", "true", "HEAD~1"],
                GitFactorExpectation::default()
                    .code(EXIT_SOFTWARE)
                    .stderr_suffix("git command failed: git reset failed (exit 1)\n")
                    .factor_state_exists(true),
                path,
            );
            assert!(
                wrapper.path().exists(),
                "native reset wrapper retained during start"
            );
            assert_eq!(git(repo, &["rev-parse", "HEAD^{tree}"]), selected_tree);
            assert_eq!(
                git(repo, &["show", "-s", "--format=%P", "HEAD"]),
                *reset_parent
            );
            assert_eq!(
                read_journal(repo)
                    .pointer("/state/phase")
                    .and_then(serde_json::Value::as_str),
                Some("opening")
            );
        } else {
            run_git_factor(
                repo,
                &["--exec", "true", "HEAD"],
                GitFactorExpectation::default(),
            );
        }
        let head = git(repo, &["rev-parse", "HEAD"]);
        let refs = git(repo, &["show-ref"]);
        let index = fs::read(repo.join(".git/index")).or_abort();
        let journal = fs::read(checkpoint_journal(repo)).or_abort();
        let native = git_dir(repo).join("rebase-merge");
        let done = fs::read(native.join("done")).or_abort();
        let todo = fs::read(native.join("git-rebase-todo")).or_abort();
        write_file(repo, "unrelated", "user bytes\n");
        let phase = if pending { "opening" } else { "selecting" };
        let expected = format!(
            "{{\"operation\":\"status\",\"session\":{{\"checkpoint\":\"{checkpoint}\",\"phase\":\"{phase}\",\"rebase\":{{\"in_progress\":true,\"required\":true}},\"split_count\":0,\"target\":{{\"commit\":\"{selected}\",\"commit_count\":1,\"span_starts_at_root\":{root}}}}}}}\n"
        );
        run_git_factor(
            repo,
            &["--status"],
            GitFactorExpectation::default().stdout(expected),
        );
        assert_eq!(git(repo, &["rev-parse", "HEAD"]), head);
        assert_eq!(git(repo, &["show-ref"]), refs);
        assert_eq!(fs::read(repo.join(".git/index")).or_abort(), index);
        assert_eq!(fs::read(checkpoint_journal(repo)).or_abort(), journal);
        assert_eq!(fs::read(native.join("done")).or_abort(), done);
        assert_eq!(fs::read(native.join("git-rebase-todo")).or_abort(), todo);
        assert_eq!(fs::read(repo.join("atom")).or_abort(), b"atom\n");
        assert_eq!(fs::read(repo.join("unrelated")).or_abort(), b"user bytes\n");
        if !root {
            assert_eq!(fs::read(repo.join("base")).or_abort(), b"base\n");
        }
    }

    #[test]
    fn message_only_submission_and_explicit_continue_preserve_paragraphs_and_final_tree() {
        for explicit in [false, true] {
            let directory = init_repo();
            let repo = directory.path();
            commit_file(repo, "base", "base\n", "Base");
            write_file(repo, "left", "left\n");
            write_file(repo, "right", "right\n");
            git(repo, &["add", "left", "right"]);
            git(repo, &["commit", "-m", "Selected source"]);
            let original_tree = git(repo, &["rev-parse", "HEAD^{tree}"]);
            run_git_factor(
                repo,
                &["--exec", "true", "HEAD"],
                GitFactorExpectation::default(),
            );
            git(repo, &["add", "left"]);
            let first_tree = git(repo, &["write-tree"]);
            run_git_factor(
                repo,
                &["--message", "First atom", "--message", "First rationale"],
                GitFactorExpectation::default().factor_state_exists(true),
            );
            assert_true_gate_message(repo, "HEAD", "First atom\n\nFirst rationale", &first_tree);
            assert_eq!(git(repo, &["show", "HEAD:left"]), "left");
            assert_eq!(fs::read(repo.join("right")).or_abort(), b"right\n");
            git(repo, &["add", "right"]);
            let mut final_arguments =
                vec!["--message", "Second atom", "--message", "Second rationale"];
            if explicit {
                final_arguments.insert(0, "--continue");
            }
            run_git_factor(
                repo,
                &final_arguments,
                GitFactorExpectation::default()
                    .stdout(
                        "{\"operation\":\"continue\",\"result\":\"complete\",\"split_count\":2}\n",
                    )
                    .factor_state_exists(false),
            );
            assert_true_gate_message(
                repo,
                "HEAD",
                "Second atom\n\nSecond rationale",
                &original_tree,
            );
            assert_true_gate_message(repo, "HEAD~1", "First atom\n\nFirst rationale", &first_tree);
            assert_eq!(git(repo, &["rev-parse", "HEAD^{tree}"]), original_tree);
            assert_eq!(git(repo, &["status", "--porcelain=v1"]), "");
        }
    }

    #[test]
    fn message_only_without_session_preserves_native_state() {
        let directory = init_repo();
        let repo = directory.path();
        commit_file(repo, "base", "base\n", "Base");
        write_file(repo, "unrelated", "user bytes\n");
        let head = git(repo, &["rev-parse", "HEAD"]);
        let index = fs::read(repo.join(".git/index")).or_abort();
        let mut expectation = GitFactorExpectation::default()
            .code(EXIT_USAGE)
            .stderr("no active factor session\n")
            .factor_state_exists(false);
        expectation.stdout = Some(StreamExpectation::new_exact(String::new()));
        run_git_factor(repo, &["--message", "Selected atom"], expectation);
        assert_eq!(git(repo, &["rev-parse", "HEAD"]), head);
        assert_eq!(fs::read(repo.join(".git/index")).or_abort(), index);
        assert_eq!(fs::read(repo.join("base")).or_abort(), b"base\n");
        assert_eq!(fs::read(repo.join("unrelated")).or_abort(), b"user bytes\n");
    }

    #[test]
    fn message_only_without_staging_preserves_native_session() {
        let directory = init_repo();
        let repo = directory.path();
        commit_file(repo, "base", "base\n", "Base");
        commit_file(repo, "atom", "atom\n", "Selected source");
        run_git_factor(
            repo,
            &["--exec", "true", "HEAD"],
            GitFactorExpectation::default(),
        );
        let head = git(repo, &["rev-parse", "HEAD"]);
        let index = fs::read(repo.join(".git/index")).or_abort();
        let journal = fs::read(checkpoint_journal(repo)).or_abort();
        let status = Command::new(git_factor_bin())
            .current_dir(repo)
            .arg("--status")
            .output()
            .or_abort();
        assert_eq!(status.status.code(), Some(EXIT_OK));
        let mut expectation = GitFactorExpectation::default()
            .code(EXIT_USAGE)
            .stderr("no staged changes to commit\nNEXT: stage exactly one atomic change, then rerun:\n  git factor --continue --message \"type: description\"\n")
            .factor_state_exists(true);
        expectation.stdout = Some(StreamExpectation::new_exact(String::new()));
        run_git_factor(repo, &["--message", "Selected atom"], expectation);
        assert_eq!(git(repo, &["rev-parse", "HEAD"]), head);
        assert_eq!(fs::read(repo.join(".git/index")).or_abort(), index);
        assert_eq!(fs::read(checkpoint_journal(repo)).or_abort(), journal);
        let observed = Command::new(git_factor_bin())
            .current_dir(repo)
            .arg("--status")
            .output()
            .or_abort();
        assert_eq!(observed.status.code(), Some(EXIT_OK));
        assert_eq!(observed.stdout, status.stdout);
        assert_eq!(fs::read(repo.join("base")).or_abort(), b"base\n");
        assert_eq!(fs::read(repo.join("atom")).or_abort(), b"atom\n");
    }
    #[test]
    fn continue_from_child_preserves_same_named_tracked_file() {
        let dir = init_repo();
        let repo = dir.path();
        fs::create_dir_all(repo.join("sub")).or_abort();
        write_file(repo, "victim", "original root bytes\n");
        write_file(repo, "sub/victim", "protected tracked child bytes\n");
        git(repo, &["add", "--all"]);
        git(repo, &["commit", "--message", "Add original files"]);
        git(repo, &["rm", "--", "victim"]);
        git(repo, &["commit", "--message", "Remove root file"]);
        start_session(repo);
        git(repo, &["add", "--", "victim"]);
        assert_eq!(
            git(
                repo,
                &[
                    "diff",
                    "--cached",
                    "--name-status",
                    "--no-relative",
                    "--no-renames"
                ]
            ),
            "D\tvictim"
        );
        assert!(!repo.join("victim").exists());
        let expected_tree = git(repo, &["write-tree"]);
        let bytes = fs::read(repo.join("sub/victim")).or_abort();

        let output = Command::new(git_factor_bin())
            .current_dir(repo.join("sub"))
            .args(["--continue", "--message", "Remove root file"])
            .output()
            .or_abort();

        assert_eq!(output.status.code(), Some(EXIT_OK));
        assert_eq!(
            output.stdout,
            b"{\"operation\":\"continue\",\"result\":\"complete\",\"split_count\":1}\n"
        );
        let short_head = git(repo, &["rev-parse", "--short", "HEAD"]);
        let executable = fs::canonicalize(git_factor_bin()).or_abort();
        let expected_stderr = format!(
            concat!(
                "HEAD is now at {short_head} Remove root file\n",
                "Rebasing (3/4)\rExecuting: '{executable}' checkpoint-gate-remainder\n",
                "Rebasing (4/4)\rExecuting: '{executable}' checkpoint-terminal\n",
                "Successfully rebased and updated refs/heads/main.\n"
            ),
            executable = executable.display(),
            short_head = short_head
        );
        assert_eq!(output.stderr, expected_stderr.as_bytes());
        assert_eq!(fs::read(repo.join("sub/victim")).or_abort(), bytes);
        assert!(!repo.join("victim").exists());
        assert_eq!(git(repo, &["rev-parse", "HEAD^{tree}"]), expected_tree);
        assert_eq!(git(repo, &["write-tree"]), expected_tree);
        assert_eq!(git_status_porcelain(repo), "");
        assert!(!git_dir(repo).join("factor").exists());
        assert!(!git_dir(repo).join("rebase-merge").exists());
        assert!(!git_dir(repo).join("rebase-apply").exists());
    }

    #[test]
    fn abort_message_preserves_active_native_session_before_mutation() {
        let directory = init_repo();
        let repo = directory.path();
        commit_file(repo, "base", "base\n", "Add base");
        commit_file(repo, "selected", "selected\n", "Add selected file");
        start_session(repo);
        write_file(repo, "selected", "staged user bytes\n");
        git(repo, &["add", "selected"]);
        write_file(repo, "unrelated", "untracked user bytes\n");
        let before = snapshot_selection(repo);

        run_git_factor(
            repo,
            &["--abort", "--message", "Preserve this active session"],
            GitFactorExpectation {
                code: EXIT_USAGE,
                stdout: Some(StreamExpectation::new_exact(String::new())),
                stderr: Some(StreamExpectation::new_exact(
                    "--abort cannot be combined with other options\n".to_owned(),
                )),
                ..GitFactorExpectation::default()
            },
        );

        let immediate_index = fs::read(repo.join(".git/index")).or_abort();
        assert_eq!(immediate_index, before.index);
        assert_eq!(snapshot_selection(repo), before);
        assert_eq!(
            fs::read(repo.join("selected")).or_abort(),
            b"staged user bytes\n"
        );
        assert_eq!(
            fs::read(repo.join("unrelated")).or_abort(),
            b"untracked user bytes\n"
        );
        assert!(repo.join(".git/rebase-merge").is_dir());
        assert!(!repo.join(".git/rebase-apply").exists());
    }

    #[test]
    fn continue_preserves_untracked_files_while_isolating_the_gate() {
        let directory = init_repo();
        let repo = directory.path();
        commit_file(repo, "file.txt", "one\n", "Add base");
        commit_file(repo, "file.txt", "one\ntwo\n", "Add second line");
        run_git_factor(
            repo,
            &["--exec", "test ! -e deleted-path.txt", "HEAD"],
            GitFactorExpectation::default(),
        );
        git(repo, &["add", "--all"]);
        write_file(repo, "deleted-path.txt", "unrelated user bytes\n");

        run_git_factor(
            repo,
            &["--continue", "--message", "Add second line"],
            GitFactorExpectation::default()
                .stdout("{\"operation\":\"continue\",\"result\":\"complete\",\"split_count\":1}\n"),
        );

        assert_eq!(
            fs::read(repo.join("deleted-path.txt")).or_abort(),
            b"unrelated user bytes\n"
        );
        assert_eq!(git_status_porcelain(repo), "?? deleted-path.txt");
        assert!(!checkpoint_journal(repo).exists());
    }

    fn arrange_native_original_pool() -> NativeOriginalPool {
        let directory = init_repo();
        let repo = directory.path();
        write_file(repo, "keep", "unchanged anchor\n");
        git(repo, &["add", "keep"]);
        commit_file(repo, "victim", "original file\n", "Add original file");
        let base = git(repo, &["rev-parse", "HEAD"]);
        git(repo, &["branch", "foreign", &base]);
        git(repo, &["tag", "foreign", &base]);
        let foreign_refs = git(
            repo,
            &[
                "show-ref",
                "--verify",
                "refs/heads/foreign",
                "refs/tags/foreign",
            ],
        );
        fs::remove_file(repo.join("victim")).or_abort();
        fs::create_dir_all(repo.join("victim")).or_abort();
        write_file(repo, "victim/x", "saved remainder\n");
        git(repo, &["add", "--all"]);
        git(
            repo,
            &[
                "commit",
                "--quiet",
                "--message",
                "Replace file with directory",
            ],
        );
        let final_tree = git(repo, &["rev-parse", "HEAD^{tree}"]);
        start_session(repo);
        git(repo, &["add", "--update", "--", "victim"]);
        let deletion_tree = git(repo, &["write-tree"]);
        assert_eq!(
            git(repo, &["ls-tree", "--name-only", &deletion_tree]),
            "keep"
        );
        NativeOriginalPool {
            base,
            deletion_tree,
            directory,
            final_tree,
            foreign_refs,
        }
    }

    #[test]
    fn original_pool_directory_is_captured_and_reopened() {
        let fixture = arrange_native_original_pool();
        let repo = fixture.directory.path();

        run_git_factor(
            repo,
            &["--message", "Remove original file"],
            GitFactorExpectation::default()
                .stdout(expected_remaining_stdout("[]", "[\"victim/x\"]")),
        );

        let journal = read_journal(repo);
        assert_eq!(
            journal
                .pointer("/state/phase")
                .and_then(serde_json::Value::as_str),
            Some("selecting")
        );
        let head = git(repo, &["rev-parse", "HEAD"]);
        assert_eq!(
            journal
                .pointer("/state/head")
                .and_then(serde_json::Value::as_str),
            Some(head.as_str())
        );
        assert_eq!(
            journal
                .pointer("/state/base")
                .and_then(serde_json::Value::as_str),
            Some(head.as_str())
        );
        assert_eq!(git(repo, &["rev-parse", "HEAD^"]), fixture.base);
        assert_eq!(
            git(repo, &["rev-parse", "HEAD^{tree}"]),
            fixture.deletion_tree
        );
        assert_eq!(git(repo, &["write-tree"]), fixture.deletion_tree);
        assert_eq!(
            git(repo, &["rev-parse", "refs/heads/main^{tree}"]),
            fixture.final_tree
        );
        assert_eq!(
            journal
                .get("checkpoint")
                .and_then(serde_json::Value::as_str),
            Some(git(repo, &["rev-parse", "refs/heads/main"]).as_str())
        );
        assert_eq!(
            git(
                repo,
                &[
                    "show-ref",
                    "--verify",
                    "refs/heads/foreign",
                    "refs/tags/foreign"
                ]
            ),
            fixture.foreign_refs
        );
        assert_eq!(git_status_porcelain(repo), "?? victim/");
        assert_eq!(
            fs::read(repo.join("keep")).or_abort(),
            b"unchanged anchor\n"
        );
        assert_eq!(
            fs::read(repo.join("victim/x")).or_abort(),
            b"saved remainder\n"
        );
    }

    #[test]
    fn changed_pool_directory_is_refused_without_mutating_selection() {
        let fixture = arrange_native_original_pool();
        let repo = fixture.directory.path();
        write_file(repo, "victim/x", "saved remainder\nchanged user bytes\n");
        let expected_index_directory = TempDir::new().or_abort();
        let expected_index = expected_index_directory.path().join("index");
        for args in [vec!["read-tree", "refs/heads/main"], vec!["add", "--all"]] {
            let result = Command::new("git")
                .args(args)
                .env("GIT_INDEX_FILE", &expected_index)
                .current_dir(repo)
                .output()
                .or_abort();
            assert!(result.status.success());
        }
        let result = Command::new("git")
            .args(["write-tree"])
            .env("GIT_INDEX_FILE", &expected_index)
            .current_dir(repo)
            .output()
            .or_abort();
        assert!(result.status.success());
        let actual_tree = String::from_utf8(result.stdout).or_abort();
        let diagnostic = format!(
            "tree hash mismatch: expected {}, got {}\n",
            fixture.final_tree,
            actual_tree.trim_end()
        );
        let before = snapshot_selection(repo);

        run_git_factor(
            repo,
            &["--message", "Remove original file"],
            GitFactorExpectation {
                code: EXIT_TEMPFAIL,
                stdout: Some(StreamExpectation::new_exact(String::new())),
                stderr: Some(StreamExpectation::new_exact(diagnostic)),
                ..GitFactorExpectation::default()
            },
        );

        let immediate_index = fs::read(repo.join(".git/index")).or_abort();
        assert_eq!(immediate_index, before.index);
        assert_eq!(snapshot_selection(repo), before);
        assert_eq!(git(repo, &["write-tree"]), fixture.deletion_tree);
        assert_eq!(
            git(
                repo,
                &[
                    "show-ref",
                    "--verify",
                    "refs/heads/foreign",
                    "refs/tags/foreign"
                ]
            ),
            fixture.foreign_refs
        );
        assert_eq!(
            fs::read(repo.join("keep")).or_abort(),
            b"unchanged anchor\n"
        );
        assert_eq!(
            fs::read(repo.join("victim/x")).or_abort(),
            b"saved remainder\nchanged user bytes\n"
        );
    }

    #[test]
    fn actor_routing_to_main_refuses_before_materializing_partial_candidate() {
        let directory = init_repo();
        let repo = directory.path();
        commit_file(repo, "keep", "unchanged anchor\n", "Add base");
        commit_file(
            repo,
            "atom",
            "selected atom\nsaved same-file remainder\n",
            "Add two lines",
        );
        start_session(repo);
        write_file(repo, "atom", "selected atom\n");
        git(repo, &["add", "atom"]);
        write_file(repo, "atom", "selected atom\nsaved same-file remainder\n");
        write_file(repo, "user", "unrelated user bytes\0\n");
        git(repo, &["config", "extensions.worktreeConfig", "true"]);
        let root = fs::canonicalize(repo).or_abort();
        git(repo, &["config", "core.worktree", root.to_str().or_abort()]);
        let before = snapshot_selection(repo);
        let config = fs::read(repo.join(".git/config")).or_abort();
        let anchor = fs::read(repo.join("keep")).or_abort();
        let contents = fs::read(repo.join("atom")).or_abort();
        let user = fs::read(repo.join("user")).or_abort();

        let output = Command::new(git_factor_bin())
            .current_dir(repo)
            .args(["--message", "Add selected atom"])
            .output()
            .or_abort();

        let immediate_index = fs::read(repo.join(".git/index")).or_abort();
        assert_eq!(fs::read(repo.join("atom")).or_abort(), contents);
        assert_eq!(immediate_index, before.index);
        assert_eq!(snapshot_selection(repo), before);
        assert_eq!(fs::read(repo.join(".git/config")).or_abort(), config);
        assert_eq!(fs::read(repo.join("keep")).or_abort(), anchor);
        assert_eq!(fs::read(repo.join("user")).or_abort(), user);
        assert_eq!(output.status.code(), Some(EXIT_SOFTWARE));
        assert_eq!(output.stdout, b"");
        assert_eq!(
            output.stderr,
            b"git command failed: candidate Git working directory belongs to another worktree\n"
        );
    }
}
