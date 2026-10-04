use super::public_cli::PublicCli;
use super::*;
use crate::git_factor::engine::tests::{LogPath, MessageSelection};
use EXIT_SOFTWARE;
use core::iter;

#[test]
fn abort_status() {
    let fixture = PublicCli::pure();
    let before = fixture.facts();
    let arguments = iter::once("git-factor")
        .chain(["--abort", "--status"].iter().copied())
        .map(OsString::from)
        .collect::<Vec<_>>();

    let code = main_entry_with_vec(fixture.context().io, Ok(fixture.context()), &arguments);

    assert_eq!(code, EXIT_USAGE);
    assert_eq!(fixture.stdout(), "");
    assert_eq!(
        fixture.stderr(),
        format!("{}\n", "--abort cannot be combined with other options")
    );
    assert_eq!(fixture.calls(), Vec::<String>::new());
    assert_eq!(fixture.facts(), before);
}

#[test]
fn abort_continue() {
    let fixture = PublicCli::pure();
    let before = fixture.facts();
    let arguments = iter::once("git-factor")
        .chain(["--abort", "--continue"].iter().copied())
        .map(OsString::from)
        .collect::<Vec<_>>();

    let code = main_entry_with_vec(fixture.context().io, Ok(fixture.context()), &arguments);

    assert_eq!(code, EXIT_USAGE);
    assert_eq!(fixture.stdout(), "");
    assert_eq!(
        fixture.stderr(),
        format!("{}\n", "--abort cannot be combined with other options")
    );
    assert_eq!(fixture.calls(), Vec::<String>::new());
    assert_eq!(fixture.facts(), before);
}

#[test]
fn abort_retry() {
    let fixture = PublicCli::pure();
    let before = fixture.facts();
    let arguments = iter::once("git-factor")
        .chain(["--abort", "--retry"].iter().copied())
        .map(OsString::from)
        .collect::<Vec<_>>();

    let code = main_entry_with_vec(fixture.context().io, Ok(fixture.context()), &arguments);

    assert_eq!(code, EXIT_USAGE);
    assert_eq!(fixture.stdout(), "");
    assert_eq!(
        fixture.stderr(),
        format!("{}\n", "--abort cannot be combined with other options")
    );
    assert_eq!(fixture.calls(), Vec::<String>::new());
    assert_eq!(fixture.facts(), before);
}

#[test]
fn abort_finish() {
    let fixture = PublicCli::pure();
    let before = fixture.facts();
    let arguments = iter::once("git-factor")
        .chain(["--abort", "--finish"].iter().copied())
        .map(OsString::from)
        .collect::<Vec<_>>();

    let code = main_entry_with_vec(fixture.context().io, Ok(fixture.context()), &arguments);

    assert_eq!(code, EXIT_USAGE);
    assert_eq!(fixture.stdout(), "");
    assert_eq!(
        fixture.stderr(),
        format!("{}\n", "--abort cannot be combined with other options")
    );
    assert_eq!(fixture.calls(), Vec::<String>::new());
    assert_eq!(fixture.facts(), before);
}

#[test]
fn abort_exec() {
    let fixture = PublicCli::pure();
    let before = fixture.facts();
    let arguments = iter::once("git-factor")
        .chain(["--abort", "--exec", "true"].iter().copied())
        .map(OsString::from)
        .collect::<Vec<_>>();

    let code = main_entry_with_vec(fixture.context().io, Ok(fixture.context()), &arguments);

    assert_eq!(code, EXIT_USAGE);
    assert_eq!(fixture.stdout(), "");
    assert_eq!(
        fixture.stderr(),
        format!("{}\n", "--abort cannot be combined with other options")
    );
    assert_eq!(fixture.calls(), Vec::<String>::new());
    assert_eq!(fixture.facts(), before);
}

#[test]
fn abort_head() {
    let fixture = PublicCli::pure();
    let before = fixture.facts();
    let arguments = iter::once("git-factor")
        .chain(["--abort", "HEAD"].iter().copied())
        .map(OsString::from)
        .collect::<Vec<_>>();

    let code = main_entry_with_vec(fixture.context().io, Ok(fixture.context()), &arguments);

    assert_eq!(code, EXIT_USAGE);
    assert_eq!(fixture.stdout(), "");
    assert_eq!(
        fixture.stderr(),
        format!("{}\n", "--abort cannot be combined with other options")
    );
    assert_eq!(fixture.calls(), Vec::<String>::new());
    assert_eq!(fixture.facts(), before);
}

#[test]
fn status_continue() {
    let fixture = PublicCli::pure();
    let before = fixture.facts();
    let arguments = iter::once("git-factor")
        .chain(["--status", "--continue"].iter().copied())
        .map(OsString::from)
        .collect::<Vec<_>>();

    let code = main_entry_with_vec(fixture.context().io, Ok(fixture.context()), &arguments);

    assert_eq!(code, EXIT_USAGE);
    assert_eq!(fixture.stdout(), "");
    assert_eq!(
        fixture.stderr(),
        format!("{}\n", "--status cannot be combined with other options")
    );
    assert_eq!(fixture.calls(), Vec::<String>::new());
    assert_eq!(fixture.facts(), before);
}

#[test]
fn status_retry() {
    let fixture = PublicCli::pure();
    let before = fixture.facts();
    let arguments = iter::once("git-factor")
        .chain(["--status", "--retry"].iter().copied())
        .map(OsString::from)
        .collect::<Vec<_>>();

    let code = main_entry_with_vec(fixture.context().io, Ok(fixture.context()), &arguments);

    assert_eq!(code, EXIT_USAGE);
    assert_eq!(fixture.stdout(), "");
    assert_eq!(
        fixture.stderr(),
        format!("{}\n", "--status cannot be combined with other options")
    );
    assert_eq!(fixture.calls(), Vec::<String>::new());
    assert_eq!(fixture.facts(), before);
}

#[test]
fn status_finish() {
    let fixture = PublicCli::pure();
    let before = fixture.facts();
    let arguments = iter::once("git-factor")
        .chain(["--status", "--finish"].iter().copied())
        .map(OsString::from)
        .collect::<Vec<_>>();

    let code = main_entry_with_vec(fixture.context().io, Ok(fixture.context()), &arguments);

    assert_eq!(code, EXIT_USAGE);
    assert_eq!(fixture.stdout(), "");
    assert_eq!(
        fixture.stderr(),
        format!("{}\n", "--status cannot be combined with other options")
    );
    assert_eq!(fixture.calls(), Vec::<String>::new());
    assert_eq!(fixture.facts(), before);
}

#[test]
fn status_exec() {
    let fixture = PublicCli::pure();
    let before = fixture.facts();
    let arguments = iter::once("git-factor")
        .chain(["--status", "--exec", "true"].iter().copied())
        .map(OsString::from)
        .collect::<Vec<_>>();

    let code = main_entry_with_vec(fixture.context().io, Ok(fixture.context()), &arguments);

    assert_eq!(code, EXIT_USAGE);
    assert_eq!(fixture.stdout(), "");
    assert_eq!(
        fixture.stderr(),
        format!("{}\n", "--status cannot be combined with other options")
    );
    assert_eq!(fixture.calls(), Vec::<String>::new());
    assert_eq!(fixture.facts(), before);
}

#[test]
fn status_head() {
    let fixture = PublicCli::pure();
    let before = fixture.facts();
    let arguments = iter::once("git-factor")
        .chain(["--status", "HEAD"].iter().copied())
        .map(OsString::from)
        .collect::<Vec<_>>();

    let code = main_entry_with_vec(fixture.context().io, Ok(fixture.context()), &arguments);

    assert_eq!(code, EXIT_USAGE);
    assert_eq!(fixture.stdout(), "");
    assert_eq!(
        fixture.stderr(),
        format!("{}\n", "--status cannot be combined with other options")
    );
    assert_eq!(fixture.calls(), Vec::<String>::new());
    assert_eq!(fixture.facts(), before);
}

#[test]
fn status_message() {
    let fixture = PublicCli::pure();
    let before = fixture.facts();
    let arguments = iter::once("git-factor")
        .chain(["--status", "--message", "true"].iter().copied())
        .map(OsString::from)
        .collect::<Vec<_>>();

    let code = main_entry_with_vec(fixture.context().io, Ok(fixture.context()), &arguments);

    assert_eq!(code, EXIT_USAGE);
    assert_eq!(fixture.stdout(), "");
    assert_eq!(
        fixture.stderr(),
        format!("{}\n", "--status cannot be combined with other options")
    );
    assert_eq!(fixture.calls(), Vec::<String>::new());
    assert_eq!(fixture.facts(), before);
}

#[test]
fn retry_continue() {
    let fixture = PublicCli::pure();
    let before = fixture.facts();
    let arguments = iter::once("git-factor")
        .chain(["--retry", "--continue"].iter().copied())
        .map(OsString::from)
        .collect::<Vec<_>>();

    let code = main_entry_with_vec(fixture.context().io, Ok(fixture.context()), &arguments);

    assert_eq!(code, EXIT_USAGE);
    assert_eq!(fixture.stdout(), "");
    assert_eq!(
        fixture.stderr(),
        format!("{}\n", "--retry cannot be combined with other options")
    );
    assert_eq!(fixture.calls(), Vec::<String>::new());
    assert_eq!(fixture.facts(), before);
}

#[test]
fn retry_finish() {
    let fixture = PublicCli::pure();
    let before = fixture.facts();
    let arguments = iter::once("git-factor")
        .chain(["--retry", "--finish"].iter().copied())
        .map(OsString::from)
        .collect::<Vec<_>>();

    let code = main_entry_with_vec(fixture.context().io, Ok(fixture.context()), &arguments);

    assert_eq!(code, EXIT_USAGE);
    assert_eq!(fixture.stdout(), "");
    assert_eq!(
        fixture.stderr(),
        format!("{}\n", "--retry cannot be combined with other options")
    );
    assert_eq!(fixture.calls(), Vec::<String>::new());
    assert_eq!(fixture.facts(), before);
}

#[test]
fn retry_exec() {
    let fixture = PublicCli::pure();
    let before = fixture.facts();
    let arguments = iter::once("git-factor")
        .chain(["--retry", "--exec", "true"].iter().copied())
        .map(OsString::from)
        .collect::<Vec<_>>();

    let code = main_entry_with_vec(fixture.context().io, Ok(fixture.context()), &arguments);

    assert_eq!(code, EXIT_USAGE);
    assert_eq!(fixture.stdout(), "");
    assert_eq!(
        fixture.stderr(),
        format!("{}\n", "--retry cannot be combined with other options")
    );
    assert_eq!(fixture.calls(), Vec::<String>::new());
    assert_eq!(fixture.facts(), before);
}

#[test]
fn retry_head() {
    let fixture = PublicCli::pure();
    let before = fixture.facts();
    let arguments = iter::once("git-factor")
        .chain(["--retry", "HEAD"].iter().copied())
        .map(OsString::from)
        .collect::<Vec<_>>();

    let code = main_entry_with_vec(fixture.context().io, Ok(fixture.context()), &arguments);

    assert_eq!(code, EXIT_USAGE);
    assert_eq!(fixture.stdout(), "");
    assert_eq!(
        fixture.stderr(),
        format!("{}\n", "--retry cannot be combined with other options")
    );
    assert_eq!(fixture.calls(), Vec::<String>::new());
    assert_eq!(fixture.facts(), before);
}

#[test]
fn retry_message() {
    let fixture = PublicCli::pure();
    let before = fixture.facts();
    let arguments = iter::once("git-factor")
        .chain(["--retry", "--message", "true"].iter().copied())
        .map(OsString::from)
        .collect::<Vec<_>>();

    let code = main_entry_with_vec(fixture.context().io, Ok(fixture.context()), &arguments);

    assert_eq!(code, EXIT_USAGE);
    assert_eq!(fixture.stdout(), "");
    assert_eq!(
        fixture.stderr(),
        format!("{}\n", "--retry cannot be combined with other options")
    );
    assert_eq!(fixture.calls(), Vec::<String>::new());
    assert_eq!(fixture.facts(), before);
}

#[test]
fn finish_continue() {
    let fixture = PublicCli::pure();
    let before = fixture.facts();
    let arguments = iter::once("git-factor")
        .chain(["--finish", "--continue"].iter().copied())
        .map(OsString::from)
        .collect::<Vec<_>>();

    let code = main_entry_with_vec(fixture.context().io, Ok(fixture.context()), &arguments);

    assert_eq!(code, EXIT_USAGE);
    assert_eq!(fixture.stdout(), "");
    assert_eq!(
        fixture.stderr(),
        format!(
            "{}\n",
            "--finish cannot be combined with --continue, --exec, or COMMIT"
        )
    );
    assert_eq!(fixture.calls(), Vec::<String>::new());
    assert_eq!(fixture.facts(), before);
}

#[test]
fn finish_exec() {
    let fixture = PublicCli::pure();
    let before = fixture.facts();
    let arguments = iter::once("git-factor")
        .chain(["--finish", "--exec", "true"].iter().copied())
        .map(OsString::from)
        .collect::<Vec<_>>();

    let code = main_entry_with_vec(fixture.context().io, Ok(fixture.context()), &arguments);

    assert_eq!(code, EXIT_USAGE);
    assert_eq!(fixture.stdout(), "");
    assert_eq!(
        fixture.stderr(),
        format!(
            "{}\n",
            "--finish cannot be combined with --continue, --exec, or COMMIT"
        )
    );
    assert_eq!(fixture.calls(), Vec::<String>::new());
    assert_eq!(fixture.facts(), before);
}

#[test]
fn finish_head() {
    let fixture = PublicCli::pure();
    let before = fixture.facts();
    let arguments = iter::once("git-factor")
        .chain(["--finish", "HEAD"].iter().copied())
        .map(OsString::from)
        .collect::<Vec<_>>();

    let code = main_entry_with_vec(fixture.context().io, Ok(fixture.context()), &arguments);

    assert_eq!(code, EXIT_USAGE);
    assert_eq!(fixture.stdout(), "");
    assert_eq!(
        fixture.stderr(),
        format!(
            "{}\n",
            "--finish cannot be combined with --continue, --exec, or COMMIT"
        )
    );
    assert_eq!(fixture.calls(), Vec::<String>::new());
    assert_eq!(fixture.facts(), before);
}

#[test]
fn continue_exec() {
    let fixture = PublicCli::pure();
    let before = fixture.facts();
    let arguments = iter::once("git-factor")
        .chain(["--continue", "--exec", "true"].iter().copied())
        .map(OsString::from)
        .collect::<Vec<_>>();

    let code = main_entry_with_vec(fixture.context().io, Ok(fixture.context()), &arguments);

    assert_eq!(code, EXIT_USAGE);
    assert_eq!(fixture.stdout(), "");
    assert_eq!(
        fixture.stderr(),
        format!(
            "{}\n",
            "--continue cannot be combined with --exec or COMMIT"
        )
    );
    assert_eq!(fixture.calls(), Vec::<String>::new());
    assert_eq!(fixture.facts(), before);
}

#[test]
fn continue_head() {
    let fixture = PublicCli::pure();
    let before = fixture.facts();
    let arguments = iter::once("git-factor")
        .chain(["--continue", "HEAD"].iter().copied())
        .map(OsString::from)
        .collect::<Vec<_>>();

    let code = main_entry_with_vec(fixture.context().io, Ok(fixture.context()), &arguments);

    assert_eq!(code, EXIT_USAGE);
    assert_eq!(fixture.stdout(), "");
    assert_eq!(
        fixture.stderr(),
        format!(
            "{}\n",
            "--continue cannot be combined with --exec or COMMIT"
        )
    );
    assert_eq!(fixture.calls(), Vec::<String>::new());
    assert_eq!(fixture.facts(), before);
}

#[test]
fn missing_exec() {
    let fixture = PublicCli::pure();
    let before = fixture.facts();
    let arguments = iter::once("git-factor")
        .chain(["HEAD"])
        .map(OsString::from)
        .collect::<Vec<_>>();

    let code = main_entry_with_vec(fixture.context().io, Ok(fixture.context()), &arguments);

    assert_eq!(code, EXIT_USAGE);
    assert_eq!(fixture.stdout(), "");
    assert_eq!(
        fixture.stderr(),
        format!(
            "{}\n",
            "--exec <COMMAND> is required when starting a factor session"
        )
    );
    assert_eq!(fixture.calls(), Vec::<String>::new());
    assert_eq!(fixture.facts(), before);
}

#[test]
fn message_with_start_gate() {
    let fixture = PublicCli::pure();
    let before = fixture.facts();
    let arguments = iter::once("git-factor")
        .chain(
            ["--exec", "true", "--message", "Selected atom"]
                .iter()
                .copied(),
        )
        .map(OsString::from)
        .collect::<Vec<_>>();

    let code = main_entry_with_vec(fixture.context().io, Ok(fixture.context()), &arguments);

    assert_eq!(code, EXIT_USAGE);
    assert_eq!(fixture.stdout(), "");
    assert_eq!(
        fixture.stderr(),
        format!("{}\n", "--message cannot be combined with --exec or COMMIT")
    );
    assert_eq!(fixture.calls(), Vec::<String>::new());
    assert_eq!(fixture.facts(), before);
}

#[test]
fn abort_message_refuses_before_native_or_saved_state_effects() {
    let fixture = PublicCli::pure();
    let before = fixture.facts();
    let arguments = iter::once("git-factor")
        .chain(
            ["--abort", "--message", "Keep the saved session"]
                .iter()
                .copied(),
        )
        .map(OsString::from)
        .collect::<Vec<_>>();

    let code = main_entry_with_vec(fixture.context().io, Ok(fixture.context()), &arguments);

    assert_eq!(code, EXIT_USAGE);
    assert_eq!(fixture.stdout(), "");
    assert_eq!(
        fixture.stderr(),
        format!("{}\n", "--abort cannot be combined with other options")
    );
    assert_eq!(fixture.calls(), Vec::<String>::new());
    assert_eq!(fixture.facts(), before);
}

#[test]
fn invalid_gate_name_is_refused_before_git_queries() {
    let fixture = PublicCli::pure();
    let before = fixture.facts();
    let arguments = ["git-factor", "--gate", "bad_name", "true"].map(OsString::from);

    let code = main_entry_with_vec(fixture.context().io, Ok(fixture.context()), &arguments);

    assert_eq!(code, EXIT_USAGE);
    assert_eq!(fixture.stdout(), "");
    assert_eq!(
        fixture.stderr(),
        "git command failed: gate names must be unique ASCII trailer names\n"
    );
    assert_eq!(fixture.calls(), Vec::<String>::new());
    assert_eq!(fixture.facts(), before);
}

#[test]
fn duplicate_gate_names_are_refused_before_git_queries() {
    let fixture = PublicCli::pure();
    let before = fixture.facts();
    let arguments = [
        "git-factor",
        "--gate",
        "Test",
        "true",
        "--gate",
        "test",
        "false",
    ]
    .map(OsString::from);

    let code = main_entry_with_vec(fixture.context().io, Ok(fixture.context()), &arguments);

    assert_eq!(code, EXIT_USAGE);
    assert_eq!(fixture.stdout(), "");
    assert_eq!(
        fixture.stderr(),
        "git command failed: gate names must be unique ASCII trailer names\n"
    );
    assert_eq!(fixture.calls(), Vec::<String>::new());
    assert_eq!(fixture.facts(), before);
}

#[test]
fn message_only_refuses_without_an_active_session() {
    let fixture = PublicCli::inactive();
    let before = fixture.facts();
    let arguments = ["git-factor", "--message", "Add one atom"].map(OsString::from);

    let code = main_entry_with_vec(fixture.context().io, Ok(fixture.context()), &arguments);

    assert_eq!(code, EXIT_USAGE);
    assert_eq!(fixture.stdout(), "");
    assert_eq!(fixture.stderr(), "no active factor session\n");
    assert_eq!(fixture.calls(), fixture.expected_inactive_calls());
    assert_eq!(fixture.facts(), before);
}

#[test]
fn message_only_submits_staged_changes() {
    let fixture = MessageSelection::arrange(true);
    let paragraphs = ["Add selected tree"];
    let mut arguments = vec![OsString::from("git-factor")];
    for paragraph in paragraphs {
        arguments.extend([OsString::from("--message"), OsString::from(paragraph)]);
    }

    let code = main_entry_with_vec(fixture.context().io, Ok(fixture.context()), &arguments);

    assert_eq!(code, EXIT_OK, "{}", fixture.stderr());
    assert_eq!(
        fixture.stdout(),
        "{\"operation\":\"continue\",\"result\":\"complete\",\"split_count\":1}\n"
    );
    assert_eq!(fixture.stderr(), "");
    assert_eq!(
        fixture.git(&["log", "-1", "--format=%B"]),
        format!(
            "{}\n\nGate-test: {}\n {}",
            paragraphs.join("\n\n"),
            fixture.command_hash(),
            fixture.final_tree()
        )
    );
    assert_eq!(
        fixture.git(&["rev-parse", "HEAD^{tree}"]),
        fixture.final_tree()
    );
    assert_eq!(fixture.git(&["rev-parse", "HEAD^"]), fixture.base());
    assert_eq!(fixture.user_bytes(), b"user bytes\0\n");
    assert!(!fixture.session_active());
}

#[test]
fn explicit_continue_still_submits_staged_changes() {
    let fixture = MessageSelection::arrange(true);
    let paragraphs = ["Add selected tree"];
    let mut arguments = vec![OsString::from("git-factor")];
    arguments.push(OsString::from("--continue"));
    for paragraph in paragraphs {
        arguments.extend([OsString::from("--message"), OsString::from(paragraph)]);
    }

    let code = main_entry_with_vec(fixture.context().io, Ok(fixture.context()), &arguments);

    assert_eq!(code, EXIT_OK, "{}", fixture.stderr());
    assert_eq!(
        fixture.stdout(),
        "{\"operation\":\"continue\",\"result\":\"complete\",\"split_count\":1}\n"
    );
    assert_eq!(fixture.stderr(), "");
    assert_eq!(
        fixture.git(&["log", "-1", "--format=%B"]),
        format!(
            "{}\n\nGate-test: {}\n {}",
            paragraphs.join("\n\n"),
            fixture.command_hash(),
            fixture.final_tree()
        )
    );
    assert_eq!(
        fixture.git(&["rev-parse", "HEAD^{tree}"]),
        fixture.final_tree()
    );
    assert_eq!(fixture.git(&["rev-parse", "HEAD^"]), fixture.base());
    assert_eq!(fixture.user_bytes(), b"user bytes\0\n");
    assert!(!fixture.session_active());
}

#[test]
fn message_only_preserves_multiple_paragraphs() {
    let fixture = MessageSelection::arrange(true);
    let paragraphs = [
        "Add selected tree",
        "Preserve the explanation",
        "Keep a third paragraph",
    ];
    let mut arguments = vec![OsString::from("git-factor")];
    for paragraph in paragraphs {
        arguments.extend([OsString::from("--message"), OsString::from(paragraph)]);
    }

    let code = main_entry_with_vec(fixture.context().io, Ok(fixture.context()), &arguments);

    assert_eq!(code, EXIT_OK, "{}", fixture.stderr());
    assert_eq!(
        fixture.stdout(),
        "{\"operation\":\"continue\",\"result\":\"complete\",\"split_count\":1}\n"
    );
    assert_eq!(fixture.stderr(), "");
    assert_eq!(
        fixture.git(&["log", "-1", "--format=%B"]),
        format!(
            "{}\n\nGate-test: {}\n {}",
            paragraphs.join("\n\n"),
            fixture.command_hash(),
            fixture.final_tree()
        )
    );
    assert_eq!(
        fixture.git(&["rev-parse", "HEAD^{tree}"]),
        fixture.final_tree()
    );
    assert_eq!(fixture.git(&["rev-parse", "HEAD^"]), fixture.base());
    assert_eq!(fixture.user_bytes(), b"user bytes\0\n");
    assert!(!fixture.session_active());
}

#[test]
fn message_only_refuses_without_staged_changes() {
    let fixture = MessageSelection::arrange(false);
    let before = fixture.frame();
    let arguments = ["git-factor", "--message", "Add selected tree"].map(OsString::from);

    let code = main_entry_with_vec(fixture.context().io, Ok(fixture.context()), &arguments);

    assert_eq!(code, EXIT_USAGE);
    assert_eq!(fixture.stdout(), "");
    assert_eq!(
        fixture.stderr(),
        "no staged changes to commit\nNEXT: stage exactly one atomic change, then rerun:\n  git factor --continue --message \"type: description\"\n"
    );
    assert_eq!(fixture.frame(), before);
    assert_eq!(fixture.user_bytes(), b"user bytes\0\n");
    assert!(fixture.session_active());
}

#[test]
fn version_stream_is_exact() {
    let fixture = PublicCli::pure();
    let before = fixture.facts();
    let arguments = ["git-factor", "--version"].map(OsString::from);

    let code = main_entry_with_vec(fixture.context().io, Ok(fixture.context()), &arguments);

    assert_eq!(code, EXIT_OK);
    assert_eq!(fixture.stdout(), "git-factor 0.1.0\n");
    assert_eq!(fixture.stderr(), "");
    assert_eq!(fixture.calls(), Vec::<String>::new());
    assert_eq!(fixture.facts(), before);
}
#[test]
fn invalid_flag_stream_is_exact() {
    let fixture = PublicCli::pure();
    let before = fixture.facts();
    let arguments = ["git-factor", "--not-real"].map(OsString::from);

    let code = main_entry_with_vec(fixture.context().io, Ok(fixture.context()), &arguments);

    assert_eq!(code, EXIT_USAGE);
    assert_eq!(fixture.stdout(), "");
    assert_eq!(
        fixture.stderr(),
        "error: unexpected argument '--not-real' found\n\n  tip: to pass '--not-real' as a value, use '-- --not-real'\n\nUsage: git-factor [OPTIONS] [COMMIT]...\n\nFor more information, try '--help'.\n"
    );
    assert_eq!(fixture.calls(), Vec::<String>::new());
    assert_eq!(fixture.facts(), before);
}
#[test]
fn help_stream_is_exact() {
    let fixture = PublicCli::pure();
    let before = fixture.facts();
    let arguments = ["git-factor", "--help"].map(OsString::from);

    let code = main_entry_with_vec(fixture.context().io, Ok(fixture.context()), &arguments);

    assert_eq!(code, EXIT_OK);
    assert_eq!(fixture.stdout(), include_str!("main_entry/help.txt"));
    assert_eq!(fixture.stderr(), "");
    assert_eq!(fixture.calls(), Vec::<String>::new());
    assert_eq!(fixture.facts(), before);
}
#[test]
fn no_arguments_prints_the_full_help_stream() {
    let fixture = PublicCli::pure();
    let before = fixture.facts();
    let arguments = ["git-factor"].map(OsString::from);

    let code = main_entry_with_vec(fixture.context().io, Ok(fixture.context()), &arguments);

    assert_eq!(code, EXIT_OK);
    assert_eq!(fixture.stdout(), include_str!("main_entry/help.txt"));
    assert_eq!(fixture.stderr(), "");
    assert_eq!(fixture.calls(), Vec::<String>::new());
    assert_eq!(fixture.facts(), before);
}
#[test]
fn parser_suggestion_is_forwarded_without_route_effects() {
    let fixture = PublicCli::pure();
    let before = fixture.facts();
    let arguments = ["git-factor", "--not-iu"].map(OsString::from);

    let code = main_entry_with_vec(fixture.context().io, Ok(fixture.context()), &arguments);

    assert_eq!(code, EXIT_USAGE);
    assert_eq!(fixture.stdout(), "");
    assert_eq!(
        fixture.stderr(),
        "error: unexpected argument '--not-iu' found\n\n  tip: a similar argument exists: '--continue'\n\nUsage: git-factor --continue [COMMIT]...\n\nFor more information, try '--help'.\n"
    );
    assert_eq!(fixture.calls(), Vec::<String>::new());
    assert_eq!(fixture.facts(), before);
}
#[test]
fn parser_stderr_failure_is_reported_without_mutation() {
    let fixture = PublicCli::pure();
    let before = fixture.facts();
    let output = super::public_cli::OutputRefusal::new(fixture.context().io, false);
    let context = Ctx {
        io: &output,
        ..fixture.context()
    };
    let arguments = ["git-factor", "--not-real"].map(OsString::from);

    let code = main_entry_with_vec(context.io, Ok(context.clone()), &arguments);

    assert_eq!(code, EXIT_SOFTWARE);
    assert_eq!(fixture.stdout(), "");
    assert_eq!(fixture.stderr(), "failed to write output: output denied\n");
    assert!(output.fired());
    assert_eq!(fixture.calls(), fixture.expected_diagnostic_calls());
    assert_eq!(fixture.facts(), before);
}
#[test]
fn parser_stdout_failure_is_reported_without_mutation() {
    let fixture = PublicCli::pure();
    let before = fixture.facts();
    let output = super::public_cli::OutputRefusal::new(fixture.context().io, false);
    let context = Ctx {
        io: &output,
        ..fixture.context()
    };
    let arguments = ["git-factor", "--help"].map(OsString::from);

    let code = main_entry_with_vec(context.io, Ok(context.clone()), &arguments);

    assert_eq!(code, EXIT_SOFTWARE);
    assert_eq!(fixture.stdout(), "");
    assert_eq!(fixture.stderr(), "failed to write output: output denied\n");
    assert!(output.fired());
    assert_eq!(fixture.calls(), fixture.expected_diagnostic_calls());
    assert_eq!(fixture.facts(), before);
}
#[test]
fn implicit_help_failure_is_reported_without_mutation() {
    let fixture = PublicCli::pure();
    let before = fixture.facts();
    let output = super::public_cli::OutputRefusal::new(fixture.context().io, false);
    let context = Ctx {
        io: &output,
        ..fixture.context()
    };
    let arguments = ["git-factor"].map(OsString::from);

    let code = main_entry_with_vec(context.io, Ok(context.clone()), &arguments);

    assert_eq!(code, EXIT_SOFTWARE);
    assert_eq!(fixture.stdout(), "");
    assert_eq!(fixture.stderr(), "failed to write output: output denied\n");
    assert!(output.fired());
    assert_eq!(fixture.calls(), fixture.expected_diagnostic_calls());
    assert_eq!(fixture.facts(), before);
}

#[test]
fn public_status_reports_final_newline_failure_with_saved_facts_retained() {
    let fixture = MessageSelection::arrange(true);
    let before = fixture.frame();
    let body = format!(
        r#"{{"operation":"status","session":{{"checkpoint":"{}","phase":"selecting","rebase":{{"in_progress":true,"required":true}},"split_count":0,"target":{{"commit":"{}","commit_count":1,"span_starts_at_root":false}}}}}}"#,
        fixture.target(),
        fixture.target()
    );
    let output =
        crate::git_factor::tests::public_cli::OutputRefusal::new(fixture.context().io, true);
    let context = Ctx {
        io: &output,
        ..fixture.context()
    };
    let arguments = ["git-factor", "--status"].map(OsString::from);

    let code = main_entry_with_vec(context.io, Ok(context), &arguments);

    assert_eq!(code, EXIT_SOFTWARE);
    assert_eq!(fixture.stdout(), if true { body } else { String::new() });
    assert_eq!(fixture.stderr(), "failed to write output: output denied\n");
    assert!(output.fired());
    assert_eq!(fixture.frame(), before);
    assert!(!fixture.error_log_exists());
}

#[test]
fn public_status_reports_body_failure_with_saved_facts_retained() {
    let fixture = MessageSelection::arrange(true);
    let before = fixture.frame();
    let body = format!(
        r#"{{"operation":"status","session":{{"checkpoint":"{}","phase":"selecting","rebase":{{"in_progress":true,"required":true}},"split_count":0,"target":{{"commit":"{}","commit_count":1,"span_starts_at_root":false}}}}}}"#,
        fixture.target(),
        fixture.target()
    );
    let output =
        crate::git_factor::tests::public_cli::OutputRefusal::new(fixture.context().io, false);
    let context = Ctx {
        io: &output,
        ..fixture.context()
    };
    let arguments = ["git-factor", "--status"].map(OsString::from);

    let code = main_entry_with_vec(context.io, Ok(context), &arguments);

    assert_eq!(code, EXIT_SOFTWARE);
    assert_eq!(fixture.stdout(), if false { body } else { String::new() });
    assert_eq!(fixture.stderr(), "failed to write output: output denied\n");
    assert!(output.fired());
    assert_eq!(fixture.frame(), before);
    assert!(!fixture.error_log_exists());
}

#[test]
fn inactive_status_reports_final_newline_failure_without_logging() {
    let fixture = PublicCli::inactive();
    let before = fixture.facts();
    let body = "{\"operation\":\"status\",\"session\":null}".to_owned();
    let output =
        crate::git_factor::tests::public_cli::OutputRefusal::new(fixture.context().io, true);
    let context = Ctx {
        io: &output,
        ..fixture.context()
    };
    let arguments = ["git-factor", "--status"].map(OsString::from);

    let code = main_entry_with_vec(context.io, Ok(context), &arguments);

    assert_eq!(code, EXIT_SOFTWARE);
    assert_eq!(fixture.stdout(), if true { body } else { String::new() });
    assert_eq!(fixture.stderr(), "failed to write output: output denied\n");
    assert!(output.fired());
    assert_eq!(fixture.facts(), before);
    assert_eq!(fixture.calls(), fixture.expected_inactive_calls());
}

#[test]
fn inactive_status_reports_body_failure_without_logging() {
    let fixture = PublicCli::inactive();
    let before = fixture.facts();
    let body = "{\"operation\":\"status\",\"session\":null}".to_owned();
    let output =
        crate::git_factor::tests::public_cli::OutputRefusal::new(fixture.context().io, false);
    let context = Ctx {
        io: &output,
        ..fixture.context()
    };
    let arguments = ["git-factor", "--status"].map(OsString::from);

    let code = main_entry_with_vec(context.io, Ok(context), &arguments);

    assert_eq!(code, EXIT_SOFTWARE);
    assert_eq!(fixture.stdout(), if false { body } else { String::new() });
    assert_eq!(fixture.stderr(), "failed to write output: output denied\n");
    assert!(output.fired());
    assert_eq!(fixture.facts(), before);
    assert_eq!(fixture.calls(), fixture.expected_inactive_calls());
}

#[test]
fn public_status_journal_read_failure_preserves_the_complete_session_frame() {
    let fixture = MessageSelection::arrange(true);
    fixture.git(&["branch", "protected", fixture.target()]);
    fixture.git(&["tag", "protected-tag", fixture.base()]);
    let before = fixture.frame();
    let filesystem =
        super::public_cli::JournalReadRefusal::new(fixture.context().fs, &fixture.context().cwd);
    let context = Ctx {
        fs: &filesystem,
        ..fixture.context()
    };
    let arguments = ["git-factor", "--status"].map(OsString::from);

    let code = main_entry_with_vec(context.io, Ok(context), &arguments);

    let after = fixture.frame();
    assert_eq!(
        (code, fixture.stdout(), fixture.stderr(), filesystem.calls()),
        (
            EXIT_SOFTWARE,
            String::new(),
            "failed to read state: journal read denied\n".to_owned(),
            1
        )
    );
    assert_eq!(after, before);
    assert!(!fixture.error_log_exists());
}

#[test]
fn public_continue_journal_read_failure_writes_only_the_admitted_diagnostic_log() {
    let fixture = MessageSelection::arrange(true);
    fixture.git(&["branch", "protected", fixture.target()]);
    fixture.git(&["tag", "protected-tag", fixture.base()]);
    let before = fixture.frame();
    let filesystem =
        super::public_cli::JournalReadRefusal::new(fixture.context().fs, &fixture.context().cwd);
    let context = Ctx {
        fs: &filesystem,
        ..fixture.context()
    };
    let arguments = ["git-factor", "--continue"].map(OsString::from);

    let code = main_entry_with_vec(context.io, Ok(context), &arguments);

    let mut after = fixture.frame();
    let log_path = Path::new(".git/factor/error.log");
    let log = fs::read_to_string(fixture.context().cwd.join(log_path))
        .or_abort("admitted diagnostic log");
    assert_eq!(
        (code, fixture.stdout(), fixture.stderr(), filesystem.calls()),
        (
            EXIT_SOFTWARE,
            String::new(),
            "failed to read state: journal read denied\n".to_owned(),
            3
        )
    );
    assert_eq!(
        after.files_mut().remove(log_path),
        Some(LogPath::File(log.as_bytes().to_vec()))
    );
    assert_eq!(after, before);
    for expected in [
        "argv=git-factor --continue",
        "error=failed to read state: journal read denied",
        "source_0=journal read denied",
        "factor_phase=selecting",
    ] {
        assert_eq!(log.lines().filter(|line| *line == expected).count(), 1);
    }
}
