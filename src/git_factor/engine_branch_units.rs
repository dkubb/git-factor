#[test]
fn start_preserves_unicode_branch_identity_beside_a_same_tip_alias() {
    let fixture = super::NativeBranch::arrange();
    let native = &fixture.native;
    let repository = &native.repository;
    let expected = native.expected_start();

    let (code, stdout) = repository.invoke(&["--exec", "true", &native.source]);

    let after = fixture.frame();
    let journal: serde_json::Value =
        serde_json::from_slice(&repository.journal()).or_abort("observed session journal");
    assert_eq!(
        journal.get("branch").and_then(serde_json::Value::as_str),
        Some("refs/heads/topic\u{a0}")
    );
    assert_eq!(code, EXIT_OK, "{}", repository.output.stderr.borrow());
    assert_eq!(repository.output.stderr.borrow().as_str(), "");
    let response: serde_json::Value = serde_json::from_str(&stdout).or_abort("start JSON");
    assert_eq!(response, expected);
    assert!(stdout.ends_with('\n'));
    assert_eq!(stdout.lines().count(), 1);
    assert_eq!(
        journal
            .get("checkpoint")
            .and_then(serde_json::Value::as_str),
        Some(native.source.as_str())
    );
    assert_eq!(
        journal
            .get("final_tree")
            .and_then(serde_json::Value::as_str),
        Some(native.final_tree.as_str())
    );
    assert_eq!(
        journal
            .pointer("/state/phase")
            .and_then(serde_json::Value::as_str),
        Some("selecting")
    );
    assert_eq!(after.head, native.base);
    assert_eq!(fixture.protected_refs(), fixture.protected);
    assert_eq!(
        fs::read(
            repository
                .environment
                .cwd
                .join(".git/rebase-merge/head-name")
        )
        .or_abort("native owning branch"),
        "refs/heads/topic\u{a0}\n".as_bytes()
    );
    assert_eq!(
        fs::read(repository.environment.cwd.join(".git/config")).or_abort("config"),
        native.config
    );
    assert_eq!(
        fs::read(repository.environment.cwd.join("user")).or_abort("user bytes"),
        b"unrelated author bytes\0\n"
    );
    assert_eq!(
        fs::read(repository.environment.cwd.join("atom")).or_abort("selection bytes"),
        b"base\natom\nremainder\n"
    );
    assert_eq!(repository.index(), after.index);
}

#[test]
fn continue_preserves_unicode_branch_session_and_protected_aliases() {
    let fixture = super::NativeBranch::arrange();
    let native = &fixture.native;
    let repository = &native.repository;
    repository.success(&["--exec", "true", &native.source]);
    let before = fixture.frame();
    let mut expected = native.expected_start();
    expected
        .as_object_mut()
        .or_abort("selection response fields")
        .insert(
            "operation".to_owned(),
            serde_json::Value::String("continue".to_owned()),
        );

    let (code, stdout) = repository.invoke(&["--continue"]);

    let after = fixture.frame();
    assert_eq!(code, EXIT_OK, "{}", repository.output.stderr.borrow());
    assert_eq!(repository.output.stderr.borrow().as_str(), "");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&stdout).or_abort("continue JSON"),
        expected
    );
    assert!(stdout.ends_with('\n'));
    assert_eq!(stdout.lines().count(), 1);
    assert_eq!(after, before);
    assert_eq!(fixture.protected_refs(), fixture.protected);
}

#[test]
fn abort_restores_unicode_branch_without_moving_same_tip_aliases() {
    use std::process::Command;
    let fixture = super::NativeBranch::arrange();
    let native = &fixture.native;
    let repository = &native.repository;
    repository.success(&["--exec", "true", &native.source]);
    let before = fixture.frame();
    let expected_refs = before
        .refs
        .lines()
        .filter(|line| !line.ends_with(" refs/factor/session-lease"))
        .collect::<Vec<_>>()
        .join("\n");

    let (code, stdout) = repository.invoke(&["--abort"]);

    let after = fixture.frame();
    assert_eq!(code, EXIT_OK, "{}", repository.output.stderr.borrow());
    assert_eq!(repository.output.stderr.borrow().as_str(), "");
    assert_eq!(
        stdout,
        "{\"operation\":\"abort\",\"rebase\":{\"in_progress\":false},\"actions\":{}}\n"
    );
    assert_eq!(after.head, native.source);
    let branch = Command::new("git")
        .args(["rev-parse", "--symbolic-full-name", "HEAD"])
        .current_dir(&repository.environment.cwd)
        .output()
        .or_abort("exact native branch observation");
    assert!(branch.status.success());
    assert_eq!(branch.stdout, "refs/heads/topic\u{a0}\n".as_bytes());
    assert_eq!(
        repository.git(&["rev-parse", "HEAD^{tree}"]),
        native.final_tree
    );
    assert_eq!(fixture.protected_refs(), fixture.protected);
    assert_eq!(after.refs, expected_refs);
    assert_eq!(
        fs::read(repository.environment.cwd.join(".git/config")).or_abort("config"),
        native.config
    );
    assert_eq!(
        fs::read(repository.environment.cwd.join("user")).or_abort("user bytes"),
        b"unrelated author bytes\0\n"
    );
    assert_eq!(
        fs::read(repository.environment.cwd.join("atom")).or_abort("source bytes"),
        b"base\natom\nremainder\n"
    );
    assert!(
        !repository
            .environment
            .cwd
            .join(".git/factor-journal.json")
            .exists()
    );
    assert!(
        !repository
            .environment
            .cwd
            .join(".git/rebase-merge")
            .exists()
    );
    assert_eq!(repository.index(), after.index);
}

#[test]
fn finish_completes_unicode_branch_without_moving_same_tip_aliases() {
    let fixture = super::NativeBranch::arrange();
    let native = &fixture.native;
    let repository = &native.repository;
    let root = &repository.environment.cwd;
    repository.success(&["--exec", "true", &native.source]);
    let saved: serde_json::Value =
        serde_json::from_slice(&repository.journal()).or_abort("saved native branch ownership");
    assert_eq!(
        saved.get("branch").or_abort("saved branch"),
        "refs/heads/topic\u{a0}"
    );
    let before = fixture.frame();
    let proof_reference = format!(
        "refs/factor/gates/f32a5804e292d30bedf68f62d32fb75d87e99fd9/{}",
        native.final_tree
    );
    let expected_refs = before
        .refs
        .lines()
        .filter(|line| {
            !line.ends_with(" refs/factor/session-lease")
                && !line.ends_with(" refs/heads/topic\u{a0}")
                && !line.ends_with(&format!(" {proof_reference}"))
        })
        .collect::<Vec<_>>()
        .join("\n");

    let (code, stdout) = repository.invoke(&["--finish"]);

    let after = fixture.frame();
    assert_eq!(
        (code, stdout, repository.output.stderr.borrow().clone()),
        (
            EXIT_OK,
            "{\"operation\":\"finish\",\"result\":\"complete\",\"split_count\":1}\n".to_owned(),
            String::new()
        )
    );
    let branch = super::Command::new("git")
        .args(["rev-parse", "--symbolic-full-name", "HEAD"])
        .current_dir(root)
        .output()
        .or_abort("native terminal branch identity");
    assert!(branch.status.success());
    assert_eq!(branch.stdout, "refs/heads/topic\u{a0}\n".as_bytes());
    assert_eq!(
        repository.git(&["rev-parse", "HEAD^{tree}"]),
        native.final_tree
    );
    assert_eq!(fixture.protected_refs(), fixture.protected);
    let proof = repository.git(&["rev-parse", "--verify", &proof_reference]);
    assert_eq!(
        repository.git(&["rev-parse", &format!("{proof}^{{tree}}")]),
        native.final_tree
    );
    assert_eq!(
        repository.git(&[
            "show",
            "-s",
            "--format=%(trailers:only,unfold=true)",
            &proof
        ]),
        format!(
            "Gate-exec-f32a5804e292d30bedf68f62d32fb75d87e99fd9: f32a5804e292d30bedf68f62d32fb75d87e99fd9 {}",
            native.final_tree
        )
    );
    assert_eq!(
        repository.git(&["rev-parse", "refs/heads/topic\u{a0}"]),
        after.head
    );
    assert_eq!(
        after
            .refs
            .lines()
            .filter(|line| !line.ends_with(" refs/heads/topic\u{a0}")
                && !line.ends_with(&format!(" {proof_reference}")))
            .collect::<Vec<_>>()
            .join("\n"),
        expected_refs
    );
    assert_eq!(
        fs::read(root.join(".git/config")).or_abort("config"),
        native.config
    );
    assert_eq!(
        fs::read(root.join("user")).or_abort("unrelated bytes"),
        b"unrelated author bytes\0\n"
    );
    assert_eq!(
        fs::read(root.join("atom")).or_abort("completed selection"),
        b"base\natom\nremainder\n"
    );
    assert!(!root.join(".git/factor-journal.json").exists());
    assert!(!root.join(".git/rebase-merge").exists());
    assert!(
        repository
            .git(&["diff", "--cached", "--name-only"])
            .is_empty()
    );
    assert!(repository.git(&["diff", "--name-only"]).is_empty());
    assert_eq!(repository.index(), after.index);
}
