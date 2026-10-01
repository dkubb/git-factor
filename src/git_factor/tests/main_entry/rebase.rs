use super::super::*;
#[cfg(unix)]
use crate::git_factor::proptests::{NonUtf8Fs, TestRunner};
use crate::git_factor::proptests::{
    OverflowSplitCountFs, ScriptedRunner, TestEnv, TestFs, TestIo, success_status,
};

#[test]
fn run_start_rebase_uses_parent_arg_when_not_root() {
    let io = Box::leak(Box::new(TestIo::default()));
    let env = Box::leak(Box::new(TestEnv));
    let fs = Box::leak(Box::new(OverflowSplitCountFs));
    let runner = Box::leak(Box::new(ScriptedRunner::new(
        vec![
            Ok(Output {
                status: success_status(),
                stdout: b"abcd123\n".to_vec(),
                stderr: Vec::new(),
            }),
            Ok(Output {
                status: success_status(),
                stdout: b".git\n".to_vec(),
                stderr: Vec::new(),
            }),
        ],
        vec![Ok(success_status())],
    )));
    let ctx = Ctx {
        cwd: PathBuf::from("."),
        env,
        fs,
        io,
        runner,
    };
    let resolved = NonEmpty::new(
        CommitSha::new("1111111111111111111111111111111111111111".to_owned()).or_abort("valid sha"),
    );
    let span = CommitSpan::new(resolved, BaseParent::Commit);
    let start_head =
        CommitSha::new("3333333333333333333333333333333333333333".to_owned()).or_abort("valid sha");
    let exec_command = NonEmptyString::try_from("true".to_owned()).or_abort("non-empty");

    run_start_rebase_in(
        &ctx,
        &span,
        &StateDir::new(PathBuf::from(".git/factor")),
        &start_head,
        &exec_command,
    )
    .or_abort("non-root rebase should succeed");

    let calls = runner.status_calls.borrow();
    let args = calls.first().or_abort("status call should be captured");
    assert!(!args.iter().any(|arg| arg == "--root"));
    assert!(
        args.iter()
            .any(|arg| arg == "1111111111111111111111111111111111111111^")
    );
}

#[test]
fn run_start_rebase_uses_root_arg_when_root() {
    let io = Box::leak(Box::new(TestIo::default()));
    let env = Box::leak(Box::new(TestEnv));
    let fs = Box::leak(Box::new(OverflowSplitCountFs));
    let runner = Box::leak(Box::new(ScriptedRunner::new(
        vec![
            Ok(Output {
                status: success_status(),
                stdout: b"abcd123\n".to_vec(),
                stderr: Vec::new(),
            }),
            Ok(Output {
                status: success_status(),
                stdout: b".git\n".to_vec(),
                stderr: Vec::new(),
            }),
        ],
        vec![Ok(success_status())],
    )));
    let ctx = Ctx {
        cwd: PathBuf::from("."),
        env,
        fs,
        io,
        runner,
    };
    let resolved = NonEmpty::new(
        CommitSha::new("1111111111111111111111111111111111111111".to_owned()).or_abort("valid sha"),
    );
    let span = CommitSpan::new(resolved, BaseParent::Root);
    let start_head =
        CommitSha::new("3333333333333333333333333333333333333333".to_owned()).or_abort("valid sha");
    let exec_command = NonEmptyString::try_from("true".to_owned()).or_abort("non-empty");

    run_start_rebase_in(
        &ctx,
        &span,
        &StateDir::new(PathBuf::from(".git/factor")),
        &start_head,
        &exec_command,
    )
    .or_abort("root rebase should succeed");

    let calls = runner.status_calls.borrow();
    let args = calls.first().or_abort("status call should be captured");
    assert!(args.iter().any(|arg| arg == "--root"));
    assert!(
        !args
            .iter()
            .any(|arg| arg == "1111111111111111111111111111111111111111^")
    );
}

#[test]
fn run_start_rebase_propagates_status_io_errors() {
    let io = Box::leak(Box::new(TestIo::default()));
    let env = Box::leak(Box::new(TestEnv));
    let fs = Box::leak(Box::new(TestFs));
    let runner = Box::leak(Box::new(ScriptedRunner::new(
        vec![Ok(Output {
            status: success_status(),
            stdout: b"abcd123\n".to_vec(),
            stderr: Vec::new(),
        })],
        vec![Err(io::Error::other("runner failed"))],
    )));
    let ctx = Ctx {
        cwd: PathBuf::from("."),
        env,
        fs,
        io,
        runner,
    };
    let resolved = NonEmpty::new(
        CommitSha::new("1111111111111111111111111111111111111111".to_owned()).or_abort("valid sha"),
    );
    let span = CommitSpan::new(resolved, BaseParent::Commit);
    let start_head =
        CommitSha::new("3333333333333333333333333333333333333333".to_owned()).or_abort("valid sha");
    let exec_command = NonEmptyString::try_from("true".to_owned()).or_abort("non-empty");

    let err = run_start_rebase_in(
        &ctx,
        &span,
        &StateDir::new(PathBuf::from(".git/factor")),
        &start_head,
        &exec_command,
    )
    .err_or_abort("runner status failure should map to git command error");
    assert_eq!(
        err.to_string(),
        "git command failed: git rebase: runner failed"
    );
}

#[cfg(unix)]
#[test]
fn run_start_rebase_reports_non_utf8_editor_path() {
    let io = Box::leak(Box::new(TestIo::default()));
    let env = Box::leak(Box::new(TestEnv));
    let fs = Box::leak(Box::new(NonUtf8Fs));
    let runner = Box::leak(Box::new(TestRunner));
    let ctx = Ctx {
        cwd: PathBuf::from("."),
        env,
        fs,
        io,
        runner,
    };
    let resolved = NonEmpty::new(
        CommitSha::new("1111111111111111111111111111111111111111".to_owned()).or_abort("valid sha"),
    );
    let span = CommitSpan::new(resolved, BaseParent::Commit);
    let start_head =
        CommitSha::new("3333333333333333333333333333333333333333".to_owned()).or_abort("valid sha");
    let exec_command = NonEmptyString::try_from("true".to_owned()).or_abort("non-empty");

    let err = run_start_rebase_in(
        &ctx,
        &span,
        &StateDir::new(PathBuf::from(".git/factor")),
        &start_head,
        &exec_command,
    )
    .err_or_abort("non-utf8 editor path should fail");
    assert_eq!(
        err.to_string(),
        "git command failed: editor path is not valid UTF-8"
    );
}
