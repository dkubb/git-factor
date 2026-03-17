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

/// Removes the empty root commit left by `mixed_reset_to_empty()`.
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

/// Resets HEAD to an empty commit so all files appear as unstaged additions.
///
/// Used for root commits where `git reset --mixed HEAD~1` is not possible.
#[cfg(test)]
pub(in crate::git_factor) fn mixed_reset_to_empty(ctx: &Ctx<'_>) -> Result<(), FactorError> {
    /// The well-known SHA-1 hash of an empty tree object in git.
    const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

    let commit_sha = git_output(ctx, &["commit-tree", EMPTY_TREE, "-m", "empty"])?;
    run_git(ctx, &["reset", "--quiet", &commit_sha])
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

/// Validates that a commit is an ancestor of HEAD.
#[cfg(test)]
pub(in crate::git_factor) fn validate_ancestor(
    ctx: &Ctx<'_>,
    sha: &CommitSha,
) -> Result<(), FactorError> {
    let status = match command_status_with(
        ctx,
        "git",
        &["merge-base", "--is-ancestor", sha.as_str(), "HEAD"],
        &[],
        true,
    ) {
        Ok(status) => status,
        Err(err) => return Err(FactorError::GitCommand(non_empty_msg(err.to_string()))),
    };

    if status.success() {
        Ok(())
    } else {
        Err(FactorError::NotAncestor(sha.clone()))
    }
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
    let has_second_parent = match command_status_with(
        ctx,
        "git",
        &["rev-parse", "--quiet", "--verify", &format!("{sha}^2")],
        &[],
        true,
    ) {
        Ok(status) => status.success(),
        Err(_err) => false,
    };

    if has_second_parent {
        return Err(FactorError::MergeCommit(sha.clone()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::iter;
    use std::env;
    use std::ffi::OsString;
    #[cfg(unix)]
    use std::os::unix::ffi::OsStringExt as _;
    use std::os::unix::process::ExitStatusExt as _;
    use std::process::Command;
    use std::process::Output;
    use tempfile::TempDir;

    const SPAN_START_SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const SPAN_END_SHA: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    struct TestEnv {
        cwd: PathBuf,
    }

    impl Env for TestEnv {
        fn current_dir(&self) -> io::Result<PathBuf> {
            Ok(self.cwd.clone())
        }

        fn current_exe(&self) -> io::Result<PathBuf> {
            env::current_exe()
        }

        fn var_os(&self, _key: &str) -> Option<OsString> {
            None
        }
    }

    struct FailingExeEnv {
        cwd: PathBuf,
    }

    impl Env for FailingExeEnv {
        fn current_dir(&self) -> io::Result<PathBuf> {
            Ok(self.cwd.clone())
        }

        fn current_exe(&self) -> io::Result<PathBuf> {
            Err(io::Error::other("forced current_exe failure"))
        }

        fn var_os(&self, _key: &str) -> Option<OsString> {
            None
        }
    }

    #[cfg(unix)]
    struct NonUtf8Fs;

    #[cfg(unix)]
    impl Fs for NonUtf8Fs {
        fn canonicalize(&self, _path: &Path) -> io::Result<PathBuf> {
            let mut bytes = b"/tmp/".to_vec();
            bytes.push(0xff);
            bytes.extend_from_slice(b"/bin/git-factor");
            Ok(PathBuf::from(OsString::from_vec(bytes)))
        }

        fn create_dir_all(&self, path: &Path) -> io::Result<()> {
            REAL_FS.create_dir_all(path)
        }

        fn exists(&self, path: &Path) -> bool {
            REAL_FS.exists(path)
        }

        fn is_dir(&self, path: &Path) -> bool {
            REAL_FS.is_dir(path)
        }

        fn read_to_string(&self, path: &Path) -> io::Result<String> {
            REAL_FS.read_to_string(path)
        }

        fn remove_dir_all(&self, path: &Path) -> io::Result<()> {
            REAL_FS.remove_dir_all(path)
        }

        fn remove_file(&self, path: &Path) -> io::Result<()> {
            REAL_FS.remove_file(path)
        }

        fn write_string(&self, path: &Path, content: &str) -> io::Result<()> {
            REAL_FS.write_string(path, content)
        }
    }

    #[derive(Copy, Clone, Eq, PartialEq)]
    enum RootFailure {
        CommitTree,
        LsTree,
        RevList,
        ShortRoot,
    }

    struct RootRunner {
        fail_on: Option<RootFailure>,
    }

    impl Runner for RootRunner {
        fn output(&self, _bin: &str, args: &[&str], _cwd: &Path) -> io::Result<Output> {
            let stdout = match *args {
                ["rev-list", "--max-parents=0", "HEAD"] => {
                    if self.fail_on == Some(RootFailure::RevList) {
                        return Err(io::Error::other("forced rev-list failure"));
                    }
                    "a".repeat(COMMIT_SHA_HEX_LEN).into_bytes()
                }
                ["ls-tree", _] => {
                    if self.fail_on == Some(RootFailure::LsTree) {
                        return Err(io::Error::other("forced ls-tree failure"));
                    }
                    Vec::new()
                }
                ["rev-parse", "--short", _] => {
                    if self.fail_on == Some(RootFailure::ShortRoot) {
                        return Err(io::Error::other("forced short-root failure"));
                    }
                    b"aaaaaaa\n".to_vec()
                }
                ["commit-tree", _, "-m", "empty"] => {
                    if self.fail_on == Some(RootFailure::CommitTree) {
                        return Err(io::Error::other("forced commit-tree failure"));
                    }
                    format!("{}\n", "b".repeat(COMMIT_SHA_HEX_LEN)).into_bytes()
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

    struct RebaseArgsRunner;

    impl Runner for RebaseArgsRunner {
        fn output(&self, _bin: &str, args: &[&str], _cwd: &Path) -> io::Result<Output> {
            let stdout = match *args {
                ["rev-list", "--max-parents=0", "HEAD"] => {
                    format!("{}\n", "a".repeat(COMMIT_SHA_HEX_LEN)).into_bytes()
                }
                ["ls-tree", _] => Vec::new(),
                ["rev-parse", "--short", _] => b"aaaaaaa\n".to_vec(),
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
            args: &[&str],
            envs: &[(&str, &str)],
            _quiet: bool,
            _cwd: &Path,
        ) -> io::Result<ExitStatus> {
            if args
                != [
                    "rebase",
                    "--empty",
                    "drop",
                    "--interactive",
                    "--quiet",
                    "--root",
                ]
            {
                return Err(io::Error::other("unexpected status args"));
            }
            if !envs.contains(&("GIT_EDITOR", "false")) {
                return Err(io::Error::other("missing GIT_EDITOR=false env var"));
            }
            if !envs
                .iter()
                .any(|&(key, value)| key == "GIT_SEQUENCE_EDITOR" && value.contains("--drop"))
            {
                return Err(io::Error::other("missing GIT_SEQUENCE_EDITOR with --drop"));
            }

            Ok(ExitStatus::from_raw(0))
        }
    }

    struct RangeLookupRunner;

    impl Runner for RangeLookupRunner {
        fn output(&self, _bin: &str, args: &[&str], _cwd: &Path) -> io::Result<Output> {
            let expected_range = format!("{SPAN_START_SHA}..{SPAN_END_SHA}");
            let (status, stdout, stderr) = if args == ["rev-parse", "--verify", "start"] {
                (
                    ExitStatus::from_raw(0),
                    format!("{SPAN_START_SHA}\n").into_bytes(),
                    Vec::new(),
                )
            } else if args == ["rev-parse", "--verify", "end"] {
                (
                    ExitStatus::from_raw(0),
                    format!("{SPAN_END_SHA}\n").into_bytes(),
                    Vec::new(),
                )
            } else if args
                == [
                    "rev-list",
                    "--reverse",
                    "--ancestry-path",
                    expected_range.as_str(),
                ]
            {
                (
                    ExitStatus::from_raw(256),
                    Vec::new(),
                    b"forced range lookup failure\n".to_vec(),
                )
            } else {
                return Err(io::Error::other(format!(
                    "unexpected output args: {}",
                    args.join(" ")
                )));
            };

            Ok(Output {
                status,
                stdout,
                stderr,
            })
        }

        fn status(
            &self,
            _bin: &str,
            args: &[&str],
            _envs: &[(&str, &str)],
            _quiet: bool,
            _cwd: &Path,
        ) -> io::Result<ExitStatus> {
            let start_merge_ref = format!("{SPAN_START_SHA}^2");
            let end_merge_ref = format!("{SPAN_END_SHA}^2");
            if args == ["rev-parse", "--quiet", "--verify", start_merge_ref.as_str()]
                || args == ["rev-parse", "--quiet", "--verify", end_merge_ref.as_str()]
            {
                Ok(ExitStatus::from_raw(256))
            } else {
                Err(io::Error::other(format!(
                    "unexpected status args: {}",
                    args.join(" ")
                )))
            }
        }
    }

    struct ParentLookupRunner;

    impl Runner for ParentLookupRunner {
        fn output(&self, _bin: &str, args: &[&str], _cwd: &Path) -> io::Result<Output> {
            let end_parent_ref = format!("{SPAN_END_SHA}^");
            if args == ["rev-parse", "--verify", end_parent_ref.as_str()] {
                Ok(Output {
                    status: ExitStatus::from_raw(256),
                    stdout: Vec::new(),
                    stderr: b"forced parent lookup failure\n".to_vec(),
                })
            } else {
                Err(io::Error::other(format!(
                    "unexpected output args: {}",
                    args.join(" ")
                )))
            }
        }

        fn status(
            &self,
            _bin: &str,
            args: &[&str],
            _envs: &[(&str, &str)],
            _quiet: bool,
            _cwd: &Path,
        ) -> io::Result<ExitStatus> {
            let start_merge_ref = format!("{SPAN_START_SHA}^2");
            let end_merge_ref = format!("{SPAN_END_SHA}^2");
            if args == ["rev-parse", "--quiet", "--verify", start_merge_ref.as_str()]
                || args == ["rev-parse", "--quiet", "--verify", end_merge_ref.as_str()]
            {
                Ok(ExitStatus::from_raw(256))
            } else {
                Err(io::Error::other(format!(
                    "unexpected status args: {}",
                    args.join(" ")
                )))
            }
        }
    }

    struct ParentParseRunner;

    impl Runner for ParentParseRunner {
        fn output(&self, _bin: &str, args: &[&str], _cwd: &Path) -> io::Result<Output> {
            let end_parent_ref = format!("{SPAN_END_SHA}^");
            if args == ["rev-parse", "--verify", end_parent_ref.as_str()] {
                Ok(Output {
                    status: ExitStatus::from_raw(0),
                    stdout: b"not-a-commit\n".to_vec(),
                    stderr: Vec::new(),
                })
            } else {
                Err(io::Error::other(format!(
                    "unexpected output args: {}",
                    args.join(" ")
                )))
            }
        }

        fn status(
            &self,
            _bin: &str,
            args: &[&str],
            _envs: &[(&str, &str)],
            _quiet: bool,
            _cwd: &Path,
        ) -> io::Result<ExitStatus> {
            let start_merge_ref = format!("{SPAN_START_SHA}^2");
            let end_merge_ref = format!("{SPAN_END_SHA}^2");
            if args == ["rev-parse", "--quiet", "--verify", start_merge_ref.as_str()]
                || args == ["rev-parse", "--quiet", "--verify", end_merge_ref.as_str()]
            {
                Ok(ExitStatus::from_raw(256))
            } else {
                Err(io::Error::other(format!(
                    "unexpected status args: {}",
                    args.join(" ")
                )))
            }
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

    fn git_command_message(error: &FactorError) -> Option<String> {
        if let &FactorError::GitCommand(_) = error {
            return error
                .to_string()
                .strip_prefix("git command failed: ")
                .map(str::to_owned);
        }
        None
    }

    fn invalid_commit_message(error: &FactorError) -> Option<String> {
        if let &FactorError::InvalidCommit(_) = error {
            return error
                .to_string()
                .strip_prefix("invalid commit: ")
                .map(str::to_owned);
        }
        None
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

    fn init_empty_root_repo(path: &Path) {
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

        let commit = Command::new("git")
            .args([
                "-c",
                "commit.template=",
                "-c",
                "core.hooksPath=.git/hooks",
                "commit",
                "--no-verify",
                "--allow-empty",
                "--quiet",
                "-m",
                "empty root",
            ])
            .current_dir(path)
            .status()
            .or_abort("run git commit --allow-empty");
        assert!(commit.success(), "git commit --allow-empty failed");
    }

    fn head_sha(path: &Path) -> String {
        let output = Command::new("git")
            .args(["rev-parse", "--verify", "HEAD"])
            .current_dir(path)
            .output()
            .or_abort("read HEAD");
        assert!(output.status.success(), "git rev-parse HEAD failed");
        String::from_utf8(output.stdout)
            .or_abort("decode HEAD")
            .trim()
            .to_owned()
    }

    fn commit_file(path: &Path, file: &str, content: &str, message: &str) -> String {
        fs::write(path.join(file), content).or_abort("write file");
        let add = Command::new("git")
            .args(["add", file])
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
                message,
            ])
            .current_dir(path)
            .status()
            .or_abort("run git commit");
        assert!(commit.success(), "git commit failed");

        head_sha(path)
    }

    fn init_linear_repo(path: &Path) -> [String; 3] {
        init_git_repo(path);
        let first = head_sha(path);
        let second = commit_file(path, "two.txt", "two\n", "second");
        let third = commit_file(path, "three.txt", "three\n", "third");
        [first, second, third]
    }

    #[test]
    fn resolve_commit_refs_rejects_symmetric_diff_ranges() {
        let dir = TempDir::new().or_abort("tempdir");
        let ctx = ctx_for(dir.path());
        let commit_ref = NonEmptyString::try_from("HEAD...HEAD".to_owned()).or_abort("non-empty");
        let refs = NonEmpty::singleton(commit_ref);
        let err = resolve_commit_refs(&ctx, &refs).err_or_abort("symmetric diff must be rejected");

        let message = invalid_commit_message(&err).or_abort("expected InvalidCommit");
        assert!(
            message.contains("symmetric diff"),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    fn resolve_commit_refs_rejects_unknown_single_ref() {
        let dir = TempDir::new().or_abort("tempdir");
        init_git_repo(dir.path());
        let ctx = ctx_for(dir.path());
        let commit_ref =
            NonEmptyString::try_from("definitely-not-a-ref".to_owned()).or_abort("non-empty");
        let refs = NonEmpty::singleton(commit_ref);

        let err = resolve_commit_refs(&ctx, &refs).err_or_abort("unknown ref must be rejected");
        let message = invalid_commit_message(&err).or_abort("expected InvalidCommit");
        assert_eq!(message, "definitely-not-a-ref");
    }

    #[test]
    fn resolve_head_commit_maps_git_errors_to_invalid_head() {
        let dir = TempDir::new().or_abort("tempdir");
        let ctx = ctx_for(dir.path());

        let err = resolve_head_commit(&ctx).err_or_abort("missing HEAD should fail");
        let message = invalid_commit_message(&err).or_abort("expected InvalidCommit");
        assert_eq!(message, "HEAD");
    }

    #[test]
    fn resolve_commit_refs_accepts_known_single_ref() {
        let dir = TempDir::new().or_abort("tempdir");
        init_git_repo(dir.path());
        let ctx = ctx_for(dir.path());
        let commit_ref = NonEmptyString::try_from("HEAD".to_owned()).or_abort("non-empty");
        let refs = NonEmpty::singleton(commit_ref);
        let commits = resolve_commit_refs(&ctx, &refs).or_abort("HEAD should resolve");
        let head = resolve_head_commit(&ctx).or_abort("resolve");
        let mut iter = commits.iter();
        assert_eq!(iter.next(), Some(&head));
        assert_eq!(iter.next(), None);
    }

    #[test]
    fn resolve_commit_refs_rejects_unknown_range_ref() {
        let dir = TempDir::new().or_abort("tempdir");
        init_git_repo(dir.path());
        let ctx = ctx_for(dir.path());
        let commit_ref =
            NonEmptyString::try_from("deadbeef..HEAD".to_owned()).or_abort("non-empty");
        let refs = NonEmpty::singleton(commit_ref);

        let err =
            resolve_commit_refs(&ctx, &refs).err_or_abort("unknown range ref must be rejected");
        let message = invalid_commit_message(&err).or_abort("expected InvalidCommit");
        assert_eq!(message, "deadbeef..HEAD");
    }

    #[test]
    fn resolve_commit_span_accepts_known_single_ref() {
        let dir = TempDir::new().or_abort("tempdir");
        init_git_repo(dir.path());
        let ctx = ctx_for(dir.path());
        let refs = NonEmpty::singleton(NonEmptyString::try_from("HEAD".to_owned()).or_abort("ref"));

        let commits = resolve_commit_span(&ctx, &refs).or_abort("HEAD span should resolve");

        assert_eq!(commits.len(), 1);
        assert_eq!(
            commits.first(),
            &resolve_head_commit(&ctx).or_abort("resolve HEAD")
        );
    }

    #[test]
    fn resolve_commit_span_accepts_inclusive_two_ref_span() {
        let dir = TempDir::new().or_abort("tempdir");
        let [first, second, third] = init_linear_repo(dir.path());
        let ctx = ctx_for(dir.path());
        let refs = NonEmpty {
            head: NonEmptyString::try_from(first.clone()).or_abort("start"),
            tail: vec![NonEmptyString::try_from(third.clone()).or_abort("end")],
        };

        let commits = resolve_commit_span(&ctx, &refs).or_abort("inclusive span should resolve");
        let actual: Vec<&str> = commits.iter().map(CommitSha::as_str).collect();

        assert_eq!(
            actual,
            vec![first.as_str(), second.as_str(), third.as_str()]
        );
    }

    #[test]
    fn resolve_commit_span_accepts_identical_two_ref_span() {
        let dir = TempDir::new().or_abort("tempdir");
        let [first, _second, _third] = init_linear_repo(dir.path());
        let ctx = ctx_for(dir.path());
        let refs = NonEmpty {
            head: NonEmptyString::try_from(first.clone()).or_abort("start"),
            tail: vec![NonEmptyString::try_from(first.clone()).or_abort("end")],
        };

        let commits = resolve_commit_span(&ctx, &refs).or_abort("identical refs should resolve");
        let actual: Vec<&str> = commits.iter().map(CommitSha::as_str).collect();

        assert_eq!(actual, vec![first.as_str()]);
    }

    #[test]
    fn resolve_commit_span_accepts_exclusive_dotted_range() {
        let dir = TempDir::new().or_abort("tempdir");
        let [first, second, third] = init_linear_repo(dir.path());
        let ctx = ctx_for(dir.path());
        let refs = NonEmpty::singleton(
            NonEmptyString::try_from(format!("{first}..{third}")).or_abort("range"),
        );

        let commits = resolve_commit_span(&ctx, &refs).or_abort("exclusive span should resolve");
        let actual: Vec<&str> = commits.iter().map(CommitSha::as_str).collect();

        assert_eq!(actual, vec![second.as_str(), third.as_str()]);
    }

    #[test]
    fn resolve_commit_span_accepts_inclusive_git_native_range() {
        let dir = TempDir::new().or_abort("tempdir");
        let [_first, second, third] = init_linear_repo(dir.path());
        let ctx = ctx_for(dir.path());
        let refs = NonEmpty::singleton(
            NonEmptyString::try_from(format!("{second}^..{third}")).or_abort("range"),
        );

        let commits = resolve_commit_span(&ctx, &refs).or_abort("inclusive git range");
        let actual: Vec<&str> = commits.iter().map(CommitSha::as_str).collect();

        assert_eq!(actual, vec![second.as_str(), third.as_str()]);
    }

    #[test]
    fn resolve_commit_span_rejects_more_than_two_commit_args() {
        let dir = TempDir::new().or_abort("tempdir");
        let [first, second, third] = init_linear_repo(dir.path());
        let ctx = ctx_for(dir.path());
        let refs = NonEmpty {
            head: NonEmptyString::try_from(first).or_abort("first"),
            tail: vec![
                NonEmptyString::try_from(second).or_abort("second"),
                NonEmptyString::try_from(third).or_abort("third"),
            ],
        };

        let err = resolve_commit_span(&ctx, &refs).err_or_abort("too many args should be rejected");
        let message = invalid_commit_message(&err).or_abort("expected InvalidCommit");
        assert_eq!(
            message,
            "commit arguments must resolve to a single contiguous span"
        );
    }

    #[test]
    fn resolve_commit_span_rejects_symmetric_diff_range() {
        let dir = TempDir::new().or_abort("tempdir");
        let [first, _second, third] = init_linear_repo(dir.path());
        let ctx = ctx_for(dir.path());
        let refs = NonEmpty::singleton(
            NonEmptyString::try_from(format!("{first}...{third}")).or_abort("range"),
        );

        let err = resolve_commit_span(&ctx, &refs).err_or_abort("symmetric diff should reject");
        let message = invalid_commit_message(&err).or_abort("expected InvalidCommit");
        assert!(
            message.contains("symmetric diff"),
            "unexpected message: {message}"
        );
    }

    #[test]
    fn resolve_commit_span_rejects_dotted_two_arg_form() {
        let dir = TempDir::new().or_abort("tempdir");
        let [first, _second, third] = init_linear_repo(dir.path());
        let ctx = ctx_for(dir.path());
        let refs = NonEmpty {
            head: NonEmptyString::try_from(format!("{first}..{third}")).or_abort("start"),
            tail: vec![NonEmptyString::try_from(third).or_abort("end")],
        };

        let err =
            resolve_commit_span(&ctx, &refs).err_or_abort("mixed dotted/two-arg form should fail");
        let message = invalid_commit_message(&err).or_abort("expected InvalidCommit");
        assert!(
            message.contains("use either a single dotted range or two plain commit refs"),
            "unexpected message: {message}"
        );
    }

    #[test]
    fn resolve_commit_span_rejects_empty_dotted_range() {
        let dir = TempDir::new().or_abort("tempdir");
        let [_first, _second, third] = init_linear_repo(dir.path());
        let ctx = ctx_for(dir.path());
        let refs = NonEmpty::singleton(
            NonEmptyString::try_from(format!("{third}..{third}")).or_abort("range"),
        );

        let err = resolve_commit_span(&ctx, &refs).err_or_abort("empty dotted range should fail");
        let message = invalid_commit_message(&err).or_abort("expected InvalidCommit");
        assert_eq!(message, format!("{third}..{third}"));
    }

    #[test]
    fn resolve_commit_span_rejects_non_ancestry_two_ref_span() {
        let dir = TempDir::new().or_abort("tempdir");
        init_git_repo(dir.path());
        let ctx = ctx_for(dir.path());
        let branch =
            git_output(&ctx, &["rev-parse", "--abbrev-ref", "HEAD"]).or_abort("current branch");
        run_git(&ctx, &["checkout", "--quiet", "-b", "topic"]).or_abort("create topic");
        let topic = commit_file(dir.path(), "topic.txt", "topic\n", "topic");
        run_git(&ctx, &["checkout", "--quiet", branch.as_str()]).or_abort("checkout branch");
        let main = commit_file(dir.path(), "main.txt", "main\n", "main");
        let refs = NonEmpty {
            head: NonEmptyString::try_from(topic).or_abort("topic"),
            tail: vec![NonEmptyString::try_from(main).or_abort("main")],
        };

        let err = resolve_commit_span(&ctx, &refs).err_or_abort("non-ancestry span should reject");
        let message = invalid_commit_message(&err).or_abort("expected InvalidCommit");
        assert!(
            message.contains("contiguous ancestry span"),
            "unexpected message: {message}"
        );
    }

    #[test]
    fn resolve_commit_span_rejects_unknown_start_ref_in_two_ref_span() {
        let dir = TempDir::new().or_abort("tempdir");
        let [_first, _second, third] = init_linear_repo(dir.path());
        let ctx = ctx_for(dir.path());
        let refs = NonEmpty {
            head: NonEmptyString::try_from("deadbeef".to_owned()).or_abort("start"),
            tail: vec![NonEmptyString::try_from(third).or_abort("end")],
        };

        let err = resolve_commit_span(&ctx, &refs).err_or_abort("unknown start ref should reject");
        let message = invalid_commit_message(&err).or_abort("expected InvalidCommit");
        assert_eq!(message, "deadbeef");
    }

    #[test]
    fn resolve_commit_span_rejects_unknown_end_ref_in_two_ref_span() {
        let dir = TempDir::new().or_abort("tempdir");
        let [first, _second, _third] = init_linear_repo(dir.path());
        let ctx = ctx_for(dir.path());
        let refs = NonEmpty {
            head: NonEmptyString::try_from(first).or_abort("start"),
            tail: vec![NonEmptyString::try_from("deadbeef".to_owned()).or_abort("end")],
        };

        let err = resolve_commit_span(&ctx, &refs).err_or_abort("unknown end ref should reject");
        let message = invalid_commit_message(&err).or_abort("expected InvalidCommit");
        assert_eq!(message, "deadbeef");
    }

    #[test]
    fn resolve_commit_span_rejects_merge_commits() {
        let dir = TempDir::new().or_abort("tempdir");
        init_git_repo(dir.path());
        let ctx = ctx_for(dir.path());
        let branch =
            git_output(&ctx, &["rev-parse", "--abbrev-ref", "HEAD"]).or_abort("current branch");
        run_git(&ctx, &["checkout", "--quiet", "-b", "topic"]).or_abort("create topic");
        let _topic = commit_file(dir.path(), "topic.txt", "topic\n", "topic");
        run_git(&ctx, &["checkout", "--quiet", branch.as_str()]).or_abort("checkout branch");
        let _main = commit_file(dir.path(), "main.txt", "main\n", "main");
        run_git(&ctx, &["merge", "--quiet", "--no-ff", "--no-edit", "topic"])
            .or_abort("merge topic");
        let merge = head_sha(dir.path());
        let refs = NonEmpty::singleton(NonEmptyString::try_from(merge).or_abort("merge"));

        let err = resolve_commit_span(&ctx, &refs).err_or_abort("merge commit should reject");
        assert!(
            err.to_string().contains("is a merge commit"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn resolve_commit_span_rejects_two_ref_span_when_rev_list_lookup_fails() {
        let dir = TempDir::new().or_abort("tempdir");
        let runner = RangeLookupRunner;
        let env = TestEnv {
            cwd: dir.path().to_path_buf(),
        };
        let ctx = Ctx {
            runner: &runner,
            cwd: dir.path().to_path_buf(),
            io: &REAL_IO,
            env: &env,
            fs: &REAL_FS,
        };
        let refs = NonEmpty {
            head: NonEmptyString::try_from("start".to_owned()).or_abort("start"),
            tail: vec![NonEmptyString::try_from("end".to_owned()).or_abort("end")],
        };

        let err = resolve_commit_span(&ctx, &refs).err_or_abort("range lookup should fail");

        assert_eq!(
            invalid_commit_message(&err).or_abort("expected InvalidCommit"),
            "start end (range must resolve to a contiguous ancestry span)"
        );
    }

    #[test]
    fn validate_contiguous_span_rejects_skipped_parent() {
        let dir = TempDir::new().or_abort("tempdir");
        let [first, _second, third] = init_linear_repo(dir.path());
        let ctx = ctx_for(dir.path());
        let commits = NonEmpty {
            head: CommitSha::new(first).or_abort("first"),
            tail: vec![CommitSha::new(third).or_abort("third")],
        };

        let err =
            validate_contiguous_span(&ctx, &commits).err_or_abort("skipped parent should reject");
        let message = invalid_commit_message(&err).or_abort("expected InvalidCommit");
        assert!(
            message.contains("contiguous ancestry span"),
            "unexpected message: {message}"
        );
    }

    #[test]
    fn validate_contiguous_span_rejects_missing_parent_lookup() {
        let dir = TempDir::new().or_abort("tempdir");
        let runner = ParentLookupRunner;
        let env = TestEnv {
            cwd: dir.path().to_path_buf(),
        };
        let ctx = Ctx {
            runner: &runner,
            cwd: dir.path().to_path_buf(),
            io: &REAL_IO,
            env: &env,
            fs: &REAL_FS,
        };
        let commits = NonEmpty {
            head: CommitSha::new(SPAN_START_SHA.to_owned()).or_abort("start"),
            tail: vec![CommitSha::new(SPAN_END_SHA.to_owned()).or_abort("end")],
        };

        let err =
            validate_contiguous_span(&ctx, &commits).err_or_abort("parent lookup should fail");

        assert_eq!(
            invalid_commit_message(&err).or_abort("expected InvalidCommit"),
            format!(
                "{SPAN_START_SHA} {SPAN_END_SHA} (selected commits must form a contiguous ancestry span)"
            )
        );
    }

    #[test]
    fn validate_contiguous_span_rejects_invalid_parent_sha_output() {
        let dir = TempDir::new().or_abort("tempdir");
        let runner = ParentParseRunner;
        let env = TestEnv {
            cwd: dir.path().to_path_buf(),
        };
        let ctx = Ctx {
            runner: &runner,
            cwd: dir.path().to_path_buf(),
            io: &REAL_IO,
            env: &env,
            fs: &REAL_FS,
        };
        let commits = NonEmpty {
            head: CommitSha::new(SPAN_START_SHA.to_owned()).or_abort("start"),
            tail: vec![CommitSha::new(SPAN_END_SHA.to_owned()).or_abort("end")],
        };

        let err =
            validate_contiguous_span(&ctx, &commits).err_or_abort("invalid parent sha should fail");

        assert_eq!(
            invalid_commit_message(&err).or_abort("expected InvalidCommit"),
            SPAN_END_SHA
        );
    }

    #[test]
    fn validate_not_merge_rejects_merge_commit() {
        let dir = TempDir::new().or_abort("tempdir");
        init_git_repo(dir.path());
        let ctx = ctx_for(dir.path());

        let branch =
            git_output(&ctx, &["rev-parse", "--abbrev-ref", "HEAD"]).or_abort("current branch");

        run_git(&ctx, &["checkout", "--quiet", "-b", "topic"]).or_abort("create branch");
        fs::write(dir.path().join("topic.txt"), "topic\n").or_abort("write topic file");
        run_git(&ctx, &["add", "topic.txt"]).or_abort("add topic");
        run_git(
            &ctx,
            &[
                "-c",
                "commit.template=",
                "-c",
                "core.hooksPath=.git/hooks",
                "commit",
                "--no-verify",
                "--quiet",
                "-m",
                "topic change",
            ],
        )
        .or_abort("commit topic");

        run_git(&ctx, &["checkout", "--quiet", branch.as_str()]).or_abort("return branch");
        fs::write(dir.path().join("main.txt"), "main\n").or_abort("write main file");
        run_git(&ctx, &["add", "main.txt"]).or_abort("add main");
        run_git(
            &ctx,
            &[
                "-c",
                "commit.template=",
                "-c",
                "core.hooksPath=.git/hooks",
                "commit",
                "--no-verify",
                "--quiet",
                "-m",
                "main change",
            ],
        )
        .or_abort("commit main");
        run_git(&ctx, &["merge", "--quiet", "--no-ff", "--no-edit", "topic"])
            .or_abort("merge topic");

        let merge_sha = CommitSha::new(
            git_output(&ctx, &["rev-parse", "--verify", "HEAD"]).or_abort("merge sha"),
        )
        .or_abort("valid merge sha");
        let topic_sha = CommitSha::new(
            git_output(&ctx, &["rev-parse", "--verify", "topic"]).or_abort("topic sha"),
        )
        .or_abort("valid topic sha");
        validate_not_merge(&ctx, &topic_sha).or_abort("non-merge commit should validate");
        let err =
            validate_not_merge(&ctx, &merge_sha).err_or_abort("merge commit should be rejected");

        assert_eq!(
            err.to_string(),
            format!("commit {merge_sha} is a merge commit and cannot be split")
        );
    }

    #[test]
    fn remove_empty_root_in_reports_git_output_and_editor_path_failures() {
        let dir = TempDir::new().or_abort("tempdir");
        let env = TestEnv {
            cwd: dir.path().to_path_buf(),
        };

        for (failure, expected) in [
            (RootFailure::RevList, "forced rev-list failure"),
            (RootFailure::LsTree, "forced ls-tree failure"),
            (RootFailure::ShortRoot, "forced short-root failure"),
        ] {
            let runner = RootRunner {
                fail_on: Some(failure),
            };
            let ctx = Ctx {
                runner: &runner,
                cwd: dir.path().to_path_buf(),
                io: &REAL_IO,
                env: &env,
                fs: &REAL_FS,
            };
            let err = remove_empty_root_in(&ctx).err_or_abort("remove_empty_root_in should fail");
            let message = git_command_message(&err).or_abort("expected GitCommand");
            assert!(message.contains(expected), "unexpected error: {err:?}");
        }

        let runner = RootRunner { fail_on: None };
        let failing_env = FailingExeEnv {
            cwd: dir.path().to_path_buf(),
        };
        assert_eq!(
            failing_env.current_dir().or_abort("cwd"),
            dir.path().to_path_buf()
        );
        assert!(failing_env.var_os("TRACE").is_none());
        let editor_ctx = Ctx {
            runner: &runner,
            cwd: dir.path().to_path_buf(),
            io: &REAL_IO,
            env: &failing_env,
            fs: &REAL_FS,
        };
        let editor_err =
            remove_empty_root_in(&editor_ctx).err_or_abort("editor path resolution should fail");
        let editor_message = git_command_message(&editor_err).or_abort("expected GitCommand");
        assert!(
            editor_message.contains("forced current_exe failure"),
            "unexpected error: {editor_err:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn remove_empty_root_in_reports_non_utf8_editor_path() {
        let dir = TempDir::new().or_abort("tempdir");
        let runner = RootRunner { fail_on: None };
        let env = TestEnv {
            cwd: dir.path().to_path_buf(),
        };
        let fs = NonUtf8Fs;
        let ctx = Ctx {
            runner: &runner,
            cwd: dir.path().to_path_buf(),
            io: &REAL_IO,
            env: &env,
            fs: &fs,
        };

        let err =
            remove_empty_root_in(&ctx).err_or_abort("non-utf8 editor path should fail cleanup");
        let message = git_command_message(&err).or_abort("expected GitCommand");
        assert_eq!(message, "editor path is not valid UTF-8");
    }

    #[test]
    fn root_runner_helpers_cover_unexpected_and_status_paths() {
        let runner = RootRunner { fail_on: None };
        let status = runner
            .status("git", &["status"], &[], false, Path::new("."))
            .or_abort("status");
        assert!(status.success());
        let unexpected = runner
            .output("git", &["status"], Path::new("."))
            .err_or_abort("unexpected args should fail");
        assert!(
            unexpected.to_string().contains("unexpected args"),
            "unexpected error: {unexpected:?}"
        );
    }

    #[test]
    fn range_lookup_runner_covers_expected_and_unexpected_paths() {
        let range_runner = RangeLookupRunner;
        let start_output = range_runner
            .output("git", &["rev-parse", "--verify", "start"], Path::new("."))
            .or_abort("resolve start ref");
        assert!(start_output.status.success());
        assert_eq!(
            start_output.stdout,
            format!("{SPAN_START_SHA}\n").into_bytes()
        );
        assert!(start_output.stderr.is_empty());

        let end_output = range_runner
            .output("git", &["rev-parse", "--verify", "end"], Path::new("."))
            .or_abort("resolve end ref");
        assert!(end_output.status.success());
        assert_eq!(end_output.stdout, format!("{SPAN_END_SHA}\n").into_bytes());
        assert!(end_output.stderr.is_empty());

        let range_output = range_runner
            .output(
                "git",
                &[
                    "rev-list",
                    "--reverse",
                    "--ancestry-path",
                    &format!("{SPAN_START_SHA}..{SPAN_END_SHA}"),
                ],
                Path::new("."),
            )
            .or_abort("range lookup output");
        assert!(!range_output.status.success());
        assert!(range_output.stdout.is_empty());
        assert_eq!(range_output.stderr, b"forced range lookup failure\n");

        let start_merge = range_runner
            .status(
                "git",
                &[
                    "rev-parse",
                    "--quiet",
                    "--verify",
                    &format!("{SPAN_START_SHA}^2"),
                ],
                &[],
                false,
                Path::new("."),
            )
            .or_abort("start merge check");
        assert!(!start_merge.success());

        let end_merge = range_runner
            .status(
                "git",
                &[
                    "rev-parse",
                    "--quiet",
                    "--verify",
                    &format!("{SPAN_END_SHA}^2"),
                ],
                &[],
                false,
                Path::new("."),
            )
            .or_abort("end merge check");
        assert!(!end_merge.success());

        let unexpected_range_output = range_runner
            .output("git", &["status"], Path::new("."))
            .err_or_abort("unexpected range output args should fail");
        assert!(
            unexpected_range_output
                .to_string()
                .contains("unexpected output args"),
            "unexpected error: {unexpected_range_output:?}"
        );

        let unexpected_range_status = range_runner
            .status("git", &["status"], &[], false, Path::new("."))
            .err_or_abort("unexpected range status args should fail");
        assert!(
            unexpected_range_status
                .to_string()
                .contains("unexpected status args"),
            "unexpected error: {unexpected_range_status:?}"
        );
    }

    #[test]
    fn parent_lookup_runner_covers_expected_and_unexpected_paths() {
        let parent_lookup_runner = ParentLookupRunner;
        let missing_parent_output = parent_lookup_runner
            .output(
                "git",
                &["rev-parse", "--verify", &format!("{SPAN_END_SHA}^")],
                Path::new("."),
            )
            .or_abort("missing parent output");
        assert!(!missing_parent_output.status.success());
        assert!(missing_parent_output.stdout.is_empty());
        assert_eq!(
            missing_parent_output.stderr,
            b"forced parent lookup failure\n"
        );

        let lookup_start_merge = parent_lookup_runner
            .status(
                "git",
                &[
                    "rev-parse",
                    "--quiet",
                    "--verify",
                    &format!("{SPAN_START_SHA}^2"),
                ],
                &[],
                false,
                Path::new("."),
            )
            .or_abort("lookup start merge check");
        assert!(!lookup_start_merge.success());

        let lookup_end_merge = parent_lookup_runner
            .status(
                "git",
                &[
                    "rev-parse",
                    "--quiet",
                    "--verify",
                    &format!("{SPAN_END_SHA}^2"),
                ],
                &[],
                false,
                Path::new("."),
            )
            .or_abort("lookup end merge check");
        assert!(!lookup_end_merge.success());

        let unexpected_lookup_output = parent_lookup_runner
            .output("git", &["status"], Path::new("."))
            .err_or_abort("unexpected lookup output args should fail");
        assert!(
            unexpected_lookup_output
                .to_string()
                .contains("unexpected output args"),
            "unexpected error: {unexpected_lookup_output:?}"
        );

        let unexpected_lookup_status = parent_lookup_runner
            .status("git", &["status"], &[], false, Path::new("."))
            .err_or_abort("unexpected lookup status args should fail");
        assert!(
            unexpected_lookup_status
                .to_string()
                .contains("unexpected status args"),
            "unexpected error: {unexpected_lookup_status:?}"
        );
    }

    #[test]
    fn parent_parse_runner_covers_expected_and_unexpected_paths() {
        let parent_parse_runner = ParentParseRunner;
        let invalid_parent_output = parent_parse_runner
            .output(
                "git",
                &["rev-parse", "--verify", &format!("{SPAN_END_SHA}^")],
                Path::new("."),
            )
            .or_abort("invalid parent output");
        assert!(invalid_parent_output.status.success());
        assert_eq!(invalid_parent_output.stdout, b"not-a-commit\n");
        assert!(invalid_parent_output.stderr.is_empty());

        let parse_start_merge = parent_parse_runner
            .status(
                "git",
                &[
                    "rev-parse",
                    "--quiet",
                    "--verify",
                    &format!("{SPAN_START_SHA}^2"),
                ],
                &[],
                false,
                Path::new("."),
            )
            .or_abort("parse start merge check");
        assert!(!parse_start_merge.success());

        let parse_end_merge = parent_parse_runner
            .status(
                "git",
                &[
                    "rev-parse",
                    "--quiet",
                    "--verify",
                    &format!("{SPAN_END_SHA}^2"),
                ],
                &[],
                false,
                Path::new("."),
            )
            .or_abort("parse end merge check");
        assert!(!parse_end_merge.success());

        let unexpected_parse_output = parent_parse_runner
            .output("git", &["status"], Path::new("."))
            .err_or_abort("unexpected parse output args should fail");
        assert!(
            unexpected_parse_output
                .to_string()
                .contains("unexpected output args"),
            "unexpected error: {unexpected_parse_output:?}"
        );

        let unexpected_parse_status = parent_parse_runner
            .status("git", &["status"], &[], false, Path::new("."))
            .err_or_abort("unexpected parse status args should fail");
        assert!(
            unexpected_parse_status
                .to_string()
                .contains("unexpected status args"),
            "unexpected error: {unexpected_parse_status:?}"
        );
    }

    #[test]
    fn env_and_fs_helpers_cover_delegated_paths() {
        let dir = TempDir::new().or_abort("tempdir");
        let env = TestEnv {
            cwd: dir.path().to_path_buf(),
        };
        assert_eq!(env.current_dir().or_abort("test env cwd"), dir.path());
        assert!(env.var_os("OTHER_ENV").is_none());

        let failing_env = FailingExeEnv {
            cwd: dir.path().to_path_buf(),
        };
        assert_eq!(
            failing_env.current_dir().or_abort("failing env cwd"),
            dir.path()
        );
        assert!(failing_env.var_os("OTHER_ENV").is_none());

        #[cfg(unix)]
        {
            let fs = NonUtf8Fs;
            let mkdir = dir.path().join("mkdir");
            fs.create_dir_all(&mkdir).or_abort("mkdir");
            assert!(fs.is_dir(&mkdir));
            assert!(fs.exists(&mkdir));

            let file = dir.path().join("file.txt");
            fs.write_string(&file, "hello").or_abort("write file");
            let content = fs.read_to_string(&file).or_abort("read file");
            assert_eq!(content, "hello");
            fs.remove_file(&file).or_abort("remove file");
            assert!(!fs.exists(&file));

            let rm_dir = dir.path().join("rm-dir");
            fs.create_dir_all(&rm_dir).or_abort("create rm dir");
            fs.remove_dir_all(&rm_dir).or_abort("remove rm dir");
            assert!(!fs.exists(&rm_dir));
        }
    }

    #[test]
    fn error_extractors_cover_non_matching_variants() {
        let git_command = FactorError::GitCommand(non_empty_msg("boom".to_owned()));
        let invalid_commit = FactorError::InvalidCommit("deadbeef".to_owned());

        assert_eq!(git_command_message(&git_command), Some("boom".to_owned()));
        assert_eq!(git_command_message(&invalid_commit), None);
        assert_eq!(
            invalid_commit_message(&invalid_commit),
            Some("deadbeef".to_owned())
        );
        assert_eq!(invalid_commit_message(&git_command), None);
    }

    #[test]
    fn mixed_reset_to_empty_with_root_runner_uses_commit_tree_output() {
        let dir = TempDir::new().or_abort("tempdir");
        let runner = RootRunner { fail_on: None };
        let env = TestEnv {
            cwd: dir.path().to_path_buf(),
        };
        let ctx = Ctx {
            runner: &runner,
            cwd: dir.path().to_path_buf(),
            io: &REAL_IO,
            env: &env,
            fs: &REAL_FS,
        };
        mixed_reset_to_empty(&ctx).or_abort("reset should succeed with scripted commit tree");
    }

    #[test]
    fn mixed_reset_to_empty_reports_commit_tree_failure() {
        let dir = TempDir::new().or_abort("tempdir");
        let runner = RootRunner {
            fail_on: Some(RootFailure::CommitTree),
        };
        let env = TestEnv {
            cwd: dir.path().to_path_buf(),
        };
        let ctx = Ctx {
            runner: &runner,
            cwd: dir.path().to_path_buf(),
            io: &REAL_IO,
            env: &env,
            fs: &REAL_FS,
        };

        let err = mixed_reset_to_empty(&ctx).err_or_abort("commit-tree failure should be returned");
        let message = git_command_message(&err).or_abort("expected GitCommand");
        assert!(
            message.contains("forced commit-tree failure"),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    fn sort_topologically_orders_commits_from_oldest_to_newest() {
        let dir = TempDir::new().or_abort("tempdir");
        init_git_repo(dir.path());
        let ctx = ctx_for(dir.path());

        fs::write(dir.path().join("a.txt"), "a\n").or_abort("write a");
        run_git(&ctx, &["add", "a.txt"]).or_abort("add a");
        run_git(
            &ctx,
            &[
                "-c",
                "commit.template=",
                "-c",
                "core.hooksPath=.git/hooks",
                "commit",
                "--no-verify",
                "--quiet",
                "-m",
                "a",
            ],
        )
        .or_abort("commit a");
        let first_commit =
            CommitSha::new(git_output(&ctx, &["rev-parse", "--verify", "HEAD"]).or_abort("sha a"))
                .or_abort("valid sha a");

        fs::write(dir.path().join("b.txt"), "b\n").or_abort("write b");
        run_git(&ctx, &["add", "b.txt"]).or_abort("add b");
        run_git(
            &ctx,
            &[
                "-c",
                "commit.template=",
                "-c",
                "core.hooksPath=.git/hooks",
                "commit",
                "--no-verify",
                "--quiet",
                "-m",
                "b",
            ],
        )
        .or_abort("commit b");
        let second_commit =
            CommitSha::new(git_output(&ctx, &["rev-parse", "--verify", "HEAD"]).or_abort("sha b"))
                .or_abort("valid sha b");

        let commits = Commits::try_from(BTreeSet::from([
            first_commit.clone(),
            second_commit.clone(),
        ]))
        .or_abort("non-empty set");
        let sorted = sort_topologically(&ctx, &commits).or_abort("sort should succeed");
        let sorted_vec: Vec<_> = sorted.into_iter().collect();
        assert_eq!(sorted_vec, vec![first_commit, second_commit]);
    }

    #[test]
    fn sort_topologically_reports_git_output_failure_for_unknown_commits() {
        let dir = TempDir::new().or_abort("tempdir");
        init_git_repo(dir.path());
        let ctx = ctx_for(dir.path());

        let fake = CommitSha::new("f".repeat(COMMIT_SHA_HEX_LEN)).or_abort("valid sha");
        let commits = Commits::try_from(BTreeSet::from([fake])).or_abort("non-empty set");
        let err = sort_topologically(&ctx, &commits).err_or_abort("git rev-list should fail");
        assert!(
            git_command_message(&err)
                .or_abort("git command errors should expose a message")
                .contains("bad object"),
            "error was: {err:?}"
        );
    }

    #[test]
    fn sort_topologically_reports_empty_sorted_output() {
        struct EmptySortRunner;

        impl Runner for EmptySortRunner {
            fn output(&self, _bin: &str, args: &[&str], _cwd: &Path) -> io::Result<Output> {
                if !args.starts_with(&["rev-list", "--reverse", "--topo-order"]) {
                    return Err(io::Error::other("unexpected args"));
                }
                Ok(Output {
                    status: ExitStatus::from_raw(0),
                    stdout: b"\n".to_vec(),
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

        let dir = TempDir::new().or_abort("tempdir");
        let commit = CommitSha::new("a".repeat(COMMIT_SHA_HEX_LEN)).or_abort("valid sha");
        let commits = Commits::try_from(iter::once(commit).collect::<BTreeSet<_>>())
            .or_abort("non-empty commit set");
        let runner = EmptySortRunner;
        let env = TestEnv {
            cwd: dir.path().to_path_buf(),
        };
        let ctx = Ctx {
            runner: &runner,
            cwd: dir.path().to_path_buf(),
            io: &REAL_IO,
            env: &env,
            fs: &REAL_FS,
        };
        let status = runner
            .status("git", &["status"], &[], false, dir.path())
            .or_abort("status should succeed");
        assert_eq!(status.code(), Some(i32::default()));
        let unexpected = runner
            .output("git", &["unexpected"], dir.path())
            .err_or_abort("unexpected args should fail");
        assert_eq!(unexpected.to_string(), "unexpected args");

        let err =
            sort_topologically(&ctx, &commits).err_or_abort("empty sorted output should fail");
        let message = git_command_message(&err).or_abort("expected GitCommand");
        assert!(
            message.contains("no commits after sorting"),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    fn validate_ancestor_wraps_command_status_io_error() {
        struct AncestorIoErrorRunner;

        impl Runner for AncestorIoErrorRunner {
            fn output(&self, _bin: &str, _args: &[&str], _cwd: &Path) -> io::Result<Output> {
                Err(io::Error::other("output should not be called"))
            }

            fn status(
                &self,
                _bin: &str,
                _args: &[&str],
                _envs: &[(&str, &str)],
                _quiet: bool,
                _cwd: &Path,
            ) -> io::Result<ExitStatus> {
                Err(io::Error::other("forced merge-base status io failure"))
            }
        }

        let dir = TempDir::new().or_abort("tempdir");
        let runner = AncestorIoErrorRunner;
        let env = TestEnv {
            cwd: dir.path().to_path_buf(),
        };
        let ctx = Ctx {
            runner: &runner,
            cwd: dir.path().to_path_buf(),
            io: &REAL_IO,
            env: &env,
            fs: &REAL_FS,
        };
        let sha = CommitSha::new("a".repeat(COMMIT_SHA_HEX_LEN)).or_abort("valid sha");
        let output_err = runner
            .output("git", &["status"], dir.path())
            .err_or_abort("output method should fail");
        assert!(
            output_err
                .to_string()
                .contains("output should not be called"),
            "unexpected output error: {output_err}"
        );

        let err = validate_ancestor(&ctx, &sha).err_or_abort("status io error should fail");
        let message = git_command_message(&err).or_abort("expected GitCommand");
        assert!(
            message.contains("forced merge-base status io failure"),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    fn validate_ancestor_returns_ok_for_head_commit() {
        let dir = TempDir::new().or_abort("tempdir");
        init_git_repo(dir.path());
        let ctx = ctx_for(dir.path());
        let head = resolve_head_commit(&ctx).or_abort("resolve");
        validate_ancestor(&ctx, &head).or_abort("HEAD should be ancestor of HEAD");
    }

    #[test]
    fn validate_exec_syntax_wraps_command_status_io_error() {
        struct ExecSyntaxIoErrorRunner;

        impl Runner for ExecSyntaxIoErrorRunner {
            fn output(&self, _bin: &str, _args: &[&str], _cwd: &Path) -> io::Result<Output> {
                Err(io::Error::other("output should not be called"))
            }

            fn status(
                &self,
                _bin: &str,
                _args: &[&str],
                _envs: &[(&str, &str)],
                _quiet: bool,
                _cwd: &Path,
            ) -> io::Result<ExitStatus> {
                Err(io::Error::other("forced bash syntax status io failure"))
            }
        }

        let dir = TempDir::new().or_abort("tempdir");
        let runner = ExecSyntaxIoErrorRunner;
        let env = TestEnv {
            cwd: dir.path().to_path_buf(),
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

        let err = validate_exec_syntax(&ctx, "echo ok").err_or_abort("status io error should fail");
        let message = git_command_message(&err).or_abort("expected GitCommand");
        assert!(
            message.contains("forced bash syntax status io failure"),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    fn validate_exec_syntax_returns_ok_for_valid_shell_command() {
        let dir = TempDir::new().or_abort("tempdir");
        let ctx = ctx_for(dir.path());
        validate_exec_syntax(&ctx, "echo ok")
            .or_abort("valid shell command should pass syntax check");
    }

    #[test]
    fn remove_empty_root_in_returns_early_when_root_has_content() {
        let dir = TempDir::new().or_abort("tempdir");
        init_git_repo(dir.path());
        let ctx = ctx_for(dir.path());

        remove_empty_root_in(&ctx).or_abort("remove_empty_root_in should return early");
    }

    #[test]
    fn remove_empty_root_in_reports_rebase_failure_for_empty_root() {
        let dir = TempDir::new().or_abort("tempdir");
        init_empty_root_repo(dir.path());
        let ctx = ctx_for(dir.path());

        let err =
            remove_empty_root_in(&ctx).err_or_abort("rebase should fail without sequence editor");
        let message = git_command_message(&err).or_abort("expected GitCommand");
        assert!(
            message.contains("rebase to remove empty root failed"),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    fn remove_empty_root_in_reports_missing_root_commit() {
        struct NoRootRunner;

        impl Runner for NoRootRunner {
            fn output(&self, _bin: &str, args: &[&str], _cwd: &Path) -> io::Result<Output> {
                if args != ["rev-list", "--max-parents=0", "HEAD"] {
                    return Err(io::Error::other("unexpected args"));
                }
                Ok(Output {
                    status: ExitStatus::from_raw(0),
                    stdout: b"\n".to_vec(),
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

        let dir = TempDir::new().or_abort("tempdir");
        let env = TestEnv {
            cwd: dir.path().to_path_buf(),
        };
        let runner = NoRootRunner;
        let status = runner
            .status("git", &["status"], &[], false, dir.path())
            .or_abort("status should succeed");
        assert_eq!(status.code(), Some(i32::default()));
        let output = runner
            .output("git", &["rev-list", "--max-parents=0", "HEAD"], dir.path())
            .or_abort("output should succeed");
        assert!(output.stderr.is_empty(), "stderr should be empty");
        let output_err = runner
            .output("git", &["unexpected"], dir.path())
            .err_or_abort("unexpected args should fail");
        assert_eq!(output_err.to_string(), "unexpected args");
        let ctx = Ctx {
            runner: &runner,
            cwd: dir.path().to_path_buf(),
            io: &REAL_IO,
            env: &env,
            fs: &REAL_FS,
        };

        let err = remove_empty_root_in(&ctx).err_or_abort("missing root should fail");
        let message = git_command_message(&err).or_abort("expected GitCommand");
        assert!(
            message.contains("no root commit found"),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    fn remove_empty_root_in_propagates_rebase_status_io_error() {
        struct RebaseStatusIoErrorRunner;

        impl Runner for RebaseStatusIoErrorRunner {
            fn output(&self, _bin: &str, args: &[&str], _cwd: &Path) -> io::Result<Output> {
                let stdout = match *args {
                    ["rev-list", "--max-parents=0", "HEAD"] => {
                        format!("{}\n", "a".repeat(COMMIT_SHA_HEX_LEN)).into_bytes()
                    }
                    ["ls-tree", _] => Vec::new(),
                    ["rev-parse", "--short", _] => b"aaaaaaa\n".to_vec(),
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
                args: &[&str],
                _envs: &[(&str, &str)],
                _quiet: bool,
                _cwd: &Path,
            ) -> io::Result<ExitStatus> {
                if args
                    != [
                        "rebase",
                        "--empty",
                        "drop",
                        "--interactive",
                        "--quiet",
                        "--root",
                    ]
                {
                    return Err(io::Error::other("unexpected status args"));
                }
                Err(io::Error::other("rebase status io fail"))
            }
        }

        let dir = TempDir::new().or_abort("tempdir");
        fs::write(dir.path().join("git-factor"), "").or_abort("create git-factor");
        let env = TestEnv {
            cwd: dir.path().to_path_buf(),
        };
        let runner = RebaseStatusIoErrorRunner;
        let unexpected_output_err = runner
            .output("git", &["unexpected"], dir.path())
            .err_or_abort("unexpected args should fail");
        assert!(
            unexpected_output_err
                .to_string()
                .contains("unexpected args"),
            "unexpected error: {unexpected_output_err}"
        );
        let status_err = runner
            .status("git", &["status"], &[], false, dir.path())
            .err_or_abort("unexpected status args should fail");
        assert_eq!(status_err.to_string(), "unexpected status args");
        let ctx = Ctx {
            runner: &runner,
            cwd: dir.path().to_path_buf(),
            io: &REAL_IO,
            env: &env,
            fs: &REAL_FS,
        };

        let err =
            remove_empty_root_in(&ctx).err_or_abort("rebase status io failure should propagate");
        let message = git_command_message(&err).or_abort("expected GitCommand");
        assert!(
            message.contains("rebase status io fail"),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    fn remove_empty_root_in_rejects_multiple_root_commits() {
        struct MultiRootRunner;

        impl Runner for MultiRootRunner {
            fn output(&self, _bin: &str, args: &[&str], _cwd: &Path) -> io::Result<Output> {
                if args != ["rev-list", "--max-parents=0", "HEAD"] {
                    return Err(io::Error::other("unexpected args"));
                }
                let stdout = format!(
                    "{}\n{}\n",
                    "a".repeat(COMMIT_SHA_HEX_LEN),
                    "b".repeat(COMMIT_SHA_HEX_LEN)
                )
                .into_bytes();
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

        let dir = TempDir::new().or_abort("tempdir");
        let env = TestEnv {
            cwd: dir.path().to_path_buf(),
        };
        let runner = MultiRootRunner;
        let status = runner
            .status("git", &["status"], &[], false, dir.path())
            .or_abort("status should succeed");
        assert_eq!(status.code(), Some(i32::default()));
        let output_err = runner
            .output("git", &["unexpected"], dir.path())
            .err_or_abort("unexpected args should fail");
        assert_eq!(output_err.to_string(), "unexpected args");
        let ctx = Ctx {
            runner: &runner,
            cwd: dir.path().to_path_buf(),
            io: &REAL_IO,
            env: &env,
            fs: &REAL_FS,
        };

        let err = remove_empty_root_in(&ctx).err_or_abort("multiple roots should fail");
        let message = git_command_message(&err).or_abort("expected GitCommand");
        assert!(
            message.contains("multiple root commits found"),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    fn remove_empty_root_in_passes_empty_drop_to_rebase() {
        let dir = TempDir::new().or_abort("tempdir");
        let env = TestEnv {
            cwd: dir.path().to_path_buf(),
        };
        let runner = RebaseArgsRunner;
        let unexpected = runner
            .output("git", &["unexpected"], dir.path())
            .err_or_abort("unexpected args should error");
        assert!(
            unexpected.to_string().contains("unexpected args"),
            "err was: {unexpected}"
        );
        let expected_rebase_args = [
            "rebase",
            "--empty",
            "drop",
            "--interactive",
            "--quiet",
            "--root",
        ];
        let unexpected_status_args_err = runner
            .status("git", &["status"], &[], false, dir.path())
            .err_or_abort("unexpected status args should fail");
        assert_eq!(
            unexpected_status_args_err.to_string(),
            "unexpected status args"
        );
        let missing_editor_env_err = runner
            .status(
                "git",
                &expected_rebase_args,
                &[("GIT_SEQUENCE_EDITOR", "git-factor --drop")],
                false,
                dir.path(),
            )
            .err_or_abort("missing GIT_EDITOR should fail");
        assert_eq!(
            missing_editor_env_err.to_string(),
            "missing GIT_EDITOR=false env var"
        );
        let missing_sequence_editor_env_err = runner
            .status(
                "git",
                &expected_rebase_args,
                &[("GIT_EDITOR", "false")],
                false,
                dir.path(),
            )
            .err_or_abort("missing GIT_SEQUENCE_EDITOR should fail");
        assert_eq!(
            missing_sequence_editor_env_err.to_string(),
            "missing GIT_SEQUENCE_EDITOR with --drop"
        );

        let ctx = Ctx {
            runner: &runner,
            cwd: dir.path().to_path_buf(),
            io: &REAL_IO,
            env: &env,
            fs: &REAL_FS,
        };

        remove_empty_root_in(&ctx)
            .or_abort("remove_empty_root_in should pass expected rebase args");
    }

    #[test]
    fn init_empty_root_repo_creates_single_empty_root_commit() {
        let dir = TempDir::new().or_abort("tempdir");
        init_empty_root_repo(dir.path());
        let ctx = ctx_for(dir.path());

        let roots = git_output(&ctx, &["rev-list", "--max-parents=0", "HEAD"])
            .or_abort("rev-list should succeed");
        let root_count = roots.lines().filter(|line| !line.trim().is_empty()).count();
        assert_eq!(root_count, 1);

        let root_sha = roots
            .lines()
            .find(|line| !line.trim().is_empty())
            .or_abort("single root sha should exist");
        let tree = git_output(&ctx, &["ls-tree", root_sha]).or_abort("ls-tree should succeed");
        assert!(tree.is_empty(), "root commit should be empty");
    }

    #[test]
    fn mixed_reset_to_empty_resets_index_to_empty_tree() {
        const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

        let dir = TempDir::new().or_abort("tempdir");
        init_git_repo(dir.path());
        let ctx = ctx_for(dir.path());

        mixed_reset_to_empty(&ctx).or_abort("mixed_reset_to_empty should succeed");
        let tree = git_output(&ctx, &["write-tree"]).or_abort("write-tree");
        assert_eq!(tree, EMPTY_TREE);
    }

    #[test]
    fn proptest_run_non_panicking_unit_suite() {
        env_and_fs_helpers_cover_delegated_paths();
        error_extractors_cover_non_matching_variants();
        init_empty_root_repo_creates_single_empty_root_commit();
        mixed_reset_to_empty_reports_commit_tree_failure();
        mixed_reset_to_empty_resets_index_to_empty_tree();
        mixed_reset_to_empty_with_root_runner_uses_commit_tree_output();
        remove_empty_root_in_passes_empty_drop_to_rebase();
        #[cfg(unix)]
        remove_empty_root_in_reports_non_utf8_editor_path();
        remove_empty_root_in_propagates_rebase_status_io_error();
        remove_empty_root_in_rejects_multiple_root_commits();
        remove_empty_root_in_reports_git_output_and_editor_path_failures();
        remove_empty_root_in_reports_missing_root_commit();
        remove_empty_root_in_reports_rebase_failure_for_empty_root();
        remove_empty_root_in_returns_early_when_root_has_content();
        resolve_commit_refs_accepts_known_single_ref();
        resolve_commit_refs_rejects_symmetric_diff_ranges();
        resolve_commit_refs_rejects_unknown_range_ref();
        resolve_commit_refs_rejects_unknown_single_ref();
        resolve_commit_span_rejects_unknown_start_ref_in_two_ref_span();
        resolve_commit_span_rejects_unknown_end_ref_in_two_ref_span();
        resolve_head_commit_maps_git_errors_to_invalid_head();
        root_runner_helpers_cover_unexpected_and_status_paths();
        sort_topologically_orders_commits_from_oldest_to_newest();
        sort_topologically_reports_empty_sorted_output();
        sort_topologically_reports_git_output_failure_for_unknown_commits();
        validate_ancestor_returns_ok_for_head_commit();
        validate_ancestor_wraps_command_status_io_error();
        validate_exec_syntax_returns_ok_for_valid_shell_command();
        validate_exec_syntax_wraps_command_status_io_error();
        validate_not_merge_rejects_merge_commit();
    }
}
