#[cfg(test)]
use super::types::Commits;
#[cfg_attr(
    not(test),
    expect(
        clippy::wildcard_imports,
        reason = "module intentionally shares parent items via a grouped import"
    )
)]
use super::*;
#[cfg(test)]
use alloc::collections::BTreeSet;
#[cfg(test)]
use std::collections::HashSet;

/// Actual-parent admission contracts.
#[cfg(test)]
#[path = "validation_parent_tests.rs"]
mod parent_contracts;

/// Reads consecutive parent records from an actual commit object.
pub(in crate::git_factor) fn base_parent_in(
    ctx: &Ctx<'_>,
    commit: &CommitSha,
) -> Result<BaseParent, FactorError> {
    let object = git_output(ctx, &["cat-file", "commit", commit.as_str()])?;
    let header = object
        .split_once("\n\n")
        .map_or(object.as_str(), |(header, _body)| header);
    let mut lines = header.lines();
    let tree = lines
        .next()
        .and_then(|line| line.strip_prefix("tree "))
        .ok_or_else(|| {
            FactorError::GitCommand(non_empty_msg(format!(
                "malformed commit object {commit}: missing tree header"
            )))
        })?;
    TreeHash::new(tree).map_err(|_error| {
        FactorError::GitCommand(non_empty_msg(format!(
            "malformed commit object {commit}: invalid tree header '{tree}'"
        )))
    })?;
    let parents = lines
        .map_while(|line| line.strip_prefix("parent "))
        .map(|parent| {
            CommitSha::new(parent.to_owned()).map_err(|_error| {
                FactorError::GitCommand(non_empty_msg(format!(
                    "malformed commit object {commit}: invalid parent header '{parent}'"
                )))
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    match parents.len() {
        0 => Ok(BaseParent::Root),
        1 => Ok(BaseParent::Commit),
        _ => Err(FactorError::MergeCommit(commit.clone())),
    }
}

/// Removes the empty root commit created during a root-commit factor session.
///
/// After a root-commit factor session completes, the history contains an empty
/// commit at the root. This function rebases `--root --interactive` with a
/// sequence editor that drops the empty commit by SHA.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "root-commit cleanup is extracted for clarity and targeted tests"
    )
)]
pub(in crate::git_factor) fn remove_empty_root_in(ctx: &Ctx<'_>) -> Result<(), FactorError> {
    let roots = git_output(ctx, &["rev-list", "--max-parents=0", "HEAD"])?;
    let root_lines: Vec<&str> = roots
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    let root = match *root_lines.as_slice() {
        [] => {
            return Err(FactorError::GitCommand(non_empty_msg(
                "no root commit found for empty-root cleanup".to_owned(),
            )));
        }
        [root] => root,
        [..] => {
            return Err(FactorError::GitCommand(non_empty_msg(
                "multiple root commits found; empty-root cleanup requires a single-root history"
                    .to_owned(),
            )));
        }
    };

    let root_tree = git_output(ctx, &["ls-tree", root])?;
    if !root_tree.is_empty() {
        return Ok(());
    }

    let short_root = git_output(ctx, &["rev-parse", "--short", root])?;
    let editor = editor_path(ctx)?;
    let Some(editor_str) = editor.to_str() else {
        return Err(FactorError::GitCommand(non_empty_msg(
            "editor path is not valid UTF-8".to_owned(),
        )));
    };
    let seq_editor = format!(
        "{} {} {}",
        shell_quote(editor_str),
        shell_quote("--drop"),
        shell_quote(short_root.as_str())
    );

    let rebase_status = command_status_with(
        ctx,
        "git",
        &[
            "rebase",
            "--empty",
            "drop",
            "--interactive",
            "--no-autosquash",
            "--no-update-refs",
            "--quiet",
            "--root",
        ],
        &[
            ("GIT_EDITOR", "false"),
            ("GIT_SEQUENCE_EDITOR", &seq_editor),
        ],
        false,
    )?;

    if rebase_status.success() {
        return Ok(());
    }

    Err(FactorError::GitCommand(non_empty_msg(format!(
        "rebase to remove empty root failed (exit {})",
        status_code(rebase_status)
    ))))
}

/// Resolves a commit reference to a full SHA.
pub(in crate::git_factor) fn resolve_commit(
    ctx: &Ctx<'_>,
    commit: &str,
) -> Result<CommitSha, FactorError> {
    let sha = git_output(ctx, &["rev-parse", "--verify", commit])
        .map_err(|_git_err| FactorError::InvalidCommit(commit.to_owned()))?;
    CommitSha::new(sha)
}

/// Resolves `HEAD` to a full SHA.
#[cfg(test)]
pub(in crate::git_factor) fn resolve_head_commit(ctx: &Ctx<'_>) -> Result<CommitSha, FactorError> {
    resolve_commit(ctx, "HEAD")
}

/// Resolves commit refs and ranges into a deduplicated, chronologically
/// ordered list of full SHAs.
///
/// Refs containing `..` are expanded via `git rev-list`. Single refs are
/// resolved via `git rev-parse --verify`. The final list is sorted in
/// chronological order (oldest first) to match the order that interactive
/// rebase will stop at each `edit` commit.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "ref resolution is extracted for clarity and targeted tests"
    )
)]
#[cfg(test)]
pub(in crate::git_factor) fn resolve_commit_refs(
    ctx: &Ctx<'_>,
    refs: &NonEmpty<NonEmptyString>,
) -> Result<Commits, FactorError> {
    let mut set = BTreeSet::new();

    for commit_ref in refs {
        let ref_str = commit_ref.as_str();
        if ref_str.contains("...") {
            return Err(FactorError::InvalidCommit(format!(
                "{ref_str} (symmetric diff '...' is not supported, use '..')"
            )));
        }
        if ref_str.contains("..") {
            // Range: expand via git rev-list.
            let output = match git_output(ctx, &["rev-list", ref_str]) {
                Ok(output) => output,
                Err(_err) => return Err(FactorError::InvalidCommit(ref_str.to_owned())),
            };
            for line in output.lines() {
                if let Ok(sha) = CommitSha::new(line.to_owned()) {
                    set.insert(sha);
                }
            }
        } else {
            set.insert(resolve_commit(ctx, commit_ref.as_str())?);
        }
    }

    Commits::try_from(set)
}

/// Resolves CLI commit arguments into one contiguous, oldest-first ancestry
/// span.
///
/// Supported forms are:
/// - `<rev>`
/// - `<start> <end>` (inclusive)
/// - `<start>..<end>` (exclusive start)
/// - `<start>^..<end>` (inclusive start using git-native syntax)
///
/// The resolved commits must form one contiguous, merge-free ancestry path.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "Production start flow uses a dedicated span resolver while tests still exercise the lower-level helpers directly"
    )
)]
pub(in crate::git_factor) fn resolve_commit_span(
    ctx: &Ctx<'_>,
    refs: &NonEmpty<NonEmptyString>,
) -> Result<NonEmpty<CommitSha>, FactorError> {
    let mut tail = refs.tail.iter();
    let commits = if let Some(end_ref) = tail.next() {
        if tail.next().is_some() {
            return Err(FactorError::InvalidCommit(
                "commit arguments must resolve to a single contiguous span".to_owned(),
            ));
        }

        let start = refs.first().as_str();
        let end = end_ref.as_str();
        if start.contains("..") || end.contains("..") {
            return Err(FactorError::InvalidCommit(format!(
                "{start} {end} (use either a single dotted range or two plain commit refs)"
            )));
        }
        resolve_inclusive_span(ctx, start, end)?
    } else {
        let ref_str = refs.first().as_str();
        if ref_str.contains("...") {
            return Err(FactorError::InvalidCommit(format!(
                "{ref_str} (symmetric diff '...' is not supported, use '..')"
            )));
        }
        if ref_str.contains("..") {
            resolve_span_from_range_expr(ctx, ref_str)?
        } else {
            NonEmpty::singleton(resolve_commit(ctx, ref_str)?)
        }
    };

    validate_contiguous_span(ctx, &commits)?;
    Ok(commits)
}

/// Sorts commits topologically (parent before child) to match rebase stop order.
///
/// Uses a single `git rev-list --topo-order --reverse` call with all target
/// SHAs as tips, then filters the output to only the target commits. This
/// ensures the order matches how interactive rebase processes commits.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "topological sorting is extracted for clarity and targeted tests"
    )
)]
#[cfg(test)]
pub(in crate::git_factor) fn sort_topologically(
    ctx: &Ctx<'_>,
    commits: &Commits,
) -> Result<NonEmpty<CommitSha>, FactorError> {
    let target_shas: HashSet<&str> = commits.iter().map(CommitSha::as_str).collect();

    let mut args = vec!["rev-list", "--reverse", "--topo-order"];
    args.extend(commits.iter().map(CommitSha::as_str));

    let sorted: Vec<CommitSha> = git_output(ctx, &args)?
        .lines()
        .filter(|line| target_shas.contains(line))
        .filter_map(|line| CommitSha::new(line.to_owned()).ok())
        .collect();

    let Some(sorted_commits) = NonEmpty::from_vec(sorted) else {
        return Err(FactorError::GitCommand(non_empty_msg(
            "no commits after sorting".to_owned(),
        )));
    };

    Ok(sorted_commits)
}

#[expect(
    clippy::single_call_fn,
    reason = "range-expression expansion stays isolated for testing and future reuse"
)]
/// Resolves one dotted revision range into an oldest-first ancestry span.
fn resolve_span_from_range_expr(
    ctx: &Ctx<'_>,
    range_expr: &str,
) -> Result<NonEmpty<CommitSha>, FactorError> {
    let output = match git_output(
        ctx,
        &["rev-list", "--reverse", "--ancestry-path", range_expr],
    ) {
        Ok(output) => output,
        Err(_err) => return Err(FactorError::InvalidCommit(range_expr.to_owned())),
    };

    parse_non_empty_commit_lines(&output, range_expr)
}

#[expect(
    clippy::single_call_fn,
    reason = "inclusive span expansion stays isolated for testing and future reuse"
)]
/// Resolves two plain refs into one inclusive oldest-first ancestry span.
fn resolve_inclusive_span(
    ctx: &Ctx<'_>,
    start_ref: &str,
    end_ref: &str,
) -> Result<NonEmpty<CommitSha>, FactorError> {
    let start = resolve_commit(ctx, start_ref)?;
    let end = resolve_commit(ctx, end_ref)?;

    if start == end {
        return Ok(NonEmpty::singleton(start));
    }

    let range_expr = format!("{start}..{end}");
    let descendant_output = match git_output(
        ctx,
        &[
            "rev-list",
            "--reverse",
            "--ancestry-path",
            range_expr.as_str(),
        ],
    ) {
        Ok(output) => output,
        Err(_err) => {
            return Err(FactorError::InvalidCommit(format!(
                "{start_ref} {end_ref} (range must resolve to a contiguous ancestry span)"
            )));
        }
    };

    let descendants = parse_commit_lines(&descendant_output);
    if descendants.is_empty() {
        return Err(FactorError::InvalidCommit(format!(
            "{start_ref} {end_ref} (range must resolve to a contiguous ancestry span)"
        )));
    }

    Ok(NonEmpty {
        head: start,
        tail: descendants,
    })
}

#[expect(
    clippy::single_call_fn,
    reason = "line parsing stays extracted for reuse by dotted and inclusive span helpers"
)]
/// Parses one non-empty newline-separated list of commit SHAs.
fn parse_non_empty_commit_lines(
    output: &str,
    input: &str,
) -> Result<NonEmpty<CommitSha>, FactorError> {
    let mut commits = parse_commit_lines(output).into_iter();
    let Some(head) = commits.next() else {
        return Err(FactorError::InvalidCommit(input.to_owned()));
    };

    Ok(NonEmpty {
        head,
        tail: commits.collect(),
    })
}

/// Parses newline-separated commit SHAs, discarding invalid lines.
fn parse_commit_lines(output: &str) -> Vec<CommitSha> {
    output
        .lines()
        .filter_map(|line| CommitSha::new(line.to_owned()).ok())
        .collect()
}

#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "span validation stays isolated while start flow still uses per-target logic"
    )
)]
/// Validates that one resolved span is contiguous and contains no merge commits.
fn validate_contiguous_span(
    ctx: &Ctx<'_>,
    commits: &NonEmpty<CommitSha>,
) -> Result<(), FactorError> {
    for commit in commits {
        validate_not_merge(ctx, commit)?;
    }

    let ordered: Vec<&CommitSha> = commits.iter().collect();
    for &[parent, child] in ordered.array_windows::<2>() {
        let first_parent = match git_output(ctx, &["rev-parse", "--verify", &format!("{child}^")]) {
            Ok(parent_sha) => match CommitSha::new(parent_sha) {
                Ok(parsed_parent_sha) => parsed_parent_sha,
                Err(_err) => return Err(FactorError::InvalidCommit(child.to_string())),
            },
            Err(_err) => {
                return Err(FactorError::InvalidCommit(format!(
                    "{parent} {child} (selected commits must form a contiguous ancestry span)"
                )));
            }
        };

        if first_parent != *parent {
            return Err(FactorError::InvalidCommit(format!(
                "{parent} {child} (selected commits must form a contiguous ancestry span)"
            )));
        }
    }

    Ok(())
}

/// Validates that an exec command has valid bash syntax.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "syntax validation remains extracted for focused tests and error mapping"
    )
)]
pub(in crate::git_factor) fn validate_exec_syntax(
    ctx: &Ctx<'_>,
    command: &str,
) -> Result<(), FactorError> {
    let status = match command_status_with(
        ctx,
        "bash",
        &["--norc", "--noprofile", "-n", "-c", command],
        &[],
        true,
    ) {
        Ok(status) => status,
        Err(err) => {
            return Err(FactorError::GitCommand(non_empty_msg(format!(
                "bash syntax check: {err}"
            ))));
        }
    };

    if status.success() {
        Ok(())
    } else {
        Err(FactorError::InvalidExecSyntax(command.to_owned()))
    }
}

#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "merge validation stays isolated for targeted testing and span checks"
    )
)]
/// Validates that a commit is not a merge commit.
pub(in crate::git_factor) fn validate_not_merge(
    ctx: &Ctx<'_>,
    sha: &CommitSha,
) -> Result<(), FactorError> {
    base_parent_in(ctx, sha).map(|_parent| ())
}

#[cfg(test)]
#[path = "tests/validation/resolve_contract.rs"]
mod resolve_contract;

#[cfg(test)]
#[path = "tests/validation/proptests.rs"]
mod proptests;

#[cfg(test)]
#[path = "tests/validation/tests.rs"]
mod tests;
