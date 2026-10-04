mod bind {
    use proptest::prelude::*;
    proptest! {
        #[test]
        fn specifications_round_trip_exactly_with_rebound_identity(name in "[A-Za-z][A-Za-z0-9-]{0,16}", text in "[A-Za-z][A-Za-z0-9 $;]{0,20}") {
            super::super::tests::check_spec_round_trip(&name, &text);
        }
    }
}

mod gate {
    mod command {
        use super::super::super::named;
        use super::super::super::tests::{command, ctx, fixture};
        use crate::test_support::OrAbort as _;
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn preserves_generated_command_bytes(text in "[A-Za-z][A-Za-z0-9 $;]{0,128}") {
                let directory = fixture();
                let context = ctx(directory.path());
                let gate = named(&context, "test", command(&text)).or_abort("gate");

                let observed = gate.command();

                prop_assert_eq!(observed.as_str(), text);
            }
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
            use proptest::prelude::*;

            proptest! {
                #[test]
                fn invalid_initial_characters_cannot_bypass_name_admission(suffix in ".{0,128}") {
                    let representation = serde_json::Value::String(format!("1{suffix}"));

                    let error = serde_json::from_value::<GateName>(representation).err_or_abort("invalid suffix");

                    prop_assert_eq!(error.to_string(), "git command failed: gate names must be unique ASCII trailer names");
                }

                #[test]
                fn admitted_suffixes_round_trip_exactly(name in "[A-Za-z][A-Za-z0-9-]{0,128}") {
                    let representation = serde_json::Value::String(name);

                    let admitted = serde_json::from_value::<GateName>(representation.clone()).or_abort("admitted suffix");

                    prop_assert_eq!(serde_json::to_value(admitted).or_abort("suffix serialization"), representation);
                }
            }
        }
        mod deserialize_in_place {
            use super::super::super::super::GateName;
            use crate::test_support::{OrAbort as _, ResultOrAbort as _};
            use proptest::prelude::*;
            use serde::Deserialize as _;

            proptest! {
                #[test]
                fn rejects_invalid_replacement_without_losing_prior_suffix(suffix in ".{0,128}") {
                    let mut admitted = serde_json::from_value::<GateName>(serde_json::Value::String("Previous".to_owned())).or_abort("previous name");
                    let representation = serde_json::Value::String(format!("1{suffix}"));

                    let error = GateName::deserialize_in_place(representation, &mut admitted).err_or_abort("invalid replacement");

                    prop_assert_eq!(error.to_string(), "git command failed: gate names must be unique ASCII trailer names");
                    prop_assert_eq!(serde_json::to_value(admitted).or_abort("preserved serialization"), serde_json::Value::String("Previous".to_owned()));
                }

                #[test]
                fn replaces_previous_suffix_with_every_admitted_generated_name(name in "[A-Za-z][A-Za-z0-9-]{0,128}") {
                    let mut admitted = serde_json::from_value::<GateName>(serde_json::Value::String("Previous".to_owned())).or_abort("previous name");
                    let representation = serde_json::Value::String(name);

                    GateName::deserialize_in_place(representation.clone(), &mut admitted).or_abort("admitted replacement");

                    prop_assert_eq!(serde_json::to_value(admitted).or_abort("replacement serialization"), representation);
                }
            }
        }
    }
    mod fmt {
        mod display {
            mod fmt {
                use super::super::super::super::super::GateName;
                use crate::test_support::OrAbort as _;
                use proptest::prelude::*;

                proptest! {
                    #[test]
                    fn preserves_exact_generated_suffix(name in "[A-Za-z][A-Za-z0-9-]{0,128}") {
                        let admitted = serde_json::from_value::<GateName>(serde_json::Value::String(name.clone())).or_abort("admitted suffix");

                        let rendered = admitted.to_string();

                        prop_assert_eq!(rendered, name);
                    }
                }
            }
        }
    }
}

mod gate_set {
    mod as_slice {
        use super::super::super::{
            GateSet,
            tests::{ctx, fixture},
        };
        use crate::test_support::OrAbort as _;
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn retains_every_generated_gate_in_cli_order(count in super::super::gate_count()) {
                let directory = fixture();
                let context = ctx(directory.path());
                let gates = GateSet::new(super::super::arrange_gates(&context, count)).or_abort("gate set");

                let observed = gates.as_slice();

                prop_assert_eq!(observed.iter().map(|gate| gate.name.as_str().to_owned()).collect::<Vec<_>>(), (0..count).map(|index| format!("test-{index}")).collect::<Vec<_>>());
            }
        }
    }
    mod new {
        use super::super::super::{
            GateSet,
            tests::{ctx, fixture},
        };
        use crate::test_support::{OrAbort as _, ResultOrAbort as _};
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn admits_nonempty_distinct_generated_sets(count in super::super::gate_count()) {
                let directory = fixture();
                let context = ctx(directory.path());
                let gates = super::super::arrange_gates(&context, count);

                let admitted = GateSet::new(gates).or_abort("admitted set");

                prop_assert_eq!(admitted.as_slice().len(), usize::from(count));
            }

            #[test]
            fn rejects_generated_case_fold_aliases(name in "[A-Za-z][A-Za-z0-9-]{0,128}") {
                let directory = fixture();
                let context = ctx(directory.path());
                let first = super::super::super::named(&context, &name, super::super::super::tests::command("true")).or_abort("first");
                let second = super::super::super::named(&context, &name.to_ascii_uppercase(), super::super::super::tests::command("false")).or_abort("second");

                let error = GateSet::new(vec![first, second]).err_or_abort("duplicate set");

                prop_assert_eq!(error.to_string(), "git command failed: gate names must be unique ASCII trailer names");
            }
        }
    }
    mod serialize {
        use super::super::super::{
            GateSet,
            tests::{ctx, fixture},
        };
        use crate::test_support::OrAbort as _;
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn emits_only_ordered_specifications_for_generated_sets(count in super::super::gate_count()) {
                let directory = fixture();
                let context = ctx(directory.path());
                let gates = GateSet::new(super::super::arrange_gates(&context, count)).or_abort("gate set");

                let observed = serde_json::to_value(gates).or_abort("specifications");

                prop_assert_eq!(observed, serde_json::Value::Array((0..count).map(|index| super::super::super::tests::spec_value(&format!("test-{index}"), "true")).collect()));
            }
        }
    }
}

mod legacy {
    use proptest::prelude::*;
    proptest! { #[test] fn generated_legacy_identity_is_order_independent(text in "[A-Za-z][A-Za-z0-9 $;]{0,20}") { super::super::tests::check_identity(&text); } }
}

mod named {
    use proptest::prelude::*;
    proptest! { #[test] fn generated_commands_keep_byte_identity(text in "[A-Za-z][A-Za-z0-9 $;]{0,20}") { super::super::tests::check_identity(&text); } }
}

mod parse_records {
    use proptest::prelude::*;
    proptest! { #[test] fn generated_records_remove_only_owned_spans(key in "Gate-[A-Za-z][A-Za-z0-9-]{0,16}", value in "[A-Za-z0-9]{1,20}") { super::super::tests::check_raw_records(&key, &value); } }
}

mod validate_names {
    use proptest::prelude::*;
    proptest! { #[test] fn generated_names_obey_case_insensitive_uniqueness(name in "[A-Za-z][A-Za-z0-9-]{0,16}") { super::super::tests::check_names(&name); } }
}

mod verify_observed {
    use proptest::prelude::*;
    proptest! { #[test] fn generated_message_bodies_survive_ordered_verification(label in "[A-Za-z][A-Za-z0-9 ]{0,20}") { super::super::tests::check_verify(&label); } }
}

use core::ops::RangeInclusive;

fn arrange_gates(context: &super::Ctx<'_>, count: u8) -> Vec<super::Gate> {
    use crate::test_support::OrAbort as _;
    (0..count)
        .map(|index| {
            super::named(
                context,
                &format!("test-{index}"),
                super::tests::command("true"),
            )
            .or_abort("admitted gate")
        })
        .collect()
}

fn gate_count() -> RangeInclusive<u8> {
    1..=4
}
