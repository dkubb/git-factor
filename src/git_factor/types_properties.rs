mod branch_ref {
    mod as_str {
        use super::super::*;
        proptest! {
            #[test]
            fn preserves_generated_reference_spelling(name in "[A-Za-z][A-Za-z0-9_-]{0,30}", suffix in prop::sample::select(vec!["", "\u{a0}"])) {
                let input = format!("refs/heads/{name}{suffix}");
                let reference = BranchRef::new(input.clone()).or_abort("branch");
                let actual = reference.as_str();
                prop_assert_eq!(actual, input);
            }
        }
    }
    mod new {
        use super::super::*;
        proptest! {
            #[test]
            fn rejects_native_ref_grammar_violations(name in "[A-Za-z][A-Za-z0-9]{0,16}", defect in prop::sample::select(vec!["..", "@{", ".lock", " ", "?", "\\"])) {
                let input = format!("refs/heads/{name}{defect}");
                let actual = BranchRef::new(input);
                prop_assert!(matches!(actual, Err(FactorError::GitCommand(message)) if message.as_str() == "invalid checkpoint branch"));
            }
        }
    }
}
mod commit_sha {
    mod as_ref {
        mod str {
            use super::super::super::*;
            proptest! {
                #[test]
                fn preserves_generated_native_hash(input in "[0-9a-fA-F]{40}") {
                    let hash = CommitSha::new(input.clone()).or_abort("hash");
                    let actual: &str = hash.as_ref();
                    prop_assert_eq!(actual, input);
                }
            }
        }
    }
}
mod tree_hash {
    mod as_ref {
        mod str {
            use super::super::super::*;
            proptest! {
                #[test]
                fn preserves_generated_native_hash(input in "[0-9a-fA-F]{40}") {
                    let hash = TreeHash::new(&input).or_abort("hash");
                    let actual: &str = hash.as_ref();
                    prop_assert_eq!(actual, input);
                }
            }
        }
    }
}
mod session_id {
    mod new {
        use super::super::*;
        proptest! {
            #[test]
            fn rejects_non_object_tokens(input in "[g-zG-Z]{1,50}") {
                let actual = SessionId::new(input);
                prop_assert!(matches!(actual, Err(FactorError::GitCommand(message)) if message.as_str() == "invalid session identity"));
            }
        }
    }
    mod fmt {
        mod display {
            mod fmt {
                use super::super::super::super::*;
                proptest! {
                    #[test]
                    fn preserves_generated_identity(input in "[0-9a-fA-F]{40}") {
                        let session = SessionId::new(input.clone()).or_abort("session");
                        let actual = session.to_string();
                        prop_assert_eq!(actual, input);
                    }
                }
            }
        }
    }
}
mod sha {
    mod fmt {
        mod display {
            mod fmt {
                use super::super::super::super::*;
                proptest! {
                    #[test]
                    fn preserves_generated_object_identity(input in "[0-9a-fA-F]{40}") {
                        let hash = Sha::parse(input.clone()).or_abort("hash");
                        let actual = hash.to_string();
                        prop_assert_eq!(actual, input);
                    }
                }
            }
        }
    }
    mod serde {
        mod serialize {
            use super::super::super::*;
            proptest! {
                #[test]
                fn emits_generated_identity_as_json_string(input in "[0-9a-fA-F]{40}") {
                    let hash = Sha::parse(input.clone()).or_abort("hash");
                    let actual = serde_json::to_value(&hash).or_abort("serialize");
                    prop_assert_eq!(actual, serde_json::Value::String(input));
                }
            }
        }
    }
}
mod state_dir {
    mod new {
        use super::super::*;
        proptest! {
            #[test]
            fn preserves_generated_path_components(component in "[A-Za-z0-9 _.-]{1,32}", absolute in any::<bool>()) {
                let input = if absolute { PathBuf::from("/").join(component) } else { PathBuf::from(component) };
                let actual = StateDir::new(input.clone());
                prop_assert_eq!(actual.as_path(), input);
            }
        }
    }
}

use super::{BranchRef, CommitSha, FactorError, SessionId, Sha, StateDir, TreeHash};
use crate::test_support::OrAbort as _;
use proptest::prelude::*;
use std::path::PathBuf;
