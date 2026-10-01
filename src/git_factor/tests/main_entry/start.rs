use super::super::*;

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
