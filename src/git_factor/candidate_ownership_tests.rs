//! Native contracts for the private exact-owned registration recovery boundary.

use super::{
    ActorOwner, Ctx, FactorError, SessionId, StateDir, record_ownership, remove_owned_actors,
};
use crate::git_factor::{REAL_ENV, REAL_FS, REAL_IO, REAL_RUNNER, Runner};
use crate::test_support::OrAbort as _;
use core::cell::Cell;
use std::fs;
use std::io;
use std::os::unix::ffi::{OsStrExt as _, OsStringExt as _};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Output};
use tempfile::TempDir;

#[derive(Clone, Copy, Debug)]
pub(in crate::git_factor::candidate) enum Refusal {
    ActorMismatch,
    CommonMismatch,
    MalformedMarker,
    MissingMarker,
    MixedRegistrations,
    NestedScratch,
    NonUtf8Path,
    SessionMismatch,
}

impl Refusal {
    pub(in crate::git_factor::candidate) const ALL: [Self; 8] = [
        Self::MissingMarker,
        Self::MalformedMarker,
        Self::SessionMismatch,
        Self::ActorMismatch,
        Self::CommonMismatch,
        Self::NestedScratch,
        Self::MixedRegistrations,
        Self::NonUtf8Path,
    ];
}

struct Repository {
    directory: TempDir,
    main: PathBuf,
    session: SessionId,
    state: StateDir,
}

/// Mutates one captured registration path byte, without creating an invalid filename.
struct RawPathListing {
    actor: Vec<u8>,
    observed: Cell<bool>,
}

impl Runner for RawPathListing {
    #[expect(
        clippy::panic_in_result_fn,
        reason = "the forwarding fixture asserts native registration and exact byte-substitution bounds independently of the production refusal"
    )]
    fn output(
        &self,
        bin: &str,
        args: &[&str],
        envs: &[(&str, Option<&str>)],
        cwd: &Path,
    ) -> io::Result<Output> {
        let mut output = REAL_RUNNER.output(bin, args, envs, cwd)?;
        if bin == "git" && args == ["worktree", "list", "--porcelain", "-z"] {
            assert!(
                output.status.success(),
                "native listing must succeed before mutation"
            );
            assert!(
                !self.observed.replace(true),
                "exactly one owned listing is substituted"
            );
            let record = [b"worktree ".as_slice(), self.actor.as_slice()].concat();
            let window = record
                .len()
                .checked_add(1)
                .or_abort("terminated native record length");
            let start = output
                .stdout
                .windows(window)
                .position(|part| part.strip_suffix(&[0]) == Some(record.as_slice()))
                .or_abort("exact native owned path record");
            let last = start
                .checked_add(record.len())
                .and_then(|end| end.checked_sub(1))
                .or_abort("native owned path byte offset");
            let byte = output
                .stdout
                .get_mut(last)
                .or_abort("native owned path byte");
            assert_eq!(*byte, b'e', "actual registered worktree path suffix");
            *byte = 255;
        }
        Ok(output)
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

impl Repository {
    fn actor(&self, scratch: &Path) -> PathBuf {
        fs::create_dir_all(scratch).or_abort("actor scratch");
        record_ownership(&self.context(), scratch, &self.session)
            .or_abort("actual owner publication");
        let actor = scratch.join("worktree");
        let output = Command::new("git")
            .current_dir(&self.main)
            .args(["worktree", "add", "--detach", "--no-checkout"])
            .arg(&actor)
            .arg("HEAD")
            .output()
            .or_abort("native registration");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        fs::write(actor.join("disposable"), b"owned scratch bytes\n").or_abort("actor bytes");
        actor
    }

    fn context(&self) -> Ctx<'_> {
        Ctx {
            cwd: self.main.clone(),
            env: &REAL_ENV,
            fs: &REAL_FS,
            io: &REAL_IO,
            runner: &REAL_RUNNER,
        }
    }

    fn main_facts(&self) -> Vec<Vec<u8>> {
        vec![
            native(&self.main, &["rev-parse", "HEAD", "HEAD^{tree}"]).stdout,
            native(&self.main, &["show-ref"]).stdout,
            fs::read(self.main.join(".git/index")).or_abort("raw main index"),
            fs::read(self.main.join(".git/config")).or_abort("main config"),
            fs::read(self.main.join("tracked")).or_abort("main tracked bytes"),
            fs::read(self.main.join("unrelated")).or_abort("unrelated bytes"),
        ]
    }

    #[expect(
        clippy::create_dir,
        reason = "the native fixture requires exclusive creation of one main directory"
    )]
    fn new(bytes: &[u8]) -> Self {
        let directory = TempDir::new().or_abort("native ownership fixture");
        let main = directory.path().join("main");
        fs::create_dir(&main).or_abort("main directory");
        native(&main, &["init", "--quiet", "--initial-branch=main"]);
        native(&main, &["config", "user.name", "Actor ownership contract"]);
        native(&main, &["config", "user.email", "actor@example.invalid"]);
        fs::write(main.join("tracked"), b"main tree\n").or_abort("tracked fixture");
        native(&main, &["add", "tracked"]);
        native(&main, &["commit", "--quiet", "--message", "Base"]);
        fs::write(main.join("unrelated"), bytes).or_abort("unrelated user bytes");
        let state = StateDir::new(main.join(".git/factor"));
        fs::create_dir_all(state.as_path()).or_abort("session directory");
        let session = SessionId::new("a".repeat(40)).or_abort("checked session");
        Self {
            directory,
            main,
            session,
            state,
        }
    }
}

fn native(cwd: &Path, arguments: &[&str]) -> Output {
    let output = Command::new("git")
        .current_dir(cwd)
        .args(arguments)
        .output()
        .or_abort("native Git observation");
    assert!(
        output.status.success(),
        "{arguments:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

/// Snapshots fixture bytes without following symbolic links or interpreting markers.
fn files(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut result = Vec::new();
    for entry in fs::read_dir(root).or_abort("fixture directory") {
        let path = entry.or_abort("fixture entry").path();
        let metadata = fs::symlink_metadata(&path).or_abort("fixture metadata");
        if metadata.is_dir() {
            result.extend(files(&path));
        } else if metadata.file_type().is_symlink() {
            result.push((
                path.clone(),
                fs::read_link(&path)
                    .or_abort("fixture link")
                    .into_os_string()
                    .into_vec(),
            ));
        } else {
            result.push((path.clone(), fs::read(&path).or_abort("fixture bytes")));
        }
    }
    result.sort_by(|left, right| left.0.cmp(&right.0));
    result
}

#[expect(
    clippy::single_call_fn,
    reason = "one arrangement helper publishes each precise ownership-corruption stimulus before the actual recovery Act"
)]
fn corrupt_marker(scratch: &Path, case: Refusal) {
    let marker = scratch.join("owner.json");
    match case {
        Refusal::MissingMarker | Refusal::MixedRegistrations => {
            fs::remove_file(marker).or_abort("missing marker stimulus");
        }
        Refusal::MalformedMarker => {
            fs::write(marker, b"{broken").or_abort("malformed marker stimulus");
        }
        Refusal::SessionMismatch | Refusal::ActorMismatch | Refusal::CommonMismatch => {
            let mut owner: ActorOwner =
                serde_json::from_slice(&fs::read(&marker).or_abort("actual marker"))
                    .or_abort("actual owner");
            if matches!(case, Refusal::SessionMismatch) {
                owner.session = SessionId::new("b".repeat(40)).or_abort("other checked session");
            } else if matches!(case, Refusal::ActorMismatch) {
                owner.actor = scratch.join("other-worktree");
            } else {
                owner.common = scratch.join("other-common");
            }
            fs::write(
                marker,
                serde_json::to_vec(&owner).or_abort("wrong-owner JSON"),
            )
            .or_abort("wrong marker stimulus");
        }
        Refusal::NestedScratch | Refusal::NonUtf8Path => {}
    }
}

pub(in crate::git_factor::candidate) fn verify_refusal(case: Refusal, stem: &str, bytes: &[u8]) {
    let repository = Repository::new(bytes);
    let root = repository.state.as_path();
    let scratch = match case {
        Refusal::NestedScratch => root.join("nested").join(stem),
        Refusal::ActorMismatch
        | Refusal::CommonMismatch
        | Refusal::MalformedMarker
        | Refusal::MissingMarker
        | Refusal::MixedRegistrations
        | Refusal::NonUtf8Path
        | Refusal::SessionMismatch => root.join(format!("z-{stem}")),
    };
    let actor = repository.actor(&scratch);
    corrupt_marker(&scratch, case);
    let owned = matches!(case, Refusal::MixedRegistrations)
        .then(|| repository.actor(&root.join(format!("a-{stem}"))));
    let registrations = native(&repository.main, &["worktree", "list", "--porcelain", "-z"]).stdout;
    if let Some(owned_actor) = owned.as_ref() {
        let owned_bytes = owned_actor.to_str().or_abort("owned path UTF8").as_bytes();
        let foreign = scratch.to_str().or_abort("foreign path UTF8").as_bytes();
        let owned_position = registrations
            .windows(owned_bytes.len())
            .position(|part| part == owned_bytes)
            .or_abort("listed owned actor");
        let foreign_position = registrations
            .windows(foreign.len())
            .position(|part| part == foreign)
            .or_abort("listed foreign actor");
        assert!(
            owned_position < foreign_position,
            "must admit owned before encountering foreign"
        );
    }
    let before = files(repository.directory.path());
    let facts = repository.main_facts();
    let listing = RawPathListing {
        actor: fs::canonicalize(&actor)
            .or_abort("canonical registered actor")
            .as_os_str()
            .as_bytes()
            .to_vec(),
        observed: Cell::new(false),
    };
    let mut context = repository.context();
    if matches!(case, Refusal::NonUtf8Path) {
        context.runner = &listing;
    }
    let error = remove_owned_actors(&context, &repository.state, &repository.session)
        .err()
        .or_abort("registration must refuse");
    assert_eq!(listing.observed.get(), matches!(case, Refusal::NonUtf8Path));
    match case {
        Refusal::MissingMarker | Refusal::MixedRegistrations => {
            assert!(
                matches!(error, FactorError::StateRead(failure) if failure.kind() == io::ErrorKind::NotFound)
            );
        }
        Refusal::MalformedMarker => assert_eq!(
            error.to_string(),
            "git command failed: unknown actor ownership must be preserved"
        ),
        Refusal::NonUtf8Path => assert_eq!(
            error.to_string(),
            "git command failed: unknown actor registration path must be preserved"
        ),
        Refusal::SessionMismatch
        | Refusal::ActorMismatch
        | Refusal::CommonMismatch
        | Refusal::NestedScratch => {
            assert_eq!(
                error.to_string(),
                "git command failed: foreign actor registration must be preserved"
            );
        }
    }
    assert_eq!(repository.main_facts(), facts);
    assert_eq!(
        native(&repository.main, &["worktree", "list", "--porcelain", "-z"]).stdout,
        registrations
    );
    assert_eq!(
        files(repository.directory.path()),
        before,
        "{case:?}: preserve all ownership evidence and user objects"
    );
}

pub(in crate::git_factor::candidate) fn verify_locked_cleanup(
    stem: &str,
    reason: &str,
    bytes: &[u8],
) {
    let repository = Repository::new(bytes);
    let scratch = repository.state.as_path().join(stem);
    let actor = repository.actor(&scratch);
    let owned_admin = PathBuf::from(
        fs::read_to_string(actor.join(".git"))
            .or_abort("owned gitfile")
            .trim()
            .strip_prefix("gitdir: ")
            .or_abort("owned admin path"),
    );
    let external_scratch = repository.directory.path().join("external");
    let external = repository.actor(&external_scratch);
    native(
        &repository.main,
        &[
            "worktree",
            "lock",
            "--reason",
            reason,
            actor.to_str().or_abort("owned actor UTF8"),
        ],
    );
    assert!(
        owned_admin.join("locked").exists(),
        "completed actor must carry a native lock"
    );
    let facts = repository.main_facts();
    let external_files = files(&external_scratch);
    let external_admin = PathBuf::from(
        fs::read_to_string(external.join(".git"))
            .or_abort("external gitfile")
            .trim()
            .strip_prefix("gitdir: ")
            .or_abort("external admin path"),
    );
    let external_admin_files = files(&external_admin);
    remove_owned_actors(
        &repository.context(),
        &repository.state,
        &repository.session,
    )
    .or_abort("remove admitted locked actor");
    assert!(!scratch.exists());
    assert!(!owned_admin.exists());
    let listed = native(&repository.main, &["worktree", "list", "--porcelain", "-z"]).stdout;
    assert!(
        !listed
            .windows(actor.as_os_str().len())
            .any(|part| part == actor.to_str().or_abort("owned actor UTF8").as_bytes())
    );
    assert!(
        listed
            .windows(external.as_os_str().len())
            .any(|part| part == external.to_str().or_abort("external actor UTF8").as_bytes())
    );
    assert_eq!(repository.main_facts(), facts);
    assert_eq!(files(&external_scratch), external_files);
    assert_eq!(files(&external_admin), external_admin_files);
}
