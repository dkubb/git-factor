//! Binary entrypoint for `git-factor`.

use git_factor::git_factor::main_entry as git_factor_main_entry;
use std::process::exit;

fn main() {
    exit(git_factor_main_entry());
}
