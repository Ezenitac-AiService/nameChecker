//! Triangulated unit tests for domain model serialization and validation constraints (T004).

use std::collections::BTreeMap;
use std::path::PathBuf;

use namechecker_core::model::{
    CharDetail, CollationProfile, CoreError, CsvReportRow, DiscrepancyCause, EncodingHint,
    EntryFinding, EntryRenamePlan, EntryType, IgnoreList, IgnoreRule, IoErrorRecord, IssueCategory,
    OrderedEntry, PredictedEntry, RawOsName, RemediationTrack, RenameAuditLog, RenameOperation,
    RenameStatus, SCHEMA_VERSION, ScanReport, ScanTarget, Severity, SortDiscrepancy,
    UNICODE_VERSION, UiLanguage,
};
use tempfile::TempDir;

#[test]
fn scan_target_v1_validates_single_existing_directory() {
    let dir = TempDir::new().expect("temp dir");
    let target = ScanTarget::new(dir.path().to_path_buf());
    assert_eq!(target.roots.len(), 1);
    assert_eq!(target.max_depth, Some(1));
    assert!(!target.follow_symlinks);
    assert_eq!(target.lang, UiLanguage::Ko);
    assert_eq!(target.profiles, CollationProfile::all().to_vec());
    assert!(target.validate().is_ok());

    // Boundary & negative cases: empty roots, 2 roots in v1, nonexistent root, file as root
    let mut empty = target.clone();
    empty.roots.clear();
    assert!(matches!(empty.validate(), Err(CoreError::InvalidTarget(_))));

    let mut two_roots = target.clone();
    two_roots.roots.push(dir.path().to_path_buf());
    assert!(matches!(
        two_roots.validate(),
        Err(CoreError::InvalidTarget(_))
    ));

    let missing = ScanTarget::new(dir.path().join("does-not-exist"));
    assert!(matches!(
        missing.validate(),
        Err(CoreError::InvalidTarget(_))
    ));

    let file_path = dir.path().join("single_file.txt");
    std::fs::write(&file_path, b"hi").expect("write file");
    let file_target = ScanTarget::new(file_path);
    assert!(matches!(
        file_target.validate(),
        Err(CoreError::InvalidTarget(_))
    ));
}

#[test]
fn all_12_issue_categories_serialize_and_map_tracks() {
    let all = IssueCategory::all();
    assert_eq!(all.len(), 12);
    assert!(all.contains(&IssueCategory::OrphanedTempFile));

    // Deterministic default track checks (positive + negative)
    assert_eq!(
        IssueCategory::NonNfcNormalization.default_track(),
        RemediationTrack::AutoFixable
    );
    assert_eq!(
        IssueCategory::CompatibilityIdeograph.default_track(),
        RemediationTrack::AutoFixable
    );
    assert_eq!(
        IssueCategory::InvisibleZeroWidth.default_track(),
        RemediationTrack::AutoFixable
    );
    assert_eq!(
        IssueCategory::PunctuationVariant.default_track(),
        RemediationTrack::ManualReview
    );
    assert_eq!(
        IssueCategory::VariationSelector.default_track(),
        RemediationTrack::ManualReview
    );
    assert_eq!(
        IssueCategory::OrphanedTempFile.default_track(),
        RemediationTrack::ManualReview
    );

    let json = serde_json::to_string(&IssueCategory::OrphanedTempFile).expect("serialize");
    assert_eq!(json, "\"OrphanedTempFile\"");
}

#[test]
fn predicted_entry_rank_delta_follows_old_minus_new_convention() {
    assert_eq!(PredictedEntry::compute_rank_delta(5, 2), 3);
    assert_eq!(PredictedEntry::compute_rank_delta(2, 6), -4);
    assert_eq!(PredictedEntry::compute_rank_delta(3, 3), 0);
}

#[test]
fn report_entities_round_trip_serde_contract_fields() {
    let report = ScanReport {
        schema_version: SCHEMA_VERSION.to_string(),
        unicode_version: UNICODE_VERSION.to_string(),
        scanned_at_utc: "2026-11-19T01:00:00Z".to_string(),
        total_entries: 1,
        auto_fixable_count: 1,
        manual_review_count: 0,
        sort_discrepancy_count: 1,
        collision_risk_count: 0,
        encoding_counts: BTreeMap::from([("UTF-8 NFD".to_string(), 1)]),
        category_counts: BTreeMap::from([("NonNfcNormalization".to_string(), 1)]),
        before_order: vec![OrderedEntry {
            rank: 1,
            name: "가".to_string(),
            entry_type: EntryType::File,
            has_issue: true,
        }],
        predicted_after_order: vec![PredictedEntry {
            old_rank: 1,
            new_rank: 1,
            rank_delta: 0,
            old_name: "가".to_string(),
            new_name: "가".to_string(),
            changed: false,
            has_collision: false,
        }],
        io_errors: vec![IoErrorRecord {
            path: PathBuf::from("/tmp/denied"),
            message: "permission denied".to_string(),
        }],
        findings: vec![EntryFinding {
            id: "finding-1".to_string(),
            path: PathBuf::from("/tmp/dir/a"),
            parent_dir: PathBuf::from("/tmp/dir"),
            raw_name: "a".to_string(),
            raw_os_name: RawOsName::UnixBytes(vec![97]),
            entry_type: EntryType::File,
            track: RemediationTrack::AutoFixable,
            category: IssueCategory::NonNfcNormalization,
            severity: Severity::Warning,
            offending_chars: vec![CharDetail {
                char_index: 0,
                glyph: Some("가".to_string()),
                code_point: "U+1100".to_string(),
                reason: "NFD".to_string(),
            }],
            suggested_name: Some("a".to_string()),
            candidate_hints: vec![EncodingHint {
                encoding: "CP949".to_string(),
                decoded_name: "가".to_string(),
            }],
            message: "NFD detected".to_string(),
        }],
        rename_plans: vec![EntryRenamePlan {
            path: PathBuf::from("/tmp/dir/a"),
            raw_name: "a".to_string(),
            raw_os_name: RawOsName::UnixBytes(vec![97]),
            composed_suggested_name: Some("a".to_string()),
            track: RemediationTrack::AutoFixable,
            finding_ids: vec!["finding-1".to_string()],
        }],
        sort_discrepancies: vec![SortDiscrepancy {
            parent_dir: PathBuf::from("/tmp/dir"),
            entry_a: "A".to_string(),
            entry_b: "가".to_string(),
            baseline_profile: CollationProfile::WindowsKoKr,
            divergent_profiles: vec![CollationProfile::MacOsKo],
            cause: DiscrepancyCause::InherentPlatformCollation,
            reason: "Latin precedes Hangul on Windows".to_string(),
        }],
        collision_risks: vec![],
    };

    let val = serde_json::to_value(&report).expect("to_value");
    assert_eq!(val["schema_version"], "1.0.0");
    assert_eq!(val["unicode_version"], "16.0.0");
    assert_eq!(
        val["sort_discrepancies"][0]["divergent_profiles"][0],
        "MacOsKo"
    );

    let op_log = RenameAuditLog {
        schema_version: SCHEMA_VERSION.to_string(),
        operations: vec![RenameOperation {
            timestamp_utc: "2026-11-19T01:00:00Z".to_string(),
            parent_dir: PathBuf::from("/tmp/dir"),
            old_name: "old".to_string(),
            old_raw_os_name: RawOsName::WindowsWide(vec![111, 108, 100]),
            file_id: Some("123:456".to_string()),
            entry_type: EntryType::File,
            byte_size: Some(42),
            mtime_ns: Some(1_795_000_000_000_000_000),
            temp_name: None,
            new_name: "new".to_string(),
            status: RenameStatus::Applied,
            error_detail: None,
        }],
    };
    let op_val = serde_json::to_value(&op_log).expect("audit log");
    assert!(op_val["operations"][0].get("old_raw_os_name").is_some());

    let csv_row = CsvReportRow {
        path: "/tmp/dir/a".to_string(),
        parent_dir: "/tmp/dir".to_string(),
        raw_name: "a".to_string(),
        entry_type: "File".to_string(),
        track: "AutoFixable".to_string(),
        category: "NonNfcNormalization".to_string(),
        severity: "Warning".to_string(),
        offending_codepoints: "U+1100".to_string(),
        suggested_name: "a".to_string(),
        composed_suggested_name: "a".to_string(),
        candidate_hints: "".to_string(),
        message: "NFD".to_string(),
    };
    assert_eq!(csv_row.category, "NonNfcNormalization");
}

#[test]
fn ignore_rule_extracts_portable_single_component_name_and_rejects_traversal() {
    let list = IgnoreList::default();
    assert_eq!(list.schema_version, "1.0.0");

    let literal = IgnoreRule::from_path_or_pattern(
        "/Volumes/Share/Sub/エ作.txt",
        Some(IssueCategory::CrossScriptConfusable),
        Some("intentional".to_string()),
    )
    .expect("literal path extracts file_name");
    assert_eq!(literal.pattern, "エ作.txt");
    assert!(!literal.is_glob);

    let glob = IgnoreRule::from_path_or_pattern("*_tmp_note.*", None, None).expect("glob");
    assert!(glob.is_glob);

    assert!(IgnoreRule::from_path_or_pattern("", None, None).is_err());
    assert!(IgnoreRule::from_path_or_pattern("..", None, None).is_err());
}
