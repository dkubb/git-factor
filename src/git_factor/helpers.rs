#[cfg_attr(
    not(test),
    expect(
        clippy::wildcard_imports,
        reason = "module intentionally shares parent items via a grouped import"
    )
)]
use super::*;
use std::process::ExitStatus;

#[cfg(test)]
use crate::test_support::OrAbort as _;

/// Maps a `FactorError` to an `(exit_code, message)` tuple.
pub(in crate::git_factor) fn error_to_exit(error: &FactorError) -> (i32, String) {
    let code = match error {
        &FactorError::ActiveRebase
        | &FactorError::ActiveSession
        | &FactorError::NoActiveSession
        | &FactorError::NoStagedChanges
        | &FactorError::Usage(_) => EXIT_USAGE,

        &FactorError::GitDir(_)
        | &FactorError::InvalidCommit(_)
        | &FactorError::InvalidExecSyntax(_)
        | &FactorError::MergeCommit(_)
        | &FactorError::NotAncestor(_)
        | &FactorError::NotGitRepo => EXIT_DATAERR,

        &FactorError::ExecFailed { .. } | &FactorError::TreeHashMismatch { .. } => EXIT_TEMPFAIL,

        &FactorError::GitCommand(_)
        | &FactorError::StateRead(_)
        | &FactorError::StateWrite(_)
        | &FactorError::Io(_) => EXIT_SOFTWARE,
    };
    (code, error.to_string())
}

/// Shell-quotes a single argument using single-quote wrapping.
///
/// This is used when constructing `GIT_SEQUENCE_EDITOR`, which git interprets
/// as a shell command line.
pub(in crate::git_factor) fn shell_quote(arg: &str) -> String {
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

/// Returns the absolute path to `git-sequence-editor`, resolved as a sibling
/// of the current executable.
pub(in crate::git_factor) fn editor_path(ctx: &Ctx<'_>) -> Result<PathBuf, FactorError> {
    let exe = match ctx.env.current_exe() {
        Ok(exe) => exe,
        Err(err) => {
            return Err(FactorError::GitCommand(non_empty_msg(format!(
                "cannot resolve current exe: {err}"
            ))));
        }
    };
    let script_path = match ctx.fs.canonicalize(&exe) {
        Ok(script_path) => script_path,
        Err(err) => {
            return Err(FactorError::GitCommand(non_empty_msg(format!(
                "cannot canonicalize exe: {err}"
            ))));
        }
    };
    let Some(dir) = script_path.parent() else {
        return Err(FactorError::GitCommand(non_empty_msg(
            "executable has no parent directory".to_owned(),
        )));
    };

    Ok(dir.join("git-sequence-editor"))
}

/// Returns the path to the factor state directory.
pub(in crate::git_factor) fn factor_dir_in(ctx: &Ctx<'_>) -> Result<StateDir, FactorError> {
    Ok(StateDir::new(git_dir_in(ctx)?.join("factor")))
}

/// Extracts exit code from process status.
pub(in crate::git_factor) fn status_code(status: ExitStatus) -> i32 {
    status.code().unwrap_or(EXIT_SOFTWARE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_quote_wraps_and_escapes_single_quotes() {
        assert_eq!(shell_quote("abc"), "'abc'");
        assert_eq!(shell_quote("a b"), "'a b'");
        assert_eq!(shell_quote("a'b"), "'a'\\''b'");
    }

    #[test]
    fn error_to_exit_maps_variants_to_expected_exit_codes() {
        let sha = CommitSha::new("a".repeat(COMMIT_SHA_HEX_LEN)).or_abort("valid sha");
        let exec = NonEmptyString::try_from("true".to_owned()).or_abort("non-empty");
        let one: i32 = 1;

        let (code_active_session, _msg_active_session) = error_to_exit(&FactorError::ActiveSession);
        assert_eq!(code_active_session, EXIT_USAGE);

        let (code_no_staged, _msg_no_staged) = error_to_exit(&FactorError::NoStagedChanges);
        assert_eq!(code_no_staged, EXIT_USAGE);

        let (code_invalid_commit, _msg_invalid_commit) =
            error_to_exit(&FactorError::InvalidCommit("bad".to_owned()));
        assert_eq!(code_invalid_commit, EXIT_DATAERR);

        let sha_merge = CommitSha::new("a".repeat(COMMIT_SHA_HEX_LEN)).or_abort("valid sha");
        let (code_merge, _msg_merge) = error_to_exit(&FactorError::MergeCommit(sha_merge));
        assert_eq!(code_merge, EXIT_DATAERR);

        let (code_not_ancestor, _msg_not_ancestor) = error_to_exit(&FactorError::NotAncestor(sha));
        assert_eq!(code_not_ancestor, EXIT_DATAERR);

        let (code_exec_failed, _msg_exec_failed) = error_to_exit(&FactorError::ExecFailed {
            code: one,
            command: exec,
        });
        assert_eq!(code_exec_failed, EXIT_TEMPFAIL);

        let (code_git_command, _msg_git_command) =
            error_to_exit(&FactorError::GitCommand(non_empty_msg("nope".to_owned())));
        assert_eq!(code_git_command, EXIT_SOFTWARE);
    }

    #[test]
    fn proptest_run_non_panicking_unit_suite() {
        error_to_exit_maps_variants_to_expected_exit_codes();
        shell_quote_wraps_and_escapes_single_quotes();
    }
}

#[cfg(test)]
mod proptests {
    use proptest::prelude::*;

    use super::*;

    proptest! {
        #[test]
        fn proptest_shell_quote_round_trips_single_quote_escaping(input in any::<String>()) {
            let quoted = shell_quote(input.as_str());

            prop_assert!(quoted.starts_with('\''));
            prop_assert!(quoted.ends_with('\''));
            let inner = quoted
                .strip_prefix('\'')
                .and_then(|value| value.strip_suffix('\''));
            prop_assert!(
                inner.is_some(),
                "quoted output should be wrapped in single quotes"
            );
            let quoted_inner = inner.unwrap_or_default();
            let restored = quoted_inner.replace("'\\''", "'");
            prop_assert_eq!(restored, input);
        }
    }
}
