//! `git-sequence-editor` is a strict `GIT_SEQUENCE_EDITOR` helper that rewrites
//! interactive rebase todo lists.

#![expect(
    clippy::print_stderr,
    reason = "Helper prints diagnostics and warnings to stderr for git to surface to the user"
)]

/// CLI argument model for `git-sequence-editor`.
#[path = "git_sequence_editor/cli.rs"]
mod cli;
/// Todo parsing and rewrite logic for `git-sequence-editor`.
#[path = "git_sequence_editor/todo.rs"]
mod todo;

use std::fs;

use clap::Parser as _;

use self::cli::Cli;
use self::todo::{build_requested_actions, rewrite_todo, todo_shas_in, validate_todo_format};

/// Runs the editor logic.
fn run_for(cli: &Cli) -> Result<(), String> {
    let content =
        fs::read_to_string(cli.file()).map_err(|err| format!("failed to read todo file: {err}"))?;

    validate_todo_format(&content)?;
    let todo_shas = todo_shas_in(&content);
    let requested = build_requested_actions(cli, &todo_shas)?;

    let (output, warnings) = rewrite_todo(&content, &requested);

    fs::write(cli.file(), &output).map_err(|err| format!("failed to write todo file: {err}"))?;

    for warning in warnings {
        eprintln!("{warning}");
    }

    Ok(())
}

/// Runs `git-sequence-editor` from parsed CLI arguments and returns an exit code.
#[inline]
#[must_use]
pub fn main_entry() -> i32 {
    let cli = Cli::parse();
    if let Err(message) = run_for(&cli) {
        eprintln!("{message}");
        return 1;
    }
    0
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};
    use std::path::Path;

    use non_empty_string::NonEmptyString;

    use super::todo::{
        Action, TodoSha, is_hex40, parse_todo_action, parse_todo_sha, resolve_requested_sha,
    };
    use super::*;

    #[test]
    fn rewrite_todo_rewrites_action_for_matching_sha_only() {
        let requested =
            BTreeMap::from([(TodoSha::new("def5678").expect("todo sha"), Action::Edit)]);
        let input = "\
pick abc1234 first\n\
pick def5678 second\n\
exec echo hi\n\
";

        let (output, warnings) = rewrite_todo(input, &requested);
        assert_eq!(warnings, Vec::<String>::new());
        assert_eq!(
            output,
            "\
pick abc1234 first\n\
edit def5678 second\n\
exec echo hi\n\
"
        );
    }

    #[test]
    fn rewrite_todo_warns_when_action_is_already_set() {
        let requested =
            BTreeMap::from([(TodoSha::new("abc1234").expect("todo sha"), Action::Pick)]);
        let input = "pick abc1234 first\n";
        let (output, warnings) = rewrite_todo(input, &requested);
        assert_eq!(output, "pick abc1234 first\n");
        assert_eq!(
            warnings,
            vec!["WARN: abc1234: requested 'pick', but todo already had 'pick'".to_owned()]
        );
    }

    #[test]
    fn resolve_requested_sha_requires_exact_match_for_short_sha() {
        let todo_shas = BTreeSet::from([TodoSha::new("abc1234").expect("todo sha")]);
        let sha = NonEmptyString::try_from("abc".to_owned()).expect("non-empty");
        let err = resolve_requested_sha(&sha, &todo_shas).expect_err("must error");
        assert_eq!(err, "sha not present in todo: abc");
    }

    #[test]
    fn is_hex40_accepts_uppercase_hex() {
        let sha = "A".repeat(40);
        assert!(is_hex40(sha.as_str()));
    }

    #[test]
    fn validate_no_duplicates_rejects_duplicates() {
        let shas = vec![
            NonEmptyString::try_from("abc1234".to_owned()).expect("non-empty"),
            NonEmptyString::try_from("abc1234".to_owned()).expect("non-empty"),
        ];
        let err = todo::build_requested_actions(
            &Cli::for_tests(shas, vec![], Path::new("todo").to_path_buf(), vec![]),
            &BTreeSet::from([TodoSha::new("abc1234").expect("todo sha")]),
        )
        .expect_err("must error");
        assert_eq!(err, "duplicate drop sha: abc1234");
    }

    #[test]
    fn build_requested_actions_rejects_cross_action_duplicates() {
        let content = "pick abc1234 first\n";
        let todo_shas = todo_shas_in(content);
        let cli = Cli::for_tests(
            vec![NonEmptyString::try_from("abc1234".to_owned()).expect("non-empty")],
            vec![],
            Path::new("todo").to_path_buf(),
            vec![NonEmptyString::try_from("abc1234".to_owned()).expect("non-empty")],
        );
        let err = build_requested_actions(&cli, &todo_shas).expect_err("must error");
        assert_eq!(err, "sha specified multiple times: abc1234");
    }

    #[test]
    fn todo_sha_new_rejects_empty_token() {
        let err = TodoSha::new("").expect_err("must error");
        assert_eq!(err, "internal error: todo sha was unexpectedly empty");
    }

    #[test]
    fn parse_todo_action_ignores_blank_and_comment_lines() {
        assert_eq!(parse_todo_action(""), None);
        assert_eq!(parse_todo_action("   "), None);
        assert_eq!(parse_todo_action("# comment"), None);
        assert_eq!(parse_todo_action("   # comment"), None);
    }

    #[test]
    fn parse_todo_sha_ignores_blank_comment_and_non_commit_actions() {
        assert_eq!(parse_todo_sha(""), None);
        assert_eq!(parse_todo_sha("   "), None);
        assert_eq!(parse_todo_sha("# comment"), None);
        assert_eq!(parse_todo_sha("   # comment"), None);
        assert_eq!(parse_todo_sha("exec echo hi"), None);
        assert_eq!(parse_todo_sha("break"), None);
        assert_eq!(parse_todo_sha("label topic"), None);
        assert_eq!(parse_todo_sha("reset topic"), None);
        assert_eq!(parse_todo_sha("merge -C deadbeef topic"), None);
        assert_eq!(parse_todo_sha("noop"), None);
        assert_eq!(parse_todo_sha("update-ref refs/heads/main"), None);
    }
}
