//! Real-file arrangements and strict query observations for repository snapshots.
use crate::git_factor::{COMMIT_SHA_HEX_LEN, REAL_ENV, REAL_FS, REAL_IO, Runner};

use super::*;
use crate::test_support::OrAbort as _;
use alloc::collections::VecDeque;
use core::cell::RefCell;
use std::io;
use std::os::unix::process::ExitStatusExt as _;
use std::process::{ExitStatus, Output};
use tempfile::TempDir;

/// Distinct query outcomes consumed by the snapshot observer.
#[derive(Clone, Copy, Debug)]
pub(in crate::git_factor::trace) enum Reply {
    /// The child succeeds without a value.
    Empty,
    /// The child returns the arranged value.
    Present,
    /// The child rejects the query with exit 128 and empty stdout.
    Rejected,
    /// The child cannot be launched.
    Unavailable,
}

/// The directory observation determines whether metadata queries are possible.
#[derive(Clone, Copy, Debug)]
pub(in crate::git_factor::trace) enum Directory {
    /// Git reports a directory and the given top-level query outcome.
    Available(Reply),
    /// Git rejects the directory query with exit 128.
    Rejected,
    /// Git cannot report its directory.
    Unavailable,
}

/// Constructive current-journal observations, independent of session admission.
#[derive(Clone, Copy, Debug)]
pub(in crate::git_factor::trace) enum Factor {
    /// No journal exists.
    Absent,
    /// A complete current journal records the selected phase.
    Current(JournalPhase),
    /// The journal cannot be parsed as the current schema.
    Malformed,
    /// The journal pathname is a directory and cannot be read as text.
    Unreadable,
}

/// The six current phases have independently expected diagnostic source semantics.
#[derive(Clone, Copy, Debug)]
pub(in crate::git_factor::trace) enum JournalPhase {
    Closing,
    Opening,
    Preparing,
    Replaying,
    Selecting,
    Verified,
}

/// Constructive native rebase-file worlds.
#[derive(Clone, Copy, Debug)]
pub(in crate::git_factor::trace) enum Rebase {
    /// Neither native directory exists.
    Absent,
    /// Apply metadata includes a patch file.
    Apply,
    /// Apply metadata has no patch and invalid counters.
    ApplyMissing,
    /// Both directories exist; merge metadata takes precedence.
    Merge,
    /// Merge todo has only comments and done has only whitespace.
    MergeEmpty,
}

/// Input facts assembled independently of the snapshot implementation.
#[derive(Clone, Copy, Debug)]
#[expect(
    clippy::field_scoped_visibility_modifiers,
    reason = "canonical sibling providers arrange the private snapshot fixture facts"
)]
pub(in crate::git_factor::trace) struct World {
    pub(in crate::git_factor::trace) directory: Directory,
    pub(in crate::git_factor::trace) factor: Factor,
    pub(in crate::git_factor::trace) head: Reply,
    pub(in crate::git_factor::trace) rebase: Rebase,
    pub(in crate::git_factor::trace) tree: Reply,
}

impl World {
    /// Arranges a complete journal without an active rebase.
    pub(in crate::git_factor::trace) const fn complete() -> Self {
        Self {
            directory: Directory::Available(Reply::Present),
            factor: Factor::Current(JournalPhase::Selecting),
            head: Reply::Present,
            rebase: Rebase::Absent,
            tree: Reply::Present,
        }
    }
}

/// One expected query and its independently arranged reply.
struct Query {
    args: Vec<String>,
    reply: io::Result<Output>,
}

/// Strict read-only child-process capability, scoped to one temporary repository.
struct Queries {
    cwd: PathBuf,
    remaining: RefCell<VecDeque<Query>>,
}

impl Runner for Queries {
    #[expect(
        clippy::panic_in_result_fn,
        reason = "query identity violations must fail instead of becoming optional missing metadata"
    )]
    fn output(
        &self,
        bin: &str,
        args: &[&str],
        _envs: &[(&str, Option<&str>)],
        cwd: &Path,
    ) -> io::Result<Output> {
        assert_eq!(bin, "git");
        assert_eq!(cwd, self.cwd);
        let query = self
            .remaining
            .borrow_mut()
            .pop_front()
            .or_abort("expected query");
        assert_eq!(args, query.args);
        query.reply
    }

    #[expect(
        clippy::panic,
        clippy::panic_in_result_fn,
        reason = "a mutating launch violates the read-only snapshot contract"
    )]
    fn status(
        &self,
        _bin: &str,
        _args: &[&str],
        _envs: &[(&str, Option<&str>)],
        _quiet: bool,
        _cwd: &Path,
    ) -> io::Result<ExitStatus> {
        panic!("snapshot observation must not launch a mutating command")
    }
}

/// Owned arrangement with an independent complete readback oracle.
pub(in crate::git_factor::trace) struct Arrangement {
    expected: RepoSnapshot,
    files: Vec<(PathBuf, String)>,
    queries: Queries,
    root: TempDir,
}

impl Arrangement {
    fn arrange_factor(&mut self, factor: Factor) {
        match factor {
            Factor::Absent => {}
            Factor::Malformed => self.write(".git/factor-journal.json", "{not a journal}"),
            Factor::Unreadable => {
                fs::create_dir_all(self.root.path().join(".git/factor-journal.json"))
                    .or_abort("unreadable journal directory");
            }
            Factor::Current(phase) => {
                let checkpoint = "a".repeat(COMMIT_SHA_HEX_LEN);
                let final_tree = "b".repeat(COMMIT_SHA_HEX_LEN);
                let source = "c".repeat(COMMIT_SHA_HEX_LEN);
                let anchor = "d".repeat(COMMIT_SHA_HEX_LEN);
                let base = "e".repeat(COMMIT_SHA_HEX_LEN);
                let lease = "f".repeat(COMMIT_SHA_HEX_LEN);
                let (phase_name, has_source, state) = match phase {
                    JournalPhase::Closing => (
                        "closing",
                        false,
                        format!(
                            r#"{{"phase":"closing","lease":"{lease}","outcome":{{"result":"complete","atom":"{source}"}}}}"#
                        ),
                    ),
                    JournalPhase::Opening => (
                        "opening",
                        true,
                        format!(
                            r#"{{"phase":"opening","lease":"{lease}","source":"{source}","anchor":"{anchor}","base":"{base}","head":"{base}"}}"#
                        ),
                    ),
                    JournalPhase::Preparing => (
                        "preparing",
                        false,
                        format!(
                            r#"{{"phase":"preparing","tip":"{anchor}","base":"{base}","previous_lease":null}}"#
                        ),
                    ),
                    JournalPhase::Replaying => (
                        "replaying",
                        true,
                        format!(
                            r#"{{"phase":"replaying","lease":"{lease}","source":"{source}","anchor":"{anchor}","base":"{base}","head":"{base}","atom":"{source}","remainder":{{"state":"absent"}},"accepted":[]}}"#
                        ),
                    ),
                    JournalPhase::Selecting => (
                        "selecting",
                        true,
                        format!(
                            r#"{{"phase":"selecting","lease":"{lease}","source":"{source}","anchor":"{anchor}","base":"{base}","head":"{base}"}}"#
                        ),
                    ),
                    JournalPhase::Verified => (
                        "verified",
                        false,
                        format!(
                            r#"{{"phase":"verified","lease":"{lease}","atom":"{source}","anchor":"{anchor}","base":"{base}","tip":"{checkpoint}","remainder":{{"state":"absent"}}}}"#
                        ),
                    ),
                };
                let journal = format!(
                    r#"{{"branch":"refs/heads/main","checkpoint":"{checkpoint}","final_tree":"{final_tree}","format":"checkpoint_v2","gates":[],"original_base":"{base}","original_tip":"{anchor}","session":"{lease}","state":{state}}}"#
                );
                self.write(".git/factor-journal.json", &journal);
                self.expected.factor_checkpoint = Some(checkpoint);
                self.expected.factor_final_tree = Some(final_tree);
                self.expected.factor_phase = Some(phase_name.to_owned());
                self.expected.factor_source = has_source.then_some(source);
            }
        }
    }

    #[expect(
        clippy::literal_string_with_formatting_args,
        reason = "Git's tree revision syntax is literal input"
    )]
    fn arrange_queries(&mut self, world: World, token: &str) {
        let head = format!("head-{token}");
        let tree = format!("tree-{token}");
        self.query(&["rev-parse", "--verify", "HEAD"], world.head, &head);
        self.expected.head = expected_reply(world.head, head);
        self.query(&["rev-parse", "--verify", "HEAD^{tree}"], world.tree, &tree);
        self.expected.head_tree = expected_reply(world.tree, tree);
        match world.directory {
            Directory::Unavailable => {
                self.query(&["rev-parse", "--git-dir"], Reply::Unavailable, "");
            }
            Directory::Rejected => {
                self.query(&["rev-parse", "--git-dir"], Reply::Rejected, "");
            }
            Directory::Available(reply) => {
                self.query(&["rev-parse", "--git-dir"], Reply::Present, ".git");
                self.expected.git_dir =
                    Some(self.root.path().join(".git").to_string_lossy().into_owned());
                let top = format!("/repository/{token}");
                self.query(&["rev-parse", "--show-toplevel"], reply, &top);
                self.expected.toplevel = expected_reply(reply, top);
            }
        }
        let paths = format!("MM staged-{token}\n M unstaged-{token}\n?? untracked-{token}");
        self.query(
            &[
                "--no-optional-locks",
                "status",
                "--porcelain=v1",
                "--untracked-files=all",
            ],
            Reply::Present,
            &paths,
        );
        self.expected.staged_paths = vec![format!("staged-{token}")];
        self.expected.unstaged_paths = vec![format!("staged-{token}"), format!("unstaged-{token}")];
        self.expected.untracked_paths = vec![format!("untracked-{token}")];
    }

    fn arrange_rebase(&mut self, rebase: Rebase, token: &str, count: u32) {
        match rebase {
            Rebase::Absent => {}
            Rebase::Merge => {
                self.write(".git/rebase-apply/next", "999");
                self.write(".git/rebase-merge/msgnum", &format!(" {count}\n"));
                self.write(".git/rebase-merge/end", "17");
                self.write(
                    ".git/rebase-merge/git-rebase-todo",
                    &format!(" \n# comment\n \n pick {token} selected\nexec later\n"),
                );
                self.write(
                    ".git/rebase-merge/done",
                    &format!("pick earlier\n exec {token} \n \n"),
                );
                self.expected.rebase_state = Some(RebaseState::Merge);
                self.expected.rebase_msgnum = Some(RebaseCounter(count));
                self.expected.rebase_end = Some(RebaseCounter(17));
                self.expected.rebase_todo_head = Some(format!("pick {token} selected"));
                self.expected.rebase_done_tail = Some(format!("exec {token}"));
            }
            Rebase::MergeEmpty => {
                self.write(".git/rebase-merge/msgnum", "invalid");
                self.write(
                    ".git/rebase-merge/git-rebase-todo",
                    "# comment\n # another\n",
                );
                self.write(".git/rebase-merge/done", " \n\t\n");
                self.expected.rebase_state = Some(RebaseState::Merge);
            }
            Rebase::Apply => {
                self.write(".git/rebase-apply/next", &format!(" {count}\n"));
                self.write(".git/rebase-apply/last", "17");
                self.write(".git/rebase-apply/patch", token);
                self.expected.rebase_state = Some(RebaseState::Apply);
                self.expected.rebase_msgnum = Some(RebaseCounter(count));
                self.expected.rebase_end = Some(RebaseCounter(17));
                self.expected.rebase_todo_head = Some("patch".to_owned());
            }
            Rebase::ApplyMissing => {
                self.write(".git/rebase-apply/next", "invalid");
                self.expected.rebase_state = Some(RebaseState::Apply);
            }
        }
    }

    /// Checks every snapshot field, complete query consumption, and arranged file bytes.
    pub(in crate::git_factor::trace) fn assert_observation(&self, result: &RepoSnapshot) {
        assert_eq!(result.factor_checkpoint, self.expected.factor_checkpoint);
        assert_eq!(result.factor_final_tree, self.expected.factor_final_tree);
        assert_eq!(result.factor_phase, self.expected.factor_phase);
        assert_eq!(result.factor_source, self.expected.factor_source);
        assert_eq!(result.git_dir, self.expected.git_dir);
        assert_eq!(result.head, self.expected.head);
        assert_eq!(result.head_tree, self.expected.head_tree);
        assert_eq!(result.rebase_done_tail, self.expected.rebase_done_tail);
        assert_eq!(result.rebase_end, self.expected.rebase_end);
        assert_eq!(result.rebase_msgnum, self.expected.rebase_msgnum);
        assert_eq!(result.rebase_state, self.expected.rebase_state);
        assert_eq!(result.rebase_todo_head, self.expected.rebase_todo_head);
        assert_eq!(result.staged_paths, self.expected.staged_paths);
        assert_eq!(result.toplevel, self.expected.toplevel);
        assert_eq!(result.unstaged_paths, self.expected.unstaged_paths);
        assert_eq!(result.untracked_paths, self.expected.untracked_paths);
        assert!(self.queries.remaining.borrow().is_empty());
        for entry in &self.files {
            let path = &entry.0;
            let contents = &entry.1;
            assert_eq!(
                fs::read_to_string(path).or_abort("preserved metadata"),
                *contents
            );
        }
    }

    /// Supplies only the observer's existing process/filesystem capabilities.
    pub(in crate::git_factor::trace) fn context(&self) -> Ctx<'_> {
        Ctx {
            cwd: self.root.path().to_path_buf(),
            env: &REAL_ENV,
            fs: &REAL_FS,
            io: &REAL_IO,
            runner: &self.queries,
        }
    }

    /// Arranges real metadata files and exact child replies without invoking the collector.
    pub(in crate::git_factor::trace) fn new(world: World, token: &str, count: u32) -> Self {
        let root = TempDir::new().or_abort("temporary snapshot repository");
        let cwd = root.path().to_path_buf();
        let git_dir = cwd.join(".git");
        fs::create_dir_all(&git_dir).or_abort("arrange Git metadata directory");
        let mut arrangement = Self {
            expected: RepoSnapshot::default(),
            files: Vec::new(),
            queries: Queries {
                cwd,
                remaining: RefCell::new(VecDeque::new()),
            },
            root,
        };
        arrangement.write("unrelated", "preserved bytes");
        arrangement.arrange_queries(world, token);
        if matches!(world.directory, Directory::Available(_)) {
            arrangement.arrange_factor(world.factor);
            arrangement.arrange_rebase(world.rebase, token, count);
        }
        arrangement
    }

    fn query(&self, args: &[&str], reply: Reply, value: &str) {
        let output = match reply {
            Reply::Unavailable => Err(io::Error::new(io::ErrorKind::NotFound, "query unavailable")),
            Reply::Rejected => Ok(Output {
                status: ExitStatus::from_raw(128 << 8),
                stdout: Vec::new(),
                stderr: b"fatal: query rejected\n".to_vec(),
            }),
            Reply::Empty => Ok(Output {
                status: ExitStatus::from_raw(0),
                stdout: Vec::new(),
                stderr: Vec::new(),
            }),
            Reply::Present => Ok(Output {
                status: ExitStatus::from_raw(0),
                stdout: format!("{value}\n").into_bytes(),
                stderr: Vec::new(),
            }),
        };
        self.queries.remaining.borrow_mut().push_back(Query {
            args: args.iter().map(|arg| (*arg).to_owned()).collect(),
            reply: output,
        });
    }

    fn write(&mut self, relative: &str, value: &str) {
        let path = self.root.path().join(relative);
        let parent = path.parent().or_abort("metadata file parent");
        fs::create_dir_all(parent).or_abort("metadata parent");
        fs::write(&path, value).or_abort("metadata file");
        self.files.push((path, value.to_owned()));
    }
}

fn expected_reply(reply: Reply, value: String) -> Option<String> {
    match reply {
        Reply::Unavailable | Reply::Empty | Reply::Rejected => None,
        Reply::Present => Some(value),
    }
}
