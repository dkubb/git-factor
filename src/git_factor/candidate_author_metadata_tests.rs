//! Real author query inputs and independently expected native identity fields.
use super::Ctx;
use super::path_queries::{PathQuery, PathQueryFrame};
use crate::test_support::OrAbort as _;
use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::NamedTempFile;

#[derive(Clone, Copy, Debug)]
pub(in crate::git_factor::candidate) enum AuthorAlias {
    NegativeZero,
    NonBreakingNameSpace,
    TrailingNameSpace,
    TrailingNameTab,
}

pub(in crate::git_factor::candidate) struct AuthorQuery {
    expected: String,
    paths: PathQuery,
    tip: String,
}
impl AuthorQuery {
    pub(in crate::git_factor::candidate) fn arrange(
        name: &str,
        epoch: u32,
        zone: &str,
        encoded_source: bool,
    ) -> Self {
        let paths = PathQuery::arrange("author", b"source bytes", false);
        let root = paths.context().cwd;
        let date = format!("{epoch} {zone}");
        let encoding = if encoded_source {
            "ISO-8859-1"
        } else {
            "UTF-8"
        };
        let output = Command::new("git")
            .args([
                "-c",
                &format!("i18n.commitEncoding={encoding}"),
                "commit",
                "--amend",
                "--quiet",
                "--no-edit",
                "--reset-author",
            ])
            .env("GIT_AUTHOR_NAME", name)
            .env("GIT_AUTHOR_EMAIL", "native-author@example.invalid")
            .env("GIT_AUTHOR_DATE", format!("@{date}"))
            .current_dir(&root)
            .output()
            .or_abort("native original author construction");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let config = Command::new("git")
            .args(["config", "i18n.logOutputEncoding", "ISO-8859-1"])
            .current_dir(&root)
            .output()
            .or_abort("native configured log encoding");
        assert!(config.status.success());
        let identity = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(&root)
            .output()
            .or_abort("original author commit");
        assert!(identity.status.success());
        let tip = String::from_utf8(identity.stdout)
            .or_abort("native identity UTF-8")
            .trim_end_matches('\n')
            .to_owned();
        Self {
            expected: format!("{name}\0native-author@example.invalid\0{date}"),
            paths,
            tip,
        }
    }
    pub(in crate::git_factor::candidate) fn arrange_alias(
        stem: &str,
        epoch: u32,
        alias: AuthorAlias,
    ) -> Self {
        let name = format!("{stem} Jos\u{e9}");
        let mut fixture = Self::arrange(&name, epoch, "+0000", false);
        let root = fixture.context().cwd;
        let (suffix, zone, native_suffix) = match alias {
            AuthorAlias::NegativeZero => ("", "-0000", ""),
            AuthorAlias::NonBreakingNameSpace => ("\u{a0}", "+0000", "\u{a0}"),
            AuthorAlias::TrailingNameSpace => ("  ", "+0000", ""),
            AuthorAlias::TrailingNameTab => (" \t", "+0000", ""),
        };
        let raw = native_output(&root, &["cat-file", "commit", &fixture.tip]);
        let original = String::from_utf8(raw).or_abort("original alias object");
        let old_header = original
            .lines()
            .find(|line| line.starts_with("author "))
            .or_abort("original author header");
        let email = "native-author@example.invalid";
        let date = format!("{epoch} {zone}");
        let alias_name = format!("{name}{suffix}");
        let header = format!("author {alias_name} <{email}> {date}");
        let object = NamedTempFile::new().or_abort("external author object input");
        fs::write(object.path(), original.replacen(old_header, &header, 1))
            .or_abort("valid raw native author alias");
        let identity = native_output(
            &root,
            &[
                "hash-object",
                "-t",
                "commit",
                "-w",
                object.path().to_str().or_abort("object path"),
            ],
        );
        fixture.tip = String::from_utf8(identity)
            .or_abort("native alias identity")
            .trim_end_matches('\n')
            .to_owned();
        native_output(&root, &["fsck", "--strict", &fixture.tip]);
        let tree = original
            .lines()
            .next()
            .and_then(|line| line.strip_prefix("tree "))
            .or_abort("original tree");
        let control = Command::new("git")
            .args(["commit-tree", tree, "-m", "Add native alias control"])
            .env("GIT_AUTHOR_NAME", &alias_name)
            .env("GIT_AUTHOR_EMAIL", email)
            .env("GIT_AUTHOR_DATE", format!("@{date}"))
            .current_dir(&root)
            .output()
            .or_abort("native normalization control");
        assert!(control.status.success());
        let control_identity = String::from_utf8(control.stdout).or_abort("control identity");
        let control_object =
            native_output(&root, &["cat-file", "commit", control_identity.trim_end()]);
        let native_name = format!("{name}{native_suffix}");
        let expected_header = format!("author {native_name} <{email}> {epoch} +0000");
        assert!(
            control_object
                .split(|byte| *byte == b'\n')
                .any(|line| line == expected_header.as_bytes())
        );
        fixture.expected = format!("{native_name}\0{email}\0{epoch} +0000");
        fixture
    }

    pub(in crate::git_factor::candidate) fn context(&self) -> Ctx<'_> {
        self.paths.context()
    }
    pub(in crate::git_factor::candidate) fn expected(&self) -> &str {
        &self.expected
    }
    pub(in crate::git_factor::candidate) fn frame(&self) -> PathQueryFrame {
        self.paths.frame()
    }
    pub(in crate::git_factor::candidate) fn tip(&self) -> &str {
        &self.tip
    }
}

fn native_output(root: &Path, arguments: &[&str]) -> Vec<u8> {
    let output = Command::new("git")
        .args(arguments)
        .current_dir(root)
        .output()
        .or_abort("native alias arrangement");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}
