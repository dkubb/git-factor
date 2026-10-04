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
        | &FactorError::PrerequisiteObservation(_)
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

/// Extracts exit code from process status.
pub(in crate::git_factor) fn status_code(status: ExitStatus) -> i32 {
    status.code().unwrap_or(EXIT_SOFTWARE)
}

#[cfg(test)]
mod tests {

    mod status_code {
        use crate::test_support::OrAbort as _;
        use std::os::unix::process::ExitStatusExt as _;
        use std::process::ExitStatus;
        #[test]
        fn maps_a_signal_to_the_operational_fallback() {
            let signal: i32 = 15;
            let actual = super::super::status_code(ExitStatus::from_raw(signal));
            assert_eq!(actual, super::super::EXIT_SOFTWARE);
        }
        #[test]
        fn preserves_a_native_nonzero_exit() {
            let code: u8 = 42;
            let raw = i32::from(code)
                .checked_shl(8)
                .or_abort("native wait-status shift count is below i32 width");
            let actual = super::super::status_code(ExitStatus::from_raw(raw));
            assert_eq!(actual, i32::from(code));
        }
    }
    mod error_to_exit {
        #[test]
        fn prerequisite_observation_retains_operational_exit_and_primary_diagnostic() {
            let error = super::super::FactorError::PrerequisiteObservation(
                super::super::non_empty_msg("owned spawn failure".to_owned()),
            );
            assert_eq!(
                super::super::error_to_exit(&error),
                (
                    super::super::EXIT_SOFTWARE,
                    "git command failed: owned spawn failure".to_owned()
                )
            );
        }
    }
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
}

#[cfg(test)]
mod proptests {

    mod status_code {
        use crate::test_support::OrAbort as _;
        use proptest::prelude::*;
        use std::os::unix::process::ExitStatusExt as _;
        use std::process::ExitStatus;
        const SIGNAL_FIRST: i32 = 1;
        const SIGNAL_LAST: i32 = 31;
        proptest! {
            #[test]
            fn preserves_generated_native_exits(code in any::<u8>()) {
                let raw = i32::from(code).checked_shl(8).or_abort("native wait-status shift count is below i32 width");
                let actual = super::super::status_code(ExitStatus::from_raw(raw));
                prop_assert_eq!(actual, i32::from(code));
            }
            #[test]
            fn maps_generated_native_signals_to_operational_fallback(signal in SIGNAL_FIRST..=SIGNAL_LAST) {
                let actual = super::super::status_code(ExitStatus::from_raw(signal));
                prop_assert_eq!(actual, super::super::EXIT_SOFTWARE);
            }
        }
    }
    mod error_to_exit {
        proptest::proptest! {
            #[test]
            fn preserves_generated_prerequisite_exit_and_diagnostic(diagnostic in ".{1,40}") {
                let error = super::super::FactorError::PrerequisiteObservation(
                    super::super::non_empty_msg(diagnostic.clone()),
                );
                proptest::prop_assert_eq!(super::super::error_to_exit(&error),
                    (super::super::EXIT_SOFTWARE, format!("git command failed: {diagnostic}")));
            }
        }
    }
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
