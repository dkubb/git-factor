# Repository Guidelines

## Project Structure & Module Organization

- Core implementations live in library modules:
  - `src/git_factor.rs`
  - `src/git_sequence_editor.rs`
- Binaries:
  - `src/bin/git-factor.rs` (`git-factor`)
  - `src/bin/git-sequence-editor.rs` (`git-sequence-editor`)
- Integration tests live in `tests/`:
  - `tests/git-factor.rs`
  - `tests/git-sequence-editor.rs`
  - shared helpers in `tests/support/mod.rs`

## Build, Test, and Development Commands

Use `just` recipes (preferred over raw cargo commands):

- `just build`: compile the workspace.
- `just check`: fast type-check.
- `just fmt-check`: verify formatting.
- `just lint`: run clippy with repo lint policy.
- `just test`: run all tests (`cargo test-all` + doc tests).
- `just coverage`: run coverage collection (`cargo coverage --no-report`).
- `just ci`: full validation (`fmt-check`, `lint`, `test`, `coverage`, `deny`).

## Coding Style & Naming Conventions

- Rust edition: 2024.
- Formatting: `cargo fmt-all` (2-space/4-space decisions are rustfmt-controlled; do not hand-format).
- Lints are strict (`warnings = deny`, clippy restriction/pedantic sets enabled).
- Prefer explicit, long-form CLI flags in code and tests.
- Use descriptive `snake_case` names; integration test names should describe observable behavior (example: `rejects_merge_commits`).

## Testing Guidelines

- Integration tests are contract tests and should assert exact behavior: exit code, stdout, stderr, and side effects.
- Prefer exact string matching; use regex only when output is not fully controlled.
- Keep test bodies simple and deterministic; push complexity into helpers in `tests/support/mod.rs`.
- Unit/property tests should live alongside code in `mod tests` / `mod proptests`.

## Commit & Pull Request Guidelines

- Follow Conventional Commit style seen in history: `feat: ...`, `fix: ...`, `test: ...`, `docs: ...`, `chore: ...`.
- Keep commits atomic and reviewable; one behavioral change per commit when possible.
- Before opening a PR, run `just ci` and include:
  - what behavior changed,
  - which tests were added/updated,
  - any known limitations or follow-up tasks.
