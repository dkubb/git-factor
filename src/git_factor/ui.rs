#[cfg_attr(
    not(test),
    expect(
        clippy::wildcard_imports,
        reason = "module intentionally shares parent items via a grouped import"
    )
)]
use super::*;

/// Returns true if a factor session is currently active.
pub(in crate::git_factor) fn is_factor_active_in(ctx: &Ctx<'_>) -> bool {
    factor_dir_in(ctx).is_ok_and(|dir| ctx.fs.is_dir(dir.as_path()))
}

/// Writes each line to stdout with a trailing newline.
fn out_lines(ctx: &Ctx<'_>, lines: &[&str]) -> Result<(), FactorError> {
    for line in lines {
        ctx.outln(line)?;
    }
    Ok(())
}

/// Returns true if a rebase is currently in progress.
pub(in crate::git_factor) fn is_mid_rebase_in(ctx: &Ctx<'_>) -> bool {
    git_dir_in(ctx).is_ok_and(|dir| {
        ctx.fs.is_dir(&dir.join(REBASE_MERGE_DIR)) || ctx.fs.is_dir(&dir.join(REBASE_APPLY_DIR))
    })
}

/// Prints contextual hints using a precomputed remaining-stat line.
pub(in crate::git_factor) fn print_hints_with_remaining_in(
    ctx: &Ctx<'_>,
    remaining: &str,
) -> Result<(), FactorError> {
    let toplevel = git_output(ctx, &["rev-parse", "--show-toplevel"])?;
    let rust_ref = Path::new(&toplevel).join("references/rust.md");

    out_lines(
        ctx,
        &[
            "HINTS:",
            "  - Find the ONE smallest addition nothing depends on",
            "  - Target 15-30 lines (50 max)",
            "  - Message: single concrete action, no \"and\"/\"or\"",
            "  - Verify: git log --oneline | wc -l",
            "  - NEVER use git commit. ONLY use git factor --continue.",
        ],
    )?;
    if !remaining.is_empty() {
        ctx.outln(&format!("  REMAINING: {remaining}"))?;
    }
    if ctx.fs.exists(&rust_ref) {
        ctx.outln(&format!("  REFERENCE: {}", rust_ref.display()))?;
    }
    ctx.outln("  RECOVERY: git factor --abort")?;

    if ctx.env.var_os("CLAUDECODE").is_some() {
        out_lines(
            ctx,
            &[
                "<claude>",
                "- If context is above 50%, pause and ask the user to /compact.",
                "- Do NOT stop early. Keep committing until \"Complete\".",
                "- Do NOT use git commit directly. ONLY use git-factor --continue.",
                "- Each commit MUST pass the exec gate. No shortcuts.",
                "</claude>",
            ],
        )?;
    }

    Ok(())
}

/// Prints the session-started guide with pending file summary.
pub(in crate::git_factor) fn print_session_started(
    ctx: &Ctx<'_>,
    started: &str,
    message: &str,
) -> Result<(), FactorError> {
    let stat_output = git_output(ctx, &["diff", "--stat"])?;
    let remaining = stat_output.lines().last().unwrap_or_default().to_owned();
    let untracked_output = git_output(ctx, &["ls-files", "--others", "--exclude-standard"])?;
    let original_message = format!("ORIGINAL MESSAGE: {message}");
    out_lines(ctx, &[started, original_message.as_str(), "UNSTAGED:"])?;
    for line in stat_output.lines() {
        ctx.outln(&format!("  {line}"))?;
    }
    if !untracked_output.is_empty() {
        ctx.outln("UNTRACKED:")?;
        for line in untracked_output.lines() {
            ctx.outln(&format!("  {line}"))?;
        }
    }
    let is_advance = started.starts_with("FACTOR: Now splitting ");
    if is_advance {
        out_lines(
            ctx,
            &[
                "",
                "NEXT: Stage changes for the next commit, then run:",
                "  git factor --continue --message \"type: description\"",
                "",
            ],
        )?;
    } else {
        out_lines(
            ctx,
            &[
                "",
                "NEXT: Stage changes for the first atomic commit, then run:",
                "  git factor --continue --message \"type: description\"",
                "",
                "Run git factor -h for command help or git-factor --help for the full workflow guide.",
                "",
            ],
        )?;
    }
    print_hints_with_remaining_in(ctx, remaining.as_str())?;

    Ok(())
}

#[cfg(test)]
mod tests {
    mod print_hints_with_remaining_in {
        use super::super::hint_contracts::{
            complete, rejected, spawn_failed, with_guidance, with_guidance_value, with_reference,
            with_remaining, without_guidance, without_reference, without_remaining, write_failed,
        };
        use super::super::*;
        use crate::non_empty_string::NonEmptyString;
        use crate::test_support::OrAbort as _;
        use core::num::{NonZeroU8, NonZeroUsize};
        #[test]
        fn empty_remaining_without_optional_guidance() {
            let world = complete(
                without_remaining(),
                &without_reference(),
                &without_guidance(),
            );
            let repo = world.directory.path();
            let ctx = Ctx {
                cwd: repo.to_path_buf(),
                env: &world.env,
                runner: &world.runner,
                io: &world.io,
                fs: &REAL_FS,
            };

            let result = super::super::print_hints_with_remaining_in(&ctx, &world.remaining);

            let (stdout, stderr, calls) = (
                world.io.stdout.borrow(),
                world.io.stderr.borrow(),
                world.runner.calls.borrow(),
            );

            let reference_read = fs::read(repo.join("references/rust.md"));

            assert_eq!(
                result.as_ref().map_err(ToString::to_string),
                world.expected_result.as_ref().map_err(Clone::clone)
            );
            assert_eq!(
                (
                    stdout.as_str(),
                    stderr.as_str(),
                    calls.as_slice(),
                    world.io.attempted.get(),
                    fs::read(repo.join(".git/index")).or_abort("saved index remains"),
                    fs::read(repo.join("user.txt")).or_abort("saved user remains"),
                    reference_read.as_ref().map_err(io::Error::kind)
                ),
                (
                    world.expected_stdout.as_str(),
                    "",
                    world.expected_calls.as_slice(),
                    world.expected_writes,
                    b"saved index".to_vec(),
                    b"saved user bytes".to_vec(),
                    world.expected_reference.as_ref().map_err(Clone::clone)
                )
            );
        }

        #[test]
        fn remaining_with_reference_and_claude_guidance() {
            let world = complete(
                with_remaining(
                    &NonEmptyString::try_from("2 files changed")
                        .or_abort("nonempty fixed remaining"),
                ),
                &with_reference(),
                &with_guidance(),
            );
            let repo = world.directory.path();
            let ctx = Ctx {
                cwd: repo.to_path_buf(),
                env: &world.env,
                runner: &world.runner,
                io: &world.io,
                fs: &REAL_FS,
            };

            let result = super::super::print_hints_with_remaining_in(&ctx, &world.remaining);

            let (stdout, stderr, calls) = (
                world.io.stdout.borrow(),
                world.io.stderr.borrow(),
                world.runner.calls.borrow(),
            );

            let reference_read = fs::read(repo.join("references/rust.md"));

            assert_eq!(
                result.as_ref().map_err(ToString::to_string),
                world.expected_result.as_ref().map_err(Clone::clone)
            );
            assert_eq!(
                (
                    stdout.as_str(),
                    stderr.as_str(),
                    calls.as_slice(),
                    world.io.attempted.get(),
                    fs::read(repo.join(".git/index")).or_abort("saved index remains"),
                    fs::read(repo.join("user.txt")).or_abort("saved user remains"),
                    reference_read.as_ref().map_err(io::Error::kind)
                ),
                (
                    world.expected_stdout.as_str(),
                    "",
                    world.expected_calls.as_slice(),
                    world.expected_writes,
                    b"saved index".to_vec(),
                    b"saved user bytes".to_vec(),
                    world.expected_reference.as_ref().map_err(Clone::clone)
                )
            );
        }

        #[test]
        fn single_space_remaining_is_printed() {
            let world = complete(
                with_remaining(&NonEmptyString::try_from(" ").or_abort("nonempty fixed remaining")),
                &with_reference(),
                &with_guidance(),
            );
            let repo = world.directory.path();
            let ctx = Ctx {
                cwd: repo.to_path_buf(),
                env: &world.env,
                runner: &world.runner,
                io: &world.io,
                fs: &REAL_FS,
            };

            let result = super::super::print_hints_with_remaining_in(&ctx, &world.remaining);

            let (stdout, stderr, calls) = (
                world.io.stdout.borrow(),
                world.io.stderr.borrow(),
                world.runner.calls.borrow(),
            );

            let reference_read = fs::read(repo.join("references/rust.md"));

            assert_eq!(
                result.as_ref().map_err(ToString::to_string),
                world.expected_result.as_ref().map_err(Clone::clone)
            );
            assert_eq!(
                (
                    stdout.as_str(),
                    stderr.as_str(),
                    calls.as_slice(),
                    world.io.attempted.get(),
                    fs::read(repo.join(".git/index")).or_abort("saved index remains"),
                    fs::read(repo.join("user.txt")).or_abort("saved user remains"),
                    reference_read.as_ref().map_err(io::Error::kind)
                ),
                (
                    world.expected_stdout.as_str(),
                    "",
                    world.expected_calls.as_slice(),
                    world.expected_writes,
                    b"saved index".to_vec(),
                    b"saved user bytes".to_vec(),
                    world.expected_reference.as_ref().map_err(Clone::clone)
                )
            );
        }

        #[test]
        fn empty_claudecode_value_keeps_guidance() {
            let world = complete(
                with_remaining(
                    &NonEmptyString::try_from("2 files changed")
                        .or_abort("nonempty fixed remaining"),
                ),
                &with_reference(),
                &with_guidance_value(OsString::new()),
            );
            let repo = world.directory.path();
            let ctx = Ctx {
                cwd: repo.to_path_buf(),
                env: &world.env,
                runner: &world.runner,
                io: &world.io,
                fs: &REAL_FS,
            };

            let result = super::super::print_hints_with_remaining_in(&ctx, &world.remaining);

            let (stdout, stderr, calls) = (
                world.io.stdout.borrow(),
                world.io.stderr.borrow(),
                world.runner.calls.borrow(),
            );

            let reference_read = fs::read(repo.join("references/rust.md"));

            assert_eq!(
                result.as_ref().map_err(ToString::to_string),
                world.expected_result.as_ref().map_err(Clone::clone)
            );
            assert_eq!(
                (
                    stdout.as_str(),
                    stderr.as_str(),
                    calls.as_slice(),
                    world.io.attempted.get(),
                    fs::read(repo.join(".git/index")).or_abort("saved index remains"),
                    fs::read(repo.join("user.txt")).or_abort("saved user remains"),
                    reference_read.as_ref().map_err(io::Error::kind)
                ),
                (
                    world.expected_stdout.as_str(),
                    "",
                    world.expected_calls.as_slice(),
                    world.expected_writes,
                    b"saved index".to_vec(),
                    b"saved user bytes".to_vec(),
                    world.expected_reference.as_ref().map_err(Clone::clone)
                )
            );
        }

        #[test]
        fn nested_cwd_discovers_repository_root_reference() {
            let mut world = complete(
                with_remaining(
                    &NonEmptyString::try_from("2 files changed")
                        .or_abort("nonempty fixed remaining"),
                ),
                &with_reference(),
                &with_guidance(),
            );
            let repo = world.directory.path();
            let nested = repo.join("nested");
            fs::create_dir_all(&nested).or_abort("owned nested current directory");
            fs::write(nested.join("user.txt"), b"saved nested user bytes")
                .or_abort("save nested user");
            world.env.repo = nested.clone();
            world.expected_calls = vec![super::super::hint_contracts::Call::Output {
                args: vec!["rev-parse".to_owned(), "--show-toplevel".to_owned()],
                bin: "git".to_owned(),
                cwd: nested.clone(),
            }];
            let ctx = Ctx {
                cwd: nested.clone(),
                env: &world.env,
                runner: &world.runner,
                io: &world.io,
                fs: &REAL_FS,
            };

            let result = super::super::print_hints_with_remaining_in(&ctx, &world.remaining);

            let (stdout, stderr, calls) = (
                world.io.stdout.borrow(),
                world.io.stderr.borrow(),
                world.runner.calls.borrow(),
            );

            let reference_read = fs::read(repo.join("references/rust.md"));

            assert_eq!(
                result.as_ref().map_err(ToString::to_string),
                world.expected_result.as_ref().map_err(Clone::clone)
            );
            assert_eq!(
                (
                    stdout.as_str(),
                    stderr.as_str(),
                    calls.as_slice(),
                    world.io.attempted.get(),
                    fs::read(repo.join(".git/index")).or_abort("saved index remains"),
                    fs::read(repo.join("user.txt")).or_abort("saved user remains"),
                    reference_read.as_ref().map_err(io::Error::kind),
                    fs::read(nested.join("user.txt")).or_abort("nested user remains"),
                    fs::read(nested.join("references/rust.md"))
                        .as_ref()
                        .map_err(io::Error::kind)
                ),
                (
                    world.expected_stdout.as_str(),
                    "",
                    world.expected_calls.as_slice(),
                    world.expected_writes,
                    b"saved index".to_vec(),
                    b"saved user bytes".to_vec(),
                    world.expected_reference.as_ref().map_err(Clone::clone),
                    b"saved nested user bytes".to_vec(),
                    Err(io::ErrorKind::NotFound)
                )
            );
        }

        #[test]
        fn nested_reference_is_omitted_when_repository_root_reference_is_absent() {
            let mut world = complete(
                with_remaining(
                    &NonEmptyString::try_from("2 files changed")
                        .or_abort("nonempty fixed remaining"),
                ),
                &without_reference(),
                &with_guidance(),
            );
            let repo = world.directory.path();
            let nested = repo.join("nested");
            fs::create_dir_all(&nested).or_abort("owned nested current directory");
            fs::write(nested.join("user.txt"), b"saved nested user bytes")
                .or_abort("save nested user");
            fs::create_dir_all(nested.join("references"))
                .or_abort("owned nested reference directory");
            fs::write(nested.join("references/rust.md"), b"nested decoy")
                .or_abort("save nested decoy reference");
            world.env.repo = nested.clone();
            world.expected_calls = vec![super::super::hint_contracts::Call::Output {
                args: vec!["rev-parse".to_owned(), "--show-toplevel".to_owned()],
                bin: "git".to_owned(),
                cwd: nested.clone(),
            }];
            let ctx = Ctx {
                cwd: nested.clone(),
                env: &world.env,
                runner: &world.runner,
                io: &world.io,
                fs: &REAL_FS,
            };

            let result = super::super::print_hints_with_remaining_in(&ctx, &world.remaining);

            let (stdout, stderr, calls) = (
                world.io.stdout.borrow(),
                world.io.stderr.borrow(),
                world.runner.calls.borrow(),
            );

            let reference_read = fs::read(repo.join("references/rust.md"));

            assert_eq!(
                result.as_ref().map_err(ToString::to_string),
                world.expected_result.as_ref().map_err(Clone::clone)
            );
            assert_eq!(
                (
                    stdout.as_str(),
                    stderr.as_str(),
                    calls.as_slice(),
                    world.io.attempted.get(),
                    fs::read(repo.join(".git/index")).or_abort("saved index remains"),
                    fs::read(repo.join("user.txt")).or_abort("saved user remains"),
                    reference_read.as_ref().map_err(io::Error::kind),
                    fs::read(nested.join("user.txt")).or_abort("nested user remains"),
                    fs::read(nested.join("references/rust.md"))
                        .as_ref()
                        .map_err(io::Error::kind)
                ),
                (
                    world.expected_stdout.as_str(),
                    "",
                    world.expected_calls.as_slice(),
                    world.expected_writes,
                    b"saved index".to_vec(),
                    b"saved user bytes".to_vec(),
                    world.expected_reference.as_ref().map_err(Clone::clone),
                    b"saved nested user bytes".to_vec(),
                    Ok(&b"nested decoy".to_vec())
                )
            );
        }

        #[test]
        fn query_rejection_precedes_all_output() {
            let world = rejected(
                "pending".to_owned(),
                &with_reference(),
                &with_guidance(),
                NonZeroU8::new(23).or_abort("nonzero rejection"),
            );
            let repo = world.directory.path();
            let ctx = Ctx {
                cwd: repo.to_path_buf(),
                env: &world.env,
                runner: &world.runner,
                io: &world.io,
                fs: &REAL_FS,
            };

            let result = super::super::print_hints_with_remaining_in(&ctx, &world.remaining);

            let (stdout, stderr, calls) = (
                world.io.stdout.borrow(),
                world.io.stderr.borrow(),
                world.runner.calls.borrow(),
            );

            let reference_read = fs::read(repo.join("references/rust.md"));

            assert_eq!(
                result.as_ref().map_err(ToString::to_string),
                world.expected_result.as_ref().map_err(Clone::clone)
            );
            assert_eq!(
                (
                    stdout.as_str(),
                    stderr.as_str(),
                    calls.as_slice(),
                    world.io.attempted.get(),
                    fs::read(repo.join(".git/index")).or_abort("saved index remains"),
                    fs::read(repo.join("user.txt")).or_abort("saved user remains"),
                    reference_read.as_ref().map_err(io::Error::kind)
                ),
                (
                    world.expected_stdout.as_str(),
                    "",
                    world.expected_calls.as_slice(),
                    world.expected_writes,
                    b"saved index".to_vec(),
                    b"saved user bytes".to_vec(),
                    world.expected_reference.as_ref().map_err(Clone::clone)
                )
            );
        }

        #[test]
        fn query_spawn_observes_trigger_then_snapshot_queries() {
            let world = spawn_failed("pending".to_owned(), &with_reference(), &with_guidance());
            let repo = world.directory.path();
            let ctx = Ctx {
                cwd: repo.to_path_buf(),
                env: &world.env,
                runner: &world.runner,
                io: &world.io,
                fs: &REAL_FS,
            };

            let result = super::super::print_hints_with_remaining_in(&ctx, &world.remaining);

            let (stdout, stderr, calls) = (
                world.io.stdout.borrow(),
                world.io.stderr.borrow(),
                world.runner.calls.borrow(),
            );

            let reference_read = fs::read(repo.join("references/rust.md"));

            assert_eq!(
                result.as_ref().map_err(ToString::to_string),
                world.expected_result.as_ref().map_err(Clone::clone)
            );
            assert_eq!(
                (
                    stdout.as_str(),
                    stderr.as_str(),
                    calls.as_slice(),
                    world.io.attempted.get(),
                    fs::read(repo.join(".git/index")).or_abort("saved index remains"),
                    fs::read(repo.join("user.txt")).or_abort("saved user remains"),
                    reference_read.as_ref().map_err(io::Error::kind)
                ),
                (
                    world.expected_stdout.as_str(),
                    "",
                    world.expected_calls.as_slice(),
                    world.expected_writes,
                    b"saved index".to_vec(),
                    b"saved user bytes".to_vec(),
                    world.expected_reference.as_ref().map_err(Clone::clone)
                )
            );
        }

        #[test]
        fn output_write_1_preserves_exact_prefix_and_saved_bytes() {
            let world = write_failed(
                with_remaining(
                    &NonEmptyString::try_from("2 files changed")
                        .or_abort("nonempty fixed remaining"),
                ),
                &with_reference(),
                &with_guidance(),
                NonZeroUsize::new(1).or_abort("positive write ordinal"),
            );
            let repo = world.directory.path();
            let ctx = Ctx {
                cwd: repo.to_path_buf(),
                env: &world.env,
                runner: &world.runner,
                io: &world.io,
                fs: &REAL_FS,
            };

            let result = super::super::print_hints_with_remaining_in(&ctx, &world.remaining);

            let (stdout, stderr, calls) = (
                world.io.stdout.borrow(),
                world.io.stderr.borrow(),
                world.runner.calls.borrow(),
            );

            let reference_read = fs::read(repo.join("references/rust.md"));

            assert_eq!(
                result.as_ref().map_err(ToString::to_string),
                world.expected_result.as_ref().map_err(Clone::clone)
            );
            assert_eq!(
                (
                    stdout.as_str(),
                    stderr.as_str(),
                    calls.as_slice(),
                    world.io.attempted.get(),
                    fs::read(repo.join(".git/index")).or_abort("saved index remains"),
                    fs::read(repo.join("user.txt")).or_abort("saved user remains"),
                    reference_read.as_ref().map_err(io::Error::kind)
                ),
                (
                    world.expected_stdout.as_str(),
                    "",
                    world.expected_calls.as_slice(),
                    world.expected_writes,
                    b"saved index".to_vec(),
                    b"saved user bytes".to_vec(),
                    world.expected_reference.as_ref().map_err(Clone::clone)
                )
            );
        }

        #[test]
        fn output_write_2_preserves_exact_prefix_and_saved_bytes() {
            let world = write_failed(
                with_remaining(
                    &NonEmptyString::try_from("2 files changed")
                        .or_abort("nonempty fixed remaining"),
                ),
                &with_reference(),
                &with_guidance(),
                NonZeroUsize::new(2).or_abort("positive write ordinal"),
            );
            let repo = world.directory.path();
            let ctx = Ctx {
                cwd: repo.to_path_buf(),
                env: &world.env,
                runner: &world.runner,
                io: &world.io,
                fs: &REAL_FS,
            };

            let result = super::super::print_hints_with_remaining_in(&ctx, &world.remaining);

            let (stdout, stderr, calls) = (
                world.io.stdout.borrow(),
                world.io.stderr.borrow(),
                world.runner.calls.borrow(),
            );

            let reference_read = fs::read(repo.join("references/rust.md"));

            assert_eq!(
                result.as_ref().map_err(ToString::to_string),
                world.expected_result.as_ref().map_err(Clone::clone)
            );
            assert_eq!(
                (
                    stdout.as_str(),
                    stderr.as_str(),
                    calls.as_slice(),
                    world.io.attempted.get(),
                    fs::read(repo.join(".git/index")).or_abort("saved index remains"),
                    fs::read(repo.join("user.txt")).or_abort("saved user remains"),
                    reference_read.as_ref().map_err(io::Error::kind)
                ),
                (
                    world.expected_stdout.as_str(),
                    "",
                    world.expected_calls.as_slice(),
                    world.expected_writes,
                    b"saved index".to_vec(),
                    b"saved user bytes".to_vec(),
                    world.expected_reference.as_ref().map_err(Clone::clone)
                )
            );
        }

        #[test]
        fn output_write_3_preserves_exact_prefix_and_saved_bytes() {
            let world = write_failed(
                with_remaining(
                    &NonEmptyString::try_from("2 files changed")
                        .or_abort("nonempty fixed remaining"),
                ),
                &with_reference(),
                &with_guidance(),
                NonZeroUsize::new(3).or_abort("positive write ordinal"),
            );
            let repo = world.directory.path();
            let ctx = Ctx {
                cwd: repo.to_path_buf(),
                env: &world.env,
                runner: &world.runner,
                io: &world.io,
                fs: &REAL_FS,
            };

            let result = super::super::print_hints_with_remaining_in(&ctx, &world.remaining);

            let (stdout, stderr, calls) = (
                world.io.stdout.borrow(),
                world.io.stderr.borrow(),
                world.runner.calls.borrow(),
            );

            let reference_read = fs::read(repo.join("references/rust.md"));

            assert_eq!(
                result.as_ref().map_err(ToString::to_string),
                world.expected_result.as_ref().map_err(Clone::clone)
            );
            assert_eq!(
                (
                    stdout.as_str(),
                    stderr.as_str(),
                    calls.as_slice(),
                    world.io.attempted.get(),
                    fs::read(repo.join(".git/index")).or_abort("saved index remains"),
                    fs::read(repo.join("user.txt")).or_abort("saved user remains"),
                    reference_read.as_ref().map_err(io::Error::kind)
                ),
                (
                    world.expected_stdout.as_str(),
                    "",
                    world.expected_calls.as_slice(),
                    world.expected_writes,
                    b"saved index".to_vec(),
                    b"saved user bytes".to_vec(),
                    world.expected_reference.as_ref().map_err(Clone::clone)
                )
            );
        }

        #[test]
        fn output_write_4_preserves_exact_prefix_and_saved_bytes() {
            let world = write_failed(
                with_remaining(
                    &NonEmptyString::try_from("2 files changed")
                        .or_abort("nonempty fixed remaining"),
                ),
                &with_reference(),
                &with_guidance(),
                NonZeroUsize::new(4).or_abort("positive write ordinal"),
            );
            let repo = world.directory.path();
            let ctx = Ctx {
                cwd: repo.to_path_buf(),
                env: &world.env,
                runner: &world.runner,
                io: &world.io,
                fs: &REAL_FS,
            };

            let result = super::super::print_hints_with_remaining_in(&ctx, &world.remaining);

            let (stdout, stderr, calls) = (
                world.io.stdout.borrow(),
                world.io.stderr.borrow(),
                world.runner.calls.borrow(),
            );

            let reference_read = fs::read(repo.join("references/rust.md"));

            assert_eq!(
                result.as_ref().map_err(ToString::to_string),
                world.expected_result.as_ref().map_err(Clone::clone)
            );
            assert_eq!(
                (
                    stdout.as_str(),
                    stderr.as_str(),
                    calls.as_slice(),
                    world.io.attempted.get(),
                    fs::read(repo.join(".git/index")).or_abort("saved index remains"),
                    fs::read(repo.join("user.txt")).or_abort("saved user remains"),
                    reference_read.as_ref().map_err(io::Error::kind)
                ),
                (
                    world.expected_stdout.as_str(),
                    "",
                    world.expected_calls.as_slice(),
                    world.expected_writes,
                    b"saved index".to_vec(),
                    b"saved user bytes".to_vec(),
                    world.expected_reference.as_ref().map_err(Clone::clone)
                )
            );
        }

        #[test]
        fn output_write_5_preserves_exact_prefix_and_saved_bytes() {
            let world = write_failed(
                with_remaining(
                    &NonEmptyString::try_from("2 files changed")
                        .or_abort("nonempty fixed remaining"),
                ),
                &with_reference(),
                &with_guidance(),
                NonZeroUsize::new(5).or_abort("positive write ordinal"),
            );
            let repo = world.directory.path();
            let ctx = Ctx {
                cwd: repo.to_path_buf(),
                env: &world.env,
                runner: &world.runner,
                io: &world.io,
                fs: &REAL_FS,
            };

            let result = super::super::print_hints_with_remaining_in(&ctx, &world.remaining);

            let (stdout, stderr, calls) = (
                world.io.stdout.borrow(),
                world.io.stderr.borrow(),
                world.runner.calls.borrow(),
            );

            let reference_read = fs::read(repo.join("references/rust.md"));

            assert_eq!(
                result.as_ref().map_err(ToString::to_string),
                world.expected_result.as_ref().map_err(Clone::clone)
            );
            assert_eq!(
                (
                    stdout.as_str(),
                    stderr.as_str(),
                    calls.as_slice(),
                    world.io.attempted.get(),
                    fs::read(repo.join(".git/index")).or_abort("saved index remains"),
                    fs::read(repo.join("user.txt")).or_abort("saved user remains"),
                    reference_read.as_ref().map_err(io::Error::kind)
                ),
                (
                    world.expected_stdout.as_str(),
                    "",
                    world.expected_calls.as_slice(),
                    world.expected_writes,
                    b"saved index".to_vec(),
                    b"saved user bytes".to_vec(),
                    world.expected_reference.as_ref().map_err(Clone::clone)
                )
            );
        }

        #[test]
        fn output_write_6_preserves_exact_prefix_and_saved_bytes() {
            let world = write_failed(
                with_remaining(
                    &NonEmptyString::try_from("2 files changed")
                        .or_abort("nonempty fixed remaining"),
                ),
                &with_reference(),
                &with_guidance(),
                NonZeroUsize::new(6).or_abort("positive write ordinal"),
            );
            let repo = world.directory.path();
            let ctx = Ctx {
                cwd: repo.to_path_buf(),
                env: &world.env,
                runner: &world.runner,
                io: &world.io,
                fs: &REAL_FS,
            };

            let result = super::super::print_hints_with_remaining_in(&ctx, &world.remaining);

            let (stdout, stderr, calls) = (
                world.io.stdout.borrow(),
                world.io.stderr.borrow(),
                world.runner.calls.borrow(),
            );

            let reference_read = fs::read(repo.join("references/rust.md"));

            assert_eq!(
                result.as_ref().map_err(ToString::to_string),
                world.expected_result.as_ref().map_err(Clone::clone)
            );
            assert_eq!(
                (
                    stdout.as_str(),
                    stderr.as_str(),
                    calls.as_slice(),
                    world.io.attempted.get(),
                    fs::read(repo.join(".git/index")).or_abort("saved index remains"),
                    fs::read(repo.join("user.txt")).or_abort("saved user remains"),
                    reference_read.as_ref().map_err(io::Error::kind)
                ),
                (
                    world.expected_stdout.as_str(),
                    "",
                    world.expected_calls.as_slice(),
                    world.expected_writes,
                    b"saved index".to_vec(),
                    b"saved user bytes".to_vec(),
                    world.expected_reference.as_ref().map_err(Clone::clone)
                )
            );
        }

        #[test]
        fn output_write_7_preserves_exact_prefix_and_saved_bytes() {
            let world = write_failed(
                with_remaining(
                    &NonEmptyString::try_from("2 files changed")
                        .or_abort("nonempty fixed remaining"),
                ),
                &with_reference(),
                &with_guidance(),
                NonZeroUsize::new(7).or_abort("positive write ordinal"),
            );
            let repo = world.directory.path();
            let ctx = Ctx {
                cwd: repo.to_path_buf(),
                env: &world.env,
                runner: &world.runner,
                io: &world.io,
                fs: &REAL_FS,
            };

            let result = super::super::print_hints_with_remaining_in(&ctx, &world.remaining);

            let (stdout, stderr, calls) = (
                world.io.stdout.borrow(),
                world.io.stderr.borrow(),
                world.runner.calls.borrow(),
            );

            let reference_read = fs::read(repo.join("references/rust.md"));

            assert_eq!(
                result.as_ref().map_err(ToString::to_string),
                world.expected_result.as_ref().map_err(Clone::clone)
            );
            assert_eq!(
                (
                    stdout.as_str(),
                    stderr.as_str(),
                    calls.as_slice(),
                    world.io.attempted.get(),
                    fs::read(repo.join(".git/index")).or_abort("saved index remains"),
                    fs::read(repo.join("user.txt")).or_abort("saved user remains"),
                    reference_read.as_ref().map_err(io::Error::kind)
                ),
                (
                    world.expected_stdout.as_str(),
                    "",
                    world.expected_calls.as_slice(),
                    world.expected_writes,
                    b"saved index".to_vec(),
                    b"saved user bytes".to_vec(),
                    world.expected_reference.as_ref().map_err(Clone::clone)
                )
            );
        }

        #[test]
        fn output_write_8_preserves_exact_prefix_and_saved_bytes() {
            let world = write_failed(
                with_remaining(
                    &NonEmptyString::try_from("2 files changed")
                        .or_abort("nonempty fixed remaining"),
                ),
                &with_reference(),
                &with_guidance(),
                NonZeroUsize::new(8).or_abort("positive write ordinal"),
            );
            let repo = world.directory.path();
            let ctx = Ctx {
                cwd: repo.to_path_buf(),
                env: &world.env,
                runner: &world.runner,
                io: &world.io,
                fs: &REAL_FS,
            };

            let result = super::super::print_hints_with_remaining_in(&ctx, &world.remaining);

            let (stdout, stderr, calls) = (
                world.io.stdout.borrow(),
                world.io.stderr.borrow(),
                world.runner.calls.borrow(),
            );

            let reference_read = fs::read(repo.join("references/rust.md"));

            assert_eq!(
                result.as_ref().map_err(ToString::to_string),
                world.expected_result.as_ref().map_err(Clone::clone)
            );
            assert_eq!(
                (
                    stdout.as_str(),
                    stderr.as_str(),
                    calls.as_slice(),
                    world.io.attempted.get(),
                    fs::read(repo.join(".git/index")).or_abort("saved index remains"),
                    fs::read(repo.join("user.txt")).or_abort("saved user remains"),
                    reference_read.as_ref().map_err(io::Error::kind)
                ),
                (
                    world.expected_stdout.as_str(),
                    "",
                    world.expected_calls.as_slice(),
                    world.expected_writes,
                    b"saved index".to_vec(),
                    b"saved user bytes".to_vec(),
                    world.expected_reference.as_ref().map_err(Clone::clone)
                )
            );
        }

        #[test]
        fn output_write_9_preserves_exact_prefix_and_saved_bytes() {
            let world = write_failed(
                with_remaining(
                    &NonEmptyString::try_from("2 files changed")
                        .or_abort("nonempty fixed remaining"),
                ),
                &with_reference(),
                &with_guidance(),
                NonZeroUsize::new(9).or_abort("positive write ordinal"),
            );
            let repo = world.directory.path();
            let ctx = Ctx {
                cwd: repo.to_path_buf(),
                env: &world.env,
                runner: &world.runner,
                io: &world.io,
                fs: &REAL_FS,
            };

            let result = super::super::print_hints_with_remaining_in(&ctx, &world.remaining);

            let (stdout, stderr, calls) = (
                world.io.stdout.borrow(),
                world.io.stderr.borrow(),
                world.runner.calls.borrow(),
            );

            let reference_read = fs::read(repo.join("references/rust.md"));

            assert_eq!(
                result.as_ref().map_err(ToString::to_string),
                world.expected_result.as_ref().map_err(Clone::clone)
            );
            assert_eq!(
                (
                    stdout.as_str(),
                    stderr.as_str(),
                    calls.as_slice(),
                    world.io.attempted.get(),
                    fs::read(repo.join(".git/index")).or_abort("saved index remains"),
                    fs::read(repo.join("user.txt")).or_abort("saved user remains"),
                    reference_read.as_ref().map_err(io::Error::kind)
                ),
                (
                    world.expected_stdout.as_str(),
                    "",
                    world.expected_calls.as_slice(),
                    world.expected_writes,
                    b"saved index".to_vec(),
                    b"saved user bytes".to_vec(),
                    world.expected_reference.as_ref().map_err(Clone::clone)
                )
            );
        }

        #[test]
        fn output_write_10_preserves_exact_prefix_and_saved_bytes() {
            let world = write_failed(
                with_remaining(
                    &NonEmptyString::try_from("2 files changed")
                        .or_abort("nonempty fixed remaining"),
                ),
                &with_reference(),
                &with_guidance(),
                NonZeroUsize::new(10).or_abort("positive write ordinal"),
            );
            let repo = world.directory.path();
            let ctx = Ctx {
                cwd: repo.to_path_buf(),
                env: &world.env,
                runner: &world.runner,
                io: &world.io,
                fs: &REAL_FS,
            };

            let result = super::super::print_hints_with_remaining_in(&ctx, &world.remaining);

            let (stdout, stderr, calls) = (
                world.io.stdout.borrow(),
                world.io.stderr.borrow(),
                world.runner.calls.borrow(),
            );

            let reference_read = fs::read(repo.join("references/rust.md"));

            assert_eq!(
                result.as_ref().map_err(ToString::to_string),
                world.expected_result.as_ref().map_err(Clone::clone)
            );
            assert_eq!(
                (
                    stdout.as_str(),
                    stderr.as_str(),
                    calls.as_slice(),
                    world.io.attempted.get(),
                    fs::read(repo.join(".git/index")).or_abort("saved index remains"),
                    fs::read(repo.join("user.txt")).or_abort("saved user remains"),
                    reference_read.as_ref().map_err(io::Error::kind)
                ),
                (
                    world.expected_stdout.as_str(),
                    "",
                    world.expected_calls.as_slice(),
                    world.expected_writes,
                    b"saved index".to_vec(),
                    b"saved user bytes".to_vec(),
                    world.expected_reference.as_ref().map_err(Clone::clone)
                )
            );
        }

        #[test]
        fn output_write_11_preserves_exact_prefix_and_saved_bytes() {
            let world = write_failed(
                with_remaining(
                    &NonEmptyString::try_from("2 files changed")
                        .or_abort("nonempty fixed remaining"),
                ),
                &with_reference(),
                &with_guidance(),
                NonZeroUsize::new(11).or_abort("positive write ordinal"),
            );
            let repo = world.directory.path();
            let ctx = Ctx {
                cwd: repo.to_path_buf(),
                env: &world.env,
                runner: &world.runner,
                io: &world.io,
                fs: &REAL_FS,
            };

            let result = super::super::print_hints_with_remaining_in(&ctx, &world.remaining);

            let (stdout, stderr, calls) = (
                world.io.stdout.borrow(),
                world.io.stderr.borrow(),
                world.runner.calls.borrow(),
            );

            let reference_read = fs::read(repo.join("references/rust.md"));

            assert_eq!(
                result.as_ref().map_err(ToString::to_string),
                world.expected_result.as_ref().map_err(Clone::clone)
            );
            assert_eq!(
                (
                    stdout.as_str(),
                    stderr.as_str(),
                    calls.as_slice(),
                    world.io.attempted.get(),
                    fs::read(repo.join(".git/index")).or_abort("saved index remains"),
                    fs::read(repo.join("user.txt")).or_abort("saved user remains"),
                    reference_read.as_ref().map_err(io::Error::kind)
                ),
                (
                    world.expected_stdout.as_str(),
                    "",
                    world.expected_calls.as_slice(),
                    world.expected_writes,
                    b"saved index".to_vec(),
                    b"saved user bytes".to_vec(),
                    world.expected_reference.as_ref().map_err(Clone::clone)
                )
            );
        }

        #[test]
        fn output_write_12_preserves_exact_prefix_and_saved_bytes() {
            let world = write_failed(
                with_remaining(
                    &NonEmptyString::try_from("2 files changed")
                        .or_abort("nonempty fixed remaining"),
                ),
                &with_reference(),
                &with_guidance(),
                NonZeroUsize::new(12).or_abort("positive write ordinal"),
            );
            let repo = world.directory.path();
            let ctx = Ctx {
                cwd: repo.to_path_buf(),
                env: &world.env,
                runner: &world.runner,
                io: &world.io,
                fs: &REAL_FS,
            };

            let result = super::super::print_hints_with_remaining_in(&ctx, &world.remaining);

            let (stdout, stderr, calls) = (
                world.io.stdout.borrow(),
                world.io.stderr.borrow(),
                world.runner.calls.borrow(),
            );

            let reference_read = fs::read(repo.join("references/rust.md"));

            assert_eq!(
                result.as_ref().map_err(ToString::to_string),
                world.expected_result.as_ref().map_err(Clone::clone)
            );
            assert_eq!(
                (
                    stdout.as_str(),
                    stderr.as_str(),
                    calls.as_slice(),
                    world.io.attempted.get(),
                    fs::read(repo.join(".git/index")).or_abort("saved index remains"),
                    fs::read(repo.join("user.txt")).or_abort("saved user remains"),
                    reference_read.as_ref().map_err(io::Error::kind)
                ),
                (
                    world.expected_stdout.as_str(),
                    "",
                    world.expected_calls.as_slice(),
                    world.expected_writes,
                    b"saved index".to_vec(),
                    b"saved user bytes".to_vec(),
                    world.expected_reference.as_ref().map_err(Clone::clone)
                )
            );
        }

        #[test]
        fn output_write_13_preserves_exact_prefix_and_saved_bytes() {
            let world = write_failed(
                with_remaining(
                    &NonEmptyString::try_from("2 files changed")
                        .or_abort("nonempty fixed remaining"),
                ),
                &with_reference(),
                &with_guidance(),
                NonZeroUsize::new(13).or_abort("positive write ordinal"),
            );
            let repo = world.directory.path();
            let ctx = Ctx {
                cwd: repo.to_path_buf(),
                env: &world.env,
                runner: &world.runner,
                io: &world.io,
                fs: &REAL_FS,
            };

            let result = super::super::print_hints_with_remaining_in(&ctx, &world.remaining);

            let (stdout, stderr, calls) = (
                world.io.stdout.borrow(),
                world.io.stderr.borrow(),
                world.runner.calls.borrow(),
            );

            let reference_read = fs::read(repo.join("references/rust.md"));

            assert_eq!(
                result.as_ref().map_err(ToString::to_string),
                world.expected_result.as_ref().map_err(Clone::clone)
            );
            assert_eq!(
                (
                    stdout.as_str(),
                    stderr.as_str(),
                    calls.as_slice(),
                    world.io.attempted.get(),
                    fs::read(repo.join(".git/index")).or_abort("saved index remains"),
                    fs::read(repo.join("user.txt")).or_abort("saved user remains"),
                    reference_read.as_ref().map_err(io::Error::kind)
                ),
                (
                    world.expected_stdout.as_str(),
                    "",
                    world.expected_calls.as_slice(),
                    world.expected_writes,
                    b"saved index".to_vec(),
                    b"saved user bytes".to_vec(),
                    world.expected_reference.as_ref().map_err(Clone::clone)
                )
            );
        }

        #[test]
        fn output_write_14_preserves_exact_prefix_and_saved_bytes() {
            let world = write_failed(
                with_remaining(
                    &NonEmptyString::try_from("2 files changed")
                        .or_abort("nonempty fixed remaining"),
                ),
                &with_reference(),
                &with_guidance(),
                NonZeroUsize::new(14).or_abort("positive write ordinal"),
            );
            let repo = world.directory.path();
            let ctx = Ctx {
                cwd: repo.to_path_buf(),
                env: &world.env,
                runner: &world.runner,
                io: &world.io,
                fs: &REAL_FS,
            };

            let result = super::super::print_hints_with_remaining_in(&ctx, &world.remaining);

            let (stdout, stderr, calls) = (
                world.io.stdout.borrow(),
                world.io.stderr.borrow(),
                world.runner.calls.borrow(),
            );

            let reference_read = fs::read(repo.join("references/rust.md"));

            assert_eq!(
                result.as_ref().map_err(ToString::to_string),
                world.expected_result.as_ref().map_err(Clone::clone)
            );
            assert_eq!(
                (
                    stdout.as_str(),
                    stderr.as_str(),
                    calls.as_slice(),
                    world.io.attempted.get(),
                    fs::read(repo.join(".git/index")).or_abort("saved index remains"),
                    fs::read(repo.join("user.txt")).or_abort("saved user remains"),
                    reference_read.as_ref().map_err(io::Error::kind)
                ),
                (
                    world.expected_stdout.as_str(),
                    "",
                    world.expected_calls.as_slice(),
                    world.expected_writes,
                    b"saved index".to_vec(),
                    b"saved user bytes".to_vec(),
                    world.expected_reference.as_ref().map_err(Clone::clone)
                )
            );
        }

        #[test]
        fn output_write_15_preserves_exact_prefix_and_saved_bytes() {
            let world = write_failed(
                with_remaining(
                    &NonEmptyString::try_from("2 files changed")
                        .or_abort("nonempty fixed remaining"),
                ),
                &with_reference(),
                &with_guidance(),
                NonZeroUsize::new(15).or_abort("positive write ordinal"),
            );
            let repo = world.directory.path();
            let ctx = Ctx {
                cwd: repo.to_path_buf(),
                env: &world.env,
                runner: &world.runner,
                io: &world.io,
                fs: &REAL_FS,
            };

            let result = super::super::print_hints_with_remaining_in(&ctx, &world.remaining);

            let (stdout, stderr, calls) = (
                world.io.stdout.borrow(),
                world.io.stderr.borrow(),
                world.runner.calls.borrow(),
            );

            let reference_read = fs::read(repo.join("references/rust.md"));

            assert_eq!(
                result.as_ref().map_err(ToString::to_string),
                world.expected_result.as_ref().map_err(Clone::clone)
            );
            assert_eq!(
                (
                    stdout.as_str(),
                    stderr.as_str(),
                    calls.as_slice(),
                    world.io.attempted.get(),
                    fs::read(repo.join(".git/index")).or_abort("saved index remains"),
                    fs::read(repo.join("user.txt")).or_abort("saved user remains"),
                    reference_read.as_ref().map_err(io::Error::kind)
                ),
                (
                    world.expected_stdout.as_str(),
                    "",
                    world.expected_calls.as_slice(),
                    world.expected_writes,
                    b"saved index".to_vec(),
                    b"saved user bytes".to_vec(),
                    world.expected_reference.as_ref().map_err(Clone::clone)
                )
            );
        }

        #[test]
        fn output_write_16_preserves_exact_prefix_and_saved_bytes() {
            let world = write_failed(
                with_remaining(
                    &NonEmptyString::try_from("2 files changed")
                        .or_abort("nonempty fixed remaining"),
                ),
                &with_reference(),
                &with_guidance(),
                NonZeroUsize::new(16).or_abort("positive write ordinal"),
            );
            let repo = world.directory.path();
            let ctx = Ctx {
                cwd: repo.to_path_buf(),
                env: &world.env,
                runner: &world.runner,
                io: &world.io,
                fs: &REAL_FS,
            };

            let result = super::super::print_hints_with_remaining_in(&ctx, &world.remaining);

            let (stdout, stderr, calls) = (
                world.io.stdout.borrow(),
                world.io.stderr.borrow(),
                world.runner.calls.borrow(),
            );

            let reference_read = fs::read(repo.join("references/rust.md"));

            assert_eq!(
                result.as_ref().map_err(ToString::to_string),
                world.expected_result.as_ref().map_err(Clone::clone)
            );
            assert_eq!(
                (
                    stdout.as_str(),
                    stderr.as_str(),
                    calls.as_slice(),
                    world.io.attempted.get(),
                    fs::read(repo.join(".git/index")).or_abort("saved index remains"),
                    fs::read(repo.join("user.txt")).or_abort("saved user remains"),
                    reference_read.as_ref().map_err(io::Error::kind)
                ),
                (
                    world.expected_stdout.as_str(),
                    "",
                    world.expected_calls.as_slice(),
                    world.expected_writes,
                    b"saved index".to_vec(),
                    b"saved user bytes".to_vec(),
                    world.expected_reference.as_ref().map_err(Clone::clone)
                )
            );
        }

        #[test]
        fn output_write_17_preserves_exact_prefix_and_saved_bytes() {
            let world = write_failed(
                with_remaining(
                    &NonEmptyString::try_from("2 files changed")
                        .or_abort("nonempty fixed remaining"),
                ),
                &with_reference(),
                &with_guidance(),
                NonZeroUsize::new(17).or_abort("positive write ordinal"),
            );
            let repo = world.directory.path();
            let ctx = Ctx {
                cwd: repo.to_path_buf(),
                env: &world.env,
                runner: &world.runner,
                io: &world.io,
                fs: &REAL_FS,
            };

            let result = super::super::print_hints_with_remaining_in(&ctx, &world.remaining);

            let (stdout, stderr, calls) = (
                world.io.stdout.borrow(),
                world.io.stderr.borrow(),
                world.runner.calls.borrow(),
            );

            let reference_read = fs::read(repo.join("references/rust.md"));

            assert_eq!(
                result.as_ref().map_err(ToString::to_string),
                world.expected_result.as_ref().map_err(Clone::clone)
            );
            assert_eq!(
                (
                    stdout.as_str(),
                    stderr.as_str(),
                    calls.as_slice(),
                    world.io.attempted.get(),
                    fs::read(repo.join(".git/index")).or_abort("saved index remains"),
                    fs::read(repo.join("user.txt")).or_abort("saved user remains"),
                    reference_read.as_ref().map_err(io::Error::kind)
                ),
                (
                    world.expected_stdout.as_str(),
                    "",
                    world.expected_calls.as_slice(),
                    world.expected_writes,
                    b"saved index".to_vec(),
                    b"saved user bytes".to_vec(),
                    world.expected_reference.as_ref().map_err(Clone::clone)
                )
            );
        }

        #[test]
        fn output_write_18_preserves_exact_prefix_and_saved_bytes() {
            let world = write_failed(
                with_remaining(
                    &NonEmptyString::try_from("2 files changed")
                        .or_abort("nonempty fixed remaining"),
                ),
                &with_reference(),
                &with_guidance(),
                NonZeroUsize::new(18).or_abort("positive write ordinal"),
            );
            let repo = world.directory.path();
            let ctx = Ctx {
                cwd: repo.to_path_buf(),
                env: &world.env,
                runner: &world.runner,
                io: &world.io,
                fs: &REAL_FS,
            };

            let result = super::super::print_hints_with_remaining_in(&ctx, &world.remaining);

            let (stdout, stderr, calls) = (
                world.io.stdout.borrow(),
                world.io.stderr.borrow(),
                world.runner.calls.borrow(),
            );

            let reference_read = fs::read(repo.join("references/rust.md"));

            assert_eq!(
                result.as_ref().map_err(ToString::to_string),
                world.expected_result.as_ref().map_err(Clone::clone)
            );
            assert_eq!(
                (
                    stdout.as_str(),
                    stderr.as_str(),
                    calls.as_slice(),
                    world.io.attempted.get(),
                    fs::read(repo.join(".git/index")).or_abort("saved index remains"),
                    fs::read(repo.join("user.txt")).or_abort("saved user remains"),
                    reference_read.as_ref().map_err(io::Error::kind)
                ),
                (
                    world.expected_stdout.as_str(),
                    "",
                    world.expected_calls.as_slice(),
                    world.expected_writes,
                    b"saved index".to_vec(),
                    b"saved user bytes".to_vec(),
                    world.expected_reference.as_ref().map_err(Clone::clone)
                )
            );
        }

        #[test]
        fn output_write_19_preserves_exact_prefix_and_saved_bytes() {
            let world = write_failed(
                with_remaining(
                    &NonEmptyString::try_from("2 files changed")
                        .or_abort("nonempty fixed remaining"),
                ),
                &with_reference(),
                &with_guidance(),
                NonZeroUsize::new(19).or_abort("positive write ordinal"),
            );
            let repo = world.directory.path();
            let ctx = Ctx {
                cwd: repo.to_path_buf(),
                env: &world.env,
                runner: &world.runner,
                io: &world.io,
                fs: &REAL_FS,
            };

            let result = super::super::print_hints_with_remaining_in(&ctx, &world.remaining);

            let (stdout, stderr, calls) = (
                world.io.stdout.borrow(),
                world.io.stderr.borrow(),
                world.runner.calls.borrow(),
            );

            let reference_read = fs::read(repo.join("references/rust.md"));

            assert_eq!(
                result.as_ref().map_err(ToString::to_string),
                world.expected_result.as_ref().map_err(Clone::clone)
            );
            assert_eq!(
                (
                    stdout.as_str(),
                    stderr.as_str(),
                    calls.as_slice(),
                    world.io.attempted.get(),
                    fs::read(repo.join(".git/index")).or_abort("saved index remains"),
                    fs::read(repo.join("user.txt")).or_abort("saved user remains"),
                    reference_read.as_ref().map_err(io::Error::kind)
                ),
                (
                    world.expected_stdout.as_str(),
                    "",
                    world.expected_calls.as_slice(),
                    world.expected_writes,
                    b"saved index".to_vec(),
                    b"saved user bytes".to_vec(),
                    world.expected_reference.as_ref().map_err(Clone::clone)
                )
            );
        }

        #[test]
        fn output_write_20_preserves_exact_prefix_and_saved_bytes() {
            let world = write_failed(
                with_remaining(
                    &NonEmptyString::try_from("2 files changed")
                        .or_abort("nonempty fixed remaining"),
                ),
                &with_reference(),
                &with_guidance(),
                NonZeroUsize::new(20).or_abort("positive write ordinal"),
            );
            let repo = world.directory.path();
            let ctx = Ctx {
                cwd: repo.to_path_buf(),
                env: &world.env,
                runner: &world.runner,
                io: &world.io,
                fs: &REAL_FS,
            };

            let result = super::super::print_hints_with_remaining_in(&ctx, &world.remaining);

            let (stdout, stderr, calls) = (
                world.io.stdout.borrow(),
                world.io.stderr.borrow(),
                world.runner.calls.borrow(),
            );

            let reference_read = fs::read(repo.join("references/rust.md"));

            assert_eq!(
                result.as_ref().map_err(ToString::to_string),
                world.expected_result.as_ref().map_err(Clone::clone)
            );
            assert_eq!(
                (
                    stdout.as_str(),
                    stderr.as_str(),
                    calls.as_slice(),
                    world.io.attempted.get(),
                    fs::read(repo.join(".git/index")).or_abort("saved index remains"),
                    fs::read(repo.join("user.txt")).or_abort("saved user remains"),
                    reference_read.as_ref().map_err(io::Error::kind)
                ),
                (
                    world.expected_stdout.as_str(),
                    "",
                    world.expected_calls.as_slice(),
                    world.expected_writes,
                    b"saved index".to_vec(),
                    b"saved user bytes".to_vec(),
                    world.expected_reference.as_ref().map_err(Clone::clone)
                )
            );
        }

        #[test]
        fn output_write_21_preserves_exact_prefix_and_saved_bytes() {
            let world = write_failed(
                with_remaining(
                    &NonEmptyString::try_from("2 files changed")
                        .or_abort("nonempty fixed remaining"),
                ),
                &with_reference(),
                &with_guidance(),
                NonZeroUsize::new(21).or_abort("positive write ordinal"),
            );
            let repo = world.directory.path();
            let ctx = Ctx {
                cwd: repo.to_path_buf(),
                env: &world.env,
                runner: &world.runner,
                io: &world.io,
                fs: &REAL_FS,
            };

            let result = super::super::print_hints_with_remaining_in(&ctx, &world.remaining);

            let (stdout, stderr, calls) = (
                world.io.stdout.borrow(),
                world.io.stderr.borrow(),
                world.runner.calls.borrow(),
            );

            let reference_read = fs::read(repo.join("references/rust.md"));

            assert_eq!(
                result.as_ref().map_err(ToString::to_string),
                world.expected_result.as_ref().map_err(Clone::clone)
            );
            assert_eq!(
                (
                    stdout.as_str(),
                    stderr.as_str(),
                    calls.as_slice(),
                    world.io.attempted.get(),
                    fs::read(repo.join(".git/index")).or_abort("saved index remains"),
                    fs::read(repo.join("user.txt")).or_abort("saved user remains"),
                    reference_read.as_ref().map_err(io::Error::kind)
                ),
                (
                    world.expected_stdout.as_str(),
                    "",
                    world.expected_calls.as_slice(),
                    world.expected_writes,
                    b"saved index".to_vec(),
                    b"saved user bytes".to_vec(),
                    world.expected_reference.as_ref().map_err(Clone::clone)
                )
            );
        }

        #[test]
        fn output_write_22_preserves_exact_prefix_and_saved_bytes() {
            let world = write_failed(
                with_remaining(
                    &NonEmptyString::try_from("2 files changed")
                        .or_abort("nonempty fixed remaining"),
                ),
                &with_reference(),
                &with_guidance(),
                NonZeroUsize::new(22).or_abort("positive write ordinal"),
            );
            let repo = world.directory.path();
            let ctx = Ctx {
                cwd: repo.to_path_buf(),
                env: &world.env,
                runner: &world.runner,
                io: &world.io,
                fs: &REAL_FS,
            };

            let result = super::super::print_hints_with_remaining_in(&ctx, &world.remaining);

            let (stdout, stderr, calls) = (
                world.io.stdout.borrow(),
                world.io.stderr.borrow(),
                world.runner.calls.borrow(),
            );

            let reference_read = fs::read(repo.join("references/rust.md"));

            assert_eq!(
                result.as_ref().map_err(ToString::to_string),
                world.expected_result.as_ref().map_err(Clone::clone)
            );
            assert_eq!(
                (
                    stdout.as_str(),
                    stderr.as_str(),
                    calls.as_slice(),
                    world.io.attempted.get(),
                    fs::read(repo.join(".git/index")).or_abort("saved index remains"),
                    fs::read(repo.join("user.txt")).or_abort("saved user remains"),
                    reference_read.as_ref().map_err(io::Error::kind)
                ),
                (
                    world.expected_stdout.as_str(),
                    "",
                    world.expected_calls.as_slice(),
                    world.expected_writes,
                    b"saved index".to_vec(),
                    b"saved user bytes".to_vec(),
                    world.expected_reference.as_ref().map_err(Clone::clone)
                )
            );
        }

        #[test]
        fn output_write_23_preserves_exact_prefix_and_saved_bytes() {
            let world = write_failed(
                with_remaining(
                    &NonEmptyString::try_from("2 files changed")
                        .or_abort("nonempty fixed remaining"),
                ),
                &with_reference(),
                &with_guidance(),
                NonZeroUsize::new(23).or_abort("positive write ordinal"),
            );
            let repo = world.directory.path();
            let ctx = Ctx {
                cwd: repo.to_path_buf(),
                env: &world.env,
                runner: &world.runner,
                io: &world.io,
                fs: &REAL_FS,
            };

            let result = super::super::print_hints_with_remaining_in(&ctx, &world.remaining);

            let (stdout, stderr, calls) = (
                world.io.stdout.borrow(),
                world.io.stderr.borrow(),
                world.runner.calls.borrow(),
            );

            let reference_read = fs::read(repo.join("references/rust.md"));

            assert_eq!(
                result.as_ref().map_err(ToString::to_string),
                world.expected_result.as_ref().map_err(Clone::clone)
            );
            assert_eq!(
                (
                    stdout.as_str(),
                    stderr.as_str(),
                    calls.as_slice(),
                    world.io.attempted.get(),
                    fs::read(repo.join(".git/index")).or_abort("saved index remains"),
                    fs::read(repo.join("user.txt")).or_abort("saved user remains"),
                    reference_read.as_ref().map_err(io::Error::kind)
                ),
                (
                    world.expected_stdout.as_str(),
                    "",
                    world.expected_calls.as_slice(),
                    world.expected_writes,
                    b"saved index".to_vec(),
                    b"saved user bytes".to_vec(),
                    world.expected_reference.as_ref().map_err(Clone::clone)
                )
            );
        }

        #[test]
        fn output_write_24_preserves_exact_prefix_and_saved_bytes() {
            let world = write_failed(
                with_remaining(
                    &NonEmptyString::try_from("2 files changed")
                        .or_abort("nonempty fixed remaining"),
                ),
                &with_reference(),
                &with_guidance(),
                NonZeroUsize::new(24).or_abort("positive write ordinal"),
            );
            let repo = world.directory.path();
            let ctx = Ctx {
                cwd: repo.to_path_buf(),
                env: &world.env,
                runner: &world.runner,
                io: &world.io,
                fs: &REAL_FS,
            };

            let result = super::super::print_hints_with_remaining_in(&ctx, &world.remaining);

            let (stdout, stderr, calls) = (
                world.io.stdout.borrow(),
                world.io.stderr.borrow(),
                world.runner.calls.borrow(),
            );

            let reference_read = fs::read(repo.join("references/rust.md"));

            assert_eq!(
                result.as_ref().map_err(ToString::to_string),
                world.expected_result.as_ref().map_err(Clone::clone)
            );
            assert_eq!(
                (
                    stdout.as_str(),
                    stderr.as_str(),
                    calls.as_slice(),
                    world.io.attempted.get(),
                    fs::read(repo.join(".git/index")).or_abort("saved index remains"),
                    fs::read(repo.join("user.txt")).or_abort("saved user remains"),
                    reference_read.as_ref().map_err(io::Error::kind)
                ),
                (
                    world.expected_stdout.as_str(),
                    "",
                    world.expected_calls.as_slice(),
                    world.expected_writes,
                    b"saved index".to_vec(),
                    b"saved user bytes".to_vec(),
                    world.expected_reference.as_ref().map_err(Clone::clone)
                )
            );
        }

        #[test]
        fn output_write_25_preserves_exact_prefix_and_saved_bytes() {
            let world = write_failed(
                with_remaining(
                    &NonEmptyString::try_from("2 files changed")
                        .or_abort("nonempty fixed remaining"),
                ),
                &with_reference(),
                &with_guidance(),
                NonZeroUsize::new(25).or_abort("positive write ordinal"),
            );
            let repo = world.directory.path();
            let ctx = Ctx {
                cwd: repo.to_path_buf(),
                env: &world.env,
                runner: &world.runner,
                io: &world.io,
                fs: &REAL_FS,
            };

            let result = super::super::print_hints_with_remaining_in(&ctx, &world.remaining);

            let (stdout, stderr, calls) = (
                world.io.stdout.borrow(),
                world.io.stderr.borrow(),
                world.runner.calls.borrow(),
            );

            let reference_read = fs::read(repo.join("references/rust.md"));

            assert_eq!(
                result.as_ref().map_err(ToString::to_string),
                world.expected_result.as_ref().map_err(Clone::clone)
            );
            assert_eq!(
                (
                    stdout.as_str(),
                    stderr.as_str(),
                    calls.as_slice(),
                    world.io.attempted.get(),
                    fs::read(repo.join(".git/index")).or_abort("saved index remains"),
                    fs::read(repo.join("user.txt")).or_abort("saved user remains"),
                    reference_read.as_ref().map_err(io::Error::kind)
                ),
                (
                    world.expected_stdout.as_str(),
                    "",
                    world.expected_calls.as_slice(),
                    world.expected_writes,
                    b"saved index".to_vec(),
                    b"saved user bytes".to_vec(),
                    world.expected_reference.as_ref().map_err(Clone::clone)
                )
            );
        }

        #[test]
        fn output_write_26_preserves_exact_prefix_and_saved_bytes() {
            let world = write_failed(
                with_remaining(
                    &NonEmptyString::try_from("2 files changed")
                        .or_abort("nonempty fixed remaining"),
                ),
                &with_reference(),
                &with_guidance(),
                NonZeroUsize::new(26).or_abort("positive write ordinal"),
            );
            let repo = world.directory.path();
            let ctx = Ctx {
                cwd: repo.to_path_buf(),
                env: &world.env,
                runner: &world.runner,
                io: &world.io,
                fs: &REAL_FS,
            };

            let result = super::super::print_hints_with_remaining_in(&ctx, &world.remaining);

            let (stdout, stderr, calls) = (
                world.io.stdout.borrow(),
                world.io.stderr.borrow(),
                world.runner.calls.borrow(),
            );

            let reference_read = fs::read(repo.join("references/rust.md"));

            assert_eq!(
                result.as_ref().map_err(ToString::to_string),
                world.expected_result.as_ref().map_err(Clone::clone)
            );
            assert_eq!(
                (
                    stdout.as_str(),
                    stderr.as_str(),
                    calls.as_slice(),
                    world.io.attempted.get(),
                    fs::read(repo.join(".git/index")).or_abort("saved index remains"),
                    fs::read(repo.join("user.txt")).or_abort("saved user remains"),
                    reference_read.as_ref().map_err(io::Error::kind)
                ),
                (
                    world.expected_stdout.as_str(),
                    "",
                    world.expected_calls.as_slice(),
                    world.expected_writes,
                    b"saved index".to_vec(),
                    b"saved user bytes".to_vec(),
                    world.expected_reference.as_ref().map_err(Clone::clone)
                )
            );
        }

        #[test]
        fn output_write_27_preserves_exact_prefix_and_saved_bytes() {
            let world = write_failed(
                with_remaining(
                    &NonEmptyString::try_from("2 files changed")
                        .or_abort("nonempty fixed remaining"),
                ),
                &with_reference(),
                &with_guidance(),
                NonZeroUsize::new(27).or_abort("positive write ordinal"),
            );
            let repo = world.directory.path();
            let ctx = Ctx {
                cwd: repo.to_path_buf(),
                env: &world.env,
                runner: &world.runner,
                io: &world.io,
                fs: &REAL_FS,
            };

            let result = super::super::print_hints_with_remaining_in(&ctx, &world.remaining);

            let (stdout, stderr, calls) = (
                world.io.stdout.borrow(),
                world.io.stderr.borrow(),
                world.runner.calls.borrow(),
            );

            let reference_read = fs::read(repo.join("references/rust.md"));

            assert_eq!(
                result.as_ref().map_err(ToString::to_string),
                world.expected_result.as_ref().map_err(Clone::clone)
            );
            assert_eq!(
                (
                    stdout.as_str(),
                    stderr.as_str(),
                    calls.as_slice(),
                    world.io.attempted.get(),
                    fs::read(repo.join(".git/index")).or_abort("saved index remains"),
                    fs::read(repo.join("user.txt")).or_abort("saved user remains"),
                    reference_read.as_ref().map_err(io::Error::kind)
                ),
                (
                    world.expected_stdout.as_str(),
                    "",
                    world.expected_calls.as_slice(),
                    world.expected_writes,
                    b"saved index".to_vec(),
                    b"saved user bytes".to_vec(),
                    world.expected_reference.as_ref().map_err(Clone::clone)
                )
            );
        }

        #[test]
        fn output_write_28_preserves_exact_prefix_and_saved_bytes() {
            let world = write_failed(
                with_remaining(
                    &NonEmptyString::try_from("2 files changed")
                        .or_abort("nonempty fixed remaining"),
                ),
                &with_reference(),
                &with_guidance(),
                NonZeroUsize::new(28).or_abort("positive write ordinal"),
            );
            let repo = world.directory.path();
            let ctx = Ctx {
                cwd: repo.to_path_buf(),
                env: &world.env,
                runner: &world.runner,
                io: &world.io,
                fs: &REAL_FS,
            };

            let result = super::super::print_hints_with_remaining_in(&ctx, &world.remaining);

            let (stdout, stderr, calls) = (
                world.io.stdout.borrow(),
                world.io.stderr.borrow(),
                world.runner.calls.borrow(),
            );

            let reference_read = fs::read(repo.join("references/rust.md"));

            assert_eq!(
                result.as_ref().map_err(ToString::to_string),
                world.expected_result.as_ref().map_err(Clone::clone)
            );
            assert_eq!(
                (
                    stdout.as_str(),
                    stderr.as_str(),
                    calls.as_slice(),
                    world.io.attempted.get(),
                    fs::read(repo.join(".git/index")).or_abort("saved index remains"),
                    fs::read(repo.join("user.txt")).or_abort("saved user remains"),
                    reference_read.as_ref().map_err(io::Error::kind)
                ),
                (
                    world.expected_stdout.as_str(),
                    "",
                    world.expected_calls.as_slice(),
                    world.expected_writes,
                    b"saved index".to_vec(),
                    b"saved user bytes".to_vec(),
                    world.expected_reference.as_ref().map_err(Clone::clone)
                )
            );
        }

        #[test]
        fn output_write_29_preserves_exact_prefix_and_saved_bytes() {
            let world = write_failed(
                with_remaining(
                    &NonEmptyString::try_from("2 files changed")
                        .or_abort("nonempty fixed remaining"),
                ),
                &with_reference(),
                &with_guidance(),
                NonZeroUsize::new(29).or_abort("positive write ordinal"),
            );
            let repo = world.directory.path();
            let ctx = Ctx {
                cwd: repo.to_path_buf(),
                env: &world.env,
                runner: &world.runner,
                io: &world.io,
                fs: &REAL_FS,
            };

            let result = super::super::print_hints_with_remaining_in(&ctx, &world.remaining);

            let (stdout, stderr, calls) = (
                world.io.stdout.borrow(),
                world.io.stderr.borrow(),
                world.runner.calls.borrow(),
            );

            let reference_read = fs::read(repo.join("references/rust.md"));

            assert_eq!(
                result.as_ref().map_err(ToString::to_string),
                world.expected_result.as_ref().map_err(Clone::clone)
            );
            assert_eq!(
                (
                    stdout.as_str(),
                    stderr.as_str(),
                    calls.as_slice(),
                    world.io.attempted.get(),
                    fs::read(repo.join(".git/index")).or_abort("saved index remains"),
                    fs::read(repo.join("user.txt")).or_abort("saved user remains"),
                    reference_read.as_ref().map_err(io::Error::kind)
                ),
                (
                    world.expected_stdout.as_str(),
                    "",
                    world.expected_calls.as_slice(),
                    world.expected_writes,
                    b"saved index".to_vec(),
                    b"saved user bytes".to_vec(),
                    world.expected_reference.as_ref().map_err(Clone::clone)
                )
            );
        }

        #[test]
        fn output_write_30_preserves_exact_prefix_and_saved_bytes() {
            let world = write_failed(
                with_remaining(
                    &NonEmptyString::try_from("2 files changed")
                        .or_abort("nonempty fixed remaining"),
                ),
                &with_reference(),
                &with_guidance(),
                NonZeroUsize::new(30).or_abort("positive write ordinal"),
            );
            let repo = world.directory.path();
            let ctx = Ctx {
                cwd: repo.to_path_buf(),
                env: &world.env,
                runner: &world.runner,
                io: &world.io,
                fs: &REAL_FS,
            };

            let result = super::super::print_hints_with_remaining_in(&ctx, &world.remaining);

            let (stdout, stderr, calls) = (
                world.io.stdout.borrow(),
                world.io.stderr.borrow(),
                world.runner.calls.borrow(),
            );

            let reference_read = fs::read(repo.join("references/rust.md"));

            assert_eq!(
                result.as_ref().map_err(ToString::to_string),
                world.expected_result.as_ref().map_err(Clone::clone)
            );
            assert_eq!(
                (
                    stdout.as_str(),
                    stderr.as_str(),
                    calls.as_slice(),
                    world.io.attempted.get(),
                    fs::read(repo.join(".git/index")).or_abort("saved index remains"),
                    fs::read(repo.join("user.txt")).or_abort("saved user remains"),
                    reference_read.as_ref().map_err(io::Error::kind)
                ),
                (
                    world.expected_stdout.as_str(),
                    "",
                    world.expected_calls.as_slice(),
                    world.expected_writes,
                    b"saved index".to_vec(),
                    b"saved user bytes".to_vec(),
                    world.expected_reference.as_ref().map_err(Clone::clone)
                )
            );
        }
    }

    use super::*;
    use std::env;
    use std::ffi::OsString;
    use std::os::unix::process::ExitStatusExt as _;
    use std::process::Output;
    use std::sync::{Mutex, PoisonError};
    use tempfile::TempDir;

    struct HintEnv {
        claude_code: bool,
        cwd: PathBuf,
    }

    impl Env for HintEnv {
        fn current_dir(&self) -> io::Result<PathBuf> {
            Ok(self.cwd.clone())
        }

        fn current_exe(&self) -> io::Result<PathBuf> {
            env::current_exe()
        }

        fn var_os(&self, key: &str) -> Option<OsString> {
            if key == "CLAUDECODE" && self.claude_code {
                return Some(OsString::from("1"));
            }
            None
        }
    }

    #[derive(Default)]
    struct BufferIo {
        stderr: Mutex<String>,
        stdout: Mutex<String>,
    }

    impl BufferIo {
        fn stdout(&self) -> String {
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

    #[derive(Copy, Clone, Eq, PartialEq)]
    enum HintFailure {
        DiffStat,
        TopLevel,
        Untracked,
    }

    struct HintRunner {
        diff_stat: String,
        fail_on: Option<HintFailure>,
        toplevel: PathBuf,
        untracked: String,
    }

    impl Runner for HintRunner {
        fn output(&self, _bin: &str, args: &[&str], _cwd: &Path) -> io::Result<Output> {
            if args == ["diff", "--stat"] && self.fail_on == Some(HintFailure::DiffStat) {
                return Err(io::Error::other("forced diff failure"));
            }
            if args == ["ls-files", "--others", "--exclude-standard"]
                && self.fail_on == Some(HintFailure::Untracked)
            {
                return Err(io::Error::other("forced untracked failure"));
            }
            if args == ["rev-parse", "--show-toplevel"]
                && self.fail_on == Some(HintFailure::TopLevel)
            {
                return Err(io::Error::other("forced show-toplevel failure"));
            }
            let stdout = match *args {
                ["diff", "--stat"] => self.diff_stat.as_bytes().to_vec(),
                ["ls-files", "--others", "--exclude-standard"] => {
                    self.untracked.as_bytes().to_vec()
                }
                ["rev-parse", "--show-toplevel"] => {
                    format!("{}\n", self.toplevel.display()).into_bytes()
                }
                _ => {
                    return Err(io::Error::other(format!(
                        "unexpected args: {}",
                        args.join(" ")
                    )));
                }
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

    fn git_command_message(error: &FactorError) -> Option<String> {
        if let &FactorError::GitCommand(_) = error {
            return error
                .to_string()
                .strip_prefix("git command failed: ")
                .map(str::to_owned);
        }
        None
    }

    #[test]
    fn io_helpers_cover_buffer_and_error_extractor_paths() {
        let buffer = BufferIo::default();
        buffer.err("warn").or_abort("buffer err");
        buffer.errln("note").or_abort("buffer errln");
        buffer.out("ok").or_abort("buffer out");
        buffer.outln("done").or_abort("buffer outln");

        let stderr = buffer
            .stderr
            .lock()
            .or_abort("stderr lock should not be poisoned")
            .clone();
        assert_eq!(stderr, "warnnote\n");
        assert_eq!(buffer.stdout(), "okdone\n");

        let fail_io = FailOnExactTextIo {
            text: "trigger".to_owned(),
        };
        fail_io.errln("ignored").or_abort("fail io errln");
        let out_err = fail_io
            .out("trigger")
            .err_or_abort("exact text should fail");
        assert_eq!(out_err.to_string(), "io fail");
        let outln_err = fail_io
            .outln("trigger")
            .err_or_abort("exact line should fail");
        assert_eq!(outln_err.to_string(), "io fail");

        assert!(git_command_message(&FactorError::NoActiveSession).is_none());
    }

    /// Prints contextual hints to guide the next commit.
    ///
    /// Displays remaining diff size, reference file locations, and recovery
    /// instructions after each successful split or session start. When running
    /// under Claude Code (`CLAUDECODE=1`), adds LLM-specific guidance.
    pub(in crate::git_factor) fn print_hints_in(ctx: &Ctx<'_>) -> Result<(), FactorError> {
        let stat = git_output(ctx, &["diff", "--stat"])?;
        let remaining = stat.lines().last().unwrap_or_default().to_owned();
        print_hints_with_remaining_in(ctx, remaining.as_str())
    }

    #[test]
    fn print_hints_in_includes_reference_and_claude_guidance() {
        let dir = TempDir::new().or_abort("tempdir");
        let references_dir = dir.path().join("references");
        fs::create_dir_all(&references_dir).or_abort("create references dir");
        let rust_reference = references_dir.join("rust.md");
        fs::write(&rust_reference, "# rust\n").or_abort("write rust reference");

        let runner = HintRunner {
            diff_stat: " file.txt | 1 +\n 1 file changed, 1 insertion(+)\n".to_owned(),
            untracked: String::new(),
            toplevel: dir.path().to_path_buf(),
            fail_on: None,
        };
        let io = BufferIo::default();
        let env = HintEnv {
            cwd: dir.path().to_path_buf(),
            claude_code: true,
        };
        assert_eq!(
            env.current_dir().or_abort("hint env cwd"),
            dir.path().to_path_buf()
        );
        let exe = env.current_exe().or_abort("hint env current_exe");
        assert!(exe.is_absolute(), "current_exe should be absolute: {exe:?}");
        assert_eq!(env.var_os("CLAUDECODE"), Some(OsString::from("1")));
        assert!(env.var_os("OTHER_ENV").is_none());
        io.err("note").or_abort("write stderr");
        let ctx = Ctx {
            runner: &runner,
            cwd: dir.path().to_path_buf(),
            io: &io,
            env: &env,
            fs: &REAL_FS,
        };

        print_hints_in(&ctx).or_abort("print_hints_in should succeed");
        let stdout = io.stdout();
        assert!(stdout.contains("HINTS:"), "stdout: {stdout}");
        assert!(stdout.contains("REMAINING:"), "stdout: {stdout}");
        assert!(
            stdout.contains("1 file changed, 1 insertion(+)"),
            "stdout: {stdout}"
        );
        assert!(
            stdout.contains(&format!("REFERENCE: {}", rust_reference.display())),
            "stdout: {stdout}"
        );
        assert!(stdout.contains("<claude>"), "stdout: {stdout}");
        assert!(
            stdout.contains("ONLY use git-factor --continue"),
            "stdout: {stdout}"
        );
    }

    #[test]
    fn print_hints_in_omits_remaining_when_diff_stat_is_empty() {
        let dir = TempDir::new().or_abort("tempdir");
        let runner = HintRunner {
            diff_stat: String::new(),
            untracked: String::new(),
            toplevel: dir.path().to_path_buf(),
            fail_on: None,
        };
        let io = BufferIo::default();
        let env = HintEnv {
            cwd: dir.path().to_path_buf(),
            claude_code: false,
        };
        let ctx = Ctx {
            runner: &runner,
            cwd: dir.path().to_path_buf(),
            io: &io,
            env: &env,
            fs: &REAL_FS,
        };

        print_hints_in(&ctx).or_abort("print_hints_in should succeed");
        let stdout = io.stdout();
        assert!(stdout.contains("HINTS:"), "stdout: {stdout}");
        assert!(!stdout.contains("REMAINING:"), "stdout: {stdout}");
        assert!(
            stdout.contains("RECOVERY: git factor --abort"),
            "stdout: {stdout}"
        );
    }

    #[test]
    fn print_hints_in_reports_diff_stat_failure() {
        let dir = TempDir::new().or_abort("tempdir");
        let runner = HintRunner {
            diff_stat: String::new(),
            untracked: String::new(),
            toplevel: dir.path().to_path_buf(),
            fail_on: Some(HintFailure::DiffStat),
        };
        let env = HintEnv {
            cwd: dir.path().to_path_buf(),
            claude_code: false,
        };
        let ctx = Ctx {
            runner: &runner,
            cwd: dir.path().to_path_buf(),
            io: &REAL_IO,
            env: &env,
            fs: &REAL_FS,
        };

        let err = print_hints_in(&ctx).err_or_abort("diff stat failure should propagate");
        let message = git_command_message(&err).or_abort("expected GitCommand");
        assert!(
            message.contains("forced diff failure"),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    fn print_session_started_reports_single_commit_and_untracked_paths() {
        let dir = TempDir::new().or_abort("tempdir");
        let references_dir = dir.path().join("references");
        fs::create_dir_all(&references_dir).or_abort("create references dir");
        fs::write(references_dir.join("rust.md"), "# rust\n").or_abort("write rust reference");

        let runner = HintRunner {
            diff_stat: " file.txt | 1 +\n 1 file changed, 1 insertion(+)\n".to_owned(),
            untracked: "new.txt\n".to_owned(),
            toplevel: dir.path().to_path_buf(),
            fail_on: None,
        };
        let io = BufferIo::default();
        let env = HintEnv {
            cwd: dir.path().to_path_buf(),
            claude_code: false,
        };
        let status = runner
            .status("git", &["status"], &[], true, dir.path())
            .or_abort("runner status");
        assert!(status.success(), "status should be success");
        let unexpected = runner
            .output("git", &["unexpected"], dir.path())
            .err_or_abort("unexpected command should fail");
        assert!(
            unexpected.to_string().contains("unexpected args"),
            "unexpected error: {unexpected:?}"
        );
        let ctx = Ctx {
            runner: &runner,
            cwd: dir.path().to_path_buf(),
            io: &io,
            env: &env,
            fs: &REAL_FS,
        };

        print_session_started(
            &ctx,
            "FACTOR: Split session started for aaaaaaa.",
            "feat: example",
        )
        .or_abort("print_session_started should succeed");
        let stdout = io.stdout();
        assert!(
            stdout.contains("FACTOR: Split session started for aaaaaaa."),
            "stdout: {stdout}"
        );
        assert!(
            stdout.contains("ORIGINAL MESSAGE: feat: example"),
            "stdout: {stdout}"
        );
        assert!(stdout.contains("UNTRACKED:"), "stdout: {stdout}");
        assert!(stdout.contains("new.txt"), "stdout: {stdout}");
        assert!(
            stdout.contains(
                "Run git factor -h for command help or git-factor --help for the full workflow guide."
            ),
            "stdout: {stdout}"
        );
    }

    #[test]
    fn print_session_started_reports_advance_guidance() {
        let dir = TempDir::new().or_abort("tempdir");
        let runner = HintRunner {
            diff_stat: " file.txt | 1 +\n 1 file changed, 1 insertion(+)\n".to_owned(),
            untracked: String::new(),
            toplevel: dir.path().to_path_buf(),
            fail_on: None,
        };
        let io = BufferIo::default();
        let env = HintEnv {
            cwd: dir.path().to_path_buf(),
            claude_code: false,
        };
        let ctx = Ctx {
            runner: &runner,
            cwd: dir.path().to_path_buf(),
            io: &io,
            env: &env,
            fs: &REAL_FS,
        };

        print_session_started(&ctx, "FACTOR: Now splitting aaaaaaa.", "feat: example")
            .or_abort("print_session_started should succeed");
        let stdout = io.stdout();
        assert!(
            stdout.contains("NEXT: Stage changes for the next commit, then run:"),
            "stdout: {stdout}"
        );
        assert!(
            !stdout.contains(
                "Run git factor -h for command help or git-factor --help for the full workflow guide."
            ),
            "stdout: {stdout}"
        );
    }

    #[test]
    fn print_session_started_reports_advance_guidance_output_failure() {
        let dir = TempDir::new().or_abort("tempdir");
        let runner = HintRunner {
            diff_stat: " file.txt | 1 +\n 1 file changed, 1 insertion(+)\n".to_owned(),
            untracked: String::new(),
            toplevel: dir.path().to_path_buf(),
            fail_on: None,
        };
        let io = FailOnExactTextIo {
            text: "NEXT: Stage changes for the next commit, then run:".to_owned(),
        };
        let env = HintEnv {
            cwd: dir.path().to_path_buf(),
            claude_code: false,
        };
        let ctx = Ctx {
            runner: &runner,
            cwd: dir.path().to_path_buf(),
            io: &io,
            env: &env,
            fs: &REAL_FS,
        };

        let err = print_session_started(&ctx, "FACTOR: Now splitting aaaaaaa.", "feat: example")
            .err_or_abort("advance guidance output should fail");
        assert!(
            err.to_string().contains("io fail"),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    fn print_hints_in_reports_io_failures_for_reference_and_claude_lines() {
        let dir = TempDir::new().or_abort("tempdir");
        let references_dir = dir.path().join("references");
        fs::create_dir_all(&references_dir).or_abort("create references dir");
        let rust_reference = references_dir.join("rust.md");
        fs::write(&rust_reference, "# rust\n").or_abort("write rust reference");

        let runner = HintRunner {
            diff_stat: " file.txt | 1 +\n 1 file changed, 1 insertion(+)\n".to_owned(),
            untracked: String::new(),
            toplevel: dir.path().to_path_buf(),
            fail_on: None,
        };
        let env = HintEnv {
            cwd: dir.path().to_path_buf(),
            claude_code: false,
        };
        let io_reference = FailOnExactTextIo {
            text: format!("  REFERENCE: {}", rust_reference.display()),
        };
        io_reference.err("stderr").or_abort("err should succeed");
        let ctx_reference = Ctx {
            runner: &runner,
            cwd: dir.path().to_path_buf(),
            io: &io_reference,
            env: &env,
            fs: &REAL_FS,
        };

        let reference_err =
            print_hints_in(&ctx_reference).err_or_abort("reference output should fail");
        assert!(
            reference_err.to_string().contains("io fail"),
            "unexpected error: {reference_err:?}"
        );

        let claude_env = HintEnv {
            cwd: dir.path().to_path_buf(),
            claude_code: true,
        };
        let io_claude = FailOnExactTextIo {
            text: "</claude>".to_owned(),
        };
        let ctx_claude = Ctx {
            runner: &runner,
            cwd: dir.path().to_path_buf(),
            io: &io_claude,
            env: &claude_env,
            fs: &REAL_FS,
        };

        let claude_err = print_hints_in(&ctx_claude).err_or_abort("claude output should fail");
        assert!(
            claude_err.to_string().contains("io fail"),
            "unexpected error: {claude_err:?}"
        );

        let toplevel_failure_runner = HintRunner {
            diff_stat: " file.txt | 1 +\n".to_owned(),
            untracked: String::new(),
            toplevel: dir.path().to_path_buf(),
            fail_on: Some(HintFailure::TopLevel),
        };
        let toplevel_ctx = Ctx {
            runner: &toplevel_failure_runner,
            cwd: dir.path().to_path_buf(),
            io: &REAL_IO,
            env: &env,
            fs: &REAL_FS,
        };
        let toplevel_err = print_hints_in(&toplevel_ctx).err_or_abort("show-toplevel should fail");
        let toplevel_message = git_command_message(&toplevel_err).or_abort("expected GitCommand");
        assert!(
            toplevel_message.contains("forced show-toplevel failure"),
            "unexpected error: {toplevel_err:?}"
        );
    }

    #[test]
    fn print_session_started_reports_runner_failures() {
        let dir = TempDir::new().or_abort("tempdir");
        let started = "FACTOR: Split session started for aaaaaaa.";
        let env = HintEnv {
            cwd: dir.path().to_path_buf(),
            claude_code: false,
        };

        let diff_failure_runner = HintRunner {
            diff_stat: String::new(),
            untracked: String::new(),
            toplevel: dir.path().to_path_buf(),
            fail_on: Some(HintFailure::DiffStat),
        };
        let diff_ctx = Ctx {
            runner: &diff_failure_runner,
            cwd: dir.path().to_path_buf(),
            io: &REAL_IO,
            env: &env,
            fs: &REAL_FS,
        };
        let diff_err = print_session_started(&diff_ctx, started, "feat: test")
            .err_or_abort("diff command should fail");
        let diff_message = git_command_message(&diff_err).or_abort("expected GitCommand");
        assert!(
            diff_message.contains("forced diff failure"),
            "unexpected error: {diff_err:?}"
        );

        let untracked_failure_runner = HintRunner {
            diff_stat: " file.txt | 1 +\n".to_owned(),
            untracked: String::new(),
            toplevel: dir.path().to_path_buf(),
            fail_on: Some(HintFailure::Untracked),
        };
        let untracked_ctx = Ctx {
            runner: &untracked_failure_runner,
            cwd: dir.path().to_path_buf(),
            io: &REAL_IO,
            env: &env,
            fs: &REAL_FS,
        };
        let untracked_err = print_session_started(&untracked_ctx, started, "feat: test")
            .err_or_abort("untracked command should fail");
        let untracked_message = git_command_message(&untracked_err).or_abort("expected GitCommand");
        assert!(
            untracked_message.contains("forced untracked failure"),
            "unexpected error: {untracked_err:?}"
        );
    }

    #[test]
    fn print_session_started_reports_output_failures() {
        let dir = TempDir::new().or_abort("tempdir");
        let started = "FACTOR: Split session started for aaaaaaa.";
        let env = HintEnv {
            claude_code: false,
            cwd: dir.path().to_path_buf(),
        };

        let io_runner = HintRunner {
            diff_stat: " file.txt | 1 +\n".to_owned(),
            fail_on: None,
            toplevel: dir.path().to_path_buf(),
            untracked: "new.txt\n".to_owned(),
        };

        let stat_line_io = FailOnExactTextIo {
            text: "  file.txt | 1 +".to_owned(),
        };
        let stat_line_ctx = Ctx {
            runner: &io_runner,
            cwd: dir.path().to_path_buf(),
            io: &stat_line_io,
            env: &env,
            fs: &REAL_FS,
        };
        let stat_line_err = print_session_started(&stat_line_ctx, started, "feat: test")
            .err_or_abort("stat line should fail");
        assert!(
            stat_line_err.to_string().contains("io fail"),
            "unexpected error: {stat_line_err:?}"
        );

        let header_io = FailOnExactTextIo {
            text: "UNTRACKED:".to_owned(),
        };
        let header_ctx = Ctx {
            runner: &io_runner,
            cwd: dir.path().to_path_buf(),
            io: &header_io,
            env: &env,
            fs: &REAL_FS,
        };
        let header_err = print_session_started(&header_ctx, started, "feat: test")
            .err_or_abort("untracked header should fail");
        assert!(
            header_err.to_string().contains("io fail"),
            "unexpected error: {header_err:?}"
        );

        let line_io = FailOnExactTextIo {
            text: "  new.txt".to_owned(),
        };
        let line_ctx = Ctx {
            runner: &io_runner,
            cwd: dir.path().to_path_buf(),
            io: &line_io,
            env: &env,
            fs: &REAL_FS,
        };
        let line_err = print_session_started(&line_ctx, started, "feat: test")
            .err_or_abort("untracked line should fail");
        assert!(
            line_err.to_string().contains("io fail"),
            "unexpected error: {line_err:?}"
        );

        let footer_io = FailOnExactTextIo {
            text:
                "Run git factor -h for command help or git-factor --help for the full workflow guide."
                    .to_owned(),
        };
        let footer_ctx = Ctx {
            runner: &io_runner,
            cwd: dir.path().to_path_buf(),
            io: &footer_io,
            env: &env,
            fs: &REAL_FS,
        };
        let footer_err = print_session_started(&footer_ctx, started, "feat: test")
            .err_or_abort("footer output should fail");
        assert!(
            footer_err.to_string().contains("io fail"),
            "unexpected error: {footer_err:?}"
        );
    }
}

#[cfg(test)]
#[path = "ui_hint_contracts.rs"]
mod hint_contracts;

#[cfg(test)]
mod proptests {
    mod print_hints_with_remaining_in {
        use super::super::hint_contracts::{
            complete, rejected, spawn_failed, with_guidance, with_guidance_value, with_reference,
            with_remaining, without_guidance, without_reference, without_remaining, write_failed,
        };
        use super::super::*;
        use crate::non_empty_string::NonEmptyString;
        use crate::test_support::OrAbort as _;
        use core::num::{NonZeroU8, NonZeroUsize};
        use core::ops::RangeInclusive;
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn renders_generated_remaining_reference_and_environment(
                remaining in prop_oneof![Just(()).prop_map(|()| without_remaining()),
                    "[A-Za-z0-9 ,]{1,40}".prop_map(|input| {
                        with_remaining(
                            &NonEmptyString::try_from(input).or_abort("nonempty generated remaining"),
                        )
                    })],
                reference in prop_oneof![Just(()).prop_map(|()| without_reference()),
                    Just(()).prop_map(|()| with_reference())],
                guidance in prop_oneof![Just(()).prop_map(|()| without_guidance()),
                    any::<String>().prop_map(|value| with_guidance_value(OsString::from(value)))]
            ) {
                let world = complete(remaining, &reference, &guidance);
                let repo = world.directory.path();
                let ctx = Ctx {
                    cwd: repo.to_path_buf(),
                    env: &world.env,
                    runner: &world.runner,
                    io: &world.io,
                    fs: &REAL_FS,
                };

                let result =
                    super::super::print_hints_with_remaining_in(&ctx, &world.remaining);

                let (stdout, stderr, calls) = (
                    world.io.stdout.borrow(),
                    world.io.stderr.borrow(),
                    world.runner.calls.borrow(),
                );

                let reference_read = fs::read(repo.join("references/rust.md"));

                prop_assert_eq!(
                    result.as_ref().map_err(ToString::to_string),
                    world.expected_result.as_ref().map_err(Clone::clone)
                );
                prop_assert_eq!(
                    (
                        stdout.as_str(),
                        stderr.as_str(),
                        calls.as_slice(),
                        world.io.attempted.get(),
                        fs::read(repo.join(".git/index")).or_abort("saved index remains"),
                        fs::read(repo.join("user.txt")).or_abort("saved user remains"),
                        reference_read.as_ref().map_err(io::Error::kind)
                    ),
                    (
                        world.expected_stdout.as_str(),
                        "",
                        world.expected_calls.as_slice(),
                        world.expected_writes,
                        b"saved index".to_vec(),
                        b"saved user bytes".to_vec(),
                        world.expected_reference.as_ref().map_err(Clone::clone)
                    )
                );
            }

            #[test]
            fn query_rejections_preserve_generated_inputs(
                remaining in "[A-Za-z0-9 ,]{0,40}",
                code in RangeInclusive::<u8>::new(1, u8::MAX),
                reference in prop_oneof![Just(()).prop_map(|()| without_reference()),
                    Just(()).prop_map(|()| with_reference())],
                guidance in prop_oneof![Just(()).prop_map(|()| without_guidance()),
                    Just(()).prop_map(|()| with_guidance())]
            ) {
                let world = rejected(
                    remaining,
                    &reference,
                    &guidance,
                    NonZeroU8::new(code).or_abort("generated nonzero rejection"),
                );
                let repo = world.directory.path();
                let ctx = Ctx {
                    cwd: repo.to_path_buf(),
                    env: &world.env,
                    runner: &world.runner,
                    io: &world.io,
                    fs: &REAL_FS,
                };

                let result =
                    super::super::print_hints_with_remaining_in(&ctx, &world.remaining);

                let (stdout, stderr, calls) = (
                    world.io.stdout.borrow(),
                    world.io.stderr.borrow(),
                    world.runner.calls.borrow(),
                );

                let reference_read = fs::read(repo.join("references/rust.md"));

                prop_assert_eq!(
                    result.as_ref().map_err(ToString::to_string),
                    world.expected_result.as_ref().map_err(Clone::clone)
                );
                prop_assert_eq!(
                    (
                        stdout.as_str(),
                        stderr.as_str(),
                        calls.as_slice(),
                        world.io.attempted.get(),
                        fs::read(repo.join(".git/index")).or_abort("saved index remains"),
                        fs::read(repo.join("user.txt")).or_abort("saved user remains"),
                        reference_read.as_ref().map_err(io::Error::kind)
                    ),
                    (
                        world.expected_stdout.as_str(),
                        "",
                        world.expected_calls.as_slice(),
                        world.expected_writes,
                        b"saved index".to_vec(),
                        b"saved user bytes".to_vec(),
                        world.expected_reference.as_ref().map_err(Clone::clone)
                    )
                );
            }

            #[test]
            fn query_spawn_preserves_generated_inputs(
                remaining in "[A-Za-z0-9 ,]{0,40}",
                reference in prop_oneof![Just(()).prop_map(|()| without_reference()),
                    Just(()).prop_map(|()| with_reference())],
                guidance in prop_oneof![Just(()).prop_map(|()| without_guidance()),
                    Just(()).prop_map(|()| with_guidance())]
            ) {
                let world = spawn_failed(remaining, &reference, &guidance);
                let repo = world.directory.path();
                let ctx = Ctx {
                    cwd: repo.to_path_buf(),
                    env: &world.env,
                    runner: &world.runner,
                    io: &world.io,
                    fs: &REAL_FS,
                };

                let result =
                    super::super::print_hints_with_remaining_in(&ctx, &world.remaining);

                let (stdout, stderr, calls) = (
                    world.io.stdout.borrow(),
                    world.io.stderr.borrow(),
                    world.runner.calls.borrow(),
                );

                let reference_read = fs::read(repo.join("references/rust.md"));

                prop_assert_eq!(
                    result.as_ref().map_err(ToString::to_string),
                    world.expected_result.as_ref().map_err(Clone::clone)
                );
                prop_assert_eq!(
                    (
                        stdout.as_str(),
                        stderr.as_str(),
                        calls.as_slice(),
                        world.io.attempted.get(),
                        fs::read(repo.join(".git/index")).or_abort("saved index remains"),
                        fs::read(repo.join("user.txt")).or_abort("saved user remains"),
                        reference_read.as_ref().map_err(io::Error::kind)
                    ),
                    (
                        world.expected_stdout.as_str(),
                        "",
                        world.expected_calls.as_slice(),
                        world.expected_writes,
                        b"saved index".to_vec(),
                        b"saved user bytes".to_vec(),
                        world.expected_reference.as_ref().map_err(Clone::clone)
                    )
                );
            }

            #[test]
            fn base_hint_writes(
                remaining in "[A-Za-z0-9 ,]{1,40}",
                slot in RangeInclusive::<usize>::new(1, 12)
            ) {
                let world = write_failed(
                    with_remaining(
                        &NonEmptyString::try_from(remaining)
                            .or_abort("nonempty generated remaining"),
                    ),
                    &with_reference(),
                    &with_guidance(),
                    NonZeroUsize::new(slot).or_abort("generated write ordinal"),
                );
                let repo = world.directory.path();
                let ctx = Ctx {
                    cwd: repo.to_path_buf(),
                    env: &world.env,
                    runner: &world.runner,
                    io: &world.io,
                    fs: &REAL_FS,
                };

                let result =
                    super::super::print_hints_with_remaining_in(&ctx, &world.remaining);

                let (stdout, stderr, calls) = (
                    world.io.stdout.borrow(),
                    world.io.stderr.borrow(),
                    world.runner.calls.borrow(),
                );

                let reference_read = fs::read(repo.join("references/rust.md"));

                prop_assert_eq!(
                    result.as_ref().map_err(ToString::to_string),
                    world.expected_result.as_ref().map_err(Clone::clone)
                );
                prop_assert_eq!(
                    (
                        stdout.as_str(),
                        stderr.as_str(),
                        calls.as_slice(),
                        world.io.attempted.get(),
                        fs::read(repo.join(".git/index")).or_abort("saved index remains"),
                        fs::read(repo.join("user.txt")).or_abort("saved user remains"),
                        reference_read.as_ref().map_err(io::Error::kind)
                    ),
                    (
                        world.expected_stdout.as_str(),
                        "",
                        world.expected_calls.as_slice(),
                        world.expected_writes,
                        b"saved index".to_vec(),
                        b"saved user bytes".to_vec(),
                        world.expected_reference.as_ref().map_err(Clone::clone)
                    )
                );
            }

            #[test]
            fn remaining_and_reference_writes(
                remaining in "[A-Za-z0-9 ,]{1,40}",
                slot in RangeInclusive::<usize>::new(13, 16)
            ) {
                let world = write_failed(
                    with_remaining(
                        &NonEmptyString::try_from(remaining)
                            .or_abort("nonempty generated remaining"),
                    ),
                    &with_reference(),
                    &with_guidance(),
                    NonZeroUsize::new(slot).or_abort("generated write ordinal"),
                );
                let repo = world.directory.path();
                let ctx = Ctx {
                    cwd: repo.to_path_buf(),
                    env: &world.env,
                    runner: &world.runner,
                    io: &world.io,
                    fs: &REAL_FS,
                };

                let result =
                    super::super::print_hints_with_remaining_in(&ctx, &world.remaining);

                let (stdout, stderr, calls) = (
                    world.io.stdout.borrow(),
                    world.io.stderr.borrow(),
                    world.runner.calls.borrow(),
                );

                let reference_read = fs::read(repo.join("references/rust.md"));

                prop_assert_eq!(
                    result.as_ref().map_err(ToString::to_string),
                    world.expected_result.as_ref().map_err(Clone::clone)
                );
                prop_assert_eq!(
                    (
                        stdout.as_str(),
                        stderr.as_str(),
                        calls.as_slice(),
                        world.io.attempted.get(),
                        fs::read(repo.join(".git/index")).or_abort("saved index remains"),
                        fs::read(repo.join("user.txt")).or_abort("saved user remains"),
                        reference_read.as_ref().map_err(io::Error::kind)
                    ),
                    (
                        world.expected_stdout.as_str(),
                        "",
                        world.expected_calls.as_slice(),
                        world.expected_writes,
                        b"saved index".to_vec(),
                        b"saved user bytes".to_vec(),
                        world.expected_reference.as_ref().map_err(Clone::clone)
                    )
                );
            }

            #[test]
            fn recovery_writes(
                remaining in "[A-Za-z0-9 ,]{1,40}",
                slot in RangeInclusive::<usize>::new(17, 18)
            ) {
                let world = write_failed(
                    with_remaining(
                        &NonEmptyString::try_from(remaining)
                            .or_abort("nonempty generated remaining"),
                    ),
                    &with_reference(),
                    &with_guidance(),
                    NonZeroUsize::new(slot).or_abort("generated write ordinal"),
                );
                let repo = world.directory.path();
                let ctx = Ctx {
                    cwd: repo.to_path_buf(),
                    env: &world.env,
                    runner: &world.runner,
                    io: &world.io,
                    fs: &REAL_FS,
                };

                let result =
                    super::super::print_hints_with_remaining_in(&ctx, &world.remaining);

                let (stdout, stderr, calls) = (
                    world.io.stdout.borrow(),
                    world.io.stderr.borrow(),
                    world.runner.calls.borrow(),
                );

                let reference_read = fs::read(repo.join("references/rust.md"));

                prop_assert_eq!(
                    result.as_ref().map_err(ToString::to_string),
                    world.expected_result.as_ref().map_err(Clone::clone)
                );
                prop_assert_eq!(
                    (
                        stdout.as_str(),
                        stderr.as_str(),
                        calls.as_slice(),
                        world.io.attempted.get(),
                        fs::read(repo.join(".git/index")).or_abort("saved index remains"),
                        fs::read(repo.join("user.txt")).or_abort("saved user remains"),
                        reference_read.as_ref().map_err(io::Error::kind)
                    ),
                    (
                        world.expected_stdout.as_str(),
                        "",
                        world.expected_calls.as_slice(),
                        world.expected_writes,
                        b"saved index".to_vec(),
                        b"saved user bytes".to_vec(),
                        world.expected_reference.as_ref().map_err(Clone::clone)
                    )
                );
            }

            #[test]
            fn claude_guidance_writes(
                remaining in "[A-Za-z0-9 ,]{1,40}",
                slot in RangeInclusive::<usize>::new(19, 30)
            ) {
                let world = write_failed(
                    with_remaining(
                        &NonEmptyString::try_from(remaining)
                            .or_abort("nonempty generated remaining"),
                    ),
                    &with_reference(),
                    &with_guidance(),
                    NonZeroUsize::new(slot).or_abort("generated write ordinal"),
                );
                let repo = world.directory.path();
                let ctx = Ctx {
                    cwd: repo.to_path_buf(),
                    env: &world.env,
                    runner: &world.runner,
                    io: &world.io,
                    fs: &REAL_FS,
                };

                let result =
                    super::super::print_hints_with_remaining_in(&ctx, &world.remaining);

                let (stdout, stderr, calls) = (
                    world.io.stdout.borrow(),
                    world.io.stderr.borrow(),
                    world.runner.calls.borrow(),
                );

                let reference_read = fs::read(repo.join("references/rust.md"));

                prop_assert_eq!(
                    result.as_ref().map_err(ToString::to_string),
                    world.expected_result.as_ref().map_err(Clone::clone)
                );
                prop_assert_eq!(
                    (
                        stdout.as_str(),
                        stderr.as_str(),
                        calls.as_slice(),
                        world.io.attempted.get(),
                        fs::read(repo.join(".git/index")).or_abort("saved index remains"),
                        fs::read(repo.join("user.txt")).or_abort("saved user remains"),
                        reference_read.as_ref().map_err(io::Error::kind)
                    ),
                    (
                        world.expected_stdout.as_str(),
                        "",
                        world.expected_calls.as_slice(),
                        world.expected_writes,
                        b"saved index".to_vec(),
                        b"saved user bytes".to_vec(),
                        world.expected_reference.as_ref().map_err(Clone::clone)
                    )
                );
            }

        }
    }
}
