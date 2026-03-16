---
name: git-factor
description: >-
  This skill should be used when the user asks to "split a commit",
  "factor a commit", "break up a large commit", "split into atomic
  commits", "re-split commits", or mentions "git factor". It provides
  the step-by-step workflow for using the git-factor tool to split
  one commit or one contiguous commit span into smaller, atomic commits with
  rebase.
compatibility: Unified agent skills CLI
version: 17
metadata:
  author: dkubb
  updated: "2026-03-16"
triggers:
  - "split a commit"
  - "factor a commit"
  - "break up a commit"
  - "split into atomic commits"
  - "re-split commits"
  - "git factor"
---

# Splitting Commits with git-factor

Use this skill to split one commit or one contiguous commit span into small, reviewable,
atomic commits with `git factor`.

Keep steps short. Keep each commit small.

## When to Activate

- The user asks to split or factor a commit.
- The user asks to break up a large commit into atomic commits.
- The user asks to re-split an already rewritten commit.
- The task requires `git factor` workflows or debug help.

## When Not to Use

- The user only wants to inspect history without rewriting commits.
- The change is already in clean atomic commits.
- The user wants general interactive rebase help without factoring.

## Prerequisites

- `git-factor` and `git-sequence-editor` are available on `$PATH`.
- The repository must be fully clean before `git factor --exec`.
  - Require `git status --porcelain=v1` to be empty.
  - Treat any non-empty output as a blocker because start-gate results can be
    affected by local tracked, unstaged, or untracked state.
  - If the user wants to preserve local work, run
    `git stash push -u -m 'pre-factor stash'` and record the stash ID for later
    restore.
- Use the strictest available validation command for `--exec`.

## Commands

| Command | Purpose |
|---|---|
| `git factor --exec 'CMD' COMMIT` | Start a one-commit factor session |
| `git factor --exec 'CMD' START END` | Start an inclusive span factor session |
| `git factor --continue -m 'msg'` | Commit currently staged atom |
| `git factor --retry` | Discard current split attempt and restore the pool |
| `git factor --finish -m 'msg'` | Commit remaining atom with message |
| `git factor --finish` | Commit remaining atom with original message |
| `git factor --abort` | Abort active factor step |
| `git factor --status` | Show active session state and progress |

Fallback only when subcommand wiring is unavailable:

- `git-factor --exec/--continue/--finish/--abort`

## Inputs

- Target commit-ish or contiguous commit span.
- Validation gate command for `--exec`.
- Planned sequence of atomic commit messages.

## Outputs

1. A sequence of atomic commits preserving the gated tip tree.
2. A clean working state between `--continue` steps, excluding
   explicitly
   allowed gate artifacts.
3. Clear recovery guidance when gating or tree convergence fails.
4. No leftover `.git/factor` metadata after successful completion.

## Process

### 1) Plan the sequence first

- Identify atomic units and dependencies before running `git factor`.
- Choose commit order with dependencies before dependents.
- Decide whether you are preserving or replacing seams.
  - A single commit input preserves that seam and splits just that commit.
  - A range input replaces the old seams and refactors the whole span into a
    new commit story.
- Range semantics:
  - Documented forms are `<rev>` and `<start> <end>`.
  - Git-native dotted ranges are also accepted.
    - `<start>..<end>` uses git's exclusive-start range semantics.
    - `<start>^..<end>` is the git-native inclusive form.
  - Symmetric diff (`...`) is intentionally unsupported.
  - The selected commits must resolve to one contiguous ancestry span and
    must not include merge commits.
- Start coarse, then refine.
  - First peel off the largest unit on the dependency frontier.
  - Prefer a whole file or module before method-level slicing when both are
    independently valid.
  - After coarse splits are anchored, refactor or re-factor those commits
    further only if needed.
- Keep each atom small and independently valid.

Reference for decomposition details:

- `references/rust.md`
- `references/markdown.md`
- `references/commit-design.md`

### 2) Start with strict gates

```bash
git factor --exec 'VALIDATION_CMD' HEAD
```

- Multiple `--exec` flags are joined with `&&`.
- The combined gate runs before the session starts and before each split
  commit.
- The start gate establishes a green baseline commit for the session.
- For range inputs, the baseline is the gated tip state of the selected span.
  Old seams inside the span are discarded.
- The start gate must begin on a fully clean repository state and must leave
  the repository fully clean.
- Gate commands should be read-only relative to the repo.
  - Write artifacts under `/tmp`, not inside the repository.
- Choose the strictest deterministic gate that passes on the current branch.
  - If full CI fails due unrelated pre-existing issues, use the strongest
    passing subset and record the reason.
- For minimal fixtures (no `justfile`, no `Cargo.toml`), use the strongest
  deterministic structural gate available and record why.
  - Rust single file example:
    `LLVM_PROFILE_FILE=/tmp/%p.profraw rustc --crate-type lib app.rs -o /tmp/gate-check.rlib`
- Keep gate artifacts out of the repo.
  - Example: set `LLVM_PROFILE_FILE=/tmp/%p.profraw` for coverage tools.
  - Keep this env var for all factor commands in the session, not only `--exec`:
    `git factor --continue ...` and `git factor --finish ...`
- Runtime strategy for heavy gates:
  - Expect long silent runs once the real test and coverage paths are active.
  - Prefer foreground runs and wait for a result.
  - If an external timeout kills the run, recover from that failure explicitly;
    do not weaken the gate by default.

If the start gate fails for a single-commit `HEAD` session:

- fix the current commit
- stage the intended changes
- amend the commit
- rerun `git factor --exec 'VALIDATION_CMD' HEAD`

If the start gate fails for a range session:

- fix the current tip commit
- stage the intended changes
- amend the commit
- run `git rebase --continue`
- when rebase pauses again at the factor break, stage the first atom and run
  `git factor --continue`

### 3) Stage one atom and continue

- Stage only one atomic unit.
- Do not stage the full original commit in an early `--continue`; keep explicit
  unstaged remainder so the split cannot collapse into one commit.
- Use the convergence model to your advantage.
  - You may edit the remainder aggressively to isolate the next atom.
  - After a successful `--continue`, `git factor` restores the unstaged
    remainder toward the recorded target tree.
  - Do not rely on temporary stash rehearsal as the primary workflow.
- If `git add --patch` cannot split a hunk, stage using an index-only patch
  (`git apply --cached`) so only one atom is committed.
- Prefer dependency-frontier splits first.
  - Good early atoms are units with no internal dependencies on the unstaged
    remainder.
  - A standalone module is one example, but the general rule is to peel off
    whichever independently valid unit has the fewest remaining dependencies.
- Run `git factor --continue -m 'type: description'`.
- The staged changes must contain the next atomic split.
- After a successful `--continue`, the remaining pool is restored as unstaged
  changes from the recorded green baseline commit.
- If you staged the wrong slice or deleted too much while rehearsing the next
  atom, run `git factor --retry`.
  - This keeps the session active.
  - It discards the current split attempt.
  - It restores the same remaining pool you were converging toward.
- Repeat until one atom remains.
- When the next split stops being trivial, finish the session and start a new
  factor session on the smaller top commit instead of stretching one session
  too far.

### 4) Finish the final atom

```bash
git factor --finish -m 'type: description'
```

or:

```bash
git factor --finish
```

- `--finish` restores the remaining pool, verifies the tree hash matches the
  recorded green baseline, and commits the final split.
- For range sessions, a successful finish preserves the gated tip tree, not
  the original internal seams.
- When no `--message` is given, the original commit message is reused.
- Successful completion should remove `.git/factor`.

## Quality Rules

- One thing per commit.
- No large commits; split atoms aggressively.
- Add imports in the same commit as first use.
- The start gate must prove the full target commit is green.
- Every split commit must pass the full gate.
- Prefer TDD reconstruction when splitting monolithic behavior changes.

Detailed standards:

- `references/commit-design.md`
- `references/tdd-reconstruction.md`

## Troubleshooting and Debugging

For day-to-day use, keep moving with `--continue`, `--retry`, `--finish`, and
`--abort`.
Use `git factor --status` when you need a quick session snapshot.
For deeper debugging and reproducibility, use references:

- `references/tracing-and-repro.md`
- `references/recovery.md`
- `references/known-issues.md`

If `--continue` fails with no staged changes, next action is:

- stage exactly one atomic unit (`git add -p` or `git apply --cached`)
- rerun `git factor --continue -m 'type: subject'`

If you lose track of the current split attempt or want to discard a bad
staging plan:

- run `git factor --retry`
- confirm the remaining pool is back in the worktree
- restage the next atom from that restored baseline

If `git factor --exec ...` fails before the split session opens:

- for single-commit sessions, fix, stage, amend, and rerun `git factor`
- for range sessions, fix the tip commit, stage, amend, run
  `git rebase --continue`, then stage the first atom and run
  `git factor --continue` when rebase pauses again

If `git factor` reports the repository is not clean before a start gate:

- commit the intended baseline changes into the current commit, or
- stash/remove the extra changes,
- rerun the start flow only after `git status --porcelain=v1` is empty

If a factor session is already active (`--status` shows rebase/session state):

- do not start another `--exec` over `HEAD` inside that same rebase
- continue the active session with `--continue`/`--finish`
- start a new `--exec` range only after the active session has fully completed

If `git factor --help` looks unavailable:

- try an actual command (`git factor --exec ...`) to validate subcommand setup
- use `git-factor` binary only as fallback in that environment

## Check List

- Plan includes explicit atom ordering and messages.
- `--exec` gate is strict and deterministic.
- Start the session only from a fully clean repository state.
- Each `--continue` commits one atomic unit.
- Final `--finish` is still a small atomic commit.
- Successful completion removes `.git/factor`.
- Use tracing reference when behavior is unexpected.
- Stop after successful split unless user asks for follow-up.
