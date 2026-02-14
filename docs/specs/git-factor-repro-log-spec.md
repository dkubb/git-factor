# git-factor Reproduction Log Spec v1

## Goal

Capture enough deterministic execution data from `git factor` and
`git-sequence-editor` to let another Codex instance:

- diagnose a failure from one log artifact;
- replay the session in an isolated worktree; and
- prove whether behavior changed after a fix.

This spec defines a machine-readable event log plus a compact bundle of state
artifacts.

## Scope

- Commands: `git factor` (`start`, `continue`, `finish`, `abort`) and
  `git-sequence-editor`.
- Runs in normal repos and linked worktrees.
- Captures command intent, git topology checkpoints, factor state transitions,
  subprocess execution, and final outcome.

## Non-Goals

- Capturing full file contents for every commit.
- Recording all shell activity in the user terminal.
- Replacing CI logs or generic git tracing.

## Artifact Layout

All artifacts for one session live under one directory:

```text
git-factor-log-<session_id>/
  session.json
  events.jsonl
  factor-state/
    <snapshot files>
  commands/
    <event_id>-stdout.txt
    <event_id>-stderr.txt
  checksums.sha256
```

## Session Metadata (`session.json`)

Required fields:

- `schema_version`: `"1.0"`
- `session_id`: stable UUID for the session
- `tool`: `"git-factor"`
- `tool_version`: semantic version or git describe output
- `started_at`: RFC3339 UTC timestamp
- `host`:
  - `os`
  - `arch`
  - `hostname`
- `git`:
  - `version`
  - `repo_root`
  - `git_dir`
  - `worktree_root`
  - `is_linked_worktree`
- `process`:
  - `pid`
  - `ppid`
  - `cwd`
- `initial_refs`:
  - `head`
  - `head_tree`
  - `head_parent` (nullable for root)
  - `current_branch` (nullable in detached HEAD)

## Event Stream (`events.jsonl`)

One JSON object per line. Common required fields:

- `event_id`: monotonic integer
- `time`: RFC3339 UTC timestamp
- `type`: event type string
- `session_id`
- `op_id`: logical operation id (one CLI invocation)

### Event Types

1. `command_started`
- `argv`
- `env_allowlist` (selected env vars only; see redaction)
- `cwd`
- `state_dir` (`<git-dir>/factor` path if present)
- `factor_state_pre` (inline summary; full snapshots go in `factor-state/`)

2. `git_checkpoint`
- `head`
- `head_tree`
- `index_tree` (`git write-tree` when available)
- `status_porcelain_v2`
- `rebase_mode` (`none`, `rebase-merge`, `rebase-apply`)
- `rebase_head` (nullable)

3. `subprocess`
- `program`
- `args`
- `cwd`
- `exit_code`
- `duration_ms`
- `stdout_path` (nullable)
- `stderr_path` (nullable)

4. `factor_state_snapshot`
- `snapshot_name`
- `files` (names present under `<git-dir>/factor`)
- `key_values` (parsed values for known keys: `commits`, `current_index`,
  `split_count`, `requires_rebase`, `exec`, `is_root`, `expected_tree`)

5. `tree_validation`
- `expected_tree`
- `actual_tree`
- `source` (`state.expected_tree` or `commit_parent_tree`)
- `matched` (bool)

6. `command_finished`
- `exit_code`
- `result` (`ok`, `usage_error`, `git_error`, `exec_failed`, `panic`)
- `message` (short human summary)

## Deterministic Replay Contract

To replay a reported failure, a Codex instance must be able to:

1. recreate an isolated worktree from `initial_refs.head` (or equivalent
   commit);
2. restore recorded factor state snapshot for each op boundary;
3. execute recorded `argv` in order;
4. compare observed `git_checkpoint` and `tree_validation` events;
5. detect first divergence event id.

Replay should run in `/tmp` by default to avoid touching primary worktrees.

## Redaction and Safety

- Never log full environment by default.
- Allowlist env keys: `PATH`, `HOME`, `SHELL`, `TERM`, `USER`,
  `GIT_DIR`, `GIT_WORK_TREE`, `GIT_SEQUENCE_EDITOR`.
- Hash sensitive path segments when requested (`--redact-paths`).
- Store command stdout/stderr separately to keep event stream compact.
- Do not log file contents unless explicitly enabled (`--capture-patch`).

## CLI Surface (Proposed)

- `--log-session <dir>`: enable logging bundle output
- `--log-level <minimal|normal|debug>`:
  - `minimal`: command boundaries and final outcome
  - `normal`: + checkpoints and state snapshots
  - `debug`: + subprocess stdout/stderr capture for all git calls
- `--redact-paths`: anonymize absolute paths in event payloads

Environment override:

- `GIT_FACTOR_LOG_SESSION=<dir>`

## Backward Compatibility

- Unknown event fields must be ignored by readers.
- New event types must not break v1 parsers.
- Parsers should gate strictness on `schema_version`.

## Handoff Instructions for Other Codex Instances

When reporting a bug, provide:

- the `git-factor-log-<session_id>` directory;
- the failing command text; and
- whether the session was in a linked worktree.

With this bundle, Codex should identify the first divergence point without
asking for additional manual transcript details.

