//! Completes each validated split as a durable Git checkpoint, then opens a fresh remaining selection.
use super::output;
use super::types::{BranchRef, CommitSha, SessionId};
use super::{
    Ctx, FactorError, NonEmpty, NonEmptyString, StateDir, TreeHash, candidate, command_status_with,
    gate, git_dir_in, git_output, git_raw_output, is_mid_rebase_in, non_empty_msg,
    resolve_commit_span, run_git, shell_quote, trace_note,
};
use alloc::collections::BTreeSet;
use core::fmt::Write as _;
use core::iter::once;
use core::num::{NonZeroI32, NonZeroU64, NonZeroUsize};
use serde::{Deserialize, Serialize};
use std::ffi::OsString;
use std::io;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use tempfile::{NamedTempFile, TempDir};

/// Single repository-wide reachability lease owned by the admitted session.
const LEASE: &str = "refs/factor/session-lease";
/// Git boolean-query exit code for a false result rather than an operational error.
const GIT_FALSE: i32 = 1;
/// Git empty-tree object used for parentless root selection.
const EMPTY: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";
/// Transient command authority; abort never executes transcript callbacks.
#[derive(Clone, Copy)]
enum Admission {
    /// Abort may admit an unavailable original executable from the owned transcript.
    Abort,
    /// Native callback and resume admission use the current canonical executable.
    Resume,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// Persisted format identity; legacy state is never interpreted as this journal.
enum Format {
    /// Completed-round journal format.
    CheckpointV2,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "snake_case", deny_unknown_fields)]
/// Terminal facts retained until exact-owned cleanup completes.
enum Outcome {
    /// The current attempt was abandoned at the last captured checkpoint.
    Aborted {
        /// Last captured atom boundary preceding the abandoned selection.
        base: Option<CommitSha>,
    },
    /// All selected changes became independently validated atoms.
    Complete {
        /// Independently validated final atom represented by the completed outcome.
        atom: CommitSha,
    },
}
/// Native commit projection is permitted only inside the owned selection pool.
#[derive(Clone, Copy)]
enum StagedPurpose<'source> {
    /// Replay and initial admission preserve every staged intention.
    Clean,
    /// A candidate may omit only source-owned intent-to-add entries.
    Selection {
        /// Parent seam; absence denotes a parentless root source.
        base: Option<&'source CommitSha>,
        /// Combined source whose changed paths define the owned pool.
        source: &'source CommitSha,
    },
}

/// Construction and independent acceptance are distinct durable remainder facts.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(
    tag = "state",
    content = "commit",
    rename_all = "snake_case",
    deny_unknown_fields
)]
enum Remainder {
    /// No remaining tree change was constructed.
    Absent,
    /// Independently validated remainder commit.
    Accepted(CommitSha),
    /// Constructed remainder object awaiting independent validation.
    Pending(CommitSha),
}

impl Remainder {
    /// Progress consumers require the actual remainder acceptance producer.
    #[expect(
        clippy::pattern_type_mismatch,
        reason = "borrowed remainder observation preserves the owned durable commit identity"
    )]
    fn accepted(&self) -> Result<Option<&CommitSha>, FactorError> {
        match self {
            Self::Absent => Ok(None),
            Self::Accepted(commit) => Ok(Some(commit)),
            Self::Pending(_) => Err(failure(
                "remainder lacks independent acceptance; automatic recovery is unavailable after bypassing git factor --continue",
            )),
        }
    }
    /// The observed object is usable for reachability and ownership, not acceptance.
    #[expect(
        clippy::pattern_type_mismatch,
        reason = "borrowed remainder observation preserves the owned durable commit identity"
    )]
    const fn observed(&self) -> Option<&CommitSha> {
        match self {
            Self::Absent => None,
            Self::Pending(commit) | Self::Accepted(commit) => Some(commit),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "phase", rename_all = "snake_case", deny_unknown_fields)]
/// Durable mutation intent and only the facts required by that recovery phase.
enum Phase {
    /// Native terminal state verified before owned journal and lease cleanup.
    Closing {
        /// Exact-owned capsule retaining all published phase objects.
        lease: CommitSha,
        /// Verified terminal result awaiting owned cleanup.
        outcome: Outcome,
    },
    /// Pinned selection source awaiting native rebase opening or initial reset.
    Opening {
        /// Original selected seam preceding replayed descendants.
        anchor: CommitSha,
        /// Parent boundary preceding the remaining selected change.
        base: Option<CommitSha>,
        /// Exact detached HEAD admitted for this selection.
        head: CommitSha,
        /// Exact-owned capsule retaining all published phase objects.
        lease: CommitSha,
        /// Combined selected source with original tip metadata.
        source: CommitSha,
    },
    /// Branch-reachable input admitted before synthetic objects are constructed.
    Preparing {
        /// Parent boundary preceding the remaining selected change.
        base: Option<CommitSha>,
        /// Owned reachability capsule observed before preparation.
        previous_lease: Option<CommitSha>,
        /// Branch-reachable source tip or verified replay tip.
        tip: CommitSha,
    },
    /// Validated atom and remainder awaiting ordered descendant replay.
    Replaying {
        /// Exact commits already admitted in the replay corridor.
        accepted: Vec<CommitSha>,
        /// Original selected seam preceding replayed descendants.
        anchor: CommitSha,
        /// Independently validated selected commit.
        atom: CommitSha,
        /// Parent boundary preceding the remaining selected change.
        base: Option<CommitSha>,
        /// Exact detached HEAD admitted for this selection.
        head: CommitSha,
        /// Exact-owned capsule retaining all published phase objects.
        lease: CommitSha,
        /// Constructed or independently accepted remainder, absent when exhausted.
        remainder: Remainder,
        /// Combined selected source with original tip metadata.
        source: CommitSha,
    },
    /// A detached selection with the original source exposed unstaged.
    Selecting {
        /// Original selected seam preceding replayed descendants.
        anchor: CommitSha,
        /// Parent boundary preceding the remaining selected change.
        base: Option<CommitSha>,
        /// Exact detached HEAD admitted for this selection.
        head: CommitSha,
        /// Exact-owned capsule retaining all published phase objects.
        lease: CommitSha,
        /// Combined selected source with original tip metadata.
        source: CommitSha,
    },
    /// Final-tree and gate witness persisted before native rebase completion.
    Verified {
        /// Original selected seam preceding replayed descendants.
        anchor: CommitSha,
        /// Independently validated selected commit.
        atom: CommitSha,
        /// Parent boundary preceding the remaining selected change.
        base: Option<CommitSha>,
        /// Exact-owned capsule retaining all published phase objects.
        lease: CommitSha,
        /// Constructed or independently accepted remainder, absent when exhausted.
        remainder: Remainder,
        /// Branch-reachable source tip or verified replay tip.
        tip: CommitSha,
    },
}
#[derive(Clone, Debug, Serialize)]
/// Admitted session facts; derived command hashes are bound during ingress.
struct Journal {
    /// Checked current branch reference captured at initial admission.
    branch: BranchRef,
    /// Last durably completed branch tip.
    checkpoint: CommitSha,
    /// Fixed original branch-tip tree that every captured round must preserve.
    final_tree: TreeHash,
    /// Checked journal format identity.
    format: Format,
    /// Ordered admitted tree-only gate specifications.
    gates: gate::GateSet,
    /// Initial selection parent, absent for a parentless root range.
    original_base: Option<CommitSha>,
    /// Original branch-reachable selected tip used for stable source metadata.
    original_tip: CommitSha,
    /// Immutable opaque identity shared by owned recovery artifacts.
    session: SessionId,
    /// Phase-specific durable intent.
    state: Phase,
}

/// Untrusted persisted facts; gate hashes are derived only after parsing specs.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredJournal {
    /// Checked current branch reference captured at initial admission.
    branch: BranchRef,
    /// Last durably completed branch tip.
    checkpoint: CommitSha,
    /// Fixed original branch-tip tree that every captured round must preserve.
    final_tree: TreeHash,
    /// Checked journal format identity.
    format: Format,
    /// Ordered admitted tree-only gate specifications.
    gates: Vec<gate::GateSpec>,
    /// Initial selection parent, absent for a parentless root range.
    original_base: Option<CommitSha>,
    /// Original branch-reachable selected tip used for stable source metadata.
    original_tip: CommitSha,
    /// Immutable opaque identity shared by owned recovery artifacts.
    session: SessionId,
    /// Phase-specific durable intent.
    state: Phase,
}
#[derive(Clone, Copy)]
/// Public operation that owns the eventual normalized response.
enum Invocation {
    /// Submit or resume the current remaining selection.
    Continue,
    /// Submit the complete remaining source with its original message.
    Finish,
    /// Begin the initial selected range.
    Start,
}
impl Invocation {
    /// Preserves the actual initiating command when an already-completed round is cleaned up.
    const fn checkpoint_completion(self) -> output::CheckpointCompletionOperation {
        match self {
            Self::Finish => output::CheckpointCompletionOperation::Finish,
            Self::Start | Self::Continue => output::CheckpointCompletionOperation::Continue,
        }
    }
    /// Selects the terminal JSON operation without storing a second mode flag.
    const fn completion(self) -> output::CompletionOperation {
        match self {
            Self::Finish => output::CompletionOperation::Finish,
            Self::Start | Self::Continue => output::CompletionOperation::Continue,
        }
    }
    /// Preserves the initiating public operation through replay and restart.
    const fn operation(self) -> output::CommandOperation {
        match self {
            Self::Start => output::CommandOperation::Start,
            Self::Continue => output::CommandOperation::Continue,
            Self::Finish => output::CommandOperation::Finish,
        }
    }

    /// Selects the open-pool JSON operation for the current invocation.
    const fn selection(self) -> output::SelectionOperation {
        match self {
            Self::Start => output::SelectionOperation::Start,
            Self::Continue | Self::Finish => output::SelectionOperation::Continue,
        }
    }
}

/// One consumed own callback in the exact original schedule; never persisted.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ReplaySlot {
    /// Original descendant at its fixed corridor position.
    Descendant(usize),
    /// Independently validated selected-change remainder.
    Remainder,
    /// Final-tree verification after the original corridor.
    Terminal,
}

/// Native observation separates callback admission from parent continuation.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ReplayPosition {
    /// Callback removed from todo after native launch.
    Consumed(ReplaySlot),
    /// Native pick or break precedes the next callback.
    Pending(Option<ReplaySlot>),
    /// Failed callback retained at the first actionable todo position.
    Retained(ReplaySlot),
}

impl ReplayPosition {
    /// Both consumed and rescheduled callbacks retain the same original slot.
    const fn consumed(self) -> Option<ReplaySlot> {
        match self {
            Self::Pending(_) => None,
            Self::Consumed(slot) | Self::Retained(slot) => Some(slot),
        }
    }
}

/// One full transcript observation binds progress and future native checkout identities.
struct ReplayObservation {
    /// Original source objects still awaiting native checkout.
    pending_picks: Vec<CommitSha>,
    /// Callback frontier proven by the complete native transcript.
    position: ReplayPosition,
}

/// One short-lived action in the existing native schedule.
#[derive(Clone, PartialEq, Eq)]
enum ReplayAction {
    /// Owned break exposing the combined selected change.
    Break,
    /// Exact generated callback bound to its source slot.
    Exec(ReplaySlot),
    /// Original source object admitted by native pick identity.
    Pick(CommitSha),
}

/// Three native source seams, derived only after the full schedule is proved.
#[derive(Clone, Copy, PartialEq, Eq)]
enum OpeningPosition {
    /// Native source pick awaits completion before the break.
    FinishSource,
    /// Source pick completed and the owned break was consumed.
    ReadyBreak,
    /// Native source pick was rescheduled before completion.
    RetrySource,
}

/// Non-authorizing checkpoint facts used only by native-query-free diagnostics.
#[derive(Debug, Eq, PartialEq)]
pub(in crate::git_factor) struct JournalFacts {
    /// Last recorded completed branch tip; observation does not admit ownership.
    checkpoint: CommitSha,
    /// Recorded immutable final tree.
    final_tree: TreeHash,
    /// Recorded durable transition name.
    phase: &'static str,
    /// Combined source only where the phase records that fact.
    source: Option<CommitSha>,
}

impl JournalFacts {
    /// Returns the recorded completed checkpoint without admitting it.
    pub(in crate::git_factor) const fn checkpoint(&self) -> &CommitSha {
        &self.checkpoint
    }
    /// Returns the recorded final tree without validating current Git state.
    pub(in crate::git_factor) const fn final_tree(&self) -> &TreeHash {
        &self.final_tree
    }
    /// Returns the recorded transition name.
    pub(in crate::git_factor) const fn phase(&self) -> &'static str {
        self.phase
    }
    /// Returns the source recorded by the applicable transition.
    pub(in crate::git_factor) const fn source(&self) -> Option<&CommitSha> {
        self.source.as_ref()
    }
}

/// Require a released native Git dialect before observing or changing checkpoint authority.
fn require_supported_git(ctx: &Ctx<'_>) -> Result<(), FactorError> {
    // Compatibility is a prerequisite: traced queries and session diagnostics need
    // an admitted native dialect, so this first observation uses the Runner directly.
    let observed = ctx
        .runner
        .output("git", &["version"], &[], &ctx.cwd)
        .map_err(|error| {
            FactorError::PrerequisiteObservation(non_empty_msg(format!("git version: {error}")))
        })?;
    if !observed.status.success() {
        let stderr = String::from_utf8_lossy(&observed.stderr);
        let diagnostic = stderr.trim();
        let message = if diagnostic.is_empty() {
            observed.status.to_string()
        } else {
            diagnostic.to_owned()
        };
        return Err(FactorError::PrerequisiteObservation(non_empty_msg(message)));
    }
    let output = String::from_utf8_lossy(&observed.stdout).trim().to_owned();
    let token = output.strip_prefix("git version ");
    let supported = token.and_then(|version| {
        let mut fields = version.split('.');
        let numeric = |field: &str| {
            if !field.is_empty() && field.bytes().all(|byte| byte.is_ascii_digit()) {
                field.parse::<u64>().ok()
            } else {
                None
            }
        };
        let major = numeric(fields.next()?)?;
        let minor = numeric(fields.next()?)?;
        let patch = numeric(fields.next()?)?;
        let released = fields.next().is_none();
        released.then(|| (major, minor, patch) >= (2, 56, 0))
    });
    if supported == Some(true) {
        return Ok(());
    }
    Err(FactorError::Usage(non_empty_msg(format!(
        "Git 2.56.0 or newer released Git is required; observed {output}"
    ))))
}

/// Constructs a contextual refusal without changing native Git state.
fn failure(text: &str) -> FactorError {
    FactorError::GitCommand(non_empty_msg(text.to_owned()))
}
/// Reads native branch identity without trimming valid reference bytes.
fn head_reference(ctx: &Ctx<'_>) -> Result<String, FactorError> {
    let output = git_raw_output(ctx, &["rev-parse", "--symbolic-full-name", "HEAD"])?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let diagnostic = stderr.trim();
        let message = if diagnostic.is_empty() {
            output.status.to_string()
        } else {
            diagnostic.to_owned()
        };
        return Err(FactorError::GitCommand(non_empty_msg(message)));
    }
    let reference = String::from_utf8(output.stdout)
        .map_err(|error| failure(&format!("native branch identity is not UTF-8: {error}")))?;
    Ok(reference
        .strip_suffix('\n')
        .unwrap_or(&reference)
        .to_owned())
}
/// Admits an observed commit identifier at its checked boundary.
fn sha(value: impl AsRef<str>) -> Result<CommitSha, FactorError> {
    CommitSha::new(value.as_ref().to_owned())
}
/// Reads and admits the tree identifier for a commit or treeish.
fn tree(ctx: &Ctx<'_>, value: impl AsRef<str>) -> Result<TreeHash, FactorError> {
    TreeHash::new(&git_output(
        ctx,
        &["rev-parse", &format!("{}^{{tree}}", value.as_ref())],
    )?)
}
/// Runs a Git query whose result must be a checked commit identifier.
fn commit_output(ctx: &Ctx<'_>, args: &[&str]) -> Result<CommitSha, FactorError> {
    CommitSha::new(git_output(ctx, args)?)
}
/// Reads exact parent metadata for ancestry admission.
fn parents(ctx: &Ctx<'_>, value: impl AsRef<str>) -> Result<String, FactorError> {
    git_output(ctx, &["show", "--format=%P", "--no-patch", value.as_ref()])
}
/// Locates this worktree’s owned session state directory.
fn state_dir(ctx: &Ctx<'_>) -> Result<PathBuf, FactorError> {
    Ok(git_dir_in(ctx)?.join("factor"))
}
/// Locates the sole authoritative durable journal.
fn journal_path(ctx: &Ctx<'_>) -> Result<PathBuf, FactorError> {
    Ok(git_dir_in(ctx)?.join("factor-journal.json"))
}

/// Observes foreign path occupancy without following a dangling symbolic link.
fn path_present(ctx: &Ctx<'_>, path: &Path) -> Result<bool, FactorError> {
    match ctx.fs.symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(FactorError::StateRead(error)),
    }
}

/// Pins every published object before atomically replacing the sole journal.
fn save(ctx: &Ctx<'_>, journal: &mut Journal) -> Result<(), FactorError> {
    if !matches!(journal.state, Phase::Preparing { .. }) {
        let current = pin_objects(ctx, journal, &[])?;
        set_lease(journal, current);
    }
    let content = serde_json::to_string(journal)
        .map_err(|error| failure(&format!("cannot serialize checkpoint journal: {error}")))?;
    ctx.fs
        .write_atomic_string(&journal_path(ctx)?, &content)
        .map_err(FactorError::StateWrite)?;
    ctx.fs
        .create_dir_all(&state_dir(ctx)?)
        .map_err(FactorError::StateWrite)?;
    Ok(())
}

/// Borrows the exact lease fact owned by the current durable phase.
#[expect(
    clippy::pattern_type_mismatch,
    reason = "borrowed phase facts preserve canonical journal ownership without cloning or a second representation"
)]
const fn lease(j: &Journal) -> Option<&CommitSha> {
    match &j.state {
        Phase::Preparing { previous_lease, .. } => previous_lease.as_ref(),
        Phase::Opening { lease, .. }
        | Phase::Selecting { lease, .. }
        | Phase::Replaying { lease, .. }
        | Phase::Verified { lease, .. }
        | Phase::Closing { lease, .. } => Some(lease),
    }
}
/// Replaces the phase’s reachability fact after publishing a new capsule.
#[expect(
    clippy::pattern_type_mismatch,
    reason = "borrowed phase facts preserve canonical journal ownership without cloning or a second representation"
)]
#[expect(
    clippy::single_call_fn,
    reason = "this named runtime boundary keeps the checkpoint transition readable"
)]
fn set_lease(j: &mut Journal, value: CommitSha) {
    match &mut j.state {
        Phase::Preparing { previous_lease, .. } => *previous_lease = Some(value),
        Phase::Opening { lease, .. }
        | Phase::Selecting { lease, .. }
        | Phase::Replaying { lease, .. }
        | Phase::Verified { lease, .. }
        | Phase::Closing { lease, .. } => *lease = value,
    }
}
/// Pins a combined source and advances only after admitted clean input.
fn prepare(ctx: &Ctx<'_>, mut j: Journal, invocation: Invocation) -> Result<i32, FactorError> {
    let Phase::Preparing { tip, base, .. } = j.state.clone() else {
        return Err(failure("preparing facts are missing"));
    };
    if is_mid_rebase_in(ctx) {
        return Err(failure("preparing intent cannot replace an active rebase"));
    }
    tracked_clean(ctx)?;
    admit_initial_untracked(ctx, base.as_ref(), &tip, &j.checkpoint)?;
    let source = metadata_commit(ctx, &tip, &tree(ctx, &tip)?, base.as_ref(), None)?;
    let head = if let Some(admitted_base) = base.as_ref() {
        admitted_base.clone()
    } else {
        metadata_commit(
            ctx,
            &tip,
            &TreeHash::new(EMPTY)?,
            None,
            Some("Temporary root selection\n"),
        )?
    };
    j.state = Phase::Opening {
        lease: pin_objects(ctx, &j, &[source.clone(), head.clone(), tip.clone()])?,
        source,
        base,
        head,
        anchor: tip,
    };
    save(ctx, &mut j)?;
    open(ctx, j, invocation)
}
/// Observes the repository-wide session lease with explicit missing-ref handling.
fn lease_current(ctx: &Ctx<'_>) -> Result<Option<CommitSha>, FactorError> {
    let output = git_raw_output(ctx, &["rev-parse", "--verify", "--quiet", LEASE])?;
    if output.status.success() {
        let value = String::from_utf8(output.stdout)
            .map_err(|error| failure(&format!("lease identity is not UTF-8: {error}")))?
            .trim()
            .to_owned();
        return Ok(Some(sha(&value)?));
    }
    if super::status_code(output.status) == GIT_FALSE
        && output.stdout.is_empty()
        && output.stderr.is_empty()
    {
        return Ok(None);
    }
    Err(failure("cannot observe owned session lease"))
}
/// Requires the lease capsule to belong to this immutable session identity.
fn owned_lease(ctx: &Ctx<'_>, j: &Journal, current: &CommitSha) -> Result<(), FactorError> {
    if git_output(
        ctx,
        &["show", "--format=%B", "--no-patch", current.as_str()],
    )? != format!("git-factor session lease {}", j.session)
    {
        return Err(failure("session lease was replaced by an unknown object"));
    }
    if let Some(previous) = lease(j) {
        ancestor(ctx, previous, current)?;
    }
    Ok(())
}

/// Distinguishes an owned terminal lease from a well-formed later session capsule.
fn closing_lease_is_owned(
    ctx: &Ctx<'_>,
    j: &Journal,
    current: &CommitSha,
) -> Result<bool, FactorError> {
    let object = git_raw_output(ctx, &["cat-file", "commit", current.as_str()])?;
    if !object.status.success()
        || tree(ctx, current)? != TreeHash::new(EMPTY)?
        || parents(ctx, current)?.is_empty()
    {
        return Err(failure("closing lease is not an available session capsule"));
    }
    let text = String::from_utf8(object.stdout)
        .map_err(|error| failure(&format!("closing lease is not UTF-8: {error}")))?;
    let session = text
        .split_once("\n\n")
        .and_then(|(_, body)| body.strip_suffix('\n'))
        .and_then(|body| body.strip_prefix("git-factor session lease "))
        .ok_or_else(|| failure("closing lease has malformed session ownership"))?;
    let observed = SessionId::new(session.to_owned())?;
    Ok(observed == j.session)
}

/// Deletes only the exact owned lease; a later session's valid capsule is preserved.
#[expect(
    clippy::single_call_fn,
    reason = "one named terminal ownership boundary keeps foreign-session observation distinct from native deletion"
)]
fn cleanup_lease(ctx: &Ctx<'_>, j: &Journal) -> Result<Option<CommitSha>, FactorError> {
    let Some(current) = lease_current(ctx)? else {
        return Ok(None);
    };
    if !closing_lease_is_owned(ctx, j, &current)? {
        return Ok(None);
    }
    owned_lease(ctx, j, &current)?;
    Ok(Some(current))
}
/// Enumerates all persisted commit facts that must remain owned and GC-reachable.
#[expect(
    clippy::pattern_type_mismatch,
    reason = "borrowed phase facts preserve canonical journal ownership without cloning or a second representation"
)]
fn phase_objects(j: &Journal) -> BTreeSet<CommitSha> {
    let mut objects = BTreeSet::from([j.checkpoint.clone()]);
    if !matches!(j.state, Phase::Closing { .. }) {
        objects.insert(j.original_tip.clone());
    }
    objects.extend(j.original_base.iter().cloned());
    match &j.state {
        Phase::Preparing { tip, base, .. } => {
            objects.insert(tip.clone());
            if let Some(admitted_base) = base.as_ref() {
                objects.insert(admitted_base.clone());
            }
        }
        Phase::Opening {
            source,
            head,
            anchor,
            ..
        } => {
            objects.extend([source.clone(), head.clone(), anchor.clone()]);
        }
        Phase::Selecting {
            source,
            head,
            anchor,
            base,
            ..
        } => {
            objects.extend([source.clone(), head.clone(), anchor.clone()]);
            objects.extend(base.iter().cloned());
        }
        Phase::Replaying {
            source,
            head,
            atom,
            remainder,
            accepted,
            anchor,
            base,
            ..
        } => {
            objects.extend([source.clone(), head.clone(), atom.clone(), anchor.clone()]);
            objects.extend(base.iter().cloned());
            objects.extend(accepted.iter().cloned());
            if let Some(admitted_remainder) = remainder.observed() {
                objects.insert(admitted_remainder.clone());
            }
        }
        Phase::Verified {
            atom,
            remainder,
            tip,
            anchor,
            base,
            ..
        } => {
            objects.extend([atom.clone(), tip.clone(), anchor.clone()]);
            objects.extend(base.iter().cloned());
            if let Some(admitted_remainder) = remainder.observed() {
                objects.insert(admitted_remainder.clone());
            }
        }
        Phase::Closing { outcome, .. } => match outcome {
            Outcome::Complete { atom } => {
                objects.insert(atom.clone());
            }
            Outcome::Aborted { base } => objects.extend(base.iter().cloned()),
        },
    }
    objects
}

/// Pins phase objects and previous capsules before publishing their identifiers.
fn pin_objects(ctx: &Ctx<'_>, j: &Journal, extra: &[CommitSha]) -> Result<CommitSha, FactorError> {
    let current = lease_current(ctx)?;
    if lease(j).is_none() {
        if let Some(observed) = current.as_ref() {
            owned_lease(ctx, j, observed)?;
        }
    } else {
        owned_lease(
            ctx,
            j,
            current
                .as_ref()
                .ok_or_else(|| failure("session lease disappeared before journal update"))?,
        )?;
    }
    let mut objects = phase_objects(j);
    objects.extend(current.iter().cloned());
    objects.extend(extra.iter().cloned());
    let message = format!("git-factor session lease {}", j.session);
    let mut args = vec!["commit-tree", EMPTY, "-m", &message];
    for object in &objects {
        args.extend(["-p", object.as_str()]);
    }
    let capsule = commit_output(ctx, &args)?;
    run_git(
        ctx,
        &[
            "-c",
            "core.fsync=reference",
            "update-ref",
            LEASE,
            capsule.as_str(),
            current.as_ref().map_or(
                "0000000000000000000000000000000000000000",
                CommitSha::as_str,
            ),
        ],
    )?;
    Ok(capsule)
}
/// Persists terminal intent before deleting exact-owned recovery artifacts.
#[expect(
    clippy::pattern_type_mismatch,
    reason = "borrowed journal and transcript facts use ordinary reference matching without cloning identities or ref-pattern scaffolding"
)]
fn cleanup(
    ctx: &Ctx<'_>,
    mut j: Journal,
    requested_outcome: Outcome,
    operation: output::CheckpointCompletionOperation,
) -> Result<i32, FactorError> {
    let outcome = match &j.state {
        Phase::Closing { outcome, .. } => outcome.clone(),
        &Phase::Preparing { .. }
        | &Phase::Opening { .. }
        | &Phase::Selecting { .. }
        | &Phase::Replaying { .. }
        | &Phase::Verified { .. } => requested_outcome,
    };
    let response = closing_outcome(ctx, &outcome, &j)?;
    if !matches!(j.state, Phase::Closing { .. }) {
        j.state = Phase::Closing {
            lease: match lease(&j) {
                Some(current) => current.clone(),
                None => pin_objects(ctx, &j, &[])?,
            },
            outcome,
        };
        save(ctx, &mut j)?;
    }
    if let Some(current) = cleanup_lease(ctx, &j)? {
        run_git(
            ctx,
            &[
                "-c",
                "core.fsync=reference",
                "update-ref",
                "-d",
                LEASE,
                current.as_str(),
            ],
        )?;
    }
    if path_present(ctx, &state_dir(ctx)?)? {
        ctx.fs
            .remove_dir_all(&state_dir(ctx)?)
            .map_err(FactorError::StateWrite)?;
    }
    ctx.fs
        .remove_atomic_file(&journal_path(ctx)?)
        .map_err(FactorError::StateWrite)?;
    match response {
        output::ClosingOutcome::Complete(count) => {
            output::checkpoint_completed(ctx, operation, count)?;
        }
        output::ClosingOutcome::Aborted(_) => output::aborted(ctx, false)?,
    }
    Ok(0)
}
/// Parses and refines every durable fact before any recovery cleanup.
fn load(ctx: &Ctx<'_>) -> Result<Journal, FactorError> {
    load_for(ctx, Admission::Resume)
}

/// Loads recovery facts under the current command's callback authority.
fn load_for(ctx: &Ctx<'_>, admission: Admission) -> Result<Journal, FactorError> {
    let path = journal_path(ctx)?;
    if !path_present(ctx, &path)? && path_present(ctx, &state_dir(ctx)?)? {
        return Err(failure(
            "existing legacy session must be finished or aborted with its originating version",
        ));
    }
    let journal_metadata = ctx
        .fs
        .symlink_metadata(&path)
        .map_err(FactorError::StateRead)?;
    if !journal_metadata.is_file() || journal_metadata.file_type().is_symlink() {
        return Err(failure(
            "canonical checkpoint journal is not an owned regular file",
        ));
    }
    let scratch = state_dir(ctx)?;
    if path_present(ctx, &scratch)? {
        let scratch_metadata = ctx
            .fs
            .symlink_metadata(&scratch)
            .map_err(FactorError::StateRead)?;
        if !scratch_metadata.is_dir() || scratch_metadata.file_type().is_symlink() {
            return Err(failure("checkpoint scratch was replaced by a foreign path"));
        }
    }
    let bytes = ctx
        .fs
        .read_to_string(&path)
        .map_err(FactorError::StateRead)?;
    let stored: StoredJournal = serde_json::from_str(&bytes).map_err(|error| failure(&format!("unsupported or corrupt checkpoint journal; legacy sessions require their originating version: {error}")))?;
    let journal = Journal {
        format: stored.format,
        session: stored.session,
        original_tip: stored.original_tip,
        branch: stored.branch,
        checkpoint: stored.checkpoint,
        final_tree: stored.final_tree,
        original_base: stored.original_base,
        gates: gate::bind(ctx, stored.gates)?,
        state: stored.state,
    };
    if !matches!(journal.state, Phase::Preparing { .. }) && lease(&journal).is_none() {
        return Err(failure("checkpoint journal lacks its owned lease"));
    }
    validate_for(ctx, &journal, admission)?;
    if matches!(admission, Admission::Abort) && is_mid_rebase_in(ctx) {
        let _owned_schedule = replay_position_for(ctx, &journal, admission)?;
    }
    Ok(journal)
}

/// Projects parsed journal metadata without native queries, admission or cleanup.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "diagnostic projection must remain independent of authorizing session load"
    )
)]
pub(in crate::git_factor) fn journal_snapshot(
    ctx: &Ctx<'_>,
    git_dir: &Path,
) -> Option<JournalFacts> {
    let contents = ctx
        .fs
        .read_to_string(&git_dir.join("factor-journal.json"))
        .ok()?;
    let stored: StoredJournal = serde_json::from_str(&contents).ok()?;
    let (phase, source) = match stored.state {
        Phase::Closing { .. } => ("closing", None),
        Phase::Opening { source, .. } => ("opening", Some(source)),
        Phase::Preparing { .. } => ("preparing", None),
        Phase::Replaying { source, .. } => ("replaying", Some(source)),
        Phase::Selecting { source, .. } => ("selecting", Some(source)),
        Phase::Verified { .. } => ("verified", None),
    };
    Some(JournalFacts {
        checkpoint: stored.checkpoint,
        final_tree: stored.final_tree,
        phase,
        source,
    })
}

/// Admits the actual current session before best-effort diagnostic scratch writes.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "the actual root diagnostic consumer must admit authority before best-effort scratch writes"
    )
)]
pub(in crate::git_factor) fn error_log_directory(ctx: &Ctx<'_>) -> Option<StateDir> {
    let _journal = load(ctx).ok()?;
    let scratch = state_dir(ctx).ok()?;
    let metadata = ctx.fs.symlink_metadata(&scratch).ok()?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return None;
    }
    Some(StateDir::new(scratch))
}
/// Reclaims exact owned scratch only for an admitted mutating recovery command.
fn recover(ctx: &Ctx<'_>) -> Result<Journal, FactorError> {
    recover_for(ctx, Admission::Resume)
}

/// Retains settlement semantics while applying abort-only executable tolerance.
fn recover_for(ctx: &Ctx<'_>, admission: Admission) -> Result<Journal, FactorError> {
    let journal = load_for(ctx, admission)?;
    if matches!(admission, Admission::Resume)
        && let Phase::Selecting { head, .. } = journal.state.clone()
        && (opening_position_for(ctx, &journal, admission)? != OpeningPosition::ReadyBreak
            || commit_output(ctx, &["rev-parse", "HEAD"])? != head)
    {
        return Err(failure(
            "selection has not retained its consumed source break",
        ));
    }
    settle_terminal_rebase(ctx, &journal, admission)?;
    if path_present(ctx, &state_dir(ctx)?)? {
        candidate::remove_owned_actors(ctx, &StateDir::new(state_dir(ctx)?), &journal.session)?;
    }
    Ok(journal)
}
/// Requires a persisted commit object to remain available.
fn object(ctx: &Ctx<'_>, value: &CommitSha) -> Result<(), FactorError> {
    let observed = git_raw_output(ctx, &["cat-file", "-e", &format!("{value}^{{commit}}")])?;
    if !observed.status.success() {
        return Err(failure("checkpoint journal names an unavailable commit"));
    }
    Ok(())
}
/// Observes the accepted normalized author tuple used by native commit reconstruction.
fn source_author(ctx: &Ctx<'_>, commit: &CommitSha) -> Result<Vec<u8>, FactorError> {
    candidate::author_metadata(ctx, commit.as_str()).map(String::into_bytes)
}

/// Requires observed ancestry, distinguishing false from failed observation.
fn ancestor(ctx: &Ctx<'_>, ancestor: &CommitSha, tip: &CommitSha) -> Result<(), FactorError> {
    let output = git_raw_output(
        ctx,
        &[
            "merge-base",
            "--is-ancestor",
            ancestor.as_str(),
            tip.as_str(),
        ],
    )?;
    if output.status.success() {
        return Ok(());
    }
    if super::status_code(output.status) == GIT_FALSE {
        return Err(FactorError::NotAncestor(ancestor.clone()));
    }
    Err(failure("cannot observe checkpoint ancestry"))
}

/// Applies transient callback identity without changing durable ownership facts.
#[expect(
    clippy::pattern_type_mismatch,
    reason = "borrowed phase facts preserve canonical journal ownership without cloning or a second representation"
)]
fn validate_for(ctx: &Ctx<'_>, j: &Journal, admission: Admission) -> Result<(), FactorError> {
    validate_ownership(ctx, j, admission)?;
    match &j.state {
        Phase::Opening { source, .. }
        | Phase::Selecting { source, .. }
        | Phase::Replaying { source, .. } => {
            if source_author(ctx, source)? != source_author(ctx, &j.original_tip)? {
                return Err(failure("combined source changed the original author"));
            }
        }
        Phase::Preparing { .. } | Phase::Verified { .. } | Phase::Closing { .. } => {}
    }
    validate_phase(ctx, j)
}

/// Admits Git's terminal interval after its branch write, before attachment or state removal.
#[expect(
    clippy::too_many_lines,
    reason = "one complete native terminal witness keeps the required branch, transcript, ownership and pending-side-effect guards reviewable together"
)]
#[expect(
    clippy::pattern_type_mismatch,
    reason = "borrowed journal and transcript facts use ordinary reference matching without cloning identities or ref-pattern scaffolding"
)]
fn native_terminal_finish(
    ctx: &Ctx<'_>,
    j: &Journal,
    admission: Admission,
) -> Result<bool, FactorError> {
    let Phase::Verified {
        anchor,
        atom,
        base,
        remainder,
        tip,
        ..
    } = &j.state
    else {
        return Ok(false);
    };
    if !is_mid_rebase_in(ctx) {
        return Ok(false);
    }
    let reference = head_reference(ctx)?;
    let branch_tip = commit_output(ctx, &["rev-parse", j.branch.as_str()])?;
    if reference == "HEAD" && branch_tip == j.checkpoint {
        // Verified is persisted before native finish; the branch write may still be pending.
        return Ok(false);
    }
    let directory = git_dir_in(ctx)?.join("rebase-merge");
    if !ctx.fs.is_dir(&directory)
        || ctx.fs.is_dir(&git_dir_in(ctx)?.join("rebase-apply"))
        || reference != "HEAD" && reference != j.branch.as_str()
        || commit_output(ctx, &["rev-parse", "HEAD"])? != *tip
        || branch_tip != *tip
        || tree(ctx, tip)? != j.final_tree
        || ctx
            .fs
            .read_to_string(&directory.join("head-name"))
            .map_err(FactorError::StateRead)?
            .trim_end_matches('\n')
            != j.branch.as_str()
        || ctx
            .fs
            .read_to_string(&directory.join("orig-head"))
            .map_err(FactorError::StateRead)?
            .trim_end_matches('\n')
            != j.checkpoint.as_str()
    {
        return Err(failure(
            "attached native replay lacks its exact terminal authority",
        ));
    }
    let todo = ctx
        .fs
        .read_to_string(&directory.join("git-rebase-todo"))
        .map_err(FactorError::StateRead)?;
    if todo
        .lines()
        .any(|line| !line.trim().is_empty() && !line.trim_start().starts_with('#'))
    {
        return Err(failure(
            "attached terminal replay still has pending commands",
        ));
    }
    let done = ctx
        .fs
        .read_to_string(&directory.join("done"))
        .map_err(FactorError::StateRead)?;
    let last = done
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty() && !line.trim_start().starts_with('#'));
    let terminal = format!("{} checkpoint-terminal", callback_program(ctx, admission)?);
    let terminal_action = last
        .map(|line| parse_replay_action(ctx, line, &[], &[(ReplaySlot::Terminal, terminal)]))
        .transpose()?
        .flatten();
    if terminal_action != Some(ReplayAction::Exec(ReplaySlot::Terminal)) {
        return Err(failure(
            "attached terminal replay lacks its final owned exec",
        ));
    }
    for pending in [
        "autostash",
        "update-refs",
        "refs-to-delete",
        "rewritten-pending",
    ] {
        if ctx.fs.exists(&directory.join(pending)) {
            return Err(failure(
                "attached terminal replay has pending native side effects",
            ));
        }
    }
    if parents(ctx, atom)? != base.as_ref().map_or("", CommitSha::as_str) {
        return Err(failure("terminal atom changed its captured predecessor"));
    }
    if let Some(remaining) = remainder.accepted()?
        && parents(ctx, remaining)? != atom.as_str()
    {
        return Err(failure("terminal remainder changed its accepted atom"));
    }
    let boundary = remainder.accepted()?.unwrap_or(atom);
    ancestor(ctx, boundary, tip)?;
    let corridor = descendants(ctx, boundary, tip)?;
    if corridor.len() != descendants(ctx, anchor, &j.checkpoint)?.len() {
        return Err(failure(
            "attached terminal replay changed its original corridor",
        ));
    }
    let mut previous = boundary;
    for current in &corridor {
        if parents(ctx, current)? != previous.as_str() {
            return Err(failure(
                "attached terminal replay is not the admitted linear corridor",
            ));
        }
        previous = current;
    }
    tracked_clean(ctx)?;
    Ok(true)
}

/// Forgets only completed native metadata; verified commits and user files remain untouched.
#[expect(
    clippy::single_call_fn,
    reason = "one named recovery boundary separates terminal native metadata cleanup from pure journal admission"
)]
#[expect(
    clippy::pattern_type_mismatch,
    reason = "borrowed journal and transcript facts use ordinary reference matching without cloning identities or ref-pattern scaffolding"
)]
fn settle_terminal_rebase(
    ctx: &Ctx<'_>,
    j: &Journal,
    admission: Admission,
) -> Result<(), FactorError> {
    let Phase::Verified { tip, .. } = &j.state else {
        return Ok(());
    };
    if is_mid_rebase_in(ctx) {
        if !native_terminal_finish(ctx, j, admission)? {
            return Ok(());
        }
        // Post-rewrite external side effects have no exactly-once guarantee after process death.
        // The accepted contract here is the durable tree and native commit-message validation.
        run_git(ctx, &["rebase", "--quit"])?;
    }
    if is_mid_rebase_in(ctx) {
        return Err(failure(
            "terminal native cleanup did not retain its owned attachment",
        ));
    }
    let reference = head_reference(ctx)?;
    if reference != "HEAD" && reference != j.branch.as_str()
        || commit_output(ctx, &["rev-parse", "HEAD"])? != *tip
        || commit_output(ctx, &["rev-parse", j.branch.as_str()])? != *tip
        || tree(ctx, tip)? != j.final_tree
    {
        return Err(failure(
            "terminal native cleanup changed its verified checkpoint",
        ));
    }
    tracked_clean(ctx)?;
    if reference == "HEAD" {
        run_git(ctx, &["symbolic-ref", "HEAD", j.branch.as_str()])?;
    }
    if head_reference(ctx)? != j.branch.as_str()
        || commit_output(ctx, &["rev-parse", "HEAD"])? != *tip
    {
        return Err(failure(
            "terminal native cleanup did not restore its owned branch",
        ));
    }
    tracked_clean(ctx)
}

/// Checks lease, branch, and native rebase authority before phase admission.
#[expect(
    clippy::single_call_fn,
    clippy::pattern_type_mismatch,
    reason = "a named ownership admission boundary retains canonical borrowed journal facts"
)]
fn validate_ownership(ctx: &Ctx<'_>, j: &Journal, admission: Admission) -> Result<(), FactorError> {
    match lease(j) {
        Some(_) => match lease_current(ctx)? {
            Some(current) if matches!(j.state, Phase::Closing { .. }) => {
                if closing_lease_is_owned(ctx, j, &current)? {
                    owned_lease(ctx, j, &current)?;
                    for object in phase_objects(j) {
                        ancestor(ctx, &object, &current)?;
                    }
                }
            }
            Some(current) => {
                owned_lease(ctx, j, &current)?;
                for object in phase_objects(j) {
                    ancestor(ctx, &object, &current)?;
                }
            }
            None if matches!(j.state, Phase::Closing { .. }) => {}
            None => return Err(failure("checkpoint object lease disappeared")),
        },
        None if matches!(j.state, Phase::Preparing { .. }) => {
            if let Some(current) = lease_current(ctx)? {
                owned_lease(ctx, j, &current)?;
                for object in phase_objects(j) {
                    ancestor(ctx, &object, &current)?;
                }
            } else {
                for object in phase_objects(j) {
                    ancestor(ctx, &object, &j.checkpoint)?;
                }
            }
        }
        None => return Err(failure("leased phase lacks its object lease")),
    }
    if !matches!(j.state, Phase::Closing { .. }) {
        object(ctx, &j.original_tip)?;
        if let Some(base) = j.original_base.as_ref() {
            ancestor(ctx, base, &j.original_tip)?;
        }
    }
    object(ctx, &j.checkpoint)?;
    if tree(ctx, &j.checkpoint)? != j.final_tree {
        return Err(failure("checkpoint does not preserve the fixed final tree"));
    }
    if !git_raw_output(ctx, &["check-ref-format", j.branch.as_str()])?
        .status
        .success()
        || !j.branch.as_str().starts_with("refs/heads/")
    {
        return Err(failure("invalid checkpoint branch"));
    }
    if let Some(base) = j.original_base.as_ref() {
        object(ctx, base)?;
        ancestor(ctx, base, &j.checkpoint)?;
    }
    let terminal_finish = native_terminal_finish(ctx, j, admission)?;
    if is_mid_rebase_in(ctx) {
        if !matches!(j.state, Phase::Opening { .. }) && !terminal_finish {
            require_detached(ctx)?;
        }
        let directory = git_dir_in(ctx)?.join("rebase-merge");
        let original = ctx
            .fs
            .read_to_string(&directory.join("orig-head"))
            .map_err(FactorError::StateRead)?;
        let branch = ctx
            .fs
            .read_to_string(&directory.join("head-name"))
            .map_err(FactorError::StateRead)?;
        if original.trim_end_matches('\n') != j.checkpoint.as_str()
            || branch.trim_end_matches('\n') != j.branch.as_str()
        {
            return Err(failure("active rebase belongs to another checkpoint"));
        }
    }
    let expected_branch = match &j.state {
        Phase::Verified { tip, .. } if !is_mid_rebase_in(ctx) || terminal_finish => tip,
        Phase::Preparing { .. }
        | Phase::Opening { .. }
        | Phase::Selecting { .. }
        | Phase::Replaying { .. }
        | Phase::Verified { .. }
        | Phase::Closing { .. } => &j.checkpoint,
    };
    if commit_output(ctx, &["rev-parse", j.branch.as_str()])? != *expected_branch {
        return Err(failure("checkpoint branch changed outside this session"));
    }
    Ok(())
}

/// Checks only the facts required by the admitted durable phase.
#[expect(
    clippy::single_call_fn,
    clippy::pattern_type_mismatch,
    reason = "a named phase admission boundary retains canonical borrowed journal facts"
)]
fn validate_phase(ctx: &Ctx<'_>, j: &Journal) -> Result<(), FactorError> {
    match &j.state {
        Phase::Preparing { .. } => validate_preparation(ctx, j)?,
        Phase::Opening {
            source,
            base,
            head,
            anchor,
            ..
        }
        | Phase::Selecting {
            source,
            base,
            head,
            anchor,
            ..
        } => {
            selection_facts(ctx, source, base.as_ref(), head, anchor)?;
            object(ctx, anchor)?;
            ancestor(ctx, anchor, &j.checkpoint)?;
        }
        Phase::Replaying { .. } => validate_replay(ctx, j)?,
        Phase::Verified {
            atom,
            remainder,
            tip,
            ..
        } => {
            object(ctx, atom)?;
            object(ctx, tip)?;
            if commit_output(ctx, &["rev-parse", "HEAD"])? != *tip {
                return Err(failure("terminal HEAD changed outside its verified tip"));
            }
            if !is_mid_rebase_in(ctx) {
                let reference = head_reference(ctx)?;
                if reference != "HEAD" && reference != j.branch.as_str() {
                    return Err(failure("completed terminal HEAD belongs to another branch"));
                }
                tracked_clean(ctx)?;
            }
            remainder.accepted()?;
            if tree(ctx, tip)? != j.final_tree {
                return Err(failure("terminal witness violates final-tree preservation"));
            }
            ancestor(ctx, atom, tip)?;
            if let Some(admitted_remainder) = remainder.observed() {
                object(ctx, admitted_remainder)?;
                ancestor(ctx, admitted_remainder, tip)?;
            }
        }
        Phase::Closing { outcome, .. } => {
            if is_mid_rebase_in(ctx)
                || commit_output(ctx, &["rev-parse", "HEAD"])? != j.checkpoint
                || head_reference(ctx)? != j.branch.as_str()
            {
                return Err(failure(
                    "closing requires the restored completed branch checkpoint",
                ));
            }
            let base = match outcome {
                Outcome::Complete { atom } => Some(atom),
                Outcome::Aborted { base } => base.as_ref(),
            };
            if let Some(admitted_base) = base.as_ref() {
                object(ctx, admitted_base)?;
                ancestor(ctx, admitted_base, &j.checkpoint)?;
                if let Some(original) = j.original_base.as_ref() {
                    ancestor(ctx, original, admitted_base)?;
                }
            }
            if base.is_none() && j.original_base.is_some() {
                return Err(failure("closing crossed the original parent seam"));
            }
            closing_outcome(ctx, outcome, j)?;
        }
    }
    Ok(())
}

/// Admits the durable preparing facts before their native transition.
#[expect(
    clippy::single_call_fn,
    clippy::pattern_type_mismatch,
    reason = "a named phase admission boundary preserves borrowed canonical journal facts"
)]
fn validate_preparation(ctx: &Ctx<'_>, j: &Journal) -> Result<(), FactorError> {
    let Phase::Preparing {
        tip,
        base,
        previous_lease,
    } = &j.state
    else {
        return Err(failure("phase admission called for another phase"));
    };

    if is_mid_rebase_in(ctx)
        || commit_output(ctx, &["rev-parse", "HEAD"])? != j.checkpoint
        || head_reference(ctx)? != j.branch.as_str()
    {
        return Err(failure("preparation checkpoint is no longer checked out"));
    }
    object(ctx, tip)?;
    ancestor(ctx, tip, &j.checkpoint)?;
    if previous_lease.is_none() && (tip != &j.original_tip || base != &j.original_base) {
        return Err(failure(
            "initial preparation changed its original selected range",
        ));
    }
    if source_author(ctx, tip)? != source_author(ctx, &j.original_tip)?
        || tree(ctx, tip)? != tree(ctx, &j.original_tip)?
    {
        return Err(failure(
            "preparation changed original selected source provenance",
        ));
    }
    if previous_lease.is_some() && parents(ctx, tip)? != base.as_ref().map_or("", CommitSha::as_str)
    {
        return Err(failure(
            "remaining preparation changed its captured atom boundary",
        ));
    }
    if let Some(admitted_base) = base.as_ref() {
        object(ctx, admitted_base)?;
        ancestor(ctx, admitted_base, tip)?;
        if let Some(original) = j.original_base.as_ref() {
            ancestor(ctx, original, admitted_base)?;
        }
    }
    if base.is_none() && j.original_base.is_some() {
        return Err(failure(
            "preparing selection crossed the original parent seam",
        ));
    }
    Ok(())
}

/// Admits the durable replaying facts before their native transition.
#[expect(
    clippy::single_call_fn,
    clippy::pattern_type_mismatch,
    reason = "a named phase admission boundary preserves borrowed canonical journal facts"
)]
fn validate_replay(ctx: &Ctx<'_>, j: &Journal) -> Result<(), FactorError> {
    let Phase::Replaying {
        source,
        base,
        head,
        atom,
        remainder,
        accepted,
        anchor,
        ..
    } = &j.state
    else {
        return Err(failure("phase admission called for another phase"));
    };

    object(ctx, anchor)?;
    ancestor(ctx, anchor, &j.checkpoint)?;
    if !accepted.is_empty() {
        remainder.accepted()?;
    }
    if accepted.len() > descendants(ctx, anchor, &j.checkpoint)?.len() {
        return Err(failure(
            "replay accepted more descendants than its original todo",
        ));
    }
    let mut previous = remainder.observed().unwrap_or(atom);
    for observed in accepted {
        object(ctx, observed)?;
        if parents(ctx, observed)? != previous.as_str() {
            return Err(failure("accepted replay corridor changed"));
        }
        previous = observed;
    }
    selection_facts(ctx, source, base.as_ref(), head, anchor)?;
    object(ctx, atom)?;
    if parents(ctx, atom)? != base.as_ref().map_or("", CommitSha::as_str) {
        return Err(failure("candidate parent does not match selection base"));
    }
    if let Some(admitted_remainder) = remainder.observed() {
        object(ctx, admitted_remainder)?;
        if parents(ctx, admitted_remainder)? != atom.as_str()
            || tree(ctx, admitted_remainder)? != tree(ctx, source)?
        {
            return Err(failure("remainder does not conserve the selected tree"));
        }
    }
    Ok(())
}
/// Requires combined source ancestry and the exact admitted selection boundary.
fn selection_facts(
    ctx: &Ctx<'_>,
    source: &CommitSha,
    base: Option<&CommitSha>,
    head: &CommitSha,
    anchor: &CommitSha,
) -> Result<(), FactorError> {
    object(ctx, source)?;
    object(ctx, head)?;
    object(ctx, anchor)?;
    if tree(ctx, source)? != tree(ctx, anchor)? {
        return Err(failure(
            "combined source differs from its branch-reachable anchor",
        ));
    }
    if parents(ctx, source)? != base.map_or("", CommitSha::as_str) {
        return Err(failure("combined selection has the wrong parent"));
    }
    if let Some(admitted_base) = base {
        object(ctx, admitted_base)?;
        if head != admitted_base {
            return Err(failure("selection HEAD differs from its parent"));
        }
    }
    if base.is_none() && (tree(ctx, head)?.as_str() != EMPTY || !parents(ctx, head)?.is_empty()) {
        return Err(failure("root selection sentinel is invalid"));
    }
    Ok(())
}

/// Creates a metadata-preserving synthetic commit without changing main HEAD.
fn metadata_commit(
    ctx: &Ctx<'_>,
    source: &CommitSha,
    selected_tree: &TreeHash,
    parent: Option<&CommitSha>,
    message: Option<&str>,
) -> Result<CommitSha, FactorError> {
    let body = if let Some(admitted_message) = message {
        admitted_message.to_owned()
    } else {
        let object = git_raw_output(ctx, &["cat-file", "commit", source.as_str()])?;
        if !object.status.success() {
            return Err(failure("cannot observe source message"));
        }
        let message_object = String::from_utf8(object.stdout)
            .map_err(|error| failure(&format!("source message is not UTF-8: {error}")))?;
        message_object
            .split_once("\n\n")
            .ok_or_else(|| failure("source message boundary missing"))?
            .1
            .to_owned()
    };
    let metadata = candidate::author_metadata(ctx, source.as_str())?;
    let fields = metadata
        .strip_suffix('\n')
        .unwrap_or(&metadata)
        .split('\0')
        .collect::<Vec<_>>();
    let &[name, email, date] = fields.as_slice() else {
        return Err(failure("invalid source author tuple"));
    };
    let file = NamedTempFile::new().map_err(FactorError::StateWrite)?;
    ctx.fs
        .write_string(file.path(), &body)
        .map_err(FactorError::StateWrite)?;
    let path = file
        .path()
        .to_str()
        .ok_or_else(|| failure("message path is not UTF-8"))?;
    let mut args = vec!["commit-tree", selected_tree.as_str(), "-F", path];
    if let Some(admitted_parent) = parent {
        args.extend(["-p", admitted_parent.as_str()]);
    }
    let native_date = format!("@{date}");
    let output = super::command_output_with(
        ctx,
        "git",
        &args,
        &[
            ("GIT_AUTHOR_NAME", Some(name)),
            ("GIT_AUTHOR_EMAIL", Some(email)),
            ("GIT_AUTHOR_DATE", Some(native_date.as_str())),
        ],
    )?;
    if !output.status.success() {
        return Err(failure("cannot construct checkpoint object"));
    }
    let result = String::from_utf8(output.stdout)
        .map_err(|error| failure(&format!("object identity is not UTF-8: {error}")))?
        .trim()
        .to_owned();
    sha(&result)
}
/// Enumerates the original replay corridor in chronological order.
fn descendants(
    ctx: &Ctx<'_>,
    anchor: &CommitSha,
    tip: &CommitSha,
) -> Result<Vec<CommitSha>, FactorError> {
    let range = format!("{anchor}..{tip}");
    if !git_output(ctx, &["rev-list", "--min-parents=2", &range])?.is_empty() {
        return Err(failure(
            "merge descendants cannot be replayed as a linear checkpoint corridor",
        ));
    }
    git_output(ctx, &["rev-list", "--reverse", &range])?
        .lines()
        .map(sha)
        .collect::<Result<Vec<_>, _>>()
}
/// Resolves the executable used by native rebase callbacks.
fn exe(ctx: &Ctx<'_>) -> Result<String, FactorError> {
    let observed = ctx.env.current_exe().map_err(FactorError::Io)?;
    let canonical = ctx
        .fs
        .canonicalize(&observed)
        .map_err(FactorError::Io)?
        .to_str()
        .map(str::to_owned)
        .ok_or_else(|| failure("executable path is not UTF-8"))?;
    if canonical.contains('\n') {
        return Err(failure("executable path contains a newline"));
    }
    Ok(canonical)
}
/// Builds a quoted internal callback command for native Git.
fn hidden(ctx: &Ctx<'_>, command: &str) -> Result<String, FactorError> {
    Ok(format!("{} checkpoint-{command}", shell_quote(&exe(ctx)?)))
}
/// Replaces the admitted native todo with combined selection and ordered gates.
#[expect(
    clippy::pattern_type_mismatch,
    reason = "borrowed phase facts preserve canonical journal ownership without cloning or a second representation"
)]
#[expect(
    clippy::single_call_fn,
    reason = "this named runtime boundary keeps the checkpoint transition readable"
)]
fn editor(ctx: &Ctx<'_>, path: &str, j: &Journal) -> Result<(), FactorError> {
    let Phase::Opening { source, anchor, .. } = &j.state else {
        return Err(failure("editor requires opening intent"));
    };
    let mut todo = format!(
        "pick {source}\nbreak\nexec {}\n",
        hidden(ctx, "gate-remainder")?
    );
    for descendant in descendants(ctx, anchor, &j.checkpoint)? {
        #[expect(
            clippy::expect_used,
            clippy::unwrap_in_result,
            reason = "formatting into an owned String cannot fail"
        )]
        writeln!(
            todo,
            "pick {descendant}\nexec {}",
            hidden(ctx, "gate-descendant")?
        )
        .expect("String formatting is infallible");
    }
    #[expect(
        clippy::expect_used,
        clippy::unwrap_in_result,
        reason = "formatting into an owned String cannot fail"
    )]
    writeln!(todo, "exec {}", hidden(ctx, "terminal")?).expect("String formatting is infallible");
    ctx.fs
        .write_string(Path::new(path), &todo)
        .map_err(FactorError::StateWrite)
}
/// Emits selection facts after the durable phase is established.
#[expect(
    clippy::pattern_type_mismatch,
    reason = "borrowed phase facts preserve canonical journal ownership without cloning or a second representation"
)]
fn report_selection(
    ctx: &Ctx<'_>,
    j: &Journal,
    invocation: Invocation,
) -> Result<i32, FactorError> {
    let Phase::Selecting { base, .. } = &j.state else {
        return Err(failure("selection output lacks admitted pool"));
    };
    let completed = split_count(ctx, base.as_ref(), j)?;
    if let Some(count) = NonZeroU64::new(completed) {
        output::checkpoint_remaining(ctx, output::CheckpointTransition::Committed(count))?;
    } else {
        let range = j.original_base.as_ref().map_or_else(
            || j.original_tip.to_string(),
            |original| format!("{original}..{}", j.original_tip),
        );
        let count = git_output(ctx, &["rev-list", "--count", &range])?
            .parse::<NonZeroUsize>()
            .map_err(|error| failure(&format!("initial range count is invalid: {error}")))?;
        let message = super::commit_message(ctx, &j.original_tip)?;
        let short = NonEmptyString::try_from(git_output(
            ctx,
            &["rev-parse", "--short", j.original_tip.as_str()],
        )?)
        .map_err(|_error| failure("empty short SHA"))?;
        output::started_checkpoint(
            ctx,
            &j.original_tip,
            count,
            invocation.selection(),
            &message,
            short.as_str(),
        )?;
    }
    Ok(0)
}

/// Derive the real staged tree while leaving its cache-tree/stat bytes untouched.
fn staged_tree(ctx: &Ctx<'_>, purpose: StagedPurpose<'_>) -> Result<TreeHash, FactorError> {
    let scratch = TempDir::new().map_err(FactorError::StateWrite)?;
    let index = scratch.path().join("index");
    let path = index
        .to_str()
        .ok_or_else(|| failure("staged observation index is not UTF-8"))?;
    let environment = [
        ("GIT_INDEX_FILE", Some(path)),
        ("GIT_OPTIONAL_LOCKS", Some("0")),
    ];
    let empty = super::command_output_with(
        ctx,
        "git",
        &["-c", "core.splitIndex=false", "read-tree", "--empty"],
        &environment,
    )?;
    if !empty.status.success() {
        return Err(failure("cannot initialize staged observation index"));
    }
    let command = format!(
        "set -o pipefail; git ls-files --full-name --stage -z -- :/ | GIT_INDEX_FILE={} git -c core.splitIndex=false update-index -z --index-info",
        shell_quote(path)
    );
    let populated = super::command_output_with(
        ctx,
        "bash",
        &["-c", &command],
        &[("GIT_OPTIONAL_LOCKS", Some("0"))],
    )?;
    if !populated.status.success() {
        return Err(failure("cannot observe real staged entries"));
    }
    let written = super::command_output_with(
        ctx,
        "git",
        &["-c", "core.splitIndex=false", "write-tree"],
        &environment,
    )?;
    if !written.status.success() {
        return Err(failure(
            "cannot observe staged tree with unresolved entries",
        ));
    }
    let reconstructed = TreeHash::new(
        String::from_utf8(written.stdout)
            .map_err(|error| failure(&format!("staged tree identity is not UTF-8: {error}")))?
            .trim(),
    )?;
    // Reconstruction loses intent-to-add flags. Native cached deletion semantics
    // identify those entries even when their paths already exist in HEAD.
    let intention = super::command_output_with(
        ctx,
        "git",
        &[
            "diff",
            "--cached",
            "--name-only",
            "--no-renames",
            "--no-relative",
            "--diff-filter=D",
            "-z",
            "--ita-invisible-in-index",
            reconstructed.as_str(),
            "--",
            ":/",
        ],
        &[("GIT_OPTIONAL_LOCKS", Some("0"))],
    )?;
    if !intention.status.success() {
        return Err(failure("cannot observe native intent-to-add entries"));
    }
    if intention.stdout.is_empty() {
        return Ok(reconstructed);
    }
    admit_staged_intentions(ctx, purpose, &intention.stdout)?;
    project_staged_intentions(ctx, scratch.path(), path, &intention.stdout)
}

/// Project only admitted paths at the lossless native root, then check native equivalence.
#[expect(
    clippy::single_call_fn,
    reason = "one native projection boundary keeps exact input, routing and checked commit-tree postcondition together"
)]
fn project_staged_intentions(
    ctx: &Ctx<'_>,
    scratch: &Path,
    index: &str,
    intention: &[u8],
) -> Result<TreeHash, FactorError> {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt as _;
    // update-index --stdin interprets its NUL paths relative to its cwd.
    let top = git_raw_output(ctx, &["rev-parse", "--show-toplevel"])?;
    if !top.status.success() {
        return Err(failure("cannot observe native intention worktree root"));
    }
    let root_bytes = top
        .stdout
        .strip_suffix(b"\n")
        .ok_or_else(|| failure("native intention worktree root lacks output terminator"))?;
    let mut root_ctx = ctx.clone();
    root_ctx.cwd = Path::new(OsStr::from_bytes(root_bytes)).to_path_buf();
    let mut intention_file = NamedTempFile::new_in(scratch).map_err(FactorError::StateWrite)?;
    intention_file
        .write_all(intention)
        .map_err(FactorError::StateWrite)?;
    let intention_path = intention_file
        .path()
        .to_str()
        .ok_or_else(|| failure("native intention input path is not UTF-8"))?;
    let projection_command = format!(
        "GIT_INDEX_FILE={} git -c core.splitIndex=false update-index --force-remove -z --stdin <{}",
        shell_quote(index),
        shell_quote(intention_path)
    );
    let projected = super::command_output_with(
        &root_ctx,
        "bash",
        &["-c", &projection_command],
        &[("GIT_OPTIONAL_LOCKS", Some("0"))],
    )?;
    if !projected.status.success() {
        return Err(failure(
            "cannot project native intent-to-add staged entries",
        ));
    }
    let projected_tree = super::command_output_with(
        ctx,
        "git",
        &["-c", "core.splitIndex=false", "write-tree"],
        &[
            ("GIT_INDEX_FILE", Some(index)),
            ("GIT_OPTIONAL_LOCKS", Some("0")),
        ],
    )?;
    if !projected_tree.status.success() {
        return Err(failure("cannot observe projected native staged tree"));
    }
    let tree = TreeHash::new(
        String::from_utf8(projected_tree.stdout)
            .map_err(|error| failure(&format!("staged tree identity is not UTF-8: {error}")))?
            .trim(),
    )?;
    let native = super::command_output_with(
        ctx,
        "git",
        &[
            "diff",
            "--cached",
            "--quiet",
            "--no-ext-diff",
            "--no-textconv",
            "--no-renames",
            "--no-relative",
            "--ita-invisible-in-index",
            tree.as_str(),
            "--",
            ":/",
        ],
        &[("GIT_OPTIONAL_LOCKS", Some("0"))],
    )?;
    if !native.status.success() {
        return Err(failure(
            "projected staged tree differs from native commit semantics",
        ));
    }
    Ok(tree)
}
/// Checks ITA bytes before any native commit projection may hide them.
#[expect(
    clippy::single_call_fn,
    reason = "staged observation has one source-ownership boundary before private-index projection"
)]
fn admit_staged_intentions(
    ctx: &Ctx<'_>,
    purpose: StagedPurpose<'_>,
    paths: &[u8],
) -> Result<(), FactorError> {
    match purpose {
        StagedPurpose::Clean => {
            return Err(failure(
                "tracked working tree and staged tree must match HEAD",
            ));
        }
        StagedPurpose::Selection { source, base } => {
            let allowed = candidate::changed_paths(
                ctx,
                base.map_or(EMPTY, CommitSha::as_str),
                source.as_str(),
            )?;
            let allowed_paths = allowed
                .split(|byte| *byte == 0)
                .filter(|intent_path| !intent_path.is_empty())
                .collect::<BTreeSet<_>>();
            if paths
                .split(|byte| *byte == 0)
                .filter(|intent_path| !intent_path.is_empty())
                .any(|intent_path| !allowed_paths.contains(intent_path))
            {
                return Err(failure(
                    "intent-to-add paths must belong to the remaining selected change",
                ));
            }
        }
    }
    Ok(())
}
/// Requires both physical tracked bytes and the real staged tree to equal HEAD.
fn tracked_clean(ctx: &Ctx<'_>) -> Result<(), FactorError> {
    if !physical_abort_paths(ctx)?.is_empty()
        || staged_tree(ctx, StagedPurpose::Clean)? != tree(ctx, "HEAD")?
    {
        return Err(failure(
            "tracked working tree and staged tree must match HEAD",
        ));
    }
    Ok(())
}
/// Active native replay mutations must never target an attached user branch.
fn require_detached(ctx: &Ctx<'_>) -> Result<(), FactorError> {
    if head_reference(ctx)? != "HEAD" {
        return Err(failure(
            "active selection HEAD was attached to another branch",
        ));
    }
    Ok(())
}
/// Independently validates and stamps the current replay commit in an owned actor.
fn verify_commit(ctx: &Ctx<'_>, j: &Journal) -> Result<CommitSha, FactorError> {
    require_detached(ctx)?;
    tracked_clean(ctx)?;
    let original = sha(&commit_output(ctx, &["rev-parse", "HEAD"])?)?;
    let parent_text = parents(ctx, original.as_str())?;
    if parent_text.contains(' ') {
        return Err(failure("merge gate positions are unsupported"));
    }
    let parent = if parent_text.is_empty() {
        None
    } else {
        Some(sha(&parent_text)?)
    };
    let selected = tree(ctx, "HEAD")?;
    let message = candidate_message(ctx, &original)?;
    let current = candidate::validate_named_in(
        ctx,
        &StateDir::new(state_dir(ctx)?),
        &j.session,
        candidate::Candidate::observed(
            &original,
            &NonEmpty::new(message),
            parent.as_ref(),
            &selected,
        ),
        &j.gates,
    )?;
    require_detached(ctx)?;
    if commit_output(ctx, &["rev-parse", "HEAD"])? != original
        || staged_tree(ctx, StagedPurpose::Clean)? != selected
    {
        return Err(failure(
            "replay position changed during isolated validation",
        ));
    }
    run_git(ctx, &["reset", "--soft", current.as_str()])?;
    Ok(current)
}
/// Preserves native message bytes while transferring the final LF to candidate rendering.
fn candidate_message(ctx: &Ctx<'_>, commit: &CommitSha) -> Result<NonEmptyString, FactorError> {
    let object = git_raw_output(ctx, &["cat-file", "commit", commit.as_str()])?;
    if !object.status.success() {
        return Err(failure("cannot observe candidate source message"));
    }
    let text = String::from_utf8(object.stdout)
        .map_err(|error| failure(&format!("candidate source message is not UTF-8: {error}")))?;
    let body = text
        .split_once("\n\n")
        .ok_or_else(|| failure("candidate source message is missing"))?
        .1;
    NonEmptyString::try_from(body.strip_suffix('\n').unwrap_or(body).to_owned())
        .map_err(|error| failure(&format!("empty candidate source message: {error}")))
}
/// Opens a new bounded native rebase and exposes its admitted source.
#[expect(
    clippy::pattern_type_mismatch,
    reason = "borrowed phase facts preserve canonical journal ownership without cloning or a second representation"
)]
#[expect(
    clippy::too_many_lines,
    reason = "opening admission keeps native ownership, physical selection, baseline validation and reset order together before publishing Selecting"
)]
fn open(ctx: &Ctx<'_>, mut j: Journal, invocation: Invocation) -> Result<i32, FactorError> {
    let Phase::Opening {
        source,
        base,
        head,
        anchor,
        ..
    } = j.state.clone()
    else {
        return Err(failure("opening intent missing"));
    };
    if !is_mid_rebase_in(ctx) {
        tracked_clean(ctx)?;
        if head_reference(ctx)? != j.branch.as_str()
            || commit_output(ctx, &["rev-parse", "HEAD"])? != j.checkpoint
        {
            return Err(failure("opening checkpoint is no longer checked out"));
        }
        admit_initial_untracked(ctx, base.as_ref(), &anchor, &j.checkpoint)?;
        let sequence = hidden(ctx, "edit")?;
        let mut args = vec![
            "-c",
            "rebase.missingCommitsCheck=ignore",
            "rebase",
            "--interactive",
            "--no-ff",
            "--reschedule-failed-exec",
            "--no-update-refs",
            "--no-autostash",
            "--no-autosquash",
            "--no-rebase-merges",
            "--empty=keep",
            "--keep-empty",
        ];
        if let Some(admitted_base) = base.as_ref() {
            args.push(admitted_base.as_str());
        } else {
            args.push("--root");
        }
        if !command_status_with(
            ctx,
            "git",
            &args,
            &[
                ("GIT_SEQUENCE_EDITOR", Some(sequence.as_str())),
                ("GIT_EDITOR", Some("true")),
            ],
            false,
        )?
        .success()
        {
            output::start_recovery(ctx, invocation.operation())?;
            return Ok(super::EXIT_TEMPFAIL);
        }
    }
    require_detached(ctx)?;
    // Even a recovered mixed reset must retain its complete controlled transcript.
    // HEAD equality proves the selection seam, not that native replay reached it.
    let opening = opening_position(ctx, &j)?;
    if opening != OpeningPosition::ReadyBreak {
        match opening {
            OpeningPosition::RetrySource => {
                admitted_opening_onto(ctx, base.as_ref())?;
            }
            OpeningPosition::FinishSource => {
                if tree(ctx, "HEAD")? != tree(ctx, &source)?
                    || parents(ctx, "HEAD")? != base.as_ref().map_or("", CommitSha::as_str)
                {
                    return Err(failure(
                        "completed opening source differs from admitted tree or parent",
                    ));
                }
            }
            OpeningPosition::ReadyBreak => {}
        }
        // Still-present obstruction refuses before native mutation, including ignored
        // aliases and shared physical identities that Git's overwrite check cannot see.
        tracked_clean(ctx)?;
        admit_initial_untracked(ctx, base.as_ref(), &anchor, &j.checkpoint)?;
        if !command_status_with(
            ctx,
            "git",
            &["rebase", "--continue"],
            &[("GIT_EDITOR", Some("true"))],
            false,
        )?
        .success()
        {
            output::start_recovery(ctx, invocation.operation())?;
            return Ok(super::EXIT_TEMPFAIL);
        }
        require_detached(ctx)?;
        if opening_position(ctx, &j)? != OpeningPosition::ReadyBreak {
            return Err(failure(
                "opening continuation did not reach its owned source break",
            ));
        }
    }
    let actual = commit_output(ctx, &["rev-parse", "HEAD"])?;
    if actual != head {
        if tree(ctx, "HEAD")? != tree(ctx, &source)?
            || parents(ctx, "HEAD")? != base.as_ref().map_or("", CommitSha::as_str)
        {
            return Err(failure("opening rebase does not match the combined source"));
        }
        // Baseline metadata acceptance precedes a worktree-changing mixed reset.
        admitted(ctx, &source, base.as_ref())?;
        if let Err(error) = verify_commit(ctx, &j) {
            if let FactorError::ExecFailed { code, command } = &error
                && let Some(exit_code) = NonZeroI32::new(*code)
            {
                output::gate_failed(
                    ctx,
                    output::GateFailureOrigin::Baseline(invocation.selection()),
                    command,
                    exit_code,
                )?;
            }
            return Err(error);
        }
        run_git(ctx, &["reset", "--mixed", "--quiet", head.as_str()])?;
    }
    admitted(ctx, &source, base.as_ref())?;
    j.state = Phase::Selecting {
        lease: lease(&j)
            .cloned()
            .ok_or_else(|| failure("active phase lacks lease"))?,
        source,
        base,
        head,
        anchor,
    };
    save(ctx, &mut j)?;
    report_selection(ctx, &j, invocation)
}
/// Reconciles a persisted terminal witness with a completed native rebase.
fn reconcile(ctx: &Ctx<'_>, mut j: Journal) -> Result<Journal, FactorError> {
    if let Phase::Verified {
        atom,
        remainder,
        tip,
        ..
    } = j.state.clone()
        && !is_mid_rebase_in(ctx)
    {
        if commit_output(ctx, &["rev-parse", j.branch.as_str()])? != tip
            || tree(ctx, &tip)? != j.final_tree
        {
            return Err(failure(
                "finished rebase does not match its terminal witness",
            ));
        }
        j.checkpoint = tip;
        j.state = match remainder {
            Remainder::Accepted(source) => Phase::Preparing {
                tip: source,
                base: Some(atom),
                previous_lease: lease(&j).cloned(),
            },
            Remainder::Absent => Phase::Closing {
                lease: lease(&j)
                    .cloned()
                    .ok_or_else(|| failure("verified phase lacks lease"))?,
                outcome: Outcome::Complete { atom },
            },
            Remainder::Pending(_) => {
                return Err(failure("terminal witness contains an unaccepted remainder"));
            }
        };
        save(ctx, &mut j)?;
    }
    Ok(j)
}
/// Refuse foreign repaired HEADs before allowing Git to advance its todo.
#[expect(
    clippy::pattern_type_mismatch,
    reason = "borrowed phase facts preserve canonical journal ownership without cloning or a second representation"
)]
#[expect(
    clippy::single_call_fn,
    reason = "this named runtime boundary keeps the checkpoint transition readable"
)]
fn admit_replay_head(ctx: &Ctx<'_>, journal: &Journal) -> Result<(), FactorError> {
    let Phase::Replaying {
        source,
        atom,
        remainder,
        accepted,
        ..
    } = &journal.state
    else {
        return Err(failure("replay admission lacks its accepted corridor"));
    };
    let actual = commit_output(ctx, &["rev-parse", "HEAD"])?;
    let last = accepted
        .last()
        .or_else(|| remainder.observed())
        .unwrap_or(atom);
    if actual == *last {
        return Ok(());
    }
    let parent = parents(ctx, &actual)?;
    if !accepted.is_empty() {
        let before_last = accepted
            .iter()
            .rev()
            .nth(1)
            .or_else(|| remainder.observed())
            .unwrap_or(atom);
        if parent == last.as_str() || parent == before_last.as_str() {
            return Ok(());
        }
    }
    if accepted.is_empty() {
        if let Some(admitted_remainder) = remainder.observed()
            && (parent == admitted_remainder.as_str()
                || parent == atom.as_str() && tree(ctx, &actual)? == tree(ctx, source)?)
        {
            return Ok(());
        }
        if matches!(remainder, Remainder::Absent) && parent == atom.as_str() {
            return Ok(());
        }
    }
    Err(failure(
        "current replay HEAD is outside its accepted predecessor corridor; automatic recovery is unavailable",
    ))
}
/// Resumes native replay only after phase and current HEAD admission.
#[expect(
    clippy::pattern_type_mismatch,
    reason = "borrowed journal and transcript facts use ordinary reference matching without cloning identities or ref-pattern scaffolding"
)]
#[expect(
    clippy::too_many_lines,
    reason = "replay recovery keeps phase dispatch and accepted predecessor checks adjacent so native continuation cannot bypass admission"
)]
fn advance(ctx: &Ctx<'_>, previous: Journal, invocation: Invocation) -> Result<i32, FactorError> {
    let j = reconcile(ctx, previous)?;
    match j.state.clone() {
        Phase::Preparing { .. } => prepare(ctx, j, invocation),
        Phase::Opening { .. } => open(ctx, j, invocation),
        Phase::Selecting { head, .. } => {
            if !is_mid_rebase_in(ctx) || commit_output(ctx, &["rev-parse", "HEAD"])? != head {
                return Err(failure(
                    "selection no longer owns its expected HEAD and rebase",
                ));
            }
            report_selection(ctx, &j, invocation)
        }
        Phase::Replaying {
            source,
            base,
            head,
            atom,
            remainder,
            accepted,
            anchor,
            ..
        } => {
            if !is_mid_rebase_in(ctx) {
                return Err(failure(
                    "replay has no terminal witness; refusing to discard progress",
                ));
            }
            let position = replay_position(ctx, &j)?.position;
            if commit_output(ctx, &["rev-parse", "HEAD"])? == head {
                if position != ReplayPosition::Pending(None)
                    || !accepted.is_empty()
                    || matches!(remainder, Remainder::Accepted(_))
                {
                    return Err(failure(
                        "saved selection HEAD is outside the initial replay promotion frontier",
                    ));
                }
                admitted(ctx, &source, base.as_ref())?;
                run_git(
                    ctx,
                    &[
                        "reset",
                        "--hard",
                        remainder.observed().unwrap_or(&atom).as_str(),
                    ],
                )?;
            }
            admit_replay_head(ctx, &j)?;
            if matches!(
                position,
                ReplayPosition::Retained(ReplaySlot::Descendant(_) | ReplaySlot::Terminal)
            ) {
                remainder.accepted()?;
            }
            if position == ReplayPosition::Retained(ReplaySlot::Terminal)
                && accepted.len() != descendants(ctx, &anchor, &j.checkpoint)?.len()
            {
                return Err(failure(
                    "terminal replay omitted an original descendant; automatic recovery is unavailable after bypassing git factor --continue",
                ));
            }
            match position {
                ReplayPosition::Pending(last_consumed) => {
                    match last_consumed {
                        Some(ReplaySlot::Remainder) => {
                            remainder.accepted()?;
                        }
                        Some(ReplaySlot::Descendant(descendant_index))
                            if descendant_index >= accepted.len() =>
                        {
                            return Err(failure(
                                "native replay skipped an unaccepted original descendant; automatic recovery is unavailable after bypassing git factor --continue",
                            ));
                        }
                        Some(ReplaySlot::Descendant(_) | ReplaySlot::Terminal) | None => {}
                    }
                    let proof = accepted
                        .last()
                        .or(match &remainder {
                            Remainder::Accepted(commit) => Some(commit),
                            Remainder::Absent | Remainder::Pending(_) => None,
                        })
                        .unwrap_or(&atom);
                    let actual = commit_output(ctx, &["rev-parse", "HEAD"])?;
                    match ancestor(ctx, proof, &actual) {
                        Ok(()) => {}
                        Err(FactorError::NotAncestor(_)) => {
                            return Err(failure(
                                "pending native replay rewrote an accepted predecessor; automatic recovery is unavailable after bypassing git factor --continue",
                            ));
                        }
                        Err(error) => return Err(error),
                    }
                    finish_replay(ctx, j, invocation)
                }
                ReplayPosition::Retained(_) => finish_replay(ctx, j, invocation),
                ReplayPosition::Consumed(
                    slot @ (ReplaySlot::Remainder | ReplaySlot::Descendant(_)),
                ) => {
                    let accepted_journal = accept_replay_position(ctx, j, slot)?;
                    finish_replay(ctx, accepted_journal, invocation)
                }
                ReplayPosition::Consumed(ReplaySlot::Terminal) => {
                    terminal(ctx)?;
                    finish_replay(ctx, load(ctx)?, invocation)
                }
            }
        }
        Phase::Verified { .. } => finish_replay(ctx, j, invocation),
        Phase::Closing { outcome, .. } => {
            cleanup(ctx, j, outcome, invocation.checkpoint_completion())
        }
    }
}
/// Captures completed progress and immediately prepares any remaining change.
fn finish_replay(ctx: &Ctx<'_>, j: Journal, invocation: Invocation) -> Result<i32, FactorError> {
    if !is_mid_rebase_in(ctx) {
        return advance(ctx, j, invocation);
    }
    admit_remaining_untracked(ctx, &j)?;
    if !command_status_with(
        ctx,
        "git",
        &["rebase", "--continue"],
        &[("GIT_EDITOR", Some("true"))],
        false,
    )?
    .success()
    {
        output::start_recovery(ctx, invocation.operation())?;
        return Ok(super::EXIT_TEMPFAIL);
    }
    advance(ctx, recover(ctx)?, invocation)
}
/// Admits full main selection bytes and real staged paths without resetting.
fn admitted(
    ctx: &Ctx<'_>,
    source: &CommitSha,
    base: Option<&CommitSha>,
) -> Result<(), FactorError> {
    let selected = staged_tree(ctx, StagedPurpose::Selection { source, base })?;
    let scratch = TempDir::new_in(state_dir(ctx)?).map_err(FactorError::StateWrite)?;
    candidate::admit_selection(ctx, scratch.path(), source, base, &selected)
}
/// Validates an isolated atom before publishing replay intent or promoting it.
#[expect(
    clippy::pattern_type_mismatch,
    reason = "borrowed phase facts preserve canonical journal ownership without cloning or a second representation"
)]
fn submit(
    ctx: &Ctx<'_>,
    mut j: Journal,
    message_parts: &[NonEmptyString],
    invocation: Invocation,
) -> Result<i32, FactorError> {
    let Phase::Selecting {
        source,
        base,
        head,
        anchor,
        ..
    } = j.state.clone()
    else {
        return Err(failure("--message requires a selection phase"));
    };
    if !is_mid_rebase_in(ctx) || commit_output(ctx, &["rev-parse", "HEAD"])? != head {
        return Err(failure("selection is detached from its owned rebase"));
    }
    require_detached(ctx)?;
    let purpose = StagedPurpose::Selection {
        source: &source,
        base: base.as_ref(),
    };
    let selected = staged_tree(ctx, purpose)?;
    if selected == tree(ctx, &head)? {
        return Err(FactorError::NoStagedChanges);
    }
    let messages = NonEmpty::from_vec(message_parts.to_vec())
        .ok_or_else(|| failure("missing candidate message"))?;
    let candidate_result = candidate::validate_named_in(
        ctx,
        &StateDir::new(state_dir(ctx)?),
        &j.session,
        candidate::Candidate::new(&source, &messages, base.as_ref(), &selected),
        &j.gates,
    );
    require_detached(ctx)?;
    let atom = match candidate_result {
        Ok(candidate) => candidate,
        Err(error) => {
            if let FactorError::ExecFailed { code, command } = &error
                && let Some(exit_code) = NonZeroI32::new(*code)
            {
                output::gate_failed(
                    ctx,
                    output::GateFailureOrigin::Candidate(invocation.completion()),
                    command,
                    exit_code,
                )?;
            }
            return Err(error);
        }
    };
    if commit_output(ctx, &["rev-parse", "HEAD"])? != head || staged_tree(ctx, purpose)? != selected
    {
        return Err(failure(
            "selection changed during isolated candidate validation",
        ));
    }
    let remainder = if selected == tree(ctx, &source)? {
        Remainder::Absent
    } else {
        Remainder::Pending(metadata_commit(
            ctx,
            &source,
            &tree(ctx, &source)?,
            Some(&atom),
            None,
        )?)
    };
    j.state = Phase::Replaying {
        lease: lease(&j)
            .cloned()
            .ok_or_else(|| failure("active phase lacks lease"))?,
        source,
        base,
        head,
        atom,
        remainder,
        accepted: Vec::new(),
        anchor,
    };
    save(ctx, &mut j)?;
    advance(ctx, j, invocation)
}
/// Reads changed paths losslessly as raw NUL-delimited bytes.
fn diff_paths(ctx: &Ctx<'_>, args: &[&str]) -> Result<BTreeSet<Vec<u8>>, FactorError> {
    let output =
        super::command_output_with(ctx, "git", args, &[("GIT_OPTIONAL_LOCKS", Some("0"))])?;
    if !output.status.success() {
        return Err(failure("cannot admit abort paths"));
    }
    if !output.stdout.is_empty() && !output.stdout.ends_with(&[0]) {
        return Err(failure("invalid NUL-separated abort paths"));
    }
    Ok(output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .map(<[u8]>::to_vec)
        .collect())
}
/// Observe physical tracked bytes without relying on real-index visibility flags.
fn physical_abort_paths(ctx: &Ctx<'_>) -> Result<BTreeSet<Vec<u8>>, FactorError> {
    let scratch = TempDir::new().map_err(FactorError::StateWrite)?;
    let index = scratch.path().join("index");
    let index_text = index
        .to_str()
        .ok_or_else(|| failure("abort observation index is not UTF-8"))?;
    let environment = [
        ("GIT_INDEX_FILE", Some(index_text)),
        ("GIT_OPTIONAL_LOCKS", Some("0")),
    ];
    for args in [
        vec![
            "-c",
            "core.ignorestat=false",
            "-c",
            "core.splitIndex=false",
            "-c",
            "core.sparseCheckout=false",
            "read-tree",
            "HEAD",
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
        let output = super::command_output_with(ctx, "git", &args, &environment)?;
        if !output.status.success() {
            return Err(failure("cannot observe physical tracked abort paths"));
        }
    }
    let output = super::command_output_with(
        ctx,
        "git",
        &[
            "diff",
            "--cached",
            "--name-only",
            "--no-renames",
            "--no-relative",
            "-z",
            "HEAD",
        ],
        &environment,
    )?;
    if !output.status.success() || (!output.stdout.is_empty() && !output.stdout.ends_with(&[0])) {
        return Err(failure("cannot admit physical tracked abort paths"));
    }
    Ok(output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .map(<[u8]>::to_vec)
        .collect())
}
/// Union every original commit's paths, including additions later canceled.
fn attempt_paths(
    ctx: &Ctx<'_>,
    base: Option<&CommitSha>,
    checkpoint: &CommitSha,
) -> Result<BTreeSet<Vec<u8>>, FactorError> {
    let range = base.map_or_else(
        || checkpoint.to_string(),
        |boundary| format!("{boundary}..{checkpoint}"),
    );
    let commits = git_output(ctx, &["rev-list", "--reverse", &range])?;
    let mut paths = BTreeSet::new();
    for raw_commit in commits.lines() {
        let commit = CommitSha::new(raw_commit.to_owned())?;
        if parents(ctx, &commit)?.contains(' ') {
            return Err(failure("attempt path corridor contains a merge"));
        }
        paths.extend(diff_paths(
            ctx,
            &[
                "diff-tree",
                "--no-relative",
                "--root",
                "--no-commit-id",
                "--name-only",
                "--no-renames",
                "-r",
                "-z",
                commit.as_str(),
            ],
        )?);
    }
    Ok(paths)
}
/// Matches actual untracked objects against protected paths using kernel path resolution.
fn admit_untracked_objects(
    ctx: &Ctx<'_>,
    protected: &BTreeSet<Vec<u8>>,
    untracked: &BTreeSet<Vec<u8>>,
) -> Result<(), FactorError> {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt as _;
    let top = git_raw_output(ctx, &["rev-parse", "--show-toplevel"])?;
    if !top.status.success() {
        return Err(failure("cannot observe protected worktree root"));
    }
    let root_bytes = top
        .stdout
        .strip_suffix(b"\n")
        .ok_or_else(|| failure("protected worktree root lacks output terminator"))?;
    let root = Path::new(OsStr::from_bytes(root_bytes));
    let mut objects = BTreeSet::new();
    for path in protected {
        if let Some(object) = candidate::physical_path_objects(ctx, root, path)?.last() {
            objects.insert(*object);
        }
    }
    for path in untracked {
        if candidate::physical_path_objects(ctx, root, path)?
            .iter()
            .any(|object| objects.contains(object))
        {
            return Err(failure(
                "checkpoint would overwrite unrelated untracked work",
            ));
        }
    }
    Ok(())
}

/// Preserve untracked paths before opening any intermediate original tree.
fn admit_initial_untracked(
    ctx: &Ctx<'_>,
    base: Option<&CommitSha>,
    tip: &CommitSha,
    checkpoint: &CommitSha,
) -> Result<(), FactorError> {
    let mut protected = diff_paths(
        ctx,
        &[
            "ls-tree",
            "--full-tree",
            "-r",
            "-z",
            "--name-only",
            tip.as_str(),
        ],
    )?;
    protected.extend(diff_paths(
        ctx,
        &[
            "ls-tree",
            "--full-tree",
            "-r",
            "-z",
            "--name-only",
            checkpoint.as_str(),
        ],
    )?);
    if let Some(admitted_base) = base.as_ref() {
        protected.extend(diff_paths(
            ctx,
            &[
                "ls-tree",
                "--full-tree",
                "-r",
                "-z",
                "--name-only",
                admitted_base.as_str(),
            ],
        )?);
    }
    protected.extend(attempt_paths(ctx, base, checkpoint)?);
    admit_protected_untracked(ctx, &protected)
}

/// Native replay may replace every path in an unaccepted original descendant.
#[expect(
    clippy::single_call_fn,
    reason = "remaining-checkout protection stays a named physical admission before native replay can replace user files"
)]
fn admit_remaining_untracked(ctx: &Ctx<'_>, journal: &Journal) -> Result<(), FactorError> {
    if !matches!(journal.state, Phase::Replaying { .. }) {
        return Ok(());
    }
    let observation = replay_position(ctx, journal)?;
    let mut protected = BTreeSet::new();
    for commit in observation.pending_picks {
        protected.extend(diff_paths(
            ctx,
            &[
                "diff-tree",
                "--no-relative",
                "--root",
                "--no-commit-id",
                "--name-only",
                "--no-renames",
                "-r",
                "-z",
                commit.as_str(),
            ],
        )?);
    }
    admit_protected_untracked(ctx, &protected)
}

/// Includes ignored input and physical aliases without demanding clean conflict resolution.
fn admit_protected_untracked(
    ctx: &Ctx<'_>,
    protected: &BTreeSet<Vec<u8>>,
) -> Result<(), FactorError> {
    let untracked = diff_paths(
        ctx,
        &["ls-files", "--others", "--full-name", "-z", "--", ":/"],
    )?;
    admit_untracked_objects(ctx, protected, &untracked)?;
    if untracked.iter().any(|path| {
        protected.iter().any(|target| {
            path == target
                || path
                    .strip_prefix(target.as_slice())
                    .is_some_and(|suffix| suffix.starts_with(b"/"))
                || target
                    .strip_prefix(path.as_slice())
                    .is_some_and(|suffix| suffix.starts_with(b"/"))
        })
    }) {
        return Err(failure(
            "checkpoint would overwrite unrelated untracked work",
        ));
    }
    Ok(())
}
/// Admits the current attempt’s source and physical dirty path ownership.
#[expect(
    clippy::pattern_type_mismatch,
    reason = "borrowed phase facts preserve canonical journal ownership without cloning or a second representation"
)]
#[expect(
    clippy::single_call_fn,
    reason = "this named runtime boundary keeps the checkpoint transition readable"
)]
fn abort_source(ctx: &Ctx<'_>, j: &Journal) -> Result<CommitSha, FactorError> {
    let (anchor, corridor_base, original_source) = match &j.state {
        Phase::Selecting {
            source, base, head, ..
        } => {
            if commit_output(ctx, &["rev-parse", "HEAD"])? != *head {
                return Err(failure("selection HEAD changed before abort"));
            }
            admitted(ctx, source, base.as_ref())?;
            return Ok(source.clone());
        }
        Phase::Opening {
            source,
            anchor,
            base,
            ..
        }
        | Phase::Replaying {
            source,
            anchor,
            base,
            ..
        } => (anchor, base, Some(source)),
        Phase::Verified { anchor, base, .. } => (anchor, base, None),
        Phase::Preparing { .. } | Phase::Closing { .. } => {
            return Err(failure("captured phase unexpectedly owns a rebase"));
        }
    };
    let owned = attempt_paths(ctx, corridor_base.as_ref(), &j.checkpoint)?;
    let mut changed = physical_abort_paths(ctx)?;
    changed.extend(diff_paths(
        ctx,
        &[
            "diff",
            "--cached",
            "--name-only",
            "--no-renames",
            "--no-relative",
            "-z",
            "HEAD",
        ],
    )?);
    if !changed.is_subset(&owned) {
        return Err(failure("abort would discard unrelated tracked work"));
    }
    let head = commit_output(ctx, &["rev-parse", "HEAD"])?;
    match &j.state {
        Phase::Opening {
            head: selection,
            base,
            ..
        } => {
            if opening_position_for(ctx, j, Admission::Abort)? == OpeningPosition::RetrySource {
                // Nothing has been picked. Prime the actual native empty/base tree,
                // never mark unrelated source-path obstructions as attempt-owned.
                return admitted_opening_onto(ctx, base.as_ref());
            }
            if head == *selection {
                let source = original_source.ok_or_else(|| failure("opening source missing"))?;
                admitted(ctx, source, base.as_ref())?;
                return Ok(source.clone());
            }
            if parents(ctx, &head)? != base.as_ref().map_or("", CommitSha::as_str)
                || tree(ctx, &head)? != tree(ctx, anchor)?
            {
                return Err(failure("opening HEAD no longer belongs to this round"));
            }
        }
        Phase::Replaying {
            head: selection,
            atom,
            remainder,
            accepted,
            source,
            base,
            ..
        } => {
            if head == *selection {
                admitted(ctx, source, base.as_ref())?;
                return Ok(source.clone());
            }
            let predecessor = accepted
                .last()
                .or_else(|| remainder.observed())
                .unwrap_or(atom);
            if head != *predecessor && parents(ctx, &head)? != predecessor.as_str() {
                return Err(failure("replay HEAD rewrote its accepted predecessor"));
            }
        }
        Phase::Verified { tip, .. } if head != *tip => {
            return Err(failure("terminal HEAD changed before abort"));
        }
        Phase::Verified { .. }
        | Phase::Selecting { .. }
        | Phase::Preparing { .. }
        | Phase::Closing { .. } => {}
    }
    Ok(head)
}

/// Restores only the latest captured checkpoint after user-work admission.
#[expect(
    clippy::single_call_fn,
    reason = "this named runtime boundary keeps the checkpoint transition readable"
)]
fn abort(ctx: &Ctx<'_>, previous: Journal) -> Result<i32, FactorError> {
    let j = reconcile(ctx, previous)?;
    if is_mid_rebase_in(ctx) {
        let source = abort_source(ctx, &j)?;
        let present = diff_paths(
            ctx,
            &[
                "ls-tree",
                "--full-tree",
                "-r",
                "-z",
                "--name-only",
                source.as_str(),
            ],
        )?;
        let destination = diff_paths(
            ctx,
            &[
                "ls-tree",
                "--full-tree",
                "-r",
                "-z",
                "--name-only",
                j.checkpoint.as_str(),
            ],
        )?;
        let untracked = diff_paths(
            ctx,
            &["ls-files", "--others", "--full-name", "-z", "--", ":/"],
        )?;
        let foreign = untracked.difference(&present).cloned().collect();
        let overwritten = destination.difference(&present).cloned().collect();
        admit_untracked_objects(ctx, &overwritten, &foreign)?;
        for path in untracked.difference(&present) {
            if destination.iter().any(|target| {
                path == target
                    || path
                        .strip_prefix(target.as_slice())
                        .is_some_and(|suffix| suffix.starts_with(b"/"))
                    || target
                        .strip_prefix(path.as_slice())
                        .is_some_and(|suffix| suffix.starts_with(b"/"))
            }) {
                return Err(failure("abort would overwrite unrelated untracked work"));
            }
        }
        // Prime only after selection/work preservation checks; avoids newly-added untracked
        // selected files blocking Git's restoration of the captured branch checkpoint.
        run_git(ctx, &["read-tree", source.as_str()])?;
        run_git(ctx, &["rebase", "--abort"])?;
    }
    if commit_output(ctx, &["rev-parse", "HEAD"])? != j.checkpoint {
        return Err(failure(
            "abort did not restore the latest completed checkpoint",
        ));
    }
    let base = phase_base(&j).cloned();
    cleanup(
        ctx,
        j,
        Outcome::Aborted { base },
        output::CheckpointCompletionOperation::Abort,
    )
}

/// Executes the owned native rebase editor and gate positions.
#[expect(
    clippy::single_call_fn,
    reason = "this named runtime boundary keeps the checkpoint transition readable"
)]
pub(in crate::git_factor) fn entry(
    ctx: &Ctx<'_>,
    arguments: &[OsString],
) -> Result<i32, FactorError> {
    require_supported_git(ctx)?;
    let args = arguments
        .iter()
        .map(|value| {
            value
                .to_str()
                .ok_or_else(|| failure("internal command values must be UTF-8"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    if let Some(command) = args.first() {
        match *command {
            "checkpoint-edit" => {
                let j = recover(ctx)?;
                editor(
                    ctx,
                    args.get(1).ok_or_else(|| failure("missing todo path"))?,
                    &j,
                )?;
                return Ok(0);
            }
            "checkpoint-gate-remainder" | "checkpoint-gate-descendant" => {
                return gate_position(ctx, command);
            }
            "checkpoint-terminal" => {
                return terminal(ctx);
            }
            _ => {}
        }
    }
    Err(failure("unknown internal checkpoint command"))
}

/// Restores the current selection index after preserving worktree ownership.
#[expect(
    clippy::pattern_type_mismatch,
    reason = "borrowed phase facts preserve canonical journal ownership without cloning or a second representation"
)]
#[expect(
    clippy::single_call_fn,
    reason = "this named runtime boundary keeps the checkpoint transition readable"
)]
fn retry(ctx: &Ctx<'_>, j: &Journal) -> Result<i32, FactorError> {
    let Phase::Selecting {
        source, base, head, ..
    } = &j.state
    else {
        return Err(failure("retry requires an open selection"));
    };
    if !is_mid_rebase_in(ctx) || commit_output(ctx, &["rev-parse", "HEAD"])? != *head {
        return Err(failure("selection HEAD changed before retry"));
    }
    admitted(ctx, source, base.as_ref())?;
    run_git(ctx, &["reset", "--mixed", "--quiet", head.as_str()])?;
    output::checkpoint_remaining(
        ctx,
        output::CheckpointTransition::Retried(split_count(ctx, base.as_ref(), j)?),
    )?;
    Ok(0)
}

/// Binds native text to generated actions, ignoring only pick subject text.
#[expect(
    clippy::pattern_type_mismatch,
    reason = "borrowed journal and transcript facts use ordinary reference matching without cloning identities or ref-pattern scaffolding"
)]
fn parse_replay_action(
    ctx: &Ctx<'_>,
    native_line: &str,
    source_ids: &[CommitSha],
    callbacks: &[(ReplaySlot, String)],
) -> Result<Option<ReplayAction>, FactorError> {
    let line = native_line.trim_start();
    if line.is_empty() || line.starts_with('#') {
        return Ok(None);
    }
    let (command, payload) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
    let rest = payload.trim_start();
    match command {
        "pick" | "p" => {
            let token = rest
                .split_whitespace()
                .next()
                .ok_or_else(|| failure("native pick lacks source identity"))?;
            if token.is_empty() || !token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                return Err(failure(
                    "native pick is not a bound hexadecimal source identity",
                ));
            }
            let normalized_token = token.to_ascii_lowercase();
            let mut matching = source_ids
                .iter()
                .filter(|source| source.as_str().starts_with(&normalized_token));
            let source = matching
                .next()
                .ok_or_else(|| failure("native pick changed its original source slot"))?;
            if matching.next().is_some() {
                return Err(failure(
                    "native pick identity is ambiguous within the original corridor",
                ));
            }
            if normalized_token.len() != source.as_str().len()
                && commit_output(
                    ctx,
                    &[
                        "rev-parse",
                        "--verify",
                        &format!("{normalized_token}^{{commit}}"),
                    ],
                )? != *source
            {
                return Err(failure(
                    "native pick abbreviation resolves outside its original source slot",
                ));
            }
            Ok(Some(ReplayAction::Pick(source.clone())))
        }
        "break" | "b" if rest.is_empty() => Ok(Some(ReplayAction::Break)),
        "exec" | "x" => callbacks
            .iter()
            .find(|(_, callback)| callback == rest)
            .map(|(position, _)| Some(ReplayAction::Exec(*position)))
            .ok_or_else(|| failure("native replay contains a foreign exec payload")),
        _ => Err(failure(
            "native replay changed its controlled pick/break/exec dialect",
        )),
    }
}

/// Admits one canonical quoted absolute program, without resolving or executing it.
fn callback_program(ctx: &Ctx<'_>, admission: Admission) -> Result<String, FactorError> {
    if matches!(admission, Admission::Resume) {
        return Ok(shell_quote(&exe(ctx)?));
    }
    let directory = git_dir_in(ctx)?.join("rebase-merge");
    let mut program = None;
    for file in ["done", "git-rebase-todo"] {
        let text = ctx
            .fs
            .read_to_string(&directory.join(file))
            .map_err(FactorError::StateRead)?;
        for line in text.lines() {
            let (command, rest) = line
                .trim_start()
                .split_once(char::is_whitespace)
                .unwrap_or((line, ""));
            if !matches!(command, "exec" | "x") {
                continue;
            }
            let quoted = [
                " checkpoint-gate-remainder",
                " checkpoint-gate-descendant",
                " checkpoint-terminal",
            ]
            .into_iter()
            .find_map(|suffix| rest.trim_start().strip_suffix(suffix))
            .ok_or_else(|| failure("native replay contains a foreign exec payload"))?;
            let inner = quoted
                .strip_prefix('\'')
                .and_then(|value| value.strip_suffix('\''))
                .ok_or_else(|| failure("native callback program is not canonically quoted"))?;
            let absolute = inner.replace("'\\''", "'");
            if !Path::new(&absolute).is_absolute() || shell_quote(&absolute) != quoted {
                return Err(failure(
                    "native callback program is not a quoted absolute path",
                ));
            }
            if program.as_ref().is_some_and(|previous| previous != quoted) {
                return Err(failure("native replay contains mixed callback programs"));
            }
            program = Some(quoted.to_owned());
        }
    }
    program.ok_or_else(|| failure("native replay lacks its owned callbacks"))
}

/// Refines Verified's discarded source identity from its retained owned transcript.
#[expect(
    clippy::single_call_fn,
    reason = "terminal abort alone reconstructs the discarded source under one complete lease, tree, parent and author witness"
)]
fn terminal_source_for_abort(
    ctx: &Ctx<'_>,
    journal: &Journal,
    anchor: &CommitSha,
    base: Option<&CommitSha>,
) -> Result<CommitSha, FactorError> {
    let done = ctx
        .fs
        .read_to_string(&git_dir_in(ctx)?.join("rebase-merge/done"))
        .map_err(FactorError::StateRead)?;
    let first = done
        .lines()
        .map(str::trim_start)
        .find(|line| !line.is_empty() && !line.starts_with('#'))
        .ok_or_else(|| failure("terminal replay lacks its original source pick"))?;
    let (command, payload) = first.split_once(char::is_whitespace).unwrap_or((first, ""));
    let token = payload
        .split_whitespace()
        .next()
        .ok_or_else(|| failure("terminal replay lacks its original source identity"))?;
    if !matches!(command, "pick" | "p") || !token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(failure("terminal replay changed its original source pick"));
    }
    let source = commit_output(
        ctx,
        &["rev-parse", "--verify", &format!("{token}^{{commit}}")],
    )?;
    ancestor(
        ctx,
        &source,
        lease(journal).ok_or_else(|| failure("terminal replay lacks its owned lease"))?,
    )?;
    if parents(ctx, &source)? != base.map_or("", CommitSha::as_str)
        || tree(ctx, &source)? != tree(ctx, anchor)?
        || source_author(ctx, &source)? != source_author(ctx, &journal.original_tip)?
    {
        return Err(failure(
            "terminal replay changed its retained combined source",
        ));
    }
    Ok(source)
}

/// Validates original slots and adjacent native retry actions without counting retries.
fn replay_position(ctx: &Ctx<'_>, journal: &Journal) -> Result<ReplayObservation, FactorError> {
    replay_position_for(ctx, journal, Admission::Resume)
}

/// Uses the same complete schedule grammar for resume and checkpoint-safe abort.
#[expect(
    clippy::pattern_type_mismatch,
    reason = "borrowed journal and transcript facts use ordinary reference matching without cloning identities or ref-pattern scaffolding"
)]
#[expect(
    clippy::too_many_lines,
    reason = "complete transcript admission binds original picks, adjacent native retries and consumed callbacks before exposing a recovery frontier"
)]
fn replay_position_for(
    ctx: &Ctx<'_>,
    journal: &Journal,
    admission: Admission,
) -> Result<ReplayObservation, FactorError> {
    let terminal_source;
    let (source, anchor) = match &journal.state {
        Phase::Replaying { source, anchor, .. } | Phase::Opening { source, anchor, .. } => {
            (source, anchor)
        }
        Phase::Selecting { source, anchor, .. } => (source, anchor),
        Phase::Verified { anchor, base, .. } if matches!(admission, Admission::Abort) => {
            terminal_source = terminal_source_for_abort(ctx, journal, anchor, base.as_ref())?;
            (&terminal_source, anchor)
        }
        Phase::Closing { .. } | Phase::Preparing { .. } | Phase::Verified { .. } => {
            return Err(failure("native position requires opening or replay intent"));
        }
    };
    let originals = descendants(ctx, anchor, &journal.checkpoint)?;
    let source_ids = once(source.clone())
        .chain(originals.iter().cloned())
        .collect::<Vec<_>>();
    let program = callback_program(ctx, admission)?;
    let remainder = format!("{program} checkpoint-gate-remainder");
    let descendant = format!("{program} checkpoint-gate-descendant");
    let terminal = format!("{program} checkpoint-terminal");
    let mut schedule = vec![
        ReplayAction::Pick(source.clone()),
        ReplayAction::Break,
        ReplayAction::Exec(ReplaySlot::Remainder),
    ];
    for (index, original) in originals.iter().enumerate() {
        schedule.push(ReplayAction::Pick(original.clone()));
        schedule.push(ReplayAction::Exec(ReplaySlot::Descendant(index)));
    }
    schedule.push(ReplayAction::Exec(ReplaySlot::Terminal));
    let directory = git_dir_in(ctx)?.join("rebase-merge");
    let done = ctx
        .fs
        .read_to_string(&directory.join("done"))
        .map_err(FactorError::StateRead)?;
    let todo = ctx
        .fs
        .read_to_string(&directory.join("git-rebase-todo"))
        .map_err(FactorError::StateRead)?;
    let mut cursor: usize = 0;
    let mut last_done = None;
    let mut last_consumed = None;
    let mut pending_picks = Vec::new();
    let mut previous = None;
    let mut retained_callback = None;
    for (is_done, text) in [(true, done.as_str()), (false, todo.as_str())] {
        let mut first_actionable = true;
        for line in text.lines() {
            // Bind the repeated descendant payload to its source slot, never an exec count.
            let current_position = match schedule.get(cursor) {
                Some(ReplayAction::Exec(ReplaySlot::Descendant(index))) => {
                    Some(ReplaySlot::Descendant(*index))
                }
                _ => match cursor
                    .checked_sub(1)
                    .and_then(|previous_index| schedule.get(previous_index))
                {
                    Some(ReplayAction::Exec(ReplaySlot::Descendant(index))) => {
                        Some(ReplaySlot::Descendant(*index))
                    }
                    _ => None,
                },
            };
            let mut callbacks = vec![
                (ReplaySlot::Remainder, remainder.clone()),
                (ReplaySlot::Terminal, terminal.clone()),
            ];
            if let Some(position) = current_position {
                callbacks.push((position, descendant.clone()));
            }
            if current_position.is_none() {
                let actionable = line.trim_start();
                let (command, rest) = actionable
                    .split_once(char::is_whitespace)
                    .unwrap_or((actionable, ""));
                if matches!(command, "exec" | "x") && rest.trim_start() == descendant {
                    return Err(failure(
                        "native replay omitted, repeated, or reordered an original source slot",
                    ));
                }
            }
            let Some(action) = parse_replay_action(ctx, line, &source_ids, &callbacks)? else {
                continue;
            };
            let repeated = previous.as_ref() == Some(&action);
            let native_retry = matches!(action, ReplayAction::Pick(_) | ReplayAction::Exec(_))
                && repeated
                && (is_done || first_actionable);
            if !is_done
                && first_actionable
                && repeated
                && let ReplayAction::Exec(slot) = &action
            {
                retained_callback = Some(*slot);
            }
            if schedule.get(cursor) == Some(&action) {
                cursor = cursor
                    .checked_add(1)
                    .ok_or_else(|| failure("native replay position overflow"))?;
            } else if !native_retry {
                return Err(failure(
                    "native replay omitted, repeated, or reordered an original source slot",
                ));
            } else {
                // An adjacent native retry retains the admitted source slot.
            }
            if is_done {
                last_done = match &action {
                    ReplayAction::Exec(position) => {
                        last_consumed = Some(*position);
                        Some(*position)
                    }
                    ReplayAction::Pick(_) | ReplayAction::Break => None,
                };
            } else if let ReplayAction::Pick(original) = &action {
                pending_picks.push(original.clone());
            } else {
                // Future breaks and callbacks introduce no checkout identity.
            }
            first_actionable = false;
            previous = Some(action);
        }
    }
    if cursor != schedule.len() {
        return Err(failure(
            "native replay omitted part of its generated schedule",
        ));
    }
    for marker in [
        directory.join("stopped-sha"),
        directory.join("amend"),
        git_dir_in(ctx)?.join("REBASE_HEAD"),
    ] {
        if path_present(ctx, &marker)? {
            return Ok(ReplayObservation {
                position: ReplayPosition::Pending(last_consumed),
                pending_picks,
            });
        }
    }
    if let Some(slot) = last_done {
        // Abort admits attempt-owned dirty work through its physical/path guards.
        if matches!(admission, Admission::Resume) {
            tracked_clean(ctx)?;
        }
        return Ok(ReplayObservation {
            position: if retained_callback == Some(slot) {
                ReplayPosition::Retained(slot)
            } else {
                ReplayPosition::Consumed(slot)
            },
            pending_picks,
        });
    }
    Ok(ReplayObservation {
        position: ReplayPosition::Pending(last_consumed),
        pending_picks,
    })
}

/// Binds the source-not-yet-picked HEAD to Git's actual native onto.
fn admitted_opening_onto(
    ctx: &Ctx<'_>,
    base: Option<&CommitSha>,
) -> Result<CommitSha, FactorError> {
    let directory = git_dir_in(ctx)?.join("rebase-merge");
    let onto_text = ctx
        .fs
        .read_to_string(&directory.join("onto"))
        .map_err(FactorError::StateRead)?;
    let onto = sha(onto_text.strip_suffix('\n').unwrap_or(&onto_text))?;
    if commit_output(ctx, &["rev-parse", "HEAD"])? != onto {
        return Err(failure(
            "opening source retry HEAD differs from native onto",
        ));
    }
    if let Some(parent) = base {
        if &onto != parent {
            return Err(failure("opening native onto differs from selected parent"));
        }
    } else if tree(ctx, &onto)?.as_str() != EMPTY || !parents(ctx, &onto)?.is_empty() {
        return Err(failure(
            "opening native root onto is not parentless empty tree",
        ));
    } else {
        // The root seam is admitted as parentless and empty.
    }
    Ok(onto)
}

/// Admits only the initial source seams, never progress beyond the owned break.
fn opening_position(ctx: &Ctx<'_>, journal: &Journal) -> Result<OpeningPosition, FactorError> {
    opening_position_for(ctx, journal, Admission::Resume)
}

/// Applies command-specific executable authority to the same opening prefix proof.
#[expect(
    clippy::pattern_type_mismatch,
    reason = "borrowed journal and transcript facts use ordinary reference matching without cloning identities or ref-pattern scaffolding"
)]
fn opening_position_for(
    ctx: &Ctx<'_>,
    journal: &Journal,
    admission: Admission,
) -> Result<OpeningPosition, FactorError> {
    let (Phase::Opening { source, anchor, .. } | Phase::Selecting { source, anchor, .. }) =
        &journal.state
    else {
        return Err(failure("source retry requires opening intent"));
    };
    // Reuse the entire controlled schedule proof before examining the narrower prefix.
    if !matches!(
        replay_position_for(ctx, journal, admission)?.position,
        ReplayPosition::Pending(_)
    ) {
        return Err(failure(
            "opening replay already advanced beyond its source break",
        ));
    }
    let ids = once(source.clone())
        .chain(descendants(ctx, anchor, &journal.checkpoint)?)
        .collect::<Vec<_>>();
    let directory = git_dir_in(ctx)?.join("rebase-merge");
    let done = ctx
        .fs
        .read_to_string(&directory.join("done"))
        .map_err(FactorError::StateRead)?;
    let todo = ctx
        .fs
        .read_to_string(&directory.join("git-rebase-todo"))
        .map_err(FactorError::StateRead)?;
    let actions = |text: &str| {
        text.lines()
            .filter_map(|line| parse_replay_action(ctx, line, &ids, &[]).transpose())
            .collect::<Result<Vec<_>, _>>()
    };
    let completed = actions(&done)?;
    let source_pick = ReplayAction::Pick(source.clone());
    if let Some((ReplayAction::Break, picks)) = completed.split_last()
        && !picks.is_empty()
        && picks.iter().all(|action| *action == source_pick)
    {
        return Ok(OpeningPosition::ReadyBreak);
    }
    if completed.is_empty() || !completed.iter().all(|action| *action == source_pick) {
        return Err(failure(
            "opening replay has not reached its admitted source break or bound source retry",
        ));
    }
    // Only the first two actionable todo commands are needed here. The full proof
    // above has already bound all following own callbacks and original descendants.
    let leading_lines = todo
        .lines()
        .filter(|line| !line.trim().is_empty() && !line.trim_start().starts_with('#'))
        .take(2)
        .collect::<Vec<_>>();
    if leading_lines
        .first()
        .is_some_and(|line| matches!(line.trim(), "break" | "b"))
    {
        return Ok(OpeningPosition::FinishSource);
    }
    let leading = leading_lines
        .into_iter()
        .filter_map(|line| parse_replay_action(ctx, line, &ids, &[]).transpose())
        .collect::<Result<Vec<_>, _>>()?;
    if leading.first() != Some(&source_pick) || leading.get(1) != Some(&ReplayAction::Break) {
        return Err(failure(
            "opening replay has not reached its admitted source break or bound source retry",
        ));
    }
    Ok(OpeningPosition::RetrySource)
}

/// Revalidates only the native source slot admitted by the controlled schedule.
fn accept_replay_position(
    ctx: &Ctx<'_>,
    mut journal: Journal,
    position: ReplaySlot,
) -> Result<Journal, FactorError> {
    let Phase::Replaying {
        source,
        base,
        head,
        atom,
        mut remainder,
        mut accepted,
        anchor,
        ..
    } = journal.state.clone()
    else {
        return Err(failure("gate acceptance requires replay intent"));
    };
    match position {
        ReplaySlot::Remainder => {
            if !accepted.is_empty() {
                return Err(failure(
                    "remainder callback moved behind accepted descendants",
                ));
            }
            if !matches!(remainder, Remainder::Absent) {
                if parents(ctx, "HEAD")? != atom.as_str()
                    || tree(ctx, "HEAD")? != tree(ctx, &source)?
                {
                    return Err(failure(
                        "repaired remainder violates its accepted parent or selected tree",
                    ));
                }
                // The metadata remainder was constructed before its normal/native gates.
                // SHA equality alone must never turn it into a positive acceptance proof.
                remainder = Remainder::Accepted(verify_commit(ctx, &journal)?);
            } else if commit_output(ctx, &["rev-parse", "HEAD"])? != atom {
                return Err(failure(
                    "empty remainder callback changed its accepted atom",
                ));
            } else {
                // With no remainder, the atom is the accepted callback tree.
            }
        }
        ReplaySlot::Descendant(slot) => {
            remainder.accepted()?;
            let actual = commit_output(ctx, &["rev-parse", "HEAD"])?;
            let same_last_slot = slot.checked_add(1) == Some(accepted.len());
            if same_last_slot && accepted.last() == Some(&actual) {
                return Ok(journal);
            }
            let predecessor = if same_last_slot {
                accepted
                    .len()
                    .checked_sub(2)
                    .and_then(|index| accepted.get(index))
                    .or_else(|| remainder.observed())
                    .unwrap_or(&atom)
            } else if slot == accepted.len() {
                accepted
                    .last()
                    .or_else(|| remainder.observed())
                    .unwrap_or(&atom)
            } else {
                return Err(failure(
                    "native replay skipped or rewound unverified descendant progress; automatic recovery is unavailable after bypassing git factor --continue",
                ));
            };
            if actual == *predecessor {
                return Err(failure(
                    "native replay omitted an original descendant; repair with an explicit commit before continuing",
                ));
            }
            if parents(ctx, &actual)? != predecessor.as_str() {
                return Err(failure(
                    "replayed descendant changed its accepted predecessor",
                ));
            }
            let verified = verify_commit(ctx, &journal)?;
            if same_last_slot {
                let last = accepted
                    .last_mut()
                    .ok_or_else(|| failure("accepted source slot has no commit"))?;
                *last = verified;
            } else {
                accepted.push(verified);
            }
        }
        ReplaySlot::Terminal => {
            return Err(failure("native gate lacks its completed source position"));
        }
    }
    journal.state = Phase::Replaying {
        lease: lease(&journal)
            .cloned()
            .ok_or_else(|| failure("active phase lacks lease"))?,
        source,
        base,
        head,
        atom,
        remainder,
        accepted,
        anchor,
    };
    save(ctx, &mut journal)?;
    Ok(journal)
}

/// Native callback and lost-callback recovery share the same source-slot admission.
#[expect(
    clippy::single_call_fn,
    reason = "the external gate callback binds its source slot before mutating independent acceptance"
)]
fn gate_position(ctx: &Ctx<'_>, command: &str) -> Result<i32, FactorError> {
    let journal = recover(ctx)?;
    let position = replay_position(ctx, &journal)?
        .position
        .consumed()
        .ok_or_else(|| failure("native callback lacks a completed original source slot"))?;
    let correct_callback = matches!(
        (command, position),
        ("checkpoint-gate-remainder", ReplaySlot::Remainder)
            | ("checkpoint-gate-descendant", ReplaySlot::Descendant(_))
    );
    if !correct_callback {
        return Err(failure(
            "native callback does not match its consumed original source slot",
        ));
    }
    let _admitted = accept_replay_position(ctx, journal, position)?;
    Ok(0)
}

/// Persists a gate and final-tree witness before Git completes native replay.
fn terminal(ctx: &Ctx<'_>) -> Result<i32, FactorError> {
    let mut j = recover(ctx)?;
    if matches!(j.state, Phase::Replaying { .. })
        && replay_position(ctx, &j)?.position.consumed() != Some(ReplaySlot::Terminal)
    {
        return Err(failure(
            "terminal callback lacks its consumed original schedule position",
        ));
    }
    if tree(ctx, "HEAD")? != j.final_tree {
        return Err(failure(
            "replay final tree differs from its fixed original tip",
        ));
    }
    match j.state.clone() {
        Phase::Replaying {
            atom,
            remainder,
            accepted,
            anchor,
            ..
        } => {
            remainder.accepted()?;
            if accepted.len() != descendants(ctx, &anchor, &j.checkpoint)?.len() {
                return Err(failure(
                    "terminal replay omitted an original descendant; automatic recovery is unavailable after bypassing git factor --continue",
                ));
            }
            let observed = commit_output(ctx, &["rev-parse", "HEAD"])?;
            if !accepted.is_empty() {
                let expected_parent = accepted
                    .iter()
                    .rev()
                    .nth(1)
                    .or_else(|| remainder.observed())
                    .unwrap_or(&atom);
                if parents(ctx, &observed)? != expected_parent.as_str() {
                    return Err(failure(
                        "terminal repair rewrote an earlier accepted descendant",
                    ));
                }
                // Recheck final native message and command/tree proofs even when only
                // the terminal exec was rescheduled after a user's final amend.
                let _accepted_tip = verify_commit(ctx, &j)?;
            }
            if accepted.is_empty() && observed != *remainder.observed().unwrap_or(&atom) {
                return Err(failure(
                    "terminal replay changed an accepted atom or remainder",
                ));
            }
        }
        Phase::Verified { .. } => {}
        Phase::Preparing { .. }
        | Phase::Opening { .. }
        | Phase::Selecting { .. }
        | Phase::Closing { .. } => {
            return Err(failure("terminal witness is not attached to replay"));
        }
    }
    match j.state.clone() {
        Phase::Replaying {
            atom,
            remainder,
            anchor,
            base,
            ..
        } => {
            j.state = Phase::Verified {
                lease: lease(&j)
                    .cloned()
                    .ok_or_else(|| failure("active phase lacks lease"))?,
                atom,
                remainder,
                tip: commit_output(ctx, &["rev-parse", "HEAD"])?,
                anchor,
                base,
            };
            save(ctx, &mut j)?;
        }
        Phase::Verified { tip, .. } if tip == commit_output(ctx, &["rev-parse", "HEAD"])? => {}
        Phase::Verified { .. }
        | Phase::Preparing { .. }
        | Phase::Opening { .. }
        | Phase::Selecting { .. }
        | Phase::Closing { .. } => {
            return Err(failure("terminal witness is not attached to replay"));
        }
    }
    Ok(0)
}
/// Admits user work before selecting the entire remaining source.
#[expect(
    clippy::pattern_type_mismatch,
    reason = "borrowed phase facts preserve canonical journal ownership without cloning or a second representation"
)]
#[expect(
    clippy::single_call_fn,
    reason = "this named runtime boundary keeps the checkpoint transition readable"
)]
fn finish(
    ctx: &Ctx<'_>,
    j: Journal,
    provided_messages: &[NonEmptyString],
) -> Result<i32, FactorError> {
    let Phase::Selecting {
        source, base, head, ..
    } = &j.state
    else {
        return Err(failure("finish requires an open selection"));
    };
    if !is_mid_rebase_in(ctx) || commit_output(ctx, &["rev-parse", "HEAD"])? != *head {
        return Err(failure("selection HEAD changed before finish"));
    }
    admitted(ctx, source, base.as_ref())?;
    let owned;
    let messages = if provided_messages.is_empty() {
        owned = vec![candidate_message(ctx, source)?];
        &owned
    } else {
        provided_messages
    };
    run_git(ctx, &["read-tree", source.as_str()])?;
    submit(ctx, j, messages, Invocation::Finish)
}
/// Borrows the atom boundary represented by the current phase.
#[expect(
    clippy::pattern_type_mismatch,
    reason = "borrowed phase facts preserve canonical journal ownership without cloning or a second representation"
)]
#[expect(
    clippy::single_call_fn,
    reason = "this named runtime boundary keeps the checkpoint transition readable"
)]
const fn phase_base(j: &Journal) -> Option<&CommitSha> {
    match &j.state {
        Phase::Preparing { base, .. }
        | Phase::Opening { base, .. }
        | Phase::Selecting { base, .. }
        | Phase::Replaying { base, .. }
        | Phase::Verified { base, .. }
        | Phase::Closing {
            outcome: Outcome::Aborted { base },
            ..
        } => base.as_ref(),
        Phase::Closing {
            outcome: Outcome::Complete { atom },
            ..
        } => Some(atom),
    }
}

/// Derives completed progress from actual checkpoint ancestry without a stored counter.
fn split_count(
    ctx: &Ctx<'_>,
    boundary: Option<&CommitSha>,
    j: &Journal,
) -> Result<u64, FactorError> {
    let Some(base) = boundary else { return Ok(0) };
    let range = j.original_base.as_ref().map_or_else(
        || base.to_string(),
        |original| format!("{original}..{base}"),
    );
    git_output(ctx, &["rev-list", "--count", &range])?
        .parse()
        .map_err(|error| {
            failure(&format!(
                "split count exceeds supported output range: {error}"
            ))
        })
}
/// Emits observed durable phase and derived progress without mutation.
#[expect(
    clippy::pattern_type_mismatch,
    reason = "borrowed phase facts preserve canonical journal ownership without cloning or a second representation"
)]
#[expect(
    clippy::single_call_fn,
    reason = "this named runtime boundary keeps the checkpoint transition readable"
)]
fn report_status(ctx: &Ctx<'_>, j: &Journal) -> Result<i32, FactorError> {
    if let Phase::Closing { outcome, .. } = &j.state {
        output::closing_status(
            ctx,
            &j.original_tip,
            &j.checkpoint,
            closing_outcome(ctx, outcome, j)?,
            j.original_base.is_none(),
            is_mid_rebase_in(ctx),
        )?;
        return Ok(0);
    }
    let (phase, base) = match &j.state {
        Phase::Preparing { base, .. } => (output::CheckpointPhase::Preparing, base.as_ref()),
        Phase::Opening { base, .. } => (output::CheckpointPhase::Opening, base.as_ref()),
        Phase::Selecting { base, .. } => (output::CheckpointPhase::Selecting, base.as_ref()),
        Phase::Replaying { base, .. } => (output::CheckpointPhase::Replaying, base.as_ref()),
        Phase::Verified { base, .. } => (output::CheckpointPhase::Verified, base.as_ref()),
        Phase::Closing {
            outcome: Outcome::Complete { atom },
            ..
        } => (output::CheckpointPhase::Closing, Some(atom)),
        Phase::Closing {
            outcome: Outcome::Aborted { base },
            ..
        } => (output::CheckpointPhase::Closing, base.as_ref()),
    };
    let range = j.original_base.as_ref().map_or_else(
        || j.original_tip.to_string(),
        |original| format!("{original}..{}", j.original_tip),
    );
    let commit_count = git_output(ctx, &["rev-list", "--count", &range])?
        .parse::<NonZeroU64>()
        .map_err(|error| failure(&format!("initial range count is invalid: {error}")))?;
    output::checkpoint_status(
        ctx,
        &j.original_tip,
        &j.checkpoint,
        output::CheckpointCounts::new(commit_count, split_count(ctx, base, j)?),
        phase,
        j.original_base.is_none(),
        is_mid_rebase_in(ctx),
    )?;
    Ok(0)
}

/// Derives terminal progress while its ancestry remains available, before owned cleanup.
#[expect(
    clippy::pattern_type_mismatch,
    reason = "borrowed phase facts preserve canonical journal ownership without cloning or a second representation"
)]
fn closing_outcome(
    ctx: &Ctx<'_>,
    outcome: &Outcome,
    journal: &Journal,
) -> Result<output::ClosingOutcome, FactorError> {
    match outcome {
        Outcome::Complete { atom } => NonZeroU64::new(split_count(ctx, Some(atom), journal)?)
            .map(output::ClosingOutcome::Complete)
            .ok_or_else(|| failure("completed closing lacks an accepted atom")),
        Outcome::Aborted { base } => Ok(output::ClosingOutcome::Aborted(split_count(
            ctx,
            base.as_ref(),
            journal,
        )?)),
    }
}

/// Executes the admitted public CLI against the single checkpoint journal.
#[expect(
    clippy::single_call_fn,
    reason = "this named runtime boundary keeps the checkpoint transition readable"
)]
pub(in crate::git_factor) fn run(
    ctx: &Ctx<'_>,
    cli: &super::Cli,
    matches: &clap::ArgMatches,
) -> Result<i32, FactorError> {
    validate_cli(cli)?;
    let advancing = cli.continue_flag()
        || cli.finish()
        || cli.retry()
        || cli.abort()
        || cli.status()
        || !cli.message().is_empty();
    require_supported_git(ctx)?;
    let event = if cli.abort() {
        "factor_cmd_abort"
    } else if cli.status() {
        "factor_cmd_status"
    } else if cli.retry() {
        "factor_cmd_retry"
    } else if cli.finish() {
        "factor_cmd_finish"
    } else if cli.continue_flag() || !cli.message().is_empty() {
        "factor_cmd_continue"
    } else {
        "factor_cmd_start"
    };
    trace_note(ctx, event, &[]);

    let has_journal = path_present(ctx, &journal_path(ctx)?)?;
    let has_scratch = path_present(ctx, &state_dir(ctx)?)?;
    if cli.status() && !has_journal && !has_scratch {
        output::inactive_status(ctx)?;
        return Ok(0);
    }
    if advancing {
        if !has_journal && !has_scratch {
            return Err(super::FactorError::NoActiveSession);
        }
        if cli.status() {
            return report_status(ctx, &load(ctx)?);
        }
        let j = recover_for(
            ctx,
            if cli.abort() {
                Admission::Abort
            } else {
                Admission::Resume
            },
        )?;
        if cli.abort() {
            return abort(ctx, j);
        }
        if cli.retry() {
            return retry(ctx, &j);
        }
        if cli.finish() {
            return finish(ctx, j, cli.message());
        }
        if !cli.message().is_empty() {
            return submit(ctx, j, cli.message(), Invocation::Continue);
        }
        return advance(ctx, j, Invocation::Continue);
    }
    let requests = cli_gates(ctx, cli, matches)?;
    start(ctx, cli, requests)
}
/// Rejects pure option conflicts and invalid gate names before operational I/O.
#[expect(
    clippy::single_call_fn,
    reason = "pure CLI admission precedes all operational Git observations"
)]
fn validate_cli(cli: &super::Cli) -> Result<(), FactorError> {
    let has_start = !cli.exec().is_empty() || !cli.gate().is_empty() || !cli.commits().is_empty();
    let conflict = if cli.abort() {
        (cli.status()
            || cli.continue_flag()
            || cli.retry()
            || cli.finish()
            || has_start
            || !cli.message().is_empty())
        .then_some("--abort cannot be combined with other options")
    } else if cli.status() {
        (cli.continue_flag()
            || cli.retry()
            || cli.finish()
            || has_start
            || !cli.message().is_empty())
        .then_some("--status cannot be combined with other options")
    } else if cli.retry() {
        (cli.continue_flag() || cli.finish() || has_start || !cli.message().is_empty())
            .then_some("--retry cannot be combined with other options")
    } else if cli.finish() {
        (cli.continue_flag() || has_start).then_some(if cli.gate().is_empty() {
            "--finish cannot be combined with --continue, --exec, or COMMIT"
        } else {
            "--finish cannot be combined with --continue, --exec, --gate, or COMMIT"
        })
    } else if cli.continue_flag() {
        has_start.then_some(if cli.gate().is_empty() {
            "--continue cannot be combined with --exec or COMMIT"
        } else {
            "--continue cannot be combined with --exec, --gate, or COMMIT"
        })
    } else if !cli.message().is_empty() {
        has_start.then_some(if cli.gate().is_empty() {
            "--message cannot be combined with --exec or COMMIT"
        } else {
            "--message cannot be combined with --exec, --gate, or COMMIT"
        })
    } else {
        (cli.exec().is_empty() && cli.gate().is_empty())
            .then_some("--exec <COMMAND> is required when starting a factor session")
    };
    if let Some(diagnostic) = conflict {
        return Err(FactorError::Usage(non_empty_msg(diagnostic.to_owned())));
    }
    let names = cli
        .gate()
        .as_chunks::<2>()
        .0
        .iter()
        .filter_map(|pair| pair.first())
        .map(NonEmptyString::as_str)
        .collect::<Vec<_>>();
    gate::validate_names(&names)
        .map_err(|error| FactorError::Usage(non_empty_msg(error.to_string())))?;
    Ok(())
}

/// Binds the ordered CLI gate specifications exactly once at session ingress.
#[expect(
    clippy::single_call_fn,
    reason = "gate admission is distinct from durable session construction"
)]
fn cli_gates(
    ctx: &Ctx<'_>,
    cli: &super::Cli,
    matches: &clap::ArgMatches,
) -> Result<gate::GateSet, FactorError> {
    let mut ordered = Vec::new();
    if let Some(indices) = matches.indices_of("exec") {
        for (index, command) in indices.zip(cli.exec()) {
            ordered.push((index, gate::legacy(ctx, command.clone())?));
        }
    }
    if let Some(indices) = matches.indices_of("gate") {
        for (index, (name, command)) in indices.step_by(2).zip(
            cli.gate()
                .iter()
                .step_by(2)
                .zip(cli.gate().iter().skip(1).step_by(2)),
        ) {
            ordered.push((index, gate::named(ctx, name.as_str(), command.clone())?));
        }
    }
    ordered.sort_by_key(|entry| entry.0);
    let requests = gate::GateSet::new(ordered.into_iter().map(|entry| entry.1).collect())
        .map_err(|error| FactorError::Usage(non_empty_msg(error.to_string())))?;
    Ok(requests)
}
/// Admits and persists the initial selected range before any native mutation.
#[expect(
    clippy::single_call_fn,
    reason = "fresh admission is a distinct checkpoint runtime transition"
)]
fn start(ctx: &Ctx<'_>, cli: &super::Cli, requests: gate::GateSet) -> Result<i32, FactorError> {
    let refs = cli.commits();
    let directory = state_dir(ctx)?;
    if lease_current(ctx)?.is_some() {
        return Err(failure("unknown foreign session lease must be preserved"));
    }
    if path_present(ctx, &journal_path(ctx)?)? {
        return Err(FactorError::ActiveSession);
    }
    if path_present(ctx, &directory)? {
        return Err(failure(
            "existing legacy or active session must be finished or aborted with its originating version",
        ));
    }
    if is_mid_rebase_in(ctx) {
        return Err(FactorError::ActiveRebase);
    }
    for gate in requests.as_slice() {
        super::validate_exec_syntax(ctx, gate.command())?;
    }
    let values = if refs.is_empty() {
        vec![
            NonEmptyString::try_from("HEAD".to_owned())
                .map_err(|error| failure(&format!("HEAD: {error}")))?,
        ]
    } else {
        refs.to_vec()
    };
    let commits = resolve_commit_span(
        ctx,
        &NonEmpty::from_vec(values).ok_or_else(|| failure("missing range"))?,
    )?;
    let span = super::CommitSpan::new(
        commits.clone(),
        super::validation::base_parent_in(ctx, &commits.head)?,
    );
    if !super::validation::has_tree_change(ctx, &span)? {
        output::empty_selection(ctx)?;
        return Ok(super::EXIT_DATAERR);
    }
    let anchor = span.tip_commit().clone();
    let base = if span.is_root() {
        None
    } else {
        Some(commit_output(
            ctx,
            &["rev-parse", &format!("{}^", span.first_commit())],
        )?)
    };
    let checkpoint = commit_output(ctx, &["rev-parse", "HEAD"])?;
    ancestor(ctx, &anchor, &checkpoint)?;
    tracked_clean(ctx)?;
    admit_initial_untracked(ctx, base.as_ref(), &anchor, &checkpoint)?;
    // The selected tip supplies the combined message; descendants keep theirs.
    // Admit this existing UTF-8/nonempty grammar before publishing any session.
    let _tip_message = candidate_message(ctx, &anchor)?;
    for descendant in descendants(ctx, &anchor, &checkpoint)? {
        let _message = candidate_message(ctx, &descendant)?;
    }
    let branch = head_reference(ctx)?;
    if !branch.starts_with("refs/heads/") {
        return Err(failure("session start requires a checked-out branch"));
    }
    // Native todo commands are line-delimited; reject an unrepresentable callback
    // before publishing any journal or reachability lease.
    let _callback = exe(ctx)?;
    let mut j = Journal {
        format: Format::CheckpointV2,
        session: {
            let file = NamedTempFile::new().map_err(FactorError::StateWrite)?;
            ctx.fs
                .write_string(
                    file.path(),
                    &format!("{} {}", checkpoint, file.path().display()),
                )
                .map_err(FactorError::StateWrite)?;
            SessionId::new(git_output(
                ctx,
                &[
                    "hash-object",
                    "--no-filters",
                    &file.path().to_string_lossy(),
                ],
            )?)?
        },
        original_tip: anchor.clone(),
        branch: BranchRef::new(branch)?,
        checkpoint: checkpoint.clone(),
        final_tree: tree(ctx, &checkpoint)?,
        original_base: base.clone(),
        gates: requests,
        state: Phase::Preparing {
            tip: anchor,
            base,
            previous_lease: None,
        },
    };
    validate_for(ctx, &j, Admission::Resume)?;
    save(ctx, &mut j)?;
    prepare(ctx, j, Invocation::Start)
}

#[cfg(test)]
#[path = "engine_tests.rs"]
pub(in crate::git_factor) mod tests;

#[cfg(test)]
#[path = "engine_proptests.rs"]
mod proptests;
