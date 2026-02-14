# git-factor Grouped Start Spec v1

## Goal

Allow factoring multiple contiguous commits as one temporary commit, then split
that temporary commit into atomic commits with normal `git factor --continue` /
`git factor --finish`.

This is for cases where a set of adjacent commits should be re-factored in a
different order than the current history.

## Current Behavior (Baseline)

- `git factor` currently resolves selected commits and splits each selected
  commit individually.
- A pre-start exec gate is only run for single-commit `HEAD` sessions
  (`single_head_session` path in `src/git_factor.rs`).
- Multi-commit sessions rely on rebase `--exec` during the rebase flow.

## Scope

- Add grouped-start mode for `git factor` start command.
- Keep `--continue`, `--finish`, `--abort`, and `--status` behavior unchanged.
- Preserve deterministic logging/tracing and exact CLI messaging style.

## Non-Goals

- Grouping non-contiguous commits in v1.
- Grouping across merge commits.
- Supporting multiple independent grouped spans in one invocation.

## CLI Proposal

Working flag name: `--group`.

Usage:

```bash
git factor --group --exec 'just test' HEAD~3..HEAD
```

Constraints:

- `--group` is valid only on session start (same mode where `--exec` is used).
- `--group` requires at least 2 resolved commits.
- `--group` cannot be combined with `--continue`, `--finish`, `--abort`,
  `--status`, or `--message`.

Future naming option (if desired): add a clearer long-form alias such as
`--squash-input`, keep `--group` as an alias for ergonomics.

## Contiguity Definition

Resolved commits must form one contiguous first-parent chain in rebase order.

Given ordered commits `c1..cn` (oldest to newest), contiguity requires:

- `n >= 2`
- for every `i` in `[2..n]`, first-parent(`ci`) == `c(i-1)`

If this check fails, return a usage error with explicit guidance:

- "grouping requires contiguous commits"
- suggest passing a single contiguous range (for example `A..B`)

## Start Flow (Grouped Mode)

1. Resolve and topologically sort commits as normal.
2. Validate all existing preconditions (ancestor, non-merge, exec syntax).
3. Validate grouped contiguity (new).
4. Enter a preparatory non-interactive rebase from `parent(c1)` (or `--root`).
5. Rewrite the todo for the selected span:
   - `c1`: `pick`
   - `c2..cn`: `fixup`
   - insert `break` immediately after `cn` is applied
6. Preserve existing `--exec` behavior in the rebase arguments.
7. Ensure no editor prompt is possible:
   - `GIT_EDITOR=false`
   - `GIT_SEQUENCE_EDITOR=<git-sequence-editor command>`
8. Rebase stops at `break` with grouped commit at `HEAD`.
9. Run explicit pre-factor gate on this grouped commit before resetting:
   - `bash -c <combined exec command>`
10. If the gate fails:
   - abort start with `ExecFailed`
   - clean up factor state
   - leave clear next-step instructions
11. If the gate passes:
   - capture expected tree from grouped `HEAD`
   - initialize factor state for one split target
   - reset `HEAD~1` (or empty-root path) to expose grouped diff as unstaged
   - print session start output

## Session State Changes

No mandatory new state keys are required for correctness.

Optional recommended keys for clarity and debugging:

- `group_mode=true|false`
- `group_size=<N>`
- `group_original_commits=<sha1\nsha2\n...>`
- `start_head=<sha>`

`commits` should store the actual grouped target commit SHA used for metadata and
tree comparison in this session.

## Continue/Finish Behavior

Unchanged semantics:

- `--continue`: commit staged slice, run gate, compare against expected grouped
  tree, restore remainder.
- `--finish`: stage remaining grouped diff, verify tree convergence, run gate,
  create final split commit.
- After grouped target converges, advance/continue the rebase as today.

## Abort Semantics

Current baseline: `git factor --abort` aborts only the current factor step and
does not auto-abort the surrounding rebase.

Grouped recommendation (v1):

- Record `start_head` at session start (before preparatory squash rebase).
- If `group_mode=true`, `git factor --abort` should restore repository state to
  `start_head`:
  1. run `git rebase --abort` when a rebase is active,
  2. run `git reset --hard --quiet <start_head>`,
  3. run `git clean --force --quiet -d`,
  4. remove factor state directory.

Rationale:

- Grouped mode introduces an intentional temporary squashed commit.
- Users expect abort to roll back that temporary history rewrite.
- This makes grouped abort a one-command recovery path.

Compatibility note:

- Non-group sessions can keep current step-abort semantics.
- If desired later, add an explicit full-rollback flag (for example
  `--abort --full`) for non-group sessions.

## Error Handling

New explicit errors:

- `--group requires at least two contiguous commits`
- `--group is only valid for session start`
- `selected commits are not contiguous`

Sad-path requirements:

- No editor should open unexpectedly.
- Failures should emit actionable next steps.
- Factor state must not remain half-initialized on failed grouped start.

## Tracing and Repro Logging

Add grouped-specific trace events/fields:

- `group_mode`, `group_size`
- `group_first_commit`, `group_last_commit`
- `group_contiguous=true|false`
- `grouped_head_commit` after preparatory rebase stop
- `group_pre_gate_result` (exit code + command)

This ensures grouped-start failures are replayable from logs alone.

## Test Plan

### Unit tests

- Contiguity validator accepts linear adjacent commits.
- Contiguity validator rejects gaps and disjoint selections.
- CLI argument validation for `--group` combinations.

### Integration tests

- Happy path: group 2+ contiguous commits, split into new atomic commits.
- Pre-gate failure before split loop:
  - start fails
  - no active factor session remains
  - expected stderr/stdout guidance is emitted
- Non-contiguous input fails with usage guidance.
- Root-range grouped start works.
- Existing non-group behavior remains unchanged.
- Trace file includes grouped fields and expected side effects.

## Open Questions

1. Should v1 support only one grouped span per start (recommended), or multiple
   spans in one command?
2. Should we keep `--group` as final name, or add a clearer primary name such
   as `--squash-input` and keep `--group` as alias?
3. On grouped pre-gate failure, should we always auto-abort rebase, or leave
   repository at the grouped stop for manual inspection with explicit recovery
   commands?
