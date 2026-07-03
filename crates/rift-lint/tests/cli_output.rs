//! Issue #347: rift-lint honors NO_COLOR / non-TTY stdout and emits pure JSON with `-o json`.

use std::path::PathBuf;
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_rift-lint");

fn write_tmp() -> PathBuf {
    let p = std::env::temp_dir().join(format!(
        "rift_lint_347_{}_{}.json",
        std::process::id(),
        line!()
    ));
    std::fs::write(&p, r#"{"port":8000,"protocol":"http","stubs":[]}"#).expect("write");
    p
}

// AC3: `-o json` prints ONLY JSON on stdout — no decorative banner, no ANSI escapes.
#[test]
fn lint_json_stdout_is_pure_json() {
    let f = write_tmp();
    let out = Command::new(BIN)
        .args([f.to_str().unwrap(), "-o", "json"])
        .output()
        .expect("run rift-lint");
    let stdout = String::from_utf8(out.stdout).expect("utf8");
    assert!(
        !stdout.contains('\x1b'),
        "json-mode stdout must contain no ANSI escapes, got: {stdout:?}"
    );
    assert!(
        !stdout.contains("Rift Imposter Linter"),
        "json-mode stdout must not carry the human banner"
    );
    serde_json::from_str::<serde_json::Value>(stdout.trim()).expect("stdout parses as JSON");
    let _ = std::fs::remove_file(f);
}

// AC1: text mode with a piped (non-TTY) stdout — as in this test — emits no ANSI and no banner.
#[test]
fn lint_text_piped_has_no_ansi_or_banner() {
    let f = write_tmp();
    let out = Command::new(BIN)
        .arg(f.to_str().unwrap())
        .output()
        .expect("run rift-lint");
    let stdout = String::from_utf8(out.stdout).expect("utf8");
    assert!(
        !stdout.contains('\x1b'),
        "piped stdout must contain no ANSI escapes, got: {stdout:?}"
    );
    assert!(
        !stdout.contains("Rift Imposter Linter"),
        "piped stdout must not carry the decorative banner"
    );
    let _ = std::fs::remove_file(f);
}

// AC3 edge: `-o json` on a directory with no imposter files still emits valid JSON on stdout
// (not empty input), so a consumer piping to `jq` never chokes.
#[test]
fn lint_json_empty_dir_still_emits_json() {
    let dir = std::env::temp_dir().join(format!("rift_lint_347_empty_{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("mkdir");
    let out = Command::new(BIN)
        .args([dir.to_str().unwrap(), "-o", "json"])
        .output()
        .expect("run rift-lint");
    let stdout = String::from_utf8(out.stdout).expect("utf8");
    serde_json::from_str::<serde_json::Value>(stdout.trim())
        .expect("no-files json mode still yields valid JSON on stdout");
    let _ = std::fs::remove_dir_all(&dir);
}

// AC1: NO_COLOR is honored regardless of TTY.
#[test]
fn lint_no_color_env_disables_ansi() {
    let f = write_tmp();
    let out = Command::new(BIN)
        .arg(f.to_str().unwrap())
        .env("NO_COLOR", "1")
        .output()
        .expect("run rift-lint");
    let stdout = String::from_utf8(out.stdout).expect("utf8");
    assert!(
        !stdout.contains('\x1b'),
        "NO_COLOR must disable ANSI escapes, got: {stdout:?}"
    );
    let _ = std::fs::remove_file(f);
}
