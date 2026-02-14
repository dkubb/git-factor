# Git-Factor Deterministic Trace Spec (v1)

## Goal
Capture enough deterministic session data to replay and debug `git factor` behavior across machines and Codex sessions.

## Enable tracing
Set one environment variable before running `git factor`:

```bash
export GIT_FACTOR_TRACE_LOG=/absolute/path/to/factor-trace.jsonl
```

When unset, tracing is disabled and behavior is unchanged.

## Format
- File format: JSON Lines (one JSON object per line)
- Event ordering: append-only in execution order
- Timestamp field: `ts_unix_ms` (UTC epoch milliseconds)

## Event types

### 1. `process`
Recorded for every subprocess call run by `git-factor` wrappers (`git` and `bash`).

Fields:
- `event`: `"process"`
- `mode`: `"status" | "output"`
- `bin`: executable name (`git`, `bash`, etc.)
- `args`: argv array
- `env`: explicit env vars passed to child process (e.g. `GIT_SEQUENCE_EDITOR=...`)
- `quiet`: whether subprocess stdout/stderr were silenced
- `spawned`: whether process spawn succeeded
- `duration_ms`: wall-clock runtime
- `exit_code`: process exit code or `null` on spawn failure
- `stdout`: captured stdout (trimmed to max bytes)
- `stderr`: captured stderr (trimmed to max bytes)

State snapshots are attached both before and after each process call:
- `before_head`, `after_head`
- `before_head_tree`, `after_head_tree`
- `before_git_dir`, `after_git_dir` (absolute git dir; critical for worktree session reconstruction)
- `before_toplevel`, `after_toplevel`
- `before_staged_paths`, `after_staged_paths`
- `before_unstaged_paths`, `after_unstaged_paths`
- `before_untracked_paths`, `after_untracked_paths`
- `before_factor_current_index`, `after_factor_current_index`
- `before_factor_split_count`, `after_factor_split_count`
- `before_factor_requires_rebase`, `after_factor_requires_rebase`
- `before_factor_expected_tree`, `after_factor_expected_tree`
- `before_factor_current_commit`, `after_factor_current_commit`
- `before_rebase_state`, `after_rebase_state` (`rebase-merge`, `rebase-apply`, or `null`)
- `before_rebase_msgnum`, `after_rebase_msgnum`
- `before_rebase_end`, `after_rebase_end`
- `before_rebase_todo_head`, `after_rebase_todo_head`
- `before_rebase_done_tail`, `after_rebase_done_tail`

### 2. semantic note events
Recorded for important `git-factor` decisions:
- `factor_cmd_start`
- `factor_cmd_continue`
- `factor_cmd_finish`
- `factor_cmd_abort`
- `expected_tree_source` (state vs original commit fallback)
- `tree_compare_continue`
- `tree_compare_finish`

Each note includes `state_*` snapshot fields (same schema as above, but with `state_` prefix), plus event-specific fields such as:
- `source`
- `expected_tree`
- `actual_tree`
- `converged`
- `original_commit`

## Deterministic replay checklist
A replay/debug agent should:
1. Recreate repo/worktree at `before_head` for the first failing action.
2. Reconstruct factor state from `before_factor_*` fields and `factor/` files implied by snapshots.
3. Reconstruct rebase phase from `before_rebase_*` fields.
4. Re-run failing `process` event command (`bin` + `args` + `env`) in that state.
5. Verify resulting `after_*` snapshot equivalence.
6. For tree mismatches, use `tree_compare_*` + `expected_tree_source` to determine whether mismatch came from stale expected-tree provenance vs actual index/tree divergence.

## Hand-off contract for other Codex sessions
When reporting a bug, provide:
- The `factor-trace.jsonl` file
- The failing event line number(s)
- The first event where expected vs actual diverged
- Repo URL + commit under test
- OS + git version

This is sufficient for deterministic triage without relying on mutable shell history.
