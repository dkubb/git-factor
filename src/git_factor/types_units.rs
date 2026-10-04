// Direct observers for checkpoint identity representations.
mod branch_ref {
    mod as_str {
        #[test]
        fn preserves_native_reference_spelling() {
            use crate::test_support::OrAbort as _;
            let reference =
                super::super::super::BranchRef::new("refs/heads/topic\u{a0}".to_owned())
                    .or_abort("branch");
            let actual = reference.as_str();
            assert_eq!(actual, "refs/heads/topic\u{a0}");
        }
    }
    mod new {
        #[test]
        fn refuses_non_branch_reference() {
            let actual = super::super::super::BranchRef::new("refs/tags/topic".to_owned());
            assert!(
                matches!(actual, Err(super::super::super::FactorError::GitCommand(message)) if message.as_str() == "invalid checkpoint branch")
            );
        }
    }
}
mod commit_sha {
    mod as_ref {
        mod str {
            #[test]
            fn borrows_exact_native_hash() {
                use crate::test_support::OrAbort as _;
                let input = "Ab".repeat(20);
                let hash =
                    super::super::super::super::CommitSha::new(input.clone()).or_abort("commit");
                let actual: &str = hash.as_ref();
                assert_eq!(actual, input);
            }
        }
    }
}
mod tree_hash {
    mod as_ref {
        mod str {
            #[test]
            fn borrows_exact_native_hash() {
                use crate::test_support::OrAbort as _;
                let input = "bC".repeat(20);
                let hash = super::super::super::super::TreeHash::new(&input).or_abort("tree");
                let actual: &str = hash.as_ref();
                assert_eq!(actual, input);
            }
        }
    }
}
mod session_id {
    mod new {
        #[test]
        fn refuses_non_object_identity() {
            let actual = super::super::super::SessionId::new("session".to_owned());
            assert!(
                matches!(actual, Err(super::super::super::FactorError::GitCommand(message)) if message.as_str() == "invalid session identity")
            );
        }
    }
    mod fmt {
        mod display {
            mod fmt {
                #[test]
                fn displays_full_identity_without_normalization() {
                    use crate::test_support::OrAbort as _;
                    let input = "Bc".repeat(20);
                    let session = super::super::super::super::super::SessionId::new(input.clone())
                        .or_abort("session");
                    let actual = session.to_string();
                    assert_eq!(actual, input);
                }
            }
        }
    }
}
mod sha {
    mod fmt {
        mod display {
            mod fmt {
                #[test]
                fn displays_full_object_identity() {
                    use crate::test_support::OrAbort as _;
                    let input = "Cd".repeat(20);
                    let hash = super::super::super::super::super::Sha::parse(input.clone())
                        .or_abort("hash");
                    let actual = hash.to_string();
                    assert_eq!(actual, input);
                }
            }
        }
    }
    mod serde {
        mod serialize {
            #[test]
            fn emits_a_json_string_without_wrapper_fields() {
                use crate::test_support::OrAbort as _;
                let input = "De".repeat(20);
                let hash = super::super::super::super::Sha::parse(input.clone()).or_abort("hash");
                let actual = serde_json::to_value(&hash).or_abort("serialize");
                assert_eq!(actual, serde_json::Value::String(input));
            }
        }
    }
}
mod state_dir {
    mod new {
        #[test]
        fn preserves_relative_space_bearing_path() {
            use std::path::PathBuf;
            let input = PathBuf::from("relative/state ");
            let actual = super::super::super::StateDir::new(input.clone());
            assert_eq!(actual.as_path(), input);
        }
    }
}
