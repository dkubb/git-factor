use std::collections::HashMap;
use std::ffi::OsString;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Mutex;
use std::thread;

use super::*;
use tempfile::TempDir;

#[cfg(unix)]
use std::os::unix::ffi::OsStringExt as _;

#[cfg(unix)]
struct NonUtf8Fs;

#[cfg(unix)]
impl Fs for NonUtf8Fs {
    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        REAL_FS.create_dir_all(path)
    }

    fn remove_dir_all(&self, path: &Path) -> io::Result<()> {
        REAL_FS.remove_dir_all(path)
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        REAL_FS.remove_file(path)
    }

    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        REAL_FS.read_to_string(path)
    }

    fn write_string(&self, path: &Path, content: &str) -> io::Result<()> {
        REAL_FS.write_string(path, content)
    }

    fn canonicalize(&self, _path: &Path) -> io::Result<PathBuf> {
        let mut bytes = b"/tmp/".to_vec();
        bytes.push(0xff);
        bytes.extend_from_slice(b"/bin/git-factor");
        Ok(PathBuf::from(OsString::from_vec(bytes)))
    }

    fn is_dir(&self, path: &Path) -> bool {
        REAL_FS.is_dir(path)
    }

    fn exists(&self, path: &Path) -> bool {
        REAL_FS.exists(path)
    }
}

struct FailingRequiresRebaseWriteFs;

impl Fs for FailingRequiresRebaseWriteFs {
    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        REAL_FS.create_dir_all(path)
    }

    fn remove_dir_all(&self, path: &Path) -> io::Result<()> {
        REAL_FS.remove_dir_all(path)
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        REAL_FS.remove_file(path)
    }

    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        REAL_FS.read_to_string(path)
    }

    fn write_string(&self, path: &Path, content: &str) -> io::Result<()> {
        if path
            .file_name()
            .is_some_and(|name| name == "requires_rebase")
        {
            return Err(io::Error::other("requires_rebase write failed"));
        }
        REAL_FS.write_string(path, content)
    }

    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        REAL_FS.canonicalize(path)
    }

    fn is_dir(&self, path: &Path) -> bool {
        REAL_FS.is_dir(path)
    }

    fn exists(&self, path: &Path) -> bool {
        REAL_FS.exists(path)
    }
}

fn ctx_for(path: &Path) -> Ctx<'static> {
    Ctx {
        runner: &REAL_RUNNER,
        cwd: path.to_path_buf(),
        io: &REAL_IO,
        env: &REAL_ENV,
        fs: &REAL_FS,
    }
}

#[cfg(unix)]
fn exit_status(code: i32) -> ExitStatus {
    use std::os::unix::process::ExitStatusExt as _;
    ExitStatus::from_raw(code)
}

#[derive(Default)]
struct ScriptedRunner {
    outputs: HashMap<String, Output>,
    statuses: HashMap<String, ExitStatus>,
}

impl ScriptedRunner {
    fn output_key(bin: &str, args: &[&str], cwd: &Path) -> String {
        format!(
            "output\x1f{bin}\x1f{}\x1f{}",
            cwd.display(),
            args.join("\x1f")
        )
    }

    fn status_key(
        bin: &str,
        args: &[&str],
        envs: &[(&str, &str)],
        quiet: bool,
        cwd: &Path,
    ) -> String {
        let env_key = envs
            .iter()
            .map(|&(k, v)| format!("{k}={v}"))
            .collect::<Vec<String>>()
            .join("\x1f");
        format!(
            "status\x1f{bin}\x1f{}\x1f{quiet}\x1f{env_key}\x1f{}",
            cwd.display(),
            args.join("\x1f")
        )
    }

    fn with_output(mut self, bin: &str, args: &[&str], cwd: &Path, stdout: &str) -> Self {
        let key = Self::output_key(bin, args, cwd);
        self.outputs.insert(
            key,
            Output {
                status: exit_status(0),
                stdout: stdout.as_bytes().to_vec(),
                stderr: Vec::new(),
            },
        );
        self
    }

    fn with_status(
        mut self,
        bin: &str,
        args: &[&str],
        envs: &[(&str, &str)],
        quiet: bool,
        cwd: &Path,
        code: i32,
    ) -> Self {
        let key = Self::status_key(bin, args, envs, quiet, cwd);
        self.statuses.insert(key, exit_status(code));
        self
    }
}

impl Runner for ScriptedRunner {
    fn output(&self, bin: &str, args: &[&str], cwd: &Path) -> io::Result<Output> {
        let key = Self::output_key(bin, args, cwd);
        self.outputs
            .get(&key)
            .cloned()
            .ok_or_else(|| io::Error::other(format!("unexpected output call: {key}")))
    }

    fn status(
        &self,
        bin: &str,
        args: &[&str],
        envs: &[(&str, &str)],
        quiet: bool,
        cwd: &Path,
    ) -> io::Result<ExitStatus> {
        let key = Self::status_key(bin, args, envs, quiet, cwd);
        self.statuses
            .get(&key)
            .copied()
            .ok_or_else(|| io::Error::other(format!("unexpected status call: {key}")))
    }
}

#[test]
fn scripted_runner_missing_output_is_an_error() {
    let dir = TempDir::new().expect("tempdir");
    let runner = ScriptedRunner::default();

    let err = runner
        .output("git", &["version"], dir.path())
        .expect_err("expected missing scripted output to error");

    assert!(
        err.to_string().contains("unexpected output call"),
        "unexpected error: {err}"
    );
}

#[test]
fn scripted_runner_missing_status_is_an_error() {
    let dir = TempDir::new().expect("tempdir");
    let runner = ScriptedRunner::default();

    let err = runner
        .status("git", &["status"], &[], false, dir.path())
        .expect_err("expected missing scripted status to error");

    assert!(
        err.to_string().contains("unexpected status call"),
        "unexpected error: {err}"
    );
}

#[test]
fn scripted_runner_status_includes_env_key() {
    let dir = TempDir::new().expect("tempdir");
    let runner = ScriptedRunner::default();

    let err = runner
        .status(
            "git",
            &["status"],
            &[("GIT_OPTIONAL_LOCKS", "0")],
            false,
            dir.path(),
        )
        .expect_err("expected missing scripted status to error");

    assert!(
        err.to_string().contains("unexpected status call"),
        "unexpected error: {err}"
    );
}

#[test]
