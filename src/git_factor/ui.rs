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

/// Prints contextual hints to guide the next commit.
///
/// Displays remaining diff size, reference file locations, and recovery
/// instructions after each successful split or session start. When running
/// under Claude Code (`CLAUDECODE=1`), adds LLM-specific guidance.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "tests exercise the full hint bundle through this convenience wrapper"
    )
)]
pub(in crate::git_factor) fn print_hints_in(ctx: &Ctx<'_>) -> Result<(), FactorError> {
    let stat = git_output(ctx, &["diff", "--stat"])?;
    let remaining = stat.lines().last().unwrap_or_default().to_owned();
    print_hints_with_remaining_in(ctx, remaining.as_str())
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
