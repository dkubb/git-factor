mod fs {

    mod symlink_metadata {
        use crate::git_factor::ctx::{Fs as _, REAL_FS};
        use crate::test_support::OrAbort as _;
        use std::fs;
        use std::os::unix::fs::symlink;
        #[test]
        fn observes_the_link_without_following_its_directory_target() {
            let root = tempfile::tempdir().or_abort("metadata fixture");
            let link = root.path().join("link");
            symlink(root.path(), &link).or_abort("directory link");
            let actual = REAL_FS.symlink_metadata(&link).or_abort("metadata");
            assert!(actual.is_symlink());
            assert!(!actual.is_dir());
            assert!(!actual.is_file());
            assert_eq!(fs::read_link(&link).or_abort("link readback"), root.path());
        }
    }
    mod write_atomic_string {
        use crate::git_factor::ctx::{Fs as _, REAL_FS};
        use crate::test_support::OrAbort as _;
        use std::fs;
        use std::io;
        use std::path::Path;
        #[test]
        fn replaces_only_the_requested_file_and_leaves_no_temporary_file() {
            let root = tempfile::tempdir().or_abort("publication fixture");
            let journal = root.path().join("journal");
            let foreign = root.path().join("foreign");
            fs::write(&journal, b"old journal").or_abort("old journal");
            fs::write(&foreign, b"foreign bytes").or_abort("foreign bytes");
            let actual = REAL_FS.write_atomic_string(&journal, "new journal\n");
            actual.or_abort("atomic publication");
            assert_eq!(fs::read(&journal).or_abort("journal"), b"new journal\n");
            assert_eq!(fs::read(&foreign).or_abort("foreign"), b"foreign bytes");
            assert_eq!(fs::read_dir(root.path()).or_abort("inventory").count(), 2);
        }
        #[test]
        fn refuses_a_parentless_target_before_io() {
            let actual = REAL_FS.write_atomic_string(Path::new(""), "journal");
            let error = actual.err().or_abort("parentless refusal");
            assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
            assert_eq!(error.to_string(), "journal path lacks parent");
        }
    }
    mod remove_atomic_file {
        use crate::git_factor::ctx::{Fs as _, REAL_FS};
        use crate::test_support::OrAbort as _;
        use std::fs;
        use std::io;
        #[cfg(unix)]
        use std::os::unix::fs::symlink;
        use std::path::Path;
        use tempfile::TempDir;

        #[test]
        #[expect(
            clippy::create_dir,
            reason = "the fixture requires exclusive creation of one legacy directory whose bytes must survive the journal unlink"
        )]
        fn removes_only_exact_journal_and_preserves_legacy_storage() {
            let directory = TempDir::new().or_abort("owned native directory");
            let journal = directory.path().join("factor-journal.json");
            let foreign = directory.path().join("foreign");
            let legacy = directory.path().join("factor");
            fs::create_dir(&legacy).or_abort("foreign legacy directory");
            fs::write(legacy.join("legacy"), b"legacy bytes\n").or_abort("legacy bytes");
            fs::write(&foreign, b"foreign bytes\n").or_abort("foreign bytes");
            fs::write(&journal, b"durable journal bytes\n").or_abort("journal bytes");

            let actual = REAL_FS.remove_atomic_file(&journal);

            actual.or_abort("native exact unlink and parent sync");
            assert!(!journal.exists());
            assert_eq!(
                fs::read(&foreign).or_abort("foreign readback"),
                b"foreign bytes\n"
            );
            assert_eq!(
                fs::read(legacy.join("legacy")).or_abort("legacy readback"),
                b"legacy bytes\n"
            );
        }

        #[test]
        fn rejects_parentless_path_before_io() {
            let actual = REAL_FS.remove_atomic_file(Path::new(""));

            let error = actual.err().or_abort("parentless path must refuse");
            assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
            assert_eq!(error.to_string(), "journal path lacks parent");
        }

        #[test]
        fn missing_journal_refuses_and_preserves_foreign_bytes() {
            let directory = TempDir::new().or_abort("owned native directory");
            let journal = directory.path().join("factor-journal.json");
            let foreign = directory.path().join("foreign");
            fs::write(&foreign, b"foreign bytes\n").or_abort("foreign bytes");

            let actual = REAL_FS.remove_atomic_file(&journal);

            assert_eq!(
                actual.err().or_abort("missing journal must refuse").kind(),
                io::ErrorKind::NotFound
            );
            assert!(!journal.exists());
            assert_eq!(
                fs::read(&foreign).or_abort("foreign readback"),
                b"foreign bytes\n"
            );
        }

        #[cfg(unix)]
        #[test]
        fn unlinks_a_journal_link_without_following_its_target() {
            let directory = TempDir::new().or_abort("owned native directory");
            let journal = directory.path().join("factor-journal.json");
            let target = directory.path().join("foreign-target");
            let dangling = directory.path().join("foreign-dangling");
            fs::write(&target, b"foreign target bytes\n").or_abort("foreign target bytes");
            symlink(&target, &journal).or_abort("journal symlink");
            symlink("missing-foreign-target", &dangling).or_abort("foreign dangling symlink");

            let actual = REAL_FS.remove_atomic_file(&journal);

            actual.or_abort("native unlink and parent sync");
            drop(
                fs::symlink_metadata(&journal)
                    .err()
                    .or_abort("journal object must be absent"),
            );
            assert_eq!(
                fs::read(&target).or_abort("foreign target readback"),
                b"foreign target bytes\n"
            );
            assert_eq!(
                fs::read_link(&dangling).or_abort("foreign dangling readback"),
                Path::new("missing-foreign-target")
            );
        }
    }
}
