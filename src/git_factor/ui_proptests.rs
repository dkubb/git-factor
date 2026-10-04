mod is_mid_rebase_in {
    use super::super::{
        fixture::{Fixture, Marker},
        is_mid_rebase_in,
    };
    use crate::test_support::OrAbort as _;
    use proptest::prelude::*;
    use std::fs;
    proptest! {
        #[test]
        fn detects_generated_physical_markers_and_refuses_non_repository_inputs(marker in prop::sample::select(vec![Marker::Apply, Marker::Both, Marker::File, Marker::Merge, Marker::Missing, Marker::NotRepository])) {
        let fixture = Fixture::new(marker);
        let admin = fixture.directory().join(".git");
        let before_head = fs::read(admin.join("HEAD")).ok();
        let before_marker = fs::read(admin.join("rebase-merge")).ok();
        let expected = marker.expected();
        let actual = is_mid_rebase_in(&fixture.context());
        prop_assert_eq!(actual, expected);
        prop_assert_eq!(fs::read(admin.join("HEAD")).ok(), before_head);
        prop_assert_eq!(fs::read(admin.join("rebase-merge")).ok(), before_marker);
        prop_assert_eq!(fs::read(fixture.directory().join("user")).or_abort("user bytes"), b"protected user bytes".to_vec());
        prop_assert!(!admin.join("index").exists());
        }
    }
}
