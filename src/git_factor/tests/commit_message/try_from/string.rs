use crate::git_factor::CommitMessage;
use crate::test_support::{OrAbort as _, ResultOrAbort as _};

#[test]
fn commit_message_try_from_rejects_invalid_values() {
    let too_long = "a".repeat(CommitMessage::MAX_LEN + 1);
    let too_long_err = CommitMessage::try_from(too_long).err_or_abort("too-long message");
    assert_eq!(
        too_long_err.to_string(),
        format!(
            "git command failed: commit message exceeds {} bytes ({} bytes)",
            CommitMessage::MAX_LEN,
            CommitMessage::MAX_LEN + 1
        )
    );

    let newline_message =
        CommitMessage::try_from("line 1\nline 2".to_owned()).or_abort("newline should be accepted");
    assert_eq!(newline_message.as_str(), "line 1\nline 2");

    let tab_message =
        CommitMessage::try_from("column\tvalue".to_owned()).or_abort("tab should be accepted");
    assert_eq!(tab_message.as_str(), "column\tvalue");

    let control_err =
        CommitMessage::try_from("bad\u{7f}message".to_owned()).err_or_abort("control char");
    assert_eq!(
        control_err.to_string(),
        "git command failed: commit message contains control characters"
    );

    let empty_err = CommitMessage::try_from(String::new()).err_or_abort("empty message");
    assert_eq!(
        empty_err.to_string(),
        "git command failed: empty commit message"
    );
}
