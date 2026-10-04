mod staged_tree {
    use crate::test_support::OrAbort as _;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn preserves_generated_native_commit_semantics_and_index_flags(
            world in prop::sample::select(vec![
                super::super::tests::StagedPath::New,
                super::super::tests::StagedPath::TrackedContent,
                super::super::tests::StagedPath::TrackedEmpty,
                super::super::tests::StagedPath::StagedEmpty,
            ]),
            name in "[a-z]{1,8}( [a-z]{1,8})?",
            newline in any::<bool>(),
            content in prop::collection::vec(any::<u8>(), 0..=32),
        ) {
            let path = if newline { format!("generated-{name}\npath") } else { format!("generated-{name}") };
            let fixture = super::super::tests::NativeIndexObservation::arrange(world, &path, &content);
            let context = fixture.context();
            let before = fixture.frame();
            let purpose = super::super::StagedPurpose::Selection { base: Some(fixture.base()), source: fixture.source() };

            let observed = super::super::staged_tree(&context, purpose);

            let tree = observed.or_abort("native staged-tree observation");
            prop_assert_eq!(tree.as_str(), fixture.expected_tree());
            prop_assert_eq!(fixture.observed_frame(&before), before);
        }
    }
}

mod journal_facts {
    mod checkpoint {
        use proptest::prelude::*;
        proptest! {
            #[test]
            fn preserves_generated_recorded_object(checkpoint in "[0-9a-f]{40}") {
                let facts = super::super::super::tests::recorded_journal_facts(&checkpoint, "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb", "preparing", None);

                let observed = facts.checkpoint().as_str();

                prop_assert_eq!(observed, checkpoint);
            }
        }
    }
    mod final_tree {
        use proptest::prelude::*;
        proptest! {
            #[test]
            fn preserves_generated_recorded_object(tree in "[0-9a-f]{40}") {
                let facts = super::super::super::tests::recorded_journal_facts("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", &tree, "preparing", None);

                let observed = facts.final_tree().as_str();

                prop_assert_eq!(observed, tree);
            }
        }
    }
    mod phase {
        use super::super::super::tests::journal_observation::PhaseInput;
        use proptest::prelude::*;
        proptest! {
            #[test]
            fn preserves_closed_recorded_phase(input in prop::sample::select(vec![PhaseInput::Preparing, PhaseInput::Opening, PhaseInput::Selecting, PhaseInput::Replaying, PhaseInput::Verified, PhaseInput::Closing])) {
                let (phase, source) = match input {
                    PhaseInput::Preparing => ("preparing", None),
                    PhaseInput::Opening => ("opening", Some("cccccccccccccccccccccccccccccccccccccccc")),
                    PhaseInput::Selecting => ("selecting", Some("cccccccccccccccccccccccccccccccccccccccc")),
                    PhaseInput::Replaying => ("replaying", Some("cccccccccccccccccccccccccccccccccccccccc")),
                    PhaseInput::Verified => ("verified", None),
                    PhaseInput::Closing => ("closing", None),
                };
                let facts = super::super::super::tests::recorded_journal_facts("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb", phase, source);

                let observed = facts.phase();

                prop_assert_eq!(observed, phase);
            }
        }
    }
    mod source {
        use proptest::prelude::*;
        proptest! {
            #[test]
            fn preserves_applicable_generated_source(value in prop::option::of("[0-9a-f]{40}")) {
                let expected = value.as_deref();
                let phase = if expected.is_some() { "selecting" } else { "preparing" };
                let facts = super::super::super::tests::recorded_journal_facts("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb", phase, expected);

                let observed = facts.source().map(super::super::super::CommitSha::as_str);

                prop_assert_eq!(observed, expected);
            }
        }
    }
}

mod error_log_directory {
    use proptest::prelude::*;
    proptest! {
        #[test]
        fn observes_generated_native_session_worlds_without_mutation(
            world in prop::sample::select(super::super::tests::LogWorld::ALL.to_vec()),
            content in "[A-Za-z0-9 ]{1,24}",
        ) {
            super::super::tests::verify_error_log_observation(world, &content);
        }
    }
}
mod entry {
    use proptest::prelude::any;
    use proptest::sample::select;
    proptest::proptest! {
        #[test]
        fn failed_prerequisite_ignores_generated_success_looking_stdout(
            failure in select(vec![
                super::super::tests::VersionFailure::Query,
                super::super::tests::VersionFailure::SilentQuery,
                super::super::tests::VersionFailure::Spawn,
                super::super::tests::VersionFailure::WhitespaceQuery,
            ]),
            patch in any::<u16>(),
        ) {
            super::super::tests::verify_git_version_admission(true, &format!("git version 2.56.{patch}"), false, failure);
        }
    }

    proptest::proptest! {
        #[test]
        fn rejects_constructively_malformed_version_before_authority(
            malformed in select(super::super::tests::MalformedGitVersion::ALL.to_vec()),
            value in any::<u16>(),
        ) {
            super::super::tests::verify_malformed_git_version(true, malformed, value);
        }
    }

    use super::super::tests::VersionFailure;
    use core::ops::RangeInclusive;
    proptest::proptest! {
        #[test]
        fn admits_generated_released_git_floor_without_checkpoint_mutation(
            major in RangeInclusive::<u16>::new(1, 4), minor in RangeInclusive::<u16>::new(0, 70), patch in RangeInclusive::<u16>::new(0, 100),
        ) {
            let text=format!("git version {major}.{minor}.{patch}");
            super::super::tests::verify_git_version_admission(true,&text,
                (major,minor,patch)>=(2,56,0),VersionFailure::None);
        }
    }
    use proptest::prelude::*;
    proptest! {
        #[test]
        fn cached_remainder_callbacks_preserve_generated_raw_message_bytes(
            terminal_lfs in RangeInclusive::<usize>::new(1, 8),
            body in "[a-zA-Z0-9 ]{1,40}",
        ) {
            super::super::tests::verify_raw_remainder_message(terminal_lfs, &format!("{body}  "));
        }
        #[test]
        fn generated_root_or_nonroot_round_runs_real_native_callbacks(
            root in any::<bool>(),
            title in "[a-zA-Z0-9 ]{1,40}",
        ) {
            super::super::tests::verify_round(root, &format!("Extract generated callback {title}"));
        }
        #[test]
        fn native_callbacks_and_generated_unknown_command_refuse_outside_their_contract(suffix in "[a-z]{1,12}") {
            super::super::tests::verify_unknown_internal_command(&suffix);
        }
    }
}

mod outcome {
    mod deserialize {
        use super::super::super::Outcome;
        use crate::test_support::{OrAbort as _, ResultOrAbort as _};
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn rejects_unknown_terminal_outcome_fields(
                complete in any::<bool>(),
                suffix in "[a-z]{1,16}",
            ) {
                let field = format!("extra_{suffix}");
                let (representation_text, expected_field) = if complete {
                    (format!(r#"{{"result":"complete","atom":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","{field}":1}}"#), "atom")
                } else {
                    (format!(r#"{{"result":"aborted","base":null,"{field}":1}}"#), "base")
                };

                let representation: serde_json::Value = serde_json::from_str(&representation_text).or_abort("valid JSON input");

                let observed = serde_json::from_value::<Outcome>(representation);

                let error = observed.err_or_abort("closed terminal outcome");
                prop_assert_eq!(error.to_string(), format!("unknown field `{field}`, expected `{expected_field}`"));
            }
        }
    }
}

mod run {
    use crate::exit_codes::{EXIT_OK, EXIT_TEMPFAIL};
    use crate::git_factor::main_entry_with_vec;
    use proptest::sample::select;
    use std::ffi::OsString;
    proptest::proptest! {
        #[test]
        fn preserves_generated_original_pool_directory_boundary(
            world in select(vec![super::super::tests::PoolWorld::Original, super::super::tests::PoolWorld::Edited]),
            contents in "[a-zA-Z0-9 \n]{1,40}",
        ) {
            use crate::test_support::OrAbort as _;
            use super::super::tests::{OriginalPool, PoolWorld};
            let fixture = OriginalPool::arrange(world, &contents);
            let before = fixture.frame();
            let expected = OriginalPool::expected_output();
            let arguments = ["git-factor", "--message", "Remove original file"].map(OsString::from);

            let code = main_entry_with_vec(fixture.context().io, Ok(fixture.context()), &arguments);

            let after = fixture.frame();
            match world {
                PoolWorld::Edited => {
                    proptest::prop_assert_eq!(code, EXIT_TEMPFAIL);
                    proptest::prop_assert_eq!(fixture.stdout(), "");
                    proptest::prop_assert_eq!(fixture.stderr(), fixture.diagnostic());
                    proptest::prop_assert_eq!(after, before);
                    proptest::prop_assert_eq!(fixture.git(&["write-tree"]), fixture.deletion_tree());
                }
                PoolWorld::Original => {
                    proptest::prop_assert_eq!(code, EXIT_OK);
                    proptest::prop_assert_eq!(serde_json::from_str::<serde_json::Value>(&fixture.stdout()).or_abort("normalized result"), expected);
                    proptest::prop_assert_eq!(fixture.stderr(), "");
                    let journal = fixture.journal();
                    proptest::prop_assert!(matches!(journal.state, super::super::Phase::Selecting { .. }), "capture restarts selection");
                    proptest::prop_assert_eq!(fixture.git(&["rev-parse", "HEAD^"]), fixture.base());
                    proptest::prop_assert_eq!(fixture.git(&["rev-parse", "HEAD^{tree}"]), fixture.deletion_tree());
                    proptest::prop_assert_eq!(fixture.git(&["write-tree"]), fixture.deletion_tree());
                    proptest::prop_assert_eq!(fixture.git(&["rev-parse", "refs/heads/main"]), journal.checkpoint.as_str());
                    proptest::prop_assert_eq!(fixture.git(&["rev-parse", "refs/heads/main^{tree}"]), fixture.final_tree());
                    proptest::prop_assert_eq!(fixture.git(&["show-ref", "--verify", "refs/heads/foreign", "refs/tags/foreign"]), fixture.foreign_refs());
                    proptest::prop_assert_eq!(fixture.git(&["diff", "--name-only"]), "");
                    proptest::prop_assert_eq!(after.remainder(), fixture.contents());
                }
            }
        }
    }

    proptest::proptest! {
        #[test]
        fn failed_prerequisite_ignores_generated_success_looking_stdout(
            failure in select(vec![
                super::super::tests::VersionFailure::Query,
                super::super::tests::VersionFailure::SilentQuery,
                super::super::tests::VersionFailure::Spawn,
                super::super::tests::VersionFailure::WhitespaceQuery,
            ]),
            patch in any::<u16>(),
        ) {
            super::super::tests::verify_git_version_admission(false, &format!("git version 2.56.{patch}"), false, failure);
        }
    }

    proptest::proptest! {
        #[test]
        fn rejects_constructively_malformed_version_before_authority(
            malformed in select(super::super::tests::MalformedGitVersion::ALL.to_vec()),
            value in any::<u16>(),
        ) {
            super::super::tests::verify_malformed_git_version(false, malformed, value);
        }
    }

    use super::super::tests::VersionFailure;
    use core::ops::RangeInclusive;
    proptest::proptest! {
        #[test]
        fn admits_generated_released_git_floor_without_checkpoint_mutation(
            major in RangeInclusive::<u16>::new(1, 4), minor in RangeInclusive::<u16>::new(0, 70), patch in RangeInclusive::<u16>::new(0, 100),
        ) {
            let text=format!("git version {major}.{minor}.{patch}");
            super::super::tests::verify_git_version_admission(false,&text,
                (major,minor,patch)>=(2,56,0),VersionFailure::None);
        }
    }
    // Register only this generated provider under existing proptests::run.
    // Strategy construction chooses a value; it performs no native invocation.
    proptest::proptest! {
        #[test]
        fn generated_native_lifecycle_preserves_owned_transition(
            scenario in select(super::super::tests::NativeScenario::ALL.to_vec()),
            contents in ".{0,40}",
        ) {
            super::super::tests::verify_generated_e1(scenario, &contents);
        }
    }

    use proptest::prelude::*;
    proptest! {
        #[test]
        fn generated_foreign_administrative_or_physical_input_is_preserved(
            world in select(super::super::tests::FoundationInput::ALL.to_vec()),
            contents in ".{0,40}",
        ) {
            super::super::tests::verify_foundation_input(world, &contents);
        }
        #[test]
        fn refuses_generated_nonancestor_without_running_gate_or_publishing_journal(
            selected_content in "[a-zA-Z0-9 ]{1,40}",
            user_content in ".{0,40}",
        ) {
            super::super::tests::verify_nonancestor(&selected_content, &user_content);
        }
        #[test]
        fn first_journal_publication_preserves_arbitrary_foreign_file_bytes(bytes in ".{0,40}") {
            super::super::tests::verify_foreign_session_path(true, super::super::tests::ForeignPathKind::File, &bytes);
        }
        #[test]
        fn finish_preserves_generated_original_raw_message_bytes(
            terminal_lfs in RangeInclusive::<usize>::new(1, 8),
            body in "[a-zA-Z0-9 ]{1,40}",
        ) {
            super::super::tests::verify_raw_finish_message(terminal_lfs, &format!("{body}  "));
        }
        #[test]
        fn interrupted_preparation_preserves_arbitrary_ignored_bytes(content in ".{0,40}") {
            super::super::tests::verify_generated_readmission(&content);
        }

        #[test]
        fn empty_selection_preserves_arbitrary_original_content(content in ".{0,40}") {
            super::super::tests::verify_empty_admission(&content);
        }
    }
}

mod journal_snapshot {
    use super::super::tests::journal_observation::{Fixture, Input, PhaseInput};
    use crate::test_support::OrAbort as _;
    use proptest::prelude::*;
    use std::fs;
    proptest! {
        #[test]
        fn observes_generated_current_and_unavailable_metadata_without_native_queries(
            input in prop::sample::select(vec![Input::Absent, Input::Malformed, Input::Unreadable,
                Input::Current(PhaseInput::Closing), Input::Current(PhaseInput::Opening),
                Input::Current(PhaseInput::Preparing), Input::Current(PhaseInput::Replaying),
                Input::Current(PhaseInput::Selecting), Input::Current(PhaseInput::Verified)]),
            commit in "[a-f0-9]{40}", tree in "[a-f0-9]{40}",
        ) {
            let fixture = Fixture::arrange(input, &commit, &tree);
            let context = fixture.context();
            let path = context.cwd.join(".git/factor-journal.json");

            let result = super::super::journal_snapshot(&context, &context.cwd.join(".git"));

            prop_assert_eq!(result.as_ref(), fixture.expected());
            prop_assert_eq!(fixture.calls(), 0);
            match input {
                Input::Current(_) | Input::Malformed => prop_assert_eq!(
                    fs::read(&path).or_abort("journal readback"),
                    fixture.metadata().or_abort("recorded journal").as_slice()),
                Input::Absent => prop_assert!(!path.exists()),
                Input::Unreadable => prop_assert!(path.is_dir()),
            }
            prop_assert_eq!(fs::read(context.cwd.join("user")).or_abort("user readback"), b"protected user bytes\n");
            prop_assert_eq!(fixture.stdout(), "");
            prop_assert_eq!(fixture.stderr(), "");
        }
    }
}
