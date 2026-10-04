//! Private E1 provider draft; consumed only by the current real engine test bridge.
//! No direct-private runtime calls and no synthetic journal/native replay state.

use super::*;
use crate::exit_codes::EXIT_TEMPFAIL;
use core::cell::Cell;
use core::fmt::Write as _;
use core::str::from_utf8;
use core::time::Duration;
use std::ffi::OsStr;
use std::io::Write as _;
use std::os::unix::ffi::OsStrExt as _;
use std::process::ExitStatus;
use std::process::Output;
use std::process::Stdio;

/// Native SHA-1 identity of the independently specified empty Git tree object.
const EMPTY: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

#[derive(Clone, Copy)]
enum BypassBoundary {
    FinalDescendant,
    IntermediateDescendant,
    IntermediatePendingPick,
    RemainderPendingPick,
    RemainderWithDescendants,
    RemainderWithoutDescendants,
}

#[derive(Clone, Copy, serde::Deserialize, serde::Serialize)]
enum ChildBoundary {
    AfterWrite,
    AfterWriteKill,
    BeforeCallbackKill,
    BeforeWrite,
}

/// Independent executable-observation errors, exercised through actual public run.
#[derive(Clone, Copy)]
pub(in crate::git_factor::engine::tests) enum ExecutableFailure {
    Canonicalize,
    CurrentExe,
    #[cfg(target_os = "linux")]
    Utf8,
}

/// Actual one-line todo edits at an admitted native pause, never fake history.
#[derive(Clone, Copy)]
enum GrammarEdit {
    ForeignExec,
    FutureExec,
    FuturePick,
    MissingPick,
}

#[derive(Clone, Copy)]
enum OpeningControl {
    Abort,
    CopiedAbort,
    ForeignHead,
    MovedAbort,
    Retry,
    WrongPick,
}

#[derive(Clone, Copy)]
enum OpeningOrigin {
    NonRoot,
    Root,
}

#[derive(Clone, Copy)]
enum PredecessorControl {
    Amend,
    Drop,
    SelectionRewind,
}

/// Stops actual Opening at its native break before baseline actor construction.
struct BeforeBaselineCandidate {
    admin: PathBuf,
    refused: Cell<bool>,
}

struct BeforeNativeContinue {
    refused: Cell<bool>,
}

/// Optional fixture-only child adapter, outside usable run/entry prefixes.
pub(in crate::git_factor::engine::tests) struct ChildPublication {
    configuration: ChildStimulus,
}

#[derive(serde::Deserialize, serde::Serialize)]
struct ChildStimulus {
    accepted_count: usize,
    admin: PathBuf,
    boundary: ChildBoundary,
    callback: String,
    checkpoint: String,
    evidence: PathBuf,
    launcher: PathBuf,
    original_pick: String,
}

/// Owns a one-shot post-editor obstruction; native Git creates the retry transcript.
#[derive(serde::Serialize, serde::Deserialize)]
struct EditorObstruction {
    admin: PathBuf,
    base: Option<String>,
    bytes: String,
    checkpoint: String,
    evidence: PathBuf,
    launcher: PathBuf,
    path: PathBuf,
}

#[derive(serde::Deserialize, serde::Serialize)]
struct NativeIngress {
    admin: PathBuf,
    arguments: Vec<String>,
    cwd: PathBuf,
    parent: u32,
    pid: u32,
}

/// Owns the actual native process launch and records its PID/cwd before child dispatch.
#[derive(Debug, Eq, PartialEq)]
struct OpeningAbortFrame {
    atom: Vec<u8>,
    base: Vec<u8>,
    config: Vec<u8>,
    head: String,
    index: Vec<u8>,
    journal: Option<Vec<u8>>,
    native: Option<BTreeMap<PathBuf, LogPath>>,
    refs: String,
    remainder: Vec<u8>,
    user: Vec<u8>,
}

struct OwnedNativeContinue<'fixture> {
    configuration: &'fixture ChildStimulus,
}

/// Observes only this fixture's exact native invocation; other commands retain `REAL_RUNNER`.
struct OwnedNativeObservation<'fixture> {
    admin: &'fixture Path,
    arguments: &'fixture [&'fixture str],
    evidence: &'fixture Path,
}

/// Parent-only fault: the native source break happened before selecting publication.
struct ReadyBreakPublication {
    admin: PathBuf,
    refused: Cell<bool>,
}

struct UnavailableExecutable<'fixture>(&'fixture NativeEnv);
impl Runner for OwnedNativeContinue<'_> {
    fn output(
        &self,
        bin: &str,
        args: &[&str],
        envs: &[(&str, Option<&str>)],
        cwd: &Path,
    ) -> io::Result<Output> {
        REAL_RUNNER.output(bin, args, envs, cwd)
    }
    fn status(
        &self,
        bin: &str,
        args: &[&str],
        envs: &[(&str, Option<&str>)],
        quiet: bool,
        cwd: &Path,
    ) -> io::Result<ExitStatus> {
        if bin != "git" || args != ["rebase", "--continue"] {
            return REAL_RUNNER.status(bin, args, envs, quiet, cwd);
        }
        owned_native_status(
            bin,
            args,
            envs,
            quiet,
            cwd,
            &self.configuration.admin,
            &self.configuration.evidence,
            false,
        )
    }
}
impl Runner for OwnedNativeObservation<'_> {
    fn output(
        &self,
        bin: &str,
        args: &[&str],
        envs: &[(&str, Option<&str>)],
        cwd: &Path,
    ) -> io::Result<Output> {
        REAL_RUNNER.output(bin, args, envs, cwd)
    }
    fn status(
        &self,
        bin: &str,
        args: &[&str],
        envs: &[(&str, Option<&str>)],
        quiet: bool,
        cwd: &Path,
    ) -> io::Result<ExitStatus> {
        if bin != "git" || args != self.arguments {
            return REAL_RUNNER.status(bin, args, envs, quiet, cwd);
        }
        owned_native_status(bin, args, envs, quiet, cwd, self.admin, self.evidence, true)
    }
}

impl ChildPublication {
    #[expect(
        clippy::panic,
        reason = "fixture authority mismatches must terminate the test before any native process can be targeted"
    )]
    fn claim(&self) -> bool {
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(self.configuration.evidence.join("fired"))
        {
            Ok(mut marker) => {
                writeln!(
                    marker,
                    "{} {} {}",
                    process::id(),
                    self.configuration.callback,
                    self.configuration.original_pick
                )
                .or_abort("actual one-shot evidence");
                true
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => false,
            Err(error) => panic!("cannot claim owned E1 stimulus: {error}"),
        }
    }
    #[expect(
        clippy::single_call_fn,
        reason = "named native fixture boundaries keep ownership, stimulus, and conservation visible to the direct canonical provider"
    )]
    pub(in crate::git_factor::engine::tests) fn configured(
        arguments: &[OsString],
        environment: &NativeEnv,
    ) -> Option<Self> {
        let path = env::var_os("FACTOR_TEST_E1_STIMULUS")?;
        let configuration: ChildStimulus =
            serde_json::from_slice(&fs::read(path).or_abort("owned E1 child configuration"))
                .or_abort("valid E1 child configuration");
        let actual_admin =
            fs::canonicalize(environment.cwd.join(".git")).or_abort("canonical native E1 admin");
        assert_eq!(actual_admin, configuration.admin);
        assert_eq!(
            fs::canonicalize(&environment.executable).or_abort("owned E1 launcher"),
            configuration.launcher
        );
        assert!(configuration.evidence.is_dir());
        if configuration.evidence.join("fired").is_file() {
            return None;
        }
        if arguments.get(1).and_then(|argument| argument.to_str())
            != Some(configuration.callback.as_str())
        {
            return None;
        }
        fs::write(
            configuration.evidence.join("observed-child-cwd"),
            environment.cwd.as_os_str().as_bytes(),
        )
        .or_abort("actual child cwd evidence");
        let current_journal: serde_json::Value = serde_json::from_slice(
            &fs::read(actual_admin.join("factor-journal.json")).or_abort("real current E1 journal"),
        )
        .or_abort("current journal grammar");
        assert_eq!(
            current_journal
                .pointer("/checkpoint")
                .and_then(serde_json::Value::as_str),
            Some(configuration.checkpoint.as_str())
        );
        let adapter = Self { configuration };
        if matches!(
            adapter.configuration.boundary,
            ChildBoundary::BeforeCallbackKill
        ) && adapter.original_slot_matches()
            && adapter.claim()
        {
            adapter.snapshot("before-callback");
            adapter.kill_native_ancestor();
            process::exit(99);
        }
        Some(adapter)
    }

    fn kill_native_ancestor(&self) {
        kill_owned_native(&self.configuration, &["rebase", "--continue"]);
    }

    fn original_slot_matches(&self) -> bool {
        let done = fs::read_to_string(self.configuration.admin.join("rebase-merge/done"))
            .or_abort("actual native E1 done");
        let picked = done
            .lines()
            .rev()
            .find_map(|line| {
                let mut fields = line.split_whitespace();
                match fields.next() {
                    Some("pick" | "p") => fields.next(),
                    _ => None,
                }
            })
            .or_abort("actual native original pick");
        assert!(picked.bytes().all(|byte| byte.is_ascii_hexdigit()));
        let output = Command::new("git")
            .args(["rev-parse", "--verify", &format!("{picked}^{{commit}}")])
            .env("GIT_DIR", &self.configuration.admin)
            .output()
            .or_abort("resolve native source slot");
        assert!(output.status.success());
        String::from_utf8(output.stdout)
            .or_abort("source slot UTF-8")
            .trim_end()
            == self.configuration.original_pick
    }

    fn snapshot(&self, boundary: &str) {
        if matches!(self.configuration.boundary, ChildBoundary::BeforeWrite) {
            // Capture the physical index before any native metadata query.
            fs::copy(
                self.configuration.admin.join("index"),
                self.configuration.evidence.join("boundary-index"),
            )
            .or_abort("raw publication-boundary index");
            for (name, arguments) in [
                ("boundary-head", vec!["rev-parse", "HEAD"]),
                ("boundary-checkpoint", vec!["rev-parse", "refs/heads/main"]),
                ("boundary-refs", vec!["show-ref"]),
            ] {
                let output = Command::new("git")
                    .args(arguments)
                    .env("GIT_DIR", &self.configuration.admin)
                    .output()
                    .or_abort("actual publication-boundary native facts");
                assert!(output.status.success());
                fs::write(self.configuration.evidence.join(name), output.stdout)
                    .or_abort("publication-boundary native evidence");
            }
            let root = self
                .configuration
                .admin
                .parent()
                .or_abort("ordinary fixture root");
            for name in ["base", "atom", "remainder", "first", "second", "unrelated"] {
                if root.join(name).is_file() {
                    fs::copy(
                        root.join(name),
                        self.configuration.evidence.join(format!("boundary-{name}")),
                    )
                    .or_abort("actual publication-boundary user and tracked bytes");
                }
            }
        }

        for native in ["done", "git-rebase-todo"] {
            fs::copy(
                self.configuration.admin.join("rebase-merge").join(native),
                self.configuration.evidence.join(native),
            )
            .or_abort("actual consumed native transcript");
        }
        fs::copy(
            self.configuration.admin.join("factor-journal.json"),
            self.configuration.evidence.join("observed-journal.json"),
        )
        .or_abort("actual boundary journal");
        fs::write(self.configuration.evidence.join("boundary"), boundary)
            .or_abort("boundary evidence");
        for marker in ["stopped-sha", "amend"] {
            assert!(
                !self
                    .configuration
                    .admin
                    .join("rebase-merge")
                    .join(marker)
                    .exists()
            );
        }
        assert!(!self.configuration.admin.join("REBASE_HEAD").exists());
    }
}

impl Fs for ChildPublication {
    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        REAL_FS.canonicalize(path)
    }
    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        REAL_FS.create_dir_all(path)
    }
    fn exists(&self, path: &Path) -> bool {
        REAL_FS.exists(path)
    }
    fn is_dir(&self, path: &Path) -> bool {
        REAL_FS.is_dir(path)
    }
    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        REAL_FS.read_to_string(path)
    }
    fn remove_atomic_file(&self, path: &Path) -> io::Result<()> {
        REAL_FS.remove_atomic_file(path)
    }
    fn remove_dir_all(&self, path: &Path) -> io::Result<()> {
        REAL_FS.remove_dir_all(path)
    }
    fn symlink_metadata(&self, path: &Path) -> io::Result<fs::Metadata> {
        REAL_FS.symlink_metadata(path)
    }
    #[expect(
        clippy::panic,
        reason = "fixture authority mismatches must terminate the test before any native process can be targeted"
    )]
    #[expect(
        clippy::panic_in_result_fn,
        reason = "the owned test adapter refuses invalid fixture authority rather than converting assertion failure into product recovery"
    )]
    fn write_atomic_string(&self, path: &Path, content: &str) -> io::Result<()> {
        let admitted_path = path.file_name() == Some(OsStr::new("factor-journal.json"))
            && path.parent().map(fs::canonicalize).transpose()?
                == Some(self.configuration.admin.clone());
        if !admitted_path || !self.original_slot_matches() {
            return REAL_FS.write_atomic_string(path, content);
        }
        fs::write(
            self.configuration.evidence.join("observed-write-path"),
            path.as_os_str().as_bytes(),
        )?;
        fs::write(
            self.configuration.evidence.join("observed-child-cwd"),
            env::current_dir()?.as_os_str().as_bytes(),
        )?;
        let proposed: serde_json::Value =
            serde_json::from_str(content).or_abort("actual proposed journal payload");
        let target = if self.configuration.callback == "checkpoint-terminal" {
            proposed
                .pointer("/state/phase")
                .and_then(serde_json::Value::as_str)
                == Some("verified")
        } else {
            proposed
                .pointer("/state/phase")
                .and_then(serde_json::Value::as_str)
                == Some("replaying")
                && proposed
                    .pointer("/state/accepted")
                    .and_then(serde_json::Value::as_array)
                    .map(Vec::len)
                    == Some(self.configuration.accepted_count)
        };
        if !target || !self.claim() {
            return REAL_FS.write_atomic_string(path, content);
        }
        fs::write(
            self.configuration.evidence.join("proposed-journal.json"),
            content,
        )
        .or_abort("actual proposed boundary payload");
        match self.configuration.boundary {
            ChildBoundary::BeforeWrite => self.snapshot("before-write"),
            ChildBoundary::AfterWrite | ChildBoundary::AfterWriteKill => {
                REAL_FS.write_atomic_string(path, content)?;
                self.snapshot("after-write-returned-successfully");
            }
            ChildBoundary::BeforeCallbackKill => {
                panic!("before callback stimulus must execute before dispatcher")
            }
        }
        if matches!(self.configuration.boundary, ChildBoundary::AfterWriteKill) {
            self.kill_native_ancestor();
            // The native owner is dead. Do not let this orphan dispatch/log into a
            // journal while the parent can already be reconciling the same attempt.
            process::exit(99);
        }
        Err(io::Error::other("owned E1 child publication interruption"))
    }
    fn write_string(&self, path: &Path, content: &str) -> io::Result<()> {
        REAL_FS.write_string(path, content)
    }
}
impl Runner for BeforeNativeContinue {
    fn output(
        &self,
        bin: &str,
        args: &[&str],
        envs: &[(&str, Option<&str>)],
        cwd: &Path,
    ) -> io::Result<Output> {
        REAL_RUNNER.output(bin, args, envs, cwd)
    }
    fn status(
        &self,
        bin: &str,
        args: &[&str],
        envs: &[(&str, Option<&str>)],
        quiet: bool,
        cwd: &Path,
    ) -> io::Result<ExitStatus> {
        if bin == "git" && args == ["rebase", "--continue"] && !self.refused.replace(true) {
            return Err(io::Error::other("owned before-native-continue refusal"));
        }
        REAL_RUNNER.status(bin, args, envs, quiet, cwd)
    }
}
impl Fs for ReadyBreakPublication {
    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        REAL_FS.canonicalize(path)
    }
    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        REAL_FS.create_dir_all(path)
    }
    fn exists(&self, path: &Path) -> bool {
        REAL_FS.exists(path)
    }
    fn is_dir(&self, path: &Path) -> bool {
        REAL_FS.is_dir(path)
    }
    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        REAL_FS.read_to_string(path)
    }
    fn remove_atomic_file(&self, path: &Path) -> io::Result<()> {
        REAL_FS.remove_atomic_file(path)
    }
    fn remove_dir_all(&self, path: &Path) -> io::Result<()> {
        REAL_FS.remove_dir_all(path)
    }
    fn symlink_metadata(&self, path: &Path) -> io::Result<fs::Metadata> {
        REAL_FS.symlink_metadata(path)
    }
    fn write_atomic_string(&self, path: &Path, content: &str) -> io::Result<()> {
        let actual: serde_json::Value =
            serde_json::from_str(content).or_abort("real proposed selection journal");
        if path.file_name() == Some(OsStr::new("factor-journal.json"))
            && path.parent().map(fs::canonicalize).transpose()? == Some(self.admin.clone())
            && actual
                .pointer("/state/phase")
                .and_then(serde_json::Value::as_str)
                == Some("selecting")
            && !self.refused.replace(true)
        {
            return Err(io::Error::other(
                "owned ready-break selection publication refusal",
            ));
        }
        REAL_FS.write_atomic_string(path, content)
    }
    fn write_string(&self, path: &Path, content: &str) -> io::Result<()> {
        REAL_FS.write_string(path, content)
    }
}
impl Env for UnavailableExecutable<'_> {
    fn current_dir(&self) -> io::Result<PathBuf> {
        self.0.current_dir()
    }
    fn current_exe(&self) -> io::Result<PathBuf> {
        Err(io::Error::other("owned executable observation failure"))
    }
    fn var_os(&self, key: &str) -> Option<OsString> {
        self.0.var_os(key)
    }
}
impl Runner for BeforeBaselineCandidate {
    fn output(
        &self,
        bin: &str,
        args: &[&str],
        envs: &[(&str, Option<&str>)],
        cwd: &Path,
    ) -> io::Result<Output> {
        if bin == "git"
            && args.first() == Some(&"commit-tree")
            && let Ok(done) = fs::read_to_string(self.admin.join("rebase-merge/done"))
            && done
                .lines()
                .rev()
                .find(|line| !line.trim().is_empty() && !line.starts_with('#'))
                == Some("break")
            && !self.refused.replace(true)
        {
            return Err(io::Error::other(
                "owned baseline candidate construction refusal",
            ));
        }
        REAL_RUNNER.output(bin, args, envs, cwd)
    }
    fn status(
        &self,
        bin: &str,
        args: &[&str],
        envs: &[(&str, Option<&str>)],
        quiet: bool,
        cwd: &Path,
    ) -> io::Result<ExitStatus> {
        REAL_RUNNER.status(bin, args, envs, quiet, cwd)
    }
}

/// Called only after the genuine test child dispatcher returns from checkpoint-edit.
#[expect(
    clippy::single_call_fn,
    reason = "named native fixture boundaries keep ownership, stimulus, and conservation visible to the direct canonical provider"
)]
pub(in crate::git_factor::engine::tests) fn after_child_dispatch(
    arguments: &[OsString],
    environment: &NativeEnv,
    code: i32,
) {
    if code != EXIT_OK
        || arguments.get(1).and_then(|argument| argument.to_str()) != Some("checkpoint-edit")
    {
        return;
    }
    let Some(path) = env::var_os("FACTOR_TEST_E1_EDITOR_OBSTRUCTION") else {
        return;
    };
    let configuration: EditorObstruction =
        serde_json::from_slice(&fs::read(path).or_abort("owned editor config"))
            .or_abort("editor configuration grammar");
    if configuration.evidence.join("fired").is_file() {
        return;
    }
    let root = fs::canonicalize(&environment.cwd).or_abort("actual editor root");
    assert_eq!(
        fs::canonicalize(root.join(".git")).or_abort("actual editor admin"),
        configuration.admin
    );
    assert_eq!(
        fs::canonicalize(&environment.executable).or_abort("actual editor launcher"),
        configuration.launcher
    );
    assert_eq!(configuration.path.parent(), Some(root.as_path()));
    let bytes =
        fs::read(configuration.admin.join("factor-journal.json")).or_abort("actual editor journal");
    let journal: serde_json::Value =
        serde_json::from_slice(&bytes).or_abort("actual Opening journal");
    assert_eq!(
        journal
            .pointer("/state/phase")
            .and_then(serde_json::Value::as_str),
        Some("opening")
    );
    assert_eq!(
        journal
            .pointer("/checkpoint")
            .and_then(serde_json::Value::as_str),
        Some(configuration.checkpoint.as_str())
    );
    assert_eq!(
        journal
            .pointer("/state/base")
            .and_then(serde_json::Value::as_str),
        configuration.base.as_deref(),
    );
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(configuration.evidence.join("fired"))
        .or_abort("claim exact editor stimulus once");
    fs::write(configuration.evidence.join("actual-journal"), bytes)
        .or_abort("source boundary evidence");
    let mut input = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&configuration.path)
        .or_abort("only create absent owned path after real editor");
    input
        .write_all(configuration.bytes.as_bytes())
        .or_abort("owned obstruction bytes");
}

#[expect(
    clippy::cognitive_complexity,
    reason = "the closed fixture boundary matrix keeps before/after native interruption and preservation assertions explicit"
)]
#[expect(
    clippy::create_dir,
    reason = "exclusive fixture directories must refuse existing occupancy rather than silently admit unrelated files"
)]
#[expect(
    clippy::too_many_lines,
    reason = "one decisive native lifecycle keeps its exact interruption evidence and full preservation oracle together"
)]
fn amended_conflict_control(control: PredecessorControl, body: &str) {
    let repository = owned_repository();
    repository.write("base", "base\n");
    repository.write("shared", "base\n");
    repository.commit("Base");
    repository.write("atom", "atom\n");
    repository.write("remainder", "remainder\n");
    let selected = repository.commit("Selected source");
    repository.write("shared", "one\n");
    let first = repository.commit("First descendant");
    repository.write("shared", "two\n");
    repository.commit("Second descendant");
    let final_tree = repository.git(&["rev-parse", "HEAD^{tree}"]);
    repository.success(&["--gate", "tests", "true", &selected]);
    repository.git(&["add", "atom"]);
    let evidence = repository.directory.path().join("e1-conflict-evidence");
    fs::create_dir(&evidence).or_abort("owned conflict evidence");
    let configuration = ChildStimulus {
        admin: fs::canonicalize(repository.environment.cwd.join(".git"))
            .or_abort("native conflict admin"),
        launcher: fs::canonicalize(&repository.environment.executable)
            .or_abort("owned conflict launcher"),
        checkpoint: repository.git(&["rev-parse", "refs/heads/main"]),
        original_pick: first,
        callback: "checkpoint-gate-descendant".to_owned(),
        accepted_count: 1,
        boundary: ChildBoundary::AfterWrite,
        evidence,
    };
    install_stimulus(&repository, &configuration);
    assert_ne!(repository.invoke(&["--message", "Extract atom"]).0, EXIT_OK);
    assert!(configuration.evidence.join("fired").is_file());
    let before: serde_json::Value =
        serde_json::from_slice(&repository.journal()).or_abort("published first source slot");
    let before_sha = before
        .pointer("/state/accepted/0")
        .and_then(serde_json::Value::as_str)
        .or_abort("accepted first SHA")
        .to_owned();
    let predecessor = repository.git(&["rev-parse", "HEAD^"]);
    repository.write("shared", body);
    repository.git(&["add", "shared"]);
    repository.git(&["commit", "--amend", "--no-edit", "--quiet"]);
    let amended = repository.git(&["rev-parse", "HEAD"]);
    assert_ne!(amended, before_sha);
    assert_eq!(repository.git(&["rev-parse", "HEAD^"]), predecessor);
    // Native reruns the exact same first source slot, then genuinely conflicts on the second pick.
    assert_ne!(repository.invoke(&["--continue"]).0, EXIT_OK);
    let stopped: serde_json::Value = serde_json::from_slice(&repository.journal())
        .or_abort("same slot replacement before next conflict");
    assert_eq!(
        stopped
            .pointer("/state/accepted")
            .and_then(serde_json::Value::as_array)
            .map(Vec::len),
        Some(1)
    );
    let returned = stopped
        .pointer("/state/accepted/0")
        .and_then(serde_json::Value::as_str)
        .or_abort("returned gated replacement SHA");
    assert_ne!(returned, before_sha);
    assert_eq!(repository.git(&["rev-parse", "HEAD"]), returned);
    assert_eq!(repository.git(&["rev-parse", "HEAD^"]), predecessor);
    let gated_tree = repository.git(&["rev-parse", "HEAD^{tree}"]);
    assert!(
        repository
            .git(&[
                "show",
                "-s",
                "--format=%(trailers:only,unfold=true)",
                "HEAD"
            ])
            .lines()
            .any(|line| line.starts_with("Gate-tests: ") && line.ends_with(&gated_tree))
    );
    assert!(
        configuration
            .admin
            .join("rebase-merge/stopped-sha")
            .is_file()
    );
    assert!(configuration.admin.join("REBASE_HEAD").is_file());
    // Ours is physically/index clean yet still a native stopped pick.
    repository.git(&["checkout", "--ours", "--", "shared"]);
    repository.git(&["add", "shared"]);
    assert_eq!(repository.git(&["diff", "--cached", "--name-only"]), "");
    assert_eq!(repository.git(&["diff", "--name-only"]), "");
    assert!(
        configuration
            .admin
            .join("rebase-merge/stopped-sha")
            .is_file()
    );
    if matches!(control, PredecessorControl::SelectionRewind) {
        let saved = stopped
            .pointer("/state/head")
            .and_then(serde_json::Value::as_str)
            .or_abort("saved selection HEAD");
        let source = stopped
            .pointer("/state/source")
            .and_then(serde_json::Value::as_str)
            .or_abort("saved combined source");
        repository.git(&["update-ref", "--no-deref", "HEAD", saved]);
        repository.git(&["read-tree", source]);
        repository.git(&["checkout-index", "--all", "--force"]);
        let index = repository.index();
        let references = repository.git(&["show-ref"]);
        let journal = repository.journal();
        let native = configuration.admin.join("rebase-merge");
        let done = fs::read(native.join("done")).or_abort("real stopped second pick done");
        let todo =
            fs::read(native.join("git-rebase-todo")).or_abort("real pending second slot callback");
        let refused = repository.invoke(&["--continue"]);
        assert_ne!(refused.0, EXIT_OK);
        assert!(refused.1.is_empty());
        assert!(
            repository
                .output
                .stderr
                .borrow()
                .contains("saved selection HEAD is outside the initial replay promotion frontier")
        );
        assert_eq!(repository.journal(), journal);
        assert_eq!(repository.index(), index);
        assert_eq!(repository.git(&["rev-parse", "HEAD"]), saved);
        assert_eq!(repository.git(&["show-ref"]), references);
        assert_eq!(
            fs::read(native.join("done")).or_abort("done preserved"),
            done
        );
        assert_eq!(
            fs::read(native.join("git-rebase-todo")).or_abort("todo preserved"),
            todo
        );
        assert_eq!(
            fs::read(repository.environment.cwd.join("shared"))
                .or_abort("rewound source bytes preserved"),
            b"base\n"
        );
        assert_eq!(
            fs::read(repository.environment.cwd.join("atom")).or_abort("rewound atom preserved"),
            b"atom\n"
        );
        assert_eq!(
            fs::read(repository.environment.cwd.join("remainder"))
                .or_abort("rewound remainder preserved"),
            b"remainder\n"
        );
        return;
    }
    if matches!(control, PredecessorControl::Amend) {
        repository.git(&[
            "commit",
            "--amend",
            "--quiet",
            "--message",
            "External rewrite of accepted predecessor",
        ]);
        let rewritten = repository.git(&["rev-parse", "HEAD"]);
        assert_ne!(rewritten, returned);
        let index = repository.index();
        let references = repository.git(&["show-ref"]);
        let journal = repository.journal();
        let native = configuration.admin.join("rebase-merge");
        let done = fs::read(native.join("done"))
            .or_abort("actual stopped pick done before predecessor rewrite recovery");
        let todo = fs::read(native.join("git-rebase-todo"))
            .or_abort("actual stopped pick todo before predecessor rewrite recovery");
        let refused = repository.invoke(&["--continue"]);
        assert_ne!(refused.0, EXIT_OK);
        assert!(refused.1.is_empty());
        assert!(repository.output.stderr.borrow().contains("pending native replay rewrote an accepted predecessor; automatic recovery is unavailable"));
        assert_eq!(repository.journal(), journal);
        assert_eq!(repository.index(), index);
        assert_eq!(repository.git(&["rev-parse", "HEAD"]), rewritten);
        assert_eq!(repository.git(&["show-ref"]), references);
        assert_eq!(
            fs::read(native.join("done")).or_abort("rewritten-predecessor done preservation"),
            done
        );
        assert_eq!(
            fs::read(native.join("git-rebase-todo"))
                .or_abort("rewritten-predecessor todo preservation"),
            todo
        );
        assert_eq!(
            fs::read(repository.environment.cwd.join("shared"))
                .or_abort("rewritten predecessor physical bytes"),
            body.as_bytes()
        );
        return;
    }
    let dropped_evidence = repository.directory.path().join("dropped-native-stderr");
    fs::create_dir(&dropped_evidence).or_abort("owned dropped-pick native evidence");
    let observed = OwnedNativeObservation {
        admin: &configuration.admin,
        evidence: &dropped_evidence,
        arguments: &["rebase", "--continue"],
    };
    let (code, _) = repository.invoke_with(&["--continue"], &observed, &REAL_FS);
    assert_ne!(code, EXIT_OK);
    assert!(
        String::from_utf8_lossy(
            &fs::read(dropped_evidence.join("native-stderr"))
                .or_abort("actual native dropped callback stderr")
        )
        .contains("native replay omitted an original descendant; repair with an explicit commit")
    );
    assert!(
        !configuration
            .admin
            .join("rebase-merge/stopped-sha")
            .exists()
    );
    assert!(!configuration.admin.join("REBASE_HEAD").exists());
    let dropped: serde_json::Value =
        serde_json::from_slice(&repository.journal()).or_abort("dropped next slot refused");
    assert_eq!(
        dropped.pointer("/state/accepted"),
        stopped.pointer("/state/accepted")
    );
    assert_eq!(repository.git(&["rev-parse", "HEAD"]), returned);
    // Repair with a real direct child carrying the original fixed final tree.
    repository.write("shared", "two\n");
    repository.git(&["add", "shared"]);
    repository.git(&["commit", "--quiet", "--message", "Repair second descendant"]);
    repository.success(&["--continue"]);
    repository.success(&["--finish"]);
    assert_eq!(repository.git(&["rev-parse", "HEAD^{tree}"]), final_tree);
}

/// Same-slot tree repair replaces durable progress; a dropped next source does not.
pub(in crate::git_factor::engine::tests) fn amended_then_dropped_conflict(body: &str) {
    amended_conflict_control(PredecessorControl::Drop, body);
}

/// Native owner reaping can precede the child's evidence publication by a few ticks.
#[expect(
    clippy::panic,
    reason = "fixture authority mismatches must terminate the test before any native process can be targeted"
)]
fn await_kill_status(evidence: &Path) -> String {
    let attempts: Range<usize> = 0..100;
    for _attempt in attempts {
        match fs::read_to_string(evidence.join("kill-status")) {
            Ok(value) if value == "0\n" => return value,
            Ok(value) if value.is_empty() => {}
            Ok(value) => panic!("unexpected owned kill evidence: {value:?}"),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => panic!("cannot observe owned kill evidence: {error}"),
        }
        thread::sleep(Duration::from_millis(5));
    }
    panic!("owned native kill evidence was not published within its bounded wait");
}

pub(in crate::git_factor::engine::tests) fn before_exec_control(body: &str) {
    let repository = owned_repository();
    repository.write("base", "base\n");
    repository.commit("Base");
    repository.write("atom", body);
    repository.write("remainder", "remainder\n");
    let selected = repository.commit("Selected source");
    repository.write("descendant", "descendant\n");
    repository.commit("Descendant");
    let final_tree = repository.git(&["rev-parse", "HEAD^{tree}"]);
    repository.success(&["--exec", "true", &selected]);
    repository.git(&["add", "atom"]);
    let runner = BeforeNativeContinue {
        refused: Cell::new(false),
    };
    let (code, _) = repository.invoke_with(&["--message", "Extract atom"], &runner, &REAL_FS);
    assert_ne!(code, EXIT_OK);
    assert!(runner.refused.get());
    let journal: serde_json::Value =
        serde_json::from_slice(&repository.journal()).or_abort("actual replay intent");
    assert_eq!(
        journal
            .pointer("/state/accepted")
            .and_then(serde_json::Value::as_array)
            .map(Vec::len),
        Some(0)
    );
    let native = repository.environment.cwd.join(".git/rebase-merge");
    let done = fs::read_to_string(native.join("done")).or_abort("genuine source break");
    assert_eq!(
        done.lines()
            .rev()
            .find(|line| !line.trim().is_empty() && !line.trim_start().starts_with('#')),
        Some("break")
    );
    let todo = fs::read_to_string(native.join("git-rebase-todo"))
        .or_abort("genuine pending remainder callback");
    assert!(
        todo.lines()
            .find(|line| !line.trim().is_empty() && !line.trim_start().starts_with('#'))
            .is_some_and(|line| line.contains("checkpoint-gate-remainder"))
    );
    repository.success(&["--continue"]);
    repository.success(&["--finish"]);
    assert_eq!(repository.git(&["rev-parse", "HEAD^{tree}"]), final_tree);
}

#[expect(
    clippy::cognitive_complexity,
    reason = "the closed fixture boundary matrix keeps before/after native interruption and preservation assertions explicit"
)]
#[expect(
    clippy::create_dir,
    reason = "exclusive fixture directories must refuse existing occupancy rather than silently admit unrelated files"
)]
#[expect(
    clippy::literal_string_with_formatting_args,
    reason = "literal Git revision selectors and expected serialized data intentionally contain braces"
)]
#[expect(
    clippy::too_many_lines,
    reason = "one decisive native lifecycle keeps its exact interruption evidence and full preservation oracle together"
)]
fn bypass_control(boundary: BypassBoundary, body: &str) {
    let repository = owned_repository();
    repository.write("base", "base\n");
    repository.commit("Base");
    repository.write("atom", "atom\n");
    repository.write("second_atom", "second atom\n");
    repository.write("remainder", body);
    let selected = repository.commit("Selected source");
    if !matches!(boundary, BypassBoundary::RemainderWithoutDescendants) {
        repository.write("first", "first\n");
        repository.commit("First descendant");
        repository.write("second", "second\n");
        repository.commit("Second descendant");
    }
    repository.success(&["--gate", "tests", "true", &selected]);
    repository.git(&["add", "atom"]);
    repository.success(&["--message", "First completed atom"]);
    let previous: serde_json::Value =
        serde_json::from_slice(&repository.journal()).or_abort("completed first round");
    let earlier_atom = previous
        .pointer("/state/base")
        .and_then(serde_json::Value::as_str)
        .or_abort("earlier captured atom")
        .to_owned();
    let checkpoint = repository.git(&["rev-parse", "refs/heads/main"]);
    let checkpoint_tree = repository.git(&["rev-parse", "refs/heads/main^{tree}"]);
    let remainder_callback = matches!(
        boundary,
        BypassBoundary::RemainderWithoutDescendants
            | BypassBoundary::RemainderWithDescendants
            | BypassBoundary::RemainderPendingPick
    );
    let original_pick = if remainder_callback {
        previous
            .pointer("/state/source")
            .and_then(serde_json::Value::as_str)
            .or_abort("real remaining source pick")
            .to_owned()
    } else if matches!(boundary, BypassBoundary::FinalDescendant) {
        repository.git(&["rev-parse", "refs/heads/main"])
    } else {
        repository.git(&["rev-parse", "refs/heads/main~1"])
    };
    let callback = if remainder_callback {
        "checkpoint-gate-remainder"
    } else {
        "checkpoint-gate-descendant"
    };
    repository.write("unrelated", "preserve user bytes\n");
    repository.git(&["add", "second_atom"]);
    let evidence = repository.directory.path().join("bypass-evidence");
    fs::create_dir(&evidence).or_abort("owned bypass evidence");
    let configuration = ChildStimulus {
        admin: fs::canonicalize(repository.environment.cwd.join(".git"))
            .or_abort("actual bypass admin"),
        launcher: fs::canonicalize(&repository.environment.executable)
            .or_abort("actual bypass launcher"),
        checkpoint: checkpoint.clone(),
        original_pick,
        callback: callback.to_owned(),
        accepted_count: usize::from(!remainder_callback),
        boundary: ChildBoundary::BeforeCallbackKill,
        evidence,
    };
    install_stimulus(&repository, &configuration);
    let runner = OwnedNativeContinue {
        configuration: &configuration,
    };
    assert_ne!(
        repository
            .invoke_with(&["--message", "Second proposed atom"], &runner, &REAL_FS)
            .0,
        EXIT_OK
    );
    assert!(configuration.evidence.join("fired").is_file());
    assert_eq!(await_kill_status(&configuration.evidence), "0\n");
    let paused: serde_json::Value =
        serde_json::from_slice(&repository.journal()).or_abort("genuine lost callback journal");
    assert_eq!(
        paused
            .pointer("/state/accepted")
            .and_then(serde_json::Value::as_array)
            .map(Vec::len),
        Some(usize::from(matches!(
            boundary,
            BypassBoundary::FinalDescendant
        )))
    );
    assert_eq!(
        paused
            .pointer("/state/remainder/state")
            .and_then(serde_json::Value::as_str),
        Some(if remainder_callback {
            "pending"
        } else {
            "accepted"
        })
    );
    let native = configuration.admin.join("rebase-merge");
    let done =
        fs::read_to_string(native.join("done")).or_abort("consumed unaccepted original slot");
    assert!(
        done.lines()
            .rev()
            .find(|line| !line.trim().is_empty() && !line.trim_start().starts_with('#'))
            .is_some_and(|line| line.contains(callback))
    );
    let pending_pick = matches!(
        boundary,
        BypassBoundary::IntermediatePendingPick | BypassBoundary::RemainderPendingPick
    );
    let obstructed = if remainder_callback {
        "first"
    } else {
        "second"
    };
    if pending_pick {
        repository.write(obstructed, "preserved external pick obstruction\n");
    }
    let bypass = Command::new("git")
        .args(["rebase", "--continue"])
        .env("GIT_EDITOR", "true")
        .current_dir(&repository.environment.cwd)
        .output()
        .or_abort("actual manual native bypass");
    assert!(!bypass.status.success());
    assert!(
        String::from_utf8_lossy(&bypass.stderr).contains(if pending_pick {
            "untracked working tree files"
        } else if remainder_callback {
            "remainder lacks independent acceptance"
        } else if matches!(boundary, BypassBoundary::FinalDescendant) {
            "terminal replay omitted an original descendant"
        } else {
            "native replay skipped or rewound unverified descendant progress"
        })
    );
    let saved_obstruction = repository
        .directory
        .path()
        .join("saved-pending-obstruction");
    if pending_pick {
        let native_done =
            fs::read_to_string(native.join("done")).or_abort("actual failed future pick done");
        let native_todo = fs::read_to_string(native.join("git-rebase-todo"))
            .or_abort("actual rescheduled future pick todo");
        let last = native_done
            .lines()
            .rev()
            .find(|line| !line.trim().is_empty() && !line.trim_start().starts_with('#'))
            .or_abort("actual failed pick done line");
        let first = native_todo
            .lines()
            .find(|line| !line.trim().is_empty() && !line.trim_start().starts_with('#'))
            .or_abort("actual first pending retry pick");
        assert!(last.starts_with("pick "));
        assert_eq!(last, first);
        fs::rename(
            repository.environment.cwd.join(obstructed),
            &saved_obstruction,
        )
        .or_abort("move only owned obstruction before factor Act");
    }
    let journal = repository.journal();
    let index = repository.index();
    let head = repository.git(&["rev-parse", "HEAD"]);
    let tree = repository.git(&["rev-parse", "HEAD^{tree}"]);
    let refs = repository.git(&["show-ref"]);
    let bypassed_done = fs::read(native.join("done")).or_abort("post-bypass done");
    let todo = fs::read(native.join("git-rebase-todo")).or_abort("post-bypass todo");
    let (code, output) = repository.invoke(&["--continue"]);
    assert_ne!(code, EXIT_OK);
    assert_eq!(output, "");
    assert!(repository.output.stderr.borrow().contains(if pending_pick && !remainder_callback {
        "native replay skipped an unaccepted original descendant; automatic recovery is unavailable"
    } else if remainder_callback {
        "remainder lacks independent acceptance; automatic recovery is unavailable"
    } else if matches!(boundary, BypassBoundary::FinalDescendant) {
        "terminal replay omitted an original descendant; automatic recovery is unavailable"
    } else { "current replay HEAD is outside its accepted predecessor corridor; automatic recovery is unavailable" }));
    assert_eq!(repository.journal(), journal);
    assert_eq!(repository.index(), index);
    assert_eq!(repository.git(&["rev-parse", "HEAD"]), head);
    assert_eq!(repository.git(&["rev-parse", "HEAD^{tree}"]), tree);
    assert_eq!(repository.git(&["show-ref"]), refs);
    assert_eq!(
        fs::read(native.join("done")).or_abort("refused native done unchanged"),
        bypassed_done
    );
    assert_eq!(
        fs::read(native.join("git-rebase-todo")).or_abort("refused native todo unchanged"),
        todo
    );
    assert_eq!(
        repository.git(&["rev-parse", "refs/heads/main"]),
        checkpoint
    );
    assert_eq!(
        repository.git(&["rev-parse", "refs/heads/main^{tree}"]),
        checkpoint_tree
    );
    assert!(
        Command::new("git")
            .args([
                "merge-base",
                "--is-ancestor",
                &earlier_atom,
                "refs/heads/main"
            ])
            .current_dir(&repository.environment.cwd)
            .status()
            .or_abort("earlier durable checkpoint remains ancestor")
            .success()
    );
    for (path, expected) in [
        ("base", "base\n"),
        ("atom", "atom\n"),
        ("second_atom", "second atom\n"),
        ("remainder", body),
        ("unrelated", "preserve user bytes\n"),
    ] {
        assert_eq!(
            fs::read(repository.environment.cwd.join(path))
                .or_abort("bypass refusal physical conservation"),
            expected.as_bytes()
        );
    }
    if pending_pick {
        assert_eq!(
            fs::read(&saved_obstruction).or_abort("pending bypass relocated input conservation"),
            b"preserved external pick obstruction\n"
        );
        assert!(!repository.environment.cwd.join(obstructed).exists());
        if !remainder_callback {
            assert_eq!(
                fs::read(repository.environment.cwd.join("first"))
                    .or_abort("unaccepted first descendant stays physical"),
                b"first\n"
            );
        }
    } else if !matches!(boundary, BypassBoundary::RemainderWithoutDescendants) {
        assert_eq!(
            fs::read(repository.environment.cwd.join("first"))
                .or_abort("bypass first descendant bytes"),
            b"first\n"
        );
        if remainder_callback {
            assert!(!repository.environment.cwd.join("second").exists());
        } else {
            assert_eq!(
                fs::read(repository.environment.cwd.join("second"))
                    .or_abort("bypass second descendant bytes"),
                b"second\n"
            );
        }
    } else {
        // The no-descendant world has no descendant byte obligations.
    }
    // No abort-success promise outside the admitted accepted corridor.
}

/// Native bypass may skip a consumed unaccepted slot; public recovery refuses truthfully.
pub(in crate::git_factor::engine::tests) fn bypass_lost_callback_refusal(body: &str) {
    bypass_control(BypassBoundary::IntermediateDescendant, body);
}

pub(in crate::git_factor::engine::tests) fn bypass_pending_pick_refusal(
    remainder: bool,
    body: &str,
) {
    bypass_control(
        if remainder {
            BypassBoundary::RemainderPendingPick
        } else {
            BypassBoundary::IntermediatePendingPick
        },
        body,
    );
}

#[expect(
    clippy::cognitive_complexity,
    reason = "the closed fixture boundary matrix keeps before/after native interruption and preservation assertions explicit"
)]
#[expect(
    clippy::create_dir,
    reason = "exclusive fixture directories must refuse existing occupancy rather than silently admit unrelated files"
)]
#[expect(
    clippy::too_many_lines,
    reason = "one decisive native lifecycle keeps its exact interruption evidence and full preservation oracle together"
)]
fn child_interruption(callback: &str, final_slot: bool, boundary: ChildBoundary, body: &str) {
    let repository = owned_repository();
    repository.write("base", "base\n");
    repository.commit("Base");
    repository.write("atom", "atom\n");
    repository.write("remainder", "remainder\n");
    let selected = repository.commit("Selected source");
    repository.write("first", body);
    let first = repository.commit("First descendant");
    repository.write("second", "second\n");
    let second = repository.commit("Second descendant");
    let final_tree = repository.git(&["rev-parse", "HEAD^{tree}"]);
    if matches!(boundary, ChildBoundary::BeforeWrite) {
        repository.git(&["branch", "foreign", &second]);
        repository.git(&["tag", "foreign", &second]);
    }

    repository.write("unrelated", "preserved unrelated bytes\n");
    let proofs = repository.directory.path().join("e1-proofs");
    fs::create_dir(&proofs).or_abort("owned acceptance proof logs");
    let gates = proofs.join("gate-trees");
    let hooks = proofs.join("hook-trees");
    let hook_messages = proofs.join("hook-messages");
    let gate = format!(
        "git write-tree >> {}",
        shell_quote(gates.to_str().or_abort("tree gate observation path"))
    );
    let hook = repository
        .environment
        .cwd
        .join(".git/e1-owned-hooks/commit-msg");
    fs::write(
        &hook,
        format!(
            "#!/bin/bash\nset -eu\ngit write-tree >> {}\ngit hash-object \"$1\" >> {}\n",
            shell_quote(hooks.to_str().or_abort("native hook observation path")),
            shell_quote(
                hook_messages
                    .to_str()
                    .or_abort("native message observation path")
            )
        ),
    )
    .or_abort("owned native acceptance hook");
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o700))
        .or_abort("native hook executable");
    repository.git(&[
        "config",
        "core.hooksPath",
        hook.parent()
            .or_abort("hook directory")
            .to_str()
            .or_abort("native hook directory UTF-8"),
    ]);
    repository.success(&["--gate", "tests", &gate, &selected]);
    let selecting: serde_json::Value =
        serde_json::from_slice(&repository.journal()).or_abort("admitted selecting source");
    let source = selecting
        .pointer("/state/source")
        .and_then(serde_json::Value::as_str)
        .or_abort("combined native source pick")
        .to_owned();
    repository.git(&["add", "atom"]);
    let evidence = repository.directory.path().join("e1-evidence");
    fs::create_dir(&evidence).or_abort("owned E1 evidence");
    let configuration = ChildStimulus {
        admin: fs::canonicalize(repository.environment.cwd.join(".git"))
            .or_abort("native fixture admin"),
        launcher: fs::canonicalize(&repository.environment.executable)
            .or_abort("owned fixture launcher"),
        checkpoint: repository.git(&["rev-parse", "refs/heads/main"]),
        original_pick: if callback == "checkpoint-gate-remainder" {
            source
        } else if final_slot {
            second.clone()
        } else {
            first
        },
        callback: callback.to_owned(),
        accepted_count: if callback == "checkpoint-gate-remainder" {
            0
        } else if final_slot {
            2
        } else {
            1
        },
        boundary,
        evidence,
    };
    install_stimulus(&repository, &configuration);
    let runner = OwnedNativeContinue {
        configuration: &configuration,
    };
    let (code, _) = repository.invoke_with(&["--message", "Extract atom"], &runner, &REAL_FS);

    if matches!(boundary, ChildBoundary::BeforeWrite) {
        // This immediate readback precedes all observing Git commands.
        assert_eq!(
            repository.index(),
            fs::read(configuration.evidence.join("boundary-index")).or_abort("raw boundary index")
        );
        for (name, arguments) in [
            ("boundary-head", vec!["rev-parse", "HEAD"]),
            ("boundary-checkpoint", vec!["rev-parse", "refs/heads/main"]),
            ("boundary-refs", vec!["show-ref"]),
        ] {
            assert_eq!(
                repository.git(&arguments),
                fs::read_to_string(configuration.evidence.join(name))
                    .or_abort("boundary native facts")
                    .trim_end()
            );
        }
        assert_eq!(repository.git(&["rev-parse", "refs/heads/foreign"]), second);
        assert_eq!(repository.git(&["rev-parse", "refs/tags/foreign"]), second);
        assert_eq!(
            repository.git(&["rev-parse", "refs/heads/main"]),
            configuration.checkpoint
        );
        for (name, expected) in [
            ("base", Some("base\n")),
            ("atom", Some("atom\n")),
            ("remainder", Some("remainder\n")),
            (
                "first",
                (callback != "checkpoint-gate-remainder").then_some(body),
            ),
            (
                "second",
                (callback != "checkpoint-gate-remainder" && final_slot).then_some("second\n"),
            ),
            ("unrelated", Some("preserved unrelated bytes\n")),
        ] {
            let bytes = fs::read(configuration.evidence.join(format!("boundary-{name}"))).ok();
            assert_eq!(fs::read(repository.environment.cwd.join(name)).ok(), bytes);
            assert_eq!(bytes.as_deref(), expected.map(str::as_bytes));
        }
    }
    assert_ne!(code, EXIT_OK);
    assert!(
        configuration.evidence.join("fired").is_file(),
        "child boundary did not fire"
    );
    let after_write = matches!(
        boundary,
        ChildBoundary::AfterWrite | ChildBoundary::AfterWriteKill
    );
    assert_eq!(
        fs::read_to_string(configuration.evidence.join("boundary"))
            .or_abort("actual publication boundary"),
        if after_write {
            "after-write-returned-successfully"
        } else if matches!(boundary, ChildBoundary::BeforeCallbackKill) {
            "before-callback"
        } else {
            "before-write"
        }
    );
    let journal: serde_json::Value =
        serde_json::from_slice(&repository.journal()).or_abort("paused publication journal");
    if matches!(boundary, ChildBoundary::BeforeWrite) {
        assert_eq!(
            repository.journal(),
            fs::read(configuration.evidence.join("observed-journal.json"))
                .or_abort("exact pre-publication durable journal")
        );
        let proposed: serde_json::Value = serde_json::from_slice(
            &fs::read(configuration.evidence.join("proposed-journal.json"))
                .or_abort("actual accepted proposal"),
        )
        .or_abort("proposal JSON");
        if callback == "checkpoint-gate-remainder" {
            assert_eq!(
                journal
                    .pointer("/state/remainder/state")
                    .and_then(serde_json::Value::as_str),
                Some("pending")
            );
            assert_eq!(
                proposed
                    .pointer("/state/remainder/state")
                    .and_then(serde_json::Value::as_str),
                Some("accepted")
            );
        }
        if callback == "checkpoint-terminal" {
            assert_eq!(
                journal
                    .pointer("/state/phase")
                    .and_then(serde_json::Value::as_str),
                Some("replaying")
            );
            assert_eq!(
                proposed
                    .pointer("/state/phase")
                    .and_then(serde_json::Value::as_str),
                Some("verified")
            );
        }
    }
    let current_tree = repository.git(&["rev-parse", "HEAD^{tree}"]);
    let count = |path: &Path| {
        fs::read_to_string(path)
            .or_abort("actual proof log")
            .lines()
            .filter(|line| *line == current_tree)
            .count()
    };
    let before_gate_count = count(&gates);
    let before_hook_count = count(&hooks);
    if matches!(boundary, ChildBoundary::BeforeCallbackKill)
        && callback == "checkpoint-gate-descendant"
    {
        assert_eq!(
            before_gate_count, 0,
            "consumed next tree was never validated before interruption"
        );
    } else {
        assert_eq!(
            before_gate_count, 1,
            "tree-only gate has one positive proof for this exact tree"
        );
    }
    if callback == "checkpoint-gate-descendant" {
        assert_eq!(
            journal
                .pointer("/state/accepted")
                .and_then(serde_json::Value::as_array)
                .map(Vec::len),
            Some(
                configuration
                    .accepted_count
                    .checked_sub(usize::from(!after_write))
                    .or_abort("admitted publication count")
            )
        );
    }
    assert_eq!(
        journal
            .pointer("/state/phase")
            .and_then(serde_json::Value::as_str),
        Some(if callback == "checkpoint-terminal" && after_write {
            "verified"
        } else {
            "replaying"
        })
    );
    if after_write || matches!(boundary, ChildBoundary::BeforeWrite) {
        assert!(
            before_hook_count > 0,
            "the proposed publication follows actual native acceptance"
        );
        let mut hash = Command::new("git")
            .args(["hash-object", "--stdin"])
            .current_dir(&repository.environment.cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .or_abort("hash exact accepted native message");
        hash.stdin
            .take()
            .or_abort("owned message hash input")
            .write_all(&raw_current_message(&repository))
            .or_abort("raw accepted message bytes");
        let result = hash.wait_with_output().or_abort("native message hash");
        assert!(result.status.success());
        let expected = String::from_utf8(result.stdout).or_abort("native message identity UTF-8");
        assert!(
            fs::read_to_string(&hook_messages)
                .or_abort("actual hook message evidence")
                .lines()
                .any(|observed| observed == expected.trim_end()),
            "published exact message must have reached native acceptance hook"
        );
        if callback == "checkpoint-terminal" && after_write {
            let verified_tip = repository.git(&["rev-parse", "HEAD"]);
            assert_eq!(
                journal
                    .pointer("/state/tip")
                    .and_then(serde_json::Value::as_str),
                Some(verified_tip.as_str())
            );
        }
    }
    let todo = fs::read_to_string(configuration.admin.join("rebase-merge/git-rebase-todo"))
        .or_abort("actual rescheduled todo");
    if matches!(
        boundary,
        ChildBoundary::BeforeWrite | ChildBoundary::AfterWrite
    ) {
        assert!(todo.lines().any(|line| line.contains(callback)));
    } else {
        assert_eq!(await_kill_status(&configuration.evidence), "0\n");
        assert!(
            !todo
                .lines()
                .find(|line| !line.trim().is_empty() && !line.trim_start().starts_with('#'))
                .is_some_and(|line| line.contains(callback))
        );
        let done = fs::read_to_string(configuration.evidence.join("done"))
            .or_abort("actual consumed callback");
        assert!(
            done.lines()
                .rev()
                .find(|line| !line.trim().is_empty() && !line.trim_start().starts_with('#'))
                .is_some_and(|line| line.contains(callback))
        );
    }
    let atom = journal
        .pointer("/state/atom")
        .and_then(serde_json::Value::as_str)
        .or_abort("captured proposed atom")
        .to_owned();
    let resumed = repository.success(&["--continue"]);
    assert_eq!(
        count(&gates),
        1,
        "recovery must validate each new tree exactly once and reuse positive proofs"
    );
    if callback == "checkpoint-gate-descendant" && !final_slot {
        let trees = fs::read_to_string(&gates).or_abort("ordered native tree proofs");
        let proof_position = trees
            .lines()
            .position(|tree| tree == current_tree)
            .or_abort("intermediate tree proof");
        let last = trees
            .lines()
            .position(|tree| tree == final_tree)
            .or_abort("later final tree proof");
        assert!(
            proof_position < last,
            "the intermediate tree must be gated before the later descendant"
        );
    }
    if matches!(boundary, ChildBoundary::BeforeCallbackKill) {
        assert!(
            count(&hooks) > before_hook_count,
            "cached tree proof must not skip native message acceptance"
        );
    } else if matches!(boundary, ChildBoundary::AfterWriteKill) {
        assert!(
            count(&hooks) >= before_hook_count,
            "durably accepted message evidence remains authoritative"
        );
    } else {
        // Graceful retries retain their existing acceptance evidence.
    }
    assert_eq!(
        resumed
            .pointer("/operation")
            .and_then(serde_json::Value::as_str),
        Some("continue")
    );
    assert_eq!(
        repository.git(&["rev-parse", "refs/heads/main^{tree}"]),
        final_tree
    );
    assert_eq!(
        repository.git(&["rev-list", "--count", &format!("{atom}..refs/heads/main")]),
        "3"
    );
    assert!(
        Command::new("git")
            .args(["merge-base", "--is-ancestor", &atom, "refs/heads/main"])
            .current_dir(&repository.environment.cwd)
            .status()
            .or_abort("atom checkpoint ancestry")
            .success()
    );

    if matches!(boundary, ChildBoundary::BeforeWrite) {
        assert_eq!(repository.git(&["rev-parse", "refs/heads/foreign"]), second);
        assert_eq!(repository.git(&["rev-parse", "refs/tags/foreign"]), second);
        let checkpoint = repository.git(&["rev-parse", "refs/heads/main"]);
        assert_ne!(checkpoint, configuration.checkpoint);
        repository.success(&["--abort"]);
        assert_eq!(repository.git(&["rev-parse", "HEAD"]), checkpoint);
    } else {
        repository.success(&["--finish"]);
    }
    assert_eq!(repository.git(&["rev-parse", "HEAD^{tree}"]), final_tree);
    assert_eq!(
        fs::read(repository.environment.cwd.join("unrelated")).or_abort("foreign bytes preserved"),
        b"preserved unrelated bytes\n"
    );
}

pub(in crate::git_factor::engine::tests) fn consumed_intermediate_retry(body: &str) {
    child_interruption(
        "checkpoint-gate-descendant",
        false,
        ChildBoundary::BeforeCallbackKill,
        body,
    );
}

pub(in crate::git_factor::engine::tests) fn consumed_retry(
    callback: &str,
    after_write: bool,
    body: &str,
) {
    child_interruption(
        callback,
        true,
        if after_write {
            ChildBoundary::AfterWriteKill
        } else {
            ChildBoundary::BeforeCallbackKill
        },
        body,
    );
}

/// Interrupted publication after the real gate/message promotion, before journal write.
pub(in crate::git_factor::engine::tests) fn validated_publication_retry(
    callback: &str,
    body: &str,
) {
    child_interruption(callback, true, ChildBoundary::BeforeWrite, body);
}

/// Actual filesystem symlink aliases bind to one canonical callback identity.
pub(in crate::git_factor::engine::tests) fn executable_alias_continuation(body: &str) {
    let mut repository = owned_repository();
    repository.write("base", "base\n");
    repository.commit("Base");
    repository.write("atom", "atom\n");
    repository.write("remainder", body);
    let selected = repository.commit("Selected source");
    repository.write("descendant", "descendant\n");
    repository.commit("Real descendant");
    let final_tree = repository.git(&["rev-parse", "HEAD^{tree}"]);
    let launcher =
        fs::canonicalize(&repository.environment.executable).or_abort("canonical real launcher");
    let first = repository.directory.path().join("first-launcher-alias");
    let second = repository.directory.path().join("second-launcher-alias");
    symlink(&launcher, &first).or_abort("first actual executable symlink");
    symlink(&launcher, &second).or_abort("second actual executable symlink");
    repository.environment.executable = first.clone();
    repository.success(&["--gate", "tests", "true", &selected]);
    repository.git(&["add", "atom"]);
    let pause = BeforeNativeContinue {
        refused: Cell::new(false),
    };
    assert_ne!(
        repository
            .invoke_with(&["--message", "Extract aliased atom"], &pause, &REAL_FS)
            .0,
        EXIT_OK
    );
    assert!(pause.refused.get());
    let native = repository.environment.cwd.join(".git/rebase-merge");
    let todo = fs::read_to_string(native.join("git-rebase-todo"))
        .or_abort("actual canonical callback todo");
    let canonical = shell_quote(launcher.to_str().or_abort("canonical launcher UTF-8"));
    assert!(
        todo.lines()
            .filter(|line| line.starts_with("exec "))
            .all(|line| line.starts_with(&format!("exec {canonical} checkpoint-")))
    );
    assert!(!todo.contains(first.to_str().or_abort("first alias UTF-8")));
    assert!(!todo.contains(second.to_str().or_abort("second alias UTF-8")));
    repository.environment.executable = second;
    repository.success(&["--continue"]);
    repository.success(&["--finish"]);
    assert_eq!(repository.git(&["rev-parse", "HEAD^{tree}"]), final_tree);
}

/// Different actual hard-link/copy paths are not silently trusted as callback aliases.
pub(in crate::git_factor::engine::tests) fn executable_distinct_path_refusal(
    hard_link: bool,
    body: &str,
) {
    let mut repository = owned_repository();
    repository.write("base", "base\n");
    repository.commit("Base");
    repository.write("atom", "atom\n");
    repository.write("remainder", body);
    let selected = repository.commit("Selected source");
    repository.write("descendant", "descendant\n");
    repository.commit("Real descendant");
    repository.success(&["--gate", "tests", "true", &selected]);
    repository.git(&["add", "atom"]);
    let pause = BeforeNativeContinue {
        refused: Cell::new(false),
    };
    assert_ne!(
        repository
            .invoke_with(&["--message", "Extract atom"], &pause, &REAL_FS)
            .0,
        EXIT_OK
    );
    assert!(pause.refused.get());
    let other = repository.directory.path().join("different-launcher");
    if hard_link {
        fs::hard_link(&repository.environment.executable, &other)
            .or_abort("actual distinct hardlink path");
    } else {
        fs::copy(&repository.environment.executable, &other)
            .or_abort("actual different launcher copy");
    }
    assert_ne!(
        fs::canonicalize(&other).or_abort("different actual canonical path"),
        fs::canonicalize(&repository.environment.executable)
            .or_abort("original actual canonical path")
    );
    repository.environment.executable = other;
    let journal = repository.journal();
    let index = repository.index();
    let head = repository.git(&["rev-parse", "HEAD"]);
    let refs = repository.git(&["show-ref"]);
    let native = repository.environment.cwd.join(".git/rebase-merge");
    let done = fs::read(native.join("done")).or_abort("distinct path native done");
    let todo = fs::read(native.join("git-rebase-todo")).or_abort("distinct path native todo");
    assert_ne!(repository.invoke(&["--continue"]).0, EXIT_OK);
    assert!(
        repository
            .output
            .stderr
            .borrow()
            .contains("native replay contains a foreign exec payload")
    );
    assert_eq!(repository.journal(), journal);
    assert_eq!(repository.index(), index);
    assert_eq!(repository.git(&["rev-parse", "HEAD"]), head);
    assert_eq!(repository.git(&["show-ref"]), refs);
    assert_eq!(
        fs::read(native.join("done")).or_abort("distinct path done conservation"),
        done
    );
    assert_eq!(
        fs::read(native.join("git-rebase-todo")).or_abort("distinct path todo conservation"),
        todo
    );
    assert_eq!(
        fs::read(repository.environment.cwd.join("atom")).or_abort("distinct path atom bytes"),
        b"atom\n"
    );
    assert_eq!(
        fs::read(repository.environment.cwd.join("remainder"))
            .or_abort("distinct path remainder bytes"),
        body.as_bytes()
    );
}

#[expect(
    clippy::too_many_lines,
    reason = "one decisive native lifecycle keeps its exact interruption evidence and full preservation oracle together"
)]
pub(in crate::git_factor::engine::tests) fn executable_observation_refusal(
    boundary: ExecutableFailure,
    body: &str,
) {
    let mut repository = owned_repository();
    repository.write("base", "base\n");
    repository.commit("Base");
    repository.write("atom", "atom\n");
    repository.write("remainder", body);
    let selected = repository.commit("Selected source");
    repository.write("descendant", "descendant\n");
    repository.commit("Real descendant");
    repository.success(&["--gate", "tests", "true", &selected]);
    repository.git(&["add", "atom"]);
    let pause = BeforeNativeContinue {
        refused: Cell::new(false),
    };
    assert_ne!(
        repository
            .invoke_with(&["--message", "Extract atom"], &pause, &REAL_FS)
            .0,
        EXIT_OK
    );
    assert!(pause.refused.get());
    match boundary {
        ExecutableFailure::CurrentExe => {}
        ExecutableFailure::Canonicalize => {
            repository.environment.executable =
                repository.directory.path().join("known-absent-launcher");
            assert!(!repository.environment.executable.exists());
        }
        #[cfg(target_os = "linux")]
        ExecutableFailure::Utf8 => {
            let actual = repository
                .directory
                .path()
                .join(OsString::from_vec(b"actual-launcher-\xff".to_vec()));
            fs::copy(&repository.environment.executable, &actual)
                .or_abort("actual nonUTF8 executable path");
            repository.environment.executable = actual;
        }
    }
    let journal = repository.journal();
    let index = repository.index();
    let head = repository.git(&["rev-parse", "HEAD"]);
    let refs = repository.git(&["show-ref"]);
    let native = repository.environment.cwd.join(".git/rebase-merge");
    let done = fs::read(native.join("done")).or_abort("observation-error native done");
    let todo = fs::read(native.join("git-rebase-todo")).or_abort("observation-error native todo");
    repository.output.stdout.borrow_mut().clear();
    repository.output.stderr.borrow_mut().clear();
    let unavailable = UnavailableExecutable(&repository.environment);
    let environment: &dyn Env = if matches!(boundary, ExecutableFailure::CurrentExe) {
        &unavailable
    } else {
        &repository.environment
    };
    let context = Ctx {
        cwd: repository.environment.cwd.clone(),
        env: environment,
        fs: &REAL_FS,
        io: &repository.output,
        runner: &REAL_RUNNER,
    };
    let arguments = [OsString::from("git-factor"), OsString::from("--continue")];
    let code = main_entry_with_vec(context.io, Ok(context), &arguments);
    assert_ne!(code, EXIT_OK);
    assert_eq!(*repository.output.stdout.borrow(), "");
    match boundary {
        ExecutableFailure::CurrentExe => assert!(
            repository
                .output
                .stderr
                .borrow()
                .contains("owned executable observation failure")
        ),
        ExecutableFailure::Canonicalize => assert!(
            repository
                .output
                .stderr
                .borrow()
                .to_ascii_lowercase()
                .contains("no such file")
        ),
        #[cfg(target_os = "linux")]
        ExecutableFailure::Utf8 => assert!(
            repository
                .output
                .stderr
                .borrow()
                .contains("executable path is not UTF-8")
        ),
    }
    assert_eq!(repository.journal(), journal);
    assert_eq!(repository.index(), index);
    assert_eq!(repository.git(&["rev-parse", "HEAD"]), head);
    assert_eq!(repository.git(&["show-ref"]), refs);
    assert_eq!(
        fs::read(native.join("done")).or_abort("observation-error done preserved"),
        done
    );
    assert_eq!(
        fs::read(native.join("git-rebase-todo")).or_abort("observation-error todo preserved"),
        todo
    );
    assert_eq!(
        fs::read(repository.environment.cwd.join("atom")).or_abort("observation-error atom bytes"),
        b"atom\n"
    );
    assert_eq!(
        fs::read(repository.environment.cwd.join("remainder"))
            .or_abort("observation-error remainder bytes"),
        body.as_bytes()
    );
}

#[expect(
    clippy::cognitive_complexity,
    reason = "the closed fixture boundary matrix keeps before/after native interruption and preservation assertions explicit"
)]
#[expect(
    clippy::create_dir,
    reason = "exclusive fixture directories must refuse existing occupancy rather than silently admit unrelated files"
)]
#[expect(
    clippy::too_many_lines,
    reason = "one decisive native lifecycle keeps its exact interruption evidence and full preservation oracle together"
)]
fn failed_pick_control(
    direct_native_second_failure: bool,
    duplicate_last: bool,
    body: &str,
    obstruction: &str,
) {
    let repository = owned_repository();
    repository.write("base", "base\n");
    repository.commit("Base");
    repository.write("atom", "atom\n");
    repository.write("remainder", "remainder\n");
    let selected = repository.commit("Selected source");
    repository.write("first", "first\n");
    let first = repository.commit("First descendant");
    repository.write("later", body);
    let second = repository.commit("Second descendant");
    let final_tree = repository.git(&["rev-parse", "HEAD^{tree}"]);
    let gates = repository.directory.path().join("failed-pick-gate-trees");
    let gate = format!(
        "git write-tree >> {}",
        shell_quote(gates.to_str().or_abort("native proof log UTF-8"))
    );
    repository.success(&["--gate", "tests", &gate, &selected]);
    repository.git(&["add", "atom"]);
    let evidence = repository.directory.path().join("failed-pick-evidence");
    fs::create_dir(&evidence).or_abort("owned failed-pick evidence");
    let configuration = ChildStimulus {
        admin: fs::canonicalize(repository.environment.cwd.join(".git"))
            .or_abort("native failed-pick admin"),
        launcher: fs::canonicalize(&repository.environment.executable)
            .or_abort("failed-pick launcher"),
        checkpoint: repository.git(&["rev-parse", "refs/heads/main"]),
        original_pick: first,
        callback: "checkpoint-gate-descendant".to_owned(),
        accepted_count: 1,
        boundary: ChildBoundary::AfterWrite,
        evidence,
    };
    install_stimulus(&repository, &configuration);
    assert_ne!(repository.invoke(&["--message", "Extract atom"]).0, EXIT_OK);
    assert!(configuration.evidence.join("fired").is_file());
    repository.write("later", obstruction);
    // The engine now preserves obstruction before delegation. Establish the real
    // native failed-pick transcript by this explicitly owned external Git action.
    let first_failure = Command::new("git")
        .args(["rebase", "--continue"])
        .env("GIT_EDITOR", "true")
        .current_dir(&repository.environment.cwd)
        .output()
        .or_abort("actual owned first native failed pick");
    assert!(!first_failure.status.success());
    assert!(
        String::from_utf8_lossy(&first_failure.stderr).contains("untracked working tree files")
    );
    let native = configuration.admin.join("rebase-merge");
    let last_pick = |path: &Path| {
        fs::read_to_string(path)
            .or_abort("native failed-pick transcript")
            .lines()
            .filter_map(|line| {
                let mut fields = line.split_whitespace();
                match fields.next() {
                    Some("pick" | "p") => fields.next().map(str::to_owned),
                    _ => None,
                }
            })
            .collect::<Vec<_>>()
    };
    let resolve =
        |token: &str| repository.git(&["rev-parse", "--verify", &format!("{token}^{{commit}}")]);
    let before = last_pick(&native.join("done"));
    assert_eq!(
        resolve(before.last().or_abort("actual failed source slot")),
        second
    );
    let pending = last_pick(&native.join("git-rebase-todo"));
    assert_eq!(
        resolve(pending.first().or_abort("real rescheduled source pick")),
        second
    );
    let journal = repository.journal();
    let index = repository.index();
    let head = repository.git(&["rev-parse", "HEAD"]);
    let refs = repository.git(&["show-ref"]);
    if duplicate_last {
        let path = native.join("git-rebase-todo");
        let original = fs::read_to_string(&path).or_abort("real last-done retry todo");
        let pick = original
            .lines()
            .find(|line| line.starts_with("pick "))
            .or_abort("real first retry pick");
        let changed = original.replacen(&format!("{pick}\n"), &format!("{pick}\n{pick}\n"), 1);
        assert_ne!(changed, original);
        let done =
            fs::read(native.join("done")).or_abort("real failed-pick done before external edit");
        fs::write(&path, &changed).or_abort("one extra last-done pick at pending front");
        assert_ne!(repository.invoke(&["--continue"]).0, EXIT_OK);
        assert!(
            repository
                .output
                .stderr
                .borrow()
                .contains("native replay omitted, repeated, or reordered an original source slot")
        );
        assert_eq!(repository.journal(), journal);
        assert_eq!(repository.index(), index);
        assert_eq!(repository.git(&["rev-parse", "HEAD"]), head);
        assert_eq!(repository.git(&["show-ref"]), refs);
        assert_eq!(
            fs::read(native.join("done")).or_abort("duplicate refusal done conserved"),
            done
        );
        assert_eq!(
            fs::read_to_string(&path).or_abort("duplicate external input conserved"),
            changed
        );
        assert_eq!(
            fs::read(repository.environment.cwd.join("later"))
                .or_abort("duplicate refusal user bytes conserved"),
            obstruction.as_bytes()
        );
        fs::write(&path, &original).or_abort("restore only owned duplicate edit");
    }
    if direct_native_second_failure {
        let failed = Command::new("git")
            .args(["rebase", "--continue"])
            .env("GIT_EDITOR", "true")
            .current_dir(&repository.environment.cwd)
            .output()
            .or_abort("actual direct native retry");
        assert!(!failed.status.success());
        assert!(String::from_utf8_lossy(&failed.stderr).contains("untracked working tree files"));
    } else {
        let done = fs::read(native.join("done")).or_abort("pre-admission actual done");
        let todo = fs::read(native.join("git-rebase-todo")).or_abort("pre-admission actual todo");
        let refusal = repository.invoke(&["--continue"]);
        assert_ne!(refusal.0, EXIT_OK);
        assert!(refusal.1.is_empty());
        assert!(
            repository
                .output
                .stderr
                .borrow()
                .contains("checkpoint would overwrite unrelated untracked work")
        );
        assert_eq!(
            fs::read(native.join("done")).or_abort("no native launch on obstruction"),
            done
        );
        assert_eq!(
            fs::read(native.join("git-rebase-todo")).or_abort("pending failed pick conserved"),
            todo
        );
    }
    assert_eq!(repository.journal(), journal);
    assert_eq!(repository.index(), index);
    assert_eq!(repository.git(&["rev-parse", "HEAD"]), head);
    assert_eq!(repository.git(&["show-ref"]), refs);
    assert_eq!(
        fs::read(repository.environment.cwd.join("later")).or_abort("owned obstruction preserved"),
        obstruction.as_bytes()
    );
    let twice = last_pick(&native.join("done"));
    if direct_native_second_failure {
        assert_eq!(
            twice.len(),
            before
                .len()
                .checked_add(1)
                .or_abort("next native failed pick attempt")
        );
        assert_eq!(
            resolve(twice.last().or_abort("second failed source slot")),
            second
        );
        assert_eq!(
            resolve(
                twice
                    .iter()
                    .rev()
                    .nth(1)
                    .or_abort("first failed source slot")
            ),
            second
        );
    } else {
        assert_eq!(twice, before);
    }
    let proofs = fs::read_to_string(&gates).or_abort("observed first-prefix proof");
    assert_eq!(proofs.lines().filter(|tree| *tree == final_tree).count(), 0);
    let saved = repository.directory.path().join("preserved-user-input");
    fs::rename(repository.environment.cwd.join("later"), &saved)
        .or_abort("move only known fixture-owned input");
    let atom: String = serde_json::from_slice::<serde_json::Value>(&journal)
        .or_abort("accepted first-prefix journal")
        .pointer("/state/atom")
        .and_then(serde_json::Value::as_str)
        .or_abort("immutable atom")
        .to_owned();
    repository.success(&["--continue"]);
    assert_eq!(
        repository.git(&["rev-parse", "refs/heads/main^{tree}"]),
        final_tree
    );
    assert_eq!(
        repository.git(&["rev-list", "--count", &format!("{atom}..refs/heads/main")]),
        "3"
    );
    assert_eq!(
        fs::read_to_string(&gates)
            .or_abort("observed retried-prefix proof")
            .lines()
            .filter(|tree| *tree == final_tree)
            .count(),
        1
    );
    repository.success(&["--finish"]);
    assert_eq!(repository.git(&["rev-parse", "HEAD^{tree}"]), final_tree);
    assert_eq!(
        fs::read(&saved).or_abort("relocated user input conserved"),
        obstruction.as_bytes()
    );
    assert_eq!(
        fs::read(repository.environment.cwd.join("later"))
            .or_abort("original tracked later path restored"),
        body.as_bytes()
    );
}

/// Native failed picks retry their original slot, including more than one failure.
pub(in crate::git_factor::engine::tests) fn failed_pick_retry(
    direct_native_second_failure: bool,
    body: &str,
    obstruction: &str,
) {
    failed_pick_control(direct_native_second_failure, false, body, obstruction);
}

pub(in crate::git_factor::engine::tests) fn final_descendant_bypass_refusal(body: &str) {
    bypass_control(BypassBoundary::FinalDescendant, body);
}

pub(in crate::git_factor::engine::tests) fn future_retry_refusal(exec: bool, body: &str) {
    grammar_edit_refusal(
        if exec {
            GrammarEdit::FutureExec
        } else {
            GrammarEdit::FuturePick
        },
        body,
    );
}

fn grammar_edit_refusal(edit: GrammarEdit, body: &str) {
    let repository = owned_repository();
    repository.write("base", "base\n");
    repository.commit("Base");
    repository.write("atom", "atom\n");
    repository.write("remainder", "remainder\n");
    let selected = repository.commit("Selected source");
    repository.write("descendant", body);
    repository.commit("Descendant");
    repository.success(&["--exec", "true", &selected]);
    repository.git(&["add", "atom"]);
    let runner = BeforeNativeContinue {
        refused: Cell::new(false),
    };
    assert_ne!(
        repository
            .invoke_with(&["--message", "Extract atom"], &runner, &REAL_FS)
            .0,
        EXIT_OK
    );
    assert!(runner.refused.get());
    repository.write("unrelated", "preserved unrelated bytes\n");
    let native = repository.environment.cwd.join(".git/rebase-merge");
    let todo_path = native.join("git-rebase-todo");
    let original = fs::read_to_string(&todo_path).or_abort("genuine pre-edit native todo");
    let changed = match edit {
        GrammarEdit::ForeignExec => {
            original.replacen("checkpoint-gate-remainder", "checkpoint-foreign-input", 1)
        }
        GrammarEdit::MissingPick => {
            let picked = original
                .lines()
                .find(|line| line.starts_with("pick "))
                .or_abort("real descendant pick");
            original.replacen(&format!("{picked}\n"), "", 1)
        }
        GrammarEdit::FutureExec | GrammarEdit::FuturePick => {
            let prefix = if matches!(edit, GrammarEdit::FutureExec) {
                "exec "
            } else {
                "pick "
            };
            let action = original
                .lines()
                .find(|line| line.starts_with(prefix))
                .or_abort("real future action");
            original.replacen(&format!("{action}\n"), &format!("{action}\n{action}\n"), 1)
        }
    };
    assert_ne!(changed, original);
    fs::write(&todo_path, &changed).or_abort("one actual external todo edit");
    let journal = repository.journal();
    let index = repository.index();
    let head = repository.git(&["rev-parse", "HEAD"]);
    let refs = repository.git(&["show-ref"]);
    let done = fs::read(native.join("done")).or_abort("after-edit native done snapshot");
    let (code, output) = repository.invoke(&["--continue"]);
    assert_ne!(code, EXIT_OK);
    assert_eq!(output, "");
    let diagnostic = repository.output.stderr.borrow();
    assert!(
        diagnostic.contains(if matches!(edit, GrammarEdit::ForeignExec) {
            "native replay contains a foreign exec payload"
        } else {
            "native replay omitted, repeated, or reordered an original source slot"
        }),
        "refusal must belong to the parser, not an unrelated earlier failure: {diagnostic}"
    );
    assert_eq!(repository.journal(), journal);
    assert_eq!(repository.index(), index);
    assert_eq!(repository.git(&["rev-parse", "HEAD"]), head);
    assert_eq!(repository.git(&["show-ref"]), refs);
    assert_eq!(
        fs::read_to_string(&todo_path).or_abort("external todo conserved"),
        changed
    );
    assert_eq!(
        fs::read(native.join("done")).or_abort("native done conserved"),
        done
    );
    assert!(!repository.environment.cwd.join("descendant").exists());
    for (path, expected) in [
        ("base", b"base\n".as_slice()),
        ("atom", b"atom\n".as_slice()),
        ("remainder", b"remainder\n".as_slice()),
    ] {
        assert_eq!(
            fs::read(repository.environment.cwd.join(path))
                .or_abort("physical tracked bytes preserved"),
            expected
        );
    }
    assert_eq!(
        fs::read(repository.environment.cwd.join("unrelated"))
            .or_abort("unrelated bytes preserved"),
        b"preserved unrelated bytes\n"
    );
}

/// A one-line external edit of a genuine native todo is the adversarial input.
pub(in crate::git_factor::engine::tests) fn grammar_refusal(foreign_exec: bool, body: &str) {
    grammar_edit_refusal(
        if foreign_exec {
            GrammarEdit::ForeignExec
        } else {
            GrammarEdit::MissingPick
        },
        body,
    );
}

/// An ignored future descendant input must survive factor's pre-native replay refusal.
pub(in crate::git_factor::engine::tests) fn ignored_replay_obstruction(body: &str) {
    let repository = owned_repository();
    repository.write("base", "base\n");
    repository.commit("Base");
    repository.write("atom", "atom\n");
    repository.write("second_atom", "second atom\n");
    repository.write("remainder", "remainder\n");
    let selected = repository.commit("Selected source");
    repository.write("later", "original descendant bytes\n");
    repository.commit("Real descendant");
    let final_tree = repository.git(&["rev-parse", "HEAD^{tree}"]);
    repository.success(&["--gate", "tests", "true", &selected]);
    repository.git(&["add", "atom"]);
    repository.success(&["--message", "First captured atom"]);
    let earlier = repository.git(&[
        "log",
        "--format=%H",
        "--grep=^First captured atom$",
        "refs/heads/main",
    ]);
    assert!(!earlier.is_empty());
    let hooks = repository.git(&["config", "--get", "core.hooksPath"]);
    let hook = Path::new(&hooks).join("commit-msg");
    fs::write(
        &hook,
        "#!/bin/bash\nset -eu\nif grep -q '^Selected source$' \"$1\"; then exit 23; fi\n",
    )
    .or_abort("owned remainder message refusal");
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o700))
        .or_abort("owned remainder hook executable");
    repository.git(&["add", "second_atom"]);
    assert_ne!(
        repository.invoke(&["--message", "Second captured atom"]).0,
        EXIT_OK
    );
    fs::remove_file(&hook).or_abort("remove only owned remainder refusal hook");
    let admin =
        fs::canonicalize(repository.environment.cwd.join(".git")).or_abort("ignored replay admin");
    fs::write(admin.join("info/exclude"), "later\n").or_abort("owned ignored input configuration");
    repository.write("later", body);
    let journal = repository.journal();
    let index = repository.index();
    let head = repository.git(&["rev-parse", "HEAD"]);
    let refs = repository.git(&["show-ref"]);
    let native = admin.join("rebase-merge");
    let done = fs::read(native.join("done")).or_abort("actual paused remainder done");
    let todo = fs::read(native.join("git-rebase-todo")).or_abort("actual pending descendant todo");
    assert!(String::from_utf8_lossy(&todo).contains("checkpoint-gate-remainder"));
    let refused = repository.invoke(&["--continue"]);
    assert_ne!(refused.0, EXIT_OK);
    assert!(refused.1.is_empty());
    assert!(
        repository
            .output
            .stderr
            .borrow()
            .contains("checkpoint would overwrite unrelated untracked work")
    );
    assert_eq!(repository.journal(), journal);
    assert_eq!(repository.index(), index);
    assert_eq!(repository.git(&["rev-parse", "HEAD"]), head);
    assert_eq!(repository.git(&["show-ref"]), refs);
    assert_eq!(
        fs::read(native.join("done")).or_abort("pre-native done conservation"),
        done
    );
    assert_eq!(
        fs::read(native.join("git-rebase-todo")).or_abort("pre-native todo conservation"),
        todo
    );
    assert_eq!(
        fs::read(repository.environment.cwd.join("later"))
            .or_abort("ignored input bytes conserved"),
        body.as_bytes()
    );
    assert_eq!(
        repository.git(&["merge-base", &earlier, "refs/heads/main"]),
        earlier
    );
    let saved = repository.directory.path().join("preserved-ignored-input");
    fs::rename(repository.environment.cwd.join("later"), &saved)
        .or_abort("move only own ignored input");
    repository.success(&["--continue"]);
    repository.success(&["--finish"]);
    assert_eq!(repository.git(&["rev-parse", "HEAD^{tree}"]), final_tree);
    assert_eq!(repository.git(&["merge-base", &earlier, "HEAD"]), earlier);
    assert_eq!(
        fs::read(saved).or_abort("relocated ignored input preserved"),
        body.as_bytes()
    );
}

fn install_stimulus(repository: &Repository, configuration: &ChildStimulus) {
    let path = repository.directory.path().join("e1-stimulus.json");
    fs::write(
        &path,
        serde_json::to_vec(configuration).or_abort("test stimulus serialization"),
    )
    .or_abort("fixture-owned stimulus config");
    let old =
        fs::read_to_string(&repository.environment.executable).or_abort("owned launcher script");
    let assignment = format!(
        "export FACTOR_TEST_E1_STIMULUS={}\n",
        shell_quote(path.to_str().or_abort("stimulus path UTF-8"))
    );
    let changed = old.replacen("set -eu\n", &format!("set -eu\n{assignment}"), 1);
    assert_ne!(changed, old);
    fs::write(&repository.environment.executable, changed).or_abort("owned configured launcher");
}

#[expect(
    clippy::panic,
    reason = "fixture authority mismatches must terminate the test before any native process can be targeted"
)]
fn kill_owned_native(configuration: &ChildStimulus, expected_arguments: &[&str]) {
    let path = configuration.evidence.join("native-ingress.json");
    let attempts: Range<usize> = 0..100;
    for _attempt in attempts {
        if path.is_file() {
            break;
        }
        thread::sleep(Duration::from_millis(5));
    }
    let admitted: NativeIngress =
        serde_json::from_slice(&fs::read(&path).or_abort("own native launch PID proof"))
            .or_abort("native launch proof grammar");
    assert_ne!(admitted.pid, 0);
    assert_ne!(admitted.pid, process::id());
    assert_eq!(admitted.admin, configuration.admin);
    assert_eq!(
        fs::canonicalize(admitted.cwd.join(".git")).or_abort("admitted fixture cwd"),
        configuration.admin
    );
    assert_eq!(admitted.arguments, expected_arguments);
    let mut pid = process::id();
    let mut observed = String::new();
    let depths: Range<usize> = 0..12;
    for _depth in depths {
        let result = Command::new("ps")
            .args([
                "-ww",
                "-p",
                &pid.to_string(),
                "-o",
                "ppid=",
                "-o",
                "command=",
            ])
            .output()
            .or_abort("full owned parent ancestry");
        assert!(result.status.success());
        let line = String::from_utf8(result.stdout).or_abort("ancestor command UTF-8");
        let (parent, command) = line
            .trim()
            .split_once(char::is_whitespace)
            .or_abort("ancestor parent and command");
        writeln!(observed, "{pid} {}", line.trim()).or_abort("record owned native ancestry");
        let native = command.split_whitespace().collect::<Vec<_>>();
        let first_native = native
            .first()
            .and_then(|value| Path::new(value).file_name())
            == Some(OsStr::new("git"));
        if first_native {
            assert_eq!(
                pid, admitted.pid,
                "refuse the first unrelated native Git ancestor"
            );
            assert_eq!(
                parent.parse::<u32>().or_abort("native owner parent"),
                admitted.parent
            );
            assert_eq!(native.get(1..), Some(expected_arguments));
            assert_ne!(pid, process::id());
            fs::write(configuration.evidence.join("native-ancestry"), observed)
                .or_abort("verified native ancestry evidence");
            fs::write(configuration.evidence.join("native-pid"), pid.to_string())
                .or_abort("exact native victim evidence");
            let killed = Command::new("kill")
                .args(["-KILL", &pid.to_string()])
                .status()
                .or_abort("kill only owned verified native Git");
            assert!(killed.success());
            fs::write(configuration.evidence.join("kill-status"), "0\n")
                .or_abort("actual kill success");
            return;
        }
        pid = parent
            .parse::<NonZeroU32>()
            .or_abort("positive parent PID")
            .get();
    }
    fs::write(configuration.evidence.join("refused-ancestry"), observed)
        .or_abort("refused ancestry evidence");
    panic!("refuse interruption: exact native rebase ancestor was not established");
}

pub(in crate::git_factor::engine::tests) fn last_done_pick_duplicate_refusal(
    body: &str,
    obstruction: &str,
) {
    failed_pick_control(false, true, body, obstruction);
}

/// A native message hook may not hide main tracked edits behind new sparse config.
#[expect(
    clippy::create_dir,
    reason = "exclusive fixture directories must refuse existing occupancy rather than silently admit unrelated files"
)]
#[expect(
    clippy::too_many_lines,
    reason = "one decisive native lifecycle keeps its exact interruption evidence and full preservation oracle together"
)]
pub(in crate::git_factor::engine::tests) fn observed_sparse_hook_refusal(body: &str) {
    let repository = owned_repository();
    repository.write("rootfile", "original root bytes\n");
    fs::create_dir(repository.environment.cwd.join("nested"))
        .or_abort("owned nested fixture directory");
    repository.write("nested/base", "nested base\n");
    repository.commit("Base");
    repository.write("atom", "atom\n");
    let selected = repository.commit("Selected source");
    repository.write("nested/descendant", "descendant\n");
    let descendant = repository.commit("Sparse observed descendant");
    repository.success(&["--gate", "tests", "true", &selected]);
    repository.git(&["add", "atom"]);
    let root = fs::canonicalize(&repository.environment.cwd).or_abort("canonical sparse main");
    let admin = fs::canonicalize(root.join(".git")).or_abort("canonical sparse admin");
    let evidence = repository
        .directory
        .path()
        .join("sparse-consumed-descendant");
    fs::create_dir(&evidence).or_abort("owned consumed callback evidence");
    let configuration = ChildStimulus {
        admin: admin.clone(),
        launcher: fs::canonicalize(&repository.environment.executable)
            .or_abort("owned sparse callback launcher"),
        checkpoint: repository.git(&["rev-parse", "refs/heads/main"]),
        original_pick: descendant,
        callback: "checkpoint-gate-descendant".to_owned(),
        accepted_count: 0,
        boundary: ChildBoundary::BeforeCallbackKill,
        evidence,
    };
    install_stimulus(&repository, &configuration);
    let runner = OwnedNativeContinue {
        configuration: &configuration,
    };
    assert_ne!(
        repository
            .invoke_with(&["--message", "Extract atom"], &runner, &REAL_FS)
            .0,
        EXIT_OK
    );
    assert!(
        configuration.evidence.join("fired").is_file(),
        "actual consumed descendant interruption must fire before verification"
    );
    assert_eq!(
        fs::read(configuration.evidence.join("boundary")).or_abort("actual interruption boundary"),
        b"before-callback"
    );
    assert_eq!(await_kill_status(&configuration.evidence), "0\n");
    assert_eq!(
        fs::read(configuration.evidence.join("done"))
            .or_abort("genuine consumed callback transcript"),
        fs::read(admin.join("rebase-merge/done")).or_abort("live consumed callback done")
    );
    let hooks = repository.git(&["config", "--get", "core.hooksPath"]);
    let hook = Path::new(&hooks).join("commit-msg");
    let payload = repository.directory.path().join("hook-root-bytes");
    let changed_bytes = format!("native hook edit\n{body}");
    fs::write(&payload, &changed_bytes).or_abort("owned hook payload");
    let marker = repository.directory.path().join("hook-main-mutated");
    let rootfile = root.join("rootfile");
    let pattern = admin.join("info/sparse-checkout");
    fs::write(&hook, format!(
        "#!/bin/bash\nset -eu\nif test ! -e {marker}; then\n cp {payload} {rootfile}\n printf 'nested/\\n' > {pattern}\n env -u GIT_DIR -u GIT_COMMON_DIR -u GIT_INDEX_FILE -u GIT_WORK_TREE git -C {root} config core.sparseCheckout true\n touch {marker}\nfi\n",
        marker=shell_quote(marker.to_str().or_abort("owned marker path")),
        payload=shell_quote(payload.to_str().or_abort("owned payload path")),
        rootfile=shell_quote(rootfile.to_str().or_abort("main root file path")),
        pattern=shell_quote(pattern.to_str().or_abort("main sparse pattern path")),
        root=shell_quote(root.to_str().or_abort("main root path")),
    )).or_abort("owned main mutation hook");
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o700))
        .or_abort("owned sparse mutation hook executable");
    let native = admin.join("rebase-merge");
    let journal = repository.journal();
    let index = repository.index();
    let head = repository.git(&["rev-parse", "HEAD"]);
    let branches = repository.git(&[
        "for-each-ref",
        "--format=%(refname) %(objectname)",
        "refs/heads",
        "refs/tags",
    ]);
    let lease = repository.git(&["rev-parse", "refs/factor/session-lease"]);
    let done = fs::read(native.join("done")).or_abort("actual paused descendant done");
    let todo =
        fs::read(native.join("git-rebase-todo")).or_abort("actual retained descendant callback");
    let result = repository.invoke(&["--continue"]);
    assert_ne!(result.0, EXIT_OK);
    assert!(result.1.is_empty());
    assert!(
        marker.is_file(),
        "native hook must reach the actual actor boundary"
    );
    assert!(
        repository
            .output
            .stderr
            .borrow()
            .contains("tree hash mismatch"),
        "main physical post-admission must reject: {}",
        repository.output.stderr.borrow()
    );
    assert_eq!(repository.journal(), journal);
    assert_eq!(repository.index(), index);
    assert_eq!(repository.git(&["rev-parse", "HEAD"]), head);
    assert_eq!(
        repository.git(&[
            "for-each-ref",
            "--format=%(refname) %(objectname)",
            "refs/heads",
            "refs/tags"
        ]),
        branches
    );
    assert_eq!(
        repository.git(&["rev-parse", "refs/factor/session-lease"]),
        lease
    );
    assert_eq!(
        fs::read(native.join("done")).or_abort("done unchanged"),
        done
    );
    assert_eq!(
        fs::read(native.join("git-rebase-todo")).or_abort("todo unchanged"),
        todo
    );
    assert_eq!(
        fs::read(rootfile).or_abort("hook main bytes preserved"),
        changed_bytes.as_bytes()
    );
    assert_eq!(
        fs::read(root.join("nested/base")).or_abort("unaffected main bytes"),
        b"nested base\n"
    );
    assert_eq!(
        repository.git(&["config", "--get", "core.sparseCheckout"]),
        "true"
    );
}

pub(in crate::git_factor::engine::tests) fn opening_advanced_refusal(body: &str) {
    ready_break_control(true, body);
}

/// Opening will expose a selection, so deleted recreation must refuse before baseline gates.
pub(in crate::git_factor::engine::tests) fn opening_deleted_recreation_refusal(body: &str) {
    let repository = owned_repository();
    repository.write("base", "base\n");
    repository.write("deleted", "original tracked bytes\n");
    repository.commit("Base");
    fs::remove_file(repository.environment.cwd.join("deleted")).or_abort("owned selected deletion");
    repository.write("atom", "atom\n");
    let selected = repository.commit("Selected deletion");
    repository.write("descendant", "descendant\n");
    repository.commit("Real descendant");
    let admin = fs::canonicalize(repository.environment.cwd.join(".git"))
        .or_abort("owned baseline native admin");
    let runner = BeforeBaselineCandidate {
        admin: admin.clone(),
        refused: Cell::new(false),
    };
    let proof = repository.directory.path().join("baseline-tree-gate");
    let gate = format!(
        "printf 'passed\\n' >> {}",
        shell_quote(proof.to_str().or_abort("proof path"))
    );
    assert_ne!(
        repository
            .invoke_with(&["--gate", "tests", &gate, &selected], &runner, &REAL_FS)
            .0,
        EXIT_OK
    );
    assert!(
        runner.refused.get(),
        "actual baseline candidate construction must be interrupted"
    );
    assert!(!proof.exists(), "baseline gate has not yet run");
    let native = admin.join("rebase-merge");
    let done = fs::read(native.join("done")).or_abort("actual owned break done");
    assert_eq!(String::from_utf8_lossy(&done).lines().last(), Some("break"));
    let journal = repository.journal();
    let admitted: serde_json::Value =
        serde_json::from_slice(&journal).or_abort("real Opening journal");
    assert_eq!(
        admitted
            .pointer("/state/phase")
            .and_then(serde_json::Value::as_str),
        Some("opening")
    );
    let source = admitted
        .pointer("/state/source")
        .and_then(serde_json::Value::as_str)
        .or_abort("owned source SHA");
    let head = repository.git(&["rev-parse", "HEAD"]);
    assert_eq!(
        repository.git(&["rev-parse", "HEAD^{tree}"]),
        repository.git(&["rev-parse", &format!("{source}^{{tree}}")])
    );
    fs::write(admin.join("info/exclude"), "deleted\n").or_abort("ignored recreation config");
    repository.write("deleted", body);
    let index = repository.index();
    let references = repository.git(&["show-ref"]);
    let todo = fs::read(native.join("git-rebase-todo")).or_abort("pending original native actions");
    let result = repository.invoke(&["--continue"]);
    assert_ne!(result.0, EXIT_OK);
    assert!(result.1.is_empty());
    assert!(
        repository
            .output
            .stderr
            .borrow()
            .contains("a deleted selection path has been recreated"),
        "selection pre-admission must own refusal: {}",
        repository.output.stderr.borrow()
    );
    assert_eq!(repository.journal(), journal);
    assert_eq!(repository.index(), index);
    assert_eq!(repository.git(&["rev-parse", "HEAD"]), head);
    assert_eq!(repository.git(&["show-ref"]), references);
    assert_eq!(
        fs::read(native.join("done")).or_abort("done preserved"),
        done
    );
    assert_eq!(
        fs::read(native.join("git-rebase-todo")).or_abort("todo preserved"),
        todo
    );
    assert_eq!(
        fs::read(repository.environment.cwd.join("deleted")).or_abort("recreation preserved"),
        body.as_bytes()
    );
    assert_eq!(
        fs::read(repository.environment.cwd.join("base")).or_abort("base frame preserved"),
        b"base\n"
    );
    assert_eq!(
        fs::read(repository.environment.cwd.join("atom")).or_abort("atom preserved"),
        b"atom\n"
    );
    assert!(
        !proof.exists(),
        "refused selection must never reach baseline gates"
    );
}

pub(in crate::git_factor::engine::tests) fn opening_foreign_head_refusal(
    body: &str,
    obstruction: &str,
) {
    opening_root_control(OpeningControl::ForeignHead, body, obstruction);
}

pub(in crate::git_factor::engine::tests) fn opening_nonroot_retry_abort(
    body: &str,
    obstruction: &str,
) {
    opening_source_control(
        OpeningOrigin::NonRoot,
        OpeningControl::Abort,
        body,
        obstruction,
    );
}

/// The tool's controlled range rewrite must not inherit Git's interactive missing-pick policy.
pub(in crate::git_factor::engine::tests) fn opening_with_missing_commit_check(range: bool) {
    let (repository, base, selected) = opening_pool_repository(range);
    repository.git(&["config", "rebase.missingCommitsCheck", "error"]);
    let final_tree = repository.git(&["rev-parse", "HEAD^{tree}"]);
    let references = repository.git(&["show-ref"]);
    let configuration =
        fs::read(repository.environment.cwd.join(".git/config")).or_abort("persistent policy");
    let argument = if range {
        format!("{base}..{selected}")
    } else {
        selected.clone()
    };

    let (code, stdout) = repository.invoke(&["--gate", "tests", "true", &argument]);

    let raw_index = repository.index();
    let journal: serde_json::Value =
        serde_json::from_slice(&repository.journal()).or_abort("actual opening journal");
    let result: serde_json::Value =
        serde_json::from_str(&stdout).or_abort("actual normalized start result");
    let phase = journal
        .pointer("/state/phase")
        .and_then(serde_json::Value::as_str);
    let done_exists = repository
        .environment
        .cwd
        .join(".git/rebase-merge/done")
        .is_file();
    assert_eq!(
        (code, phase, done_exists),
        (EXIT_OK, Some("selecting"), true)
    );
    assert_eq!(
        result
            .pointer("/operation")
            .and_then(serde_json::Value::as_str),
        Some("start")
    );
    assert_eq!(
        result
            .pointer("/target/commit")
            .and_then(serde_json::Value::as_str),
        Some(selected.as_str())
    );
    let commit_count: u64 = if range { 2 } else { 1 };
    assert_eq!(
        result
            .pointer("/target/commit_count")
            .and_then(serde_json::Value::as_u64),
        Some(commit_count)
    );
    assert!(!raw_index.is_empty());
    assert_eq!(repository.git(&["rev-parse", "refs/heads/main"]), selected);
    assert_eq!(
        repository.git(&["rev-parse", "refs/heads/main^{tree}"]),
        final_tree
    );
    assert_eq!(
        repository.git(&["config", "rebase.missingCommitsCheck"]),
        "error"
    );
    assert_eq!(
        fs::read(repository.environment.cwd.join(".git/config")).or_abort("policy unchanged"),
        configuration
    );
    assert_eq!(
        fs::read(repository.environment.cwd.join("atom")).or_abort("selected atom"),
        b"atom\n"
    );
    assert_eq!(
        fs::read(repository.environment.cwd.join("remainder")).or_abort("selected remainder"),
        b"pool remainder\n"
    );
    assert_eq!(
        fs::read(repository.environment.cwd.join("user")).or_abort("protected user"),
        b"unrelated user bytes\n"
    );

    let proof_ref = "refs/factor/gates/f32a5804e292d30bedf68f62d32fb75d87e99fd9/e8601b6f10e929585fe9c3f99df6f549acd0f358";
    let proof = repository.git(&["rev-parse", proof_ref]);
    assert_eq!(
        repository.git(&["rev-parse", &format!("{proof_ref}^{{tree}}")]),
        final_tree
    );
    let completed_refs = format!("{proof} {proof_ref}\n{references}");
    let lease = journal
        .pointer("/state/lease")
        .and_then(serde_json::Value::as_str)
        .or_abort("current owned lease");
    let active_refs =
        format!("{proof} {proof_ref}\n{lease} refs/factor/session-lease\n{references}");
    assert_eq!(repository.git(&["show-ref"]), active_refs);

    repository.success(&["--abort"]);

    assert_eq!(repository.git(&["show-ref"]), completed_refs);
    assert_eq!(repository.git(&["rev-parse", "HEAD^{tree}"]), final_tree);
    assert_eq!(
        repository.git(&["config", "rebase.missingCommitsCheck"]),
        "error"
    );
    assert_eq!(
        fs::read(repository.environment.cwd.join("user")).or_abort("user after abort"),
        b"unrelated user bytes\n"
    );
}

fn opening_pool_repository(range: bool) -> (Repository, String, String) {
    let repository = owned_repository();
    repository.write("base", "base\n");
    let base = repository.commit("Base");
    repository.write("atom", "atom\n");
    if range {
        repository.commit("Add atom source");
    }
    repository.write("remainder", "pool remainder\n");
    let selected = repository.commit("Selected source");
    repository.git(&["branch", "foreign", &selected]);
    repository.git(&["tag", "foreign", &selected]);
    repository.write("user", "unrelated user bytes\n");
    (repository, base, selected)
}

fn opening_abort_frame(repository: &Repository) -> OpeningAbortFrame {
    // Physical index bytes precede every observing Git query.
    let index = repository.index();
    let root = &repository.environment.cwd;
    OpeningAbortFrame {
        atom: fs::read(root.join("atom")).or_abort("selected atom bytes"),
        base: fs::read(root.join("base")).or_abort("base bytes"),
        config: fs::read(root.join(".git/config")).or_abort("native config"),
        head: repository.git(&["rev-parse", "HEAD"]),
        index,
        journal: fs::read(root.join(".git/factor-journal.json")).ok(),
        native: root
            .join(".git/rebase-merge")
            .is_dir()
            .then(|| log_observation_inventory(&root.join(".git/rebase-merge"))),
        refs: repository.git(&["show-ref"]),
        remainder: fs::read(root.join("remainder")).or_abort("pool bytes"),
        user: fs::read(root.join("user")).or_abort("unrelated user bytes"),
    }
}

/// A real mixed-reset Opening must protect selected-path edits just like Selecting.
pub(in crate::git_factor::engine::tests) fn opening_ready_break_abort(modified: bool) {
    let (repository, _, selected) = opening_pool_repository(false);
    let final_tree = repository.git(&["rev-parse", "HEAD^{tree}"]);
    let failure = ReadyBreakPublication {
        admin: fs::canonicalize(repository.environment.cwd.join(".git")).or_abort("actual admin"),
        refused: Cell::new(false),
    };
    let (start_code, _) =
        repository.invoke_with(&["--gate", "tests", "true"], &REAL_RUNNER, &failure);
    assert_ne!(start_code, EXIT_OK);
    assert!(failure.refused.get());
    let journal: serde_json::Value =
        serde_json::from_slice(&repository.journal()).or_abort("actual interrupted Opening");
    assert_eq!(
        journal
            .pointer("/state/phase")
            .and_then(serde_json::Value::as_str),
        Some("opening")
    );
    assert_eq!(
        journal
            .pointer("/state/head")
            .and_then(serde_json::Value::as_str),
        Some(repository.git(&["rev-parse", "HEAD"]).as_str())
    );
    if modified {
        repository.write("atom", "user changed selected bytes\n");
    }
    let before = opening_abort_frame(&repository);

    let (code, stdout) = repository.invoke(&["--abort"]);

    if modified {
        // The immediate physical frame must survive the refused public Abort.
        let after = opening_abort_frame(&repository);
        assert_eq!(after, before);
        assert_eq!(code, EXIT_TEMPFAIL);
        assert_eq!(stdout, "");
        assert_eq!(
            *repository.output.stderr.borrow(),
            "tree hash mismatch: expected e8601b6f10e929585fe9c3f99df6f549acd0f358, got 3c81b826bbd60a0d9471fa228f69a86ec9026ab2\n"
        );
    } else {
        let index = repository.index();
        assert_eq!(code, EXIT_OK);
        assert_eq!(
            stdout,
            "{\"operation\":\"abort\",\"rebase\":{\"in_progress\":false},\"actions\":{}}\n"
        );
        assert_eq!(*repository.output.stderr.borrow(), "");
        assert!(!index.is_empty());
        assert_eq!(repository.git(&["rev-parse", "HEAD"]), selected);
        assert_eq!(repository.git(&["rev-parse", "HEAD^{tree}"]), final_tree);
        assert_eq!(repository.git(&["symbolic-ref", "HEAD"]), "refs/heads/main");
        assert_eq!(
            repository.git(&["show-ref"]),
            before
                .refs
                .lines()
                .filter(|line| !line.contains(" refs/factor/session-lease"))
                .collect::<Vec<_>>()
                .join("\n")
        );
        assert!(!failure.admin.join("factor-journal.json").exists());
        assert!(!failure.admin.join("rebase-merge").exists());
        assert_eq!(
            fs::read(repository.environment.cwd.join("atom")).or_abort("restored atom"),
            b"atom\n"
        );
        assert_eq!(
            fs::read(repository.environment.cwd.join("remainder")).or_abort("restored remainder"),
            before.remainder
        );
        assert_eq!(
            fs::read(repository.environment.cwd.join("user")).or_abort("unrelated bytes"),
            before.user
        );
    }
}

pub(in crate::git_factor::engine::tests) fn opening_ready_break_control(body: &str) {
    ready_break_control(false, body);
}

fn opening_root_control(control: OpeningControl, body: &str, obstruction: &str) {
    opening_source_control(OpeningOrigin::Root, control, body, obstruction);
}

/// The initial root pick actually fails; factor retry readmits before invoking Git.
pub(in crate::git_factor::engine::tests) fn opening_root_retry(body: &str, obstruction: &str) {
    opening_root_control(OpeningControl::Retry, body, obstruction);
}

pub(in crate::git_factor::engine::tests) fn opening_root_retry_abort(
    body: &str,
    obstruction: &str,
) {
    opening_root_control(OpeningControl::Abort, body, obstruction);
}

#[expect(
    clippy::cognitive_complexity,
    reason = "the closed fixture boundary matrix keeps before/after native interruption and preservation assertions explicit"
)]
#[expect(
    clippy::create_dir,
    reason = "exclusive fixture directories must refuse existing occupancy rather than silently admit unrelated files"
)]
#[expect(
    clippy::pattern_type_mismatch,
    reason = "borrowed fixture facts retain ownership without cloning or a second representation"
)]
#[expect(
    clippy::too_many_lines,
    reason = "one decisive native lifecycle keeps its exact interruption evidence and full preservation oracle together"
)]
fn opening_source_control(
    origin: OpeningOrigin,
    control: OpeningControl,
    body: &str,
    obstruction: &str,
) {
    let mut repository = owned_repository();
    let base = match origin {
        OpeningOrigin::Root => None,
        OpeningOrigin::NonRoot => {
            repository.write("base", "base\n");
            Some(repository.commit("Base"))
        }
    };
    repository.write("atom", "atom\n");
    repository.write("remainder", body);
    repository.write("temporary", "selected canceled path\n");
    let selected = repository.commit("Root selected source");
    fs::remove_file(repository.environment.cwd.join("temporary"))
        .or_abort("actual canceled source path");
    repository.write("descendant", "descendant\n");
    repository.commit("Real descendant");
    let checkpoint = repository.git(&["rev-parse", "HEAD"]);
    let final_tree = repository.git(&["rev-parse", "HEAD^{tree}"]);
    let evidence = repository.directory.path().join("opening-editor-evidence");
    fs::create_dir(&evidence).or_abort("editor evidence");
    let root = fs::canonicalize(&repository.environment.cwd).or_abort("canonical editor root");
    let configuration = EditorObstruction {
        base: base.clone(),
        admin: fs::canonicalize(root.join(".git")).or_abort("canonical root admin"),
        launcher: fs::canonicalize(&repository.environment.executable)
            .or_abort("canonical root launcher"),
        checkpoint,
        path: root.join("temporary"),
        bytes: obstruction.to_owned(),
        evidence,
    };
    let config_path = repository.directory.path().join("editor-obstruction.json");
    fs::write(
        &config_path,
        serde_json::to_vec(&configuration).or_abort("editor serialization"),
    )
    .or_abort("editor config");
    let original =
        fs::read_to_string(&repository.environment.executable).or_abort("owned root launcher");
    let assignment = format!(
        "export FACTOR_TEST_E1_EDITOR_OBSTRUCTION={}\n",
        shell_quote(config_path.to_str().or_abort("editor config UTF-8"))
    );
    let changed = original.replacen("set -eu\n", &format!("set -eu\n{assignment}"), 1);
    assert_ne!(changed, original);
    fs::write(&repository.environment.executable, changed).or_abort("root editor owned launcher");
    let native_evidence = repository.directory.path().join("opening-native-stderr");
    fs::create_dir(&native_evidence).or_abort("fresh opening native stderr evidence");
    let native_arguments = [
        "-c",
        "rebase.missingCommitsCheck=ignore",
        "rebase",
        "--interactive",
        "--no-ff",
        "--reschedule-failed-exec",
        "--no-update-refs",
        "--no-autostash",
        "--no-autosquash",
        "--no-rebase-merges",
        "--empty=keep",
        "--keep-empty",
        base.as_deref().unwrap_or("--root"),
    ];
    let runner = OwnedNativeObservation {
        admin: &configuration.admin,
        evidence: &native_evidence,
        arguments: &native_arguments,
    };
    let (code, output) =
        repository.invoke_with(&["--gate", "tests", "true", &selected], &runner, &REAL_FS);
    assert_eq!(code, EXIT_TEMPFAIL);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&output)
            .or_abort("opening recovery JSON")
            .pointer("/operation")
            .and_then(serde_json::Value::as_str),
        Some("start")
    );
    let opening_ingress: NativeIngress = serde_json::from_slice(
        &fs::read(native_evidence.join("native-ingress.json"))
            .or_abort("owned native ingress bytes"),
    )
    .or_abort("owned native ingress record");
    assert_eq!(opening_ingress.arguments, native_arguments);
    assert_eq!(opening_ingress.admin, configuration.admin);
    assert_eq!(
        opening_ingress.cwd,
        fs::canonicalize(&repository.environment.cwd).or_abort("owned observed native cwd")
    );
    assert_eq!(opening_ingress.parent, process::id());
    assert_ne!(opening_ingress.pid, 0);
    assert_ne!(opening_ingress.pid, opening_ingress.parent);
    assert!(
        String::from_utf8_lossy(
            &fs::read(native_evidence.join("native-stderr")).or_abort("actual native stderr bytes")
        )
        .contains("untracked working tree files"),
        "actual native overwrite refusal must fire"
    );
    assert!(configuration.evidence.join("fired").is_file());
    let journal_bytes = repository.journal();
    let index = repository.index();
    let journal: serde_json::Value =
        serde_json::from_slice(&journal_bytes).or_abort("failed source Opening");
    assert_eq!(
        journal
            .pointer("/state/phase")
            .and_then(serde_json::Value::as_str),
        Some("opening")
    );
    let head = repository.git(&["rev-parse", "HEAD"]);
    let refs = repository.git(&["show-ref"]);
    let saved_head = journal
        .pointer("/state/head")
        .and_then(serde_json::Value::as_str);
    if let Some(parent) = &base {
        assert_eq!(head, *parent);
        assert_eq!(saved_head, Some(head.as_str()));
        assert_eq!(
            repository.git(&["rev-parse", "HEAD^{tree}"]),
            repository.git(&["rev-parse", &format!("{parent}^{{tree}}")])
        );
    } else {
        assert_ne!(saved_head, Some(head.as_str()));
        assert_eq!(repository.git(&["rev-parse", "HEAD^{tree}"]), EMPTY);
    }
    let native = configuration.admin.join("rebase-merge");
    let done = fs::read(native.join("done")).or_abort("genuine failed root pick done");
    let todo =
        fs::read(native.join("git-rebase-todo")).or_abort("genuine rescheduled root pick todo");
    let source = journal
        .pointer("/state/source")
        .and_then(serde_json::Value::as_str)
        .or_abort("bound source SHA");
    let done_text = from_utf8(&done).or_abort("native done UTF-8");
    let picks = done_text
        .lines()
        .filter(|line| !line.trim().is_empty() && !line.starts_with('#'))
        .collect::<Vec<_>>();
    assert_eq!(picks.len(), 1);
    let picked = picks
        .first()
        .or_abort("actual source pick")
        .split_whitespace()
        .nth(1)
        .or_abort("source token");
    assert_eq!(
        repository.git(&["rev-parse", "--verify", &format!("{picked}^{{commit}}")]),
        source
    );
    if matches!(control, OpeningControl::WrongPick) {
        let todo_text = from_utf8(&todo).or_abort("real source retry todo UTF-8");
        let own = todo_text
            .lines()
            .find(|line| line.starts_with("pick "))
            .or_abort("real pending source pick");
        let token = own
            .split_whitespace()
            .nth(1)
            .or_abort("original pending source token");
        let descendant = repository.git(&["rev-parse", "refs/heads/main"]);
        let changed_todo =
            todo_text.replacen(&format!("pick {token}"), &format!("pick {descendant}"), 1);
        assert_ne!(changed_todo, todo_text);
        fs::write(native.join("git-rebase-todo"), &changed_todo)
            .or_abort("one external wrong-source line edit");
        assert_ne!(repository.invoke(&["--continue"]).0, EXIT_OK);
        assert!(
            repository
                .output
                .stderr
                .borrow()
                .contains("native replay omitted, repeated, or reordered an original source slot")
        );
        assert_eq!(repository.journal(), journal_bytes);
        assert_eq!(repository.index(), index);
        assert_eq!(repository.git(&["rev-parse", "HEAD"]), head);
        assert_eq!(repository.git(&["show-ref"]), refs);
        assert_eq!(
            fs::read(native.join("done")).or_abort("wrong-pick done preserved"),
            done
        );
        assert_eq!(
            fs::read_to_string(native.join("git-rebase-todo"))
                .or_abort("external wrong pick preserved"),
            changed_todo
        );
        assert_eq!(
            fs::read(&configuration.path).or_abort("wrong-pick obstruction preserved"),
            obstruction.as_bytes()
        );
        fs::write(native.join("git-rebase-todo"), &todo)
            .or_abort("restore only fixture-owned edit");
    }
    if matches!(control, OpeningControl::ForeignHead) {
        let foreign = repository.git(&["commit-tree", EMPTY, "-m", "Foreign detached empty root"]);
        assert_ne!(foreign, head);
        repository.git(&["update-ref", "--no-deref", "HEAD", &foreign]);
        let foreign_refs = repository.git(&["show-ref"]);
        assert_ne!(repository.invoke(&["--continue"]).0, EXIT_OK);
        assert!(
            repository
                .output
                .stderr
                .borrow()
                .contains("opening source retry HEAD differs from native onto")
        );
        assert_eq!(repository.journal(), journal_bytes);
        assert_eq!(repository.index(), index);
        assert_eq!(repository.git(&["rev-parse", "HEAD"]), foreign);
        assert_eq!(repository.git(&["show-ref"]), foreign_refs);
        assert_eq!(
            fs::read(native.join("done")).or_abort("foreign HEAD done preserved"),
            done
        );
        assert_eq!(
            fs::read(native.join("git-rebase-todo"))
                .or_abort("foreign HEAD pending source preserved"),
            todo
        );
        assert_eq!(
            fs::read(&configuration.path).or_abort("foreign HEAD obstruction preserved"),
            obstruction.as_bytes()
        );
        repository.git(&["update-ref", "--no-deref", "HEAD", &head]);
    }
    if matches!(
        control,
        OpeningControl::CopiedAbort | OpeningControl::MovedAbort
    ) {
        relocate_executable(
            &mut repository,
            matches!(control, OpeningControl::MovedAbort),
        );
    }
    // Relocated abort is the sole Act in its new recovery witness.
    let attempts: Range<usize> = if matches!(
        control,
        OpeningControl::CopiedAbort | OpeningControl::MovedAbort
    ) {
        0..0
    } else {
        0..2
    };
    for _attempt in attempts {
        assert_ne!(repository.invoke(&["--continue"]).0, EXIT_OK);
        assert!(
            repository
                .output
                .stderr
                .borrow()
                .contains("checkpoint would overwrite unrelated untracked work"),
            "source obstruction must refuse in untracked admission: {}",
            repository.output.stderr.borrow()
        );
        assert_eq!(repository.journal(), journal_bytes);
        assert_eq!(repository.index(), index);
        assert_eq!(repository.git(&["rev-parse", "HEAD"]), head);
        assert_eq!(repository.git(&["show-ref"]), refs);
        assert_eq!(
            fs::read(native.join("done")).or_abort("no native retry before admission"),
            done
        );
        assert_eq!(
            fs::read(native.join("git-rebase-todo")).or_abort("native retry remains pending"),
            todo
        );
        assert_eq!(
            fs::read(&configuration.path).or_abort("obstruction preserved"),
            obstruction.as_bytes()
        );
    }
    if matches!(
        control,
        OpeningControl::Abort | OpeningControl::CopiedAbort | OpeningControl::MovedAbort
    ) {
        repository.success(&["--abort"]);
        assert_eq!(repository.git(&["rev-parse", "HEAD^{tree}"]), final_tree);
        assert_eq!(
            repository.git(&["rev-parse", "--symbolic-full-name", "HEAD"]),
            "refs/heads/main"
        );
        assert_eq!(
            repository.git(&["rev-parse", "HEAD"]),
            configuration.checkpoint
        );
        let without_owned_lease = refs
            .lines()
            .filter(|line| !line.ends_with(" refs/factor/session-lease"))
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(repository.git(&["show-ref"]), without_owned_lease);
        assert_eq!(
            fs::read(&configuration.path).or_abort("unpicked root obstruction survives abort"),
            obstruction.as_bytes()
        );
        assert_eq!(
            fs::read(repository.environment.cwd.join("atom"))
                .or_abort("root checkpoint atom bytes"),
            b"atom\n"
        );
        assert_eq!(
            fs::read(repository.environment.cwd.join("remainder"))
                .or_abort("root checkpoint remainder bytes"),
            body.as_bytes()
        );
        assert!(!native.exists());
        assert!(!configuration.admin.join("factor-journal.json").exists());
        return;
    }
    let relocated = repository.directory.path().join("saved-obstruction");
    fs::rename(&configuration.path, &relocated).or_abort("move only fixture-owned obstruction");
    let resumed = repository.success(&["--continue"]);
    assert_eq!(
        resumed
            .pointer("/operation")
            .and_then(serde_json::Value::as_str),
        Some("continue")
    );
    let selecting: serde_json::Value =
        serde_json::from_slice(&repository.journal()).or_abort("resumed source Selecting");
    assert_eq!(
        selecting
            .pointer("/state/phase")
            .and_then(serde_json::Value::as_str),
        Some("selecting")
    );
    assert_eq!(
        fs::read(repository.environment.cwd.join("temporary"))
            .or_abort("source selection path restored"),
        b"selected canceled path\n"
    );
    repository.git(&["add", "atom"]);
    repository.success(&["--message", "Extract root atom"]);
    repository.success(&["--finish"]);
    assert_eq!(repository.git(&["rev-parse", "HEAD^{tree}"]), final_tree);
    assert_eq!(
        fs::read(relocated).or_abort("relocated root input preserved"),
        obstruction.as_bytes()
    );
}

#[expect(
    clippy::create_dir,
    reason = "exclusive fixture directories must refuse existing occupancy rather than silently admit unrelated files"
)]
#[expect(
    clippy::too_many_lines,
    reason = "one decisive native lifecycle keeps its exact interruption evidence and full preservation oracle together"
)]
pub(in crate::git_factor::engine::tests) fn opening_source_finished_before_break(body: &str) {
    let repository = owned_repository();
    repository.write("atom", "atom\n");
    repository.write("remainder", body);
    let selected = repository.commit("Root selected source");
    repository.write("descendant", "descendant\n");
    repository.commit("Real descendant");
    let checkpoint = repository.git(&["rev-parse", "HEAD"]);
    let final_tree = repository.git(&["rev-parse", "HEAD^{tree}"]);
    let evidence = repository.directory.path().join("source-finished-evidence");
    fs::create_dir(&evidence).or_abort("owned source-finished evidence");
    let configuration = ChildStimulus {
        admin: fs::canonicalize(repository.environment.cwd.join(".git"))
            .or_abort("source-finished canonical admin"),
        launcher: fs::canonicalize(&repository.environment.executable)
            .or_abort("source-finished launcher"),
        checkpoint,
        original_pick: selected.clone(),
        callback: "e1-source-finished".to_owned(),
        accepted_count: 0,
        boundary: ChildBoundary::BeforeCallbackKill,
        evidence,
    };
    let config_path = repository
        .directory
        .path()
        .join("source-finished-config.json");
    fs::write(
        &config_path,
        serde_json::to_vec(&configuration).or_abort("source-finished configuration bytes"),
    )
    .or_abort("owned source-finished configuration");
    let original = fs::read_to_string(&repository.environment.executable)
        .or_abort("owned source-finished launcher body");
    let assignment = format!(
        "export FACTOR_TEST_E1_SOURCE_FINISHED={}\n",
        shell_quote(config_path.to_str().or_abort("source config UTF-8"))
    );
    let changed = original.replacen("set -eu\n", &format!("set -eu\n{assignment}"), 1);
    assert_ne!(changed, original);
    fs::write(&repository.environment.executable, changed).or_abort("own source hook launcher");
    let hooks = repository.git(&["config", "--get", "core.hooksPath"]);
    let hook = Path::new(&hooks).join("reference-transaction");
    fs::write(
        &hook,
        format!(
            "#!/bin/bash\nset -eu\ntest \"$1\" = committed || exit 0\ntest \"$(git rev-parse --absolute-git-dir)\" = {} || exit 0\nexec {} e1-source-finished \"$1\"\n",
            shell_quote(configuration.admin.to_str().or_abort("source admin UTF-8")),
            shell_quote(configuration.launcher.to_str().or_abort("source launcher UTF-8"))
        ),
    )
    .or_abort("actual owned reference-transaction hook");
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o700))
        .or_abort("source hook executable");
    let arguments = [
        "-c",
        "rebase.missingCommitsCheck=ignore",
        "rebase",
        "--interactive",
        "--no-ff",
        "--reschedule-failed-exec",
        "--no-update-refs",
        "--no-autostash",
        "--no-autosquash",
        "--no-rebase-merges",
        "--empty=keep",
        "--keep-empty",
        "--root",
    ];
    let runner = OwnedNativeObservation {
        admin: &configuration.admin,
        evidence: &configuration.evidence,
        arguments: &arguments,
    };
    assert_eq!(
        repository
            .invoke_with(&["--gate", "tests", "true", &selected], &runner, &REAL_FS)
            .0,
        EXIT_TEMPFAIL
    );
    assert_eq!(await_kill_status(&configuration.evidence), "0\n");
    assert!(configuration.evidence.join("fired").is_file());
    fs::remove_file(&hook).or_abort("remove only own one-shot source hook");
    let journal: serde_json::Value =
        serde_json::from_slice(&repository.journal()).or_abort("actual source-finished Opening");
    assert_eq!(
        journal
            .pointer("/state/phase")
            .and_then(serde_json::Value::as_str),
        Some("opening")
    );
    let source = journal
        .pointer("/state/source")
        .and_then(serde_json::Value::as_str)
        .or_abort("actual saved source");
    assert_eq!(
        repository.git(&["rev-parse", "HEAD^{tree}"]),
        repository.git(&["rev-parse", &format!("{source}^{{tree}}")])
    );
    assert_eq!(repository.git(&["show", "-s", "--format=%P", "HEAD"]), "");
    let native = configuration.admin.join("rebase-merge");
    let todo = fs::read_to_string(native.join("git-rebase-todo")).or_abort("actual pre-break todo");
    assert_eq!(
        todo.lines()
            .find(|line| !line.trim().is_empty() && !line.trim_start().starts_with('#')),
        Some("break")
    );
    repository.success(&["--continue"]);
    let selecting: serde_json::Value =
        serde_json::from_slice(&repository.journal()).or_abort("source-finished resumed selection");
    assert_eq!(
        selecting
            .pointer("/state/phase")
            .and_then(serde_json::Value::as_str),
        Some("selecting")
    );
    assert!(!repository.environment.cwd.join("descendant").exists());
    repository.git(&["add", "atom"]);
    repository.success(&["--message", "Extract source-finished atom"]);
    repository.success(&["--finish"]);
    assert_eq!(repository.git(&["rev-parse", "HEAD^{tree}"]), final_tree);
}

pub(in crate::git_factor::engine::tests) fn opening_wrong_pick_refusal(
    body: &str,
    obstruction: &str,
) {
    opening_root_control(OpeningControl::WrongPick, body, obstruction);
}

/// Own one native invocation per fresh evidence directory; observed stderr uses `create_new`.
/// For nonquiet stderr observation, reusing its directory refuses before spawn.
#[expect(
    clippy::panic_in_result_fn,
    reason = "the owned test adapter refuses invalid fixture authority rather than converting assertion failure into product recovery"
)]
#[expect(
    clippy::too_many_arguments,
    reason = "this test-only native adapter binds exact process arguments, capabilities, and ownership evidence at one launch boundary"
)]
fn owned_native_status(
    bin: &str,
    args: &[&str],
    envs: &[(&str, Option<&str>)],
    quiet: bool,
    cwd: &Path,
    expected_admin: &Path,
    evidence: &Path,
    observe_stderr: bool,
) -> io::Result<ExitStatus> {
    let native_cwd = fs::canonicalize(cwd)?;
    let admin = fs::canonicalize(native_cwd.join(".git"))?;
    assert_eq!(admin, expected_admin);
    let mut command = Command::new(bin);
    command.args(args).current_dir(&native_cwd);
    for &(key, value) in envs {
        match value {
            Some(assignment) => {
                command.env(key, assignment);
            }
            None => {
                command.env_remove(key);
            }
        }
    }
    let stderr_path = evidence.join("native-stderr");
    if quiet {
        command.stdout(Stdio::null()).stderr(Stdio::null());
    } else {
        command.stdout(fs::OpenOptions::new().write(true).open("/dev/stderr")?);
        if observe_stderr {
            command.stderr(
                fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&stderr_path)?,
            );
        } else {
            command.stderr(Stdio::inherit());
        }
    }
    let mut child = command.spawn()?;
    let admitted = NativeIngress {
        pid: child.id(),
        parent: process::id(),
        cwd: native_cwd,
        admin,
        arguments: args.iter().map(|argument| (*argument).to_owned()).collect(),
    };
    // Publication still precedes fixture child admission to kill this exact owned PID.
    let published = (|| {
        let bytes = serde_json::to_vec(&admitted).map_err(io::Error::other)?;
        REAL_FS.write_atomic_string(
            &evidence.join("native-ingress.json"),
            from_utf8(&bytes).map_err(io::Error::other)?,
        )
    })();
    if let Err(error) = published {
        let _kill = child.kill();
        let _wait = child.wait();
        return Err(error);
    }
    let status = match child.wait() {
        Ok(status) => status,
        Err(error) => {
            let _kill = child.kill();
            let _wait = child.wait();
            return Err(error);
        }
    };
    if observe_stderr && !quiet {
        // Test-only spool: native stderr is forwarded byte-for-byte after reaping.
        // Native stdout keeps its immediate real-stderr routing; there is no pipe/EOF thread.
        let bytes = fs::read(&stderr_path)?;
        io::stderr().write_all(&bytes)?;
    }
    Ok(status)
}

/// Pins only this disposable fixture's native transcript and hook inputs.
#[expect(
    clippy::create_dir,
    reason = "exclusive fixture directories must refuse existing occupancy rather than silently admit unrelated files"
)]
fn owned_repository() -> Repository {
    let repository = Repository::new();
    for (key, value) in [
        ("rebase.abbreviateCommands", "false"),
        ("rebase.instructionFormat", "%s"),
        ("core.abbrev", "auto"),
        ("core.commentChar", "#"),
        ("core.commentString", "#"),
    ] {
        repository.git(&["config", "--local", key, value]);
    }
    let hooks = repository.environment.cwd.join(".git/e1-owned-hooks");
    fs::create_dir(&hooks).or_abort("fixture-owned empty hooks");
    repository.git(&[
        "config",
        "--local",
        "core.hooksPath",
        hooks.to_str().or_abort("owned hooks UTF-8"),
    ]);
    repository
}

pub(in crate::git_factor::engine::tests) fn pending_amended_predecessor_refusal(body: &str) {
    amended_conflict_control(PredecessorControl::Amend, body);
}

pub(in crate::git_factor::engine::tests) fn pending_selection_rewind_refusal(body: &str) {
    amended_conflict_control(PredecessorControl::SelectionRewind, body);
}

/// No prototype format can infer the new independent-remainder acceptance fact.
pub(in crate::git_factor::engine::tests) fn prototype_format_refusal(body: &str) {
    let repository = owned_repository();
    repository.write("base", "base\n");
    repository.commit("Base");
    repository.write("atom", "atom\n");
    repository.write("remainder", body);
    repository.commit("Selected source");
    repository.success(&["--gate", "tests", "true"]);
    let original = repository.journal();
    let original_text = from_utf8(&original).or_abort("actual v2 journal UTF-8");
    let changed = original_text.replacen("checkpoint_v2", "checkpoint_v1", 1);
    assert_ne!(changed, original_text);
    let admin =
        fs::canonicalize(repository.environment.cwd.join(".git")).or_abort("format refusal admin");
    fs::write(admin.join("factor-journal.json"), &changed)
        .or_abort("only external prototype format edit");
    let index = repository.index();
    let head = repository.git(&["rev-parse", "HEAD"]);
    let refs = repository.git(&["show-ref"]);
    let native = admin.join("rebase-merge");
    let done = fs::read(native.join("done")).or_abort("format refusal done snapshot");
    let todo = fs::read(native.join("git-rebase-todo")).or_abort("format refusal todo snapshot");
    for operation in ["--status", "--continue", "--abort"] {
        assert_ne!(repository.invoke(&[operation]).0, EXIT_OK);
        assert!(repository.output.stderr.borrow().contains("checkpoint_v1"));
        assert_eq!(repository.journal(), changed.as_bytes());
        assert_eq!(repository.index(), index);
        assert_eq!(repository.git(&["rev-parse", "HEAD"]), head);
        assert_eq!(repository.git(&["show-ref"]), refs);
        assert_eq!(
            fs::read(native.join("done")).or_abort("format refusal done conservation"),
            done
        );
        assert_eq!(
            fs::read(native.join("git-rebase-todo")).or_abort("format refusal todo conservation"),
            todo
        );
        assert_eq!(
            fs::read(repository.environment.cwd.join("atom")).or_abort("format atom conservation"),
            b"atom\n"
        );
        assert_eq!(
            fs::read(repository.environment.cwd.join("remainder"))
                .or_abort("format remainder conservation"),
            body.as_bytes()
        );
    }
    fs::write(admin.join("factor-journal.json"), original)
        .or_abort("restore only external format input");
    repository.success(&["--abort"]);
}

/// Four real child publication failures: intermediate/final × before/after rename.
pub(in crate::git_factor::engine::tests) fn publication_retry(
    final_slot: bool,
    after_write: bool,
    body: &str,
) {
    child_interruption(
        "checkpoint-gate-descendant",
        final_slot,
        if after_write {
            ChildBoundary::AfterWrite
        } else {
            ChildBoundary::BeforeWrite
        },
        body,
    );
}

#[expect(
    clippy::cognitive_complexity,
    reason = "the closed fixture boundary matrix keeps before/after native interruption and preservation assertions explicit"
)]
#[expect(
    clippy::too_many_lines,
    reason = "one decisive native lifecycle keeps its exact interruption evidence and full preservation oracle together"
)]
fn ready_break_control(advance_native: bool, body: &str) {
    let repository = owned_repository();
    repository.write("base", "base\n");
    repository.commit("Base");
    repository.write("atom", "atom\n");
    repository.write("remainder", body);
    let selected = repository.commit("Selected source");
    repository.write("descendant", "descendant\n");
    repository.commit("Real descendant");
    let final_tree = repository.git(&["rev-parse", "HEAD^{tree}"]);
    let failure = ReadyBreakPublication {
        admin: fs::canonicalize(repository.environment.cwd.join(".git"))
            .or_abort("ready-break admin"),
        refused: Cell::new(false),
    };
    let (code, _) = repository.invoke_with(
        &["--gate", "tests", "true", &selected],
        &REAL_RUNNER,
        &failure,
    );
    assert_ne!(code, EXIT_OK);
    assert!(failure.refused.get());
    let journal: serde_json::Value = serde_json::from_slice(&repository.journal())
        .or_abort("actual Opening after native source break");
    assert_eq!(
        journal
            .pointer("/state/phase")
            .and_then(serde_json::Value::as_str),
        Some("opening")
    );
    assert_eq!(
        journal
            .pointer("/state/head")
            .and_then(serde_json::Value::as_str),
        Some(repository.git(&["rev-parse", "HEAD"]).as_str())
    );
    let native = failure.admin.join("rebase-merge");
    let done = fs::read(native.join("done")).or_abort("actual ready source break done");
    let todo =
        fs::read(native.join("git-rebase-todo")).or_abort("actual pending own remainder todo");
    assert_eq!(
        from_utf8(&done)
            .or_abort("ready source break UTF-8")
            .lines()
            .rev()
            .find(|line| !line.trim().is_empty() && !line.trim_start().starts_with('#')),
        Some("break")
    );
    assert!(!repository.environment.cwd.join("descendant").exists());
    let checkpoint = repository.git(&["rev-parse", "refs/heads/main"]);
    let index = repository.index();
    let references = repository.git(&["show-ref"]);
    let journal_bytes = repository.journal();
    let actual_head = repository.git(&["rev-parse", "HEAD"]);
    if advance_native {
        let bypass = Command::new("git")
            .args(["rebase", "--continue"])
            .current_dir(&repository.environment.cwd)
            .env("GIT_EDITOR", "true")
            .output()
            .or_abort("actual external native advance beyond source break");
        assert!(!bypass.status.success());
        let advanced_done = fs::read(native.join("done")).or_abort("actual advanced Opening done");
        let advanced_todo =
            fs::read(native.join("git-rebase-todo")).or_abort("actual retained remainder exec");
        assert_ne!(advanced_done, done);
        // A failed exec is consumed into done and rescheduled at the same front
        // position, so the remaining todo legitimately retains identical bytes.
        assert_eq!(advanced_todo, todo);
        assert!(String::from_utf8_lossy(&advanced_done).contains("checkpoint-gate-remainder"));
        assert_ne!(repository.invoke(&["--continue"]).0, EXIT_OK);
        assert!(
            repository
                .output
                .stderr
                .borrow()
                .contains("opening replay already advanced beyond its source break")
        );
        assert_eq!(repository.journal(), journal_bytes);
        assert_eq!(repository.index(), index);
        assert_eq!(repository.git(&["rev-parse", "HEAD"]), actual_head);
        assert_eq!(repository.git(&["show-ref"]), references);
        assert_eq!(
            fs::read(native.join("done")).or_abort("advanced Opening done conservation"),
            advanced_done
        );
        assert_eq!(
            fs::read(native.join("git-rebase-todo")).or_abort("advanced Opening todo conservation"),
            advanced_todo
        );
        assert_eq!(
            fs::read(repository.environment.cwd.join("atom")).or_abort("advanced atom bytes"),
            b"atom\n"
        );
        assert_eq!(
            fs::read(repository.environment.cwd.join("remainder"))
                .or_abort("advanced remainder bytes"),
            body.as_bytes()
        );
        return;
    }
    let todo_text = from_utf8(&todo).or_abort("saved-head todo UTF-8");
    let own_pick = todo_text
        .lines()
        .find(|line| line.starts_with("pick "))
        .or_abort("real future descendant pick");
    let picked = own_pick
        .split_whitespace()
        .nth(1)
        .or_abort("future descendant token");
    let source = journal
        .pointer("/state/source")
        .and_then(serde_json::Value::as_str)
        .or_abort("owned source SHA");
    let changed = todo_text.replacen(&format!("pick {picked}"), &format!("pick {source}"), 1);
    assert_ne!(changed, todo_text);
    fs::write(native.join("git-rebase-todo"), &changed)
        .or_abort("one wrong future pick at actual saved-head seam");
    assert_ne!(repository.invoke(&["--continue"]).0, EXIT_OK);
    assert!(
        repository
            .output
            .stderr
            .borrow()
            .contains("native replay omitted, repeated, or reordered an original source slot")
    );
    assert_eq!(repository.journal(), journal_bytes);
    assert_eq!(repository.index(), index);
    assert_eq!(repository.git(&["rev-parse", "HEAD"]), actual_head);
    assert_eq!(repository.git(&["show-ref"]), references);
    assert_eq!(
        fs::read(native.join("done")).or_abort("saved-head wrong-pick done preserved"),
        done
    );
    assert_eq!(
        fs::read_to_string(native.join("git-rebase-todo"))
            .or_abort("saved-head wrong-pick input preserved"),
        changed
    );
    assert_eq!(
        fs::read(repository.environment.cwd.join("atom"))
            .or_abort("saved-head atom bytes preserved"),
        b"atom\n"
    );
    assert_eq!(
        fs::read(repository.environment.cwd.join("remainder"))
            .or_abort("saved-head remainder bytes preserved"),
        body.as_bytes()
    );
    fs::write(native.join("git-rebase-todo"), &todo)
        .or_abort("restore only fixture-owned saved-head edit");
    repository.success(&["--continue"]);
    assert_eq!(
        fs::read(native.join("done")).or_abort("ready-break continuation must not advance done"),
        done
    );
    assert_eq!(
        fs::read(native.join("git-rebase-todo"))
            .or_abort("ready-break continuation must not advance todo"),
        todo
    );
    assert_eq!(
        repository.git(&["rev-parse", "refs/heads/main"]),
        checkpoint
    );
    assert!(!repository.environment.cwd.join("descendant").exists());
    let selecting: serde_json::Value = serde_json::from_slice(&repository.journal())
        .or_abort("Selecting after ready break recovery");
    assert_eq!(
        selecting
            .pointer("/state/phase")
            .and_then(serde_json::Value::as_str),
        Some("selecting")
    );
    repository.git(&["add", "atom"]);
    repository.success(&["--message", "Extract ready-source atom"]);
    repository.success(&["--finish"]);
    assert_eq!(repository.git(&["rev-parse", "HEAD^{tree}"]), final_tree);
}

/// An exec-only metadata repair must preserve recreated files; future picks still refuse.
pub(in crate::git_factor::engine::tests) fn recreated_descendant_path(
    future_pick: bool,
    body: &str,
) {
    let repository = owned_repository();
    repository.write("base", "base\n");
    repository.write("deleted", "original tracked bytes\n");
    repository.commit("Base");
    repository.write("atom", "atom\n");
    repository.write("remainder", "remainder\n");
    let selected = repository.commit("Selected source");
    fs::remove_file(repository.environment.cwd.join("deleted"))
        .or_abort("owned first descendant deletion");
    repository.commit("Delete tracked path");
    if future_pick {
        repository.write("deleted", "future descendant replacement\n");
    } else {
        repository.write("later", "future independent descendant\n");
    }
    repository.commit("Later descendant");
    let final_tree = repository.git(&["rev-parse", "HEAD^{tree}"]);
    repository.success(&["--gate", "tests", "true", &selected]);
    let hooks = repository.git(&["config", "--get", "core.hooksPath"]);
    let hook = Path::new(&hooks).join("commit-msg");
    fs::write(
        &hook,
        "#!/bin/bash\nset -eu\nif grep -q '^Delete tracked path$' \"$1\"; then exit 23; fi\n",
    )
    .or_abort("owned descendant message refusal");
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o700))
        .or_abort("owned descendant hook executable");
    repository.git(&["add", "atom", "remainder"]);
    assert_ne!(repository.invoke(&["--message", "Extract atom"]).0, EXIT_OK);
    fs::remove_file(&hook).or_abort("remove only own descendant refusal hook");
    assert!(!repository.environment.cwd.join("deleted").exists());
    repository.write("deleted", body);
    let admin = fs::canonicalize(repository.environment.cwd.join(".git"))
        .or_abort("recreated descendant admin");
    fs::write(admin.join("info/exclude"), "deleted\n")
        .or_abort("owned ignored recreation configuration");
    let native = admin.join("rebase-merge");
    let journal = repository.journal();
    let index = repository.index();
    let head = repository.git(&["rev-parse", "HEAD"]);
    let refs = repository.git(&["show-ref"]);
    let done = fs::read(native.join("done")).or_abort("actual descendant done");
    let todo = fs::read(native.join("git-rebase-todo")).or_abort("actual descendant todo");
    assert!(String::from_utf8_lossy(&todo).contains("checkpoint-gate-descendant"));
    let result = repository.invoke(&["--continue"]);
    if future_pick {
        assert_ne!(result.0, EXIT_OK);
        assert!(result.1.is_empty());
        assert!(
            repository
                .output
                .stderr
                .borrow()
                .contains("checkpoint would overwrite unrelated untracked work")
        );
        assert_eq!(repository.journal(), journal);
        assert_eq!(repository.index(), index);
        assert_eq!(repository.git(&["rev-parse", "HEAD"]), head);
        assert_eq!(repository.git(&["show-ref"]), refs);
        assert_eq!(
            fs::read(native.join("done")).or_abort("done preserved"),
            done
        );
        assert_eq!(
            fs::read(native.join("git-rebase-todo")).or_abort("todo preserved"),
            todo
        );
        assert_eq!(
            fs::read(repository.environment.cwd.join("deleted"))
                .or_abort("ignored future collision preserved"),
            body.as_bytes()
        );
        let saved = repository.directory.path().join("saved-recreated-input");
        fs::rename(repository.environment.cwd.join("deleted"), &saved)
            .or_abort("move only own obstruction");
        repository.success(&["--continue"]);
        assert_eq!(
            fs::read(saved).or_abort("relocated input retained"),
            body.as_bytes()
        );
    } else {
        assert_eq!(result.0, EXIT_OK);
        assert_eq!(
            fs::read(repository.environment.cwd.join("deleted"))
                .or_abort("exec-only metadata repair preserved recreation"),
            body.as_bytes()
        );
    }
    assert_eq!(repository.git(&["rev-parse", "HEAD^{tree}"]), final_tree);
    assert!(
        repository
            .git(&["log", "--format=%s", "HEAD"])
            .contains("Extract atom")
    );
}

pub(in crate::git_factor::engine::tests) fn remainder_bypass_refusal(
    descendants: bool,
    body: &str,
) {
    bypass_control(
        if descendants {
            BypassBoundary::RemainderWithDescendants
        } else {
            BypassBoundary::RemainderWithoutDescendants
        },
        body,
    );
}

/// A deterministic failing descendant gate must rerun natively with recovery JSON.
pub(in crate::git_factor::engine::tests) fn retained_descendant_still_fails(body: &str) {
    let repository = owned_repository();
    repository.write("base", "base\n");
    repository.commit("Base");
    repository.write("atom", "atom\n");
    repository.write("remainder", "remainder\n");
    let selected = repository.commit("Selected source");
    repository.write("first", body);
    let original = repository.commit("Failing descendant");
    repository.write("second", "second\n");
    repository.commit("Later descendant");
    repository.success(&[
        "--gate",
        "tests",
        "if test -f first; then exit 23; fi",
        &selected,
    ]);
    repository.git(&["add", "atom"]);
    assert_eq!(
        repository.invoke(&["--message", "Extract atom"]).0,
        EXIT_TEMPFAIL
    );
    let native = repository.environment.cwd.join(".git/rebase-merge");
    let before = fs::read_to_string(native.join("done"))
        .or_abort("native initial failing descendant callback");
    let todo = fs::read_to_string(native.join("git-rebase-todo"))
        .or_abort("actual retained descendant callback");
    assert!(
        todo.lines()
            .find(|line| !line.trim().is_empty() && !line.trim_start().starts_with('#'))
            .is_some_and(|line| line.contains("checkpoint-gate-descendant"))
    );
    let picked = before
        .lines()
        .rfind(|line| line.starts_with("pick "))
        .or_abort("original failing pick")
        .split_whitespace()
        .nth(1)
        .or_abort("failing pick token");
    assert_eq!(
        repository.git(&["rev-parse", "--verify", &format!("{picked}^{{commit}}")]),
        original
    );
    let journal = repository.journal();
    let head = repository.git(&["rev-parse", "HEAD"]);
    let branch = repository.git(&["rev-parse", "refs/heads/main"]);
    let (code, output) = repository.invoke(&["--continue"]);
    assert_eq!(code, EXIT_TEMPFAIL);
    let observed: serde_json::Value =
        serde_json::from_str(&output).or_abort("normalized retained-callback recovery JSON");
    let expected: serde_json::Value = serde_json::from_str(r#"{"actions":{"amend":["git","commit","--amend","--no-edit"],"continue_factor":["git","factor","--continue"],"stage":["git","add","<paths>"]},"operation":"continue","result":"recovery_required"}"#)
        .or_abort("independent recovery JSON oracle");
    assert_eq!(observed, expected);
    let after =
        fs::read_to_string(native.join("done")).or_abort("actual native failed callback rerun");
    assert_eq!(
        after
            .lines()
            .filter(|line| line.contains("checkpoint-gate-descendant"))
            .count(),
        before
            .lines()
            .filter(|line| line.contains("checkpoint-gate-descendant"))
            .count()
            .checked_add(1)
            .or_abort("next retained native callback attempt")
    );
    assert_eq!(repository.journal(), journal);
    assert_eq!(repository.git(&["rev-parse", "HEAD"]), head);
    assert_eq!(repository.git(&["rev-parse", "refs/heads/main"]), branch);
    assert_eq!(
        fs::read(repository.environment.cwd.join("first"))
            .or_abort("deterministic failure input preserved"),
        body.as_bytes()
    );
    assert!(!repository.environment.cwd.join("second").exists());
}

/// Fixture reference-transaction callback: only a real completed source HEAD update can fire.
#[expect(
    clippy::exit,
    reason = "the owned child executor must terminate immediately after its precisely admitted native interruption"
)]
#[expect(
    clippy::single_call_fn,
    reason = "named native fixture boundaries keep ownership, stimulus, and conservation visible to the direct canonical provider"
)]
#[expect(
    clippy::too_many_lines,
    reason = "one decisive native lifecycle keeps its exact interruption evidence and full preservation oracle together"
)]
pub(in crate::git_factor::engine::tests) fn source_finished_hook(
    arguments: &[OsString],
    environment: &NativeEnv,
) -> bool {
    if arguments.get(1).and_then(|argument| argument.to_str()) != Some("e1-source-finished") {
        return false;
    }
    if arguments.get(2).and_then(|argument| argument.to_str()) != Some("committed") {
        return true;
    }
    let config = env::var_os("FACTOR_TEST_E1_SOURCE_FINISHED")
        .or_abort("exact source-finished fixture payload");
    let configuration: ChildStimulus =
        serde_json::from_slice(&fs::read(config).or_abort("source hook configuration"))
            .or_abort("source hook input grammar");
    assert_eq!(
        fs::canonicalize(environment.cwd.join(".git")).or_abort("source hook admin"),
        configuration.admin
    );
    assert_eq!(
        fs::canonicalize(&environment.executable).or_abort("source hook launcher"),
        configuration.launcher
    );
    let native = configuration.admin.join("rebase-merge");
    if !native.is_dir() || configuration.evidence.join("fired").exists() {
        return true;
    }
    let journal_bytes = fs::read(configuration.admin.join("factor-journal.json"))
        .or_abort("actual source-hook journal");
    let journal: serde_json::Value =
        serde_json::from_slice(&journal_bytes).or_abort("actual source-hook state");
    if journal
        .pointer("/state/phase")
        .and_then(serde_json::Value::as_str)
        != Some("opening")
    {
        return true;
    }
    let source = journal
        .pointer("/state/source")
        .and_then(serde_json::Value::as_str)
        .or_abort("owned source-hook SHA");
    let actual = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(&environment.cwd)
        .output()
        .or_abort("source hook actual HEAD");
    assert!(actual.status.success());
    let actual_head = String::from_utf8(actual.stdout).or_abort("source-hook HEAD UTF-8");
    let observe = |commit: &str| {
        let output = Command::new("git")
            .args(["show", "-s", "--format=%T%n%P", commit])
            .current_dir(&environment.cwd)
            .output()
            .or_abort("source hook physical tree and parent");
        assert!(output.status.success());
        output.stdout
    };
    if observe(actual_head.trim_end()) != observe(source) {
        return true;
    }
    let done = fs::read_to_string(native.join("done")).or_abort("source-hook actual done");
    let actions = done
        .lines()
        .filter(|line| !line.trim().is_empty() && !line.trim_start().starts_with('#'))
        .collect::<Vec<_>>();
    assert_eq!(actions.len(), 1);
    let picked = actions
        .first()
        .or_abort("one native source pick")
        .split_whitespace()
        .nth(1)
        .or_abort("source pick token");
    let resolved_pick = Command::new("git")
        .args(["rev-parse", "--verify", &format!("{picked}^{{commit}}")])
        .current_dir(&environment.cwd)
        .output()
        .or_abort("resolve exact native source pick");
    assert!(resolved_pick.status.success());
    assert_eq!(
        String::from_utf8(resolved_pick.stdout)
            .or_abort("picked source UTF-8")
            .trim_end(),
        source
    );
    let todo = fs::read_to_string(native.join("git-rebase-todo"))
        .or_abort("source hook pending native break");
    assert_eq!(
        todo.lines()
            .find(|line| !line.trim().is_empty() && !line.trim_start().starts_with('#')),
        Some("break")
    );
    fs::write(
        configuration.evidence.join("fired"),
        "committed source HEAD before break\n",
    )
    .or_abort("source-finished hook claim");
    fs::write(
        configuration.evidence.join("observed-journal.json"),
        journal_bytes,
    )
    .or_abort("source hook actual journal evidence");
    fs::write(configuration.evidence.join("done"), done)
        .or_abort("source hook actual done evidence");
    fs::write(configuration.evidence.join("git-rebase-todo"), todo)
        .or_abort("source hook actual todo evidence");
    kill_owned_native(
        &configuration,
        &[
            "-c",
            "rebase.missingCommitsCheck=ignore",
            "rebase",
            "--interactive",
            "--no-ff",
            "--reschedule-failed-exec",
            "--no-update-refs",
            "--no-autostash",
            "--no-autosquash",
            "--no-rebase-merges",
            "--empty=keep",
            "--keep-empty",
            "--root",
        ],
    );
    process::exit(99);
}

/// Relocates only the actual native callback launcher, leaving its payload unchanged.
fn relocate_executable(repository: &mut Repository, moved: bool) {
    let previous = repository.environment.executable.clone();
    let relocated = repository
        .directory
        .path()
        .join("relocated factor's launcher");
    let bytes = fs::read(&previous).or_abort("original callback launcher bytes");
    if moved {
        fs::rename(&previous, &relocated).or_abort("move actual callback launcher");
        assert!(!previous.exists());
    } else {
        fs::copy(&previous, &relocated).or_abort("copy actual callback launcher");
        assert_eq!(
            fs::read(&previous).or_abort("original launcher retained"),
            bytes
        );
    }
    assert_eq!(
        fs::read(&relocated).or_abort("relocated launcher payload"),
        bytes
    );
    repository.environment.executable = relocated;
}

pub(in crate::git_factor::engine::tests) fn relocated_opening_abort(root: bool, moved: bool) {
    opening_source_control(
        if root {
            OpeningOrigin::Root
        } else {
            OpeningOrigin::NonRoot
        },
        if moved {
            OpeningControl::MovedAbort
        } else {
            OpeningControl::CopiedAbort
        },
        "remaining bytes\n",
        "unrelated source obstruction\n",
    );
}

/// Builds a real native replay pause, optionally after an already captured round.
fn relocated_replay(captured: bool) -> (Repository, String) {
    let repository = owned_repository();
    repository.write("base", "base\n");
    repository.commit("Base");
    repository.write("atom", "atom\n");
    repository.write("second", "second\n");
    repository.write("remainder", "remainder\n");
    let source = repository.commit("Selected source");
    repository.write("descendant", "descendant\n");
    repository.commit("Real descendant");
    repository.success(&["--gate", "tests", "true", &source]);
    repository.git(&["add", "atom"]);
    if captured {
        let committed = repository.success(&["--message", "First completed checkpoint"]);
        assert_eq!(
            committed
                .pointer("/split_count")
                .and_then(serde_json::Value::as_u64),
            Some(1)
        );
        let captured_branch = repository.git(&["rev-parse", "refs/heads/main"]);
        let captured_journal: serde_json::Value = serde_json::from_slice(&repository.journal())
            .or_abort("first completed checkpoint journal");
        assert_eq!(
            captured_journal
                .pointer("/checkpoint")
                .and_then(serde_json::Value::as_str),
            Some(captured_branch.as_str())
        );
        assert_eq!(
            repository
                .git(&["log", "--format=%s", "refs/heads/main"])
                .lines()
                .filter(|subject| *subject == "First completed checkpoint")
                .count(),
            1
        );
        repository.git(&["add", "second"]);
    }
    let checkpoint = repository.git(&["rev-parse", "refs/heads/main"]);
    let pause = BeforeNativeContinue {
        refused: Cell::new(false),
    };
    assert_ne!(
        repository
            .invoke_with(&["--message", "Pending atom"], &pause, &REAL_FS)
            .0,
        EXIT_OK
    );
    assert!(pause.refused.get());
    let journal: serde_json::Value =
        serde_json::from_slice(&repository.journal()).or_abort("paused replay journal");
    assert_eq!(
        journal
            .pointer("/state/phase")
            .and_then(serde_json::Value::as_str),
        Some("replaying")
    );
    assert_eq!(
        repository.git(&["rev-parse", "refs/heads/main"]),
        checkpoint
    );
    (repository, checkpoint)
}

#[expect(
    clippy::literal_string_with_formatting_args,
    reason = "HEAD^{tree} is literal Git revision syntax, not Rust interpolation"
)]
pub(in crate::git_factor::engine::tests) fn relocated_replay_abort(captured: bool, moved: bool) {
    let (mut repository, checkpoint) = relocated_replay(captured);
    let journal: serde_json::Value =
        serde_json::from_slice(&repository.journal()).or_abort("checkpoint witness");
    assert_eq!(
        journal
            .pointer("/checkpoint")
            .and_then(serde_json::Value::as_str),
        Some(checkpoint.as_str())
    );
    let tree = repository.git(&["rev-parse", &format!("{checkpoint}^{{tree}}")]);
    let refs = repository.git(&["show-ref"]);
    repository.write(".git/info/exclude", "ignored\n");
    repository.write("ignored", "ignored user bytes\n");
    repository.write("untracked", "untracked user bytes\n");
    relocate_executable(&mut repository, moved);

    let result = repository.success(&["--abort"]);

    assert_eq!(
        result
            .pointer("/operation")
            .and_then(serde_json::Value::as_str),
        Some("abort")
    );
    assert_eq!(repository.git(&["rev-parse", "HEAD"]), checkpoint);
    assert_eq!(repository.git(&["rev-parse", "HEAD^{tree}"]), tree);
    assert_eq!(repository.git(&["symbolic-ref", "HEAD"]), "refs/heads/main");
    assert_eq!(
        repository.git(&["show-ref"]),
        refs.lines()
            .filter(|line| !line.ends_with(" refs/factor/session-lease"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    for (path, bytes) in [
        ("atom", "atom\n"),
        ("second", "second\n"),
        ("remainder", "remainder\n"),
        ("descendant", "descendant\n"),
        ("ignored", "ignored user bytes\n"),
        ("untracked", "untracked user bytes\n"),
    ] {
        assert_eq!(
            fs::read(repository.environment.cwd.join(path)).or_abort("preserved abort file"),
            bytes.as_bytes()
        );
    }
    assert!(
        !repository
            .environment
            .cwd
            .join(".git/rebase-merge")
            .exists()
    );
    assert!(
        !repository
            .environment
            .cwd
            .join(".git/factor-journal.json")
            .exists()
    );
    assert!(!repository.environment.cwd.join(".git/factor").exists());
}

#[expect(
    clippy::literal_string_with_formatting_args,
    reason = "HEAD^{tree} is literal Git revision syntax, not Rust interpolation"
)]
pub(in crate::git_factor::engine::tests) fn relocated_terminal_abort(detached: bool) {
    let mut repository = super::interrupted_attached_terminal(false);
    let tip = repository.git(&["rev-parse", "HEAD"]);
    let tree = repository.git(&["rev-parse", "HEAD^{tree}"]);
    let refs = repository.git(&["show-ref"]);
    if detached {
        repository.git(&["update-ref", "--no-deref", "HEAD", &tip]);
        assert_eq!(
            repository.git(&["rev-parse", "--symbolic-full-name", "HEAD"]),
            "HEAD"
        );
    }
    relocate_executable(&mut repository, true);

    repository.success(&["--abort"]);

    assert_eq!(repository.git(&["rev-parse", "HEAD"]), tip);
    assert_eq!(repository.git(&["rev-parse", "HEAD^{tree}"]), tree);
    assert_eq!(repository.git(&["symbolic-ref", "HEAD"]), "refs/heads/main");
    assert_eq!(
        repository.git(&["show-ref"]),
        refs.lines()
            .filter(|line| !line.ends_with(" refs/factor/session-lease"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    assert_eq!(
        fs::read(repository.environment.cwd.join("unrelated")).or_abort("terminal user bytes"),
        b"preserved unrelated bytes\n"
    );
    assert!(
        !repository
            .environment
            .cwd
            .join(".git/rebase-merge")
            .exists()
    );
    assert!(
        !repository
            .environment
            .cwd
            .join(".git/factor-journal.json")
            .exists()
    );
}

/// Rejects transcript authority corruption before any checkpoint or native mutation.
#[expect(
    clippy::literal_string_with_formatting_args,
    reason = "HEAD^{tree} is literal Git revision syntax, not Rust interpolation"
)]
pub(in crate::git_factor::engine::tests) fn relocated_abort_refusal(edit: &str) {
    let mut repository = if edit == "source" {
        super::interrupted_attached_terminal(false)
    } else {
        relocated_replay(false).0
    };
    let native = repository.environment.cwd.join(".git/rebase-merge");
    if edit == "source" {
        let done = fs::read_to_string(native.join("done")).or_abort("original terminal transcript");
        let first = done.lines().next().or_abort("source pick");
        let changed = format!(
            "pick {} changed source",
            repository.git(&["rev-parse", "HEAD"])
        );
        fs::write(native.join("done"), done.replacen(first, &changed, 1))
            .or_abort("tampered first source pick");
    } else {
        let todo = fs::read_to_string(native.join("git-rebase-todo"))
            .or_abort("original pending transcript");
        let original = shell_quote(
            fs::canonicalize(&repository.environment.executable)
                .or_abort("actual canonical native callback path")
                .to_str()
                .or_abort("launcher UTF-8"),
        );
        let replacement = if edit == "mixed" {
            shell_quote("/foreign program")
        } else {
            format!("{original}; true")
        };
        let changed_todo = todo.replacen(&original, &replacement, 1);
        assert_ne!(
            changed_todo, todo,
            "native payload mutation must actually occur"
        );
        assert_eq!(changed_todo.matches(&replacement).count(), 1);
        assert_eq!(changed_todo.replacen(&replacement, &original, 1), todo);
        fs::write(native.join("git-rebase-todo"), changed_todo)
            .or_abort("foreign callback payload");
    }
    relocate_executable(&mut repository, true);
    let journal = repository.journal();
    let index = repository.index();
    let refs = repository.git(&["show-ref"]);
    let head = repository.git(&["rev-parse", "HEAD"]);
    let tree = repository.git(&["rev-parse", "HEAD^{tree}"]);
    let done = fs::read(native.join("done")).or_abort("refused transcript done");
    let todo = fs::read(native.join("git-rebase-todo")).or_abort("refused transcript todo");

    assert_ne!(repository.invoke(&["--abort"]).0, EXIT_OK);

    assert_eq!(repository.journal(), journal);
    assert_eq!(repository.index(), index);
    assert_eq!(repository.git(&["show-ref"]), refs);
    assert_eq!(repository.git(&["rev-parse", "HEAD"]), head);
    assert_eq!(repository.git(&["rev-parse", "HEAD^{tree}"]), tree);
    assert_eq!(
        fs::read(native.join("done")).or_abort("done preserved"),
        done
    );
    assert_eq!(
        fs::read(native.join("git-rebase-todo")).or_abort("todo preserved"),
        todo
    );
}

/// Moving the original launcher cannot authorize continued callback execution.
#[expect(
    clippy::single_call_fn,
    reason = "one named native witness keeps moved-path refusal and complete conservation together"
)]
pub(in crate::git_factor::engine::tests) fn moved_replay_continue_refusal() {
    let (mut repository, _checkpoint) = relocated_replay(false);
    relocate_executable(&mut repository, true);
    let native = repository.environment.cwd.join(".git/rebase-merge");
    let journal = repository.journal();
    let index = repository.index();
    let refs = repository.git(&["show-ref"]);
    let head = repository.git(&["rev-parse", "HEAD"]);
    let done = fs::read(native.join("done")).or_abort("original done transcript");
    let todo = fs::read(native.join("git-rebase-todo")).or_abort("original todo transcript");

    assert_ne!(repository.invoke(&["--continue"]).0, EXIT_OK);

    assert!(
        repository
            .output
            .stderr
            .borrow()
            .contains("native replay contains a foreign exec payload")
    );
    assert_eq!(repository.journal(), journal);
    assert_eq!(repository.index(), index);
    assert_eq!(repository.git(&["show-ref"]), refs);
    assert_eq!(repository.git(&["rev-parse", "HEAD"]), head);
    assert_eq!(
        fs::read(native.join("done")).or_abort("done conserved"),
        done
    );
    assert_eq!(
        fs::read(native.join("git-rebase-todo")).or_abort("todo conserved"),
        todo
    );
    assert!(!repository.environment.cwd.join("descendant").exists());
    for (path, bytes) in [
        ("atom", "atom\n"),
        ("second", "second\n"),
        ("remainder", "remainder\n"),
    ] {
        assert_eq!(
            fs::read(repository.environment.cwd.join(path)).or_abort("continued refusal bytes"),
            bytes.as_bytes()
        );
    }
}

/// A failed native exec retains its slot; abort owns path safety rather than a clean frontier.
#[expect(
    clippy::literal_string_with_formatting_args,
    reason = "HEAD^{tree} is literal Git revision syntax, not Rust interpolation"
)]
pub(in crate::git_factor::engine::tests) fn relocated_failed_exec_abort(unrelated: bool) {
    let mut repository = failed_exec_replay();
    let checkpoint = repository.git(&["rev-parse", "refs/heads/main"]);
    let tree = repository.git(&["rev-parse", &format!("{checkpoint}^{{tree}}")]);
    let native = repository.environment.cwd.join(".git/rebase-merge");
    let done = fs::read_to_string(native.join("done")).or_abort("failed exec consumed transcript");
    let todo = fs::read_to_string(native.join("git-rebase-todo"))
        .or_abort("failed exec rescheduled transcript");
    let dirty_path = if unrelated { "base" } else { "atom" };
    repository.write(dirty_path, "user dirty bytes\n");
    relocate_executable(&mut repository, true);
    let journal = repository.journal();
    let index = repository.index();
    let head = repository.git(&["rev-parse", "HEAD"]);
    let refs = repository.git(&["show-ref"]);

    let (code, _) = repository.invoke(&["--abort"]);

    if unrelated {
        assert_ne!(code, EXIT_OK);
        assert!(
            repository
                .output
                .stderr
                .borrow()
                .contains("abort would discard unrelated tracked work")
        );
        assert_eq!(repository.journal(), journal);
        assert_eq!(repository.index(), index);
        assert_eq!(repository.git(&["rev-parse", "HEAD"]), head);
        assert_eq!(repository.git(&["show-ref"]), refs);
        assert_eq!(
            fs::read_to_string(native.join("done")).or_abort("done unchanged"),
            done
        );
        assert_eq!(
            fs::read_to_string(native.join("git-rebase-todo")).or_abort("todo unchanged"),
            todo
        );
        assert_eq!(
            fs::read(repository.environment.cwd.join("base"))
                .or_abort("unrelated dirty work preserved"),
            b"user dirty bytes\n"
        );
    } else {
        assert_eq!(code, EXIT_OK, "{}", repository.output.stderr.borrow());
        assert_eq!(repository.git(&["rev-parse", "HEAD"]), checkpoint);
        assert_eq!(repository.git(&["rev-parse", "HEAD^{tree}"]), tree);
        assert_eq!(repository.git(&["symbolic-ref", "HEAD"]), "refs/heads/main");
        assert_eq!(
            repository.git(&["show-ref"]),
            refs.lines()
                .filter(|line| !line.ends_with(" refs/factor/session-lease"))
                .collect::<Vec<_>>()
                .join("\n")
        );
        assert!(!native.exists());
        assert!(
            !repository
                .environment
                .cwd
                .join(".git/factor-journal.json")
                .exists()
        );
    }
    for (path, bytes) in [
        ("atom", "atom\n"),
        ("remainder", "remainder\n"),
        ("descendant", "descendant\n"),
    ] {
        assert_eq!(
            fs::read(repository.environment.cwd.join(path)).or_abort("remaining user files"),
            bytes.as_bytes()
        );
    }
}

/// Arranges an actual failed and rescheduled callback before abort observes dirty work.
#[expect(
    clippy::single_call_fn,
    reason = "the paired dirty-work abort oracles share one real native failed-exec arrangement"
)]
fn failed_exec_replay() -> Repository {
    let repository = owned_repository();
    repository.write("base", "base\n");
    repository.commit("Base");
    repository.write("atom", "atom\n");
    repository.write("remainder", "remainder\n");
    let source = repository.commit("Selected source");
    repository.write("descendant", "descendant\n");
    repository.commit("Real descendant");
    repository.success(&["--gate", "tests", "test ! -e descendant", &source]);
    repository.git(&["add", "atom"]);
    assert_ne!(repository.invoke(&["--message", "Pending atom"]).0, EXIT_OK);
    let journal: serde_json::Value =
        serde_json::from_slice(&repository.journal()).or_abort("actual failed descendant journal");
    assert_eq!(
        journal
            .pointer("/state/phase")
            .and_then(serde_json::Value::as_str),
        Some("replaying")
    );
    let native = repository.environment.cwd.join(".git/rebase-merge");
    let done = fs::read_to_string(native.join("done")).or_abort("failed exec consumed transcript");
    let todo = fs::read_to_string(native.join("git-rebase-todo"))
        .or_abort("failed exec rescheduled transcript");
    // The remainder tree is already stamped; the new descendant is the failing gate.
    let callback = format!(
        "exec {} checkpoint-gate-descendant",
        shell_quote(
            fs::canonicalize(&repository.environment.executable)
                .or_abort("canonical failed-exec callback")
                .to_str()
                .or_abort("original callback path")
        )
    );
    assert_eq!(done.lines().last(), Some(callback.as_str()));
    assert_eq!(todo.lines().next(), Some(callback.as_str()));
    repository
}
