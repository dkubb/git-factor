#[path = "candidate_environment_tests.rs"]
pub(in crate::git_factor::candidate) mod environment;

#[path = "candidate_fault_tests.rs"]
pub(in crate::git_factor::candidate) mod faults;

use super::*;
use crate::git_factor::{Fs, Io, REAL_ENV, REAL_FS, REAL_RUNNER, Runner};
use crate::test_support::{OrAbort as _, ResultOrAbort as _};
use core::cell::{Cell, RefCell};
use std::ffi::OsString;
use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Output};

#[derive(Clone, Copy, Debug)]
pub(in crate::git_factor::candidate) enum ValidationCase {
    AttachmentMutation,
    AuthorMutation,
    GateFailure,
    HeadMutation,
    MessageRejection,
    ParentMutation,
    Passing,
    TreeMutation,
    WorktreeMutation,
}

impl ValidationCase {
    pub(in crate::git_factor::candidate) const ALL: [Self; 9] = [
        Self::AuthorMutation,
        Self::AttachmentMutation,
        Self::GateFailure,
        Self::WorktreeMutation,
        Self::HeadMutation,
        Self::MessageRejection,
        Self::ParentMutation,
        Self::Passing,
        Self::TreeMutation,
    ];
}

struct ProcessFailure {
    call: Cell<usize>,
    fail_at: usize,
}

struct CounterWriteFailure {
    file: &'static str,
}

impl Fs for CounterWriteFailure {
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
        REAL_FS.write_atomic_string(path, content)
    }
    fn write_string(&self, path: &Path, content: &str) -> io::Result<()> {
        if path.file_name().is_some_and(|name| name == self.file) {
            return Err(io::Error::other("counter persistence unavailable"));
        }
        REAL_FS.write_string(path, content)
    }
}

impl ProcessFailure {
    fn observe(&self) -> io::Result<()> {
        let next = self
            .call
            .get()
            .checked_add(1)
            .ok_or_else(|| io::Error::other("process counter overflow"))?;
        self.call.set(next);
        if next == self.fail_at {
            return Err(io::Error::other("candidate process unavailable"));
        }
        Ok(())
    }
}

impl Runner for ProcessFailure {
    fn output(
        &self,
        bin: &str,
        args: &[&str],
        envs: &[(&str, Option<&str>)],
        cwd: &Path,
    ) -> io::Result<Output> {
        self.observe()?;
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
        self.observe()?;
        REAL_RUNNER.status(bin, args, envs, quiet, cwd)
    }
}

#[derive(Default)]
struct CapturedIo {
    err: RefCell<String>,
    out: RefCell<String>,
}

impl Io for CapturedIo {
    fn err(&self, text: &str) -> io::Result<()> {
        self.err.borrow_mut().push_str(text);
        Ok(())
    }
    fn errln(&self, line: &str) -> io::Result<()> {
        self.err(&format!("{line}\n"))
    }
    fn out(&self, text: &str) -> io::Result<()> {
        self.out.borrow_mut().push_str(text);
        Ok(())
    }
    fn outln(&self, line: &str) -> io::Result<()> {
        self.out(&format!("{line}\n"))
    }
}

fn git(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .or_abort("native Git");
    assert!(
        output.status.success(),
        "Git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .or_abort("Git text")
        .trim_end_matches('\n')
        .to_owned()
}

/// Builds the same named gate/session request used by actual isolated validation.
fn validate_candidate(
    ctx: &Ctx<'_>,
    state: &StateDir,
    original: &CommitSha,
    messages: &NonEmpty<NonEmptyString>,
    parent: Option<&CommitSha>,
    tree: &TreeHash,
    gate: &NonEmptyString,
) -> Result<CommitSha, FactorError> {
    let gates = GateSet::new(vec![super::super::gate::legacy(ctx, gate.clone())?])?;
    validate_named_in(
        ctx,
        state,
        &SessionId::new(original.to_string())?,
        Candidate::new(original, messages, parent, tree),
        &gates,
    )
}

fn prepare_selection(repo: &Path, root: bool) -> CommitSha {
    prepare_selection_with(repo, root, "atom\n")
}

fn prepare_selection_with(repo: &Path, root: bool, atom: &str) -> CommitSha {
    git(repo, &["init", "--quiet"]);
    git(repo, &["config", "user.name", "Original Author"]);
    git(repo, &["config", "user.email", "original@example.com"]);
    if !root {
        fs::write(repo.join("base"), "base\n").or_abort("base");
        fs::write(repo.join("obsolete"), "deleted by selection\n").or_abort("deleted base leaf");
        git(repo, &["add", "base", "obsolete"]);
        git(repo, &["commit", "--quiet", "--message", "Add base"]);
        fs::remove_file(repo.join("obsolete")).or_abort("selected deletion");
    }
    fs::write(repo.join("atom"), atom).or_abort("atom");
    fs::write(repo.join("remainder"), "remainder\n").or_abort("remainder");
    git(repo, &["add", "--all"]);
    git(
        repo,
        &["commit", "--quiet", "--message", "Add combined change"],
    );
    let original = CommitSha::new(git(repo, &["rev-parse", "HEAD"])).or_abort("source");
    if root {
        git(repo, &["read-tree", "--empty"]);
    } else {
        git(repo, &["reset", "--quiet", "HEAD^"]);
    }
    git(repo, &["config", "user.name", "Current Committer"]);
    git(repo, &["config", "user.email", "current@example.com"]);
    git(repo, &["add", "atom"]);
    fs::write(repo.join("user-file"), "unrelated\n").or_abort("user work");
    original
}

/// Observes source identity independently through native Git formatting.
fn author_identity(repo: &Path, commit: &CommitSha) -> String {
    git(
        repo,
        &[
            "show",
            "--format=%an%x00%ae%x00%at",
            "--no-patch",
            commit.as_str(),
        ],
    )
}

/// Each unavailable process refuses without altering the user's selection.
pub(in crate::git_factor::candidate) fn verify_process_failure(fail_at: usize) -> usize {
    let directory = TempDir::new().or_abort("process fault fixture");
    let repo = directory.path();
    let original = prepare_selection(repo, false);
    let head = CommitSha::new(git(repo, &["rev-parse", "HEAD"])).or_abort("selection HEAD");
    let tree = TreeHash::new(&git(repo, &["write-tree"])).or_abort("selected tree");
    let state = StateDir::new(repo.join(".git/factor"));
    fs::create_dir_all(state.as_path()).or_abort("owned state");
    let index = git(repo, &["ls-files", "--stage"]);
    let runner = ProcessFailure {
        call: Cell::new(0),
        fail_at,
    };
    let io = CapturedIo::default();
    let ctx = Ctx {
        cwd: repo.to_path_buf(),
        env: &REAL_ENV,
        fs: &REAL_FS,
        io: &io,
        runner: &runner,
    };
    let messages =
        NonEmpty::new(NonEmptyString::try_from("Add atom".to_owned()).or_abort("message"));
    let gate = NonEmptyString::try_from("true".to_owned()).or_abort("gate");
    let result = validate_candidate(
        &ctx,
        &state,
        &original,
        &messages,
        Some(&head),
        &tree,
        &gate,
    );
    let calls = runner.call.get();
    if fail_at <= calls {
        let error = result.err_or_abort("unavailable process refuses");
        assert!(
            error.to_string().contains("candidate process unavailable")
                || matches!(&error, FactorError::InvalidCommit(value) if value.as_str() == "HEAD"),
            "fault {fail_at}: {error}"
        );
    } else {
        let _candidate = result.or_abort("all processes available");
    }
    assert_eq!(git(repo, &["rev-parse", "HEAD"]), head.as_str());
    assert_eq!(git(repo, &["ls-files", "--stage"]), index);
    assert_eq!(
        fs::read_to_string(repo.join("remainder")).or_abort("remaining file"),
        "remainder\n"
    );
    assert_eq!(
        fs::read_to_string(repo.join("user-file")).or_abort("user file"),
        "unrelated\n"
    );
    assert!(io.out.borrow().is_empty());
    calls
}

#[expect(
    clippy::single_call_fn,
    reason = "candidate commit observations are separate from selection preservation"
)]
fn verify_candidate(
    repo: &Path,
    candidate: &CommitSha,
    tree: &TreeHash,
    parent: Option<&CommitSha>,
    author: &str,
    message: &str,
) {
    assert_eq!(
        git(repo, &["rev-parse", &format!("{candidate}^{{tree}}")]),
        tree.as_str()
    );
    assert_eq!(
        git(
            repo,
            &["show", "--format=%P", "--no-patch", candidate.as_str()]
        ),
        parent.map_or("", CommitSha::as_str)
    );
    assert_eq!(author_identity(repo, candidate), author);
    assert_eq!(
        git(
            repo,
            &[
                "show",
                "--format=%cn%x00%ce",
                "--no-patch",
                candidate.as_str()
            ]
        ),
        "Current Committer\0current@example.com"
    );
    assert_eq!(
        git(
            repo,
            &["show", "--format=%B", "--no-patch", candidate.as_str()]
        ),
        message
    );
}

#[expect(
    clippy::single_call_fn,
    reason = "fault hook setup is separate from preservation assertions"
)]
fn install_hook(path: &Path, hook: &str, original: &CommitSha) {
    if hook.is_empty() {
        return;
    }
    let script = if hook == "author" {
        "#!/bin/sh\ngit -c core.hooksPath=/dev/null commit --quiet --amend --no-edit --author=\"Changed Author <changed@example.com>\"\n".to_owned()
    } else if hook == "tree" {
        "#!/bin/sh\nprintf changed > atom\ngit add atom\ngit -c core.hooksPath=/dev/null commit --quiet --amend --no-edit\n".to_owned()
    } else if hook == "commit-msg" {
        "#!/bin/sh\nexit 42\n".to_owned()
    } else {
        format!(
            "#!/bin/sh\ngit update-ref HEAD \"$(git commit-tree HEAD^{{tree}} -p {original} -m changed)\"\n"
        )
    };
    fs::write(path, script).or_abort("candidate hook");
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).or_abort("hook mode");
}

/// Derives the exact expected proof independently through Git's native blob digest.
#[expect(
    clippy::single_call_fn,
    reason = "the passing candidate fixture independently derives its complete stamped message"
)]
fn stamped_message(
    repo: &Path,
    state: &StateDir,
    command: &str,
    message: &str,
    tree: &TreeHash,
) -> String {
    let command_file = state.as_path().join("expected-command");
    fs::write(&command_file, command).or_abort("expected gate command");
    let command_path = command_file.to_str().or_abort("gate command path");
    let digest = git(repo, &["hash-object", "--no-filters", command_path]);
    format!("{message}\n\nGate-exec-{digest}:\n {digest}\n {tree}")
}

/// Checks actual parent/author metadata, independent tree selection and preservation.
pub(in crate::git_factor::candidate) fn verify_validation(
    root: bool,
    message: &str,
    case: ValidationCase,
) {
    verify_request(root, message, case, "atom\n");
}

/// Admits constructed borrowed requests against distinct native selected trees.
pub(in crate::git_factor::candidate) fn verify_request(
    root: bool,
    message: &str,
    case: ValidationCase,
    atom: &str,
) {
    let directory = TempDir::new().or_abort("candidate fixture");
    let repo = directory.path();
    let original = prepare_selection_with(repo, root, atom);
    let author = author_identity(repo, &original);
    let head = CommitSha::new(git(repo, &["rev-parse", "HEAD"])).or_abort("selection HEAD");
    let tree = TreeHash::new(&git(repo, &["write-tree"])).or_abort("staged tree");
    let status = git(repo, &["status", "--porcelain=v1"]);
    let index = fs::read(repo.join(".git/index")).or_abort("index");
    let worktrees = git(repo, &["worktree", "list", "--porcelain"]);
    let state = StateDir::new(repo.join(".git/factor"));
    fs::create_dir_all(state.as_path()).or_abort("owned state");
    let messages = NonEmpty::new(NonEmptyString::try_from(message.to_owned()).or_abort("message"));
    git(repo, &["config", "core.hooksPath", ".git/hooks"]);
    let (gate_command, outcome, hook) = match case {
        ValidationCase::AuthorMutation => ("exit 0", "mutation", "author"),
        ValidationCase::AttachmentMutation => (
            "git update-ref refs/heads/attached HEAD && git symbolic-ref HEAD refs/heads/attached",
            "mutation",
            "",
        ),
        ValidationCase::TreeMutation => ("exit 0", "mutation", "tree"),
        ValidationCase::GateFailure => ("exit 23", "gate_failed", ""),
        ValidationCase::WorktreeMutation => ("printf changed > atom", "mutation", ""),
        ValidationCase::HeadMutation => (
            "git commit --quiet --allow-empty --message changed",
            "mutation",
            "",
        ),
        ValidationCase::MessageRejection => ("exit 0", "mutation", "commit-msg"),
        ValidationCase::ParentMutation => ("exit 0", "mutation", "post-commit"),
        ValidationCase::Passing => ("test -f atom && test ! -e remainder", "passed", ""),
    };
    let hook_name = if matches!(hook, "author" | "tree") {
        "post-commit"
    } else {
        hook
    };
    let hook_path = repo.join(".git/hooks").join(hook_name);
    install_hook(&hook_path, hook, &original);
    let io = CapturedIo::default();
    let ctx = Ctx {
        cwd: repo.to_path_buf(),
        env: &REAL_ENV,
        fs: &REAL_FS,
        io: &io,
        runner: &REAL_RUNNER,
    };
    let gate = NonEmptyString::try_from(gate_command.to_owned()).or_abort("gate");
    let result = validate_candidate(
        &ctx,
        &state,
        &original,
        &messages,
        if root { None } else { Some(&head) },
        &tree,
        &gate,
    );
    if outcome == "passed" {
        let candidate = result.or_abort("validated candidate");
        verify_candidate(
            repo,
            &candidate,
            &tree,
            if root { None } else { Some(&head) },
            &author,
            &stamped_message(repo, &state, gate_command, message, &tree),
        );
    } else if outcome == "gate_failed" {
        let expected_code: i32 = 23;
        assert!(
            matches!(result, Err(FactorError::ExecFailed { code, .. }) if code == expected_code)
        );
    } else {
        let _error = result.err_or_abort("candidate mutation must refuse");
    }
    assert!(io.out.borrow().is_empty());
    if !hook.is_empty() {
        fs::remove_file(&hook_path).or_abort("remove candidate hook");
    }
    assert_eq!(git(repo, &["rev-parse", "HEAD"]), head.as_str());
    assert_eq!(
        fs::read(repo.join(".git/index")).or_abort("retained index"),
        index
    );
    assert_eq!(git(repo, &["status", "--porcelain=v1"]), status);
    assert_eq!(git(repo, &["worktree", "list", "--porcelain"]), worktrees);
    assert_eq!(
        fs::read_to_string(repo.join("remainder")).or_abort("remainder"),
        "remainder\n"
    );
    assert_eq!(
        fs::read_to_string(repo.join("user-file")).or_abort("user work"),
        "unrelated\n"
    );
}

#[expect(
    clippy::single_call_fn,
    reason = "native transition setup stays separate from its acceptance and conservation oracle"
)]
fn prepare_path_transition(
    repo: &Path,
    directory_first: bool,
    case_rename: bool,
    content: &str,
) -> (CommitSha, CommitSha, &'static str) {
    git(repo, &["init", "--quiet"]);
    git(repo, &["config", "user.name", "Original Author"]);
    git(repo, &["config", "user.email", "original@example.com"]);
    let old = if directory_first {
        "obsolete/leaf"
    } else {
        "obsolete"
    };
    fs::create_dir_all(repo.join(old).parent().or_abort("old path parent"))
        .or_abort("base directory");
    fs::write(repo.join(old), "base\n").or_abort("old source path");
    git(repo, &["add", "--all"]);
    git(repo, &["commit", "--quiet", "--message", "Add base"]);
    let parent = CommitSha::new(git(repo, &["rev-parse", "HEAD"])).or_abort("transition parent");
    git(repo, &["rm", "--cached", "--quiet", "--", old]);
    fs::remove_file(repo.join(old)).or_abort("source deletion");
    if directory_first {
        fs::remove_dir(repo.join("obsolete")).or_abort("source directory deletion");
    }
    let new = if case_rename {
        "Obsolete"
    } else if directory_first {
        "obsolete"
    } else {
        "obsolete/leaf"
    };
    fs::create_dir_all(repo.join(new).parent().or_abort("new path parent"))
        .or_abort("source directory");
    fs::write(repo.join(new), content).or_abort("owned replacement");
    fs::write(repo.join("atom"), "selected atom\n").or_abort("selected atom");
    git(repo, &["add", "--all"]);
    git(
        repo,
        &["commit", "--quiet", "--message", "Change source paths"],
    );
    let original = CommitSha::new(git(repo, &["rev-parse", "HEAD"])).or_abort("transition source");
    assert_eq!(
        git(repo, &["ls-tree", "-r", "--name-only", original.as_str()]),
        if case_rename {
            format!("{new}\natom")
        } else {
            format!("atom\n{new}")
        },
    );
    (original, parent, new)
}

/// Valid source-owned path transitions remain selectable across physical alias admission.
pub(in crate::git_factor::candidate) fn verify_owned_path_transition(
    directory_first: bool,
    case_rename: bool,
    content: &str,
) {
    let fixture = TempDir::new().or_abort("owned transition fixture");
    let repo = fixture.path();
    let (original, parent, new) =
        prepare_path_transition(repo, directory_first, case_rename, content);
    git(repo, &["reset", "--quiet", parent.as_str()]);
    git(repo, &["add", "atom"]);
    let tree = TreeHash::new(&git(repo, &["write-tree"])).or_abort("atom tree");
    let index = fs::read(repo.join(".git/index")).or_abort("main index");
    let refs = git(
        repo,
        &[
            "for-each-ref",
            "--format=%(refname) %(objectname)",
            "refs/heads",
            "refs/tags",
        ],
    );
    let io = CapturedIo::default();
    let ctx = Ctx {
        cwd: repo.to_path_buf(),
        env: &REAL_ENV,
        fs: &REAL_FS,
        io: &io,
        runner: &REAL_RUNNER,
    };
    let state = StateDir::new(repo.join(".git/factor"));
    fs::create_dir_all(state.as_path()).or_abort("candidate state");
    let messages =
        NonEmpty::new(NonEmptyString::try_from("Add selected atom".to_owned()).or_abort("message"));
    let gate = NonEmptyString::try_from("true".to_owned()).or_abort("gate");
    let accepted = validate_candidate(
        &ctx,
        &state,
        &original,
        &messages,
        Some(&parent),
        &tree,
        &gate,
    )
    .or_abort("owned source transition accepted");
    assert_eq!(
        git(repo, &["rev-parse", &format!("{accepted}^{{tree}}")]),
        tree.as_str()
    );
    assert_eq!(
        git(repo, &["rev-parse", &format!("{accepted}^")]),
        parent.as_str()
    );
    assert_eq!(
        author_identity(repo, &accepted),
        author_identity(repo, &original)
    );
    assert_eq!(
        fs::read(repo.join(new)).or_abort("preserved source bytes"),
        content.as_bytes()
    );
    assert_eq!(git(repo, &["rev-parse", "HEAD"]), parent.as_str());
    assert_eq!(
        fs::read(repo.join(".git/index")).or_abort("preserved main index"),
        index
    );
    assert_eq!(
        git(
            repo,
            &[
                "for-each-ref",
                "--format=%(refname) %(objectname)",
                "refs/heads",
                "refs/tags"
            ]
        ),
        refs
    );
    assert!(io.out.borrow().is_empty());
}

/// Named validation refuses recreated source deletions outside the caller directory.
pub(in crate::git_factor::candidate) fn verify_recreated_deleted_path(
    message: &str,
    contents: &str,
    ignored: bool,
) {
    verify_recreation(message, contents, ignored, false);
}

/// Kernel aliases are refused only when the filesystem actually aliases the deleted name.
pub(in crate::git_factor::candidate) fn verify_recreated_deleted_alias(
    message: &str,
    contents: &str,
    ignored: bool,
) {
    verify_recreation(message, contents, ignored, true);
}

fn verify_recreation(message: &str, contents: &str, ignored: bool, alias: bool) {
    let directory = TempDir::new().or_abort("outside-child refusal fixture");
    let repo = directory.path();
    let original = prepare_selection(repo, false);
    let head = CommitSha::new(git(repo, &["rev-parse", "HEAD"])).or_abort("selection HEAD");
    let tree = TreeHash::new(&git(repo, &["write-tree"])).or_abort("selected tree");
    let child = repo.join("child");
    fs::create_dir_all(&child).or_abort("caller directory");
    let recreated = if alias { "Obsolete" } else { "obsolete" };
    fs::write(repo.join(recreated), contents).or_abort("recreated deleted user file");
    if alias {
        let Ok(original_name) = fs::symlink_metadata(repo.join("obsolete")) else {
            return;
        };
        let alias_name = fs::symlink_metadata(repo.join(recreated)).or_abort("alias metadata");
        assert_eq!(
            (original_name.dev(), original_name.ino()),
            (alias_name.dev(), alias_name.ino())
        );
    }
    if ignored {
        fs::write(repo.join(".git/info/exclude"), format!("{recreated}\n"))
            .or_abort("ignored recreation");
    }
    let state = StateDir::new(repo.join(".git/factor"));
    fs::create_dir_all(state.as_path()).or_abort("owned session state");
    let journal = state.as_path().join("journal.json");
    fs::write(&journal, "preserved journal\n").or_abort("journal sentinel");
    let index = fs::read(repo.join(".git/index")).or_abort("original index bytes");
    let refs = git(repo, &["for-each-ref", "--format=%(refname) %(objectname)"]);
    let worktrees = git(repo, &["worktree", "list", "--porcelain"]);
    let io = CapturedIo::default();
    let ctx = Ctx {
        cwd: child,
        env: &REAL_ENV,
        fs: &REAL_FS,
        io: &io,
        runner: &REAL_RUNNER,
    };
    let messages = NonEmpty::new(NonEmptyString::try_from(message.to_owned()).or_abort("message"));
    let gate = NonEmptyString::try_from("true".to_owned()).or_abort("gate");
    let error = validate_candidate(
        &ctx,
        &state,
        &original,
        &messages,
        Some(&head),
        &tree,
        &gate,
    )
    .err_or_abort("outside-child recreated deletion refuses");
    assert_eq!(
        error.to_string(),
        "git command failed: a deleted selection path has been recreated"
    );
    assert_eq!(
        fs::read(repo.join(recreated)).or_abort("preserved user bytes"),
        contents.as_bytes()
    );
    assert_eq!(git(repo, &["rev-parse", "HEAD"]), head.as_str());
    assert_eq!(
        fs::read(repo.join(".git/index")).or_abort("preserved index"),
        index
    );
    assert_eq!(
        git(repo, &["for-each-ref", "--format=%(refname) %(objectname)"]),
        refs
    );
    assert_eq!(git(repo, &["worktree", "list", "--porcelain"]), worktrees);
    assert_eq!(
        fs::read(&journal).or_abort("preserved journal"),
        b"preserved journal\n"
    );
    assert!(io.out.borrow().is_empty());
}

/// Admission rejects foreign staging and tracked edits without changing the selection.
pub(in crate::git_factor::candidate) fn verify_admission(root: bool) {
    let directory = TempDir::new().or_abort("admission fixture");
    let repo = directory.path();
    let original = prepare_selection(repo, root);
    let head = CommitSha::new(git(repo, &["rev-parse", "HEAD"])).or_abort("parent");
    let io = CapturedIo::default();
    let ctx = Ctx {
        cwd: repo.to_path_buf(),
        env: &REAL_ENV,
        fs: &REAL_FS,
        io: &io,
        runner: &REAL_RUNNER,
    };
    for foreign in [false, true] {
        if foreign {
            git(repo, &["add", "user-file"]);
        } else {
            fs::write(repo.join("remainder"), "unexpected edit\n").or_abort("tracked edit");
        }
        let tree = TreeHash::new(&git(repo, &["write-tree"])).or_abort("staged tree");
        let index = fs::read(repo.join(".git/index")).or_abort("index");
        let scratch = TempDir::new().or_abort("scratch index");
        let result = admit_selection(
            &ctx,
            scratch.path(),
            &original,
            if root { None } else { Some(&head) },
            &tree,
        );
        if foreign {
            assert!(matches!(result, Err(FactorError::GitCommand(_))));
        } else {
            assert!(matches!(result, Err(FactorError::TreeHashMismatch { .. })));
            fs::write(repo.join("remainder"), "remainder\n").or_abort("restore fixture");
        }
        assert_eq!(
            fs::read(repo.join(".git/index")).or_abort("retained index"),
            index
        );
        assert_eq!(git(repo, &["rev-parse", "HEAD"]), head.as_str());
    }
    git(repo, &["reset", "--quiet", "--", "user-file"]);
    git(repo, &["config", "core.filemode", "false"]);
    fs::set_permissions(repo.join("atom"), fs::Permissions::from_mode(0o755))
        .or_abort("unexpected mode");
    let tree = TreeHash::new(&git(repo, &["write-tree"])).or_abort("staged tree");
    let scratch = TempDir::new().or_abort("mode observation index");
    let error = admit_selection(
        &ctx,
        scratch.path(),
        &original,
        if root { None } else { Some(&head) },
        &tree,
    )
    .err_or_abort("executable changes remain observable");
    assert!(matches!(error, FactorError::TreeHashMismatch { .. }));
    fs::set_permissions(repo.join("atom"), fs::Permissions::from_mode(0o644))
        .or_abort("fixture mode");
    if !root {
        fs::write(repo.join("obsolete"), "unexpected recreation\n").or_abort("recreated leaf");
        let deleted_index = TempDir::new().or_abort("deleted leaf observation index");
        let recreation_error =
            admit_selection(&ctx, deleted_index.path(), &original, Some(&head), &tree)
                .err_or_abort("deleted leaf recreation refuses");
        assert_eq!(
            recreation_error.to_string(),
            "git command failed: a deleted selection path has been recreated"
        );
        assert_eq!(
            fs::read_to_string(repo.join("obsolete")).or_abort("preserved recreation"),
            "unexpected recreation\n"
        );
    }
}
