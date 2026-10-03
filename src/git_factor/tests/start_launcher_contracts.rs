use super::*;
use alloc::collections::VecDeque;
use core::num::NonZeroU8;

#[derive(Clone, Debug)]
pub(in crate::git_factor) enum LaunchFault {
    #[cfg(unix)]
    BeginEncoding(String),
    BeginExecutable,
    EditorCanonicalize,
    #[cfg(unix)]
    EditorEncoding(String),
    EditorExecutable,
    NativeLaunch,
    #[cfg(unix)]
    PreflightEncoding(String),
    PreflightExecutable,
    ShortIo,
    ShortRejected(NonZeroU8),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::git_factor) enum LaunchCall {
    Canonicalize(PathBuf),
    CreateDirectory(PathBuf),
    CurrentDirectory,
    CurrentExecutable,
    Exists(PathBuf),
    IsDirectory(PathBuf),
    Read(PathBuf),
    RemoveDirectory(PathBuf),
    RemoveFile(PathBuf),
    Runner(RecordedCall),
    Stderr(String),
    StderrLine(String),
    Stdout(String),
    StdoutLine(String),
    Variable(String),
    Write(PathBuf, String),
}

#[derive(Clone)]
enum PathReply {
    Failure(&'static str),
    Path(PathBuf),
}

#[expect(
    clippy::field_scoped_visibility_modifiers,
    reason = "sibling canonical tests read only deliberately shared fixture facts"
)]
pub(in crate::git_factor) struct LaunchFixture {
    calls: RefCell<Vec<LaunchCall>>,
    canonical: PathReply,
    cwd: PathBuf,
    pub(in crate::git_factor) exec: NonEmptyString,
    executable: RefCell<VecDeque<PathReply>>,
    expected_calls: Vec<LaunchCall>,
    expected_error: String,
    runner: ObservedRunner,
    pub(in crate::git_factor) span: CommitSpan,
    pub(in crate::git_factor) start_head: CommitSha,
    pub(in crate::git_factor) state: StateDir,
}

impl LaunchFixture {
    pub(in crate::git_factor) fn calls(&self) -> Vec<LaunchCall> {
        self.calls.borrow().clone()
    }

    pub(in crate::git_factor) fn ctx(&self) -> Ctx<'_> {
        Ctx {
            cwd: self.cwd.clone(),
            env: self,
            fs: self,
            io: self,
            runner: self,
        }
    }

    pub(in crate::git_factor) fn expected_calls(&self) -> &[LaunchCall] {
        &self.expected_calls
    }

    pub(in crate::git_factor) fn expected_error(&self) -> &str {
        &self.expected_error
    }

    pub(in crate::git_factor) fn new(
        selected: NonEmpty<CommitSha>,
        root: bool,
        fault: &LaunchFault,
    ) -> Self {
        let start_head = (u8::MIN..16)
            .map(|digit| format!("{digit:x}").repeat(SHA_LEN))
            .find(|candidate| selected.iter().all(|sha| sha.as_str() != candidate))
            .map(|candidate| CommitSha::new(candidate).or_abort("admitted start HEAD"))
            .or_abort("sixteen HEAD candidates exceed four selected commits");
        let cwd = PathBuf::from("/contract/launcher");
        let exe = cwd.join("git-factor");
        let mut executable = VecDeque::new();
        let mut canonical = PathReply::Path(exe.clone());
        let mut expected_calls = Vec::new();
        let mut runner = ObservedRunner::default();
        for stage in [
            LaunchStage::Editor,
            LaunchStage::Canonical,
            LaunchStage::Short,
            LaunchStage::Preflight,
            LaunchStage::Begin,
            LaunchStage::Native,
        ] {
            match stage {
                LaunchStage::Editor | LaunchStage::Preflight | LaunchStage::Begin => {
                    expected_calls.push(LaunchCall::CurrentExecutable);
                    executable.push_back(fault.path_reply(stage, &exe));
                }
                LaunchStage::Canonical => {
                    expected_calls.push(LaunchCall::Canonicalize(exe.clone()));
                    canonical = fault.path_reply(stage, &exe);
                }
                LaunchStage::Short => {
                    runner = fault.short_script(&mut expected_calls, &selected, &cwd);
                }
                LaunchStage::Native => {
                    expected_calls.push(LaunchCall::Variable("GIT_FACTOR_TRACE_LOG".to_owned()));
                    expected_calls.push(native_call(&selected, root, &start_head, &cwd));
                    // Native is the second runner request after the short query.
                    runner.io_fault_at = NonZeroUsize::new(2);
                }
            }
            if stage == fault.cutoff() {
                break;
            }
        }
        Self {
            calls: RefCell::new(Vec::new()),
            canonical,
            cwd,
            exec: NonEmptyString::try_from("true".to_owned()).or_abort("exec"),
            executable: RefCell::new(executable),
            expected_calls,
            expected_error: fault.expected_error().to_owned(),
            runner,
            span: CommitSpan::new(
                selected,
                if root {
                    BaseParent::Root
                } else {
                    BaseParent::Commit
                },
            ),
            start_head,
            state: StateDir::new(PathBuf::from("/contract/launcher/.git/factor")),
        }
    }

    pub(in crate::git_factor) fn remaining_executable_replies(&self) -> usize {
        self.executable.borrow().len()
    }

    pub(in crate::git_factor) fn remaining_keys(&self) -> Vec<String> {
        self.runner.remaining_keys()
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum LaunchStage {
    Begin,
    Canonical,
    Editor,
    Native,
    Preflight,
    Short,
}

impl LaunchFault {
    fn cutoff(&self) -> LaunchStage {
        match *self {
            Self::BeginExecutable => LaunchStage::Begin,
            #[cfg(unix)]
            Self::BeginEncoding(_) => LaunchStage::Begin,
            Self::EditorCanonicalize => LaunchStage::Canonical,
            #[cfg(unix)]
            Self::EditorEncoding(_) => LaunchStage::Canonical,
            Self::EditorExecutable => LaunchStage::Editor,
            Self::NativeLaunch => LaunchStage::Native,
            Self::PreflightExecutable => LaunchStage::Preflight,
            #[cfg(unix)]
            Self::PreflightEncoding(_) => LaunchStage::Preflight,
            Self::ShortIo | Self::ShortRejected(_) => LaunchStage::Short,
        }
    }

    fn expected_error(&self) -> &'static str {
        match *self {
            Self::BeginExecutable => {
                "git command failed: cannot resolve current executable: begin executable refused"
            }
            #[cfg(unix)]
            Self::BeginEncoding(_) | Self::PreflightEncoding(_) => {
                "git command failed: current executable path is not valid UTF-8"
            }
            Self::EditorCanonicalize => {
                "git command failed: cannot canonicalize exe: editor canonicalization refused"
            }
            #[cfg(unix)]
            Self::EditorEncoding(_) => "git command failed: editor path is not valid UTF-8",
            Self::EditorExecutable => {
                "git command failed: cannot resolve current exe: editor executable refused"
            }
            Self::NativeLaunch => "git command failed: git rebase: selected query IO failure",
            Self::PreflightExecutable => concat!(
                "git command failed: cannot resolve current executable: ",
                "preflight executable refused",
            ),
            Self::ShortIo => "git command failed: git rev-parse: selected query IO failure",
            Self::ShortRejected(_) => "git command failed: launcher short query refused",
        }
    }

    fn path_reply(&self, stage: LaunchStage, exe: &Path) -> PathReply {
        match (self.clone(), stage) {
            (Self::EditorExecutable, LaunchStage::Editor) => {
                PathReply::Failure("editor executable refused")
            }
            (Self::EditorCanonicalize, LaunchStage::Canonical) => {
                PathReply::Failure("editor canonicalization refused")
            }
            (Self::PreflightExecutable, LaunchStage::Preflight) => {
                PathReply::Failure("preflight executable refused")
            }
            (Self::BeginExecutable, LaunchStage::Begin) => {
                PathReply::Failure("begin executable refused")
            }
            #[cfg(unix)]
            (Self::EditorEncoding(suffix), LaunchStage::Canonical)
            | (Self::PreflightEncoding(suffix), LaunchStage::Preflight)
            | (Self::BeginEncoding(suffix), LaunchStage::Begin) => {
                use std::os::unix::ffi::OsStringExt as _;

                let mut bytes = b"/contract/launcher/".to_vec();
                bytes.push(0xff);
                bytes.extend_from_slice(suffix.as_bytes());
                bytes.extend_from_slice(b"/bin/git-factor");
                PathReply::Path(PathBuf::from(OsString::from_vec(bytes)))
            }
            _ => PathReply::Path(exe.to_path_buf()),
        }
    }

    fn short_script(
        &self,
        calls: &mut Vec<LaunchCall>,
        selected: &NonEmpty<CommitSha>,
        cwd: &Path,
    ) -> ObservedRunner {
        calls.extend([
            LaunchCall::Variable("GIT_FACTOR_TRACE_LOG".to_owned()),
            LaunchCall::Runner(RecordedCall::output(
                "git",
                &["rev-parse", "--short", selected.last().as_str()],
                cwd,
            )),
        ]);
        let runner = match *self {
            Self::ShortIo => ObservedRunner {
                io_fault_at: NonZeroUsize::new(1),
                ..ObservedRunner::default()
            },
            Self::ShortRejected(code) => ObservedRunner::default().with_output_status(
                "git",
                &["rev-parse", "--short", selected.last().as_str()],
                cwd,
                i32::from(code.get()) << 8,
                "abcdef0\n",
                "launcher short query refused\n",
            ),
            Self::BeginExecutable
            | Self::EditorCanonicalize
            | Self::EditorExecutable
            | Self::NativeLaunch
            | Self::PreflightExecutable => ObservedRunner::default().with_output(
                "git",
                &["rev-parse", "--short", selected.last().as_str()],
                cwd,
                "abcdef0\n",
            ),
            #[cfg(unix)]
            Self::BeginEncoding(_) | Self::EditorEncoding(_) | Self::PreflightEncoding(_) => {
                ObservedRunner::default().with_output(
                    "git",
                    &["rev-parse", "--short", selected.last().as_str()],
                    cwd,
                    "abcdef0\n",
                )
            }
        };
        calls.push(LaunchCall::Variable("GIT_FACTOR_TRACE_LOG".to_owned()));
        runner
    }
}

impl Env for LaunchFixture {
    fn current_dir(&self) -> io::Result<PathBuf> {
        self.calls.borrow_mut().push(LaunchCall::CurrentDirectory);
        Err(io::Error::other("unexpected current directory query"))
    }

    fn current_exe(&self) -> io::Result<PathBuf> {
        self.calls.borrow_mut().push(LaunchCall::CurrentExecutable);
        let reply = self.executable.borrow_mut().pop_front();
        match reply {
            Some(PathReply::Path(path)) => Ok(path),
            Some(PathReply::Failure(message)) => Err(io::Error::other(message)),
            None => Err(io::Error::other("unexpected executable query")),
        }
    }

    fn var_os(&self, key: &str) -> Option<OsString> {
        self.calls
            .borrow_mut()
            .push(LaunchCall::Variable(key.to_owned()));
        None
    }
}

impl Fs for LaunchFixture {
    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        self.calls
            .borrow_mut()
            .push(LaunchCall::Canonicalize(path.to_path_buf()));
        match self.canonical.clone() {
            PathReply::Path(value) => Ok(value),
            PathReply::Failure(message) => Err(io::Error::other(message)),
        }
    }

    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        self.calls
            .borrow_mut()
            .push(LaunchCall::CreateDirectory(path.to_path_buf()));
        Err(io::Error::other("unexpected directory creation"))
    }

    fn exists(&self, path: &Path) -> bool {
        self.calls
            .borrow_mut()
            .push(LaunchCall::Exists(path.to_path_buf()));
        false
    }

    fn is_dir(&self, path: &Path) -> bool {
        self.calls
            .borrow_mut()
            .push(LaunchCall::IsDirectory(path.to_path_buf()));
        false
    }

    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        self.calls
            .borrow_mut()
            .push(LaunchCall::Read(path.to_path_buf()));
        Err(io::Error::other("unexpected file read"))
    }

    fn remove_dir_all(&self, path: &Path) -> io::Result<()> {
        self.calls
            .borrow_mut()
            .push(LaunchCall::RemoveDirectory(path.to_path_buf()));
        Err(io::Error::other("unexpected directory removal"))
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        self.calls
            .borrow_mut()
            .push(LaunchCall::RemoveFile(path.to_path_buf()));
        Err(io::Error::other("unexpected file removal"))
    }

    fn write_string(&self, path: &Path, content: &str) -> io::Result<()> {
        self.calls
            .borrow_mut()
            .push(LaunchCall::Write(path.to_path_buf(), content.to_owned()));
        Err(io::Error::other("unexpected file write"))
    }
}

impl Io for LaunchFixture {
    fn err(&self, text: &str) -> io::Result<()> {
        self.calls
            .borrow_mut()
            .push(LaunchCall::Stderr(text.to_owned()));
        Err(io::Error::other("unexpected stderr write"))
    }

    fn errln(&self, line: &str) -> io::Result<()> {
        self.calls
            .borrow_mut()
            .push(LaunchCall::StderrLine(line.to_owned()));
        Err(io::Error::other("unexpected stderr line"))
    }

    fn out(&self, text: &str) -> io::Result<()> {
        self.calls
            .borrow_mut()
            .push(LaunchCall::Stdout(text.to_owned()));
        Err(io::Error::other("unexpected stdout write"))
    }

    fn outln(&self, line: &str) -> io::Result<()> {
        self.calls
            .borrow_mut()
            .push(LaunchCall::StdoutLine(line.to_owned()));
        Err(io::Error::other("unexpected stdout line"))
    }
}

impl Runner for LaunchFixture {
    fn output(&self, bin: &str, args: &[&str], cwd: &Path) -> io::Result<Output> {
        self.calls
            .borrow_mut()
            .push(LaunchCall::Runner(RecordedCall::output(bin, args, cwd)));
        self.runner.output(bin, args, cwd)
    }

    fn status(
        &self,
        bin: &str,
        args: &[&str],
        envs: &[(&str, &str)],
        quiet: bool,
        cwd: &Path,
    ) -> io::Result<ExitStatus> {
        self.calls
            .borrow_mut()
            .push(LaunchCall::Runner(RecordedCall::status(
                bin, args, envs, quiet, cwd,
            )));
        self.runner.status(bin, args, envs, quiet, cwd)
    }
}

#[expect(
    clippy::single_call_fn,
    reason = "independent literal native request recipe, apart from prefix construction"
)]
fn native_call(
    selected: &NonEmpty<CommitSha>,
    root: bool,
    start_head: &CommitSha,
    cwd: &Path,
) -> LaunchCall {
    let index = selected
        .iter()
        .count()
        .checked_sub(1)
        .or_abort("nonempty span");
    let commits = selected
        .iter()
        .map(CommitSha::as_str)
        .collect::<Vec<_>>()
        .join(",");
    let preflight =
        format!("'/contract/launcher/git-factor' 'rebase-exec-preflight' '{index}' 'true'");
    let begin = format!(
        concat!(
            "'/contract/launcher/git-factor' 'rebase-exec-begin' '{index}' ",
            "'{}' '{root}' 'true' '{commits}'",
        ),
        start_head.as_str(),
        index = index,
        root = root,
        commits = commits,
    );
    let sequence = format!(
        concat!(
            "'/contract/launcher/git-sequence-editor' '--factor-target' 'abcdef0' ",
            "'--factor-preflight' '{}' '--factor-begin' '{}'",
        ),
        preflight.replace('\'', "'\\''"),
        begin.replace('\'', "'\\''"),
    );
    let parent = format!("{}^", selected.first());
    LaunchCall::Runner(RecordedCall::status(
        "git",
        &[
            "rebase",
            "--empty",
            "drop",
            "--interactive",
            "--no-autosquash",
            "--no-autostash",
            "--no-rebase-merges",
            "--no-update-refs",
            "--no-stat",
            "--quiet",
            "--reschedule-failed-exec",
            if root { "--root" } else { &parent },
        ],
        &[("GIT_EDITOR", "false"), ("GIT_SEQUENCE_EDITOR", &sequence)],
        false,
        cwd,
    ))
}
