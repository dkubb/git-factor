#[cfg_attr(
    not(test),
    expect(
        clippy::wildcard_imports,
        reason = "module intentionally shares parent items via a grouped import"
    )
)]
use super::*;

/// Returns true if a rebase is currently in progress.
pub(in crate::git_factor) fn is_mid_rebase_in(ctx: &Ctx<'_>) -> bool {
    git_dir_in(ctx).is_ok_and(|dir| {
        ctx.fs.is_dir(&dir.join(REBASE_MERGE_DIR)) || ctx.fs.is_dir(&dir.join(REBASE_APPLY_DIR))
    })
}

#[cfg(test)]
#[path = "ui_fixture.rs"]
mod fixture;
#[cfg(test)]
#[path = "ui_proptests.rs"]
mod proptests;
#[cfg(test)]
#[path = "ui_tests.rs"]
mod tests;
