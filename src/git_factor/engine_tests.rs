#[path = "engine_e1_tests.rs"]
mod e1;
mod entry {
    #[test]
    fn preserves_silent_query_failure_without_authority_effects() {
        super::verify_git_version_admission(
            true,
            "git version 9.9.9",
            false,
            super::VersionFailure::SilentQuery,
        );
    }
    #[test]
    fn preserves_whitespace_query_failure_without_authority_effects() {
        super::verify_git_version_admission(
            true,
            "git version 9.9.9",
            false,
            super::VersionFailure::WhitespaceQuery,
        );
    }

    #[test]
    fn rejects_empty_major_version_before_authority() {
        super::verify_malformed_git_version(true, super::MalformedGitVersion::EmptyMajor, 7);
    }
    #[test]
    fn rejects_empty_minor_version_before_authority() {
        super::verify_malformed_git_version(true, super::MalformedGitVersion::EmptyMinor, 7);
    }
    #[test]
    fn rejects_empty_patch_version_before_authority() {
        super::verify_malformed_git_version(true, super::MalformedGitVersion::EmptyPatch, 7);
    }
    #[test]
    fn rejects_missing_patch_version_before_authority() {
        super::verify_malformed_git_version(true, super::MalformedGitVersion::MissingPatch, 7);
    }
    #[test]
    fn rejects_nondigit_major_version_before_authority() {
        super::verify_malformed_git_version(true, super::MalformedGitVersion::NondigitMajor, 7);
    }
    #[test]
    fn rejects_nondigit_minor_version_before_authority() {
        super::verify_malformed_git_version(true, super::MalformedGitVersion::NondigitMinor, 7);
    }
    #[test]
    fn rejects_nondigit_patch_version_before_authority() {
        super::verify_malformed_git_version(true, super::MalformedGitVersion::NondigitPatch, 7);
    }
    #[test]
    fn rejects_overflow_major_version_before_authority() {
        super::verify_malformed_git_version(true, super::MalformedGitVersion::OverflowMajor, 7);
    }
    #[test]
    fn rejects_overflow_minor_version_before_authority() {
        super::verify_malformed_git_version(true, super::MalformedGitVersion::OverflowMinor, 7);
    }
    #[test]
    fn rejects_overflow_patch_version_before_authority() {
        super::verify_malformed_git_version(true, super::MalformedGitVersion::OverflowPatch, 7);
    }
    #[test]
    fn rejects_unexpected_distribution_version_before_authority() {
        super::verify_malformed_git_version(
            true,
            super::MalformedGitVersion::UnexpectedDistribution,
            7,
        );
    }

    #[test]
    fn cached_remainder_callbacks_preserve_raw_commit_message_bytes() {
        for terminal_lfs in [1, 3, 5] {
            super::verify_raw_remainder_message(terminal_lfs, "Exact body  ");
        }
    }
    #[test]
    fn native_editor_remainder_descendant_and_terminal_follow_actual_cli_dispatch() {
        super::verify_round(false, "Extract internal-callback atom");
    }
    #[test]
    fn requires_supported_released_git_before_checkpoint_effects() {
        super::version_boundaries(true);
    }
    #[test]
    fn root_native_callbacks_follow_actual_cli_dispatch() {
        super::verify_round(true, "Extract internal-root callback atom");
    }
    #[test]
    fn unknown_internal_command_refuses_before_git_mutation() {
        super::verify_unknown_internal_command("unit");
    }
}

mod physical_abort_paths {
    use crate::test_support::OrAbort as _;
    use alloc::collections::BTreeSet;

    #[test]
    fn configured_split_index_observation_preserves_the_complete_native_frame() {
        let fixture = super::NativeIndexObservation::arrange(
            super::StagedPath::TrackedContent,
            "physical",
            b"selected bytes",
        );
        fixture
            .repository
            .git(&["config", "core.splitIndex", "true"]);
        let before = fixture.frame();
        let expected = BTreeSet::from([b"atom".to_vec(), b"physical".to_vec()]);

        let observed = super::super::physical_abort_paths(&fixture.context());

        assert_eq!(observed.or_abort("native physical paths"), expected);
        assert_eq!(fixture.frame(), before);
    }
}

mod staged_tree {
    use crate::test_support::OrAbort as _;

    #[test]
    fn configured_split_index_observation_preserves_the_complete_native_frame() {
        let fixture = super::NativeIndexObservation::arrange(
            super::StagedPath::New,
            "intent",
            b"selected bytes",
        );
        fixture
            .repository
            .git(&["config", "core.splitIndex", "true"]);
        let context = fixture.context();
        let before = fixture.frame();
        let purpose = super::super::StagedPurpose::Selection {
            base: Some(fixture.base()),
            source: fixture.source(),
        };

        let observed = super::super::staged_tree(&context, purpose);

        assert_eq!(
            observed.or_abort("native staged tree").as_str(),
            fixture.expected_tree()
        );
        let after = fixture.frame();
        let shared_indexes = after
            .files
            .keys()
            .filter(|path| {
                path.file_name()
                    .is_some_and(|name| name.as_encoded_bytes().starts_with(b"sharedindex."))
            })
            .collect::<Vec<_>>();
        assert!(
            shared_indexes.is_empty(),
            "private observation created shared-index files: {shared_indexes:?}"
        );
        assert_eq!(fixture.observed_frame(&before), before);
    }

    #[test]
    fn retains_a_deliberately_staged_empty_path_without_touching_the_real_index() {
        let fixture =
            super::NativeIndexObservation::arrange(super::StagedPath::StagedEmpty, "empty", &[]);
        let context = fixture.context();
        let before = fixture.frame();
        let purpose = super::super::StagedPurpose::Selection {
            base: Some(fixture.base()),
            source: fixture.source(),
        };

        let observed = super::super::staged_tree(&context, purpose);

        assert_eq!(
            observed.or_abort("native staged tree").as_str(),
            fixture.expected_tree()
        );
        assert_eq!(fixture.observed_frame(&before), before);
    }

    #[test]
    fn refuses_a_successful_native_projection_that_does_not_remove_intentions() {
        let fixture = super::NativeIndexObservation::arrange(
            super::StagedPath::New,
            "intent",
            b"native bytes",
        );
        let omission = super::ProjectionOmission {
            fired: super::Cell::new(false),
        };
        let mut context = fixture.context();
        context.runner = &omission;
        let before = fixture.frame();
        let purpose = super::super::StagedPurpose::Selection {
            base: Some(fixture.base()),
            source: fixture.source(),
        };

        let observed = super::super::staged_tree(&context, purpose);

        assert!(
            omission.fired.get(),
            "native successful no-op projection stimulus"
        );
        assert_eq!(
            observed
                .map(|tree| tree.as_str().to_owned())
                .map_err(|error| error.to_string()),
            Err(String::from(
                "git command failed: projected staged tree differs from native commit semantics"
            ))
        );
        assert_eq!(fixture.observed_frame(&before), before);
    }
}

mod error_log_directory {
    #[test]
    fn absent_session_has_no_diagnostic_directory() {
        super::verify_error_log_observation(super::LogWorld::AbsentSession, "unit bytes");
    }
    #[test]
    fn admits_a_real_selecting_session_without_mutating_it() {
        super::verify_error_log_observation(super::LogWorld::Admitted, "unit bytes");
    }
    #[test]
    fn admitted_session_without_scratch_has_no_diagnostic_directory() {
        super::verify_error_log_observation(super::LogWorld::AbsentScratch, "unit bytes");
    }
    #[test]
    fn foreign_scratch_file_is_preserved_without_a_diagnostic_directory() {
        super::verify_error_log_observation(super::LogWorld::ForeignScratch, "unit bytes");
    }
    #[test]
    fn malformed_canonical_journal_has_no_diagnostic_directory() {
        super::verify_error_log_observation(super::LogWorld::CorruptJournal, "unit bytes");
    }
    #[test]
    fn post_admission_directory_symlink_is_preserved_and_unavailable() {
        super::verify_error_log_observation(
            super::LogWorld::LateScratchSymlink,
            "late foreign bytes",
        );
    }
    #[test]
    fn post_admission_foreign_file_is_preserved_and_unavailable() {
        super::verify_error_log_observation(super::LogWorld::LateScratchFile, "late foreign bytes");
    }
    #[test]
    fn unavailable_final_scratch_metadata_has_no_diagnostic_directory() {
        super::verify_error_log_observation(super::LogWorld::ScratchMetadataError, "unit bytes");
    }
    #[test]
    fn unavailable_journal_metadata_has_no_diagnostic_directory() {
        super::verify_error_log_observation(super::LogWorld::JournalMetadataError, "unit bytes");
    }
}

#[path = "engine_fault_tests.rs"]
mod faults;

#[path = "engine_journal_snapshot_tests.rs"]
pub(in crate::git_factor::engine) mod journal_observation;

mod journal_facts {
    mod checkpoint {
        #[test]
        fn borrows_exact_recorded_value() {
            let facts = super::super::recorded_journal_facts(
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                "preparing",
                None,
            );

            let observed = facts.checkpoint().as_str();

            assert_eq!(observed, "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
        }
    }
    mod final_tree {
        #[test]
        fn borrows_exact_recorded_value() {
            let facts = super::super::recorded_journal_facts(
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                "preparing",
                None,
            );

            let observed = facts.final_tree().as_str();

            assert_eq!(observed, "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb");
        }
    }
    mod phase {
        #[test]
        fn borrows_exact_recorded_value() {
            let facts = super::super::recorded_journal_facts(
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                "preparing",
                None,
            );

            let observed = facts.phase();

            assert_eq!(observed, "preparing");
        }
    }
    mod source {
        #[test]
        fn leaves_preparing_source_unavailable() {
            let facts = super::super::recorded_journal_facts(
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                "preparing",
                None,
            );

            let observed = facts.source();

            assert_eq!(observed, None);
        }
        #[test]
        fn borrows_selection_source() {
            let facts = super::super::recorded_journal_facts(
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                "selecting",
                Some("cccccccccccccccccccccccccccccccccccccccc"),
            );

            let observed = facts.source().map(super::super::super::CommitSha::as_str);

            assert_eq!(observed, Some("cccccccccccccccccccccccccccccccccccccccc"));
        }
    }
}

mod journal_snapshot {
    use super::journal_observation::{Fixture, Input, PhaseInput};
    use super::*;
    #[test]
    fn reads_selection_without_binding_gates_or_admitting_ownership() {
        let fixture = Fixture::arrange(
            Input::Current(PhaseInput::Selecting),
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        );
        let context = fixture.context();
        let path = context.cwd.join(".git/factor-journal.json");

        let result = super::super::journal_snapshot(&context, &context.cwd.join(".git"));

        assert_eq!(result.as_ref(), fixture.expected());
        assert_eq!(fixture.calls(), 0);
        assert_eq!(
            fs::read(&path).or_abort("journal readback"),
            fixture.metadata().or_abort("recorded journal").as_slice()
        );
        assert_eq!(
            fs::read(context.cwd.join("user")).or_abort("user readback"),
            b"protected user bytes\n"
        );
        assert_eq!(fixture.stdout(), "");
        assert_eq!(fixture.stderr(), "");
    }
}

mod outcome {
    mod deserialize {
        use super::super::super::Outcome;
        use crate::test_support::{OrAbort as _, ResultOrAbort as _};

        #[test]
        fn rejects_unknown_aborted_outcome_fields() {
            let representation: serde_json::Value =
                serde_json::from_str(r#"{"result":"aborted","base":null,"extra":1}"#)
                    .or_abort("valid JSON input");

            let observed = serde_json::from_value::<Outcome>(representation);

            let error = observed.err_or_abort("closed aborted outcome");
            assert_eq!(error.to_string(), "unknown field `extra`, expected `base`");
        }
    }
}

mod process_child {
    use super::*;
    /// Explicit executor for native Git child calls, outside contract providers.
    #[test]
    #[ignore = "invoked only by the owned native checkpoint launcher"]
    #[expect(
        clippy::exit,
        reason = "explicit native test executor must preserve the actual shared CLI exit status"
    )]
    fn invoke() {
        let argument_file = env::var_os("FACTOR_TEST_ARGUMENTS").or_abort("owned child arguments");
        let bytes = fs::read(argument_file).or_abort("child argument file");
        let mut arguments = vec![OsString::from("git-factor")];
        let payload = bytes.strip_suffix(&[0]).or_abort("complete child payload");
        arguments.extend(
            payload
                .split(|byte| *byte == 0)
                .map(|value| OsString::from_vec(value.to_vec())),
        );
        let environment = NativeEnv {
            cwd: env::current_dir().or_abort("native Git child cwd"),
            executable: PathBuf::from(
                env::var_os("FACTOR_TEST_EXECUTABLE").or_abort("owned child launcher"),
            ),
        };
        if super::e1::source_finished_hook(&arguments, &environment) {
            process::exit(0);
        }
        let child_filesystem = super::e1::ChildPublication::configured(&arguments, &environment);
        let filesystem: &dyn Fs = match child_filesystem.as_ref() {
            Some(adapter) => adapter,
            None => &REAL_FS,
        };
        let context = Ctx {
            cwd: environment.cwd.clone(),
            env: &environment,
            fs: filesystem,
            io: &REAL_IO,
            runner: &REAL_RUNNER,
        };
        let code = main_entry_with_vec(context.io, Ok(context), &arguments);
        super::e1::after_child_dispatch(&arguments, &environment, code);
        process::exit(code);
    }
}

mod run {
    use super::{EXIT_OK, EXIT_SOFTWARE, EXIT_TEMPFAIL, EXIT_USAGE, Path, fs};
    use std::os::unix::fs::PermissionsExt as _;

    include!("engine_admission_units.rs");
    include!("engine_pool_units.rs");
    include!("engine_author_units.rs");
    include!("engine_branch_units.rs");
    include!("engine_gate_stamp_units.rs");

    #[test]
    fn empty_selected_tip_message_refuses_before_session_or_gate_mutation() {
        let fixture = super::SourceMessageAdmission::arrange(super::MessageLocation::Tip, false);
        let before = fixture.frame();

        let (code, stdout) = fixture
            .repository
            .invoke(&["--exec", &fixture.gate, &fixture.target]);

        let after = fixture.frame();
        assert_eq!(
            (
                code,
                stdout,
                fixture.repository.output.stderr.borrow().clone(),
                fixture.gate_marker.exists()
            ),
            (EXIT_SOFTWARE, String::new(), fixture.expected_error, false)
        );
        assert_eq!(after, before);
    }

    #[test]
    fn empty_replayed_descendant_message_refuses_before_session_or_gate_mutation() {
        let fixture =
            super::SourceMessageAdmission::arrange(super::MessageLocation::Descendant, false);
        let before = fixture.frame();

        let (code, stdout) = fixture
            .repository
            .invoke(&["--exec", &fixture.gate, &fixture.target]);

        let after = fixture.frame();
        assert_eq!(
            (
                code,
                stdout,
                fixture.repository.output.stderr.borrow().clone(),
                fixture.gate_marker.exists()
            ),
            (EXIT_SOFTWARE, String::new(), fixture.expected_error, false)
        );
        assert_eq!(after, before);
    }

    #[test]
    fn non_utf8_selected_tip_message_refuses_before_session_or_gate_mutation() {
        let fixture = super::SourceMessageAdmission::arrange(super::MessageLocation::Tip, true);
        let before = fixture.frame();

        let (code, stdout) = fixture
            .repository
            .invoke(&["--exec", &fixture.gate, &fixture.target]);

        let after = fixture.frame();
        assert_eq!(
            (
                code,
                stdout,
                fixture.repository.output.stderr.borrow().clone(),
                fixture.gate_marker.exists()
            ),
            (EXIT_SOFTWARE, String::new(), fixture.expected_error, false)
        );
        assert_eq!(after, before);
    }

    #[test]
    fn non_utf8_replayed_descendant_message_refuses_before_session_or_gate_mutation() {
        let fixture =
            super::SourceMessageAdmission::arrange(super::MessageLocation::Descendant, true);
        let before = fixture.frame();

        let (code, stdout) = fixture
            .repository
            .invoke(&["--exec", &fixture.gate, &fixture.target]);

        let after = fixture.frame();
        assert_eq!(
            (
                code,
                stdout,
                fixture.repository.output.stderr.borrow().clone(),
                fixture.gate_marker.exists()
            ),
            (EXIT_SOFTWARE, String::new(), fixture.expected_error, false)
        );
        assert_eq!(after, before);
    }

    #[test]
    fn aborted_closing_journal_unlink_preserves_captured_checkpoint() {
        super::faults::aborted_closing_unlink();
    }

    #[test]
    fn refuses_actor_routing_to_main_without_mutating_selection() {
        let repository = super::Repository::new();
        repository.write("keep", "unchanged anchor\n");
        repository.commit("Add base");
        repository.write("atom", "selected atom\nsaved same-file remainder\n");
        repository.commit("Add two lines");
        repository.success(&["--exec", "true", "HEAD"]);
        repository.write("atom", "selected atom\n");
        repository.git(&["add", "atom"]);
        repository.write("atom", "selected atom\nsaved same-file remainder\n");
        repository.write("user", "unrelated user bytes\0\n");
        repository.git(&["config", "extensions.worktreeConfig", "true"]);
        let root = &repository.environment.cwd;
        let canonical = fs::canonicalize(root).or_abort("main worktree route");
        repository.git(&[
            "config",
            "core.worktree",
            canonical.to_str().or_abort("main route UTF-8"),
        ]);
        repository.output.stdout.borrow_mut().clear();
        repository.output.stderr.borrow_mut().clear();
        let frame = || {
            let index = repository.index();
            (
                index,
                repository.journal(),
                repository.git(&["rev-parse", "HEAD"]),
                repository.git(&["show-ref"]),
                fs::read(root.join(".git/config")).or_abort("main config"),
                fs::read(root.join(".git/rebase-merge/done")).or_abort("native done"),
                fs::read(root.join(".git/rebase-merge/git-rebase-todo")).or_abort("native todo"),
                fs::read(root.join("keep")).or_abort("anchor"),
                fs::read(root.join("atom")).or_abort("saved remainder"),
                fs::read(root.join("user")).or_abort("unrelated user bytes"),
            )
        };
        let before = frame();
        let context = super::Ctx {
            cwd: root.clone(),
            env: &repository.environment,
            fs: &super::REAL_FS,
            io: &repository.output,
            runner: &super::REAL_RUNNER,
        };
        let arguments = ["git-factor", "--message", "Add selected atom"].map(super::OsString::from);

        let code = super::main_entry_with_vec(context.io, Ok(context), &arguments);

        let after = frame();
        assert_eq!(after, before);
        assert_eq!(code, EXIT_SOFTWARE);
        assert_eq!(*repository.output.stdout.borrow(), "");
        assert_eq!(
            *repository.output.stderr.borrow(),
            "git command failed: candidate Git working directory belongs to another worktree\n"
        );
    }

    #[test]
    fn preserves_silent_query_failure_without_authority_effects() {
        super::verify_git_version_admission(
            false,
            "git version 9.9.9",
            false,
            super::VersionFailure::SilentQuery,
        );
    }
    #[test]
    fn preserves_whitespace_query_failure_without_authority_effects() {
        super::verify_git_version_admission(
            false,
            "git version 9.9.9",
            false,
            super::VersionFailure::WhitespaceQuery,
        );
    }

    #[test]
    fn rejects_empty_major_version_before_authority() {
        super::verify_malformed_git_version(false, super::MalformedGitVersion::EmptyMajor, 7);
    }
    #[test]
    fn rejects_empty_minor_version_before_authority() {
        super::verify_malformed_git_version(false, super::MalformedGitVersion::EmptyMinor, 7);
    }
    #[test]
    fn rejects_empty_patch_version_before_authority() {
        super::verify_malformed_git_version(false, super::MalformedGitVersion::EmptyPatch, 7);
    }
    #[test]
    fn rejects_missing_patch_version_before_authority() {
        super::verify_malformed_git_version(false, super::MalformedGitVersion::MissingPatch, 7);
    }
    #[test]
    fn rejects_nondigit_major_version_before_authority() {
        super::verify_malformed_git_version(false, super::MalformedGitVersion::NondigitMajor, 7);
    }
    #[test]
    fn rejects_nondigit_minor_version_before_authority() {
        super::verify_malformed_git_version(false, super::MalformedGitVersion::NondigitMinor, 7);
    }
    #[test]
    fn rejects_nondigit_patch_version_before_authority() {
        super::verify_malformed_git_version(false, super::MalformedGitVersion::NondigitPatch, 7);
    }
    #[test]
    fn rejects_overflow_major_version_before_authority() {
        super::verify_malformed_git_version(false, super::MalformedGitVersion::OverflowMajor, 7);
    }
    #[test]
    fn rejects_overflow_minor_version_before_authority() {
        super::verify_malformed_git_version(false, super::MalformedGitVersion::OverflowMinor, 7);
    }
    #[test]
    fn rejects_overflow_patch_version_before_authority() {
        super::verify_malformed_git_version(false, super::MalformedGitVersion::OverflowPatch, 7);
    }
    #[test]
    fn rejects_unexpected_distribution_version_before_authority() {
        super::verify_malformed_git_version(
            false,
            super::MalformedGitVersion::UnexpectedDistribution,
            7,
        );
    }

    #[test]
    fn abort_refuses_destination_collisions_before_priming_real_index() {
        for alias in [false, true] {
            for ignored in [false, true] {
                super::verify_abort_deleted_collision(alias, ignored);
            }
        }
    }
    #[test]
    fn canonical_authority_survives_birth_and_partial_scratch_retirement() {
        super::journal_lifecycle_census();
    }
    #[test]
    fn capture_publication_and_terminal_cleanup_faults_preserve_recovery_authority() {
        super::durability_publication_census();
    }
    #[test]
    fn captures_nonroot_round_and_aborts_only_latest_attempt() {
        super::verify_round(false, "Extract atom");
    }
    #[test]
    fn captures_parentless_root_atom_and_preserves_final_tree() {
        super::verify_round(true, "Extract root atom");
    }
    #[test]
    fn closing_preserves_a_later_linked_worktree_session() {
        super::faults::closing_preserves_reacquired_lease(false);
        super::faults::closing_preserves_reacquired_lease(true);
    }
    #[test]
    fn closing_refuses_malformed_and_unobservable_lease_authority() {
        super::unknown_closing_lease_census();
    }
    #[test]
    fn collapses_root_and_nonroot_ranges_without_replaying_internal_commits() {
        super::verify_range(false);
        super::verify_range(true);
    }
    #[test]
    fn finish_preserves_original_raw_message_and_unrelated_trailers() {
        for terminal_lfs in [1, 3, 5] {
            super::verify_raw_finish_message(terminal_lfs, "Exact body  ");
        }
    }
    #[test]
    fn initial_journal_and_scratch_foreign_paths_refuse_before_publication() {
        super::foreign_session_path_census();
    }
    #[test]
    fn isolated_candidate_gate_failure_preserves_main_selection() {
        super::verify_candidate_gate_failure();
    }
    #[test]
    fn journal_publication_before_and_after_rename_reconciles_owned_lease() {
        super::faults::journal_publication(false);
        super::faults::journal_publication(true);
    }

    #[test]
    fn native_amended_same_slot_then_dropped_next_slot() {
        super::e1::amended_then_dropped_conflict("repaired first tree\n");
    }
    #[test]
    fn relocated_failed_exec_abort_discards_only_attempt_owned_dirty_work() {
        super::e1::relocated_failed_exec_abort(false);
    }

    #[test]
    fn relocated_failed_exec_abort_preserves_unrelated_dirty_work() {
        super::e1::relocated_failed_exec_abort(true);
    }

    #[test]
    fn moved_replay_continue_refuses_original_callback_identity() {
        super::e1::moved_replay_continue_refusal();
    }

    #[test]
    fn relocated_opening_root_abort() {
        super::e1::relocated_opening_abort(true, true);
    }

    #[test]
    fn copied_opening_nonroot_abort() {
        super::e1::relocated_opening_abort(false, false);
    }

    #[test]
    fn moved_replay_abort_before_capture() {
        super::e1::relocated_replay_abort(false, true);
    }

    #[test]
    fn copied_replay_abort_after_capture() {
        super::e1::relocated_replay_abort(true, false);
    }

    #[test]
    fn moved_attached_terminal_abort() {
        super::e1::relocated_terminal_abort(false);
    }

    #[test]
    fn moved_detached_terminal_abort() {
        super::e1::relocated_terminal_abort(true);
    }

    #[test]
    fn relocated_abort_refuses_mixed_programs() {
        super::e1::relocated_abort_refusal("mixed");
    }

    #[test]
    fn relocated_abort_refuses_shell_operators() {
        super::e1::relocated_abort_refusal("operator");
    }

    #[test]
    fn relocated_abort_refuses_tampered_terminal_source() {
        super::e1::relocated_abort_refusal("source");
    }

    #[test]
    fn native_attached_terminal_interval_is_observational_then_recoverable() {
        super::verify_attached_terminal_recovery(false);
        super::verify_attached_terminal_recovery(true);
    }
    #[test]
    fn native_abbreviated_terminal_exec_recovers_the_attached_checkpoint() {
        let repository = super::interrupted_attached_terminal(true);
        let root = &repository.environment.cwd;
        let index_before = repository.index();
        let journal_before: serde_json::Value =
            serde_json::from_slice(&repository.journal()).or_abort("verified native interval");
        let captured = journal_before
            .pointer("/state/tip")
            .and_then(serde_json::Value::as_str)
            .or_abort("native completed checkpoint");
        let atom = journal_before
            .pointer("/state/atom")
            .and_then(serde_json::Value::as_str)
            .or_abort("completed atom");
        let final_tree = journal_before
            .get("final_tree")
            .and_then(serde_json::Value::as_str)
            .or_abort("fixed final tree");
        let protected = repository.git(&["rev-parse", "protected"]);
        let protected_tag = repository.git(&["rev-parse", "protected-tag"]);
        let config_before = fs::read(root.join(".git/config")).or_abort("native config");
        let done = fs::read_to_string(root.join(".git/rebase-merge/done"))
            .or_abort("native abbreviated transcript");
        assert!(
            done.lines().last().is_some_and(
                |line| line.starts_with("x ") && line.ends_with(" checkpoint-terminal")
            )
        );
        assert_eq!(repository.git(&["rev-parse", "HEAD"]), captured);
        assert_eq!(repository.index(), index_before);

        let (code, stdout) = repository.invoke(&["--continue"]);

        assert_eq!(code, EXIT_OK, "{}", repository.output.stderr.borrow());
        assert_eq!(repository.output.stderr.borrow().as_str(), "");
        let response: serde_json::Value =
            serde_json::from_str(&stdout).or_abort("checkpoint response");
        assert_eq!(
            response
                .get("operation")
                .and_then(serde_json::Value::as_str),
            Some("continue")
        );
        assert_eq!(
            response.get("result").and_then(serde_json::Value::as_str),
            Some("committed")
        );
        assert_eq!(
            response
                .get("split_count")
                .and_then(serde_json::Value::as_u64),
            Some(1)
        );
        let reopened: serde_json::Value =
            serde_json::from_slice(&repository.journal()).or_abort("reopened selection");
        assert_eq!(
            reopened
                .get("checkpoint")
                .and_then(serde_json::Value::as_str),
            Some(captured)
        );
        assert_eq!(
            reopened
                .pointer("/state/phase")
                .and_then(serde_json::Value::as_str),
            Some("selecting")
        );
        assert_eq!(repository.git(&["rev-parse", "HEAD"]), atom);
        assert_eq!(repository.git(&["rev-parse", "main^{tree}"]), final_tree);
        assert_eq!(repository.git(&["rev-parse", "protected"]), protected);
        assert_eq!(
            repository.git(&["rev-parse", "protected-tag"]),
            protected_tag
        );
        assert_eq!(
            fs::read(root.join(".git/config")).or_abort("native config"),
            config_before
        );
        assert_eq!(
            fs::read(root.join("unrelated")).or_abort("unrelated bytes"),
            b"preserved unrelated bytes\n"
        );
    }
    #[test]
    fn native_attached_terminal_interval_refuses_pending_git_work() {
        super::verify_attached_terminal_pending_work();
    }

    #[test]
    fn native_before_native_exec() {
        super::e1::before_exec_control("first\n");
    }
    #[test]
    fn native_detached_terminal_finish_recovers_before_and_after_quit() {
        for after_quit in [false, true] {
            for abort in [false, true] {
                super::verify_detached_terminal_recovery(after_quit, abort);
            }
        }
    }

    #[test]
    fn native_executable_canonicalization_io() {
        super::e1::executable_observation_refusal(
            super::e1::ExecutableFailure::Canonicalize,
            "first\n",
        );
    }

    #[test]
    fn native_executable_copied_path() {
        super::e1::executable_distinct_path_refusal(false, "first\n");
    }

    #[test]
    fn native_executable_hard_linked_path() {
        super::e1::executable_distinct_path_refusal(true, "first\n");
    }

    #[test]
    fn native_executable_observation_io() {
        super::e1::executable_observation_refusal(
            super::e1::ExecutableFailure::CurrentExe,
            "first\n",
        );
    }

    #[test]
    fn native_executable_symlink_aliases() {
        super::e1::executable_alias_continuation("first\n");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn native_executable_utf8() {
        super::e1::executable_observation_refusal(super::e1::ExecutableFailure::Utf8, "first\n");
    }

    #[test]
    fn native_failed_pick_factor_retry() {
        super::e1::failed_pick_retry(false, "first\n", "preserved obstruction\n");
    }

    #[test]
    fn native_failed_pick_native_retry() {
        super::e1::failed_pick_retry(true, "first\n", "preserved obstruction\n");
    }

    #[test]
    fn native_final_after_publication() {
        super::e1::publication_retry(true, true, "first\n");
    }

    #[test]
    fn native_final_before_publication() {
        super::e1::publication_retry(true, false, "first\n");
    }

    #[test]
    fn native_final_descendant_bypass() {
        super::e1::final_descendant_bypass_refusal("first\n");
    }

    #[test]
    fn native_final_descendant_consumed_after_acceptance() {
        super::e1::consumed_retry("checkpoint-gate-descendant", true, "first\n");
    }

    #[test]
    fn native_final_descendant_consumed_before_acceptance() {
        super::e1::consumed_retry("checkpoint-gate-descendant", false, "first\n");
    }

    #[test]
    fn native_foreign_exec() {
        super::e1::grammar_refusal(true, "first\n");
    }

    #[test]
    fn native_future_exec_duplicate() {
        super::e1::future_retry_refusal(true, "first\n");
    }

    #[test]
    fn native_future_pick_duplicate() {
        super::e1::future_retry_refusal(false, "first\n");
    }

    #[test]
    fn native_ignored_replay_obstruction() {
        super::e1::ignored_replay_obstruction("preserved ignored bytes\n");
    }

    #[test]
    fn native_intermediate_after_publication() {
        super::e1::publication_retry(false, true, "first\n");
    }

    // Register each following leaf directly under existing tests::run.

    #[test]
    fn native_intermediate_before_publication() {
        super::e1::publication_retry(false, false, "first\n");
    }

    #[test]
    fn native_intermediate_consumed_before_acceptance() {
        super::e1::consumed_intermediate_retry("first\n");
    }

    #[test]
    fn native_last_done_pick_duplicate() {
        super::e1::last_done_pick_duplicate_refusal("first\n", "preserved obstruction\n");
    }

    #[test]
    fn native_manual_bypass_refusal() {
        super::e1::bypass_lost_callback_refusal("first\n");
    }

    #[test]
    fn native_missing_original_pick() {
        super::e1::grammar_refusal(false, "first\n");
    }

    #[test]
    fn native_observed_sparse_hook() {
        super::e1::observed_sparse_hook_refusal("hook changed physical root bytes\n");
    }

    #[test]
    fn native_opening_advanced() {
        super::e1::opening_advanced_refusal("first\n");
    }

    #[test]
    fn native_opening_deleted_recreation() {
        super::e1::opening_deleted_recreation_refusal("preserved ignored recreation\n");
    }

    #[test]
    fn native_opening_foreign_head() {
        super::e1::opening_foreign_head_refusal("first\n", "owned root obstruction\n");
    }
    #[test]
    fn native_opening_nonroot_abort() {
        super::e1::opening_nonroot_retry_abort("selected bytes\n", "preserved canceled input\n");
    }

    #[test]
    fn opening_ready_break_abort_preserves_user_edited_pool() {
        super::e1::opening_ready_break_abort(true);
    }
    #[test]
    fn opening_ready_break_abort_restores_unmodified_checkpoint() {
        super::e1::opening_ready_break_abort(false);
    }

    #[test]
    fn opening_single_source_ignores_interactive_missing_commit_policy() {
        super::e1::opening_with_missing_commit_check(false);
    }
    #[test]
    fn opening_contiguous_range_ignores_interactive_missing_commit_policy() {
        super::e1::opening_with_missing_commit_check(true);
    }

    #[test]
    fn native_opening_ready_break() {
        super::e1::opening_ready_break_control("first\n");
    }
    #[test]
    fn native_opening_root_abort() {
        super::e1::opening_root_retry_abort("first\n", "owned root obstruction\n");
    }

    #[test]
    fn native_opening_root_retry() {
        super::e1::opening_root_retry("first\n", "owned root obstruction\n");
    }

    #[test]
    fn native_opening_source_finished() {
        super::e1::opening_source_finished_before_break("first\n");
    }

    #[test]
    fn native_opening_wrong_pick() {
        super::e1::opening_wrong_pick_refusal("first\n", "owned root obstruction\n");
    }

    #[test]
    fn native_pending_amended_predecessor() {
        super::e1::pending_amended_predecessor_refusal("repaired first tree\n");
    }
    #[test]
    fn native_pending_descendant_bypass() {
        super::e1::bypass_pending_pick_refusal(false, "first\n");
    }
    #[test]
    fn native_pending_remainder_bypass() {
        super::e1::bypass_pending_pick_refusal(true, "first\n");
    }

    #[test]
    fn native_pending_selection_rewind() {
        super::e1::pending_selection_rewind_refusal("amended first source bytes\n");
    }

    #[test]
    fn native_prototype_format_refusal() {
        super::e1::prototype_format_refusal("first\n");
    }

    #[test]
    fn native_recreated_descendant_exec() {
        super::e1::recreated_descendant_path(false, "unrelated recreated bytes\n");
    }

    #[test]
    fn native_recreated_future_pick() {
        super::e1::recreated_descendant_path(true, "protected ignored bytes\n");
    }
    #[test]
    fn native_ref_rebase_and_actor_interruption_boundaries_recover() {
        for boundary in [
            super::faults::Boundary::LeasePublished,
            super::faults::Boundary::RebaseOpened,
            super::faults::Boundary::ReplayCompleted,
            super::faults::Boundary::ActorRegistered,
            super::faults::Boundary::ActorCleanup,
        ] {
            super::faults::process_interruption(boundary);
        }
    }

    #[test]
    fn native_remainder_bypass_with_descendants() {
        super::e1::remainder_bypass_refusal(true, "first\n");
    }

    #[test]
    fn native_remainder_bypass_without_descendants() {
        super::e1::remainder_bypass_refusal(false, "first\n");
    }

    #[test]
    fn native_remainder_consumed_after_acceptance() {
        super::e1::consumed_retry("checkpoint-gate-remainder", true, "first\n");
    }

    #[test]
    fn native_remainder_consumed_before_acceptance() {
        super::e1::consumed_retry("checkpoint-gate-remainder", false, "first\n");
    }

    #[test]
    fn native_retained_descendant_still_fails() {
        super::e1::retained_descendant_still_fails("first\n");
    }

    #[test]
    fn native_terminal_consumed_after_acceptance() {
        super::e1::consumed_retry("checkpoint-terminal", true, "first\n");
    }

    #[test]
    fn selecting_replay_intent_before_publication_preserves_staged_atom() {
        super::faults::selecting_publication(false);
    }

    #[test]
    fn selecting_replay_intent_after_publication_resumes_pending_remainder() {
        super::faults::selecting_publication(true);
    }

    #[test]
    fn native_remainder_validated_before_acceptance_publication() {
        super::e1::validated_publication_retry(
            "checkpoint-gate-remainder",
            "generated descendant\n",
        );
    }

    #[test]
    fn native_terminal_verified_before_witness_publication() {
        super::e1::validated_publication_retry("checkpoint-terminal", "generated descendant\n");
    }

    #[test]
    fn native_terminal_consumed_before_acceptance() {
        super::e1::consumed_retry("checkpoint-terminal", false, "first\n");
    }
    #[test]
    fn observes_whole_repository_when_invoked_from_child_directory() {
        super::verify_subdirectory_admission(false);
        super::verify_subdirectory_admission(true);
    }
    #[test]
    fn output_interruption_preserves_captured_progress_and_can_resume() {
        super::faults::output_interruption(1);
        super::faults::output_interruption(2);
    }
    #[test]
    fn preparing_and_opening_readmit_foreign_paths_before_native_mutation() {
        for boundary in [
            super::ReadmissionBoundary::InitialPreparing,
            super::ReadmissionBoundary::RestartPreparing,
            super::ReadmissionBoundary::Opening,
        ] {
            for hard_link in [false, true] {
                super::verify_readmission_collision(boundary, hard_link, "foreign ignored bytes\n");
            }
        }
    }
    #[test]
    fn refuses_closing_phase_while_native_rebase_remains_active() {
        super::verify_impossible_closing();
    }
    #[test]
    fn refuses_corrupt_preparation_before_publishing_opening() {
        super::faults::corrupt_preparation();
    }
    #[test]
    fn new_intent_path_matches_native_commit_tree() {
        let fixture = super::NativeStagedSelection::arrange(super::StagedPath::New);
        let repository = &fixture.repository;
        let root = &repository.environment.cwd;
        let config_before = fs::read(root.join(".git/config")).or_abort("native config bytes");

        let (code, stdout) = repository.invoke(&["--message", "Add native staged atom"]);

        assert_eq!(code, EXIT_OK);
        assert_eq!(repository.output.stderr.borrow().as_str(), "");
        let result: serde_json::Value =
            serde_json::from_str(&stdout).or_abort("one completed checkpoint response");
        assert_eq!(
            result.get("operation").and_then(serde_json::Value::as_str),
            Some("continue")
        );
        assert_eq!(
            result.get("result").and_then(serde_json::Value::as_str),
            Some("committed")
        );
        assert_eq!(
            result
                .get("split_count")
                .and_then(serde_json::Value::as_u64),
            Some(1)
        );
        assert_eq!(
            repository.git(&["rev-parse", "HEAD^{tree}"]),
            fixture.expected_atom_tree
        );
        assert_eq!(
            repository.git(&["rev-parse", "main^{tree}"]),
            fixture.final_tree
        );
        assert_eq!(
            repository.git(&["rev-parse", "protected"]),
            fixture.original
        );
        assert_eq!(
            repository.git(&["rev-parse", "protected-tag"]),
            fixture.original
        );
        assert_eq!(
            fs::read(root.join(".git/config")).or_abort("native config"),
            config_before
        );
        assert_eq!(
            fs::read(root.join("user")).or_abort("user bytes"),
            b"protected user bytes\n"
        );
        assert_eq!(
            fs::read(root.join("atom")).or_abort("atom bytes"),
            b"selected atom\n"
        );
        assert_eq!(
            fs::read(root.join("remainder")).or_abort("remainder bytes"),
            b"selected remainder\n"
        );
    }

    #[test]
    fn head_present_intent_path_matches_native_commit_tree() {
        let fixture = super::NativeStagedSelection::arrange(super::StagedPath::TrackedContent);
        let repository = &fixture.repository;
        let root = &repository.environment.cwd;
        let config_before = fs::read(root.join(".git/config")).or_abort("native config bytes");

        let (code, stdout) = repository.invoke(&["--message", "Add native staged atom"]);

        assert_eq!(code, EXIT_OK);
        assert_eq!(repository.output.stderr.borrow().as_str(), "");
        let result: serde_json::Value =
            serde_json::from_str(&stdout).or_abort("one completed checkpoint response");
        assert_eq!(
            result.get("operation").and_then(serde_json::Value::as_str),
            Some("continue")
        );
        assert_eq!(
            result.get("result").and_then(serde_json::Value::as_str),
            Some("committed")
        );
        assert_eq!(
            result
                .get("split_count")
                .and_then(serde_json::Value::as_u64),
            Some(1)
        );
        assert_eq!(
            repository.git(&["rev-parse", "HEAD^{tree}"]),
            fixture.expected_atom_tree
        );
        assert_eq!(
            repository.git(&["rev-parse", "main^{tree}"]),
            fixture.final_tree
        );
        assert_eq!(
            repository.git(&["rev-parse", "protected"]),
            fixture.original
        );
        assert_eq!(
            repository.git(&["rev-parse", "protected-tag"]),
            fixture.original
        );
        assert_eq!(
            fs::read(root.join(".git/config")).or_abort("native config"),
            config_before
        );
        assert_eq!(
            fs::read(root.join("user")).or_abort("user bytes"),
            b"protected user bytes\n"
        );
        assert_eq!(
            fs::read(root.join("atom")).or_abort("atom bytes"),
            b"selected atom\n"
        );
        assert_eq!(
            fs::read(root.join("remainder")).or_abort("remainder bytes"),
            b"selected remainder\n"
        );
    }

    #[test]
    fn head_present_empty_intent_path_matches_native_commit_tree() {
        let fixture = super::NativeStagedSelection::arrange(super::StagedPath::TrackedEmpty);
        let repository = &fixture.repository;
        let root = &repository.environment.cwd;
        let config_before = fs::read(root.join(".git/config")).or_abort("native config bytes");

        let (code, stdout) = repository.invoke(&["--message", "Add native staged atom"]);

        assert_eq!(code, EXIT_OK);
        assert_eq!(repository.output.stderr.borrow().as_str(), "");
        let result: serde_json::Value =
            serde_json::from_str(&stdout).or_abort("one completed checkpoint response");
        assert_eq!(
            result.get("operation").and_then(serde_json::Value::as_str),
            Some("continue")
        );
        assert_eq!(
            result.get("result").and_then(serde_json::Value::as_str),
            Some("committed")
        );
        assert_eq!(
            result
                .get("split_count")
                .and_then(serde_json::Value::as_u64),
            Some(1)
        );
        assert_eq!(
            repository.git(&["rev-parse", "HEAD^{tree}"]),
            fixture.expected_atom_tree
        );
        assert_eq!(
            repository.git(&["rev-parse", "main^{tree}"]),
            fixture.final_tree
        );
        assert_eq!(
            repository.git(&["rev-parse", "protected"]),
            fixture.original
        );
        assert_eq!(
            repository.git(&["rev-parse", "protected-tag"]),
            fixture.original
        );
        assert_eq!(
            fs::read(root.join(".git/config")).or_abort("native config"),
            config_before
        );
        assert_eq!(
            fs::read(root.join("user")).or_abort("user bytes"),
            b"protected user bytes\n"
        );
        assert_eq!(
            fs::read(root.join("atom")).or_abort("atom bytes"),
            b"selected atom\n"
        );
        assert_eq!(
            fs::read(root.join("remainder")).or_abort("remainder bytes"),
            b"selected remainder\n"
        );
    }

    #[test]
    fn staged_empty_path_is_retained_in_native_commit_tree() {
        let fixture = super::NativeStagedSelection::arrange(super::StagedPath::StagedEmpty);
        let repository = &fixture.repository;
        let root = &repository.environment.cwd;
        let config_before = fs::read(root.join(".git/config")).or_abort("native config bytes");

        let (code, stdout) = repository.invoke(&["--message", "Add native staged atom"]);

        assert_eq!(code, EXIT_OK);
        assert_eq!(repository.output.stderr.borrow().as_str(), "");
        let result: serde_json::Value =
            serde_json::from_str(&stdout).or_abort("one completed checkpoint response");
        assert_eq!(
            result.get("operation").and_then(serde_json::Value::as_str),
            Some("continue")
        );
        assert_eq!(
            result.get("result").and_then(serde_json::Value::as_str),
            Some("committed")
        );
        assert_eq!(
            result
                .get("split_count")
                .and_then(serde_json::Value::as_u64),
            Some(1)
        );
        assert_eq!(
            repository.git(&["rev-parse", "HEAD^{tree}"]),
            fixture.expected_atom_tree
        );
        assert_eq!(
            repository.git(&["rev-parse", "main^{tree}"]),
            fixture.final_tree
        );
        assert_eq!(
            repository.git(&["rev-parse", "protected"]),
            fixture.original
        );
        assert_eq!(
            repository.git(&["rev-parse", "protected-tag"]),
            fixture.original
        );
        assert_eq!(
            fs::read(root.join(".git/config")).or_abort("native config"),
            config_before
        );
        assert_eq!(
            fs::read(root.join("user")).or_abort("user bytes"),
            b"protected user bytes\n"
        );
        assert_eq!(
            fs::read(root.join("atom")).or_abort("atom bytes"),
            b"selected atom\n"
        );
        assert_eq!(
            fs::read(root.join("remainder")).or_abort("remainder bytes"),
            b"selected remainder\n"
        );
    }

    #[test]
    fn abort_preserves_outside_source_intent_to_add_descendant_path() {
        let repository = super::outside_source_intent_repository();
        let root = &repository.environment.cwd;
        let index_before = repository.index();
        let head_before = repository.git(&["rev-parse", "HEAD"]);
        let refs_before = repository.git(&["show-ref"]);
        let files_before = super::log_observation_inventory(root);
        let journal_before: serde_json::Value = serde_json::from_slice(&repository.journal())
            .or_abort("recorded source ownership roots");
        let user_before = fs::read(root.join("gen/notes.md")).or_abort("user bytes");

        let (code, stdout) = repository.invoke(&["--abort"]);

        let index_after = repository.index();
        let user_after = fs::read(root.join("gen/notes.md")).ok();
        assert_eq!(
            (code, stdout, user_after),
            (EXIT_SOFTWARE, String::new(), Some(user_before))
        );
        assert_eq!(index_after, index_before);
        assert_eq!(repository.git(&["rev-parse", "HEAD"]), head_before);
        assert_eq!(repository.git(&["show-ref"]), refs_before);
        assert_eq!(
            repository.output.stderr.borrow().as_str(),
            "git command failed: intent-to-add paths must belong to the remaining selected change\n"
        );
        let mut files_after = super::log_observation_inventory(root);
        let error_log_path = Path::new(".git/factor/error.log");
        assert!(!files_before.contains_key(error_log_path));
        let error_log = files_after
            .remove(error_log_path)
            .or_abort("owned diagnostic log");
        assert!(matches!(error_log, super::LogPath::File(contents)
            if contents.windows(b"error=git command failed: intent-to-add paths must belong to the remaining selected change\n".len())
                .any(|window| window == b"error=git command failed: intent-to-add paths must belong to the remaining selected change\n")));
        let protected_after = super::observe_unreachable_tree_additions(
            &repository,
            &files_before,
            &journal_before,
            files_after,
        );
        assert_eq!(protected_after, files_before);
    }

    #[test]
    fn submission_preserves_outside_source_intent_to_add_descendant_path() {
        let repository = super::outside_source_intent_repository();
        let root = &repository.environment.cwd;
        let index_before = repository.index();
        let head_before = repository.git(&["rev-parse", "HEAD"]);
        let refs_before = repository.git(&["show-ref"]);
        let files_before = super::log_observation_inventory(root);
        let journal_before: serde_json::Value = serde_json::from_slice(&repository.journal())
            .or_abort("recorded source ownership roots");
        let user_before = fs::read(root.join("gen/notes.md")).or_abort("user bytes");

        let (code, stdout) = repository.invoke(&["--message", "Add selected atom"]);

        let index_after = repository.index();
        let user_after = fs::read(root.join("gen/notes.md")).ok();
        assert_eq!(
            (code, stdout, user_after),
            (EXIT_SOFTWARE, String::new(), Some(user_before))
        );
        assert_eq!(index_after, index_before);
        assert_eq!(repository.git(&["rev-parse", "HEAD"]), head_before);
        assert_eq!(repository.git(&["show-ref"]), refs_before);
        assert_eq!(
            repository.output.stderr.borrow().as_str(),
            "git command failed: intent-to-add paths must belong to the remaining selected change\n"
        );
        let mut files_after = super::log_observation_inventory(root);
        let error_log_path = Path::new(".git/factor/error.log");
        assert!(!files_before.contains_key(error_log_path));
        let error_log = files_after
            .remove(error_log_path)
            .or_abort("owned diagnostic log");
        assert!(matches!(error_log, super::LogPath::File(contents)
            if contents.windows(b"error=git command failed: intent-to-add paths must belong to the remaining selected change\n".len())
                .any(|window| window == b"error=git command failed: intent-to-add paths must belong to the remaining selected change\n")));
        let protected_after = super::observe_unreachable_tree_additions(
            &repository,
            &files_before,
            &journal_before,
            files_after,
        );
        assert_eq!(protected_after, files_before);
    }

    #[test]
    fn clean_admission_refuses_head_present_empty_intention_before_mutation() {
        let repository = super::Repository::new();
        let root = &repository.environment.cwd;
        repository.write("empty", "");
        repository.write("atom", "original atom\n");
        repository.commit("Add original empty path");
        repository.write("atom", "selected atom\n");
        repository.commit("Change selected atom");
        repository.git(&["branch", "protected"]);
        repository.git(&["tag", "protected-tag"]);
        repository.write("user", "protected user bytes\n");
        repository.git(&["rm", "--cached", "--", "empty"]);
        repository.git(&["add", "--intent-to-add", "--", "empty"]);
        let index_before = repository.index();
        let head_before = repository.git(&["rev-parse", "HEAD"]);
        let refs_before = repository.git(&["show-ref"]);
        let files_before = super::log_observation_inventory(root);

        let (code, stdout) = repository.invoke(&["--exec", "true", "HEAD"]);

        let index_after = repository.index();
        assert_eq!(code, EXIT_SOFTWARE);
        assert_eq!(stdout, "");
        assert_eq!(
            repository.output.stderr.borrow().as_str(),
            "git command failed: tracked working tree and staged tree must match HEAD\n"
        );
        assert_eq!(index_after, index_before);
        assert_eq!(repository.git(&["rev-parse", "HEAD"]), head_before);
        assert_eq!(repository.git(&["show-ref"]), refs_before);
        assert_eq!(super::log_observation_inventory(root), files_before);
    }

    #[test]
    fn subdirectory_intent_projection_matches_native_commit_tree() {
        let fixture = super::NativeStagedSelection::arrange(super::StagedPath::TrackedContent);
        let repository = &fixture.repository;
        let root = &repository.environment.cwd;
        let child = root.join("sub");
        fs::create_dir_all(&child).or_abort("caller child directory");
        repository.output.stdout.borrow_mut().clear();
        repository.output.stderr.borrow_mut().clear();
        let config_before = fs::read(root.join(".git/config")).or_abort("native config");
        let context = super::Ctx {
            cwd: child,
            env: &repository.environment,
            fs: &super::REAL_FS,
            io: &repository.output,
            runner: &super::REAL_RUNNER,
        };
        let arguments =
            ["git-factor", "--message", "Add native staged atom"].map(super::OsString::from);

        let code = super::main_entry_with_vec(context.io, Ok(context), &arguments);

        assert_eq!(code, EXIT_OK);
        assert_eq!(repository.output.stderr.borrow().as_str(), "");
        let result: serde_json::Value = serde_json::from_str(&repository.output.stdout.borrow())
            .or_abort("one completed checkpoint response");
        assert_eq!(
            result.get("operation").and_then(serde_json::Value::as_str),
            Some("continue")
        );
        assert_eq!(
            result.get("result").and_then(serde_json::Value::as_str),
            Some("committed")
        );
        assert_eq!(
            result
                .get("split_count")
                .and_then(serde_json::Value::as_u64),
            Some(1)
        );
        assert_eq!(
            repository.git(&["rev-parse", "HEAD^{tree}"]),
            fixture.expected_atom_tree
        );
        assert_eq!(
            repository.git(&["rev-parse", "main^{tree}"]),
            fixture.final_tree
        );
        assert_eq!(
            repository.git(&["rev-parse", "protected"]),
            fixture.original
        );
        assert_eq!(
            repository.git(&["rev-parse", "protected-tag"]),
            fixture.original
        );
        assert_eq!(
            fs::read(root.join(".git/config")).or_abort("native config"),
            config_before
        );
        assert_eq!(
            fs::read(root.join("user")).or_abort("user bytes"),
            b"protected user bytes\n"
        );
        assert_eq!(
            fs::read(root.join("atom")).or_abort("atom bytes"),
            b"selected atom\n"
        );
        assert_eq!(
            fs::read(root.join("remainder")).or_abort("remainder bytes"),
            b"selected remainder\n"
        );
    }

    #[test]
    fn trailing_space_linked_root_preserves_native_intent_projection() {
        let mut repository = super::Repository::new();
        let main = repository.environment.cwd.clone();
        repository.write("base", "original tree\n");
        repository.commit("Add original tree");
        repository.write("atom", "selected atom\n");
        repository.write("intent", "selected intention\n");
        repository.write("remainder", "selected remainder\n");
        let original = repository.commit("Add selected source");
        let final_tree = repository.git(&["rev-parse", "HEAD^{tree}"]);
        repository.git(&["tag", "protected-tag"]);
        fs::create_dir_all(main.join("wt")).or_abort("similarly named main directory");
        fs::write(main.join("wt/user"), b"protected main bytes\n").or_abort("main user bytes");
        let linked = main.join("wt ");
        repository.git(&[
            "worktree",
            "add",
            "--quiet",
            "-b",
            "selected",
            linked.to_str().or_abort("linked root UTF-8"),
            "main",
        ]);
        repository.environment.cwd = linked.clone();
        let admin = super::PathBuf::from(repository.git(&["rev-parse", "--absolute-git-dir"]));
        repository.success(&["--exec", "true", "HEAD"]);
        repository.git(&["add", "--", "atom"]);
        repository.git(&["add", "--intent-to-add", "--", "intent"]);
        let expected_atom_tree = repository.git(&["write-tree"]);
        repository.write("user", "protected linked bytes\n");
        let main_index_before = fs::read(main.join(".git/index")).or_abort("main raw index");
        let linked_index_before = fs::read(admin.join("index")).or_abort("linked raw index");
        let config_before = fs::read(main.join(".git/config")).or_abort("common config");
        let journal_before: serde_json::Value = serde_json::from_slice(
            &fs::read(admin.join("factor-journal.json")).or_abort("linked journal"),
        )
        .or_abort("recorded selected checkpoint");
        let expected_checkpoint = serde_json::Value::String(original.clone());
        assert_eq!(journal_before.get("checkpoint"), Some(&expected_checkpoint));

        let (code, stdout) = repository.invoke(&["--message", "Add linked native atom"]);

        let main_index_after =
            fs::read(main.join(".git/index")).or_abort("main index first readback");
        let linked_index_after =
            fs::read(admin.join("index")).or_abort("linked index first readback");
        assert_eq!(code, EXIT_OK, "{}", repository.output.stderr.borrow());
        assert_eq!(repository.output.stderr.borrow().as_str(), "");
        let response: serde_json::Value =
            serde_json::from_str(&stdout).or_abort("one checkpoint response");
        assert_eq!(
            response.get("result").and_then(serde_json::Value::as_str),
            Some("committed")
        );
        assert_eq!(
            repository.git(&["rev-parse", "HEAD^{tree}"]),
            expected_atom_tree
        );
        assert_eq!(
            repository.git(&["rev-parse", "selected^{tree}"]),
            final_tree
        );
        assert_eq!(repository.git(&["rev-parse", "main"]), original);
        assert_eq!(repository.git(&["rev-parse", "protected-tag"]), original);
        assert_eq!(main_index_after, main_index_before);
        assert_ne!(
            linked_index_after, linked_index_before,
            "owned capture reopens an unstaged remainder"
        );
        assert_eq!(
            fs::read(main.join(".git/config")).or_abort("common config"),
            config_before
        );
        assert_eq!(
            fs::read(main.join("wt/user")).or_abort("main user bytes"),
            b"protected main bytes\n"
        );
        assert_eq!(
            fs::read(linked.join("user")).or_abort("linked user bytes"),
            b"protected linked bytes\n"
        );
        assert_eq!(
            fs::read(linked.join("intent")).or_abort("remaining intention"),
            b"selected intention\n"
        );
        let reopened: serde_json::Value = serde_json::from_slice(
            &fs::read(admin.join("factor-journal.json")).or_abort("reopened journal"),
        )
        .or_abort("reopened linked selection");
        assert_eq!(
            reopened
                .pointer("/state/phase")
                .and_then(serde_json::Value::as_str),
            Some("selecting")
        );
        assert_eq!(
            reopened
                .get("final_tree")
                .and_then(serde_json::Value::as_str),
            Some(final_tree.as_str())
        );
    }

    #[test]
    fn split_index_projection_preserves_native_binary_mode_symlink_and_path_semantics() {
        let repository = super::Repository::new();
        let root = &repository.environment.cwd;
        repository.write("base", "original tree\n");
        repository.commit("Add original tree");
        let intent_path = "intent-\u{3bb}";
        let binary_path = "binary space\nname";
        let binary = b"selected\0binary\xffbytes\n";
        fs::write(root.join(intent_path), b"remaining intent bytes\n")
            .or_abort("source-owned Unicode path");
        fs::write(root.join(binary_path), binary).or_abort("binary source bytes");
        repository.write("executable", "#!/bin/sh\nexit 0\n");
        fs::set_permissions(
            root.join("executable"),
            super::fs::Permissions::from_mode(0o755),
        )
        .or_abort("native executable mode");
        super::symlink(binary_path, root.join("link")).or_abort("native symbolic link");
        repository.write("remainder", "remaining selected bytes\n");
        let checkpoint = repository.commit("Add mixed native path objects");
        let final_tree = repository.git(&["rev-parse", "HEAD^{tree}"]);
        repository.git(&["branch", "protected"]);
        repository.git(&["tag", "protected-tag"]);
        repository.success(&["--exec", "true", "HEAD"]);
        repository.git(&["add", "--", binary_path, "executable", "link"]);
        let intent = super::Command::new("git")
            .args(["add", "--intent-to-add", "--"])
            .arg(intent_path)
            .current_dir(root)
            .output()
            .or_abort("native Unicode intent entry");
        assert!(intent.status.success(), "{:?}", intent.stderr);
        repository.git(&["update-index", "--split-index"]);
        let expected_tree = repository.git(&["write-tree"]);
        let config_before = fs::read(root.join(".git/config")).or_abort("native config");
        repository.write("user", "protected user bytes\n");

        let (code, stdout) = repository.invoke(&["--message", "Add mixed native atom"]);

        assert_eq!(code, EXIT_OK);
        assert_eq!(repository.output.stderr.borrow().as_str(), "");
        let result: serde_json::Value =
            serde_json::from_str(&stdout).or_abort("one checkpoint response");
        assert_eq!(
            result.get("result").and_then(serde_json::Value::as_str),
            Some("committed")
        );
        assert_eq!(repository.git(&["rev-parse", "HEAD^{tree}"]), expected_tree);
        assert_eq!(repository.git(&["rev-parse", "main^{tree}"]), final_tree);
        assert_eq!(repository.git(&["rev-parse", "protected"]), checkpoint);
        assert_eq!(repository.git(&["rev-parse", "protected-tag"]), checkpoint);
        assert_eq!(
            fs::read(root.join(".git/config")).or_abort("native config"),
            config_before
        );
        assert_eq!(
            fs::read(root.join(binary_path)).or_abort("binary bytes"),
            binary
        );
        assert_eq!(
            fs::read_link(root.join("link")).or_abort("link target"),
            super::PathBuf::from(binary_path)
        );
        assert_eq!(
            fs::read(root.join(intent_path)).or_abort("intent bytes"),
            b"remaining intent bytes\n"
        );
        assert_eq!(
            fs::read(root.join("user")).or_abort("user bytes"),
            b"protected user bytes\n"
        );
    }

    #[test]
    fn unmerged_index_refuses_before_intent_projection_and_preserves_selection() {
        let repository = super::Repository::new();
        let root = &repository.environment.cwd;
        repository.write("atom", "original atom\n");
        let base = repository.commit("Add original atom");
        repository.write("atom", "selected atom\n");
        repository.write("remainder", "remaining selected bytes\n");
        let source = repository.commit("Change selected atom");
        repository.git(&["checkout", "--quiet", "--detach", &base]);
        repository.write("atom", "conflicting atom\n");
        let conflicting = repository.commit("Change conflicting atom");
        repository.git(&["checkout", "--quiet", "main"]);
        repository.git(&["branch", "protected", &conflicting]);
        repository.git(&["tag", "protected-tag", &conflicting]);
        repository.success(&["--exec", "true", "HEAD"]);
        repository.git(&["read-tree", &source]);
        repository.git(&["read-tree", "-m", "-i", &base, &source, &conflicting]);
        assert!(!repository.git(&["ls-files", "--unmerged"]).is_empty());
        repository.write("user", "protected user bytes\n");
        let index_before = repository.index();
        let head_before = repository.git(&["rev-parse", "HEAD"]);
        let refs_before = repository.git(&["show-ref"]);
        let files_before = super::log_observation_inventory(root);

        let (code, stdout) = repository.invoke(&["--message", "Add unresolved atom"]);

        let index_after = repository.index();
        assert_eq!(code, EXIT_SOFTWARE);
        assert_eq!(stdout, "");
        assert_eq!(
            repository.output.stderr.borrow().as_str(),
            "git command failed: cannot observe staged tree with unresolved entries\n"
        );
        assert_eq!(index_after, index_before);
        assert_eq!(repository.git(&["rev-parse", "HEAD"]), head_before);
        assert_eq!(repository.git(&["show-ref"]), refs_before);
        let mut files_after = super::log_observation_inventory(root);
        let log_path = Path::new(".git/factor/error.log");
        assert!(!files_before.contains_key(log_path));
        assert!(
            matches!(files_after.remove(log_path), Some(super::LogPath::File(contents))
            if contents.windows(b"cannot observe staged tree with unresolved entries".len())
                .any(|window| window == b"cannot observe staged tree with unresolved entries"))
        );
        assert_eq!(files_after, files_before);
    }

    #[test]
    fn intent_to_add_only_is_an_empty_native_selection() {
        let repository = super::open_selection();
        repository.git(&["branch", "protected"]);
        repository.git(&["tag", "protected-tag"]);
        repository.write("user", "protected user bytes\n");
        repository.git(&["add", "--intent-to-add", "--", "atom"]);
        let root = &repository.environment.cwd;
        let index_before = repository.index();
        let head_before = repository.git(&["rev-parse", "HEAD"]);
        let refs_before = repository.git(&["show-ref"]);
        let files_before = super::log_observation_inventory(root);
        let journal_before: serde_json::Value = serde_json::from_slice(&repository.journal())
            .or_abort("independent journal graph roots");

        let (code, stdout) = repository.invoke(&["--message", "Add selected atom"]);

        let index_after = repository.index();
        assert_eq!(code, EXIT_USAGE);
        assert_eq!(stdout, "");
        assert_eq!(
            repository.output.stderr.borrow().as_str(),
            "no staged changes to commit\nNEXT: stage exactly one atomic change, then rerun:\n  git factor --continue --message \"type: description\"\n"
        );
        assert_eq!(index_after, index_before);
        assert_eq!(repository.git(&["rev-parse", "HEAD"]), head_before);
        assert_eq!(repository.git(&["show-ref"]), refs_before);
        let files_after = super::observe_unreachable_tree_additions(
            &repository,
            &files_before,
            &journal_before,
            super::log_observation_inventory(root),
        );
        assert_eq!(files_after, files_before);
    }

    #[test]
    fn refuses_empty_selection_before_head_index_or_journal_mutation() {
        super::verify_empty_admission("base\n");
    }
    #[test]
    fn refuses_fabricated_completed_zero_progress_before_owned_cleanup() {
        super::verify_completed_zero_refusal();
    }

    #[test]
    fn refuses_nonancestor_before_gate_or_journal_through_actual_dispatch() {
        super::verify_nonancestor("selected native bytes\n", "preserved user bytes\n");
    }
    #[test]
    fn refuses_recreated_deleted_path_outside_invocation_directory() {
        super::verify_recreated_deleted_path();
    }
    #[test]
    fn refuses_same_sha_foreign_attachment_for_each_mutating_operation() {
        super::verify_changed_attachment();
    }
    #[test]
    fn refuses_unowned_same_tree_source_metadata_before_any_mutation() {
        super::verify_corrupt_source();
    }
    #[test]
    fn requires_supported_released_git_before_checkpoint_effects() {
        super::version_boundaries(false);
    }
    #[test]
    fn run_and_report_skips_error_log_for_expected_continue_recovery() {
        use crate::test_support::OrAbort as _;

        let repository = super::Repository::new();
        repository.write("base", "base\n");
        repository.commit("Base");
        repository.write("atom", "selected atom\n");
        repository.write("remainder", "selected remainder\n");
        let target = repository.commit("Combined selected change");
        repository.write("unrelated", "preserve user bytes\n");
        repository.success(&["--exec", "true", &target]);
        let before = repository.journal();
        let journal = serde_json::from_slice::<serde_json::Value>(&before)
            .or_abort("admitted selecting journal");
        assert_eq!(
            journal
                .pointer("/state/phase")
                .and_then(serde_json::Value::as_str),
            Some("selecting")
        );
        assert_eq!(repository.git(&["diff", "--cached", "--name-only"]), "");
        let head = repository.git(&["rev-parse", "HEAD"]);
        let references = repository.git(&["show-ref"]);
        let difference = repository.git(&["diff"]);
        let index = repository.index();

        let (code, stdout) = repository.invoke(&["--continue", "--message", "Add selected atom"]);

        assert_eq!((code, stdout, repository.output.stderr.borrow().clone()), (
            EXIT_USAGE,
            String::new(),
            "no staged changes to commit\nNEXT: stage exactly one atomic change, then rerun:\n  git factor --continue --message \"type: description\"\n".to_owned(),
        ));
        assert_eq!(repository.journal(), before);
        assert_eq!(repository.index(), index);
        assert_eq!(repository.git(&["rev-parse", "HEAD"]), head);
        assert_eq!(repository.git(&["show-ref"]), references);
        assert_eq!(repository.git(&["diff"]), difference);
        assert_eq!(repository.git(&["diff", "--cached", "--name-only"]), "");
        assert_eq!(
            fs::read(repository.environment.cwd.join("unrelated"))
                .or_abort("preserved unrelated bytes"),
            b"preserve user bytes\n"
        );
        assert!(
            !repository
                .environment
                .cwd
                .join(".git/factor/error.log")
                .exists()
        );
    }
    #[test]
    fn start_refuses_original_checkout_collisions_before_journal_publication() {
        for alias in [false, true] {
            for ignored in [false, true] {
                super::verify_initial_checkout_collision(alias, ignored);
            }
        }
    }
    #[test]
    fn status_observes_without_removing_the_running_candidate_actor() {
        super::faults::concurrent_status();
    }
}
use crate::exit_codes::{EXIT_DATAERR, EXIT_OK, EXIT_SOFTWARE, EXIT_TEMPFAIL, EXIT_USAGE};
use crate::test_support::OrAbort as _;
use alloc::collections::BTreeMap;
use core::cell::{Cell, RefCell};
use core::iter;
use core::num::NonZeroU32;
use core::ops::Range;
use core::time::Duration;
use std::ffi::{OsStr, OsString};
use std::io;
use std::os::unix::ffi::OsStringExt as _;
use std::os::unix::fs::{PermissionsExt as _, symlink};
use std::os::unix::process::ExitStatusExt as _;
use std::path::{Path, PathBuf};
use std::process::{self, Command, ExitStatus, Output};
use std::{env, fs, thread};

use tempfile::TempDir;

use crate::git_factor::ctx::{Env, Fs, Io, REAL_FS, REAL_IO, REAL_RUNNER, Runner};
use crate::git_factor::{Ctx, main_entry_with_vec, shell_quote};

/// Native index flags and commit trees arranged without a factor session.
pub(in crate::git_factor::engine) struct NativeIndexObservation {
    /// Parent of the independently committed selected source.
    base: super::CommitSha,
    /// Native flag-preserving write-tree result, not a reconstructed projection.
    expected_tree: String,
    /// Complete native fixture, including independently held source refs.
    repository: Repository,
    /// Source defining the allowed selection paths.
    source: super::CommitSha,
}

/// Complete before/after frame; unreferenced loose tree additions are checked separately.
#[derive(Debug, PartialEq, Eq)]
pub(in crate::git_factor::engine) struct NativeIndexFrame {
    /// Every preexisting native, tracked and unrelated path and byte sequence.
    files: BTreeMap<PathBuf, LogPath>,
    /// Current native HEAD identity.
    head: String,
    /// Real index bytes read before any native readback.
    index: Vec<u8>,
    /// All refs, including the source's protected ref.
    refs: String,
}

/// The original selected tip and replayed descendants remain actual message consumers.
#[derive(Clone, Copy)]
enum MessageLocation {
    Descendant,
    Tip,
}

/// Unsupported native message grammar arranged before any factor invocation.
struct SourceMessageAdmission {
    /// Exact independently expected source-message refusal.
    expected_error: String,
    /// A gate with an external marker proving whether validation ran.
    gate: String,
    /// Owned external gate marker, outside all actor/source trees.
    gate_marker: PathBuf,
    /// Actual native source repository.
    repository: Repository,
    /// Original selected tip; a descendant message may be the unsupported consumer.
    target: String,
}

/// Native branch identity beside a same-tip alias with a different UTF-8 suffix.
struct NativeBranch {
    native: NativeAuthor,
    protected: String,
}

impl NativeBranch {
    fn arrange() -> Self {
        let native = NativeAuthor::arrange(MessageLocation::Tip, false);
        let repository = &native.repository;
        repository.git(&["branch", "topic"]);
        repository.git(&["checkout", "--quiet", "-b", "topic\u{a0}"]);
        let protected = repository.git(&[
            "show-ref",
            "--verify",
            "refs/heads/main",
            "refs/heads/topic",
            "refs/heads/protected",
            "refs/tags/protected-tag",
        ]);
        Self { native, protected }
    }

    fn frame(&self) -> NativeIndexFrame {
        let repository = &self.native.repository;
        let index = repository.index();
        NativeIndexFrame {
            files: log_observation_inventory(&repository.environment.cwd),
            head: repository.git(&["rev-parse", "HEAD"]),
            index,
            refs: repository.git(&["show-ref"]),
        }
    }

    fn protected_refs(&self) -> String {
        self.native.repository.git(&[
            "show-ref",
            "--verify",
            "refs/heads/main",
            "refs/heads/topic",
            "refs/heads/protected",
            "refs/tags/protected-tag",
        ])
    }
}

struct NativeAuthor {
    /// Raw original author header, independent of formatted log output.
    author: Vec<u8>,
    /// Selection parent, before the original tip.
    base: String,
    /// Configuration bytes which must remain unchanged.
    config: Vec<u8>,
    /// Original completed tree, including a possible replayed descendant.
    final_tree: String,
    /// Unrelated branches and tags at their original identities.
    protected: String,
    /// Actual native repository and immutable callback launcher.
    repository: Repository,
    /// Native unambiguous original source abbreviation.
    short_source: String,
    /// Original selected source, separate from any descendant.
    source: String,
}

#[derive(Clone, Copy)]
enum NativeAuthorAlias {
    NegativeZero,
    TrailingNameSpace,
}

impl NativeAuthor {
    fn arrange(location: MessageLocation, encoded_source: bool) -> Self {
        let repository = Repository::new();
        if encoded_source {
            repository.git(&["config", "i18n.commitEncoding", "ISO-8859-1"]);
        }
        repository.write("atom", "base\n");
        let base = repository.commit("Add author base");
        repository.write("atom", "base\natom\nremainder\n");
        repository.git(&["add", "--all"]);
        match location {
            MessageLocation::Tip => {
                repository.git(&[
                    "-c",
                    "user.name=Jos\u{e9}",
                    "commit",
                    "--quiet",
                    "--message",
                    "Add author source",
                ]);
            }
            MessageLocation::Descendant => {
                repository.git(&["commit", "--quiet", "--message", "Add author source"]);
            }
        }
        let source = repository.git(&["rev-parse", "HEAD"]);
        let short_source = repository.git(&["rev-parse", "--short", "HEAD"]);
        if matches!(location, MessageLocation::Descendant) {
            repository.write("tail", "replayed descendant\n");
            repository.git(&["add", "--all"]);
            repository.git(&[
                "-c",
                "user.name=Jos\u{e9}",
                "commit",
                "--quiet",
                "--message",
                "Add author descendant",
            ]);
        }
        let author = Self::raw_author(&repository, "HEAD");
        assert!(author.starts_with("author Jos\u{e9} <".as_bytes()));
        let final_tree = repository.git(&["rev-parse", "HEAD^{tree}"]);
        repository.git(&["branch", "protected"]);
        repository.git(&["tag", "protected-tag"]);
        let protected = repository.git(&[
            "show-ref",
            "--verify",
            "refs/heads/protected",
            "refs/tags/protected-tag",
        ]);
        repository.git(&["config", "i18n.logOutputEncoding", "ISO-8859-1"]);
        repository.write("user", "unrelated author bytes\0\n");
        let config = fs::read(repository.environment.cwd.join(".git/config"))
            .or_abort("original config bytes");
        Self {
            author,
            base,
            config,
            final_tree,
            protected,
            repository,
            short_source,
            source,
        }
    }

    fn arrange_alias(alias: NativeAuthorAlias) -> Self {
        let mut fixture = Self::arrange(MessageLocation::Tip, false);
        let repository = &fixture.repository;
        let original = repository.git(&["cat-file", "commit", &fixture.source]);
        let old_author =
            String::from_utf8(fixture.author.clone()).or_abort("original UTF-8 author header");
        let (name, date) = match alias {
            NativeAuthorAlias::NegativeZero => ("Jos\u{e9}", "1000000000 -0000"),
            NativeAuthorAlias::TrailingNameSpace => ("Jos\u{e9}  ", "1000000000 +0000"),
        };
        let (_, metadata) = old_author
            .split_once(" <")
            .or_abort("original author identity");
        let (email, _) = metadata.split_once("> ").or_abort("original author date");
        let replacement = format!("author {name} <{email}> {date}");
        let object = repository.directory.path().join("native-author-alias");
        fs::write(&object, original.replacen(&old_author, &replacement, 1))
            .or_abort("valid native author alias object");
        fixture.source = repository.git(&[
            "hash-object",
            "-t",
            "commit",
            "-w",
            object.to_str().or_abort("native object path"),
        ]);
        repository.git(&["fsck", "--strict", &fixture.source]);
        repository.git(&["reset", "--soft", &fixture.source]);
        fixture.short_source = repository.git(&["rev-parse", "--short", "HEAD"]);
        let control = Command::new("git")
            .args([
                "commit-tree",
                &fixture.final_tree,
                "-m",
                "Add native author control",
            ])
            .env("GIT_AUTHOR_NAME", name)
            .env("GIT_AUTHOR_EMAIL", email)
            .env("GIT_AUTHOR_DATE", date)
            .current_dir(&repository.environment.cwd)
            .output()
            .or_abort("independent native author construction");
        assert!(control.status.success());
        let control_identity =
            String::from_utf8(control.stdout).or_abort("native control identity");
        fixture.author = Self::raw_author(repository, control_identity.trim_end());
        fixture
    }

    fn arrange_epoch(location: MessageLocation, epoch: u32) -> Self {
        let mut fixture = Self::arrange(location, false);
        let repository = &fixture.repository;
        let date = format!("@{epoch} +0000");
        let output = Command::new("git")
            .args(["commit", "--amend", "--quiet", "--no-edit", "--date", &date])
            .current_dir(&repository.environment.cwd)
            .output()
            .or_abort("native early-epoch author construction");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        fixture.author = Self::raw_author(repository, "HEAD");
        assert!(
            fixture
                .author
                .ends_with(format!("> {epoch} +0000").as_bytes())
        );
        if matches!(location, MessageLocation::Tip) {
            fixture.source = repository.git(&["rev-parse", "HEAD"]);
            fixture.short_source = repository.git(&["rev-parse", "--short", "HEAD"]);
        }
        repository.git(&["branch", "--force", "protected", "HEAD"]);
        repository.git(&["tag", "--force", "protected-tag", "HEAD"]);
        fixture.protected = repository.git(&[
            "show-ref",
            "--verify",
            "refs/heads/protected",
            "refs/tags/protected-tag",
        ]);
        fixture
    }

    /// Constructs the complete expected start response from original fixture inputs.
    fn expected_start(&self) -> serde_json::Value {
        let commit_count: u64 = 1;
        let mut expected = OriginalPool::expected_output();
        let fields = expected
            .as_object_mut()
            .or_abort("expected selection fields");
        fields.remove("result");
        fields.remove("split_count");
        fields.insert(
            "operation".to_owned(),
            serde_json::Value::String("start".to_owned()),
        );
        fields.insert(
            "changes".to_owned(),
            serde_json::from_str(concat!(
                r#"{"unstaged":[{"path":"atom","kind":"text","added":2,"deleted":0}],"#,
                r#""untracked":["user"]}"#,
            ))
            .or_abort("expected initial changes"),
        );
        fields.insert(
            "target".to_owned(),
            serde_json::Value::Object(serde_json::Map::from_iter([
                (
                    "commit".to_owned(),
                    serde_json::Value::String(self.source.clone()),
                ),
                (
                    "commit_count".to_owned(),
                    serde_json::Value::Number(commit_count.into()),
                ),
                (
                    "message".to_owned(),
                    serde_json::Value::String("Add author source".to_owned()),
                ),
                (
                    "short_commit".to_owned(),
                    serde_json::Value::String(self.short_source.clone()),
                ),
            ])),
        );
        expected
    }

    fn raw_author(repository: &Repository, commit: &str) -> Vec<u8> {
        let output = Command::new("git")
            .args(["cat-file", "commit", commit])
            .current_dir(&repository.environment.cwd)
            .output()
            .or_abort("raw native author object");
        assert!(output.status.success());
        output
            .stdout
            .split(|byte| *byte == b'\n')
            .find(|line| line.starts_with(b"author "))
            .or_abort("original raw author header")
            .to_vec()
    }
}

impl SourceMessageAdmission {
    fn arrange(location: MessageLocation, non_utf8: bool) -> Self {
        let repository = Repository::new();
        repository.write("base", "original tree\n");
        repository.commit("Add original tree");
        repository.write("atom", "selected atom\n");
        let original = matches!(location, MessageLocation::Descendant)
            .then(|| repository.commit("Add selected source"));
        if original.is_some() {
            repository.write("descendant", "replayed descendant\n");
        }
        let message = repository.directory.path().join("native-message");
        fs::write(
            &message,
            if non_utf8 {
                b"invalid \xff\n".as_slice()
            } else {
                b"".as_slice()
            },
        )
        .or_abort("unsupported native message bytes");
        repository.git(&["add", "--all"]);
        repository.git(&[
            "-c",
            "i18n.commitEncoding=ISO-8859-1",
            "commit",
            "--quiet",
            "--allow-empty-message",
            "--cleanup=verbatim",
            "--file",
            message.to_str().or_abort("native message path"),
        ]);
        let bad_commit = repository.git(&["rev-parse", "HEAD"]);
        let target = original.unwrap_or_else(|| bad_commit.clone());
        let expected_error = if non_utf8 {
            let raw = Command::new("git")
                .args(["cat-file", "commit", &bad_commit])
                .current_dir(&repository.environment.cwd)
                .output()
                .or_abort("native raw commit");
            assert!(raw.status.success());
            let error = String::from_utf8(raw.stdout)
                .err()
                .or_abort("actual non-UTF-8 commit object");
            format!("git command failed: candidate source message is not UTF-8: {error}\n")
        } else {
            String::from(
                "git command failed: empty candidate source message: value must not be empty\n",
            )
        };
        repository.git(&["branch", "protected"]);
        repository.git(&["tag", "protected-tag"]);
        repository.write("unrelated", "protected user bytes\0\n");
        let gate_marker = repository.directory.path().join("gate-ran");
        let gate = format!(
            "printf ran > {}",
            shell_quote(gate_marker.to_str().or_abort("gate marker path"))
        );
        Self {
            expected_error,
            gate,
            gate_marker,
            repository,
            target,
        }
    }

    fn frame(&self) -> NativeIndexFrame {
        let index = self.repository.index();
        NativeIndexFrame {
            files: log_observation_inventory(&self.repository.environment.cwd),
            head: self.repository.git(&["rev-parse", "HEAD"]),
            index,
            refs: self.repository.git(&["show-ref"]),
        }
    }
}

impl NativeIndexObservation {
    /// Arranges native flags through Git's own index operations.
    pub(in crate::git_factor::engine) fn arrange(
        world: StagedPath,
        path: &str,
        content: &[u8],
    ) -> Self {
        let repository = Repository::new();
        let root = &repository.environment.cwd;
        repository.write("atom", "original atom\n");
        match world {
            StagedPath::TrackedContent => repository.write(path, "original path\n"),
            StagedPath::TrackedEmpty => repository.write(path, ""),
            StagedPath::New | StagedPath::StagedEmpty => {}
        }
        let base = repository.commit("Add native index base");
        repository.write("atom", "selected atom\n");
        let mut selected: Vec<u8> = vec![0];
        selected.extend_from_slice(content);
        fs::write(
            root.join(path),
            if matches!(world, StagedPath::StagedEmpty) {
                &[]
            } else {
                selected.as_slice()
            },
        )
        .or_abort("generated source bytes");
        let source = repository.commit("Change native index source");
        repository.git(&["branch", "protected-source"]);
        repository.git(&["reset", "--mixed", &base]);
        match world {
            StagedPath::TrackedContent | StagedPath::TrackedEmpty => {
                repository.git(&["rm", "--cached", "--", path]);
                repository.git(&["add", "--intent-to-add", "--", path]);
            }
            StagedPath::New => {
                repository.git(&["add", "--intent-to-add", "--", path]);
            }
            StagedPath::StagedEmpty => {
                repository.git(&["add", "--", path]);
            }
        }
        repository.git(&["add", "--", "atom"]);
        let expected_tree = repository.git(&["write-tree"]);
        repository.write("unrelated", "protected user bytes\0\n");
        Self {
            base: super::CommitSha::new(base).or_abort("native base commit"),
            expected_tree,
            repository,
            source: super::CommitSha::new(source).or_abort("native source commit"),
        }
    }

    /// Independently committed selection parent.
    pub(in crate::git_factor::engine) const fn base(&self) -> &super::CommitSha {
        &self.base
    }

    /// Borrow the actual repository without changing cwd or process environment.
    pub(in crate::git_factor::engine) fn context(&self) -> Ctx<'_> {
        Ctx {
            cwd: self.repository.environment.cwd.clone(),
            env: &self.repository.environment,
            fs: &REAL_FS,
            io: &self.repository.output,
            runner: &REAL_RUNNER,
        }
    }

    /// Native commit tree observed with the real index flags intact.
    pub(in crate::git_factor::engine) fn expected_tree(&self) -> &str {
        &self.expected_tree
    }

    /// Read real index bytes first, then the independent native/file frame.
    pub(in crate::git_factor::engine) fn frame(&self) -> NativeIndexFrame {
        let index = self.repository.index();
        NativeIndexFrame {
            files: log_observation_inventory(&self.repository.environment.cwd),
            head: self.repository.git(&["rev-parse", "HEAD"]),
            index,
            refs: self.repository.git(&["show-ref"]),
        }
    }

    /// Validate only new unreachable trees, preserving the complete original frame.
    pub(in crate::git_factor::engine) fn observed_frame(
        &self,
        before: &NativeIndexFrame,
    ) -> NativeIndexFrame {
        let mut after = self.frame();
        // This primitive fixture has no journal; native refs hold base and source.
        after.files = observe_unreachable_tree_additions(
            &self.repository,
            &before.files,
            &serde_json::Value::Null,
            after.files,
        );
        after
    }

    /// Independently committed source defining allowed paths.
    pub(in crate::git_factor::engine) const fn source(&self) -> &super::CommitSha {
        &self.source
    }
}

/// Native commit distinguishes intention from a deliberately staged empty file.
#[derive(Clone, Copy, Debug)]
pub(in crate::git_factor::engine) enum StagedPath {
    New,
    StagedEmpty,
    TrackedContent,
    TrackedEmpty,
}

struct NativeStagedSelection {
    expected_atom_tree: String,
    final_tree: String,
    original: String,
    repository: Repository,
}

/// Constructively malformed released-version observations; each input is rejected independently.
#[derive(Clone, Copy, Debug)]
pub(in crate::git_factor::engine) enum MalformedGitVersion {
    EmptyMajor,
    EmptyMinor,
    EmptyPatch,
    MissingPatch,
    NondigitMajor,
    NondigitMinor,
    NondigitPatch,
    OverflowMajor,
    OverflowMinor,
    OverflowPatch,
    UnexpectedDistribution,
}
impl MalformedGitVersion {
    pub(in crate::git_factor::engine) const ALL: &'static [Self] = &[
        Self::EmptyMajor,
        Self::EmptyMinor,
        Self::EmptyPatch,
        Self::MissingPatch,
        Self::NondigitMajor,
        Self::NondigitMinor,
        Self::NondigitPatch,
        Self::OverflowMajor,
        Self::OverflowMinor,
        Self::OverflowPatch,
        Self::UnexpectedDistribution,
    ];
    fn observation(self, value: u16) -> String {
        match self {
            Self::EmptyMajor => format!("git version .{value}.0"),
            Self::EmptyMinor => format!("git version {value}..0"),
            Self::EmptyPatch => format!("git version 2.{value}."),
            Self::MissingPatch => format!("git version 2.{value}"),
            Self::NondigitMajor => format!("git version x{value}.56.0"),
            Self::NondigitMinor => format!("git version 2.x{value}.0"),
            Self::NondigitPatch => format!("git version 2.56.x{value}"),
            Self::OverflowMajor => format!("git version 18446744073709551616{value}.56.0"),
            Self::OverflowMinor => format!("git version 2.18446744073709551616{value}.0"),
            Self::OverflowPatch => format!("git version 2.56.18446744073709551616{value}"),
            Self::UnexpectedDistribution => format!("git version 2.56.0.vendor.{value}"),
        }
    }
}
/// Closed fixture ingress for foreign filesystem occupants.
#[derive(Clone, Copy)]
pub(in crate::git_factor::engine) enum ForeignPathKind {
    Dangling,
    Directory,
    File,
    Symlink,
}
/// One meaningful foundation input is arranged per generated case.
#[derive(Clone, Copy, Debug)]
pub(in crate::git_factor::engine) enum FoundationInput {
    InitialHardlink,
    InitialIgnored,
    JournalDirectory,
    JournalFile,
    JournalSymlink,
    OpeningHardlink,
    OpeningIgnored,
    RestartHardlink,
    RestartIgnored,
    ScratchDirectory,
    ScratchFile,
    ScratchSymlink,
}

/// Physical foreign scratch input supplied at the final lstat.
enum LateScratch {
    File(String),
    Symlink(PathBuf),
}

/// Exact native filesystem variants represented in the observation oracle.
#[derive(Debug, Eq, PartialEq)]
pub(in crate::git_factor) enum LogPath {
    Directory,
    File(Vec<u8>),
    Symlink(PathBuf),
}

/// Exactly one possible external stimulus at the observation boundary.
enum LogStimulus {
    FinalScratchUnavailable,
    JournalUnavailable,
    Observe,
    ReplaceFinalScratch(LateScratch),
}

/// Closed production-valid diagnostic observer worlds, constructed through actual dispatch.
#[derive(Clone, Copy, Debug)]
pub(in crate::git_factor::engine) enum LogWorld {
    AbsentScratch,
    AbsentSession,
    Admitted,
    CorruptJournal,
    ForeignScratch,
    JournalMetadataError,
    LateScratchFile,
    LateScratchSymlink,
    ScratchMetadataError,
}

// Narrow test-only insertions; preserve other owners' current providers.

/// Closed actual native lifecycle inputs; no setup executes a test world.
#[derive(Clone, Copy, Debug)]
pub(in crate::git_factor::engine) enum NativeScenario {
    AmendedSameSlotThenDroppedNextSlot,
    BeforeNativeExec,
    ExecutableCanonicalizationIo,
    ExecutableCopiedPath,
    ExecutableHardLinkedPath,
    ExecutableObservationIo,
    ExecutableSymlinkAliases,
    #[cfg(target_os = "linux")]
    ExecutableUtf8,
    FailedPickFactorRetry,
    FailedPickNativeRetry,
    FinalAfterPublication,
    FinalBeforePublication,
    FinalDescendantBypass,
    FinalDescendantConsumedAfterAcceptance,
    FinalDescendantConsumedBeforeAcceptance,
    ForeignExec,
    FutureExecDuplicate,
    FuturePickDuplicate,
    IgnoredReplayObstruction,
    IntermediateAfterPublication,
    IntermediateBeforePublication,
    IntermediateConsumedBeforeAcceptance,
    LastDonePickDuplicate,
    ManualBypassRefusal,
    MissingOriginalPick,
    ObservedSparseHook,
    OpeningAdvanced,
    OpeningDeletedRecreation,
    OpeningForeignHead,
    OpeningNonrootAbort,
    OpeningReadyBreak,
    OpeningRootAbort,
    OpeningRootRetry,
    OpeningSourceFinished,
    OpeningWrongPick,
    PendingAmendedPredecessor,
    PendingDescendantBypass,
    PendingRemainderBypass,
    PendingSelectionRewind,
    PrototypeFormatRefusal,
    RecreatedDescendantExec,
    RecreatedFuturePick,
    RemainderBypassWithDescendants,
    RemainderBypassWithoutDescendants,
    RemainderConsumedAfterAcceptance,
    RemainderConsumedBeforeAcceptance,
    RetainedDescendantStillFails,
    TerminalConsumedAfterAcceptance,
    TerminalConsumedBeforeAcceptance,
}

/// Existing durable intents at which foreign work can appear after initial admission.
#[derive(Clone, Copy, Debug)]
enum ReadmissionBoundary {
    InitialPreparing,
    Opening,
    RestartPreparing,
}
#[derive(Clone, Copy, Debug)]
pub(in crate::git_factor::engine) enum VersionFailure {
    None,
    Query,
    SilentQuery,
    Spawn,
    WhitespaceQuery,
}

/// Source-owned pool bytes are admitted; changed bytes are protected user work.
#[derive(Clone, Copy, Debug)]
pub(in crate::git_factor::engine) enum PoolWorld {
    Edited,
    Original,
}

/// Native pool arrangement shared by independently registered public observers.
pub(in crate::git_factor::engine) struct OriginalPool {
    base: String,
    contents: Vec<u8>,
    deletion_tree: String,
    diagnostic: String,
    final_tree: String,
    foreign_refs: String,
    repository: Repository,
}

#[derive(Debug, Eq, PartialEq)]
pub(in crate::git_factor::engine) struct PoolFrame {
    done: Vec<u8>,
    head: String,
    index: Vec<u8>,
    journal: Vec<u8>,
    keep: Vec<u8>,
    refs: String,
    remainder: Vec<u8>,
    todo: Vec<u8>,
}

impl PoolFrame {
    pub(in crate::git_factor::engine) fn remainder(&self) -> &[u8] {
        &self.remainder
    }
}

/// Actual checkpoint selection used by canonical public message observers.
pub(in crate::git_factor) struct MessageSelection {
    base: String,
    command_hash: String,
    final_tree: String,
    repository: Repository,
    target: String,
}

#[derive(Debug, Eq, PartialEq)]
pub(in crate::git_factor) struct MessageFrame {
    atom: Vec<u8>,
    done: Vec<u8>,
    files: BTreeMap<PathBuf, LogPath>,
    head: String,
    index: Vec<u8>,
    journal: Vec<u8>,
    refs: String,
    todo: Vec<u8>,
    user: Vec<u8>,
}

impl MessageFrame {
    pub(in crate::git_factor) fn files_mut(&mut self) -> &mut BTreeMap<PathBuf, LogPath> {
        &mut self.files
    }
}

#[derive(Debug, Default)]
struct Capture {
    stderr: RefCell<String>,
    stdout: RefCell<String>,
}

/// Delegates real observations and applies at most one external stimulus.
struct LogObservationFs {
    admin: PathBuf,
    calls: Cell<usize>,
    fired: Cell<bool>,
    stimulus: LogStimulus,
}
#[derive(Debug)]
struct NativeEnv {
    cwd: PathBuf,
    executable: PathBuf,
}

/// Faults only after real native quit, leaving Verified authority before branch attachment.
struct QuitInterruption {
    fired: Cell<bool>,
}

/// Interrupt only the first native action after the desired durable intent exists.
struct ReadmissionInterruption {
    boundary: ReadmissionBoundary,
    fired: Cell<bool>,
    journal: PathBuf,
}

/// An owned native repository; no process-wide directory or stream changes.
struct Repository {
    directory: TempDir,
    environment: NativeEnv,
    output: Capture,
}

/// Native edit-todo supplies its supported abbreviated dialect before replay resumes.
struct AbbreviatedTodo {
    /// Exact edited native todo, before any callback executes.
    edited: RefCell<String>,
    /// The existing long-dialect fixture remains unchanged when disabled.
    enabled: bool,
    /// The stimulus runs once at the first actual native continuation.
    fired: Cell<bool>,
}

/// Replaces only the private removal child with a real successful native no-op.
struct ProjectionOmission {
    /// The exact native projection request reached the stimulus.
    fired: Cell<bool>,
}

impl Runner for ProjectionOmission {
    fn output(
        &self,
        bin: &str,
        args: &[&str],
        envs: &[(&str, Option<&str>)],
        cwd: &Path,
    ) -> io::Result<Output> {
        if bin == "bash"
            && args.first() == Some(&"-c")
            && args.get(1).is_some_and(|command| {
                command
                    .contains("git -c core.splitIndex=false update-index --force-remove -z --stdin")
            })
        {
            self.fired.set(true);
            return REAL_RUNNER.output("true", &[], envs, cwd);
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
        REAL_RUNNER.status(bin, args, envs, quiet, cwd)
    }
}

impl Runner for AbbreviatedTodo {
    fn output(
        &self,
        bin: &str,
        args: &[&str],
        envs: &[(&str, Option<&str>)],
        cwd: &Path,
    ) -> io::Result<Output> {
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
        if self.enabled && bin == "git" && args == ["rebase", "--continue"] && !self.fired.get() {
            let edited = REAL_RUNNER.output(
                "git",
                &["-c", "sequence.editor=true", "rebase", "--edit-todo"],
                &[("GIT_SEQUENCE_EDITOR", Some("true"))],
                cwd,
            )?;
            if !edited.status.success() {
                return Err(io::Error::other(
                    String::from_utf8_lossy(&edited.stderr).into_owned(),
                ));
            }
            *self.edited.borrow_mut() =
                fs::read_to_string(cwd.join(".git/rebase-merge/git-rebase-todo"))?;
            self.fired.set(true);
        }
        REAL_RUNNER.status(bin, args, envs, quiet, cwd)
    }
}

/// Script only actual compatibility observation; all other native queries retain their runner.
struct VersionObservation<'fixture> {
    calls: Cell<usize>,
    failure: VersionFailure,
    later: Cell<usize>,
    text: &'fixture str,
}

/// Own a trace destination without changing process-wide environment.
struct VersionTraceEnvironment<'fixture> {
    native: &'fixture NativeEnv,
    trace: PathBuf,
}
impl FoundationInput {
    pub(in crate::git_factor::engine) const ALL: &'static [Self] = &[
        Self::JournalFile,
        Self::JournalDirectory,
        Self::JournalSymlink,
        Self::ScratchFile,
        Self::ScratchDirectory,
        Self::ScratchSymlink,
        Self::InitialIgnored,
        Self::InitialHardlink,
        Self::RestartIgnored,
        Self::RestartHardlink,
        Self::OpeningIgnored,
        Self::OpeningHardlink,
    ];
}

impl NativeScenario {
    /// Complete host-applicable scenario domain; constructing it performs no Acts.
    pub(in crate::git_factor::engine) const ALL: &'static [Self] = &[
        Self::IntermediateBeforePublication,
        Self::IntermediateAfterPublication,
        Self::FinalBeforePublication,
        Self::FinalAfterPublication,
        Self::RemainderConsumedBeforeAcceptance,
        Self::RemainderConsumedAfterAcceptance,
        Self::FinalDescendantConsumedBeforeAcceptance,
        Self::FinalDescendantConsumedAfterAcceptance,
        Self::TerminalConsumedBeforeAcceptance,
        Self::TerminalConsumedAfterAcceptance,
        Self::IntermediateConsumedBeforeAcceptance,
        Self::ForeignExec,
        Self::MissingOriginalPick,
        Self::FuturePickDuplicate,
        Self::FutureExecDuplicate,
        Self::BeforeNativeExec,
        Self::AmendedSameSlotThenDroppedNextSlot,
        Self::FailedPickFactorRetry,
        Self::FailedPickNativeRetry,
        Self::OpeningRootRetry,
        Self::OpeningWrongPick,
        Self::OpeningReadyBreak,
        Self::ManualBypassRefusal,
        Self::LastDonePickDuplicate,
        Self::RetainedDescendantStillFails,
        Self::RemainderBypassWithoutDescendants,
        Self::RemainderBypassWithDescendants,
        Self::FinalDescendantBypass,
        Self::OpeningForeignHead,
        Self::OpeningAdvanced,
        Self::OpeningSourceFinished,
        Self::PrototypeFormatRefusal,
        Self::IgnoredReplayObstruction,
        Self::OpeningRootAbort,
        Self::PendingRemainderBypass,
        Self::PendingDescendantBypass,
        Self::PendingAmendedPredecessor,
        Self::RecreatedDescendantExec,
        Self::RecreatedFuturePick,
        Self::ObservedSparseHook,
        Self::PendingSelectionRewind,
        Self::OpeningNonrootAbort,
        Self::OpeningDeletedRecreation,
        Self::ExecutableSymlinkAliases,
        Self::ExecutableCopiedPath,
        Self::ExecutableHardLinkedPath,
        Self::ExecutableObservationIo,
        Self::ExecutableCanonicalizationIo,
        #[cfg(target_os = "linux")]
        Self::ExecutableUtf8,
    ];
}
impl Io for Capture {
    fn err(&self, text: &str) -> io::Result<()> {
        self.stderr.borrow_mut().push_str(text);
        Ok(())
    }
    fn errln(&self, line: &str) -> io::Result<()> {
        self.err(&format!("{line}\n"))
    }
    fn out(&self, text: &str) -> io::Result<()> {
        self.stdout.borrow_mut().push_str(text);
        Ok(())
    }
    fn outln(&self, line: &str) -> io::Result<()> {
        self.out(&format!("{line}\n"))
    }
}
impl Env for NativeEnv {
    fn current_dir(&self) -> io::Result<PathBuf> {
        Ok(self.cwd.clone())
    }
    fn current_exe(&self) -> io::Result<PathBuf> {
        Ok(self.executable.clone())
    }
    fn var_os(&self, key: &str) -> Option<OsString> {
        env::var_os(key)
    }
}
impl Repository {
    fn commit(&self, message: &str) -> String {
        self.git(&["add", "--all"]);
        self.git(&["commit", "--quiet", "--message", message]);
        self.git(&["rev-parse", "HEAD"])
    }
    fn git(&self, arguments: &[&str]) -> String {
        let output = Command::new("git")
            .args(arguments)
            .current_dir(&self.environment.cwd)
            .output()
            .or_abort("native Git process");
        assert!(
            output.status.success(),
            "Git {arguments:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout)
            .or_abort("observed Git metadata UTF-8")
            .trim_end()
            .to_owned()
    }
    fn index(&self) -> Vec<u8> {
        fs::read(self.environment.cwd.join(".git/index")).or_abort("actual index bytes")
    }
    fn invoke(&self, arguments: &[&str]) -> (i32, String) {
        self.invoke_with(arguments, &REAL_RUNNER, &REAL_FS)
    }
    fn invoke_using(
        &self,
        arguments: &[&str],
        runner: &dyn Runner,
        filesystem: &dyn Fs,
        output: &dyn Io,
    ) -> (i32, String) {
        self.output.stdout.borrow_mut().clear();
        self.output.stderr.borrow_mut().clear();
        let context = Ctx {
            cwd: self.environment.cwd.clone(),
            env: &self.environment,
            fs: filesystem,
            io: output,
            runner,
        };
        let native_arguments = iter::once(OsString::from("git-factor"))
            .chain(arguments.iter().map(OsString::from))
            .collect::<Vec<_>>();
        let code = main_entry_with_vec(context.io, Ok(context), &native_arguments);
        (code, self.output.stdout.borrow().clone())
    }
    fn invoke_with(
        &self,
        arguments: &[&str],
        runner: &dyn Runner,
        filesystem: &dyn Fs,
    ) -> (i32, String) {
        self.invoke_using(arguments, runner, filesystem, &self.output)
    }
    fn journal(&self) -> Vec<u8> {
        fs::read(self.environment.cwd.join(".git/factor-journal.json")).or_abort("durable journal")
    }
    fn new() -> Self {
        let directory = TempDir::new().or_abort("owned fixture directory");
        let cwd = directory.path().join("repository");
        fs::create_dir_all(&cwd).or_abort("native repository directory");
        let executable = directory.path().join("factor-child");
        let source_exe = env::current_exe().or_abort("instrumented test executable");
        let test_exe = directory.path().join("instrumented-child");
        fs::copy(source_exe, &test_exe).or_abort("immutable instrumented child image");
        let script = format!(
            "#!/bin/bash\nset -eu\narguments=$(mktemp)\ntrap 'rm -f \"$arguments\"' EXIT\nprintf '%s\\0' \"$@\" >\"$arguments\"\nFACTOR_TEST_ARGUMENTS=\"$arguments\" FACTOR_TEST_EXECUTABLE={} {} --exact git_factor::engine::tests::process_child::invoke --ignored --nocapture >&2\n",
            shell_quote(executable.to_str().or_abort("fixture path UTF-8")),
            shell_quote(test_exe.to_str().or_abort("test executable path UTF-8")),
        );
        fs::write(&executable, script).or_abort("child launcher");
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700))
            .or_abort("executable child launcher");
        let repository = Self {
            directory,
            environment: NativeEnv { cwd, executable },
            output: Capture::default(),
        };
        repository.git(&["init", "--quiet", "--initial-branch=main"]);
        repository.git(&["config", "user.name", "Checkpoint contract"]);
        repository.git(&["config", "user.email", "checkpoint@example.invalid"]);
        repository
    }
    fn success(&self, arguments: &[&str]) -> serde_json::Value {
        let (code, output) = self.invoke(arguments);
        assert_eq!(
            code,
            EXIT_OK,
            "{arguments:?}: {}",
            self.output.stderr.borrow()
        );
        serde_json::from_str(&output).or_abort("one normalized JSON response")
    }
    fn write(&self, path: &str, contents: &str) {
        fs::write(self.environment.cwd.join(path), contents).or_abort("fixture file");
    }
}
impl Env for VersionTraceEnvironment<'_> {
    fn current_dir(&self) -> io::Result<PathBuf> {
        self.native.current_dir()
    }
    fn current_exe(&self) -> io::Result<PathBuf> {
        self.native.current_exe()
    }
    fn var_os(&self, key: &str) -> Option<OsString> {
        if key == "GIT_FACTOR_TRACE_LOG" {
            Some(self.trace.as_os_str().to_owned())
        } else {
            self.native.var_os(key)
        }
    }
}
impl Runner for VersionObservation<'_> {
    #[expect(
        clippy::panic_in_result_fn,
        reason = "asserting the first native observation is the actual test adapter contract, not a recoverable product error"
    )]
    fn output(
        &self,
        bin: &str,
        args: &[&str],
        envs: &[(&str, Option<&str>)],
        cwd: &Path,
    ) -> io::Result<Output> {
        if bin == "git" && args == ["version"] {
            assert_eq!(
                self.later.get(),
                0,
                "version must be the first native observation"
            );
            self.calls.set(
                self.calls
                    .get()
                    .checked_add(1)
                    .or_abort("bounded version observation count"),
            );
            if matches!(self.failure, VersionFailure::Spawn) {
                return Err(io::Error::other("owned Git version spawn failure"));
            }
            return Ok(Output {
                status: ExitStatus::from_raw(
                    if matches!(
                        self.failure,
                        VersionFailure::Query
                            | VersionFailure::SilentQuery
                            | VersionFailure::WhitespaceQuery
                    ) {
                        256
                    } else {
                        0
                    },
                ),
                stdout: self.text.as_bytes().to_vec(),
                stderr: match self.failure {
                    VersionFailure::Query => b"owned Git version query failure".to_vec(),
                    VersionFailure::WhitespaceQuery => b" \n\t ".to_vec(),
                    VersionFailure::None | VersionFailure::SilentQuery | VersionFailure::Spawn => {
                        Vec::new()
                    }
                },
            });
        }
        self.later.set(
            self.later
                .get()
                .checked_add(1)
                .or_abort("bounded later native observation count"),
        );
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
        self.later.set(
            self.later
                .get()
                .checked_add(1)
                .or_abort("bounded later native observation count"),
        );
        REAL_RUNNER.status(bin, args, envs, quiet, cwd)
    }
}
impl ReadmissionInterruption {
    fn preparing(&self) -> bool {
        fs::read(&self.journal)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
            .and_then(|value| {
                value
                    .pointer("/state/phase")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned)
            })
            .is_some_and(|phase| phase == "preparing")
    }
}
impl Runner for ReadmissionInterruption {
    fn output(
        &self,
        bin: &str,
        args: &[&str],
        envs: &[(&str, Option<&str>)],
        cwd: &Path,
    ) -> io::Result<Output> {
        if bin == "git"
            && args.first() == Some(&"commit-tree")
            && self.preparing()
            && !matches!(self.boundary, ReadmissionBoundary::Opening)
            && !self.fired.replace(true)
        {
            return Err(io::Error::other("interrupted admitted preparing intent"));
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
        if bin == "git"
            && args.contains(&"rebase")
            && args.contains(&"--interactive")
            && matches!(self.boundary, ReadmissionBoundary::Opening)
            && !self.fired.replace(true)
        {
            return Err(io::Error::other("interrupted admitted opening intent"));
        }
        REAL_RUNNER.status(bin, args, envs, quiet, cwd)
    }
}
impl Runner for QuitInterruption {
    fn output(
        &self,
        bin: &str,
        args: &[&str],
        envs: &[(&str, Option<&str>)],
        cwd: &Path,
    ) -> io::Result<Output> {
        REAL_RUNNER.output(bin, args, envs, cwd)
    }
    #[expect(
        clippy::panic_in_result_fn,
        reason = "the owned test adapter refuses invalid fixture authority rather than converting assertion failure into product recovery"
    )]
    fn status(
        &self,
        bin: &str,
        args: &[&str],
        envs: &[(&str, Option<&str>)],
        quiet: bool,
        cwd: &Path,
    ) -> io::Result<ExitStatus> {
        if bin == "git" && args == ["rebase", "--quit"] && !self.fired.replace(true) {
            let status = REAL_RUNNER.status(bin, args, envs, quiet, cwd)?;
            assert!(status.success(), "actual terminal quit must complete");
            return Err(io::Error::other(
                "interrupted after real native terminal quit",
            ));
        }
        REAL_RUNNER.status(bin, args, envs, quiet, cwd)
    }
}
impl LogWorld {
    pub(in crate::git_factor::engine) const ALL: [Self; 9] = [
        Self::AbsentScratch,
        Self::AbsentSession,
        Self::Admitted,
        Self::CorruptJournal,
        Self::ForeignScratch,
        Self::JournalMetadataError,
        Self::LateScratchFile,
        Self::LateScratchSymlink,
        Self::ScratchMetadataError,
    ];
}
impl Fs for LogObservationFs {
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
    #[expect(
        clippy::match_ref_pats,
        reason = "explicit borrowed fixture variants preserve owned payloads under contradictory pattern restriction lints"
    )]
    #[expect(
        clippy::needless_borrowed_reference,
        reason = "explicit borrowed fixture variants preserve owned payloads under contradictory pattern restriction lints"
    )]
    #[expect(
        clippy::ref_patterns,
        reason = "explicit borrowed fixture variants preserve owned payloads without cloning under contradictory pattern restriction lints"
    )]
    fn symlink_metadata(&self, path: &Path) -> io::Result<fs::Metadata> {
        let watched = match &self.stimulus {
            &LogStimulus::JournalUnavailable => self.admin.join("factor-journal.json"),
            &LogStimulus::Observe
            | &LogStimulus::FinalScratchUnavailable
            | &LogStimulus::ReplaceFinalScratch(_) => self.admin.join("factor"),
        };
        if path == watched {
            let calls = self
                .calls
                .get()
                .checked_add(1)
                .ok_or_else(|| io::Error::other("metadata observation count overflow"))?;
            self.calls.set(calls);
            match &self.stimulus {
                &LogStimulus::JournalUnavailable if calls == 1 => {
                    self.fired.set(true);
                    return Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        "diagnostic metadata unavailable",
                    ));
                }
                &LogStimulus::FinalScratchUnavailable if calls == 3 => {
                    self.fired.set(true);
                    return Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        "diagnostic metadata unavailable",
                    ));
                }
                &LogStimulus::ReplaceFinalScratch(ref replacement) if calls == 3 => {
                    REAL_FS.remove_dir_all(path)?;
                    match replacement {
                        &LateScratch::File(ref body) => fs::write(path, body)?,
                        &LateScratch::Symlink(ref target) => symlink(target, path)?,
                    }
                    self.fired.set(true);
                }
                &LogStimulus::Observe
                | &LogStimulus::JournalUnavailable
                | &LogStimulus::FinalScratchUnavailable
                | &LogStimulus::ReplaceFinalScratch(_) => {}
            }
        }
        REAL_FS.symlink_metadata(path)
    }
    fn write_atomic_string(&self, path: &Path, content: &str) -> io::Result<()> {
        REAL_FS.write_atomic_string(path, content)
    }
    fn write_string(&self, path: &Path, content: &str) -> io::Result<()> {
        REAL_FS.write_string(path, content)
    }
}

impl OriginalPool {
    pub(in crate::git_factor::engine) fn arrange(world: PoolWorld, contents: &str) -> Self {
        let repository = Repository::new();
        repository.write("keep", "unchanged anchor\n");
        repository.write("victim", "original file\n");
        let base = repository.commit("Add original file");
        repository.git(&["branch", "foreign", &base]);
        repository.git(&["tag", "foreign", &base]);
        let foreign_refs = repository.git(&[
            "show-ref",
            "--verify",
            "refs/heads/foreign",
            "refs/tags/foreign",
        ]);
        let root = &repository.environment.cwd;
        fs::remove_file(root.join("victim")).or_abort("selected deletion");
        fs::create_dir_all(root.join("victim")).or_abort("selected directory");
        repository.write("victim/x", contents);
        repository.commit("Replace file with directory");
        let final_tree = repository.git(&["rev-parse", "HEAD^{tree}"]);
        repository.success(&["--exec", "true", "HEAD"]);
        repository.git(&["add", "--update", "--", "victim"]);
        let deletion_tree = repository.git(&["write-tree"]);
        assert_eq!(
            repository.git(&["ls-tree", "--name-only", &deletion_tree]),
            "keep"
        );
        let physical = match world {
            PoolWorld::Original => contents.to_owned(),
            PoolWorld::Edited => format!("{contents}\nchanged user bytes\n"),
        };
        repository.write("victim/x", &physical);
        // Independently build the expected physical tree without changing the main index.
        let observer_index = repository.directory.path().join("expected-index");
        for arguments in [vec!["read-tree", "refs/heads/main"], vec!["add", "--all"]] {
            let output = Command::new("git")
                .args(arguments)
                .env("GIT_INDEX_FILE", &observer_index)
                .current_dir(root)
                .output()
                .or_abort("independent expected tree");
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        let output = Command::new("git")
            .args(["write-tree"])
            .env("GIT_INDEX_FILE", &observer_index)
            .current_dir(root)
            .output()
            .or_abort("independent physical tree");
        assert!(output.status.success());
        let actual_tree = String::from_utf8(output.stdout).or_abort("expected tree UTF-8");
        let diagnostic = format!(
            "tree hash mismatch: expected {final_tree}, got {}\n",
            actual_tree.trim_end()
        );
        repository.output.stdout.borrow_mut().clear();
        repository.output.stderr.borrow_mut().clear();
        Self {
            repository,
            base,
            deletion_tree,
            final_tree,
            foreign_refs,
            contents: physical.into_bytes(),
            diagnostic,
        }
    }
    pub(in crate::git_factor::engine) fn base(&self) -> &str {
        &self.base
    }
    pub(in crate::git_factor::engine) fn contents(&self) -> &[u8] {
        &self.contents
    }
    pub(in crate::git_factor::engine) fn context(&self) -> Ctx<'_> {
        Ctx {
            cwd: self.repository.environment.cwd.clone(),
            env: &self.repository.environment,
            fs: &REAL_FS,
            io: &self.repository.output,
            runner: &REAL_RUNNER,
        }
    }
    pub(in crate::git_factor::engine) fn deletion_tree(&self) -> &str {
        &self.deletion_tree
    }
    pub(in crate::git_factor::engine) fn diagnostic(&self) -> &str {
        &self.diagnostic
    }
    pub(in crate::git_factor::engine) fn expected_output() -> serde_json::Value {
        let split_count: u32 = 1;
        let mut guidance = vec![
            "Stage one independently valid atomic change.",
            "Use one concrete action in the commit message.",
            "Submit each atom through git factor so its gates run.",
        ];
        if env::var_os("CLAUDECODE").is_some() {
            guidance.extend([
                "Above 50% context, pause and ask the user to /compact.",
                "Continue splitting until the session is complete.",
            ]);
        }
        serde_json::Value::Object(serde_json::Map::from_iter([
            (
                "operation".to_owned(),
                serde_json::Value::String("continue".to_owned()),
            ),
            (
                "result".to_owned(),
                serde_json::Value::String("committed".to_owned()),
            ),
            (
                "split_count".to_owned(),
                serde_json::Value::Number(split_count.into()),
            ),
            (
                "actions".to_owned(),
                serde_json::Value::Object(serde_json::Map::from_iter([
                    (
                        "abort".to_owned(),
                        serde_json::Value::Array(vec![
                            serde_json::Value::String("git".to_owned()),
                            serde_json::Value::String("factor".to_owned()),
                            serde_json::Value::String("--abort".to_owned()),
                        ]),
                    ),
                    (
                        "submit".to_owned(),
                        serde_json::Value::Array(vec![
                            serde_json::Value::String("git".to_owned()),
                            serde_json::Value::String("factor".to_owned()),
                            serde_json::Value::String("--continue".to_owned()),
                            serde_json::Value::String("--message".to_owned()),
                            serde_json::Value::String("<message>".to_owned()),
                        ]),
                    ),
                ])),
            ),
            (
                "changes".to_owned(),
                serde_json::Value::Object(serde_json::Map::from_iter([
                    ("unstaged".to_owned(), serde_json::Value::Array(Vec::new())),
                    (
                        "untracked".to_owned(),
                        serde_json::Value::Array(vec![serde_json::Value::String(
                            "victim/x".to_owned(),
                        )]),
                    ),
                ])),
            ),
            (
                "guidance".to_owned(),
                serde_json::Value::Array(
                    guidance
                        .into_iter()
                        .map(|line| serde_json::Value::String(line.to_owned()))
                        .collect(),
                ),
            ),
            (
                "references".to_owned(),
                serde_json::Value::Array(Vec::new()),
            ),
        ]))
    }
    pub(in crate::git_factor::engine) fn final_tree(&self) -> &str {
        &self.final_tree
    }
    pub(in crate::git_factor::engine) fn foreign_refs(&self) -> &str {
        &self.foreign_refs
    }

    pub(in crate::git_factor::engine) fn frame(&self) -> PoolFrame {
        let root = &self.repository.environment.cwd;
        // Read the raw index before any native metadata observer can refresh it.
        let index = self.repository.index();
        PoolFrame {
            index,
            journal: self.repository.journal(),
            head: self.repository.git(&["rev-parse", "HEAD"]),
            refs: self.repository.git(&["show-ref"]),
            done: fs::read(root.join(".git/rebase-merge/done")).or_abort("native done"),
            todo: fs::read(root.join(".git/rebase-merge/git-rebase-todo")).or_abort("native todo"),
            keep: fs::read(root.join("keep")).or_abort("anchor bytes"),
            remainder: fs::read(root.join("victim/x")).or_abort("remainder bytes"),
        }
    }
    pub(in crate::git_factor::engine) fn git(&self, args: &[&str]) -> String {
        self.repository.git(args)
    }
    pub(in crate::git_factor::engine) fn journal(&self) -> super::StoredJournal {
        serde_json::from_slice(&self.repository.journal()).or_abort("checkpoint journal")
    }
    pub(in crate::git_factor::engine) fn stderr(&self) -> String {
        self.repository.output.stderr.borrow().clone()
    }
    pub(in crate::git_factor::engine) fn stdout(&self) -> String {
        self.repository.output.stdout.borrow().clone()
    }
}

impl NativeStagedSelection {
    fn arrange(world: StagedPath) -> Self {
        let repository = Repository::new();
        repository.write("atom", "original atom\n");
        match world {
            StagedPath::TrackedContent => repository.write("path", "original path\n"),
            StagedPath::TrackedEmpty => repository.write("path", ""),
            StagedPath::New | StagedPath::StagedEmpty => {}
        }
        repository.commit("Add original tree");
        repository.write("atom", "selected atom\n");
        repository.write("remainder", "selected remainder\n");
        repository.write(
            "path",
            match world {
                StagedPath::StagedEmpty => "",
                StagedPath::New | StagedPath::TrackedContent | StagedPath::TrackedEmpty => {
                    "selected path\n"
                }
            },
        );
        let original = repository.commit("Change selected tree");
        let final_tree = repository.git(&["rev-parse", "HEAD^{tree}"]);
        repository.git(&["branch", "protected"]);
        repository.git(&["tag", "protected-tag"]);
        repository.success(&["--exec", "true", "HEAD"]);
        match world {
            StagedPath::TrackedContent | StagedPath::TrackedEmpty => {
                repository.git(&["rm", "--cached", "--", "path"]);
                repository.git(&["add", "--intent-to-add", "--", "path"]);
            }
            StagedPath::New => {
                repository.git(&["add", "--intent-to-add", "--", "path"]);
            }
            StagedPath::StagedEmpty => {
                repository.git(&["add", "--", "path"]);
            }
        }
        repository.git(&["add", "--", "atom"]);
        // Native write-tree is the independently specified commit-tree oracle.
        let expected_atom_tree = repository.git(&["write-tree"]);
        repository.write("user", "protected user bytes\n");
        Self {
            expected_atom_tree,
            final_tree,
            original,
            repository,
        }
    }
}

impl MessageSelection {
    pub(in crate::git_factor) fn arrange(staged: bool) -> Self {
        let repository = Repository::new();
        repository.write("atom", "original tree\n");
        let base = repository.commit("Add original tree");
        repository.write("atom", "selected tree\n");
        let target = repository.commit("Change selected tree");
        let final_tree = repository.git(&["rev-parse", "HEAD^{tree}"]);
        repository.success(&["--gate", "test", "true", "HEAD"]);
        if staged {
            repository.git(&["add", "--", "atom"]);
        }
        repository.write("user", "user bytes\0\n");
        let command_file = repository.directory.path().join("command");
        fs::write(&command_file, "true").or_abort("independent command bytes");
        let command_hash = repository.git(&[
            "hash-object",
            "--no-filters",
            command_file.to_str().or_abort("command path"),
        ]);
        repository.output.stdout.borrow_mut().clear();
        repository.output.stderr.borrow_mut().clear();
        Self {
            base,
            command_hash,
            final_tree,
            repository,
            target,
        }
    }
    pub(in crate::git_factor) fn base(&self) -> &str {
        &self.base
    }
    pub(in crate::git_factor) fn command_hash(&self) -> &str {
        &self.command_hash
    }
    pub(in crate::git_factor) fn context(&self) -> Ctx<'_> {
        Ctx {
            cwd: self.repository.environment.cwd.clone(),
            env: &self.repository.environment,
            fs: &REAL_FS,
            io: &self.repository.output,
            runner: &REAL_RUNNER,
        }
    }
    pub(in crate::git_factor) fn error_log_exists(&self) -> bool {
        self.repository
            .environment
            .cwd
            .join(".git/factor/error.log")
            .exists()
    }
    pub(in crate::git_factor) fn final_tree(&self) -> &str {
        &self.final_tree
    }
    pub(in crate::git_factor) fn frame(&self) -> MessageFrame {
        let root = &self.repository.environment.cwd;
        let index = self.repository.index();
        MessageFrame {
            files: log_observation_inventory(root),
            index,
            journal: self.repository.journal(),
            head: self.repository.git(&["rev-parse", "HEAD"]),
            refs: self.repository.git(&["show-ref"]),
            done: fs::read(root.join(".git/rebase-merge/done")).or_abort("native done"),
            todo: fs::read(root.join(".git/rebase-merge/git-rebase-todo")).or_abort("native todo"),
            atom: fs::read(root.join("atom")).or_abort("selected bytes"),
            user: self.user_bytes(),
        }
    }
    pub(in crate::git_factor) fn git(&self, arguments: &[&str]) -> String {
        self.repository.git(arguments)
    }
    pub(in crate::git_factor) fn session_active(&self) -> bool {
        self.repository
            .environment
            .cwd
            .join(".git/factor-journal.json")
            .exists()
    }
    pub(in crate::git_factor) fn stderr(&self) -> String {
        self.repository.output.stderr.borrow().clone()
    }
    pub(in crate::git_factor) fn stdout(&self) -> String {
        self.repository.output.stdout.borrow().clone()
    }
    pub(in crate::git_factor) fn target(&self) -> &str {
        &self.target
    }

    pub(in crate::git_factor) fn user_bytes(&self) -> Vec<u8> {
        fs::read(self.repository.environment.cwd.join("user")).or_abort("protected user bytes")
    }
}

pub(in crate::git_factor::engine) fn recorded_journal_facts(
    checkpoint: &str,
    tree: &str,
    phase: &'static str,
    source: Option<&str>,
) -> super::JournalFacts {
    super::JournalFacts {
        checkpoint: super::CommitSha::new(checkpoint.to_owned())
            .or_abort("valid recorded checkpoint"),
        final_tree: super::TreeHash::new(tree).or_abort("valid recorded tree"),
        phase,
        source: source
            .map(|value| super::CommitSha::new(value.to_owned()).or_abort("valid recorded source")),
    }
}

/// Qualifies each distinct journal capture and terminal cleanup uncertainty once.
#[expect(
    clippy::single_call_fn,
    reason = "named native fixture boundaries keep ownership, stimulus, and conservation visible to the direct canonical provider"
)]
pub(in crate::git_factor::engine) fn durability_publication_census() {
    for empty in [false, true] {
        for after_replace in [false, true] {
            faults::capture_publication(empty, after_replace);
        }
    }
    for after in [false, true] {
        for abort in [false, true] {
            faults::closing_interruption(faults::ClosingBoundary::Lease, after, abort);
        }
    }
    for after in [false, true] {
        for abort in [false, true] {
            faults::closing_interruption(faults::ClosingBoundary::Directory, after, abort);
        }
    }
    for abort in [false, true] {
        faults::closing_interruption(faults::ClosingBoundary::DirectoryPartial, false, abort);
        faults::closing_interruption(faults::ClosingBoundary::Journal, false, abort);
    }
    faults::closing_interruption(faults::ClosingBoundary::Journal, true, false);
}

/// Both canonical locations admit no foreign directory, file, or symbolic link.
#[expect(
    clippy::single_call_fn,
    reason = "named native fixture boundaries keep ownership, stimulus, and conservation visible to the direct canonical provider"
)]
pub(in crate::git_factor::engine) fn foreign_session_path_census() {
    for journal in [false, true] {
        for kind in [
            ForeignPathKind::File,
            ForeignPathKind::Directory,
            ForeignPathKind::Symlink,
            ForeignPathKind::Dangling,
        ] {
            verify_foreign_session_path(journal, kind, "unchanged foreign bytes\n");
        }
    }
}

/// Native Git reaches branch attachment itself; no journal/HEAD attachment is fabricated.
#[expect(
    clippy::too_many_lines,
    reason = "one decisive native lifecycle keeps its exact interruption evidence and full preservation oracle together"
)]
fn interrupted_attached_terminal(abbreviated: bool) -> Repository {
    let repository = Repository::new();
    repository.write("base", "base\n");
    repository.commit("Base");
    repository.write("atom", "atom\n");
    repository.write("remainder", "remainder\n");
    let selected = repository.commit("Selected source");
    repository.write("descendant", "distinct descendant bytes\n");
    repository.commit("Add real replay descendant");
    if abbreviated {
        repository.git(&["config", "rebase.abbreviateCommands", "true"]);
        repository.git(&["branch", "protected"]);
        repository.git(&["tag", "protected-tag"]);
    }
    repository.success(&["--exec", "true", &selected]);
    repository.git(&["add", "atom"]);
    let hook = repository.environment.cwd.join(".git/hooks/post-rewrite");
    let evidence = repository.environment.cwd.join(".git");
    let quote = |name: &str| {
        shell_quote(
            evidence
                .join(name)
                .to_str()
                .or_abort("owned evidence path UTF-8"),
        )
    };
    let script = format!(
        "#!/bin/bash\nprintf '%s %s %s\\n' \"$1\" \"$$\" \"$PPID\" >> {calls}\nif test \"$1\" = rebase; then\nps -p \"$PPID\" -o comm= > {parent}\ngit rev-parse HEAD 'HEAD^{{tree}}' refs/heads/main > {snapshot}\ngit symbolic-ref HEAD >> {snapshot}\nif test -d {native}; then printf 'rebase-merge\\n' >> {snapshot}; fi\ncat {original} {branch} >> {snapshot}\nkill -KILL \"$PPID\"\nprintf '%s\\n' \"$?\" > {kill_status}\nfi\n",
        calls = quote("terminal-hook-calls"),
        parent = quote("terminal-hook-parent"),
        snapshot = quote("terminal-hook-snapshot"),
        native = quote("rebase-merge"),
        original = quote("rebase-merge/orig-head"),
        branch = quote("rebase-merge/head-name"),
        kill_status = quote("terminal-hook-kill-status"),
    );
    fs::write(&hook, script).or_abort("native rebase completion interruption hook");
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o700))
        .or_abort("native hook executable");
    let abbreviation = AbbreviatedTodo {
        enabled: abbreviated,
        edited: RefCell::new(String::new()),
        fired: Cell::new(false),
    };
    let (code, _) = repository.invoke_with(
        &["--message", "Capture before native cleanup"],
        &abbreviation,
        &REAL_FS,
    );
    if abbreviated {
        assert!(abbreviation.fired.get(), "actual native edit-todo stimulus");
        assert!(
            abbreviation
                .edited
                .borrow()
                .lines()
                .any(|line| line.starts_with("x ") && line.ends_with(" checkpoint-terminal"))
        );
    }
    assert_ne!(
        code,
        EXIT_OK,
        "native hook must interrupt: calls={:?}, parent={:?}, snapshot={:?}",
        fs::read_to_string(evidence.join("terminal-hook-calls")),
        fs::read_to_string(evidence.join("terminal-hook-parent")),
        fs::read_to_string(evidence.join("terminal-hook-snapshot"))
    );
    // Git's killed parent can return before the orphan hook finishes its final evidence write.
    let attempts: Range<usize> = 0..100;
    for _attempt in attempts {
        if evidence.join("terminal-hook-kill-status").is_file() {
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    let calls = fs::read_to_string(evidence.join("terminal-hook-calls"))
        .or_abort("actual hook call evidence");
    let rebase_calls = calls
        .lines()
        .filter(|line| line.starts_with("rebase "))
        .collect::<Vec<_>>();
    assert_eq!(
        rebase_calls.len(),
        1,
        "one actual native rebase hook must cause interruption: {calls}"
    );
    let fields = rebase_calls
        .first()
        .or_abort("native rebase call")
        .split_whitespace()
        .collect::<Vec<_>>();
    assert_eq!(fields.len(), 3);
    let hook_pid = fields
        .get(1)
        .or_abort("hook PID")
        .parse::<NonZeroU32>()
        .or_abort("positive hook PID");
    let parent_pid = fields
        .get(2)
        .or_abort("native parent PID")
        .parse::<NonZeroU32>()
        .or_abort("positive native parent PID");
    assert_ne!(hook_pid, parent_pid);
    let parent = fs::read_to_string(evidence.join("terminal-hook-parent"))
        .or_abort("native parent command evidence");
    assert_eq!(
        Path::new(parent.trim()).file_name(),
        Some(OsStr::new("git"))
    );
    assert_eq!(
        fs::read_to_string(evidence.join("terminal-hook-kill-status"))
            .or_abort("actual kill status")
            .trim(),
        "0"
    );
    let journal: serde_json::Value =
        serde_json::from_slice(&repository.journal()).or_abort("actual terminal journal");
    assert_eq!(
        journal
            .pointer("/state/phase")
            .and_then(serde_json::Value::as_str),
        Some("verified")
    );
    let tip = journal
        .pointer("/state/tip")
        .and_then(serde_json::Value::as_str)
        .or_abort("actual verified tip");
    let final_tree = journal
        .pointer("/final_tree")
        .and_then(serde_json::Value::as_str)
        .or_abort("fixed final tree");
    let checkpoint = journal
        .pointer("/checkpoint")
        .and_then(serde_json::Value::as_str)
        .or_abort("previous checkpoint");
    assert_eq!(
        fs::read_to_string(evidence.join("terminal-hook-snapshot"))
            .or_abort("actual pre-kill native snapshot"),
        format!(
            "{tip}\n{final_tree}\n{tip}\nrefs/heads/main\nrebase-merge\n{checkpoint}\nrefs/heads/main\n"
        )
    );
    assert_eq!(repository.git(&["rev-parse", "HEAD"]), tip);
    assert_eq!(repository.git(&["rev-parse", "refs/heads/main"]), tip);
    assert_eq!(repository.git(&["symbolic-ref", "HEAD"]), "refs/heads/main");
    assert!(
        repository
            .environment
            .cwd
            .join(".git/rebase-merge")
            .is_dir()
    );
    fs::remove_file(hook).or_abort("remove only owned interruption hook");
    repository.write("unrelated", "preserved unrelated bytes\n");
    repository
}

/// Native reference transaction creates the real branch-updated, detached-HEAD interval.
#[expect(
    clippy::single_call_fn,
    reason = "named native fixture boundaries keep ownership, stimulus, and conservation visible to the direct canonical provider"
)]
fn interrupted_detached_terminal() -> Repository {
    let repository = Repository::new();
    repository.write("base", "base\n");
    repository.commit("Base");
    repository.write("atom", "atom\n");
    let selected = repository.commit("Selected source");
    repository.write("descendant", "real descendant\n");
    repository.commit("Replay descendant");
    repository.success(&["--exec", "true", &selected]);
    let admin = repository.environment.cwd.join(".git");
    let native_admin = fs::canonicalize(&admin).or_abort("canonical native admin");
    let admin_text = native_admin.to_str().or_abort("native admin UTF-8");
    let hook = admin.join("hooks/reference-transaction");
    let script = format!(
        "#!/bin/bash\nset -eu\nif test \"$1\" != prepared; then exit 0; fi\nif test \"$(git rev-parse --absolute-git-dir)\" != {admin}; then exit 0; fi\nwhile read -r old new name; do\nif test \"$name\" = refs/heads/main && ! git symbolic-ref -q HEAD >/dev/null; then\nprintf '%s\\n' \"$old $new $name\" >{admin}/detached-terminal-transaction\nprintf 'owned fixture lock\\n' >{admin}/HEAD.lock\nfi\ndone\n",
        admin = shell_quote(admin_text),
    );
    fs::write(&hook, script).or_abort("owned native reference hook");
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o700)).or_abort("reference hook mode");
    repository.git(&["add", "atom"]);
    let (code, _) = repository.invoke(&["--message", "Extract complete atom"]);
    assert_ne!(
        code, EXIT_OK,
        "native HEAD attachment must fail on owned lock"
    );
    let journal: serde_json::Value =
        serde_json::from_slice(&repository.journal()).or_abort("Verified journal");
    assert_eq!(
        journal
            .pointer("/state/phase")
            .and_then(serde_json::Value::as_str),
        Some("verified")
    );
    let tip = journal
        .pointer("/state/tip")
        .and_then(serde_json::Value::as_str)
        .or_abort("verified tip");
    let checkpoint = journal
        .get("checkpoint")
        .and_then(serde_json::Value::as_str)
        .or_abort("old checkpoint");
    assert_eq!(repository.git(&["rev-parse", "HEAD"]), tip);
    assert_eq!(repository.git(&["rev-parse", "refs/heads/main"]), tip);
    assert_eq!(
        repository.git(&["rev-parse", "--symbolic-full-name", "HEAD"]),
        "HEAD"
    );
    assert_eq!(
        fs::read_to_string(admin.join("detached-terminal-transaction"))
            .or_abort("real transaction marker"),
        format!("{checkpoint} {tip} refs/heads/main\n")
    );
    assert!(admin.join("rebase-merge").is_dir());
    fs::remove_file(hook).or_abort("remove only owned reference hook");
    repository.write("unrelated", "preserved terminal user bytes\n");
    repository
}

/// Initial authority publication never produces a legacy-shaped directory.
#[expect(
    clippy::single_call_fn,
    reason = "named native fixture boundaries keep ownership, stimulus, and conservation visible to the direct canonical provider"
)]
pub(in crate::git_factor::engine) fn journal_lifecycle_census() {
    faults::closing_preserves_reacquired_lease(false);
    faults::closing_preserves_reacquired_lease(true);
    unknown_closing_lease_census();
    for boundary in [
        faults::BirthBoundary::BeforeRename,
        faults::BirthBoundary::AfterRename,
        faults::BirthBoundary::ScratchCreation,
    ] {
        faults::birth_interruption(boundary);
    }
    for boundary in [
        faults::LegacyBoundary::DanglingLink,
        faults::LegacyBoundary::Directory,
        faults::LegacyBoundary::File,
    ] {
        faults::legacy_start_refusal(boundary);
    }
}

/// Exact path/type/content inventory includes native index, refs, journal and user files.
fn log_observation_inventory(root: &Path) -> BTreeMap<PathBuf, LogPath> {
    fn collect(path: &Path, root: &Path, bytes: &mut BTreeMap<PathBuf, LogPath>) {
        for item in fs::read_dir(path).or_abort("diagnostic inventory directory") {
            let entry = item.or_abort("diagnostic inventory entry");
            let child = entry.path();
            let metadata = fs::symlink_metadata(&child).or_abort("diagnostic inventory metadata");
            let relative = child
                .strip_prefix(root)
                .or_abort("owned diagnostic inventory")
                .to_path_buf();
            let contents = if metadata.file_type().is_symlink() {
                LogPath::Symlink(fs::read_link(&child).or_abort("diagnostic inventory link"))
            } else if metadata.is_dir() {
                LogPath::Directory
            } else {
                LogPath::File(fs::read(&child).or_abort("diagnostic inventory bytes"))
            };
            let previous = bytes.insert(relative, contents);
            assert!(previous.is_none());
            if metadata.is_dir() {
                collect(&child, root, bytes);
            }
        }
    }
    let mut bytes = BTreeMap::new();
    collect(root, root, &mut bytes);
    bytes
}

/// Arranges actual divergent branch tips without invoking the factor dispatcher.
#[expect(
    clippy::single_call_fn,
    reason = "named native fixture boundaries keep ownership, stimulus, and conservation visible to the direct canonical provider"
)]
fn nonancestor_repository(selected_content: &str, user_content: &str) -> (Repository, String) {
    let repository = Repository::new();
    repository.write("base", "base\n");
    repository.commit("Base");
    repository.git(&["checkout", "--quiet", "-b", "unrelated"]);
    repository.write("selected", selected_content);
    let selected = repository.commit("Unrelated selected change");
    repository.git(&["checkout", "--quiet", "main"]);
    repository.write("current", "current branch bytes\n");
    repository.commit("Current branch change");
    repository.write("user", user_content);
    (repository, selected)
}

/// Status admits the completed native interval but performs no cleanup or capture.
#[expect(
    clippy::single_call_fn,
    reason = "named native fixture boundaries keep ownership, stimulus, and conservation visible to the direct canonical provider"
)]
fn observe_attached_terminal(repository: &Repository) {
    let journal = repository.journal();
    let index = repository.index();
    let refs = repository.git(&["show-ref"]);
    let head = repository.git(&["rev-parse", "HEAD"]);
    let observed = repository.success(&["--status"]);
    assert_eq!(
        observed
            .pointer("/session/phase")
            .and_then(serde_json::Value::as_str),
        Some("verified")
    );
    assert!(
        repository
            .environment
            .cwd
            .join(".git/rebase-merge")
            .is_dir()
    );
    assert_eq!(repository.journal(), journal);
    assert_eq!(repository.index(), index);
    assert_eq!(repository.git(&["show-ref"]), refs);
    assert_eq!(repository.git(&["rev-parse", "HEAD"]), head);
    assert_eq!(
        fs::read(repository.environment.cwd.join("unrelated")).or_abort("preserved status bytes"),
        b"preserved unrelated bytes\n"
    );
}

fn open_selection() -> Repository {
    let repository = Repository::new();
    repository.write("base", "base\n");
    repository.commit("Base");
    repository.write("atom", "atom\n");
    repository.write("remainder", "remainder\n");
    repository.commit("Selected source");
    repository.success(&["--exec", "true", "HEAD"]);
    repository
}

/// Returns the full preserved frame after independently validating new loose trees.
fn observe_unreachable_tree_additions(
    repository: &Repository,
    files_before: &BTreeMap<PathBuf, LogPath>,
    journal_before: &serde_json::Value,
    mut files_after: BTreeMap<PathBuf, LogPath>,
) -> BTreeMap<PathBuf, LogPath> {
    let additions = files_after
        .keys()
        .filter(|path| !files_before.contains_key(*path))
        .cloned()
        .collect::<Vec<_>>();
    let journal_roots = ["checkpoint", "final_tree", "original_tip", "original_base"]
        .iter()
        .filter_map(|field| {
            journal_before
                .get(field)
                .and_then(serde_json::Value::as_str)
        })
        .chain(
            ["source", "base", "head", "anchor", "lease"]
                .iter()
                .filter_map(|field| {
                    journal_before
                        .get("state")
                        .and_then(|state| state.get(field))
                        .and_then(serde_json::Value::as_str)
                }),
        );
    let reachable_args = ["rev-list", "--objects", "--all", "HEAD"]
        .into_iter()
        .chain(journal_roots)
        .collect::<Vec<_>>();
    let reachable = repository.git(&reachable_args);
    for path in &additions {
        let relative = path
            .strip_prefix(".git/objects")
            .or_abort("new observation paths belong to the loose object store");
        let spelling = relative.to_str().or_abort("loose object path UTF-8");
        let mut components = spelling.split('/');
        let shard = components.next().or_abort("loose object shard");
        assert_eq!(shard.len(), 2);
        assert!(shard.bytes().all(|byte| byte.is_ascii_hexdigit()));
        if let Some(suffix) = components.next() {
            assert_eq!(suffix.len(), 38);
            assert!(suffix.bytes().all(|byte| byte.is_ascii_hexdigit()));
            assert!(components.next().is_none());
            let object = format!("{shard}{suffix}");
            assert_eq!(repository.git(&["cat-file", "-t", &object]), "tree");
            assert!(!reachable.lines().any(|line| line.starts_with(&object)));
        } else {
            assert_eq!(files_after.get(path), Some(&LogPath::Directory));
            assert!(
                additions
                    .iter()
                    .any(|child| child != path && child.starts_with(path))
            );
        }
    }
    // Every preexisting object and session/native/user path remains exact;
    // only independently validated unreachable loose trees may be added.
    files_after.retain(|path, _| files_before.contains_key(path));
    files_after
}

/// User ITA work collides only with a later descendant, outside the source pool.
fn outside_source_intent_repository() -> Repository {
    let repository = Repository::new();
    repository.write("base", "base\n");
    repository.commit("Add base tree");
    repository.write("atom", "selected atom\n");
    repository.write("remainder", "selected remainder\n");
    let source = repository.commit("Change selected source");
    let generated = repository.environment.cwd.join("gen");
    fs::create_dir_all(&generated).or_abort("descendant directory");
    repository.write("gen/notes.md", "descendant notes\n");
    repository.commit("Add descendant notes");
    repository.git(&["branch", "protected"]);
    repository.git(&["tag", "protected-tag"]);
    repository.success(&["--exec", "true", &source]);
    fs::create_dir_all(&generated).or_abort("user directory");
    repository.write("gen/notes.md", "user notes must survive\n");
    repository.git(&["add", "--intent-to-add", "--", "gen/notes.md"]);
    repository.git(&["add", "--", "atom"]);
    repository
}

/// Observes the exact native object body; the metadata string observer trims whitespace.
fn raw_current_message(repository: &Repository) -> Vec<u8> {
    let output = Command::new("git")
        .args(["cat-file", "commit", "HEAD"])
        .current_dir(&repository.environment.cwd)
        .output()
        .or_abort("raw native commit object");
    assert!(output.status.success(), "raw object observation failed");
    let boundary = output
        .stdout
        .windows(2)
        .position(|bytes| bytes == b"\n\n")
        .or_abort("raw native header boundary")
        .checked_add(2)
        .or_abort("message boundary offset");
    output.stdout.get(boundary..).or_abort("raw body").to_vec()
}

/// Constructs an independent byte oracle before any managed gate record exists.
fn raw_message_fixture(terminal_lfs: usize, body: &str) -> (Repository, String, String) {
    let repository = Repository::new();
    repository.git(&["config", "commit.cleanup", "verbatim"]);
    repository.write("base", "base\n");
    repository.commit("Base");
    repository.write("atom", "atom\n");
    repository.write("remainder", "remainder\n");
    let prefix =
        format!("Selected source\n\n{body}\n\nSigned-off-by: Author <author@example.invalid>");
    let original = format!("{prefix}{}", "\n".repeat(terminal_lfs));
    repository.commit(&original);
    assert_eq!(raw_current_message(&repository), original.as_bytes());
    let selected_tree = repository.git(&["rev-parse", "HEAD^{tree}"]);
    let command_path = repository.environment.cwd.join(".git/raw-message-command");
    fs::write(&command_path, b"true").or_abort("exact command bytes");
    let command_hash = repository.git(&[
        "hash-object",
        "--no-filters",
        command_path.to_str().or_abort("fixture command path"),
    ]);
    let extra_lfs = terminal_lfs
        .checked_sub(1)
        .or_abort("positive terminal LF count");
    let expected = format!(
        "{prefix}\nGate-tests: {command_hash}\n {selected_tree}\n{}",
        "\n".repeat(extra_lfs),
    );
    repository.success(&["--gate", "tests", "true", "HEAD"]);
    (repository, original, expected)
}

/// Every observation is taken before the attempted corrupt-state operation.
fn refuses_unchanged(repository: &Repository, arguments: &[&str]) {
    let journal = repository.journal();
    let index = repository.index();
    let head = repository.git(&["rev-parse", "HEAD"]);
    let branch = repository.git(&["rev-parse", "refs/heads/main"]);
    let lease = repository.git(&["rev-parse", "refs/factor/session-lease"]);
    let todo = fs::read(
        repository
            .environment
            .cwd
            .join(".git/rebase-merge/git-rebase-todo"),
    )
    .or_abort("active native todo");
    let (code, stdout) = repository.invoke(arguments);
    assert_ne!(code, EXIT_OK);
    assert_eq!(stdout, "");
    assert_eq!(repository.journal(), journal);
    assert_eq!(repository.index(), index);
    assert_eq!(repository.git(&["rev-parse", "HEAD"]), head);
    assert_eq!(repository.git(&["rev-parse", "refs/heads/main"]), branch);
    assert_eq!(
        repository.git(&["rev-parse", "refs/factor/session-lease"]),
        lease
    );
    assert_eq!(
        fs::read(
            repository
                .environment
                .cwd
                .join(".git/rebase-merge/git-rebase-todo")
        )
        .or_abort("preserved native todo"),
        todo
    );
}

/// Foreign valid capsules differ from malformed objects and failed ownership observations.
pub(in crate::git_factor::engine) fn unknown_closing_lease_census() {
    for query_failure in [false, true] {
        for command in ["--status", "--continue", "--abort"] {
            faults::closing_refuses_unknown_lease(query_failure, command);
        }
    }
}

/// Git abort must not be the first component to discover a destination collision.
#[expect(
    clippy::single_call_fn,
    reason = "named native fixture boundaries keep ownership, stimulus, and conservation visible to the direct canonical provider"
)]
pub(in crate::git_factor::engine) fn verify_abort_deleted_collision(alias: bool, ignored: bool) {
    let repository = Repository::new();
    let root = &repository.environment.cwd;
    repository.write("base", "original bytes\n");
    repository.commit("Base");
    repository.write("atom", "atom\n");
    repository.commit("Selected source");
    let selected = repository.git(&["rev-parse", "HEAD"]);
    repository.write("obsolete.txt", "descendant bytes\n");
    repository.commit("Restore in descendant");
    repository.success(&["--exec", "true", &selected]);
    if ignored {
        fs::write(
            root.join(".git/info/exclude"),
            "obsolete.txt\nOBSOLETE.TXT\n",
        )
        .or_abort("ignored collision fixture");
    }
    let path = root.join(if alias {
        "OBSOLETE.TXT"
    } else {
        "obsolete.txt"
    });
    if alias {
        fs::write(&path, "unrelated user bytes\n").or_abort("alias user recreation");
        if !root.join("obsolete.txt").exists() {
            return; // This filesystem distinguishes the two spellings.
        }
    } else {
        symlink(root.join("missing-user-target"), &path).or_abort("dangling user link");
    }
    refuses_unchanged(&repository, &["--abort"]);
    if alias {
        assert_eq!(
            fs::read(&path).or_abort("preserved alias bytes"),
            b"unrelated user bytes\n"
        );
    } else {
        assert_eq!(
            fs::read_link(&path).or_abort("preserved user link"),
            root.join("missing-user-target")
        );
    }
}

/// An exact terminal commit cannot authorize native pending side effects or more todo work.
#[expect(
    clippy::single_call_fn,
    reason = "named native fixture boundaries keep ownership, stimulus, and conservation visible to the direct canonical provider"
)]
fn verify_attached_terminal_pending_work() {
    let repository = interrupted_attached_terminal(false);
    let directory = repository.environment.cwd.join(".git/rebase-merge");
    let todo_path = directory.join("git-rebase-todo");
    let done_path = directory.join("done");
    for marker in [
        "autostash",
        "update-refs",
        "refs-to-delete",
        "rewritten-pending",
        "git-rebase-todo",
        "done",
    ] {
        let path = directory.join(marker);
        let original = fs::read(&path).ok();
        fs::write(
            &path,
            if path == todo_path || path == done_path {
                "exec true\n"
            } else {
                "foreign pending work\n"
            },
        )
        .or_abort("owned pending-native-work fixture");
        let journal = repository.journal();
        let index = repository.index();
        let refs = repository.git(&["show-ref"]);
        let head = repository.git(&["rev-parse", "HEAD"]);
        let (code, stdout) = repository.invoke(&["--continue"]);
        assert_ne!(code, EXIT_OK, "pending marker must refuse: {marker}");
        assert_eq!(stdout, "");
        assert_eq!(repository.journal(), journal);
        assert_eq!(repository.index(), index);
        assert_eq!(repository.git(&["show-ref"]), refs);
        assert_eq!(repository.git(&["rev-parse", "HEAD"]), head);
        assert_eq!(
            fs::read(&path).or_abort("preserved pending marker"),
            if path == todo_path || path == done_path {
                b"exec true\n".as_slice()
            } else {
                b"foreign pending work\n".as_slice()
            }
        );
        match original {
            Some(bytes) => fs::write(&path, bytes).or_abort("restore owned native fixture"),
            None => fs::remove_file(&path).or_abort("remove only owned native fixture"),
        }
    }
    repository.success(&["--abort"]);
}

/// Continue captures before restarting; abort keeps that just-completed checkpoint.
fn verify_attached_terminal_recovery(abort: bool) {
    let repository = interrupted_attached_terminal(false);
    observe_attached_terminal(&repository);
    let captured = repository.git(&["rev-parse", "HEAD"]);
    let expected_tree = repository.git(&["rev-parse", "HEAD^{tree}"]);
    if abort {
        repository.success(&["--abort"]);
        assert!(
            !repository
                .environment
                .cwd
                .join(".git/rebase-merge")
                .exists()
        );
        assert!(!repository.environment.cwd.join(".git/factor").exists());
    } else {
        repository.success(&["--continue"]);
        let resumed: serde_json::Value =
            serde_json::from_slice(&repository.journal()).or_abort("next round journal");
        assert_eq!(
            resumed
                .pointer("/checkpoint")
                .and_then(serde_json::Value::as_str),
            Some(captured.as_str())
        );
        assert_eq!(
            resumed
                .pointer("/state/phase")
                .and_then(serde_json::Value::as_str),
            Some("selecting")
        );
        repository.success(&["--abort"]);
    }
    assert_eq!(repository.git(&["rev-parse", "refs/heads/main"]), captured);
    assert_eq!(repository.git(&["rev-parse", "HEAD^{tree}"]), expected_tree);
    assert_eq!(repository.git(&["symbolic-ref", "HEAD"]), "refs/heads/main");
    assert_eq!(
        fs::read(repository.environment.cwd.join("unrelated"))
            .or_abort("preserved completed-interval bytes"),
        b"preserved unrelated bytes\n"
    );
}

#[expect(
    clippy::single_call_fn,
    reason = "named native fixture boundaries keep ownership, stimulus, and conservation visible to the direct canonical provider"
)]
pub(in crate::git_factor::engine) fn verify_candidate_gate_failure() {
    let repository = Repository::new();
    repository.write("base", "base\n");
    repository.commit("Base");
    repository.write("atom", "atom\n");
    repository.write("remainder", "remainder\n");
    repository.commit("Selected source");
    repository.success(&["--exec", "test -f remainder", "HEAD"]);
    repository.git(&["add", "atom"]);
    let head = repository.git(&["rev-parse", "HEAD"]);
    let index = repository.index();
    let journal = repository.journal();
    let branch = repository.git(&["rev-parse", "refs/heads/main"]);
    let (code, output) = repository.invoke(&["--message", "Refused atom"]);
    assert_eq!(code, EXIT_TEMPFAIL);
    let observed: serde_json::Value =
        serde_json::from_str(&output).or_abort("one gate failure JSON");
    assert_eq!(
        observed
            .pointer("/operation")
            .or_abort("required JSON fact"),
        "continue"
    );
    assert_eq!(
        observed.pointer("/result").or_abort("required JSON fact"),
        "gate_failed"
    );
    assert_eq!(
        observed.pointer("/gate").or_abort("required JSON fact"),
        &serde_json::from_str::<serde_json::Value>(
            r#"{"command":"test -f remainder","exit_code":1}"#
        )
        .or_abort("independent gate failure oracle")
    );
    assert_eq!(repository.git(&["rev-parse", "HEAD"]), head);
    assert_eq!(repository.index(), index);
    assert_eq!(repository.journal(), journal);
    assert_eq!(repository.git(&["rev-parse", "refs/heads/main"]), branch);
    assert_eq!(
        fs::read(repository.environment.cwd.join("remainder")).or_abort("preserved remainder"),
        b"remainder\n"
    );
    repository.success(&["--abort"]);
}

#[expect(
    clippy::single_call_fn,
    reason = "named native fixture boundaries keep ownership, stimulus, and conservation visible to the direct canonical provider"
)]
pub(in crate::git_factor::engine) fn verify_changed_attachment() {
    let repository = open_selection();
    repository.git(&["branch", "foreign", "HEAD"]);
    repository.git(&["symbolic-ref", "HEAD", "refs/heads/foreign"]);
    for arguments in [
        vec!["--retry"],
        vec!["--finish"],
        vec!["--continue"],
        vec!["--message", "Atom"],
        vec!["--abort"],
    ] {
        refuses_unchanged(&repository, &arguments);
        assert_eq!(
            repository.git(&["symbolic-ref", "HEAD"]),
            "refs/heads/foreign"
        );
    }
}

#[expect(
    clippy::single_call_fn,
    reason = "named native fixture boundaries keep ownership, stimulus, and conservation visible to the direct canonical provider"
)]
pub(in crate::git_factor::engine) fn verify_completed_zero_refusal() {
    let repository = open_selection();
    let mut journal: serde_json::Value =
        serde_json::from_slice(&repository.journal()).or_abort("saved source facts");
    repository.git(&[
        "read-tree",
        journal
            .pointer("/state/source")
            .or_abort("required JSON fact")
            .as_str()
            .or_abort("selected source"),
    ]);
    repository.git(&["rebase", "--abort"]);
    let mut terminal: serde_json::Value = serde_json::from_str(
        r#"{"phase":"closing","lease":"","outcome":{"result":"complete","atom":""}}"#,
    )
    .or_abort("independent zero-progress fixture");
    *terminal
        .pointer_mut("/lease")
        .or_abort("required JSON fact") = journal
        .pointer("/state/lease")
        .or_abort("required JSON fact")
        .clone();
    *terminal
        .pointer_mut("/outcome/atom")
        .or_abort("required JSON fact") = journal
        .pointer("/original_base")
        .or_abort("required JSON fact")
        .clone();
    *journal.pointer_mut("/state").or_abort("required JSON fact") = terminal;
    fs::write(
        repository.environment.cwd.join(".git/factor-journal.json"),
        serde_json::to_vec(&journal).or_abort("zero-progress terminal bytes"),
    )
    .or_abort("journal fault");
    let index = repository.index();
    let saved = repository.journal();
    let head = repository.git(&["rev-parse", "HEAD"]);
    let lease = repository.git(&["rev-parse", "refs/factor/session-lease"]);
    let (code, output) = repository.invoke(&["--continue"]);
    assert_eq!(code, EXIT_SOFTWARE);
    assert_eq!(output, "");
    assert!(
        repository
            .output
            .stderr
            .borrow()
            .contains("completed closing lacks an accepted atom")
    );
    assert_eq!(repository.index(), index);
    assert_eq!(repository.journal(), saved);
    assert_eq!(repository.git(&["rev-parse", "HEAD"]), head);
    assert_eq!(
        repository.git(&["rev-parse", "refs/factor/session-lease"]),
        lease
    );
}

#[expect(
    clippy::single_call_fn,
    reason = "named native fixture boundaries keep ownership, stimulus, and conservation visible to the direct canonical provider"
)]
pub(in crate::git_factor::engine) fn verify_corrupt_source() {
    let repository = open_selection();
    repository.git(&["add", "atom"]);
    let mut journal: serde_json::Value =
        serde_json::from_slice(&repository.journal()).or_abort("actual saved facts");
    let source = journal
        .pointer("/state/source")
        .or_abort("required JSON fact")
        .as_str()
        .or_abort("source commit");
    let base = journal
        .pointer("/state/base")
        .or_abort("required JSON fact")
        .as_str()
        .or_abort("actual parent");
    let tree = repository.git(&["rev-parse", &format!("{source}^{{tree}}")]);
    let foreign = repository.git(&[
        "-c",
        "user.name=Foreign Author",
        "commit-tree",
        &tree,
        "-p",
        base,
        "-m",
        "Foreign metadata",
    ]);
    *journal
        .pointer_mut("/state/source")
        .or_abort("required JSON fact") = serde_json::Value::String(foreign);
    fs::write(
        repository.environment.cwd.join(".git/factor-journal.json"),
        serde_json::to_vec(&journal).or_abort("tampered journal bytes"),
    )
    .or_abort("journal fault");
    refuses_unchanged(&repository, &["--message", "Accepted atom"]);
}

/// Both real native intervals recover from the same Verified facts without altering user work.
#[expect(
    clippy::cognitive_complexity,
    reason = "the closed fixture boundary matrix keeps before/after native interruption and preservation assertions explicit"
)]
#[expect(
    clippy::single_call_fn,
    reason = "named native fixture boundaries keep ownership, stimulus, and conservation visible to the direct canonical provider"
)]
pub(in crate::git_factor::engine) fn verify_detached_terminal_recovery(
    after_quit: bool,
    abort: bool,
) {
    let repository = interrupted_detached_terminal();
    let admin = repository.environment.cwd.join(".git");
    let journal = repository.journal();
    let refs = repository.git(&["show-ref"]);
    let index = repository.index();
    let tip = repository.git(&["rev-parse", "HEAD"]);
    let status = repository.success(&["--status"]);
    assert_eq!(
        status
            .pointer("/session/phase")
            .and_then(serde_json::Value::as_str),
        Some("verified")
    );
    assert_eq!(repository.journal(), journal);
    assert_eq!(repository.git(&["show-ref"]), refs);
    assert_eq!(repository.index(), index);
    assert!(
        admin.join("HEAD.lock").is_file(),
        "status must preserve foreign locks"
    );
    fs::remove_file(admin.join("HEAD.lock")).or_abort("remove only owned injected lock");
    let command = if abort { "--abort" } else { "--continue" };
    if after_quit {
        let interrupted = QuitInterruption {
            fired: Cell::new(false),
        };
        let (code, _) = repository.invoke_with(&[command], &interrupted, &REAL_FS);
        assert_ne!(code, EXIT_OK);
        assert!(interrupted.fired.get());
        assert!(!admin.join("rebase-merge").exists());
        assert_eq!(repository.journal(), journal);
        assert_eq!(repository.git(&["show-ref"]), refs);
        assert_eq!(
            repository.git(&["rev-parse", "--symbolic-full-name", "HEAD"]),
            "HEAD"
        );
        let recovered_status = repository.success(&["--status"]);
        assert_eq!(
            recovered_status
                .pointer("/session/phase")
                .and_then(serde_json::Value::as_str),
            Some("verified")
        );
        assert_eq!(repository.index(), index);
    }
    let result = repository.success(&[command]);
    assert_eq!(
        result.get("operation").and_then(serde_json::Value::as_str),
        Some(if abort { "abort" } else { "continue" })
    );
    assert_eq!(
        result.get("result").and_then(serde_json::Value::as_str),
        Some("complete")
    );
    assert_eq!(
        result
            .get("split_count")
            .and_then(serde_json::Value::as_u64),
        Some(1)
    );
    assert_eq!(repository.git(&["rev-parse", "HEAD"]), tip);
    let retained = refs
        .lines()
        .filter(|line| !line.ends_with(" refs/factor/session-lease"))
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(repository.git(&["show-ref"]), retained);
    assert_eq!(repository.index(), index);
    assert_eq!(repository.git(&["symbolic-ref", "HEAD"]), "refs/heads/main");
    assert!(!admin.join("factor-journal.json").exists());
    assert!(!admin.join("factor").exists());
    assert_eq!(
        fs::read(repository.environment.cwd.join("unrelated")).or_abort("terminal user bytes"),
        b"preserved terminal user bytes\n"
    );
}

pub(in crate::git_factor::engine) fn verify_empty_admission(content: &str) {
    let repository = Repository::new();
    repository.write("base", content);
    repository.commit("Base");
    repository.git(&["commit", "--allow-empty", "--quiet", "--message", "Empty"]);
    let head = repository.git(&["rev-parse", "HEAD"]);
    let index = repository.index();
    let (code, _) = repository.invoke(&["--exec", "true", "HEAD"]);
    assert_eq!(code, EXIT_DATAERR);
    assert_eq!(repository.git(&["rev-parse", "HEAD"]), head);
    assert_eq!(repository.index(), index);
    assert!(!repository.environment.cwd.join(".git/factor").exists());
    assert_eq!(
        fs::read(repository.environment.cwd.join("base")).or_abort("unchanged tracked bytes"),
        content.as_bytes()
    );
}

/// Observe this public API once against an independently arranged native world.
#[expect(
    clippy::create_dir,
    reason = "exclusive fixture directories must refuse existing occupancy rather than silently admit unrelated files"
)]
#[expect(
    clippy::ref_patterns,
    reason = "explicit borrowed fixture variants preserve owned payloads without cloning under contradictory pattern restriction lints"
)]
#[expect(
    clippy::too_many_lines,
    reason = "one decisive native lifecycle keeps its exact interruption evidence and full preservation oracle together"
)]
pub(in crate::git_factor::engine) fn verify_error_log_observation(world: LogWorld, content: &str) {
    let repository = Repository::new();
    repository.write("base", "base\n");
    repository.commit("Base");
    repository.write("atom", content);
    repository.write("remainder", "selected remainder\n");
    repository.commit("Selected source");
    repository.write("unrelated", content);
    match world {
        LogWorld::AbsentSession => {}
        LogWorld::AbsentScratch
        | LogWorld::Admitted
        | LogWorld::CorruptJournal
        | LogWorld::ForeignScratch
        | LogWorld::JournalMetadataError
        | LogWorld::LateScratchFile
        | LogWorld::LateScratchSymlink
        | LogWorld::ScratchMetadataError => {
            repository.success(&["--exec", "true", "HEAD"]);
            let selecting = serde_json::from_slice::<serde_json::Value>(&repository.journal())
                .or_abort("actual arranged journal");
            assert_eq!(
                selecting
                    .pointer("/state/phase")
                    .and_then(serde_json::Value::as_str),
                Some("selecting"),
                "actual dispatch must arrange Selecting before external inputs"
            );
        }
    }
    let journal = repository.environment.cwd.join(".git/factor-journal.json");
    let scratch = repository.environment.cwd.join(".git/factor");
    let foreign_directory = repository
        .directory
        .path()
        .join("foreign-diagnostic-directory");
    match world {
        LogWorld::CorruptJournal => fs::write(&journal, "{}").or_abort("foreign journal bytes"),
        LogWorld::AbsentScratch => {
            fs::remove_dir_all(&scratch).or_abort("missing diagnostic scratch");
        }
        LogWorld::ForeignScratch => {
            fs::remove_dir_all(&scratch).or_abort("replace diagnostic scratch");
            fs::write(&scratch, content).or_abort("foreign scratch bytes");
        }
        LogWorld::LateScratchSymlink => {
            fs::create_dir(&foreign_directory).or_abort("foreign diagnostic directory");
            fs::write(foreign_directory.join("user-file"), content)
                .or_abort("foreign diagnostic user bytes");
        }
        LogWorld::AbsentSession
        | LogWorld::Admitted
        | LogWorld::JournalMetadataError
        | LogWorld::ScratchMetadataError
        | LogWorld::LateScratchFile => {}
    }
    let expected = match world {
        LogWorld::Admitted => Some(scratch.clone()),
        LogWorld::AbsentScratch
        | LogWorld::AbsentSession
        | LogWorld::CorruptJournal
        | LogWorld::ForeignScratch
        | LogWorld::JournalMetadataError
        | LogWorld::LateScratchFile
        | LogWorld::LateScratchSymlink
        | LogWorld::ScratchMetadataError => None,
    };
    let expected_calls: usize = match world {
        LogWorld::Admitted
        | LogWorld::LateScratchFile
        | LogWorld::LateScratchSymlink
        | LogWorld::ScratchMetadataError => 3,
        LogWorld::AbsentSession | LogWorld::JournalMetadataError => 1,
        LogWorld::AbsentScratch | LogWorld::CorruptJournal | LogWorld::ForeignScratch => 2,
    };
    let stimulus = match world {
        LogWorld::JournalMetadataError => LogStimulus::JournalUnavailable,
        LogWorld::ScratchMetadataError => LogStimulus::FinalScratchUnavailable,
        LogWorld::LateScratchFile => {
            LogStimulus::ReplaceFinalScratch(LateScratch::File(content.to_owned()))
        }
        LogWorld::LateScratchSymlink => {
            LogStimulus::ReplaceFinalScratch(LateScratch::Symlink(foreign_directory))
        }
        LogWorld::AbsentScratch
        | LogWorld::AbsentSession
        | LogWorld::Admitted
        | LogWorld::CorruptJournal
        | LogWorld::ForeignScratch => LogStimulus::Observe,
    };
    let (expected_fired, replacement_contents, foreign_target) = match &stimulus {
        &LogStimulus::Observe => (false, None, None),
        &LogStimulus::JournalUnavailable | &LogStimulus::FinalScratchUnavailable => {
            (true, None, None)
        }
        &LogStimulus::ReplaceFinalScratch(LateScratch::File(ref body)) => {
            (true, Some(LogPath::File(body.as_bytes().to_vec())), None)
        }
        &LogStimulus::ReplaceFinalScratch(LateScratch::Symlink(ref target)) => {
            let inventory = BTreeMap::from([(
                PathBuf::from("user-file"),
                LogPath::File(content.as_bytes().to_vec()),
            )]);
            (
                true,
                Some(LogPath::Symlink(target.clone())),
                Some((target.clone(), inventory)),
            )
        }
    };
    let filesystem = LogObservationFs {
        admin: repository.environment.cwd.join(".git"),
        stimulus,
        calls: Cell::new(0),
        fired: Cell::new(false),
    };
    let output = Capture::default();
    let actor_context = Ctx {
        cwd: repository.environment.cwd.clone(),
        env: &repository.environment,
        fs: &filesystem,
        io: &output,
        runner: &REAL_RUNNER,
    };
    let mut expected_inventory = log_observation_inventory(&repository.environment.cwd);
    // The stimulus supplies the independent, planned external footprint.
    if let Some(replacement_bytes) = replacement_contents {
        let relative = scratch
            .strip_prefix(&repository.environment.cwd)
            .or_abort("owned diagnostic scratch")
            .to_path_buf();
        assert!(
            !expected_inventory
                .keys()
                .any(|path| path != &relative && path.starts_with(&relative)),
            "late replacement requires the arranged scratch to be empty"
        );
        let previous = expected_inventory.insert(relative, replacement_bytes);
        assert_eq!(previous, Some(LogPath::Directory));
    }

    let observed = super::error_log_directory(&actor_context);

    assert_eq!(
        observed.map(|directory| directory.as_path().to_path_buf()),
        expected
    );
    assert_eq!(
        filesystem.calls.get(),
        expected_calls,
        "exact metadata schedule keeps the final boundary after load"
    );
    assert_eq!(
        filesystem.fired.get(),
        expected_fired,
        "selected physical/error stimulus must fire exactly as arranged"
    );
    assert_eq!(
        log_observation_inventory(&repository.environment.cwd),
        expected_inventory
    );
    if let Some((target, expected_target)) = foreign_target {
        assert_eq!(log_observation_inventory(&target), expected_target);
    }
    assert!(output.stdout.borrow().is_empty());
    assert!(output.stderr.borrow().is_empty());
}

/// Refuses any pre-existing path, including dangling links which `exists()` cannot observe.
#[expect(
    clippy::create_dir,
    reason = "exclusive fixture directories must refuse existing occupancy rather than silently admit unrelated files"
)]
pub(in crate::git_factor::engine) fn verify_foreign_session_path(
    journal: bool,
    kind: ForeignPathKind,
    bytes: &str,
) {
    let repository = Repository::new();
    repository.write("base", "base\n");
    repository.commit("Base");
    repository.write("atom", "atom\n");
    repository.commit("Selected source");
    repository.write("unrelated", "preserved unrelated bytes\n");
    let admin = repository.environment.cwd.join(".git");
    let protected = admin.join(if journal {
        "factor-journal.json"
    } else {
        "factor"
    });
    let target = admin.join("foreign-path-target");
    match kind {
        ForeignPathKind::File => fs::write(&protected, bytes).or_abort("foreign regular path"),
        ForeignPathKind::Directory => {
            fs::create_dir(&protected).or_abort("foreign directory");
            fs::write(protected.join("foreign-bytes"), bytes).or_abort("foreign directory bytes");
        }
        ForeignPathKind::Symlink => {
            fs::write(&target, bytes).or_abort("foreign symlink target");
            symlink(&target, &protected).or_abort("foreign symlink");
        }
        ForeignPathKind::Dangling => {
            symlink(&target, &protected).or_abort("foreign dangling symlink");
        }
    }
    let index = repository.index();
    let head = repository.git(&["rev-parse", "HEAD"]);
    let refs = repository.git(&["show-ref"]);
    let (code, output) = repository.invoke(&["--exec", "true", "HEAD"]);
    assert_ne!(code, EXIT_OK);
    assert_eq!(output, "");
    assert_eq!(repository.index(), index);
    assert_eq!(repository.git(&["rev-parse", "HEAD"]), head);
    assert_eq!(repository.git(&["show-ref"]), refs);
    for (path, expected) in [
        ("base", b"base\n".as_slice()),
        ("atom", b"atom\n".as_slice()),
        ("unrelated", b"preserved unrelated bytes\n".as_slice()),
    ] {
        assert_eq!(
            fs::read(repository.environment.cwd.join(path)).or_abort("physical bytes preserved"),
            expected
        );
    }
    match kind {
        ForeignPathKind::File => assert_eq!(
            fs::read(&protected).or_abort("foreign file preserved"),
            bytes.as_bytes()
        ),
        ForeignPathKind::Directory => {
            assert_eq!(
                fs::read(protected.join("foreign-bytes")).or_abort("foreign bytes preserved"),
                bytes.as_bytes()
            );
            let entries = fs::read_dir(&protected)
                .or_abort("foreign directory preserved")
                .map(|entry| entry.or_abort("foreign entry").file_name())
                .collect::<Vec<_>>();
            assert_eq!(entries, vec![OsString::from("foreign-bytes")]);
        }
        ForeignPathKind::Symlink => {
            assert_eq!(
                fs::read_link(&protected).or_abort("foreign link preserved"),
                target
            );
            assert_eq!(
                fs::read(&target).or_abort("foreign target preserved"),
                bytes.as_bytes()
            );
        }
        ForeignPathKind::Dangling => {
            assert_eq!(
                fs::read_link(&protected).or_abort("dangling link preserved"),
                target
            );
            assert!(!target.exists());
        }
    }
    assert!(
        !admin
            .join(if journal {
                "factor"
            } else {
                "factor-journal.json"
            })
            .exists()
    );
}
#[expect(
    clippy::single_call_fn,
    reason = "named native fixture boundaries keep ownership, stimulus, and conservation visible to the direct canonical provider"
)]
pub(in crate::git_factor::engine) fn verify_foundation_input(
    world: FoundationInput,
    contents: &str,
) {
    match world {
        FoundationInput::JournalFile => {
            verify_foreign_session_path(true, ForeignPathKind::File, contents);
        }
        FoundationInput::JournalDirectory => {
            verify_foreign_session_path(true, ForeignPathKind::Directory, contents);
        }
        FoundationInput::JournalSymlink => {
            verify_foreign_session_path(true, ForeignPathKind::Symlink, contents);
        }
        FoundationInput::ScratchFile => {
            verify_foreign_session_path(false, ForeignPathKind::File, contents);
        }
        FoundationInput::ScratchDirectory => {
            verify_foreign_session_path(false, ForeignPathKind::Directory, contents);
        }
        FoundationInput::ScratchSymlink => {
            verify_foreign_session_path(false, ForeignPathKind::Symlink, contents);
        }
        FoundationInput::InitialIgnored => {
            verify_readmission_collision(ReadmissionBoundary::InitialPreparing, false, contents);
        }
        FoundationInput::InitialHardlink => {
            verify_readmission_collision(ReadmissionBoundary::InitialPreparing, true, contents);
        }
        FoundationInput::RestartIgnored => {
            verify_readmission_collision(ReadmissionBoundary::RestartPreparing, false, contents);
        }
        FoundationInput::RestartHardlink => {
            verify_readmission_collision(ReadmissionBoundary::RestartPreparing, true, contents);
        }
        FoundationInput::OpeningIgnored => {
            verify_readmission_collision(ReadmissionBoundary::Opening, false, contents);
        }
        FoundationInput::OpeningHardlink => {
            verify_readmission_collision(ReadmissionBoundary::Opening, true, contents);
        }
    }
}
#[expect(
    clippy::single_call_fn,
    reason = "named native fixture boundaries keep ownership, stimulus, and conservation visible to the direct canonical provider"
)]
#[expect(
    clippy::too_many_lines,
    reason = "one decisive native lifecycle keeps its exact interruption evidence and full preservation oracle together"
)]
pub(in crate::git_factor::engine) fn verify_generated_e1(scenario: NativeScenario, contents: &str) {
    match scenario {
        NativeScenario::IntermediateBeforePublication => {
            e1::publication_retry(false, false, contents);
        }
        NativeScenario::IntermediateAfterPublication => {
            e1::publication_retry(false, true, contents);
        }
        NativeScenario::FinalBeforePublication => e1::publication_retry(true, false, contents),
        NativeScenario::FinalAfterPublication => e1::publication_retry(true, true, contents),
        NativeScenario::RemainderConsumedBeforeAcceptance => {
            e1::consumed_retry("checkpoint-gate-remainder", false, contents);
        }
        NativeScenario::RemainderConsumedAfterAcceptance => {
            e1::consumed_retry("checkpoint-gate-remainder", true, contents);
        }
        NativeScenario::FinalDescendantConsumedBeforeAcceptance => {
            e1::consumed_retry("checkpoint-gate-descendant", false, contents);
        }
        NativeScenario::FinalDescendantConsumedAfterAcceptance => {
            e1::consumed_retry("checkpoint-gate-descendant", true, contents);
        }
        NativeScenario::TerminalConsumedBeforeAcceptance => {
            e1::consumed_retry("checkpoint-terminal", false, contents);
        }
        NativeScenario::TerminalConsumedAfterAcceptance => {
            e1::consumed_retry("checkpoint-terminal", true, contents);
        }
        NativeScenario::IntermediateConsumedBeforeAcceptance => {
            e1::consumed_intermediate_retry(contents);
        }
        NativeScenario::ForeignExec => e1::grammar_refusal(true, contents),
        NativeScenario::MissingOriginalPick => e1::grammar_refusal(false, contents),
        NativeScenario::FuturePickDuplicate => e1::future_retry_refusal(false, contents),
        NativeScenario::FutureExecDuplicate => e1::future_retry_refusal(true, contents),
        NativeScenario::BeforeNativeExec => e1::before_exec_control(contents),
        NativeScenario::AmendedSameSlotThenDroppedNextSlot => {
            e1::amended_then_dropped_conflict(&format!("repaired:{contents}\n"));
        }
        NativeScenario::FailedPickFactorRetry => {
            e1::failed_pick_retry(false, contents, "preserved obstruction\n");
        }
        NativeScenario::FailedPickNativeRetry => {
            e1::failed_pick_retry(true, contents, "preserved obstruction\n");
        }
        NativeScenario::OpeningRootRetry => {
            e1::opening_root_retry(contents, "owned root obstruction\n");
        }
        NativeScenario::OpeningWrongPick => {
            e1::opening_wrong_pick_refusal(contents, "owned root obstruction\n");
        }
        NativeScenario::OpeningReadyBreak => e1::opening_ready_break_control(contents),
        NativeScenario::ManualBypassRefusal => e1::bypass_lost_callback_refusal(contents),
        NativeScenario::LastDonePickDuplicate => {
            e1::last_done_pick_duplicate_refusal(contents, "preserved obstruction\n");
        }
        NativeScenario::RetainedDescendantStillFails => {
            e1::retained_descendant_still_fails(contents);
        }
        NativeScenario::RemainderBypassWithoutDescendants => {
            e1::remainder_bypass_refusal(false, contents);
        }
        NativeScenario::RemainderBypassWithDescendants => {
            e1::remainder_bypass_refusal(true, contents);
        }
        NativeScenario::FinalDescendantBypass => e1::final_descendant_bypass_refusal(contents),
        NativeScenario::OpeningForeignHead => {
            e1::opening_foreign_head_refusal(contents, "owned root obstruction\n");
        }
        NativeScenario::OpeningAdvanced => e1::opening_advanced_refusal(contents),
        NativeScenario::OpeningSourceFinished => e1::opening_source_finished_before_break(contents),
        NativeScenario::PrototypeFormatRefusal => e1::prototype_format_refusal(contents),
        NativeScenario::IgnoredReplayObstruction => e1::ignored_replay_obstruction(contents),
        NativeScenario::RecreatedDescendantExec => e1::recreated_descendant_path(false, contents),
        NativeScenario::RecreatedFuturePick => e1::recreated_descendant_path(true, contents),
        NativeScenario::ObservedSparseHook => e1::observed_sparse_hook_refusal(contents),
        NativeScenario::PendingSelectionRewind => e1::pending_selection_rewind_refusal(contents),
        NativeScenario::OpeningNonrootAbort => {
            e1::opening_nonroot_retry_abort(contents, "preserved canceled input\n");
        }
        NativeScenario::OpeningDeletedRecreation => {
            e1::opening_deleted_recreation_refusal(contents);
        }
        NativeScenario::OpeningRootAbort => {
            e1::opening_root_retry_abort(contents, "owned root obstruction\n");
        }
        NativeScenario::PendingRemainderBypass => e1::bypass_pending_pick_refusal(true, contents),
        NativeScenario::PendingDescendantBypass => e1::bypass_pending_pick_refusal(false, contents),
        NativeScenario::PendingAmendedPredecessor => {
            e1::pending_amended_predecessor_refusal(contents);
        }
        NativeScenario::ExecutableSymlinkAliases => e1::executable_alias_continuation(contents),
        NativeScenario::ExecutableCopiedPath => {
            e1::executable_distinct_path_refusal(false, contents);
        }
        NativeScenario::ExecutableHardLinkedPath => {
            e1::executable_distinct_path_refusal(true, contents);
        }
        NativeScenario::ExecutableObservationIo => {
            e1::executable_observation_refusal(e1::ExecutableFailure::CurrentExe, contents);
        }
        NativeScenario::ExecutableCanonicalizationIo => {
            e1::executable_observation_refusal(e1::ExecutableFailure::Canonicalize, contents);
        }
        #[cfg(target_os = "linux")]
        NativeScenario::ExecutableUtf8 => {
            e1::executable_observation_refusal(e1::ExecutableFailure::Utf8, contents);
        }
    }
}

/// Varies unrelated bytes through the actual interrupted-preparation consumer.
#[expect(
    clippy::single_call_fn,
    reason = "named native fixture boundaries keep ownership, stimulus, and conservation visible to the direct canonical provider"
)]
pub(in crate::git_factor::engine) fn verify_generated_readmission(contents: &str) {
    verify_readmission_collision(ReadmissionBoundary::InitialPreparing, false, contents);
}
#[expect(
    clippy::too_many_lines,
    reason = "one prerequisite Act keeps exact observation order, diagnostics, trace and native conservation in the same owning fixture"
)]
pub(in crate::git_factor::engine) fn verify_git_version_admission(
    internal: bool,
    text: &str,
    supported: bool,
    failure: VersionFailure,
) {
    let repository = Repository::new();
    repository.write("base", "base\n");
    repository.commit("Base");
    repository.write("unrelated", "preserved bytes\n");
    let head = repository.git(&["rev-parse", "HEAD"]);
    let references = repository.git(&["show-ref"]);
    let index = repository.index();
    let runner = VersionObservation {
        text,
        failure,
        calls: Cell::new(0),
        later: Cell::new(0),
    };
    let trace = repository.directory.path().join("owned-version-trace");
    fs::write(&trace, "preserved preexisting trace bytes\n").or_abort("owned trace sentinel");
    let environment = VersionTraceEnvironment {
        native: &repository.environment,
        trace: trace.clone(),
    };
    let context = Ctx {
        cwd: repository.environment.cwd.clone(),
        env: &environment,
        fs: &REAL_FS,
        io: &repository.output,
        runner: &runner,
    };
    let arguments = if internal {
        &["git-factor", "checkpoint-unknown-version-probe"][..]
    } else {
        &["git-factor", "--status"][..]
    };
    let native_arguments = arguments.iter().map(OsString::from).collect::<Vec<_>>();
    let code = main_entry_with_vec(context.io, Ok(context), &native_arguments);
    let result = (code, repository.output.stdout.borrow().clone());
    assert_eq!(runner.calls.get(), 1);
    if supported && matches!(failure, VersionFailure::None) {
        if internal {
            assert_ne!(result.0, EXIT_OK);
            assert!(
                repository
                    .output
                    .stderr
                    .borrow()
                    .contains("unknown internal checkpoint command")
            );
        } else {
            assert_eq!(result.0, EXIT_OK);
            assert!(result.1.contains("\"session\":null"));
        }
    } else {
        assert_ne!(result.0, EXIT_OK);
        assert!(result.1.is_empty());
        assert_eq!(
            fs::read(&trace).or_abort("trace refusal preservation"),
            b"preserved preexisting trace bytes\n",
            "compatibility refusal including query/spawn errors must not append session snapshots"
        );
        assert_eq!(
            runner.later.get(),
            0,
            "every prerequisite failure refuses before native authority observation"
        );
        assert_eq!(
            result.0,
            match failure {
                VersionFailure::None => EXIT_USAGE,
                VersionFailure::Query
                | VersionFailure::SilentQuery
                | VersionFailure::WhitespaceQuery
                | VersionFailure::Spawn => EXIT_SOFTWARE,
            }
        );
        let diagnostic = repository.output.stderr.borrow();
        let expected_diagnostic = match failure {
            VersionFailure::None => format!(
                "Git 2.56.0 or newer released Git is required; observed {}\n",
                text.trim()
            ),
            VersionFailure::Query => {
                "git command failed: owned Git version query failure\n".to_owned()
            }
            VersionFailure::SilentQuery | VersionFailure::WhitespaceQuery => {
                "git command failed: exit status: 1\n".to_owned()
            }
            VersionFailure::Spawn => {
                "git command failed: git version: owned Git version spawn failure\n".to_owned()
            }
        };
        assert_eq!(diagnostic.as_str(), expected_diagnostic);
    }
    assert_eq!(repository.index(), index);
    assert_eq!(repository.git(&["rev-parse", "HEAD"]), head);
    assert_eq!(repository.git(&["show-ref"]), references);
    assert!(
        !repository
            .environment
            .cwd
            .join(".git/factor-journal.json")
            .exists()
    );
    assert_eq!(
        fs::read(repository.environment.cwd.join("unrelated")).or_abort("foreign bytes"),
        b"preserved bytes\n"
    );
}

#[expect(
    clippy::single_call_fn,
    reason = "named native fixture boundaries keep ownership, stimulus, and conservation visible to the direct canonical provider"
)]
pub(in crate::git_factor::engine) fn verify_impossible_closing() {
    let repository = open_selection();
    let mut journal: serde_json::Value =
        serde_json::from_slice(&repository.journal()).or_abort("actual saved facts");
    let mut terminal: serde_json::Value = serde_json::from_str(
        r#"{"phase":"closing","lease":"","outcome":{"result":"aborted","base":null}}"#,
    )
    .or_abort("independent terminal fixture");
    *terminal
        .pointer_mut("/lease")
        .or_abort("required JSON fact") = journal
        .pointer("/state/lease")
        .or_abort("required JSON fact")
        .clone();
    *terminal
        .pointer_mut("/outcome/base")
        .or_abort("required JSON fact") = journal
        .pointer("/state/base")
        .or_abort("required JSON fact")
        .clone();
    *journal.pointer_mut("/state").or_abort("required JSON fact") = terminal;
    fs::write(
        repository.environment.cwd.join(".git/factor-journal.json"),
        serde_json::to_vec(&journal).or_abort("tampered phase bytes"),
    )
    .or_abort("journal fault");
    refuses_unchanged(&repository, &["--continue"]);
}

/// A canceled original path still owns a native checkout collision at initial admission.
#[expect(
    clippy::single_call_fn,
    reason = "named native fixture boundaries keep ownership, stimulus, and conservation visible to the direct canonical provider"
)]
pub(in crate::git_factor::engine) fn verify_initial_checkout_collision(alias: bool, ignored: bool) {
    let repository = Repository::new();
    let root = &repository.environment.cwd;
    repository.write("base", "base\n");
    repository.commit("Base");
    repository.write("obsolete.txt", "selected bytes\n");
    repository.write("atom", "atom\n");
    repository.commit("Selected source");
    let selected = repository.git(&["rev-parse", "HEAD"]);
    fs::remove_file(root.join("obsolete.txt")).or_abort("descendant deletion");
    repository.commit("Remove in descendant");
    if ignored {
        fs::write(
            root.join(".git/info/exclude"),
            "obsolete.txt\nOBSOLETE.TXT\n",
        )
        .or_abort("ignored collision fixture");
    }
    let path = root.join(if alias {
        "OBSOLETE.TXT"
    } else {
        "obsolete.txt"
    });
    if alias {
        fs::write(&path, "unrelated user bytes\n").or_abort("initial alias file");
        if !root.join("obsolete.txt").exists() {
            return; // This filesystem distinguishes the two spellings.
        }
    } else {
        symlink(root.join("missing-user-target"), &path).or_abort("initial dangling user link");
    }
    let index = repository.index();
    let head = repository.git(&["rev-parse", "HEAD"]);
    let (code, output) = repository.invoke(&["--exec", "true", &selected]);
    assert_ne!(code, EXIT_OK, "checkout collision must refuse: {output}");
    assert_eq!(output, "");
    assert_eq!(repository.index(), index);
    assert_eq!(repository.git(&["rev-parse", "HEAD"]), head);
    assert_eq!(repository.git(&["rev-parse", "refs/heads/main"]), head);
    assert!(!root.join(".git/factor").exists());
    if alias {
        assert_eq!(
            fs::read(&path).or_abort("preserved initial alias"),
            b"unrelated user bytes\n"
        );
    } else {
        assert_eq!(
            fs::read_link(&path).or_abort("preserved initial link"),
            root.join("missing-user-target")
        );
    }
}

/// Observes real unrelated ancestry through the actual shared CLI dispatcher.
#[expect(
    clippy::literal_string_with_formatting_args,
    reason = "literal Git revision selectors and expected serialized data intentionally contain braces"
)]
pub(in crate::git_factor::engine) fn verify_nonancestor(
    selected_content: &str,
    user_content: &str,
) {
    use crate::test_support::ResultOrAbort as _;

    let (repository, selected) = nonancestor_repository(selected_content, user_content);
    let admin = repository.environment.cwd.join(".git");
    let marker = repository.directory.path().join("external-gate-ran");
    let gate = format!(
        "printf ran > {}",
        shell_quote(marker.to_str().or_abort("owned gate marker UTF-8")),
    );
    let head = repository.git(&["rev-parse", "HEAD"]);
    let tree = repository.git(&["rev-parse", "HEAD^{tree}"]);
    let references = repository.git(&["show-ref"]);
    let worktrees = repository.git(&["worktree", "list", "--porcelain"]);
    let index = repository.index();
    let current =
        fs::read(repository.environment.cwd.join("current")).or_abort("current tracked bytes");
    let base = fs::read(repository.environment.cwd.join("base")).or_abort("base tracked bytes");
    let user = fs::read(repository.environment.cwd.join("user")).or_abort("unrelated user bytes");

    let (code, stdout) = repository.invoke(&["--exec", &gate, &selected]);

    assert_eq!(code, EXIT_DATAERR);
    assert_eq!(stdout, "");
    assert_eq!(
        repository.output.stderr.borrow().as_str(),
        format!("commit {selected} is not an ancestor of HEAD\n"),
    );
    assert_eq!(repository.git(&["rev-parse", "HEAD"]), head);
    assert_eq!(repository.git(&["rev-parse", "HEAD^{tree}"]), tree);
    assert_eq!(repository.git(&["show-ref"]), references);
    assert_eq!(
        repository.git(&["worktree", "list", "--porcelain"]),
        worktrees
    );
    assert_eq!(repository.index(), index);
    assert_eq!(
        fs::read(repository.environment.cwd.join("current")).or_abort("preserved current bytes"),
        current
    );
    assert_eq!(
        fs::read(repository.environment.cwd.join("base")).or_abort("preserved base bytes"),
        base
    );
    assert_eq!(
        fs::read(repository.environment.cwd.join("user")).or_abort("preserved user bytes"),
        user
    );
    assert!(!repository.environment.cwd.join("selected").exists());
    assert!(!marker.exists());
    for name in [
        "factor-journal.json",
        "factor",
        "worktrees",
        "rebase-merge",
        "rebase-apply",
    ] {
        assert_eq!(
            fs::symlink_metadata(admin.join(name))
                .err_or_abort("no native session path")
                .kind(),
            io::ErrorKind::NotFound,
        );
    }
}

/// A contiguous source span is collapsed, while each completed atom survives later rounds.
pub(in crate::git_factor::engine) fn verify_range(root: bool) {
    const TWO_COMMITS: u64 = 2;
    let repository = Repository::new();
    if !root {
        repository.write("base", "base\n");
        repository.commit("Base");
    }
    repository.write("atom", "first selected change\n");
    let first = repository.commit("First selected change");
    repository.write("remainder", "second selected change\n");
    let second = repository.commit("Second selected change");
    repository.write("descendant", "later history\n");
    repository.commit("Descendant");
    let final_tree = repository.git(&["rev-parse", "HEAD^{tree}"]);
    repository.write("unrelated", "preserved range user bytes\n");
    let response = repository.success(&["--exec", "true", &first, &second]);
    assert_eq!(
        response.pointer("/target/commit").or_abort("selected tip"),
        second.as_str()
    );
    assert_eq!(
        response
            .pointer("/target/commit_count")
            .and_then(serde_json::Value::as_u64)
            .or_abort("range count"),
        TWO_COMMITS
    );
    repository.git(&["add", "atom"]);
    repository.success(&["--message", "Extract range atom"]);
    let checkpoint = repository.git(&["rev-parse", "refs/heads/main"]);
    let ancestry = repository.git(&["rev-list", &checkpoint]);
    assert!(
        !ancestry
            .lines()
            .any(|commit| commit == first || commit == second)
    );
    assert_eq!(
        repository.git(&["rev-parse", "refs/heads/main^{tree}"]),
        final_tree
    );
    let atom = repository.git(&[
        "log",
        "--format=%H",
        "--grep=^Extract range atom$",
        &checkpoint,
    ]);
    assert!(!atom.is_empty());
    if root {
        assert_eq!(
            repository.git(&["show", "--format=%P", "--no-patch", &atom]),
            ""
        );
    }
    let complete = repository.success(&["--finish"]);
    assert_eq!(
        complete
            .pointer("/split_count")
            .and_then(serde_json::Value::as_u64)
            .or_abort("completed count"),
        TWO_COMMITS
    );
    assert_eq!(repository.git(&["rev-parse", "HEAD^{tree}"]), final_tree);
    assert!(
        repository
            .git(&["rev-list", "HEAD"])
            .lines()
            .any(|commit| commit == atom)
    );
    assert_eq!(
        fs::read(repository.environment.cwd.join("unrelated")).or_abort("preserved range work"),
        b"preserved range user bytes\n"
    );
}

/// Finish uses the native source body rather than the whitespace-trimming display observer.
pub(in crate::git_factor::engine) fn verify_raw_finish_message(terminal_lfs: usize, body: &str) {
    let (repository, _, expected) = raw_message_fixture(terminal_lfs, body);
    let result = repository.success(&["--finish"]);
    assert_eq!(
        result.get("result").and_then(serde_json::Value::as_str),
        Some("complete")
    );
    assert_eq!(raw_current_message(&repository), expected.as_bytes());
    assert!(!repository.environment.cwd.join(".git/factor").exists());
}

/// Repeated real replay callbacks must preserve the object body despite a positive proof hit.
pub(in crate::git_factor::engine) fn verify_raw_remainder_message(terminal_lfs: usize, body: &str) {
    let (repository, original, expected) = raw_message_fixture(terminal_lfs, body);
    let hook = repository.environment.cwd.join(".git/hooks/commit-msg");
    fs::write(
        &hook,
        "#!/bin/bash\nIFS= read -r title <\"$1\"\nif test \"$title\" = 'Selected source'; then exit 1; fi\n",
    )
    .or_abort("native remainder refusal hook");
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o700)).or_abort("native hook mode");
    repository.git(&["add", "atom"]);
    let (code, _) = repository.invoke(&["--message", "Extract atom"]);
    assert_ne!(code, EXIT_OK, "native remainder hook must pause replay");
    let journal: serde_json::Value =
        serde_json::from_slice(&repository.journal()).or_abort("real replay journal");
    assert_eq!(
        journal
            .pointer("/state/phase")
            .and_then(serde_json::Value::as_str),
        Some("replaying"),
    );
    assert_eq!(raw_current_message(&repository), original.as_bytes());
    fs::remove_file(&hook).or_abort("release native remainder refusal");
    let selected_tree = repository.git(&["rev-parse", "HEAD^{tree}"]);
    let retries: Range<usize> = 0..2;
    for _ in retries {
        let (callback_code, output) = repository.invoke(&["checkpoint-gate-remainder"]);
        assert_eq!(
            callback_code,
            EXIT_OK,
            "{}",
            repository.output.stderr.borrow()
        );
        assert_eq!(output, "");
        assert_eq!(raw_current_message(&repository), expected.as_bytes());
        assert_eq!(repository.git(&["rev-parse", "HEAD^{tree}"]), selected_tree);
    }
    repository.success(&["--abort"]);
}

/// Refusal preserves the completed checkpoint and exact pending native state.
fn verify_readmission_collision(boundary: ReadmissionBoundary, hard_link: bool, contents: &str) {
    let repository = Repository::new();
    repository.write("base", "base\n");
    repository.commit("Base");
    repository.write("atom", "atom\n");
    repository.write("remainder", "remainder\n");
    repository.write("obsolete", "temporary selected path\n");
    let selected = repository.commit("Selected source");
    fs::remove_file(repository.environment.cwd.join("obsolete"))
        .or_abort("descendant cancellation");
    repository.commit("Cancel temporary source path");
    let original_checkpoint = repository.git(&["rev-parse", "refs/heads/main"]);
    let runner = ReadmissionInterruption {
        boundary,
        journal: repository.environment.cwd.join(".git/factor-journal.json"),
        fired: Cell::new(false),
    };
    if matches!(boundary, ReadmissionBoundary::RestartPreparing) {
        repository.success(&["--exec", "true", &selected]);
        repository.git(&["add", "atom"]);
        let (code, _) =
            repository.invoke_with(&["--message", "Capture first atom"], &runner, &REAL_FS);
        assert_ne!(code, EXIT_OK);
        assert_ne!(
            repository.git(&["rev-parse", "refs/heads/main"]),
            original_checkpoint
        );
    } else {
        let (code, _) = repository.invoke_with(&["--exec", "true", &selected], &runner, &REAL_FS);
        assert_ne!(code, EXIT_OK);
    }
    assert!(
        runner.fired.get(),
        "native interruption boundary must be reached"
    );
    let state: serde_json::Value =
        serde_json::from_slice(&repository.journal()).or_abort("actual pending journal");
    assert_eq!(
        state
            .pointer("/state/phase")
            .and_then(serde_json::Value::as_str),
        Some(if matches!(boundary, ReadmissionBoundary::Opening) {
            "opening"
        } else {
            "preparing"
        })
    );
    let foreign = if hard_link {
        "foreign-hard-link"
    } else {
        "obsolete"
    };
    fs::write(
        repository.environment.cwd.join(".git/info/exclude"),
        format!("{foreign}\n"),
    )
    .or_abort("ignored foreign file");
    let path = repository.environment.cwd.join(foreign);
    if hard_link {
        fs::hard_link(repository.environment.cwd.join("base"), &path)
            .or_abort("foreign physical alias");
    } else {
        repository.write(foreign, contents);
    }
    let foreign_bytes = fs::read(&path).or_abort("foreign bytes before refusal");
    let journal = repository.journal();
    let index = repository.index();
    let head = repository.git(&["rev-parse", "HEAD"]);
    let reference = repository.git(&["symbolic-ref", "HEAD"]);
    let refs = repository.git(&["show-ref"]);
    let expected_tree = repository.git(&["rev-parse", "HEAD^{tree}"]);
    let rebase_paths = ["rebase-merge", "rebase-apply"];
    let native_state =
        rebase_paths.map(|name| repository.environment.cwd.join(".git").join(name).exists());
    let (code, stdout) = repository.invoke(&["--continue"]);
    assert_ne!(code, EXIT_OK);
    assert_eq!(stdout, "");
    assert_eq!(repository.journal(), journal);
    assert_eq!(repository.index(), index);
    assert_eq!(repository.git(&["rev-parse", "HEAD"]), head);
    assert_eq!(repository.git(&["symbolic-ref", "HEAD"]), reference);
    assert_eq!(repository.git(&["show-ref"]), refs);
    assert_eq!(repository.git(&["rev-parse", "HEAD^{tree}"]), expected_tree);
    assert_eq!(
        rebase_paths.map(|name| repository.environment.cwd.join(".git").join(name).exists()),
        native_state
    );
    assert_eq!(
        fs::read(&path).or_abort("preserved foreign bytes"),
        foreign_bytes
    );
    fs::remove_file(&path).or_abort("remove only owned collision fixture");
    repository.success(&["--continue"]);
    repository.success(&["--abort"]);
}

/// A selected deletion does not authorize deleting a recreated user file outside cwd.
#[expect(
    clippy::single_call_fn,
    reason = "named native fixture boundaries keep ownership, stimulus, and conservation visible to the direct canonical provider"
)]
pub(in crate::git_factor::engine) fn verify_recreated_deleted_path() {
    let mut repository = Repository::new();
    let root = repository.environment.cwd.clone();
    fs::create_dir_all(root.join("child")).or_abort("owned child directory");
    repository.write("outside.txt", "original tracked bytes\n");
    repository.write("child/inside.txt", "original inside\n");
    repository.commit("Base");
    fs::remove_file(root.join("outside.txt")).or_abort("selected deletion");
    repository.write("child/inside.txt", "selected inside\n");
    repository.commit("Delete outside and change inside");
    repository.environment.cwd = root.join("child");
    repository.success(&["--exec", "true", "HEAD"]);
    repository.git(&["add", "inside.txt"]);
    fs::write(
        root.join("outside.txt"),
        b"unrelated recreated user bytes\n",
    )
    .or_abort("user recreation");
    let index = fs::read(root.join(".git/index")).or_abort("admitted real index");
    let journal = fs::read(root.join(".git/factor-journal.json")).or_abort("actual journal");
    let head = repository.git(&["rev-parse", "HEAD"]);
    let branch = repository.git(&["rev-parse", "refs/heads/main"]);
    let lease = repository.git(&["rev-parse", "refs/factor/session-lease"]);
    let (code, output) = repository.invoke(&["--message", "Extract inside atom"]);
    assert_ne!(
        code, EXIT_OK,
        "must refuse recreated unrelated path: {output}"
    );
    assert_eq!(output, "");
    assert_eq!(
        fs::read(root.join("outside.txt")).or_abort("preserved recreated bytes"),
        b"unrelated recreated user bytes\n"
    );
    assert_eq!(
        fs::read(root.join(".git/index")).or_abort("preserved raw index"),
        index
    );
    assert_eq!(
        fs::read(root.join(".git/factor-journal.json")).or_abort("preserved journal"),
        journal
    );
    assert_eq!(repository.git(&["rev-parse", "HEAD"]), head);
    assert_eq!(repository.git(&["rev-parse", "refs/heads/main"]), branch);
    assert_eq!(
        repository.git(&["rev-parse", "refs/factor/session-lease"]),
        lease
    );
}

/// Native invariants through the shipped shared CLI dispatch.
pub(in crate::git_factor::engine) fn verify_round(root: bool, message: &str) {
    let repository = Repository::new();
    if !root {
        repository.write("base", "base\n");
        repository.commit("Base");
    }
    repository.write("atom", "selected atom\n");
    repository.write("remainder", "selected remainder\n");
    let target = repository.commit("Combined selected change");
    repository.write("descendant", "later history\n");
    repository.commit("Descendant");
    let original_tree = repository.git(&["rev-parse", "HEAD^{tree}"]);
    repository.write("unrelated", "preserve user bytes\n");
    let command_path = repository.environment.cwd.join(".git/round-command");
    fs::write(&command_path, b"true").or_abort("exact command bytes");
    let command_hash = repository.git(&[
        "hash-object",
        "--no-filters",
        command_path.to_str().or_abort("fixture command path"),
    ]);
    repository.success(&["--exec", "true", &target]);
    repository.git(&["add", "atom"]);
    let atom_tree = repository.git(&["write-tree"]);
    let expected_message =
        format!("{message}\n\nGate-exec-{command_hash}:\n {command_hash}\n {atom_tree}\n");
    repository.success(&["--message", message]);
    let captured_atom = repository.git(&["rev-parse", "HEAD"]);
    assert_eq!(
        raw_current_message(&repository),
        expected_message.as_bytes()
    );
    let checkpoint = repository.git(&["rev-parse", "refs/heads/main"]);
    assert_eq!(
        repository.git(&["rev-parse", "refs/heads/main^{tree}"]),
        original_tree
    );
    assert_eq!(repository.git(&["diff", "--name-only"]), "");
    if !root {
        let saved_index = repository.index();
        let saved_journal = repository.journal();
        repository.write("base", "foreign user edit\n");
        let (code, _) = repository.invoke(&["--abort"]);
        assert_ne!(code, EXIT_OK);
        assert_eq!(repository.index(), saved_index);
        assert_eq!(repository.journal(), saved_journal);
        assert_eq!(
            fs::read(repository.environment.cwd.join("base")).or_abort("preserved tracked file"),
            b"foreign user edit\n"
        );
        repository.write("base", "base\n");
    }
    repository.success(&["--abort"]);
    assert_eq!(repository.git(&["rev-parse", "HEAD"]), checkpoint);
    assert_eq!(repository.git(&["symbolic-ref", "HEAD"]), "refs/heads/main");
    assert_eq!(repository.git(&["rev-parse", "HEAD^{tree}"]), original_tree);
    assert!(!repository.environment.cwd.join(".git/factor").exists());
    assert_eq!(
        fs::read(repository.environment.cwd.join("unrelated")).or_abort("preserved untracked file"),
        b"preserve user bytes\n"
    );
    if root {
        let atom = repository.git(&["rev-list", "--max-parents=0", "HEAD"]);
        assert_eq!(atom, captured_atom);
    }
    assert!(repository.directory.path().exists());
}

pub(in crate::git_factor::engine) fn verify_subdirectory_admission(hidden_edit: bool) {
    let mut repository = Repository::new();
    repository.write("base", "base\n");
    repository.commit("Base");
    repository.write("atom", "atom\n");
    repository.commit("Selected");
    let root = repository.environment.cwd.clone();
    fs::create_dir_all(root.join("child")).or_abort("child directory");
    if hidden_edit {
        repository.git(&["update-index", "--assume-unchanged", "base"]);
        repository.write("base", "hidden user change\n");
    }
    let head = repository.git(&["rev-parse", "HEAD"]);
    let index = repository.index();
    repository.environment.cwd = root.join("child");
    let (code, stdout) = repository.invoke(&["--exec", "true", "HEAD"]);
    if hidden_edit {
        assert_ne!(code, EXIT_OK);
        assert_eq!(stdout, "");
        assert!(!root.join(".git/factor").exists());
        assert_eq!(fs::read(root.join(".git/index")).or_abort("index"), index);
        assert_eq!(repository.git(&["rev-parse", "HEAD"]), head);
        assert_eq!(
            fs::read(root.join("base")).or_abort("user bytes"),
            b"hidden user change\n"
        );
    } else {
        assert_eq!(code, EXIT_OK, "{}", repository.output.stderr.borrow());
        let result: serde_json::Value = serde_json::from_str(&stdout).or_abort("selection JSON");
        assert_eq!(
            result.pointer("/operation").or_abort("required JSON fact"),
            "start"
        );
        repository.success(&["--abort"]);
        assert_eq!(repository.git(&["rev-parse", "HEAD"]), head);
    }
}

pub(in crate::git_factor::engine) fn verify_unknown_internal_command(suffix: &str) {
    let repository = Repository::new();
    repository.write("base", "base\n");
    repository.commit("Base");
    let head = repository.git(&["rev-parse", "HEAD"]);
    let index = repository.index();
    let command = format!("checkpoint-unknown-{suffix}");
    let (code, output) = repository.invoke(&[&command]);
    assert_eq!(code, EXIT_SOFTWARE);
    assert_eq!(output, "");
    assert!(
        repository
            .output
            .stderr
            .borrow()
            .contains("unknown internal checkpoint command")
    );
    assert_eq!(repository.index(), index);
    assert_eq!(repository.git(&["rev-parse", "HEAD"]), head);
    assert!(!repository.environment.cwd.join(".git/factor").exists());
}
fn version_boundaries(internal: bool) {
    for (text, supported) in [
        ("git version 2.55.9", false),
        ("git version 2.56.0", true),
        ("git version 2.56.1", true),
        ("git version 3.0.0", true),
        ("git version 2.56.0.rc1", false),
        ("git version 2.56.0.dev", false),
        ("git version +2.56.0", false),
        ("git version 2.56", false),
        ("malformed", false),
    ] {
        verify_git_version_admission(internal, text, supported, VersionFailure::None);
    }
    for failure in [VersionFailure::Query, VersionFailure::Spawn] {
        verify_git_version_admission(internal, "git version 2.56.0", false, failure);
    }
}
/// Exercises version admission through the real dispatcher with a known-invalid observation.
pub(in crate::git_factor::engine) fn verify_malformed_git_version(
    internal: bool,
    malformed: MalformedGitVersion,
    value: u16,
) {
    verify_git_version_admission(
        internal,
        &malformed.observation(value),
        false,
        VersionFailure::None,
    );
}
