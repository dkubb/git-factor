//! Shared integration-test harness for `git-factor` and `git-sequence-editor`.

#![forbid(unsafe_code)]
#![expect(
    dead_code,
    reason = "shared helpers are consumed differently by each integration test target"
)]
#![expect(
    unreachable_pub,
    reason = "shared integration fixtures are exported within the tests crate namespace"
)]
#![expect(
    clippy::implicit_return,
    reason = "integration support helpers are intentionally narrow scenario utilities"
)]
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{self, Command};

use assert_cmd::prelude::*;
use tempfile::TempDir;

pub const EXIT_OK: i32 = 0;
pub const EXIT_USAGE: i32 = 64;
pub const EXIT_DATAERR: i32 = 65;
pub const EXIT_SOFTWARE: i32 = 70;
pub const EXIT_TEMPFAIL: i32 = 75;
pub const EXIT_FAILURE: i32 = 1;

pub trait CommandExpectation {
    fn expected_code(&self) -> i32;
    fn expected_stderr(&self) -> &str;
    fn expected_stdout(&self) -> &str;
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct OrderedGitReplayExpectation {
    expected_invocations: Option<usize>,
    invocation_count_file: Option<PathBuf>,
}

impl OrderedGitReplayExpectation {
    pub fn assert_consumed(&self) -> Result<(), String> {
        match (
            self.invocation_count_file.as_ref(),
            self.expected_invocations,
        ) {
            (Some(path), Some(expected_count)) => {
                let parsed = fs::read_to_string(path)
                    .unwrap_or_else(|_| "0".to_owned())
                    .parse::<usize>();
                let actual = match parsed {
                    Ok(value) => value,
                    Err(err) => return Err(format!("failed to parse invocation count: {err}")),
                };
                if actual != expected_count {
                    return Err(format!(
                        "ordered git invocation count mismatch: expected {expected_count}, got {actual}"
                    ));
                }
                Ok(())
            }
            (None, None) => Ok(()),
            _ => Err(
                "ordered replay expectation must set both count file and expected invocation count"
                    .to_owned(),
            ),
        }
    }

    pub const fn from_count_file(path: PathBuf, expected_invocations: usize) -> Self {
        Self {
            expected_invocations: Some(expected_invocations),
            invocation_count_file: Some(path),
        }
    }
}

#[derive(Clone, Debug)]
pub struct GitWrapperStep {
    pub args: Vec<String>,
    pub exit_code: i32,
    pub stderr: String,
    pub stdout: String,
}

fn must_ok<T, E>(result: Result<T, E>) -> T {
    result.unwrap_or_else(|_| process::abort())
}

fn must_some<T>(value: Option<T>) -> T {
    value.unwrap_or_else(|| process::abort())
}

pub fn git_factor_bin() -> PathBuf {
    assert_cmd::cargo::cargo_bin!("git-factor").to_path_buf()
}

pub fn git(repo: &Path, args: &[&str]) -> String {
    let output = must_ok(
        Command::new("/usr/bin/git")
            .args(args)
            .current_dir(repo)
            .output(),
    );

    assert!(
        output.status.success(),
        "git {:?} failed (exit {:?})\nstdout:\n{}\nstderr:\n{}",
        args,
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

pub fn git_status_porcelain(repo: &Path) -> String {
    git(repo, &["status", "--porcelain"])
}

pub fn git_dir(repo: &Path) -> PathBuf {
    let dir = git(repo, &["rev-parse", "--git-dir"]);
    repo.join(dir)
}

pub fn write_file(repo: &Path, rel: &str, content: &str) {
    let path = repo.join(rel);
    must_ok(fs::write(&path, content));
}

pub fn init_repo() -> TempDir {
    let dir = must_ok(TempDir::new());
    let repo = dir.path();

    git(repo, &["init"]);
    git(repo, &["config", "user.name", "Git Factor Tests"]);
    git(
        repo,
        &["config", "user.email", "git-factor-tests@example.invalid"],
    );
    // Ensure local tests are not affected by user/global git hook configuration.
    git(repo, &["config", "core.hooksPath", ".git/hooks"]);

    dir
}

pub fn commit_file(repo: &Path, rel: &str, content: &str, message: &str) {
    must_ok(fs::write(repo.join(rel), content));
    git(repo, &["add", rel]);
    git(repo, &["commit", "--message", message]);
}

pub fn start_session(repo: &Path) {
    Command::new(git_factor_bin())
        .current_dir(repo)
        .args(["--exec", "true", "HEAD"])
        .assert()
        .success();
}

fn set_executable(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mut perms = must_ok(fs::metadata(path)).permissions();
        perms.set_mode(0o755);
        must_ok(fs::set_permissions(path, perms));
    }
}

pub fn copy_bins_to(dir: &Path) -> (PathBuf, PathBuf) {
    let factor_src = git_factor_bin();
    let editor_src = assert_cmd::cargo::cargo_bin!("git-sequence-editor").to_path_buf();

    let factor_dst = dir.join("git-factor");
    let editor_dst = dir.join("git-sequence-editor");
    must_ok(fs::copy(factor_src, &factor_dst));
    must_ok(fs::copy(editor_src, &editor_dst));
    set_executable(&factor_dst);
    set_executable(&editor_dst);
    (factor_dst, editor_dst)
}

pub fn write_executable(path: &Path, content: &str) {
    must_ok(fs::write(path, content));
    set_executable(path);
}

/// Creates a PATH prefix containing a `git` wrapper that can fail on the Nth
/// invocation for the child process under test.
///
/// The wrapper:
/// - increments `GIT_FACTOR_COUNT_FILE` each invocation
/// - if invocation == `GIT_FACTOR_FAIL_AT`, exits 1
/// - otherwise execs the real git at `/usr/bin/git`
pub fn make_fault_injecting_git_wrapper() -> (TempDir, PathBuf) {
    let dir = must_ok(TempDir::new());
    let bin = dir.path().join("bin");
    must_ok(fs::create_dir_all(&bin));

    let script = bin.join("git");
    let content = r#"#!/usr/bin/env bash
set -Eeuo pipefail

	count_file="$(printenv GIT_FACTOR_COUNT_FILE 2>/dev/null || true)"
	fail_at="$(printenv GIT_FACTOR_FAIL_AT 2>/dev/null || true)"

if [ -n "$count_file" ]; then
  if [ -f "$count_file" ]; then
    count="$(cat "$count_file" 2>/dev/null || echo 0)"
  else
    count=0
  fi
  count="$((count + 1))"
  printf "%s" "$count" > "$count_file"
else
  count=0
fi

if [ -n "$fail_at" ] && [ "$fail_at" -eq "$count" ] 2>/dev/null; then
  echo "git wrapper: forced failure at invocation $count" >&2
  exit 1
fi

exec /usr/bin/git "$@"
"#;
    write_executable(&script, content);

    (dir, bin)
}

fn sh_single_quote(value: &str) -> String {
    // POSIX sh-safe single-quoted string.
    // Example: abc'def -> 'abc'"'"'def'
    let mut out = String::new();
    out.push('\'');
    for ch in value.chars() {
        if ch == '\'' {
            out.push_str("'\"'\"'");
        } else {
            out.push(ch);
        }
    }
    out.push('\'');
    out
}

/// Creates a PATH prefix containing a strict, ordered-replay `git` wrapper.
///
/// Behavior:
/// - Each invocation increments a counter in a file.
/// - The Nth invocation must match the Nth step exactly (argc + per-arg).
/// - Any extra invocations fail immediately.
///
/// The caller should assert that the counter equals `steps.len()` to ensure no
/// steps were skipped.
pub fn make_ordered_git_wrapper(steps: &[GitWrapperStep]) -> (TempDir, PathBuf, PathBuf) {
    use core::fmt::Write as _;

    let dir = must_ok(TempDir::new());
    let bin = dir.path().join("bin");
    must_ok(fs::create_dir_all(&bin));

    let count_file = dir.path().join("git-invocations");

    let mut script = String::new();
    script.push_str("#!/usr/bin/env bash\nset -Eeuo pipefail\n\n");
    must_ok(writeln!(
        script,
        "count_file={}",
        sh_single_quote(&count_file.to_string_lossy())
    ));
    script.push_str(
        r#"
if [ -f "$count_file" ]; then
  count="$(cat "$count_file" 2>/dev/null || echo 0)"
else
  count=0
fi
count="$((count + 1))"
printf "%s" "$count" > "$count_file"

fail_unexpected() {
  echo "unexpected git invocation #$count: $*" >&2
  exit 99
}

case "$count" in
"#,
    );

    for (idx, step) in steps.iter().enumerate() {
        let n = must_some(idx.checked_add(1));
        must_ok(writeln!(script, "{n})"));
        must_ok(writeln!(
            script,
            "  if [ \"$#\" -ne {} ]; then",
            step.args.len()
        ));
        script.push_str("    fail_unexpected \"$@\"\n");
        script.push_str("  fi\n");
        for (arg_idx, expected) in step.args.iter().enumerate() {
            let pos = must_some(arg_idx.checked_add(1));
            must_ok(writeln!(
                script,
                "  if [ \"${pos}\" != {} ]; then",
                sh_single_quote(expected)
            ));
            script.push_str("    fail_unexpected \"$@\"\n");
            script.push_str("  fi\n");
        }

        if !step.stdout.is_empty() {
            script.push_str("  cat <<'STDOUT'\n");
            script.push_str(&step.stdout);
            if !step.stdout.ends_with('\n') {
                script.push('\n');
            }
            script.push_str("STDOUT\n");
        }

        if !step.stderr.is_empty() {
            script.push_str("  cat <<'STDERR' >&2\n");
            script.push_str(&step.stderr);
            if !step.stderr.ends_with('\n') {
                script.push('\n');
            }
            script.push_str("STDERR\n");
        }

        must_ok(writeln!(script, "  exit {}", step.exit_code));
        script.push_str("  ;;\n");
    }

    script.push_str(
        r#"*)
  fail_unexpected "$@"
  ;;
esac
"#,
    );

    let wrapper = bin.join("git");
    write_executable(&wrapper, script.as_str());
    (dir, bin, count_file)
}

pub fn make_git_wrapper_named(name: &str, body: &str) -> (TempDir, PathBuf) {
    let dir = must_ok(TempDir::new());
    let bin = dir.path().join("bin");
    must_ok(fs::create_dir_all(&bin));
    let script = bin.join(name);
    let real_binary = format!("/usr/bin/{name}");
    let content = format!(
        "#!/usr/bin/env bash\nset -Eeuo pipefail\n\n{body}\n\nexec {} \"$@\"\n",
        sh_single_quote(real_binary.as_str())
    );
    write_executable(&script, content.as_str());
    (dir, bin)
}
