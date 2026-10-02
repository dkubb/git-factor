use core::num::NonZeroUsize;

use crate::git_factor::tests::start_contracts::DirectStart;

use super::*;

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
    let runner = start_single_head_resolution_runner(repo, &sha);
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
    )
    .with_status(
        "git",
        &["rev-parse", "--quiet", "--verify", &format!("{sha}^")],
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
    )
    .with_status(
        "git",
        &["rev-parse", "--quiet", "--verify", &format!("{sha}^")],
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
    )
    .with_status(
        "git",
        &["rev-parse", "--quiet", "--verify", &format!("{sha}^")],
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
            "git",
            &["rev-parse", "--quiet", "--verify", &format!("{sha}^")],
            &[],
            true,
            repo,
            0,
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
    )
    .with_status(
        "git",
        &["rev-parse", "--quiet", "--verify", &format!("{sha}^")],
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
    )
    .with_status(
        "git",
        &["rev-parse", "--quiet", "--verify", &format!("{sha}^")],
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
            .with_status(
                "git",
                &["rev-parse", "--quiet", "--verify", &format!("{sha}^")],
                &[],
                true,
                repo,
                1,
            )
            .with_status(
                "git",
                &["rev-parse", "--quiet", "--verify", &format!("{sha}^2")],
                &[],
                true,
                repo,
                1,
            )
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
        .with_status(
            "git",
            &["rev-parse", "--quiet", "--verify", &format!("{sha}^2")],
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
fn cmd_start_characterizes_later_merge_query_io_fail_open() {
    use crate::git_factor::tests::start_contracts::query;
    use crate::git_factor::tests::start_contracts::query::{
        QueryCase, QueryPosition, QueryReply, QueryTarget,
    };
    use alloc::collections::BTreeMap;

    let selected = NonEmpty::from_vec(vec![
        CommitSha::new("a".repeat(SHA_LEN)).or_abort("first SHA"),
        CommitSha::new("b".repeat(SHA_LEN)).or_abort("second SHA"),
    ])
    .or_abort("nonempty span");
    let case = QueryCase::Failure {
        target: QueryTarget::MergeParent(QueryPosition::Last),
        reply: QueryReply::Io,
    };
    let fixture = query::direct_start(&selected, false, &case, "gate out", "gate err");
    let ctx = fixture.ctx();

    let result = cmd_start_with_resolved_in(&ctx, &fixture.exec, &fixture.state, &fixture.selected);

    assert_eq!(result.map_err(|err| err.to_string()), Ok(EXIT_OK));
    // This pins baseline fail-open behavior; the independent Merge Fix owns its repair.
    assert_eq!(
        fixture.io.stdout(),
        concat!(
            "gate outFACTOR: Split session started for 2 commits (tip: abcdef0).\n",
            "ORIGINAL MESSAGE: subject\nUNSTAGED:\n\n",
            "NEXT: Stage changes for the first atomic commit, then run:\n",
            "  git factor --continue --message \"type: description\"\n\n",
            "Run git factor -h for command help or git-factor --help for the ",
            "full workflow guide.\n\nHINTS:\n",
            "  - Find the ONE smallest addition nothing depends on\n",
            "  - Target 15-30 lines (50 max)\n",
            "  - Message: single concrete action, no \"and\"/\"or\"\n",
            "  - Verify: git log --oneline | wc -l\n",
            "  - NEVER use git commit. ONLY use git factor --continue.\n",
            "  RECOVERY: git factor --abort\n",
        )
    );
    assert_eq!(fixture.io.stderr(), "gate err");
    let expected_journal = [
        (
            "commits",
            format!("{}\n{}\n", "a".repeat(SHA_LEN), "b".repeat(SHA_LEN)),
        ),
        ("current_index", "1\n".to_owned()),
        ("exec", "true\n".to_owned()),
        ("is_root", "false\n".to_owned()),
        ("phase", "splitting\n".to_owned()),
        ("requires_rebase", "false\n".to_owned()),
        ("split_count", "0\n".to_owned()),
        ("start_head", format!("{}\n", "b".repeat(SHA_LEN))),
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
fn cmd_start_characterizes_nonroot_parent_query_rejection_as_root() {
    use crate::git_factor::tests::start_contracts::query;
    use crate::git_factor::tests::start_contracts::query::{QueryCase, QueryReply, QueryTarget};
    use alloc::collections::BTreeMap;

    let selected = NonEmpty::new(CommitSha::new("a".repeat(SHA_LEN)).or_abort("admitted SHA"));
    let case = QueryCase::Failure {
        target: QueryTarget::Parent,
        reply: QueryReply::Rejected,
    };
    let fixture = query::direct_start(&selected, false, &case, "gate stdout", "gate stderr");
    let ctx = fixture.ctx();

    // Pins baseline fail-open behavior; the independent Parent Fix owns its repair.
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
        ("is_root", "true\n".to_owned()),
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
        Err("git command failed: git rev-parse: selected query IO failure".to_owned())
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
