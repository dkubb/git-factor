mod bind {
    use super::GateSpec;
    use crate::test_support::{OrAbort as _, ResultOrAbort as _};

    #[test]
    fn round_trip_rebinds_command_identity_without_persisting_hashes() {
        super::check_spec_round_trip("Test-1", "printf hello");
    }

    #[test]
    fn rejects_injected_command_identity() {
        let raw: serde_json::Value = serde_json::from_str(
            r#"[{"name": "test", "command": "false", "command_hash": "old"}]"#,
        )
        .or_abort("literal hash injection");

        let error =
            serde_json::from_value::<Vec<GateSpec>>(raw).err_or_abort("untrusted hash field");

        assert_eq!(
            error.to_string(),
            "unknown field `command_hash`, expected `command` or `name`"
        );
    }
}

mod gate {
    mod command {
        use super::super::{command, ctx, fixture, named};
        use crate::test_support::OrAbort as _;

        #[test]
        fn borrows_exact_command_bytes() {
            let directory = fixture();
            let context = ctx(directory.path());
            let gate = named(&context, "test", command(" printf check ")).or_abort("gate");

            let observed = gate.command();

            assert_eq!(observed.as_str(), " printf check ");
        }
    }
}

mod gate_name {
    mod deserialize {
        #[expect(
            clippy::module_inception,
            reason = "RTI scopes trait methods by both trait and method namespaces"
        )]
        mod deserialize {
            use super::super::super::super::GateName;
            use crate::test_support::{OrAbort as _, ResultOrAbort as _};

            #[test]
            fn rejects_invalid_suffix_at_journal_ingress() {
                let error = serde_json::from_value::<GateName>(serde_json::Value::String(
                    "1bad".to_owned(),
                ))
                .err_or_abort("invalid suffix");

                assert_eq!(
                    error.to_string(),
                    "git command failed: gate names must be unique ASCII trailer names"
                );
            }

            #[test]
            fn preserves_case_and_hyphens_at_journal_ingress() {
                let name =
                    serde_json::from_value::<GateName>(serde_json::Value::String("A-1".to_owned()))
                        .or_abort("valid suffix");

                assert_eq!(
                    serde_json::to_value(name).or_abort("suffix serialization"),
                    serde_json::Value::String("A-1".to_owned())
                );
            }
        }
        mod deserialize_in_place {
            use super::super::super::super::GateName;
            use crate::test_support::{OrAbort as _, ResultOrAbort as _};
            use serde::Deserialize as _;

            #[test]
            fn rejects_invalid_suffix_without_overwriting_previous_name() {
                let mut name =
                    serde_json::from_value::<GateName>(serde_json::Value::String("A".to_owned()))
                        .or_abort("valid name");

                let error = GateName::deserialize_in_place(
                    serde_json::Value::String("1bad".to_owned()),
                    &mut name,
                )
                .err_or_abort("invalid in-place suffix");

                assert_eq!(
                    error.to_string(),
                    "git command failed: gate names must be unique ASCII trailer names"
                );
                assert_eq!(
                    serde_json::to_value(name).or_abort("preserved name"),
                    serde_json::Value::String("A".to_owned())
                );
            }

            #[test]
            fn replaces_previous_name_only_after_admission() {
                let mut name =
                    serde_json::from_value::<GateName>(serde_json::Value::String("A".to_owned()))
                        .or_abort("valid name");

                GateName::deserialize_in_place(
                    serde_json::Value::String("Z-9".to_owned()),
                    &mut name,
                )
                .or_abort("admitted replacement");

                assert_eq!(
                    serde_json::to_value(name).or_abort("replacement name"),
                    serde_json::Value::String("Z-9".to_owned())
                );
            }
        }
    }
    mod fmt {
        mod display {
            mod fmt {
                use super::super::super::super::super::GateName;
                use crate::test_support::OrAbort as _;

                #[test]
                fn retains_exact_admitted_suffix() {
                    let name = serde_json::from_value::<GateName>(serde_json::Value::String(
                        "A-1".to_owned(),
                    ))
                    .or_abort("name");

                    let rendered = name.to_string();

                    assert_eq!(rendered, "A-1");
                }
            }
        }
    }
}

mod gate_set {
    mod as_slice {
        use super::super::{GateSet, command, ctx, fixture, named};
        use crate::test_support::OrAbort as _;

        #[test]
        fn retains_cli_order() {
            let directory = fixture();
            let context = ctx(directory.path());
            let first = named(&context, "Z", command("true")).or_abort("first");
            let second = named(&context, "A", command("false")).or_abort("second");
            let gates = GateSet::new(vec![first, second]).or_abort("ordered gates");

            let observed = gates.as_slice();

            assert_eq!(
                observed
                    .iter()
                    .map(|gate| gate.name.as_str())
                    .collect::<Vec<_>>(),
                vec!["Z", "A"]
            );
        }
    }
    mod new {
        use super::super::{GateSet, command, ctx, fixture, named};
        use crate::test_support::{OrAbort as _, ResultOrAbort as _};

        #[test]
        fn refuses_empty_set() {
            let error = GateSet::new(Vec::new()).err_or_abort("empty set");

            assert_eq!(
                error.to_string(),
                "git command failed: supply at least one gate"
            );
        }

        #[test]
        fn refuses_case_fold_duplicate_names() {
            let directory = fixture();
            let context = ctx(directory.path());
            let first = named(&context, "Test", command("true")).or_abort("first");
            let second = named(&context, "test", command("false")).or_abort("second");

            let error = GateSet::new(vec![first, second]).err_or_abort("duplicate names");

            assert_eq!(
                error.to_string(),
                "git command failed: gate names must be unique ASCII trailer names"
            );
        }

        #[test]
        fn admits_singleton() {
            let directory = fixture();
            let context = ctx(directory.path());
            let gate = named(&context, "A", command("true")).or_abort("gate");

            let admitted = GateSet::new(vec![gate]).or_abort("single gate");

            assert_eq!(admitted.as_slice().len(), 1);
        }
    }
    mod serialize {
        use super::super::{GateSet, command, ctx, fixture, named};
        use crate::test_support::OrAbort as _;

        #[test]
        fn emits_only_command_and_name_in_cli_order() {
            let directory = fixture();
            let context = ctx(directory.path());
            let first = named(&context, "Z", command("true")).or_abort("first");
            let second = named(&context, "A", command("false")).or_abort("second");
            let gates = GateSet::new(vec![first, second]).or_abort("ordered gates");

            let observed = serde_json::to_value(gates).or_abort("serialization");

            assert_eq!(
                observed,
                serde_json::Value::Array(vec![
                    super::super::spec_value("Z", "true"),
                    super::super::spec_value("A", "false")
                ])
            );
        }
    }
}

mod legacy {
    #[test]
    fn command_identity_determines_legacy_name() {
        super::check_identity("test -f atom");
    }
}

mod named {
    #[test]
    fn command_bytes_determine_identity() {
        super::check_identity("test -f atom");
    }
}

mod parse_records {
    #[test]
    fn preserves_unrelated_lines_when_removing_a_record() {
        super::check_raw_records("Gate-test", "hash tree");
    }
}

mod physical_observation {
    use super::{
        GateSet, HEAD_TREE, TreeHash, command, commit_message, ctx, fixture, git, named, proof_ref,
        verify_observed,
    };
    use crate::test_support::{OrAbort as _, ResultOrAbort as _};
    use core::cell::Cell;

    #[test]
    fn refuses_stamp_publication_when_post_gate_observation_fails() {
        let directory = fixture();
        let context = ctx(directory.path());
        let gate = named(&context, "test", command("true")).or_abort("gate");
        let tree =
            TreeHash::new(&git(directory.path(), &["rev-parse", HEAD_TREE])).or_abort("tree");
        let reference = proof_ref(&gate, &tree);
        let gates = GateSet::new(vec![gate]).or_abort("ordered gate");
        let observations = Cell::new(false);
        let observe = |observed: &TreeHash| {
            assert_eq!(observed, &tree);
            if observations.replace(true) {
                Err(super::failure("physical observation rejected"))
            } else {
                Ok(())
            }
        };

        let error = verify_observed(
            &context,
            &gates,
            &directory.path().join(".git/hooks"),
            &observe,
        )
        .err_or_abort("physical observation rejects selected tree");

        assert_eq!(
            error.to_string(),
            "git command failed: physical observation rejected"
        );
        assert_eq!(
            commit_message(&context).or_abort("unmodified message"),
            "Add atom\n"
        );
        assert_eq!(git(directory.path(), &["for-each-ref", &reference]), "");
    }
}

mod validate_names {
    #[test]
    fn admits_only_distinct_trailer_keys() {
        super::check_names("test-1");
    }
}

mod verify_observed {

    use super::{HEAD_TREE, Permissions, TempDir, fs, slice};
    use std::os::unix::fs::PermissionsExt as _;
    #[test]
    fn refuses_git_recognized_trailers_outside_the_losslessly_mapped_paragraph() {
        let directory = super::fixture();
        let context = super::ctx(directory.path());
        let message = "Subject\n\nGate-test: old tree\n\n# trailing comment\n";
        super::amend_message(&context, message).or_abort("native message layout");
        let gate = super::named(&context, "test", super::command("false")).or_abort("gate");

        let error = super::verify(&context, &[gate], &directory.path().join(".git/hooks"))
            .err_or_abort("unmappable native trailer layout");

        assert_eq!(
            error.to_string(),
            "git command failed: cannot map Git trailer records without changing the message"
        );
        assert_eq!(
            super::commit_message(&context).or_abort("preserved message"),
            message
        );
    }

    #[test]
    fn ordered_stamps_cache_only_exact_positive_trees() {
        super::check_verify("Add atom");
    }
    #[test]
    fn rejects_native_head_worktree_and_attachment_mutation() {
        for script in [
            "printf mutated >> atom",
            "git commit --quiet --amend --no-verify -m Changed",
            "git checkout --quiet -b escaped",
        ] {
            let directory = super::fixture();
            let ctx = super::ctx(directory.path());
            let gate = super::named(&ctx, "test", super::command(script)).or_abort("gate");
            let _rejected = super::verify(&ctx, &[gate], &directory.path().join(".git/hooks"))
                .err_or_abort("mutating gate");
            assert!(
                !super::commit_message(&ctx)
                    .or_abort("message")
                    .contains("Gate-test:")
            );
        }
    }
    #[test]
    fn folded_stamps_pass_native_line_policy_and_reuse_exact_tree_proofs() {
        let directory = super::fixture();
        let repo = directory.path();
        let context = super::ctx(repo);
        let markers = TempDir::new().or_abort("external gate marker");
        let marker = markers.path().join("gate");
        let text = format!("printf g >> '{}'", marker.display());
        let named = super::named(&context, "test", super::command(&text)).or_abort("named gate");
        let legacy = super::legacy(&context, super::command(&text)).or_abort("legacy gate");
        let tree = super::git(repo, &["rev-parse", HEAD_TREE]);
        let expected = format!(
            "Gate-test: {} {tree}\nGate-{}: {} {tree}",
            named.command_hash, legacy.name, legacy.command_hash
        );
        let hooks = repo.join(".git/hooks");
        let hook = hooks.join("commit-msg");
        fs::write(
            &hook,
            "#!/bin/sh\nawk 'length($0) > 72 { exit 19 }' \"$1\"\n",
        )
        .or_abort("native physical line policy");
        fs::set_permissions(&hook, Permissions::from_mode(0o755)).or_abort("hook permissions");

        let observed = super::verify(&context, &[named, legacy], &hooks);

        observed.or_abort("native folded final message acceptance");
        assert_eq!(
            fs::read_to_string(&marker).or_abort("one tree gate execution"),
            "g"
        );
        assert_eq!(super::git(repo, &["rev-parse", HEAD_TREE]), tree);
        let message = super::commit_message(&context).or_abort("accepted final message");
        assert!(message.lines().all(|line| line.len() <= 72));
        assert!(message.starts_with("Add atom\n\n"));
        assert_eq!(
            super::git(
                repo,
                &["show", "-s", "--format=%(trailers:only,unfold=true)"]
            ),
            expected
        );
    }

    #[test]
    fn final_stamped_message_must_pass_native_message_hooks() {
        let directory = super::fixture();
        let repo = directory.path();
        let hook = repo.join(".git/hooks/commit-msg");
        fs::write(&hook, "#!/bin/sh\nexit 19\n").or_abort("hook");
        fs::set_permissions(&hook, Permissions::from_mode(0o755)).or_abort("permissions");
        let ctx = super::ctx(repo);
        let gate = super::named(&ctx, "test", super::command("test -f atom")).or_abort("gate");
        let _rejected =
            super::verify(&ctx, &[gate], &repo.join(".git/hooks")).err_or_abort("rejecting hook");
        assert!(
            super::reusable_proof(
                &ctx,
                &super::named(&ctx, "test", super::command("test -f atom")).or_abort("same gate"),
                &super::TreeHash::new(&super::git(repo, &["rev-parse", HEAD_TREE]))
                    .or_abort("tree")
            )
            .or_abort("positive proof survives message rejection")
        );
    }
    #[test]
    fn positive_proof_survives_final_message_rejection_and_rechecks_new_message() {
        let directory = super::fixture();
        let repo = directory.path();
        let ctx = super::ctx(repo);
        let markers = TempDir::new().or_abort("external markers");
        let gate_marker = markers.path().join("gate");
        let hook_marker = markers.path().join("hook");
        let hooks = repo.join(".git/hooks");
        let hook = hooks.join("commit-msg");
        fs::write(
            &hook,
            format!(
                "#!/bin/sh\nprintf h >> '{}'\ngrep -q '^Gate-test:' \"$1\" && exit 19\nexit 0\n",
                hook_marker.display()
            ),
        )
        .or_abort("rejecting final trailer hook");
        fs::set_permissions(&hook, Permissions::from_mode(0o755)).or_abort("permissions");
        let text = format!("printf g >> '{}'", gate_marker.display());
        let gate = super::named(&ctx, "test", super::command(&text)).or_abort("gate");
        let _rejected = super::verify(&ctx, slice::from_ref(&gate), &hooks)
            .err_or_abort("rejecting native hook");
        assert_eq!(fs::read_to_string(&gate_marker).or_abort("gate count"), "g");
        assert_eq!(
            fs::read_to_string(&hook_marker).or_abort("rejected hook count"),
            "h"
        );
        // Different message, same tree. Removing the stamp demonstrates actual ref reuse.
        super::amend_message(&ctx, "Accepted message\n").or_abort("new candidate message");
        fs::write(
            &hook,
            format!(
                "#!/bin/sh\nprintf h >> '{}'\nexit 0\n",
                hook_marker.display()
            ),
        )
        .or_abort("accepting hook");
        super::verify(&ctx, &[gate], &hooks)
            .or_abort("cached proof and independent hook acceptance");
        assert_eq!(
            fs::read_to_string(&gate_marker).or_abort("one normal gate"),
            "g"
        );
        assert_eq!(
            fs::read_to_string(&hook_marker).or_abort("two native checks"),
            "hh"
        );
        assert!(
            super::commit_message(&ctx)
                .or_abort("accepted message")
                .starts_with("Accepted message\n\nGate-test:")
        );
        // Renaming a gate cannot invalidate a command/tree proof.
        super::amend_message(&ctx, "Renamed gate\n").or_abort("fresh message");
        let renamed = super::named(&ctx, "renamed", super::command(&text)).or_abort("renamed gate");
        super::verify(&ctx, &[renamed], &hooks).or_abort("same command proof under a new name");
        assert_eq!(
            fs::read_to_string(&gate_marker).or_abort("still one normal gate"),
            "g"
        );
        assert_eq!(
            fs::read_to_string(&hook_marker).or_abort("three native checks"),
            "hhh"
        );
    }
    #[test]
    fn refuses_to_reuse_unstamped_or_wrong_tree_proof_objects() {
        let directory = super::fixture();
        let repo = directory.path();
        let ctx = super::ctx(repo);
        let original = super::git(repo, &["rev-parse", "HEAD"]);
        let tree =
            super::TreeHash::new(&super::git(repo, &["rev-parse", HEAD_TREE])).or_abort("tree");
        let markers = TempDir::new().or_abort("markers");
        let marker = markers.path().join("gate");
        let gate = super::named(
            &ctx,
            "test",
            super::command(&format!("printf g >> '{}'", marker.display())),
        )
        .or_abort("gate");
        let reference = super::proof_ref(&gate, &tree);
        let hooks = repo.join(".git/hooks");
        super::verify(&ctx, slice::from_ref(&gate), &hooks).or_abort("first verification");
        super::git(repo, &["reset", "--hard", &original]);
        super::git(repo, &["update-ref", &reference, &original]);
        super::verify(&ctx, slice::from_ref(&gate), &hooks)
            .or_abort("unstamped object cannot skip gate");
        assert_eq!(fs::read_to_string(&marker).or_abort("two checks"), "gg");
        fs::write(repo.join("atom"), "different tree\n").or_abort("different tree");
        super::git(repo, &["add", "atom"]);
        super::git(
            repo,
            &[
                "commit",
                "--quiet",
                "--amend",
                "--no-verify",
                "-m",
                "Different",
            ],
        );
        let wrong_tree = super::git(repo, &["rev-parse", "HEAD"]);
        super::git(repo, &["reset", "--hard", &original]);
        super::git(repo, &["update-ref", &reference, &wrong_tree]);
        super::verify(&ctx, &[gate], &hooks).or_abort("wrong tree cannot skip gate");
        assert_eq!(fs::read_to_string(&marker).or_abort("three checks"), "ggg");
    }
    #[test]
    fn final_native_hooks_cannot_falsify_stamps_or_change_parent() {
        for (hook_name, script) in [
            (
                "commit-msg",
                "#!/bin/sh\nprintf 'Changed message\\n' > \"$1\"\n",
            ),
            (
                "post-commit",
                "#!/bin/sh\nnew=$(git commit-tree HEAD^{tree} -p HEAD -m Changed)\ngit update-ref HEAD \"$new\"\n",
            ),
        ] {
            let directory = super::fixture();
            let repo = directory.path();
            let ctx = super::ctx(repo);
            let hooks = repo.join(".git/hooks");
            let path = hooks.join(hook_name);
            fs::write(&path, script).or_abort("hook");
            fs::set_permissions(&path, Permissions::from_mode(0o755)).or_abort("permissions");
            let gate = super::named(&ctx, "test", super::command("test -f atom")).or_abort("gate");
            let _rejected =
                super::verify(&ctx, &[gate], &hooks).err_or_abort("mutating native hook");
        }
    }
    #[test]
    fn final_native_hook_cannot_change_author_while_preserving_tree_and_parents() {
        let directory = super::fixture();
        let repo = directory.path();
        let ctx = super::ctx(repo);
        let original_author = super::author_record(&ctx).or_abort("original author");
        let tree = super::git(repo, &["rev-parse", HEAD_TREE]);
        let parents = super::git(repo, &["show", "--format=%P", "--no-patch", "HEAD"]);
        let hooks = repo.join(".git/hooks");
        let hook = hooks.join("post-commit");
        fs::write(&hook, "#!/bin/sh\ngit -c core.hooksPath=/dev/null commit --quiet --amend --no-verify --no-edit --author='Changed Author <changed@example.com>'\n").or_abort("author-mutating native hook");
        fs::set_permissions(&hook, Permissions::from_mode(0o755)).or_abort("permissions");
        let gate = super::named(&ctx, "test", super::command("test -f atom")).or_abort("gate");
        let error =
            super::verify(&ctx, &[gate], &hooks).err_or_abort("author mutation refuses promotion");
        assert_eq!(
            error.to_string(),
            "git command failed: message hooks changed the original author"
        );
        // This witness would pass the old tree/parent/stamp-only guards.
        assert_eq!(super::git(repo, &["rev-parse", HEAD_TREE]), tree);
        assert_eq!(
            super::git(repo, &["show", "--format=%P", "--no-patch", "HEAD"]),
            parents
        );
        assert_ne!(
            super::author_record(&ctx).or_abort("mutated author"),
            original_author
        );
        assert!(
            super::commit_message(&ctx)
                .or_abort("retained stamp")
                .contains("Gate-test:")
        );
    }
    #[test]
    fn early_failure_cannot_leave_later_stale_or_duplicate_configured_stamps() {
        let directory = super::fixture();
        let repo = directory.path();
        let ctx = super::ctx(repo);
        let markers = TempDir::new().or_abort("markers");
        let early_marker = markers.path().join("early");
        let later_marker = markers.path().join("later");
        let first = super::named(
            &ctx,
            "first",
            super::command(&format!("printf x >> '{}'; exit 7", early_marker.display())),
        )
        .or_abort("first");
        let stale = super::named(
            &ctx,
            "stale",
            super::command(&format!("printf y >> '{}'", later_marker.display())),
        )
        .or_abort("stale");
        let duplicate =
            super::named(&ctx, "duplicate", super::command("exit 0")).or_abort("duplicate");
        let good = super::named(&ctx, "good", super::command("true")).or_abort("good");
        let tree = super::git(repo, &["rev-parse", HEAD_TREE]);
        let valid_good = format!("Gate-good: {} {tree}\n", good.command_hash);
        let message = format!(
            "Subject\n\nBody\n\nGate-stale: old tree\n old continuation\nGate-duplicate: {} {tree}\nGATE-DUPLICATE: {} {tree}\n{valid_good}Signed-off-by: Person\n unrelated continuation\n",
            duplicate.command_hash, duplicate.command_hash
        );
        super::amend_message(&ctx, &message).or_abort("initial configured records");
        let error = super::verify(
            &ctx,
            &[first, stale, duplicate, good],
            &repo.join(".git/hooks"),
        )
        .err_or_abort("early normal gate failure");
        let failure_code: i32 = 7;
        assert!(matches!(
            error,
            super::FactorError::ExecFailed { code, .. } if code == failure_code
        ));
        let invalidated_message = super::commit_message(&ctx).or_abort("invalidated message");
        assert_eq!(
            invalidated_message,
            format!(
                "Subject\n\nBody\n\n{valid_good}Signed-off-by: Person\n unrelated continuation\n"
            )
        );
        assert_eq!(fs::read_to_string(early_marker).or_abort("first ran"), "x");
        assert!(!later_marker.exists());
    }
    use crate::test_support::{OrAbort as _, ResultOrAbort as _};
}

use core::slice;

use crate::git_factor::{REAL_ENV, REAL_FS, REAL_IO, REAL_RUNNER};

use crate::test_support::{OrAbort as _, ResultOrAbort as _};

use std::fs::{self, Permissions};

use std::path::Path;

use std::process::Command;

use super::*;

use tempfile::TempDir;

pub(in crate::git_factor::gate) fn check_identity(text: &str) {
    let directory = fixture();
    let ctx = ctx(directory.path());
    let gate = named(&ctx, "test", command(text)).or_abort("named gate");
    let same = named(&ctx, "other", command(text)).or_abort("named gate");
    let compatibility = legacy(&ctx, command(text)).or_abort("legacy gate");
    assert_eq!(gate.command_hash, same.command_hash);
    assert_eq!(
        compatibility.name.as_str(),
        format!("exec-{}", gate.command_hash)
    );
    assert_ne!(
        named(&ctx, "test", command(&format!("{text} ")))
            .or_abort("different command")
            .command_hash,
        gate.command_hash
    );
    let _rejected = named(&ctx, "bad:name", command(text)).err_or_abort("invalid trailer name");
}

pub(in crate::git_factor::gate) fn check_names(name: &str) {
    validate_names(&[name]).or_abort("valid name");
    assert!(validate_names(&[name, &name.to_ascii_uppercase()]).is_err());
    for invalid in ["", "1test", "bad:name", "two words", "bad\nname", "\u{e9}"] {
        assert!(validate_names(&[invalid]).is_err(), "{invalid:?}");
    }
}

pub(in crate::git_factor::gate) fn check_raw_records(key: &str, value: &str) {
    let prefix = "Subject\n\nBody text\n\n";
    let unrelated = "Signed-off-by: Person\n unrelated continuation\n";
    let managed = format!("{key} :  {value}\n managed continuation\n");
    let text = format!("{prefix}{managed}{unrelated}");
    let records = parse_records(text.get(prefix.len()..).or_abort("paragraph"), prefix.len());
    assert_eq!(records.len(), 2);
    assert_eq!(records.first().or_abort("first record").key, key);
    assert_eq!(
        records.first().or_abort("first record").value,
        format!("{value} managed continuation")
    );
    assert_eq!(
        remove_records(&text, &records, key),
        format!("{prefix}{unrelated}")
    );
    assert_eq!(
        append_record(&text, &records, "Gate-new", "new tree"),
        format!("{text}Gate-new: new tree\n")
    );
    let body = "Subject\n\nBody with trailing blank lines\n\n\n";
    assert_eq!(
        append_record(body, &[], "Gate-new", "new tree"),
        format!("{body}Gate-new: new tree\n")
    );
    // A continuation beyond a non-trailer line must never consume body text.
    let body_text = format!("{key}: {value}\nplain body line\n indentation in body\n");
    let body_records = parse_records(&body_text, 0);
    assert_eq!(
        remove_records(&body_text, &body_records, key),
        "plain body line\n indentation in body\n"
    );
}

/// Builds the expected wire fields independently of production gate serialization.
pub(in crate::git_factor::gate) fn spec_value(name: &str, command: &str) -> serde_json::Value {
    serde_json::Value::Object(serde_json::Map::from_iter([
        (
            "command".to_owned(),
            serde_json::Value::String(command.to_owned()),
        ),
        (
            "name".to_owned(),
            serde_json::Value::String(name.to_owned()),
        ),
    ]))
}

pub(in crate::git_factor::gate) fn check_spec_round_trip(name: &str, text: &str) {
    let directory = fixture();
    let context = ctx(directory.path());
    let original = named(&context, name, command(text)).or_abort("original gate");
    let expected_hash = original.command_hash.clone();
    let gates = GateSet::new(vec![original]).or_abort("ordered gates");

    let representation = serde_json::to_value(&gates).or_abort("serialized specifications");
    let rebound = bind(
        &context,
        serde_json::from_value::<Vec<GateSpec>>(representation.clone()).or_abort("specifications"),
    )
    .or_abort("bound gates");

    assert_eq!(
        representation,
        serde_json::Value::Array(vec![spec_value(name, text)])
    );
    assert_eq!(
        serde_json::to_value(&rebound).or_abort("serialized rebound"),
        representation
    );
    let gate = rebound.as_slice().first().or_abort("rebound gate");
    assert_eq!(gate.command_hash, expected_hash);
    assert_eq!(gate.command().as_str(), text);
    assert_eq!(gate.name.as_str(), name);
}

pub(in crate::git_factor::gate) fn check_verify(label: &str) {
    let directory = fixture();
    let repo = directory.path();
    let ctx = ctx(repo);
    let markers = TempDir::new().or_abort("markers");
    let marker = markers.path().join("order");
    let first = named(
        &ctx,
        "first",
        command(&format!("printf a >> '{}'", marker.display())),
    )
    .or_abort("first");
    let second = named(
        &ctx,
        "second",
        command(&format!("printf b >> '{}'", marker.display())),
    )
    .or_abort("second");
    let tree = git(repo, &["rev-parse", HEAD_TREE]);
    let parents = git(repo, &["show", "--format=%P", "--no-patch", "HEAD"]);
    let initial = format!(
        "{label}\n\nbody Gate-first: fake text\n\nGate-first: old tree\n stale continuation\nGATE-FIRST: duplicate old\nSigned-off-by: Person\n unrelated continuation\n"
    );
    amend_message(&ctx, &initial).or_abort("initial trailers");
    let gates = [first, second];
    let _passed = verify(&ctx, &gates, &repo.join(".git/hooks")).or_abort("both pass");
    assert_eq!(fs::read_to_string(&marker).or_abort("order"), "ab");
    assert_eq!(git(repo, &["rev-parse", HEAD_TREE]), tree);
    assert_eq!(
        git(repo, &["show", "--format=%P", "--no-patch", "HEAD"]),
        parents
    );
    let message = commit_message(&ctx).or_abort("stamped");
    assert!(message.starts_with(&format!("{label}\n\nbody Gate-first: fake text\n\n")));
    assert!(message.contains("Signed-off-by: Person\n unrelated continuation\n"));
    assert!(!message.contains("stale continuation"));
    assert!(!message.contains("duplicate old"));
    for gate in &gates {
        assert_eq!(
            recognized_records(&ctx, &message)
                .or_abort("records")
                .iter()
                .filter(|record| record
                    .key
                    .eq_ignore_ascii_case(&format!("Gate-{}", gate.name)))
                .count(),
            1
        );
        assert!(
            git(
                repo,
                &["show", "-s", "--format=%(trailers:only,unfold=true)"]
            )
            .contains(&format!("Gate-{}: {} {tree}", gate.name, gate.command_hash))
        );
    }
    let _cached = verify(&ctx, &gates, &repo.join(".git/hooks")).or_abort("cached");
    assert_eq!(commit_message(&ctx).or_abort("cached message"), message);
    assert_eq!(git(repo, &["rev-parse", HEAD_TREE]), tree);
    assert_eq!(fs::read_to_string(&marker).or_abort("cached order"), "ab");
    // Only the changed command reruns; earlier passing provenance remains.
    let changed = named(&ctx, "second", command("exit 23")).or_abort("changed second");
    let error = verify(&ctx, &[gates[0].clone(), changed], &repo.join(".git/hooks"))
        .err_or_abort("failure");
    let failure_code: i32 = 23;
    assert!(matches!(error, FactorError::ExecFailed { code, .. } if code == failure_code));
    let failed_message = commit_message(&ctx).or_abort("failed message");
    assert!(failed_message.contains("Gate-first:"));
    assert!(!failed_message.contains("Gate-second:"));
    assert_eq!(fs::read_to_string(&marker).or_abort("first cached"), "ab");
}

pub(in crate::git_factor::gate) fn command(text: &str) -> NonEmptyString {
    NonEmptyString::try_from(text.to_owned()).or_abort("command")
}

pub(in crate::git_factor::gate) fn ctx(repo: &Path) -> Ctx<'_> {
    Ctx {
        cwd: repo.to_path_buf(),
        env: &REAL_ENV,
        fs: &REAL_FS,
        io: &REAL_IO,
        runner: &REAL_RUNNER,
    }
}

pub(in crate::git_factor::gate) fn fixture() -> TempDir {
    let directory = TempDir::new().or_abort("fixture");
    let repo = directory.path();
    git(repo, &["init", "--quiet"]);
    git(repo, &["config", "user.name", "Gate Test"]);
    git(repo, &["config", "user.email", "gate@example.com"]);
    fs::write(repo.join("atom"), "selected\n").or_abort("tree");
    git(repo, &["add", "atom"]);
    git(repo, &["commit", "--quiet", "-m", "Add atom"]);
    directory
}

fn git(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .or_abort("Git process");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .or_abort("Git text")
        .trim_end_matches('\n')
        .to_owned()
}
