//! Arrangement facts for direct renderer contracts; every Act and observation stays in its test.
extern crate alloc;
use super::*;
use crate::non_empty_string::NonEmptyString;
use crate::test_support::OrAbort as _;
use alloc::collections::VecDeque;
use core::cell::{Cell, RefCell};
use core::iter::{once, repeat_with};
use core::num::{NonZeroI32, NonZeroU8, NonZeroUsize};
use std::ffi::OsString;
use std::os::unix::process::ExitStatusExt as _;
use std::process::{ExitStatus, Output};
use tempfile::TempDir;

#[derive(Debug, Eq, PartialEq)]
pub(in crate::git_factor::ui) enum Call {
    Output {
        args: Vec<String>,
        bin: String,
        cwd: PathBuf,
    },
    Status {
        args: Vec<String>,
        bin: String,
        cwd: PathBuf,
        envs: Vec<(String, String)>,
        quiet: bool,
    },
}

#[derive(Debug)]
pub(in crate::git_factor::ui) struct Guidance {
    lines: Vec<&'static str>,
    variable: Option<OsString>,
}
#[derive(Debug)]
pub(in crate::git_factor::ui) struct Reference {
    paths: Vec<&'static str>,
    read: Result<Vec<u8>, io::ErrorKind>,
}
#[derive(Debug)]
pub(in crate::git_factor::ui) struct Remaining {
    input: String,
    lines: Vec<String>,
}
#[expect(
    clippy::field_scoped_visibility_modifiers,
    reason = "nested canonical arrangements set the actual environment cwd inline"
)]
pub(in crate::git_factor::ui) struct HintEnv {
    pub(in crate::git_factor::ui) repo: PathBuf,
    variable: Option<OsString>,
}
#[expect(
    clippy::field_scoped_visibility_modifiers,
    reason = "canonical sibling contracts observe the actual renderer writes inline"
)]
pub(in crate::git_factor::ui) struct HintIo {
    pub(in crate::git_factor::ui) attempted: Cell<usize>,
    responses: RefCell<VecDeque<io::Result<()>>>,
    pub(in crate::git_factor::ui) stderr: RefCell<String>,
    pub(in crate::git_factor::ui) stdout: RefCell<String>,
}
#[expect(
    clippy::field_scoped_visibility_modifiers,
    reason = "canonical sibling contracts observe complete actual requests inline"
)]
pub(in crate::git_factor::ui) struct HintRunner {
    pub(in crate::git_factor::ui) calls: RefCell<Vec<Call>>,
    responses: RefCell<VecDeque<io::Result<Output>>>,
}
#[expect(
    clippy::field_scoped_visibility_modifiers,
    reason = "shared arrangement facts are read by direct unit and property contracts"
)]
pub(in crate::git_factor::ui) struct World {
    pub(in crate::git_factor::ui) directory: TempDir,
    pub(in crate::git_factor::ui) env: HintEnv,
    pub(in crate::git_factor::ui) expected_calls: Vec<Call>,
    pub(in crate::git_factor::ui) expected_reference: Result<Vec<u8>, io::ErrorKind>,
    pub(in crate::git_factor::ui) expected_result: Result<(), String>,
    pub(in crate::git_factor::ui) expected_stdout: String,
    pub(in crate::git_factor::ui) expected_writes: usize,
    pub(in crate::git_factor::ui) io: HintIo,
    pub(in crate::git_factor::ui) remaining: String,
    pub(in crate::git_factor::ui) runner: HintRunner,
}

impl Env for HintEnv {
    fn current_dir(&self) -> io::Result<PathBuf> {
        Ok(self.repo.clone())
    }
    fn current_exe(&self) -> io::Result<PathBuf> {
        Ok(self.repo.join("git-factor"))
    }
    fn var_os(&self, key: &str) -> Option<OsString> {
        (key == "CLAUDECODE")
            .then(|| self.variable.clone())
            .flatten()
    }
}
impl Io for HintIo {
    fn err(&self, text: &str) -> io::Result<()> {
        self.stderr.borrow_mut().push_str(text);
        Ok(())
    }
    fn errln(&self, line: &str) -> io::Result<()> {
        self.err(line)?;
        self.err("\n")
    }
    fn out(&self, text: &str) -> io::Result<()> {
        self.attempted.set(
            self.attempted
                .get()
                .checked_add(1)
                .or_abort("bounded write ordinal"),
        );
        self.responses
            .borrow_mut()
            .pop_front()
            .or_abort("unadmitted renderer write")?;
        self.stdout.borrow_mut().push_str(text);
        Ok(())
    }
    fn outln(&self, line: &str) -> io::Result<()> {
        self.out(line)?;
        self.out("\n")
    }
}
impl Runner for HintRunner {
    fn output(&self, bin: &str, args: &[&str], cwd: &Path) -> io::Result<Output> {
        self.calls.borrow_mut().push(Call::Output {
            args: args.iter().map(|arg| (*arg).to_owned()).collect(),
            bin: bin.to_owned(),
            cwd: cwd.to_path_buf(),
        });
        self.responses
            .borrow_mut()
            .pop_front()
            .or_abort("unadmitted renderer query")
    }
    fn status(
        &self,
        bin: &str,
        args: &[&str],
        envs: &[(&str, &str)],
        quiet: bool,
        cwd: &Path,
    ) -> io::Result<ExitStatus> {
        self.calls.borrow_mut().push(Call::Status {
            args: args.iter().map(|arg| (*arg).to_owned()).collect(),
            bin: bin.to_owned(),
            cwd: cwd.to_path_buf(),
            envs: envs
                .iter()
                .map(|&(key, value)| (key.to_owned(), value.to_owned()))
                .collect(),
            quiet,
        });
        Err(io::Error::other("renderer must not request native status"))
    }
}

pub(in crate::git_factor::ui) fn without_guidance() -> Guidance {
    Guidance {
        lines: Vec::new(),
        variable: None,
    }
}
pub(in crate::git_factor::ui) fn with_guidance() -> Guidance {
    with_guidance_value(OsString::from("1"))
}
pub(in crate::git_factor::ui) fn with_guidance_value(value: OsString) -> Guidance {
    Guidance {
        lines: vec![
            "<claude>",
            "- If context is above 50%, pause and ask the user to /compact.",
            "- Do NOT stop early. Keep committing until \"Complete\".",
            "- Do NOT use git commit directly. ONLY use git-factor --continue.",
            "- Each commit MUST pass the exec gate. No shortcuts.",
            "</claude>",
        ],
        variable: Some(value),
    }
}
pub(in crate::git_factor::ui) fn without_reference() -> Reference {
    Reference {
        paths: Vec::new(),
        read: Err(io::ErrorKind::NotFound),
    }
}
pub(in crate::git_factor::ui) fn with_reference() -> Reference {
    Reference {
        paths: vec!["references/rust.md"],
        read: Ok(b"saved reference".to_vec()),
    }
}
pub(in crate::git_factor::ui) fn without_remaining() -> Remaining {
    Remaining {
        input: String::new(),
        lines: Vec::new(),
    }
}
pub(in crate::git_factor::ui) fn with_remaining(remaining: &NonEmptyString) -> Remaining {
    let input = remaining.as_str().to_owned();
    let lines = vec![format!("  REMAINING: {input}")];
    Remaining { input, lines }
}

fn arrange(reference: &Reference, guidance: &Guidance, remaining: String) -> World {
    let directory = TempDir::new().or_abort("owned renderer world");
    let repo = directory.path();
    fs::create_dir_all(repo.join(".git")).or_abort("owned Git directory");
    fs::write(repo.join(".git/index"), b"saved index").or_abort("save index");
    fs::write(repo.join("user.txt"), b"saved user bytes").or_abort("save user");
    for relative in &reference.paths {
        let path = repo.join(relative);
        fs::create_dir_all(path.parent().or_abort("reference parent"))
            .or_abort("reference directory");
        fs::write(path, b"saved reference").or_abort("reference bytes");
    }
    let env = HintEnv {
        repo: repo.to_path_buf(),
        variable: guidance.variable.clone(),
    };
    World {
        directory,
        env,
        expected_calls: Vec::new(),
        expected_reference: reference.read.clone(),
        expected_result: Ok(()),
        expected_stdout: String::new(),
        expected_writes: 0,
        io: HintIo {
            attempted: Cell::new(0),
            responses: RefCell::new(VecDeque::new()),
            stderr: RefCell::new(String::new()),
            stdout: RefCell::new(String::new()),
        },
        remaining,
        runner: HintRunner {
            calls: RefCell::new(Vec::new()),
            responses: RefCell::new(VecDeque::new()),
        },
    }
}

fn successful_output(stdout: String) -> Output {
    Output {
        status: ExitStatus::from_raw(0),
        stdout: stdout.into_bytes(),
        stderr: Vec::new(),
    }
}
fn query(repo: &Path, args: &[&str]) -> Call {
    Call::Output {
        args: args.iter().map(|arg| (*arg).to_owned()).collect(),
        bin: "git".to_owned(),
        cwd: repo.to_path_buf(),
    }
}
fn output_chunks(
    repo: &Path,
    remaining: Remaining,
    reference: &Reference,
    guidance: &Guidance,
) -> Vec<String> {
    let mut lines = vec![
        "HINTS:".to_owned(),
        "  - Find the ONE smallest addition nothing depends on".to_owned(),
        "  - Target 15-30 lines (50 max)".to_owned(),
        "  - Message: single concrete action, no \"and\"/\"or\"".to_owned(),
        "  - Verify: git log --oneline | wc -l".to_owned(),
        "  - NEVER use git commit. ONLY use git factor --continue.".to_owned(),
    ];
    lines.extend(remaining.lines);
    lines.extend(
        reference
            .paths
            .iter()
            .map(|path| format!("  REFERENCE: {}", repo.join(path).display())),
    );
    lines.push("  RECOVERY: git factor --abort".to_owned());
    lines.extend(guidance.lines.iter().map(|line| (*line).to_owned()));
    lines
        .into_iter()
        .flat_map(|line| [line, "\n".to_owned()])
        .collect()
}

pub(in crate::git_factor::ui) fn complete(
    remaining: Remaining,
    reference: &Reference,
    guidance: &Guidance,
) -> World {
    let input = remaining.input.clone();
    let mut world = arrange(reference, guidance, input);
    let chunks = output_chunks(world.directory.path(), remaining, reference, guidance);
    world.expected_stdout = chunks.concat();
    world.expected_writes = chunks.len();
    world.expected_calls = vec![query(
        world.directory.path(),
        &["rev-parse", "--show-toplevel"],
    )];
    *world.runner.responses.borrow_mut() = VecDeque::from([Ok(successful_output(format!(
        "{}\n",
        world.directory.path().display()
    )))]);
    *world.io.responses.borrow_mut() = repeat_with(|| Ok(())).take(chunks.len()).collect();
    world
}

pub(in crate::git_factor::ui) fn write_failed(
    remaining: Remaining,
    reference: &Reference,
    guidance: &Guidance,
    at: NonZeroUsize,
) -> World {
    let input = remaining.input.clone();
    let mut world = arrange(reference, guidance, input);
    let chunks = output_chunks(world.directory.path(), remaining, reference, guidance);
    let accepted = at.get().checked_sub(1).or_abort("positive output ordinal");
    let (_, prefix) = chunks
        .get(..at.get())
        .or_abort("admitted failing write")
        .split_last()
        .or_abort("positive output ordinal");
    world.expected_stdout = prefix.concat();
    world.expected_writes = at.get();
    world.expected_result = Err("failed to write output: hint output denied".to_owned());
    world.expected_calls = vec![query(
        world.directory.path(),
        &["rev-parse", "--show-toplevel"],
    )];
    *world.runner.responses.borrow_mut() = VecDeque::from([Ok(successful_output(format!(
        "{}\n",
        world.directory.path().display()
    )))]);
    *world.io.responses.borrow_mut() = repeat_with(|| Ok(()))
        .take(accepted)
        .chain(once(Err(io::Error::other("hint output denied"))))
        .collect();
    world
}

pub(in crate::git_factor::ui) fn rejected(
    remaining: String,
    reference: &Reference,
    guidance: &Guidance,
    code: NonZeroU8,
) -> World {
    let mut world = arrange(reference, guidance, remaining);
    world.expected_result = Err("git command failed: hint query refused".to_owned());
    world.expected_calls = vec![query(
        world.directory.path(),
        &["rev-parse", "--show-toplevel"],
    )];
    *world.runner.responses.borrow_mut() = VecDeque::from([Ok(Output {
        status: ExitStatus::from_raw(
            NonZeroI32::from(code)
                .get()
                .checked_mul(256)
                .or_abort("bounded native wait status"),
        ),
        stdout: Vec::new(),
        stderr: b"hint query refused".to_vec(),
    })]);
    world
}

pub(in crate::git_factor::ui) fn spawn_failed(
    remaining: String,
    reference: &Reference,
    guidance: &Guidance,
) -> World {
    let mut world = arrange(reference, guidance, remaining);
    world.expected_result = Err("git command failed: git rev-parse: hint query denied".to_owned());
    world.expected_calls = vec![
        query(world.directory.path(), &["rev-parse", "--show-toplevel"]),
        query(world.directory.path(), &["rev-parse", "--verify", "HEAD"]),
        query(
            world.directory.path(),
            &["rev-parse", "--verify", "HEAD^{tree}"],
        ),
        query(world.directory.path(), &["rev-parse", "--git-dir"]),
        query(world.directory.path(), &["rev-parse", "--show-toplevel"]),
        query(
            world.directory.path(),
            &[
                "--no-optional-locks",
                "status",
                "--porcelain=v1",
                "--untracked-files=all",
            ],
        ),
    ];
    *world.runner.responses.borrow_mut() = VecDeque::from([
        Err(io::Error::other("hint query denied")),
        Err(io::Error::other("snapshot head unavailable")),
        Err(io::Error::other("snapshot head unavailable")),
        Ok(successful_output(".git\n".to_owned())),
        Ok(successful_output(format!(
            "{}\n",
            world.directory.path().display()
        ))),
        Ok(successful_output(String::new())),
    ]);
    world
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_dir_returns_the_owned_repository() {
        let world = complete(without_remaining(), &with_reference(), &without_guidance());
        let repo = world.directory.path();

        let result = world.env.current_dir();

        assert_eq!(
            result.as_ref().map_err(io::Error::kind),
            Ok(&repo.to_path_buf())
        );
        let (stdout, stderr, calls) = (
            world.io.stdout.borrow(),
            world.io.stderr.borrow(),
            world.runner.calls.borrow(),
        );
        assert_eq!(
            (
                stdout.as_str(),
                stderr.as_str(),
                calls.as_slice(),
                world.io.attempted.get(),
                fs::read(repo.join(".git/index")).or_abort("saved index remains"),
                fs::read(repo.join("user.txt")).or_abort("saved user remains"),
                fs::read(repo.join("references/rust.md")).or_abort("saved reference remains")
            ),
            (
                "",
                "",
                [].as_slice(),
                0,
                b"saved index".to_vec(),
                b"saved user bytes".to_vec(),
                b"saved reference".to_vec()
            ),
        );
    }

    #[test]
    fn current_exe_returns_the_owned_program_path() {
        let world = complete(without_remaining(), &with_reference(), &without_guidance());
        let repo = world.directory.path();

        let result = world.env.current_exe();

        assert_eq!(
            result.as_ref().map_err(io::Error::kind),
            Ok(&repo.join("git-factor"))
        );
        let (stdout, stderr, calls) = (
            world.io.stdout.borrow(),
            world.io.stderr.borrow(),
            world.runner.calls.borrow(),
        );
        assert_eq!(
            (
                stdout.as_str(),
                stderr.as_str(),
                calls.as_slice(),
                world.io.attempted.get(),
                fs::read(repo.join(".git/index")).or_abort("saved index remains"),
                fs::read(repo.join("user.txt")).or_abort("saved user remains"),
                fs::read(repo.join("references/rust.md")).or_abort("saved reference remains")
            ),
            (
                "",
                "",
                [].as_slice(),
                0,
                b"saved index".to_vec(),
                b"saved user bytes".to_vec(),
                b"saved reference".to_vec()
            ),
        );
    }

    #[test]
    fn err_records_exact_stderr_bytes() {
        let world = complete(without_remaining(), &with_reference(), &without_guidance());
        let repo = world.directory.path();

        let result = world.io.err("exact stderr");

        assert_eq!(result.as_ref().map_err(io::Error::kind), Ok(&()));
        let (stdout, stderr, calls) = (
            world.io.stdout.borrow(),
            world.io.stderr.borrow(),
            world.runner.calls.borrow(),
        );
        assert_eq!(
            (
                stdout.as_str(),
                stderr.as_str(),
                calls.as_slice(),
                world.io.attempted.get(),
                fs::read(repo.join(".git/index")).or_abort("saved index remains"),
                fs::read(repo.join("user.txt")).or_abort("saved user remains"),
                fs::read(repo.join("references/rust.md")).or_abort("saved reference remains")
            ),
            (
                "",
                "exact stderr",
                [].as_slice(),
                0,
                b"saved index".to_vec(),
                b"saved user bytes".to_vec(),
                b"saved reference".to_vec()
            ),
        );
    }

    #[test]
    fn errln_records_exact_stderr_line() {
        let world = complete(without_remaining(), &with_reference(), &without_guidance());
        let repo = world.directory.path();

        let result = world.io.errln("exact stderr");

        assert_eq!(result.as_ref().map_err(io::Error::kind), Ok(&()));
        let (stdout, stderr, calls) = (
            world.io.stdout.borrow(),
            world.io.stderr.borrow(),
            world.runner.calls.borrow(),
        );
        assert_eq!(
            (
                stdout.as_str(),
                stderr.as_str(),
                calls.as_slice(),
                world.io.attempted.get(),
                fs::read(repo.join(".git/index")).or_abort("saved index remains"),
                fs::read(repo.join("user.txt")).or_abort("saved user remains"),
                fs::read(repo.join("references/rust.md")).or_abort("saved reference remains")
            ),
            (
                "",
                "exact stderr\n",
                [].as_slice(),
                0,
                b"saved index".to_vec(),
                b"saved user bytes".to_vec(),
                b"saved reference".to_vec()
            ),
        );
    }

    #[test]
    fn status_records_and_refuses_the_actual_request() {
        let world = complete(without_remaining(), &with_reference(), &without_guidance());
        let repo = world.directory.path();

        let result = world.runner.status(
            "unexpected-program",
            &["first", "second"],
            &[("OWNED", "value")],
            true,
            repo,
        );

        let error = result.err().or_abort("forbidden status is refused");
        assert_eq!(
            (error.kind(), error.to_string()),
            (
                io::ErrorKind::Other,
                "renderer must not request native status".to_owned()
            )
        );
        let (stdout, stderr, calls) = (
            world.io.stdout.borrow(),
            world.io.stderr.borrow(),
            world.runner.calls.borrow(),
        );
        assert_eq!(
            (
                stdout.as_str(),
                stderr.as_str(),
                calls.as_slice(),
                world.io.attempted.get(),
                fs::read(repo.join(".git/index")).or_abort("saved index remains"),
                fs::read(repo.join("user.txt")).or_abort("saved user remains"),
                fs::read(repo.join("references/rust.md")).or_abort("saved reference remains")
            ),
            (
                "",
                "",
                [Call::Status {
                    args: vec!["first".to_owned(), "second".to_owned()],
                    bin: "unexpected-program".to_owned(),
                    cwd: repo.to_path_buf(),
                    envs: vec![("OWNED".to_owned(), "value".to_owned())],
                    quiet: true,
                }]
                .as_slice(),
                0,
                b"saved index".to_vec(),
                b"saved user bytes".to_vec(),
                b"saved reference".to_vec()
            ),
        );
    }
}
