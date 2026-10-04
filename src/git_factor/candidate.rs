//! Candidate validation isolated from the selection worktree.

use alloc::collections::BTreeSet;

use core::str;

use serde::{Deserialize, Serialize};

use std::ffi::OsStr;

use std::io;

use std::os::unix::{ffi::OsStrExt as _, fs::MetadataExt as _};

use std::path::{Component, Path, PathBuf};

use std::process::{ExitStatus, Output};

use super::gate::GateSet;

use super::types::{CommitSha, SessionId};

use super::{
    Ctx, FactorError, NonEmpty, NonEmptyString, Runner, StateDir, TreeHash, command_output_with,
    command_status_with, ensure_repo_clean, git_output, git_raw_output, non_empty_msg, output_text,
    run_git, status_code,
};

use tempfile::TempDir;

/// Durable ownership of exactly one disposable native worktree.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ActorOwner {
    /// Exact canonical actor worktree path belonging to this session.
    actor: PathBuf,
    /// Native common directory binding the actor to its repository.
    common: PathBuf,
    /// Session identity admitted before the actor registration is created.
    session: SessionId,
}

/// Lets actor subprocesses discover their admitted Git worktree from cwd.
struct ActorRunner<'runner> {
    /// Original runner preserves tracing and process-failure handling.
    inner: &'runner dyn Runner,
}

/// Main worktree mutation footprint of the validation's eventual promotion.
#[derive(Clone, Copy)]
enum CandidatePurpose {
    /// Metadata promotion changes only HEAD and preserves the observed tree.
    Observed,
    /// Selection promotion will replace the selected source worktree.
    Selection,
}

/// Source facts and requested native message for one isolated validation.
#[derive(Clone, Copy)]
pub(in crate::git_factor) struct Candidate<'source> {
    /// Nonempty requested message paragraphs.
    messages: &'source NonEmpty<NonEmptyString>,
    /// Combined source supplying the normalized author name, email, and date.
    original: &'source CommitSha,
    /// Actual accepted parent; absent for a parentless root atom.
    parent: Option<&'source CommitSha>,
    /// Admission matches the consumer's eventual main-worktree mutation.
    purpose: CandidatePurpose,
    /// Selected tree that hooks and gates may observe but never change.
    tree: &'source TreeHash,
}

impl Runner for ActorRunner<'_> {
    fn output(
        &self,
        bin: &str,
        args: &[&str],
        envs: &[(&str, Option<&str>)],
        cwd: &Path,
    ) -> io::Result<Output> {
        self.inner.output(bin, args, &Self::environment(envs), cwd)
    }

    fn status(
        &self,
        bin: &str,
        args: &[&str],
        envs: &[(&str, Option<&str>)],
        quiet: bool,
        cwd: &Path,
    ) -> io::Result<ExitStatus> {
        self.inner
            .status(bin, args, &Self::environment(envs), quiet, cwd)
    }
}

impl<'runner> ActorRunner<'runner> {
    /// Removes inherited routing before trusted private observation assignments.
    fn environment<'environment>(
        envs: &[(&'environment str, Option<&'environment str>)],
    ) -> Vec<(&'environment str, Option<&'environment str>)> {
        [
            ("GIT_COMMON_DIR", None),
            ("GIT_DIR", None),
            ("GIT_INDEX_FILE", None),
            ("GIT_WORK_TREE", None),
        ]
        .into_iter()
        .chain(envs.iter().copied())
        .collect()
    }

    /// Admits the native worktree link and its canonical reciprocal backpointer.
    #[cfg_attr(
        not(test),
        expect(
            clippy::single_call_fn,
            reason = "actor Git ownership is admitted before any native hook runs"
        )
    )]
    fn new(ctx: &Ctx<'runner>, worktree: &str) -> Result<Self, FactorError> {
        let link = ctx
            .fs
            .read_to_string(&Path::new(worktree).join(".git"))
            .map_err(FactorError::StateRead)?;
        let directory = link
            .strip_prefix("gitdir: ")
            .ok_or_else(|| {
                FactorError::GitCommand(non_empty_msg(
                    "invalid candidate Git directory link".to_owned(),
                ))
            })?
            .trim();
        if !Path::new(directory).is_absolute() {
            return Err(FactorError::GitCommand(non_empty_msg(
                "candidate Git directory must be absolute".to_owned(),
            )));
        }
        let owner = ctx
            .fs
            .read_to_string(&Path::new(directory).join("gitdir"))
            .map_err(|error| {
                FactorError::StateRead(io::Error::new(
                    error.kind(),
                    format!("candidate Git directory backpointer: {error}"),
                ))
            })?;
        let actual_owner = ctx
            .fs
            .canonicalize(Path::new(owner.trim_end_matches('\n')))
            .map_err(FactorError::StateRead)?;
        let expected_owner = ctx
            .fs
            .canonicalize(&Path::new(worktree).join(".git"))
            .map_err(FactorError::StateRead)?;
        if actual_owner != expected_owner {
            return Err(FactorError::GitCommand(non_empty_msg(
                "candidate Git directory belongs to another worktree".to_owned(),
            )));
        }
        let runner = Self { inner: ctx.runner };
        let actor = Ctx {
            cwd: Path::new(worktree).to_path_buf(),
            runner: &runner,
            ..ctx.clone()
        };
        let observed = git_output(&actor, &["rev-parse", "--show-toplevel"])?;
        let actual_root = ctx
            .fs
            .canonicalize(Path::new(&observed))
            .map_err(FactorError::StateRead)?;
        let expected_root = ctx
            .fs
            .canonicalize(Path::new(worktree))
            .map_err(FactorError::StateRead)?;
        if actual_root != expected_root {
            return Err(FactorError::GitCommand(non_empty_msg(
                "candidate Git working directory belongs to another worktree".to_owned(),
            )));
        }
        Ok(runner)
    }
    /// Observes physical tracked files without trusting flags in the actor's index.
    fn verify_tree(
        &self,
        ctx: &Ctx<'_>,
        scratch: &Path,
        expected: &TreeHash,
    ) -> Result<(), FactorError> {
        let directory = TempDir::new_in(scratch).map_err(FactorError::StateWrite)?;
        let index = format!("{}/index", directory.path().display());
        let environment = [("GIT_INDEX_FILE", Some(index.as_str()))];
        let observation = Ctx {
            cwd: ctx.cwd.clone(),
            env: ctx.env,
            fs: ctx.fs,
            io: ctx.io,
            runner: self,
        };
        for args in [
            vec![
                "-c",
                "core.ignorestat=false",
                "-c",
                "core.splitIndex=false",
                "-c",
                "core.sparseCheckout=false",
                "read-tree",
                expected.as_str(),
            ],
            vec![
                "-c",
                "core.filemode=true",
                "-c",
                "core.splitIndex=false",
                "-c",
                "core.sparseCheckout=false",
                "add",
                "--update",
            ],
        ] {
            let status = command_status_with(&observation, "git", &args, &environment, false)?;
            if !status.success() {
                return Err(FactorError::GitCommand(non_empty_msg(format!(
                    "git {} failed (exit {})",
                    args.first().unwrap_or(&""),
                    status_code(status),
                ))));
            }
        }
        let actual = TreeHash::new(&git_output_with_env(
            &observation,
            &["-c", "core.splitIndex=false", "write-tree"],
            &environment,
        )?)?;
        if actual != *expected {
            return Err(FactorError::TreeHashMismatch {
                actual,
                expected: expected.clone(),
            });
        }
        Ok(())
    }
}

impl<'source> Candidate<'source> {
    /// Admit the main facts before and after scratch actor validation.
    fn admit(self, ctx: &Ctx<'_>, scratch: &Path) -> Result<(), FactorError> {
        match self.purpose {
            CandidatePurpose::Selection => {
                admit_selection(ctx, scratch, self.original, self.parent, self.tree)
            }
            CandidatePurpose::Observed => admit_observed(ctx, scratch, self),
        }
    }

    /// Borrow a selection request; validation admits its relationships before mutation.
    #[cfg_attr(
        not(test),
        expect(
            clippy::single_call_fn,
            reason = "selection construction retains its explicit mutation-purpose boundary before independent candidate admission"
        )
    )]
    pub(in crate::git_factor) const fn new(
        original: &'source CommitSha,
        messages: &'source NonEmpty<NonEmptyString>,
        parent: Option<&'source CommitSha>,
        tree: &'source TreeHash,
    ) -> Self {
        Self {
            purpose: CandidatePurpose::Selection,
            messages,
            original,
            parent,
            tree,
        }
    }

    /// Borrow an existing detached commit for same-tree metadata verification.
    #[cfg_attr(
        not(test),
        expect(
            clippy::single_call_fn,
            reason = "metadata construction retains its distinct physical-preservation purpose before independent candidate admission"
        )
    )]
    pub(in crate::git_factor) const fn observed(
        original: &'source CommitSha,
        messages: &'source NonEmpty<NonEmptyString>,
        parent: Option<&'source CommitSha>,
        tree: &'source TreeHash,
    ) -> Self {
        Self {
            purpose: CandidatePurpose::Observed,
            messages,
            original,
            parent,
            tree,
        }
    }
}

/// Observe existing path components without following a symlink outside the repository.
pub(in crate::git_factor) fn physical_path_objects(
    ctx: &Ctx<'_>,
    root: &Path,
    raw_path: &[u8],
) -> Result<Vec<(u64, u64)>, FactorError> {
    let mut path = root.to_path_buf();
    let mut objects = Vec::new();
    for component in Path::new(OsStr::from_bytes(raw_path)).components() {
        let Component::Normal(name) = component else {
            return Err(FactorError::GitCommand(non_empty_msg(
                "selection path is not repository relative".to_owned(),
            )));
        };
        path.push(name);
        let metadata = match ctx.fs.symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(FactorError::Io(error)),
        };
        objects.push((metadata.dev(), metadata.ino()));
        if !metadata.is_dir() {
            break;
        }
    }
    Ok(objects)
}

/// Protect unknown objects, including filesystem aliases, before native Git can replace them.
#[expect(
    clippy::single_call_fn,
    reason = "this deleted-path admission boundary keeps the main selection transaction readable"
)]
fn admit_recreated_paths(
    ctx: &Ctx<'_>,
    deleted: &[u8],
    untracked: &[u8],
) -> Result<(), FactorError> {
    let deleted_paths = deleted
        .split(|byte| *byte == b'\0')
        .filter(|path| !path.is_empty())
        .collect::<BTreeSet<_>>();
    let top = git_raw_output(ctx, &["rev-parse", "--show-toplevel"])?;
    if !top.status.success() {
        return Err(FactorError::GitCommand(non_empty_msg(
            "cannot inspect selection worktree root".to_owned(),
        )));
    }
    let root_bytes = top.stdout.strip_suffix(b"\n").ok_or_else(|| {
        FactorError::GitCommand(non_empty_msg(
            "selection worktree root has no output terminator".to_owned(),
        ))
    })?;
    let root = Path::new(OsStr::from_bytes(root_bytes));
    let mut deleted_objects = BTreeSet::new();
    for path in &deleted_paths {
        if let Some(object) = physical_path_objects(ctx, root, path)?.last() {
            deleted_objects.insert(*object);
        }
    }
    let mut recreated = false;
    for path in untracked
        .split(|byte| *byte == b'\0')
        .filter(|path| !path.is_empty())
    {
        if deleted_paths.contains(path)
            || physical_path_objects(ctx, root, path)?
                .iter()
                .any(|object| deleted_objects.contains(object))
        {
            recreated = true;
            break;
        }
    }
    if recreated {
        return Err(FactorError::GitCommand(non_empty_msg(
            "a deleted selection path has been recreated".to_owned(),
        )));
    }
    Ok(())
}

/// Requires the staged atom to belong to the remaining selection and preserves foreign files.
pub(in crate::git_factor) fn admit_selection(
    ctx: &Ctx<'_>,
    scratch: &Path,
    original: &CommitSha,
    parent: Option<&CommitSha>,
    tree: &TreeHash,
) -> Result<(), FactorError> {
    let base = parent.map_or(
        "4b825dc642cb6eb9a060e54bf8d69288fbee4904",
        CommitSha::as_str,
    );
    let allowed = changed_paths(ctx, base, original.as_str())?;
    let selected = changed_paths(ctx, base, tree.as_str())?;
    let allowed_paths = allowed
        .split(|byte| *byte == b'\0')
        .filter(|path| !path.is_empty())
        .collect::<BTreeSet<_>>();
    if selected
        .split(|byte| *byte == b'\0')
        .filter(|path| !path.is_empty())
        .any(|path| !allowed_paths.contains(path))
    {
        return Err(FactorError::GitCommand(non_empty_msg(
            "staged paths must belong to the remaining selected change".to_owned(),
        )));
    }
    let index = scratch.join("selection-index");
    let index_file = index.to_str().ok_or_else(|| {
        FactorError::GitCommand(non_empty_msg(
            "selection index path is not valid UTF-8".to_owned(),
        ))
    })?;
    let environment = [("GIT_INDEX_FILE", Some(index_file))];
    for args in [
        vec![
            "-c",
            "core.ignorestat=false",
            "-c",
            "core.splitIndex=false",
            "-c",
            "core.sparseCheckout=false",
            "read-tree",
            original.as_str(),
        ],
        vec![
            "-c",
            "core.filemode=true",
            "-c",
            "core.splitIndex=false",
            "-c",
            "core.sparseCheckout=false",
            "add",
            "--update",
        ],
    ] {
        let status = command_status_with(ctx, "git", &args, &environment, true)?;
        if !status.success() {
            return Err(FactorError::GitCommand(non_empty_msg(
                "cannot observe the remaining selection without changing its index".to_owned(),
            )));
        }
    }
    let deleted = git_raw_output(
        ctx,
        &[
            "diff",
            "--name-only",
            "-z",
            "--no-renames",
            "--no-relative",
            "--diff-filter=D",
            base,
            original.as_str(),
        ],
    )?;
    let untracked = command_output_with(
        ctx,
        "git",
        &["ls-files", "--others", "--full-name", "-z", "--", ":/"],
        &environment,
    )?;
    if !deleted.status.success() || !untracked.status.success() {
        return Err(FactorError::GitCommand(non_empty_msg(
            "cannot inspect deleted selection paths".to_owned(),
        )));
    }
    admit_recreated_paths(ctx, &deleted.stdout, &untracked.stdout)?;
    let actual = TreeHash::new(&git_output_with_env(
        ctx,
        &["-c", "core.splitIndex=false", "write-tree"],
        &environment,
    )?)?;
    let expected = TreeHash::new(&git_output(
        ctx,
        &["rev-parse", &format!("{original}^{{tree}}")],
    )?)?;
    if actual != expected {
        return Err(FactorError::TreeHashMismatch { actual, expected });
    }
    Ok(())
}

/// Observes changed paths as bytes so unusual filenames retain their identity.
pub(in crate::git_factor) fn changed_paths(
    ctx: &Ctx<'_>,
    base: &str,
    tip: &str,
) -> Result<Vec<u8>, FactorError> {
    let observed = git_raw_output(
        ctx,
        &[
            "diff",
            "--name-only",
            "-z",
            "--no-renames",
            "--no-relative",
            base,
            tip,
        ],
    )?;
    if !observed.status.success() {
        return Err(FactorError::GitCommand(non_empty_msg(
            "cannot inspect candidate selection paths".to_owned(),
        )));
    }
    Ok(observed.stdout)
}

/// Captures a Git observation with explicit environment assignments, without shell evaluation.
fn git_output_with_env(
    ctx: &Ctx<'_>,
    args: &[&str],
    environment: &[(&str, Option<&str>)],
) -> Result<String, FactorError> {
    let observed = command_output_with(ctx, "git", args, environment)?;
    let (stdout, stderr) = output_text(&observed);
    if !observed.status.success() {
        return Err(FactorError::GitCommand(non_empty_msg(format!(
            "git {} failed (exit {}): {stderr}",
            args.join(" "),
            status_code(observed.status)
        ))));
    }
    Ok(stdout.trim().to_owned())
}

/// Publish exact actor ownership before native worktree registration.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "the ownership marker is published once before registering its actor"
    )
)]
fn record_ownership(
    ctx: &Ctx<'_>,
    directory: &Path,
    session: &SessionId,
) -> Result<(), FactorError> {
    let common = PathBuf::from(git_output(
        ctx,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )?);
    let owner = ActorOwner {
        session: session.clone(),
        actor: ctx
            .fs
            .canonicalize(directory)
            .map_err(FactorError::StateRead)?
            .join("worktree"),
        common,
    };
    ctx.fs
        .write_atomic_string(
            &directory.join("owner.json"),
            &serde_json::to_string(&owner).map_err(|_error| {
                FactorError::GitCommand(non_empty_msg(
                    "cannot serialize actor ownership".to_owned(),
                ))
            })?,
        )
        .map_err(FactorError::StateWrite)?;
    Ok(())
}

/// Admits every in-session registration before removing exact owned scratch actors.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "resume admits all owned registrations before any cleanup mutation"
    )
)]
pub(in crate::git_factor) fn remove_owned_actors(
    ctx: &Ctx<'_>,
    state: &StateDir,
    session: &SessionId,
) -> Result<(), FactorError> {
    let root = ctx
        .fs
        .canonicalize(state.as_path())
        .map_err(FactorError::StateRead)?;
    let common = PathBuf::from(git_output(
        ctx,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )?);
    let listed = git_raw_output(ctx, &["worktree", "list", "--porcelain", "-z"])?;
    if !listed.status.success() {
        return Err(FactorError::GitCommand(non_empty_msg(
            "cannot observe owned actor registrations".to_owned(),
        )));
    }
    let mut actors = Vec::new();
    let prefix = format!("{}/", root.display());
    for record in listed.stdout.split(|byte| *byte == 0) {
        let Some(path) = record.strip_prefix(b"worktree ") else {
            continue;
        };
        if !path.starts_with(prefix.as_bytes()) {
            continue;
        }
        let text = str::from_utf8(path).map_err(|_error| {
            FactorError::GitCommand(non_empty_msg(
                "unknown actor registration path must be preserved".to_owned(),
            ))
        })?;
        let actor = PathBuf::from(text);
        let scratch = actor.parent().ok_or_else(|| {
            FactorError::GitCommand(non_empty_msg("owned actor lacks scratch parent".to_owned()))
        })?;
        let marker = ctx
            .fs
            .read_to_string(&scratch.join("owner.json"))
            .map_err(FactorError::StateRead)?;
        let owner: ActorOwner = serde_json::from_str(&marker).map_err(|_error| {
            FactorError::GitCommand(non_empty_msg(
                "unknown actor ownership must be preserved".to_owned(),
            ))
        })?;
        if owner.session != *session
            || owner.actor != actor
            || owner.common != common
            || scratch.parent() != Some(root.as_path())
        {
            return Err(FactorError::GitCommand(non_empty_msg(
                "foreign actor registration must be preserved".to_owned(),
            )));
        }
        actors.push(actor);
    }
    for actor in actors {
        let path = actor.to_str().ok_or_else(|| {
            FactorError::GitCommand(non_empty_msg("owned actor path is not UTF-8".to_owned()))
        })?;
        run_git(ctx, &["worktree", "remove", "--force", "--force", path])?;
        let scratch = actor.parent().ok_or_else(|| {
            FactorError::GitCommand(non_empty_msg("owned actor lacks scratch parent".to_owned()))
        })?;
        ctx.fs
            .remove_dir_all(scratch)
            .map_err(FactorError::StateWrite)?;
    }
    Ok(())
}

/// Observes the original UTF-8 author fields without Git pretty-format re-encoding.
pub(in crate::git_factor) fn author_metadata(
    ctx: &Ctx<'_>,
    commit: &str,
) -> Result<String, FactorError> {
    let output = git_raw_output(ctx, &["cat-file", "commit", commit])?;
    if !output.status.success() {
        return Err(FactorError::GitCommand(non_empty_msg(format!(
            "cannot observe candidate author (exit {}): {}",
            status_code(output.status),
            String::from_utf8_lossy(&output.stderr).trim()
        ))));
    }
    let text = String::from_utf8(output.stdout).map_err(|error| {
        FactorError::GitCommand(non_empty_msg(format!(
            "candidate author is not UTF-8: {error}"
        )))
    })?;
    let malformed = || {
        FactorError::GitCommand(non_empty_msg(
            "truncated candidate author metadata".to_owned(),
        ))
    };
    let headers = text.split_once("\n\n").ok_or_else(malformed)?.0;
    let author = headers
        .split('\n')
        .find_map(|line| line.strip_prefix("author "))
        .ok_or_else(malformed)?;
    let (identity, date) = author.rsplit_once("> ").ok_or_else(malformed)?;
    let (name, email) = identity.rsplit_once(" <").ok_or_else(malformed)?;
    let native_name = name.trim_end_matches(|character: char| character.is_ascii_whitespace());
    let native_date = date
        .strip_suffix(" -0000")
        .map_or_else(|| date.to_owned(), |timestamp| format!("{timestamp} +0000"));
    Ok(format!("{native_name}\0{email}\0{native_date}"))
}

/// Runs native message validation and each ordered gate in the bound owned actor.
#[expect(
    clippy::single_call_fn,
    reason = "bound actor validation forms one transaction within owned cleanup"
)]
fn validate_actor(
    ctx: &Ctx<'_>,
    directory: &TempDir,
    candidate: Candidate<'_>,
    provisional: &CommitSha,
    hooks: &str,
    native_message: (&str, &str),
    gates: &GateSet,
) -> Result<CommitSha, FactorError> {
    let (metadata, message_file) = native_message;
    let Candidate { parent, tree, .. } = candidate;
    let worktree = directory.path().join("worktree");
    let worktree_path = worktree.to_str().ok_or_else(|| {
        FactorError::GitCommand(non_empty_msg("owned actor path is not UTF-8".to_owned()))
    })?;
    let hook_config = format!("core.hooksPath={hooks}");
    let runner = ActorRunner::new(ctx, worktree_path)?;
    let candidate_ctx = Ctx {
        cwd: worktree.clone(),
        env: ctx.env,
        fs: ctx.fs,
        io: ctx.io,
        runner: &runner,
    };

    run_git(
        &candidate_ctx,
        &["reset", "--hard", "--quiet", provisional.as_str()],
    )?;
    run_git(
        &candidate_ctx,
        &[
            "-c",
            &hook_config,
            "commit",
            "--quiet",
            "--amend",
            "--allow-empty",
            "--cleanup=verbatim",
            "--file",
            message_file,
        ],
    )?;
    let parents = git_output(
        &candidate_ctx,
        &["show", "--format=%P", "--no-patch", "HEAD"],
    )?;
    if parents != parent.map_or("", CommitSha::as_str) {
        return Err(FactorError::GitCommand(non_empty_msg(
            "candidate hooks changed the selected parent".to_owned(),
        )));
    }
    if author_metadata(&candidate_ctx, "HEAD")? != metadata {
        return Err(FactorError::GitCommand(non_empty_msg(
            "candidate hooks changed the original author".to_owned(),
        )));
    }
    let candidate_tree = TreeHash::new(&git_output(
        &candidate_ctx,
        &["rev-parse", concat!("HEAD^", "{", "tree", "}")],
    )?)?;
    if candidate_tree != *tree {
        return Err(FactorError::TreeHashMismatch {
            actual: candidate_tree,
            expected: tree.clone(),
        });
    }
    ensure_repo_clean(
        &candidate_ctx,
        "candidate hooks must preserve the selected tree",
    )?;
    runner.verify_tree(&candidate_ctx, directory.path(), tree)?;
    super::gate::verify_observed(&candidate_ctx, gates, Path::new(hooks), &|expected| {
        runner.verify_tree(&candidate_ctx, directory.path(), expected)
    })
}

/// Metadata-only promotion cannot consume unrelated files recreated after a deletion.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "same-tree promotion has a distinct physical conservation admission before and after native actor validation"
    )
)]
fn admit_observed(
    ctx: &Ctx<'_>,
    scratch: &Path,
    candidate: Candidate<'_>,
) -> Result<(), FactorError> {
    let Candidate {
        original,
        parent,
        tree,
        ..
    } = candidate;
    let expected_parent = parent.map_or("", CommitSha::as_str);
    if git_output(ctx, &["rev-parse", "--symbolic-full-name", "HEAD"])? != "HEAD"
        || git_output(ctx, &["rev-parse", "HEAD"])? != original.as_str()
        || git_output(
            ctx,
            &["show", "--format=%P", "--no-patch", original.as_str()],
        )? != expected_parent
        || git_output(ctx, &["rev-parse", &format!("{original}^{{tree}}")])? != tree.as_str()
    {
        return Err(FactorError::GitCommand(non_empty_msg(
            "observed replay commit identity changed during isolated validation".to_owned(),
        )));
    }
    let staged = command_status_with(
        ctx,
        "git",
        &["diff", "--cached", "--quiet", tree.as_str(), "--", ":/"],
        &[("GIT_OPTIONAL_LOCKS", Some("0"))],
        true,
    )?;
    if !staged.success() {
        return Err(FactorError::GitCommand(non_empty_msg(
            "observed replay staged tree no longer matches its commit".to_owned(),
        )));
    }
    let index = scratch.join("observed-index");
    let index_file = index.to_str().ok_or_else(|| {
        FactorError::GitCommand(non_empty_msg(
            "observed index path is not valid UTF-8".to_owned(),
        ))
    })?;
    // Each admission rebuilds this private index; the user's real index is untouched.
    let environment = [("GIT_INDEX_FILE", Some(index_file))];
    for args in [
        vec![
            "-c",
            "core.ignorestat=false",
            "-c",
            "core.splitIndex=false",
            "-c",
            "core.sparseCheckout=false",
            "read-tree",
            original.as_str(),
        ],
        vec![
            "-c",
            "core.filemode=true",
            "-c",
            "core.splitIndex=false",
            "-c",
            "core.sparseCheckout=false",
            "add",
            "--update",
        ],
    ] {
        let status = command_status_with(ctx, "git", &args, &environment, true)?;
        if !status.success() {
            return Err(FactorError::GitCommand(non_empty_msg(
                "cannot observe physical replay tree".to_owned(),
            )));
        }
    }
    let actual = TreeHash::new(&git_output_with_env(
        ctx,
        &["-c", "core.splitIndex=false", "write-tree"],
        &environment,
    )?)?;
    if actual != *tree {
        return Err(FactorError::TreeHashMismatch {
            actual,
            expected: tree.clone(),
        });
    }
    Ok(())
}

/// Validates an isolated actual candidate with the ordered named checks.
pub(in crate::git_factor) fn validate_named_in(
    ctx: &Ctx<'_>,
    state_dir: &StateDir,
    session: &SessionId,
    candidate: Candidate<'_>,
    gates: &GateSet,
) -> Result<CommitSha, FactorError> {
    let Candidate {
        original,
        messages,
        parent,
        tree,
        ..
    } = candidate;
    let state_path = state_dir.as_path().to_str().ok_or_else(|| {
        FactorError::GitCommand(non_empty_msg(
            "candidate state path is not valid UTF-8".to_owned(),
        ))
    })?;
    let directory = TempDir::new_in(state_path).map_err(FactorError::StateWrite)?;
    let message_path = directory.path().join("message");
    // Tempfile appends an ASCII name to the admitted UTF-8 parent.
    let directory_path = directory.path().display().to_string();
    let message_file = format!("{directory_path}/message");
    let message = messages
        .iter()
        .map(NonEmptyString::as_str)
        .collect::<Vec<_>>()
        .join("\n\n");
    ctx.fs
        .write_string(&message_path, &format!("{message}\n"))
        .map_err(FactorError::StateWrite)?;
    let metadata = author_metadata(ctx, original.as_str())?;
    let fields = metadata.split('\0').collect::<Vec<_>>();
    let &[name, email, date] = fields.as_slice() else {
        return Err(FactorError::GitCommand(non_empty_msg(
            "truncated candidate author metadata".to_owned(),
        )));
    };
    let mut args = vec!["commit-tree", tree.as_str(), "-F", &message_file];
    if let Some(parent_commit) = parent {
        args.extend(["-p", parent_commit.as_str()]);
    }
    candidate.admit(ctx, directory.path())?;
    let native_date = format!("@{date}");
    let provisional = CommitSha::new(git_output_with_env(
        ctx,
        &args,
        &[
            ("GIT_AUTHOR_NAME", Some(name)),
            ("GIT_AUTHOR_EMAIL", Some(email)),
            ("GIT_AUTHOR_DATE", Some(native_date.as_str())),
        ],
    )?)?;
    let worktree_path = format!("{directory_path}/worktree");
    let hook_output = git_raw_output(
        ctx,
        &["rev-parse", "--path-format=absolute", "--git-path", "hooks"],
    )?;
    if !hook_output.status.success() {
        let stderr = String::from_utf8_lossy(&hook_output.stderr);
        let diagnostic = stderr.trim();
        let hook_diagnostic = if diagnostic.is_empty() {
            hook_output.status.to_string()
        } else {
            diagnostic.to_owned()
        };
        return Err(FactorError::GitCommand(non_empty_msg(hook_diagnostic)));
    }
    let hook_record = String::from_utf8(hook_output.stdout).map_err(|error| {
        FactorError::GitCommand(non_empty_msg(format!(
            "candidate hooks path is not UTF-8: {error}"
        )))
    })?;
    let hooks = hook_record.strip_suffix('\n').unwrap_or(&hook_record);
    record_ownership(ctx, directory.path(), session)?;
    if let Err(err) = run_git(
        ctx,
        &[
            "worktree",
            "add",
            "--quiet",
            "--detach",
            "--no-checkout",
            &worktree_path,
            provisional.as_str(),
        ],
    ) {
        let _retained = directory.keep();
        return Err(err);
    }
    let result = validate_actor(
        ctx,
        &directory,
        candidate,
        &provisional,
        hooks,
        (&metadata, &message_file),
        gates,
    );
    if let Err(err) = run_git(ctx, &["worktree", "remove", "--force", &worktree_path]) {
        let _retained = directory.keep();
        return Err(err);
    }
    let validated = result?;
    candidate.admit(ctx, directory.path())?;
    Ok(validated)
}

#[cfg(test)]
#[path = "candidate_tests.rs"]
pub(in crate::git_factor) mod contracts;

#[cfg(test)]
#[path = "candidate_ownership_tests.rs"]
mod ownership_contracts;

#[cfg(test)]
#[path = "candidate_changed_paths_tests.rs"]
mod path_queries;

#[cfg(test)]
#[path = "candidate_configured_index_tests.rs"]
mod configured_indexes;

#[cfg(test)]
#[path = "candidate_runner_observation.rs"]
mod runner_observation;

#[cfg(test)]
mod proptests {
    mod actor_runner {
        include!("candidate_runner_properties.rs");
    }
    mod author_metadata {
        use super::super::{
            author_metadata,
            author_queries::{AuthorAlias, AuthorQuery},
        };
        use crate::test_support::OrAbort as _;
        use proptest::prelude::*;
        use proptest::sample::select;

        const EPOCH_END: u32 = 2_000_000_000;
        const EPOCH_START: u32 = 0;

        proptest! {
            #[test]
            fn preserves_generated_native_author_aliases_and_complete_frame(
                stem in "[A-Z][a-z]{0,12}",
                epoch in EPOCH_START..EPOCH_END,
                alias in select(vec![
                    AuthorAlias::NegativeZero,
                    AuthorAlias::NonBreakingNameSpace,
                    AuthorAlias::TrailingNameSpace,
                    AuthorAlias::TrailingNameTab,
                ]),
            ) {
                let fixture = AuthorQuery::arrange_alias(&stem, epoch, alias);
                let before = fixture.frame();

                let observed = author_metadata(&fixture.context(), fixture.tip());

                assert_eq!(observed.or_abort("native-normalized author tuple"), fixture.expected());
                assert_eq!(fixture.frame(), before);
            }

            #[test]
            fn preserves_generated_raw_authors_across_source_and_log_encodings(
                stem in "[A-Z][a-z]{0,12}",
                epoch in EPOCH_START..EPOCH_END,
                zone in select(vec!["+0000", "-0700", "+0230"]),
                encoded_source in any::<bool>(),
            ) {
                let name = format!("{stem} Jos\u{e9}");
                let fixture = AuthorQuery::arrange(&name, epoch, zone, encoded_source);
                let before = fixture.frame();

                let observed = author_metadata(&fixture.context(), fixture.tip());

                assert_eq!(observed.or_abort("raw author tuple"), fixture.expected());
                assert_eq!(fixture.frame(), before);
            }
        }
    }

    mod changed_paths {
        use super::super::{changed_paths, path_queries::PathQuery};
        use proptest::collection::vec;
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn preserves_generated_native_byte_paths_and_query_refusals(
                stem in "[a-zA-Z0-9_-]{1,24}",
                content in vec(any::<u8>(), 0..32),
                newline in any::<bool>(),
                refused in any::<bool>(),
            ) {
                let fixture = PathQuery::arrange(&stem, &content, newline);
                let before = fixture.frame();
                let tip = if refused { "refs/heads/absent" } else { fixture.tip() };
                let expected = if refused { Err("git command failed: cannot inspect candidate selection paths".to_owned()) } else { Ok(fixture.expected().to_vec()) };

                let observed = changed_paths(&fixture.context(), fixture.base(), tip);

                prop_assert_eq!(observed.map_err(|error| error.to_string()), expected);
                prop_assert_eq!(fixture.frame(), before);
            }
        }
    }
    mod validate_named_in_environment {
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn gates_create_independent_repositories_with_native_actor_hooks(
                root in any::<bool>(),
                value in "[A-Za-z0-9]{1,24}",
            ) {
                super::super::contracts::environment::verify_fixture_repository(root, &value);
            }
        }
    }
    mod remove_owned_actors {
        use proptest::prelude::*;
        use proptest::sample::select;

        proptest! {
            #[test]
            fn preserves_generated_refused_registration_sets(
                case in select(super::super::ownership_contracts::Refusal::ALL.to_vec()),
                stem in "[a-z]{1,12}",
                bytes in prop::collection::vec(any::<u8>(), 0..64),
            ) {
                super::super::ownership_contracts::verify_refusal(case, &stem, &bytes);
            }

            #[test]
            fn removes_only_admitted_locked_scratch_actors(
                stem in "[a-z]{1,12}",
                reason in "[A-Za-z ]{0,24}",
                bytes in prop::collection::vec(any::<u8>(), 0..64),
            ) {
                super::super::ownership_contracts::verify_locked_cleanup(&stem, &reason, &bytes);
            }
        }
    }
    mod admit_selection {
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn preserves_generated_selection_boundaries(root in any::<bool>()) {
                super::super::contracts::verify_admission(root);
                super::super::contracts::faults::verify_observation_failures(root);
                super::super::contracts::faults::verify_state_paths(root);
            }
        }
    }
    mod physical_path_objects {
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn preserves_generated_owned_and_foreign_physical_paths(
                directory_first in any::<bool>(),
                case_rename in any::<bool>(),
                ignored in any::<bool>(),
                content in "[A-Za-z0-9\n]{1,32}",
            ) {
                super::super::contracts::verify_owned_path_transition(directory_first, case_rename, &content);
                super::super::contracts::verify_recreated_deleted_alias("Add selected atom", &content, ignored);
            }
        }
    }
    mod candidate {
        mod new {
            use proptest::prelude::*;

            proptest! {
                #[test]
                fn admits_generated_messages_and_selected_trees(
                    root in any::<bool>(),
                    message in "[A-Za-z][A-Za-z0-9 ]{0,20}",
                    atom in "[A-Za-z0-9\n]{0,64}",
                ) {
                    super::super::super::contracts::verify_request(
                        root,
                        &message,
                        super::super::super::contracts::ValidationCase::Passing,
                        &atom,
                    );
                }
            }
        }
        mod observed {
            use super::super::super::{
                Candidate, CandidatePurpose, CommitSha, NonEmptyString, TreeHash,
            };
            use crate::test_support::OrAbort as _;
            use nonempty::NonEmpty;
            use proptest::collection::vec;
            use proptest::prelude::*;

            proptest! {
                #[test]
                fn preserves_generated_observed_request_facts(
                    root in any::<bool>(),
                    paragraphs in vec("[A-Za-z][A-Za-z0-9 ]{0,20}", 1..=3),
                    source in "[0-9a-f]{40}",
                    selected_tree in "[0-9a-f]{40}",
                ) {
                    let messages = NonEmpty::from_vec(paragraphs.into_iter().map(|paragraph| NonEmptyString::new(paragraph).or_abort("nonempty paragraph")).collect()).or_abort("nonempty paragraphs");
                    let original = CommitSha::new(source).or_abort("source identity");
                    let tree = TreeHash::new(&selected_tree).or_abort("tree identity");
                    let parent_commit = CommitSha::new("3".repeat(40)).or_abort("parent identity");
                    let parent = (!root).then_some(&parent_commit);

                    let observed = Candidate::observed(&original, &messages, parent, &tree);

                    prop_assert_eq!((observed.messages, observed.original, observed.parent, observed.tree), (&messages, &original, parent, &tree));
                    prop_assert!(matches!(observed.purpose, CandidatePurpose::Observed));
                }
            }
        }
    }
    mod validate_named_in {
        use proptest::prelude::*;
        use proptest::sample::select;

        const FIRST_PROCESS: usize = 1;
        const PROCESS_END: usize = 40;

        proptest! {
            #[test]
            fn preserves_recreated_deleted_paths_outside_caller_directory(
                ignored in any::<bool>(),
                contents in "[A-Za-z0-9\n]{1,32}",
                message in "[A-Za-z][A-Za-z0-9 ]{0,20}",
            ) {
                super::super::contracts::verify_recreated_deleted_path(&message, &contents, ignored);
            }

            #[test]
            fn preserves_recreated_deleted_filesystem_aliases(
                ignored in any::<bool>(),
                contents in "[A-Za-z0-9\n]{1,32}",
                message in "[A-Za-z][A-Za-z0-9 ]{0,20}",
            ) {
                super::super::contracts::verify_recreated_deleted_alias(&message, &contents, ignored);
            }

            #[test]
            fn isolates_generated_root_and_ordinary_candidates(root in any::<bool>(), message in "[A-Za-z][A-Za-z0-9 ]{0,20}", case in select(super::super::contracts::ValidationCase::ALL.to_vec())) {
                super::super::contracts::verify_validation(root, &message, case);
            }

            #[test]
            fn preserves_selection_at_generated_process_failures(fail_at in FIRST_PROCESS..PROCESS_END) {
                let _calls = super::super::contracts::verify_process_failure(fail_at);
            }

            #[test]
            fn refuses_generated_invalid_observations(root in any::<bool>()) {
                super::super::contracts::faults::verify_observation_failures(root);
                super::super::contracts::faults::verify_state_paths(root);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    mod admit_observed {
        use super::super::{
            Candidate, NonEmpty, NonEmptyString, admit_observed,
            configured_indexes::ConfiguredIndex,
        };
        use crate::test_support::OrAbort as _;

        #[test]
        fn configured_split_index_admission_preserves_the_complete_native_frame() {
            let fixture = ConfiguredIndex::arrange(true);
            fixture.enable_split_index();
            let messages = NonEmpty::new(
                NonEmptyString::new("Change native source".to_owned()).or_abort("message"),
            );
            let candidate = Candidate::observed(
                fixture.source(),
                &messages,
                Some(fixture.base()),
                fixture.tree(),
            );
            let before = fixture.frame();

            let observed = admit_observed(&fixture.context(), fixture.scratch(), candidate);

            observed.or_abort("native observed admission");
            assert_eq!(fixture.frame(), before);
        }
    }

    mod actor_runner {
        include!("candidate_runner_units.rs");
        mod verify_tree {
            use super::super::super::{ActorRunner, configured_indexes::ConfiguredIndex};
            use crate::test_support::OrAbort as _;

            #[test]
            fn configured_split_index_verification_preserves_the_complete_native_frame() {
                let fixture = ConfiguredIndex::arrange(true);
                let actor = fixture.arrange_actor();
                fixture.enable_split_index();
                let mut context = fixture.context();
                context.cwd = actor.clone();
                let before = fixture.frame();

                let observed =
                    ActorRunner::new(&context, actor.to_str().or_abort("native actor path"))
                        .and_then(|runner| {
                            runner.verify_tree(&context, fixture.scratch(), fixture.tree())
                        });

                observed.or_abort("admitted native actor tree");
                assert_eq!(fixture.frame(), before);
            }
        }
    }

    mod author_metadata {
        use super::super::{
            author_metadata,
            author_queries::{AuthorAlias, AuthorQuery},
        };
        use crate::git_factor::FactorError;
        use crate::test_support::{OrAbort as _, ResultOrAbort as _};

        #[test]
        fn preserves_native_epoch_below_bare_date_boundary() {
            let fixture = AuthorQuery::arrange("Native", 99_999_999, "+0000", false);
            let before = fixture.frame();

            let observed = author_metadata(&fixture.context(), fixture.tip());

            assert_eq!(observed.or_abort("native author tuple"), fixture.expected());
            assert_eq!(fixture.frame(), before);
        }

        #[test]
        fn preserves_native_epoch_at_bare_date_boundary() {
            let fixture = AuthorQuery::arrange("Native", 100_000_000, "+0000", false);
            let before = fixture.frame();

            let observed = author_metadata(&fixture.context(), fixture.tip());

            assert_eq!(observed.or_abort("native author tuple"), fixture.expected());
            assert_eq!(fixture.frame(), before);
        }

        #[test]
        fn preserves_native_negative_zero_date_without_mutating_any_facts() {
            let fixture =
                AuthorQuery::arrange_alias("Native", 1_000_000_000, AuthorAlias::NegativeZero);
            let before = fixture.frame();

            let observed = author_metadata(&fixture.context(), fixture.tip());

            assert_eq!(
                observed.or_abort("native-normalized author tuple"),
                fixture.expected()
            );
            assert_eq!(fixture.frame(), before);
        }

        #[test]
        fn preserves_native_tab_suffix_without_mutating_any_facts() {
            let fixture =
                AuthorQuery::arrange_alias("Native", 1_000_000_000, AuthorAlias::TrailingNameTab);
            let before = fixture.frame();

            let observed = author_metadata(&fixture.context(), fixture.tip());

            assert_eq!(
                observed.or_abort("native-normalized author tuple"),
                fixture.expected()
            );
            assert_eq!(fixture.frame(), before);
        }

        #[test]
        fn preserves_raw_author_fields_and_complete_native_frame() {
            let fixture = AuthorQuery::arrange("Jos\u{e9}", 1_000_000_000, "-0700", true);
            let before = fixture.frame();

            let observed = author_metadata(&fixture.context(), fixture.tip());

            assert_eq!(observed.or_abort("raw author tuple"), fixture.expected());
            assert_eq!(fixture.frame(), before);
        }

        #[test]
        fn refuses_native_object_query_failure_without_mutating_any_facts() {
            let fixture = AuthorQuery::arrange("Jos\u{e9}", 1_000_000_000, "+0230", false);
            let before = fixture.frame();

            let error = author_metadata(&fixture.context(), "refs/heads/absent")
                .err_or_abort("failed native author query");

            assert!(matches!(error, FactorError::GitCommand(_)));
            assert_eq!(
                error.to_string(),
                "git command failed: cannot observe candidate author (exit 128): fatal: Not a valid object name refs/heads/absent"
            );
            assert_eq!(fixture.frame(), before);
        }
    }

    mod changed_paths {
        use super::super::{changed_paths, path_queries::PathQuery};
        use crate::test_support::{OrAbort as _, ResultOrAbort as _};

        #[test]
        fn returns_exact_nul_paths_without_mutating_native_or_user_facts() {
            let fixture = PathQuery::arrange("unit", b"binary\0bytes", true);
            let before = fixture.frame();

            let observed = changed_paths(&fixture.context(), fixture.base(), fixture.tip());

            assert_eq!(observed.or_abort("native paths"), fixture.expected());
            assert_eq!(fixture.frame(), before);
        }
        #[test]
        fn refuses_a_failed_native_query_without_mutating_any_saved_facts() {
            let fixture = PathQuery::arrange("unit", b"selected bytes", false);
            let before = fixture.frame();

            let error = changed_paths(&fixture.context(), fixture.base(), "refs/heads/absent")
                .err_or_abort("native query refusal");

            assert_eq!(
                error.to_string(),
                "git command failed: cannot inspect candidate selection paths"
            );
            assert_eq!(fixture.frame(), before);
        }
    }
    mod validate_named_in_environment {
        #[test]
        fn gate_fixture_and_native_hooks_preserve_the_selection_repository() {
            super::super::contracts::environment::verify_fixture_repository(false, "independent");
        }
    }
    mod remove_owned_actors {
        #[test]
        fn refuses_unowned_registrations_before_removing_any_actor() {
            for case in super::super::ownership_contracts::Refusal::ALL {
                super::super::ownership_contracts::verify_refusal(case, "unit", b"foreign bytes\n");
            }
        }

        #[test]
        fn removes_complete_locked_owned_actor_and_preserves_external_registration() {
            super::super::ownership_contracts::verify_locked_cleanup(
                "unit",
                "initializing",
                b"foreign bytes\n",
            );
        }
    }
    mod admit_selection {
        use super::super::{admit_selection, configured_indexes::ConfiguredIndex};
        use crate::test_support::OrAbort as _;

        #[test]
        fn configured_split_index_admission_preserves_the_complete_native_frame() {
            let fixture = ConfiguredIndex::arrange(false);
            fixture.enable_split_index();
            let before = fixture.frame();

            let observed = admit_selection(
                &fixture.context(),
                fixture.scratch(),
                fixture.source(),
                Some(fixture.base()),
                fixture.tree(),
            );

            observed.or_abort("native selection admission");
            assert_eq!(fixture.frame(), before);
        }

        #[test]
        fn refuses_foreign_staging_and_modified_selection() {
            for root in [false, true] {
                super::super::contracts::verify_admission(root);
                super::super::contracts::faults::verify_observation_failures(root);
                super::super::contracts::faults::verify_state_paths(root);
            }
        }
    }
    mod physical_path_objects {
        #[test]
        fn distinguishes_owned_transitions_from_foreign_aliases() {
            for (directory_first, case_rename) in
                [(false, true), (false, false), (true, false), (true, true)]
            {
                super::super::contracts::verify_owned_path_transition(
                    directory_first,
                    case_rename,
                    "source bytes\n",
                );
            }
            for ignored in [false, true] {
                super::super::contracts::verify_recreated_deleted_alias(
                    "Add selected atom",
                    "foreign alias\n",
                    ignored,
                );
            }
        }
    }
    mod candidate {
        mod new {
            #[test]
            fn admits_parentless_and_ordinary_native_requests() {
                for (root, atom) in [(true, "root atom\n"), (false, "ordinary atom\n")] {
                    super::super::super::contracts::verify_request(
                        root,
                        "Add borrowed request",
                        super::super::super::contracts::ValidationCase::Passing,
                        atom,
                    );
                }
            }
        }
        mod observed {
            use super::super::super::{
                Candidate, CandidatePurpose, CommitSha, NonEmptyString, TreeHash,
            };
            use crate::test_support::OrAbort as _;
            use nonempty::NonEmpty;

            #[test]
            fn preserves_parentless_observed_request_facts() {
                let messages = NonEmpty::new(
                    NonEmptyString::new("Add observed request".to_owned()).or_abort("message"),
                );
                let original = CommitSha::new("1".repeat(40)).or_abort("source identity");
                let tree = TreeHash::new(&"2".repeat(40)).or_abort("tree identity");
                let parent = None;

                let observed = Candidate::observed(&original, &messages, parent, &tree);

                assert_eq!(
                    (
                        observed.messages,
                        observed.original,
                        observed.parent,
                        observed.tree
                    ),
                    (&messages, &original, parent, &tree)
                );
                assert!(matches!(observed.purpose, CandidatePurpose::Observed));
            }

            #[test]
            fn preserves_ordinary_observed_request_facts() {
                let messages = NonEmpty::new(
                    NonEmptyString::new("Add observed request".to_owned()).or_abort("message"),
                );
                let original = CommitSha::new("1".repeat(40)).or_abort("source identity");
                let tree = TreeHash::new(&"2".repeat(40)).or_abort("tree identity");
                let parent_commit = CommitSha::new("3".repeat(40)).or_abort("parent identity");
                let parent = Some(&parent_commit);

                let observed = Candidate::observed(&original, &messages, parent, &tree);

                assert_eq!(
                    (
                        observed.messages,
                        observed.original,
                        observed.parent,
                        observed.tree
                    ),
                    (&messages, &original, parent, &tree)
                );
                assert!(matches!(observed.purpose, CandidatePurpose::Observed));
            }
        }
    }
    mod validate_named_in {
        #[test]
        fn refuses_recreated_deleted_paths_outside_caller_directory() {
            for ignored in [false, true] {
                super::super::contracts::verify_recreated_deleted_path(
                    "Add selected atom",
                    "restored user bytes\n",
                    ignored,
                );
            }
        }

        #[test]
        fn refuses_recreated_deleted_filesystem_aliases() {
            for ignored in [false, true] {
                super::super::contracts::verify_recreated_deleted_alias(
                    "Add selected atom",
                    "restored alias user bytes\n",
                    ignored,
                );
            }
        }

        #[test]
        fn preserves_selection_when_candidate_gate_fails() {
            super::super::contracts::verify_validation(
                false,
                "A",
                super::super::contracts::ValidationCase::GateFailure,
            );
        }

        #[test]
        fn refuses_invalid_observations_and_filesystem_paths() {
            for root in [false, true] {
                super::super::contracts::faults::verify_observation_failures(root);
                super::super::contracts::faults::verify_state_paths(root);
            }
        }
        #[test]
        fn preserves_selection_at_every_process_failure() {
            let calls = super::super::contracts::verify_process_failure(usize::MAX);
            for fail_at in 1..=calls {
                let _calls = super::super::contracts::verify_process_failure(fail_at);
            }
        }

        #[test]
        fn isolates_root_and_ordinary_candidates() {
            for root in [false, true] {
                for case in super::super::contracts::ValidationCase::ALL {
                    super::super::contracts::verify_validation(root, "Add selected atom", case);
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "candidate_author_metadata_tests.rs"]
mod author_queries;
