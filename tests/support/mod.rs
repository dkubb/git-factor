//! Shared integration-test harness for `git-factor` and `git-sequence-editor`.

#![forbid(unsafe_code)]
#![expect(
    dead_code,
    reason = "shared helpers are consumed differently by each integration test target"
)]
#![expect(
    clippy::arithmetic_side_effects,
    reason = "ordered git wrapper script generation uses deterministic counter math"
)]
#![expect(
    clippy::literal_string_with_formatting_args,
    reason = "shell script fixtures intentionally contain ${VAR:-default} style expansions"
)]
#![expect(
    unreachable_pub,
    reason = "support is intentionally private to each integration test crate while exposing helpers internally"
)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use assert_cmd::prelude::*;
use predicates::prelude::*;
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
                let actual = fs::read_to_string(path)
                    .unwrap_or_else(|_| "0".to_owned())
                    .parse::<usize>()
                    .map_err(|err| format!("failed to parse invocation count: {err}"))?;
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

    pub fn from_count_file(path: PathBuf, expected_invocations: usize) -> Self {
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

pub fn assert_command_exact(command: &mut Command, expected: &dyn CommandExpectation) {
    command
        .assert()
        .code(expected.expected_code())
        .stdout(predicate::str::diff(expected.expected_stdout().to_owned()))
        .stderr(predicate::str::diff(expected.expected_stderr().to_owned()));
}

pub fn git_factor_bin() -> PathBuf {
    assert_cmd::cargo::cargo_bin!("git-factor").to_path_buf()
}

pub fn git_sequence_editor_bin() -> PathBuf {
    assert_cmd::cargo::cargo_bin!("git-sequence-editor").to_path_buf()
}

fn real_git() -> &'static Path {
    static REAL_GIT: OnceLock<PathBuf> = OnceLock::new();
    REAL_GIT
        .get_or_init(|| PathBuf::from("/usr/bin/git"))
        .as_path()
}

pub fn git(repo: &Path, args: &[&str]) -> String {
    let output = Command::new(real_git())
        .args(args)
        .current_dir(repo)
        .output()
        .expect("git command should start");

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
    fs::write(&path, content).expect("write file");
}

pub fn init_repo() -> TempDir {
    let dir = TempDir::new().expect("tempdir");
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
    write_file(repo, rel, content);
    git(repo, &["add", rel]);
    git(repo, &["commit", "--message", message]);
}

pub fn start_session(repo: &Path) {
    start_session_with_exec(repo, "true");
}

pub fn start_session_with_exec(repo: &Path, exec: &str) {
    Command::new(git_factor_bin())
        .current_dir(repo)
        .args(["--exec", exec, "HEAD"])
        .assert()
        .success();
}

fn set_executable(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mut perms = fs::metadata(path).expect("metadata").permissions();
        perms.set_mode(0o755);
        fs::set_permissions(path, perms).expect("set permissions");
    }
}

pub fn copy_bins_to(dir: &Path) -> (PathBuf, PathBuf) {
    let factor_src = git_factor_bin();
    let editor_src = git_sequence_editor_bin();

    let factor_dst = dir.join("git-factor");
    let editor_dst = dir.join("git-sequence-editor");
    fs::copy(factor_src, &factor_dst).expect("copy git-factor");
    fs::copy(editor_src, &editor_dst).expect("copy git-sequence-editor");
    set_executable(&factor_dst);
    set_executable(&editor_dst);
    (factor_dst, editor_dst)
}

pub fn write_executable(path: &Path, content: &str) {
    fs::write(path, content).expect("write executable");
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
    let dir = TempDir::new().expect("tempdir");
    let bin = dir.path().join("bin");
    fs::create_dir_all(&bin).expect("create bin dir");

    let script = bin.join("git");
    let content = r#"#!/usr/bin/env bash
set -Eeuo pipefail

count_file="${GIT_FACTOR_COUNT_FILE:-}"
fail_at="${GIT_FACTOR_FAIL_AT:-}"

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

    let dir = TempDir::new().expect("tempdir");
    let bin = dir.path().join("bin");
    fs::create_dir_all(&bin).expect("create bin dir");

    let count_file = dir.path().join("git-invocations");

    let mut script = String::new();
    script.push_str("#!/usr/bin/env bash\nset -Eeuo pipefail\n\n");
    writeln!(
        script,
        "count_file={}",
        sh_single_quote(&count_file.to_string_lossy())
    )
    .expect("write script");
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
        let n = idx + 1;
        writeln!(script, "{n})").expect("write script");
        writeln!(script, "  if [ \"$#\" -ne {} ]; then", step.args.len()).expect("write script");
        script.push_str("    fail_unexpected \"$@\"\n");
        script.push_str("  fi\n");
        for (arg_idx, expected) in step.args.iter().enumerate() {
            let pos = arg_idx + 1;
            writeln!(
                script,
                "  if [ \"${pos}\" != {} ]; then",
                sh_single_quote(expected)
            )
            .expect("write script");
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

        writeln!(script, "  exit {}", step.exit_code).expect("write script");
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
    let dir = TempDir::new().expect("tempdir");
    let bin = dir.path().join("bin");
    fs::create_dir_all(&bin).expect("create bin dir");
    let script = bin.join(name);
    let real_binary = format!("/usr/bin/{name}");
    let content = format!(
        "#!/usr/bin/env bash\nset -Eeuo pipefail\n\n{body}\n\nexec {} \"$@\"\n",
        sh_single_quote(real_binary.as_str())
    );
    write_executable(&script, content.as_str());
    (dir, bin)
}
