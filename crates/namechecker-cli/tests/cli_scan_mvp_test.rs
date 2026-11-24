//! CLI contract tests for `namechecker scan` MVP text output and exit codes (2 > 1 > 0) (T013).

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

use namechecker_core::unicode_data::normalize_nfd;

#[test]
fn scan_exits_0_for_clean_directory() {
    let dir = TempDir::new().expect("tempdir");
    std::fs::write(dir.path().join("정상파일_01.txt"), b"ok").expect("write clean");

    let mut cmd = Command::cargo_bin("namechecker").expect("binary exists");
    cmd.args(["scan", dir.path().to_str().expect("utf8 path")]);
    cmd.assert()
        .code(0)
        .stdout(predicate::str::contains("점검 요약"));
}

#[test]
fn scan_exits_1_for_findings_with_korean_two_track_report() {
    let dir = TempDir::new().expect("tempdir");
    let nfd = format!("{}.txt", normalize_nfd("테스트"));
    std::fs::write(dir.path().join(nfd), b"nfd").expect("write nfd");
    std::fs::write(dir.path().join("CON.txt"), b"hazard").expect("write hazard");

    let mut cmd = Command::cargo_bin("namechecker").expect("binary exists");
    cmd.args(["scan", dir.path().to_str().expect("utf8 path")]);
    cmd.assert()
        .code(1)
        .stdout(predicate::str::contains("자동 수정 제안"))
        .stdout(predicate::str::contains("수동 확인 필요"));
}

#[test]
fn scan_exits_2_for_nonexistent_path() {
    let mut cmd = Command::cargo_bin("namechecker").expect("binary exists");
    cmd.args(["scan", "/tmp/namechecker-definitely-missing-path-xyz-12345"]);
    cmd.assert().code(2);
}
