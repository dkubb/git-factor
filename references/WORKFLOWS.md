# Git Factor Workflows

## Single Commit at HEAD

No rebase needed. The commit is reset directly.

```bash
git factor --exec 'cargo test'
# Changes from HEAD are now unstaged in worktree

git add src/parser.rs
git factor --continue -m 'refactor(parser): simplify token handling'

git factor --finish -m 'feat(api): add /users endpoint'
```

## Multiple Commits (Range)

Targets are sorted topologically (oldest first) and processed
sequentially via interactive rebase with `edit` stops.

```bash
git factor --exec 'make test' HEAD~3..HEAD

# Tool stops at first target commit, unstages its changes
git add src/fix.rs
git factor --continue -m 'fix(core): handle null input'
git factor --finish

# Automatically advances to next target commit
git add src/feature.rs
git factor --continue -m 'feat(core): add validation'
git factor --finish -m 'feat(core): add error reporting'

# Continues until all target commits are processed
```

## Specific Commit (Non-HEAD)

```bash
git factor --exec 'cargo test' abc1234
```

Uses interactive rebase to stop at that commit, then same
continue/finish workflow.

## Multiple Exec Gates

Multiple `--exec` flags are joined with `&&`:

```bash
git factor --exec 'cargo build' --exec 'cargo test' --exec 'cargo clippy'
# Equivalent to: bash -c 'cargo build && cargo test && cargo clippy'
```

## Exec Gate Failure Recovery

When `--continue` runs the exec gate and it fails (exit 75):

1. The tool rolls back the staged changes
2. Restores the full pool of remaining changes
3. You restage differently and retry `--continue`

This is the normal workflow — not an error condition. Stage your
best guess and let the gate tell you what's missing. Each failure
narrows the search space.

```bash
git add src/incomplete_change.rs
git factor --continue -m 'refactor: extract helper'
# error: exec gate failed (exit 75)

# Restage with the missing piece
git add src/incomplete_change.rs src/dependency.rs
git factor --continue -m 'refactor: extract helper'
```

### Fail-Fast Gates

Choose gates that fail quickly so iterations are cheap:

```bash
# Fast: typecheck catches missing imports/deps in seconds
git factor --exec 'python -m py_compile src/*.py' --exec 'pytest -q'

# Multiple gates run left-to-right, short-circuiting on failure
git factor --exec 'cargo check' --exec 'cargo test'
```

## Root Commit Splitting

The first commit in a repository (no parent) is supported:

```bash
git factor --exec 'cargo test' <root-sha>
```

The tool creates a temporary empty tree commit, performs the split,
then automatically drops the empty root commit via a cleanup rebase.

## Multi-Pass Factoring

For complex commits, don't try to split perfectly in one session.
Peel off easy slices, finish with the rest, then factor again:

```bash
# Pass 1: pull out the obvious slices
git factor --exec 'pytest -q'
git add src/new_module.py tests/test_new_module.py
git factor --continue -m 'feat: add new module'
git factor --finish  # everything else goes here

# Pass 2: factor the remainder further
git factor --exec 'pytest -q'
git add src/calculator.py tests/test_calculator.py
git factor --continue -m 'refactor: extract validation'
git factor --finish -m 'feat: add divide operation'
```

Each pass peels off the low-hanging fruit. The final commit
gets smaller each round.

## Session State

State persists in `.git/factor/`. Check it anytime:

```bash
git factor --status
```

If something goes wrong, abort cleanly:

```bash
git factor --abort
```

This resets to the pre-session HEAD and cleans up `.git/factor/`.
