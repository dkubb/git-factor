# git-factor

Split a large git commit into smaller atomic commits using interactive
rebase.

`git-factor` manages the rebase machinery so you can focus on staging
one logical change at a time. Each split commit is validated by an exec
gate before it is accepted.

## Binaries

The crate produces two binaries:

- **`git-factor`** -- the main CLI, invoked as `git factor` when on
  `$PATH`.
- **`git-sequence-editor`** -- a helper that rewrites the rebase todo
  list. `git-factor` calls it automatically; you do not need to invoke
  it directly.

## Install

Install both binaries into `~/.local/bin` (must be on `$PATH`):

```bash
just install-local
```

Or with cargo directly:

```bash
cargo install --path . --root ~/.local --force
```

## Quick start

```bash
# 1. Start a factor session on the latest commit
git factor --exec 'cargo test' HEAD

# 2. Stage one atomic change
git add --patch

# 3. Commit the staged slice (gate runs automatically)
git factor --continue --message 'feat: add login endpoint'

# 4. Repeat steps 2-3 for each atom

# 5. Commit the remaining changes
git factor --finish --message 'refactor: extract auth helpers'
# Or reuse the original commit message:
git factor --finish
```

## Command reference

### Start a session

```bash
# Split the latest commit
git factor --exec 'make test' HEAD

# Split a range of commits
git factor --exec 'make check' HEAD~3..HEAD

# Split specific commits
git factor --exec 'npm test' abc1234 def5678
```

`--exec` is required and accepts one or more shell commands. Multiple
`--exec` flags are joined with `&&`.

### Continue (commit one slice)

```bash
git factor --continue --message 'type: description'
```

`--message` is required. Multiple `--message` flags produce separate
paragraphs, matching `git commit` behavior.

### Finish (commit remaining changes)

```bash
# With an explicit message
git factor --finish --message 'type: description'

# Reuse the original commit message
git factor --finish
```

### Abort

```bash
git factor --abort
```

Resets the working tree for the current commit step. If a multi-commit
rebase is still active, run `git rebase --abort` afterward to cancel
the full rebase.

### Status

```bash
git factor --status
```

Prints session details (current commit, split count, rebase state) or
reports that no session is active.

## Recovery

| Situation | Command |
|---|---|
| Exec gate failed | Fix the issue, re-stage, retry `--continue` |
| Abort current split step | `git factor --abort` |
| Abort the entire rebase | `git factor --abort && git rebase --abort` |
| Check session state | `git factor --status` |

## Development

The project uses [just](https://github.com/casey/just) as a command
runner. Run `just` to list available recipes:

```bash
just build       # Build the project
just check       # Check code compiles
just test        # Run tests (nextest + doctests)
just lint        # Lint with clippy
just fmt         # Format code
just fmt-check   # Check formatting without changes
just fix         # Auto-fix lint and format issues
just coverage    # Run coverage with branch tracking
just deny        # Run cargo-deny security audit
just ci          # Full CI pipeline (fmt, lint, test, coverage, deny)
just clean       # Clean build artifacts
```

## Testing

Tests use [cargo-nextest](https://nexte.st/) via cargo aliases defined
in `.cargo/config.toml`. Integration tests exercise the CLI through
`assert_cmd`. Run the full suite with:

```bash
just test
```

Coverage requires [cargo-llvm-cov](https://github.com/taiki-e/cargo-llvm-cov):

```bash
just coverage
```

## License

Private. See [Cargo.toml](Cargo.toml) for details.
