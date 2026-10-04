mod fs {

    mod symlink_metadata {
        use crate::git_factor::ctx::{Fs as _, REAL_FS};
        use crate::test_support::OrAbort as _;
        use proptest::prelude::*;
        use std::fs;
        use std::io;
        use std::os::unix::fs::symlink;
        #[derive(Clone, Copy, Debug)]
        enum PhysicalKind {
            Directory,
            File,
            Link,
            Missing,
        }
        proptest! {
            #[test]
            fn observes_generated_physical_kinds_without_following_links(
                kind in prop::sample::select(vec![PhysicalKind::Directory, PhysicalKind::File, PhysicalKind::Link, PhysicalKind::Missing]), contents in ".{0,40}",
            ) {
                let root = tempfile::tempdir().or_abort("metadata fixture");
                let path = root.path().join("observed");
                match kind {
                    PhysicalKind::File => fs::write(&path, &contents).or_abort("file"),
                    PhysicalKind::Directory => fs::create_dir_all(&path).or_abort("directory"),
                    PhysicalKind::Link => symlink(root.path(), &path).or_abort("link"),
                    PhysicalKind::Missing => (),
                }
                let expected = match kind {
                    PhysicalKind::File => Ok((true, false, false)),
                    PhysicalKind::Directory => Ok((false, true, false)),
                    PhysicalKind::Link => Ok((false, false, true)),
                    PhysicalKind::Missing => Err(io::ErrorKind::NotFound),
                };
                let actual = REAL_FS.symlink_metadata(&path)
                    .map(|metadata| (metadata.is_file(), metadata.is_dir(), metadata.is_symlink()))
                    .map_err(|error| error.kind());
                prop_assert_eq!(actual, expected);
                if matches!(kind, PhysicalKind::File) { prop_assert_eq!(fs::read_to_string(&path).or_abort("file readback"), contents); }
                if matches!(kind, PhysicalKind::Link) { prop_assert_eq!(fs::read_link(&path).or_abort("link readback"), root.path()); }
            }
        }
    }
    mod write_atomic_string {
        use crate::git_factor::ctx::{Fs as _, REAL_FS};
        use crate::test_support::OrAbort as _;
        use proptest::prelude::*;
        use std::fs;
        proptest! {
            #[test]
            fn publishes_generated_unicode_and_preserves_foreign_bytes(content in any::<String>(), old in any::<String>(), replace in any::<bool>()) {
                let root = tempfile::tempdir().or_abort("publication fixture");
                let journal = root.path().join("journal");
                let foreign = root.path().join("foreign");
                fs::write(&foreign, old.as_bytes()).or_abort("foreign bytes");
                if replace { fs::write(&journal, b"previous journal").or_abort("old journal"); }
                let actual = REAL_FS.write_atomic_string(&journal, &content);
                actual.or_abort("publication");
                prop_assert_eq!(fs::read(&journal).or_abort("journal"), content.as_bytes());
                prop_assert_eq!(fs::read(&foreign).or_abort("foreign"), old.as_bytes());
                prop_assert_eq!(fs::read_dir(root.path()).or_abort("inventory").count(), 2);
            }
        }
    }
    mod remove_atomic_file {
        use crate::git_factor::ctx::{Fs as _, REAL_FS};
        use crate::test_support::OrAbort as _;
        use proptest::prelude::*;
        use std::ffi::OsString;
        use std::fs;
        use std::io;
        use tempfile::TempDir;

        proptest! {
            #[test]
            #[expect(clippy::create_dir, reason = "the fixture requires exclusive creation of one legacy directory whose bytes must survive the journal unlink")]
            fn exact_unlink_preserves_generated_foreign_and_legacy_bytes(
                stem in "[a-z]{1,12}",
                journal_bytes in prop::collection::vec(any::<u8>(), 0..128),
                foreign_bytes in prop::collection::vec(any::<u8>(), 0..128),
                present in any::<bool>(),
            ) {
                let directory = TempDir::new().or_abort("owned generated native directory");
                let journal = directory.path().join(format!("{stem}-journal"));
                let foreign = directory.path().join(format!("{stem}-foreign"));
                let legacy = directory.path().join("factor");
                fs::create_dir(&legacy).or_abort("legacy directory");
                fs::write(legacy.join("legacy"), &foreign_bytes).or_abort("legacy bytes");
                fs::write(&foreign, &foreign_bytes).or_abort("foreign bytes");
                if present {
                    fs::write(&journal, &journal_bytes).or_abort("journal bytes");
                }
                let expected = if present { Ok(()) } else { Err(io::ErrorKind::NotFound) };

                let actual = REAL_FS.remove_atomic_file(&journal).map_err(|error| error.kind());

                prop_assert_eq!(actual, expected);
                prop_assert!(fs::symlink_metadata(&journal).is_err());
                let observed_foreign = fs::read(&foreign).or_abort("foreign readback");
                prop_assert_eq!(observed_foreign.as_slice(), foreign_bytes.as_slice());
                let observed_legacy = fs::read(legacy.join("legacy")).or_abort("legacy readback");
                prop_assert_eq!(observed_legacy.as_slice(), foreign_bytes.as_slice());
                let mut names = fs::read_dir(directory.path()).or_abort("native inventory")
                    .map(|entry| entry.or_abort("native directory entry").file_name())
                    .collect::<Vec<_>>();
                names.sort();
                let mut expected_names: Vec<OsString> = vec![
                    "factor".into(),
                    format!("{stem}-foreign").into(),
                ];
                expected_names.sort();
                prop_assert_eq!(names, expected_names);
            }
        }
    }
}
