//! Ordered, tree-bound gate verification on an actual isolated commit.
//! Positive command/tree proofs survive message rejection and session abort.

use alloc::collections::BTreeSet;

use core::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};

use std::path::Path;

use super::types::{CommitSha, Sha};

use super::{
    Ctx, FactorError, NonEmptyString, TreeHash, command_status_with, ensure_repo_clean, git_output,
    git_raw_output, non_empty_msg, run_git, status_code,
};

use tempfile::NamedTempFile;

/// Native revision expression selecting the current commit tree.
const HEAD_TREE: &str = "HEAD^{tree}";

/// One named deterministic tree check, with command identity derived by Git.
#[derive(Clone, Debug)]
pub(in crate::git_factor) struct Gate {
    /// Exact command bytes.
    command: NonEmptyString,
    /// Git blob identity of the command.
    command_hash: Sha,
    /// Validated trailer suffix.
    name: GateName,
}

/// An admitted ASCII Git trailer suffix.
#[derive(Clone, Debug, Serialize)]
#[serde(transparent)]
pub(in crate::git_factor) struct GateName(NonEmptyString);

/// Ordered nonempty checks with case-insensitive trailer uniqueness.
#[derive(Clone, Debug)]
pub(in crate::git_factor) struct GateSet(Vec<Gate>);

/// Serialized gate specification; command identity is reconstructed at admission.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::git_factor) struct GateSpec {
    /// Exact command bytes.
    command: NonEmptyString,
    /// Validated trailer suffix.
    name: GateName,
}

/// Recognized trailer and its half-open span in the original message.
#[derive(Debug)]
struct Record {
    /// First byte after the original record.
    end: usize,
    /// Original trailer key.
    key: String,
    /// First byte of the original record.
    start: usize,
    /// Unfolded trailer value.
    value: String,
}

impl Gate {
    /// Borrow the exact command bytes.
    pub(in crate::git_factor) const fn command(&self) -> &NonEmptyString {
        &self.command
    }
}

impl GateName {
    /// Return the admitted trailer suffix.
    const fn as_str(&self) -> &str {
        self.0.as_str()
    }
    /// Reject names outside the trailer suffix grammar.
    fn new(name: String) -> Result<Self, FactorError> {
        validate_names(&[&name])?;
        Ok(Self(
            NonEmptyString::try_from(name).map_err(|_error| failure("empty gate name"))?,
        ))
    }
}

impl GateSet {
    /// Borrow the admitted ordered checks.
    pub(in crate::git_factor) fn as_slice(&self) -> &[Gate] {
        &self.0
    }
    /// Admit a nonempty ordered set with distinct case-folded names.
    pub(in crate::git_factor) fn new(gates: Vec<Gate>) -> Result<Self, FactorError> {
        if gates.is_empty() {
            return Err(failure("supply at least one gate"));
        }
        let mut names = BTreeSet::new();
        if gates
            .iter()
            .any(|gate| !names.insert(gate.name.as_str().to_ascii_lowercase()))
        {
            return Err(failure("gate names must be unique ASCII trailer names"));
        }
        Ok(Self(gates))
    }
}

impl Serialize for GateSet {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        #[derive(Serialize)]
        struct Specification<'gate> {
            /// Exact command bytes.
            command: &'gate NonEmptyString,
            /// Validated trailer suffix.
            name: &'gate GateName,
        }
        self.0
            .iter()
            .map(|gate| Specification {
                name: &gate.name,
                command: &gate.command,
            })
            .collect::<Vec<_>>()
            .serialize(serializer)
    }
}

impl fmt::Display for GateName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for GateName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(<String as Deserialize>::deserialize(deserializer)?).map_err(D::Error::custom)
    }
    fn deserialize_in_place<D>(deserializer: D, place: &mut Self) -> Result<(), D::Error>
    where
        D: Deserializer<'de>,
    {
        *place = Self::deserialize(deserializer)?;
        Ok(())
    }
}

/// Stamp metadata without repeating native hooks.
fn amend_message(ctx: &Ctx<'_>, message: &str) -> Result<(), FactorError> {
    let file = NamedTempFile::new().map_err(FactorError::StateWrite)?;
    ctx.fs
        .write_string(file.path(), message)
        .map_err(FactorError::StateWrite)?;
    let path = file
        .path()
        .to_str()
        .ok_or_else(|| failure("gate message path is not valid UTF-8"))?;
    // Hook/message validation belongs to candidate creation. Stamping must not rerun hooks.
    run_git(
        ctx,
        &[
            "-c",
            "core.hooksPath=/dev/null",
            "commit",
            "--quiet",
            "--amend",
            "--only",
            "--allow-empty",
            "--no-verify",
            "--no-post-rewrite",
            "--no-gpg-sign",
            "--cleanup=verbatim",
            "-F",
            path,
        ],
    )
}

/// Append one trailer while retaining the original message bytes.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "trailer surgery preserves message spans before metadata amendment"
    )
)]
#[expect(
    clippy::arithmetic_side_effects,
    clippy::string_slice,
    reason = "private record offsets belong to this exact message, are ordered UTF-8 boundaries <= message.len(), and any included ASCII newline fits in the message"
)]
fn append_record(message: &str, records: &[Record], key: &str, value: &str) -> String {
    let mut trailer = format!("{key}:");
    for field in value.split(' ') {
        let line = trailer.rsplit('\n').next().unwrap_or_default();
        let width = line
            .len()
            .checked_add(field.len())
            .and_then(|size| size.checked_add(1));
        if width.is_none_or(|size| size > 72) {
            trailer.push('\n');
        }
        trailer.push(' ');
        trailer.push_str(field);
    }
    trailer.push('\n');
    records.last().map_or_else(
        || {
            let mut result = message.to_owned();
            if !result.ends_with('\n') {
                result.push('\n');
            }
            if !result.ends_with("\n\n") {
                result.push('\n');
            }
            result.push_str(&trailer);
            result
        },
        |last| {
            let end = last.end + usize::from(message.as_bytes().get(last.end) == Some(&b'\n'));
            let mut result = message[..end].to_owned();
            if !result.ends_with('\n') {
                result.push('\n');
            }
            result.push_str(&trailer);
            result.push_str(&message[end..]);
            result
        },
    )
}

// Compare the raw author record: name, email, timestamp and timezone all retain identity.
/// Read the raw author header without normalizing date or timezone.
fn author_record(ctx: &Ctx<'_>) -> Result<Vec<u8>, FactorError> {
    let observed = git_raw_output(ctx, &["cat-file", "commit", "HEAD"])?;
    if !observed.status.success() {
        return Err(failure("cannot observe original candidate author"));
    }
    observed
        .stdout
        .split(|byte| *byte == b'\n')
        .take_while(|line| !line.is_empty())
        .find(|line| line.starts_with(b"author "))
        .map(<[u8]>::to_vec)
        .ok_or_else(|| failure("candidate commit lacks an author record"))
}

/// Bind persisted specifications to their actual command hashes before use.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "journal specifications acquire command identity at one admission boundary"
    )
)]
pub(in crate::git_factor) fn bind(
    ctx: &Ctx<'_>,
    specs: Vec<GateSpec>,
) -> Result<GateSet, FactorError> {
    GateSet::new(
        specs
            .into_iter()
            .map(|spec| named(ctx, spec.name.as_str(), spec.command))
            .collect::<Result<Vec<_>, _>>()?,
    )
}

/// Read the current commit message verbatim.
fn commit_message(ctx: &Ctx<'_>) -> Result<String, FactorError> {
    object_message(ctx, "HEAD")
}

/// Classify a rejected gate contract as a Git failure.
fn failure(message: &str) -> FactorError {
    FactorError::GitCommand(non_empty_msg(message.to_owned()))
}

/// Derive a Git blob digest without publishing an object.
fn hash_command(ctx: &Ctx<'_>, command: &str) -> Result<Sha, FactorError> {
    let file = NamedTempFile::new().map_err(FactorError::StateWrite)?;
    ctx.fs
        .write_string(file.path(), command)
        .map_err(FactorError::StateWrite)?;
    let path = file
        .path()
        .to_str()
        .ok_or_else(|| failure("gate command path is not valid UTF-8"))?;
    // No -w: calculate Git's blob digest without writing an object or updating a ref.
    Sha::parse(git_output(ctx, &["hash-object", "--no-filters", path])?)
        .map_err(|_error| failure("invalid command hash"))
}

// Remove every invalid configured stamp before any normal gate can fail or interrupt.
/// Remove all invalid configured stamps before any gate executes.
#[expect(
    clippy::single_call_fn,
    reason = "all configured stale proofs must be invalidated before the ordered loop"
)]
fn invalidate_stale(ctx: &Ctx<'_>, gates: &GateSet, tree: &TreeHash) -> Result<(), FactorError> {
    let mut message = commit_message(ctx)?;
    let mut changed = false;
    for gate in gates.as_slice() {
        let records = recognized_records(ctx, &message)?;
        let key = format!("Gate-{}", gate.name);
        let own = records
            .iter()
            .filter(|record| record.key.eq_ignore_ascii_case(&key))
            .collect::<Vec<_>>();
        if !own.is_empty()
            && (!matches!(own.as_slice(), [record] if record.value == format!("{} {tree}", gate.command_hash)))
        {
            message = remove_records(&message, &records, &key);
            changed = true;
        }
    }
    if changed {
        amend_message(ctx, &message)?;
    }
    Ok(())
}

/// Constructs a legacy exec gate whose name remains stable when reordered.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "legacy CLI commands acquire stable names at one admission boundary"
    )
)]
pub(in crate::git_factor) fn legacy(
    ctx: &Ctx<'_>,
    command: NonEmptyString,
) -> Result<Gate, FactorError> {
    let command_hash = hash_command(ctx, command.as_str())?;
    Ok(Gate {
        name: GateName::new(format!("exec-{command_hash}"))?,
        command,
        command_hash,
    })
}

/// Constructs a named gate. Command bytes, including whitespace, determine identity.
pub(in crate::git_factor) fn named(
    ctx: &Ctx<'_>,
    name: &str,
    command: NonEmptyString,
) -> Result<Gate, FactorError> {
    let admitted_name = GateName::new(name.to_owned())?;
    let command_hash = hash_command(ctx, command.as_str())?;
    Ok(Gate {
        name: admitted_name,
        command,
        command_hash,
    })
}

/// Read a commit message after the raw header boundary.
fn object_message(ctx: &Ctx<'_>, commit: &str) -> Result<String, FactorError> {
    let observed = git_raw_output(ctx, &["cat-file", "commit", commit])?;
    if !observed.status.success() {
        return Err(failure("cannot read candidate commit message"));
    }
    let raw = String::from_utf8(observed.stdout)
        .map_err(|_error| failure("candidate commit message is not valid UTF-8"))?;
    raw.split_once("\n\n")
        .map(|(_, message)| message.to_owned())
        .ok_or_else(|| failure("candidate commit lacks a message boundary"))
}

/// Track original trailer spans and continuation values.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "offset is the start of this exact message paragraph; each cumulative ASCII-newline boundary is ordered and <= message.len()"
)]
#[expect(
    clippy::indexing_slicing,
    reason = "active indices are assigned only from records.len() immediately before pushing their corresponding record"
)]
fn parse_records(text: &str, offset: usize) -> Vec<Record> {
    let mut records: Vec<Record> = Vec::new();
    let mut position = offset;
    let mut active = None;
    for line in text.split_inclusive('\n') {
        let raw = line.trim_end_matches('\n');
        if raw.starts_with([' ', '\t']) {
            if let Some(index) = active {
                let previous: &mut Record = &mut records[index];
                if !previous.value.is_empty() {
                    previous.value.push(' ');
                }
                previous.value.push_str(raw.trim());
                previous.end = position + line.len();
            }
        } else if let Some((key, value)) = raw.split_once(':') {
            let trimmed_key = key.trim();
            if !trimmed_key.is_empty()
                && trimmed_key
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            {
                active = Some(records.len());
                records.push(Record {
                    key: trimmed_key.to_owned(),
                    value: value.trim().to_owned(),
                    start: position,
                    end: position + line.len(),
                });
            } else {
                active = None;
            }
        } else {
            active = None;
        }
        position += line.len();
    }
    records
}

// Proof refs are positive evidence only. They deliberately survive session abort.
/// Address the durable positive proof for a command and tree.
fn proof_ref(gate: &Gate, tree: &TreeHash) -> String {
    format!("refs/factor/gates/{}/{tree}", gate.command_hash)
}

/// Retain the current passing commit under its proof reference.
fn publish_proof(ctx: &Ctx<'_>, gate: &Gate, tree: &TreeHash) -> Result<(), FactorError> {
    run_git(ctx, &["update-ref", &proof_ref(gate, tree), "HEAD"])
}

/// Map Git-recognized trailers to their original message spans.
fn recognized_records(ctx: &Ctx<'_>, message: &str) -> Result<Vec<Record>, FactorError> {
    let file = NamedTempFile::new().map_err(FactorError::StateWrite)?;
    ctx.fs
        .write_string(file.path(), message)
        .map_err(FactorError::StateWrite)?;
    let path = file
        .path()
        .to_str()
        .ok_or_else(|| failure("gate trailer path is not valid UTF-8"))?;
    let interpreted = git_output(
        ctx,
        &[
            "-c",
            "trailer.separators=:",
            "interpret-trailers",
            "--only-input",
            "--only-trailers",
            "--no-unfold",
            "--no-divider",
            path,
        ],
    )?;
    if interpreted.is_empty() {
        return Ok(Vec::new());
    }
    let normalized = parse_records(&interpreted, 0);
    // Git recognizes the final paragraph. Preserve source spelling and continuations.
    let end = message.trim_end_matches('\n').len();
    #[expect(
        clippy::string_slice,
        reason = "trim_end_matches supplies a UTF-8 boundary no greater than this message length"
    )]
    let trimmed = &message[..end];
    let start = trimmed.rfind("\n\n").map_or(0, |position| {
        #[expect(clippy::arithmetic_side_effects, reason = "the matched two ASCII newlines fit entirely inside this message, so position + 2 <= message.len()")]
        { position + 2 }
    });
    #[expect(
        clippy::string_slice,
        reason = "start and end derive from ordered ASCII-newline boundaries in this exact message and both are <= message.len()"
    )]
    let records = parse_records(&message[start..end], start);
    let recognized = records
        .into_iter()
        .filter(|record| {
            normalized.iter().any(|candidate| {
                candidate.key.eq_ignore_ascii_case(&record.key) && candidate.value == record.value
            })
        })
        .collect::<Vec<_>>();
    // Refuse a grammar we cannot map losslessly; never silently retain managed stale stamps.
    if recognized.len() != normalized.len() {
        return Err(failure(
            "cannot map Git trailer records without changing the message",
        ));
    }
    Ok(recognized)
}

/// Remove only spans belonging to the selected trailer key.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "stale trailer surgery is isolated from native trailer recognition"
    )
)]
#[expect(
    clippy::string_slice,
    reason = "private record offsets belong to this exact message and are ordered UTF-8 slice boundaries <= message.len()"
)]
fn remove_records(message: &str, records: &[Record], key: &str) -> String {
    let mut result = String::new();
    let mut position = 0;
    for record in records
        .iter()
        .filter(|record| record.key.eq_ignore_ascii_case(key))
    {
        result.push_str(&message[position..record.start]);
        position = record.end;
    }
    result.push_str(&message[position..]);
    result
}

/// Validate positive evidence before skipping a command.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "positive cache evidence is admitted through one verification boundary"
    )
)]
fn reusable_proof(ctx: &Ctx<'_>, gate: &Gate, tree: &TreeHash) -> Result<bool, FactorError> {
    let reference = proof_ref(gate, tree);
    let observed = git_raw_output(ctx, &["rev-parse", "--verify", "--quiet", &reference])?;
    if !observed.status.success() {
        let absent_proof_code: i32 = 1;
        if observed.status.code() == Some(absent_proof_code)
            && observed.stdout.is_empty()
            && observed.stderr.is_empty()
        {
            return Ok(false);
        }
        return Err(failure("cannot observe positive gate proof"));
    }
    let raw_commit = String::from_utf8(observed.stdout)
        .map_err(|_error| failure("gate proof object is not valid UTF-8"))?;
    let commit = CommitSha::new(raw_commit.trim().to_owned())?;
    let pointed_tree = git_output(ctx, &["rev-parse", &format!("{commit}^{{tree}}")])?;
    if pointed_tree != tree.to_string() {
        return Ok(false);
    }
    let message = object_message(ctx, commit.as_str())?;
    let records = recognized_records(ctx, &message)?;
    let expected = format!("{} {tree}", gate.command_hash);
    Ok(records.iter().any(|record| {
        record
            .key
            .get(..5)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("Gate-"))
            && record
                .key
                .get(5..)
                .is_some_and(|name| validate_names(&[name]).is_ok())
            && record.value == expected
            && records
                .iter()
                .filter(|other| other.key.eq_ignore_ascii_case(&record.key))
                .count()
                == 1
    }))
}

/// Run native hooks and reject changed commit identity or gate stamps.
#[expect(
    clippy::single_call_fn,
    reason = "native hooks validate the final stamped commit independently"
)]
fn validate_final_message(
    ctx: &Ctx<'_>,
    hooks: &Path,
    tree: &TreeHash,
    gates: &GateSet,
    original_author: &[u8],
) -> Result<(), FactorError> {
    let before_parents = git_output(ctx, &["show", "--format=%P", "--no-patch", "HEAD"])?;
    let before_ref = git_output(ctx, &["rev-parse", "--symbolic-full-name", "HEAD"])?;
    let message = commit_message(ctx)?;
    let file = NamedTempFile::new().map_err(FactorError::StateWrite)?;
    ctx.fs
        .write_string(file.path(), &message)
        .map_err(FactorError::StateWrite)?;
    let path = file
        .path()
        .to_str()
        .ok_or_else(|| failure("gate message path is not valid UTF-8"))?;
    let hooks_path = hooks
        .to_str()
        .ok_or_else(|| failure("gate hooks path is not valid UTF-8"))?;
    run_git(
        ctx,
        &[
            "-c",
            &format!("core.hooksPath={hooks_path}"),
            "commit",
            "--quiet",
            "--amend",
            "--only",
            "--allow-empty",
            "--cleanup=verbatim",
            "-F",
            path,
        ],
    )?;
    ensure_repo_clean(ctx, "message hooks must preserve the selected tree")?;
    if git_output(ctx, &["rev-parse", HEAD_TREE])? != tree.to_string()
        || git_output(ctx, &["show", "--format=%P", "--no-patch", "HEAD"])? != before_parents
        || git_output(ctx, &["rev-parse", "--symbolic-full-name", "HEAD"])? != before_ref
    {
        return Err(failure("message hooks changed the selected tree or parent"));
    }
    if author_record(ctx)? != original_author {
        return Err(failure("message hooks changed the original author"));
    }
    // Message hooks may edit messages, but cannot erase or falsify a passing stamp.
    let final_message = commit_message(ctx)?;
    let records = recognized_records(ctx, &final_message)?;
    for gate in gates.as_slice() {
        let key = format!("Gate-{}", gate.name);
        let own = records
            .iter()
            .filter(|record| record.key.eq_ignore_ascii_case(&key))
            .collect::<Vec<_>>();
        if !matches!(own.as_slice(), [record] if record.value == format!("{} {tree}", gate.command_hash))
        {
            return Err(failure("message hooks changed a passing gate stamp"));
        }
    }
    Ok(())
}

/// Rejects aliases that could address the same Git trailer or inject metadata.
pub(in crate::git_factor) fn validate_names(names: &[&str]) -> Result<(), FactorError> {
    let mut seen = BTreeSet::new();
    for name in names {
        let mut bytes = name.bytes();
        if !bytes.next().is_some_and(|byte| byte.is_ascii_alphabetic())
            || !bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            || !seen.insert(name.to_ascii_lowercase())
        {
            return Err(failure("gate names must be unique ASCII trailer names"));
        }
    }
    Ok(())
}

#[cfg(test)]
/// Exercise gate verification without a physical observation adapter.
fn verify(ctx: &Ctx<'_>, gates: &[Gate], hooks: &Path) -> Result<CommitSha, FactorError> {
    verify_observed(ctx, &GateSet::new(gates.to_vec())?, hooks, &|_| Ok(()))
}

/// Runs ordered tree gates, then independently validates the final stamped message natively.
/// A failed gate returns its ordinary `ExecFailed` error; earlier positive stamps survive.
#[cfg_attr(
    not(test),
    expect(
        clippy::single_call_fn,
        reason = "candidate validation owns the complete gate protocol"
    )
)]
pub(in crate::git_factor) fn verify_observed(
    ctx: &Ctx<'_>,
    gates: &GateSet,
    hooks: &Path,
    observe: &dyn Fn(&TreeHash) -> Result<(), FactorError>,
) -> Result<CommitSha, FactorError> {
    ensure_repo_clean(ctx, "gate verification requires a clean candidate")?;
    let expected_tree = TreeHash::new(&git_output(ctx, &["rev-parse", HEAD_TREE])?)?;
    let original_author = author_record(ctx)?;
    observe(&expected_tree)?;
    invalidate_stale(ctx, gates, &expected_tree)?;
    for gate in gates.as_slice() {
        let message = commit_message(ctx)?;
        let records = recognized_records(ctx, &message)?;
        let key = format!("Gate-{}", gate.name);
        let value = format!("{} {}", gate.command_hash, expected_tree);
        let own = records
            .iter()
            .filter(|record| record.key.eq_ignore_ascii_case(&key))
            .collect::<Vec<_>>();
        let current_proof = matches!(own.as_slice(), [record] if record.value == value);
        let cached = current_proof || reusable_proof(ctx, gate, &expected_tree)?;
        if current_proof {
            publish_proof(ctx, gate, &expected_tree)?;
            continue;
        }
        if !cached {
            let before = CommitSha::new(git_output(ctx, &["rev-parse", "HEAD"])?)?;
            let before_ref = git_output(ctx, &["rev-parse", "--symbolic-full-name", "HEAD"])?;
            let status =
                command_status_with(ctx, "bash", &["-c", gate.command.as_str()], &[], false)?;
            // Validate effects even when the gate reports failure; its mutation must not be stamped.
            ensure_repo_clean(ctx, "normal gates must preserve the selected tree")?;
            let after = CommitSha::new(git_output(ctx, &["rev-parse", "HEAD"])?)?;
            if before != after
                || git_output(ctx, &["rev-parse", "--symbolic-full-name", "HEAD"])? != before_ref
                || TreeHash::new(&git_output(ctx, &["rev-parse", HEAD_TREE])?)? != expected_tree
            {
                return Err(failure("normal gate changed HEAD or its selected tree"));
            }
            observe(&expected_tree)?;
            if !status.success() {
                return Err(FactorError::ExecFailed {
                    code: status_code(status),
                    command: gate.command.clone(),
                });
            }
        }
        let unstamped_message = commit_message(ctx)?;
        let unstamped_records = recognized_records(ctx, &unstamped_message)?;
        let stamped = append_record(&unstamped_message, &unstamped_records, &key, &value);
        amend_message(ctx, &stamped)?;
        publish_proof(ctx, gate, &expected_tree)?;
    }
    validate_final_message(ctx, hooks, &expected_tree, gates, &original_author)?;
    observe(&expected_tree)?;
    CommitSha::new(git_output(ctx, &["rev-parse", "HEAD"])?)
}

#[cfg(test)]
#[path = "gate_proptests.rs"]
mod proptests;

#[cfg(test)]
#[path = "gate_tests.rs"]
mod tests;
