use std::fs;
use std::process::Command;

use tempfile::tempdir;

fn cli_bin() -> &'static str {
    env!("CARGO_BIN_EXE_namechecker")
}

#[test]
fn cli_supports_format_json_csv_lang_excludes_and_depth_filtering() {
    let dir = tempdir().unwrap();

    // Clean mixed ASCII + Hangul + Hanja files (InherentPlatformCollation only -> exit 0!)
    fs::write(dir.path().join("A.txt"), b"a").unwrap();
    fs::write(dir.path().join("가.txt"), b"ga").unwrap();
    fs::write(dir.path().join("日.txt"), b"il").unwrap();

    // Default metadata inside @eaDir should be excluded by default
    let eadir = dir.path().join("@eaDir");
    fs::create_dir(&eadir).unwrap();
    fs::write(eadir.join("CON"), b"hidden").unwrap();

    // Subdirectory file with issue only visible with --max-depth 2
    let sub = dir.path().join("sub");
    fs::create_dir(&sub).unwrap();
    fs::write(sub.join("⽊.txt"), b"kangxi").unwrap();

    // Clean folder at depth 1 must exit 0 even with InherentPlatformCollation discrepancies
    let out_clean = Command::new(cli_bin())
        .args(["scan", "--format", "json", "--lang", "en"])
        .arg(dir.path())
        .output()
        .unwrap();
    assert_eq!(
        out_clean.status.code(),
        Some(0),
        "clean folder with InherentPlatformCollation must exit 0; stderr={}",
        String::from_utf8_lossy(&out_clean.stderr)
    );
    let clean_json: serde_json::Value = serde_json::from_slice(&out_clean.stdout).unwrap();
    assert_eq!(clean_json["schema_version"], "1.0.0");
    assert!(clean_json["sort_discrepancy_count"].as_u64().unwrap() > 0);
    assert_eq!(clean_json["findings"].as_array().unwrap().len(), 0);

    // --max-depth 2 surfaces ⽊.txt -> exit 1
    let out_depth2 = Command::new(cli_bin())
        .args(["scan", "--max-depth", "2", "--format", "json"])
        .arg(dir.path())
        .output()
        .unwrap();
    assert_eq!(out_depth2.status.code(), Some(1));
    let depth2_json: serde_json::Value = serde_json::from_slice(&out_depth2.stdout).unwrap();
    assert_eq!(depth2_json["findings"][0]["category"], "KangxiRadical");

    // --track manual-review filters out AutoFixable KangxiRadical -> 0 findings
    let out_track = Command::new(cli_bin())
        .args([
            "scan",
            "--max-depth",
            "2",
            "--track",
            "manual-review",
            "--format",
            "json",
        ])
        .arg(dir.path())
        .output()
        .unwrap();
    assert_eq!(out_track.status.code(), Some(0));
    let track_json: serde_json::Value = serde_json::from_slice(&out_track.stdout).unwrap();
    assert_eq!(track_json["findings"].as_array().unwrap().len(), 0);

    // --exclude "*.txt" excludes .txt files at depth 2 too
    let out_excl = Command::new(cli_bin())
        .args([
            "scan",
            "--max-depth",
            "2",
            "--exclude",
            "*.txt",
            "--format",
            "json",
        ])
        .arg(dir.path())
        .output()
        .unwrap();
    assert_eq!(out_excl.status.code(), Some(0));

    // --format csv writes BOM CSV via --output outside scanned target
    let out_dir = tempdir().unwrap();
    let out_file = out_dir.path().join("report.csv");
    let out_csv = Command::new(cli_bin())
        .args(["scan", "--max-depth", "2", "--format", "csv", "--output"])
        .arg(&out_file)
        .arg(dir.path())
        .output()
        .unwrap();
    assert_eq!(out_csv.status.code(), Some(1));
    let csv_bytes = fs::read(&out_file).unwrap();
    assert!(csv_bytes.starts_with(&[0xEF, 0xBB, 0xBF]));
}

#[test]
fn cli_exits_2_over_1_precedence_with_no_default_excludes_and_unreadable_target() {
    let dir = tempdir().unwrap();

    // Hidden inside @eaDir, excluded by default, exposed when --no-default-excludes + --max-depth 2
    let eadir = dir.path().join("@eaDir");
    fs::create_dir(&eadir).unwrap();
    fs::write(eadir.join("CON"), b"reserved").unwrap();

    // Default excludes ignore @eaDir -> exit 0
    let out_default = Command::new(cli_bin())
        .args(["scan", "--max-depth", "2", "--format", "json"])
        .arg(dir.path())
        .output()
        .unwrap();
    assert_eq!(out_default.status.code(), Some(0));

    // --no-default-excludes surfaces @eaDir/CON (Severity::Error WindowsSmbHazard) -> exit 1
    let out_no_default = Command::new(cli_bin())
        .args([
            "scan",
            "--max-depth",
            "2",
            "--no-default-excludes",
            "--severity",
            "error",
            "--format",
            "json",
        ])
        .arg(dir.path())
        .output()
        .unwrap();
    assert_eq!(out_no_default.status.code(), Some(1));
    let json_val: serde_json::Value = serde_json::from_slice(&out_no_default.stdout).unwrap();
    assert!(
        json_val["findings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["category"] == "WindowsSmbHazard")
    );

    // Unreadable subdir alongside valid finding -> exit code 2 (precedence 2 > 1 > 0)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let blocked = dir.path().join("unreadable");
        fs::create_dir(&blocked).unwrap();
        fs::set_permissions(&blocked, fs::Permissions::from_mode(0o000)).unwrap();
        if fs::read_dir(&blocked).is_err() {
            let out_err = Command::new(cli_bin())
                .args([
                    "scan",
                    "--max-depth",
                    "2",
                    "--no-default-excludes",
                    "--format",
                    "json",
                ])
                .arg(dir.path())
                .output()
                .unwrap();
            let _ = fs::set_permissions(&blocked, fs::Permissions::from_mode(0o755));
            assert_eq!(out_err.status.code(), Some(2));
        } else {
            let _ = fs::set_permissions(&blocked, fs::Permissions::from_mode(0o755));
        }
    }

    let bad_path = dir.path().join("does_not_exist");
    let out = Command::new(cli_bin())
        .args(["scan", "--severity", "warning"])
        .arg(&bad_path)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn cli_accepts_follow_symlinks_cycle_safe_profiles_aliases_and_track_all_auto_manual() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("⽊.txt"), b"kangxi").unwrap();
    fs::write(dir.path().join("note〜.txt"), b"wave").unwrap();

    #[cfg(unix)]
    {
        let sub = dir.path().join("sub");
        fs::create_dir(&sub).unwrap();
        fs::write(sub.join("nested_⽊.txt"), b"n").unwrap();
        std::os::unix::fs::symlink(dir.path(), sub.join("loop_back")).unwrap();

        let out_sym = Command::new(cli_bin())
            .args([
                "scan",
                "--follow-symlinks",
                "--max-depth",
                "3",
                "--profiles",
                "windows-ko,linux-c",
                "--track",
                "auto",
                "--format",
                "json",
            ])
            .arg(dir.path())
            .output()
            .unwrap();
        assert_eq!(out_sym.status.code(), Some(1));
        let v: serde_json::Value = serde_json::from_slice(&out_sym.stdout).unwrap();
        assert_eq!(v["manual_review_count"], 0);
        assert_eq!(v["auto_fixable_count"], 2);
    }

    let out_manual = Command::new(cli_bin())
        .args(["scan", "--track", "manual", "--format", "json"])
        .arg(dir.path())
        .output()
        .unwrap();
    assert_eq!(out_manual.status.code(), Some(1));
    let m: serde_json::Value = serde_json::from_slice(&out_manual.stdout).unwrap();
    assert_eq!(m["auto_fixable_count"], 0);
    assert_eq!(m["manual_review_count"], 1);

    let out_all = Command::new(cli_bin())
        .args(["scan", "--track", "all", "--format", "json"])
        .arg(dir.path())
        .output()
        .unwrap();
    assert_eq!(out_all.status.code(), Some(1));
    let a: serde_json::Value = serde_json::from_slice(&out_all.stdout).unwrap();
    assert_eq!(a["auto_fixable_count"], 1);
    assert_eq!(a["manual_review_count"], 1);
}

#[test]
fn cli_supports_default_scan_subcommand_category_filter_and_no_arg_exit_2() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("⽊.txt"), b"kangxi").unwrap();
    fs::write(dir.path().join("note〜.txt"), b"wave").unwrap();

    // Default scan without explicit `scan` subcommand
    let out_default = Command::new(cli_bin())
        .args(["--format", "json", "--category", "KangxiRadical"])
        .arg(dir.path())
        .output()
        .unwrap();
    assert_eq!(
        out_default.status.code(),
        Some(1),
        "default scan failed: stderr={}",
        String::from_utf8_lossy(&out_default.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out_default.stdout).unwrap();
    assert_eq!(v["auto_fixable_count"], 1);
    assert_eq!(v["manual_review_count"], 0);
    assert_eq!(v["findings"].as_array().unwrap().len(), 1);
    assert_eq!(v["findings"][0]["category"], "KangxiRadical");

    // Directory literally named `fix` disambiguated with `./fix` or `scan fix`
    let fix_dir = dir.path().join("fix");
    fs::create_dir(&fix_dir).unwrap();
    let out_fix_dir = Command::new(cli_bin())
        .current_dir(dir.path())
        .args(["./fix", "--format", "json"])
        .output()
        .unwrap();
    assert_eq!(out_fix_dir.status.code(), Some(0));

    // Invoking with neither subcommand nor <PATH> exits with code 2
    let out_empty = Command::new(cli_bin()).output().unwrap();
    assert_eq!(out_empty.status.code(), Some(2));
}

#[test]
fn cli_scan_help_documents_canonical_profile_names() {
    let help = Command::new(cli_bin())
        .args(["scan", "--help"])
        .output()
        .unwrap();
    assert!(help.status.success());
    let stdout = String::from_utf8_lossy(&help.stdout);
    assert!(
        stdout.contains("windows-ko,macos-ko,linux-c,ja,zh"),
        "expected canonical profiles in --help: {stdout}"
    );
}
