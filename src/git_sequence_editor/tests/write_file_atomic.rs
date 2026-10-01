use super::super::*;
use core::error::Error as _;

#[test]
fn refuses_empty_path() {
    let error =
        write_file_atomic(Path::new(""), "replacement").err_or_abort("refuse missing parent");
    assert_eq!(
        error.to_string(),
        "failed to determine parent directory for: "
    );
    assert!(matches!(error, AtomicWriteError::MissingParent(reported) if reported.is_empty()));
}

#[test]
fn refuses_path_without_file_name() {
    let error =
        write_file_atomic(Path::new("."), "replacement").err_or_abort("refuse missing filename");
    assert_eq!(error.to_string(), "failed to determine file name for: .");
    assert!(matches!(error, AtomicWriteError::MissingFileName(reported) if reported == "."));
}

#[test]
fn refuses_missing_parent() {
    let (_guard, directory, path) = atomic_write_fixture("missing/todo");
    let error = write_file_atomic(&path, "replacement").err_or_abort("refuse missing parent");
    assert_eq!(
        error.to_string(),
        format!(
            "failed to create temporary todo file for {}: {}",
            path.display(),
            error.source().or_abort("filesystem error cause")
        )
    );
    assert!(
        matches!(error, AtomicWriteError::CreateTemp(reported, _) if reported == path.display().to_string())
    );
    assert!(
        fs::read_dir(directory.path())
            .or_abort("observe parent directory")
            .next()
            .is_none()
    );
}

#[test]
fn cleans_temporary_file_after_rename_failure() {
    let (_guard, directory, path) = atomic_write_fixture("todo");
    fs::create_dir_all(&path).or_abort("directory obstruction");
    let error = write_file_atomic(&path, "replacement").err_or_abort("refuse directory target");
    assert_eq!(
        error.to_string(),
        format!(
            "failed to atomically replace todo file {}: {}",
            path.display(),
            error.source().or_abort("filesystem error cause")
        )
    );
    assert!(
        matches!(error, AtomicWriteError::ReplaceFile(reported, _) if reported == path.display().to_string())
    );
    assert!(path.is_dir());
    let mut entries = fs::read_dir(directory.path())
        .or_abort("observe parent directory")
        .map(|entry| entry.or_abort("observe entry").file_name())
        .collect::<Vec<_>>();
    entries.sort();
    assert_eq!(entries, vec![OsString::from("todo")]);
}

#[test]
fn preserves_reserved_temporary_slot() {
    let (_guard, directory, path) = atomic_write_fixture("todo");
    fs::write(&path, "original").or_abort("original todo");
    let reserved_name = format!("todo.tmp{}.0", process::id());
    let reserved = directory.path().join(&reserved_name);
    fs::write(&reserved, "reserved").or_abort("occupied temporary slot");
    write_file_atomic(&path, "replacement").or_abort("atomic replacement");
    assert_eq!(
        fs::read_to_string(&path).or_abort("observe replacement"),
        "replacement"
    );
    assert_eq!(
        fs::read_to_string(&reserved).or_abort("observe occupied slot"),
        "reserved"
    );
    let mut entries = fs::read_dir(directory.path())
        .or_abort("observe parent directory")
        .map(|entry| entry.or_abort("observe entry").file_name())
        .collect::<Vec<_>>();
    entries.sort();
    assert_eq!(
        entries,
        vec![OsString::from("todo"), OsString::from(reserved_name)]
    );
}

#[test]
fn refuses_exhausted_temporary_slots() {
    let (_guard, directory, path) = atomic_write_fixture("todo");
    let pid = process::id();
    let mut expected = Vec::new();
    for attempt in 0..TEMP_FILE_ATTEMPTS_MAX {
        let name = format!("todo.tmp{pid}.{attempt}");
        fs::write(directory.path().join(&name), "reserved").or_abort("occupied temporary slot");
        expected.push(OsString::from(name));
    }
    expected.sort();
    let error =
        write_file_atomic(&path, "replacement").err_or_abort("refuse exhausted temporary slots");
    assert_eq!(
        error.to_string(),
        format!(
            "failed to create a unique temporary file for {}",
            path.display()
        )
    );
    assert!(
        matches!(error, AtomicWriteError::TempNameExhausted(reported) if reported == path.display().to_string())
    );
    assert!(!path.exists());
    let mut entries = fs::read_dir(directory.path())
        .or_abort("observe parent directory")
        .map(|entry| entry.or_abort("observe entry").file_name())
        .collect::<Vec<_>>();
    entries.sort();
    assert_eq!(entries, expected);
}

#[test]
fn cleans_temporary_file_after_write_failure() {
    let (_guard, directory, path) = atomic_write_fixture("todo");
    fs::write(&path, "original").or_abort("original todo");
    set_write_fail_point(WriteFailPoint::Write);
    let result = write_file_atomic(&path, "replacement");
    set_write_fail_point(WriteFailPoint::None);
    let error = result.err_or_abort("injected atomic write failure");
    assert_eq!(
        error.to_string(),
        format!(
            "failed to write temporary todo file for {}: injected write failure",
            path.display()
        )
    );
    assert!(
        matches!(error, AtomicWriteError::WriteTemp(reported, _) if reported == path.display().to_string())
    );
    assert_eq!(
        fs::read_to_string(&path).or_abort("observe todo"),
        "original"
    );
    let mut entries = fs::read_dir(directory.path())
        .or_abort("observe parent directory")
        .map(|entry| entry.or_abort("observe entry").file_name())
        .collect::<Vec<_>>();
    entries.sort();
    assert_eq!(entries, vec![OsString::from("todo")]);
}

#[test]
fn cleans_temporary_file_after_sync_failure() {
    let (_guard, directory, path) = atomic_write_fixture("todo");
    fs::write(&path, "original").or_abort("original todo");
    set_write_fail_point(WriteFailPoint::Sync);
    let result = write_file_atomic(&path, "replacement");
    set_write_fail_point(WriteFailPoint::None);
    let error = result.err_or_abort("injected atomic write failure");
    assert_eq!(
        error.to_string(),
        format!(
            "failed to sync temporary todo file for {}: injected sync failure",
            path.display()
        )
    );
    assert!(
        matches!(error, AtomicWriteError::SyncTemp(reported, _) if reported == path.display().to_string())
    );
    assert_eq!(
        fs::read_to_string(&path).or_abort("observe todo"),
        "original"
    );
    let mut entries = fs::read_dir(directory.path())
        .or_abort("observe parent directory")
        .map(|entry| entry.or_abort("observe entry").file_name())
        .collect::<Vec<_>>();
    entries.sort();
    assert_eq!(entries, vec![OsString::from("todo")]);
}

#[cfg(unix)]
#[test]
fn reports_parent_open_failure_after_replacement() {
    let (_guard, directory, path) = atomic_write_fixture("todo");
    fs::write(&path, "original").or_abort("original todo");
    set_write_fail_point(WriteFailPoint::OpenParentDir);
    let result = write_file_atomic(&path, "replacement");
    set_write_fail_point(WriteFailPoint::None);
    let error = result.err_or_abort("injected atomic write failure");
    assert_eq!(
        error.to_string(),
        format!(
            "failed to open parent directory for {}: injected parent-dir open failure",
            path.display()
        )
    );
    assert!(
        matches!(error, AtomicWriteError::OpenParentDir(reported, _) if reported == path.display().to_string())
    );
    assert_eq!(
        fs::read_to_string(&path).or_abort("observe todo"),
        "replacement"
    );
    let mut entries = fs::read_dir(directory.path())
        .or_abort("observe parent directory")
        .map(|entry| entry.or_abort("observe entry").file_name())
        .collect::<Vec<_>>();
    entries.sort();
    assert_eq!(entries, vec![OsString::from("todo")]);
}

#[cfg(unix)]
#[test]
fn reports_parent_sync_failure_after_replacement() {
    let (_guard, directory, path) = atomic_write_fixture("todo");
    fs::write(&path, "original").or_abort("original todo");
    set_write_fail_point(WriteFailPoint::SyncParentDir);
    let result = write_file_atomic(&path, "replacement");
    set_write_fail_point(WriteFailPoint::None);
    let error = result.err_or_abort("injected atomic write failure");
    assert_eq!(
        error.to_string(),
        format!(
            "failed to sync parent directory for {}: injected parent-dir sync failure",
            path.display()
        )
    );
    assert!(
        matches!(error, AtomicWriteError::SyncParentDir(reported, _) if reported == path.display().to_string())
    );
    assert_eq!(
        fs::read_to_string(&path).or_abort("observe todo"),
        "replacement"
    );
    let mut entries = fs::read_dir(directory.path())
        .or_abort("observe parent directory")
        .map(|entry| entry.or_abort("observe entry").file_name())
        .collect::<Vec<_>>();
    entries.sort();
    assert_eq!(entries, vec![OsString::from("todo")]);
}
