use super::*;

/// Seeds competing main-worktree routes without changing the test process environment.
struct MainRoutes {
    environment: [(&'static str, String); 4],
}

impl MainRoutes {
    fn assignments<'route>(
        &'route self,
        assignments: &[(&'route str, Option<&'route str>)],
    ) -> Vec<(&'route str, Option<&'route str>)> {
        self.environment
            .iter()
            .map(|entry| (entry.0, Some(entry.1.as_str())))
            .chain(assignments.iter().copied())
            .collect()
    }
}

impl Runner for MainRoutes {
    fn output(
        &self,
        bin: &str,
        args: &[&str],
        envs: &[(&str, Option<&str>)],
        cwd: &Path,
    ) -> io::Result<Output> {
        REAL_RUNNER.output(bin, args, &self.assignments(envs), cwd)
    }
    fn status(
        &self,
        bin: &str,
        args: &[&str],
        envs: &[(&str, Option<&str>)],
        quiet: bool,
        cwd: &Path,
    ) -> io::Result<ExitStatus> {
        REAL_RUNNER.status(bin, args, &self.assignments(envs), quiet, cwd)
    }
}

/// An ordinary gate creates its own repository; native hooks still discover the actor.
#[expect(
    clippy::too_many_lines,
    reason = "the native environment fixture keeps isolated gate, hook identity, and main conservation assertions beside its single validation Act"
)]
#[expect(
    clippy::create_dir,
    reason = "the fixture requires exclusive creation of one directory rather than admitting an existing directory"
)]
pub(in crate::git_factor::candidate) fn verify_fixture_repository(root: bool, value: &str) {
    let directory = TempDir::new().or_abort("independent gate fixture");
    let repo = directory.path().join("main");
    fs::create_dir(&repo).or_abort("main repository");
    let original = prepare_selection(&repo, root);
    let head = CommitSha::new(git(&repo, &["rev-parse", "HEAD"])).or_abort("selection HEAD");
    let tree = TreeHash::new(&git(&repo, &["write-tree"])).or_abort("selected tree");
    let state = StateDir::new(repo.join(".git/factor"));
    fs::create_dir_all(state.as_path()).or_abort("owned state");
    let fixture = directory.path().join("fixture");
    let hook_record = directory.path().join("hook-record");
    let gate_record = directory.path().join("gate-record");
    let hook = repo.join(".git/hooks/commit-msg");
    fs::write(
        &hook,
        format!(
            "#!/bin/sh\ngit rev-parse --show-toplevel > '{}'\n",
            hook_record.display()
        ),
    )
    .or_abort("native hook");
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).or_abort("hook executable");
    let script = format!(
        "git rev-parse --show-toplevel > '{}' && git init --quiet '{}' && \
         git -C '{}' config user.name Fixture && git -C '{}' config user.email fixture@example.com && \
         git -C '{}' config factorprobe.result '{}' && printf fixture > '{}/content' && \
         git -C '{}' add content && git -C '{}' commit --quiet --message Fixture",
        gate_record.display(),
        fixture.display(),
        fixture.display(),
        fixture.display(),
        fixture.display(),
        value,
        fixture.display(),
        fixture.display(),
        fixture.display(),
    );
    let command_file = directory.path().join("gate-command");
    fs::write(&command_file, &script).or_abort("literal command");
    let digest = git(
        &repo,
        &[
            "hash-object",
            "--no-filters",
            command_file.to_str().or_abort("command path"),
        ],
    );
    let proof_ref = format!("refs/factor/gates/{digest}/{tree}");
    let gate = NonEmptyString::try_from(script).or_abort("gate command");
    let messages =
        NonEmpty::new(NonEmptyString::try_from("Add selected atom".to_owned()).or_abort("message"));
    let index = fs::read(repo.join(".git/index")).or_abort("initial index");
    let config = fs::read(repo.join(".git/config")).or_abort("initial config");
    let references = git(&repo, &["show-ref"]);
    let io = CapturedIo::default();
    let main_git = repo.join(".git");
    let routes = MainRoutes {
        environment: [
            ("GIT_COMMON_DIR", main_git.display().to_string()),
            ("GIT_DIR", main_git.display().to_string()),
            (
                "GIT_INDEX_FILE",
                main_git.join("index").display().to_string(),
            ),
            ("GIT_WORK_TREE", repo.display().to_string()),
        ],
    };
    let ctx = Ctx {
        cwd: repo.clone(),
        env: &REAL_ENV,
        fs: &REAL_FS,
        io: &io,
        runner: &routes,
    };

    let result = validate_candidate(
        &ctx,
        &state,
        &original,
        &messages,
        if root { None } else { Some(&head) },
        &tree,
        &gate,
    );

    let candidate = result.or_abort("fixture gate passes");
    assert_eq!(git(&repo, &["rev-parse", "HEAD"]), head.as_str());
    assert_eq!(
        fs::read(repo.join(".git/index")).or_abort("retained index"),
        index
    );
    assert_eq!(
        fs::read(repo.join(".git/config")).or_abort("retained config"),
        config
    );
    assert_eq!(
        fs::read(repo.join("atom")).or_abort("selected bytes"),
        b"atom\n"
    );
    assert_eq!(
        fs::read(repo.join("remainder")).or_abort("remainder bytes"),
        b"remainder\n"
    );
    assert_eq!(
        fs::read(repo.join("user-file")).or_abort("unrelated bytes"),
        b"unrelated\n"
    );
    assert!(fixture.join(".git").is_dir());
    assert_eq!(git(&fixture, &["config", "factorprobe.result"]), value);
    assert_eq!(git(&fixture, &["show", "HEAD:content"]), "fixture");
    assert_eq!(git(&fixture, &["log", "-1", "--format=%s"]), "Fixture");
    let actor = fs::read_to_string(&gate_record).or_abort("gate actor identity");
    assert_eq!(
        fs::read_to_string(&hook_record).or_abort("native hook actor identity"),
        actor
    );
    let owned_state = fs::canonicalize(state.as_path()).or_abort("canonical owned state");
    // Native Git reports the canonical actor path; cleanup has already removed it.
    assert!(Path::new(actor.trim_end_matches('\n')).starts_with(&owned_state));
    let proof = git(&repo, &["rev-parse", &proof_ref]);
    assert_eq!(
        git(&repo, &["rev-parse", &format!("{proof}^{{tree}}")]),
        tree.as_str()
    );
    let mut expected_refs = references
        .lines()
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    let inserted = expected_refs.insert(format!("{proof} {proof_ref}"));
    assert!(inserted);
    assert_eq!(
        git(&repo, &["show-ref"])
            .lines()
            .map(str::to_owned)
            .collect::<BTreeSet<_>>(),
        expected_refs
    );
    assert_eq!(
        git(&repo, &["rev-parse", &format!("{candidate}^{{tree}}")]),
        tree.as_str()
    );
    assert!(io.out.borrow().is_empty());
}
