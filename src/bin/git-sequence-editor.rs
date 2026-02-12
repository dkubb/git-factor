//! Binary entrypoint for `git-sequence-editor`.

use git_factor::git_sequence_editor::main_entry as git_sequence_editor_main_entry;
use std::process::exit;

fn main() {
    exit(git_sequence_editor_main_entry());
}
