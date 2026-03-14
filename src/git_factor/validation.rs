#[cfg_attr(
    not(test),
    expect(
        clippy::wildcard_imports,
        reason = "module intentionally shares parent items via a grouped import"
    )
)]
use super::*;

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
    use std::process::ExitStatus;
    use std::process::Output;
    use tempfile::TempDir;

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
