use crate::exit_codes::{EXIT_OK, EXIT_SOFTWARE};
use crate::test_support::OrAbort as _;
use core::cell::{Cell, RefCell};
use std::ffi::OsString;
use std::fs;
use std::io;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Output};

use super::{Capture, Io};
use super::{Fs, REAL_FS, REAL_RUNNER, Repository, Runner, open_selection};

/// Real first-publication and scratch-creation boundaries, without manufactured authority.
#[derive(Clone, Copy, Debug)]
pub(in crate::git_factor::engine::tests) enum BirthBoundary {
    AfterRename,
    BeforeRename,
    ScratchCreation,
}

/// Bounded native mutation boundaries whose results can be uncertain to callers.
#[derive(Clone, Copy, Debug)]
pub(in crate::git_factor::engine::tests) enum Boundary {
    ActorCleanup,
    ActorRegistered,
    LeasePublished,
    RebaseOpened,
    ReplayCompleted,
}

/// Real owned cleanup operations interrupted before or after their native effects.
#[derive(Clone, Copy)]
pub(in crate::git_factor::engine::tests) enum ClosingBoundary {
    Directory,
    DirectoryPartial,
    Journal,
    Lease,
}

#[derive(Clone, Copy, Debug)]
pub(in crate::git_factor::engine::tests) enum LegacyBoundary {
    DanglingLink,
    Directory,
    File,
}
struct BirthInterruption {
    boundary: BirthBoundary,
    fired: Cell<bool>,
    journal: PathBuf,
    published: RefCell<Vec<Vec<u8>>>,
    scratch: PathBuf,
    scratch_observation: RefCell<Option<Vec<u8>>>,
}
struct ClosingInterruption {
    after: bool,
    boundary: ClosingBoundary,
    fired: Cell<bool>,
}

struct ClosingLeaseQueryFailure {
    fired: Cell<bool>,
    lease: String,
}

struct InterruptedOutput<'capture> {
    at: u8,
    capture: &'capture Capture,
    writes: Cell<u8>,
}

struct InterruptedRunner {
    boundary: Boundary,
    fired: Cell<bool>,
}

/// Actual journal publication targets, with no independent predicate switches.
#[derive(Clone, Copy)]
enum PublicationTarget {
    CheckpointCapture,
    InitialReplay,
    SecondWrite,
}

/// A journal-only fault; normal native files retain the actual Fs behavior.
struct JournalPublication {
    after_replace: bool,
    calls: Cell<u8>,
    target: PublicationTarget,
}

#[derive(Debug, Eq, PartialEq)]
struct SelectingFrame {
    checkpoint: String,
    done: Vec<u8>,
    files: Vec<Vec<u8>>,
    head: String,
    index: Vec<u8>,
    protected_refs: Vec<String>,
    todo: Vec<u8>,
}

impl SelectingFrame {
    fn read(repository: &Repository) -> Self {
        let index = repository.index();
        let native = repository.environment.cwd.join(".git/rebase-merge");
        Self {
            checkpoint: repository.git(&["rev-parse", "refs/heads/main"]),
            done: fs::read(native.join("done")).or_abort("selection done"),
            files: ["atom", "remainder", "unrelated"]
                .map(|path| {
                    fs::read(repository.environment.cwd.join(path)).or_abort("protected file")
                })
                .to_vec(),
            head: repository.git(&["rev-parse", "HEAD"]),
            index,
            protected_refs: unrelated_refs(repository),
            todo: fs::read(native.join("git-rebase-todo")).or_abort("selection todo"),
        }
    }
}

struct StatusDuringGate<'repository> {
    observed: Cell<bool>,
    repository: &'repository Repository,
}
impl Runner for InterruptedRunner {
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
        let matches = bin == "git"
            && match self.boundary {
                Boundary::ActorCleanup => args.starts_with(&["worktree", "remove"]),
                Boundary::ActorRegistered => args.starts_with(&["worktree", "add"]),
                Boundary::LeasePublished => {
                    args.contains(&"update-ref")
                        && args.contains(&"refs/factor/session-lease")
                        && !args.contains(&"-d")
                }
                Boundary::RebaseOpened => {
                    args.contains(&"rebase") && args.contains(&"--interactive")
                }
                Boundary::ReplayCompleted => args == ["rebase", "--continue"],
            };
        if matches && !self.fired.replace(true) {
            if !matches!(self.boundary, Boundary::ActorCleanup) {
                let status = REAL_RUNNER.status(bin, args, envs, quiet, cwd)?;
                if !status.success() {
                    return Err(io::Error::other(
                        "native mutation did not complete before interruption",
                    ));
                }
            }
            Err(io::Error::other("injected native mutation interruption"))
        } else {
            REAL_RUNNER.status(bin, args, envs, quiet, cwd)
        }
    }
}
impl Fs for JournalPublication {
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
        if path
            .file_name()
            .is_some_and(|name| name == "factor-journal.json")
        {
            let call = self
                .calls
                .get()
                .checked_add(1)
                .or_abort("bounded journal writes");
            self.calls.set(call);
            let journal: serde_json::Value =
                serde_json::from_str(content).or_abort("published journal JSON");
            let phase = journal
                .pointer("/state/phase")
                .and_then(serde_json::Value::as_str);
            let target = match self.target {
                PublicationTarget::CheckpointCapture => {
                    matches!(phase, Some("preparing" | "closing"))
                }
                PublicationTarget::InitialReplay => {
                    phase == Some("replaying")
                        && journal
                            .pointer("/state/accepted")
                            .and_then(serde_json::Value::as_array)
                            .is_some_and(Vec::is_empty)
                        && journal
                            .pointer("/state/remainder/state")
                            .and_then(serde_json::Value::as_str)
                            == Some("pending")
                }
                PublicationTarget::SecondWrite => call == 2,
            };
            if target {
                if self.after_replace {
                    REAL_FS.write_atomic_string(path, content)?;
                }
                return Err(io::Error::other(
                    "injected journal publication interruption",
                ));
            }
        }
        REAL_FS.write_atomic_string(path, content)
    }
    fn write_string(&self, path: &Path, content: &str) -> io::Result<()> {
        REAL_FS.write_string(path, content)
    }
}
impl Io for InterruptedOutput<'_> {
    fn err(&self, text: &str) -> io::Result<()> {
        self.capture.err(text)
    }
    fn errln(&self, line: &str) -> io::Result<()> {
        self.capture.errln(line)
    }
    fn out(&self, text: &str) -> io::Result<()> {
        let writes = self
            .writes
            .get()
            .checked_add(1)
            .or_abort("bounded output writes");
        self.writes.set(writes);
        if writes == self.at {
            Err(io::Error::other("injected result output interruption"))
        } else {
            self.capture.out(text)
        }
    }
    fn outln(&self, line: &str) -> io::Result<()> {
        self.out(line)?;
        self.out("\n")
    }
}
impl Runner for StatusDuringGate<'_> {
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
        if bin == "bash"
            && args == ["-c", "true"]
            && cwd != self.repository.environment.cwd
            && !self.observed.replace(true)
        {
            observe_live_status(self.repository, cwd);
        }
        REAL_RUNNER.status(bin, args, envs, quiet, cwd)
    }
}
impl Runner for ClosingInterruption {
    fn output(
        &self,
        bin: &str,
        args: &[&str],
        envs: &[(&str, Option<&str>)],
        cwd: &Path,
    ) -> io::Result<Output> {
        REAL_RUNNER.output(bin, args, envs, cwd)
    }
    #[expect(
        clippy::panic_in_result_fn,
        reason = "the owned test adapter refuses invalid fixture authority rather than converting assertion failure into product recovery"
    )]
    fn status(
        &self,
        bin: &str,
        args: &[&str],
        envs: &[(&str, Option<&str>)],
        quiet: bool,
        cwd: &Path,
    ) -> io::Result<ExitStatus> {
        let selected = matches!(self.boundary, ClosingBoundary::Lease)
            && bin == "git"
            && args.contains(&"update-ref")
            && args.contains(&"-d")
            && args.contains(&"refs/factor/session-lease");
        if selected && !self.fired.replace(true) {
            if self.after {
                let status = REAL_RUNNER.status(bin, args, envs, quiet, cwd)?;
                assert!(status.success());
            }
            return Err(io::Error::other("injected Closing lease interruption"));
        }
        REAL_RUNNER.status(bin, args, envs, quiet, cwd)
    }
}
impl Fs for ClosingInterruption {
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
        let selected = matches!(self.boundary, ClosingBoundary::Journal)
            && path
                .file_name()
                .is_some_and(|name| name == "factor-journal.json");
        if selected && !self.fired.replace(true) {
            if self.after {
                REAL_FS.remove_atomic_file(path)?;
            }
            return Err(io::Error::other(
                "injected Closing journal unlink uncertainty",
            ));
        }
        REAL_FS.remove_atomic_file(path)
    }
    fn remove_dir_all(&self, path: &Path) -> io::Result<()> {
        let selected = matches!(
            self.boundary,
            ClosingBoundary::Directory | ClosingBoundary::DirectoryPartial
        ) && path.file_name().is_some_and(|name| name == "factor");
        if selected && !self.fired.replace(true) {
            if matches!(self.boundary, ClosingBoundary::DirectoryPartial) {
                fs::remove_file(path.join("owned-partial-entry"))?;
            } else if self.after {
                REAL_FS.remove_dir_all(path)?;
            } else {
                // Before-removal refusal preserves the complete owned directory.
            }
            return Err(io::Error::other("injected Closing directory interruption"));
        }
        REAL_FS.remove_dir_all(path)
    }
    fn symlink_metadata(&self, path: &Path) -> io::Result<fs::Metadata> {
        REAL_FS.symlink_metadata(path)
    }
    fn write_atomic_string(&self, path: &Path, content: &str) -> io::Result<()> {
        REAL_FS.write_atomic_string(path, content)
    }
    fn write_string(&self, path: &Path, content: &str) -> io::Result<()> {
        REAL_FS.write_string(path, content)
    }
}
impl Fs for BirthInterruption {
    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        REAL_FS.canonicalize(path)
    }
    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        if path == self.scratch && matches!(self.boundary, BirthBoundary::ScratchCreation) {
            self.fired.set(true);
            *self.scratch_observation.borrow_mut() = Some(fs::read(&self.journal)?);
            return Err(io::Error::other(
                "injected published-journal scratch creation failure",
            ));
        }
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
        if path == self.journal {
            self.published
                .borrow_mut()
                .push(content.as_bytes().to_vec());
            if !matches!(self.boundary, BirthBoundary::ScratchCreation) && !self.fired.replace(true)
            {
                if matches!(self.boundary, BirthBoundary::AfterRename) {
                    REAL_FS.write_atomic_string(path, content)?;
                }
                return Err(io::Error::other(
                    "injected initial journal publication uncertainty",
                ));
            }
        }
        REAL_FS.write_atomic_string(path, content)
    }
    fn write_string(&self, path: &Path, content: &str) -> io::Result<()> {
        REAL_FS.write_string(path, content)
    }
}
impl Runner for ClosingLeaseQueryFailure {
    fn output(
        &self,
        bin: &str,
        args: &[&str],
        envs: &[(&str, Option<&str>)],
        cwd: &Path,
    ) -> io::Result<Output> {
        if bin == "git" && args == ["cat-file", "commit", self.lease.as_str()] {
            self.fired.set(true);
            return Err(io::Error::other(
                "injected current lease observation failure",
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

#[expect(
    clippy::assertions_on_result_states,
    reason = "the native refusal result is itself the independently observed contract boundary"
)]
#[expect(
    clippy::cognitive_complexity,
    reason = "the closed fixture boundary matrix keeps before/after native interruption and preservation assertions explicit"
)]
#[expect(
    clippy::literal_string_with_formatting_args,
    reason = "literal Git revision selectors and expected serialized data intentionally contain braces"
)]
#[expect(
    clippy::single_call_fn,
    reason = "named native fixture boundaries keep ownership, stimulus, and conservation visible to the direct canonical provider"
)]
#[expect(
    clippy::too_many_lines,
    reason = "one decisive native lifecycle keeps its exact interruption evidence and full preservation oracle together"
)]
pub(in crate::git_factor::engine::tests) fn birth_interruption(boundary: BirthBoundary) {
    let repository = initial_repository();
    let head = repository.git(&["rev-parse", "HEAD"]);
    let tree = repository.git(&["rev-parse", "HEAD^{tree}"]);
    let index = repository.index();
    let refs = repository.git(&["show-ref"]);
    let actors = repository.git(&["worktree", "list", "--porcelain"]);
    let journal_path = repository.environment.cwd.join(".git/factor-journal.json");
    let scratch = repository.environment.cwd.join(".git/factor");
    let foreign = repository.environment.cwd.join(".git/foreign-marker");
    let dangling = repository.environment.cwd.join(".git/foreign-dangling");
    fs::write(&foreign, b"foreign bytes\n").or_abort("foreign marker");
    symlink("missing-foreign-target", &dangling).or_abort("foreign dangling link");
    let filesystem = BirthInterruption {
        boundary,
        fired: Cell::new(false),
        journal: journal_path.clone(),
        published: RefCell::new(Vec::new()),
        scratch: scratch.clone(),
        scratch_observation: RefCell::new(None),
    };

    let (code, stdout) =
        repository.invoke_with(&["--exec", "true", "HEAD"], &REAL_RUNNER, &filesystem);

    assert_ne!(code, EXIT_OK);
    assert_eq!(stdout, "");
    assert!(
        filesystem.fired.get(),
        "intended first-publication boundary must fire"
    );
    assert_eq!(filesystem.published.borrow().len(), 1);
    assert_eq!(repository.git(&["rev-parse", "HEAD"]), head);
    assert_eq!(repository.git(&["rev-parse", "HEAD^{tree}"]), tree);
    assert_eq!(repository.index(), index);
    assert_eq!(repository.git(&["show-ref"]), refs);
    assert_eq!(repository.git(&["worktree", "list", "--porcelain"]), actors);
    assert_eq!(
        fs::read(&foreign).or_abort("foreign readback"),
        b"foreign bytes\n"
    );
    assert_eq!(
        fs::read_link(&dangling).or_abort("dangling readback"),
        Path::new("missing-foreign-target")
    );
    assert_eq!(
        fs::read(repository.environment.cwd.join("unrelated")).or_abort("user readback"),
        b"user bytes\n"
    );
    if matches!(boundary, BirthBoundary::BeforeRename) {
        assert!(fs::symlink_metadata(&journal_path).is_err());
        assert!(fs::symlink_metadata(&scratch).is_err());
        let observed = repository.success(&["--status"]);
        assert_eq!(observed.pointer("/session"), Some(&serde_json::Value::Null));
        assert!(fs::symlink_metadata(&journal_path).is_err());
        assert!(fs::symlink_metadata(&scratch).is_err());
        repository.success(&["--exec", "true", "HEAD"]);
    } else {
        let durable = repository.journal();
        assert_eq!(
            durable,
            *filesystem
                .published
                .borrow()
                .first()
                .or_abort("first write bytes")
        );
        let journal: serde_json::Value =
            serde_json::from_slice(&durable).or_abort("real Preparing journal");
        assert_eq!(
            journal
                .pointer("/state/phase")
                .and_then(serde_json::Value::as_str),
            Some("preparing")
        );
        assert_eq!(
            journal
                .pointer("/checkpoint")
                .and_then(serde_json::Value::as_str),
            Some(head.as_str())
        );
        assert_eq!(
            journal
                .pointer("/original_tip")
                .and_then(serde_json::Value::as_str),
            Some(head.as_str())
        );
        assert_eq!(
            journal
                .pointer("/final_tree")
                .and_then(serde_json::Value::as_str),
            Some(tree.as_str())
        );
        if matches!(boundary, BirthBoundary::ScratchCreation) {
            assert_eq!(
                *filesystem.scratch_observation.borrow(),
                Some(durable.clone())
            );
            assert!(fs::symlink_metadata(&scratch).is_err());
        }
        let observed = repository.success(&["--status"]);
        assert_eq!(
            observed
                .pointer("/session/phase")
                .and_then(serde_json::Value::as_str),
            Some("preparing")
        );
        assert_eq!(
            observed
                .pointer("/session/split_count")
                .and_then(serde_json::Value::as_u64),
            Some(0)
        );
        assert_eq!(repository.journal(), durable);
        assert_eq!(repository.index(), index);
        assert_eq!(repository.git(&["show-ref"]), refs);
        assert_eq!(repository.git(&["worktree", "list", "--porcelain"]), actors);
        repository.success(&["--continue"]);
    }
    assert_eq!(phase(&repository), "selecting");
    let command_hash = "f32a5804e292d30bedf68f62d32fb75d87e99fd9";
    let reference = format!("refs/factor/gates/{command_hash}/{tree}");
    let proof = repository.git(&["rev-parse", "--verify", &reference]);
    assert_eq!(
        repository.git(&["rev-parse", &format!("{proof}^{{tree}}")]),
        tree
    );
    assert_eq!(
        repository.git(&["show", "--format=%B", "--no-patch", &proof]),
        format!("Selected source\n\nGate-exec-{command_hash}:\n {command_hash}\n {tree}")
    );
    repository.success(&["--abort"]);
    assert!(!journal_path.exists());
    assert!(!scratch.exists());
    assert_eq!(repository.git(&["rev-parse", "HEAD"]), head);
    assert_eq!(repository.git(&["rev-parse", "HEAD^{tree}"]), tree);
    assert_eq!(
        repository.git(&["show-ref"]),
        format!("{proof} {reference}\n{refs}")
    );
    assert_eq!(
        fs::read(&foreign).or_abort("final foreign bytes"),
        b"foreign bytes\n"
    );
    assert_eq!(
        fs::read_link(&dangling).or_abort("final dangling bytes"),
        Path::new("missing-foreign-target")
    );
    assert_eq!(
        fs::read(repository.environment.cwd.join("unrelated")).or_abort("final user bytes"),
        b"user bytes\n"
    );
}

fn capture_filesystem(after_replace: bool) -> JournalPublication {
    JournalPublication {
        after_replace,
        calls: Cell::new(0),
        target: PublicationTarget::CheckpointCapture,
    }
}

/// The same native completed checkpoint survives either side of capture replacement.
#[expect(
    clippy::cognitive_complexity,
    reason = "the closed fixture boundary matrix keeps before/after native interruption and preservation assertions explicit"
)]
#[expect(
    clippy::single_call_fn,
    reason = "named native fixture boundaries keep ownership, stimulus, and conservation visible to the direct canonical provider"
)]
#[expect(
    clippy::too_many_lines,
    reason = "one decisive native lifecycle keeps its exact interruption evidence and full preservation oracle together"
)]
pub(in crate::git_factor::engine::tests) fn capture_publication(empty: bool, after_replace: bool) {
    let repository = verified_repository(empty);
    let checkpoint = repository.git(&["rev-parse", "refs/heads/main"]);
    let fixed_tree = repository.git(&["rev-parse", "HEAD^{tree}"]);
    let index = repository.index();
    let foreign = repository.git(&["rev-parse", "refs/heads/foreign"]);
    let preserved_refs = unrelated_refs(&repository);
    let old_bytes = repository.journal();
    let old: serde_json::Value = serde_json::from_slice(&old_bytes).or_abort("old journal");
    let filesystem = capture_filesystem(after_replace);
    let (code, stdout) = repository.invoke_with(&["--continue"], &REAL_RUNNER, &filesystem);
    assert_ne!(code, EXIT_OK);
    assert_eq!(stdout, "");
    assert_eq!(filesystem.calls.get(), 1);
    assert_eq!(unrelated_refs(&repository), preserved_refs);
    observe_completed(&repository, &checkpoint, &index, &foreign);
    let journal: serde_json::Value =
        serde_json::from_slice(&repository.journal()).or_abort("capture journal");
    assert_eq!(journal.pointer("/final_tree"), old.pointer("/final_tree"));
    assert_eq!(
        phase(&repository),
        if after_replace {
            if empty { "closing" } else { "preparing" }
        } else {
            "verified"
        }
    );
    if after_replace {
        assert_eq!(
            journal
                .pointer("/checkpoint")
                .and_then(serde_json::Value::as_str),
            Some(checkpoint.as_str())
        );
    } else {
        assert_eq!(repository.journal(), old_bytes);
    }
    observe_capture_status(&repository, empty, after_replace);
    repository.git(&["reflog", "expire", "--expire=now", "--all"]);
    repository.git(&["prune", "--expire=now"]);
    let resumed = repository.success(&["--continue"]);
    if !empty {
        assert_eq!(phase(&repository), "selecting", "resumed JSON: {resumed}");
        assert_eq!(
            resumed
                .pointer("/operation")
                .and_then(serde_json::Value::as_str),
            Some("continue"),
            "resumed JSON: {resumed}"
        );
        assert_eq!(
            resumed
                .pointer("/result")
                .and_then(serde_json::Value::as_str),
            Some("committed"),
            "resumed JSON: {resumed}"
        );
        assert_eq!(
            resumed
                .pointer("/split_count")
                .and_then(serde_json::Value::as_u64),
            Some(1),
            "resumed JSON: {resumed}"
        );
        let unstaged: serde_json::Value =
            serde_json::from_str("[]").or_abort("empty tracked changes");
        let untracked: serde_json::Value = serde_json::from_str(r#"["remainder","unrelated"]"#)
            .or_abort("exact text-path changes");
        assert_eq!(
            resumed.pointer("/changes/unstaged"),
            Some(&unstaged),
            "resumed JSON: {resumed}"
        );
        assert_eq!(
            resumed.pointer("/changes/untracked"),
            Some(&untracked),
            "resumed JSON: {resumed}"
        );
        assert_eq!(
            repository.git(&["diff", "--name-only"]),
            "",
            "resumed JSON: {resumed}"
        );
        assert_eq!(
            repository.git(&["diff", "--cached", "--name-only"]),
            "",
            "resumed JSON: {resumed}"
        );
        assert_eq!(
            repository.git(&["ls-files", "--others", "--exclude-standard"]),
            "remainder\nunrelated",
            "resumed JSON: {resumed}"
        );
        assert_eq!(
            fs::read(repository.environment.cwd.join("remainder"))
                .or_abort("remaining source bytes"),
            b"remainder\n",
            "resumed JSON: {resumed}"
        );
        assert_eq!(
            repository.git(&["ls-files", "--", "atom"]),
            "atom",
            "resumed JSON: {resumed}"
        );
        assert_eq!(
            repository.git(&["show", "HEAD:atom"]),
            "atom",
            "resumed JSON: {resumed}"
        );
        repository.success(&["--retry"]);
        repository.success(&["--abort"]);
    }
    let completed_index = repository.index();
    observe_completed(&repository, &checkpoint, &completed_index, &foreign);
    assert_eq!(unrelated_refs(&repository), preserved_refs);
    assert_eq!(repository.git(&["write-tree"]), fixed_tree);
    assert_eq!(repository.git(&["rev-parse", "HEAD^{tree}"]), fixed_tree);
}

#[expect(
    clippy::cognitive_complexity,
    reason = "the closed fixture boundary matrix keeps before/after native interruption and preservation assertions explicit"
)]
#[expect(
    clippy::literal_string_with_formatting_args,
    reason = "literal Git revision selectors and expected serialized data intentionally contain braces"
)]
#[expect(
    clippy::too_many_lines,
    reason = "one decisive native lifecycle keeps its exact interruption evidence and full preservation oracle together"
)]
pub(in crate::git_factor::engine::tests) fn closing_interruption(
    boundary: ClosingBoundary,
    after: bool,
    abort: bool,
) {
    let repository = verified_repository(true);
    let capture = capture_filesystem(true);
    let (code, _) = repository.invoke_with(&["--continue"], &REAL_RUNNER, &capture);
    assert_ne!(code, EXIT_OK);
    assert_eq!(phase(&repository), "closing");
    let checkpoint = repository.git(&["rev-parse", "HEAD"]);
    let tree = repository.git(&["rev-parse", "HEAD^{tree}"]);
    let index = repository.index();
    let foreign = repository.git(&["rev-parse", "refs/heads/foreign"]);
    let journal = repository.journal();
    let preserved_refs = unrelated_refs(&repository);
    let stored: serde_json::Value = serde_json::from_slice(&journal).or_abort("Closing journal");
    let original = stored
        .pointer("/original_tip")
        .and_then(serde_json::Value::as_str)
        .or_abort("original tip");
    let scratch = repository.environment.cwd.join(".git/factor");
    if matches!(boundary, ClosingBoundary::DirectoryPartial) {
        fs::write(
            scratch.join("owned-partial-entry"),
            b"owned removed scratch bytes\n",
        )
        .or_abort("partial-removal entry");
        fs::write(
            scratch.join("owned-partial-survivor"),
            b"owned surviving scratch bytes\n",
        )
        .or_abort("partial-removal survivor");
    }
    let interruption = ClosingInterruption {
        boundary,
        after,
        fired: Cell::new(false),
    };
    let (interrupted_code, stdout) =
        repository.invoke_with(&["--continue"], &interruption, &interruption);
    assert_ne!(interrupted_code, EXIT_OK);
    assert_eq!(stdout, "");
    assert!(interruption.fired.get());
    if (matches!(boundary, ClosingBoundary::Directory) && !after)
        || matches!(boundary, ClosingBoundary::DirectoryPartial)
    {
        let diagnostic = fs::read_to_string(scratch.join("error.log"))
            .or_abort("unexpected admitted Closing failure diagnostic");
        let fields = diagnostic
            .lines()
            .filter(|line| line.starts_with("argv=") || line.starts_with("error="))
            .collect::<Vec<_>>();
        assert_eq!(
            fields,
            [
                "argv=git-factor --continue",
                "error=failed to write state: injected Closing directory interruption",
            ]
        );
    }
    if matches!(boundary, ClosingBoundary::DirectoryPartial) {
        assert!(!scratch.join("owned-partial-entry").exists());
        assert_eq!(
            fs::read(scratch.join("owned-partial-survivor")).or_abort("partial scratch readback"),
            b"owned surviving scratch bytes\n"
        );
    }
    if matches!(boundary, ClosingBoundary::Directory) && after {
        assert!(!scratch.exists());
        assert_eq!(repository.journal(), journal);
    }
    assert_eq!(unrelated_refs(&repository), preserved_refs);
    observe_completed(&repository, &checkpoint, &index, &foreign);
    let removed = matches!(boundary, ClosingBoundary::Journal) && after;
    let refs = repository.git(&["show-ref"]);
    assert_eq!(
        refs.contains("refs/factor/session-lease"),
        matches!(boundary, ClosingBoundary::Lease) && !after
    );
    repository.git(&["reflog", "expire", "--expire=now", "--all"]);
    repository.git(&["prune", "--expire=now"]);
    if !matches!(boundary, ClosingBoundary::Lease) || after {
        let object = REAL_RUNNER
            .output(
                "git",
                &["cat-file", "-e", original],
                &[],
                &repository.environment.cwd,
            )
            .or_abort("pruned original observation");
        assert!(
            !object.status.success(),
            "Closing must survive actual original-tip collection"
        );
    }
    if !removed {
        assert_eq!(repository.journal(), journal);
        let observed = repository.success(&["--status"]);
        assert_eq!(
            observed,
            serde_json::from_str::<serde_json::Value>(&format!(
                r#"{{"operation":"status","session":{{"checkpoint":{checkpoint_json},"outcome":"complete","phase":"closing","rebase":{{"in_progress":false,"required":false}},"split_count":1,"target":{{"commit":{original_json},"span_starts_at_root":false}}}}}}"#,
                checkpoint_json = serde_json::to_string(&checkpoint).or_abort("expected checkpoint JSON"),
                original_json = serde_json::to_string(&original).or_abort("expected original JSON"),
            )).or_abort("literal Closing status JSON")
        );
        assert_eq!(repository.journal(), journal);
        assert_eq!(repository.git(&["show-ref"]), refs);
        observe_completed(&repository, &checkpoint, &index, &foreign);
        let completed = repository.success(&[if abort { "--abort" } else { "--continue" }]);
        assert_eq!(
            completed
                .pointer("/result")
                .and_then(serde_json::Value::as_str),
            Some("complete")
        );
        assert_eq!(
            completed
                .pointer("/split_count")
                .and_then(serde_json::Value::as_u64),
            Some(1)
        );
        assert_eq!(
            completed
                .pointer("/operation")
                .and_then(serde_json::Value::as_str),
            Some(if abort { "abort" } else { "continue" })
        );
    }
    assert!(
        !repository
            .environment
            .cwd
            .join(".git/factor-journal.json")
            .exists()
    );
    assert!(!repository.environment.cwd.join(".git/factor").exists());
    observe_completed(&repository, &checkpoint, &index, &foreign);
    assert_eq!(unrelated_refs(&repository), preserved_refs);
    assert_eq!(repository.git(&["rev-parse", "HEAD^{tree}"]), tree);
}

fn observe_aborted_bytes(repository: &Repository) {
    for (path, expected) in [
        ("base", b"base\n".as_slice()),
        ("atom", b"atom\n".as_slice()),
        ("remainder", b"remainder\n".as_slice()),
        ("unrelated", b"preserved abort bytes\n".as_slice()),
    ] {
        assert_eq!(
            fs::read(repository.environment.cwd.join(path))
                .or_abort("aborted checkpoint and protected user bytes"),
            expected
        );
    }
}

/// An aborted outcome remains recoverable after uncertain journal unlink.
#[expect(
    clippy::single_call_fn,
    reason = "the canonical run provider owns this actual Aborted Closing lifecycle scenario"
)]
pub(in crate::git_factor::engine::tests) fn aborted_closing_unlink() {
    let repository = open_selection();
    repository.git(&["add", "atom"]);
    repository.success(&["--message", "Extract captured atom"]);
    let captured = repository.git(&["rev-parse", "refs/heads/main"]);
    let final_tree = repository.git(&["rev-parse", "refs/heads/main^{tree}"]);
    let atom = repository.git(&["rev-parse", "HEAD"]);
    repository.git(&["branch", "foreign", &captured]);
    repository.git(&["tag", "foreign", &captured]);
    repository.write("unrelated", "preserved abort bytes\n");
    let protected_refs = unrelated_refs(&repository);
    let interruption = ClosingInterruption {
        after: false,
        boundary: ClosingBoundary::Journal,
        fired: Cell::new(false),
    };

    let (code, stdout) = repository.invoke_with(&["--abort"], &interruption, &interruption);

    let index = repository.index();
    assert_eq!(code, EXIT_SOFTWARE);
    assert_eq!(stdout, "");
    assert_eq!(
        *repository.output.stderr.borrow(),
        "failed to write state: injected Closing journal unlink uncertainty\n"
    );
    assert!(interruption.fired.get());
    assert_eq!(repository.git(&["rev-parse", "HEAD"]), captured);
    assert_eq!(repository.git(&["rev-parse", "HEAD^{tree}"]), final_tree);
    assert_eq!(repository.git(&["symbolic-ref", "HEAD"]), "refs/heads/main");
    assert_eq!(repository.git(&["diff", "--cached", &captured]), "");
    assert_eq!(unrelated_refs(&repository), protected_refs);
    assert!(
        !repository
            .environment
            .cwd
            .join(".git/rebase-merge")
            .exists()
    );
    let bytes = repository.journal();
    let stored: serde_json::Value =
        serde_json::from_slice(&bytes).or_abort("actual Aborted Closing journal");
    assert_eq!(
        stored
            .pointer("/state/phase")
            .and_then(serde_json::Value::as_str),
        Some("closing")
    );
    assert_eq!(
        stored
            .pointer("/state/outcome/result")
            .and_then(serde_json::Value::as_str),
        Some("aborted")
    );
    assert_eq!(
        stored
            .pointer("/state/outcome/base")
            .and_then(serde_json::Value::as_str),
        Some(atom.as_str())
    );
    assert_eq!(
        stored
            .pointer("/checkpoint")
            .and_then(serde_json::Value::as_str),
        Some(captured.as_str())
    );
    let references = repository.git(&["show-ref"]);
    assert!(!references.contains(" refs/factor/session-lease"));
    observe_aborted_bytes(&repository);

    let resumed = repository.success(&["--continue"]);

    assert_eq!(repository.index(), index);
    assert_eq!(
        resumed,
        serde_json::from_str::<serde_json::Value>(
            r#"{"operation":"abort","actions":{},"rebase":{"in_progress":false}}"#
        )
        .or_abort("expected aborted response")
    );
    assert_eq!(*repository.output.stderr.borrow(), "");
    assert_eq!(repository.git(&["rev-parse", "HEAD"]), captured);
    assert_eq!(repository.git(&["rev-parse", "HEAD^{tree}"]), final_tree);
    assert_eq!(repository.git(&["show-ref"]), references);
    assert!(
        !repository
            .environment
            .cwd
            .join(".git/factor-journal.json")
            .exists()
    );
    assert!(!repository.environment.cwd.join(".git/factor").exists());
    observe_aborted_bytes(&repository);
}

/// A released Closing lease can be reacquired by another real linked-worktree session.
#[expect(
    clippy::cognitive_complexity,
    reason = "the closed fixture boundary matrix keeps before/after native interruption and preservation assertions explicit"
)]
#[expect(
    clippy::too_many_lines,
    reason = "one decisive native lifecycle keeps its exact interruption evidence and full preservation oracle together"
)]
pub(in crate::git_factor::engine::tests) fn closing_preserves_reacquired_lease(abort: bool) {
    let repository = verified_repository(true);
    let capture = capture_filesystem(true);
    let (code, _) = repository.invoke_with(&["--continue"], &REAL_RUNNER, &capture);
    assert_ne!(code, EXIT_OK);
    assert_eq!(phase(&repository), "closing");
    let original_journal = repository.journal();
    let stored: serde_json::Value =
        serde_json::from_slice(&original_journal).or_abort("actual Closing journal");
    let original = stored
        .pointer("/original_tip")
        .and_then(serde_json::Value::as_str)
        .or_abort("original tip");
    let interruption = ClosingInterruption {
        boundary: ClosingBoundary::Lease,
        after: true,
        fired: Cell::new(false),
    };
    let (interrupted_code, stdout) =
        repository.invoke_with(&["--continue"], &interruption, &interruption);
    assert_ne!(interrupted_code, EXIT_OK);
    assert_eq!(stdout, "");
    assert!(interruption.fired.get());
    assert!(
        !repository
            .git(&["show-ref"])
            .contains("refs/factor/session-lease")
    );
    assert_eq!(repository.journal(), original_journal);
    let checkpoint = repository.git(&["rev-parse", "HEAD"]);
    let checkpoint_tree = repository.git(&["rev-parse", "HEAD^{tree}"]);
    let index = repository.index();

    let linked = Repository::new();
    fs::remove_dir_all(&linked.environment.cwd).or_abort("owned unused fixture repository");
    let linked_path = linked
        .environment
        .cwd
        .to_str()
        .or_abort("linked path UTF-8");
    repository.git(&[
        "worktree",
        "add",
        "--quiet",
        "-b",
        "linked-session",
        linked_path,
        &checkpoint,
    ]);
    linked.write("linked-change", "linked session bytes\n");
    linked.commit("Linked selected change");
    linked.success(&["--exec", "true", "HEAD"]);
    let linked_gitdir = PathBuf::from(linked.git(&["rev-parse", "--absolute-git-dir"]));
    let linked_journal_path = linked_gitdir.join("factor-journal.json");
    let linked_journal = fs::read(&linked_journal_path).or_abort("linked actual journal");
    let linked_state: serde_json::Value =
        serde_json::from_slice(&linked_journal).or_abort("linked journal JSON");
    assert_eq!(
        linked_state
            .pointer("/state/phase")
            .and_then(serde_json::Value::as_str),
        Some("selecting")
    );
    let linked_index = fs::read(linked_gitdir.join("index")).or_abort("linked raw index");
    let linked_head = linked.git(&["rev-parse", "HEAD"]);
    let linked_tree = linked.git(&["ls-files", "--stage"]);
    let linked_diff = linked.git(&["diff", "--binary"]);
    let linked_untracked = linked.git(&["ls-files", "--others", "--exclude-standard"]);
    #[expect(
        clippy::filetype_is_file,
        reason = "this native metadata oracle must require regular files, refusing FIFO, socket, and device entries before reading"
    )]
    let observe_native_files = || {
        let mut files = fs::read_dir(linked_gitdir.join("rebase-merge"))
            .or_abort("B native recovery directory")
            .map(|entry| {
                let recovery_entry = entry.or_abort("B native recovery entry");
                let file_type = recovery_entry.file_type().or_abort("B native file type");
                assert!(file_type.is_file());
                (
                    recovery_entry.file_name(),
                    fs::read(recovery_entry.path()).or_abort("B native recovery bytes"),
                )
            })
            .collect::<Vec<_>>();
        files.sort();
        files
    };
    let linked_native = observe_native_files();
    let lease = repository.git(&["rev-parse", "refs/factor/session-lease"]);
    let capsule = repository.git(&["cat-file", "commit", &lease]);
    let refs = repository.git(&["show-ref"]);
    let registrations = repository.git(&["worktree", "list", "--porcelain"]);
    let observe_linked = || {
        assert_eq!(repository.git(&["cat-file", "commit", &lease]), capsule);
        assert_eq!(
            fs::read(&linked_journal_path).or_abort("preserved B journal"),
            linked_journal
        );
        assert_eq!(
            fs::read(linked_gitdir.join("index")).or_abort("preserved B index"),
            linked_index
        );
        assert_eq!(linked.git(&["rev-parse", "HEAD"]), linked_head);
        assert_eq!(linked.git(&["ls-files", "--stage"]), linked_tree);
        assert_eq!(linked.git(&["diff", "--binary"]), linked_diff);
        assert_eq!(
            linked.git(&["ls-files", "--others", "--exclude-standard"]),
            linked_untracked
        );
        assert_eq!(observe_native_files(), linked_native);
    };
    repository.git(&["reflog", "expire", "--expire=now", "--all"]);
    repository.git(&["prune", "--expire=now"]);
    let collected = REAL_RUNNER
        .output(
            "git",
            &["cat-file", "-e", original],
            &[],
            &repository.environment.cwd,
        )
        .or_abort("original collection observation");
    assert!(
        !collected.status.success(),
        "A Closing must survive original-tip collection"
    );

    let observed = repository.success(&["--status"]);

    assert_eq!(
        observed,
        serde_json::from_str::<serde_json::Value>(&format!(
                r#"{{"operation":"status","session":{{"checkpoint":{checkpoint_json},"outcome":"complete","phase":"closing","rebase":{{"in_progress":false,"required":false}},"split_count":1,"target":{{"commit":{original_json},"span_starts_at_root":false}}}}}}"#,
                checkpoint_json = serde_json::to_string(&checkpoint).or_abort("expected checkpoint JSON"),
                original_json = serde_json::to_string(&original).or_abort("expected original JSON"),
            )).or_abort("literal Closing status JSON")
    );
    assert_eq!(repository.journal(), original_journal);
    assert_eq!(repository.index(), index);
    assert_eq!(repository.git(&["show-ref"]), refs);
    assert_eq!(
        repository.git(&["worktree", "list", "--porcelain"]),
        registrations
    );
    observe_linked();
    let completed = repository.success(&[if abort { "--abort" } else { "--continue" }]);
    assert_eq!(
        completed
            .pointer("/result")
            .and_then(serde_json::Value::as_str),
        Some("complete")
    );
    assert_eq!(
        completed
            .pointer("/split_count")
            .and_then(serde_json::Value::as_u64),
        Some(1)
    );
    assert_eq!(
        completed
            .pointer("/operation")
            .and_then(serde_json::Value::as_str),
        Some(if abort { "abort" } else { "continue" })
    );
    assert!(
        !repository
            .environment
            .cwd
            .join(".git/factor-journal.json")
            .exists()
    );
    assert!(!repository.environment.cwd.join(".git/factor").exists());
    assert_eq!(repository.git(&["rev-parse", "HEAD"]), checkpoint);
    assert_eq!(
        repository.git(&["rev-parse", "HEAD^{tree}"]),
        checkpoint_tree
    );
    assert_eq!(repository.index(), index);
    assert_eq!(repository.git(&["show-ref"]), refs);
    assert_eq!(
        repository.git(&["worktree", "list", "--porcelain"]),
        registrations
    );
    observe_linked();
    assert_eq!(
        fs::read(repository.environment.cwd.join("unrelated")).or_abort("A user bytes"),
        b"user bytes\n"
    );
    assert_eq!(
        fs::read(linked.environment.cwd.join("linked-change")).or_abort("B change bytes"),
        b"linked session bytes\n"
    );
    linked.success(&["--abort"]);
}

/// Unknown native lease objects and failed observations never authorize deletion.
#[expect(
    clippy::literal_string_with_formatting_args,
    reason = "literal Git revision selectors and expected serialized data intentionally contain braces"
)]
#[expect(
    clippy::single_call_fn,
    reason = "named native fixture boundaries keep ownership, stimulus, and conservation visible to the direct canonical provider"
)]
pub(in crate::git_factor::engine::tests) fn closing_refuses_unknown_lease(
    query_failure: bool,
    operation: &str,
) {
    let repository = verified_repository(true);
    let capture = capture_filesystem(true);
    let (code, _) = repository.invoke_with(&["--continue"], &REAL_RUNNER, &capture);
    assert_ne!(code, EXIT_OK);
    assert_eq!(phase(&repository), "closing");
    if !query_failure {
        let foreign = repository.git(&["rev-parse", "HEAD"]);
        repository.git(&["update-ref", "refs/factor/session-lease", &foreign]);
    }
    let lease = repository.git(&["rev-parse", "refs/factor/session-lease"]);
    let capsule = repository.git(&["cat-file", "commit", &lease]);
    let runner = ClosingLeaseQueryFailure {
        fired: Cell::new(false),
        lease,
    };
    let journal = repository.journal();
    let head = repository.git(&["rev-parse", "HEAD"]);
    let tree = repository.git(&["rev-parse", "HEAD^{tree}"]);
    let index = repository.index();
    let refs = repository.git(&["show-ref"]);
    let actors = repository.git(&["worktree", "list", "--porcelain"]);
    let actual_runner: &dyn Runner = if query_failure { &runner } else { &REAL_RUNNER };

    let (refusal_code, stdout) = repository.invoke_with(&[operation], actual_runner, &REAL_FS);

    assert_ne!(refusal_code, EXIT_OK);
    assert_eq!(stdout, "");
    assert_eq!(runner.fired.get(), query_failure);
    assert_eq!(repository.journal(), journal);
    assert_eq!(repository.git(&["rev-parse", "HEAD"]), head);
    assert_eq!(repository.git(&["rev-parse", "HEAD^{tree}"]), tree);
    assert_eq!(repository.index(), index);
    assert_eq!(repository.git(&["show-ref"]), refs);
    assert_eq!(repository.git(&["worktree", "list", "--porcelain"]), actors);
    assert_eq!(
        repository.git(&["cat-file", "commit", &runner.lease]),
        capsule
    );
    assert_eq!(
        fs::read(repository.environment.cwd.join("unrelated")).or_abort("user bytes"),
        b"user bytes\n"
    );
}
/// A successful candidate continues after a simultaneous pure status observation.
#[expect(
    clippy::single_call_fn,
    reason = "named native fixture boundaries keep ownership, stimulus, and conservation visible to the direct canonical provider"
)]
pub(in crate::git_factor::engine::tests) fn concurrent_status() {
    let repository = open_selection();
    repository.git(&["add", "atom"]);
    let runner = StatusDuringGate {
        observed: Cell::new(false),
        repository: &repository,
    };
    let (code, stdout) = repository.invoke_with(
        &["--message", "Extract with concurrent observer"],
        &runner,
        &REAL_FS,
    );
    assert_eq!(code, EXIT_OK, "{}", repository.output.stderr.borrow());
    assert!(runner.observed.get(), "actual actor gate must be reached");
    let response: serde_json::Value =
        serde_json::from_str(&stdout).or_abort("captured checkpoint result");
    assert_eq!(
        response.pointer("/result").or_abort("checkpoint result"),
        "committed"
    );
    repository.success(&["--abort"]);
}

/// Refuses a pinned descendant masquerading as the original selected source.
#[expect(
    clippy::single_call_fn,
    reason = "named native fixture boundaries keep ownership, stimulus, and conservation visible to the direct canonical provider"
)]
pub(in crate::git_factor::engine::tests) fn corrupt_preparation() {
    let repository = initial_repository();
    let selected = repository.git(&["rev-parse", "HEAD"]);
    repository.write("descendant", "later tree\n");
    repository.commit("Later descendant");
    let checkpoint = repository.git(&["rev-parse", "HEAD"]);
    let filesystem = JournalPublication {
        calls: Cell::new(0),
        after_replace: false,
        target: PublicationTarget::SecondWrite,
    };
    let (initial_code, _) =
        repository.invoke_with(&["--exec", "true", &selected], &REAL_RUNNER, &filesystem);
    assert_ne!(initial_code, EXIT_OK);
    assert_eq!(phase(&repository), "preparing");
    let original = repository.journal();
    let mut corrupt: serde_json::Value =
        serde_json::from_slice(&original).or_abort("actual journal");
    *corrupt
        .pointer_mut("/state/tip")
        .or_abort("required JSON fact") = serde_json::Value::String(checkpoint.clone());
    let path = repository.environment.cwd.join(".git/factor-journal.json");
    fs::write(
        &path,
        serde_json::to_vec(&corrupt).or_abort("corrupt journal fixture"),
    )
    .or_abort("publish fixture");
    let journal = repository.journal();
    let index = repository.index();
    let lease = repository.git(&["rev-parse", "refs/factor/session-lease"]);
    let (code, stdout) = repository.invoke(&["--continue"]);
    assert_ne!(code, EXIT_OK);
    assert_eq!(stdout, "");
    assert!(
        repository
            .output
            .stderr
            .borrow()
            .contains("original selected range")
    );
    assert_eq!(repository.journal(), journal);
    assert_eq!(repository.index(), index);
    assert_eq!(repository.git(&["rev-parse", "HEAD"]), checkpoint);
    assert_eq!(
        repository.git(&["rev-parse", "refs/heads/main"]),
        checkpoint
    );
    assert_eq!(
        repository.git(&["rev-parse", "refs/factor/session-lease"]),
        lease
    );
    fs::write(path, original).or_abort("restore authoritative journal");
    repository.success(&["--abort"]);
}
fn finish_recovery(repository: &Repository) {
    repository.git(&["reflog", "expire", "--expire=now", "--all"]);
    repository.git(&["prune", "--expire=now"]);
    repository.success(&["--continue"]);
    assert_eq!(phase(repository), "selecting");
    repository.success(&["--abort"]);
    assert_eq!(
        fs::read(repository.environment.cwd.join("unrelated"))
            .or_abort("preserved unrelated bytes"),
        b"user bytes\n"
    );
    assert_eq!(
        repository
            .git(&["worktree", "list", "--porcelain"])
            .lines()
            .filter(|line| line.starts_with("worktree "))
            .count(),
        1
    );
}
fn initial_repository() -> Repository {
    let repository = Repository::new();
    repository.write("base", "base\n");
    repository.commit("Base");
    repository.write("atom", "atom\n");
    repository.write("remainder", "remainder\n");
    repository.commit("Selected source");
    repository.write("unrelated", "user bytes\n");
    repository
}

pub(in crate::git_factor::engine::tests) fn journal_publication(after_replace: bool) {
    let repository = initial_repository();
    let head = repository.git(&["rev-parse", "HEAD"]);
    let index = repository.index();
    let filesystem = JournalPublication {
        calls: Cell::new(0),
        after_replace,
        target: PublicationTarget::SecondWrite,
    };
    let (code, stdout) =
        repository.invoke_with(&["--exec", "true", "HEAD"], &REAL_RUNNER, &filesystem);
    assert_ne!(code, EXIT_OK);
    assert_eq!(stdout, "");
    assert!(
        repository
            .output
            .stderr
            .borrow()
            .contains("injected journal publication interruption")
    );
    assert_eq!(filesystem.calls.get(), 2);
    assert_eq!(
        phase(&repository),
        if after_replace {
            "opening"
        } else {
            "preparing"
        }
    );
    assert_eq!(repository.git(&["rev-parse", "HEAD"]), head);
    assert_eq!(repository.index(), index);
    finish_recovery(&repository);
}

#[expect(
    clippy::create_dir,
    reason = "exclusive fixture directories must refuse existing occupancy rather than silently admit unrelated files"
)]
#[expect(
    clippy::single_call_fn,
    reason = "named native fixture boundaries keep ownership, stimulus, and conservation visible to the direct canonical provider"
)]
pub(in crate::git_factor::engine::tests) fn legacy_start_refusal(boundary: LegacyBoundary) {
    let repository = initial_repository();
    let legacy = repository.environment.cwd.join(".git/factor");
    match boundary {
        LegacyBoundary::DanglingLink => {
            symlink("missing-legacy-target", &legacy).or_abort("legacy dangling link");
        }
        LegacyBoundary::Directory => {
            fs::create_dir(&legacy).or_abort("legacy directory");
            fs::write(legacy.join("foreign"), b"legacy foreign bytes\n").or_abort("legacy bytes");
        }
        LegacyBoundary::File => fs::write(&legacy, b"legacy file bytes\n").or_abort("legacy file"),
    }
    let head = repository.git(&["rev-parse", "HEAD"]);
    let index = repository.index();
    let refs = repository.git(&["show-ref"]);
    let actors = repository.git(&["worktree", "list", "--porcelain"]);

    let (code, stdout) = repository.invoke(&["--exec", "true", "HEAD"]);

    assert_ne!(code, EXIT_OK);
    assert_eq!(stdout, "");
    assert!(repository.output.stderr.borrow().contains(
        "existing legacy or active session must be finished or aborted with its originating version"
    ));
    assert!(
        !repository
            .environment
            .cwd
            .join(".git/factor-journal.json")
            .exists()
    );
    assert_eq!(repository.git(&["rev-parse", "HEAD"]), head);
    assert_eq!(repository.index(), index);
    assert_eq!(repository.git(&["show-ref"]), refs);
    assert_eq!(repository.git(&["worktree", "list", "--porcelain"]), actors);
    match boundary {
        LegacyBoundary::DanglingLink => assert_eq!(
            fs::read_link(&legacy).or_abort("legacy link readback"),
            Path::new("missing-legacy-target")
        ),
        LegacyBoundary::Directory => assert_eq!(
            fs::read(legacy.join("foreign")).or_abort("legacy directory readback"),
            b"legacy foreign bytes\n"
        ),
        LegacyBoundary::File => assert_eq!(
            fs::read(&legacy).or_abort("legacy file readback"),
            b"legacy file bytes\n"
        ),
    }
    assert_eq!(
        fs::read(repository.environment.cwd.join("unrelated")).or_abort("user bytes"),
        b"user bytes\n"
    );
}

/// Public status observes the durable capture phase without promoting terminal intent.
#[expect(
    clippy::single_call_fn,
    reason = "named native fixture boundaries keep ownership, stimulus, and conservation visible to the direct canonical provider"
)]
fn observe_capture_status(repository: &Repository, empty: bool, after_replace: bool) {
    let durable_bytes = repository.journal();
    let journal: serde_json::Value =
        serde_json::from_slice(&durable_bytes).or_abort("durable capture journal");
    let checkpoint = repository.git(&["rev-parse", "HEAD"]);
    let fixed_tree = repository.git(&["rev-parse", "HEAD^{tree}"]);
    let index = repository.index();
    let foreign = repository.git(&["rev-parse", "refs/heads/foreign"]);
    let refs = repository.git(&["show-ref"]);
    let actors = repository.git(&["worktree", "list", "--porcelain"]);
    let status = repository.success(&["--status"]);
    assert_eq!(
        status
            .pointer("/operation")
            .and_then(serde_json::Value::as_str),
        Some("status")
    );
    assert_eq!(
        status
            .pointer("/session/phase")
            .and_then(serde_json::Value::as_str),
        Some(phase(repository).as_str())
    );
    assert_eq!(
        status.pointer("/session/checkpoint"),
        journal.pointer("/checkpoint")
    );
    assert_eq!(
        status.pointer("/session/target/commit"),
        journal.pointer("/original_tip")
    );
    assert_eq!(
        status
            .pointer("/session/target/span_starts_at_root")
            .and_then(serde_json::Value::as_bool),
        Some(false)
    );
    assert_eq!(
        status
            .pointer("/session/split_count")
            .and_then(serde_json::Value::as_u64),
        Some(u64::from(after_replace))
    );
    assert_eq!(
        status
            .pointer("/session/rebase/in_progress")
            .and_then(serde_json::Value::as_bool),
        Some(false)
    );
    assert_eq!(
        status
            .pointer("/session/rebase/required")
            .and_then(serde_json::Value::as_bool),
        Some(!after_replace)
    );
    if empty && after_replace {
        assert_eq!(
            status
                .pointer("/session/outcome")
                .and_then(serde_json::Value::as_str),
            Some("complete")
        );
    } else {
        assert_eq!(
            status
                .pointer("/session/target/commit_count")
                .and_then(serde_json::Value::as_u64),
            Some(1)
        );
    }
    assert_eq!(repository.journal(), durable_bytes);
    assert_eq!(repository.git(&["show-ref"]), refs);
    assert_eq!(repository.git(&["worktree", "list", "--porcelain"]), actors);
    assert_eq!(repository.git(&["rev-parse", "HEAD^{tree}"]), fixed_tree);
    observe_completed(repository, &checkpoint, &index, &foreign);
}

fn observe_completed(repository: &Repository, checkpoint: &str, index: &[u8], foreign: &str) {
    assert_eq!(repository.git(&["rev-parse", "HEAD"]), checkpoint);
    assert_eq!(
        repository.git(&["rev-parse", "refs/heads/main"]),
        checkpoint
    );
    assert_eq!(
        repository.git(&["rev-parse", "refs/heads/foreign"]),
        foreign
    );
    assert_eq!(repository.index(), index);
    assert!(
        !repository
            .environment
            .cwd
            .join(".git/rebase-merge")
            .exists()
    );
    assert_eq!(
        fs::read(repository.environment.cwd.join("unrelated")).or_abort("user bytes"),
        b"user bytes\n"
    );
    let registrations = repository.git(&["worktree", "list", "--porcelain"]);
    assert_eq!(
        registrations
            .lines()
            .filter(|line| line.starts_with("worktree "))
            .count(),
        1
    );
}
/// Queries the public observer while an admitted normal gate owns its native actor.
#[expect(
    clippy::single_call_fn,
    reason = "this observer runs at the native gate boundary"
)]
fn observe_live_status(repository: &Repository, actor: &Path) {
    let index = repository.index();
    let journal = repository.journal();
    let head = repository.git(&["rev-parse", "HEAD"]);
    let registrations = repository.git(&["worktree", "list", "--porcelain"]);
    assert!(actor.exists());
    let output = Capture::default();
    let context = super::Ctx {
        cwd: repository.environment.cwd.clone(),
        env: &repository.environment,
        fs: &REAL_FS,
        io: &output,
        runner: &REAL_RUNNER,
    };
    let args = [OsString::from("git-factor"), OsString::from("--status")];
    assert_eq!(
        super::main_entry_with_vec(context.io, Ok(context), &args),
        EXIT_OK
    );
    let response: serde_json::Value =
        serde_json::from_str(&output.stdout.borrow()).or_abort("independent status JSON");
    assert_eq!(
        response
            .pointer("/session/phase")
            .or_abort("observed phase"),
        "selecting"
    );
    assert!(
        actor.exists(),
        "read-only status must preserve the running actor"
    );
    assert_eq!(
        repository.git(&["worktree", "list", "--porcelain"]),
        registrations
    );
    assert_eq!(repository.index(), index);
    assert_eq!(repository.journal(), journal);
    assert_eq!(repository.git(&["rev-parse", "HEAD"]), head);
}

pub(in crate::git_factor::engine::tests) fn output_interruption(at: u8) {
    let repository = open_selection();
    repository.write("unrelated", "user bytes\n");
    repository.git(&["add", "atom"]);
    let original_checkpoint = repository.git(&["rev-parse", "refs/heads/main"]);
    let expected_tree = repository.git(&["rev-parse", "refs/heads/main^{tree}"]);
    let output = InterruptedOutput {
        capture: &repository.output,
        writes: Cell::new(0),
        at,
    };
    let (code, partial) = repository.invoke_using(
        &["--message", "Captured before output interruption"],
        &REAL_RUNNER,
        &REAL_FS,
        &output,
    );
    assert_eq!(code, EXIT_SOFTWARE);
    assert_eq!(output.writes.get(), at);
    assert!(
        repository
            .output
            .stderr
            .borrow()
            .contains("injected result output interruption")
    );
    if at == 1 {
        assert_eq!(partial, "");
    } else {
        let parsed: serde_json::Value =
            serde_json::from_str(&partial).or_abort("complete JSON before failed newline");
        assert_eq!(
            parsed.pointer("/result").or_abort("required JSON fact"),
            "committed"
        );
        assert!(!partial.ends_with('\n'));
    }
    assert_eq!(phase(&repository), "selecting");
    let captured_checkpoint = repository.git(&["rev-parse", "refs/heads/main"]);
    assert_ne!(captured_checkpoint, original_checkpoint);
    assert_eq!(
        repository.git(&["rev-parse", "refs/heads/main^{tree}"]),
        expected_tree
    );
    repository.success(&["--status"]);
    repository.success(&["--retry"]);
    repository.success(&["--abort"]);
    assert_eq!(repository.git(&["rev-parse", "HEAD"]), captured_checkpoint);
    assert_eq!(
        fs::read(repository.environment.cwd.join("unrelated")).or_abort("preserved user bytes"),
        b"user bytes\n"
    );
}
fn phase(repository: &Repository) -> String {
    let journal: serde_json::Value =
        serde_json::from_slice(&repository.journal()).or_abort("actual journal");
    journal
        .pointer("/state/phase")
        .or_abort("required JSON fact")
        .as_str()
        .or_abort("tagged phase")
        .to_owned()
}

#[expect(
    clippy::single_call_fn,
    reason = "named native fixture boundaries keep ownership, stimulus, and conservation visible to the direct canonical provider"
)]
pub(in crate::git_factor::engine::tests) fn process_interruption(boundary: Boundary) {
    let repository = match boundary {
        Boundary::LeasePublished | Boundary::RebaseOpened => initial_repository(),
        Boundary::ReplayCompleted | Boundary::ActorRegistered | Boundary::ActorCleanup => {
            let repository = open_selection();
            repository.write("unrelated", "user bytes\n");
            repository.git(&["add", "atom"]);
            repository
        }
    };
    let runner = InterruptedRunner {
        boundary,
        fired: Cell::new(false),
    };
    let initial_head = repository.git(&["rev-parse", "HEAD"]);
    let initial_index = repository.index();
    let arguments = match boundary {
        Boundary::LeasePublished | Boundary::RebaseOpened => vec!["--exec", "true", "HEAD"],
        Boundary::ReplayCompleted | Boundary::ActorRegistered | Boundary::ActorCleanup => {
            vec!["--message", "Accepted checkpoint atom"]
        }
    };
    let (code, stdout) = repository.invoke_with(&arguments, &runner, &REAL_FS);
    assert_ne!(code, EXIT_OK);
    assert_eq!(stdout, "");
    assert!(runner.fired.get());
    assert!(
        repository
            .output
            .stderr
            .borrow()
            .contains("injected native mutation interruption")
    );
    match boundary {
        Boundary::LeasePublished => {
            assert_eq!(phase(&repository), "preparing");
            assert_eq!(repository.index(), initial_index);
            assert_eq!(repository.git(&["rev-parse", "HEAD"]), initial_head);
        }
        Boundary::RebaseOpened => assert_eq!(phase(&repository), "opening"),
        Boundary::ReplayCompleted => {
            assert_eq!(phase(&repository), "verified");
            assert!(
                !repository
                    .environment
                    .cwd
                    .join(".git/rebase-merge")
                    .exists()
            );
        }
        Boundary::ActorRegistered | Boundary::ActorCleanup => {
            assert_eq!(phase(&repository), "selecting");
            assert_eq!(repository.index(), initial_index);
            assert_eq!(repository.git(&["rev-parse", "HEAD"]), initial_head);
            assert_eq!(
                repository
                    .git(&["worktree", "list", "--porcelain"])
                    .lines()
                    .filter(|line| line.starts_with("worktree "))
                    .count(),
                2
            );
        }
    }
    finish_recovery(&repository);
}

fn unrelated_refs(repository: &Repository) -> Vec<String> {
    repository
        .git(&["show-ref"])
        .lines()
        .filter(|line| !line.ends_with(" refs/heads/main") && !line.contains(" refs/factor/"))
        .map(str::to_owned)
        .collect()
}

/// Stops a real completed replay before the parent invocation captures its checkpoint.
fn verified_repository(empty: bool) -> Repository {
    let repository = open_selection();
    repository.write("unrelated", "user bytes\n");
    let foreign = repository.git(&["rev-parse", "refs/heads/main^"]);
    repository.git(&["update-ref", "refs/heads/foreign", &foreign]);
    repository.git(&["update-ref", "refs/tags/foreign", &foreign]);
    if empty {
        repository.git(&["add", "atom", "remainder"]);
    } else {
        repository.git(&["add", "atom"]);
    }
    let runner = InterruptedRunner {
        boundary: Boundary::ReplayCompleted,
        fired: Cell::new(false),
    };
    let (code, _) = repository.invoke_with(
        &["--message", "Durable capture boundary"],
        &runner,
        &REAL_FS,
    );
    assert_ne!(code, EXIT_OK);
    assert!(runner.fired.get());
    assert_eq!(phase(&repository), "verified");
    assert!(
        !repository
            .environment
            .cwd
            .join(".git/rebase-merge")
            .exists()
    );
    repository
}

/// The actual first replay-intent publication, before any main index promotion.
pub(in crate::git_factor::engine::tests) fn selecting_publication(after_replace: bool) {
    let repository = initial_repository();
    repository.git(&["branch", "foreign"]);
    repository.git(&["tag", "foreign"]);
    repository.success(&["--exec", "true", "HEAD"]);
    repository.git(&["add", "atom"]);
    let before = SelectingFrame::read(&repository);
    assert_eq!(
        before.files,
        [
            b"atom\n".to_vec(),
            b"remainder\n".to_vec(),
            b"user bytes\n".to_vec()
        ]
    );
    let old_journal = repository.journal();
    let final_tree = repository.git(&["rev-parse", "refs/heads/main^{tree}"]);
    let old_lease = repository.git(&["rev-parse", "refs/factor/session-lease"]);
    let filesystem = JournalPublication {
        after_replace,
        calls: Cell::new(0),
        target: PublicationTarget::InitialReplay,
    };

    let (code, stdout) =
        repository.invoke_with(&["--message", "Extract atom"], &REAL_RUNNER, &filesystem);

    assert_eq!(code, EXIT_SOFTWARE);
    assert_eq!(stdout, "");
    assert_eq!(
        *repository.output.stderr.borrow(),
        "failed to write state: injected journal publication interruption\n"
    );
    assert_eq!(filesystem.calls.get(), 1);
    assert_eq!(SelectingFrame::read(&repository), before);
    let current_lease = repository.git(&["rev-parse", "refs/factor/session-lease"]);
    assert_ne!(current_lease, old_lease);
    let ancestry = REAL_RUNNER
        .output(
            "git",
            &["merge-base", "--is-ancestor", &old_lease, &current_lease],
            &[],
            &repository.environment.cwd,
        )
        .or_abort("owned capsule ancestry");
    assert!(ancestry.status.success());
    let stored: serde_json::Value =
        serde_json::from_slice(&repository.journal()).or_abort("actual interrupted journal");
    assert_eq!(
        stored
            .pointer("/checkpoint")
            .and_then(serde_json::Value::as_str),
        Some(before.checkpoint.as_str())
    );
    if after_replace {
        assert_eq!(phase(&repository), "replaying");
        assert_eq!(
            stored.pointer("/state/accepted"),
            Some(&serde_json::json!([]))
        );
        assert_eq!(
            stored
                .pointer("/state/remainder/state")
                .and_then(serde_json::Value::as_str),
            Some("pending")
        );
    } else {
        assert_eq!(phase(&repository), "selecting");
        assert_eq!(repository.journal(), old_journal);
        repository.success(&["--continue"]);
        assert_eq!(repository.index(), before.index);
        repository.success(&["--message", "Extract atom"]);
    }
    if after_replace {
        repository.success(&["--continue"]);
    }
    assert_eq!(phase(&repository), "selecting");
    let captured = repository.git(&["rev-parse", "refs/heads/main"]);
    assert_ne!(captured, before.checkpoint);
    assert_eq!(
        repository.git(&["rev-parse", "refs/heads/main^{tree}"]),
        final_tree
    );
    repository.success(&["--abort"]);
    assert_eq!(repository.git(&["rev-parse", "HEAD"]), captured);
    assert_eq!(unrelated_refs(&repository), before.protected_refs);
    assert_eq!(
        fs::read(repository.environment.cwd.join("unrelated")).or_abort("user readback"),
        b"user bytes\n"
    );
}
