use std::collections::BTreeMap;
use std::path::PathBuf;

use namechecker_core::model::{
    CharDetail, CollationProfile, DiscrepancyCause, EncodingHint, EntryFinding, EntryRenamePlan,
    EntryType, IssueCategory, RawOsName, RemediationTrack, SCHEMA_VERSION, ScanReport, Severity,
    SortDiscrepancy, UNICODE_VERSION, UiLanguage,
};
use namechecker_core::report::{
    escape_csv_formula, format_csv, format_json, format_text, sanitize_terminal_text,
};

fn sample_report() -> ScanReport {
    let dir = PathBuf::from("/tmp/sample");
    let malicious_name = "=CMD('calc')\x1b[31m\u{202E}bad\t.txt".to_string();
    let p = dir.join(&malicious_name);
    let raw_os = RawOsName::UnixBytes(malicious_name.as_bytes().to_vec());

    let finding = EntryFinding {
        id: "id-1".to_string(),
        path: p.clone(),
        parent_dir: dir.clone(),
        raw_name: malicious_name.clone(),
        raw_os_name: raw_os.clone(),
        entry_type: EntryType::File,
        track: RemediationTrack::ManualReview,
        category: IssueCategory::OrphanedTempFile,
        severity: Severity::Error,
        offending_chars: vec![CharDetail {
            char_index: 12,
            glyph: None,
            code_point: "U+001B".to_string(),
            reason: "control".to_string(),
        }],
        suggested_name: None,
        candidate_hints: vec![EncodingHint {
            encoding: "UndoRecovery".to_string(),
            decoded_name: "+recover_with_undo".to_string(),
        }],
        message: "Run `namechecker undo`".to_string(),
    };

    let plan = EntryRenamePlan {
        path: p,
        raw_name: malicious_name,
        raw_os_name: raw_os,
        composed_suggested_name: Some("-safe_draft.txt".to_string()),
        track: RemediationTrack::ManualReview,
        finding_ids: vec!["id-1".to_string()],
    };

    ScanReport {
        schema_version: SCHEMA_VERSION.to_string(),
        unicode_version: UNICODE_VERSION.to_string(),
        scanned_at_utc: "2026-11-19T00:00:00Z".to_string(),
        total_entries: 1,
        auto_fixable_count: 0,
        manual_review_count: 1,
        sort_discrepancy_count: 1,
        collision_risk_count: 0,
        encoding_counts: BTreeMap::from([("UTF-8 NFC".to_string(), 1)]),
        category_counts: BTreeMap::from([("OrphanedTempFile".to_string(), 1)]),
        before_order: Vec::new(),
        predicted_after_order: Vec::new(),
        io_errors: Vec::new(),
        findings: vec![finding],
        rename_plans: vec![plan],
        sort_discrepancies: vec![SortDiscrepancy {
            parent_dir: dir,
            entry_a: "A".to_string(),
            entry_b: "가".to_string(),
            baseline_profile: CollationProfile::WindowsKoKr,
            divergent_profiles: vec![
                CollationProfile::MacOsKo,
                CollationProfile::Japanese,
                CollationProfile::Chinese,
                CollationProfile::LinuxCodePoint,
            ],
            cause: DiscrepancyCause::InherentPlatformCollation,
            reason: "inherent".to_string(),
        }],
        collision_risks: Vec::new(),
    }
}

#[test]
fn terminal_text_sanitizes_ansi_osc_controls_and_bidi_overrides() {
    let dirty = "hello\x1b[31m\x1b]2;pwned\x07\u{202E}world\n";
    let safe = sanitize_terminal_text(dirty);
    assert!(!safe.contains('\x1b'));
    assert!(!safe.contains('\x07'));
    assert!(!safe.contains('\u{202E}'));
    assert!(!safe.contains('\n'));
    assert!(safe.contains("<U+001B>"));
    assert!(safe.contains("<U+202E>"));

    let report = sample_report();
    let rendered = format_text(&report, UiLanguage::En);
    assert!(!rendered.contains('\x1b'));
    assert!(!rendered.contains('\u{202E}'));
    assert!(rendered.contains("OrphanedTempFile") || rendered.contains("Orphaned"));
}

#[test]
fn json_report_matches_contract_schema_fields() {
    let report = sample_report();
    let json_str = format_json(&report).expect("format_json");
    let v: serde_json::Value = serde_json::from_str(&json_str).unwrap();

    assert_eq!(v["schema_version"], "1.0.0");
    assert_eq!(v["unicode_version"], "16.0.0");
    assert!(v["rename_plans"].is_array());
    assert_eq!(v["findings"][0]["category"], "OrphanedTempFile");
    assert_eq!(
        v["sort_discrepancies"][0]["cause"],
        "InherentPlatformCollation"
    );
    assert_eq!(
        v["sort_discrepancies"][0]["divergent_profiles"]
            .as_array()
            .unwrap()
            .len(),
        4
    );
}

#[test]
fn csv_report_includes_utf8_bom_12_columns_and_formula_injection_escaping() {
    assert_eq!(escape_csv_formula("=1+1"), "'=1+1");
    assert_eq!(escape_csv_formula("+cmd"), "'+cmd");
    assert_eq!(escape_csv_formula("-cmd"), "'-cmd");
    assert_eq!(escape_csv_formula("@SUM"), "'@SUM");
    assert_eq!(escape_csv_formula("\tcmd"), "'\tcmd");
    assert_eq!(escape_csv_formula("safe.txt"), "safe.txt");

    let report = sample_report();
    let bytes = format_csv(&report).expect("format_csv");
    assert!(bytes.starts_with(&[0xEF, 0xBB, 0xBF]), "missing UTF-8 BOM");

    let body = std::str::from_utf8(&bytes[3..]).unwrap();
    let mut rdr = csv::ReaderBuilder::new().from_reader(body.as_bytes());
    let headers = rdr.headers().unwrap();
    assert_eq!(
        headers.iter().collect::<Vec<_>>(),
        vec![
            "path",
            "parent_dir",
            "raw_name",
            "entry_type",
            "track",
            "category",
            "severity",
            "offending_codepoints",
            "suggested_name",
            "composed_suggested_name",
            "candidate_hints",
            "message"
        ]
    );

    let rows: Vec<_> = rdr.records().map(|r| r.unwrap()).collect();
    assert_eq!(rows.len(), 1);
    assert!(
        rows[0][2].starts_with("'=CMD"),
        "raw_name formula escaped: {:?}",
        &rows[0][2]
    );
    assert!(!rows[0][2].contains('\x1b'));
    assert_eq!(&rows[0][9], "'-safe_draft.txt");
    assert_eq!(&rows[0][10], "UndoRecovery:+recover_with_undo");
}

#[test]
fn text_report_includes_severity_codepoints_hints_and_localized_ko_en_summary() {
    let report = sample_report();

    let ko = format_text(&report, UiLanguage::Ko);
    assert!(ko.contains("총 항목: 1"), "missing Korean summary: {ko}");
    assert!(ko.contains("Error"), "missing severity: {ko}");
    assert!(ko.contains("U+001B"), "missing offending codepoint: {ko}");
    assert!(ko.contains("UndoRecovery"), "missing candidate hint: {ko}");

    let en = format_text(&report, UiLanguage::En);
    assert!(
        en.contains("Total entries: 1"),
        "missing English summary: {en}"
    );
    assert!(en.contains("Error"), "missing severity: {en}");
    assert!(en.contains("U+001B"), "missing offending codepoint: {en}");
    assert!(en.contains("UndoRecovery"), "missing candidate hint: {en}");
}

#[test]
fn csv_report_emits_12_column_header_when_findings_empty_and_no_duplicate_when_non_empty() {
    let mut empty_report = sample_report();
    empty_report.findings.clear();
    empty_report.rename_plans.clear();
    empty_report.manual_review_count = 0;

    let bytes = format_csv(&empty_report).expect("empty csv");
    assert_eq!(&bytes[..3], &[0xEF, 0xBB, 0xBF], "UTF-8 BOM required");
    let body = std::str::from_utf8(&bytes[3..]).unwrap();
    let lines: Vec<&str> = body.lines().collect();
    assert_eq!(
        lines.len(),
        1,
        "empty CSV must contain exactly 1 header row: {body:?}"
    );
    assert_eq!(
        lines[0],
        "path,parent_dir,raw_name,entry_type,track,category,severity,offending_codepoints,suggested_name,composed_suggested_name,candidate_hints,message"
    );

    // Non-empty report must emit exactly 1 header + 1 data row (no duplicate header)
    let non_empty_bytes = format_csv(&sample_report()).expect("non-empty csv");
    let non_empty_body = std::str::from_utf8(&non_empty_bytes[3..]).unwrap();
    let non_empty_lines: Vec<&str> = non_empty_body.lines().collect();
    assert_eq!(
        non_empty_lines.len(),
        2,
        "expected 1 header + 1 row: {non_empty_body:?}"
    );
}

#[test]
fn filter_report_supports_category_filtering_and_recomputes_summary_counts() {
    use namechecker_core::report::filter_report;

    let mut report = sample_report();
    report
        .before_order
        .push(namechecker_core::model::OrderedEntry {
            rank: 1,
            name: report.findings[0].raw_name.clone(),
            entry_type: EntryType::File,
            has_issue: true,
        });
    let kept = filter_report(
        report.clone(),
        None,
        None,
        Some(&[IssueCategory::OrphanedTempFile]),
    );
    assert_eq!(kept.findings.len(), 1);
    assert_eq!(kept.manual_review_count, 1);
    assert_eq!(kept.category_counts.get("OrphanedTempFile"), Some(&1));

    let filtered_out = filter_report(report, None, None, Some(&[IssueCategory::KangxiRadical]));
    assert_eq!(filtered_out.findings.len(), 0);
    assert_eq!(filtered_out.manual_review_count, 0);
    assert!(filtered_out.category_counts.is_empty());
    assert!(filtered_out.rename_plans.is_empty());
    assert!(!filtered_out.before_order[0].has_issue);
}

#[test]
fn format_json_handles_non_utf8_unix_report_paths_lossily() {
    #[cfg(unix)]
    {
        use namechecker_core::model::IoErrorRecord;
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;
        let bad_dir = PathBuf::from(OsStr::from_bytes(b"/tmp/dir_\xff"));
        let bad_file = bad_dir.join(OsStr::from_bytes(b"file_\xfe.txt"));

        let mut report = sample_report();
        report.findings[0].path = bad_file.clone();
        report.findings[0].parent_dir = bad_dir.clone();
        report.rename_plans[0].path = bad_file.clone();
        report.io_errors.push(IoErrorRecord {
            path: bad_file,
            message: "err".to_string(),
        });
        report.sort_discrepancies.push(SortDiscrepancy {
            parent_dir: bad_dir.clone(),
            entry_a: "a".to_string(),
            entry_b: "b".to_string(),
            baseline_profile: CollationProfile::WindowsKoKr,
            divergent_profiles: vec![CollationProfile::LinuxCodePoint],
            cause: DiscrepancyCause::EncodingAnomaly,
            reason: "reason".to_string(),
        });
        report
            .collision_risks
            .push(namechecker_core::model::CollisionRisk {
                parent_dir: bad_dir,
                entries: vec!["a".to_string(), "A".to_string()],
                normalized_key: "a".to_string(),
                collision_type: namechecker_core::model::CollisionType::CaseInsensitive,
                affected_platforms: vec!["Windows".to_string()],
            });

        let json = format_json(&report).expect("format_json must handle non-UTF-8 paths lossily");
        assert!(
            json.contains("file_\u{FFFD}.txt"),
            "expected lossy U+FFFD path in JSON: {json}"
        );
    }
}
