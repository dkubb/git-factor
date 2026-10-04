mod is_mid_rebase_in {
    use super::super::{
        fixture::{Fixture, Marker},
        is_mid_rebase_in,
    };
    use crate::test_support::OrAbort as _;
    use std::fs;
    #[test]
    fn detects_actual_merge_directory_without_mutating_the_repository() {
        let marker = Marker::Merge;
        let fixture = Fixture::new(marker);
        let admin = fixture.directory().join(".git");
        let before_head = fs::read(admin.join("HEAD")).ok();
        let before_marker = fs::read(admin.join("rebase-merge")).ok();
        let expected = marker.expected();
        let actual = is_mid_rebase_in(&fixture.context());
        assert_eq!(actual, expected);
        assert_eq!(fs::read(admin.join("HEAD")).ok(), before_head);
        assert_eq!(fs::read(admin.join("rebase-merge")).ok(), before_marker);
        assert_eq!(
            fs::read(fixture.directory().join("user")).or_abort("user bytes"),
            b"protected user bytes".to_vec()
        );
        assert!(!admin.join("index").exists());
    }
}
