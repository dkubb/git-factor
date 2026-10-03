# git-factor

`git-factor` splits one Git commit, or one contiguous commit span, into smaller
atomic commits while proving each emitted commit passes a deterministic gate.

Once installed, use it as a normal Git subcommand:

```bash
git factor -h
```

Use the standalone binary for the full long help output:

```bash
git-factor --help
```

For the agent-oriented workflow and operating rules, see
[SKILL.md](./SKILL.md).

## What It Does

`git factor` helps you:

- prove the current target state passes a gate before splitting starts
- stage one atomic slice at a time
- commit each slice only after the gate passes
- restore the remaining unstaged pool after each successful split
- finish with a final tree that matches the original gated tip tree

This makes it useful for turning a large commit, or a messy patch span, into a
series of smaller reviewable commits.

## Installation

### From This Repository

```bash
cargo install --path . --force
```

This installs both:

- `git-factor`
- `git-sequence-editor`

Make sure Cargo's bin directory is on your `PATH`:

```bash
export PATH="${HOME}/.cargo/bin:${PATH}"
```

Then verify the install:

```bash
git factor -h
git-factor --version
git factor --version
```

## Requirements

- Git 2.38 or newer
- Rust and Cargo
- a fully clean repository before `git factor --exec ...`
- a deterministic validation command for `--exec`

The start gate must run on a clean repository and must leave the repository
clean. If `git status --porcelain=v1` is not empty, fix that first by
committing, stashing, or removing local changes.

## Basic Workflow

### Split One Commit

```bash
git factor --exec 'just ci' HEAD
```

Then:

```bash
git add --patch
git factor --continue --message 'feat: extract parser setup'
git add --patch
git factor --continue --message 'refactor: isolate validation'
git factor --finish --message 'feat: restore remaining workflow'
```

### Refactor a Whole Commit Span

```bash
git factor --exec 'just ci' HEAD~2 HEAD
```

This does **not** preserve the original seams inside the range. Instead, it:

1. proves the tip of the selected span is green
2. records that tip tree as the target
3. turns the whole span into one remaining pool
4. lets you emit a new sequence of smaller commits

## Accepted Commit Inputs

Documented forms:

- `<rev>`: split one commit
- `<start> <end>`: split one inclusive span

Git-native forms also work:

- `<start>..<end>`: exclusive-start range
- `<start>^..<end>`: inclusive-start range

Examples:

```bash
git factor --exec 'cargo test' HEAD
git factor --exec 'just ci' HEAD~3 HEAD
git factor --exec 'cargo test' HEAD~3..HEAD
git factor --exec 'cargo test' HEAD~3^..HEAD
```

Restrictions:

- the selected commits must resolve to one contiguous ancestry span
- merge commits in the span are rejected
- symmetric diff (`...`) is not supported

## Session Commands

Start:

```bash
git factor --exec 'just ci' HEAD
```

Commit the currently staged slice:

```bash
git factor --continue --message 'feat: add parser'
```

Discard the current split attempt and restore the remaining pool:

```bash
git factor --retry
```

Finish by committing the remaining pool:

```bash
git factor --finish
```

Abort the active session and restore the repository:

```bash
git factor --abort
```

Inspect the current session:

```bash
git factor --status
```

## How The Gate Works

The `--exec` command is your quality gate.

- The start gate must pass before the factor session opens.
- The same gate runs before each `--continue` commit is created.
- `git factor` restores the remaining unstaged pool after each successful
  `--continue`.
- `--finish` verifies the final tree matches the recorded gated tip tree.

Use the strictest deterministic gate you have. Typical choices:

```bash
git factor --exec 'just ci' HEAD
git factor --exec 'cargo test' HEAD
git factor --exec 'npm test' HEAD
```

## Recovery

If the start gate fails, fix the target commit first.

Single-commit sessions:

1. fix the files
2. stage the intended changes
3. amend the commit
4. rerun `git factor --exec ...`

Range sessions:

1. fix the current tip commit
2. stage the intended changes
3. amend the commit
4. run `git rebase --continue`

If you stage the wrong slice during an active session:

```bash
git factor --retry
```

That keeps the session active and restores the remaining pool so you can try
again.

## Practical Advice

- Start with the strictest passing gate.
- Split easy leaf nodes first.
- Prefer coarse splits before fine-grained ones.
- Finish a session once the next split stops being trivial, then factor the new
  top commit again if needed.
- Keep each commit review-sized and independently valid.

## Related Documentation

- [SKILL.md](./SKILL.md): agent workflow and detailed operational guidance

## Status Output

`git factor --status` writes one JSON object followed by a newline. An inactive
session is `{"operation":"status","session":null}`. An active session exposes
`phase`, `split_count`, `rebase.in_progress`, `rebase.required`, and
`target.commit`, `target.index`, `target.span_starts_at_root`.

`target.index` is the zero-based position in the saved selected commit sequence;
`target.commit` is that position's current commit. The root flag describes the
selected span's original root boundary. Status observes the current session.

## Abort Output

`git factor --abort` reports the existing cleanup result as one JSON object,
followed by a newline. `rebase.in_progress` is observed after factor state is
removed. When true, `actions.abort_rebase` contains `["git", "rebase", "--abort"]`;
otherwise `actions` is empty. Existing reset, cleanup and external-rebase
behavior is unchanged.
