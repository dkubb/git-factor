# Git Factor CLI Reference

## Usage

```text
git factor [OPTIONS] [COMMIT]...
```

`[COMMIT]...` accepts SHAs, branch names, or ranges (`A..B`).
Ranges are expanded via `git rev-list`. Defaults to `HEAD`.

## Flags

- `--exec <CMD>` -- validation gate (required to start, repeatable,
  joined with `&&`)
- `--continue` -- commit staged changes as one split (requires `-m`)
- `--finish` -- commit all remaining changes (optional `-m`,
  defaults to original message)
- `--abort` -- cancel session, reset to pre-session state
- `--status` -- show session details or "no active session"
- `-m, --message <MSG>` -- commit message (repeatable, each becomes
  a separate paragraph)

## Mutual Exclusivity

- `--abort` and `--status` are standalone (no other flags)
- `--finish` cannot combine with `--continue`, `--exec`, or `COMMIT`
- `--continue` cannot combine with `--exec` or `COMMIT`
- `-m` only with `--continue` or `--finish`
- No arguments at all prints help

## Exit Codes

- **0** -- success
- **64** -- usage error (bad flags, no/already active session,
  no staged changes)
- **65** -- data error (not a repo, invalid commit, merge commit,
  not ancestor of HEAD)
- **70** -- internal error (git command failure, state I/O)
- **75** -- recoverable (exec gate failed, tree hash mismatch)

## Constraints

- Must be inside a git repository
- No active factor session or rebase when starting
- Target commits must be ancestors of HEAD
- Target commits must not be merge commits
- Symmetric diff ranges (`...`) are rejected
- `--exec` is always required when starting
