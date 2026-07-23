//! Shared support for the process-spawning integration suites (`mountebank_compatibility`,
//! `rift_extensions`, `issue_360_script_cli`, `corpus_replay`).
//!
//! This lives at `tests/support/mod.rs`, not `tests/support.rs`: Cargo treats every `.rs` file
//! directly under `tests/` as its own test binary target, so a bare `tests/support.rs` would be
//! compiled and run as an (empty) test suite in its own right. The `support/mod.rs` layout is a
//! module, not a target, and each suite pulls it in with `#[path = "support/mod.rs"] mod support;`
//! (or an equivalent `mod support;` alongside a `tests/support/mod.rs` file — either resolves the
//! same way).

use std::path::PathBuf;

/// Path to the rift **server** binary under test (the `rift-http-proxy` process these suites spawn
/// and drive over the admin API / CLI).
///
/// Resolution order:
/// 1. `RIFT_SERVER_BIN`, if set — must name an existing, executable file. This lets any
///    Mountebank-compatible build (a differently-packaged rift, e.g. a musl static image, or a
///    downstream binary that composes rift's `ServerBuilder`) be driven through these suites
///    instead of this repo's own debug build. A set-but-bad override panics rather than falling
///    back silently: quietly testing the wrong binary is worse than failing loudly.
/// 2. Otherwise `CARGO_BIN_EXE_rift-http-proxy` — today's behaviour, unchanged.
///
/// Only the **server** is overridable this way. `corpus_replay.rs` also spawns `rift-verify`
/// (the reference `_verify` replayer); that stays pinned to `CARGO_BIN_EXE_rift-verify` — it is
/// part of the test harness, not the binary under test, and `RIFT_SERVER_BIN` deliberately does
/// not touch it. (Hence `RIFT_SERVER_BIN`, not a generic `RIFT_BIN`: with two binaries in play,
/// a generic name would be ambiguous about which one it overrides.)
pub fn server_bin() -> PathBuf {
    if let Ok(path) = std::env::var("RIFT_SERVER_BIN") {
        let bin = PathBuf::from(&path);
        let executable = bin
            .metadata()
            .map(|m| m.is_file() && is_executable(&m))
            .unwrap_or(false);
        assert!(
            executable,
            "RIFT_SERVER_BIN={path} does not name an existing, executable file"
        );
        return bin;
    }
    PathBuf::from(env!("CARGO_BIN_EXE_rift-http-proxy"))
}

#[cfg(unix)]
fn is_executable(meta: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    meta.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn is_executable(_meta: &std::fs::Metadata) -> bool {
    // No portable executable-bit check off Unix; existence-as-a-file is the best we can assert.
    true
}
