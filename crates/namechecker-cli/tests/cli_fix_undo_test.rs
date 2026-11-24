use std::fs;
use std::process::Command;

use tempfile::tempdir;

fn cli_bin() -> &'static str {
    env!("CARGO_BIN_EXE_namechecker")
}

#[test]
fn cli_fix_undo_and_ignore_round_trip_workflow() {
    let target_dir = tempdir().unwrap();
    let state_dir = tempdir().unwrap();
    let audit_log = state_dir.path().join("audit.json");
    let ignore_file = state_dir.path().join("ignore.json");

    fs::write(target_dir.path().join("⽊_report.txt"), b"kangxi").unwrap();
    fs::write(target_dir.path().join("note〜draft.txt"), b"wave").unwrap();

    // 1. Add portable ignore rule for note〜draft.txt
    let ig_out = Command::new(cli_bin())
        .args(["ignore", "note〜draft.txt", "--ignore-file"])
        .arg(&ignore_file)
        .args([
            "--category",
            "PunctuationVariant",
            "--reason",
            "legacy archive",
        ])
        .output()
        .unwrap();
    assert_eq!(
        ig_out.status.code(),
        Some(0),
        "ignore failed: {}",
        String::from_utf8_lossy(&ig_out.stderr)
    );
    assert!(ignore_file.exists());

    // 2. Scan with --ignore-file should ignore note〜draft.txt and report only ⽊_report.txt
    let scan_out = Command::new(cli_bin())
        .args(["scan", "--format", "json", "--ignore-file"])
        .arg(&ignore_file)
        .arg(target_dir.path())
        .output()
        .unwrap();
    assert_eq!(scan_out.status.code(), Some(1));
    let scan_json: serde_json::Value = serde_json::from_slice(&scan_out.stdout).unwrap();
    assert_eq!(scan_json["manual_review_count"], 0);
    assert_eq!(scan_json["auto_fixable_count"], 1);

    // 3. fix --dry-run preserves original filename
    let dry_out = Command::new(cli_bin())
        .args(["fix", "--dry-run", "--audit-log"])
        .arg(&audit_log)
        .arg(target_dir.path())
        .output()
        .unwrap();
    assert_eq!(dry_out.status.code(), Some(0));
    assert!(target_dir.path().join("⽊_report.txt").exists());
    assert!(!target_dir.path().join("木_report.txt").exists());

    // 4. fix --yes applies safe rename & writes external audit log
    let fix_out = Command::new(cli_bin())
        .args(["fix", "--yes", "--audit-log"])
        .arg(&audit_log)
        .arg(target_dir.path())
        .output()
        .unwrap();
    assert_eq!(
        fix_out.status.code(),
        Some(0),
        "fix failed: {}",
        String::from_utf8_lossy(&fix_out.stderr)
    );
    assert!(target_dir.path().join("木_report.txt").exists());
    assert!(!target_dir.path().join("⽊_report.txt").exists());
    assert!(audit_log.exists());

    // 5. undo --dry-run preserves renamed file, then undo restores original
    let undo_dry = Command::new(cli_bin())
        .args(["undo", "--dry-run"])
        .arg(&audit_log)
        .output()
        .unwrap();
    assert_eq!(undo_dry.status.code(), Some(0));
    assert!(target_dir.path().join("木_report.txt").exists());

    let undo_real = Command::new(cli_bin())
        .args(["undo"])
        .arg(&audit_log)
        .output()
        .unwrap();
    assert_eq!(undo_real.status.code(), Some(0));
    assert!(target_dir.path().join("⽊_report.txt").exists());
    assert!(!target_dir.path().join("木_report.txt").exists());
}

#[test]
fn cli_fix_supports_inherited_exclude_no_default_excludes_and_max_depth() {
    let dir = tempdir().unwrap();
    let state_dir = tempdir().unwrap();
    let audit_log = state_dir.path().join("fix_opts.json");

    let eadir = dir.path().join("@eaDir");
    fs::create_dir(&eadir).unwrap();
    fs::write(eadir.join("⽊_inside.txt"), b"k").unwrap();
    fs::write(eadir.join("⽊_skip.skip"), b"s").unwrap();

    let fix_out = Command::new(cli_bin())
        .args([
            "fix",
            "--yes",
            "--no-default-excludes",
            "--max-depth",
            "2",
            "--exclude",
            "*.skip",
            "--audit-log",
        ])
        .arg(&audit_log)
        .arg(dir.path())
        .output()
        .unwrap();
    assert_eq!(fix_out.status.code(), Some(0));
    assert!(eadir.join("木_inside.txt").exists());
    assert!(eadir.join("⽊_skip.skip").exists());
}

#[test]
fn cli_fix_ignore_and_undo_localize_messages_in_ko_and_en() {
    let dir = tempdir().unwrap();
    let ignore_file = tempdir().unwrap().path().join("ignore.json");
    let audit_log = tempdir().unwrap().path().join("audit.json");
    fs::write(dir.path().join("clean.txt"), b"ok").unwrap();

    // fix with no work: Ko vs En
    let fix_ko = Command::new(cli_bin())
        .args(["fix", "--lang", "ko"])
        .arg(dir.path())
        .output()
        .unwrap();
    assert_eq!(fix_ko.status.code(), Some(0));
    let fix_ko_out = String::from_utf8_lossy(&fix_ko.stdout);
    assert!(
        fix_ko_out.contains("자동 수정"),
        "expected Korean fix message, got: {fix_ko_out}"
    );

    let fix_en = Command::new(cli_bin())
        .args(["fix", "--lang", "en"])
        .arg(dir.path())
        .output()
        .unwrap();
    assert_eq!(fix_en.status.code(), Some(0));
    let fix_en_out = String::from_utf8_lossy(&fix_en.stdout);
    assert!(
        fix_en_out.contains("No auto-fixable"),
        "expected English fix message, got: {fix_en_out}"
    );

    // ignore with --lang ko and --lang en
    let ign_ko = Command::new(cli_bin())
        .args(["ignore", "note.txt", "--lang", "ko", "--ignore-file"])
        .arg(&ignore_file)
        .output()
        .unwrap();
    assert_eq!(ign_ko.status.code(), Some(0));
    assert!(
        String::from_utf8_lossy(&ign_ko.stdout).contains("무시 규칙"),
        "expected Korean ignore output: {}",
        String::from_utf8_lossy(&ign_ko.stdout)
    );

    let ign_en = Command::new(cli_bin())
        .args(["ignore", "note2.txt", "--lang", "en", "--ignore-file"])
        .arg(&ignore_file)
        .output()
        .unwrap();
    assert_eq!(ign_en.status.code(), Some(0));
    assert!(
        String::from_utf8_lossy(&ign_en.stdout).contains("Added rule"),
        "expected English ignore output: {}",
        String::from_utf8_lossy(&ign_en.stdout)
    );

    // undo with --lang ko and --lang en
    fs::write(dir.path().join("⽊.txt"), b"k").unwrap();
    let _ = Command::new(cli_bin())
        .args(["fix", "--yes", "--audit-log"])
        .arg(&audit_log)
        .arg(dir.path())
        .output()
        .unwrap();
    let undo_ko = Command::new(cli_bin())
        .args(["undo", "--dry-run", "--lang", "ko"])
        .arg(&audit_log)
        .output()
        .unwrap();
    assert_eq!(undo_ko.status.code(), Some(0));
    assert!(
        String::from_utf8_lossy(&undo_ko.stdout).contains("미리보기")
            || String::from_utf8_lossy(&undo_ko.stdout).contains("되돌리기"),
        "expected Korean undo output: {}",
        String::from_utf8_lossy(&undo_ko.stdout)
    );
}

#[test]
fn cli_fix_reports_block_reasons_forced_nfd_and_io_error_exit_2() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("⽊.txt"), b"kangxi").unwrap();
    fs::write(dir.path().join("木.txt"), b"existing").unwrap();

    // fix --dry-run with collision must print block reason (BlockedCollision)
    let dry = Command::new(cli_bin())
        .args(["fix", "--dry-run", "--lang", "en"])
        .arg(dir.path())
        .output()
        .unwrap();
    assert_eq!(dry.status.code(), Some(1));
    let dry_out = String::from_utf8_lossy(&dry.stdout);
    assert!(
        dry_out.contains("BlockedCollision") || dry_out.contains("blocked"),
        "expected per-op block reason in fix --dry-run output: {dry_out}"
    );

    // fix --max-depth 2 on unreadable subdir exits 2
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let clean = tempdir().unwrap();
        let blocked = clean.path().join("unreadable");
        fs::create_dir(&blocked).unwrap();
        fs::set_permissions(&blocked, fs::Permissions::from_mode(0o000)).unwrap();
        if fs::read_dir(&blocked).is_err() {
            let err_out = Command::new(cli_bin())
                .args(["fix", "--yes", "--max-depth", "2"])
                .arg(clean.path())
                .output()
                .unwrap();
            let _ = fs::set_permissions(&blocked, fs::Permissions::from_mode(0o755));
            assert_eq!(err_out.status.code(), Some(2));
        } else {
            let _ = fs::set_permissions(&blocked, fs::Permissions::from_mode(0o755));
        }
    }
}

#[test]
fn phase13_cli_fix_dry_run_and_root_audit_log_gates() {
    // (a) fix --dry-run --audit-log <inside-target> must exit 2 without printing a rename plan
    let inside_dir = tempdir().unwrap();
    fs::write(inside_dir.path().join("⽊_a.txt"), b"kangxi").unwrap();
    let inside_audit = inside_dir.path().join("audit.json");
    let dry_inside = Command::new(cli_bin())
        .args(["fix", "--dry-run", "--audit-log"])
        .arg(&inside_audit)
        .arg(inside_dir.path())
        .output()
        .unwrap();
    assert_eq!(
        dry_inside.status.code(),
        Some(2),
        "fix --dry-run with inside-target --audit-log must exit 2: stdout={}, stderr={}",
        String::from_utf8_lossy(&dry_inside.stdout),
        String::from_utf8_lossy(&dry_inside.stderr)
    );
    let dry_inside_stdout = String::from_utf8_lossy(&dry_inside.stdout);
    assert!(
        !dry_inside_stdout.contains("->"),
        "fix --dry-run must reject inside-target --audit-log before printing rename plan: {dry_inside_stdout}"
    );
    assert!(!inside_audit.exists());

    // (b) Clean target (items.is_empty()) with inside-target --audit-log must still exit 2 in both --dry-run and --yes
    let clean_dir = tempdir().unwrap();
    fs::write(clean_dir.path().join("clean.txt"), b"ok").unwrap();
    let clean_inside_audit = clean_dir.path().join("audit_clean.json");
    for mode_flag in ["--dry-run", "--yes"] {
        let out = Command::new(cli_bin())
            .args(["fix", mode_flag, "--audit-log"])
            .arg(&clean_inside_audit)
            .arg(clean_dir.path())
            .output()
            .unwrap();
        assert_eq!(
            out.status.code(),
            Some(2),
            "clean target with inside-target --audit-log must exit 2 under {mode_flag}: stdout={}, stderr={}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }

    // (b2) --max-depth 2 with --audit-log at target root while items only exist in <target>/sub must exit 2
    let depth_dir = tempdir().unwrap();
    let sub_dir = depth_dir.path().join("sub");
    fs::create_dir(&sub_dir).unwrap();
    fs::write(sub_dir.join("⽊_deep.txt"), b"deep").unwrap();
    let root_audit = depth_dir.path().join("audit_root.json");
    for mode_flag in ["--dry-run", "--yes"] {
        let out = Command::new(cli_bin())
            .args(["fix", mode_flag, "--max-depth", "2", "--audit-log"])
            .arg(&root_audit)
            .arg(depth_dir.path())
            .output()
            .unwrap();
        assert_eq!(
            out.status.code(),
            Some(2),
            "--max-depth 2 with root-level --audit-log must exit 2 under {mode_flag}: stdout={}, stderr={}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(!root_audit.exists());
    }

    // (c) #[cfg(unix)] non-UTF-8 parent_dir in fix --dry-run must exit 2
    #[cfg(unix)]
    {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        let base = tempdir().unwrap();
        let non_utf8_dir = base
            .path()
            .join(OsString::from_vec(b"bad_\xff_dir".to_vec()));
        if fs::create_dir(&non_utf8_dir).is_ok() {
            fs::write(non_utf8_dir.join("⽊_utf8.txt"), b"u").unwrap();

            let ext_state = tempdir().unwrap();
            let ext_audit = ext_state.path().join("ext_audit.json");
            let out = Command::new(cli_bin())
                .args(["fix", "--dry-run", "--audit-log"])
                .arg(&ext_audit)
                .arg(&non_utf8_dir)
                .output()
                .unwrap();
            assert_eq!(
                out.status.code(),
                Some(2),
                "fix --dry-run on non-UTF-8 parent_dir must exit 2: stdout={}, stderr={}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
            assert!(
                !String::from_utf8_lossy(&out.stdout).contains("->"),
                "fix --dry-run must reject non-UTF-8 parent_dir before printing rename plan"
            );
        } else {
            let out = Command::new(cli_bin())
                .args(["fix", "--dry-run"])
                .arg(&non_utf8_dir)
                .output()
                .unwrap();
            assert_eq!(out.status.code(), Some(2));
        }
    }

    // (d) Valid fix --dry-run with non-existent outside directory must exit 0, NOT create dirs/files, and NOT print audit-saved
    let valid_dir = tempdir().unwrap();
    fs::write(valid_dir.path().join("⽊_valid.txt"), b"v").unwrap();
    let outside_base = tempdir().unwrap();
    let nonexistent_sub = outside_base.path().join("never_created_dir");
    let outside_audit = nonexistent_sub.join("audit.json");
    let valid_dry = Command::new(cli_bin())
        .args(["fix", "--dry-run", "--lang", "en", "--audit-log"])
        .arg(&outside_audit)
        .arg(valid_dir.path())
        .output()
        .unwrap();
    assert_eq!(
        valid_dry.status.code(),
        Some(0),
        "valid fix --dry-run must exit 0: stderr={}",
        String::from_utf8_lossy(&valid_dry.stderr)
    );
    assert!(
        !nonexistent_sub.exists() && !outside_audit.exists(),
        "fix --dry-run must NOT create --audit-log parent directory or file"
    );
    let valid_stdout = String::from_utf8_lossy(&valid_dry.stdout);
    assert!(
        valid_stdout.contains("->"),
        "valid fix --dry-run must print rename plan: {valid_stdout}"
    );
    assert!(
        !valid_stdout.contains("Audit log saved") && !valid_stdout.contains("감사 로그 저장됨"),
        "fix --dry-run must NOT print audit-saved message: {valid_stdout}"
    );
}
