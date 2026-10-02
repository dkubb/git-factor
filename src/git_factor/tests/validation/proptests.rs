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
        fn output(&self, bin: &str, args: &[&str], cwd: &Path) -> io::Result<Output> {
            self.calls
                .borrow_mut()
                .push(format!("output {bin} {args:?} cwd={cwd:?}"));
            Err(io::Error::other("syntax validation must use status"))
        }

        fn status(
            &self,
            bin: &str,
            args: &[&str],
            envs: &[(&str, &str)],
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

mod remove_empty_root_in {
    use super::super::*;
    use core::cell::RefCell;
    use core::ops::RangeInclusive;
    use proptest::prelude::*;
    use std::os::unix::ffi::OsStringExt as _;
    use std::os::unix::process::ExitStatusExt as _;

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum Fault {
        Canonicalize,
        EditorEncoding,
        Executable,
        RebaseLaunch,
        RevList,
        RootTree,
        ShortRoot,
    }

    struct RootObservation<'input> {
        calls: RefCell<Vec<String>>,
        directory: &'input str,
        exit_code: u8,
        fault: Option<Fault>,
        has_content: bool,
        padding: &'input str,
        roots: &'input [String],
        short_root: &'input str,
    }

    impl RootObservation<'_> {
        fn refused<T>(&self, operation: &str) -> io::Result<T> {
            self.calls.borrow_mut().push(operation.to_owned());
            Err(io::Error::other("root cleanup must not use this IO class"))
        }
    }

    impl Runner for RootObservation<'_> {
        fn output(&self, bin: &str, args: &[&str], cwd: &Path) -> io::Result<Output> {
            self.calls
                .borrow_mut()
                .push(format!("output {bin} {args:?} cwd={cwd:?}"));
            let failed_query = match (self.fault, bin, args) {
                (Some(Fault::RevList), "git", &["rev-list", "--max-parents=0", "HEAD"]) => {
                    Some("root listing unavailable\n")
                }
                (Some(Fault::RootTree), "git", &["ls-tree", _]) => Some("root tree unavailable\n"),
                (Some(Fault::ShortRoot), "git", &["rev-parse", "--short", _]) => {
                    Some("root abbreviation unavailable\n")
                }
                _ => None,
            };
            if let Some(stderr) = failed_query {
                return Ok(Output {
                    status: ExitStatus::from_raw(1 << 8),
                    stdout: Vec::new(),
                    stderr: stderr.as_bytes().to_vec(),
                });
            }
            let stdout = match (bin, args) {
                ("git", &["rev-list", "--max-parents=0", "HEAD"]) => format!(
                    "{}{}{}\n",
                    self.padding,
                    self.roots.join("\n"),
                    self.padding
                ),
                ("git", &["ls-tree", root])
                    if self.roots.first().map(String::as_str) == Some(root) =>
                {
                    if self.has_content {
                        "100644 blob file\n".to_owned()
                    } else {
                        String::new()
                    }
                }
                ("git", &["rev-parse", "--short", root])
                    if self.roots.first().map(String::as_str) == Some(root) =>
                {
                    format!("{}{}{}\n", self.padding, self.short_root, self.padding)
                }
                _ => {
                    return Ok(Output {
                        status: ExitStatus::from_raw(1 << 8),
                        stdout: Vec::new(),
                        stderr: b"unexpected root query".to_vec(),
                    });
                }
            };
            Ok(Output {
                status: ExitStatus::from_raw(0),
                stdout: stdout.into_bytes(),
                stderr: Vec::new(),
            })
        }

        fn status(
            &self,
            bin: &str,
            args: &[&str],
            envs: &[(&str, &str)],
            quiet: bool,
            cwd: &Path,
        ) -> io::Result<ExitStatus> {
            self.calls.borrow_mut().push(format!(
                "status {bin} {args:?} {envs:?} quiet={quiet} cwd={cwd:?}"
            ));
            if self.fault == Some(Fault::RebaseLaunch) {
                return Err(io::Error::other("root rebase unavailable"));
            }
            Ok(ExitStatus::from_raw(i32::from(self.exit_code) << 8))
        }
    }

    impl Env for RootObservation<'_> {
        fn current_dir(&self) -> io::Result<PathBuf> {
            self.refused("current_dir")
        }
        fn current_exe(&self) -> io::Result<PathBuf> {
            self.calls.borrow_mut().push("current_exe".to_owned());
            if self.fault == Some(Fault::Executable) {
                return Err(io::Error::other("root executable unavailable"));
            }
            Ok(PathBuf::from("/unresolved/git-factor"))
        }
        fn var_os(&self, key: &str) -> Option<OsString> {
            assert_eq!(key, "GIT_FACTOR_TRACE_LOG");
            None
        }
    }

    impl Fs for RootObservation<'_> {
        fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
            self.calls
                .borrow_mut()
                .push(format!("canonicalize {path:?}"));
            if self.fault == Some(Fault::Canonicalize) {
                return Err(io::Error::other(
                    "root executable canonicalization unavailable",
                ));
            }
            if self.fault == Some(Fault::EditorEncoding) {
                return Ok(PathBuf::from(OsString::from_vec(
                    b"/invalid-\xff/git-factor".to_vec(),
                )));
            }
            Ok(Path::new(self.directory).join("git-factor"))
        }
        fn create_dir_all(&self, _path: &Path) -> io::Result<()> {
            self.refused("create_dir_all")
        }
        fn exists(&self, _path: &Path) -> bool {
            self.calls.borrow_mut().push("exists".to_owned());
            false
        }
        fn is_dir(&self, _path: &Path) -> bool {
            self.calls.borrow_mut().push("is_dir".to_owned());
            false
        }
        fn read_to_string(&self, _path: &Path) -> io::Result<String> {
            self.refused("read_to_string")
        }
        fn remove_dir_all(&self, _path: &Path) -> io::Result<()> {
            self.refused("remove_dir_all")
        }
        fn remove_file(&self, _path: &Path) -> io::Result<()> {
            self.refused("remove_file")
        }
        fn write_string(&self, _path: &Path, _content: &str) -> io::Result<()> {
            self.refused("write_string")
        }
    }

    impl Io for RootObservation<'_> {
        fn err(&self, _text: &str) -> io::Result<()> {
            self.refused("stderr")
        }
        fn errln(&self, _line: &str) -> io::Result<()> {
            self.refused("stderr line")
        }
        fn out(&self, _text: &str) -> io::Result<()> {
            self.refused("stdout")
        }
        fn outln(&self, _line: &str) -> io::Result<()> {
            self.refused("stdout line")
        }
    }

    proptest! {
        #[test]
        fn preserves_root_identity_exact_replay_and_refused_io(
            roots in prop::collection::vec(
                "[0-9a-f]{40}", RangeInclusive::<usize>::new(0, 3)
            ),
            padding in "[ \n\t]{0,8}",
            short_len in RangeInclusive::<usize>::new(7, 12),
            has_content in any::<bool>(),
            exit_code in prop_oneof![Just::<u8>(0), RangeInclusive::<u8>::new(1, 127)],
            editor in prop::sample::select(vec![
                ("/contract/bin", "'/contract/bin/git-sequence-editor'"),
                ("/root bin", "'/root bin/git-sequence-editor'"),
                ("/root'bin", "'/root'\\''bin/git-sequence-editor'"),
            ]),
        ) {
            let root = roots.first().map_or("", String::as_str);
            let short_root = root.get(..short_len).unwrap_or_default();
            let observation = RootObservation {
                calls: RefCell::new(Vec::new()),
                directory: editor.0,
                exit_code,
                fault: None,
                has_content,
                padding: &padding,
                roots: &roots,
                short_root,
            };
            let ctx = Ctx {
                cwd: PathBuf::from("/contract/repository"),
                env: &observation,
                fs: &observation,
                io: &observation,
                runner: &observation,
            };
            let mut expected_calls = vec![concat!(
                "output git [\"rev-list\", \"--max-parents=0\", \"HEAD\"]",
                " cwd=\"/contract/repository\"",
            ).to_owned()];
            let expected = match roots.len() {
                0 => Err(concat!(
                    "git command failed: no root commit found ",
                    "for empty-root cleanup",
                ).to_owned()),
                1 => {
                    expected_calls.push(format!(
                        "output git [\"ls-tree\", {root:?}] cwd=\"/contract/repository\""
                    ));
                    if has_content {
                        Ok(())
                    } else {
                        expected_calls.push(format!(concat!(
                            "output git [\"rev-parse\", \"--short\", {:?}]",
                            " cwd=\"/contract/repository\"",
                        ), root));
                        expected_calls.push("current_exe".to_owned());
                        expected_calls.push("canonicalize \"/unresolved/git-factor\"".to_owned());
                        let sequence_editor = format!("{} '--drop' '{short_root}'", editor.1);
                        expected_calls.push(format!(concat!(
                            "status git [\"rebase\", \"--empty\", \"drop\", ",
                            "\"--interactive\", \"--no-update-refs\", ",
                            "\"--quiet\", \"--root\"] ",
                            "[(\"GIT_EDITOR\", \"false\"), ",
                            "(\"GIT_SEQUENCE_EDITOR\", {:?})] quiet=false",
                            " cwd=\"/contract/repository\"",
                        ), sequence_editor));
                        if exit_code == 0 {
                            Ok(())
                        } else {
                            Err(format!(concat!(
                                "git command failed: rebase to remove empty root failed ",
                                "(exit {})",
                            ), exit_code))
                        }
                    }
                }
                _ => Err(concat!(
                    "git command failed: multiple root commits found; ",
                    "empty-root cleanup requires a single-root history",
                ).to_owned()),
            };

            let actual = super::super::remove_empty_root_in(&ctx)
                .map_err(|error| error.to_string());

            prop_assert_eq!(actual, expected);
            prop_assert_eq!(&*observation.calls.borrow(), &expected_calls);
        }

        #[test]
        fn preserves_failure_payloads_and_stops_at_the_failed_boundary(
            root in "[0-9a-f]{40}",
            short_len in RangeInclusive::<usize>::new(7, 12),
            fault in prop::sample::select(vec![
                Fault::RevList,
                Fault::RootTree,
                Fault::ShortRoot,
                Fault::Executable,
                Fault::Canonicalize,
                Fault::EditorEncoding,
                Fault::RebaseLaunch,
            ]),
        ) {
            let roots = [root.clone()];
            let short_root = root.get(..short_len).unwrap_or_default();
            let observation = RootObservation {
                calls: RefCell::new(Vec::new()),
                directory: "/contract/bin",
                exit_code: 0,
                fault: Some(fault),
                has_content: false,
                padding: "",
                roots: &roots,
                short_root,
            };
            let ctx = Ctx {
                cwd: PathBuf::from("/contract/repository"),
                env: &observation,
                fs: &observation,
                io: &observation,
                runner: &observation,
            };
            let root_query = concat!(
                "output git [\"rev-list\", \"--max-parents=0\", \"HEAD\"]",
                " cwd=\"/contract/repository\"",
            ).to_owned();
            let tree_query = format!(
                "output git [\"ls-tree\", {root:?}] cwd=\"/contract/repository\""
            );
            let short_query = format!(concat!(
                "output git [\"rev-parse\", \"--short\", {:?}]",
                " cwd=\"/contract/repository\"",
            ), root);
            let executable = "current_exe".to_owned();
            let canonicalize = "canonicalize \"/unresolved/git-factor\"".to_owned();
            let sequence_editor = format!(
                "'/contract/bin/git-sequence-editor' '--drop' '{short_root}'"
            );
            let rebase = format!(concat!(
                "status git [\"rebase\", \"--empty\", \"drop\", ",
                "\"--interactive\", \"--no-update-refs\", ",
                "\"--quiet\", \"--root\"] ",
                "[(\"GIT_EDITOR\", \"false\"), (\"GIT_SEQUENCE_EDITOR\", {:?})]",
                " quiet=false cwd=\"/contract/repository\"",
            ), sequence_editor);
            let (expected_error, expected_calls) = match fault {
                Fault::RevList => (
                    "git command failed: root listing unavailable",
                    vec![root_query],
                ),
                Fault::RootTree => (
                    "git command failed: root tree unavailable",
                    vec![root_query, tree_query],
                ),
                Fault::ShortRoot => (
                    "git command failed: root abbreviation unavailable",
                    vec![root_query, tree_query, short_query],
                ),
                Fault::Executable => (
                    "git command failed: cannot resolve current exe: root executable unavailable",
                    vec![root_query, tree_query, short_query, executable],
                ),
                Fault::Canonicalize => (
                    concat!(
                        "git command failed: cannot canonicalize exe: ",
                        "root executable canonicalization unavailable",
                    ),
                    vec![root_query, tree_query, short_query, executable, canonicalize],
                ),
                Fault::EditorEncoding => (
                    "git command failed: editor path is not valid UTF-8",
                    vec![root_query, tree_query, short_query, executable, canonicalize],
                ),
                Fault::RebaseLaunch => (
                    "git command failed: git rebase: root rebase unavailable",
                    vec![root_query, tree_query, short_query, executable, canonicalize, rebase],
                ),
            };

            let actual = super::super::remove_empty_root_in(&ctx)
                .map_err(|error| error.to_string());

            prop_assert_eq!(actual, Err(expected_error.to_owned()));
            prop_assert_eq!(&*observation.calls.borrow(), &expected_calls);
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
            let (reply, expected, snapshot) = match case {
                Case::LaunchFailure => (
                    Reply::IoFailure,
                    Err(reference.clone()),
                    vec![
                        concat!(
                            r#"output git ["rev-parse", "--verify", "HEAD"]"#,
                            r#" cwd="/contract/repository""#,
                        ).to_owned(),
                        concat!(
                            r#"output git ["rev-parse", "--verify", "HEAD^{tree}"]"#,
                            r#" cwd="/contract/repository""#,
                        ).to_owned(),
                        concat!(
                            r#"output git ["rev-parse", "--git-dir"]"#,
                            r#" cwd="/contract/repository""#,
                        ).to_owned(),
                        concat!(
                            r#"output git ["--no-optional-locks", "status", "--porcelain=v1", "#,
                            r#""--untracked-files=all"]"#,
                            r#" cwd="/contract/repository""#,
                        ).to_owned(),
                    ],
                ),
                Case::Success(sha) => (
                    Reply::Output {
                        exit_code: 0,
                        stdout: format!("{padding}{sha}{padding}\n").into_bytes(),
                    },
                    Ok(sha),
                    Vec::new(),
                ),
                Case::Malformed(stdout) => (
                    Reply::Output {
                        exit_code: 0,
                        stdout: format!("{padding}{stdout}{padding}\n").into_bytes(),
                    },
                    Err(stdout),
                    Vec::new(),
                ),
                Case::Nonzero(exit_code, stdout) => (
                    Reply::Output {
                        exit_code,
                        stdout: format!("{padding}{stdout}{padding}\n").into_bytes(),
                    },
                    Err(reference.clone()),
                    Vec::new(),
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
            let mut expected_calls = vec![format!(
                concat!(
                    "output git [\"rev-parse\", \"--verify\", {:?}]",
                    " cwd=\"/contract/repository\"",
                ),
                reference,
            )];
            expected_calls.extend(snapshot);

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
