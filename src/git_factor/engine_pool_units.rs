use crate::test_support::OrAbort as _;

#[test]
fn captures_original_pool_directory_and_restarts_selection() {
    let fixture = super::OriginalPool::arrange(super::PoolWorld::Original, "saved remainder\n");
    let expected = super::OriginalPool::expected_output();
    let arguments = ["git-factor", "--message", "Remove original file"].map(super::OsString::from);

    let code = super::main_entry_with_vec(fixture.context().io, Ok(fixture.context()), &arguments);

    assert_eq!(code, super::EXIT_OK, "{}", fixture.stderr());
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&fixture.stdout()).or_abort("normalized result"),
        expected
    );
    assert_eq!(fixture.stderr(), "");
    let journal = fixture.journal();
    let (recorded_atom, recorded_head) = match journal.state {
        super::super::Phase::Selecting { base, head, .. } => (base, Some(head)),
        super::super::Phase::Closing { .. }
        | super::super::Phase::Opening { .. }
        | super::super::Phase::Preparing { .. }
        | super::super::Phase::Replaying { .. }
        | super::super::Phase::Verified { .. } => (None, None),
    };
    let atom = recorded_atom.or_abort("successful capture must restart selection");
    let head = recorded_head.or_abort("selection retains detached HEAD");
    assert_eq!(head, atom);
    assert_eq!(fixture.git(&["rev-parse", "HEAD"]), atom.as_str());
    assert_eq!(fixture.git(&["rev-parse", "HEAD^"]), fixture.base());
    assert_eq!(
        fixture.git(&["rev-parse", "HEAD^{tree}"]),
        fixture.deletion_tree()
    );
    assert_eq!(fixture.git(&["write-tree"]), fixture.deletion_tree());
    assert_eq!(
        fixture.git(&["rev-parse", "refs/heads/main"]),
        journal.checkpoint.as_str()
    );
    assert_eq!(
        fixture.git(&["rev-parse", "refs/heads/main^{tree}"]),
        fixture.final_tree()
    );
    assert_eq!(
        fixture.git(&[
            "show-ref",
            "--verify",
            "refs/heads/foreign",
            "refs/tags/foreign"
        ]),
        fixture.foreign_refs()
    );
    assert_eq!(fixture.git(&["diff", "--name-only"]), "");
    assert_eq!(fixture.frame().remainder, fixture.contents());
}

#[test]
fn refuses_changed_pool_directory_without_mutating_selection() {
    let fixture = super::OriginalPool::arrange(super::PoolWorld::Edited, "saved remainder\n");
    let before = fixture.frame();
    let arguments = ["git-factor", "--message", "Remove original file"].map(super::OsString::from);

    let code = super::main_entry_with_vec(fixture.context().io, Ok(fixture.context()), &arguments);

    let after = fixture.frame();
    assert_eq!(code, EXIT_TEMPFAIL);
    assert_eq!(fixture.stdout(), "");
    assert_eq!(fixture.stderr(), fixture.diagnostic());
    assert_eq!(after, before);
    assert_eq!(fixture.git(&["write-tree"]), fixture.deletion_tree());
}
