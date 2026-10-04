use super::*;
use std::os::unix::ffi::OsStringExt as _;
use std::os::unix::process::ExitStatusExt as _;

#[derive(Clone, Copy)]
enum Response {
    Exit,
    Text(&'static str),
}

struct ObservationFault {
    actor: bool,
    args: &'static [&'static str],
    observed: Cell<bool>,
    response: Response,
}

impl ObservationFault {
    fn matches(&self, args: &[&str], cwd: &Path) -> bool {
        let actor = cwd.file_name().is_some_and(|name| name == "worktree");
        let matches = actor == self.actor && args.starts_with(self.args);
        if matches {
            self.observed.set(true);
        }
        matches
    }
}

impl Runner for ObservationFault {
    fn output(
        &self,
        bin: &str,
        args: &[&str],
        envs: &[(&str, Option<&str>)],
        cwd: &Path,
    ) -> io::Result<Output> {
        if bin == "git" && self.matches(args, cwd) {
            return Ok(Output {
                status: ExitStatus::from_raw(if matches!(self.response, Response::Exit) {
                    17 << 8
                } else {
                    0
                }),
                stdout: match self.response {
                    Response::Exit => Vec::new(),
                    Response::Text(value) => value.as_bytes().to_vec(),
                },
                stderr: b"observation rejected".to_vec(),
            });
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
        if bin == "git" && self.matches(args, cwd) {
            return Ok(ExitStatus::from_raw(17 << 8));
        }
        REAL_RUNNER.status(bin, args, envs, quiet, cwd)
    }
}

/// A failing observation must not create or promote a candidate in the selection worktree.
pub(in crate::git_factor::candidate) fn verify_observation_failures(root: bool) {
    let directory = TempDir::new().or_abort("observation fixture");
    let repo = directory.path();
    let original = prepare_selection(repo, root);
    let head = CommitSha::new(git(repo, &["rev-parse", "HEAD"])).or_abort("HEAD");
    let tree = TreeHash::new(&git(repo, &["write-tree"])).or_abort("staged tree");
    let before = fs::read(repo.join(".git/index")).or_abort("index");
    let state = StateDir::new(repo.join(".git/factor"));
    fs::create_dir_all(state.as_path()).or_abort("state");
    let messages =
        NonEmpty::new(NonEmptyString::try_from("Add atom".to_owned()).or_abort("message"));
    let gate = NonEmptyString::try_from("true".to_owned()).or_abort("gate");
    let source_object: &[&str] = &["cat-file", "commit"];
    let private_write_tree: &[&str] = &["-c", "core.splitIndex=false", "write-tree"];
    for (actor, args, response) in [
        (false, source_object, Response::Text("truncated")),
        (false, &["diff"][..], Response::Exit),
        (
            false,
            &[
                "-c",
                "core.ignorestat=false",
                "-c",
                "core.splitIndex=false",
                "-c",
                "core.sparseCheckout=false",
                "read-tree",
            ][..],
            Response::Exit,
        ),
        (
            false,
            &[
                "-c",
                "core.filemode=true",
                "-c",
                "core.splitIndex=false",
                "-c",
                "core.sparseCheckout=false",
                "add",
                "--update",
            ][..],
            Response::Exit,
        ),
        (
            false,
            &[
                "diff",
                "--name-only",
                "-z",
                "--no-renames",
                "--no-relative",
                "--diff-filter=D",
            ][..],
            Response::Exit,
        ),
        (false, &["ls-files"][..], Response::Exit),
        (false, private_write_tree, Response::Exit),
        (false, private_write_tree, Response::Text("invalid")),
        (false, &["rev-parse"][..], Response::Text("invalid")),
        (false, &["commit-tree"][..], Response::Exit),
        (false, &["commit-tree"][..], Response::Text("invalid")),
        (
            true,
            &["rev-parse", concat!("HEAD^", "{", "tree", "}")][..],
            Response::Text("invalid"),
        ),
    ] {
        let runner = ObservationFault {
            actor,
            args,
            observed: Cell::new(false),
            response,
        };
        let io = CapturedIo::default();
        let ctx = Ctx {
            cwd: repo.to_path_buf(),
            env: &REAL_ENV,
            fs: &REAL_FS,
            io: &io,
            runner: &runner,
        };
        let _error = validate_candidate(
            &ctx,
            &state,
            &original,
            &messages,
            if root { None } else { Some(&head) },
            &tree,
            &gate,
        )
        .err_or_abort("observation must refuse");
        assert!(runner.observed.get(), "unreached fault {args:?}");
        assert_eq!(git(repo, &["rev-parse", "HEAD"]), head.as_str());
        assert_eq!(
            fs::read(repo.join(".git/index")).or_abort("preserved index"),
            before
        );
        assert!(io.out.borrow().is_empty());
    }
}

/// Invalid filesystem paths are refused before a candidate is constructed.
pub(in crate::git_factor::candidate) fn verify_state_paths(root: bool) {
    let directory = TempDir::new().or_abort("path fixture");
    let repo = directory.path();
    let original = prepare_selection(repo, root);
    let head = CommitSha::new(git(repo, &["rev-parse", "HEAD"])).or_abort("HEAD");
    let tree = TreeHash::new(&git(repo, &["write-tree"])).or_abort("staged tree");
    let io = CapturedIo::default();
    let ctx = Ctx {
        cwd: repo.to_path_buf(),
        env: &REAL_ENV,
        fs: &REAL_FS,
        io: &io,
        runner: &REAL_RUNNER,
    };
    let invalid = repo.join(OsString::from_vec(vec![b'x', 255]));
    let scratch_error = admit_selection(
        &ctx,
        &invalid,
        &original,
        if root { None } else { Some(&head) },
        &tree,
    )
    .err_or_abort("invalid index path");
    assert!(
        scratch_error
            .to_string()
            .contains("selection index path is not valid UTF-8")
    );
    let messages =
        NonEmpty::new(NonEmptyString::try_from("Add atom".to_owned()).or_abort("message"));
    let gate = NonEmptyString::try_from("true".to_owned()).or_abort("gate");
    let state = StateDir::new(invalid);
    let error = validate_candidate(
        &ctx,
        &state,
        &original,
        &messages,
        if root { None } else { Some(&head) },
        &tree,
        &gate,
    )
    .err_or_abort("invalid message path");
    assert!(
        error
            .to_string()
            .contains("candidate state path is not valid UTF-8")
    );
    let missing = StateDir::new(repo.join("missing"));
    assert!(matches!(
        validate_candidate(
            &ctx,
            &missing,
            &original,
            &messages,
            if root { None } else { Some(&head) },
            &tree,
            &gate
        ),
        Err(FactorError::StateWrite(_))
    ));
    let saved = StateDir::new(repo.join(".git/factor"));
    fs::create_dir_all(saved.as_path()).or_abort("saved state");
    let filesystem = CounterWriteFailure { file: "message" };
    let failing_ctx = Ctx {
        fs: &filesystem,
        ..ctx
    };
    assert!(matches!(
        validate_candidate(
            &failing_ctx,
            &saved,
            &original,
            &messages,
            if root { None } else { Some(&head) },
            &tree,
            &gate
        ),
        Err(FactorError::StateWrite(_))
    ));
    assert_eq!(git(repo, &["rev-parse", "HEAD"]), head.as_str());
    assert_eq!(git(repo, &["write-tree"]), tree.as_str());
    assert!(io.out.borrow().is_empty());
}
