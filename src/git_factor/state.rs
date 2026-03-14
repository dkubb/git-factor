use core::str::FromStr;
use std::io;
use std::path::Path;

use super::ctx::Ctx;
use super::error::{FactorError, non_empty_msg};
use crate::non_empty_string::NonEmptyString;

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
#[cfg_attr(
    test,
    expect(
        clippy::single_call_fn,
        reason = "write_state is introduced before the session layer persists multiple keys"
    )
)]
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
mod tests {
    use super::super::ctx::{REAL_ENV, REAL_FS, REAL_IO, REAL_RUNNER};
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    use crate::test_support::{OrAbort as _, ResultOrAbort as _};
    use tempfile::TempDir;

    fn ctx() -> Ctx<'static> {
        Ctx {
            cwd: PathBuf::from("."),
            env: &REAL_ENV,
            fs: &REAL_FS,
            io: &REAL_IO,
            runner: &REAL_RUNNER,
        }
    }

    fn assert_state_read_error(err: &FactorError) {
        assert!(
            matches!(err, &FactorError::StateRead(_)),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    fn write_state_round_trips_content() {
        let dir = TempDir::new().or_abort("tempdir");
        let state_dir = dir.path().join("factor");
        fs::create_dir_all(&state_dir).or_abort("create state dir");

        write_state(&ctx(), &state_dir, "phase", "pending_start").or_abort("write state");

        let value = read_state(&ctx(), &state_dir, "phase").or_abort("read state");
        assert_eq!(value.as_str(), "pending_start");
    }

    #[test]
    fn read_state_bool_or_default_uses_default_for_missing_file() {
        let dir = TempDir::new().or_abort("tempdir");
        let state_dir = dir.path().join("factor");
        fs::create_dir_all(&state_dir).or_abort("create state dir");

        let value = read_state_bool_or_default(&ctx(), &state_dir, "started_rebase", true)
            .or_abort("missing bool state should use default");
        assert!(value);
    }

    #[test]
    fn read_state_bool_or_default_reads_false_values() {
        let dir = TempDir::new().or_abort("tempdir");
        let state_dir = dir.path().join("factor");
        fs::create_dir_all(&state_dir).or_abort("create state dir");
        fs::write(state_dir.join("started_rebase"), "false\n").or_abort("write state");

        let value = read_state_bool_or_default(&ctx(), &state_dir, "started_rebase", true)
            .or_abort("bool state should parse");
        assert!(!value);
    }

    #[test]
    fn read_state_bool_or_default_rejects_invalid_values() {
        let dir = TempDir::new().or_abort("tempdir");
        let state_dir = dir.path().join("factor");
        fs::create_dir_all(&state_dir).or_abort("create state dir");
        fs::write(state_dir.join("started_rebase"), "maybe\n").or_abort("write state");

        let err = read_state_bool_or_default(&ctx(), &state_dir, "started_rebase", false)
            .err_or_abort("invalid bool state should fail");
        assert_eq!(
            err.to_string(),
            "git command failed: corrupted state file 'started_rebase': invalid value 'maybe'"
        );
    }

    #[test]
    fn read_state_parsed_reports_invalid_value() {
        let dir = TempDir::new().or_abort("tempdir");
        let state_dir = dir.path().join("factor");
        fs::create_dir_all(&state_dir).or_abort("create state dir");
        fs::write(state_dir.join("current_index"), "not-a-number\n").or_abort("write state");

        let err = read_state_parsed::<usize>(&ctx(), &state_dir, "current_index")
            .err_or_abort("invalid state should fail");
        assert_eq!(
            err.to_string(),
            "git command failed: corrupted state file 'current_index': invalid value 'not-a-number'"
        );
    }

    #[test]
    fn read_state_parsed_propagates_read_errors() {
        let dir = TempDir::new().or_abort("tempdir");
        let state_dir = dir.path().join("factor");
        fs::create_dir_all(&state_dir).or_abort("create state dir");

        let err = read_state_parsed::<usize>(&ctx(), &state_dir, "missing")
            .err_or_abort("missing state should fail");
        assert_state_read_error(&err);
    }

    #[test]
    fn read_state_rejects_empty_files() {
        let dir = TempDir::new().or_abort("tempdir");
        let state_dir = dir.path().join("factor");
        fs::create_dir_all(&state_dir).or_abort("create state dir");
        fs::write(state_dir.join("phase"), "\n").or_abort("write state");

        let err = read_state(&ctx(), &state_dir, "phase").err_or_abort("empty state should fail");
        assert_eq!(
            err.to_string(),
            "git command failed: corrupted state file 'phase': file is empty"
        );
    }

    #[test]
    #[should_panic(expected = "unexpected error")]
    fn assert_state_read_error_panics_on_non_state_read_errors() {
        let err = FactorError::NoActiveSession;
        assert_state_read_error(&err);
    }
}
