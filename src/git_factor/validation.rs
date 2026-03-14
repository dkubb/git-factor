#[cfg_attr(
    not(test),
    expect(
        clippy::wildcard_imports,
        reason = "module intentionally shares parent items via a grouped import"
    )
)]
use super::*;
use alloc::collections::BTreeSet;

/// Resolves a commit reference to a full SHA.
pub(in crate::git_factor) fn resolve_commit(
    ctx: &Ctx<'_>,
    commit: &str,
) -> Result<CommitSha, FactorError> {
    let sha = git_output(ctx, &["rev-parse", "--verify", commit])
        .map_err(|_git_err| FactorError::InvalidCommit(commit.to_owned()))?;
    CommitSha::new(sha)
}

/// Resolves `HEAD` to a full SHA.
#[cfg(test)]
pub(in crate::git_factor) fn resolve_head_commit(ctx: &Ctx<'_>) -> Result<CommitSha, FactorError> {
    resolve_commit(ctx, "HEAD")
}

/// Resolves commit refs and ranges into a deduplicated, chronologically
/// ordered list of full SHAs.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "ref resolution is extracted for clarity and targeted tests"
    )
)]
pub(in crate::git_factor) fn resolve_commit_refs(
    ctx: &Ctx<'_>,
    refs: &NonEmpty<NonEmptyString>,
) -> Result<Commits, FactorError> {
    let mut set = BTreeSet::new();

    for commit_ref in refs {
        let ref_str = commit_ref.as_str();
        if ref_str.contains("...") {
            return Err(FactorError::InvalidCommit(format!(
                "{ref_str} (symmetric diff '...' is not supported, use '..')"
            )));
        }
        if ref_str.contains("..") {
            let output = match git_output(ctx, &["rev-list", ref_str]) {
                Ok(output) => output,
                Err(_err) => return Err(FactorError::InvalidCommit(ref_str.to_owned())),
            };
            for line in output.lines() {
                if let Ok(sha) = CommitSha::new(line.to_owned()) {
                    set.insert(sha);
                }
            }
        } else {
            set.insert(resolve_commit(ctx, commit_ref.as_str())?);
        }
    }

    Commits::try_from(set)
}

/// Validates that an exec command has valid bash syntax.
pub(in crate::git_factor) fn validate_exec_syntax(
    ctx: &Ctx<'_>,
    command: &str,
) -> Result<(), FactorError> {
    let status = match command_status_with(
        ctx,
        "bash",
        &["--norc", "--noprofile", "-n", "-c", command],
        &[],
        true,
    ) {
        Ok(status) => status,
        Err(err) => {
            return Err(FactorError::GitCommand(non_empty_msg(format!(
                "bash syntax check: {err}"
            ))));
        }
    };

    if status.success() {
        Ok(())
    } else {
        Err(FactorError::InvalidExecSyntax(command.to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;
    use std::os::unix::process::ExitStatusExt as _;
    use std::path::Path;
    use std::process::Command;
    use std::process::ExitStatus;
    use std::process::Output;
    use tempfile::TempDir;

    fn ctx_for(path: &Path) -> Ctx<'static> {
        Ctx {
            runner: &REAL_RUNNER,
            cwd: path.to_path_buf(),
            io: &REAL_IO,
            env: &REAL_ENV,
            fs: &REAL_FS,
        }
    }

    fn invalid_commit_message(error: &FactorError) -> Option<String> {
        if let &FactorError::InvalidCommit(_) = error {
            return error
                .to_string()
                .strip_prefix("invalid commit: ")
                .map(str::to_owned);
        }
        None
    }

    fn init_git_repo(path: &Path) {
        let init = Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(path)
            .status()
            .or_abort("run git init");
        assert!(init.success(), "git init failed: {init:?}");

        let config_name = Command::new("git")
            .args(["config", "user.name", "Test User"])
            .current_dir(path)
            .status()
            .or_abort("run git config user.name");
        assert!(config_name.success(), "git config user.name failed");

        let config_email = Command::new("git")
            .args(["config", "user.email", "test@example.com"])
            .current_dir(path)
            .status()
            .or_abort("run git config user.email");
        assert!(config_email.success(), "git config user.email failed");

        std::fs::write(path.join("file.txt"), "base\n").or_abort("write file");
        let add = Command::new("git")
            .args(["add", "file.txt"])
            .current_dir(path)
            .status()
            .or_abort("run git add");
        assert!(add.success(), "git add failed");

        let commit = Command::new("git")
            .args([
                "-c",
                "commit.template=",
                "-c",
                "core.hooksPath=.git/hooks",
                "commit",
                "--no-verify",
                "--quiet",
                "-m",
                "base",
            ])
            .current_dir(path)
            .status()
            .or_abort("run git commit");
        assert!(commit.success(), "git commit failed");
    }

    #[test]
    fn resolve_commit_refs_rejects_symmetric_diff_ranges() {
        let dir = TempDir::new().or_abort("tempdir");
        let ctx = ctx_for(dir.path());
        let commit_ref = NonEmptyString::try_from("HEAD...HEAD".to_owned()).or_abort("non-empty");
        let refs = NonEmpty::singleton(commit_ref);
        let err = resolve_commit_refs(&ctx, &refs).err_or_abort("symmetric diff must be rejected");

        let message = invalid_commit_message(&err).or_abort("expected InvalidCommit");
        assert!(
            message.contains("symmetric diff"),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    fn resolve_commit_refs_rejects_unknown_single_ref() {
        let dir = TempDir::new().or_abort("tempdir");
        init_git_repo(dir.path());
        let ctx = ctx_for(dir.path());
        let commit_ref =
            NonEmptyString::try_from("definitely-not-a-ref".to_owned()).or_abort("non-empty");
        let refs = NonEmpty::singleton(commit_ref);

        let err = resolve_commit_refs(&ctx, &refs).err_or_abort("unknown ref must be rejected");
        let message = invalid_commit_message(&err).or_abort("expected InvalidCommit");
        assert_eq!(message, "definitely-not-a-ref");
    }

    #[test]
    fn resolve_head_commit_maps_git_errors_to_invalid_head() {
        let dir = TempDir::new().or_abort("tempdir");
        let ctx = ctx_for(dir.path());

        let err = resolve_head_commit(&ctx).err_or_abort("missing HEAD should fail");
        let message = invalid_commit_message(&err).or_abort("expected InvalidCommit");
        assert_eq!(message, "HEAD");
    }

    #[test]
    fn resolve_commit_refs_accepts_known_single_ref() {
        let dir = TempDir::new().or_abort("tempdir");
        init_git_repo(dir.path());
        let ctx = ctx_for(dir.path());
        let commit_ref = NonEmptyString::try_from("HEAD".to_owned()).or_abort("non-empty");
        let refs = NonEmpty::singleton(commit_ref);
        let commits = resolve_commit_refs(&ctx, &refs).or_abort("HEAD should resolve");
        let head = resolve_head_commit(&ctx).or_abort("resolve");
        let mut iter = commits.iter();
        assert_eq!(iter.next(), Some(&head));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn resolve_commit_refs_rejects_unknown_range_ref() {
        let dir = TempDir::new().or_abort("tempdir");
        init_git_repo(dir.path());
        let ctx = ctx_for(dir.path());
        let commit_ref =
            NonEmptyString::try_from("deadbeef..HEAD".to_owned()).or_abort("non-empty");
        let refs = NonEmpty::singleton(commit_ref);

        let err =
            resolve_commit_refs(&ctx, &refs).err_or_abort("unknown range ref must be rejected");
        let message = invalid_commit_message(&err).or_abort("expected InvalidCommit");
        assert_eq!(message, "deadbeef..HEAD");
    }

    #[test]
    fn validate_exec_syntax_wraps_command_status_io_error() {
        struct ExecSyntaxIoErrorRunner;

        impl Runner for ExecSyntaxIoErrorRunner {
            fn output(&self, _bin: &str, _args: &[&str], _cwd: &Path) -> io::Result<Output> {
                Err(io::Error::other("output should not be called"))
            }

            fn status(
                &self,
                _bin: &str,
                _args: &[&str],
                _envs: &[(&str, &str)],
                _quiet: bool,
                _cwd: &Path,
            ) -> io::Result<ExitStatus> {
                Err(io::Error::other("forced bash syntax status io failure"))
            }
        }

        let dir = TempDir::new().or_abort("tempdir");
        let runner = ExecSyntaxIoErrorRunner;
        let ctx = Ctx {
            runner: &runner,
            cwd: dir.path().to_path_buf(),
            io: &REAL_IO,
            env: &REAL_ENV,
            fs: &REAL_FS,
        };
        let output_err = runner
            .output("git", &["status"], dir.path())
            .err_or_abort("output method should fail");
        assert!(
            output_err
                .to_string()
                .contains("output should not be called"),
            "unexpected output error: {output_err}"
        );

        let err = validate_exec_syntax(&ctx, "echo ok").err_or_abort("status io error should fail");
        assert!(
            err.to_string()
                .contains("forced bash syntax status io failure"),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    fn validate_exec_syntax_returns_ok_for_valid_shell_command() {
        let dir = TempDir::new().or_abort("tempdir");
        let ctx = Ctx {
            runner: &REAL_RUNNER,
            cwd: dir.path().to_path_buf(),
            io: &REAL_IO,
            env: &REAL_ENV,
            fs: &REAL_FS,
        };
        validate_exec_syntax(&ctx, "echo ok")
            .or_abort("valid shell command should pass syntax check");
    }
}
