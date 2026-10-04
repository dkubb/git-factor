mod has_tree_change {
    use super::super::{
        BaseParent, CommitSha, CommitSpan, NonEmpty, has_tree_change, tree_fixture::Fixture,
    };
    use crate::test_support::OrAbort as _;
    use std::fs;
    #[test]
    fn admits_an_actual_root_binary_change_without_mutation() {
        let root = true;
        let content: Option<Vec<u8>> = Some(vec![0, 255, 1]);
        let fixture = Fixture::new(root, content.as_deref());
        let index = fs::read(fixture.directory().join(".git/index")).ok();
        let head = fixture.git(&["rev-parse", "HEAD"]);
        let refs = fixture.git(&["for-each-ref", "--format=%(refname) %(objectname)"]);
        let actual = has_tree_change(&fixture.context(), fixture.span());
        assert_eq!(actual.or_abort("tree comparison"), content.is_some());
        assert_eq!(fs::read(fixture.directory().join(".git/index")).ok(), index);
        assert_eq!(fixture.git(&["rev-parse", "HEAD"]), head);
        assert_eq!(
            fixture.git(&["for-each-ref", "--format=%(refname) %(objectname)"]),
            refs
        );
        assert_eq!(
            fs::read(fixture.directory().join("user")).or_abort("protected bytes"),
            b"unrelated user bytes".to_vec()
        );
        assert!(!fixture.directory().join(".git/factor").exists());
    }
    #[test]
    fn refuses_an_unchanged_actual_nonempty_parent_boundary() {
        let fixture = Fixture::new(false, None);
        let index = fs::read(fixture.directory().join(".git/index")).or_abort("index");
        let head = fixture.git(&["rev-parse", "HEAD"]);
        let refs = fixture.git(&["for-each-ref", "--format=%(refname) %(objectname)"]);
        let actual = has_tree_change(&fixture.context(), fixture.span());
        assert!(!actual.or_abort("tree comparison"));
        assert_eq!(
            fs::read(fixture.directory().join(".git/index")).or_abort("index retained"),
            index
        );
        assert_eq!(fixture.git(&["rev-parse", "HEAD"]), head);
        assert_eq!(
            fixture.git(&["for-each-ref", "--format=%(refname) %(objectname)"]),
            refs
        );
        assert_eq!(
            fs::read(fixture.directory().join("retained anchor")).or_abort("parent bytes retained"),
            b"nonempty parent\0\n"
        );
        assert_eq!(
            fs::read(fixture.directory().join("user")).or_abort("user bytes retained"),
            b"unrelated user bytes"
        );
        assert!(!fixture.directory().join(".git/factor").exists());
    }
    #[test]
    fn refuses_a_cancelling_range_relative_to_its_first_commit_parent() {
        let fixture = Fixture::new(false, Some(b"temporary range change\0\n"));
        let first = fixture.span().first_commit().clone();
        fixture.git(&["rm", "--", "selected binary"]);
        fixture.git(&["commit", "--quiet", "-m", "Remove temporary range change"]);
        let tip = CommitSha::new(fixture.git(&["rev-parse", "HEAD"])).or_abort("range tip");
        let span = CommitSpan::new(
            NonEmpty::from_vec(vec![first, tip]).or_abort("two-commit span"),
            BaseParent::Commit,
        );
        let index = fs::read(fixture.directory().join(".git/index")).or_abort("index");
        let head = fixture.git(&["rev-parse", "HEAD"]);
        let refs = fixture.git(&["for-each-ref", "--format=%(refname) %(objectname)"]);
        let actual = has_tree_change(&fixture.context(), &span);
        assert!(!actual.or_abort("combined range tree comparison"));
        assert_eq!(
            fs::read(fixture.directory().join(".git/index")).or_abort("index retained"),
            index
        );
        assert_eq!(fixture.git(&["rev-parse", "HEAD"]), head);
        assert_eq!(
            fixture.git(&["for-each-ref", "--format=%(refname) %(objectname)"]),
            refs
        );
        assert_eq!(
            fs::read(fixture.directory().join("retained anchor")).or_abort("parent bytes retained"),
            b"nonempty parent\0\n"
        );
        assert_eq!(
            fs::read(fixture.directory().join("user")).or_abort("user bytes retained"),
            b"unrelated user bytes"
        );
        assert!(!fixture.directory().join("selected binary").exists());
        assert!(!fixture.directory().join(".git/factor").exists());
    }
}

mod validate_exec_syntax {
    use super::*;

    #[test]
    fn validate_exec_syntax_reports_spawn_failure_as_git_command() {
        use crate::git_factor::tests::{ScriptedRunner, TestEnv, TestIo};

        let dir = TempDir::new().or_abort("tempdir");
        let repo = dir.path();

        let runner = ScriptedRunner::default();
        let io = TestIo::default();
        let env = TestEnv {
            cwd: repo.to_path_buf(),
        };
        let ctx = Ctx {
            runner: &runner,
            cwd: repo.to_path_buf(),
            io: &io,
            env: &env,
            fs: &REAL_FS,
        };

        let err = validate_exec_syntax(&ctx, "echo hi").err_or_abort("expected spawn failure");
        assert!(
            matches!(&err, FactorError::GitCommand(msg) if msg.contains("bash syntax check:") && msg.contains("unexpected status call")),
            "err was: {err:?}"
        );
    }

    #[test]
    fn validate_exec_syntax_returns_invalid_exec_syntax_on_nonzero_status() {
        use crate::git_factor::tests::{ScriptedRunner, TestEnv, TestIo};

        let dir = TempDir::new().or_abort("tempdir");
        let repo = dir.path();
        let command = "if )";
        let runner = ScriptedRunner::default().with_status(
            "bash",
            &["--norc", "--noprofile", "-n", "-c", command],
            &[],
            true,
            repo,
            2,
        );
        let io = TestIo::default();
        let env = TestEnv {
            cwd: repo.to_path_buf(),
        };
        let ctx = Ctx {
            runner: &runner,
            cwd: repo.to_path_buf(),
            io: &io,
            env: &env,
            fs: &REAL_FS,
        };

        let err = validate_exec_syntax(&ctx, command).err_or_abort("expected invalid exec syntax");
        assert!(
            matches!(&err, FactorError::InvalidExecSyntax(found) if found == command),
            "err was: {err:?}"
        );
    }

    #[test]
    fn validate_exec_syntax_wraps_command_status_io_error() {
        struct ExecSyntaxIoErrorRunner;

        impl Runner for ExecSyntaxIoErrorRunner {
            fn output(
                &self,
                _bin: &str,
                _args: &[&str],
                _envs: &[(&str, Option<&str>)],
                _cwd: &Path,
            ) -> io::Result<Output> {
                Err(io::Error::other("output should not be called"))
            }

            fn status(
                &self,
                _bin: &str,
                _args: &[&str],
                _envs: &[(&str, Option<&str>)],
                _quiet: bool,
                _cwd: &Path,
            ) -> io::Result<ExitStatus> {
                Err(io::Error::other("forced bash syntax status io failure"))
            }
        }

        let dir = TempDir::new().or_abort("tempdir");
        let runner = ExecSyntaxIoErrorRunner;
        let env = TestEnv {
            cwd: dir.path().to_path_buf(),
        };
        let ctx = Ctx {
            runner: &runner,
            cwd: dir.path().to_path_buf(),
            io: &REAL_IO,
            env: &env,
            fs: &REAL_FS,
        };
        let output_err = runner
            .output("git", &["status"], &[], dir.path())
            .err_or_abort("output method should fail");
        assert!(
            output_err
                .to_string()
                .contains("output should not be called"),
            "unexpected output error: {output_err}"
        );

        let err = validate_exec_syntax(&ctx, "echo ok").err_or_abort("status io error should fail");
        let message = git_command_message(&err).or_abort("expected GitCommand");
        assert!(
            message.contains("forced bash syntax status io failure"),
            "unexpected error: {err:?}"
        );
    }

    #[test]
    fn validate_exec_syntax_returns_ok_for_valid_shell_command() {
        let dir = TempDir::new().or_abort("tempdir");
        let ctx = ctx_for(dir.path());
        validate_exec_syntax(&ctx, "echo ok")
            .or_abort("valid shell command should pass syntax check");
    }
}

mod validate_not_merge {
    use super::super::parent_contracts::{CommitObject, Reply};

    #[test]
    fn accepts_a_root_object() {
        let fixture = CommitObject::new(
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            Reply::text("tree cccccccccccccccccccccccccccccccccccccccc\nauthor Example <example@example.com> 1 +0000\n\nsubject\n".to_owned()),
        );
        let ctx = fixture.context();

        let actual = super::super::validate_not_merge(&ctx, &fixture.commit)
            .map_err(|error| error.to_string());

        assert_eq!(actual, Ok(()));
        assert_eq!(fixture.queries.get(), 1);
        assert_eq!(fixture.mutations.get(), 0);
        assert_eq!(fixture.io.out.borrow().as_str(), "");
        assert_eq!(fixture.io.err.borrow().as_str(), "");
    }

    #[test]
    fn accepts_an_ordinary_object() {
        let fixture = CommitObject::new(
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            Reply::text("tree cccccccccccccccccccccccccccccccccccccccc\nparent bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\nauthor Example <example@example.com> 1 +0000\n\nsubject\n".to_owned()),
        );
        let ctx = fixture.context();

        let actual = super::super::validate_not_merge(&ctx, &fixture.commit)
            .map_err(|error| error.to_string());

        assert_eq!(actual, Ok(()));
        assert_eq!(fixture.queries.get(), 1);
        assert_eq!(fixture.mutations.get(), 0);
        assert_eq!(fixture.io.out.borrow().as_str(), "");
        assert_eq!(fixture.io.err.borrow().as_str(), "");
    }

    #[test]
    fn rejects_a_merge_object() {
        let fixture = CommitObject::new(
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            Reply::text("tree cccccccccccccccccccccccccccccccccccccccc\nparent bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\nparent cccccccccccccccccccccccccccccccccccccccc\nauthor Example <example@example.com> 1 +0000\n\nsubject\n".to_owned()),
        );
        let ctx = fixture.context();

        let actual = super::super::validate_not_merge(&ctx, &fixture.commit)
            .map_err(|error| error.to_string());

        assert_eq!(actual, Err("commit aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa is a merge commit and cannot be split".to_owned()));
        assert_eq!(fixture.queries.get(), 1);
        assert_eq!(fixture.mutations.get(), 0);
        assert_eq!(fixture.io.out.borrow().as_str(), "");
        assert_eq!(fixture.io.err.borrow().as_str(), "");
    }

    #[test]
    fn refuses_failed_object_queries() {
        let code: i32 = 256;
        let fixture = CommitObject::new(
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            Reply::Output {
                code,
                content: String::new(),
                error: "query refused".to_owned(),
            },
        );
        let ctx = fixture.context();

        let actual = super::super::validate_not_merge(&ctx, &fixture.commit)
            .map_err(|error| error.to_string());

        assert_eq!(actual, Err("git command failed: query refused".to_owned()));
        assert_eq!(fixture.queries.get(), 1);
        assert_eq!(fixture.mutations.get(), 0);
        assert_eq!(fixture.io.out.borrow().as_str(), "");
        assert_eq!(fixture.io.err.borrow().as_str(), "");
    }

    #[test]
    fn refuses_unavailable_object_queries() {
        let fixture = CommitObject::new(
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            Reply::SpawnError("query unavailable".to_owned()),
        );
        let ctx = fixture.context();

        let actual = super::super::validate_not_merge(&ctx, &fixture.commit)
            .map_err(|error| error.to_string());

        assert_eq!(
            actual,
            Err("git command failed: git cat-file: query unavailable".to_owned())
        );
        assert_eq!(fixture.queries.get(), 1);
        assert_eq!(fixture.mutations.get(), 0);
        assert_eq!(fixture.io.out.borrow().as_str(), "");
        assert_eq!(fixture.io.err.borrow().as_str(), "");
    }
}

mod base_parent_in {
    use super::super::parent_contracts::{CommitObject, Reply};

    #[test]
    fn refuses_an_empty_commit_object() {
        let fixture = CommitObject::new(
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            Reply::text(String::new()),
        );
        let ctx = fixture.context();
        let actual =
            super::super::base_parent_in(&ctx, &fixture.commit).map_err(|error| error.to_string());
        assert_eq!(actual, Err("git command failed: malformed commit object aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa: missing tree header".to_owned()));
        assert_eq!(fixture.queries.get(), 1);
        assert_eq!(fixture.mutations.get(), 0);
        assert_eq!(fixture.io.out.borrow().as_str(), "");
        assert_eq!(fixture.io.err.borrow().as_str(), "");
    }

    #[test]
    fn preserves_a_failed_object_query() {
        let code: i32 = 256;
        let fixture = CommitObject::new(
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            Reply::Output {
                code,
                content: String::new(),
                error: "query refused".to_owned(),
            },
        );
        let ctx = fixture.context();
        let actual =
            super::super::base_parent_in(&ctx, &fixture.commit).map_err(|error| error.to_string());
        assert_eq!(actual, Err("git command failed: query refused".to_owned()));
        assert_eq!(fixture.queries.get(), 1);
        assert_eq!(fixture.mutations.get(), 0);
        assert_eq!(fixture.io.out.borrow().as_str(), "");
        assert_eq!(fixture.io.err.borrow().as_str(), "");
    }

    #[test]
    fn refuses_an_invalid_parent_identity() {
        let fixture = CommitObject::new("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", Reply::text("tree cccccccccccccccccccccccccccccccccccccccc\nparent invalid-bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\n\nsubject".to_owned()));
        let ctx = fixture.context();
        let actual =
            super::super::base_parent_in(&ctx, &fixture.commit).map_err(|error| error.to_string());
        assert_eq!(actual, Err("git command failed: malformed commit object aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa: invalid parent header 'invalid-bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb'".to_owned()));
        assert_eq!(fixture.queries.get(), 1);
        assert_eq!(fixture.mutations.get(), 0);
        assert_eq!(fixture.io.out.borrow().as_str(), "");
        assert_eq!(fixture.io.err.borrow().as_str(), "");
    }

    #[test]
    fn refuses_an_invalid_tree_identity() {
        let fixture = CommitObject::new(
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            Reply::text(
                "tree invalid-cccccccccccccccccccccccccccccccccccccccc\n\nsubject".to_owned(),
            ),
        );
        let ctx = fixture.context();
        let actual =
            super::super::base_parent_in(&ctx, &fixture.commit).map_err(|error| error.to_string());
        assert_eq!(actual, Err("git command failed: malformed commit object aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa: invalid tree header 'invalid-cccccccccccccccccccccccccccccccccccccccc'".to_owned()));
        assert_eq!(fixture.queries.get(), 1);
        assert_eq!(fixture.mutations.get(), 0);
        assert_eq!(fixture.io.out.borrow().as_str(), "");
        assert_eq!(fixture.io.err.borrow().as_str(), "");
    }

    #[test]
    fn rejects_two_actual_parents() {
        let fixture = CommitObject::new("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", Reply::text("tree cccccccccccccccccccccccccccccccccccccccc\nparent bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\nparent bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\n\nsubject".to_owned()));
        let ctx = fixture.context();
        let actual =
            super::super::base_parent_in(&ctx, &fixture.commit).map_err(|error| error.to_string());
        assert_eq!(actual, Err("commit aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa is a merge commit and cannot be split".to_owned()));
        assert_eq!(fixture.queries.get(), 1);
        assert_eq!(fixture.mutations.get(), 0);
        assert_eq!(fixture.io.out.borrow().as_str(), "");
        assert_eq!(fixture.io.err.borrow().as_str(), "");
    }

    #[test]
    fn requires_the_tree_header() {
        let fixture = CommitObject::new(
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            Reply::text("parent bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\n\nsubject".to_owned()),
        );
        let ctx = fixture.context();
        let actual =
            super::super::base_parent_in(&ctx, &fixture.commit).map_err(|error| error.to_string());
        assert_eq!(actual, Err("git command failed: malformed commit object aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa: missing tree header".to_owned()));
        assert_eq!(fixture.queries.get(), 1);
        assert_eq!(fixture.mutations.get(), 0);
        assert_eq!(fixture.io.out.borrow().as_str(), "");
        assert_eq!(fixture.io.err.borrow().as_str(), "");
    }

    #[test]
    fn admits_one_actual_parent() {
        let fixture = CommitObject::new("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", Reply::text("tree cccccccccccccccccccccccccccccccccccccccc\nparent bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\nauthor Example <example@example.com> 1 +0000\n\nsubject".to_owned()));
        let ctx = fixture.context();
        let actual =
            super::super::base_parent_in(&ctx, &fixture.commit).map_err(|error| error.to_string());
        assert_eq!(actual, Ok(super::super::BaseParent::Commit));
        assert_eq!(fixture.queries.get(), 1);
        assert_eq!(fixture.mutations.get(), 0);
        assert_eq!(fixture.io.out.borrow().as_str(), "");
        assert_eq!(fixture.io.err.borrow().as_str(), "");
    }

    #[test]
    fn ignores_parent_text_in_signatures_and_the_message() {
        let fixture = CommitObject::new("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", Reply::text("tree cccccccccccccccccccccccccccccccccccccccc\nauthor Example <example@example.com> 1 +0000\ngpgsig signature\n parent bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\n\nparent bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_owned()));
        let ctx = fixture.context();
        let actual =
            super::super::base_parent_in(&ctx, &fixture.commit).map_err(|error| error.to_string());
        assert_eq!(actual, Ok(super::super::BaseParent::Root));
        assert_eq!(fixture.queries.get(), 1);
        assert_eq!(fixture.mutations.get(), 0);
        assert_eq!(fixture.io.out.borrow().as_str(), "");
        assert_eq!(fixture.io.err.borrow().as_str(), "");
    }

    #[test]
    fn preserves_an_object_query_spawn_failure() {
        let fixture = CommitObject::new(
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            Reply::SpawnError("lookup refused".to_owned()),
        );
        let ctx = fixture.context();
        let actual =
            super::super::base_parent_in(&ctx, &fixture.commit).map_err(|error| error.to_string());
        assert_eq!(
            actual,
            Err("git command failed: git cat-file: lookup refused".to_owned())
        );
        assert_eq!(fixture.queries.get(), 1);
        assert_eq!(fixture.mutations.get(), 0);
        assert_eq!(fixture.io.out.borrow().as_str(), "");
        assert_eq!(fixture.io.err.borrow().as_str(), "");
    }
}

mod resolve_commit {
    use super::super::resolve_contract::{Observation, Reply};
    use super::super::*;
    use core::cell::RefCell;

    #[test]
    fn preserves_hex_case_and_trims_git_output() {
        let calls = RefCell::new(Vec::new());
        let observation = Observation::new(
            &calls,
            Reply::Output {
                exit_code: 0,
                stdout: b" \tABCDEF0123456789abcdef0123456789ABCDEF01\n".to_vec(),
            },
        );
        let ctx = Ctx {
            cwd: PathBuf::from("/contract/repository"),
            env: &observation,
            fs: &observation,
            io: &observation,
            runner: &observation,
        };

        let actual = super::super::resolve_commit(&ctx, "topic~2");

        assert_eq!(
            actual.or_abort("reference must resolve").as_str(),
            "ABCDEF0123456789abcdef0123456789ABCDEF01"
        );
        assert_eq!(
            *calls.borrow(),
            vec![
                r#"output git ["rev-parse", "--verify", "topic~2"] envs=[] cwd="/contract/repository""#
                    .to_owned(),
            ]
        );
    }

    #[test]
    fn preserves_reference_on_nonzero_exit() {
        let calls = RefCell::new(Vec::new());
        let observation = Observation::new(
            &calls,
            Reply::Output {
                exit_code: 23,
                stdout: b"0123456789abcdef0123456789abcdef01234567\n".to_vec(),
            },
        );
        let ctx = Ctx {
            cwd: PathBuf::from("/contract/repository"),
            env: &observation,
            fs: &observation,
            io: &observation,
            runner: &observation,
        };

        let actual = super::super::resolve_commit(&ctx, "topic~2");

        assert!(matches!(actual, Err(FactorError::InvalidCommit(value)) if value == "topic~2"));
        assert_eq!(
            *calls.borrow(),
            vec![
                r#"output git ["rev-parse", "--verify", "topic~2"] envs=[] cwd="/contract/repository""#
                    .to_owned(),
            ]
        );
    }

    #[test]
    fn preserves_reference_on_launch_error() {
        let calls = RefCell::new(Vec::new());
        let observation = Observation::new(&calls, Reply::IoFailure);
        let ctx = Ctx {
            cwd: PathBuf::from("/contract/repository"),
            env: &observation,
            fs: &observation,
            io: &observation,
            runner: &observation,
        };

        let actual = super::super::resolve_commit(&ctx, "topic~2");

        assert!(matches!(actual, Err(FactorError::InvalidCommit(value)) if value == "topic~2"));
        assert_eq!(
            *calls.borrow(),
            vec![
                concat!(
                    r#"output git ["rev-parse", "--verify", "topic~2"]"#,
                    r#" envs=[] cwd="/contract/repository""#,
                )
                .to_owned(),
            ]
        );
    }

    #[test]
    fn rejects_empty_successful_output() {
        let calls = RefCell::new(Vec::new());
        let observation = Observation::new(
            &calls,
            Reply::Output {
                exit_code: 0,
                stdout: b" \n".to_vec(),
            },
        );
        let ctx = Ctx {
            cwd: PathBuf::from("/contract/repository"),
            env: &observation,
            fs: &observation,
            io: &observation,
            runner: &observation,
        };

        let actual = super::super::resolve_commit(&ctx, "topic~2");

        assert!(matches!(actual, Err(FactorError::InvalidCommit(value)) if value.is_empty()));
        assert_eq!(
            *calls.borrow(),
            vec![
                r#"output git ["rev-parse", "--verify", "topic~2"] envs=[] cwd="/contract/repository""#
                    .to_owned(),
            ]
        );
    }

    #[test]
    fn rejects_short_successful_output() {
        let calls = RefCell::new(Vec::new());
        let observation = Observation::new(
            &calls,
            Reply::Output {
                exit_code: 0,
                stdout: b"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n".to_vec(),
            },
        );
        let ctx = Ctx {
            cwd: PathBuf::from("/contract/repository"),
            env: &observation,
            fs: &observation,
            io: &observation,
            runner: &observation,
        };

        let actual = super::super::resolve_commit(&ctx, "topic~2");

        assert!(
            matches!(actual, Err(FactorError::InvalidCommit(value)) if value == "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
        );
        assert_eq!(
            *calls.borrow(),
            vec![
                r#"output git ["rev-parse", "--verify", "topic~2"] envs=[] cwd="/contract/repository""#
                    .to_owned(),
            ]
        );
    }

    #[test]
    fn rejects_long_successful_output() {
        let calls = RefCell::new(Vec::new());
        let observation = Observation::new(
            &calls,
            Reply::Output {
                exit_code: 0,
                stdout: b"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n".to_vec(),
            },
        );
        let ctx = Ctx {
            cwd: PathBuf::from("/contract/repository"),
            env: &observation,
            fs: &observation,
            io: &observation,
            runner: &observation,
        };

        let actual = super::super::resolve_commit(&ctx, "topic~2");

        assert!(
            matches!(actual, Err(FactorError::InvalidCommit(value)) if value == "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
        );
        assert_eq!(
            *calls.borrow(),
            vec![
                r#"output git ["rev-parse", "--verify", "topic~2"] envs=[] cwd="/contract/repository""#
                    .to_owned(),
            ]
        );
    }

    #[test]
    fn rejects_nonhex_successful_output() {
        let calls = RefCell::new(Vec::new());
        let observation = Observation::new(
            &calls,
            Reply::Output {
                exit_code: 0,
                stdout: b"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaag\n".to_vec(),
            },
        );
        let ctx = Ctx {
            cwd: PathBuf::from("/contract/repository"),
            env: &observation,
            fs: &observation,
            io: &observation,
            runner: &observation,
        };

        let actual = super::super::resolve_commit(&ctx, "topic~2");

        assert!(
            matches!(actual, Err(FactorError::InvalidCommit(value)) if value == "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaag")
        );
        assert_eq!(
            *calls.borrow(),
            vec![
                r#"output git ["rev-parse", "--verify", "topic~2"] envs=[] cwd="/contract/repository""#
                    .to_owned(),
            ]
        );
    }

    #[test]
    fn rejects_lossy_utf8_output() {
        let calls = RefCell::new(Vec::new());
        let observation = Observation::new(
            &calls,
            Reply::Output {
                exit_code: 0,
                stdout: vec![0xff],
            },
        );
        let ctx = Ctx {
            cwd: PathBuf::from("/contract/repository"),
            env: &observation,
            fs: &observation,
            io: &observation,
            runner: &observation,
        };

        let actual = super::super::resolve_commit(&ctx, "topic~2");

        assert!(matches!(actual, Err(FactorError::InvalidCommit(value)) if value == "\u{fffd}"));
        assert_eq!(
            *calls.borrow(),
            vec![
                r#"output git ["rev-parse", "--verify", "topic~2"] envs=[] cwd="/contract/repository""#
                    .to_owned(),
            ]
        );
    }
}

use super::*;
use core::iter;
use std::env;
use std::ffi::OsString;
#[cfg(unix)]
use std::os::unix::ffi::OsStringExt as _;
use std::os::unix::process::ExitStatusExt as _;
use std::process::Command;
use std::process::Output;
use tempfile::TempDir;

const SPAN_START_SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const SPAN_END_SHA: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

struct TestEnv {
    cwd: PathBuf,
}

impl Env for TestEnv {
    fn current_dir(&self) -> io::Result<PathBuf> {
        Ok(self.cwd.clone())
    }

    fn current_exe(&self) -> io::Result<PathBuf> {
        env::current_exe()
    }

    fn var_os(&self, _key: &str) -> Option<OsString> {
        None
    }
}

struct FailingExeEnv {
    cwd: PathBuf,
}

impl Env for FailingExeEnv {
    fn current_dir(&self) -> io::Result<PathBuf> {
        Ok(self.cwd.clone())
    }

    fn current_exe(&self) -> io::Result<PathBuf> {
        Err(io::Error::other("forced current_exe failure"))
    }

    fn var_os(&self, _key: &str) -> Option<OsString> {
        None
    }
}

#[cfg(unix)]
struct NonUtf8Fs;

#[cfg(unix)]
impl Fs for NonUtf8Fs {
    fn canonicalize(&self, _path: &Path) -> io::Result<PathBuf> {
        let mut bytes = b"/tmp/".to_vec();
        bytes.push(0xff);
        bytes.extend_from_slice(b"/bin/git-factor");
        Ok(PathBuf::from(OsString::from_vec(bytes)))
    }

    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        REAL_FS.create_dir_all(path)
    }

    fn exists(&self, path: &Path) -> bool {
        REAL_FS.exists(path)
    }

    fn is_dir(&self, path: &Path) -> bool {
        REAL_FS.is_dir(path)
    }

    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        REAL_FS.read_to_string(path)
    }

    fn remove_atomic_file(&self, path: &Path) -> io::Result<()> {
        REAL_FS.remove_atomic_file(path)
    }

    fn remove_dir_all(&self, path: &Path) -> io::Result<()> {
        REAL_FS.remove_dir_all(path)
    }

    fn symlink_metadata(&self, path: &Path) -> io::Result<fs::Metadata> {
        REAL_FS.symlink_metadata(path)
    }

    fn write_atomic_string(&self, path: &Path, content: &str) -> io::Result<()> {
        REAL_FS.write_atomic_string(path, content)
    }

    fn write_string(&self, path: &Path, content: &str) -> io::Result<()> {
        REAL_FS.write_string(path, content)
    }
}

struct RangeLookupRunner;

impl Runner for RangeLookupRunner {
    fn output(
        &self,
        _bin: &str,
        args: &[&str],
        _envs: &[(&str, Option<&str>)],
        _cwd: &Path,
    ) -> io::Result<Output> {
        let expected_range = format!("{SPAN_START_SHA}..{SPAN_END_SHA}");
        let (status, stdout, stderr) = if args == ["rev-parse", "--verify", "start"] {
            (
                ExitStatus::from_raw(0),
                format!("{SPAN_START_SHA}\n").into_bytes(),
                Vec::new(),
            )
        } else if args == ["rev-parse", "--verify", "end"] {
            (
                ExitStatus::from_raw(0),
                format!("{SPAN_END_SHA}\n").into_bytes(),
                Vec::new(),
            )
        } else if args
            == [
                "rev-list",
                "--reverse",
                "--ancestry-path",
                expected_range.as_str(),
            ]
        {
            (
                ExitStatus::from_raw(256),
                Vec::new(),
                b"forced range lookup failure\n".to_vec(),
            )
        } else {
            return Err(io::Error::other(format!(
                "unexpected output args: {}",
                args.join(" ")
            )));
        };

        Ok(Output {
            status,
            stdout,
            stderr,
        })
    }

    fn status(
        &self,
        _bin: &str,
        args: &[&str],
        _envs: &[(&str, Option<&str>)],
        _quiet: bool,
        _cwd: &Path,
    ) -> io::Result<ExitStatus> {
        let start_merge_ref = format!("{SPAN_START_SHA}^2");
        let end_merge_ref = format!("{SPAN_END_SHA}^2");
        if args == ["rev-parse", "--quiet", "--verify", start_merge_ref.as_str()]
            || args == ["rev-parse", "--quiet", "--verify", end_merge_ref.as_str()]
        {
            Ok(ExitStatus::from_raw(256))
        } else {
            Err(io::Error::other(format!(
                "unexpected status args: {}",
                args.join(" ")
            )))
        }
    }
}

struct ParentLookupRunner;

impl Runner for ParentLookupRunner {
    fn output(
        &self,
        _bin: &str,
        args: &[&str],
        _envs: &[(&str, Option<&str>)],
        _cwd: &Path,
    ) -> io::Result<Output> {
        if args == ["cat-file", "commit", SPAN_START_SHA] {
            return Ok(Output {
                status: ExitStatus::from_raw(0),
                stdout: concat!(
                    "tree cccccccccccccccccccccccccccccccccccccccc\n",
                    "author Example <example@example.com> 1 +0000\n",
                    "committer Example <example@example.com> 1 +0000\n\nstart\n"
                )
                .as_bytes()
                .to_vec(),
                stderr: Vec::new(),
            });
        }
        if args == ["cat-file", "commit", SPAN_END_SHA] {
            return Ok(Output {
                status: ExitStatus::from_raw(0),
                stdout: format!(
                    concat!(
                        "tree cccccccccccccccccccccccccccccccccccccccc\nparent {}\n",
                        "author Example <example@example.com> 1 +0000\n",
                        "committer Example <example@example.com> 1 +0000\n\nend\n"
                    ),
                    SPAN_START_SHA
                )
                .into_bytes(),
                stderr: Vec::new(),
            });
        }
        let end_parent_ref = format!("{SPAN_END_SHA}^");
        if args == ["rev-parse", "--verify", end_parent_ref.as_str()] {
            Ok(Output {
                status: ExitStatus::from_raw(256),
                stdout: Vec::new(),
                stderr: b"forced parent lookup failure\n".to_vec(),
            })
        } else {
            Err(io::Error::other(format!(
                "unexpected output args: {}",
                args.join(" ")
            )))
        }
    }

    fn status(
        &self,
        _bin: &str,
        args: &[&str],
        _envs: &[(&str, Option<&str>)],
        _quiet: bool,
        _cwd: &Path,
    ) -> io::Result<ExitStatus> {
        let start_merge_ref = format!("{SPAN_START_SHA}^2");
        let end_merge_ref = format!("{SPAN_END_SHA}^2");
        if args == ["rev-parse", "--quiet", "--verify", start_merge_ref.as_str()]
            || args == ["rev-parse", "--quiet", "--verify", end_merge_ref.as_str()]
        {
            Ok(ExitStatus::from_raw(256))
        } else {
            Err(io::Error::other(format!(
                "unexpected status args: {}",
                args.join(" ")
            )))
        }
    }
}

struct ParentParseRunner;

impl Runner for ParentParseRunner {
    fn output(
        &self,
        _bin: &str,
        args: &[&str],
        _envs: &[(&str, Option<&str>)],
        _cwd: &Path,
    ) -> io::Result<Output> {
        if args == ["cat-file", "commit", SPAN_START_SHA] {
            return Ok(Output {
                status: ExitStatus::from_raw(0),
                stdout: concat!(
                    "tree cccccccccccccccccccccccccccccccccccccccc\n",
                    "author Example <example@example.com> 1 +0000\n",
                    "committer Example <example@example.com> 1 +0000\n\nstart\n"
                )
                .as_bytes()
                .to_vec(),
                stderr: Vec::new(),
            });
        }
        if args == ["cat-file", "commit", SPAN_END_SHA] {
            return Ok(Output {
                status: ExitStatus::from_raw(0),
                stdout: format!(
                    concat!(
                        "tree cccccccccccccccccccccccccccccccccccccccc\nparent {}\n",
                        "author Example <example@example.com> 1 +0000\n",
                        "committer Example <example@example.com> 1 +0000\n\nend\n"
                    ),
                    SPAN_START_SHA
                )
                .into_bytes(),
                stderr: Vec::new(),
            });
        }
        let end_parent_ref = format!("{SPAN_END_SHA}^");
        if args == ["rev-parse", "--verify", end_parent_ref.as_str()] {
            Ok(Output {
                status: ExitStatus::from_raw(0),
                stdout: b"not-a-commit\n".to_vec(),
                stderr: Vec::new(),
            })
        } else {
            Err(io::Error::other(format!(
                "unexpected output args: {}",
                args.join(" ")
            )))
        }
    }

    fn status(
        &self,
        _bin: &str,
        args: &[&str],
        _envs: &[(&str, Option<&str>)],
        _quiet: bool,
        _cwd: &Path,
    ) -> io::Result<ExitStatus> {
        let start_merge_ref = format!("{SPAN_START_SHA}^2");
        let end_merge_ref = format!("{SPAN_END_SHA}^2");
        if args == ["rev-parse", "--quiet", "--verify", start_merge_ref.as_str()]
            || args == ["rev-parse", "--quiet", "--verify", end_merge_ref.as_str()]
        {
            Ok(ExitStatus::from_raw(256))
        } else {
            Err(io::Error::other(format!(
                "unexpected status args: {}",
                args.join(" ")
            )))
        }
    }
}

fn ctx_for(path: &Path) -> Ctx<'static> {
    Ctx {
        runner: &REAL_RUNNER,
        cwd: path.to_path_buf(),
        io: &REAL_IO,
        env: &REAL_ENV,
        fs: &REAL_FS,
    }
}

fn git_command_message(error: &FactorError) -> Option<String> {
    if let &FactorError::GitCommand(_) = error {
        return error
            .to_string()
            .strip_prefix("git command failed: ")
            .map(str::to_owned);
    }
    None
}

fn invalid_commit_message(error: &FactorError) -> Option<String> {
    if let &FactorError::InvalidCommit(_) = error {
        return error
            .to_string()
            .strip_prefix("invalid commit: ")
            .map(str::to_owned);
    }
    None
}

fn init_git_repo(path: &Path) {
    let init = Command::new("git")
        .args(["init", "--quiet"])
        .current_dir(path)
        .status()
        .or_abort("run git init");
    assert!(init.success(), "git init failed: {init:?}");

    let config_name = Command::new("git")
        .args(["config", "user.name", "Test User"])
        .current_dir(path)
        .status()
        .or_abort("run git config user.name");
    assert!(config_name.success(), "git config user.name failed");

    let config_email = Command::new("git")
        .args(["config", "user.email", "test@example.com"])
        .current_dir(path)
        .status()
        .or_abort("run git config user.email");
    assert!(config_email.success(), "git config user.email failed");

    fs::write(path.join("file.txt"), "base\n").or_abort("write file");
    let add = Command::new("git")
        .args(["add", "file.txt"])
        .current_dir(path)
        .status()
        .or_abort("run git add");
    assert!(add.success(), "git add failed");

    let commit = Command::new("git")
        .args([
            "-c",
            "commit.template=",
            "-c",
            "core.hooksPath=.git/hooks",
            "commit",
            "--no-verify",
            "--quiet",
            "-m",
            "base",
        ])
        .current_dir(path)
        .status()
        .or_abort("run git commit");
    assert!(commit.success(), "git commit failed");
}

#[expect(
    clippy::single_call_fn,
    reason = "native parentless empty-root arrangement remains separate from its decisive validation Act"
)]
fn init_empty_root_repo(path: &Path) {
    let init = Command::new("git")
        .args(["init", "--quiet"])
        .current_dir(path)
        .status()
        .or_abort("run git init");
    assert!(init.success(), "git init failed: {init:?}");

    let config_name = Command::new("git")
        .args(["config", "user.name", "Test User"])
        .current_dir(path)
        .status()
        .or_abort("run git config user.name");
    assert!(config_name.success(), "git config user.name failed");

    let config_email = Command::new("git")
        .args(["config", "user.email", "test@example.com"])
        .current_dir(path)
        .status()
        .or_abort("run git config user.email");
    assert!(config_email.success(), "git config user.email failed");

    let commit = Command::new("git")
        .args([
            "-c",
            "commit.template=",
            "-c",
            "core.hooksPath=.git/hooks",
            "commit",
            "--no-verify",
            "--allow-empty",
            "--quiet",
            "-m",
            "empty root",
        ])
        .current_dir(path)
        .status()
        .or_abort("run git commit --allow-empty");
    assert!(commit.success(), "git commit --allow-empty failed");
}

fn head_sha(path: &Path) -> String {
    let output = Command::new("git")
        .args(["rev-parse", "--verify", "HEAD"])
        .current_dir(path)
        .output()
        .or_abort("read HEAD");
    assert!(output.status.success(), "git rev-parse HEAD failed");
    String::from_utf8(output.stdout)
        .or_abort("decode HEAD")
        .trim()
        .to_owned()
}

fn commit_file(path: &Path, file: &str, content: &str, message: &str) -> String {
    fs::write(path.join(file), content).or_abort("write file");
    let add = Command::new("git")
        .args(["add", file])
        .current_dir(path)
        .status()
        .or_abort("run git add");
    assert!(add.success(), "git add failed");

    let commit = Command::new("git")
        .args([
            "-c",
            "commit.template=",
            "-c",
            "core.hooksPath=.git/hooks",
            "commit",
            "--no-verify",
            "--quiet",
            "-m",
            message,
        ])
        .current_dir(path)
        .status()
        .or_abort("run git commit");
    assert!(commit.success(), "git commit failed");

    head_sha(path)
}

fn init_linear_repo(path: &Path) -> [String; 3] {
    init_git_repo(path);
    let first = head_sha(path);
    let second = commit_file(path, "two.txt", "two\n", "second");
    let third = commit_file(path, "three.txt", "three\n", "third");
    [first, second, third]
}

#[test]
fn resolve_commit_refs_rejects_symmetric_diff_ranges() {
    let dir = TempDir::new().or_abort("tempdir");
    let ctx = ctx_for(dir.path());
    let commit_ref = NonEmptyString::try_from("HEAD...HEAD".to_owned()).or_abort("non-empty");
    let refs = NonEmpty::singleton(commit_ref);
    let err = resolve_commit_refs(&ctx, &refs).err_or_abort("symmetric diff must be rejected");

    let message = invalid_commit_message(&err).or_abort("expected InvalidCommit");
    assert!(
        message.contains("symmetric diff"),
        "unexpected error: {err:?}"
    );
}

#[test]
fn resolve_commit_refs_rejects_unknown_single_ref() {
    let dir = TempDir::new().or_abort("tempdir");
    init_git_repo(dir.path());
    let ctx = ctx_for(dir.path());
    let commit_ref =
        NonEmptyString::try_from("definitely-not-a-ref".to_owned()).or_abort("non-empty");
    let refs = NonEmpty::singleton(commit_ref);

    let err = resolve_commit_refs(&ctx, &refs).err_or_abort("unknown ref must be rejected");
    let message = invalid_commit_message(&err).or_abort("expected InvalidCommit");
    assert_eq!(message, "definitely-not-a-ref");
}

#[test]
fn resolve_head_commit_maps_git_errors_to_invalid_head() {
    let dir = TempDir::new().or_abort("tempdir");
    let ctx = ctx_for(dir.path());

    let err = resolve_head_commit(&ctx).err_or_abort("missing HEAD should fail");
    let message = invalid_commit_message(&err).or_abort("expected InvalidCommit");
    assert_eq!(message, "HEAD");
}

#[test]
fn resolve_commit_refs_accepts_known_single_ref() {
    let dir = TempDir::new().or_abort("tempdir");
    init_git_repo(dir.path());
    let ctx = ctx_for(dir.path());
    let commit_ref = NonEmptyString::try_from("HEAD".to_owned()).or_abort("non-empty");
    let refs = NonEmpty::singleton(commit_ref);
    let commits = resolve_commit_refs(&ctx, &refs).or_abort("HEAD should resolve");
    let head = resolve_head_commit(&ctx).or_abort("resolve");
    let mut iter = commits.iter();
    assert_eq!(iter.next(), Some(&head));
    assert_eq!(iter.next(), None);
}

#[test]
fn resolve_commit_refs_rejects_unknown_range_ref() {
    let dir = TempDir::new().or_abort("tempdir");
    init_git_repo(dir.path());
    let ctx = ctx_for(dir.path());
    let commit_ref = NonEmptyString::try_from("deadbeef..HEAD".to_owned()).or_abort("non-empty");
    let refs = NonEmpty::singleton(commit_ref);

    let err = resolve_commit_refs(&ctx, &refs).err_or_abort("unknown range ref must be rejected");
    let message = invalid_commit_message(&err).or_abort("expected InvalidCommit");
    assert_eq!(message, "deadbeef..HEAD");
}

#[test]
fn resolve_commit_span_accepts_known_single_ref() {
    let dir = TempDir::new().or_abort("tempdir");
    init_git_repo(dir.path());
    let ctx = ctx_for(dir.path());
    let refs = NonEmpty::singleton(NonEmptyString::try_from("HEAD".to_owned()).or_abort("ref"));

    let commits = resolve_commit_span(&ctx, &refs).or_abort("HEAD span should resolve");

    assert_eq!(commits.len(), 1);
    assert_eq!(
        commits.first(),
        &resolve_head_commit(&ctx).or_abort("resolve HEAD")
    );
}

#[test]
fn resolve_commit_span_accepts_inclusive_two_ref_span() {
    let dir = TempDir::new().or_abort("tempdir");
    let [first, second, third] = init_linear_repo(dir.path());
    let ctx = ctx_for(dir.path());
    let refs = NonEmpty {
        head: NonEmptyString::try_from(first.clone()).or_abort("start"),
        tail: vec![NonEmptyString::try_from(third.clone()).or_abort("end")],
    };

    let commits = resolve_commit_span(&ctx, &refs).or_abort("inclusive span should resolve");
    let actual: Vec<&str> = commits.iter().map(CommitSha::as_str).collect();

    assert_eq!(
        actual,
        vec![first.as_str(), second.as_str(), third.as_str()]
    );
}

#[test]
fn resolve_commit_span_accepts_identical_two_ref_span() {
    let dir = TempDir::new().or_abort("tempdir");
    let [first, _second, _third] = init_linear_repo(dir.path());
    let ctx = ctx_for(dir.path());
    let refs = NonEmpty {
        head: NonEmptyString::try_from(first.clone()).or_abort("start"),
        tail: vec![NonEmptyString::try_from(first.clone()).or_abort("end")],
    };

    let commits = resolve_commit_span(&ctx, &refs).or_abort("identical refs should resolve");
    let actual: Vec<&str> = commits.iter().map(CommitSha::as_str).collect();

    assert_eq!(actual, vec![first.as_str()]);
}

#[test]
fn resolve_commit_span_accepts_exclusive_dotted_range() {
    let dir = TempDir::new().or_abort("tempdir");
    let [first, second, third] = init_linear_repo(dir.path());
    let ctx = ctx_for(dir.path());
    let refs = NonEmpty::singleton(
        NonEmptyString::try_from(format!("{first}..{third}")).or_abort("range"),
    );

    let commits = resolve_commit_span(&ctx, &refs).or_abort("exclusive span should resolve");
    let actual: Vec<&str> = commits.iter().map(CommitSha::as_str).collect();

    assert_eq!(actual, vec![second.as_str(), third.as_str()]);
}

#[test]
fn resolve_commit_span_accepts_inclusive_git_native_range() {
    let dir = TempDir::new().or_abort("tempdir");
    let [_first, second, third] = init_linear_repo(dir.path());
    let ctx = ctx_for(dir.path());
    let refs = NonEmpty::singleton(
        NonEmptyString::try_from(format!("{second}^..{third}")).or_abort("range"),
    );

    let commits = resolve_commit_span(&ctx, &refs).or_abort("inclusive git range");
    let actual: Vec<&str> = commits.iter().map(CommitSha::as_str).collect();

    assert_eq!(actual, vec![second.as_str(), third.as_str()]);
}

#[test]
fn resolve_commit_span_rejects_more_than_two_commit_args() {
    let dir = TempDir::new().or_abort("tempdir");
    let [first, second, third] = init_linear_repo(dir.path());
    let ctx = ctx_for(dir.path());
    let refs = NonEmpty {
        head: NonEmptyString::try_from(first).or_abort("first"),
        tail: vec![
            NonEmptyString::try_from(second).or_abort("second"),
            NonEmptyString::try_from(third).or_abort("third"),
        ],
    };

    let err = resolve_commit_span(&ctx, &refs).err_or_abort("too many args should be rejected");
    let message = invalid_commit_message(&err).or_abort("expected InvalidCommit");
    assert_eq!(
        message,
        "commit arguments must resolve to a single contiguous span"
    );
}

#[test]
fn resolve_commit_span_rejects_symmetric_diff_range() {
    let dir = TempDir::new().or_abort("tempdir");
    let [first, _second, third] = init_linear_repo(dir.path());
    let ctx = ctx_for(dir.path());
    let refs = NonEmpty::singleton(
        NonEmptyString::try_from(format!("{first}...{third}")).or_abort("range"),
    );

    let err = resolve_commit_span(&ctx, &refs).err_or_abort("symmetric diff should reject");
    let message = invalid_commit_message(&err).or_abort("expected InvalidCommit");
    assert!(
        message.contains("symmetric diff"),
        "unexpected message: {message}"
    );
}

#[test]
fn resolve_commit_span_rejects_dotted_two_arg_form() {
    let dir = TempDir::new().or_abort("tempdir");
    let [first, _second, third] = init_linear_repo(dir.path());
    let ctx = ctx_for(dir.path());
    let refs = NonEmpty {
        head: NonEmptyString::try_from(format!("{first}..{third}")).or_abort("start"),
        tail: vec![NonEmptyString::try_from(third).or_abort("end")],
    };

    let err =
        resolve_commit_span(&ctx, &refs).err_or_abort("mixed dotted/two-arg form should fail");
    let message = invalid_commit_message(&err).or_abort("expected InvalidCommit");
    assert!(
        message.contains("use either a single dotted range or two plain commit refs"),
        "unexpected message: {message}"
    );
}

#[test]
fn resolve_commit_span_rejects_empty_dotted_range() {
    let dir = TempDir::new().or_abort("tempdir");
    let [_first, _second, third] = init_linear_repo(dir.path());
    let ctx = ctx_for(dir.path());
    let refs = NonEmpty::singleton(
        NonEmptyString::try_from(format!("{third}..{third}")).or_abort("range"),
    );

    let err = resolve_commit_span(&ctx, &refs).err_or_abort("empty dotted range should fail");
    let message = invalid_commit_message(&err).or_abort("expected InvalidCommit");
    assert_eq!(message, format!("{third}..{third}"));
}

#[test]
fn resolve_commit_span_rejects_non_ancestry_two_ref_span() {
    let dir = TempDir::new().or_abort("tempdir");
    init_git_repo(dir.path());
    let ctx = ctx_for(dir.path());
    let branch =
        git_output(&ctx, &["rev-parse", "--abbrev-ref", "HEAD"]).or_abort("current branch");
    run_git(&ctx, &["checkout", "--quiet", "-b", "topic"]).or_abort("create topic");
    let topic = commit_file(dir.path(), "topic.txt", "topic\n", "topic");
    run_git(&ctx, &["checkout", "--quiet", branch.as_str()]).or_abort("checkout branch");
    let main = commit_file(dir.path(), "main.txt", "main\n", "main");
    let refs = NonEmpty {
        head: NonEmptyString::try_from(topic).or_abort("topic"),
        tail: vec![NonEmptyString::try_from(main).or_abort("main")],
    };

    let err = resolve_commit_span(&ctx, &refs).err_or_abort("non-ancestry span should reject");
    let message = invalid_commit_message(&err).or_abort("expected InvalidCommit");
    assert!(
        message.contains("contiguous ancestry span"),
        "unexpected message: {message}"
    );
}

#[test]
fn resolve_commit_span_rejects_unknown_start_ref_in_two_ref_span() {
    let dir = TempDir::new().or_abort("tempdir");
    let [_first, _second, third] = init_linear_repo(dir.path());
    let ctx = ctx_for(dir.path());
    let refs = NonEmpty {
        head: NonEmptyString::try_from("deadbeef".to_owned()).or_abort("start"),
        tail: vec![NonEmptyString::try_from(third).or_abort("end")],
    };

    let err = resolve_commit_span(&ctx, &refs).err_or_abort("unknown start ref should reject");
    let message = invalid_commit_message(&err).or_abort("expected InvalidCommit");
    assert_eq!(message, "deadbeef");
}

#[test]
fn resolve_commit_span_rejects_unknown_end_ref_in_two_ref_span() {
    let dir = TempDir::new().or_abort("tempdir");
    let [first, _second, _third] = init_linear_repo(dir.path());
    let ctx = ctx_for(dir.path());
    let refs = NonEmpty {
        head: NonEmptyString::try_from(first).or_abort("start"),
        tail: vec![NonEmptyString::try_from("deadbeef".to_owned()).or_abort("end")],
    };

    let err = resolve_commit_span(&ctx, &refs).err_or_abort("unknown end ref should reject");
    let message = invalid_commit_message(&err).or_abort("expected InvalidCommit");
    assert_eq!(message, "deadbeef");
}

#[test]
fn resolve_commit_span_rejects_merge_commits() {
    let dir = TempDir::new().or_abort("tempdir");
    init_git_repo(dir.path());
    let ctx = ctx_for(dir.path());
    let branch =
        git_output(&ctx, &["rev-parse", "--abbrev-ref", "HEAD"]).or_abort("current branch");
    run_git(&ctx, &["checkout", "--quiet", "-b", "topic"]).or_abort("create topic");
    let _topic = commit_file(dir.path(), "topic.txt", "topic\n", "topic");
    run_git(&ctx, &["checkout", "--quiet", branch.as_str()]).or_abort("checkout branch");
    let _main = commit_file(dir.path(), "main.txt", "main\n", "main");
    run_git(&ctx, &["merge", "--quiet", "--no-ff", "--no-edit", "topic"]).or_abort("merge topic");
    let merge = head_sha(dir.path());
    let refs = NonEmpty::singleton(NonEmptyString::try_from(merge).or_abort("merge"));

    let err = resolve_commit_span(&ctx, &refs).err_or_abort("merge commit should reject");
    assert!(
        err.to_string().contains("is a merge commit"),
        "unexpected error: {err}"
    );
}

#[test]
fn resolve_commit_span_rejects_two_ref_span_when_rev_list_lookup_fails() {
    let dir = TempDir::new().or_abort("tempdir");
    let runner = RangeLookupRunner;
    let env = TestEnv {
        cwd: dir.path().to_path_buf(),
    };
    let ctx = Ctx {
        runner: &runner,
        cwd: dir.path().to_path_buf(),
        io: &REAL_IO,
        env: &env,
        fs: &REAL_FS,
    };
    let refs = NonEmpty {
        head: NonEmptyString::try_from("start".to_owned()).or_abort("start"),
        tail: vec![NonEmptyString::try_from("end".to_owned()).or_abort("end")],
    };

    let err = resolve_commit_span(&ctx, &refs).err_or_abort("range lookup should fail");

    assert_eq!(
        invalid_commit_message(&err).or_abort("expected InvalidCommit"),
        "start end (range must resolve to a contiguous ancestry span)"
    );
}

#[test]
fn validate_contiguous_span_rejects_skipped_parent() {
    let dir = TempDir::new().or_abort("tempdir");
    let [first, _second, third] = init_linear_repo(dir.path());
    let ctx = ctx_for(dir.path());
    let commits = NonEmpty {
        head: CommitSha::new(first).or_abort("first"),
        tail: vec![CommitSha::new(third).or_abort("third")],
    };

    let err = validate_contiguous_span(&ctx, &commits).err_or_abort("skipped parent should reject");
    let message = invalid_commit_message(&err).or_abort("expected InvalidCommit");
    assert!(
        message.contains("contiguous ancestry span"),
        "unexpected message: {message}"
    );
}

#[test]
fn validate_contiguous_span_rejects_missing_parent_lookup() {
    let dir = TempDir::new().or_abort("tempdir");
    let runner = ParentLookupRunner;
    let env = TestEnv {
        cwd: dir.path().to_path_buf(),
    };
    let ctx = Ctx {
        runner: &runner,
        cwd: dir.path().to_path_buf(),
        io: &REAL_IO,
        env: &env,
        fs: &REAL_FS,
    };
    let commits = NonEmpty {
        head: CommitSha::new(SPAN_START_SHA.to_owned()).or_abort("start"),
        tail: vec![CommitSha::new(SPAN_END_SHA.to_owned()).or_abort("end")],
    };

    let err = validate_contiguous_span(&ctx, &commits).err_or_abort("parent lookup should fail");

    assert_eq!(
        invalid_commit_message(&err).or_abort("expected InvalidCommit"),
        format!(
            "{SPAN_START_SHA} {SPAN_END_SHA} (selected commits must form a contiguous ancestry span)"
        )
    );
}

#[test]
fn validate_contiguous_span_rejects_invalid_parent_sha_output() {
    let dir = TempDir::new().or_abort("tempdir");
    let runner = ParentParseRunner;
    let env = TestEnv {
        cwd: dir.path().to_path_buf(),
    };
    let ctx = Ctx {
        runner: &runner,
        cwd: dir.path().to_path_buf(),
        io: &REAL_IO,
        env: &env,
        fs: &REAL_FS,
    };
    let commits = NonEmpty {
        head: CommitSha::new(SPAN_START_SHA.to_owned()).or_abort("start"),
        tail: vec![CommitSha::new(SPAN_END_SHA.to_owned()).or_abort("end")],
    };

    let err =
        validate_contiguous_span(&ctx, &commits).err_or_abort("invalid parent sha should fail");

    assert_eq!(
        invalid_commit_message(&err).or_abort("expected InvalidCommit"),
        SPAN_END_SHA
    );
}

#[test]
fn validate_not_merge_rejects_merge_commit() {
    let dir = TempDir::new().or_abort("tempdir");
    init_git_repo(dir.path());
    let ctx = ctx_for(dir.path());

    let branch =
        git_output(&ctx, &["rev-parse", "--abbrev-ref", "HEAD"]).or_abort("current branch");

    run_git(&ctx, &["checkout", "--quiet", "-b", "topic"]).or_abort("create branch");
    fs::write(dir.path().join("topic.txt"), "topic\n").or_abort("write topic file");
    run_git(&ctx, &["add", "topic.txt"]).or_abort("add topic");
    run_git(
        &ctx,
        &[
            "-c",
            "commit.template=",
            "-c",
            "core.hooksPath=.git/hooks",
            "commit",
            "--no-verify",
            "--quiet",
            "-m",
            "topic change",
        ],
    )
    .or_abort("commit topic");

    run_git(&ctx, &["checkout", "--quiet", branch.as_str()]).or_abort("return branch");
    fs::write(dir.path().join("main.txt"), "main\n").or_abort("write main file");
    run_git(&ctx, &["add", "main.txt"]).or_abort("add main");
    run_git(
        &ctx,
        &[
            "-c",
            "commit.template=",
            "-c",
            "core.hooksPath=.git/hooks",
            "commit",
            "--no-verify",
            "--quiet",
            "-m",
            "main change",
        ],
    )
    .or_abort("commit main");
    run_git(&ctx, &["merge", "--quiet", "--no-ff", "--no-edit", "topic"]).or_abort("merge topic");

    let merge_sha =
        CommitSha::new(git_output(&ctx, &["rev-parse", "--verify", "HEAD"]).or_abort("merge sha"))
            .or_abort("valid merge sha");
    let topic_sha =
        CommitSha::new(git_output(&ctx, &["rev-parse", "--verify", "topic"]).or_abort("topic sha"))
            .or_abort("valid topic sha");
    validate_not_merge(&ctx, &topic_sha).or_abort("non-merge commit should validate");
    let err = validate_not_merge(&ctx, &merge_sha).err_or_abort("merge commit should be rejected");

    assert_eq!(
        err.to_string(),
        format!("commit {merge_sha} is a merge commit and cannot be split")
    );
}

#[test]
fn range_lookup_runner_covers_expected_and_unexpected_paths() {
    let range_runner = RangeLookupRunner;
    let start_output = range_runner
        .output(
            "git",
            &["rev-parse", "--verify", "start"],
            &[],
            Path::new("."),
        )
        .or_abort("resolve start ref");
    assert!(start_output.status.success());
    assert_eq!(
        start_output.stdout,
        format!("{SPAN_START_SHA}\n").into_bytes()
    );
    assert!(start_output.stderr.is_empty());

    let end_output = range_runner
        .output(
            "git",
            &["rev-parse", "--verify", "end"],
            &[],
            Path::new("."),
        )
        .or_abort("resolve end ref");
    assert!(end_output.status.success());
    assert_eq!(end_output.stdout, format!("{SPAN_END_SHA}\n").into_bytes());
    assert!(end_output.stderr.is_empty());

    let range_output = range_runner
        .output(
            "git",
            &[
                "rev-list",
                "--reverse",
                "--ancestry-path",
                &format!("{SPAN_START_SHA}..{SPAN_END_SHA}"),
            ],
            &[],
            Path::new("."),
        )
        .or_abort("range lookup output");
    assert!(!range_output.status.success());
    assert!(range_output.stdout.is_empty());
    assert_eq!(range_output.stderr, b"forced range lookup failure\n");

    let start_merge = range_runner
        .status(
            "git",
            &[
                "rev-parse",
                "--quiet",
                "--verify",
                &format!("{SPAN_START_SHA}^2"),
            ],
            &[],
            false,
            Path::new("."),
        )
        .or_abort("start merge check");
    assert!(!start_merge.success());

    let end_merge = range_runner
        .status(
            "git",
            &[
                "rev-parse",
                "--quiet",
                "--verify",
                &format!("{SPAN_END_SHA}^2"),
            ],
            &[],
            false,
            Path::new("."),
        )
        .or_abort("end merge check");
    assert!(!end_merge.success());

    let unexpected_range_output = range_runner
        .output("git", &["status"], &[], Path::new("."))
        .err_or_abort("unexpected range output args should fail");
    assert!(
        unexpected_range_output
            .to_string()
            .contains("unexpected output args"),
        "unexpected error: {unexpected_range_output:?}"
    );

    let unexpected_range_status = range_runner
        .status("git", &["status"], &[], false, Path::new("."))
        .err_or_abort("unexpected range status args should fail");
    assert!(
        unexpected_range_status
            .to_string()
            .contains("unexpected status args"),
        "unexpected error: {unexpected_range_status:?}"
    );
}

#[test]
fn parent_lookup_runner_covers_expected_and_unexpected_paths() {
    let parent_lookup_runner = ParentLookupRunner;
    let missing_parent_output = parent_lookup_runner
        .output(
            "git",
            &["rev-parse", "--verify", &format!("{SPAN_END_SHA}^")],
            &[],
            Path::new("."),
        )
        .or_abort("missing parent output");
    assert!(!missing_parent_output.status.success());
    assert!(missing_parent_output.stdout.is_empty());
    assert_eq!(
        missing_parent_output.stderr,
        b"forced parent lookup failure\n"
    );

    let lookup_start_merge = parent_lookup_runner
        .status(
            "git",
            &[
                "rev-parse",
                "--quiet",
                "--verify",
                &format!("{SPAN_START_SHA}^2"),
            ],
            &[],
            false,
            Path::new("."),
        )
        .or_abort("lookup start merge check");
    assert!(!lookup_start_merge.success());

    let lookup_end_merge = parent_lookup_runner
        .status(
            "git",
            &[
                "rev-parse",
                "--quiet",
                "--verify",
                &format!("{SPAN_END_SHA}^2"),
            ],
            &[],
            false,
            Path::new("."),
        )
        .or_abort("lookup end merge check");
    assert!(!lookup_end_merge.success());

    let unexpected_lookup_output = parent_lookup_runner
        .output("git", &["status"], &[], Path::new("."))
        .err_or_abort("unexpected lookup output args should fail");
    assert!(
        unexpected_lookup_output
            .to_string()
            .contains("unexpected output args"),
        "unexpected error: {unexpected_lookup_output:?}"
    );

    let unexpected_lookup_status = parent_lookup_runner
        .status("git", &["status"], &[], false, Path::new("."))
        .err_or_abort("unexpected lookup status args should fail");
    assert!(
        unexpected_lookup_status
            .to_string()
            .contains("unexpected status args"),
        "unexpected error: {unexpected_lookup_status:?}"
    );
}

#[test]
fn parent_parse_runner_covers_expected_and_unexpected_paths() {
    let parent_parse_runner = ParentParseRunner;
    let invalid_parent_output = parent_parse_runner
        .output(
            "git",
            &["rev-parse", "--verify", &format!("{SPAN_END_SHA}^")],
            &[],
            Path::new("."),
        )
        .or_abort("invalid parent output");
    assert!(invalid_parent_output.status.success());
    assert_eq!(invalid_parent_output.stdout, b"not-a-commit\n");
    assert!(invalid_parent_output.stderr.is_empty());

    let parse_start_merge = parent_parse_runner
        .status(
            "git",
            &[
                "rev-parse",
                "--quiet",
                "--verify",
                &format!("{SPAN_START_SHA}^2"),
            ],
            &[],
            false,
            Path::new("."),
        )
        .or_abort("parse start merge check");
    assert!(!parse_start_merge.success());

    let parse_end_merge = parent_parse_runner
        .status(
            "git",
            &[
                "rev-parse",
                "--quiet",
                "--verify",
                &format!("{SPAN_END_SHA}^2"),
            ],
            &[],
            false,
            Path::new("."),
        )
        .or_abort("parse end merge check");
    assert!(!parse_end_merge.success());

    let unexpected_parse_output = parent_parse_runner
        .output("git", &["status"], &[], Path::new("."))
        .err_or_abort("unexpected parse output args should fail");
    assert!(
        unexpected_parse_output
            .to_string()
            .contains("unexpected output args"),
        "unexpected error: {unexpected_parse_output:?}"
    );

    let unexpected_parse_status = parent_parse_runner
        .status("git", &["status"], &[], false, Path::new("."))
        .err_or_abort("unexpected parse status args should fail");
    assert!(
        unexpected_parse_status
            .to_string()
            .contains("unexpected status args"),
        "unexpected error: {unexpected_parse_status:?}"
    );
}

#[test]
fn env_and_fs_helpers_cover_delegated_paths() {
    let dir = TempDir::new().or_abort("tempdir");
    let env = TestEnv {
        cwd: dir.path().to_path_buf(),
    };
    assert_eq!(env.current_dir().or_abort("test env cwd"), dir.path());
    assert!(env.var_os("OTHER_ENV").is_none());

    let failing_env = FailingExeEnv {
        cwd: dir.path().to_path_buf(),
    };
    assert_eq!(
        failing_env.current_dir().or_abort("failing env cwd"),
        dir.path()
    );
    assert!(failing_env.var_os("OTHER_ENV").is_none());

    #[cfg(unix)]
    {
        let fs = NonUtf8Fs;
        let mkdir = dir.path().join("mkdir");
        fs.create_dir_all(&mkdir).or_abort("mkdir");
        assert!(fs.is_dir(&mkdir));
        assert!(fs.exists(&mkdir));

        let file = dir.path().join("file.txt");
        fs.write_string(&file, "hello").or_abort("write file");
        let content = fs.read_to_string(&file).or_abort("read file");
        assert_eq!(content, "hello");

        let rm_dir = dir.path().join("rm-dir");
        fs.create_dir_all(&rm_dir).or_abort("create rm dir");
        fs.remove_dir_all(&rm_dir).or_abort("remove rm dir");
        assert!(!fs.exists(&rm_dir));
    }
}

#[test]
fn error_extractors_cover_non_matching_variants() {
    let git_command = FactorError::GitCommand(non_empty_msg("boom".to_owned()));
    let invalid_commit = FactorError::InvalidCommit("deadbeef".to_owned());

    assert_eq!(git_command_message(&git_command), Some("boom".to_owned()));
    assert_eq!(git_command_message(&invalid_commit), None);
    assert_eq!(
        invalid_commit_message(&invalid_commit),
        Some("deadbeef".to_owned())
    );
    assert_eq!(invalid_commit_message(&git_command), None);
}

#[test]
fn sort_topologically_orders_commits_from_oldest_to_newest() {
    let dir = TempDir::new().or_abort("tempdir");
    init_git_repo(dir.path());
    let ctx = ctx_for(dir.path());

    fs::write(dir.path().join("a.txt"), "a\n").or_abort("write a");
    run_git(&ctx, &["add", "a.txt"]).or_abort("add a");
    run_git(
        &ctx,
        &[
            "-c",
            "commit.template=",
            "-c",
            "core.hooksPath=.git/hooks",
            "commit",
            "--no-verify",
            "--quiet",
            "-m",
            "a",
        ],
    )
    .or_abort("commit a");
    let first_commit =
        CommitSha::new(git_output(&ctx, &["rev-parse", "--verify", "HEAD"]).or_abort("sha a"))
            .or_abort("valid sha a");

    fs::write(dir.path().join("b.txt"), "b\n").or_abort("write b");
    run_git(&ctx, &["add", "b.txt"]).or_abort("add b");
    run_git(
        &ctx,
        &[
            "-c",
            "commit.template=",
            "-c",
            "core.hooksPath=.git/hooks",
            "commit",
            "--no-verify",
            "--quiet",
            "-m",
            "b",
        ],
    )
    .or_abort("commit b");
    let second_commit =
        CommitSha::new(git_output(&ctx, &["rev-parse", "--verify", "HEAD"]).or_abort("sha b"))
            .or_abort("valid sha b");

    let commits = Commits::try_from(BTreeSet::from([
        first_commit.clone(),
        second_commit.clone(),
    ]))
    .or_abort("non-empty set");
    let sorted = sort_topologically(&ctx, &commits).or_abort("sort should succeed");
    let sorted_vec: Vec<_> = sorted.into_iter().collect();
    assert_eq!(sorted_vec, vec![first_commit, second_commit]);
}

#[test]
fn sort_topologically_reports_git_output_failure_for_unknown_commits() {
    let dir = TempDir::new().or_abort("tempdir");
    init_git_repo(dir.path());
    let ctx = ctx_for(dir.path());

    let fake = CommitSha::new("f".repeat(COMMIT_SHA_HEX_LEN)).or_abort("valid sha");
    let commits = Commits::try_from(BTreeSet::from([fake])).or_abort("non-empty set");
    let err = sort_topologically(&ctx, &commits).err_or_abort("git rev-list should fail");
    assert!(
        git_command_message(&err)
            .or_abort("git command errors should expose a message")
            .contains("bad object"),
        "error was: {err:?}"
    );
}

#[test]
fn sort_topologically_reports_empty_sorted_output() {
    struct EmptySortRunner;

    impl Runner for EmptySortRunner {
        fn output(
            &self,
            _bin: &str,
            args: &[&str],
            _envs: &[(&str, Option<&str>)],
            _cwd: &Path,
        ) -> io::Result<Output> {
            if !args.starts_with(&["rev-list", "--reverse", "--topo-order"]) {
                return Err(io::Error::other("unexpected args"));
            }
            Ok(Output {
                status: ExitStatus::from_raw(0),
                stdout: b"\n".to_vec(),
                stderr: Vec::new(),
            })
        }

        fn status(
            &self,
            _bin: &str,
            _args: &[&str],
            _envs: &[(&str, Option<&str>)],
            _quiet: bool,
            _cwd: &Path,
        ) -> io::Result<ExitStatus> {
            Ok(ExitStatus::from_raw(0))
        }
    }

    let dir = TempDir::new().or_abort("tempdir");
    let commit = CommitSha::new("a".repeat(COMMIT_SHA_HEX_LEN)).or_abort("valid sha");
    let commits = Commits::try_from(iter::once(commit).collect::<BTreeSet<_>>())
        .or_abort("non-empty commit set");
    let runner = EmptySortRunner;
    let env = TestEnv {
        cwd: dir.path().to_path_buf(),
    };
    let ctx = Ctx {
        runner: &runner,
        cwd: dir.path().to_path_buf(),
        io: &REAL_IO,
        env: &env,
        fs: &REAL_FS,
    };
    let status = runner
        .status("git", &["status"], &[], false, dir.path())
        .or_abort("status should succeed");
    assert_eq!(status.code(), Some(i32::default()));
    let unexpected = runner
        .output("git", &["unexpected"], &[], dir.path())
        .err_or_abort("unexpected args should fail");
    assert_eq!(unexpected.to_string(), "unexpected args");

    let err = sort_topologically(&ctx, &commits).err_or_abort("empty sorted output should fail");
    let message = git_command_message(&err).or_abort("expected GitCommand");
    assert!(
        message.contains("no commits after sorting"),
        "unexpected error: {err:?}"
    );
}

#[test]
fn init_empty_root_repo_creates_single_empty_root_commit() {
    let dir = TempDir::new().or_abort("tempdir");
    init_empty_root_repo(dir.path());
    let ctx = ctx_for(dir.path());

    let roots = git_output(&ctx, &["rev-list", "--max-parents=0", "HEAD"])
        .or_abort("rev-list should succeed");
    let root_count = roots.lines().filter(|line| !line.trim().is_empty()).count();
    assert_eq!(root_count, 1);

    let root_sha = roots
        .lines()
        .find(|line| !line.trim().is_empty())
        .or_abort("single root sha should exist");
    let tree = git_output(&ctx, &["ls-tree", root_sha]).or_abort("ls-tree should succeed");
    assert!(tree.is_empty(), "root commit should be empty");
}
