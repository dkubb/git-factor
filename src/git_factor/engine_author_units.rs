#[test]
fn preserves_utf8_tip_author_under_non_utf8_log_output_encoding() {
    let fixture = super::NativeAuthor::arrange(super::MessageLocation::Tip, false);
    let repository = &fixture.repository;
    let root = &repository.environment.cwd;
    let expected = fixture.expected_start();

    let (code, stdout) = repository.invoke(&["--exec", "true", &fixture.source]);

    assert_eq!(code, EXIT_OK, "{}", repository.output.stderr.borrow());
    assert_eq!(repository.output.stderr.borrow().as_str(), "");
    let response: serde_json::Value = serde_json::from_str(&stdout).or_abort("start JSON");
    assert_eq!(response, expected);
    assert!(stdout.ends_with('\n'));
    assert_eq!(stdout.lines().count(), 1);
    let index = repository.index();
    let journal: serde_json::Value =
        serde_json::from_slice(&repository.journal()).or_abort("selecting journal");
    let source = journal
        .pointer("/state/source")
        .and_then(serde_json::Value::as_str)
        .or_abort("exposed source");
    assert_eq!(
        super::NativeAuthor::raw_author(repository, source),
        fixture.author
    );
    assert_eq!(
        journal
            .pointer("/state/phase")
            .and_then(serde_json::Value::as_str),
        Some("selecting")
    );
    assert_eq!(
        journal
            .get("checkpoint")
            .and_then(serde_json::Value::as_str),
        Some(repository.git(&["rev-parse", "main"]).as_str())
    );
    assert_eq!(
        journal
            .get("final_tree")
            .and_then(serde_json::Value::as_str),
        Some(fixture.final_tree.as_str())
    );
    assert_eq!(repository.git(&["rev-parse", "HEAD"]), fixture.base);
    assert_eq!(
        repository.git(&["--no-optional-locks", "diff", "--cached", "--quiet", "HEAD"]),
        ""
    );
    assert!(root.join(".git/rebase-merge").is_dir());
    assert!(!root.join(".git/rebase-apply").exists());
    assert_eq!(
        repository.git(&["rev-parse", "main^{tree}"]),
        fixture.final_tree
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
        fs::read(root.join(".git/config")).or_abort("config after start"),
        fixture.config
    );
    assert_eq!(
        fs::read(root.join("user")).or_abort("unrelated bytes"),
        b"unrelated author bytes\0\n"
    );
    assert_eq!(
        fs::read(root.join("atom")).or_abort("exposed remainder"),
        b"base\natom\nremainder\n"
    );
    assert_eq!(repository.index(), index);
}

#[test]
fn preserves_utf8_descendant_author_under_non_utf8_log_output_encoding() {
    let fixture = super::NativeAuthor::arrange(super::MessageLocation::Descendant, false);
    let repository = &fixture.repository;
    let root = &repository.environment.cwd;
    repository.success(&["--exec", "true", &fixture.source]);
    repository.write("atom", "base\natom\n");
    repository.git(&["add", "atom"]);
    repository.write("atom", "base\natom\nremainder\n");
    let mut expected = super::OriginalPool::expected_output();
    expected
        .as_object_mut()
        .or_abort("expected selection fields")
        .insert(
            "changes".to_owned(),
            serde_json::from_str(concat!(
                r#"{"unstaged":[{"path":"atom","kind":"text","added":1,"deleted":0}],"#,
                r#""untracked":["user"]}"#,
            ))
            .or_abort("expected remaining changes"),
        );

    let (code, stdout) = repository.invoke(&["--message", "Add selected atom"]);

    assert_eq!(code, EXIT_OK, "{}", repository.output.stderr.borrow());
    assert_eq!(repository.output.stderr.borrow().as_str(), "");
    let response: serde_json::Value = serde_json::from_str(&stdout).or_abort("capture JSON");
    assert_eq!(response, expected);
    assert!(stdout.ends_with('\n'));
    assert_eq!(stdout.lines().count(), 1);
    let index = repository.index();
    assert_eq!(
        super::NativeAuthor::raw_author(repository, "main"),
        fixture.author
    );
    let journal: serde_json::Value =
        serde_json::from_slice(&repository.journal()).or_abort("reopened journal");
    assert_eq!(
        journal
            .get("checkpoint")
            .and_then(serde_json::Value::as_str),
        Some(repository.git(&["rev-parse", "main"]).as_str())
    );
    assert_eq!(
        journal
            .pointer("/state/phase")
            .and_then(serde_json::Value::as_str),
        Some("selecting")
    );
    assert_eq!(
        repository.git(&["rev-parse", "main^{tree}"]),
        fixture.final_tree
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
    assert_eq!(
        journal
            .get("final_tree")
            .and_then(serde_json::Value::as_str),
        Some(fixture.final_tree.as_str())
    );
    assert_eq!(
        journal
            .pointer("/state/head")
            .and_then(serde_json::Value::as_str),
        Some(repository.git(&["rev-parse", "HEAD"]).as_str())
    );
    assert_eq!(repository.git(&["show", "HEAD:atom"]), "base\natom");
    assert_eq!(
        repository.git(&["--no-optional-locks", "diff", "--cached", "--quiet", "HEAD"]),
        ""
    );
    assert!(root.join(".git/rebase-merge").is_dir());
    assert!(!root.join(".git/rebase-apply").exists());
    assert_eq!(repository.index(), index);
    assert!(!root.join("tail").exists());
}

#[test]
fn preserves_raw_tip_author_with_a_non_utf8_source_encoding_header() {
    let fixture = super::NativeAuthor::arrange(super::MessageLocation::Tip, true);
    let repository = &fixture.repository;
    let root = &repository.environment.cwd;
    let expected = fixture.expected_start();

    let (code, stdout) = repository.invoke(&["--exec", "true", &fixture.source]);

    assert_eq!(code, EXIT_OK, "{}", repository.output.stderr.borrow());
    assert_eq!(repository.output.stderr.borrow().as_str(), "");
    let response: serde_json::Value = serde_json::from_str(&stdout).or_abort("start JSON");
    assert_eq!(response, expected);
    assert!(stdout.ends_with('\n'));
    assert_eq!(stdout.lines().count(), 1);
    let index = repository.index();
    let journal: serde_json::Value =
        serde_json::from_slice(&repository.journal()).or_abort("selecting journal");
    let source = journal
        .pointer("/state/source")
        .and_then(serde_json::Value::as_str)
        .or_abort("exposed source");
    assert_eq!(
        super::NativeAuthor::raw_author(repository, source),
        fixture.author
    );
    assert_eq!(
        journal
            .pointer("/state/phase")
            .and_then(serde_json::Value::as_str),
        Some("selecting")
    );
    assert_eq!(
        journal
            .get("checkpoint")
            .and_then(serde_json::Value::as_str),
        Some(repository.git(&["rev-parse", "main"]).as_str())
    );
    assert_eq!(
        journal
            .get("final_tree")
            .and_then(serde_json::Value::as_str),
        Some(fixture.final_tree.as_str())
    );
    assert_eq!(repository.git(&["rev-parse", "HEAD"]), fixture.base);
    assert_eq!(
        repository.git(&["--no-optional-locks", "diff", "--cached", "--quiet", "HEAD"]),
        ""
    );
    assert!(root.join(".git/rebase-merge").is_dir());
    assert!(!root.join(".git/rebase-apply").exists());
    assert_eq!(
        repository.git(&["rev-parse", "main^{tree}"]),
        fixture.final_tree
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
        fs::read(root.join(".git/config")).or_abort("config after start"),
        fixture.config
    );
    assert_eq!(
        fs::read(root.join("user")).or_abort("unrelated bytes"),
        b"unrelated author bytes\0\n"
    );
    assert_eq!(
        fs::read(root.join("atom")).or_abort("exposed remainder"),
        b"base\natom\nremainder\n"
    );
    assert_eq!(repository.index(), index);
}

#[test]
fn preserves_raw_descendant_author_with_a_non_utf8_source_encoding_header() {
    let fixture = super::NativeAuthor::arrange(super::MessageLocation::Descendant, true);
    let repository = &fixture.repository;
    let root = &repository.environment.cwd;
    repository.success(&["--exec", "true", &fixture.source]);
    repository.write("atom", "base\natom\n");
    repository.git(&["add", "atom"]);
    repository.write("atom", "base\natom\nremainder\n");
    let mut expected = super::OriginalPool::expected_output();
    expected
        .as_object_mut()
        .or_abort("expected selection fields")
        .insert(
            "changes".to_owned(),
            serde_json::from_str(concat!(
                r#"{"unstaged":[{"path":"atom","kind":"text","added":1,"deleted":0}],"#,
                r#""untracked":["user"]}"#,
            ))
            .or_abort("expected remaining changes"),
        );

    let (code, stdout) = repository.invoke(&["--message", "Add selected atom"]);

    assert_eq!(code, EXIT_OK, "{}", repository.output.stderr.borrow());
    assert_eq!(repository.output.stderr.borrow().as_str(), "");
    let response: serde_json::Value = serde_json::from_str(&stdout).or_abort("capture JSON");
    assert_eq!(response, expected);
    assert!(stdout.ends_with('\n'));
    assert_eq!(stdout.lines().count(), 1);
    let index = repository.index();
    assert_eq!(
        super::NativeAuthor::raw_author(repository, "main"),
        fixture.author
    );
    let journal: serde_json::Value =
        serde_json::from_slice(&repository.journal()).or_abort("reopened journal");
    assert_eq!(
        journal
            .get("checkpoint")
            .and_then(serde_json::Value::as_str),
        Some(repository.git(&["rev-parse", "main"]).as_str())
    );
    assert_eq!(
        journal
            .pointer("/state/phase")
            .and_then(serde_json::Value::as_str),
        Some("selecting")
    );
    assert_eq!(
        repository.git(&["rev-parse", "main^{tree}"]),
        fixture.final_tree
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
    assert_eq!(
        journal
            .get("final_tree")
            .and_then(serde_json::Value::as_str),
        Some(fixture.final_tree.as_str())
    );
    assert_eq!(
        journal
            .pointer("/state/head")
            .and_then(serde_json::Value::as_str),
        Some(repository.git(&["rev-parse", "HEAD"]).as_str())
    );
    assert_eq!(repository.git(&["show", "HEAD:atom"]), "base\natom");
    assert_eq!(
        repository.git(&["--no-optional-locks", "diff", "--cached", "--quiet", "HEAD"]),
        ""
    );
    assert!(root.join(".git/rebase-merge").is_dir());
    assert!(!root.join(".git/rebase-apply").exists());
    assert_eq!(repository.index(), index);
    assert!(!root.join("tail").exists());
}

#[test]
fn preserves_native_negative_zero_author_date() {
    let fixture = super::NativeAuthor::arrange_alias(super::NativeAuthorAlias::NegativeZero);
    let repository = &fixture.repository;
    let root = &repository.environment.cwd;
    let expected = fixture.expected_start();

    let (code, stdout) = repository.invoke(&["--exec", "true", &fixture.source]);

    assert_eq!(code, EXIT_OK, "{}", repository.output.stderr.borrow());
    assert_eq!(repository.output.stderr.borrow().as_str(), "");
    let response: serde_json::Value = serde_json::from_str(&stdout).or_abort("start JSON");
    assert_eq!(response, expected);
    assert!(stdout.ends_with('\n'));
    assert_eq!(stdout.lines().count(), 1);
    let index = repository.index();
    let journal: serde_json::Value =
        serde_json::from_slice(&repository.journal()).or_abort("selecting journal");
    let source = journal
        .pointer("/state/source")
        .and_then(serde_json::Value::as_str)
        .or_abort("exposed source");
    assert_eq!(
        super::NativeAuthor::raw_author(repository, source),
        fixture.author
    );
    assert_eq!(
        journal
            .pointer("/state/phase")
            .and_then(serde_json::Value::as_str),
        Some("selecting")
    );
    assert_eq!(
        journal
            .get("checkpoint")
            .and_then(serde_json::Value::as_str),
        Some(repository.git(&["rev-parse", "main"]).as_str())
    );
    assert_eq!(
        journal
            .get("final_tree")
            .and_then(serde_json::Value::as_str),
        Some(fixture.final_tree.as_str())
    );
    assert_eq!(repository.git(&["rev-parse", "HEAD"]), fixture.base);
    assert_eq!(
        repository.git(&["--no-optional-locks", "diff", "--cached", "--quiet", "HEAD"]),
        ""
    );
    assert!(root.join(".git/rebase-merge").is_dir());
    assert!(!root.join(".git/rebase-apply").exists());
    assert_eq!(
        repository.git(&["rev-parse", "main^{tree}"]),
        fixture.final_tree
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
        fs::read(root.join(".git/config")).or_abort("config after start"),
        fixture.config
    );
    assert_eq!(
        fs::read(root.join("user")).or_abort("unrelated bytes"),
        b"unrelated author bytes\0\n"
    );
    assert_eq!(
        fs::read(root.join("atom")).or_abort("exposed remainder"),
        b"base\natom\nremainder\n"
    );
    assert_eq!(repository.index(), index);
}

#[test]
fn preserves_native_trailing_name_space() {
    let fixture = super::NativeAuthor::arrange_alias(super::NativeAuthorAlias::TrailingNameSpace);
    let repository = &fixture.repository;
    let root = &repository.environment.cwd;
    let expected = fixture.expected_start();

    let (code, stdout) = repository.invoke(&["--exec", "true", &fixture.source]);

    assert_eq!(code, EXIT_OK, "{}", repository.output.stderr.borrow());
    assert_eq!(repository.output.stderr.borrow().as_str(), "");
    let response: serde_json::Value = serde_json::from_str(&stdout).or_abort("start JSON");
    assert_eq!(response, expected);
    assert!(stdout.ends_with('\n'));
    assert_eq!(stdout.lines().count(), 1);
    let index = repository.index();
    let journal: serde_json::Value =
        serde_json::from_slice(&repository.journal()).or_abort("selecting journal");
    let source = journal
        .pointer("/state/source")
        .and_then(serde_json::Value::as_str)
        .or_abort("exposed source");
    assert_eq!(
        super::NativeAuthor::raw_author(repository, source),
        fixture.author
    );
    assert_eq!(
        journal
            .pointer("/state/phase")
            .and_then(serde_json::Value::as_str),
        Some("selecting")
    );
    assert_eq!(
        journal
            .get("checkpoint")
            .and_then(serde_json::Value::as_str),
        Some(repository.git(&["rev-parse", "main"]).as_str())
    );
    assert_eq!(
        journal
            .get("final_tree")
            .and_then(serde_json::Value::as_str),
        Some(fixture.final_tree.as_str())
    );
    assert_eq!(repository.git(&["rev-parse", "HEAD"]), fixture.base);
    assert_eq!(
        repository.git(&["--no-optional-locks", "diff", "--cached", "--quiet", "HEAD"]),
        ""
    );
    assert!(root.join(".git/rebase-merge").is_dir());
    assert!(!root.join(".git/rebase-apply").exists());
    assert_eq!(
        repository.git(&["rev-parse", "main^{tree}"]),
        fixture.final_tree
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
        fs::read(root.join(".git/config")).or_abort("config after start"),
        fixture.config
    );
    assert_eq!(
        fs::read(root.join("user")).or_abort("unrelated bytes"),
        b"unrelated author bytes\0\n"
    );
    assert_eq!(
        fs::read(root.join("atom")).or_abort("exposed remainder"),
        b"base\natom\nremainder\n"
    );
    assert_eq!(repository.index(), index);
}
#[test]
fn preserves_native_tip_author_at_early_epoch_boundary() {
    let fixture = super::NativeAuthor::arrange_epoch(super::MessageLocation::Tip, 99_999_999);
    let repository = &fixture.repository;
    let root = &repository.environment.cwd;
    let expected = fixture.expected_start();

    let (code, stdout) = repository.invoke(&["--exec", "true", &fixture.source]);

    assert_eq!(code, EXIT_OK, "{}", repository.output.stderr.borrow());
    assert_eq!(repository.output.stderr.borrow().as_str(), "");
    let response: serde_json::Value = serde_json::from_str(&stdout).or_abort("start JSON");
    assert_eq!(response, expected);
    assert!(stdout.ends_with('\n'));
    assert_eq!(stdout.lines().count(), 1);
    let index = repository.index();
    let journal: serde_json::Value =
        serde_json::from_slice(&repository.journal()).or_abort("selecting journal");
    let source = journal
        .pointer("/state/source")
        .and_then(serde_json::Value::as_str)
        .or_abort("exposed source");
    assert_eq!(
        super::NativeAuthor::raw_author(repository, source),
        fixture.author
    );
    assert_eq!(
        journal
            .pointer("/state/phase")
            .and_then(serde_json::Value::as_str),
        Some("selecting")
    );
    assert_eq!(
        journal
            .get("checkpoint")
            .and_then(serde_json::Value::as_str),
        Some(repository.git(&["rev-parse", "main"]).as_str())
    );
    assert_eq!(
        journal
            .get("final_tree")
            .and_then(serde_json::Value::as_str),
        Some(fixture.final_tree.as_str())
    );
    assert_eq!(repository.git(&["rev-parse", "HEAD"]), fixture.base);
    assert_eq!(
        repository.git(&["--no-optional-locks", "diff", "--cached", "--quiet", "HEAD"]),
        ""
    );
    assert!(root.join(".git/rebase-merge").is_dir());
    assert!(!root.join(".git/rebase-apply").exists());
    assert_eq!(
        repository.git(&["rev-parse", "main^{tree}"]),
        fixture.final_tree
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
        fs::read(root.join(".git/config")).or_abort("config after start"),
        fixture.config
    );
    assert_eq!(
        fs::read(root.join("user")).or_abort("unrelated bytes"),
        b"unrelated author bytes\0\n"
    );
    assert_eq!(
        fs::read(root.join("atom")).or_abort("exposed remainder"),
        b"base\natom\nremainder\n"
    );
    assert_eq!(repository.index(), index);
}

#[test]
fn preserves_native_descendant_author_at_zero_epoch() {
    let fixture = super::NativeAuthor::arrange_epoch(super::MessageLocation::Descendant, 0);
    let repository = &fixture.repository;
    let root = &repository.environment.cwd;
    repository.success(&["--exec", "true", &fixture.source]);
    repository.write("atom", "base\natom\n");
    repository.git(&["add", "atom"]);
    repository.write("atom", "base\natom\nremainder\n");
    let mut expected = super::OriginalPool::expected_output();
    expected
        .as_object_mut()
        .or_abort("expected selection fields")
        .insert(
            "changes".to_owned(),
            serde_json::from_str(concat!(
                r#"{"unstaged":[{"path":"atom","kind":"text","added":1,"deleted":0}],"#,
                r#""untracked":["user"]}"#,
            ))
            .or_abort("expected remaining changes"),
        );

    let (code, stdout) = repository.invoke(&["--message", "Add selected atom"]);

    assert_eq!(code, EXIT_OK, "{}", repository.output.stderr.borrow());
    assert_eq!(repository.output.stderr.borrow().as_str(), "");
    let response: serde_json::Value = serde_json::from_str(&stdout).or_abort("capture JSON");
    assert_eq!(response, expected);
    assert!(stdout.ends_with('\n'));
    assert_eq!(stdout.lines().count(), 1);
    let index = repository.index();
    assert_eq!(
        super::NativeAuthor::raw_author(repository, "main"),
        fixture.author
    );
    let journal: serde_json::Value =
        serde_json::from_slice(&repository.journal()).or_abort("reopened journal");
    assert_eq!(
        journal
            .get("checkpoint")
            .and_then(serde_json::Value::as_str),
        Some(repository.git(&["rev-parse", "main"]).as_str())
    );
    assert_eq!(
        journal
            .pointer("/state/phase")
            .and_then(serde_json::Value::as_str),
        Some("selecting")
    );
    assert_eq!(
        repository.git(&["rev-parse", "main^{tree}"]),
        fixture.final_tree
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
    assert_eq!(
        journal
            .get("final_tree")
            .and_then(serde_json::Value::as_str),
        Some(fixture.final_tree.as_str())
    );
    assert_eq!(
        journal
            .pointer("/state/head")
            .and_then(serde_json::Value::as_str),
        Some(repository.git(&["rev-parse", "HEAD"]).as_str())
    );
    assert_eq!(repository.git(&["show", "HEAD:atom"]), "base\natom");
    assert_eq!(
        repository.git(&["--no-optional-locks", "diff", "--cached", "--quiet", "HEAD"]),
        ""
    );
    assert!(root.join(".git/rebase-merge").is_dir());
    assert!(!root.join(".git/rebase-apply").exists());
    assert_eq!(repository.index(), index);
    assert!(!root.join("tail").exists());
}
