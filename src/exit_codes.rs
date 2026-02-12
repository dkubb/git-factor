//! Process exit codes used by CLI binaries.

/// Success exit code.
pub const EXIT_OK: i32 = 0;
/// Invalid arguments exit code.
pub const EXIT_USAGE: i32 = 64;
/// Bad input or invalid git state exit code.
pub const EXIT_DATAERR: i32 = 65;
/// Internal failure exit code.
pub const EXIT_SOFTWARE: i32 = 70;
/// Temporary failure exit code.
pub const EXIT_TEMPFAIL: i32 = 75;
