use super::output_contracts::{RecordingRunner, successful};
use super::tests::{BufferIo, TestEnv};
use crate::git_factor::{Ctx, REAL_FS, REAL_RUNNER, Runner};
use crate::test_support::OrAbort as _;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::process::Output;
use tempfile::TempDir;

/// Arrange-only resources borrowed by canonical directory contracts.
#[expect(
    clippy::field_scoped_visibility_modifiers,
    reason = "canonical sibling unit and property bodies directly inspect their private test fixture"
)]
pub(in crate::git_factor::git) struct Query {
    pub(in crate::git_factor::git) directory: TempDir,
    pub(in crate::git_factor::git) environment: TestEnv,
    pub(in crate::git_factor::git) io: BufferIo,
    pub(in crate::git_factor::git) runner: RecordingRunner,
}

impl Query {
    /// Configure the actual context to borrow these resources.
    pub(in crate::git_factor::git) fn context(&self) -> Ctx<'_> {
        Ctx {
            cwd: self.environment.cwd.clone(),
            env: &self.environment,
            fs: &REAL_FS,
            io: &self.io,
            runner: &self.runner,
        }
    }
    /// Arrange one captured reply without calling the directory parser.
    pub(in crate::git_factor::git) fn new(reply: io::Result<Output>) -> Self {
        let directory = TempDir::new().or_abort("directory contract resources");
        let environment = TestEnv {
            cwd: directory.path().to_path_buf(),
            trace_log: None,
        };
        Self {
            directory,
            environment,
            io: BufferIo::default(),
            runner: RecordingRunner::new(vec![reply]),
        }
    }
    /// Arrange native output and pre-existing trace bytes.
    pub(in crate::git_factor::git) fn successful(payload: &[u8], terminated: bool) -> Self {
        let mut output = payload.to_vec();
        if terminated {
            output.push(b'\n');
        }
        let mut fixture = Self::new(Ok(successful(&output, b"native warning\n")));
        let trace = fixture.directory.path().join("trace");
        fs::write(&trace, "existing trace bytes\n").or_abort("existing trace");
        fixture.environment.trace_log = Some(trace.into_os_string());
        fixture
    }
}

/// Native fixture; every post-Act observer stays in its canonical body.
#[expect(
    clippy::field_scoped_visibility_modifiers,
    reason = "canonical native unit directly asserts independently arranged conservation facts"
)]
pub(in crate::git_factor::git) struct NativeStatus {
    pub(in crate::git_factor::git) detached_head: Vec<u8>,
    pub(in crate::git_factor::git) directory: TempDir,
    pub(in crate::git_factor::git) environment: TestEnv,
    pub(in crate::git_factor::git) foreign: PathBuf,
    pub(in crate::git_factor::git) head: Vec<u8>,
    pub(in crate::git_factor::git) index: Vec<u8>,
    pub(in crate::git_factor::git) io: BufferIo,
    pub(in crate::git_factor::git) refs: Vec<u8>,
    pub(in crate::git_factor::git) user: PathBuf,
}
impl NativeStatus {
    /// Configure the real dispatcher context without submitting an operation.
    pub(in crate::git_factor::git) fn context(&self) -> Ctx<'_> {
        Ctx {
            cwd: self.environment.cwd.clone(),
            env: &self.environment,
            fs: &REAL_FS,
            io: &self.io,
            runner: &REAL_RUNNER,
        }
    }
    /// Create real native registrations, unrelated work and independent observations.
    #[expect(
        clippy::single_call_fn,
        reason = "one native routing witness needs a complete arrangement independently of its visible dispatcher Act and conservation assertions"
    )]
    pub(in crate::git_factor::git) fn new() -> Self {
        let directory = TempDir::new().or_abort("native directory contract");
        let root = directory.path();
        let commands = [
            vec!["init", "--quiet", "--initial-branch=main"],
            vec![
                "-c",
                "user.name=Directory contract",
                "-c",
                "user.email=directory@example.invalid",
                "commit",
                "--quiet",
                "--allow-empty",
                "-m",
                "Add empty anchor",
            ],
            vec!["worktree", "add", "--quiet", "--detach", "wt", "HEAD"],
            vec!["worktree", "add", "--quiet", "--detach", "wt\u{a0}", "HEAD"],
        ];
        for arguments in commands {
            let output = Runner::output(&REAL_RUNNER, "git", &arguments, root)
                .or_abort("native fixture command");
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        let sibling = root.join(".git/worktrees/wt/factor");
        fs::create_dir_all(&sibling).or_abort("foreign sibling state");
        let foreign = sibling.join("commits");
        fs::write(&foreign, "foreign state bytes\n").or_abort("foreign state marker");
        let cwd = root.join("wt\u{a0}");
        let user = cwd.join("user");
        fs::write(&user, b"unrelated user bytes\0\n").or_abort("unrelated work");
        let environment = TestEnv {
            cwd,
            trace_log: None,
        };
        let io = BufferIo::default();

        let head = Runner::output(&REAL_RUNNER, "git", &["rev-parse", "HEAD"], root)
            .or_abort("arranged HEAD");
        let refs =
            Runner::output(&REAL_RUNNER, "git", &["show-ref"], root).or_abort("arranged refs");
        assert!(head.status.success());
        assert!(refs.status.success());
        let index =
            fs::read(root.join(".git/worktrees/wt\u{a0}/index")).or_abort("arranged raw index");
        let detached_head =
            fs::read(root.join(".git/worktrees/wt\u{a0}/HEAD")).or_abort("arranged detached HEAD");
        Self {
            directory,
            environment,
            io,
            foreign,
            user,
            head: head.stdout,
            refs: refs.stdout,
            index,
            detached_head,
        }
    }
}
