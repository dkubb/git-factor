mod main_entry {
    use super::super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn refuses_generated_missing_parents(
            name in "[a-z]{1,8}",
        ) {
            let (_guard, directory, path) = atomic_write_fixture(&format!("{name}/todo"));
            let error = write_file_atomic(&path, "replacement").err_or_abort("refuse missing parent");
            prop_assert!(
                matches!(error, AtomicWriteError::CreateTemp(reported, _) if reported == path.display().to_string())
            );
            prop_assert!(
                fs::read_dir(directory.path())
                    .or_abort("observe parent directory")
                    .next()
                    .is_none()
            );
        }

        #[test]
        fn cleans_temporary_files_after_generated_rename_failures(
            name in "[a-z]{1,8}",
        ) {
            let (_guard, directory, path) = atomic_write_fixture(&name);
            fs::create_dir_all(&path).or_abort("directory obstruction");
            let error = write_file_atomic(&path, "replacement").err_or_abort("refuse directory target");
            prop_assert!(
                matches!(error, AtomicWriteError::ReplaceFile(reported, _) if reported == path.display().to_string())
            );
            prop_assert!(path.is_dir());
            let mut entries = fs::read_dir(directory.path())
                .or_abort("observe parent directory")
                .map(|entry| entry.or_abort("observe entry").file_name())
                .collect::<Vec<_>>();
            entries.sort();
            prop_assert_eq!(entries, vec![OsString::from(name)]);
        }

        #[test]
        fn preserves_generated_targets_and_reserved_slots(
            name in "[a-z]{1,8}",
            slots in u32::MIN..5,
            content in any::<String>(),
        ) {
            let (_guard, directory, path) = atomic_write_fixture(&name);
            fs::write(&path, "original").or_abort("original todo");
            let mut expected = vec![OsString::from(&name)];
            let pid = process::id();
            for attempt in 0..slots {
                let reserved = format!("{name}.tmp{pid}.{attempt}");
                fs::write(directory.path().join(&reserved), "reserved").or_abort("occupied temporary slot");
                expected.push(OsString::from(reserved));
            }
            expected.sort();
            write_file_atomic(&path, &content).or_abort("atomic replacement");
            prop_assert_eq!(
                fs::read_to_string(&path).or_abort("observe replacement"),
                content
            );
            for attempt in 0..slots {
                prop_assert_eq!(
                    fs::read_to_string(directory.path().join(format!("{name}.tmp{pid}.{attempt}")))
                        .or_abort("observe occupied slot"),
                    "reserved"
                );
            }
            let mut entries = fs::read_dir(directory.path())
                .or_abort("observe parent directory")
                .map(|entry| entry.or_abort("observe entry").file_name())
                .collect::<Vec<_>>();
            entries.sort();
            prop_assert_eq!(entries, expected);
        }

        #[test]
        fn cleans_temporary_files_after_generated_write_or_sync_failures(
            name in "[a-z]{1,8}",
            sync in any::<bool>(),
        ) {
            let (_guard, directory, path) = atomic_write_fixture(&name);
            fs::write(&path, "original").or_abort("original todo");
            let fault = if sync {
                WriteFailPoint::Sync
            } else {
                WriteFailPoint::Write
            };
            set_write_fail_point(fault);
            let result = write_file_atomic(&path, "replacement");
            set_write_fail_point(WriteFailPoint::None);
            let error = result.err_or_abort("injected atomic write failure");
            prop_assert!(
                matches!((fault, &error), (WriteFailPoint::Write, AtomicWriteError::WriteTemp(reported, _)) | (WriteFailPoint::Sync, AtomicWriteError::SyncTemp(reported, _)) if reported == &path.display().to_string()),
                "unexpected error: {error}"
            );
            prop_assert_eq!(
                fs::read_to_string(&path).or_abort("observe todo"),
                "original"
            );
            let mut entries = fs::read_dir(directory.path())
                .or_abort("observe parent directory")
                .map(|entry| entry.or_abort("observe entry").file_name())
                .collect::<Vec<_>>();
            entries.sort();
            prop_assert_eq!(entries, vec![OsString::from(name)]);
        }

        #[cfg(unix)]
        #[test]
        fn reports_generated_parent_failures_after_replacement(
            name in "[a-z]{1,8}",
            sync in any::<bool>(),
            content in any::<String>(),
        ) {
            let (_guard, directory, path) = atomic_write_fixture(&name);
            fs::write(&path, "original").or_abort("original todo");
            let fault = if sync {
                WriteFailPoint::SyncParentDir
            } else {
                WriteFailPoint::OpenParentDir
            };
            set_write_fail_point(fault);
            let result = write_file_atomic(&path, &content);
            set_write_fail_point(WriteFailPoint::None);
            let error = result.err_or_abort("injected parent directory failure");
            prop_assert!(
                matches!((fault, &error), (WriteFailPoint::OpenParentDir, AtomicWriteError::OpenParentDir(reported, _)) | (WriteFailPoint::SyncParentDir, AtomicWriteError::SyncParentDir(reported, _)) if reported == &path.display().to_string()),
                "unexpected error: {error}"
            );
            prop_assert_eq!(fs::read_to_string(&path).or_abort("observe todo"), content);
            let mut entries = fs::read_dir(directory.path())
                .or_abort("observe parent directory")
                .map(|entry| entry.or_abort("observe entry").file_name())
                .collect::<Vec<_>>();
            entries.sort();
            prop_assert_eq!(entries, vec![OsString::from(name)]);
        }
    }

    // Each class must cover the two path refusals and exhaustion return.
    // These fixed boundaries run once per class alongside genuine generated inputs.
    #[test]
    fn refuses_empty_path() {
        let error =
            write_file_atomic(Path::new(""), "replacement").err_or_abort("refuse missing parent");
        assert_eq!(
            error.to_string(),
            "failed to determine parent directory for: "
        );
        assert!(matches!(error, AtomicWriteError::MissingParent(reported) if reported.is_empty()));
    }

    #[test]
    fn refuses_path_without_file_name() {
        let error = write_file_atomic(Path::new("."), "replacement")
            .err_or_abort("refuse missing filename");
        assert_eq!(error.to_string(), "failed to determine file name for: .");
        assert!(matches!(error, AtomicWriteError::MissingFileName(reported) if reported == "."));
    }

    #[test]
    fn refuses_exhausted_temporary_slots() {
        let (_guard, directory, path) = atomic_write_fixture("todo");
        let pid = process::id();
        let mut expected = Vec::new();
        for attempt in 0..TEMP_FILE_ATTEMPTS_MAX {
            let name = format!("todo.tmp{pid}.{attempt}");
            fs::write(directory.path().join(&name), "reserved").or_abort("occupied temporary slot");
            expected.push(OsString::from(name));
        }
        expected.sort();
        let error = write_file_atomic(&path, "replacement")
            .err_or_abort("refuse exhausted temporary slots");
        assert!(
            matches!(error, AtomicWriteError::TempNameExhausted(reported) if reported == path.display().to_string())
        );
        assert!(!path.exists());
        let mut entries = fs::read_dir(directory.path())
            .or_abort("observe parent directory")
            .map(|entry| entry.or_abort("observe entry").file_name())
            .collect::<Vec<_>>();
        entries.sort();
        assert_eq!(entries, expected);
    }
}
