/// Arrange-only observations for direct Git-output contracts.
#[cfg(test)]
#[path = "git_output_contracts.rs"]
mod output_contracts;

#[cfg_attr(
    not(test),
    expect(
        clippy::wildcard_imports,
        reason = "module intentionally shares parent items via a grouped import"
    )
)]
use super::*;
use std::time::Instant;

/// Backend directory name for apply-based rebases.
pub(in crate::git_factor) const REBASE_APPLY_DIR: &str = "rebase-apply";

/// Backend directory name for merge-based rebases.
pub(in crate::git_factor) const REBASE_MERGE_DIR: &str = "rebase-merge";

/// Spawns a command and returns its captured output.
fn command_output(ctx: &Ctx<'_>, bin: &str, args: &[&str]) -> Result<Output, FactorError> {
    let trace_enabled = trace_log_path(ctx).is_some();
    let before = if trace_enabled {
        collect_repo_snapshot(ctx)
    } else {
        RepoSnapshot::default()
    };
    let started = Instant::now();
    let output = match ctx.runner.output(bin, args, &ctx.cwd) {
        Ok(output) => output,
        Err(err) => {
            let duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
            let after = if trace_enabled {
                collect_repo_snapshot(ctx)
            } else {
                RepoSnapshot::default()
            };
            let err_text = err.to_string();
            trace_process_command(
                ctx,
                ProcessTrace {
                    mode: "output",
                    bin,
                    args,
                    envs: &[],
                    quiet: false,
                    duration_ms,
                    exit_code: None,
                    stdout: None,
                    stderr: Some(err_text.as_str()),
                    spawned: false,
                    before: &before,
                    after: &after,
                },
            );
            return Err(FactorError::GitCommand(non_empty_msg(format!(
                "{bin} {}: {err}",
                args.first().unwrap_or(&"")
            ))));
        }
    };
    let duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let after = if trace_enabled {
        collect_repo_snapshot(ctx)
    } else {
        RepoSnapshot::default()
    };
    trace_process_command(
        ctx,
        ProcessTrace {
            mode: "output",
            bin,
            args,
            envs: &[],
            quiet: false,
            duration_ms,
            exit_code: Some(output.status.code().unwrap_or(EXIT_SOFTWARE)),
            stdout: Some(stdout.as_str()),
            stderr: Some(stderr.as_str()),
            spawned: true,
            before: &before,
            after: &after,
        },
    );

    Ok(output)
}

/// Spawns a command and returns its exit status.
///
/// This is intentionally status-only (not output) to keep error handling and
/// coverage-friendly control flow consistent across all git-factor commands.
pub(in crate::git_factor) fn command_status_with(
    ctx: &Ctx<'_>,
    bin: &str,
    args: &[&str],
    envs: &[(&str, &str)],
    quiet: bool,
) -> Result<ExitStatus, FactorError> {
    let first_arg = args.first().copied().unwrap_or("");
    let trace_enabled = trace_log_path(ctx).is_some();
    let before = trace_enabled.then(|| collect_repo_snapshot(ctx));
    let started = Instant::now();
    let result = ctx.runner.status(bin, args, envs, quiet, &ctx.cwd);
    let duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    match result {
        Ok(status) => {
            if let Some(before_snapshot) = before.as_ref() {
                let after = collect_repo_snapshot(ctx);
                trace_process_command(
                    ctx,
                    ProcessTrace {
                        mode: "status",
                        bin,
                        args,
                        envs,
                        quiet,
                        duration_ms,
                        exit_code: Some(status.code().unwrap_or(EXIT_SOFTWARE)),
                        stdout: None,
                        stderr: None,
                        spawned: true,
                        before: before_snapshot,
                        after: &after,
                    },
                );
            }
            Ok(status)
        }
        Err(err) => {
            if let Some(before_snapshot) = before.as_ref() {
                let after = collect_repo_snapshot(ctx);
                let err_text = err.to_string();
                trace_process_command(
                    ctx,
                    ProcessTrace {
                        mode: "status",
                        bin,
                        args,
                        envs,
                        quiet,
                        duration_ms,
                        exit_code: None,
                        stdout: None,
                        stderr: Some(err_text.as_str()),
                        spawned: false,
                        before: before_snapshot,
                        after: &after,
                    },
                );
            }
            Err(FactorError::GitCommand(non_empty_msg(format!(
                "{bin} {first_arg}: {err}"
            ))))
        }
    }
}

/// Runs `git <args...>` and returns its exit status.
pub(in crate::git_factor) fn git_status(
    ctx: &Ctx<'_>,
    args: &[&str],
) -> Result<ExitStatus, FactorError> {
    command_status_with(ctx, "git", args, &[], false)
}
/// Returns the absolute path to the `.git` directory.
pub(in crate::git_factor) fn git_dir_in(ctx: &Ctx<'_>) -> Result<PathBuf, FactorError> {
    let output = ctx
        .runner
        .output("git", &["rev-parse", "--git-dir"], &ctx.cwd)
        .map_err(|error| FactorError::GitDir(non_empty_msg(error.to_string())))?;

    if !output.status.success() {
        return Err(FactorError::NotGitRepo);
    }

    let path = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    let git_dir_path = PathBuf::from(path);

    // `git rev-parse --git-dir` can return a relative path (e.g. ".git"). Treat it
    // as relative to the `Ctx` working directory.
    if git_dir_path.is_relative() {
        Ok(ctx.cwd.join(git_dir_path))
    } else {
        Ok(git_dir_path)
    }
}

/// Runs a git command and returns its trimmed stdout as a `String`.
pub(in crate::git_factor) fn git_output(
    ctx: &Ctx<'_>,
    args: &[&str],
) -> Result<String, FactorError> {
    let output = command_output(ctx, "git", args)?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if !output.status.success() {
        let diagnostic = stderr.trim();
        let message = if diagnostic.is_empty() {
            output.status.to_string()
        } else {
            diagnostic.to_owned()
        };
        return Err(FactorError::GitCommand(non_empty_msg(message)));
    }

    Ok(stdout.trim().to_owned())
}

/// Runs a git command using the provided executable name/path.
#[cfg(test)]
pub(in crate::git_factor) fn git_output_with(
    ctx: &Ctx<'_>,
    bin: &str,
    args: &[&str],
) -> Result<String, FactorError> {
    let output = command_output(ctx, bin, args)?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if !output.status.success() {
        let diagnostic = stderr.trim();
        let message = if diagnostic.is_empty() {
            output.status.to_string()
        } else {
            diagnostic.to_owned()
        };
        return Err(FactorError::GitCommand(non_empty_msg(message)));
    }

    Ok(stdout.trim().to_owned())
}

/// Runs a command and returns its captured output, regardless of exit status.
pub(in crate::git_factor) fn command_output_with(
    ctx: &Ctx<'_>,
    bin: &str,
    args: &[&str],
) -> Result<Output, FactorError> {
    command_output(ctx, bin, args)
}

/// Runs `git <args...>` and returns captured output, regardless of exit status.
#[expect(
    clippy::single_call_fn,
    reason = "raw git output is intentionally centralized for status-command diagnostics"
)]
pub(in crate::git_factor) fn git_raw_output(
    ctx: &Ctx<'_>,
    args: &[&str],
) -> Result<Output, FactorError> {
    command_output(ctx, "git", args)
}

/// Runs a git command and returns success/failure.
pub(in crate::git_factor) fn run_git(ctx: &Ctx<'_>, args: &[&str]) -> Result<(), FactorError> {
    let status = command_status_with(ctx, "git", args, &[], false)?;

    if status.success() {
        Ok(())
    } else {
        Err(FactorError::GitCommand(non_empty_msg(format!(
            "git {} failed (exit {})",
            args.first().unwrap_or(&""),
            status_code(status)
        ))))
    }
}

/// Runs a git command with editor invocations disabled.
///
/// Use this for flows where opening an editor is unexpected and should fail
/// fast (for example `git rebase --continue` in automated factor sessions).
pub(in crate::git_factor) fn run_git_non_interactive(
    ctx: &Ctx<'_>,
    args: &[&str],
) -> Result<(), FactorError> {
    let status = command_status_with(
        ctx,
        "git",
        args,
        &[("GIT_EDITOR", "false"), ("GIT_SEQUENCE_EDITOR", "false")],
        false,
    )?;

    if status.success() {
        Ok(())
    } else {
        Err(FactorError::GitCommand(non_empty_msg(format!(
            "git {} failed (exit {})",
            args.first().unwrap_or(&""),
            status_code(status)
        ))))
    }
}

/// Runs a git command using the provided executable name/path.
#[cfg(test)]
pub(in crate::git_factor) fn run_git_with(
    ctx: &Ctx<'_>,
    bin: &str,
    args: &[&str],
) -> Result<(), FactorError> {
    let status = command_status_with(ctx, bin, args, &[], false)?;

    if status.success() {
        Ok(())
    } else {
        Err(FactorError::GitCommand(non_empty_msg(format!(
            "git {} failed (exit {})",
            args.first().unwrap_or(&""),
            status_code(status)
        ))))
    }
}

#[cfg(test)]
mod tests {
    mod git_output {
        use super::*;

        use super::super::output_contracts::{RecordingRunner, snapshot, successful};

        #[test]
        fn spawn_failure_without_trace_makes_only_the_requested_query() {
            let directory = TempDir::new().or_abort("output contract directory");
            let runner = RecordingRunner::new(vec![Err(io::Error::other("query launch refused"))]);
            let environment = TestEnv {
                cwd: directory.path().to_path_buf(),
                trace_log: None,
            };
            let io = BufferIo::default();
            let ctx = Ctx {
                runner: &runner,
                cwd: environment.cwd.clone(),
                env: &environment,
                io: &io,
                fs: &REAL_FS,
            };

            let result = super::super::git_output(&ctx, &["rev-parse", "--short", "HEAD"]);

            let error = result.err_or_abort("requested query cannot spawn");
            assert!(
                matches!(&error, FactorError::GitCommand(message) if message.as_str() == "git rev-parse: query launch refused"),
                "unexpected error: {error:?}"
            );
            assert_eq!(
                runner.requests(),
                vec![(
                    "git".to_owned(),
                    vec![
                        "rev-parse".to_owned(),
                        "--short".to_owned(),
                        "HEAD".to_owned()
                    ],
                    directory.path().to_path_buf()
                )]
            );
            assert_eq!(io.stdout(), "");
            assert_eq!(io.stderr(), "");
            assert_eq!(
                fs::read_dir(directory.path())
                    .or_abort("read contract directory")
                    .count(),
                0
            );
        }
        #[test]
        fn spawn_failure_with_empty_trace_path_makes_only_the_requested_query() {
            let directory = TempDir::new().or_abort("output contract directory");
            let runner = RecordingRunner::new(vec![Err(io::Error::other("query launch refused"))]);
            let environment = TestEnv {
                cwd: directory.path().to_path_buf(),
                trace_log: Some(OsString::new()),
            };
            let io = BufferIo::default();
            let ctx = Ctx {
                runner: &runner,
                cwd: environment.cwd.clone(),
                env: &environment,
                io: &io,
                fs: &REAL_FS,
            };

            let result = super::super::git_output(&ctx, &["rev-parse", "--short", "HEAD"]);

            let error = result.err_or_abort("requested query cannot spawn");
            assert!(
                matches!(&error, FactorError::GitCommand(message) if message.as_str() == "git rev-parse: query launch refused"),
                "unexpected error: {error:?}"
            );
            assert_eq!(
                runner.requests(),
                vec![(
                    "git".to_owned(),
                    vec![
                        "rev-parse".to_owned(),
                        "--short".to_owned(),
                        "HEAD".to_owned()
                    ],
                    directory.path().to_path_buf()
                )]
            );
            assert_eq!(io.stdout(), "");
            assert_eq!(io.stderr(), "");
            assert_eq!(
                fs::read_dir(directory.path())
                    .or_abort("read contract directory")
                    .count(),
                0
            );
        }
        #[test]
        fn enabled_trace_preserves_failed_query_and_both_snapshots() {
            let directory = TempDir::new().or_abort("output contract directory");
            let trace_path = directory.path().join("trace.jsonl");
            let mut replies = snapshot("before-head", "before-tree", directory.path());
            replies.push(Err(io::Error::other("query launch refused")));
            replies.extend(snapshot("after-head", "after-tree", directory.path()));
            let runner = RecordingRunner::new(replies);
            let environment = TestEnv {
                cwd: directory.path().to_path_buf(),
                trace_log: Some(trace_path.as_os_str().to_os_string()),
            };
            let io = BufferIo::default();
            let ctx = Ctx {
                runner: &runner,
                cwd: environment.cwd.clone(),
                env: &environment,
                io: &io,
                fs: &REAL_FS,
            };

            let result = super::super::git_output(&ctx, &["contract-query"]);

            let error = result.err_or_abort("requested query cannot spawn");
            assert!(
                matches!(&error, FactorError::GitCommand(message) if message.as_str() == "git contract-query: query launch refused"),
                "unexpected error: {error:?}"
            );
            let stdout_json = "null";
            let stderr_json = "\"query launch refused\"";
            assert_eq!(
                runner.requests(),
                [
                    vec!["rev-parse", "--verify", "HEAD"],
                    vec!["rev-parse", "--verify", "HEAD^{tree}"],
                    vec!["rev-parse", "--git-dir"],
                    vec!["rev-parse", "--show-toplevel"],
                    vec![
                        "--no-optional-locks",
                        "status",
                        "--porcelain=v1",
                        "--untracked-files=all"
                    ],
                    vec!["contract-query"],
                    vec!["rev-parse", "--verify", "HEAD"],
                    vec!["rev-parse", "--verify", "HEAD^{tree}"],
                    vec!["rev-parse", "--git-dir"],
                    vec!["rev-parse", "--show-toplevel"],
                    vec![
                        "--no-optional-locks",
                        "status",
                        "--porcelain=v1",
                        "--untracked-files=all"
                    ],
                ]
                .into_iter()
                .map(|args| (
                    "git".to_owned(),
                    args.into_iter().map(str::to_owned).collect(),
                    directory.path().to_path_buf()
                ))
                .collect::<Vec<_>>()
            );
            assert_eq!(io.stdout(), "");
            assert_eq!(io.stderr(), "");
            let trace = fs::read_to_string(&trace_path).or_abort("read output trace");
            assert_eq!(trace.lines().count(), 1);
            assert!(trace.ends_with('\n'));
            let mut record: serde_json::Value =
                serde_json::from_str(trace.trim_end_matches('\n')).or_abort("parse output trace");
            let object = record.as_object_mut().or_abort("trace is an object");
            assert!(
                object
                    .remove("ts_unix_ms")
                    .and_then(|value| value.as_u64())
                    .is_some()
            );
            assert!(
                object
                    .remove("duration_ms")
                    .and_then(|value| value.as_u64())
                    .is_some()
            );
            let cwd_json = serde_json::to_string(&directory.path().to_string_lossy())
                .or_abort("serialize exact contract cwd");
            let git_dir_json =
                serde_json::to_string(&directory.path().join(".git").to_string_lossy())
                    .or_abort("serialize exact Git directory");
            let expected: serde_json::Value = serde_json::from_str(&format!(r#"{{
                "event":"process","mode":"output","bin":"git","args":["contract-query"],"env":[],"quiet":false,
                "spawned":false,"exit_code":null,"stdout":{stdout_json},"stderr":{stderr_json},
                "before_head":"before-head","before_head_tree":"before-tree","before_git_dir":{git_dir_json},"before_toplevel":{cwd_json},
                "before_staged_paths":[],"before_unstaged_paths":[],"before_untracked_paths":[],
                "before_factor_current_index":null,"before_factor_split_count":null,"before_factor_requires_rebase":null,"before_factor_expected_tree":null,"before_factor_current_commit":null,
                "before_rebase_state":null,"before_rebase_msgnum":null,"before_rebase_end":null,"before_rebase_todo_head":null,"before_rebase_done_tail":null,
                "after_head":"after-head","after_head_tree":"after-tree","after_git_dir":{git_dir_json},"after_toplevel":{cwd_json},
                "after_staged_paths":[],"after_unstaged_paths":[],"after_untracked_paths":[],
                "after_factor_current_index":null,"after_factor_split_count":null,"after_factor_requires_rebase":null,"after_factor_expected_tree":null,"after_factor_current_commit":null,
                "after_rebase_state":null,"after_rebase_msgnum":null,"after_rebase_end":null,"after_rebase_todo_head":null,"after_rebase_done_tail":null
            }}"#)).or_abort("parse literal trace oracle");
            assert_eq!(record, expected);
        }
        #[test]
        fn enabled_trace_preserves_successful_query_and_both_snapshots() {
            let directory = TempDir::new().or_abort("output contract directory");
            let trace_path = directory.path().join("trace.jsonl");
            let mut replies = snapshot("before-head", "before-tree", directory.path());
            replies.push(Ok(successful(b" value\n", b"successful diagnostic")));
            replies.extend(snapshot("after-head", "after-tree", directory.path()));
            let runner = RecordingRunner::new(replies);
            let environment = TestEnv {
                cwd: directory.path().to_path_buf(),
                trace_log: Some(trace_path.as_os_str().to_os_string()),
            };
            let io = BufferIo::default();
            let ctx = Ctx {
                runner: &runner,
                cwd: environment.cwd.clone(),
                env: &environment,
                io: &io,
                fs: &REAL_FS,
            };

            let result = super::super::git_output(&ctx, &["contract-query"]);

            assert_eq!(result.or_abort("requested query succeeds"), "value");
            let stdout_json = "\" value\\n\"";
            let stderr_json = "\"successful diagnostic\"";
            assert_eq!(
                runner.requests(),
                [
                    vec!["rev-parse", "--verify", "HEAD"],
                    vec!["rev-parse", "--verify", "HEAD^{tree}"],
                    vec!["rev-parse", "--git-dir"],
                    vec!["rev-parse", "--show-toplevel"],
                    vec![
                        "--no-optional-locks",
                        "status",
                        "--porcelain=v1",
                        "--untracked-files=all"
                    ],
                    vec!["contract-query"],
                    vec!["rev-parse", "--verify", "HEAD"],
                    vec!["rev-parse", "--verify", "HEAD^{tree}"],
                    vec!["rev-parse", "--git-dir"],
                    vec!["rev-parse", "--show-toplevel"],
                    vec![
                        "--no-optional-locks",
                        "status",
                        "--porcelain=v1",
                        "--untracked-files=all"
                    ],
                ]
                .into_iter()
                .map(|args| (
                    "git".to_owned(),
                    args.into_iter().map(str::to_owned).collect(),
                    directory.path().to_path_buf()
                ))
                .collect::<Vec<_>>()
            );
            assert_eq!(io.stdout(), "");
            assert_eq!(io.stderr(), "");
            let trace = fs::read_to_string(&trace_path).or_abort("read output trace");
            assert_eq!(trace.lines().count(), 1);
            assert!(trace.ends_with('\n'));
            let mut record: serde_json::Value =
                serde_json::from_str(trace.trim_end_matches('\n')).or_abort("parse output trace");
            let object = record.as_object_mut().or_abort("trace is an object");
            assert!(
                object
                    .remove("ts_unix_ms")
                    .and_then(|value| value.as_u64())
                    .is_some()
            );
            assert!(
                object
                    .remove("duration_ms")
                    .and_then(|value| value.as_u64())
                    .is_some()
            );
            let cwd_json = serde_json::to_string(&directory.path().to_string_lossy())
                .or_abort("serialize exact contract cwd");
            let git_dir_json =
                serde_json::to_string(&directory.path().join(".git").to_string_lossy())
                    .or_abort("serialize exact Git directory");
            let expected: serde_json::Value = serde_json::from_str(&format!(r#"{{
                "event":"process","mode":"output","bin":"git","args":["contract-query"],"env":[],"quiet":false,
                "spawned":true,"exit_code":0,"stdout":{stdout_json},"stderr":{stderr_json},
                "before_head":"before-head","before_head_tree":"before-tree","before_git_dir":{git_dir_json},"before_toplevel":{cwd_json},
                "before_staged_paths":[],"before_unstaged_paths":[],"before_untracked_paths":[],
                "before_factor_current_index":null,"before_factor_split_count":null,"before_factor_requires_rebase":null,"before_factor_expected_tree":null,"before_factor_current_commit":null,
                "before_rebase_state":null,"before_rebase_msgnum":null,"before_rebase_end":null,"before_rebase_todo_head":null,"before_rebase_done_tail":null,
                "after_head":"after-head","after_head_tree":"after-tree","after_git_dir":{git_dir_json},"after_toplevel":{cwd_json},
                "after_staged_paths":[],"after_unstaged_paths":[],"after_untracked_paths":[],
                "after_factor_current_index":null,"after_factor_split_count":null,"after_factor_requires_rebase":null,"after_factor_expected_tree":null,"after_factor_current_commit":null,
                "after_rebase_state":null,"after_rebase_msgnum":null,"after_rebase_end":null,"after_rebase_todo_head":null,"after_rebase_done_tail":null
            }}"#)).or_abort("parse literal trace oracle");
            assert_eq!(record, expected);
        }
        #[test]
        fn silent_exit_reports_native_status() {
            let exit_code: i32 = 42;
            let runner = OutputOnlyRunner {
                output: Some(Output {
                    status: ExitStatus::from_raw(
                        exit_code
                            .checked_shl(u8::BITS)
                            .or_abort("native wait-status shift count is below the i32 width"),
                    ),
                    stdout: Vec::new(),
                    stderr: Vec::new(),
                }),
                ..OutputOnlyRunner::default()
            };
            let environment = TestEnv {
                cwd: PathBuf::from("."),
                trace_log: None,
            };
            let io = BufferIo::default();
            let ctx = arrange_query_context(&runner, &environment, &io);

            let result = super::super::git_output(&ctx, &["rev-parse", "--short", "HEAD"]);

            let error = result.err_or_abort("captured query must fail");
            assert!(
                matches!(
                    &error,
                    FactorError::GitCommand(message) if message.as_str() == "exit status: 42"
                ),
                "unexpected error: {error:?}"
            );
        }

        #[test]
        fn silent_signal_reports_native_status() {
            let runner = OutputOnlyRunner {
                output: Some(Output {
                    status: ExitStatus::from_raw(9),
                    stdout: Vec::new(),
                    stderr: Vec::new(),
                }),
                ..OutputOnlyRunner::default()
            };
            let environment = TestEnv {
                cwd: PathBuf::from("."),
                trace_log: None,
            };
            let io = BufferIo::default();
            let ctx = arrange_query_context(&runner, &environment, &io);

            let result = super::super::git_output(&ctx, &["rev-parse", "--short", "HEAD"]);

            let error = result.err_or_abort("captured query must fail");
            assert!(
                matches!(
                    &error,
                    FactorError::GitCommand(message) if message.as_str() == "signal: 9 (SIGKILL)"
                ),
                "unexpected error: {error:?}"
            );
        }

        #[test]
        fn successful_output_is_lossy_decoded_and_trimmed() {
            let runner = OutputOnlyRunner {
                output: Some(Output {
                    status: ExitStatus::from_raw(0),
                    stdout: b" \tresult\xff\r\n".to_vec(),
                    stderr: b"ignored successful stderr".to_vec(),
                }),
                ..OutputOnlyRunner::default()
            };
            let environment = TestEnv {
                cwd: PathBuf::from("."),
                trace_log: None,
            };
            let io = BufferIo::default();
            let ctx = arrange_query_context(&runner, &environment, &io);

            let result = super::super::git_output(&ctx, &["rev-parse", "--short", "HEAD"]);

            assert_eq!(result.or_abort("query succeeds"), "result\u{fffd}");
        }

        #[test]
        fn spawn_failure_preserves_command_and_diagnostic() {
            let runner = OutputOnlyRunner {
                fail_output: true,
                ..OutputOnlyRunner::default()
            };
            let environment = TestEnv {
                cwd: PathBuf::from("."),
                trace_log: None,
            };
            let io = BufferIo::default();
            let ctx = arrange_query_context(&runner, &environment, &io);

            let result = super::super::git_output(&ctx, &["rev-parse", "--short", "HEAD"]);

            let error = result.err_or_abort("query cannot spawn");
            assert!(
                matches!(
                    &error,
                    FactorError::GitCommand(message)
                        if message.as_str() == "git rev-parse: forced output failure"
                ),
                "unexpected error: {error:?}"
            );
        }
        #[test]
        fn nonempty_stderr_is_preserved() {
            let exit_code: i32 = 42;
            let runner = OutputOnlyRunner {
                output: Some(Output {
                    status: ExitStatus::from_raw(
                        exit_code
                            .checked_shl(u8::BITS)
                            .or_abort("native wait-status shift count is below the i32 width"),
                    ),
                    stdout: Vec::new(),
                    stderr: b" \tfatal: query failed\r\n".to_vec(),
                }),
                ..OutputOnlyRunner::default()
            };
            let environment = TestEnv {
                cwd: PathBuf::from("."),
                trace_log: None,
            };
            let io = BufferIo::default();
            let ctx = arrange_query_context(&runner, &environment, &io);

            let result = super::super::git_output(&ctx, &["rev-parse", "--short", "HEAD"]);

            let error = result.err_or_abort("captured query must fail");
            assert!(
                matches!(
                    &error,
                    FactorError::GitCommand(message) if message.as_str() == "fatal: query failed"
                ),
                "unexpected error: {error:?}"
            );
        }
    }

    mod git_output_with {
        use super::*;

        #[test]
        fn silent_exit_reports_native_status() {
            let exit_code: i32 = 42;
            let runner = OutputOnlyRunner {
                output: Some(Output {
                    status: ExitStatus::from_raw(
                        exit_code
                            .checked_shl(u8::BITS)
                            .or_abort("native wait-status shift count is below the i32 width"),
                    ),
                    stdout: Vec::new(),
                    stderr: Vec::new(),
                }),
                ..OutputOnlyRunner::default()
            };
            let environment = TestEnv {
                cwd: PathBuf::from("."),
                trace_log: None,
            };
            let io = BufferIo::default();
            let ctx = arrange_query_context(&runner, &environment, &io);

            let result =
                super::super::git_output_with(&ctx, "git", &["rev-parse", "--short", "HEAD"]);

            let error = result.err_or_abort("captured query must fail");
            assert!(
                matches!(
                    &error,
                    FactorError::GitCommand(message) if message.as_str() == "exit status: 42"
                ),
                "unexpected error: {error:?}"
            );
        }

        #[test]
        fn silent_signal_reports_native_status() {
            let runner = OutputOnlyRunner {
                output: Some(Output {
                    status: ExitStatus::from_raw(9),
                    stdout: Vec::new(),
                    stderr: Vec::new(),
                }),
                ..OutputOnlyRunner::default()
            };
            let environment = TestEnv {
                cwd: PathBuf::from("."),
                trace_log: None,
            };
            let io = BufferIo::default();
            let ctx = arrange_query_context(&runner, &environment, &io);

            let result =
                super::super::git_output_with(&ctx, "git", &["rev-parse", "--short", "HEAD"]);

            let error = result.err_or_abort("captured query must fail");
            assert!(
                matches!(
                    &error,
                    FactorError::GitCommand(message) if message.as_str() == "signal: 9 (SIGKILL)"
                ),
                "unexpected error: {error:?}"
            );
        }

        #[test]
        fn nonempty_stderr_is_preserved() {
            let exit_code: i32 = 42;
            let runner = OutputOnlyRunner {
                output: Some(Output {
                    status: ExitStatus::from_raw(
                        exit_code
                            .checked_shl(u8::BITS)
                            .or_abort("native wait-status shift count is below the i32 width"),
                    ),
                    stdout: Vec::new(),
                    stderr: b" \tfatal: query failed\r\n".to_vec(),
                }),
                ..OutputOnlyRunner::default()
            };
            let environment = TestEnv {
                cwd: PathBuf::from("."),
                trace_log: None,
            };
            let io = BufferIo::default();
            let ctx = arrange_query_context(&runner, &environment, &io);

            let result =
                super::super::git_output_with(&ctx, "git", &["rev-parse", "--short", "HEAD"]);

            let error = result.err_or_abort("captured query must fail");
            assert!(
                matches!(
                    &error,
                    FactorError::GitCommand(message) if message.as_str() == "fatal: query failed"
                ),
                "unexpected error: {error:?}"
            );
        }
    }

    use super::*;
    use std::env;
    use std::ffi::OsString;
    use std::os::unix::process::ExitStatusExt as _;
    use std::process::Command;
    use std::process::Output;
    use std::sync::{Mutex, PoisonError};
    use tempfile::TempDir;

    #[expect(
        clippy::field_scoped_visibility_modifiers,
        reason = "private unit and property providers share these arrange-only fixture fields"
    )]
    pub(in crate::git_factor::git) struct TestEnv {
        pub(in crate::git_factor::git) cwd: PathBuf,
        pub(in crate::git_factor::git) trace_log: Option<OsString>,
    }

    impl Env for TestEnv {
        fn current_dir(&self) -> io::Result<PathBuf> {
            Ok(self.cwd.clone())
        }

        fn current_exe(&self) -> io::Result<PathBuf> {
            env::current_exe()
        }

        fn var_os(&self, key: &str) -> Option<OsString> {
            if key == TRACE_LOG_ENV {
                return self.trace_log.clone();
            }
            None
        }
    }

    struct ToggleTraceEnv {
        calls: Mutex<usize>,
        cwd: PathBuf,
        trace_path: OsString,
    }

    impl Env for ToggleTraceEnv {
        fn current_dir(&self) -> io::Result<PathBuf> {
            Ok(self.cwd.clone())
        }

        fn current_exe(&self) -> io::Result<PathBuf> {
            env::current_exe()
        }

        fn var_os(&self, key: &str) -> Option<OsString> {
            if key != TRACE_LOG_ENV {
                return None;
            }
            let mut calls = self.calls.lock().or_abort("toggle env lock");
            *calls = calls
                .checked_add(1)
                .map_or(usize::MAX, |next_calls| next_calls);
            (*calls == 1).then(|| self.trace_path.clone())
        }
    }

    #[derive(Default)]
    pub(in crate::git_factor::git) struct BufferIo {
        stderr: Mutex<String>,
        stdout: Mutex<String>,
    }

    impl BufferIo {
        /// Returns the exact stderr observed by sibling direct providers.
        pub(in crate::git_factor::git) fn stderr(&self) -> String {
            self.stderr
                .lock()
                .or_abort("stderr lock should not be poisoned")
                .clone()
        }

        /// Returns the exact stdout observed by sibling direct providers.
        pub(in crate::git_factor::git) fn stdout(&self) -> String {
            self.stdout
                .lock()
                .or_abort("stdout lock should not be poisoned")
                .clone()
        }
    }

    impl Io for BufferIo {
        fn err(&self, text: &str) -> io::Result<()> {
            self.stderr
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push_str(text);
            Ok(())
        }

        fn errln(&self, line: &str) -> io::Result<()> {
            let mut stderr = self.stderr.lock().unwrap_or_else(PoisonError::into_inner);
            stderr.push_str(line);
            stderr.push('\n');
            drop(stderr);
            Ok(())
        }

        fn out(&self, text: &str) -> io::Result<()> {
            self.stdout
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push_str(text);
            Ok(())
        }

        fn outln(&self, line: &str) -> io::Result<()> {
            self.out(&format!("{line}\n"))
        }
    }

    struct FailOnExactTextIo {
        text: String,
    }

    impl Io for FailOnExactTextIo {
        fn err(&self, _text: &str) -> io::Result<()> {
            Ok(())
        }

        fn errln(&self, line: &str) -> io::Result<()> {
            self.err(&format!("{line}\n"))
        }

        fn out(&self, text: &str) -> io::Result<()> {
            if text == self.text {
                Err(io::Error::other("io fail"))
            } else {
                Ok(())
            }
        }

        fn outln(&self, line: &str) -> io::Result<()> {
            self.out(line)?;
            self.out("\n")
        }
    }

    #[derive(Default)]
    #[expect(
        clippy::field_scoped_visibility_modifiers,
        reason = "private unit and property providers share these arrange-only fixture fields"
    )]
    pub(in crate::git_factor::git) struct OutputOnlyRunner {
        pub(in crate::git_factor::git) fail_output: bool,
        pub(in crate::git_factor::git) output: Option<Output>,
    }

    impl Runner for OutputOnlyRunner {
        fn output(&self, _bin: &str, _args: &[&str], _cwd: &Path) -> io::Result<Output> {
            if self.fail_output {
                return Err(io::Error::other("forced output failure"));
            }
            self.output
                .clone()
                .ok_or_else(|| io::Error::other("missing scripted output"))
        }

        fn status(
            &self,
            _bin: &str,
            _args: &[&str],
            _envs: &[(&str, &str)],
            _quiet: bool,
            _cwd: &Path,
        ) -> io::Result<ExitStatus> {
            Ok(ExitStatus::from_raw(0))
        }
    }

    struct ShowTopLevelFailRunner;

    impl Runner for ShowTopLevelFailRunner {
        fn output(&self, _bin: &str, args: &[&str], _cwd: &Path) -> io::Result<Output> {
            if args == ["rev-parse", "--show-toplevel"] {
                return Err(io::Error::other("forced show-toplevel failure"));
            }

            let stdout = match *args {
                ["rev-parse", "--git-dir"] => b".git\n".to_vec(),
                [
                    "--no-optional-locks",
                    "status",
                    "--porcelain=v1",
                    "--untracked-files=all",
                ] => Vec::new(),
                _ => b"deadbeef\n".to_vec(),
            };

            Ok(Output {
                status: ExitStatus::from_raw(0),
                stdout,
                stderr: Vec::new(),
            })
        }

        fn status(
            &self,
            _bin: &str,
            _args: &[&str],
            _envs: &[(&str, &str)],
            _quiet: bool,
            _cwd: &Path,
        ) -> io::Result<ExitStatus> {
            Ok(ExitStatus::from_raw(0))
        }
    }

    struct NonInteractiveRunner;

    impl Runner for NonInteractiveRunner {
        fn output(&self, _bin: &str, _args: &[&str], _cwd: &Path) -> io::Result<Output> {
            Err(io::Error::other("output is not expected"))
        }

        fn status(
            &self,
            bin: &str,
            args: &[&str],
            envs: &[(&str, &str)],
            quiet: bool,
            _cwd: &Path,
        ) -> io::Result<ExitStatus> {
            if bin != "git" {
                return Err(io::Error::other("unexpected bin"));
            }
            if args != ["status"] {
                return Err(io::Error::other("unexpected args"));
            }
            if quiet {
                return Err(io::Error::other("unexpected quiet=true"));
            }
            if envs != [("GIT_EDITOR", "false"), ("GIT_SEQUENCE_EDITOR", "false")] {
                return Err(io::Error::other("unexpected envs"));
            }
            Ok(ExitStatus::from_raw(0))
        }
    }

    pub(in crate::git_factor::git) fn arrange_query_context<'context>(
        runner: &'context OutputOnlyRunner,
        environment: &'context TestEnv,
        io: &'context BufferIo,
    ) -> Ctx<'context> {
        Ctx {
            cwd: environment.cwd.clone(),
            runner,
            env: environment,
            io,
            fs: &REAL_FS,
        }
    }

    fn ctx_for(path: &Path) -> Ctx<'static> {
        Ctx {
            runner: &REAL_RUNNER,
            cwd: path.to_path_buf(),
            io: &REAL_IO,
            env: &REAL_ENV,
            fs: &REAL_FS,
        }
    }

    fn ctx_with_env<'env>(path: &Path, env: &'env dyn Env) -> Ctx<'env> {
        Ctx {
            runner: &REAL_RUNNER,
            cwd: path.to_path_buf(),
            io: &REAL_IO,
            env,
            fs: &REAL_FS,
        }
    }

    fn git_command_message(error: &FactorError) -> Option<String> {
        if let &FactorError::GitCommand(_) = error {
            return error
                .to_string()
                .strip_prefix("git command failed: ")
                .map(str::to_owned);
        }
        None
    }

    fn assert_git_command(error: &FactorError) {
        assert!(
            matches!(error, &FactorError::GitCommand(_)),
            "error was: {error:?}"
        );
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

        fs::write(path.join("file.txt"), "base\n").or_abort("write file");
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
    fn io_errln_defaults_cover_buffer_and_fail_on_exact_text_types() {
        let buffer = BufferIo::default();
        buffer
            .errln("buffer-line")
            .or_abort("buffer errln should succeed");
        let buffer_stderr = buffer
            .stderr
            .lock()
            .or_abort("stderr lock should not be poisoned")
            .clone();
        assert_eq!(buffer_stderr, "buffer-line\n");

        let fail_io = FailOnExactTextIo {
            text: "different".to_owned(),
        };
        fail_io
            .errln("fail-on-exact-text-line")
            .or_abort("default errln should call err() and succeed");
    }

    #[test]
    fn git_output_reports_spawn_errors_as_git_command() {
        let dir = TempDir::new().or_abort("tempdir");
        let ctx = ctx_for(dir.path());
        let result = git_output_with(&ctx, "git-factor-not-a-real-git-binary", &["rev-parse"]);
        let err = result.err_or_abort("git should fail to spawn");
        let message = git_command_message(&err).or_abort("expected GitCommand");
        assert!(message.contains("git"), "err was: {err:?}");
    }

    #[test]
    fn run_git_reports_spawn_errors_as_git_command() {
        let dir = TempDir::new().or_abort("tempdir");
        let ctx = ctx_for(dir.path());
        let result = run_git_with(&ctx, "git-factor-not-a-real-git-binary", &["rev-parse"]);
        let err = result.err_or_abort("git should fail to spawn");
        let message = git_command_message(&err).or_abort("expected GitCommand");
        assert!(message.contains("git"), "err was: {err:?}");
    }

    #[test]
    fn command_status_with_can_run_in_quiet_mode() {
        let dir = TempDir::new().or_abort("tempdir");
        let ctx = ctx_for(dir.path());
        let status = command_status_with(
            &ctx,
            "bash",
            &["-c", "echo hi; echo err 1>&2; exit 0"],
            &[],
            true,
        )
        .or_abort("command should run");

        assert!(status.success());
    }

    #[test]
    fn command_status_with_reports_spawn_errors_as_git_command() {
        let dir = TempDir::new().or_abort("tempdir");
        let ctx = ctx_for(dir.path());
        let err = command_status_with(
            &ctx,
            "git-factor-not-a-real-binary",
            &["rev-parse"],
            &[],
            false,
        )
        .err_or_abort("spawn should fail");

        let message = git_command_message(&err).or_abort("expected GitCommand");
        assert!(
            message.contains("git-factor-not-a-real-binary"),
            "err was: {err:?}"
        );
    }

    #[test]
    fn output_only_runner_reports_missing_scripted_output() {
        let runner = OutputOnlyRunner {
            output: None,
            fail_output: false,
        };
        let err = runner
            .output("git", &["status"], Path::new("."))
            .err_or_abort("missing scripted output should fail");
        assert!(
            err.to_string().contains("missing scripted output"),
            "err was: {err:?}"
        );

        let status = runner
            .status("git", &["status"], &[], false, Path::new("."))
            .or_abort("status path should be callable");
        assert!(status.success());
    }

    #[test]
    fn git_dir_in_preserves_absolute_git_dir_output() {
        let dir = TempDir::new().or_abort("tempdir");
        let absolute_git_dir = dir.path().join("worktrees/repo.git");
        let output = format!("{}\n", absolute_git_dir.display());
        let runner = OutputOnlyRunner {
            output: Some(Output {
                status: ExitStatus::from_raw(0),
                stdout: output.into_bytes(),
                stderr: Vec::new(),
            }),
            fail_output: false,
        };
        let env = TestEnv {
            cwd: dir.path().to_path_buf(),
            trace_log: None,
        };
        let ctx = Ctx {
            runner: &runner,
            cwd: dir.path().to_path_buf(),
            io: &REAL_IO,
            env: &env,
            fs: &REAL_FS,
        };

        let git_dir = git_dir_in(&ctx).or_abort("absolute git-dir should be returned unchanged");
        assert_eq!(git_dir, absolute_git_dir);
    }

    #[test]
    fn error_variant_extractors_cover_matching_and_non_matching_paths() {
        use std::panic::catch_unwind;

        let git_command = FactorError::GitCommand(non_empty_msg("boom".to_owned()));
        assert_eq!(git_command_message(&git_command), Some("boom".to_owned()));
        assert_git_command(&git_command);
        assert_eq!(git_command_message(&FactorError::NoActiveSession), None);
        assert!(catch_unwind(|| assert_git_command(&FactorError::NoActiveSession)).is_err());
    }

    #[test]
    fn io_line_helpers_append_newlines_for_test_implementations() {
        let buffer = BufferIo::default();
        buffer.errln("warn").or_abort("buffer errln");
        buffer.outln("ok").or_abort("buffer outln");
        assert_eq!(buffer.stderr(), "warn\n");
        assert_eq!(buffer.stdout(), "ok\n");

        let fail_io = FailOnExactTextIo {
            text: "trigger".to_owned(),
        };
        fail_io
            .errln("note")
            .or_abort("fail io errln should succeed");
    }

    #[test]
    fn proptest_io_and_error_helpers_cover_edge_paths() {
        let buffer = BufferIo::default();
        buffer.err("warn").or_abort("buffer err");
        buffer.errln("note").or_abort("buffer errln");
        buffer.out("ok").or_abort("buffer out");
        buffer.outln("done").or_abort("buffer outln");
        assert_eq!(buffer.stderr(), "warnnote\n");
        assert_eq!(buffer.stdout(), "okdone\n");

        let fail_io = FailOnExactTextIo {
            text: "trigger".to_owned(),
        };
        fail_io.errln("ignored").or_abort("fail io errln");
        fail_io
            .out("ok")
            .or_abort("non-matching text should succeed");
        fail_io
            .outln("done")
            .or_abort("non-matching line should succeed");
        let out_err = fail_io
            .out("trigger")
            .err_or_abort("exact text should fail");
        assert_eq!(out_err.to_string(), "io fail");
        let outln_err = fail_io
            .outln("trigger")
            .err_or_abort("exact line should fail");
        assert_eq!(outln_err.to_string(), "io fail");

        assert_eq!(git_command_message(&FactorError::NoActiveSession), None);
    }

    #[test]
    fn trace_helpers_cover_edge_cases() {
        let dir = TempDir::new().or_abort("tempdir");
        let env = TestEnv {
            cwd: dir.path().to_path_buf(),
            trace_log: Some(OsString::from("")),
        };
        let ctx = ctx_with_env(dir.path(), &env);

        assert_eq!(trace_log_path(&ctx), None);
        assert_eq!(
            trace_text_limit(&"x".repeat(TRACE_MAX_TEXT_BYTES + 1)).len(),
            TRACE_MAX_TEXT_BYTES
        );
        assert_eq!(
            json_escape("\\\"\n\r\t\u{0001}"),
            "\\\\\\\"\\n\\r\\t\\u0001"
        );
        assert_eq!(
            first_rebase_todo_line("\n # c\n pick a\n"),
            Some("pick a".to_owned())
        );
        assert_eq!(last_non_empty_line("\n a\n\n"), Some("a".to_owned()));
    }

    #[test]
    fn trace_helpers_cover_non_empty_path_and_none_todo_line() {
        let dir = TempDir::new().or_abort("tempdir");
        let trace_path = dir.path().join("trace-extra.jsonl");
        let env = TestEnv {
            cwd: dir.path().to_path_buf(),
            trace_log: Some(trace_path.as_os_str().to_os_string()),
        };
        let ctx = ctx_with_env(dir.path(), &env);

        assert_eq!(trace_log_path(&ctx), Some(trace_path));
        assert_eq!(trace_text_limit("short"), "short");
        assert_eq!(json_escape("\u{001F}"), "\\u001F");
        assert_eq!(first_rebase_todo_line("  # only comment\n\t# second"), None);
    }

    #[test]
    fn trace_helpers_cover_env_limit_escape_and_todo_branches() {
        let dir = TempDir::new().or_abort("tempdir");
        let trace_path = dir.path().join("trace-branches.jsonl");
        let env_none = TestEnv {
            cwd: dir.path().to_path_buf(),
            trace_log: None,
        };
        let env_empty = TestEnv {
            cwd: dir.path().to_path_buf(),
            trace_log: Some(OsString::from("")),
        };
        let env_non_empty = TestEnv {
            cwd: dir.path().to_path_buf(),
            trace_log: Some(trace_path.as_os_str().to_os_string()),
        };
        let ctx_none = ctx_with_env(dir.path(), &env_none);
        let ctx_empty = ctx_with_env(dir.path(), &env_empty);
        let ctx_non_empty = ctx_with_env(dir.path(), &env_non_empty);

        assert_eq!(trace_log_path(&ctx_none), None);
        assert_eq!(trace_log_path(&ctx_empty), None);
        assert_eq!(trace_log_path(&ctx_non_empty), Some(trace_path));

        let within_limit = "x".repeat(TRACE_MAX_TEXT_BYTES);
        let at_limit = trace_text_limit(&within_limit);
        assert_eq!(at_limit.len(), TRACE_MAX_TEXT_BYTES);

        let over_limit = format!("{}y", "x".repeat(TRACE_MAX_TEXT_BYTES));
        let limited = trace_text_limit(&over_limit);
        assert_eq!(limited.len(), TRACE_MAX_TEXT_BYTES);

        let wide_over_limit = format!("{}é", "x".repeat(TRACE_MAX_TEXT_BYTES));
        let wide_limited = trace_text_limit(&wide_over_limit);
        assert_eq!(wide_limited.len(), TRACE_MAX_TEXT_BYTES);

        assert_eq!(json_escape("\u{0007}"), "\\u0007");
        assert_eq!(
            first_rebase_todo_line("# one\n\n# two\npick deadbeef message"),
            Some("pick deadbeef message".to_owned())
        );
        assert_eq!(first_rebase_todo_line("# one\n\t# two\n"), None);
    }

    #[test]
    fn env_var_os_returns_none_for_non_trace_keys() {
        let env = TestEnv {
            cwd: PathBuf::from("/tmp"),
            trace_log: Some(OsString::from("/tmp/trace.log")),
        };
        assert!(env.var_os("SOME_OTHER_ENV").is_none());
    }

    #[test]
    fn output_only_runner_status_returns_success_status() {
        let runner = OutputOnlyRunner::default();
        let status = runner
            .status("git", &["status"], &[], true, Path::new("."))
            .or_abort("status");
        assert!(status.success());
    }

    #[test]
    fn run_git_non_interactive_sets_editor_env() {
        let dir = TempDir::new().or_abort("tempdir");
        let env = TestEnv {
            cwd: dir.path().to_path_buf(),
            trace_log: None,
        };
        let runner = NonInteractiveRunner;
        let ctx = Ctx {
            runner: &runner,
            cwd: dir.path().to_path_buf(),
            io: &REAL_IO,
            env: &env,
            fs: &REAL_FS,
        };
        let unexpected_bin_err = runner
            .status(
                "not-git",
                &["status"],
                &[("GIT_EDITOR", "false"), ("GIT_SEQUENCE_EDITOR", "false")],
                false,
                dir.path(),
            )
            .err_or_abort("unexpected bin should fail");
        assert_eq!(unexpected_bin_err.to_string(), "unexpected bin");
        let unexpected_args_err = runner
            .status(
                "git",
                &["not-status"],
                &[("GIT_EDITOR", "false"), ("GIT_SEQUENCE_EDITOR", "false")],
                false,
                dir.path(),
            )
            .err_or_abort("unexpected args should fail");
        assert_eq!(unexpected_args_err.to_string(), "unexpected args");
        let unexpected_quiet_err = runner
            .status(
                "git",
                &["status"],
                &[("GIT_EDITOR", "false"), ("GIT_SEQUENCE_EDITOR", "false")],
                true,
                dir.path(),
            )
            .err_or_abort("quiet=true should fail");
        assert_eq!(unexpected_quiet_err.to_string(), "unexpected quiet=true");
        let unexpected_env_err = runner
            .status("git", &["status"], &[], false, dir.path())
            .err_or_abort("unexpected env should fail");
        assert_eq!(unexpected_env_err.to_string(), "unexpected envs");
        run_git_non_interactive(&ctx, &["status"]).or_abort("status should succeed");
    }

    #[test]
    fn non_interactive_runner_output_is_not_expected() {
        let runner = NonInteractiveRunner;
        let err = runner
            .output("git", &["status"], Path::new("."))
            .err_or_abort("output should fail");
        assert_eq!(err.to_string(), "output is not expected");
    }

    #[test]
    fn trace_note_records_rebase_merge_and_custom_fields() {
        let dir = TempDir::new().or_abort("tempdir");
        init_git_repo(dir.path());

        let git_dir = dir.path().join(".git");
        let rebase_merge = git_dir.join("rebase-merge");
        fs::create_dir_all(&rebase_merge).or_abort("create rebase-merge");
        fs::write(rebase_merge.join("msgnum"), "2\n").or_abort("write msgnum");
        fs::write(rebase_merge.join("end"), "5\n").or_abort("write end");
        fs::write(
            rebase_merge.join("git-rebase-todo"),
            "# c\npick deadbeef step\n",
        )
        .or_abort("write todo");
        fs::write(rebase_merge.join("done"), "pick a\n\n").or_abort("write done");

        let trace_path = dir.path().join("trace/log.jsonl");
        let env = TestEnv {
            cwd: dir.path().to_path_buf(),
            trace_log: Some(trace_path.as_os_str().to_os_string()),
        };
        let ctx = ctx_with_env(dir.path(), &env);

        trace_note(&ctx, "note", &[("key", "value")]);
        let trace = fs::read_to_string(&trace_path).or_abort("read trace");
        assert!(trace.contains("\"event\":\"note\""), "trace: {trace}");
        assert!(
            trace.contains("\"state_rebase_state\":\"rebase-merge\""),
            "trace: {trace}"
        );
        assert!(
            trace.contains("\"state_rebase_todo_head\":\"pick deadbeef step\""),
            "trace: {trace}"
        );
        assert!(
            trace.contains("\"state_rebase_done_tail\":\"pick a\""),
            "trace: {trace}"
        );
        assert!(trace.contains("\"key\":\"value\""), "trace: {trace}");
    }

    #[test]
    fn trace_note_records_rebase_apply_state() {
        let dir = TempDir::new().or_abort("tempdir");
        init_git_repo(dir.path());

        let git_dir = dir.path().join(".git");
        let rebase_apply = git_dir.join("rebase-apply");
        fs::create_dir_all(&rebase_apply).or_abort("create rebase-apply");
        fs::write(rebase_apply.join("next"), "3\n").or_abort("write next");
        fs::write(rebase_apply.join("last"), "9\n").or_abort("write last");
        fs::write(rebase_apply.join("patch"), "diff --git\n").or_abort("write patch");

        let trace_path = dir.path().join("trace-apply.jsonl");
        let env = TestEnv {
            cwd: dir.path().to_path_buf(),
            trace_log: Some(trace_path.as_os_str().to_os_string()),
        };
        let ctx = ctx_with_env(dir.path(), &env);

        trace_note(&ctx, "note_apply", &[]);
        let trace = fs::read_to_string(&trace_path).or_abort("read trace");
        assert!(
            trace.contains("\"state_rebase_state\":\"rebase-apply\""),
            "trace: {trace}"
        );
        assert!(
            trace.contains("\"state_rebase_todo_head\":\"patch\""),
            "trace: {trace}"
        );
    }

    #[test]
    fn command_and_output_trace_spawn_errors_when_enabled() {
        let dir = TempDir::new().or_abort("tempdir");
        let trace_path = dir.path().join("trace-spawn.jsonl");
        let env = TestEnv {
            cwd: dir.path().to_path_buf(),
            trace_log: Some(trace_path.as_os_str().to_os_string()),
        };
        let ctx = ctx_with_env(dir.path(), &env);

        let status_err = command_status_with(
            &ctx,
            "git-factor-not-a-real-status-binary",
            &["status"],
            &[],
            false,
        )
        .err_or_abort("status spawn should fail");
        assert_git_command(&status_err);

        let output_err = git_output_with(&ctx, "git-factor-not-a-real-output-binary", &["status"])
            .err_or_abort("output spawn should fail");
        assert_git_command(&output_err);

        let trace = fs::read_to_string(&trace_path).or_abort("read trace");
        assert!(trace.contains("\"mode\":\"status\""), "trace: {trace}");
        assert!(trace.contains("\"mode\":\"output\""), "trace: {trace}");
        assert!(trace.contains("\"spawned\":false"), "trace: {trace}");
    }

    #[test]
    fn run_git_wrappers_report_nonzero_exit_status() {
        let dir = TempDir::new().or_abort("tempdir");
        init_git_repo(dir.path());
        let ctx = ctx_for(dir.path());

        let non_interactive = run_git_non_interactive(&ctx, &["definitely-not-a-command"])
            .err_or_abort("expected git failure");
        let non_interactive_message = git_command_message(&non_interactive)
            .or_abort("non-interactive must return GitCommand");
        assert!(
            non_interactive_message.contains("failed (exit"),
            "unexpected error: {non_interactive:?}"
        );

        let run_with = run_git_with(&ctx, "git", &["definitely-not-a-command"])
            .err_or_abort("expected git failure");
        let run_with_message =
            git_command_message(&run_with).or_abort("run_git_with must return GitCommand");
        assert!(
            run_with_message.contains("failed (exit"),
            "unexpected error: {run_with:?}"
        );
    }

    #[test]
    fn collect_status_paths_covers_parser_branches_and_spawn_failure() {
        let dir = TempDir::new().or_abort("tempdir");
        let status_text = "?\n M unstaged.txt\nA  staged.txt\n?? untracked.txt\n";
        let runner = OutputOnlyRunner {
            output: Some(Output {
                status: ExitStatus::from_raw(0),
                stdout: status_text.as_bytes().to_vec(),
                stderr: Vec::new(),
            }),
            fail_output: false,
        };
        let ctx = Ctx {
            runner: &runner,
            cwd: dir.path().to_path_buf(),
            io: &REAL_IO,
            env: &REAL_ENV,
            fs: &REAL_FS,
        };

        let (staged, unstaged, untracked) = collect_status_paths(&ctx);
        assert_eq!(staged, vec!["staged.txt".to_owned()]);
        assert_eq!(unstaged, vec!["unstaged.txt".to_owned()]);
        assert_eq!(untracked, vec!["untracked.txt".to_owned()]);

        let failing_runner = OutputOnlyRunner {
            output: None,
            fail_output: true,
        };
        let failing_ctx = Ctx {
            runner: &failing_runner,
            cwd: dir.path().to_path_buf(),
            io: &REAL_IO,
            env: &REAL_ENV,
            fs: &REAL_FS,
        };
        let (staged_empty, unstaged_empty, untracked_empty) = collect_status_paths(&failing_ctx);
        assert!(staged_empty.is_empty());
        assert!(unstaged_empty.is_empty());
        assert!(untracked_empty.is_empty());
    }

    #[test]
    fn collect_status_paths_handles_short_and_question_mark_second_column() {
        let dir = TempDir::new().or_abort("tempdir");
        let status_text = format!(
            "\nA? staged-only.txt\n{}\n?M odd.txt\n?? untracked.txt\n",
            "\u{00E9}\u{00E9}"
        );
        let runner = OutputOnlyRunner {
            output: Some(Output {
                status: ExitStatus::from_raw(0),
                stdout: status_text.as_bytes().to_vec(),
                stderr: Vec::new(),
            }),
            fail_output: false,
        };
        let ctx = Ctx {
            runner: &runner,
            cwd: dir.path().to_path_buf(),
            io: &REAL_IO,
            env: &REAL_ENV,
            fs: &REAL_FS,
        };

        let (staged, unstaged, untracked) = collect_status_paths(&ctx);
        assert_eq!(staged, vec!["staged-only.txt".to_owned()]);
        assert_eq!(unstaged, vec!["odd.txt".to_owned()]);
        assert_eq!(untracked, vec!["untracked.txt".to_owned()]);
    }

    #[test]
    fn collect_status_paths_respects_path_limits_and_short_lines() {
        let dir = TempDir::new().or_abort("tempdir");
        let mut lines = vec!["?".to_owned(), "A? staged-only.txt".to_owned()];
        for idx in 0..(TRACE_MAX_PATHS + 5) {
            lines.push(format!(" M unstaged-{idx}.txt"));
        }
        for idx in 0..(TRACE_MAX_PATHS + 5) {
            lines.push(format!("A  staged-{idx}.txt"));
        }
        for idx in 0..(TRACE_MAX_PATHS + 5) {
            lines.push(format!("?? untracked-{idx}.txt"));
        }
        let status_text = lines.join("\n");

        let runner = OutputOnlyRunner {
            output: Some(Output {
                status: ExitStatus::from_raw(0),
                stdout: status_text.into_bytes(),
                stderr: Vec::new(),
            }),
            fail_output: false,
        };
        let ctx = Ctx {
            runner: &runner,
            cwd: dir.path().to_path_buf(),
            io: &REAL_IO,
            env: &REAL_ENV,
            fs: &REAL_FS,
        };

        let (staged, unstaged, untracked) = collect_status_paths(&ctx);
        assert_eq!(staged.len(), TRACE_MAX_PATHS);
        assert_eq!(unstaged.len(), TRACE_MAX_PATHS);
        assert_eq!(untracked.len(), TRACE_MAX_PATHS);
        assert!(staged.iter().any(|path| path == "staged-only.txt"));
        assert!(
            !unstaged.iter().any(|path| path == "staged-only.txt"),
            "A? lines should not be counted as unstaged"
        );
    }

    #[test]
    fn collect_repo_snapshot_handles_invalid_index_and_rebase_precedence() {
        let dir = TempDir::new().or_abort("tempdir");
        init_git_repo(dir.path());
        let ctx = ctx_for(dir.path());

        let git_dir = dir.path().join(".git");
        let factor_dir = git_dir.join("factor");
        fs::create_dir_all(&factor_dir).or_abort("create factor dir");
        fs::write(factor_dir.join("commits"), "a\nb\n").or_abort("write commits");
        fs::write(factor_dir.join("current_index"), "not-a-number\n").or_abort("write index");

        let rebase_merge = git_dir.join("rebase-merge");
        fs::create_dir_all(&rebase_merge).or_abort("create rebase-merge");
        fs::write(rebase_merge.join("msgnum"), "2\n").or_abort("write msgnum");
        fs::write(rebase_merge.join("end"), "3\n").or_abort("write end");
        fs::write(
            rebase_merge.join("git-rebase-todo"),
            "pick deadbeef first\n",
        )
        .or_abort("write todo");
        fs::write(rebase_merge.join("done"), "pick feedface done\n").or_abort("write done");

        let rebase_apply = git_dir.join("rebase-apply");
        fs::create_dir_all(&rebase_apply).or_abort("create rebase-apply");
        fs::write(rebase_apply.join("next"), "9\n").or_abort("write next");
        fs::write(rebase_apply.join("last"), "10\n").or_abort("write last");

        let snapshot = collect_repo_snapshot(&ctx);
        assert_eq!(snapshot.factor_current_commit, None);
        assert_eq!(snapshot.rebase_state, Some(RebaseState::Merge));
        assert_eq!(
            snapshot.rebase_todo_head.as_deref(),
            Some("pick deadbeef first")
        );
    }

    #[test]
    fn collect_repo_snapshot_sets_current_commit_and_handles_rebase_absence() {
        let dir = TempDir::new().or_abort("tempdir");
        init_git_repo(dir.path());
        let ctx = ctx_for(dir.path());

        let git_dir = dir.path().join(".git");
        let factor_dir = git_dir.join("factor");
        fs::create_dir_all(&factor_dir).or_abort("create factor dir");
        fs::write(factor_dir.join("commits"), "one\ntwo\nthree\n").or_abort("write commits");
        fs::write(factor_dir.join("current_index"), "1\n").or_abort("write index");

        let snapshot = collect_repo_snapshot(&ctx);
        assert_eq!(snapshot.factor_current_commit.as_deref(), Some("two"));
        assert_eq!(snapshot.rebase_state, None);
    }

    #[test]
    fn collect_repo_snapshot_reads_rebase_apply_state_when_merge_is_absent() {
        let dir = TempDir::new().or_abort("tempdir");
        init_git_repo(dir.path());
        let ctx = ctx_for(dir.path());

        let git_dir = dir.path().join(".git");
        let rebase_apply = git_dir.join("rebase-apply");
        fs::create_dir_all(&rebase_apply).or_abort("create rebase-apply");
        fs::write(rebase_apply.join("next"), "4\n").or_abort("write next");
        fs::write(rebase_apply.join("last"), "8\n").or_abort("write last");
        fs::write(rebase_apply.join("patch"), "diff --git\n").or_abort("write patch");

        let snapshot = collect_repo_snapshot(&ctx);
        assert_eq!(snapshot.rebase_state, Some(RebaseState::Apply));
        assert_eq!(snapshot.rebase_todo_head.as_deref(), Some("patch"));
    }

    #[test]
    fn collect_repo_snapshot_skips_empty_toplevel_output() {
        let dir = TempDir::new().or_abort("tempdir");
        let runner = OutputOnlyRunner {
            output: Some(Output {
                status: ExitStatus::from_raw(0),
                stdout: b"\n".to_vec(),
                stderr: Vec::new(),
            }),
            fail_output: false,
        };
        let ctx = Ctx {
            runner: &runner,
            cwd: dir.path().to_path_buf(),
            io: &REAL_IO,
            env: &REAL_ENV,
            fs: &REAL_FS,
        };

        let snapshot = collect_repo_snapshot(&ctx);
        assert_eq!(snapshot.head, None);
        assert_eq!(snapshot.head_tree, None);
        let git_dir = PathBuf::from(snapshot.git_dir.or_abort("git_dir from snapshot"));
        assert_eq!(git_dir, dir.path());
        assert_eq!(snapshot.toplevel, None);
    }

    #[test]
    fn collect_repo_snapshot_handles_show_toplevel_output_failure() {
        let dir = TempDir::new().or_abort("tempdir");
        let runner = ShowTopLevelFailRunner;
        let ctx = Ctx {
            runner: &runner,
            cwd: dir.path().to_path_buf(),
            io: &REAL_IO,
            env: &REAL_ENV,
            fs: &REAL_FS,
        };

        let status = runner
            .status("git", &["status"], &[], false, dir.path())
            .or_abort("runner status");
        assert!(status.success(), "status should report success");

        let snapshot = collect_repo_snapshot(&ctx);
        let expected_git_dir = dir.path().join(".git");
        assert_eq!(snapshot.head.as_deref(), Some("deadbeef"));
        assert_eq!(snapshot.head_tree.as_deref(), Some("deadbeef"));
        assert_eq!(
            snapshot.git_dir.as_deref(),
            Some(expected_git_dir.to_string_lossy().as_ref())
        );
        assert_eq!(snapshot.toplevel, None);
    }

    #[test]
    fn trace_process_command_writes_when_tracing_is_enabled() {
        let dir = TempDir::new().or_abort("tempdir");
        init_git_repo(dir.path());

        let trace_path = dir.path().join("trace-process.jsonl");
        let env = TestEnv {
            cwd: dir.path().to_path_buf(),
            trace_log: Some(trace_path.as_os_str().to_os_string()),
        };
        let ctx = ctx_with_env(dir.path(), &env);
        let before = collect_repo_snapshot(&ctx);
        let after = collect_repo_snapshot(&ctx);

        trace_process_command(
            &ctx,
            ProcessTrace {
                mode: "status",
                bin: "git",
                args: &["status"],
                envs: &[],
                quiet: false,
                duration_ms: 1,
                exit_code: Some(i32::default()),
                stdout: None,
                stderr: None,
                spawned: true,
                before: &before,
                after: &after,
            },
        );

        let trace = fs::read_to_string(&trace_path).or_abort("read trace");
        assert!(trace.contains("\"event\":\"process\""), "trace: {trace}");
        assert!(trace.contains("\"mode\":\"status\""), "trace: {trace}");
    }

    #[test]
    fn trace_process_command_returns_early_without_trace_env() {
        let dir = TempDir::new().or_abort("tempdir");
        init_git_repo(dir.path());
        let env = TestEnv {
            cwd: dir.path().to_path_buf(),
            trace_log: None,
        };
        let ctx = ctx_with_env(dir.path(), &env);
        let before = collect_repo_snapshot(&ctx);
        let after = collect_repo_snapshot(&ctx);

        trace_process_command(
            &ctx,
            ProcessTrace {
                mode: "status",
                bin: "git",
                args: &["status"],
                envs: &[],
                quiet: false,
                duration_ms: 1,
                exit_code: Some(i32::default()),
                stdout: None,
                stderr: None,
                spawned: true,
                before: &before,
                after: &after,
            },
        );

        let has_trace_file = fs::read_dir(dir.path())
            .or_abort("read directory")
            .filter_map(Result::ok)
            .any(|entry| {
                entry
                    .path()
                    .extension()
                    .is_some_and(|extension| extension == "jsonl")
            });
        assert!(
            !has_trace_file,
            "trace file should not be created when trace env is disabled"
        );
    }

    #[test]
    fn git_output_with_tracing_covers_success_and_nonzero_status_paths() {
        let dir = TempDir::new().or_abort("tempdir");
        init_git_repo(dir.path());

        let trace_path = dir.path().join("trace-output-status.jsonl");
        let env = TestEnv {
            cwd: dir.path().to_path_buf(),
            trace_log: Some(trace_path.as_os_str().to_os_string()),
        };
        let ctx = ctx_with_env(dir.path(), &env);

        let status_out = git_output_with(&ctx, "git", &["status", "--porcelain"])
            .or_abort("status should succeed");
        assert!(
            !status_out.contains("fatal:"),
            "unexpected status output: {status_out}"
        );

        let err = git_output_with(&ctx, "git", &["definitely-not-a-command"])
            .err_or_abort("unknown git command should fail");
        let message = git_command_message(&err).or_abort("expected GitCommand");
        assert!(
            message.contains("definitely-not-a-command"),
            "unexpected error: {err:?}"
        );

        let trace = fs::read_to_string(&trace_path).or_abort("read trace");
        assert!(trace.contains("\"mode\":\"output\""), "trace: {trace}");
        assert!(trace.contains("\"spawned\":true"), "trace: {trace}");
    }

    #[test]
    fn trace_spawn_error_paths_cover_enabled_and_disabled_tracing() {
        let dir = TempDir::new().or_abort("tempdir");
        let env_disabled = TestEnv {
            cwd: dir.path().to_path_buf(),
            trace_log: None,
        };
        let ctx_disabled = ctx_with_env(dir.path(), &env_disabled);

        let status_err_disabled = command_status_with(
            &ctx_disabled,
            "git-factor-not-a-real-status-binary",
            &["status"],
            &[],
            false,
        )
        .err_or_abort("status spawn should fail");
        assert_git_command(&status_err_disabled);

        let output_err_disabled = git_output_with(
            &ctx_disabled,
            "git-factor-not-a-real-output-binary",
            &["status"],
        )
        .err_or_abort("output spawn should fail");
        assert_git_command(&output_err_disabled);

        let trace_path = dir.path().join("trace-errors.jsonl");
        let env_enabled = TestEnv {
            cwd: dir.path().to_path_buf(),
            trace_log: Some(trace_path.as_os_str().to_os_string()),
        };
        let ctx_enabled = ctx_with_env(dir.path(), &env_enabled);

        let status_err_enabled = command_status_with(
            &ctx_enabled,
            "git-factor-not-a-real-status-binary",
            &["status"],
            &[],
            false,
        )
        .err_or_abort("status spawn should fail");
        assert_git_command(&status_err_enabled);

        let output_err_enabled = git_output_with(
            &ctx_enabled,
            "git-factor-not-a-real-output-binary",
            &["status"],
        )
        .err_or_abort("output spawn should fail");
        assert_git_command(&output_err_enabled);

        let trace = fs::read_to_string(&trace_path).or_abort("read trace");
        assert!(trace.contains("\"mode\":\"status\""), "trace: {trace}");
        assert!(trace.contains("\"mode\":\"output\""), "trace: {trace}");
    }

    #[test]
    fn append_and_trace_process_return_early_when_tracing_is_disabled_or_unwritable() {
        let dir = TempDir::new().or_abort("tempdir");
        let env_none = TestEnv {
            cwd: dir.path().to_path_buf(),
            trace_log: None,
        };
        let ctx_none = ctx_with_env(dir.path(), &env_none);
        append_trace_line(&ctx_none, "ignored");

        let env_bad = TestEnv {
            cwd: dir.path().to_path_buf(),
            trace_log: Some(dir.path().as_os_str().to_os_string()),
        };
        let ctx_bad = ctx_with_env(dir.path(), &env_bad);
        append_trace_line(&ctx_bad, "ignored");

        let before = RepoSnapshot::default();
        let after = RepoSnapshot::default();
        trace_process_command(
            &ctx_none,
            ProcessTrace {
                mode: "status",
                bin: "git",
                args: &["status"],
                envs: &[],
                quiet: false,
                duration_ms: 0,
                exit_code: None,
                stdout: None,
                stderr: None,
                spawned: false,
                before: &before,
                after: &after,
            },
        );
    }

    #[test]
    fn append_trace_line_handles_trace_path_without_parent() {
        let dir = TempDir::new().or_abort("tempdir");
        let env = TestEnv {
            cwd: dir.path().to_path_buf(),
            trace_log: Some(OsString::from("/")),
        };
        let ctx = ctx_with_env(dir.path(), &env);

        append_trace_line(&ctx, "{\"event\":\"noop\"}");
        let entries = fs::read_dir(dir.path()).or_abort("read tempdir");
        assert_eq!(entries.count(), 0);
    }

    #[test]
    fn env_methods_and_run_git_non_interactive_success_path() {
        let dir = TempDir::new().or_abort("tempdir");
        init_git_repo(dir.path());

        let trace_path = dir.path().join("trace.jsonl");
        let env = TestEnv {
            cwd: dir.path().to_path_buf(),
            trace_log: Some(trace_path.as_os_str().to_os_string()),
        };
        let ctx = ctx_with_env(dir.path(), &env);

        let cwd = env.current_dir().or_abort("cwd");
        assert_eq!(cwd, dir.path().to_path_buf());
        let exe = env.current_exe().or_abort("current exe");
        assert!(exe.is_absolute(), "current_exe should be absolute: {exe:?}");
        assert!(env.var_os(TRACE_LOG_ENV).is_some());

        run_git_non_interactive(&ctx, &["status"]).or_abort("git status should succeed");
    }

    #[test]
    fn git_output_with_spawn_error_covers_trace_toggle_and_before_none_paths() {
        let dir = TempDir::new().or_abort("tempdir");
        let runner = OutputOnlyRunner {
            output: None,
            fail_output: true,
        };

        let toggle_env = ToggleTraceEnv {
            cwd: dir.path().to_path_buf(),
            trace_path: dir
                .path()
                .join("trace-toggle.jsonl")
                .as_os_str()
                .to_os_string(),
            calls: Mutex::new(0),
        };
        assert_eq!(
            toggle_env.current_dir().or_abort("toggle current_dir"),
            dir.path().to_path_buf()
        );
        let exe = toggle_env.current_exe().or_abort("toggle current_exe");
        assert!(exe.is_absolute(), "current_exe should be absolute: {exe:?}");
        assert!(toggle_env.var_os("UNRELATED_KEY").is_none());

        let ctx_toggle = Ctx {
            runner: &runner,
            cwd: dir.path().to_path_buf(),
            io: &REAL_IO,
            env: &toggle_env,
            fs: &REAL_FS,
        };
        let with_trace_err = git_output_with(&ctx_toggle, "git", &["status"])
            .err_or_abort("expected output failure");
        let with_trace_message =
            git_command_message(&with_trace_err).or_abort("expected GitCommand");
        assert!(
            with_trace_message.contains("forced output failure"),
            "err was: {with_trace_err:?}"
        );

        let no_trace_env = TestEnv {
            cwd: dir.path().to_path_buf(),
            trace_log: None,
        };
        let ctx_no_trace = Ctx {
            runner: &runner,
            cwd: dir.path().to_path_buf(),
            io: &REAL_IO,
            env: &no_trace_env,
            fs: &REAL_FS,
        };
        let without_trace_err = git_output_with(&ctx_no_trace, "git", &["status"])
            .err_or_abort("expected output failure");
        let without_trace_message =
            git_command_message(&without_trace_err).or_abort("expected GitCommand");
        assert!(
            without_trace_message.contains("forced output failure"),
            "err was: {without_trace_err:?}"
        );
    }

    #[test]
    fn git_output_with_success_without_trace_covers_after_default_snapshot_path() {
        let dir = TempDir::new().or_abort("tempdir");
        let runner = OutputOnlyRunner {
            output: Some(Output {
                status: ExitStatus::from_raw(0),
                stdout: b"ok\n".to_vec(),
                stderr: Vec::new(),
            }),
            fail_output: false,
        };
        let no_trace_env = TestEnv {
            cwd: dir.path().to_path_buf(),
            trace_log: None,
        };
        let ctx_no_trace = Ctx {
            runner: &runner,
            cwd: dir.path().to_path_buf(),
            io: &REAL_IO,
            env: &no_trace_env,
            fs: &REAL_FS,
        };
        let output =
            git_output_with(&ctx_no_trace, "git", &["status"]).or_abort("expected output success");
        assert_eq!(output, "ok");
    }

    #[test]
    fn git_output_with_trace_covers_snapshot_collection_paths() {
        let dir = TempDir::new().or_abort("tempdir");
        let trace_path = dir.path().join("trace-git-output.jsonl");
        let runner = OutputOnlyRunner {
            output: Some(Output {
                status: ExitStatus::from_raw(0),
                stdout: b"ok\n".to_vec(),
                stderr: Vec::new(),
            }),
            fail_output: false,
        };
        let trace_env = TestEnv {
            cwd: dir.path().to_path_buf(),
            trace_log: Some(trace_path.as_os_str().to_os_string()),
        };
        let ctx = Ctx {
            runner: &runner,
            cwd: dir.path().to_path_buf(),
            io: &REAL_IO,
            env: &trace_env,
            fs: &REAL_FS,
        };

        let output = git_output(&ctx, &["status"]).or_abort("expected output success");
        assert_eq!(output, "ok");

        let trace = fs::read_to_string(&trace_path).or_abort("read trace output");
        assert!(trace.contains("\"mode\":\"output\""), "trace was: {trace}");
    }

    #[test]
    fn run_git_non_interactive_reports_nonzero_exit() {
        struct NonInteractiveFailRunner;

        impl Runner for NonInteractiveFailRunner {
            fn output(&self, _bin: &str, _args: &[&str], _cwd: &Path) -> io::Result<Output> {
                Err(io::Error::other("output should not be called"))
            }

            fn status(
                &self,
                _bin: &str,
                args: &[&str],
                _envs: &[(&str, &str)],
                _quiet: bool,
                _cwd: &Path,
            ) -> io::Result<ExitStatus> {
                if args != ["status"] {
                    return Err(io::Error::other("unexpected args"));
                }
                Ok(ExitStatus::from_raw(256))
            }
        }

        let dir = TempDir::new().or_abort("tempdir");
        let runner = NonInteractiveFailRunner;
        let env = TestEnv {
            cwd: dir.path().to_path_buf(),
            trace_log: None,
        };
        let ctx = Ctx {
            runner: &runner,
            cwd: dir.path().to_path_buf(),
            io: &REAL_IO,
            env: &env,
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
        let status_err = runner
            .status("git", &["not-status"], &[], false, dir.path())
            .err_or_abort("unexpected args should fail");
        assert_eq!(status_err.to_string(), "unexpected args");

        let err =
            run_git_non_interactive(&ctx, &["status"]).err_or_abort("non-zero status should fail");
        let message = git_command_message(&err).or_abort("expected GitCommand");
        assert!(
            message.contains("git status failed (exit 1)"),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    fn run_git_with_reports_nonzero_exit() {
        struct RunGitWithFailRunner;

        impl Runner for RunGitWithFailRunner {
            fn output(&self, _bin: &str, _args: &[&str], _cwd: &Path) -> io::Result<Output> {
                Err(io::Error::other("output should not be called"))
            }

            fn status(
                &self,
                _bin: &str,
                args: &[&str],
                _envs: &[(&str, &str)],
                _quiet: bool,
                _cwd: &Path,
            ) -> io::Result<ExitStatus> {
                if args != ["status"] {
                    return Err(io::Error::other("unexpected args"));
                }
                Ok(ExitStatus::from_raw(512))
            }
        }

        let dir = TempDir::new().or_abort("tempdir");
        let runner = RunGitWithFailRunner;
        let env = TestEnv {
            cwd: dir.path().to_path_buf(),
            trace_log: None,
        };
        let ctx = Ctx {
            runner: &runner,
            cwd: dir.path().to_path_buf(),
            io: &REAL_IO,
            env: &env,
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
        let status_err = runner
            .status("git", &["not-status"], &[], false, dir.path())
            .err_or_abort("unexpected args should fail");
        assert_eq!(status_err.to_string(), "unexpected args");

        let err =
            run_git_with(&ctx, "git", &["status"]).err_or_abort("non-zero status should fail");
        let message = git_command_message(&err).or_abort("expected GitCommand");
        assert!(
            message.contains("git status failed (exit 2)"),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    fn run_git_with_returns_ok_for_zero_status() {
        let dir = TempDir::new().or_abort("tempdir");
        init_git_repo(dir.path());
        let ctx = ctx_for(dir.path());
        run_git_with(&ctx, "git", &["status"]).or_abort("status should succeed");
    }
}

#[cfg(test)]
mod proptests {
    mod git_output {
        use super::super::tests::{BufferIo, OutputOnlyRunner, TestEnv, arrange_query_context};
        use super::*;
        use std::os::unix::process::ExitStatusExt as _;

        use super::super::output_contracts::{RecordingRunner, snapshot, successful};
        use std::ffi::OsString;
        use tempfile::TempDir;

        proptest! {
            #[test]
            fn disabled_trace_spawn_failure_makes_only_the_requested_query(
                trace_log in prop_oneof![Just(None), Just(Some(OsString::new()))],
                args in vec("[a-z]{1,12}", 0..4),
                diagnostic in "[a-z ]{1,32}",
            ) {
                let directory = TempDir::new().or_abort("output contract directory");
                let runner = RecordingRunner::new(vec![Err(io::Error::other(diagnostic.clone()))]);
                let environment = TestEnv {
                    cwd: directory.path().to_path_buf(),
                    trace_log,
                };
                let io = BufferIo::default();
                let ctx = Ctx {
                    runner: &runner,
                    cwd: environment.cwd.clone(),
                    env: &environment,
                    io: &io,
                    fs: &REAL_FS,
                };
                let arguments = args.iter().map(String::as_str).collect::<Vec<_>>();
                let expected_error = format!(
                    "git {}: {diagnostic}",
                    args.first().map_or("", String::as_str)
                );

                let result = super::super::git_output(&ctx, &arguments);

                let error = result.err_or_abort("requested query cannot spawn");
                let FactorError::GitCommand(message) = error else {
                    return Err(TestCaseError::fail("spawn failure must be a GitCommand"));
                };
                prop_assert_eq!(message.as_str(), expected_error.as_str());
                prop_assert_eq!(
                    runner.requests(),
                    vec![("git".to_owned(), args, directory.path().to_path_buf())]
                );
                prop_assert_eq!(io.stdout(), "");
                prop_assert_eq!(io.stderr(), "");
                prop_assert_eq!(
                    fs::read_dir(directory.path())
                        .or_abort("read contract directory")
                        .count(),
                    0
                );
            }
            #[test]
            fn enabled_trace_preserves_generated_launch_errors_and_snapshots(payload in "[a-z]{1,32}") {
                let directory = TempDir::new().or_abort("output contract directory");
                let trace_path = directory.path().join("trace.jsonl");
                let mut replies = snapshot("before-head", "before-tree", directory.path());
                replies.push(Err(io::Error::other(payload.clone())));
                replies.extend(snapshot("after-head", "after-tree", directory.path()));
                let runner = RecordingRunner::new(replies);
                let environment = TestEnv {
                    cwd: directory.path().to_path_buf(),
                    trace_log: Some(trace_path.as_os_str().to_os_string()),
                };
                let io = BufferIo::default();
                let ctx = Ctx {
                    runner: &runner,
                    cwd: environment.cwd.clone(),
                    env: &environment,
                    io: &io,
                    fs: &REAL_FS,
                };

                let result = super::super::git_output(&ctx, &["contract-query"]);

                let error = result.err_or_abort("requested query cannot spawn");
                let FactorError::GitCommand(message) = error else {
                    return Err(TestCaseError::fail("spawn failure must be a GitCommand"));
                };
                prop_assert_eq!(message.as_str(), format!("git contract-query: {payload}"));
                let stdout_json = "null".to_owned();
                let stderr_json = serde_json::to_string(&payload).or_abort("serialize exact error");
                prop_assert_eq!(
                    runner.requests(),
                    [
                        vec!["rev-parse", "--verify", "HEAD"],
                        vec!["rev-parse", "--verify", "HEAD^{tree}"],
                        vec!["rev-parse", "--git-dir"],
                        vec!["rev-parse", "--show-toplevel"],
                        vec![
                            "--no-optional-locks",
                            "status",
                            "--porcelain=v1",
                            "--untracked-files=all"
                        ],
                        vec!["contract-query"],
                        vec!["rev-parse", "--verify", "HEAD"],
                        vec!["rev-parse", "--verify", "HEAD^{tree}"],
                        vec!["rev-parse", "--git-dir"],
                        vec!["rev-parse", "--show-toplevel"],
                        vec![
                            "--no-optional-locks",
                            "status",
                            "--porcelain=v1",
                            "--untracked-files=all"
                        ],
                    ]
                    .into_iter()
                    .map(|args| (
                        "git".to_owned(),
                        args.into_iter().map(str::to_owned).collect(),
                        directory.path().to_path_buf()
                    ))
                    .collect::<Vec<_>>()
                );
                prop_assert_eq!(io.stdout(), "");
                prop_assert_eq!(io.stderr(), "");
                let trace = fs::read_to_string(&trace_path).or_abort("read output trace");
                prop_assert_eq!(trace.lines().count(), 1);
                prop_assert!(trace.ends_with('\n'));
                let mut record: serde_json::Value =
                    serde_json::from_str(trace.trim_end_matches('\n')).or_abort("parse output trace");
                let object = record.as_object_mut().or_abort("trace is an object");
                prop_assert!(
                    object
                        .remove("ts_unix_ms")
                        .and_then(|value| value.as_u64())
                        .is_some()
                );
                prop_assert!(
                    object
                        .remove("duration_ms")
                        .and_then(|value| value.as_u64())
                        .is_some()
                );
                let cwd_json = serde_json::to_string(&directory.path().to_string_lossy())
                    .or_abort("serialize exact contract cwd");
                let git_dir_json = serde_json::to_string(&directory.path().join(".git").to_string_lossy())
                    .or_abort("serialize exact Git directory");
                let expected: serde_json::Value = serde_json::from_str(&format!(r#"{{
                            "event":"process","mode":"output","bin":"git","args":["contract-query"],"env":[],"quiet":false,
                            "spawned":false,"exit_code":null,"stdout":{stdout_json},"stderr":{stderr_json},
                            "before_head":"before-head","before_head_tree":"before-tree","before_git_dir":{git_dir_json},"before_toplevel":{cwd_json},
                            "before_staged_paths":[],"before_unstaged_paths":[],"before_untracked_paths":[],
                            "before_factor_current_index":null,"before_factor_split_count":null,"before_factor_requires_rebase":null,"before_factor_expected_tree":null,"before_factor_current_commit":null,
                            "before_rebase_state":null,"before_rebase_msgnum":null,"before_rebase_end":null,"before_rebase_todo_head":null,"before_rebase_done_tail":null,
                            "after_head":"after-head","after_head_tree":"after-tree","after_git_dir":{git_dir_json},"after_toplevel":{cwd_json},
                            "after_staged_paths":[],"after_unstaged_paths":[],"after_untracked_paths":[],
                            "after_factor_current_index":null,"after_factor_split_count":null,"after_factor_requires_rebase":null,"after_factor_expected_tree":null,"after_factor_current_commit":null,
                            "after_rebase_state":null,"after_rebase_msgnum":null,"after_rebase_end":null,"after_rebase_todo_head":null,"after_rebase_done_tail":null
                        }}"#)).or_abort("parse literal trace oracle");
                prop_assert_eq!(record, expected);
            }
            #[test]
            fn enabled_trace_preserves_generated_success_bytes_and_snapshots(payload in "[a-z]{1,32}") {
                let directory = TempDir::new().or_abort("output contract directory");
                let trace_path = directory.path().join("trace.jsonl");
                let mut replies = snapshot("before-head", "before-tree", directory.path());
                replies.push(Ok(successful(
                    format!(" \t{payload}\n").as_bytes(),
                    b"successful diagnostic",
                )));
                replies.extend(snapshot("after-head", "after-tree", directory.path()));
                let runner = RecordingRunner::new(replies);
                let environment = TestEnv {
                    cwd: directory.path().to_path_buf(),
                    trace_log: Some(trace_path.as_os_str().to_os_string()),
                };
                let io = BufferIo::default();
                let ctx = Ctx {
                    runner: &runner,
                    cwd: environment.cwd.clone(),
                    env: &environment,
                    io: &io,
                    fs: &REAL_FS,
                };

                let result = super::super::git_output(&ctx, &["contract-query"]);

                let actual = result.or_abort("requested query succeeds");
                prop_assert_eq!(actual.as_str(), payload.as_str());
                let stdout_json =
                    serde_json::to_string(&format!(" \t{payload}\n")).or_abort("serialize exact stdout");
                let stderr_json = "\"successful diagnostic\"".to_owned();
                prop_assert_eq!(
                    runner.requests(),
                    [
                        vec!["rev-parse", "--verify", "HEAD"],
                        vec!["rev-parse", "--verify", "HEAD^{tree}"],
                        vec!["rev-parse", "--git-dir"],
                        vec!["rev-parse", "--show-toplevel"],
                        vec![
                            "--no-optional-locks",
                            "status",
                            "--porcelain=v1",
                            "--untracked-files=all"
                        ],
                        vec!["contract-query"],
                        vec!["rev-parse", "--verify", "HEAD"],
                        vec!["rev-parse", "--verify", "HEAD^{tree}"],
                        vec!["rev-parse", "--git-dir"],
                        vec!["rev-parse", "--show-toplevel"],
                        vec![
                            "--no-optional-locks",
                            "status",
                            "--porcelain=v1",
                            "--untracked-files=all"
                        ],
                    ]
                    .into_iter()
                    .map(|args| (
                        "git".to_owned(),
                        args.into_iter().map(str::to_owned).collect(),
                        directory.path().to_path_buf()
                    ))
                    .collect::<Vec<_>>()
                );
                prop_assert_eq!(io.stdout(), "");
                prop_assert_eq!(io.stderr(), "");
                let trace = fs::read_to_string(&trace_path).or_abort("read output trace");
                prop_assert_eq!(trace.lines().count(), 1);
                prop_assert!(trace.ends_with('\n'));
                let mut record: serde_json::Value =
                    serde_json::from_str(trace.trim_end_matches('\n')).or_abort("parse output trace");
                let object = record.as_object_mut().or_abort("trace is an object");
                prop_assert!(
                    object
                        .remove("ts_unix_ms")
                        .and_then(|value| value.as_u64())
                        .is_some()
                );
                prop_assert!(
                    object
                        .remove("duration_ms")
                        .and_then(|value| value.as_u64())
                        .is_some()
                );
                let cwd_json = serde_json::to_string(&directory.path().to_string_lossy())
                    .or_abort("serialize exact contract cwd");
                let git_dir_json = serde_json::to_string(&directory.path().join(".git").to_string_lossy())
                    .or_abort("serialize exact Git directory");
                let expected: serde_json::Value = serde_json::from_str(&format!(r#"{{
                            "event":"process","mode":"output","bin":"git","args":["contract-query"],"env":[],"quiet":false,
                            "spawned":true,"exit_code":0,"stdout":{stdout_json},"stderr":{stderr_json},
                            "before_head":"before-head","before_head_tree":"before-tree","before_git_dir":{git_dir_json},"before_toplevel":{cwd_json},
                            "before_staged_paths":[],"before_unstaged_paths":[],"before_untracked_paths":[],
                            "before_factor_current_index":null,"before_factor_split_count":null,"before_factor_requires_rebase":null,"before_factor_expected_tree":null,"before_factor_current_commit":null,
                            "before_rebase_state":null,"before_rebase_msgnum":null,"before_rebase_end":null,"before_rebase_todo_head":null,"before_rebase_done_tail":null,
                            "after_head":"after-head","after_head_tree":"after-tree","after_git_dir":{git_dir_json},"after_toplevel":{cwd_json},
                            "after_staged_paths":[],"after_unstaged_paths":[],"after_untracked_paths":[],
                            "after_factor_current_index":null,"after_factor_split_count":null,"after_factor_requires_rebase":null,"after_factor_expected_tree":null,"after_factor_current_commit":null,
                            "after_rebase_state":null,"after_rebase_msgnum":null,"after_rebase_end":null,"after_rebase_todo_head":null,"after_rebase_done_tail":null
                        }}"#)).or_abort("parse literal trace oracle");
                prop_assert_eq!(record, expected);
            }
            #[test]
            fn whitespace_stderr_reports_each_nonzero_exit_status(
                exit_code in 1..=u8::MAX,
                diagnostic in "[ \t\r\n]{0,8}",
            ) {
                let raw_status = i32::from(exit_code)
                    .checked_shl(u8::BITS)
                    .or_abort("native wait-status shift count is below the i32 width");
                let status = ExitStatus::from_raw(raw_status);
                let expected = status.to_string();
                let runner = OutputOnlyRunner {
                    output: Some(Output {
                        status,
                        stdout: Vec::new(),
                        stderr: diagnostic.into_bytes(),
                    }),
                    ..OutputOnlyRunner::default()
                };
                let environment = TestEnv {
                    cwd: PathBuf::from("."),
                    trace_log: None,
                };
                let io = BufferIo::default();
                let ctx = arrange_query_context(&runner, &environment, &io);

                let result = super::super::git_output(&ctx, &["rev-parse", "--short", "HEAD"]);

                let Err(FactorError::GitCommand(message)) = result else {
                    return Err(TestCaseError::fail(format!(
                        "expected GitCommand, observed {result:?}"
                    )));
                };
                prop_assert_eq!(message.as_str(), expected.as_str());
            }

            #[test]
            fn preserves_success_bytes_or_spawn_failure(
                reply in prop_oneof![
                    Just(None),
                    vec(any::<u8>(), 0..64).prop_map(Some),
                ],
            ) {
                // A spawn failure carries no unused stdout world. Success bytes independently
                // determine the exact lossy-decoded, trimmed public response.
                let expected_reply = reply.as_deref().map(|bytes| {
                    String::from_utf8_lossy(bytes).trim().to_owned()
                });
                let runner = OutputOnlyRunner {
                    fail_output: reply.is_none(),
                    output: reply.map(|stdout| Output {
                        status: ExitStatus::from_raw(0),
                        stdout,
                        stderr: b"ignored successful stderr".to_vec(),
                    }),
                };
                let environment = TestEnv {
                    cwd: PathBuf::from("."),
                    trace_log: None,
                };
                let io = BufferIo::default();
                let ctx = arrange_query_context(&runner, &environment, &io);

                let result = super::super::git_output(&ctx, &["rev-parse", "--short", "HEAD"]);

                if let Some(expected) = expected_reply {
                    let Ok(actual) = result else {
                        return Err(TestCaseError::fail(format!(
                            "successful output rejected: {result:?}"
                        )));
                    };
                    prop_assert_eq!(actual, expected);
                } else {
                    let Err(FactorError::GitCommand(message)) = result else {
                        return Err(TestCaseError::fail(format!(
                            "expected spawn diagnostic, observed {result:?}"
                        )));
                    };
                    prop_assert_eq!(
                        message.as_str(),
                        "git rev-parse: forced output failure",
                    );
                }
            }

            #[test]
            fn nonempty_stderr_takes_precedence_over_status(
                exit_code in 1..=u8::MAX,
                message in "[A-Za-z0-9_]{1,40}",
                prefix in "[ \t\r\n]{0,8}",
                suffix in "[ \t\r\n]{0,8}",
            ) {
                let raw_status = i32::from(exit_code)
                    .checked_shl(u8::BITS)
                    .or_abort("native wait-status shift count is below the i32 width");
                let status = ExitStatus::from_raw(raw_status);
                let diagnostic = format!("{prefix}{message}{suffix}");
                let runner = OutputOnlyRunner {
                    output: Some(Output {
                        status,
                        stdout: Vec::new(),
                        stderr: diagnostic.into_bytes(),
                    }),
                    ..OutputOnlyRunner::default()
                };
                let environment = TestEnv {
                    cwd: PathBuf::from("."),
                    trace_log: None,
                };
                let io = BufferIo::default();
                let ctx = arrange_query_context(&runner, &environment, &io);

                let result = super::super::git_output(&ctx, &["rev-parse", "--short", "HEAD"]);

                let Err(FactorError::GitCommand(error_message)) = result else {
                    return Err(TestCaseError::fail(format!(
                        "expected GitCommand, observed {result:?}"
                    )));
                };
                prop_assert_eq!(error_message.as_str(), message.as_str());
            }
        }
    }

    mod git_output_with {
        use super::super::tests::{BufferIo, OutputOnlyRunner, TestEnv, arrange_query_context};
        use super::*;
        use std::os::unix::process::ExitStatusExt as _;

        proptest! {
            #[test]
            fn whitespace_stderr_reports_each_nonzero_exit_status(
                exit_code in 1..=u8::MAX,
                diagnostic in "[ \t\r\n]{0,8}",
            ) {
                let raw_status = i32::from(exit_code)
                    .checked_shl(u8::BITS)
                    .or_abort("native wait-status shift count is below the i32 width");
                let status = ExitStatus::from_raw(raw_status);
                let expected = status.to_string();
                let runner = OutputOnlyRunner {
                    output: Some(Output {
                        status,
                        stdout: Vec::new(),
                        stderr: diagnostic.into_bytes(),
                    }),
                    ..OutputOnlyRunner::default()
                };
                let environment = TestEnv {
                    cwd: PathBuf::from("."),
                    trace_log: None,
                };
                let io = BufferIo::default();
                let ctx = arrange_query_context(&runner, &environment, &io);

                let result = super::super::git_output_with(
                    &ctx,
                    "git",
                    &["rev-parse", "--short", "HEAD"],
                );

                let Err(FactorError::GitCommand(message)) = result else {
                    return Err(TestCaseError::fail(format!(
                        "expected GitCommand, observed {result:?}"
                    )));
                };
                prop_assert_eq!(message.as_str(), expected.as_str());
            }

            #[test]
            fn nonempty_stderr_takes_precedence_over_status(
                exit_code in 1..=u8::MAX,
                message in "[A-Za-z0-9_]{1,40}",
                prefix in "[ \t\r\n]{0,8}",
                suffix in "[ \t\r\n]{0,8}",
            ) {
                let raw_status = i32::from(exit_code)
                    .checked_shl(u8::BITS)
                    .or_abort("native wait-status shift count is below the i32 width");
                let status = ExitStatus::from_raw(raw_status);
                let diagnostic = format!("{prefix}{message}{suffix}");
                let runner = OutputOnlyRunner {
                    output: Some(Output {
                        status,
                        stdout: Vec::new(),
                        stderr: diagnostic.into_bytes(),
                    }),
                    ..OutputOnlyRunner::default()
                };
                let environment = TestEnv {
                    cwd: PathBuf::from("."),
                    trace_log: None,
                };
                let io = BufferIo::default();
                let ctx = arrange_query_context(&runner, &environment, &io);

                let result = super::super::git_output_with(
                    &ctx,
                    "git",
                    &["rev-parse", "--short", "HEAD"],
                );

                let Err(FactorError::GitCommand(error_message)) = result else {
                    return Err(TestCaseError::fail(format!(
                        "expected GitCommand, observed {result:?}"
                    )));
                };
                prop_assert_eq!(error_message.as_str(), message.as_str());
            }
        }
    }

    use core::fmt::Write as _;

    use proptest::collection::vec;
    use proptest::prelude::*;
    use proptest::string::string_regex;

    use super::*;

    const INDENT_REGEX: &str = "[ \t]{0,6}";
    const COMMENT_PAYLOAD_REGEX: &str = "[A-Za-z0-9._/-]{0,24}";

    fn indent() -> impl Strategy<Value = String> {
        string_regex(INDENT_REGEX).or_abort("valid regex")
    }

    fn non_comment_line() -> impl Strategy<Value = String> {
        let regex = format!("[A-Za-z0-9][A-Za-z0-9 ._/-]{{0,{COMMIT_SHA_HEX_LEN}}}");
        string_regex(regex.as_str()).or_abort("valid regex")
    }

    fn blank_or_comment_line() -> impl Strategy<Value = String> {
        prop_oneof![
            indent(),
            (
                indent(),
                string_regex(COMMENT_PAYLOAD_REGEX).or_abort("valid regex"),
            )
                .prop_map(|(indent, payload)| format!("{indent}#{payload}")),
        ]
    }

    proptest! {
        #[test]
        fn proptest_trace_text_limit_returns_prefix_with_byte_cap(input in any::<String>()) {
            let limited = trace_text_limit(input.as_str());

            prop_assert!(input.starts_with(limited.as_str()));
            prop_assert!(limited.len() <= TRACE_MAX_TEXT_BYTES);
            if input.len() <= TRACE_MAX_TEXT_BYTES {
                prop_assert_eq!(limited, input);
            }
        }

        #[test]
        fn proptest_json_escape_strips_raw_control_chars(input in any::<String>()) {
            let escaped = json_escape(input.as_str());

            prop_assert!(!escaped.chars().any(char::is_control));
            prop_assert!(!escaped.contains('\n'));
            prop_assert!(!escaped.contains('\r'));
            prop_assert!(!escaped.contains('\t'));
        }

        #[test]
        fn proptest_first_rebase_todo_line_returns_first_non_comment_line(
            prefix in vec(blank_or_comment_line(), 0..8),
            first in non_comment_line(),
            suffix in vec(blank_or_comment_line(), 0..8),
        ) {
            let mut lines = Vec::new();
            lines.extend(prefix);
            lines.push(format!("  {first}  "));
            lines.extend(suffix);
            let text = lines.join("\n");

            prop_assert_eq!(
                first_rebase_todo_line(text.as_str()),
                Some(first.trim().to_owned())
            );
        }

        #[test]
        fn proptest_first_rebase_todo_line_returns_none_for_blank_or_comment_only(
            lines in vec(blank_or_comment_line(), 0..16)
        ) {
            let text = lines.join("\n");
            prop_assert_eq!(first_rebase_todo_line(text.as_str()), None);
        }

        #[test]
        fn proptest_last_non_empty_line_returns_trimmed_last_content(
            content in vec(non_comment_line(), 1..12),
            trailing in vec(indent(), 0..8),
        ) {
            let expected = content
                .last()
                .or_abort("range ensures non-empty")
                .trim()
                .to_owned();
            let mut lines = content
                .into_iter()
                .map(|line| format!("  {line}  "))
                .collect::<Vec<_>>();
            lines.extend(trailing);
            let text = lines.join("\n");

            prop_assert_eq!(last_non_empty_line(text.as_str()), Some(expected));
        }

    }

    #[test]
    fn proptest_push_json_helpers_emit_expected_fragments() {
        let mut buf = String::new();

        push_json_str(&mut buf, "k\"ey", "va\nlue");
        assert_eq!(buf, "\"k\\\"ey\":\"va\\nlue\"");

        buf.clear();
        push_json_opt_str(&mut buf, "opt", None);
        assert_eq!(buf, "\"opt\":null");

        buf.clear();
        push_json_opt_str(&mut buf, "opt", Some("x"));
        assert_eq!(buf, "\"opt\":\"x\"");

        buf.clear();
        push_json_u64(&mut buf, "u", 42);
        assert_eq!(buf, "\"u\":42");

        buf.clear();
        let neg_seven: i32 = -7;
        let _wrote_i32 = write!(buf, "\"{}\":{}", json_escape("i"), neg_seven).is_ok();
        assert_eq!(buf, "\"i\":-7");

        buf.clear();
        push_json_bool(&mut buf, "b", true);
        assert_eq!(buf, "\"b\":true");

        buf.clear();
        push_json_bool(&mut buf, "b", false);
        assert_eq!(buf, "\"b\":false");

        buf.clear();
        let values = vec!["a".to_owned(), "b\"c".to_owned()];
        push_json_array(&mut buf, "arr", &values);
        assert_eq!(buf, "\"arr\":[\"a\",\"b\\\"c\"]");
    }

    #[test]
    fn proptest_now_unix_ms_is_monotonic_within_process() {
        let first = now_unix_ms();
        let second = now_unix_ms();
        assert!(second >= first);
    }

    #[test]
    fn proptest_trace_and_escape_cover_edge_branches() {
        let long_input = "a".repeat(TRACE_MAX_TEXT_BYTES + 1);
        let limited = trace_text_limit(long_input.as_str());
        assert_eq!(limited.len(), TRACE_MAX_TEXT_BYTES);

        let escaped = json_escape("\\\"\n\r\t\u{0001}");
        assert!(escaped.contains("\\\\"));
        assert!(escaped.contains("\\\""));
        assert!(escaped.contains("\\n"));
        assert!(escaped.contains("\\r"));
        assert!(escaped.contains("\\t"));
        assert!(escaped.contains("\\u0001"));
    }
}
