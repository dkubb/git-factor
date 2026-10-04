/// Pins native hook routing and records the resulting expected configuration.
fn arrange_hook_path(repository: &super::Repository, path: &str) -> Vec<u8> {
    repository.git(&["config", "core.hooksPath", path]);
    fs::read(repository.environment.cwd.join(".git/config")).or_abort("pinned hook configuration")
}

/// Arranges executable native hooks without invoking the factor consumer.
fn arrange_executable_hook(directory: &Path, script: &str) {
    fs::create_dir_all(directory).or_abort("native hook directory");
    let hook = directory.join("commit-msg");
    fs::write(&hook, script).or_abort("native hook bytes");
    fs::set_permissions(hook, fs::Permissions::from_mode(0o755))
        .or_abort("native hook permissions");
}

#[test]
fn captures_folded_named_and_legacy_stamps_under_native_line_policy() {
    let mut fixture = super::NativeAuthor::arrange(super::MessageLocation::Tip, false);
    fixture.config = arrange_hook_path(&fixture.repository, ".git/hooks");
    arrange_executable_hook(
        &fixture.repository.environment.cwd.join(".git/hooks"),
        "#!/bin/sh\nawk 'length($0) > 72 { exit 19 }' \"$1\"\n",
    );
    let repository = &fixture.repository;
    let root = &repository.environment.cwd;
    let markers = super::TempDir::new().or_abort("external gate markers");
    let marker = markers.path().join("gate");
    let text = format!("printf g >> '{}'", marker.display());
    let command_file = markers.path().join("command");
    fs::write(&command_file, &text).or_abort("independent command bytes");
    let command_hash = repository.git(&[
        "hash-object",
        "--no-filters",
        command_file.to_str().or_abort("command path"),
    ]);
    repository.success(&["--gate", "test", &text, "--exec", &text, &fixture.source]);
    assert_eq!(
        fs::read_to_string(&marker).or_abort("initial tree gate count"),
        "g"
    );
    repository.write("atom", "base\natom\n");
    repository.git(&["add", "atom"]);
    repository.write("atom", "base\natom\nremainder\n");
    let selected_tree = repository.git(&["write-tree"]);
    let expected_trailers = format!(
        "Gate-test: {command_hash} {selected_tree}\nGate-exec-{command_hash}: {command_hash} {selected_tree}"
    );
    let mut expected = super::OriginalPool::expected_output();
    expected
        .as_object_mut()
        .or_abort("selection fields")
        .insert(
            "changes".to_owned(),
            serde_json::from_str(concat!(
                r#"{"unstaged":[{"path":"atom","kind":"text","added":1,"deleted":0}],"#,
                r#""untracked":["user"]}"#,
            ))
            .or_abort("expected remaining changes"),
        );

    let (code, stdout) = repository.invoke(&["--message", "Add selected atom"]);

    let index = repository.index();
    assert_eq!(code, EXIT_OK, "{}", repository.output.stderr.borrow());
    assert_eq!(repository.output.stderr.borrow().as_str(), "");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&stdout).or_abort("capture JSON"),
        expected
    );
    assert_eq!((stdout.ends_with('\n'), stdout.lines().count()), (true, 1));
    assert_eq!(
        fs::read_to_string(&marker).or_abort("distinct tree gate count"),
        "gg"
    );
    assert_eq!(
        repository.git(&[
            "show",
            "-s",
            "--format=%(trailers:only,unfold=true)",
            "HEAD"
        ]),
        expected_trailers
    );
    assert!(
        repository
            .git(&["show", "-s", "--format=%B", "HEAD"])
            .lines()
            .all(|line| line.len() <= 72)
    );
    assert_eq!(
        (
            repository.git(&["rev-parse", "HEAD^{tree}"]),
            repository.git(&["rev-parse", "main^{tree}"])
        ),
        (selected_tree, fixture.final_tree)
    );
    assert_eq!(
        repository.git(&[
            "show-ref",
            "--verify",
            "refs/heads/protected",
            "refs/tags/protected-tag"
        ]),
        fixture.protected
    );
    assert_eq!(
        fs::read(root.join(".git/config")).or_abort("config after capture"),
        fixture.config
    );
    assert_eq!(
        fs::read(root.join("user")).or_abort("unrelated bytes"),
        b"unrelated author bytes\0\n"
    );
    assert_eq!(
        fs::read(root.join("atom")).or_abort("remaining selection"),
        b"base\natom\nremainder\n"
    );
    assert_eq!(repository.index(), index);
}

#[test]
fn configured_hooks_path_preserves_nonbreaking_space_identity() {
    const REFUSAL_EXIT: i32 = 19;
    let fixture = super::NativeAuthor::arrange(super::MessageLocation::Tip, false);
    let repository = &fixture.repository;
    let root = &repository.environment.cwd;
    let external = super::TempDir::new().or_abort("external native hooks");
    let correct = external.path().join("hooks\u{a0}");
    let sibling = external.path().join("hooks");
    for (directory, script) in [
        (
            &correct,
            "#!/bin/sh\nif grep -qx 'Reject submitted atom' \"$1\"; then printf 'configured hook refusal\\n' >&2; exit 19; fi\n",
        ),
        (&sibling, "#!/bin/sh\nexit 0\n"),
    ] {
        arrange_executable_hook(directory, script);
    }
    let message = external.path().join("message");
    fs::write(&message, "Reject submitted atom\n").or_abort("native rejected message");
    let correct_config = format!("core.hooksPath={}", correct.display());
    let rejected = super::Command::new("git")
        .args(["-c", &correct_config, "hook", "run", "commit-msg", "--"])
        .arg(&message)
        .current_dir(root)
        .output()
        .or_abort("configured native hook control");
    assert_eq!(rejected.status.code(), Some(REFUSAL_EXIT));
    assert_eq!(rejected.stdout, b"");
    assert_eq!(rejected.stderr, b"configured hook refusal\n");
    repository.git(&[
        "-c",
        &format!("core.hooksPath={}", sibling.display()),
        "hook",
        "run",
        "commit-msg",
        "--",
        message.to_str().or_abort("control message path"),
    ]);
    arrange_hook_path(
        repository,
        correct.to_str().or_abort("configured UTF-8 hook path"),
    );
    repository.success(&["--gate", "test", "true", &fixture.source]);
    repository.write("atom", "base\natom\n");
    repository.git(&["add", "atom"]);
    repository.write("atom", "base\natom\nremainder\n");
    let index = repository.index();
    let files = super::log_observation_inventory(root);
    let refs = repository.git(&["show-ref"]);
    let head = repository.git(&["rev-parse", "HEAD"]);
    let journal = repository.journal();
    let hooks = super::log_observation_inventory(external.path());

    let (code, stdout) = repository.invoke(&["--message", "Reject submitted atom"]);

    let observed_index = repository.index();
    let mut observed_files = super::log_observation_inventory(root);
    assert_eq!(
        (code, stdout),
        (EXIT_SOFTWARE, String::new()),
        "{}",
        repository.output.stderr.borrow()
    );
    assert_eq!(
        repository.output.stderr.borrow().as_str(),
        "git command failed: git -c failed (exit 1)\n"
    );
    assert_eq!(observed_index, index);
    assert_eq!(repository.journal(), journal);
    assert_eq!(repository.git(&["rev-parse", "HEAD"]), head);
    assert_eq!(repository.git(&["show-ref"]), refs);
    assert_eq!(super::log_observation_inventory(external.path()), hooks);
    let log = observed_files.remove(&super::PathBuf::from(".git/factor/error.log"));
    assert!(matches!(log, Some(super::LogPath::File(_))));
    for (path, contents) in &files {
        assert_eq!(observed_files.get(path), Some(contents), "{path:?}");
    }
    let reachable = repository.git(&["rev-list", "--objects", "--all", "HEAD"]);
    let additions = observed_files
        .keys()
        .filter(|path| !files.contains_key(*path));
    for path in additions {
        let relative = path
            .strip_prefix(".git/objects")
            .or_abort("new loose object only");
        let spelling = relative.to_str().or_abort("loose object path");
        let mut components = spelling.split('/');
        let shard = components.next().or_abort("loose object shard");
        assert_eq!(shard.len(), 2);
        let digest = spelling.replace('/', "");
        assert!(digest.bytes().all(|byte| byte.is_ascii_hexdigit()));
        if let Some(suffix) = components.next() {
            assert_eq!(suffix.len(), 38);
            assert!(components.next().is_none());
            let object = format!("{shard}{suffix}");
            assert!(
                ["commit", "tree"].contains(&repository.git(&["cat-file", "-t", &object]).as_str())
            );
            assert!(!reachable.lines().any(|line| line.starts_with(&object)));
        } else {
            assert_eq!(observed_files.get(path), Some(&super::LogPath::Directory));
        }
    }
}
