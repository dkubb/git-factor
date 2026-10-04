# git-factor

`git-factor` splits one commit or a contiguous range into independently validated
atomic commits. Each successful split completes its rebase and records a durable
checkpoint before automatically opening the remaining change in a fresh round.
Later failed attempts cannot discard earlier completed splits.

```bash
cargo install --path . --force
git factor -h
git-factor --help
```

The package installs `git-factor` and `git-sequence-editor`. Building needs Rust
and Cargo. Runtime requires **released Git 2.56.0 or newer**. Version admission
accepts numeric `major.minor.patch`.
Unknown vendor suffixes and prerelease spellings are refused; this rule is not
an assurance about every vendor distribution.

The first checkpoint release supports macOS and Linux. Windows support is
deferred.

See [SKILL.md](./SKILL.md) for the agent workflow.

## Before starting

Start on an attached branch with tracked working bytes and index matching HEAD.
Unrelated untracked and ignored files can remain when they do not collide with
protected checkout paths. Intermediate range and descendant trees also matter.
Filesystem aliases and shared physical identities such as hard links can cause
refusal. Preserve unrelated work; do not automatically stash or delete it.

Gates must be deterministic tree checks with valid Bash syntax. They must
preserve repository bytes and commit metadata, and must not depend on commit
metadata, history or messages. Reading candidate files through HEAD is allowed. Use native Git hooks for message policy.
Put generated gate artifacts outside the checked tree.

## Commit and range inputs

```bash
git factor --gate test 'cargo test' --exec 'cargo fmt --check' HEAD
git factor --gate check 'just ci' HEAD~2 HEAD
```

Named `--gate NAME COMMAND` and legacy `--exec COMMAND` checks run separately
in supplied CLI order. Names begin with an ASCII letter and contain ASCII
letters, digits or hyphens. Names are unique without regard to case. Legacy
commands receive stable command-derived names; duplicate names are refused.

- `<rev>` selects one commit; omitted revisions default to HEAD.
- `<start> <end>` selects an inclusive contiguous range.
- `<start>..<end>` excludes start; `<start>^..<end>` includes it.

The selected tip must be an ancestor of the branch tip. Merge commits,
noncontiguous ancestry and symmetric difference (`...`) are refused. A range
replaces its internal seams with the combined tip change relative to its first
commit's parent. Root ranges use the empty tree. Descendants after the range
are replayed. A no-net-change selection, including cancelling changes, is
refused before session mutation.

## Capture atoms

```bash
git add --patch -- src/parser.rs
git factor --message 'Add parser setup'
git add --patch
git factor --message 'Refactor validation'
git factor --finish --message 'Fix remaining edge cases'
```

The combined change is left unstaged. Select one atom with `git add` or
`git add --patch`. `--message` submits directly; `--continue --message` is an
alias. Repeated messages form separate paragraphs like Git commit messages.

The candidate is an actual commit in an isolated worktree with real HEAD.
Its gates cannot see the unstaged remainder. Native hooks validate the final
stamped message; message rejection cannot publish the candidate. Git-factor
replays the remainder and subsequent history using ordered exec gates and
`--reschedule-failed-exec` and `--no-update-refs`. Unrelated branches and tags
keep their original object IDs; only the selected branch and owned lease/proof
refs change. It verifies the final branch tree against the
recorded original branch-tip tree, finishes the rebase and captures progress.

A fresh round then exposes only the remainder, excluding captured atoms. An
empty remainder completes normally without manufacturing an empty commit.
No single long-lived rebase spans all splits.

Passing evidence has this logical, unfolded value:

```text
Gate-test: <command-hash> <tree-hash>
```

Managed stamp values use native continuation lines with a 72-byte folding
target. A short named key keeps the command hash on its first line; longer
keys put both hashes on continuations. Keys and the original message body are
not wrapped, so native message hooks may still reject an overlong key or body.
Read the logical pair with
`git show -s --format='%(trailers:only,unfold=true)' HEAD`.

Matching command/tree proofs are reused. Stale stamps are removed before
checking and replaced only after success. Positive proof refs allow reuse
across rounds; changing the command or tree invalidates that evidence. Native
hooks still validate the final message when tree gates are cached.

## Recovery and progress

| Command | Effect |
|---|---|
| `git factor --message 'Add an atom'` | Validate staged atom and capture it |
| `git factor --continue` | Resume replay or interrupted recovery |
| `git factor --retry` | Unstage the current candidate in an open selection |
| `git factor --finish` | Validate all remaining change and finish |
| `git factor --abort` | Abandon the active attempt at the latest checkpoint |
| `git factor --status` | Observe phase and durable progress |

Retry is available only during an open Selecting phase. During Opening or
Replaying, use the emitted continue action or checkpoint-safe abort.
Without a message, finish uses the original selected tip message. Retry and
abort retain earlier completed atoms. Unsafe ref or filesystem interference is
refused rather than overwritten, including recreated deleted paths, ignored
files and physical aliases. Retry is not a way to discard unrelated user work.
Abort discards unfinished replay or conflict edits on attempt-owned paths. The
exposed selection pool and unrelated files must still pass preservation checks.

Resolve replay conflicts, stage the resolution, then run `git factor --continue`.
For replay gate failures, repair the environment or current replayed commit and
follow the reported action. Baseline failures allow same-tree environment or
message repair followed by continue. To change the baseline tree, abort, repair
the checkpoint and start again. Rejected staged candidates can be adjusted and
submitted again.

The journal and owned refs retain interrupted progress. Resuming owned callbacks
with `--continue` requires the original canonical executable path recorded in
the native rebase. A moved executable may perform checkpoint-safe `--abort`
after complete session and native-rebase ownership checks and protected user-work
admission, including an unfinished terminal native interval. Abort does not
execute changed callbacks. Do not edit callbacks, journal files or owned refs
to bypass admission. Legacy sessions require their originating version; manual
migration is unsupported.
Terminal cleanup removes session metadata; positive gate proofs may remain.

## Machine output

Successful session results, gate failures and recovery-required results are
normalized compact JSON on stdout with a final newline. Help, version and
other admission errors retain ordinary CLI behavior. A net-zero initial range
is refused before session mutation with exit data error and
`{"operation":"start","reason":"empty_change","result":"refused"}`. Gate and Git subprocess
diagnostics go to stderr. Inactive status is
`{"operation":"status","session":null}`. Active status reports checkpoint,
phase, completed split count, original target and rebase facts. Terminal cleanup
may also report its complete or aborted outcome.
Status does not change refs, the real index, journal, actors, or recovery
progress. Tracked-tree validation can run configured clean filters and create
unreferenced Git objects.

Selection results include available actions and file changes. Text changes have
line counts, binary changes have a binary classification, and non-UTF-8 paths
retain their byte arrays. Gate failure and paused replay use explicit
`gate_failed` and `recovery_required` results. Check both JSON and exit status;
follow emitted actions rather than inferring the phase from HEAD alone.
