---
name: git-factor
description: >-
  Split one commit or contiguous range into atomic gate-passing commits,
  capturing each checkpoint and automatically opening the remainder.
compatibility: Unified agent skills CLI
version: 19
metadata:
  author: dkubb
  updated: "2026-10-02"
triggers:
  - "split a commit"
  - "factor a commit"
  - "break up a commit"
  - "split into atomic commits"
  - "re-split commits"
  - "git factor"
---

# Splitting commits with git-factor

Use this workflow for splitting or re-splitting commits. Inspect-only requests
and general rebase questions do not require a factor session. Plan small atoms
in dependency order. Use messages such as `Add parser`, `Fix empty input` and
`Refactor validation`.

## Inspect and start

The first checkpoint release supports macOS and Linux; Windows is deferred.

Confirm installed git-factor and released Git 2.56.0 or newer. Numeric
`major.minor.patch` versions are admitted; unknown vendor or prerelease
spellings are refused. Do not promise universal vendor compatibility.

Start on an attached branch with tracked bytes and index equal to HEAD.
Unrelated untracked/ignored files can remain only where protected checkout paths
and physical identities cannot replace them. Inspect user work before starting;
never automatically stash, clean or delete it to make admission pass.

Choose strict deterministic tree checks. They must preserve bytes and metadata
and must not depend on commit metadata, history or messages. Put
artifacts outside the checked tree. Message policy belongs in native Git hooks.

```bash
git factor --gate test 'cargo test' --exec 'cargo fmt --check' HEAD
```

Checks run separately in CLI order. Named gates begin with an ASCII letter and
contain ASCII letters, digits or hyphens; case-insensitive duplicates are refused.
Legacy exec commands get stable command-derived names.

`COMMIT` selects one commit; omitted revisions default to HEAD. `START END` is
inclusive. `START..END` excludes START, while `START^..END` includes it. The tip
must be an ancestor of the branch tip. Merges, noncontiguous ancestry and `...`
are refused. A range replaces its internal seams with the combined tip change
relative to the first parent; root ranges use the empty tree. Subsequent history
is replayed. A no-net-change range is refused before mutation.

## Select and capture

```bash
git add --patch -- src/parser.rs
git factor --message 'Add parser setup'
```

The pool is unstaged. Select exactly one atom with git add or git add --patch.
`--continue --message` also submits; repeated messages create paragraphs. Avoid
moving unrelated work into the index or editing ownership metadata.

The actual candidate has real HEAD in an isolated worktree; gates cannot see
the unstaged remainder. Evidence is stamped as
`Gate-NAME: <command-hash> <tree-hash>` when unfolded. Stamp values use native
continuation lines with a 72-byte folding target; longer keys put both hashes
on continuations. Keys and the original body are not wrapped, and native hooks
may reject overlong keys or body lines. Read the logical pair with
`%(trailers:only,unfold=true)`. Matching proofs are reused, stale stamps removed
before checking, and positive evidence retained only after success.
Native hooks validate the final stamped message even when gates are cached.
Message rejection cannot publish the candidate.

Remainder and descendants replay with exec gates and --reschedule-failed-exec.
Final-tree verification preserves the recorded original branch-tip tree. A
successful split completes its rebase and captures a checkpoint, then starts a
fresh round on only the remainder. Earlier atoms survive later failures. An
empty remainder completes normally without a synthetic empty commit.

Repeat selection and submission. Keep the final remainder as one atom with:

```bash
git factor --finish --message 'Fix final edge cases'
```

Without a message, finish uses the original selected tip message. Do not start
another session inside the active one or manually stretch its rebase across
successive split rounds.

## Recover and observe

- Resolve replay conflicts, stage the resolution, then git factor --continue.
- Repair replay gate failures and follow the emitted continuation action.
- Repair baseline environment/message without changing its tree and continue.
  For a tree change, abort, repair the checkpoint and start again.
- Adjust rejected staged atoms and submit a message again.
- git factor --retry unstages the current candidate only in open Selecting.
  During Opening or Replaying, use emitted continue or checkpoint-safe abort.
- git factor --abort abandons the active attempt at the latest checkpoint.
- Retry and abort preserve previously completed atoms.

Abort discards unfinished replay/conflict edits on attempt-owned paths. The exposed
selection pool and unrelated files must still pass preservation checks.

Unsafe refs, foreign state and recreated deleted paths can block admission.
Ignored files, case aliases and shared hard-link identities are protected too.
Preserve the objects and resolve the reported boundary explicitly; refusal is
not permission to delete user files.

Resuming owned callbacks with --continue requires the original canonical
executable recorded in the rebase. A moved executable may use --abort after
complete session/native ownership and protected user-work admission, including
an unfinished terminal native interval. Abort does not execute changed callbacks.
Do not edit callbacks, journal or owned refs to bypass checks. Legacy sessions
require their originating version; do not migrate them by hand.

Status reports normalized JSON with session:null when inactive. Active status
reports phase, checkpoint, split count, original target and rebase facts;
terminal cleanup can report complete/aborted outcomes. Successful session results,
gate failures and recovery-required results are compact JSON on stdout; subprocess
diagnostics are stderr. Help, version and other admission errors retain ordinary
CLI behavior. A net-zero initial range is refused before session mutation with
exit data error and
`{"operation":"start","reason":"empty_change","result":"refused"}`.
Preserve both streams and check exit status. Selection data has typed text/binary
changes and available
actions; non-UTF-8 paths retain bytes. Follow emitted actions instead of guessing
the phase from HEAD.
Status does not change refs, the real index, journal, actors, or recovery
progress. Tracked-tree validation can run configured clean filters and create
unreferenced Git objects.

After capture, work on the automatically exposed remainder. After completion,
verify progress and final tree; no active session metadata should remain.
With --no-update-refs, unrelated branches and tags keep their original object
IDs; only the selected branch and owned lease/proof refs change.
Positive gate-proof refs may survive cleanup. Older references may describe the
retired long-lived-rebase workflow; current CLI help, emitted actions and runtime
admission are authoritative. See [README.md](./README.md).
