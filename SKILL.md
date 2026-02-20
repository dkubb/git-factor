---
name: git-factor
description: Splits a large git commit into smaller atomic commits using interactive rebase with exec-gate validation. Use when splitting commits, breaking up a commit, factoring commits, decomposing a commit, "split this commit", "break this into smaller commits", "factor this commit", "this commit is too big", commit is too large, split HEAD, split into atomic, interactive split, guided commit splitting, splitting with tests, exec gate, validated splitting, or git factor.
---

# Git Factor Skill

Splits large git commits into smaller, atomic commits with exec-gate
validation ensuring every intermediate commit passes tests. Available
as `git factor` (and companion `git sequence-editor`) on PATH.

Claude has no training data on this tool. This skill is the
authoritative reference.

## How It Works

`git factor` resets a target commit so its changes appear as
unstaged/untracked files in the working tree. You then reassemble
the changes into smaller commits by staging slices one at a time.
Each slice is validated by an exec gate before it is accepted.

**Iterative, not one-shot**: Pull out obvious slices with
`--continue`, then `--finish` with everything remaining. If the
final commit is still too large, run `git factor` on it again.
Each pass peels off the easy wins. Within a pass, stage your
best guess — if the gate fails, everything is restored
automatically. Adjust and retry.

**Session lifecycle:** start → continue (repeat) → finish →
inspect → repeat if needed

## Quick Start

Run `git factor` commands in the **foreground** with a timeout
~2x the expected gate duration. This blocks until completion
without wasting tokens on monitoring.

```bash
# 1. Start session -- commit is reset, changes become unstaged
git factor --exec 'cargo test'

# 2. Stage one logical slice and commit it
git add src/parser.rs
git factor --continue -m 'refactor(parser): simplify token handling'

# 3. Repeat step 2 as many times as needed for additional slices
git add src/validator.rs
git factor --continue -m 'fix(validator): handle empty input'

# 4. When ready, finish with ALL remaining changes
git factor --finish -m 'feat(api): add /users endpoint'
# Omit -m to reuse the original commit message
```

## Critical Rules

- **NEVER use `git commit` during a session.** Only use
  `git factor --continue` and `git factor --finish`.
- Every `--continue` and `--finish` runs the exec gate.
- If the exec gate **fails** (exit 75), the tool automatically
  rolls back the slice and restores all changes. Restage
  differently and retry. **No data is lost** — explore freely.
- `--continue` requires staged changes. It will reject with
  exit 64 if nothing is staged.
- `--abort` cleanly restores to the pre-session state. Safe to
  use at any point.

## Key Flags

- `--exec <CMD>` -- validation gate (required to start, repeatable).
  Choose gates that **fail fast** (e.g., typecheck before full
  test suite) so iterations are cheap.
- `--continue` + `-m <MSG>` -- commit staged slice
- `--finish` + optional `-m <MSG>` -- commit ALL remaining changes
  (defaults to original commit message if `-m` omitted)
- `--abort` -- cancel session, reset to pre-session state
- `--status` -- show session details
- `[COMMIT]...` -- targets (SHAs, ranges like `A..B`; default
  `HEAD`)

## Why Atomic Commits

- Reviewers understand intent faster when each commit does one thing
- Smaller diffs make it easier to spot bugs during review
- `git bisect` can pinpoint exactly which change introduced a bug
- `git revert` can surgically undo a single change without collateral
  damage
- Test failures map directly to the commit that caused them,
  creating a tight feedback loop for iteration

## When to Use

- A commit mixes multiple logical changes that should be separate
- Every intermediate commit must compile and pass tests
- Splitting commits deeper in history (not just HEAD)

## References

- **CLI details**: See
  [references/CLI.md](references/CLI.md) --
  full flag reference, mutual exclusivity rules, exit codes
- **Workflows and examples**: See
  [references/WORKFLOWS.md](references/WORKFLOWS.md) --
  multi-commit ranges, root commits, exec gate failure recovery
