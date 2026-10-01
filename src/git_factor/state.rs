use super::*;
use core::str::FromStr;

/// Reads a state file from the factor state directory.
pub(in crate::git_factor) fn read_state(
    ctx: &Ctx<'_>,
    state_dir: &Path,
    name: &str,
) -> Result<NonEmptyString, FactorError> {
    let path = state_dir.join(name);
    let content = ctx
        .fs
        .read_to_string(&path)
        .map_err(FactorError::StateRead)?;
    match NonEmptyString::try_from(content.trim().to_owned()) {
        Ok(non_empty_content) => Ok(non_empty_content),
        Err(_err) => Err(FactorError::GitCommand(non_empty_msg(format!(
            "corrupted state file '{name}': file is empty"
        )))),
    }
}

/// Reads and parses a numeric state file, returning an error on corruption.
pub(in crate::git_factor) fn read_state_parsed<T: FromStr>(
    ctx: &Ctx<'_>,
    state_dir: &Path,
    name: &str,
) -> Result<T, FactorError> {
    let value = read_state(ctx, state_dir, name)?;
    match value.as_str().parse::<T>() {
        Ok(parsed) => Ok(parsed),
        Err(_err) => Err(FactorError::GitCommand(non_empty_msg(format!(
            "corrupted state file '{name}': invalid value '{value}'"
        )))),
    }
}

/// Reads a `true`/`false` state value, with a default when the file is missing.
#[expect(clippy::single_call_fn, reason = "called by typed wrapper")]
pub(in crate::git_factor) fn read_state_bool_or_default(
    ctx: &Ctx<'_>,
    state_dir: &Path,
    name: &str,
    default: bool,
) -> Result<bool, FactorError> {
    match read_state(ctx, state_dir, name) {
        Ok(value) => match value.as_str() {
            "true" => Ok(true),
            "false" => Ok(false),
            _ => Err(FactorError::GitCommand(non_empty_msg(format!(
                "corrupted state file '{name}': invalid value '{value}'"
            )))),
        },
        Err(FactorError::StateRead(err)) if err.kind() == io::ErrorKind::NotFound => Ok(default),
        Err(err) => Err(err),
    }
}

/// Writes a state file to the factor state directory.
pub(in crate::git_factor) fn write_state(
    ctx: &Ctx<'_>,
    state_dir: &Path,
    name: &str,
    content: &str,
) -> Result<(), FactorError> {
    let path = state_dir.join(name);
    ctx.fs
        .write_string(&path, &format!("{content}\n"))
        .map_err(FactorError::StateWrite)
}

#[cfg(test)]
fn numeric_state(content: &str) -> tempfile::TempDir {
    let directory = tempfile::TempDir::new().or_abort("numeric state fixture");
    fs::write(directory.path().join("count"), content).or_abort("write numeric state");
    directory
}

#[cfg(test)]
fn numeric_context(path: &Path) -> Ctx<'static> {
    Ctx {
        runner: &REAL_RUNNER,
        cwd: path.to_path_buf(),
        io: &REAL_IO,
        env: &REAL_ENV,
        fs: &REAL_FS,
    }
}

#[cfg(test)]
mod tests {
    mod read_state_parsed {
        use super::super::*;

        #[test]
        fn admits_usize_zero() {
            let directory = numeric_state("0");
            let ctx = numeric_context(directory.path());
            let value = read_state_parsed::<usize>(&ctx, directory.path(), "count")
                .or_abort("admit numeric state");
            assert_eq!(value, 0);
        }

        #[test]
        fn admits_usize_maximum() {
            let directory = numeric_state(&usize::MAX.to_string());
            let ctx = numeric_context(directory.path());
            let value = read_state_parsed::<usize>(&ctx, directory.path(), "count")
                .or_abort("admit numeric state");
            assert_eq!(value, usize::MAX);
        }

        #[test]
        fn admits_u8_zero() {
            let directory = numeric_state("0");
            let ctx = numeric_context(directory.path());
            let value = read_state_parsed::<u8>(&ctx, directory.path(), "count")
                .or_abort("admit numeric state");
            assert_eq!(value, 0);
        }

        #[test]
        fn admits_u8_maximum() {
            let directory = numeric_state("255");
            let ctx = numeric_context(directory.path());
            let value = read_state_parsed::<u8>(&ctx, directory.path(), "count")
                .or_abort("admit numeric state");
            assert_eq!(value, 255);
        }

        #[test]
        fn admits_surrounding_whitespace() {
            let directory = numeric_state(" \n\t\u{2003}5\u{2003}\t\n ");
            let ctx = numeric_context(directory.path());
            let value = read_state_parsed::<usize>(&ctx, directory.path(), "count")
                .or_abort("admit numeric state");
            assert_eq!(value, 5);
        }

        #[test]
        fn admits_leading_plus() {
            let directory = numeric_state("+5");
            let ctx = numeric_context(directory.path());
            let value = read_state_parsed::<usize>(&ctx, directory.path(), "count")
                .or_abort("admit numeric state");
            assert_eq!(value, 5);
        }

        #[test]
        fn admits_leading_zeros() {
            let directory = numeric_state("007");
            let ctx = numeric_context(directory.path());
            let value = read_state_parsed::<usize>(&ctx, directory.path(), "count")
                .or_abort("admit numeric state");
            assert_eq!(value, 7);
        }

        #[test]
        fn refuses_u8_maximum_plus_one() {
            let directory = numeric_state("256");
            let ctx = numeric_context(directory.path());
            let error = read_state_parsed::<u8>(&ctx, directory.path(), "count")
                .err_or_abort("refuse corrupt numeric state");
            assert_eq!(
                error.to_string(),
                "git command failed: corrupted state file 'count': invalid value '256'"
            );
        }

        #[test]
        fn refuses_invalid_value() {
            let directory = numeric_state("not-a-number\n");
            let ctx = numeric_context(directory.path());
            let error = read_state_parsed::<usize>(&ctx, directory.path(), "count")
                .err_or_abort("refuse corrupt numeric state");
            assert_eq!(
                error.to_string(),
                "git command failed: corrupted state file 'count': invalid value 'not-a-number'"
            );
        }

        #[test]
        fn refuses_negative_value() {
            let directory = numeric_state("-1");
            let ctx = numeric_context(directory.path());
            let error = read_state_parsed::<usize>(&ctx, directory.path(), "count")
                .err_or_abort("refuse corrupt numeric state");
            assert_eq!(
                error.to_string(),
                "git command failed: corrupted state file 'count': invalid value '-1'"
            );
        }

        #[test]
        fn refuses_internal_whitespace() {
            let directory = numeric_state("1 2");
            let ctx = numeric_context(directory.path());
            let error = read_state_parsed::<usize>(&ctx, directory.path(), "count")
                .err_or_abort("refuse corrupt numeric state");
            assert_eq!(
                error.to_string(),
                "git command failed: corrupted state file 'count': invalid value '1 2'"
            );
        }

        #[test]
        fn refuses_zero_byte_file() {
            let directory = numeric_state("");
            let ctx = numeric_context(directory.path());
            let error = read_state_parsed::<usize>(&ctx, directory.path(), "count")
                .err_or_abort("refuse corrupt numeric state");
            assert_eq!(
                error.to_string(),
                "git command failed: corrupted state file 'count': file is empty"
            );
        }

        #[test]
        fn refuses_whitespace_only_file() {
            let directory = numeric_state(" \n\t\u{2003}");
            let ctx = numeric_context(directory.path());
            let error = read_state_parsed::<usize>(&ctx, directory.path(), "count")
                .err_or_abort("refuse corrupt numeric state");
            assert_eq!(
                error.to_string(),
                "git command failed: corrupted state file 'count': file is empty"
            );
        }

        #[test]
        fn refuses_usize_maximum_plus_one() {
            let overflow = u128::try_from(usize::MAX)
                .or_abort("convert usize maximum")
                .checked_add(1)
                .or_abort("represent first overflowing usize")
                .to_string();
            let directory = numeric_state(&overflow);
            let ctx = numeric_context(directory.path());
            let error = read_state_parsed::<usize>(&ctx, directory.path(), "count")
                .err_or_abort("refuse overflowing numeric state");
            assert_eq!(
                error.to_string(),
                format!(
                    "git command failed: corrupted state file 'count': invalid value '{overflow}'"
                )
            );
        }

        #[test]
        fn propagates_missing_file() {
            let directory = tempfile::TempDir::new().or_abort("missing state fixture");
            let ctx = numeric_context(directory.path());
            let error = read_state_parsed::<usize>(&ctx, directory.path(), "missing")
                .err_or_abort("refuse missing state");
            assert!(
                matches!(error, FactorError::StateRead(inner) if inner.kind() == io::ErrorKind::NotFound)
            );
        }
    }

    use super::*;
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

    fn assert_state_read_error(err: &FactorError) {
        assert!(
            matches!(err, &FactorError::StateRead(_)),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    fn read_state_parsed_reports_invalid_value() {
        let dir = TempDir::new().or_abort("tempdir");
        let ctx = ctx_for(dir.path());
        let state_dir = dir.path().join("factor");
        fs::create_dir_all(&state_dir).or_abort("create state dir");
        fs::write(state_dir.join("current_index"), "not-a-number\n").or_abort("write state");

        let err = read_state_parsed::<usize>(&ctx, &state_dir, "current_index")
            .err_or_abort("invalid state should fail");
        assert_eq!(
            err.to_string(),
            "git command failed: corrupted state file 'current_index': invalid value 'not-a-number'"
        );
    }

    #[test]
    fn read_state_parsed_propagates_read_errors() {
        let dir = TempDir::new().or_abort("tempdir");
        let ctx = ctx_for(dir.path());
        let state_dir = dir.path().join("factor");
        fs::create_dir_all(&state_dir).or_abort("create state dir");

        let err = read_state_parsed::<usize>(&ctx, &state_dir, "missing")
            .err_or_abort("missing state should fail");
        assert_state_read_error(&err);
    }

    #[test]
    #[should_panic(expected = "unexpected error")]
    fn assert_state_read_error_panics_on_non_state_read_errors() {
        let err = FactorError::NotGitRepo;
        assert_state_read_error(&err);
    }

    #[test]
    fn proptest_run_non_panicking_unit_suite() {
        use std::panic::catch_unwind;

        read_state_parsed_propagates_read_errors();
        read_state_parsed_reports_invalid_value();

        assert!(catch_unwind(assert_state_read_error_panics_on_non_state_read_errors).is_err());
    }
}

#[cfg(test)]
mod proptests {
    mod read_state_parsed {
        use super::super::*;
        use crate::test_support::{OrAbort as _, ResultOrAbort as _};
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn admits_generated_padded_usize(
                value in any::<usize>(),
                padding in "[ \n\r\t\u{2003}]{0,5}",
            ) {
                let content = format!("{padding}{value}{padding}");
                let directory = numeric_state(&content);
                let ctx = numeric_context(directory.path());
                let parsed =
                    read_state_parsed::<usize>(&ctx, directory.path(), "count").or_abort("admit numeric state");
                prop_assert_eq!(parsed, value);
            }

            #[test]
            fn admits_generated_padded_u8(
                value in any::<u8>(),
                padding in "[ \n\r\t\u{2003}]{0,5}",
            ) {
                let content = format!("{padding}{value}{padding}");
                let directory = numeric_state(&content);
                let ctx = numeric_context(directory.path());
                let parsed =
                    read_state_parsed::<u8>(&ctx, directory.path(), "count").or_abort("admit numeric state");
                prop_assert_eq!(parsed, value);
            }

            #[test]
            fn refuses_generated_non_numeric_values(
                value in any::<usize>(),
            ) {
                let invalid = format!("x{value}");
                let directory = numeric_state(&invalid);
                let ctx = numeric_context(directory.path());
                let error = read_state_parsed::<usize>(&ctx, directory.path(), "count")
                    .err_or_abort("refuse invalid state");
                prop_assert_eq!(
                    error.to_string(),
                    format!("git command failed: corrupted state file 'count': invalid value '{invalid}'")
                );
            }

            #[test]
            fn refuses_generated_padding_only_files(
                padding in "[ \n\r\t\u{2003}]{0,5}",
            ) {
                let directory = numeric_state(&padding);
                let ctx = numeric_context(directory.path());
                let error = read_state_parsed::<usize>(&ctx, directory.path(), "count")
                    .err_or_abort("refuse empty state");
                prop_assert_eq!(
                    error.to_string(),
                    "git command failed: corrupted state file 'count': file is empty"
                );
            }
        }
    }
}
