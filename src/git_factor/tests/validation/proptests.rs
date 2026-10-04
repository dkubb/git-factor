mod validate_exec_syntax {
    use super::super::resolve_contract::{Observation, Reply as QueryReply};
    use super::super::*;
    use core::cell::RefCell;
    use core::ops::RangeInclusive;
    use proptest::char::range;
    use proptest::collection::vec;
    use proptest::prelude::*;
    use std::os::unix::process::ExitStatusExt as _;

    #[derive(Clone, Debug)]
    enum Reply {
        Signal(i32),
        Spawn(String),
        Status(u8),
    }

    struct SyntaxRunner<'calls> {
        calls: &'calls RefCell<Vec<String>>,
        reply: Reply,
    }

    impl Runner for SyntaxRunner<'_> {
        fn output(
            &self,
            bin: &str,
            args: &[&str],
            _envs: &[(&str, Option<&str>)],
            cwd: &Path,
        ) -> io::Result<Output> {
            self.calls
                .borrow_mut()
                .push(format!("output {bin} {args:?} cwd={cwd:?}"));
            Err(io::Error::other("syntax validation must use status"))
        }

        fn status(
            &self,
            bin: &str,
            args: &[&str],
            envs: &[(&str, Option<&str>)],
            quiet: bool,
            cwd: &Path,
        ) -> io::Result<ExitStatus> {
            self.calls.borrow_mut().push(format!(
                "status {bin} {args:?} {envs:?} quiet={quiet} cwd={cwd:?}"
            ));
            match self.reply.clone() {
                Reply::Signal(signal) => Ok(ExitStatus::from_raw(signal)),
                Reply::Spawn(message) => Err(io::Error::other(message)),
                Reply::Status(code) => Ok(ExitStatus::from_raw(i32::from(code) << 8)),
            }
        }
    }

    proptest! {
        #[test]
        fn preserves_generated_syntax_status_and_launch_errors(
            command in prop_oneof![
                1 => Just(String::new()),
                7 => vec(
                    prop_oneof![
                        Just('\''),
                        Just('"'),
                        Just('\\'),
                        Just('\n'),
                        Just('\u{3bb}'),
                        Just(' '),
                        range('\u{1}', char::MAX),
                    ],
                    RangeInclusive::<usize>::new(0, 80),
                ).prop_map(|characters| characters.into_iter().collect::<String>()),
            ],
            reply in prop_oneof![
                Just(Reply::Status(0)),
                RangeInclusive::<u8>::new(1, u8::MAX).prop_map(Reply::Status),
                RangeInclusive::<u8>::new(1, 31)
                    .prop_map(|signal| Reply::Signal(i32::from(signal))),
                prop_oneof![
                    1 => Just(String::new()),
                    7 => vec(any::<char>(), RangeInclusive::<usize>::new(0, 80))
                        .prop_map(|characters| characters.into_iter().collect::<String>()),
                ].prop_map(Reply::Spawn),
            ],
            directory in "/contract/[a-z]{1,12}( [a-z]{1,8})?",
        ) {
            let expected = match reply.clone() {
                Reply::Spawn(message) => Err(format!(
                    "git command failed: bash syntax check: git command failed: bash --norc: {message}"
                )),
                Reply::Status(0) => Ok(()),
                Reply::Signal(_) | Reply::Status(_) => Err(format!(
                    "invalid exec syntax: {command}"
                )),
            };
            let calls = RefCell::new(Vec::new());
            let runner = SyntaxRunner { calls: &calls, reply };
            let capabilities = Observation::new(&calls, QueryReply::IoFailure);
            let ctx = Ctx {
                runner: &runner,
                cwd: PathBuf::from(&directory),
                io: &capabilities,
                env: &capabilities,
                fs: &capabilities,
            };
            let expected_calls = vec![format!(
                "status bash [\"--norc\", \"--noprofile\", \"-n\", \"-c\", {command:?}] [] quiet=true cwd={directory:?}"
            )];

            let actual = super::super::validate_exec_syntax(&ctx, &command)
                .map_err(|error| error.to_string());

            prop_assert_eq!(actual, expected);
            prop_assert_eq!(&*calls.borrow(), &expected_calls);
        }
    }
}

mod resolve_commit {
    use super::super::resolve_contract::{Observation, Reply};
    use super::super::*;
    use core::cell::RefCell;
    use core::ops::RangeInclusive;
    use proptest::prelude::*;

    #[derive(Debug)]
    enum Case {
        LaunchFailure,
        Malformed(String),
        Nonzero(u8, String),
        Success(String),
    }

    proptest! {
        #[test]
        fn preserves_query_identity_and_failure_payloads(
            reference in "[A-Za-z0-9_~^/-]{0,80}",
            case in prop_oneof![
                "[0-9a-fA-F]{40}".prop_map(Case::Success),
                "[0-9a-fA-F]{39}".prop_map(Case::Malformed),
                "[0-9a-fA-F]{41}".prop_map(Case::Malformed),
                "[0-9a-fA-F]{39}g".prop_map(Case::Malformed),
                Just(String::new()).prop_map(Case::Malformed),
                Just("\u{fffd}".to_owned()).prop_map(Case::Malformed),
                (RangeInclusive::<u8>::new(1, 127), "[0-9a-fA-F]{40}")
                    .prop_map(|(code, stdout)| Case::Nonzero(code, stdout)),
                Just(()).prop_map(|()| Case::LaunchFailure),
            ],
            padding in prop::sample::select(vec!["", " ", "\t", "\n"]),
        ) {
            let (reply, expected) = match case {
                Case::LaunchFailure => (Reply::IoFailure, Err(reference.clone())),
                Case::Success(sha) => (
                    Reply::Output {
                        exit_code: 0,
                        stdout: format!("{padding}{sha}{padding}\n").into_bytes(),
                    },
                    Ok(sha),
                ),
                Case::Malformed(stdout) => (
                    Reply::Output {
                        exit_code: 0,
                        stdout: format!("{padding}{stdout}{padding}\n").into_bytes(),
                    },
                    Err(stdout),
                ),
                Case::Nonzero(exit_code, stdout) => (
                    Reply::Output {
                        exit_code,
                        stdout: format!("{padding}{stdout}{padding}\n").into_bytes(),
                    },
                    Err(reference.clone()),
                ),
            };
            let calls = RefCell::new(Vec::new());
            let observation = Observation::new(&calls, reply);
            let ctx = Ctx {
                cwd: PathBuf::from("/contract/repository"),
                env: &observation,
                fs: &observation,
                io: &observation,
                runner: &observation,
            };
            let expected_calls = vec![format!(
                concat!(
                    "output git [\"rev-parse\", \"--verify\", {:?}]",
                    " envs=[] cwd=\"/contract/repository\"",
                ),
                reference,
            )];

            let actual = super::super::resolve_commit(&ctx, &reference);

            let payload = match actual {
                Ok(sha) => Ok(sha.as_str().to_owned()),
                Err(FactorError::InvalidCommit(value)) => Err(value),
                Err(other) => Err(format!("unexpected error: {other:?}")),
            };
            prop_assert_eq!(payload, expected);
            prop_assert_eq!(&*calls.borrow(), &expected_calls);
        }
    }
}

mod base_parent_in {
    use super::super::parent_contracts::{CommitObject, inputs};
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn actual_parent_records_define_the_boundary((commit, reply, expected) in inputs()) {
            let fixture = CommitObject::new(&commit, reply);
            let ctx = fixture.context();
            let actual = super::super::base_parent_in(&ctx, &fixture.commit)
                .map_err(|error| error.to_string());
            prop_assert_eq!(actual, expected);
            prop_assert_eq!(fixture.queries.get(), 1);
            prop_assert_eq!(fixture.mutations.get(), 0);
            prop_assert_eq!(fixture.io.out.borrow().clone(), "");
            prop_assert_eq!(fixture.io.err.borrow().clone(), "");
        }
    }
}

mod validate_not_merge {
    use super::super::parent_contracts::{CommitObject, inputs};
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn admits_only_successfully_read_non_merge_objects((commit, reply, expected) in inputs()) {
            let fixture = CommitObject::new(&commit, reply);
            let ctx = fixture.context();

            let actual = super::super::validate_not_merge(&ctx, &fixture.commit)
                .map_err(|error| error.to_string());

            prop_assert_eq!(actual, expected.map(|_parent| ()));
            prop_assert_eq!(fixture.queries.get(), 1);
            prop_assert_eq!(fixture.mutations.get(), 0);
            prop_assert_eq!(fixture.io.out.borrow().clone(), "");
            prop_assert_eq!(fixture.io.err.borrow().clone(), "");
        }
    }
}

mod has_tree_change {
    use super::super::{has_tree_change, tree_fixture::Fixture};
    use crate::test_support::OrAbort as _;
    use proptest::prelude::*;
    use std::fs;
    proptest! {
        #[test]
        fn compares_generated_root_and_parent_boundaries_with_real_native_trees(root in any::<bool>(), content in prop::option::of(prop::collection::vec(any::<u8>(), 1..40))) {
            let fixture = Fixture::new(root, content.as_deref());
            let index = fs::read(fixture.directory().join(".git/index")).ok();
            let head = fixture.git(&["rev-parse", "HEAD"]);
            let refs = fixture.git(&["for-each-ref", "--format=%(refname) %(objectname)"]);
            let actual = has_tree_change(&fixture.context(), fixture.span());
            prop_assert_eq!(actual.or_abort("tree comparison"), content.is_some());
            prop_assert_eq!(fs::read(fixture.directory().join(".git/index")).ok(), index);
            prop_assert_eq!(fixture.git(&["rev-parse", "HEAD"]), head);
            prop_assert_eq!(fixture.git(&["for-each-ref", "--format=%(refname) %(objectname)"]), refs);
            prop_assert_eq!(fs::read(fixture.directory().join("user")).or_abort("protected bytes"), b"unrelated user bytes".to_vec());
            prop_assert!(!fixture.directory().join(".git/factor").exists());
        }
    }
}
