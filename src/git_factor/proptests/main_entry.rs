use crate::exit_codes::{EXIT_SOFTWARE, EXIT_USAGE};
use crate::git_factor::engine::tests::MessageSelection;
use crate::git_factor::tests::public_cli::PublicCli;
use crate::git_factor::*;
use core::iter;
use proptest::prelude::*;
use std::ffi::OsString;

proptest! {
    #[test]
    fn baseline_message_with_start_gate_is_refused(
        gate in "[a-zA-Z][a-zA-Z0-9 ]{0,40}",
        message in "[a-zA-Z][a-zA-Z0-9 ]{0,40}",
    ) {
        let fixture = PublicCli::pure();
        let before = fixture.facts();
        let arguments = ["git-factor", "--exec", gate.as_str(), "--message", message.as_str()].map(OsString::from);

        let code = main_entry_with_vec(fixture.context().io, Ok(fixture.context()), &arguments);

        prop_assert_eq!(code, EXIT_USAGE);
        prop_assert_eq!(fixture.stdout(), "");
        prop_assert_eq!(fixture.stderr(), "--message cannot be combined with --exec or COMMIT\n");
        prop_assert_eq!(fixture.calls(), Vec::<String>::new());
        prop_assert_eq!(fixture.facts(), before);
    }
    #[test]
    fn commit_without_gate_is_refused_before_any_route(commit in "[a-zA-Z][a-zA-Z0-9_-]{0,40}") {
        let fixture = PublicCli::pure();
        let before = fixture.facts();
        let arguments = ["git-factor", commit.as_str()].map(OsString::from);

        let code = main_entry_with_vec(fixture.context().io, Ok(fixture.context()), &arguments);

        prop_assert_eq!(code, EXIT_USAGE);
        prop_assert_eq!(fixture.stdout(), "");
        prop_assert_eq!(fixture.stderr(), "--exec <COMMAND> is required when starting a factor session\n");
        prop_assert_eq!(fixture.calls(), Vec::<String>::new());
        prop_assert_eq!(fixture.facts(), before);
    }
    #[test]
    fn message_only_without_an_active_session_preserves_generated_refusals(message in "[a-zA-Z][a-zA-Z0-9 ]{0,40}") {
        let fixture = PublicCli::inactive();
        let before = fixture.facts();
        let arguments = ["git-factor", "--message", message.as_str()].map(OsString::from);

        let code = main_entry_with_vec(fixture.context().io, Ok(fixture.context()), &arguments);

        prop_assert_eq!(code, EXIT_USAGE);
        prop_assert_eq!(fixture.stdout(), "");
        prop_assert_eq!(fixture.stderr(), "no active factor session\n");
        prop_assert_eq!(fixture.calls(), fixture.expected_inactive_calls());
        prop_assert_eq!(fixture.facts(), before);
    }
}

proptest! {
    #[test]
    fn message_submission_preserves_explicit_form_and_paragraphs(
        explicit in any::<bool>(),
        paragraphs in prop::collection::vec("[a-z]{1,20}", 1..=3),
    ) {
        let fixture = MessageSelection::arrange(true);
        let mut arguments = vec![OsString::from("git-factor")];
        if explicit {
            arguments.push(OsString::from("--continue"));
        }
        for paragraph in &paragraphs {
            arguments.extend([OsString::from("--message"), OsString::from(paragraph.as_str())]);
        }

        let code = main_entry_with_vec(fixture.context().io, Ok(fixture.context()), &arguments);

        prop_assert_eq!(code, EXIT_OK, "{}", fixture.stderr());
        prop_assert_eq!(fixture.stdout(), "{\"operation\":\"continue\",\"result\":\"complete\",\"split_count\":1}\n");
        prop_assert_eq!(fixture.stderr(), "");
        prop_assert_eq!(fixture.git(&["log", "-1", "--format=%B"]), format!("{}\n\nGate-test: {}\n {}", paragraphs.join("\n\n"), fixture.command_hash(), fixture.final_tree()));
        prop_assert_eq!(fixture.git(&["rev-parse", "HEAD^{tree}"]), fixture.final_tree());
        prop_assert_eq!(fixture.git(&["rev-parse", "HEAD^"]), fixture.base());
        prop_assert_eq!(fixture.user_bytes(), b"user bytes\0\n");
        prop_assert!(!fixture.session_active());
    }
    #[test]
    fn message_submission_requires_staging(
        paragraphs in prop::collection::vec("[a-z]{1,20}", 1..=3),
    ) {
        let fixture = MessageSelection::arrange(false);
        let before = fixture.frame();
        let mut arguments = vec![OsString::from("git-factor")];
        for paragraph in &paragraphs {
            arguments.extend([OsString::from("--message"), OsString::from(paragraph.as_str())]);
        }

        let code = main_entry_with_vec(fixture.context().io, Ok(fixture.context()), &arguments);

        prop_assert_eq!(code, EXIT_USAGE);
        prop_assert_eq!(fixture.stdout(), "");
        prop_assert_eq!(fixture.stderr(), "no staged changes to commit\nNEXT: stage exactly one atomic change, then rerun:\n  git factor --continue --message \"type: description\"\n");
        prop_assert_eq!(fixture.frame(), before);
        prop_assert_eq!(fixture.user_bytes(), b"user bytes\0\n");
        prop_assert!(fixture.session_active());
    }
}

proptest! {
    #[test]
    fn help_spelling_preserves_the_complete_stream(explicit in any::<bool>()) {
        let arguments: &[&str] = if explicit { &["--help"] } else { &[] };
        let fixture = PublicCli::pure();
        let before = fixture.facts();
        let argv = iter::once("git-factor").chain((arguments).iter().copied()).map(OsString::from).collect::<Vec<_>>();

        let code = main_entry_with_vec(fixture.context().io, Ok(fixture.context()), &argv);

        prop_assert_eq!(code, EXIT_OK);
        prop_assert_eq!(fixture.stdout(), include_str!("../tests/main_entry/help.txt"));
        prop_assert_eq!(fixture.stderr(), "");
        prop_assert_eq!(fixture.calls(), Vec::<String>::new());
        prop_assert_eq!(fixture.facts(), before);

    }

    #[test]
    fn parser_write_failures_preserve_user_bytes(
        arguments in prop::sample::select(vec![vec!["--help"], vec!["--not-real"], Vec::<&str>::new()]),
    ) {
        let fixture = PublicCli::pure();
        let before = fixture.facts();
        let output = crate::git_factor::tests::public_cli::OutputRefusal::new(fixture.context().io, false);
        let context = Ctx { io: &output, ..fixture.context() };
        let argv = iter::once("git-factor").chain(arguments.iter().copied()).map(OsString::from).collect::<Vec<_>>();

        let code = main_entry_with_vec(context.io, Ok(context.clone()), &argv);

        prop_assert_eq!(code, EXIT_SOFTWARE);
        prop_assert_eq!(fixture.stdout(), "");
        prop_assert_eq!(fixture.stderr(), "failed to write output: output denied\n");
        prop_assert!(output.fired());
        prop_assert_eq!(fixture.calls(), fixture.expected_diagnostic_calls());
        prop_assert_eq!(fixture.facts(), before);
    }

    #[test]
    fn incompatible_operation_inputs_preserve_the_public_refusal(
        case in prop::sample::select(vec![
            ("abort","--status","--abort cannot be combined with other options"),
            ("abort","--continue","--abort cannot be combined with other options"),
            ("abort","--retry","--abort cannot be combined with other options"),
            ("abort","--finish","--abort cannot be combined with other options"),
            ("abort","--exec","--abort cannot be combined with other options"),
            ("abort","HEAD","--abort cannot be combined with other options"),
            ("status","--continue","--status cannot be combined with other options"),
            ("status","--retry","--status cannot be combined with other options"),
            ("status","--finish","--status cannot be combined with other options"),
            ("status","--exec","--status cannot be combined with other options"),
            ("status","HEAD","--status cannot be combined with other options"),
            ("status","--message","--status cannot be combined with other options"),
            ("retry","--continue","--retry cannot be combined with other options"),
            ("retry","--finish","--retry cannot be combined with other options"),
            ("retry","--exec","--retry cannot be combined with other options"),
            ("retry","HEAD","--retry cannot be combined with other options"),
            ("retry","--message","--retry cannot be combined with other options"),
            ("finish","--continue","--finish cannot be combined with --continue, --exec, or COMMIT"),
            ("finish","--exec","--finish cannot be combined with --continue, --exec, or COMMIT"),
            ("finish","HEAD","--finish cannot be combined with --continue, --exec, or COMMIT"),
            ("continue","--exec","--continue cannot be combined with --exec or COMMIT"),
            ("continue","HEAD","--continue cannot be combined with --exec or COMMIT")
        ]),
        value in "[a-zA-Z][a-zA-Z0-9 ]{0,40}",
    ) {
        let (operation, other, diagnostic) = case;
        let flag = format!("--{operation}");
        let mut arguments = vec![flag.as_str(), other];
        if matches!(other, "--exec" | "--message") { arguments.push(&value); }
        let fixture = PublicCli::pure();
        let before = fixture.facts();
        let argv = iter::once("git-factor").chain(arguments.iter().copied()).map(OsString::from).collect::<Vec<_>>();

        let code = main_entry_with_vec(fixture.context().io, Ok(fixture.context()), &argv);

        prop_assert_eq!(code, EXIT_USAGE);
        prop_assert_eq!(fixture.stdout(), "");
        prop_assert_eq!(fixture.stderr(), format!("{}\n", diagnostic));
        prop_assert_eq!(fixture.calls(), Vec::<String>::new());
        prop_assert_eq!(fixture.facts(), before);

    }

    #[test]
    fn unknown_flags_preserve_the_exact_parser_error(suffix in "[0-9]{1,12}") {
        let flag = format!("--zzzzzzzzzz-{suffix}");
        let diagnostic = format!("error: unexpected argument '{flag}' found\n\n  tip: to pass '{flag}' as a value, use '-- {flag}'\n\nUsage: git-factor [OPTIONS] [COMMIT]...\n\nFor more information, try '--help'.\n");
        let fixture = PublicCli::pure();
        let before = fixture.facts();
        let argv = iter::once("git-factor").chain([flag.as_str()]).map(OsString::from).collect::<Vec<_>>();

        let code = main_entry_with_vec(fixture.context().io, Ok(fixture.context()), &argv);

        prop_assert_eq!(code, EXIT_USAGE);
        prop_assert_eq!(fixture.stdout(), "");
        prop_assert_eq!(fixture.stderr(), diagnostic.as_str());
        prop_assert_eq!(fixture.calls(), Vec::<String>::new());
        prop_assert_eq!(fixture.facts(), before);

    }

    #[test]
    fn version_spelling_preserves_the_exact_stream(explicit in any::<bool>()) {
        let flag = if explicit { "--version" } else { "-v" };
        let fixture = PublicCli::pure();
        let before = fixture.facts();
        let argv = iter::once("git-factor").chain([flag]).map(OsString::from).collect::<Vec<_>>();

        let code = main_entry_with_vec(fixture.context().io, Ok(fixture.context()), &argv);

        prop_assert_eq!(code, EXIT_OK);
        prop_assert_eq!(fixture.stdout(), "git-factor 0.1.0\n");
        prop_assert_eq!(fixture.stderr(), "");
        prop_assert_eq!(fixture.calls(), Vec::<String>::new());
        prop_assert_eq!(fixture.facts(), before);

    }

    #[test]
    fn parser_suggestions_preserve_the_exact_stream(flag in prop::sample::select(vec!["--not-iu", "--continu"])) {
        let diagnostic = format!("error: unexpected argument '{flag}' found\n\n  tip: a similar argument exists: '--continue'\n\nUsage: git-factor --continue [COMMIT]...\n\nFor more information, try '--help'.\n");
        let fixture = PublicCli::pure();
        let before = fixture.facts();
        let argv = iter::once("git-factor").chain([flag]).map(OsString::from).collect::<Vec<_>>();

        let code = main_entry_with_vec(fixture.context().io, Ok(fixture.context()), &argv);

        prop_assert_eq!(code, EXIT_USAGE);
        prop_assert_eq!(fixture.stdout(), "");
        prop_assert_eq!(fixture.stderr(), diagnostic.as_str());
        prop_assert_eq!(fixture.calls(), Vec::<String>::new());
        prop_assert_eq!(fixture.facts(), before);

    }

    #[test]
    fn message_text_never_overrides_query_or_retry(
        operation in prop::sample::select(vec!["--status", "--retry"]),
        message in "[a-zA-Z][a-zA-Z0-9 ]{0,40}",
    ) {
        let diagnostic = match operation {
            "--status" => "--status cannot be combined with other options",
            _ => "--retry cannot be combined with other options",
        };
        let fixture = PublicCli::pure();
        let before = fixture.facts();
        let argv = iter::once("git-factor").chain([operation, "--message", &message].iter().copied()).map(OsString::from).collect::<Vec<_>>();

        let code = main_entry_with_vec(fixture.context().io, Ok(fixture.context()), &argv);

        prop_assert_eq!(code, EXIT_USAGE);
        prop_assert_eq!(fixture.stdout(), "");
        prop_assert_eq!(fixture.stderr(), format!("{}\n", diagnostic));
        prop_assert_eq!(fixture.calls(), Vec::<String>::new());
        prop_assert_eq!(fixture.facts(), before);

    }

    #[test]
    fn gate_text_never_overrides_explicit_operation(
        operation in prop::sample::select(vec!["--abort", "--status", "--retry", "--finish", "--continue"]),
        gate in "[a-zA-Z][a-zA-Z0-9 ]{0,40}",
    ) {
        let diagnostic = match operation {
            "--abort" => "--abort cannot be combined with other options",
            "--status" => "--status cannot be combined with other options",
            "--retry" => "--retry cannot be combined with other options",
            "--finish" => "--finish cannot be combined with --continue, --exec, or COMMIT",
            _ => "--continue cannot be combined with --exec or COMMIT",
        };
        let fixture = PublicCli::pure();
        let before = fixture.facts();
        let argv = iter::once("git-factor").chain([operation, "--exec", &gate].iter().copied()).map(OsString::from).collect::<Vec<_>>();

        let code = main_entry_with_vec(fixture.context().io, Ok(fixture.context()), &argv);

        prop_assert_eq!(code, EXIT_USAGE);
        prop_assert_eq!(fixture.stdout(), "");
        prop_assert_eq!(fixture.stderr(), format!("{}\n", diagnostic));
        prop_assert_eq!(fixture.calls(), Vec::<String>::new());
        prop_assert_eq!(fixture.facts(), before);

    }

    #[test]
    fn abort_messages_preserve_generated_dispatch_refusals(
        message in "[a-zA-Z][a-zA-Z0-9 ]{0,40}",
    ) {
        let fixture = PublicCli::pure();
        let before = fixture.facts();
        let argv = iter::once("git-factor").chain(["--abort", "--message", &message].iter().copied()).map(OsString::from).collect::<Vec<_>>();

        let code = main_entry_with_vec(fixture.context().io, Ok(fixture.context()), &argv);

        prop_assert_eq!(code, EXIT_USAGE);
        prop_assert_eq!(fixture.stdout(), "");
        prop_assert_eq!(fixture.stderr(), format!("{}\n", "--abort cannot be combined with other options"));
        prop_assert_eq!(fixture.calls(), Vec::<String>::new());
        prop_assert_eq!(fixture.facts(), before);

    }

    #[test]
    fn retry_and_continue_require_a_session(operation in prop::sample::select(vec!["--retry", "--continue"])) {
        let fixture = PublicCli::inactive();
        let before = fixture.facts();
        let argv = iter::once("git-factor").chain([operation]).map(OsString::from).collect::<Vec<_>>();

        let code = main_entry_with_vec(fixture.context().io, Ok(fixture.context()), &argv);

        prop_assert_eq!(code, EXIT_USAGE);
        prop_assert_eq!(fixture.stdout(), "");
        prop_assert_eq!(fixture.stderr(), "no active factor session\n");
        prop_assert_eq!(fixture.calls(), fixture.expected_inactive_calls());
        prop_assert_eq!(fixture.facts(), before);
    }

    #[test]
    fn finish_message_presence_preserves_the_session_requirement(
        message in prop::option::of("[a-zA-Z][a-zA-Z0-9 ]{0,40}"),
    ) {
        let arguments = message.as_deref().map_or_else(
            || vec!["--finish"],
            |text| vec!["--finish", "--message", text],
        );
        let fixture = PublicCli::inactive();
        let before = fixture.facts();
        let argv = iter::once("git-factor").chain(arguments.iter().copied()).map(OsString::from).collect::<Vec<_>>();

        let code = main_entry_with_vec(fixture.context().io, Ok(fixture.context()), &argv);

        prop_assert_eq!(code, EXIT_USAGE);
        prop_assert_eq!(fixture.stdout(), "");
        prop_assert_eq!(fixture.stderr(), "no active factor session\n");
        prop_assert_eq!(fixture.calls(), fixture.expected_inactive_calls());
        prop_assert_eq!(fixture.facts(), before);
    }
}

proptest! {
    #[test]
    fn active_status_write_failures_preserve_saved_facts(newline in any::<bool>()) {
        let fixture = MessageSelection::arrange(true);
        let before = fixture.frame();
        let body = format!(r#"{{"operation":"status","session":{{"checkpoint":"{}","phase":"selecting","rebase":{{"in_progress":true,"required":true}},"split_count":0,"target":{{"commit":"{}","commit_count":1,"span_starts_at_root":false}}}}}}"#, fixture.target(), fixture.target());
        let output = crate::git_factor::tests::public_cli::OutputRefusal::new(fixture.context().io, newline);
        let context = Ctx { io: &output, ..fixture.context() };
        let arguments = ["git-factor", "--status"].map(OsString::from);

        let code = main_entry_with_vec(context.io, Ok(context), &arguments);

        prop_assert_eq!(code, EXIT_SOFTWARE);
        prop_assert_eq!(fixture.stdout(), if newline { body } else { String::new() });
        prop_assert_eq!(fixture.stderr(), "failed to write output: output denied\n");
        prop_assert!(output.fired());
        prop_assert_eq!(fixture.frame(), before);
        prop_assert!(!fixture.error_log_exists());
    }
}

proptest! {
    #[test]
    fn inactive_status_write_failures_preserve_saved_facts(newline in any::<bool>()) {
        let fixture = PublicCli::inactive();
        let before = fixture.facts();
        let body = "{\"operation\":\"status\",\"session\":null}".to_owned();
        let output = crate::git_factor::tests::public_cli::OutputRefusal::new(fixture.context().io, newline);
        let context = Ctx { io: &output, ..fixture.context() };
        let arguments = ["git-factor", "--status"].map(OsString::from);

        let code = main_entry_with_vec(context.io, Ok(context), &arguments);

        prop_assert_eq!(code, EXIT_SOFTWARE);
        prop_assert_eq!(fixture.stdout(), if newline { body } else { String::new() });
        prop_assert_eq!(fixture.stderr(), "failed to write output: output denied\n");
        prop_assert!(output.fired());
        prop_assert_eq!(fixture.facts(), before);
        prop_assert_eq!(fixture.calls(), fixture.expected_inactive_calls());
    }
}
