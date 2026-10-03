use crate::exit_codes::{EXIT_OK, EXIT_SOFTWARE, EXIT_USAGE};
use crate::git_factor::tests::dispatch_contracts::{
    HeadResolutionFault, ReopenFault, default_head_failure, default_head_success, dirty_route,
    parser, parser_write_failure, refusal, reopen_failure,
};
use core::num::NonZeroUsize;

use crate::git_factor::tests::start_contracts::DirectStart;

use super::*;

#[test]
fn public_abort_refuses_unavailable_fallback_without_changing_saved_facts() {
    verify_public_abort_fallback_refusal(1);
}

#[test]
fn public_status_reports_final_newline_failure_with_saved_facts_retained() {
    status_contracts::active(
        &"a".repeat(SHA_LEN),
        0,
        false,
        false,
        "splitting",
        false,
        16,
    );
}

#[test]
fn public_status_refuses_invalid_phase_without_changing_saved_facts() {
    verify_public_status_phase_refusal("unsupported");
}

#[test]
fn public_status_refuses_empty_phase_without_changing_saved_facts() {
    verify_public_status_phase_refusal("");
}

#[test]
fn cmd_start_errors_when_symmetric_diff_range_is_requested() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let runner = ScriptedRunner::default()
        .with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n")
        .with_output("git", &["status", "--porcelain=v1"], repo, "");
    let io = TestIo::default();
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };
    let ctx = Ctx {
        runner: &runner,
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &REAL_FS,
    };

    let err = run_with_args_vec(
        &ctx,
        vec![
            OsString::from("git-factor"),
            OsString::from("--exec"),
            OsString::from("true"),
            OsString::from("HEAD...HEAD"),
        ],
    )
    .err_or_abort("expected symmetric diff range to be rejected");
    assert_eq!(
        err.to_string(),
        "invalid commit: HEAD...HEAD (symmetric diff '...' is not supported, use '..')"
    );
}

#[test]
fn cmd_start_errors_when_merge_base_spawn_fails() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let sha = "a".repeat(SHA_LEN);
    let runner = start_single_head_resolution_runner(repo, &sha).with_commit_object(
        repo,
        &sha,
        Some(&"d".repeat(SHA_LEN)),
    );
    let io = TestIo::default();
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };
    let ctx = Ctx {
        runner: &runner,
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &REAL_FS,
    };

    let err = run_with_args_vec(
        &ctx,
        vec![
            OsString::from("git-factor"),
            OsString::from("--exec"),
            OsString::from("true"),
            OsString::from("HEAD"),
        ],
    )
    .err_or_abort("expected merge-base spawn to fail");

    assert!(
        err.to_string().contains("merge-base"),
        "unexpected error: {err}"
    );
}

#[test]
fn cmd_start_errors_when_short_sha_is_empty() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let sha = "a".repeat(SHA_LEN);
    let runner = start_single_head_validation_runner(repo, &sha).with_output(
        "git",
        &["rev-parse", "--short", &sha],
        repo,
        "\n",
    );
    let io = TestIo::default();
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };
    let ctx = Ctx {
        runner: &runner,
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &REAL_FS,
    };

    let err = run_with_args_vec(
        &ctx,
        vec![
            OsString::from("git-factor"),
            OsString::from("--exec"),
            OsString::from("true"),
            OsString::from("HEAD"),
        ],
    )
    .err_or_abort("expected short SHA to be empty");

    assert_eq!(err.to_string(), "git command failed: empty short SHA");
}

#[test]
fn cmd_start_propagates_rev_parse_short_sha_output_error() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let sha = "a".repeat(SHA_LEN);
    let runner = start_single_head_validation_runner(repo, &sha);
    let io = TestIo::default();
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };
    let ctx = Ctx {
        runner: &runner,
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &REAL_FS,
    };

    let err = run_with_args_vec(
        &ctx,
        vec![
            OsString::from("git-factor"),
            OsString::from("--exec"),
            OsString::from("true"),
            OsString::from("HEAD"),
        ],
    )
    .err_or_abort("expected rev-parse --short to fail");

    assert!(
        err.to_string().contains("git rev-parse:")
            && err.to_string().contains("unexpected output call"),
        "unexpected error: {err}"
    );
}

#[test]
fn cmd_start_propagates_status_error_when_start_sequence_fails() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    fs::write(repo.join("git-factor"), "").or_abort("write fake exe");

    let sha = "a".repeat(SHA_LEN);
    let runner = with_start_gate_result(
        start_single_head_validation_runner(repo, &sha)
            .with_output("git", &["rev-parse", "--short", &sha], repo, "aaaaaaa\n")
            .with_output(
                "git",
                &["show", "--format=%B", "--no-patch", &sha],
                repo,
                "msg\n",
            ),
        repo,
        "true",
        0,
        "",
        "",
    )
    .with_output(
        "git",
        &["rev-parse", "HEAD^{tree}"],
        repo,
        &format!("{}\n", "c".repeat(SHA_LEN)),
    );

    let io = TestIo::default();
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };
    let ctx = Ctx {
        runner: &runner,
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &REAL_FS,
    };

    let err = run_with_args_vec(
        &ctx,
        vec![
            OsString::from("git-factor"),
            OsString::from("--exec"),
            OsString::from("true"),
            OsString::from("HEAD"),
        ],
    )
    .err_or_abort("expected startup status lookup to fail");

    assert!(
        matches!(
            &err,
            FactorError::GitCommand(msg)
                if msg.contains("git reset:") && msg.contains("unexpected status call")
        ),
        "err was: {err:?}"
    );
}

#[test]
fn cmd_start_propagates_rebase_status_error_in_multi_commit_session() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    fs::write(repo.join("git-factor"), "").or_abort("write fake exe");

    let sha_a = "a".repeat(SHA_LEN);
    let sha_b = "b".repeat(SHA_LEN);
    let runner = start_multi_commit_runner_base(repo, &sha_a, &sha_b)
        .with_output("git", &["rev-parse", "--short", &sha_a], repo, "aaaaaaa\n")
        .with_output("git", &["rev-parse", "--short", &sha_b], repo, "bbbbbbb\n");
    let io = TestIo::default();
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };
    let ctx = Ctx {
        runner: &runner,
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &REAL_FS,
    };

    let err = run_with_args_vec(
        &ctx,
        vec![
            OsString::from("git-factor"),
            OsString::from("--exec"),
            OsString::from("true"),
            OsString::from(sha_a.as_str()),
            OsString::from(sha_b.as_str()),
        ],
    )
    .err_or_abort("expected rebase status call to fail");

    assert!(
        matches!(&err, FactorError::GitCommand(msg) if msg.contains("git rebase:") && msg.contains("unexpected status call")),
        "err was: {err:?}"
    );
}

#[test]
fn cmd_start_propagates_requires_rebase_state_write_failure() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let fs = FailingRequiresRebaseWriteFs;

    assert_cmd_start_state_write_failure(
        repo,
        &fs,
        "failed to write state: requires_rebase write failed",
    );
}

#[test]
fn cmd_start_propagates_is_root_state_write_failure() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let fs = FailingIsRootWriteFs;

    assert_cmd_start_state_write_failure(repo, &fs, "failed to write state: is_root write failed");
}

#[test]
fn cmd_start_range_ref_inserts_shas_and_propagates_io_error_on_multi_commit_banner() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    fs::write(repo.join("git-factor"), "").or_abort("write fake exe");

    let sha_a = "a".repeat(SHA_LEN);
    let sha_b = "b".repeat(SHA_LEN);
    let runner = start_range_ref_runner(repo, &sha_a, &sha_b);
    let io = FailingIo;
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };
    let ctx = Ctx {
        runner: &runner,
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &REAL_FS,
    };

    let err = run_with_args_vec(
        &ctx,
        vec![
            OsString::from("git-factor"),
            OsString::from("--exec"),
            OsString::from("true"),
            OsString::from("a^..b"),
        ],
    )
    .err_or_abort("expected io failure");

    assert!(
        matches!(&err, FactorError::Io(inner) if inner.to_string().contains("io fail")),
        "err was: {err:?}"
    );
}

#[test]
fn cmd_start_range_ref_inserts_shas_and_starts_multi_commit_session() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    fs::write(repo.join("git-factor"), "").or_abort("write fake exe");

    let sha_a = "a".repeat(SHA_LEN);
    let sha_b = "b".repeat(SHA_LEN);
    let runner = start_range_ref_runner(repo, &sha_a, &sha_b);
    let io = TestIo::default();
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };
    let ctx = Ctx {
        runner: &runner,
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &REAL_FS,
    };

    let code = run_with_args_vec(
        &ctx,
        vec![
            OsString::from("git-factor"),
            OsString::from("--exec"),
            OsString::from("true"),
            OsString::from("a^..b"),
        ],
    )
    .or_abort("expected multi-commit start to succeed");

    assert_eq!(code, EXIT_OK);
    assert!(
        io.stdout()
            .contains("FACTOR: Split session started for 2 commits (tip: bbbbbbb)."),
        "stdout was: {}",
        io.stdout()
    );
    assert!(io.stderr().is_empty(), "stderr should be empty");
    let state_dir = repo.join(".git").join("factor");
    assert!(state_dir.is_dir(), "factor state dir should exist");
    assert_eq!(
        fs::read_to_string(state_dir.join("requires_rebase")).or_abort("read requires_rebase"),
        "false\n"
    );
}

#[test]
fn cmd_start_errors_when_factor_session_is_already_active() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    fs::create_dir_all(repo.join(".git").join("factor")).or_abort("create active factor dir");

    let runner =
        ScriptedRunner::default().with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n");
    let io = TestIo::default();
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };
    let ctx = Ctx {
        runner: &runner,
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &REAL_FS,
    };

    let exec = NonEmpty::new(NonEmptyString::try_from("true".to_owned()).or_abort("exec"));
    let commits = NonEmpty::new(NonEmptyString::try_from("HEAD".to_owned()).or_abort("commit"));

    let err = cmd_start_in(&ctx, &exec, &commits).err_or_abort("expected active session error");
    assert!(
        matches!(err, FactorError::ActiveSession),
        "err was: {err:?}"
    );
}

#[test]
fn cmd_start_errors_when_rebase_is_already_active() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    fs::create_dir_all(repo.join(".git").join("rebase-merge")).or_abort("create rebase-merge");

    let runner =
        ScriptedRunner::default().with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n");
    let io = TestIo::default();
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };
    let ctx = Ctx {
        runner: &runner,
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &REAL_FS,
    };

    let exec = NonEmpty::new(NonEmptyString::try_from("true".to_owned()).or_abort("exec"));
    let commits = NonEmpty::new(NonEmptyString::try_from("HEAD".to_owned()).or_abort("commit"));

    let err = cmd_start_in(&ctx, &exec, &commits).err_or_abort("expected active rebase error");
    assert!(matches!(err, FactorError::ActiveRebase), "err was: {err:?}");
}

#[test]
fn cmd_start_errors_when_worktree_is_dirty() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();

    let runner = ScriptedRunner::default()
        .with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n")
        .with_output(
            "git",
            &["status", "--porcelain=v1"],
            repo,
            " M src/lib.rs\n",
        );
    let io = TestIo::default();
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };
    let ctx = Ctx {
        runner: &runner,
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &REAL_FS,
    };

    let exec = NonEmpty::new(NonEmptyString::try_from("true".to_owned()).or_abort("exec"));
    let commits = NonEmpty::new(NonEmptyString::try_from("HEAD".to_owned()).or_abort("commit"));

    let err = cmd_start_in(&ctx, &exec, &commits).err_or_abort("expected dirty worktree error");
    assert!(
        matches!(
            &err,
            FactorError::GitCommand(msg)
                if msg.contains("working tree must be clean before starting")
        ),
        "err was: {err:?}"
    );
}

#[test]
fn cmd_start_single_head_session_exec_gate_failure_returns_exec_failed() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let sha = "a".repeat(SHA_LEN);
    let runner = with_start_gate_result(
        start_single_head_validation_runner(repo, &sha)
            .with_output("git", &["rev-parse", "--short", &sha], repo, "aaaaaaa\n")
            .with_output(
                "git",
                &["show", "--format=%B", "--no-patch", &sha],
                repo,
                "subject\n",
            ),
        repo,
        "true",
        1,
        "",
        "",
    );
    let io = TestIo::default();
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };
    let ctx = Ctx {
        runner: &runner,
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &REAL_FS,
    };

    let err = run_with_args_vec(
        &ctx,
        vec![
            OsString::from("git-factor"),
            OsString::from("--exec"),
            OsString::from("true"),
            OsString::from("HEAD"),
        ],
    )
    .err_or_abort("expected exec gate failure");
    assert!(
        matches!(
            &err,
            FactorError::ExecFailed { code, command } if *code != EXIT_OK && command.as_str() == "true"
        ),
        "err was: {err:?}"
    );
}

#[test]
fn cmd_start_single_head_session_exec_gate_failure_with_multiple_exec_commands_returns_exec_failed()
{
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let sha = "a".repeat(SHA_LEN);
    let runner = with_start_gate_result(
        start_single_head_validation_runner(repo, &sha)
            .with_output("git", &["rev-parse", "--short", &sha], repo, "aaaaaaa\n")
            .with_output(
                "git",
                &["show", "--format=%B", "--no-patch", &sha],
                repo,
                "subject\n",
            ),
        repo,
        "true && false",
        1,
        "",
        "",
    );
    let io = TestIo::default();
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };
    let ctx = Ctx {
        runner: &runner,
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &REAL_FS,
    };

    let err = run_with_args_vec(
        &ctx,
        vec![
            OsString::from("git-factor"),
            OsString::from("--exec"),
            OsString::from("true"),
            OsString::from("--exec"),
            OsString::from("false"),
            OsString::from("HEAD"),
        ],
    )
    .err_or_abort("expected exec gate failure");
    assert!(
        matches!(
            &err,
            FactorError::ExecFailed { code, command } if *code != EXIT_OK && command.as_str() == "true && false"
        ),
        "err was: {err:?}"
    );
}

#[test]
fn cmd_start_propagates_head_lookup_error_after_sorting() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let sha = "a".repeat(SHA_LEN);
    let runner = ScriptedRunner::default()
        .with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n")
        .with_output("git", &["status", "--porcelain=v1"], repo, "")
        .with_output(
            "git",
            &["rev-parse", "--verify", &sha],
            repo,
            &format!("{sha}\n"),
        )
        .with_output(
            "git",
            &["rev-list", "--reverse", "--topo-order", &sha],
            repo,
            &format!("{sha}\n"),
        )
        .with_commit_object(repo, &sha, Some(&"d".repeat(SHA_LEN)));
    let io = TestIo::default();
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };
    let ctx = Ctx {
        runner: &runner,
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &REAL_FS,
    };

    let err = run_with_args_vec(
        &ctx,
        vec![
            OsString::from("git-factor"),
            OsString::from("--exec"),
            OsString::from("true"),
            OsString::from(&sha),
        ],
    )
    .err_or_abort("expected HEAD lookup failure");
    assert!(
        matches!(&err, FactorError::InvalidCommit(commit) if commit == "HEAD"),
        "err was: {err:?}"
    );
}

#[test]
fn cmd_start_single_head_session_propagates_exec_status_io_error() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let sha = "a".repeat(SHA_LEN);
    let runner = start_single_head_validation_runner(repo, &sha)
        .with_output("git", &["rev-parse", "--short", &sha], repo, "aaaaaaa\n")
        .with_output(
            "git",
            &["show", "--format=%B", "--no-patch", &sha],
            repo,
            "subject\n",
        )
        .with_status(
            "bash",
            &["--norc", "--noprofile", "-n", "-c", "true"],
            &[],
            true,
            repo,
            0,
        );
    let io = TestIo::default();
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };
    let ctx = Ctx {
        runner: &runner,
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &REAL_FS,
    };

    let err = run_with_args_vec(
        &ctx,
        vec![
            OsString::from("git-factor"),
            OsString::from("--exec"),
            OsString::from("true"),
            OsString::from("HEAD"),
        ],
    )
    .err_or_abort("expected exec status lookup failure");
    assert!(
        matches!(&err, FactorError::GitCommand(msg) if msg.contains("unexpected output call")),
        "err was: {err:?}"
    );
}

#[test]
fn cmd_start_propagates_commits_state_write_failure() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let sha = "a".repeat(SHA_LEN);
    let fs = FailingWriteForFileFs {
        file_name: "commits",
        message: "commits write failed",
    };
    let runner = with_start_gate_result(
        start_single_head_validation_runner(repo, &sha)
            .with_output("git", &["rev-parse", "--short", &sha], repo, "aaaaaaa\n")
            .with_output(
                "git",
                &["show", "--format=%B", "--no-patch", &sha],
                repo,
                "subject\n",
            ),
        repo,
        "true",
        0,
        "",
        "",
    );
    let io = TestIo::default();
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };
    let ctx = Ctx {
        runner: &runner,
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &fs,
    };

    let err = run_with_args_vec(
        &ctx,
        vec![
            OsString::from("git-factor"),
            OsString::from("--exec"),
            OsString::from("true"),
            OsString::from("HEAD"),
        ],
    )
    .err_or_abort("expected commits state write failure");
    assert!(
        matches!(&err, FactorError::StateWrite(inner) if inner.to_string().contains("commits write failed")),
        "err was: {err:?}"
    );
}

#[test]
fn cmd_start_propagates_editor_path_error_in_multi_commit_session() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let sha_a = "a".repeat(SHA_LEN);
    let sha_b = "b".repeat(SHA_LEN);
    let runner = start_multi_commit_runner_base(repo, &sha_a, &sha_b);
    let io = TestIo::default();
    let env = ExeFailingEnv {
        cwd: repo.to_path_buf(),
    };
    let ctx = Ctx {
        runner: &runner,
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &REAL_FS,
    };

    let err = run_start_two_shas(&ctx, &sha_a, &sha_b).err_or_abort("expected editor path error");
    assert_eq!(
        err.to_string(),
        "git command failed: cannot resolve current exe: no exe"
    );
}

#[test]
fn cmd_start_propagates_short_sha_lookup_error_for_sequence_editor() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    fs::write(repo.join("git-factor"), "").or_abort("write fake exe");
    let sha_a = "a".repeat(SHA_LEN);
    let sha_b = "b".repeat(SHA_LEN);
    let runner = start_multi_commit_runner_base(repo, &sha_a, &sha_b).with_output(
        "git",
        &["rev-parse", "--short", &sha_a],
        repo,
        "aaaaaaa\n",
    );
    let io = TestIo::default();
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };
    let ctx = Ctx {
        runner: &runner,
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &REAL_FS,
    };

    let err =
        run_start_two_shas(&ctx, &sha_a, &sha_b).err_or_abort("expected short SHA lookup failure");
    assert!(
        matches!(&err, FactorError::GitCommand(msg) if msg.contains("git rev-parse:") && msg.contains("unexpected output call")),
        "err was: {err:?}"
    );
}

#[test]
fn cmd_start_propagates_expected_tree_capture_error_after_state_write() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let sha = "a".repeat(SHA_LEN);
    let runner = with_start_gate_result(
        start_single_head_validation_runner(repo, &sha)
            .with_output("git", &["rev-parse", "--short", &sha], repo, "aaaaaaa\n")
            .with_output(
                "git",
                &["show", "--format=%B", "--no-patch", &sha],
                repo,
                "subject\n",
            ),
        repo,
        "true",
        0,
        "",
        "",
    );
    let io = TestIo::default();
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };
    let ctx = Ctx {
        runner: &runner,
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &REAL_FS,
    };

    let err = run_with_args_vec(
        &ctx,
        vec![
            OsString::from("git-factor"),
            OsString::from("--exec"),
            OsString::from("true"),
            OsString::from("HEAD"),
        ],
    )
    .err_or_abort("expected expected-tree capture error");
    assert!(
        matches!(&err, FactorError::GitCommand(msg) if msg.contains("git rev-parse:") && msg.contains("unexpected output call")),
        "err was: {err:?}"
    );
}

#[test]
fn cmd_start_root_session_propagates_mixed_reset_error() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let sha = "a".repeat(SHA_LEN);
    let runner = with_start_gate_result(
        start_single_head_resolution_runner(repo, &sha)
            .with_status(
                "git",
                &["merge-base", "--is-ancestor", &sha, "HEAD"],
                &[],
                true,
                repo,
                0,
            )
            .with_commit_object(repo, &sha, None)
            .with_commit_object(repo, &sha, None)
            .with_output("git", &["rev-parse", "--short", &sha], repo, "aaaaaaa\n")
            .with_output(
                "git",
                &["show", "--format=%B", "--no-patch", &sha],
                repo,
                "subject\n",
            ),
        repo,
        "true",
        0,
        "",
        "",
    )
    .with_output(
        "git",
        &["rev-parse", "HEAD^{tree}"],
        repo,
        &format!("{}\n", "c".repeat(SHA_LEN)),
    );
    let io = TestIo::default();
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };
    let ctx = Ctx {
        runner: &runner,
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &REAL_FS,
    };

    let err = run_with_args_vec(
        &ctx,
        vec![
            OsString::from("git-factor"),
            OsString::from("--exec"),
            OsString::from("true"),
            OsString::from("HEAD"),
        ],
    )
    .err_or_abort("expected mixed-reset failure");
    assert!(
        matches!(&err, FactorError::GitCommand(msg) if msg.contains("git commit-tree:") && msg.contains("unexpected output call")),
        "err was: {err:?}"
    );
}

#[test]
fn cmd_start_rebase_failure_does_not_warn_when_state_was_never_created() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    fs::write(repo.join("git-factor"), "").or_abort("create git-factor");
    let sha_a = "a".repeat(SHA_LEN);
    let sha_b = "b".repeat(SHA_LEN);
    let start_head = "c".repeat(SHA_LEN);
    let seq_editor = build_sequence_editor(repo, &sha_a, &sha_b, &start_head);
    let runner = start_rebase_failure_runner(repo, &sha_a, &sha_b, &seq_editor);
    let io = TestIo::default();
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };
    let ctx = Ctx {
        runner: &runner,
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &FailingRemoveDirAllFs,
    };

    let err = run_start_two_shas(&ctx, &sha_a, &sha_b).err_or_abort("expected rebase failure");
    assert!(
        matches!(&err, FactorError::GitCommand(msg) if msg.contains("git rebase failed (exit")),
        "err was: {err:?}"
    );
    assert!(io.stderr().is_empty(), "stderr should be empty");
}

#[test]
fn cmd_start_propagates_worktree_status_error_before_commit_resolution() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let runner =
        ScriptedRunner::default().with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n");
    let io = TestIo::default();
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };
    let ctx = Ctx {
        runner: &runner,
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &REAL_FS,
    };

    let err = run_with_args_vec(
        &ctx,
        vec![
            OsString::from("git-factor"),
            OsString::from("--exec"),
            OsString::from("true"),
            OsString::from("HEAD"),
        ],
    )
    .err_or_abort("expected status output failure");
    assert!(
        matches!(&err, FactorError::GitCommand(msg) if msg.contains("git status:") && msg.contains("unexpected output call")),
        "err was: {err:?}"
    );
}

#[test]
fn cmd_start_rebase_failure_skips_cleanup_warning_io_when_state_was_never_created() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    fs::write(repo.join("git-factor"), "").or_abort("create git-factor");
    let sha_a = "a".repeat(SHA_LEN);
    let sha_b = "b".repeat(SHA_LEN);
    let start_head = "c".repeat(SHA_LEN);
    let seq_editor = build_sequence_editor(repo, &sha_a, &sha_b, &start_head);
    let runner = start_rebase_failure_runner(repo, &sha_a, &sha_b, &seq_editor);
    let io = FailingIo;
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };
    let ctx = Ctx {
        runner: &runner,
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &FailingRemoveDirAllFs,
    };

    let err =
        run_start_two_shas(&ctx, &sha_a, &sha_b).err_or_abort("expected rebase failure error");
    assert!(
        matches!(&err, FactorError::GitCommand(msg) if msg.contains("git rebase failed (exit")),
        "err was: {err:?}"
    );
}

#[test]
fn cmd_start_reports_rebase_failure_without_cleanup_warning_when_state_removal_succeeds() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    fs::write(repo.join("git-factor"), "").or_abort("create git-factor");
    let sha_a = "a".repeat(SHA_LEN);
    let sha_b = "b".repeat(SHA_LEN);
    let start_head = "c".repeat(SHA_LEN);
    let seq_editor = build_sequence_editor(repo, &sha_a, &sha_b, &start_head);
    let runner = start_rebase_failure_runner(repo, &sha_a, &sha_b, &seq_editor);
    let io = TestIo::default();
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };
    let ctx = Ctx {
        runner: &runner,
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &REAL_FS,
    };

    let err = run_start_two_shas(&ctx, &sha_a, &sha_b).err_or_abort("expected rebase failure");
    assert!(
        matches!(&err, FactorError::GitCommand(msg) if msg.contains("git rebase failed (exit")),
        "err was: {err:?}"
    );
    assert!(io.stderr().is_empty(), "stderr should be empty");
}

#[test]
fn cmd_start_single_head_session_succeeds() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let sha = "a".repeat(SHA_LEN);
    let tree = "0123456789abcdef0123456789abcdef01234567";
    let diff_stat = " src/lib.rs | 1 +\n 1 file changed, 1 insertion(+)\n";
    let runner = start_single_head_runner(repo, &sha, tree, diff_stat);
    let io = TestIo::default();
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };
    let ctx = Ctx {
        runner: &runner,
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &REAL_FS,
    };

    let code = run_with_args_vec(
        &ctx,
        vec![
            OsString::from("git-factor"),
            OsString::from("--exec"),
            OsString::from("true"),
            OsString::from("HEAD"),
        ],
    )
    .or_abort("expected single-head start to succeed");

    assert_eq!(code, EXIT_OK);
    assert!(
        io.stdout()
            .contains("FACTOR: Split session started for aaaaaaa."),
        "stdout was: {}",
        io.stdout()
    );
    assert!(io.stderr().is_empty(), "stderr should be empty");

    let state_dir = repo.join(".git").join("factor");
    assert!(state_dir.is_dir(), "factor state dir should exist");
    assert_eq!(
        fs::read_to_string(state_dir.join("requires_rebase")).or_abort("read requires_rebase"),
        "false\n"
    );
}

#[test]
fn cmd_start_single_head_session_defaults_missing_commit_to_head() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let sha = "a".repeat(SHA_LEN);
    let tree = "0123456789abcdef0123456789abcdef01234567";
    let diff_stat = " src/lib.rs | 1 +\n 1 file changed, 1 insertion(+)\n";
    let runner = start_resolved_head_runner(repo, &sha, tree, diff_stat);
    let io = TestIo::default();
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };
    let ctx = Ctx {
        runner: &runner,
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &REAL_FS,
    };

    let code = run_with_args_vec(
        &ctx,
        vec![
            OsString::from("git-factor"),
            OsString::from("--exec"),
            OsString::from("true"),
        ],
    )
    .or_abort("expected implicit-head start to succeed");

    assert_eq!(code, EXIT_OK);
    assert!(
        io.stdout()
            .contains("FACTOR: Split session started for aaaaaaa."),
        "stdout was: {}",
        io.stdout()
    );
    assert!(io.stderr().is_empty(), "stderr should be empty");

    let state_dir = repo.join(".git").join("factor");
    assert!(state_dir.is_dir(), "factor state dir should exist");
    assert_eq!(
        fs::read_to_string(state_dir.join("requires_rebase")).or_abort("read requires_rebase"),
        "false\n"
    );
}

#[test]
fn cmd_start_head_in_direct_path_succeeds() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let sha = "a".repeat(SHA_LEN);
    let tree = "0123456789abcdef0123456789abcdef01234567";
    let diff_stat = " src/lib.rs | 1 +\n 1 file changed, 1 insertion(+)\n";
    let runner = start_resolved_head_runner(repo, &sha, tree, diff_stat);
    let io = TestIo::default();
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };
    let ctx = Ctx {
        runner: &runner,
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &REAL_FS,
    };
    let exec = NonEmpty::new(NonEmptyString::try_from("true".to_owned()).or_abort("exec"));

    let state_dir = cmd_start_prep_in(&ctx).or_abort("start prep should succeed");
    let resolved_commits =
        NonEmpty::new(resolve_head_commit(&ctx).or_abort("head commit should resolve"));
    let code = cmd_start_with_resolved_in(&ctx, &exec, &state_dir, &resolved_commits)
        .or_abort("expected direct head start to succeed");

    assert_eq!(code, EXIT_OK);
}

#[test]
fn cmd_start_single_head_root_session_uses_empty_tree_reset() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let sha = "a".repeat(SHA_LEN);
    let synthetic_root = "b".repeat(SHA_LEN);
    let tree = "0123456789abcdef0123456789abcdef01234567";
    let diff_stat = " src/lib.rs | 1 +\n 1 file changed, 1 insertion(+)\n";
    let runner = start_single_head_root_runner(repo, &sha, &synthetic_root, tree, diff_stat, 0);
    let io = TestIo::default();
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };
    let ctx = Ctx {
        runner: &runner,
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &REAL_FS,
    };

    let code = run_with_args_vec(
        &ctx,
        vec![
            OsString::from("git-factor"),
            OsString::from("--exec"),
            OsString::from("true"),
            OsString::from("HEAD"),
        ],
    )
    .or_abort("expected root single-head start to succeed");
    assert_eq!(code, EXIT_OK);
}

#[test]
fn cmd_start_single_head_root_session_propagates_reset_error() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let sha = "a".repeat(SHA_LEN);
    let synthetic_root = "b".repeat(SHA_LEN);
    let tree = "0123456789abcdef0123456789abcdef01234567";
    let diff_stat = " src/lib.rs | 1 +\n 1 file changed, 1 insertion(+)\n";
    let runner = start_single_head_root_runner(repo, &sha, &synthetic_root, tree, diff_stat, 2);
    let io = TestIo::default();
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };
    let ctx = Ctx {
        runner: &runner,
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &REAL_FS,
    };

    let err = run_with_args_vec(
        &ctx,
        vec![
            OsString::from("git-factor"),
            OsString::from("--exec"),
            OsString::from("true"),
            OsString::from("HEAD"),
        ],
    )
    .err_or_abort("expected root reset failure");
    assert!(
        matches!(&err, FactorError::GitCommand(msg) if msg.contains("git reset failed")),
        "err was: {err:?}"
    );
}

#[test]
fn cmd_start_rejects_merge_commit_during_validation() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let sha = "a".repeat(SHA_LEN);
    let runner = start_single_head_resolution_runner(repo, &sha)
        .with_status(
            "git",
            &["merge-base", "--is-ancestor", &sha, "HEAD"],
            &[],
            true,
            repo,
            0,
        )
        .with_output("git", &["cat-file", "commit", &sha], repo, &format!("tree {}\nparent {}\nparent {}\nauthor Example <example@example.com> 1 +0000\n\nsubject\n", "c".repeat(SHA_LEN), "b".repeat(SHA_LEN), "d".repeat(SHA_LEN)));
    let io = TestIo::default();
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };
    let ctx = Ctx {
        runner: &runner,
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &REAL_FS,
    };
    let exec = NonEmpty::new(NonEmptyString::try_from("true".to_owned()).or_abort("exec"));
    let commits = NonEmpty::new(NonEmptyString::try_from("HEAD".to_owned()).or_abort("commit"));

    let err = cmd_start_in(&ctx, &exec, &commits).err_or_abort("expected merge commit error");
    assert!(
        matches!(&err, FactorError::MergeCommit(found) if found.as_str() == sha.as_str()),
        "err was: {err:?}"
    );
}

#[test]
fn cmd_start_propagates_commit_message_lookup_error() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let sha = "a".repeat(SHA_LEN);
    let runner = start_single_head_validation_runner(repo, &sha).with_output(
        "git",
        &["rev-parse", "--short", &sha],
        repo,
        "aaaaaaa\n",
    );
    let io = TestIo::default();
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };
    let ctx = Ctx {
        runner: &runner,
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &REAL_FS,
    };
    let exec = NonEmpty::new(NonEmptyString::try_from("true".to_owned()).or_abort("exec"));
    let commits = NonEmpty::new(NonEmptyString::try_from("HEAD".to_owned()).or_abort("commit"));

    let err =
        cmd_start_in(&ctx, &exec, &commits).err_or_abort("expected commit message lookup error");
    assert!(
        matches!(&err, FactorError::GitCommand(msg) if msg.contains("git show:") && msg.contains("unexpected output call")),
        "err was: {err:?}"
    );
}

#[test]
fn cmd_start_propagates_invalid_exec_syntax() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let sha = "a".repeat(SHA_LEN);
    let runner = start_single_head_validation_runner(repo, &sha)
        .with_output("git", &["rev-parse", "--short", &sha], repo, "aaaaaaa\n")
        .with_output(
            "git",
            &["show", "--format=%B", "--no-patch", &sha],
            repo,
            "subject\n",
        )
        .with_status(
            "bash",
            &["--norc", "--noprofile", "-n", "-c", "true"],
            &[],
            true,
            repo,
            1,
        );
    let io = TestIo::default();
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };
    let ctx = Ctx {
        runner: &runner,
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &REAL_FS,
    };
    let exec = NonEmpty::new(NonEmptyString::try_from("true".to_owned()).or_abort("exec"));
    let commits = NonEmpty::new(NonEmptyString::try_from("HEAD".to_owned()).or_abort("commit"));

    let err = cmd_start_in(&ctx, &exec, &commits).err_or_abort("expected invalid exec syntax");
    assert!(
        matches!(&err, FactorError::InvalidExecSyntax(command) if command == "true"),
        "err was: {err:?}"
    );
}

#[test]
fn cmd_start_propagates_create_dir_all_failure_for_state_dir() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    fs::create_dir_all(repo.join(".git")).or_abort("create .git");
    fs::write(repo.join(".git").join("factor"), "not a directory").or_abort("write factor file");

    let sha = "a".repeat(SHA_LEN);
    let runner =
        start_single_head_runner(repo, &sha, "0123456789abcdef0123456789abcdef01234567", "");
    let io = TestIo::default();
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };
    let ctx = Ctx {
        runner: &runner,
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &REAL_FS,
    };
    let exec = NonEmpty::new(NonEmptyString::try_from("true".to_owned()).or_abort("exec"));
    let commits = NonEmpty::new(NonEmptyString::try_from("HEAD".to_owned()).or_abort("commit"));

    let err = cmd_start_in(&ctx, &exec, &commits)
        .err_or_abort("expected state directory creation failure");
    assert!(
        matches!(&err, FactorError::StateWrite(inner) if inner.kind() == io::ErrorKind::AlreadyExists || inner.to_string().contains("exists")),
        "err was: {err:?}"
    );
}

#[test]
fn run_start_rebase_uses_parent_arg_when_not_root() {
    use crate::git_factor::proptests::{
        OverflowSplitCountFs, ScriptedRunner, TestEnv, TestIo, success_status,
    };

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
    use crate::git_factor::proptests::{
        OverflowSplitCountFs, ScriptedRunner, TestEnv, TestIo, success_status,
    };

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
    use crate::git_factor::proptests::{ScriptedRunner, TestEnv, TestFs, TestIo, success_status};

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
    use crate::git_factor::proptests::{NonUtf8Fs, TestEnv, TestIo, TestRunner};

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

#[test]
fn cmd_start_forwards_successful_gate_streams() {
    use crate::git_factor::tests::start_contracts::{DirectStart, GateCase};
    use alloc::collections::BTreeMap;

    let selected = NonEmpty::new(CommitSha::new("a".repeat(SHA_LEN)).or_abort("admitted SHA"));
    let fixture = DirectStart::new(
        &selected,
        false,
        GateCase::Pass,
        "gate stdout",
        "gate stderr",
    );
    let ctx = fixture.ctx();

    let result = cmd_start_with_resolved_in(&ctx, &fixture.exec, &fixture.state, &fixture.selected);

    assert_eq!(result.map_err(|err| err.to_string()), Ok(EXIT_OK));
    assert_eq!(
        fixture.io.stdout(),
        concat!(
            "gate stdoutFACTOR: Split session started for abcdef0.\n",
            "ORIGINAL MESSAGE: subject\n",
            "UNSTAGED:\n",
            "\n",
            "NEXT: Stage changes for the first atomic commit, then run:\n",
            "  git factor --continue --message \"type: description\"\n",
            "\n",
            "Run git factor -h for command help or git-factor --help for the ",
            "full workflow guide.\n",
            "\n",
            "HINTS:\n",
            "  - Find the ONE smallest addition nothing depends on\n",
            "  - Target 15-30 lines (50 max)\n",
            "  - Message: single concrete action, no \"and\"/\"or\"\n",
            "  - Verify: git log --oneline | wc -l\n",
            "  - NEVER use git commit. ONLY use git factor --continue.\n",
            "  RECOVERY: git factor --abort\n",
        )
    );
    assert_eq!(fixture.io.stderr(), "gate stderr");
    assert_eq!(fixture.observed_calls(), fixture.expected_calls);
    assert_eq!(fixture.remaining_keys(), Vec::<String>::new());
    // Compare net contents/layout in this tempdir, excluding factor state; Git has its own ledger.
    assert_eq!(fixture.direct_files_after(), fixture.direct_files_before);
    let expected_journal = [
        ("commits", format!("{}\n", "a".repeat(SHA_LEN))),
        ("current_index", "0\n".to_owned()),
        ("exec", "true\n".to_owned()),
        ("expected_tree", TREE_EXPECTED_NL.to_owned()),
        ("is_root", "false\n".to_owned()),
        ("phase", "splitting\n".to_owned()),
        ("requires_rebase", "false\n".to_owned()),
        ("split_count", "0\n".to_owned()),
        ("start_head", format!("{}\n", "a".repeat(SHA_LEN))),
        ("started_rebase", "false\n".to_owned()),
    ]
    .into_iter()
    .map(|(name, text)| (OsString::from(name), text))
    .collect::<BTreeMap<_, _>>();
    assert_eq!(fixture.observed_journal(), Some(expected_journal));
}

#[test]
fn cmd_start_forwards_failed_gate_streams() {
    use crate::git_factor::tests::start_contracts::{DirectStart, GateCase};

    let selected = NonEmpty::new(CommitSha::new("a".repeat(SHA_LEN)).or_abort("admitted SHA"));
    let fixture = DirectStart::new(
        &selected,
        false,
        GateCase::Fail,
        "gate stdout",
        "gate stderr",
    );
    let ctx = fixture.ctx();

    let result = cmd_start_with_resolved_in(&ctx, &fixture.exec, &fixture.state, &fixture.selected);

    assert_eq!(
        result.map_err(|err| err.to_string()),
        Err("exec gate failed: true (exit code 7)".to_owned())
    );
    assert_eq!(
        fixture.io.stdout(),
        concat!(
            "gate stdoutFACTOR: Start gate failed.\n",
            "EXEC: true\n",
            "CODE: 7\n",
            "\n",
            "NEXT: Fix the current commit, amend it, then rerun git factor.\n",
        )
    );
    assert_eq!(fixture.io.stderr(), "gate stderr");
    assert_eq!(fixture.observed_calls(), fixture.expected_calls);
    assert_eq!(fixture.remaining_keys(), Vec::<String>::new());
    // Compare net contents/layout in this tempdir, excluding factor state; Git has its own ledger.
    assert_eq!(fixture.direct_files_after(), fixture.direct_files_before);
    assert_eq!(fixture.observed_journal(), None);
}

#[test]
fn cmd_start_preserves_success_after_the_last_output_write() {
    use crate::git_factor::tests::start_contracts::{DirectStart, GateCase};
    use alloc::collections::BTreeMap;
    use core::num::NonZeroUsize;

    let selected = NonEmpty::new(CommitSha::new("a".repeat(SHA_LEN)).or_abort("admitted SHA"));
    let fixture = DirectStart::new(
        &selected,
        false,
        GateCase::PassOutputFailure(NonZeroUsize::new(19).or_abort("positive past-end boundary")),
        "gate stdout",
        "gate stderr",
    );
    let ctx = fixture.ctx();

    let result = cmd_start_with_resolved_in(&ctx, &fixture.exec, &fixture.state, &fixture.selected);

    assert_eq!(result.map_err(|err| err.to_string()), Ok(EXIT_OK));
    assert_eq!(
        fixture.io.stdout(),
        concat!(
            "gate stdoutFACTOR: Split session started for abcdef0.\n",
            "ORIGINAL MESSAGE: subject\n",
            "UNSTAGED:\n",
            "\n",
            "NEXT: Stage changes for the first atomic commit, then run:\n",
            "  git factor --continue --message \"type: description\"\n",
            "\n",
            "Run git factor -h for command help or git-factor --help for the ",
            "full workflow guide.\n",
            "\n",
            "HINTS:\n",
            "  - Find the ONE smallest addition nothing depends on\n",
            "  - Target 15-30 lines (50 max)\n",
            "  - Message: single concrete action, no \"and\"/\"or\"\n",
            "  - Verify: git log --oneline | wc -l\n",
            "  - NEVER use git commit. ONLY use git factor --continue.\n",
            "  RECOVERY: git factor --abort\n",
        )
    );
    assert_eq!(fixture.io.stderr(), "gate stderr");
    assert_eq!(fixture.observed_calls(), fixture.expected_calls);
    assert_eq!(fixture.remaining_keys(), Vec::<String>::new());
    // Compare net contents/layout in this tempdir, excluding factor state; Git has its own ledger.
    assert_eq!(fixture.direct_files_after(), fixture.direct_files_before);
    let expected_journal = [
        ("commits", format!("{}\n", "a".repeat(SHA_LEN))),
        ("current_index", "0\n".to_owned()),
        ("exec", "true\n".to_owned()),
        ("expected_tree", TREE_EXPECTED_NL.to_owned()),
        ("is_root", "false\n".to_owned()),
        ("phase", "splitting\n".to_owned()),
        ("requires_rebase", "false\n".to_owned()),
        ("split_count", "0\n".to_owned()),
        ("start_head", format!("{}\n", "a".repeat(SHA_LEN))),
        ("started_rebase", "false\n".to_owned()),
    ]
    .into_iter()
    .map(|(name, text)| (OsString::from(name), text))
    .collect::<BTreeMap<_, _>>();
    assert_eq!(fixture.observed_journal(), Some(expected_journal));
}

#[test]
fn cmd_start_preserves_failure_after_the_last_output_write() {
    use crate::git_factor::tests::start_contracts::{DirectStart, GateCase};
    use core::num::NonZeroUsize;

    let selected = NonEmpty::new(CommitSha::new("a".repeat(SHA_LEN)).or_abort("admitted SHA"));
    let fixture = DirectStart::new(
        &selected,
        false,
        GateCase::FailOutputFailure(NonZeroUsize::new(8).or_abort("positive past-end boundary")),
        "gate stdout",
        "gate stderr",
    );
    let ctx = fixture.ctx();

    let result = cmd_start_with_resolved_in(&ctx, &fixture.exec, &fixture.state, &fixture.selected);

    assert_eq!(
        result.map_err(|err| err.to_string()),
        Err("exec gate failed: true (exit code 7)".to_owned())
    );
    assert_eq!(
        fixture.io.stdout(),
        concat!(
            "gate stdoutFACTOR: Start gate failed.\n",
            "EXEC: true\n",
            "CODE: 7\n",
            "\n",
            "NEXT: Fix the current commit, amend it, then rerun git factor.\n",
        )
    );
    assert_eq!(fixture.io.stderr(), "gate stderr");
    assert_eq!(fixture.observed_calls(), fixture.expected_calls);
    assert_eq!(fixture.remaining_keys(), Vec::<String>::new());
    // Compare net contents/layout in this tempdir, excluding factor state; Git has its own ledger.
    assert_eq!(fixture.direct_files_after(), fixture.direct_files_before);
    assert_eq!(fixture.observed_journal(), None);
}

#[test]
fn cmd_start_characterizes_later_ancestor_query_failure() {
    use crate::git_factor::tests::start_contracts::query;
    use crate::git_factor::tests::start_contracts::query::{
        QueryCase, QueryPosition, QueryReply, QueryTarget,
    };

    let selected = NonEmpty::from_vec(vec![
        CommitSha::new("a".repeat(SHA_LEN)).or_abort("first SHA"),
        CommitSha::new("b".repeat(SHA_LEN)).or_abort("second SHA"),
    ])
    .or_abort("nonempty span");
    let case = QueryCase::Failure {
        target: QueryTarget::Ancestor(QueryPosition::Last),
        reply: QueryReply::Io,
    };
    let fixture = query::direct_start(&selected, false, &case, "gate out", "gate err");
    let ctx = fixture.ctx();

    let result = cmd_start_with_resolved_in(&ctx, &fixture.exec, &fixture.state, &fixture.selected);

    assert_eq!(
        result.map_err(|err| err.to_string()),
        Err(concat!(
            "git command failed: git command failed: ",
            "git merge-base: selected query IO failure"
        )
        .to_owned())
    );
    assert_eq!(fixture.io.stdout(), "");
    assert_eq!(fixture.io.stderr(), "");
    assert_eq!(fixture.observed_journal(), None);
    assert_eq!(fixture.observed_calls(), fixture.expected_calls);
    assert_eq!(fixture.remaining_keys(), Vec::<String>::new());
    // Only net contents/layout inside this tempdir, excluding factor state.
    assert_eq!(fixture.direct_files_after(), fixture.direct_files_before);
}

#[test]
fn cmd_start_preserves_partial_state_when_tree_query_fails() {
    use crate::git_factor::tests::start_contracts::query;
    use crate::git_factor::tests::start_contracts::query::{QueryCase, QueryReply, QueryTarget};
    use alloc::collections::BTreeMap;

    let selected = NonEmpty::new(CommitSha::new("a".repeat(SHA_LEN)).or_abort("admitted SHA"));
    let case = QueryCase::Failure {
        target: QueryTarget::Tree,
        reply: QueryReply::Rejected,
    };
    let fixture = query::direct_start(&selected, false, &case, "gate out", "gate err");
    let ctx = fixture.ctx();

    let result = cmd_start_with_resolved_in(&ctx, &fixture.exec, &fixture.state, &fixture.selected);

    assert_eq!(
        result.map_err(|err| err.to_string()),
        Err("git command failed: selected query refused".to_owned())
    );
    assert_eq!(fixture.io.stdout(), "gate out");
    assert_eq!(fixture.io.stderr(), "gate err");
    let expected_journal = [
        ("commits", format!("{}\n", "a".repeat(SHA_LEN))),
        ("current_index", "0\n".to_owned()),
        ("exec", "true\n".to_owned()),
        ("is_root", "false\n".to_owned()),
        ("phase", "splitting\n".to_owned()),
        ("requires_rebase", "false\n".to_owned()),
        ("split_count", "0\n".to_owned()),
        ("start_head", format!("{}\n", "a".repeat(SHA_LEN))),
        ("started_rebase", "false\n".to_owned()),
    ]
    .into_iter()
    .map(|(name, text)| (OsString::from(name), text))
    .collect::<BTreeMap<_, _>>();
    assert_eq!(fixture.observed_journal(), Some(expected_journal));
    assert_eq!(fixture.observed_calls(), fixture.expected_calls);
    assert_eq!(fixture.remaining_keys(), Vec::<String>::new());
    // Only net contents/layout inside this tempdir, excluding factor state.
    assert_eq!(fixture.direct_files_after(), fixture.direct_files_before);
}

#[test]
fn cmd_start_preserves_opened_state_when_diff_query_fails() {
    use crate::git_factor::tests::start_contracts::query;
    use crate::git_factor::tests::start_contracts::query::{QueryCase, QueryReply, QueryTarget};
    use alloc::collections::BTreeMap;

    let selected = NonEmpty::new(CommitSha::new("a".repeat(SHA_LEN)).or_abort("admitted SHA"));
    let case = QueryCase::Failure {
        target: QueryTarget::Diff,
        reply: QueryReply::Rejected,
    };
    let fixture = query::direct_start(&selected, false, &case, "gate out", "gate err");
    let ctx = fixture.ctx();

    let result = cmd_start_with_resolved_in(&ctx, &fixture.exec, &fixture.state, &fixture.selected);

    assert_eq!(
        result.map_err(|err| err.to_string()),
        Err("git command failed: selected query refused".to_owned())
    );
    assert_eq!(fixture.io.stdout(), "gate out");
    assert_eq!(fixture.io.stderr(), "gate err");
    let expected_journal = [
        ("commits", format!("{}\n", "a".repeat(SHA_LEN))),
        ("current_index", "0\n".to_owned()),
        ("exec", "true\n".to_owned()),
        ("is_root", "false\n".to_owned()),
        ("phase", "splitting\n".to_owned()),
        ("requires_rebase", "false\n".to_owned()),
        ("split_count", "0\n".to_owned()),
        ("start_head", format!("{}\n", "a".repeat(SHA_LEN))),
        ("started_rebase", "false\n".to_owned()),
        ("expected_tree", TREE_EXPECTED_NL.to_owned()),
    ]
    .into_iter()
    .map(|(name, text)| (OsString::from(name), text))
    .collect::<BTreeMap<_, _>>();
    assert_eq!(fixture.observed_journal(), Some(expected_journal));
    assert_eq!(fixture.observed_calls(), fixture.expected_calls);
    assert_eq!(fixture.remaining_keys(), Vec::<String>::new());
    // Only net contents/layout inside this tempdir, excluding factor state.
    assert_eq!(fixture.direct_files_after(), fixture.direct_files_before);
}

#[test]
fn cmd_start_refuses_later_merge_object_query_before_mutation() {
    use crate::git_factor::tests::start_contracts::query;
    use crate::git_factor::tests::start_contracts::query::{
        QueryCase, QueryPosition, QueryReply, QueryTarget,
    };

    let selected = NonEmpty::from_vec(vec![
        CommitSha::new("a".repeat(SHA_LEN)).or_abort("first SHA"),
        CommitSha::new("b".repeat(SHA_LEN)).or_abort("second SHA"),
    ])
    .or_abort("nonempty span");
    let case = QueryCase::Failure {
        target: QueryTarget::MergeParent(QueryPosition::Last),
        reply: QueryReply::Io,
    };
    let fixture = query::selected_start(&selected, &case);
    let ctx = fixture.ctx();

    let refs =
        NonEmpty::new(NonEmptyString::try_from("base..tip".to_owned()).or_abort("explicit range"));
    let result = cmd_start_in(&ctx, &fixture.exec, &refs);

    assert_eq!(
        result.map_err(|err| err.to_string()),
        Err("git command failed: git cat-file: selected query IO failure".to_owned())
    );
    assert_eq!(fixture.io.stdout(), "");
    assert_eq!(fixture.io.stderr(), "");
    assert_eq!(fixture.observed_journal(), None);
    assert_eq!(fixture.observed_calls(), fixture.expected_calls);
    assert_eq!(fixture.remaining_keys(), Vec::<String>::new());
    // Only net contents/layout inside this tempdir, excluding factor state.
    assert_eq!(fixture.direct_files_after(), fixture.direct_files_before);
}

#[test]
fn cmd_start_preserves_opened_state_when_hint_query_fails() {
    use crate::git_factor::tests::start_contracts::query;
    use crate::git_factor::tests::start_contracts::query::{QueryCase, QueryReply, QueryTarget};
    use alloc::collections::BTreeMap;

    let selected = NonEmpty::new(CommitSha::new("a".repeat(SHA_LEN)).or_abort("admitted SHA"));
    let case = QueryCase::Failure {
        target: QueryTarget::TopLevel,
        reply: QueryReply::Rejected,
    };
    let fixture = query::direct_start(&selected, false, &case, "gate out", "gate err");
    let ctx = fixture.ctx();

    let result = cmd_start_with_resolved_in(&ctx, &fixture.exec, &fixture.state, &fixture.selected);

    assert_eq!(
        result.map_err(|err| err.to_string()),
        Err("git command failed: selected query refused".to_owned())
    );
    assert_eq!(
        fixture.io.stdout(),
        concat!(
            "gate outFACTOR: Split session started for abcdef0.\n",
            "ORIGINAL MESSAGE: subject\nUNSTAGED:\n\n",
            "NEXT: Stage changes for the first atomic commit, then run:\n",
            "  git factor --continue --message \"type: description\"\n\n",
            "Run git factor -h for command help or git-factor --help for the ",
            "full workflow guide.\n\n",
        )
    );
    assert_eq!(fixture.io.stderr(), "gate err");
    let expected_journal = [
        ("commits", format!("{}\n", "a".repeat(SHA_LEN))),
        ("current_index", "0\n".to_owned()),
        ("exec", "true\n".to_owned()),
        ("is_root", "false\n".to_owned()),
        ("phase", "splitting\n".to_owned()),
        ("requires_rebase", "false\n".to_owned()),
        ("split_count", "0\n".to_owned()),
        ("start_head", format!("{}\n", "a".repeat(SHA_LEN))),
        ("started_rebase", "false\n".to_owned()),
        ("expected_tree", TREE_EXPECTED_NL.to_owned()),
    ]
    .into_iter()
    .map(|(name, text)| (OsString::from(name), text))
    .collect::<BTreeMap<_, _>>();
    assert_eq!(fixture.observed_journal(), Some(expected_journal));
    assert_eq!(fixture.observed_calls(), fixture.expected_calls);
    assert_eq!(fixture.remaining_keys(), Vec::<String>::new());
    // Only net contents/layout inside this tempdir, excluding factor state.
    assert_eq!(fixture.direct_files_after(), fixture.direct_files_before);
}

#[test]
fn cmd_start_refuses_nonroot_parent_query_rejection_before_mutation() {
    use crate::git_factor::tests::start_contracts::query;
    use crate::git_factor::tests::start_contracts::query::{QueryCase, QueryReply, QueryTarget};

    let selected = NonEmpty::new(CommitSha::new("a".repeat(SHA_LEN)).or_abort("admitted SHA"));
    let case = QueryCase::Failure {
        target: QueryTarget::Parent,
        reply: QueryReply::Rejected,
    };
    let fixture = query::direct_start(&selected, false, &case, "gate stdout", "gate stderr");
    let ctx = fixture.ctx();

    let result = cmd_start_with_resolved_in(&ctx, &fixture.exec, &fixture.state, &fixture.selected);

    assert_eq!(
        result.map_err(|err| err.to_string()),
        Err("git command failed: selected query refused".to_owned())
    );
    assert_eq!(fixture.io.stdout(), "");
    assert_eq!(fixture.io.stderr(), "");
    assert_eq!(fixture.observed_journal(), None);
    assert_eq!(fixture.observed_calls(), fixture.expected_calls);
    assert_eq!(fixture.remaining_keys(), Vec::<String>::new());
    // Compare net contents/layout in this tempdir, excluding factor state; Git has its own ledger.
    assert_eq!(fixture.direct_files_after(), fixture.direct_files_before);
}

#[test]
fn cmd_start_finished_replay_removes_populated_begin_journal() {
    use crate::git_factor::tests::start_contracts::replay;

    let selected = NonEmpty::new(CommitSha::new("a".repeat(SHA_LEN)).or_abort("admitted SHA"));
    let fixture = replay::direct_start(&selected, false, &replay::ReplayCase::FinishedWithoutPause);
    let ctx = fixture.ctx();

    let result = cmd_start_with_resolved_in(&ctx, &fixture.exec, &fixture.state, &fixture.selected);

    assert_eq!(
        result.map_err(|err| err.to_string()),
        Err(
            "git command failed: git rebase finished without pausing at the factor session break"
                .to_owned()
        )
    );
    assert_eq!(fixture.io.stdout(), "");
    assert_eq!(fixture.io.stderr(), "");
    assert_eq!(fixture.observed_journal(), None);
    assert_eq!(fixture.observed_calls(), fixture.expected_calls);
    assert_eq!(fixture.remaining_keys(), Vec::<String>::new());
    // Only direct effects outside the owned state and scripted native flag are observed here.
    assert_eq!(fixture.direct_files_after(), fixture.direct_files_before);
}

#[test]
fn cmd_start_failed_replay_removes_populated_begin_journal() {
    use crate::git_factor::tests::start_contracts::replay;

    let selected = NonEmpty::new(CommitSha::new("a".repeat(SHA_LEN)).or_abort("admitted SHA"));
    let fixture = replay::direct_start(&selected, false, &replay::ReplayCase::FailedWithoutPause);
    let ctx = fixture.ctx();

    let result = cmd_start_with_resolved_in(&ctx, &fixture.exec, &fixture.state, &fixture.selected);

    assert_eq!(
        result.map_err(|err| err.to_string()),
        Err("git command failed: git rebase failed (exit 1)".to_owned())
    );
    assert_eq!(fixture.io.stdout(), "");
    assert_eq!(fixture.io.stderr(), "");
    assert_eq!(fixture.observed_journal(), None);
    assert_eq!(fixture.observed_calls(), fixture.expected_calls);
    assert_eq!(fixture.remaining_keys(), Vec::<String>::new());
    // Only direct effects outside the owned state and scripted native flag are observed here.
    assert_eq!(fixture.direct_files_after(), fixture.direct_files_before);
}

#[test]
fn cmd_start_preflight_recovery_wait_has_no_begin_journal() {
    use crate::git_factor::tests::start_contracts::replay;

    let selected = NonEmpty::new(CommitSha::new("a".repeat(SHA_LEN)).or_abort("admitted SHA"));
    let fixture = replay::direct_start(&selected, false, &replay::ReplayCase::Waiting);
    let ctx = fixture.ctx();

    let result = cmd_start_with_resolved_in(&ctx, &fixture.exec, &fixture.state, &fixture.selected);

    assert_eq!(result.map_err(|err| err.to_string()), Ok(EXIT_TEMPFAIL));
    assert_eq!(fixture.io.stdout(), "");
    assert_eq!(fixture.io.stderr(), "");
    assert_eq!(fixture.observed_journal(), None);
    assert_eq!(fixture.observed_calls(), fixture.expected_calls);
    assert_eq!(fixture.remaining_keys(), Vec::<String>::new());
    // Only direct effects outside the owned state and scripted native flag are observed here.
    assert_eq!(fixture.direct_files_after(), fixture.direct_files_before);
}

#[test]
fn cmd_start_opens_paused_replay_with_literal_journal() {
    use crate::git_factor::tests::start_contracts::replay;
    use alloc::collections::BTreeMap;

    let selected = NonEmpty::new(CommitSha::new("a".repeat(SHA_LEN)).or_abort("admitted SHA"));
    let fixture = replay::direct_start(&selected, false, &replay::ReplayCase::Paused);
    let ctx = fixture.ctx();

    let result = cmd_start_with_resolved_in(&ctx, &fixture.exec, &fixture.state, &fixture.selected);

    assert_eq!(result.map_err(|err| err.to_string()), Ok(EXIT_OK));
    assert_eq!(
        fixture.io.stdout(),
        concat!(
            "FACTOR: Split session started for abcdef0.\n",
            "ORIGINAL MESSAGE: subject\n",
            "UNSTAGED:\n",
            "\n",
            "NEXT: Stage changes for the first atomic commit, then run:\n",
            "  git factor --continue --message \"type: description\"\n",
            "\n",
            "Run git factor -h for command help or git-factor --help for the ",
            "full workflow guide.\n",
            "\n",
            "HINTS:\n",
            "  - Find the ONE smallest addition nothing depends on\n",
            "  - Target 15-30 lines (50 max)\n",
            "  - Message: single concrete action, no \"and\"/\"or\"\n",
            "  - Verify: git log --oneline | wc -l\n",
            "  - NEVER use git commit. ONLY use git factor --continue.\n",
            "  RECOVERY: git factor --abort\n",
        )
    );
    assert_eq!(fixture.io.stderr(), "");
    assert_eq!(fixture.observed_calls(), fixture.expected_calls);
    assert_eq!(fixture.remaining_keys(), Vec::<String>::new());
    // Compare net contents/layout in this tempdir, excluding factor state; Git has its own ledger.
    assert_eq!(fixture.direct_files_after(), fixture.direct_files_before);
    let expected_journal = [
        ("commits", format!("{}\n", "a".repeat(SHA_LEN))),
        ("current_index", "0\n".to_owned()),
        ("exec", "true\n".to_owned()),
        ("expected_tree", TREE_EXPECTED_NL.to_owned()),
        ("is_root", "false\n".to_owned()),
        ("phase", "splitting\n".to_owned()),
        ("requires_rebase", "true\n".to_owned()),
        ("split_count", "0\n".to_owned()),
        ("start_head", format!("{}\n", "0".repeat(SHA_LEN))),
        ("started_rebase", "true\n".to_owned()),
    ]
    .into_iter()
    .map(|(name, text)| (OsString::from(name), text))
    .collect::<BTreeMap<_, _>>();
    assert_eq!(fixture.observed_journal(), Some(expected_journal));
}

#[test]
fn cmd_start_refuses_invalid_replay_commits_with_literal_pending_journal() {
    use crate::git_factor::tests::start_contracts::replay;
    use alloc::collections::BTreeMap;

    let selected = NonEmpty::new(CommitSha::new("a".repeat(SHA_LEN)).or_abort("admitted SHA"));
    let fixture = replay::direct_start(&selected, false, &replay::ReplayCase::InvalidCommits);
    let ctx = fixture.ctx();

    let result = cmd_start_with_resolved_in(&ctx, &fixture.exec, &fixture.state, &fixture.selected);

    assert_eq!(
        result.map_err(|err| err.to_string()),
        Err("invalid commit: bad".to_owned())
    );
    assert_eq!(fixture.io.stdout(), "");
    assert_eq!(fixture.io.stderr(), "");
    assert_eq!(fixture.observed_calls(), fixture.expected_calls);
    assert_eq!(fixture.remaining_keys(), Vec::<String>::new());
    // Compare net contents/layout in this tempdir, excluding factor state; Git has its own ledger.
    assert_eq!(fixture.direct_files_after(), fixture.direct_files_before);
    let expected_journal = [
        ("commits", "bad\n".to_owned()),
        ("current_index", "0\n".to_owned()),
        ("exec", "true\n".to_owned()),
        ("expected_tree", TREE_EXPECTED_NL.to_owned()),
        ("is_root", "false\n".to_owned()),
        ("phase", "pending_start\n".to_owned()),
        ("requires_rebase", "true\n".to_owned()),
        ("split_count", "0\n".to_owned()),
        ("start_head", format!("{}\n", "0".repeat(SHA_LEN))),
        ("started_rebase", "true\n".to_owned()),
    ]
    .into_iter()
    .map(|(name, text)| (OsString::from(name), text))
    .collect::<BTreeMap<_, _>>();
    assert_eq!(fixture.observed_journal(), Some(expected_journal));
}

#[test]
fn cmd_start_refuses_nonpending_replay_phase_with_literal_journal() {
    use crate::git_factor::tests::start_contracts::replay;
    use alloc::collections::BTreeMap;

    let selected = NonEmpty::new(CommitSha::new("a".repeat(SHA_LEN)).or_abort("admitted SHA"));
    let fixture = replay::direct_start(&selected, false, &replay::ReplayCase::InvalidPhase);
    let ctx = fixture.ctx();

    let result = cmd_start_with_resolved_in(&ctx, &fixture.exec, &fixture.state, &fixture.selected);

    assert_eq!(
        result.map_err(|err| err.to_string()),
        Err("git command failed: factor session is not waiting to begin splitting".to_owned())
    );
    assert_eq!(fixture.io.stdout(), "");
    assert_eq!(fixture.io.stderr(), "");
    assert_eq!(fixture.observed_calls(), fixture.expected_calls);
    assert_eq!(fixture.remaining_keys(), Vec::<String>::new());
    // Compare net contents/layout in this tempdir, excluding factor state; Git has its own ledger.
    assert_eq!(fixture.direct_files_after(), fixture.direct_files_before);
    let expected_journal = [
        ("commits", format!("{}\n", "a".repeat(SHA_LEN))),
        ("current_index", "0\n".to_owned()),
        ("exec", "true\n".to_owned()),
        ("expected_tree", TREE_EXPECTED_NL.to_owned()),
        ("is_root", "false\n".to_owned()),
        ("phase", "splitting\n".to_owned()),
        ("requires_rebase", "true\n".to_owned()),
        ("split_count", "0\n".to_owned()),
        ("start_head", format!("{}\n", "0".repeat(SHA_LEN))),
        ("started_rebase", "true\n".to_owned()),
    ]
    .into_iter()
    .map(|(name, text)| (OsString::from(name), text))
    .collect::<BTreeMap<_, _>>();
    assert_eq!(fixture.observed_journal(), Some(expected_journal));
}

#[test]
fn cmd_start_first_replay_banner_write_failure_retains_split_journal() {
    use crate::git_factor::tests::start_contracts::replay;
    use alloc::collections::BTreeMap;
    use core::num::NonZeroUsize;

    let selected = NonEmpty::new(CommitSha::new("a".repeat(SHA_LEN)).or_abort("admitted SHA"));
    let fixture = replay::direct_start(
        &selected,
        false,
        &replay::ReplayCase::BannerFailure(NonZeroUsize::new(1).or_abort("first write")),
    );
    let ctx = fixture.ctx();

    let result = cmd_start_with_resolved_in(&ctx, &fixture.exec, &fixture.state, &fixture.selected);

    assert_eq!(
        result.map_err(|err| err.to_string()),
        Err("failed to write output: selected output write failed".to_owned())
    );
    assert_eq!(fixture.io.stdout(), "");
    assert_eq!(fixture.io.stderr(), "");
    assert_eq!(fixture.observed_calls(), fixture.expected_calls);
    assert_eq!(fixture.remaining_keys(), Vec::<String>::new());
    // Compare net contents/layout in this tempdir, excluding factor state; Git has its own ledger.
    assert_eq!(fixture.direct_files_after(), fixture.direct_files_before);
    let expected_journal = [
        ("commits", format!("{}\n", "a".repeat(SHA_LEN))),
        ("current_index", "0\n".to_owned()),
        ("exec", "true\n".to_owned()),
        ("expected_tree", TREE_EXPECTED_NL.to_owned()),
        ("is_root", "false\n".to_owned()),
        ("phase", "splitting\n".to_owned()),
        ("requires_rebase", "true\n".to_owned()),
        ("split_count", "0\n".to_owned()),
        ("start_head", format!("{}\n", "0".repeat(SHA_LEN))),
        ("started_rebase", "true\n".to_owned()),
    ]
    .into_iter()
    .map(|(name, text)| (OsString::from(name), text))
    .collect::<BTreeMap<_, _>>();
    assert_eq!(fixture.observed_journal(), Some(expected_journal));
}

#[test]
fn cmd_start_last_replay_banner_write_failure_retains_split_journal() {
    use crate::git_factor::tests::start_contracts::replay;
    use alloc::collections::BTreeMap;
    use core::num::NonZeroUsize;

    let selected = NonEmpty::new(CommitSha::new("a".repeat(SHA_LEN)).or_abort("admitted SHA"));
    let fixture = replay::direct_start(
        &selected,
        false,
        &replay::ReplayCase::BannerFailure(NonZeroUsize::new(16).or_abort("last write")),
    );
    let ctx = fixture.ctx();

    let result = cmd_start_with_resolved_in(&ctx, &fixture.exec, &fixture.state, &fixture.selected);

    assert_eq!(
        result.map_err(|err| err.to_string()),
        Err("failed to write output: selected output write failed".to_owned())
    );
    assert_eq!(
        fixture.io.stdout(),
        concat!(
            "FACTOR: Split session started for abcdef0.\n",
            "ORIGINAL MESSAGE: subject\n",
            "UNSTAGED:\n",
            "\n",
            "NEXT: Stage changes for the first atomic commit, then run:\n",
            "  git factor --continue --message \"type: description\"\n",
            "\n",
            "Run git factor -h for command help or git-factor --help for the ",
            "full workflow guide.\n",
            "\n",
            "HINTS:\n",
            "  - Find the ONE smallest addition nothing depends on\n",
            "  - Target 15-30 lines (50 max)\n",
            "  - Message: single concrete action, no \"and\"/\"or\"\n",
            "  - Verify: git log --oneline | wc -l\n",
            "  - NEVER use git commit. ONLY use git factor --continue.\n",
        )
    );
    assert_eq!(fixture.io.stderr(), "");
    assert_eq!(fixture.observed_calls(), fixture.expected_calls);
    assert_eq!(fixture.remaining_keys(), Vec::<String>::new());
    // Compare net contents/layout in this tempdir, excluding factor state; Git has its own ledger.
    assert_eq!(fixture.direct_files_after(), fixture.direct_files_before);
    let expected_journal = [
        ("commits", format!("{}\n", "a".repeat(SHA_LEN))),
        ("current_index", "0\n".to_owned()),
        ("exec", "true\n".to_owned()),
        ("expected_tree", TREE_EXPECTED_NL.to_owned()),
        ("is_root", "false\n".to_owned()),
        ("phase", "splitting\n".to_owned()),
        ("requires_rebase", "true\n".to_owned()),
        ("split_count", "0\n".to_owned()),
        ("start_head", format!("{}\n", "0".repeat(SHA_LEN))),
        ("started_rebase", "true\n".to_owned()),
    ]
    .into_iter()
    .map(|(name, text)| (OsString::from(name), text))
    .collect::<BTreeMap<_, _>>();
    assert_eq!(fixture.observed_journal(), Some(expected_journal));
}

#[test]
fn cmd_start_past_last_replay_banner_write_finishes() {
    use crate::git_factor::tests::start_contracts::replay;
    use alloc::collections::BTreeMap;
    use core::num::NonZeroUsize;

    let selected = NonEmpty::new(CommitSha::new("a".repeat(SHA_LEN)).or_abort("admitted SHA"));
    let fixture = replay::direct_start(
        &selected,
        false,
        &replay::ReplayCase::BannerFailure(NonZeroUsize::new(17).or_abort("past last write")),
    );
    let ctx = fixture.ctx();

    let result = cmd_start_with_resolved_in(&ctx, &fixture.exec, &fixture.state, &fixture.selected);

    assert_eq!(result.map_err(|err| err.to_string()), Ok(EXIT_OK));
    assert_eq!(
        fixture.io.stdout(),
        concat!(
            "FACTOR: Split session started for abcdef0.\n",
            "ORIGINAL MESSAGE: subject\n",
            "UNSTAGED:\n",
            "\n",
            "NEXT: Stage changes for the first atomic commit, then run:\n",
            "  git factor --continue --message \"type: description\"\n",
            "\n",
            "Run git factor -h for command help or git-factor --help for the ",
            "full workflow guide.\n",
            "\n",
            "HINTS:\n",
            "  - Find the ONE smallest addition nothing depends on\n",
            "  - Target 15-30 lines (50 max)\n",
            "  - Message: single concrete action, no \"and\"/\"or\"\n",
            "  - Verify: git log --oneline | wc -l\n",
            "  - NEVER use git commit. ONLY use git factor --continue.\n",
            "  RECOVERY: git factor --abort\n",
        )
    );
    assert_eq!(fixture.io.stderr(), "");
    assert_eq!(fixture.observed_calls(), fixture.expected_calls);
    assert_eq!(fixture.remaining_keys(), Vec::<String>::new());
    // Compare net contents/layout in this tempdir, excluding factor state; Git has its own ledger.
    assert_eq!(fixture.direct_files_after(), fixture.direct_files_before);
    let expected_journal = [
        ("commits", format!("{}\n", "a".repeat(SHA_LEN))),
        ("current_index", "0\n".to_owned()),
        ("exec", "true\n".to_owned()),
        ("expected_tree", TREE_EXPECTED_NL.to_owned()),
        ("is_root", "false\n".to_owned()),
        ("phase", "splitting\n".to_owned()),
        ("requires_rebase", "true\n".to_owned()),
        ("split_count", "0\n".to_owned()),
        ("start_head", format!("{}\n", "0".repeat(SHA_LEN))),
        ("started_rebase", "true\n".to_owned()),
    ]
    .into_iter()
    .map(|(name, text)| (OsString::from(name), text))
    .collect::<BTreeMap<_, _>>();
    assert_eq!(fixture.observed_journal(), Some(expected_journal));
}

#[test]
fn cmd_start_opens_root_range_replay_with_literal_journal() {
    use crate::git_factor::tests::start_contracts::replay;
    use alloc::collections::BTreeMap;

    let selected = NonEmpty {
        head: CommitSha::new("a".repeat(SHA_LEN)).or_abort("admitted first SHA"),
        tail: vec![CommitSha::new("b".repeat(SHA_LEN)).or_abort("admitted tip SHA")],
    };
    let fixture = replay::direct_start(&selected, true, &replay::ReplayCase::Paused);
    let ctx = fixture.ctx();

    let result = cmd_start_with_resolved_in(&ctx, &fixture.exec, &fixture.state, &fixture.selected);

    assert_eq!(result.map_err(|err| err.to_string()), Ok(EXIT_OK));
    assert_eq!(
        fixture.io.stdout(),
        concat!(
            "FACTOR: Split session started for 2 commits (tip: abcdef0).\n",
            "ORIGINAL MESSAGE: subject\n",
            "UNSTAGED:\n",
            "\n",
            "NEXT: Stage changes for the first atomic commit, then run:\n",
            "  git factor --continue --message \"type: description\"\n",
            "\n",
            "Run git factor -h for command help or git-factor --help for the ",
            "full workflow guide.\n",
            "\n",
            "HINTS:\n",
            "  - Find the ONE smallest addition nothing depends on\n",
            "  - Target 15-30 lines (50 max)\n",
            "  - Message: single concrete action, no \"and\"/\"or\"\n",
            "  - Verify: git log --oneline | wc -l\n",
            "  - NEVER use git commit. ONLY use git factor --continue.\n",
            "  RECOVERY: git factor --abort\n",
        )
    );
    assert_eq!(fixture.io.stderr(), "");
    assert_eq!(fixture.observed_calls(), fixture.expected_calls);
    assert_eq!(fixture.remaining_keys(), Vec::<String>::new());
    // Compare net contents/layout in this tempdir, excluding factor state; Git has its own ledger.
    assert_eq!(fixture.direct_files_after(), fixture.direct_files_before);
    let expected_journal = [
        (
            "commits",
            format!("{}\n{}\n", "a".repeat(SHA_LEN), "b".repeat(SHA_LEN)),
        ),
        ("current_index", "1\n".to_owned()),
        ("exec", "true\n".to_owned()),
        ("expected_tree", TREE_EXPECTED_NL.to_owned()),
        ("is_root", "true\n".to_owned()),
        ("phase", "splitting\n".to_owned()),
        ("requires_rebase", "true\n".to_owned()),
        ("split_count", "0\n".to_owned()),
        ("start_head", format!("{}\n", "0".repeat(SHA_LEN))),
        ("started_rebase", "true\n".to_owned()),
    ]
    .into_iter()
    .map(|(name, text)| (OsString::from(name), text))
    .collect::<BTreeMap<_, _>>();
    assert_eq!(fixture.observed_journal(), Some(expected_journal));
}

#[test]
fn cmd_start_recovery_wait_preserves_populated_begin_journal() {
    use crate::git_factor::tests::start_contracts::replay;
    use alloc::collections::BTreeMap;

    let selected = NonEmpty::new(CommitSha::new("a".repeat(SHA_LEN)).or_abort("admitted SHA"));
    let fixture = replay::direct_start(&selected, false, &replay::ReplayCase::WaitingAfterBegin);
    let ctx = fixture.ctx();

    let result = cmd_start_with_resolved_in(&ctx, &fixture.exec, &fixture.state, &fixture.selected);

    assert_eq!(result.map_err(|err| err.to_string()), Ok(EXIT_TEMPFAIL));
    assert_eq!(fixture.io.stdout(), "");
    assert_eq!(fixture.io.stderr(), "");
    assert_eq!(fixture.observed_calls(), fixture.expected_calls);
    assert_eq!(fixture.remaining_keys(), Vec::<String>::new());
    // Compare net contents/layout in this tempdir, excluding factor state; Git has its own ledger.
    assert_eq!(fixture.direct_files_after(), fixture.direct_files_before);
    let expected_journal = [
        ("commits", format!("{}\n", "a".repeat(SHA_LEN))),
        ("current_index", "0\n".to_owned()),
        ("exec", "true\n".to_owned()),
        ("expected_tree", TREE_EXPECTED_NL.to_owned()),
        ("is_root", "false\n".to_owned()),
        ("phase", "pending_start\n".to_owned()),
        ("requires_rebase", "true\n".to_owned()),
        ("split_count", "0\n".to_owned()),
        ("start_head", format!("{}\n", "0".repeat(SHA_LEN))),
        ("started_rebase", "true\n".to_owned()),
    ]
    .into_iter()
    .map(|(name, text)| (OsString::from(name), text))
    .collect::<BTreeMap<_, _>>();
    assert_eq!(fixture.observed_journal(), Some(expected_journal));
}

#[test]
fn cmd_start_refuses_parent_query_io_before_mutation() {
    use crate::git_factor::tests::start_contracts::query;
    use crate::git_factor::tests::start_contracts::query::{QueryCase, QueryReply, QueryTarget};

    let selected = NonEmpty::new(CommitSha::new("a".repeat(SHA_LEN)).or_abort("admitted SHA"));
    let case = QueryCase::Failure {
        target: QueryTarget::Parent,
        reply: QueryReply::Io,
    };
    let fixture = query::direct_start(&selected, false, &case, "gate out", "gate err");
    let ctx = fixture.ctx();

    let result = cmd_start_with_resolved_in(&ctx, &fixture.exec, &fixture.state, &fixture.selected);

    assert_eq!(
        result.map_err(|err| err.to_string()),
        Err("git command failed: git cat-file: selected query IO failure".to_owned())
    );
    assert_eq!(fixture.io.stdout(), "");
    assert_eq!(fixture.io.stderr(), "");
    assert_eq!(fixture.observed_journal(), None);
    assert_eq!(fixture.observed_calls(), fixture.expected_calls);
    assert_eq!(fixture.remaining_keys(), Vec::<String>::new());
    // Only net contents/layout inside this tempdir, excluding factor state.
    assert_eq!(fixture.direct_files_after(), fixture.direct_files_before);
}

fn failed_gate_output(at: NonZeroUsize) -> DirectStart {
    use crate::git_factor::tests::start_contracts::GateCase;

    let selected = NonEmpty::new(CommitSha::new("a".repeat(SHA_LEN)).or_abort("admitted SHA"));
    DirectStart::new(
        &selected,
        false,
        GateCase::FailOutputFailure(at),
        "gate stdout",
        "gate stderr",
    )
}

#[test]
fn cmd_start_stops_after_gate_stdout_write_failure() {
    use core::num::NonZeroUsize;

    let fixture = failed_gate_output(NonZeroUsize::new(1).or_abort("positive write ordinal"));
    let ctx = fixture.ctx();

    let result = cmd_start_with_resolved_in(&ctx, &fixture.exec, &fixture.state, &fixture.selected);

    assert_eq!(
        result.map_err(|err| err.to_string()),
        Err("failed to write output: selected output write failed".to_owned())
    );
    assert_eq!(fixture.io.stdout(), "");
    assert_eq!(fixture.io.stderr(), "");
    assert_eq!(fixture.observed_journal(), None);
    assert_eq!(fixture.observed_calls(), fixture.expected_calls);
    assert_eq!(fixture.remaining_keys(), Vec::<String>::new());
    // Only net contents/layout inside this tempdir, excluding factor state.
    assert_eq!(fixture.direct_files_after(), fixture.direct_files_before);
}

#[test]
fn cmd_start_stops_after_gate_stderr_write_failure() {
    use core::num::NonZeroUsize;

    let fixture = failed_gate_output(NonZeroUsize::new(2).or_abort("positive write ordinal"));
    let ctx = fixture.ctx();

    let result = cmd_start_with_resolved_in(&ctx, &fixture.exec, &fixture.state, &fixture.selected);

    assert_eq!(
        result.map_err(|err| err.to_string()),
        Err("failed to write output: selected output write failed".to_owned())
    );
    assert_eq!(fixture.io.stdout(), "gate stdout");
    assert_eq!(fixture.io.stderr(), "");
    assert_eq!(fixture.observed_journal(), None);
    assert_eq!(fixture.observed_calls(), fixture.expected_calls);
    assert_eq!(fixture.remaining_keys(), Vec::<String>::new());
    // Only net contents/layout inside this tempdir, excluding factor state.
    assert_eq!(fixture.direct_files_after(), fixture.direct_files_before);
}

#[test]
fn cmd_start_stops_after_gate_failure_heading_write_failure() {
    use core::num::NonZeroUsize;

    let fixture = failed_gate_output(NonZeroUsize::new(3).or_abort("positive write ordinal"));
    let ctx = fixture.ctx();

    let result = cmd_start_with_resolved_in(&ctx, &fixture.exec, &fixture.state, &fixture.selected);

    assert_eq!(
        result.map_err(|err| err.to_string()),
        Err("failed to write output: selected output write failed".to_owned())
    );
    assert_eq!(fixture.io.stdout(), "gate stdout");
    assert_eq!(fixture.io.stderr(), "gate stderr");
    assert_eq!(fixture.observed_journal(), None);
    assert_eq!(fixture.observed_calls(), fixture.expected_calls);
    assert_eq!(fixture.remaining_keys(), Vec::<String>::new());
    // Only net contents/layout inside this tempdir, excluding factor state.
    assert_eq!(fixture.direct_files_after(), fixture.direct_files_before);
}

#[test]
fn cmd_start_stops_after_gate_failure_command_write_failure() {
    use core::num::NonZeroUsize;

    let fixture = failed_gate_output(NonZeroUsize::new(4).or_abort("positive write ordinal"));
    let ctx = fixture.ctx();

    let result = cmd_start_with_resolved_in(&ctx, &fixture.exec, &fixture.state, &fixture.selected);

    assert_eq!(
        result.map_err(|err| err.to_string()),
        Err("failed to write output: selected output write failed".to_owned())
    );
    assert_eq!(
        fixture.io.stdout(),
        "gate stdoutFACTOR: Start gate failed.\n"
    );
    assert_eq!(fixture.io.stderr(), "gate stderr");
    assert_eq!(fixture.observed_journal(), None);
    assert_eq!(fixture.observed_calls(), fixture.expected_calls);
    assert_eq!(fixture.remaining_keys(), Vec::<String>::new());
    // Only net contents/layout inside this tempdir, excluding factor state.
    assert_eq!(fixture.direct_files_after(), fixture.direct_files_before);
}

#[test]
fn cmd_start_stops_after_gate_failure_exit_code_write_failure() {
    use core::num::NonZeroUsize;

    let fixture = failed_gate_output(NonZeroUsize::new(5).or_abort("positive write ordinal"));
    let ctx = fixture.ctx();

    let result = cmd_start_with_resolved_in(&ctx, &fixture.exec, &fixture.state, &fixture.selected);

    assert_eq!(
        result.map_err(|err| err.to_string()),
        Err("failed to write output: selected output write failed".to_owned())
    );
    assert_eq!(
        fixture.io.stdout(),
        "gate stdoutFACTOR: Start gate failed.\nEXEC: true\n"
    );
    assert_eq!(fixture.io.stderr(), "gate stderr");
    assert_eq!(fixture.observed_journal(), None);
    assert_eq!(fixture.observed_calls(), fixture.expected_calls);
    assert_eq!(fixture.remaining_keys(), Vec::<String>::new());
    // Only net contents/layout inside this tempdir, excluding factor state.
    assert_eq!(fixture.direct_files_after(), fixture.direct_files_before);
}

#[test]
fn cmd_start_stops_after_gate_failure_blank_line_write_failure() {
    use core::num::NonZeroUsize;

    let fixture = failed_gate_output(NonZeroUsize::new(6).or_abort("positive write ordinal"));
    let ctx = fixture.ctx();

    let result = cmd_start_with_resolved_in(&ctx, &fixture.exec, &fixture.state, &fixture.selected);

    assert_eq!(
        result.map_err(|err| err.to_string()),
        Err("failed to write output: selected output write failed".to_owned())
    );
    assert_eq!(
        fixture.io.stdout(),
        "gate stdoutFACTOR: Start gate failed.\nEXEC: true\nCODE: 7\n"
    );
    assert_eq!(fixture.io.stderr(), "gate stderr");
    assert_eq!(fixture.observed_journal(), None);
    assert_eq!(fixture.observed_calls(), fixture.expected_calls);
    assert_eq!(fixture.remaining_keys(), Vec::<String>::new());
    // Only net contents/layout inside this tempdir, excluding factor state.
    assert_eq!(fixture.direct_files_after(), fixture.direct_files_before);
}

#[test]
fn cmd_start_stops_after_gate_failure_recovery_hint_write_failure() {
    use core::num::NonZeroUsize;

    let fixture = failed_gate_output(NonZeroUsize::new(7).or_abort("positive write ordinal"));
    let ctx = fixture.ctx();

    let result = cmd_start_with_resolved_in(&ctx, &fixture.exec, &fixture.state, &fixture.selected);

    assert_eq!(
        result.map_err(|err| err.to_string()),
        Err("failed to write output: selected output write failed".to_owned())
    );
    assert_eq!(
        fixture.io.stdout(),
        "gate stdoutFACTOR: Start gate failed.\nEXEC: true\nCODE: 7\n\n"
    );
    assert_eq!(fixture.io.stderr(), "gate stderr");
    assert_eq!(fixture.observed_journal(), None);
    assert_eq!(fixture.observed_calls(), fixture.expected_calls);
    assert_eq!(fixture.remaining_keys(), Vec::<String>::new());
    // Only net contents/layout inside this tempdir, excluding factor state.
    assert_eq!(fixture.direct_files_after(), fixture.direct_files_before);
}

#[test]
fn cmd_start_refuses_post_gate_worktree_query_before_journal_creation() {
    use crate::git_factor::tests::start_contracts::query;
    use crate::git_factor::tests::start_contracts::query::{QueryCase, QueryReply, QueryTarget};

    let selected = NonEmpty::new(CommitSha::new("a".repeat(SHA_LEN)).or_abort("admitted SHA"));
    let case = QueryCase::Failure {
        target: QueryTarget::Worktree,
        reply: QueryReply::Rejected,
    };
    let fixture = query::direct_start(&selected, false, &case, "gate out", "gate err");
    let ctx = fixture.ctx();

    let result = cmd_start_with_resolved_in(&ctx, &fixture.exec, &fixture.state, &fixture.selected);

    assert_eq!(
        result.map_err(|err| err.to_string()),
        Err(concat!(
            "git command failed: git status --porcelain=v1 produced unexpected ",
            "output (exit 128)\nSTDERR:\nselected query refused"
        )
        .to_owned())
    );
    assert_eq!(fixture.io.stdout(), "gate out");
    assert_eq!(fixture.io.stderr(), "gate err");
    assert_eq!(fixture.observed_journal(), None);
    assert_eq!(fixture.observed_calls(), fixture.expected_calls);
    assert_eq!(fixture.remaining_keys(), Vec::<String>::new());
    // Only net contents/layout inside this tempdir, excluding factor state.
    assert_eq!(fixture.direct_files_after(), fixture.direct_files_before);
}

#[test]
fn cmd_status_reports_no_active_session_message() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    fs::create_dir_all(&git_dir).or_abort("create git dir");

    let runner = with_git_dir_outputs(ScriptedRunner::default(), repo, 8);
    let io = TestIo::default();
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };
    let ctx = Ctx {
        runner: &runner,
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &REAL_FS,
    };

    let code = cmd_status_in(&ctx).or_abort("status should succeed");

    assert_eq!(code, EXIT_OK);
    assert_eq!(io.stdout(), "FACTOR: No active session.\n");
    assert!(io.stderr().is_empty(), "stderr should be empty");
}

#[test]
fn cmd_status_reports_active_session_fields() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).or_abort("create factor dir");
    fs::create_dir_all(git_dir.join("rebase-merge")).or_abort("create rebase-merge");

    let sha = "a".repeat(SHA_LEN);
    fs::write(state_dir.join("commits"), format!("{sha}\n")).or_abort("write commits");
    fs::write(state_dir.join("current_index"), "0\n").or_abort("write current index");
    fs::write(state_dir.join("split_count"), "2\n").or_abort("write split count");
    fs::write(state_dir.join("requires_rebase"), "false\n").or_abort("write requires rebase");
    fs::write(state_dir.join("is_root"), "true\n").or_abort("write is root");

    let runner = with_git_dir_outputs(ScriptedRunner::default(), repo, 8);
    let io = TestIo::default();
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };
    let ctx = Ctx {
        runner: &runner,
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &REAL_FS,
    };

    let code = cmd_status_in(&ctx).or_abort("status should succeed");

    assert_eq!(code, EXIT_OK);
    assert_eq!(
        io.stdout(),
        format!(
            concat!(
                "FACTOR: Active session.\n",
                "CURRENT_COMMIT: {}\n",
                "CURRENT_INDEX: 0\n",
                "SPLIT_COUNT: 2\n",
                "PHASE: splitting\n",
                "REQUIRES_REBASE: false\n",
                "REBASE_IN_PROGRESS: true\n",
                "IS_ROOT: true\n"
            ),
            sha
        )
    );
    assert!(io.stderr().is_empty(), "stderr should be empty");
}

#[test]
fn cmd_status_io_failures_cover_active_session_output_paths() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).or_abort("create factor dir");
    fs::create_dir_all(git_dir.join("rebase-merge")).or_abort("create rebase-merge");

    let sha = "a".repeat(SHA_LEN);
    fs::write(state_dir.join("commits"), format!("{sha}\n")).or_abort("write commits");
    fs::write(state_dir.join("current_index"), "0\n").or_abort("write current index");
    fs::write(state_dir.join("split_count"), "2\n").or_abort("write split count");
    fs::write(state_dir.join("requires_rebase"), "false\n").or_abort("write requires rebase");
    fs::write(state_dir.join("is_root"), "true\n").or_abort("write is root");

    let base_runner =
        ScriptedRunner::default().with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n");
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };

    for fail_at in 1..=14 {
        let runner = NthRunnerFailure::new(base_runner.clone(), usize::MAX);
        let io = NthIoFailure::new(fail_at);
        let ctx = Ctx {
            runner: &runner,
            cwd: repo.to_path_buf(),
            io: &io,
            env: &env,
            fs: &REAL_FS,
        };

        let err = cmd_status_in(&ctx).err_or_abort("expected io failure");
        assert!(
            matches!(&err, FactorError::Io(inner) if inner.to_string().contains("io fail")),
            "err was: {err:?}"
        );
    }
}

#[test]
fn cmd_status_io_failure_on_no_active_session_covers_outln_error_path() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    fs::create_dir_all(repo.join(".git")).or_abort("create git dir");

    let runner =
        ScriptedRunner::default().with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n");
    let io = FailingIo;
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };
    let ctx = Ctx {
        runner: &runner,
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &REAL_FS,
    };

    let err = cmd_status_in(&ctx).err_or_abort("expected io failure");
    assert!(
        matches!(&err, FactorError::Io(inner) if inner.to_string().contains("io fail")),
        "err was: {err:?}"
    );
}

#[test]
fn cmd_status_state_failures_cover_internal_question_mark_paths() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).or_abort("create factor dir");
    fs::write(
        state_dir.join("commits"),
        format!("{}\n", "a".repeat(SHA_LEN)),
    )
    .or_abort("write commits");
    fs::write(state_dir.join("current_index"), "0\n").or_abort("write current index");
    fs::write(state_dir.join("split_count"), "1\n").or_abort("write split count");
    fs::write(state_dir.join("requires_rebase"), "false\n").or_abort("write requires_rebase");
    fs::write(state_dir.join("is_root"), "false\n").or_abort("write is_root");

    let base_runner =
        ScriptedRunner::default().with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n");
    let io = TestIo::default();
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };

    let factor_dir_runner = NthRunnerFailure::new(base_runner.clone(), 2);
    let factor_dir_ctx = Ctx {
        runner: &factor_dir_runner,
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &REAL_FS,
    };
    let factor_dir_err =
        cmd_status_in(&factor_dir_ctx).err_or_abort("expected factor_dir_in failure");
    assert!(
        matches!(&factor_dir_err, FactorError::GitDir(msg) if msg.contains("forced runner failure")),
        "err was: {factor_dir_err:?}"
    );

    fs::write(
        state_dir.join("commits"),
        format!("{}\n", "a".repeat(SHA_LEN)),
    )
    .or_abort("reset commits");
    fs::write(state_dir.join("current_index"), "0\n").or_abort("reset current index");
    fs::write(state_dir.join("split_count"), "1\n").or_abort("reset split count");
    fs::write(state_dir.join("requires_rebase"), "false\n").or_abort("reset requires_rebase");
    fs::write(state_dir.join("is_root"), "false\n").or_abort("reset is_root");
    fs::remove_file(state_dir.join("commits")).or_abort("remove commits");
    let base_ctx = Ctx {
        runner: &base_runner,
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &REAL_FS,
    };
    let missing_commits_err =
        cmd_status_in(&base_ctx).err_or_abort("expected missing commits state");
    assert!(
        matches!(&missing_commits_err, FactorError::StateRead(inner) if inner.kind() == io::ErrorKind::NotFound),
        "err was: {missing_commits_err:?}"
    );

    fs::write(
        state_dir.join("commits"),
        format!("{}\n", "a".repeat(SHA_LEN)),
    )
    .or_abort("restore commits");
    fs::write(state_dir.join("current_index"), "not-a-number\n").or_abort("corrupt current_index");
    let invalid_current_index_err =
        cmd_status_in(&base_ctx).err_or_abort("expected invalid current_index");
    assert!(
        matches!(&invalid_current_index_err, FactorError::GitCommand(msg) if msg.contains("current_index")),
        "err was: {invalid_current_index_err:?}"
    );

    fs::write(state_dir.join("current_index"), "0\n").or_abort("restore current_index");
    fs::write(state_dir.join("split_count"), "not-a-number\n").or_abort("corrupt split_count");
    let invalid_split_count_err =
        cmd_status_in(&base_ctx).err_or_abort("expected invalid split_count");
    assert!(
        matches!(&invalid_split_count_err, FactorError::GitCommand(msg) if msg.contains("split_count")),
        "err was: {invalid_split_count_err:?}"
    );

    fs::write(state_dir.join("split_count"), "1\n").or_abort("restore split_count");
    fs::write(state_dir.join("requires_rebase"), "not-bool\n").or_abort("corrupt requires_rebase");
    let invalid_requires_rebase_err =
        cmd_status_in(&base_ctx).err_or_abort("expected invalid requires_rebase");
    assert!(
        matches!(&invalid_requires_rebase_err, FactorError::GitCommand(msg) if msg.contains("requires_rebase")),
        "err was: {invalid_requires_rebase_err:?}"
    );

    fs::write(state_dir.join("requires_rebase"), "false\n").or_abort("restore requires_rebase");
    fs::write(state_dir.join("is_root"), "not-bool\n").or_abort("corrupt is_root");
    let invalid_is_root_err = cmd_status_in(&base_ctx).err_or_abort("expected invalid is_root");
    assert!(
        matches!(&invalid_is_root_err, FactorError::GitCommand(msg) if msg.contains("is_root")),
        "err was: {invalid_is_root_err:?}"
    );
}

#[test]
fn cmd_status_propagates_second_current_index_read_failure() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).or_abort("create factor dir");

    let sha = "a".repeat(SHA_LEN);
    fs::write(state_dir.join("commits"), format!("{sha}\n")).or_abort("write commits");
    fs::write(state_dir.join("current_index"), "0\n").or_abort("write current_index");
    fs::write(state_dir.join("split_count"), "1\n").or_abort("write split_count");
    fs::write(state_dir.join("requires_rebase"), "false\n").or_abort("write requires_rebase");
    fs::write(state_dir.join("is_root"), "false\n").or_abort("write is_root");

    let runner =
        ScriptedRunner::default().with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n");
    let io = TestIo::default();
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };
    let fs = NthReadFailureFs::new("current_index", 2, "second current_index read failed");
    let ctx = Ctx {
        runner: &runner,
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &fs,
    };

    let err = cmd_status_in(&ctx).err_or_abort("expected second current_index read to fail");
    assert!(
        matches!(&err, FactorError::StateRead(inner) if inner.to_string().contains("second current_index read failed")),
        "err was: {err:?}"
    );
}

#[test]
fn cmd_abort_reports_rebase_hint_when_rebase_still_active() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).or_abort("create factor dir");
    fs::create_dir_all(git_dir.join("rebase-merge")).or_abort("create rebase-merge");

    let sha = "a".repeat(SHA_LEN);
    fs::write(state_dir.join("commits"), format!("{sha}\n")).or_abort("write commits");
    fs::write(state_dir.join("current_index"), "0\n").or_abort("write current index");

    let runner = ScriptedRunner::default()
        .with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n")
        .with_status(
            "git",
            &["reset", "--hard", "--quiet", &sha],
            &[],
            false,
            repo,
            0,
        )
        .with_status(
            "git",
            &["clean", "--force", "--quiet", "-d"],
            &[],
            false,
            repo,
            0,
        );
    let io = TestIo::default();
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };
    let ctx = Ctx {
        runner: &runner,
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &REAL_FS,
    };

    let code = cmd_abort_in(&ctx).or_abort("abort should succeed");

    assert_eq!(code, EXIT_OK);
    assert_eq!(
        io.stdout(),
        concat!(
            "FACTOR: Session aborted for current commit step.\n",
            "FACTOR: Rebase still active. To abort full rebase, run: git rebase --abort\n"
        )
    );
    assert!(io.stderr().is_empty(), "stderr should be empty");
    assert!(
        !state_dir.exists(),
        "factor state dir should be removed after abort"
    );
}

#[test]
fn cmd_abort_errors_when_no_active_session() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let runner =
        ScriptedRunner::default().with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n");
    let io = TestIo::default();
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };
    let ctx = Ctx {
        runner: &runner,
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &REAL_FS,
    };

    let err = cmd_abort_in(&ctx).err_or_abort("expected no active session");
    assert!(
        matches!(err, FactorError::NoActiveSession),
        "err was: {err:?}"
    );
}

#[test]
fn cmd_abort_io_failures_cover_output_paths() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).or_abort("create factor dir");
    fs::create_dir_all(git_dir.join("rebase-merge")).or_abort("create rebase-merge");

    let sha = "a".repeat(SHA_LEN);
    fs::write(state_dir.join("commits"), format!("{sha}\n")).or_abort("write commits");
    fs::write(state_dir.join("current_index"), "0\n").or_abort("write current index");

    let base_runner = ScriptedRunner::default()
        .with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n")
        .with_status(
            "git",
            &["reset", "--hard", "--quiet", &sha],
            &[],
            false,
            repo,
            0,
        )
        .with_status(
            "git",
            &["clean", "--force", "--quiet", "-d"],
            &[],
            false,
            repo,
            0,
        );
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };

    for fail_at in 1..=4 {
        fs::create_dir_all(&state_dir).or_abort("recreate factor dir");
        fs::write(state_dir.join("commits"), format!("{sha}\n")).or_abort("rewrite commits");
        fs::write(state_dir.join("current_index"), "0\n").or_abort("rewrite current index");

        let runner = NthRunnerFailure::new(base_runner.clone(), usize::MAX);
        let io = NthIoFailure::new(fail_at);
        let ctx = Ctx {
            runner: &runner,
            cwd: repo.to_path_buf(),
            io: &io,
            env: &env,
            fs: &REAL_FS,
        };

        let err = cmd_abort_in(&ctx).err_or_abort("expected io failure");
        assert!(
            matches!(&err, FactorError::Io(inner) if inner.to_string().contains("io fail")),
            "err was: {err:?}"
        );
    }
}

#[test]
fn cmd_abort_runner_failures_cover_internal_question_mark_paths() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).or_abort("create factor dir");

    let sha = "a".repeat(SHA_LEN);
    let base_runner = ScriptedRunner::default()
        .with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n")
        .with_status(
            "git",
            &["reset", "--hard", "--quiet", &sha],
            &[],
            false,
            repo,
            0,
        )
        .with_status(
            "git",
            &["clean", "--force", "--quiet", "-d"],
            &[],
            false,
            repo,
            0,
        );
    let io = TestIo::default();
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };

    for fail_at in [2, 3, 4] {
        fs::create_dir_all(&state_dir).or_abort("recreate factor dir");
        fs::write(state_dir.join("commits"), format!("{sha}\n")).or_abort("write commits");
        fs::write(state_dir.join("current_index"), "0\n").or_abort("write current index");

        let runner = NthRunnerFailure::new(base_runner.clone(), fail_at);
        let ctx = Ctx {
            runner: &runner,
            cwd: repo.to_path_buf(),
            io: &io,
            env: &env,
            fs: &REAL_FS,
        };

        let err = cmd_abort_in(&ctx).err_or_abort("expected forced runner failure");
        assert!(is_forced_runner_failure(&err), "err was: {err:?}");
    }
}

#[test]
fn cmd_abort_errors_when_current_commit_state_is_missing() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).or_abort("create factor dir");
    fs::write(state_dir.join("current_index"), "0\n").or_abort("write current index");

    let runner =
        ScriptedRunner::default().with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n");
    let io = TestIo::default();
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };
    let ctx = Ctx {
        runner: &runner,
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &REAL_FS,
    };

    let err = cmd_abort_in(&ctx).err_or_abort("expected missing state file to fail");
    assert!(
        matches!(&err, FactorError::StateRead(inner) if inner.kind() == io::ErrorKind::NotFound),
        "err was: {err:?}"
    );
}

#[test]
fn cmd_abort_propagates_start_head_state_read_error() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let state_dir = repo.join(".git").join("factor");
    fs::create_dir_all(&state_dir).or_abort("create factor dir");
    fs::write(
        state_dir.join("commits"),
        format!("{}\n", "a".repeat(SHA_LEN)),
    )
    .or_abort("write commits");
    fs::create_dir_all(state_dir.join("start_head")).or_abort("create invalid start_head");

    let runner =
        ScriptedRunner::default().with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n");
    let io = TestIo::default();
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };
    let ctx = Ctx {
        runner: &runner,
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &REAL_FS,
    };

    let err = cmd_abort_in(&ctx).err_or_abort("expected start_head read failure");
    assert!(
        matches!(&err, FactorError::StateRead(inner) if inner.kind() != io::ErrorKind::NotFound),
        "err was: {err:?}"
    );
}

#[test]
fn cmd_abort_propagates_requires_rebase_state_read_error() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let state_dir = repo.join(".git").join("factor");
    fs::create_dir_all(&state_dir).or_abort("create factor dir");
    fs::write(
        state_dir.join("commits"),
        format!("{}\n", "a".repeat(SHA_LEN)),
    )
    .or_abort("write commits");
    fs::create_dir_all(state_dir.join("requires_rebase"))
        .or_abort("create invalid requires_rebase");

    let runner =
        ScriptedRunner::default().with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n");
    let io = TestIo::default();
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };
    let ctx = Ctx {
        runner: &runner,
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &REAL_FS,
    };

    let err = cmd_abort_in(&ctx).err_or_abort("expected requires_rebase read failure");
    assert!(
        matches!(&err, FactorError::StateRead(inner) if inner.kind() != io::ErrorKind::NotFound),
        "err was: {err:?}"
    );
}

#[test]
fn cmd_abort_propagates_started_rebase_state_read_error() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let state_dir = repo.join(".git").join("factor");
    fs::create_dir_all(&state_dir).or_abort("create factor dir");
    fs::write(
        state_dir.join("commits"),
        format!("{}\n", "a".repeat(SHA_LEN)),
    )
    .or_abort("write commits");
    fs::write(state_dir.join("requires_rebase"), "false\n").or_abort("write requires_rebase");
    fs::create_dir_all(state_dir.join("started_rebase")).or_abort("create invalid started_rebase");

    let runner =
        ScriptedRunner::default().with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n");
    let io = TestIo::default();
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };
    let ctx = Ctx {
        runner: &runner,
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &REAL_FS,
    };

    let err = cmd_abort_in(&ctx).err_or_abort("expected started_rebase read failure");
    assert!(
        matches!(&err, FactorError::StateRead(inner) if inner.kind() != io::ErrorKind::NotFound),
        "err was: {err:?}"
    );
}

#[test]
fn cmd_abort_omits_rebase_hint_when_rebase_is_not_active() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).or_abort("create factor dir");

    let sha = "a".repeat(SHA_LEN);
    fs::write(state_dir.join("commits"), format!("{sha}\n")).or_abort("write commits");
    fs::write(state_dir.join("current_index"), "0\n").or_abort("write current index");

    let runner = ScriptedRunner::default()
        .with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n")
        .with_status(
            "git",
            &["reset", "--hard", "--quiet", &sha],
            &[],
            false,
            repo,
            0,
        )
        .with_status(
            "git",
            &["clean", "--force", "--quiet", "-d"],
            &[],
            false,
            repo,
            0,
        );
    let io = TestIo::default();
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };
    let ctx = Ctx {
        runner: &runner,
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &REAL_FS,
    };

    let code = cmd_abort_in(&ctx).or_abort("abort should succeed");
    assert_eq!(code, EXIT_OK);
    assert_eq!(
        io.stdout(),
        "FACTOR: Session aborted for current commit step.\n"
    );
    assert!(io.stderr().is_empty(), "stderr should be empty");
}

#[test]
fn cmd_abort_runs_rebase_abort_when_started_rebase_is_true() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).or_abort("create factor dir");
    fs::create_dir_all(git_dir.join("rebase-merge")).or_abort("create rebase-merge");

    let sha = "a".repeat(SHA_LEN);
    fs::write(state_dir.join("commits"), format!("{sha}\n")).or_abort("write commits");
    fs::write(state_dir.join("current_index"), "0\n").or_abort("write current index");
    fs::write(state_dir.join("started_rebase"), "true\n").or_abort("write started_rebase");

    let runner = with_git_dir_outputs(ScriptedRunner::default(), repo, 4)
        .with_status(
            "git",
            &["rebase", "--abort"],
            &[("GIT_EDITOR", "false"), ("GIT_SEQUENCE_EDITOR", "false")],
            false,
            repo,
            0,
        )
        .with_status(
            "git",
            &["reset", "--hard", "--quiet", &sha],
            &[],
            false,
            repo,
            0,
        )
        .with_status(
            "git",
            &["clean", "--force", "--quiet", "-d"],
            &[],
            false,
            repo,
            0,
        );
    let io = TestIo::default();
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };
    let ctx = Ctx {
        runner: &runner,
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &REAL_FS,
    };

    let code = cmd_abort_in(&ctx).or_abort("abort should succeed");

    assert_eq!(code, EXIT_OK);
    assert_eq!(
        io.stdout(),
        concat!(
            "FACTOR: Session aborted for current commit step.\n",
            "FACTOR: Rebase still active. To abort full rebase, run: git rebase --abort\n"
        )
    );
    assert!(io.stderr().is_empty(), "stderr should be empty");
}

#[test]
fn cmd_abort_errors_when_state_dir_removal_fails() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).or_abort("create factor dir");

    let sha = "a".repeat(SHA_LEN);
    fs::write(state_dir.join("commits"), format!("{sha}\n")).or_abort("write commits");
    fs::write(state_dir.join("current_index"), "0\n").or_abort("write current index");

    let runner = ScriptedRunner::default()
        .with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n")
        .with_status(
            "git",
            &["reset", "--hard", "--quiet", &sha],
            &[],
            false,
            repo,
            0,
        )
        .with_status(
            "git",
            &["clean", "--force", "--quiet", "-d"],
            &[],
            false,
            repo,
            0,
        );
    let io = TestIo::default();
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };
    let ctx = Ctx {
        runner: &runner,
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &FailingRemoveDirAllFs,
    };

    let err = cmd_abort_in(&ctx).err_or_abort("expected cleanup failure");
    assert!(
        matches!(&err, FactorError::GitCommand(msg) if msg.contains("failed to remove factor state path")),
        "err was: {err:?}"
    );
    assert!(io.stdout().is_empty(), "stdout should be empty");
    assert!(io.stderr().is_empty(), "stderr should be empty");
    assert!(
        state_dir.exists(),
        "state dir should remain when cleanup fails"
    );
}

#[test]
fn cmd_abort_errors_when_cleanup_leaves_state_path_behind() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).or_abort("create factor dir");

    let sha = "a".repeat(SHA_LEN);
    fs::write(state_dir.join("commits"), format!("{sha}\n")).or_abort("write commits");
    fs::write(state_dir.join("current_index"), "0\n").or_abort("write current index");

    let runner = ScriptedRunner::default()
        .with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n")
        .with_status(
            "git",
            &["reset", "--hard", "--quiet", &sha],
            &[],
            false,
            repo,
            0,
        )
        .with_status(
            "git",
            &["clean", "--force", "--quiet", "-d"],
            &[],
            false,
            repo,
            0,
        );
    let io = TestIo::default();
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };
    let ctx = Ctx {
        runner: &runner,
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &StickyStatePathFs,
    };

    let err = cmd_abort_in(&ctx).err_or_abort("expected persistent-state-path failure");
    assert!(
        matches!(&err, FactorError::GitCommand(msg) if msg.contains("still exists after cleanup")),
        "err was: {err:?}"
    );
    assert!(io.stdout().is_empty(), "stdout should be empty");
    assert!(io.stderr().is_empty(), "stderr should be empty");
}

#[test]
fn cmd_abort_uses_start_head_and_skips_rebase_abort_when_rebase_is_not_active() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).or_abort("create factor dir");

    let current = "a".repeat(SHA_LEN);
    let start_head = "b".repeat(SHA_LEN);
    fs::write(state_dir.join("commits"), format!("{current}\n")).or_abort("write commits");
    fs::write(state_dir.join("current_index"), "0\n").or_abort("write current index");
    fs::write(state_dir.join("started_rebase"), "true\n").or_abort("write started_rebase");
    fs::write(state_dir.join("start_head"), format!("{start_head}\n")).or_abort("write start_head");

    let runner = ScriptedRunner::default()
        .with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n")
        .with_status(
            "git",
            &["reset", "--hard", "--quiet", &start_head],
            &[],
            false,
            repo,
            0,
        )
        .with_status(
            "git",
            &["clean", "--force", "--quiet", "-d"],
            &[],
            false,
            repo,
            0,
        );
    let io = TestIo::default();
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };
    let ctx = Ctx {
        runner: &runner,
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &REAL_FS,
    };

    let code = cmd_abort_in(&ctx).or_abort("abort should succeed");
    assert_eq!(code, EXIT_OK);
    assert_eq!(
        io.stdout(),
        "FACTOR: Session aborted for current commit step.\n"
    );
    assert!(io.stderr().is_empty(), "stderr should be empty");
}

#[test]
fn cmd_abort_propagates_rebase_abort_nonzero_exit() {
    let dir = TempDir::new().or_abort("tempdir");
    let repo = dir.path();
    let git_dir = repo.join(".git");
    let state_dir = git_dir.join("factor");
    fs::create_dir_all(&state_dir).or_abort("create factor dir");
    fs::create_dir_all(git_dir.join("rebase-merge")).or_abort("create rebase-merge");

    let sha = "a".repeat(SHA_LEN);
    fs::write(state_dir.join("commits"), format!("{sha}\n")).or_abort("write commits");
    fs::write(state_dir.join("current_index"), "0\n").or_abort("write current index");
    fs::write(state_dir.join("started_rebase"), "true\n").or_abort("write started_rebase");

    let runner = ScriptedRunner::default()
        .with_output("git", &["rev-parse", "--git-dir"], repo, ".git\n")
        .with_status(
            "git",
            &["rebase", "--abort"],
            &[("GIT_EDITOR", "false"), ("GIT_SEQUENCE_EDITOR", "false")],
            false,
            repo,
            1,
        );
    let io = TestIo::default();
    let env = TestEnv {
        cwd: repo.to_path_buf(),
    };
    let ctx = Ctx {
        runner: &runner,
        cwd: repo.to_path_buf(),
        io: &io,
        env: &env,
        fs: &REAL_FS,
    };

    let err = cmd_abort_in(&ctx).err_or_abort("expected rebase --abort to fail");
    assert!(
        matches!(&err, FactorError::GitCommand(msg) if msg.contains("git rebase failed (exit")),
        "err was: {err:?}"
    );
}

#[test]
fn run_start_rebase_preserves_begin_journal_when_finished_cleanup_is_denied() {
    use crate::git_factor::tests::start_contracts::replay::cleanup;
    use alloc::collections::BTreeMap;

    let selected = NonEmpty::new(CommitSha::new("a".repeat(SHA_LEN)).or_abort("admitted SHA"));
    let fixture = cleanup::direct_launcher(&selected, false, cleanup::NoPause::Successful);
    let ctx = fixture.ctx();
    let expected_journal = [
        ("commits", format!("{}\n", "a".repeat(SHA_LEN))),
        ("current_index", "0\n".to_owned()),
        ("exec", "true\n".to_owned()),
        (
            "expected_tree",
            "dddddddddddddddddddddddddddddddddddddddd\n".to_owned(),
        ),
        ("is_root", "false\n".to_owned()),
        ("phase", "pending_start\n".to_owned()),
        ("requires_rebase", "true\n".to_owned()),
        ("split_count", "0\n".to_owned()),
        ("start_head", format!("{}\n", "0".repeat(SHA_LEN))),
        ("started_rebase", "true\n".to_owned()),
    ]
    .into_iter()
    .map(|(name, value)| (OsString::from(name), value.into_bytes()))
    .collect::<BTreeMap<_, _>>();
    assert_eq!(fixture.journal_bytes(), None);
    assert_eq!(fixture.journal_at_begin(), expected_journal);

    let result = run_start_rebase_in(
        &ctx,
        &fixture.span,
        &fixture.replay.state,
        &fixture.head,
        fixture.replay.exec.first(),
    );

    assert_eq!(
        result.map_err(|err| err.to_string()),
        Err(format!(
            "git command failed: failed to remove factor state path '{}': \
             selected factor removal denied",
            fixture.replay.state.as_path().display(),
        )),
    );
    assert_eq!(fixture.replay.io.stdout(), "");
    assert_eq!(fixture.replay.io.stderr(), "");
    assert_eq!(
        fixture.removal_attempts(),
        vec![fixture.replay.state.as_path().to_path_buf()]
    );
    assert_eq!(fixture.journal_bytes(), Some(expected_journal));
    assert_eq!(
        fixture.replay.observed_calls(),
        fixture.replay.expected_calls
    );
    assert_eq!(fixture.replay.remaining_keys(), Vec::<String>::new());
    assert_eq!(
        fixture.replay.direct_files_after(),
        fixture.replay.direct_files_before
    );
    assert!(!ctx.cwd.join(".git/rebase-merge").exists());
    assert!(!ctx.cwd.join(".git/rebase-apply").exists());
}

#[test]
fn run_start_rebase_preserves_begin_journal_when_failed_cleanup_is_denied() {
    use crate::git_factor::tests::start_contracts::replay::cleanup;
    use alloc::collections::BTreeMap;

    let selected = NonEmpty {
        head: CommitSha::new("a".repeat(SHA_LEN)).or_abort("admitted first SHA"),
        tail: vec![CommitSha::new("b".repeat(SHA_LEN)).or_abort("admitted tip SHA")],
    };
    let fixture = cleanup::direct_launcher(&selected, true, cleanup::NoPause::Failed);
    let ctx = fixture.ctx();
    let expected_journal = [
        (
            "commits",
            format!("{}\n{}\n", "a".repeat(SHA_LEN), "b".repeat(SHA_LEN)),
        ),
        ("current_index", "1\n".to_owned()),
        ("exec", "true\n".to_owned()),
        (
            "expected_tree",
            "dddddddddddddddddddddddddddddddddddddddd\n".to_owned(),
        ),
        ("is_root", "true\n".to_owned()),
        ("phase", "pending_start\n".to_owned()),
        ("requires_rebase", "true\n".to_owned()),
        ("split_count", "0\n".to_owned()),
        ("start_head", format!("{}\n", "0".repeat(SHA_LEN))),
        ("started_rebase", "true\n".to_owned()),
    ]
    .into_iter()
    .map(|(name, value)| (OsString::from(name), value.into_bytes()))
    .collect::<BTreeMap<_, _>>();
    assert_eq!(fixture.journal_bytes(), None);
    assert_eq!(fixture.journal_at_begin(), expected_journal);

    let result = run_start_rebase_in(
        &ctx,
        &fixture.span,
        &fixture.replay.state,
        &fixture.head,
        fixture.replay.exec.first(),
    );

    assert_eq!(
        result.map_err(|err| err.to_string()),
        Err(format!(
            "git command failed: failed to remove factor state path '{}': \
             selected factor removal denied",
            fixture.replay.state.as_path().display(),
        )),
    );
    assert_eq!(fixture.replay.io.stdout(), "");
    assert_eq!(fixture.replay.io.stderr(), "");
    assert_eq!(
        fixture.removal_attempts(),
        vec![fixture.replay.state.as_path().to_path_buf()]
    );
    assert_eq!(fixture.journal_bytes(), Some(expected_journal));
    assert_eq!(
        fixture.replay.observed_calls(),
        fixture.replay.expected_calls
    );
    assert_eq!(fixture.replay.remaining_keys(), Vec::<String>::new());
    assert_eq!(
        fixture.replay.direct_files_after(),
        fixture.replay.direct_files_before
    );
    assert!(!ctx.cwd.join(".git/rebase-merge").exists());
    assert!(!ctx.cwd.join(".git/rebase-apply").exists());
}

#[test]
fn run_start_rebase_preserves_preflight_executable_failure_boundary() {
    use crate::git_factor::tests::start_contracts::RecordedCall;
    use crate::git_factor::tests::start_contracts::launcher::{
        LaunchCall, LaunchFault, LaunchFixture,
    };

    let selected = NonEmpty::new(CommitSha::new("a".repeat(SHA_LEN)).or_abort("admitted SHA"));
    let fixture = LaunchFixture::new(selected, false, &LaunchFault::PreflightExecutable);
    let ctx = fixture.ctx();

    let result = run_start_rebase_in(
        &ctx,
        &fixture.span,
        &fixture.state,
        &fixture.start_head,
        &fixture.exec,
    );

    assert_eq!(
        result.map_err(|err| err.to_string()),
        Err(
            "git command failed: cannot resolve current executable: preflight executable refused"
                .to_owned()
        ),
    );
    assert_eq!(
        fixture.calls(),
        vec![
            LaunchCall::CurrentExecutable,
            LaunchCall::Canonicalize(PathBuf::from("/contract/launcher/git-factor")),
            LaunchCall::Variable("GIT_FACTOR_TRACE_LOG".to_owned()),
            LaunchCall::Runner(RecordedCall::Output {
                args: vec![
                    "rev-parse".to_owned(),
                    "--short".to_owned(),
                    "a".repeat(SHA_LEN)
                ],
                bin: "git".to_owned(),
                cwd: PathBuf::from("/contract/launcher"),
            }),
            LaunchCall::Variable("GIT_FACTOR_TRACE_LOG".to_owned()),
            LaunchCall::CurrentExecutable,
        ]
    );
    assert_eq!(fixture.remaining_executable_replies(), 0);
    assert_eq!(fixture.remaining_keys(), Vec::<String>::new());
}

#[test]
fn run_start_rebase_preserves_begin_executable_failure_boundary() {
    use crate::git_factor::tests::start_contracts::RecordedCall;
    use crate::git_factor::tests::start_contracts::launcher::{
        LaunchCall, LaunchFault, LaunchFixture,
    };

    let selected = NonEmpty::new(CommitSha::new("a".repeat(SHA_LEN)).or_abort("admitted SHA"));
    let fixture = LaunchFixture::new(selected, false, &LaunchFault::BeginExecutable);
    let ctx = fixture.ctx();

    let result = run_start_rebase_in(
        &ctx,
        &fixture.span,
        &fixture.state,
        &fixture.start_head,
        &fixture.exec,
    );

    assert_eq!(
        result.map_err(|err| err.to_string()),
        Err(
            "git command failed: cannot resolve current executable: begin executable refused"
                .to_owned()
        ),
    );
    assert_eq!(
        fixture.calls(),
        vec![
            LaunchCall::CurrentExecutable,
            LaunchCall::Canonicalize(PathBuf::from("/contract/launcher/git-factor")),
            LaunchCall::Variable("GIT_FACTOR_TRACE_LOG".to_owned()),
            LaunchCall::Runner(RecordedCall::Output {
                args: vec![
                    "rev-parse".to_owned(),
                    "--short".to_owned(),
                    "a".repeat(SHA_LEN)
                ],
                bin: "git".to_owned(),
                cwd: PathBuf::from("/contract/launcher"),
            }),
            LaunchCall::Variable("GIT_FACTOR_TRACE_LOG".to_owned()),
            LaunchCall::CurrentExecutable,
            LaunchCall::CurrentExecutable,
        ]
    );
    assert_eq!(fixture.remaining_executable_replies(), 0);
    assert_eq!(fixture.remaining_keys(), Vec::<String>::new());
}

#[test]
fn cmd_start_refuses_non_ancestor_before_gate_or_journal() {
    use crate::git_factor::tests::start_contracts::query;
    use crate::git_factor::tests::start_contracts::query::{
        QueryCase, QueryPosition, QueryReply, QueryTarget,
    };

    let selected = NonEmpty::from_vec(vec![
        CommitSha::new("a".repeat(SHA_LEN)).or_abort("first SHA"),
        CommitSha::new("b".repeat(SHA_LEN)).or_abort("second SHA"),
    ])
    .or_abort("nonempty span");
    let case = QueryCase::Failure {
        target: QueryTarget::Ancestor(QueryPosition::First),
        reply: QueryReply::Rejected,
    };
    let fixture = query::direct_start(&selected, false, &case, "gate out", "gate err");
    let ctx = fixture.ctx();

    let result = cmd_start_with_resolved_in(&ctx, &fixture.exec, &fixture.state, &fixture.selected);

    assert_eq!(
        result.map_err(|err| err.to_string()),
        Err(
            "commit aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa is not an ancestor of HEAD".to_owned()
        )
    );
    assert_eq!(fixture.io.stdout(), "");
    assert_eq!(fixture.io.stderr(), "");
    assert_eq!(fixture.observed_journal(), None);
    assert_eq!(fixture.observed_calls(), fixture.expected_calls);
    assert_eq!(fixture.remaining_keys(), Vec::<String>::new());
    // Only net contents/layout inside this tempdir, excluding factor state.
    assert_eq!(fixture.direct_files_after(), fixture.direct_files_before);
}

#[test]
fn public_continue_completes_a_staged_atom() {
    let fixture = super::continue_contracts::Continuation::new(
        &"a".repeat(SHA_LEN),
        0,
        super::continue_contracts::ContinueCase::Complete,
    );
    let before = fixture.protected_bytes();
    let args = ["git-factor", "--continue", "--message", "test: message"].map(OsString::from);

    let code = main_entry_with_vec(fixture.ctx().io, Ok(fixture.ctx()), &args);

    assert_eq!(code, EXIT_OK);
    assert_eq!(
        fixture.stdout(),
        "FACTOR: Complete. Final commit split into 1 commits.\n"
    );
    assert_eq!(fixture.stderr(), "");
    assert_eq!(fixture.journal(), fixture.expected_journal());
    assert_eq!(fixture.protected_bytes(), before);
    assert_eq!(
        fixture.full_effect_requests(),
        fixture.expected_effect_requests()
    );
    assert_eq!(
        fixture.effect_requests(),
        vec![
            vec!["git", "diff", "--quiet", "--staged"],
            vec!["git", "checkout", "--quiet", "--", "."],
            vec!["git", "clean", "--force", "--quiet", "-d"],
            vec!["git", "checkout-index", "--all", "--force", "--quiet"],
            vec!["bash", "-c", "true"],
            vec!["git", "commit", "--quiet", "--message", "test: message"],
        ]
        .into_iter()
        .map(|call| call.into_iter().map(str::to_owned).collect::<Vec<_>>())
        .collect::<Vec<_>>()
    );
    assert!(!fixture.session_active());
}

#[test]
fn public_continue_preserves_selection_after_no_staged_changes() {
    let fixture = super::continue_contracts::Continuation::new(
        &"a".repeat(SHA_LEN),
        0,
        super::continue_contracts::ContinueCase::NoStaged,
    );
    let before = fixture.protected_bytes();
    let args = ["git-factor", "--continue", "--message", "test: message"].map(OsString::from);

    let code = main_entry_with_vec(fixture.ctx().io, Ok(fixture.ctx()), &args);

    assert_eq!(code, EXIT_USAGE);
    assert_eq!(fixture.stdout(), "");
    assert_eq!(
        fixture.stderr(),
        "no staged changes to commit\nNEXT: stage exactly one atomic change, then rerun:\n  git factor --continue --message \"type: description\"\n"
    );
    assert_eq!(fixture.journal(), fixture.expected_journal());
    assert_eq!(fixture.protected_bytes(), before);
    assert_eq!(
        fixture.full_effect_requests(),
        fixture.expected_effect_requests()
    );
    assert!(
        !fixture
            .effect_requests()
            .iter()
            .any(|call| call.first().is_some_and(|bin| bin == "git")
                && call.get(1).is_some_and(|arg| arg == "commit"))
    );
}

#[test]
fn public_continue_preserves_selection_after_gate_failure() {
    let fixture = super::continue_contracts::Continuation::new(
        &"a".repeat(SHA_LEN),
        0,
        super::continue_contracts::ContinueCase::GateRejected,
    );
    let before = fixture.protected_bytes();
    let args = ["git-factor", "--continue", "--message", "test: message"].map(OsString::from);

    let code = main_entry_with_vec(fixture.ctx().io, Ok(fixture.ctx()), &args);

    assert_eq!(code, EXIT_TEMPFAIL);
    assert_eq!(
        fixture.stdout(),
        "FACTOR: Exec gate failed. No commit created.\nEXEC: true\nCODE: 7\n\nNEXT: Adjust staged changes so the exec gate passes, then retry:\n  git factor --continue --message \"type: description\"\n"
    );
    assert_eq!(fixture.stderr(), "exec gate failed: true (exit code 7)\n");
    assert_eq!(fixture.journal(), fixture.expected_journal());
    assert_eq!(fixture.protected_bytes(), before);
    assert_eq!(
        fixture.full_effect_requests(),
        fixture.expected_effect_requests()
    );
    assert!(
        !fixture
            .effect_requests()
            .iter()
            .any(|call| call.first().is_some_and(|bin| bin == "git")
                && call.get(1).is_some_and(|arg| arg == "commit"))
    );
}

#[test]
fn public_continue_preserves_selection_after_gate_spawn_failure() {
    let fixture = super::continue_contracts::Continuation::new(
        &"a".repeat(SHA_LEN),
        0,
        super::continue_contracts::ContinueCase::GateSpawn,
    );
    let before = fixture.protected_bytes();
    let args = ["git-factor", "--continue", "--message", "test: message"].map(OsString::from);

    let code = main_entry_with_vec(fixture.ctx().io, Ok(fixture.ctx()), &args);

    assert_eq!(code, EXIT_SOFTWARE);
    assert_eq!(fixture.stdout(), "");
    assert_eq!(
        fixture.stderr(),
        "git command failed: bash -c: gate spawn denied\n"
    );
    assert_eq!(fixture.journal(), fixture.expected_journal());
    assert_eq!(fixture.protected_bytes(), before);
    assert_eq!(
        fixture.full_effect_requests(),
        fixture.expected_effect_requests()
    );
    assert!(
        !fixture
            .effect_requests()
            .iter()
            .any(|call| call.first().is_some_and(|bin| bin == "git")
                && call.get(1).is_some_and(|arg| arg == "commit"))
    );
}

#[test]
fn public_continue_preserves_selection_after_pre_gate_dirty_work() {
    let fixture = super::continue_contracts::Continuation::new(
        &"a".repeat(SHA_LEN),
        0,
        super::continue_contracts::ContinueCase::PreGateDirty,
    );
    let before = fixture.protected_bytes();
    let args = ["git-factor", "--continue", "--message", "test: message"].map(OsString::from);

    let code = main_entry_with_vec(fixture.ctx().io, Ok(fixture.ctx()), &args);

    assert_eq!(code, EXIT_SOFTWARE);
    assert_eq!(fixture.stdout(), "");
    assert_eq!(
        fixture.stderr(),
        "git command failed: continue gate requires staged changes only; remove unstaged or untracked changes first\nSTATUS:\n M file.txt\n"
    );
    assert_eq!(fixture.journal(), fixture.expected_journal());
    assert_eq!(fixture.protected_bytes(), before);
    assert_eq!(
        fixture.full_effect_requests(),
        fixture.expected_effect_requests()
    );
    assert!(
        !fixture
            .effect_requests()
            .iter()
            .any(|call| call.first().is_some_and(|bin| bin == "git")
                && call.get(1).is_some_and(|arg| arg == "commit"))
    );
}

#[test]
fn public_continue_preserves_selection_after_post_gate_dirty_work() {
    let fixture = super::continue_contracts::Continuation::new(
        &"a".repeat(SHA_LEN),
        0,
        super::continue_contracts::ContinueCase::PostGateDirty,
    );
    let before = fixture.protected_bytes();
    let args = ["git-factor", "--continue", "--message", "test: message"].map(OsString::from);

    let code = main_entry_with_vec(fixture.ctx().io, Ok(fixture.ctx()), &args);

    assert_eq!(code, EXIT_SOFTWARE);
    assert_eq!(fixture.stdout(), "");
    assert_eq!(
        fixture.stderr(),
        "git command failed: exec gate must not leave unstaged or untracked changes behind\nSTATUS:\n M file.txt\n"
    );
    assert_eq!(fixture.journal(), fixture.expected_journal());
    assert_eq!(fixture.protected_bytes(), before);
    assert_eq!(
        fixture.full_effect_requests(),
        fixture.expected_effect_requests()
    );
    assert!(
        !fixture
            .effect_requests()
            .iter()
            .any(|call| call.first().is_some_and(|bin| bin == "git")
                && call.get(1).is_some_and(|arg| arg == "commit"))
    );
}

#[test]
fn public_continue_replays_the_remainder() {
    let fixture = super::continue_contracts::Continuation::new(
        &"a".repeat(SHA_LEN),
        0,
        super::continue_contracts::ContinueCase::Remainder,
    );
    let before = fixture.protected_bytes();
    let args = ["git-factor", "--continue", "--message", "test: message"].map(OsString::from);

    let code = main_entry_with_vec(fixture.ctx().io, Ok(fixture.ctx()), &args);

    assert_eq!(code, EXIT_OK);
    assert_eq!(fixture.stdout(), fixture.expected_stdout());
    assert_eq!(fixture.stderr(), "");
    assert_eq!(fixture.journal(), fixture.expected_journal());
    assert_eq!(fixture.protected_bytes(), before);
    assert_eq!(
        fixture.full_effect_requests(),
        fixture.expected_effect_requests()
    );
}

#[test]
fn public_continue_reports_staged_status_failure() {
    use super::continue_contracts::{Continuation, ContinueFault};
    let fixture = Continuation::with_fault(&"a".repeat(SHA_LEN), ContinueFault::StagedStatus);
    let before = fixture.protected_bytes();
    let args = ["git-factor", "--continue", "--message", "test: message"].map(OsString::from);

    let code = main_entry_with_vec(fixture.ctx().io, Ok(fixture.ctx()), &args);

    assert_eq!(code, EXIT_SOFTWARE);
    assert_eq!(fixture.stdout(), "");
    assert_eq!(
        fixture.stderr(),
        Continuation::fault_stderr(ContinueFault::StagedStatus)
    );
    assert_eq!(fixture.journal(), fixture.expected_journal());
    assert_eq!(fixture.protected_bytes(), before);
    assert_eq!(
        fixture.full_effect_requests(),
        fixture.expected_effect_requests()
    );
}

#[test]
fn public_continue_reports_checkout_failure() {
    use super::continue_contracts::{Continuation, ContinueFault};
    let fixture = Continuation::with_fault(&"a".repeat(SHA_LEN), ContinueFault::Checkout);
    let before = fixture.protected_bytes();
    let args = ["git-factor", "--continue", "--message", "test: message"].map(OsString::from);

    let code = main_entry_with_vec(fixture.ctx().io, Ok(fixture.ctx()), &args);

    assert_eq!(code, EXIT_SOFTWARE);
    assert_eq!(fixture.stdout(), "");
    assert_eq!(
        fixture.stderr(),
        Continuation::fault_stderr(ContinueFault::Checkout)
    );
    assert_eq!(fixture.journal(), fixture.expected_journal());
    assert_eq!(fixture.protected_bytes(), before);
    assert_eq!(
        fixture.full_effect_requests(),
        fixture.expected_effect_requests()
    );
}

#[test]
fn public_continue_reports_clean_failure() {
    use super::continue_contracts::{Continuation, ContinueFault};
    let fixture = Continuation::with_fault(&"a".repeat(SHA_LEN), ContinueFault::Clean);
    let before = fixture.protected_bytes();
    let args = ["git-factor", "--continue", "--message", "test: message"].map(OsString::from);

    let code = main_entry_with_vec(fixture.ctx().io, Ok(fixture.ctx()), &args);

    assert_eq!(code, EXIT_SOFTWARE);
    assert_eq!(fixture.stdout(), "");
    assert_eq!(
        fixture.stderr(),
        Continuation::fault_stderr(ContinueFault::Clean)
    );
    assert_eq!(fixture.journal(), fixture.expected_journal());
    assert_eq!(fixture.protected_bytes(), before);
    assert_eq!(
        fixture.full_effect_requests(),
        fixture.expected_effect_requests()
    );
}

#[test]
fn public_continue_reports_checkout_index_failure() {
    use super::continue_contracts::{Continuation, ContinueFault};
    let fixture = Continuation::with_fault(&"a".repeat(SHA_LEN), ContinueFault::CheckoutIndex);
    let before = fixture.protected_bytes();
    let args = ["git-factor", "--continue", "--message", "test: message"].map(OsString::from);

    let code = main_entry_with_vec(fixture.ctx().io, Ok(fixture.ctx()), &args);

    assert_eq!(code, EXIT_SOFTWARE);
    assert_eq!(fixture.stdout(), "");
    assert_eq!(
        fixture.stderr(),
        Continuation::fault_stderr(ContinueFault::CheckoutIndex)
    );
    assert_eq!(fixture.journal(), fixture.expected_journal());
    assert_eq!(fixture.protected_bytes(), before);
    assert_eq!(
        fixture.full_effect_requests(),
        fixture.expected_effect_requests()
    );
}

#[test]
fn public_continue_reports_deleted_paths_failure() {
    use super::continue_contracts::{Continuation, ContinueFault};
    let fixture = Continuation::with_fault(&"a".repeat(SHA_LEN), ContinueFault::DeletedPaths);
    let before = fixture.protected_bytes();
    let args = ["git-factor", "--continue", "--message", "test: message"].map(OsString::from);

    let code = main_entry_with_vec(fixture.ctx().io, Ok(fixture.ctx()), &args);

    assert_eq!(code, EXIT_SOFTWARE);
    assert_eq!(fixture.stdout(), "");
    assert_eq!(
        fixture.stderr(),
        Continuation::fault_stderr(ContinueFault::DeletedPaths)
    );
    assert_eq!(fixture.journal(), fixture.expected_journal());
    assert_eq!(fixture.protected_bytes(), before);
    assert_eq!(
        fixture.full_effect_requests(),
        fixture.expected_effect_requests()
    );
}

#[test]
fn public_continue_reports_before_status_failure() {
    use super::continue_contracts::{Continuation, ContinueFault};
    let fixture = Continuation::with_fault(&"a".repeat(SHA_LEN), ContinueFault::BeforeStatus);
    let before = fixture.protected_bytes();
    let args = ["git-factor", "--continue", "--message", "test: message"].map(OsString::from);

    let code = main_entry_with_vec(fixture.ctx().io, Ok(fixture.ctx()), &args);

    assert_eq!(code, EXIT_SOFTWARE);
    assert_eq!(fixture.stdout(), "");
    assert_eq!(
        fixture.stderr(),
        Continuation::fault_stderr(ContinueFault::BeforeStatus)
    );
    assert_eq!(fixture.journal(), fixture.expected_journal());
    assert_eq!(fixture.protected_bytes(), before);
    assert_eq!(
        fixture.full_effect_requests(),
        fixture.expected_effect_requests()
    );
}

#[test]
fn public_continue_reports_after_status_failure() {
    use super::continue_contracts::{Continuation, ContinueFault};
    let fixture = Continuation::with_fault(&"a".repeat(SHA_LEN), ContinueFault::AfterStatus);
    let before = fixture.protected_bytes();
    let args = ["git-factor", "--continue", "--message", "test: message"].map(OsString::from);

    let code = main_entry_with_vec(fixture.ctx().io, Ok(fixture.ctx()), &args);

    assert_eq!(code, EXIT_SOFTWARE);
    assert_eq!(fixture.stdout(), "");
    assert_eq!(
        fixture.stderr(),
        Continuation::fault_stderr(ContinueFault::AfterStatus)
    );
    assert_eq!(fixture.journal(), fixture.expected_journal());
    assert_eq!(fixture.protected_bytes(), before);
    assert_eq!(
        fixture.full_effect_requests(),
        fixture.expected_effect_requests()
    );
}

#[test]
fn public_continue_reports_metadata_failure() {
    use super::continue_contracts::{Continuation, ContinueFault};
    let fixture = Continuation::with_fault(&"a".repeat(SHA_LEN), ContinueFault::Metadata);
    let before = fixture.protected_bytes();
    let args = ["git-factor", "--continue", "--message", "test: message"].map(OsString::from);

    let code = main_entry_with_vec(fixture.ctx().io, Ok(fixture.ctx()), &args);

    assert_eq!(code, EXIT_SOFTWARE);
    assert_eq!(fixture.stdout(), "");
    assert_eq!(
        fixture.stderr(),
        Continuation::fault_stderr(ContinueFault::Metadata)
    );
    assert_eq!(fixture.journal(), fixture.expected_journal());
    assert_eq!(fixture.protected_bytes(), before);
    assert_eq!(
        fixture.full_effect_requests(),
        fixture.expected_effect_requests()
    );
}

#[test]
fn public_continue_reports_commit_failure() {
    use super::continue_contracts::{Continuation, ContinueFault};
    let fixture = Continuation::with_fault(&"a".repeat(SHA_LEN), ContinueFault::Commit);
    let before = fixture.protected_bytes();
    let args = ["git-factor", "--continue", "--message", "test: message"].map(OsString::from);

    let code = main_entry_with_vec(fixture.ctx().io, Ok(fixture.ctx()), &args);

    assert_eq!(code, EXIT_SOFTWARE);
    assert_eq!(fixture.stdout(), "");
    assert_eq!(
        fixture.stderr(),
        Continuation::fault_stderr(ContinueFault::Commit)
    );
    assert_eq!(fixture.journal(), fixture.expected_journal());
    assert_eq!(fixture.protected_bytes(), before);
    assert_eq!(
        fixture.full_effect_requests(),
        fixture.expected_effect_requests()
    );
}

#[test]
fn public_continue_reports_head_tree_failure() {
    use super::continue_contracts::{Continuation, ContinueFault};
    let fixture = Continuation::with_fault(&"a".repeat(SHA_LEN), ContinueFault::HeadTree);
    let before = fixture.protected_bytes();
    let args = ["git-factor", "--continue", "--message", "test: message"].map(OsString::from);

    let code = main_entry_with_vec(fixture.ctx().io, Ok(fixture.ctx()), &args);

    assert_eq!(code, EXIT_SOFTWARE);
    assert_eq!(fixture.stdout(), "");
    assert_eq!(
        fixture.stderr(),
        Continuation::fault_stderr(ContinueFault::HeadTree)
    );
    assert_eq!(fixture.journal(), fixture.expected_journal());
    assert_eq!(fixture.protected_bytes(), before);
    assert_eq!(
        fixture.full_effect_requests(),
        fixture.expected_effect_requests()
    );
}

#[test]
fn public_continue_reports_restore_failure() {
    use super::continue_contracts::{Continuation, ContinueFault};
    let fixture = Continuation::with_fault(&"a".repeat(SHA_LEN), ContinueFault::Restore);
    let before = fixture.protected_bytes();
    let args = ["git-factor", "--continue", "--message", "test: message"].map(OsString::from);

    let code = main_entry_with_vec(fixture.ctx().io, Ok(fixture.ctx()), &args);

    assert_eq!(code, EXIT_SOFTWARE);
    assert_eq!(fixture.stdout(), "");
    assert_eq!(
        fixture.stderr(),
        Continuation::fault_stderr(ContinueFault::Restore)
    );
    assert_eq!(fixture.journal(), fixture.expected_journal());
    assert_eq!(fixture.protected_bytes(), before);
    assert_eq!(
        fixture.full_effect_requests(),
        fixture.expected_effect_requests()
    );
}

#[test]
fn public_continue_reports_restored_tree_failure() {
    use super::continue_contracts::{Continuation, ContinueFault};
    let fixture = Continuation::with_fault(&"a".repeat(SHA_LEN), ContinueFault::RestoredTree);
    let before = fixture.protected_bytes();
    let args = ["git-factor", "--continue", "--message", "test: message"].map(OsString::from);

    let code = main_entry_with_vec(fixture.ctx().io, Ok(fixture.ctx()), &args);

    assert_eq!(code, EXIT_SOFTWARE);
    assert_eq!(fixture.stdout(), "");
    assert_eq!(
        fixture.stderr(),
        Continuation::fault_stderr(ContinueFault::RestoredTree)
    );
    assert_eq!(fixture.journal(), fixture.expected_journal());
    assert_eq!(fixture.protected_bytes(), before);
    assert_eq!(
        fixture.full_effect_requests(),
        fixture.expected_effect_requests()
    );
}

#[test]
fn public_continue_reports_reset_failure() {
    use super::continue_contracts::{Continuation, ContinueFault};
    let fixture = Continuation::with_fault(&"a".repeat(SHA_LEN), ContinueFault::Reset);
    let before = fixture.protected_bytes();
    let args = ["git-factor", "--continue", "--message", "test: message"].map(OsString::from);

    let code = main_entry_with_vec(fixture.ctx().io, Ok(fixture.ctx()), &args);

    assert_eq!(code, EXIT_SOFTWARE);
    assert_eq!(fixture.stdout(), "");
    assert_eq!(
        fixture.stderr(),
        Continuation::fault_stderr(ContinueFault::Reset)
    );
    assert_eq!(fixture.journal(), fixture.expected_journal());
    assert_eq!(fixture.protected_bytes(), before);
    assert_eq!(
        fixture.full_effect_requests(),
        fixture.expected_effect_requests()
    );
}

#[test]
fn public_continue_reports_diff_stat_failure() {
    use super::continue_contracts::{Continuation, ContinueFault};
    let fixture = Continuation::with_fault(&"a".repeat(SHA_LEN), ContinueFault::DiffStat);
    let before = fixture.protected_bytes();
    let args = ["git-factor", "--continue", "--message", "test: message"].map(OsString::from);

    let code = main_entry_with_vec(fixture.ctx().io, Ok(fixture.ctx()), &args);

    assert_eq!(code, EXIT_SOFTWARE);
    assert_eq!(fixture.stdout(), "");
    assert_eq!(
        fixture.stderr(),
        Continuation::fault_stderr(ContinueFault::DiffStat)
    );
    assert_eq!(fixture.journal(), fixture.expected_journal());
    assert_eq!(fixture.protected_bytes(), before);
    assert_eq!(
        fixture.full_effect_requests(),
        fixture.expected_effect_requests()
    );
}

#[test]
fn public_continue_reports_untracked_failure() {
    use super::continue_contracts::{Continuation, ContinueFault};
    let fixture = Continuation::with_fault(&"a".repeat(SHA_LEN), ContinueFault::Untracked);
    let before = fixture.protected_bytes();
    let args = ["git-factor", "--continue", "--message", "test: message"].map(OsString::from);

    let code = main_entry_with_vec(fixture.ctx().io, Ok(fixture.ctx()), &args);

    assert_eq!(code, EXIT_SOFTWARE);
    assert_eq!(fixture.stdout(), "");
    assert_eq!(
        fixture.stderr(),
        Continuation::fault_stderr(ContinueFault::Untracked)
    );
    assert_eq!(fixture.journal(), fixture.expected_journal());
    assert_eq!(fixture.protected_bytes(), before);
    assert_eq!(
        fixture.full_effect_requests(),
        fixture.expected_effect_requests()
    );
}

#[test]
fn public_continue_refuses_absent() {
    use super::continue_contracts::{Continuation, ContinueState};
    let fixture = Continuation::with_state(&"a".repeat(SHA_LEN), ContinueState::Absent);
    let journal = fixture.journal();
    let bytes = fixture.protected_bytes();
    let args = ["git-factor", "--continue", "--message", "test: message"].map(OsString::from);

    let code = main_entry_with_vec(fixture.ctx().io, Ok(fixture.ctx()), &args);

    assert_eq!(code, EXIT_USAGE);
    assert_eq!(fixture.stdout(), "");
    assert_eq!(fixture.stderr(), "no active factor session\n");
    assert_eq!(fixture.journal(), journal);
    assert_eq!(fixture.protected_bytes(), bytes);
    assert_eq!(fixture.full_effect_requests(), Vec::new());
}

#[test]
fn public_continue_refuses_pending() {
    use super::continue_contracts::{Continuation, ContinueState};
    let fixture = Continuation::with_state(&"a".repeat(SHA_LEN), ContinueState::Pending);
    let journal = fixture.journal();
    let bytes = fixture.protected_bytes();
    let args = ["git-factor", "--continue", "--message", "test: message"].map(OsString::from);

    let code = main_entry_with_vec(fixture.ctx().io, Ok(fixture.ctx()), &args);

    assert_eq!(code, EXIT_USAGE);
    assert_eq!(fixture.stdout(), "");
    assert_eq!(
        fixture.stderr(),
        "run 'git factor --continue' with no --message to begin splitting this commit\n"
    );
    assert_eq!(fixture.journal(), journal);
    assert_eq!(fixture.protected_bytes(), bytes);
    assert_eq!(fixture.full_effect_requests(), Vec::new());
}

#[test]
fn public_continue_refuses_rebase_required() {
    use super::continue_contracts::{Continuation, ContinueState};
    let fixture = Continuation::with_state(&"a".repeat(SHA_LEN), ContinueState::RebaseRequired);
    let journal = fixture.journal();
    let bytes = fixture.protected_bytes();
    let args = ["git-factor", "--continue", "--message", "test: message"].map(OsString::from);

    let code = main_entry_with_vec(fixture.ctx().io, Ok(fixture.ctx()), &args);

    assert_eq!(code, EXIT_SOFTWARE);
    assert_eq!(fixture.stdout(), "");
    assert_eq!(
        fixture.stderr(),
        "git command failed: no rebase in progress\n"
    );
    assert_eq!(fixture.journal(), journal);
    assert_eq!(fixture.protected_bytes(), bytes);
    assert_eq!(fixture.full_effect_requests(), Vec::new());
}

#[test]
fn public_continue_refuses_index_outside_span() {
    use super::continue_contracts::{Continuation, ContinueState};
    let fixture = Continuation::with_state(&"a".repeat(SHA_LEN), ContinueState::IndexOutsideSpan);
    let journal = fixture.journal();
    let bytes = fixture.protected_bytes();
    let args = ["git-factor", "--continue", "--message", "test: message"].map(OsString::from);

    let code = main_entry_with_vec(fixture.ctx().io, Ok(fixture.ctx()), &args);

    assert_eq!(code, EXIT_SOFTWARE);
    assert_eq!(fixture.stdout(), "");
    assert_eq!(
        fixture.stderr(),
        "git command failed: commit index 1 out of range (have 1 commits)\n"
    );
    assert_eq!(fixture.journal(), journal);
    assert_eq!(fixture.protected_bytes(), bytes);
    assert_eq!(fixture.full_effect_requests(), Vec::new());
}

#[test]
fn public_continue_reports_counter_overflow_after_commit() {
    use super::continue_contracts::Continuation;
    let fixture = Continuation::with_counter_overflow(&"a".repeat(SHA_LEN));
    let journal = fixture.journal();
    let bytes = fixture.protected_bytes();
    let args = ["git-factor", "--continue", "--message", "test: message"].map(OsString::from);

    let code = main_entry_with_vec(fixture.ctx().io, Ok(fixture.ctx()), &args);

    assert_eq!(code, EXIT_SOFTWARE);
    assert_eq!(fixture.stdout(), "");
    assert_eq!(
        fixture.stderr(),
        "git command failed: split_count overflow\n"
    );
    assert_eq!(fixture.journal(), journal);
    assert_eq!(fixture.protected_bytes(), bytes);
    assert_eq!(
        fixture.full_effect_requests(),
        fixture.expected_effect_requests()
    );
    assert_eq!(
        fixture.effect_requests(),
        vec![
            vec!["git", "diff", "--quiet", "--staged"],
            vec!["git", "checkout", "--quiet", "--", "."],
            vec!["git", "clean", "--force", "--quiet", "-d"],
            vec!["git", "checkout-index", "--all", "--force", "--quiet"],
            vec!["bash", "-c", "true"],
            vec!["git", "commit", "--quiet", "--message", "test: message"],
        ]
        .into_iter()
        .map(|call| call.into_iter().map(str::to_owned).collect::<Vec<_>>())
        .collect::<Vec<_>>()
    );
}

#[test]
fn public_continue_reports_read_commits_state_failure() {
    use super::continue_contracts::{Continuation, ContinueFileFault};
    let fixture =
        Continuation::with_file_fault(&"a".repeat(SHA_LEN), ContinueFileFault::ReadCommits);
    let journal = fixture.journal();
    let bytes = fixture.protected_bytes();
    let args = ["git-factor", "--continue", "--message", "test: message"].map(OsString::from);

    let code = main_entry_with_vec(fixture.ctx().io, Ok(fixture.ctx()), &args);

    assert_eq!(code, EXIT_SOFTWARE);
    assert_eq!(fixture.stdout(), "");
    assert_eq!(
        fixture.stderr(),
        "failed to read state: continuation state read denied\n"
    );
    assert!(fixture.file_fault_observed());
    assert_eq!(fixture.journal(), journal);
    assert_eq!(fixture.protected_bytes(), bytes);
    assert_eq!(
        fixture.full_effect_requests(),
        fixture.expected_effect_requests()
    );
}

#[test]
fn public_continue_reports_read_phase_state_failure() {
    use super::continue_contracts::{Continuation, ContinueFileFault};
    let fixture = Continuation::with_file_fault(&"a".repeat(SHA_LEN), ContinueFileFault::ReadPhase);
    let journal = fixture.journal();
    let bytes = fixture.protected_bytes();
    let args = ["git-factor", "--continue", "--message", "test: message"].map(OsString::from);

    let code = main_entry_with_vec(fixture.ctx().io, Ok(fixture.ctx()), &args);

    assert_eq!(code, EXIT_SOFTWARE);
    assert_eq!(fixture.stdout(), "");
    assert_eq!(
        fixture.stderr(),
        "failed to read state: continuation state read denied\n"
    );
    assert!(fixture.file_fault_observed());
    assert_eq!(fixture.journal(), journal);
    assert_eq!(fixture.protected_bytes(), bytes);
    assert_eq!(
        fixture.full_effect_requests(),
        fixture.expected_effect_requests()
    );
}

#[test]
fn public_continue_reports_read_requires_state_failure() {
    use super::continue_contracts::{Continuation, ContinueFileFault};
    let fixture =
        Continuation::with_file_fault(&"a".repeat(SHA_LEN), ContinueFileFault::ReadRequires);
    let journal = fixture.journal();
    let bytes = fixture.protected_bytes();
    let args = ["git-factor", "--continue", "--message", "test: message"].map(OsString::from);

    let code = main_entry_with_vec(fixture.ctx().io, Ok(fixture.ctx()), &args);

    assert_eq!(code, EXIT_SOFTWARE);
    assert_eq!(fixture.stdout(), "");
    assert_eq!(
        fixture.stderr(),
        "failed to read state: continuation state read denied\n"
    );
    assert!(fixture.file_fault_observed());
    assert_eq!(fixture.journal(), journal);
    assert_eq!(fixture.protected_bytes(), bytes);
    assert_eq!(
        fixture.full_effect_requests(),
        fixture.expected_effect_requests()
    );
}

#[test]
fn public_continue_reports_read_index_state_failure() {
    use super::continue_contracts::{Continuation, ContinueFileFault};
    let fixture = Continuation::with_file_fault(&"a".repeat(SHA_LEN), ContinueFileFault::ReadIndex);
    let journal = fixture.journal();
    let bytes = fixture.protected_bytes();
    let args = ["git-factor", "--continue", "--message", "test: message"].map(OsString::from);

    let code = main_entry_with_vec(fixture.ctx().io, Ok(fixture.ctx()), &args);

    assert_eq!(code, EXIT_SOFTWARE);
    assert_eq!(fixture.stdout(), "");
    assert_eq!(
        fixture.stderr(),
        "failed to read state: continuation state read denied\n"
    );
    assert!(fixture.file_fault_observed());
    assert_eq!(fixture.journal(), journal);
    assert_eq!(fixture.protected_bytes(), bytes);
    assert_eq!(
        fixture.full_effect_requests(),
        fixture.expected_effect_requests()
    );
}

#[test]
fn public_continue_reports_read_exec_state_failure() {
    use super::continue_contracts::{Continuation, ContinueFileFault};
    let fixture = Continuation::with_file_fault(&"a".repeat(SHA_LEN), ContinueFileFault::ReadExec);
    let journal = fixture.journal();
    let bytes = fixture.protected_bytes();
    let args = ["git-factor", "--continue", "--message", "test: message"].map(OsString::from);

    let code = main_entry_with_vec(fixture.ctx().io, Ok(fixture.ctx()), &args);

    assert_eq!(code, EXIT_SOFTWARE);
    assert_eq!(fixture.stdout(), "");
    assert_eq!(
        fixture.stderr(),
        "failed to read state: continuation state read denied\n"
    );
    assert!(fixture.file_fault_observed());
    assert_eq!(fixture.journal(), journal);
    assert_eq!(fixture.protected_bytes(), bytes);
    assert_eq!(
        fixture.full_effect_requests(),
        fixture.expected_effect_requests()
    );
}

#[test]
fn public_continue_reports_read_count_state_failure() {
    use super::continue_contracts::{Continuation, ContinueFileFault};
    let fixture = Continuation::with_file_fault(&"a".repeat(SHA_LEN), ContinueFileFault::ReadCount);
    let journal = fixture.journal();
    let bytes = fixture.protected_bytes();
    let args = ["git-factor", "--continue", "--message", "test: message"].map(OsString::from);

    let code = main_entry_with_vec(fixture.ctx().io, Ok(fixture.ctx()), &args);

    assert_eq!(code, EXIT_SOFTWARE);
    assert_eq!(fixture.stdout(), "");
    assert_eq!(
        fixture.stderr(),
        "failed to read state: continuation state read denied\n"
    );
    assert!(fixture.file_fault_observed());
    assert_eq!(fixture.journal(), journal);
    assert_eq!(fixture.protected_bytes(), bytes);
    assert_eq!(
        fixture.full_effect_requests(),
        fixture.expected_effect_requests()
    );
}

#[test]
fn public_continue_reports_read_expected_state_failure() {
    use super::continue_contracts::{Continuation, ContinueFileFault};
    let fixture =
        Continuation::with_file_fault(&"a".repeat(SHA_LEN), ContinueFileFault::ReadExpected);
    let mut journal = fixture.journal();
    journal.insert("split_count".to_owned(), "1\n".to_owned());
    let bytes = fixture.protected_bytes();
    let args = ["git-factor", "--continue", "--message", "test: message"].map(OsString::from);

    let code = main_entry_with_vec(fixture.ctx().io, Ok(fixture.ctx()), &args);

    assert_eq!(code, EXIT_SOFTWARE);
    assert_eq!(fixture.stdout(), "");
    assert_eq!(
        fixture.stderr(),
        "git command failed: expected tree fallback denied\n"
    );
    assert!(fixture.file_fault_observed());
    assert_eq!(fixture.journal(), journal);
    assert_eq!(fixture.protected_bytes(), bytes);
    assert_eq!(
        fixture.full_effect_requests(),
        fixture.expected_effect_requests()
    );
}

#[test]
fn public_continue_reports_write_count_state_failure() {
    use super::continue_contracts::{Continuation, ContinueFileFault};
    let fixture =
        Continuation::with_file_fault(&"a".repeat(SHA_LEN), ContinueFileFault::WriteCount);
    let journal = fixture.journal();
    let bytes = fixture.protected_bytes();
    let args = ["git-factor", "--continue", "--message", "test: message"].map(OsString::from);

    let code = main_entry_with_vec(fixture.ctx().io, Ok(fixture.ctx()), &args);

    assert_eq!(code, EXIT_SOFTWARE);
    assert_eq!(fixture.stdout(), "");
    assert_eq!(
        fixture.stderr(),
        "failed to write state: continuation state write denied\n"
    );
    assert!(fixture.file_fault_observed());
    assert_eq!(fixture.journal(), journal);
    assert_eq!(fixture.protected_bytes(), bytes);
    assert_eq!(
        fixture.full_effect_requests(),
        fixture.expected_effect_requests()
    );
}

#[test]
fn public_continue_reports_complete_write_failure() {
    use super::continue_contracts::{Continuation, ContinueWrite};
    let fixture = Continuation::with_write(&"a".repeat(SHA_LEN), ContinueWrite::Complete);
    let bytes = fixture.protected_bytes();
    let args = ["git-factor", "--continue", "--message", "test: message"].map(OsString::from);

    let code = main_entry_with_vec(fixture.ctx().io, Ok(fixture.ctx()), &args);

    assert_eq!(code, EXIT_SOFTWARE);
    assert_eq!(fixture.stdout(), ContinueWrite::Complete.prefix());
    assert_eq!(
        fixture.stderr(),
        "failed to write output: continuation output denied\n"
    );
    assert!(fixture.write_observed());
    assert_eq!(fixture.journal(), fixture.expected_journal());
    assert_eq!(fixture.protected_bytes(), bytes);
    assert_eq!(
        fixture.full_effect_requests(),
        fixture.expected_effect_requests()
    );
    assert!(!fixture.session_active());
}

#[test]
fn public_continue_reports_gate_banner_write_failure() {
    use super::continue_contracts::{Continuation, ContinueWrite};
    let fixture = Continuation::with_write(&"a".repeat(SHA_LEN), ContinueWrite::GateBanner);
    let bytes = fixture.protected_bytes();
    let args = ["git-factor", "--continue", "--message", "test: message"].map(OsString::from);

    let code = main_entry_with_vec(fixture.ctx().io, Ok(fixture.ctx()), &args);

    assert_eq!(code, EXIT_SOFTWARE);
    assert_eq!(fixture.stdout(), ContinueWrite::GateBanner.prefix());
    assert_eq!(
        fixture.stderr(),
        "failed to write output: continuation output denied\n"
    );
    assert!(fixture.write_observed());
    assert_eq!(fixture.journal(), fixture.expected_journal());
    assert_eq!(fixture.protected_bytes(), bytes);
    assert_eq!(
        fixture.full_effect_requests(),
        fixture.expected_effect_requests()
    );
}

#[test]
fn public_continue_reports_gate_command_write_failure() {
    use super::continue_contracts::{Continuation, ContinueWrite};
    let fixture = Continuation::with_write(&"a".repeat(SHA_LEN), ContinueWrite::GateCommand);
    let bytes = fixture.protected_bytes();
    let args = ["git-factor", "--continue", "--message", "test: message"].map(OsString::from);

    let code = main_entry_with_vec(fixture.ctx().io, Ok(fixture.ctx()), &args);

    assert_eq!(code, EXIT_SOFTWARE);
    assert_eq!(fixture.stdout(), ContinueWrite::GateCommand.prefix());
    assert_eq!(
        fixture.stderr(),
        "failed to write output: continuation output denied\n"
    );
    assert!(fixture.write_observed());
    assert_eq!(fixture.journal(), fixture.expected_journal());
    assert_eq!(fixture.protected_bytes(), bytes);
    assert_eq!(
        fixture.full_effect_requests(),
        fixture.expected_effect_requests()
    );
}

#[test]
fn public_continue_reports_gate_code_write_failure() {
    use super::continue_contracts::{Continuation, ContinueWrite};
    let fixture = Continuation::with_write(&"a".repeat(SHA_LEN), ContinueWrite::GateCode);
    let bytes = fixture.protected_bytes();
    let args = ["git-factor", "--continue", "--message", "test: message"].map(OsString::from);

    let code = main_entry_with_vec(fixture.ctx().io, Ok(fixture.ctx()), &args);

    assert_eq!(code, EXIT_SOFTWARE);
    assert_eq!(fixture.stdout(), ContinueWrite::GateCode.prefix());
    assert_eq!(
        fixture.stderr(),
        "failed to write output: continuation output denied\n"
    );
    assert!(fixture.write_observed());
    assert_eq!(fixture.journal(), fixture.expected_journal());
    assert_eq!(fixture.protected_bytes(), bytes);
    assert_eq!(
        fixture.full_effect_requests(),
        fixture.expected_effect_requests()
    );
}

#[test]
fn public_continue_reports_gate_separator_write_failure() {
    use super::continue_contracts::{Continuation, ContinueWrite};
    let fixture = Continuation::with_write(&"a".repeat(SHA_LEN), ContinueWrite::GateSeparator);
    let bytes = fixture.protected_bytes();
    let args = ["git-factor", "--continue", "--message", "test: message"].map(OsString::from);

    let code = main_entry_with_vec(fixture.ctx().io, Ok(fixture.ctx()), &args);

    assert_eq!(code, EXIT_SOFTWARE);
    assert_eq!(fixture.stdout(), ContinueWrite::GateSeparator.prefix());
    assert_eq!(
        fixture.stderr(),
        "failed to write output: continuation output denied\n"
    );
    assert!(fixture.write_observed());
    assert_eq!(fixture.journal(), fixture.expected_journal());
    assert_eq!(fixture.protected_bytes(), bytes);
    assert_eq!(
        fixture.full_effect_requests(),
        fixture.expected_effect_requests()
    );
}

#[test]
fn public_continue_reports_gate_next_write_failure() {
    use super::continue_contracts::{Continuation, ContinueWrite};
    let fixture = Continuation::with_write(&"a".repeat(SHA_LEN), ContinueWrite::GateNext);
    let bytes = fixture.protected_bytes();
    let args = ["git-factor", "--continue", "--message", "test: message"].map(OsString::from);

    let code = main_entry_with_vec(fixture.ctx().io, Ok(fixture.ctx()), &args);

    assert_eq!(code, EXIT_SOFTWARE);
    assert_eq!(fixture.stdout(), ContinueWrite::GateNext.prefix());
    assert_eq!(
        fixture.stderr(),
        "failed to write output: continuation output denied\n"
    );
    assert!(fixture.write_observed());
    assert_eq!(fixture.journal(), fixture.expected_journal());
    assert_eq!(fixture.protected_bytes(), bytes);
    assert_eq!(
        fixture.full_effect_requests(),
        fixture.expected_effect_requests()
    );
}

#[test]
fn public_continue_reports_gate_usage_write_failure() {
    use super::continue_contracts::{Continuation, ContinueWrite};
    let fixture = Continuation::with_write(&"a".repeat(SHA_LEN), ContinueWrite::GateUsage);
    let bytes = fixture.protected_bytes();
    let args = ["git-factor", "--continue", "--message", "test: message"].map(OsString::from);

    let code = main_entry_with_vec(fixture.ctx().io, Ok(fixture.ctx()), &args);

    assert_eq!(code, EXIT_SOFTWARE);
    assert_eq!(fixture.stdout(), ContinueWrite::GateUsage.prefix());
    assert_eq!(
        fixture.stderr(),
        "failed to write output: continuation output denied\n"
    );
    assert!(fixture.write_observed());
    assert_eq!(fixture.journal(), fixture.expected_journal());
    assert_eq!(fixture.protected_bytes(), bytes);
    assert_eq!(
        fixture.full_effect_requests(),
        fixture.expected_effect_requests()
    );
}

#[test]
fn public_continue_reports_remainder_write_failure() {
    use super::continue_contracts::{Continuation, ContinueWrite};
    let fixture = Continuation::with_write(&"a".repeat(SHA_LEN), ContinueWrite::Remainder);
    let bytes = fixture.protected_bytes();
    let args = ["git-factor", "--continue", "--message", "test: message"].map(OsString::from);

    let code = main_entry_with_vec(fixture.ctx().io, Ok(fixture.ctx()), &args);

    assert_eq!(code, EXIT_SOFTWARE);
    assert_eq!(fixture.stdout(), ContinueWrite::Remainder.prefix());
    assert_eq!(
        fixture.stderr(),
        "failed to write output: continuation output denied\n"
    );
    assert!(fixture.write_observed());
    assert_eq!(fixture.journal(), fixture.expected_journal());
    assert_eq!(fixture.protected_bytes(), bytes);
    assert_eq!(
        fixture.full_effect_requests(),
        fixture.expected_effect_requests()
    );
}

#[test]
fn public_continue_applies_staged_deletions_before_the_gate() {
    use super::continue_contracts::Continuation;
    let fixture = Continuation::with_deleted(&"a".repeat(SHA_LEN), 0);
    let bytes = fixture.protected_bytes();
    assert!(fixture.deleted_exists());
    let args = ["git-factor", "--continue", "--message", "test: message"].map(OsString::from);

    let code = main_entry_with_vec(fixture.ctx().io, Ok(fixture.ctx()), &args);

    assert_eq!(code, EXIT_OK);
    assert_eq!(
        fixture.stdout(),
        "FACTOR: Complete. Final commit split into 1 commits.\n"
    );
    assert_eq!(fixture.stderr(), "");
    assert!(!fixture.deleted_exists());
    assert_eq!(fixture.journal(), fixture.expected_journal());
    assert_eq!(fixture.protected_bytes(), bytes);
    assert_eq!(
        fixture.full_effect_requests(),
        fixture.expected_effect_requests()
    );
    assert!(!fixture.session_active());
    assert_eq!(
        fixture.deleted_requests(),
        vec![PathBuf::from("deleted.txt")]
    );
}

#[test]
fn public_continue_reports_read_count_after_commit_state_failure() {
    use super::continue_contracts::{Continuation, ContinueFileFault};
    let fixture = Continuation::with_file_fault(
        &"a".repeat(SHA_LEN),
        ContinueFileFault::ReadCountAfterCommit,
    );
    let mut journal = fixture.journal();
    journal.insert("split_count".to_owned(), "1\n".to_owned());
    let bytes = fixture.protected_bytes();
    let args = ["git-factor", "--continue", "--message", "test: message"].map(OsString::from);

    let code = main_entry_with_vec(fixture.ctx().io, Ok(fixture.ctx()), &args);

    assert_eq!(code, EXIT_SOFTWARE);
    assert_eq!(fixture.stdout(), "");
    assert_eq!(
        fixture.stderr(),
        "failed to read state: continuation state read denied\n"
    );
    assert!(fixture.file_fault_observed());
    assert_eq!(fixture.journal(), journal);
    assert_eq!(fixture.protected_bytes(), bytes);
    assert_eq!(
        fixture.full_effect_requests(),
        fixture.expected_effect_requests()
    );
}

#[test]
fn public_continue_reports_active_session_directory_query_failure() {
    use super::continue_contracts::{Continuation, ContinueFault};
    let fixture = Continuation::with_fault(&"a".repeat(SHA_LEN), ContinueFault::SessionDir);
    let journal = fixture.journal();
    let bytes = fixture.protected_bytes();
    let args = ["git-factor", "--continue", "--message", "test: message"].map(OsString::from);

    let code = main_entry_with_vec(fixture.ctx().io, Ok(fixture.ctx()), &args);

    assert_eq!(code, EXIT_DATAERR);
    assert_eq!(fixture.stdout(), "");
    assert_eq!(
        fixture.stderr(),
        "failed to determine git directory: continuation query denied\n"
    );
    assert_eq!(fixture.journal(), journal);
    assert_eq!(fixture.protected_bytes(), bytes);
    assert_eq!(
        fixture.full_effect_requests(),
        fixture.expected_effect_requests()
    );
}

#[test]
fn public_continue_prioritizes_failed_rehydration_after_gate_rejected() {
    use super::continue_contracts::{Continuation, ContinueRecoveryFailure};
    let fixture = Continuation::with_recovery_failure(
        &"a".repeat(SHA_LEN),
        ContinueRecoveryFailure::GateRejected,
    );
    let journal = fixture.journal();
    let bytes = fixture.protected_bytes();
    let args = ["git-factor", "--continue", "--message", "test: message"].map(OsString::from);

    let code = main_entry_with_vec(fixture.ctx().io, Ok(fixture.ctx()), &args);

    assert_eq!(code, EXIT_SOFTWARE);
    assert_eq!(fixture.stdout(), "");
    assert_eq!(
        fixture.stderr(),
        "git command failed: git write-tree: continuation query denied\n"
    );
    assert_eq!(fixture.journal(), journal);
    assert_eq!(fixture.protected_bytes(), bytes);
    assert_eq!(
        fixture.full_effect_requests(),
        fixture.expected_effect_requests()
    );
}

#[test]
fn public_continue_prioritizes_failed_rehydration_after_gate_spawn() {
    use super::continue_contracts::{Continuation, ContinueRecoveryFailure};
    let fixture = Continuation::with_recovery_failure(
        &"a".repeat(SHA_LEN),
        ContinueRecoveryFailure::GateSpawn,
    );
    let journal = fixture.journal();
    let bytes = fixture.protected_bytes();
    let args = ["git-factor", "--continue", "--message", "test: message"].map(OsString::from);

    let code = main_entry_with_vec(fixture.ctx().io, Ok(fixture.ctx()), &args);

    assert_eq!(code, EXIT_SOFTWARE);
    assert_eq!(fixture.stdout(), "");
    assert_eq!(
        fixture.stderr(),
        "git command failed: git write-tree: continuation query denied\n"
    );
    assert_eq!(fixture.journal(), journal);
    assert_eq!(fixture.protected_bytes(), bytes);
    assert_eq!(
        fixture.full_effect_requests(),
        fixture.expected_effect_requests()
    );
}

#[test]
fn public_continue_prioritizes_failed_rehydration_after_post_gate_dirty() {
    use super::continue_contracts::{Continuation, ContinueRecoveryFailure};
    let fixture = Continuation::with_recovery_failure(
        &"a".repeat(SHA_LEN),
        ContinueRecoveryFailure::PostGateDirty,
    );
    let journal = fixture.journal();
    let bytes = fixture.protected_bytes();
    let args = ["git-factor", "--continue", "--message", "test: message"].map(OsString::from);

    let code = main_entry_with_vec(fixture.ctx().io, Ok(fixture.ctx()), &args);

    assert_eq!(code, EXIT_SOFTWARE);
    assert_eq!(fixture.stdout(), "");
    assert_eq!(
        fixture.stderr(),
        "git command failed: git write-tree: continuation query denied\n"
    );
    assert_eq!(fixture.journal(), journal);
    assert_eq!(fixture.protected_bytes(), bytes);
    assert_eq!(
        fixture.full_effect_requests(),
        fixture.expected_effect_requests()
    );
}

#[test]
fn public_continue_rejects_malformed_head_tree_after_commit() {
    use super::continue_contracts::Continuation;
    let fixture = Continuation::with_invalid_tree(&"a".repeat(SHA_LEN), "not-a-tree", false);
    let mut journal = fixture.journal();
    journal.insert("split_count".to_owned(), "1\n".to_owned());
    let bytes = fixture.protected_bytes();
    let args = ["git-factor", "--continue", "--message", "test: message"].map(OsString::from);

    let code = main_entry_with_vec(fixture.ctx().io, Ok(fixture.ctx()), &args);

    assert_eq!(code, EXIT_SOFTWARE);
    assert_eq!(fixture.stdout(), "");
    assert_eq!(
        fixture.stderr(),
        "git command failed: invalid tree hash: 'not-a-tree'\n"
    );
    assert_eq!(fixture.journal(), journal);
    assert_eq!(fixture.protected_bytes(), bytes);
    assert_eq!(
        fixture.full_effect_requests(),
        fixture.expected_effect_requests()
    );
}

#[test]
fn public_continue_rejects_malformed_expected_tree_after_commit() {
    use super::continue_contracts::Continuation;
    let fixture = Continuation::with_invalid_tree(&"a".repeat(SHA_LEN), "not-a-tree", true);
    let mut journal = fixture.journal();
    journal.insert("split_count".to_owned(), "1\n".to_owned());
    let bytes = fixture.protected_bytes();
    let args = ["git-factor", "--continue", "--message", "test: message"].map(OsString::from);

    let code = main_entry_with_vec(fixture.ctx().io, Ok(fixture.ctx()), &args);

    assert_eq!(code, EXIT_SOFTWARE);
    assert_eq!(fixture.stdout(), "");
    assert_eq!(
        fixture.stderr(),
        "git command failed: invalid tree hash: 'not-a-tree'\n"
    );
    assert_eq!(fixture.journal(), journal);
    assert_eq!(fixture.protected_bytes(), bytes);
    assert_eq!(
        fixture.full_effect_requests(),
        fixture.expected_effect_requests()
    );
}

#[test]
fn public_continue_admits_required_active_rebase_before_no_staged_refusal() {
    use crate::git_factor::tests::continue_contracts::Continuation;

    let fixture = Continuation::with_active_rebase(&"a".repeat(SHA_LEN), true);
    let before_journal = fixture.journal();
    let before_bytes = fixture.protected_bytes();
    let args = ["git-factor", "--continue", "--message", "test: message"].map(OsString::from);

    let code = main_entry_with_vec(fixture.ctx().io, Ok(fixture.ctx()), &args);

    assert_eq!(code, EXIT_USAGE);
    assert_eq!(fixture.stdout(), "");
    assert_eq!(
        fixture.stderr(),
        concat!(
            "no staged changes to commit\n",
            "NEXT: stage exactly one atomic change, then rerun:\n",
            "  git factor --continue --message \"type: description\"\n",
        )
    );
    assert_eq!(fixture.journal(), before_journal);
    assert_eq!(fixture.protected_bytes(), before_bytes);
    assert_eq!(
        fixture.full_effect_requests(),
        fixture.expected_effect_requests()
    );
}

#[test]
fn public_continue_completes_the_last_admitted_split_count() {
    let fixture = super::continue_contracts::Continuation::new(
        &"a".repeat(SHA_LEN),
        254,
        super::continue_contracts::ContinueCase::Complete,
    );
    let before = fixture.protected_bytes();
    let args = ["git-factor", "--continue", "--message", "test: message"].map(OsString::from);

    let code = main_entry_with_vec(fixture.ctx().io, Ok(fixture.ctx()), &args);

    assert_eq!(code, EXIT_OK);
    assert_eq!(
        fixture.stdout(),
        "FACTOR: Complete. Final commit split into 255 commits.\n"
    );
    assert_eq!(fixture.stderr(), "");
    assert_eq!(fixture.journal(), fixture.expected_journal());
    assert_eq!(fixture.protected_bytes(), before);
    assert_eq!(
        fixture.full_effect_requests(),
        fixture.expected_effect_requests()
    );
    assert_eq!(
        fixture.effect_requests(),
        vec![
            vec!["git", "diff", "--quiet", "--staged"],
            vec!["git", "checkout", "--quiet", "--", "."],
            vec!["git", "clean", "--force", "--quiet", "-d"],
            vec!["git", "checkout-index", "--all", "--force", "--quiet"],
            vec!["bash", "-c", "true"],
            vec!["git", "commit", "--quiet", "--message", "test: message"],
        ]
        .into_iter()
        .map(|call| call.into_iter().map(str::to_owned).collect::<Vec<_>>())
        .collect::<Vec<_>>()
    );
    assert!(!fixture.session_active());
}

#[test]
fn public_continue_rejects_short_hex_head_tree_after_commit() {
    use super::continue_contracts::Continuation;
    let fixture = Continuation::with_invalid_tree(
        &"a".repeat(SHA_LEN),
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        false,
    );
    let mut journal = fixture.journal();
    journal.insert("split_count".to_owned(), "1\n".to_owned());
    let bytes = fixture.protected_bytes();
    let args = ["git-factor", "--continue", "--message", "test: message"].map(OsString::from);

    let code = main_entry_with_vec(fixture.ctx().io, Ok(fixture.ctx()), &args);

    assert_eq!(code, EXIT_SOFTWARE);
    assert_eq!(fixture.stdout(), "");
    assert_eq!(
        fixture.stderr(),
        "git command failed: invalid tree hash: 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'\n"
    );
    assert_eq!(fixture.journal(), journal);
    assert_eq!(fixture.protected_bytes(), bytes);
    assert_eq!(
        fixture.full_effect_requests(),
        fixture.expected_effect_requests()
    );
}

#[test]
fn public_continue_rejects_long_hex_head_tree_after_commit() {
    use super::continue_contracts::Continuation;
    let fixture = Continuation::with_invalid_tree(
        &"a".repeat(SHA_LEN),
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        false,
    );
    let mut journal = fixture.journal();
    journal.insert("split_count".to_owned(), "1\n".to_owned());
    let bytes = fixture.protected_bytes();
    let args = ["git-factor", "--continue", "--message", "test: message"].map(OsString::from);

    let code = main_entry_with_vec(fixture.ctx().io, Ok(fixture.ctx()), &args);

    assert_eq!(code, EXIT_SOFTWARE);
    assert_eq!(fixture.stdout(), "");
    assert_eq!(
        fixture.stderr(),
        "git command failed: invalid tree hash: 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'\n"
    );
    assert_eq!(fixture.journal(), journal);
    assert_eq!(fixture.protected_bytes(), bytes);
    assert_eq!(
        fixture.full_effect_requests(),
        fixture.expected_effect_requests()
    );
}

#[test]
fn public_continue_rejects_forty_byte_nonhex_head_tree_after_commit() {
    use super::continue_contracts::Continuation;
    let fixture = Continuation::with_invalid_tree(
        &"a".repeat(SHA_LEN),
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaag",
        false,
    );
    let mut journal = fixture.journal();
    journal.insert("split_count".to_owned(), "1\n".to_owned());
    let bytes = fixture.protected_bytes();
    let args = ["git-factor", "--continue", "--message", "test: message"].map(OsString::from);

    let code = main_entry_with_vec(fixture.ctx().io, Ok(fixture.ctx()), &args);

    assert_eq!(code, EXIT_SOFTWARE);
    assert_eq!(fixture.stdout(), "");
    assert_eq!(
        fixture.stderr(),
        "git command failed: invalid tree hash: 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaag'\n"
    );
    assert_eq!(fixture.journal(), journal);
    assert_eq!(fixture.protected_bytes(), bytes);
    assert_eq!(
        fixture.full_effect_requests(),
        fixture.expected_effect_requests()
    );
}

#[test]
fn public_continue_reports_the_smallest_native_gate_rejection_code() {
    use super::continue_contracts::Continuation;
    let fixture = Continuation::with_gate_code(&"a".repeat(SHA_LEN), 1);
    let journal = fixture.journal();
    let bytes = fixture.protected_bytes();
    let args = ["git-factor", "--continue", "--message", "test: message"].map(OsString::from);

    let code = main_entry_with_vec(fixture.ctx().io, Ok(fixture.ctx()), &args);

    assert_eq!(code, EXIT_TEMPFAIL);
    assert_eq!(
        fixture.stdout(),
        concat!(
            "FACTOR: Exec gate failed. No commit created.\nEXEC: true\nCODE: 1\n\n",
            "NEXT: Adjust staged changes so the exec gate passes, then retry:\n",
            "  git factor --continue --message \"type: description\"\n",
        )
    );
    assert_eq!(fixture.stderr(), "exec gate failed: true (exit code 1)\n");
    assert_eq!(fixture.journal(), journal);
    assert_eq!(fixture.protected_bytes(), bytes);
    assert_eq!(
        fixture.full_effect_requests(),
        fixture.expected_effect_requests()
    );
}

#[test]
fn public_continue_reports_the_largest_native_gate_rejection_code() {
    use super::continue_contracts::Continuation;
    let fixture = Continuation::with_gate_code(&"a".repeat(SHA_LEN), 255);
    let journal = fixture.journal();
    let bytes = fixture.protected_bytes();
    let args = ["git-factor", "--continue", "--message", "test: message"].map(OsString::from);

    let code = main_entry_with_vec(fixture.ctx().io, Ok(fixture.ctx()), &args);

    assert_eq!(code, EXIT_TEMPFAIL);
    assert_eq!(
        fixture.stdout(),
        concat!(
            "FACTOR: Exec gate failed. No commit created.\nEXEC: true\nCODE: 255\n\n",
            "NEXT: Adjust staged changes so the exec gate passes, then retry:\n",
            "  git factor --continue --message \"type: description\"\n",
        )
    );
    assert_eq!(fixture.stderr(), "exec gate failed: true (exit code 255)\n");
    assert_eq!(fixture.journal(), journal);
    assert_eq!(fixture.protected_bytes(), bytes);
    assert_eq!(
        fixture.full_effect_requests(),
        fixture.expected_effect_requests()
    );
}

#[test]
fn public_continue_without_message_refuses_an_active_split() {
    use super::continue_contracts::{Continuation, ContinueCase};
    let fixture = Continuation::new(&"a".repeat(SHA_LEN), 0, ContinueCase::Remainder);
    let journal = fixture.journal();
    let bytes = fixture.protected_bytes();
    let args = ["git-factor", "--continue"].map(OsString::from);

    let code = main_entry_with_vec(fixture.ctx().io, Ok(fixture.ctx()), &args);

    assert_eq!(code, EXIT_USAGE);
    assert_eq!(fixture.stdout(), "");
    assert_eq!(fixture.stderr(), "--continue requires --message <MSG>\n");
    assert_eq!(fixture.journal(), journal);
    assert_eq!(fixture.protected_bytes(), bytes);
    assert_eq!(fixture.effect_requests(), Vec::<Vec<String>>::new());
}

#[test]
fn public_continue_without_message_admits_pending_start_before_cleanliness_refusal() {
    use super::continue_contracts::{Continuation, ContinueState};
    let fixture = Continuation::with_state(&"a".repeat(SHA_LEN), ContinueState::Pending);
    let journal = fixture.journal();
    let bytes = fixture.protected_bytes();
    let args = ["git-factor", "--continue"].map(OsString::from);

    let code = main_entry_with_vec(fixture.ctx().io, Ok(fixture.ctx()), &args);

    assert_eq!(code, EXIT_SOFTWARE);
    assert_eq!(fixture.stdout(), "");
    assert_eq!(
        fixture.stderr(),
        concat!(
            "git command failed: baseline commit must be fully clean before opening the split session\n",
            "STATUS:\nM  file.txt\n",
        )
    );
    assert_eq!(fixture.journal(), journal);
    assert_eq!(fixture.protected_bytes(), bytes);
    assert_eq!(fixture.effect_requests(), Vec::<Vec<String>>::new());
}

#[test]
fn abort_status() {
    refusal(
        &["--abort", "--status"],
        "--abort cannot be combined with other options",
    );
}

#[test]
fn abort_continue() {
    refusal(
        &["--abort", "--continue"],
        "--abort cannot be combined with other options",
    );
}

#[test]
fn abort_retry() {
    refusal(
        &["--abort", "--retry"],
        "--abort cannot be combined with other options",
    );
}

#[test]
fn abort_finish() {
    refusal(
        &["--abort", "--finish"],
        "--abort cannot be combined with other options",
    );
}

#[test]
fn abort_exec() {
    refusal(
        &["--abort", "--exec", "true"],
        "--abort cannot be combined with other options",
    );
}

#[test]
fn abort_head() {
    refusal(
        &["--abort", "HEAD"],
        "--abort cannot be combined with other options",
    );
}

#[test]
fn status_continue() {
    refusal(
        &["--status", "--continue"],
        "--status cannot be combined with other options",
    );
}

#[test]
fn status_retry() {
    refusal(
        &["--status", "--retry"],
        "--status cannot be combined with other options",
    );
}

#[test]
fn status_finish() {
    refusal(
        &["--status", "--finish"],
        "--status cannot be combined with other options",
    );
}

#[test]
fn status_exec() {
    refusal(
        &["--status", "--exec", "true"],
        "--status cannot be combined with other options",
    );
}

#[test]
fn status_head() {
    refusal(
        &["--status", "HEAD"],
        "--status cannot be combined with other options",
    );
}

#[test]
fn status_message() {
    refusal(
        &["--status", "--message", "true"],
        "--status cannot be combined with other options",
    );
}

#[test]
fn retry_continue() {
    refusal(
        &["--retry", "--continue"],
        "--retry cannot be combined with other options",
    );
}

#[test]
fn retry_finish() {
    refusal(
        &["--retry", "--finish"],
        "--retry cannot be combined with other options",
    );
}

#[test]
fn retry_exec() {
    refusal(
        &["--retry", "--exec", "true"],
        "--retry cannot be combined with other options",
    );
}

#[test]
fn retry_head() {
    refusal(
        &["--retry", "HEAD"],
        "--retry cannot be combined with other options",
    );
}

#[test]
fn retry_message() {
    refusal(
        &["--retry", "--message", "true"],
        "--retry cannot be combined with other options",
    );
}

#[test]
fn finish_continue() {
    refusal(
        &["--finish", "--continue"],
        "--finish cannot be combined with --continue, --exec, or COMMIT",
    );
}

#[test]
fn finish_exec() {
    refusal(
        &["--finish", "--exec", "true"],
        "--finish cannot be combined with --continue, --exec, or COMMIT",
    );
}

#[test]
fn finish_head() {
    refusal(
        &["--finish", "HEAD"],
        "--finish cannot be combined with --continue, --exec, or COMMIT",
    );
}

#[test]
fn continue_exec() {
    refusal(
        &["--continue", "--exec", "true"],
        "--continue cannot be combined with --exec or COMMIT",
    );
}

#[test]
fn continue_head() {
    refusal(
        &["--continue", "HEAD"],
        "--continue cannot be combined with --exec or COMMIT",
    );
}

#[test]
fn missing_exec() {
    refusal(
        &["HEAD"],
        "--exec <COMMAND> is required when starting a factor session",
    );
}

#[test]
fn message_without_operation() {
    refusal(
        &["--exec", "true", "--message", "Selected atom"],
        "--message can only be used with --continue or --finish",
    );
}

#[test]
fn message_without_exec() {
    refusal(
        &["--message", "Selected atom"],
        "--exec <COMMAND> is required when starting a factor session",
    );
}

#[test]
fn preflight_arity() {
    refusal(
        &["rebase-exec-preflight"],
        "rebase-exec-preflight requires exactly two arguments: current-index and exec-command",
    );
}

#[test]
fn begin_arity() {
    refusal(
        &["rebase-exec-begin"],
        "rebase-exec-begin requires exactly five arguments: current-index, start-head, is-root, exec-command, and commits",
    );
}

#[test]
fn version_stream_is_exact() {
    parser(&["--version"], EXIT_OK, "git-factor 0.1.0\n", "");
}

#[test]
fn invalid_flag_stream_is_exact() {
    parser(
        &["--not-real"],
        EXIT_USAGE,
        "",
        "error: unexpected argument '--not-real' found\n\n  tip: to pass '--not-real' as a value, use '-- --not-real'\n\nUsage: git-factor [OPTIONS] [COMMIT]...\n\nFor more information, try '--help'.\n",
    );
}

#[test]
fn help_stream_is_exact() {
    parser(
        &["--help"],
        EXIT_OK,
        include_str!("main_entry/help.txt"),
        "",
    );
}

#[test]
fn no_arguments_prints_the_full_help_stream() {
    parser(&[], EXIT_OK, include_str!("main_entry/help.txt"), "");
}

#[test]
fn hidden_preflight_dispatch_checks_cleanliness() {
    dirty_route(
        &["rebase-exec-preflight", "0", "true"],
        EXIT_SOFTWARE,
        "git command failed: cannot run the start gate because the repository is not clean\nSTATUS:\n M unrelated",
        &[
            "git rev-parse --git-dir",
            "git status --porcelain=v1",
            "git rev-parse --git-dir",
        ],
    );
}

#[test]
fn hidden_begin_dispatch_checks_cleanliness() {
    dirty_route(
        &[
            "rebase-exec-begin",
            "0",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "false",
            "true",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ],
        EXIT_SOFTWARE,
        "git command failed: cannot begin the factor session because the repository is not clean\nSTATUS:\n M unrelated",
        &["git status --porcelain=v1", "git rev-parse --git-dir"],
    );
}

#[test]
fn retry_dispatch_requires_a_session() {
    dirty_route(
        &["--retry"],
        EXIT_USAGE,
        "no active factor session",
        &["git rev-parse --git-dir"],
    );
}

#[test]
fn continue_without_message_requires_a_session() {
    dirty_route(
        &["--continue"],
        EXIT_USAGE,
        "--continue requires --message <MSG>",
        &["git rev-parse --git-dir"],
    );
}

#[test]
fn default_head_start_checks_cleanliness_before_resolution() {
    dirty_route(
        &["--exec", "true"],
        EXIT_SOFTWARE,
        "git command failed: working tree must be clean before starting; stash, commit, or remove local changes\nSTATUS:\n M unrelated",
        &[
            "git rev-parse --git-dir",
            "git rev-parse --git-dir",
            "git status --porcelain=v1",
            "git rev-parse --git-dir",
        ],
    );
}

#[test]
fn parser_stderr_failure_is_reported_without_mutation() {
    parser_write_failure(&["--not-real"]);
}

#[test]
fn parser_stdout_failure_is_reported_without_mutation() {
    parser_write_failure(&["--help"]);
}

#[test]
fn implicit_help_failure_is_reported_without_mutation() {
    parser_write_failure(&[]);
}

#[test]
fn default_head_success_opens_the_selected_pool() {
    default_head_success("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
}

#[test]
fn default_head_resolution_nonzero_exit_is_a_data_error() {
    default_head_failure(HeadResolutionFault::NonzeroExit);
}

#[test]
fn default_head_resolution_launch_failure_is_a_data_error() {
    default_head_failure(HeadResolutionFault::LaunchFailure);
}

#[test]
fn parser_suggestion_is_forwarded_without_route_effects() {
    parser(
        &["--not-iu"],
        EXIT_USAGE,
        "",
        "error: unexpected argument '--not-iu' found\n\n  tip: a similar argument exists: '--continue'\n\nUsage: git-factor --continue [COMMIT]...\n\nFor more information, try '--help'.\n",
    );
}

#[test]
fn finish_dispatch_requires_a_session() {
    dirty_route(
        &["--finish"],
        EXIT_USAGE,
        "no active factor session",
        &["git rev-parse --git-dir"],
    );
}

#[test]
fn finish_with_message_dispatch_requires_a_session() {
    dirty_route(
        &["--finish", "--message", "Selected atom"],
        EXIT_USAGE,
        "no active factor session",
        &["git rev-parse --git-dir"],
    );
}

#[test]
fn continue_without_message_reports_session_reopen_query_failure() {
    reopen_failure(
        ReopenFault::GitDirectory,
        &"a".repeat(SHA_LEN),
        "unsupported",
        b"saved\0bytes",
    );
}

#[test]
fn continue_without_message_reports_unreadable_phase() {
    reopen_failure(
        ReopenFault::PhaseEncoding,
        &"a".repeat(SHA_LEN),
        "unsupported",
        b"saved\0bytes",
    );
}

#[test]
fn continue_without_message_reports_invalid_phase() {
    reopen_failure(
        ReopenFault::PhaseValue,
        &"a".repeat(SHA_LEN),
        "unsupported",
        b"saved\0bytes",
    );
}
