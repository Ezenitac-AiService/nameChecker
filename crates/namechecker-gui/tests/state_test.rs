use std::fs;
use std::time::{Duration, Instant};

use namechecker_core::model::{CollationProfile, DiscrepancyCause, ScanTarget, UiLanguage};
use namechecker_core::scanner::scan_target;
use namechecker_gui::state::NameCheckerAppState;
use tempfile::tempdir;

#[test]
fn state_tracks_3_pane_prediction_overflow_collision_discrepancies_and_workers() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("A.txt"), b"a").unwrap();
    fs::write(dir.path().join("가.txt"), b"ga").unwrap();
    fs::write(dir.path().join("日.txt"), b"il").unwrap();
    fs::write(dir.path().join("⽊_test.txt"), b"kangxi").unwrap();
    fs::write(dir.path().join("existing.txt"), b"ex").unwrap();

    // Start background worker scan
    let mut state = NameCheckerAppState::new(UiLanguage::Ko);
    state.start_scan(dir.path().to_path_buf());
    assert!(state.is_busy);

    let deadline = Instant::now() + Duration::from_secs(5);
    while state.is_busy && Instant::now() < deadline {
        state.poll_worker_events();
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(!state.is_busy, "scan worker did not finish in time");
    assert!(state.report.is_some());
    assert_eq!(state.before_order.len(), 5);
    assert_eq!(state.predicted_after_order.len(), 5);

    // At default WindowsKoKr, InherentPlatformCollation discrepancies are omitted on clean siblings
    assert!(
        state
            .visible_discrepancies
            .iter()
            .all(|d| d.cause == DiscrepancyCause::EncodingAnomaly)
    );

    // Switching profile reveals InherentPlatformCollation discrepancies
    state.set_active_profile(CollationProfile::MacOsKo);
    assert!(
        state
            .visible_discrepancies
            .iter()
            .any(|d| d.cause == DiscrepancyCause::InherentPlatformCollation)
    );
    state.set_active_profile(CollationProfile::WindowsKoKr);

    // Synchronized selection works across panes
    state.select_entry(Some("⽊_test.txt"));
    assert_eq!(state.selected_entry_name.as_deref(), Some("⽊_test.txt"));

    assert!(state.can_apply_renames());

    // Edit target to >255 bytes -> overflow blocks [수정]
    let path = dir.path().join("⽊_test.txt");
    state.update_row_edit(&path, "가".repeat(90)); // 270 UTF-8 bytes
    let row = state.rows.iter().find(|r| r.path == path).unwrap();
    assert!(row.has_length_overflow);
    assert!(!state.can_apply_renames());

    // Edit target to collide with "existing.txt" -> collision blocks [수정]
    state.update_row_edit(&path, "existing.txt".to_string());
    state.request_debounced_prediction();
    let pred_deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < pred_deadline {
        state.poll_worker_events();
        if state
            .predicted_after_order
            .iter()
            .any(|p| p.old_name == "⽊_test.txt" && p.has_collision)
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(!state.can_apply_renames());

    // Reset back to clean target "木_test.txt" and verify apply via load_report
    let clean_report = scan_target(&ScanTarget::new(dir.path().to_path_buf())).unwrap();
    state.load_report(clean_report);
    assert!(state.can_apply_renames());
}

#[test]
fn state_persists_ignore_toggles_guards_orphaned_and_switches_language() {
    use namechecker_core::ignore_list::{add_ignore_rule, load_ignore_list, save_ignore_list};
    use namechecker_core::model::IgnoreList;

    let dir = tempdir().unwrap();
    let cfg_dir = tempdir().unwrap();
    let ignore_path = cfg_dir.path().join("ignore.json");

    // Pre-populate a CLI glob rule that must be preserved when GUI toggles an exact rule off.
    let mut initial = IgnoreList::default();
    add_ignore_rule(&mut initial, "*.legacy", None, Some("cli rule".to_string())).unwrap();
    save_ignore_list(&ignore_path, &initial).unwrap();

    fs::write(dir.path().join("노트〜초안.txt"), b"wave").unwrap();
    fs::write(dir.path().join(".__nc_tmp_deadbeef"), b"tmp").unwrap();

    let mut state = NameCheckerAppState::new(UiLanguage::Ko);
    state.set_ignore_list_path(Some(ignore_path.clone()));
    state.set_language(UiLanguage::En);
    assert_eq!(state.lang, UiLanguage::En);

    state.start_scan(dir.path().to_path_buf());
    let deadline = Instant::now() + Duration::from_secs(5);
    while state.is_busy && Instant::now() < deadline {
        state.poll_worker_events();
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(!state.is_busy);

    // OrphanedTempFile cannot be ignored via GUI toggle
    let orphan_path = dir.path().join(".__nc_tmp_deadbeef");
    state.toggle_row_ignored(&orphan_path, true);
    let orphan_row = state.rows.iter().find(|r| r.path == orphan_path).unwrap();
    assert!(!orphan_row.ignored);

    // Toggling ignore ON for 노트〜초안.txt persists exact rule (category: None)
    let wave_path = dir.path().join("노트〜초안.txt");
    state.toggle_row_ignored(&wave_path, true);
    let saved = load_ignore_list(&ignore_path).unwrap();
    assert!(
        saved
            .rules
            .iter()
            .any(|r| !r.is_glob && r.pattern == "노트〜초안.txt" && r.category.is_none()),
        "ignore rule must be persisted: {saved:?}"
    );

    // Subsequent scan with ignore.json omits 노트〜초안.txt ManualReview
    state.start_scan(dir.path().to_path_buf());
    let deadline2 = Instant::now() + Duration::from_secs(5);
    while state.is_busy && Instant::now() < deadline2 {
        state.poll_worker_events();
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(state.report.as_ref().unwrap().manual_review_count, 1);

    // Toggling ignore OFF removes only exact rule and preserves *.legacy glob rule
    state.toggle_row_ignored(&wave_path, false);
    let after_off = load_ignore_list(&ignore_path).unwrap();
    assert!(
        !after_off
            .rules
            .iter()
            .any(|r| !r.is_glob && r.pattern == "노트〜초안.txt")
    );
    assert!(
        after_off
            .rules
            .iter()
            .any(|r| r.is_glob && r.pattern == "*.legacy")
    );
}

#[test]
fn state_stages_external_undo_confirmation_and_relocalizes_messages_on_language_switch() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join(".__nc_tmp_cafe1234"), b"tmp").unwrap();
    fs::write(dir.path().join("가.txt"), b"a").unwrap();
    fs::write(dir.path().join("A.txt"), b"b").unwrap();

    let report = scan_target(&ScanTarget::new(dir.path().to_path_buf())).unwrap();
    let mut state = NameCheckerAppState::new(UiLanguage::Ko);
    state.load_report(report);
    state.set_active_profile(CollationProfile::MacOsKo);

    // Language switch to En must refresh OrphanedTempFile row.message and visible_discrepancies reason
    state.set_language(UiLanguage::En);
    let orphan_row = state
        .rows
        .iter()
        .find(|r| r.raw_name == ".__nc_tmp_cafe1234")
        .unwrap();
    assert!(
        orphan_row.message.contains("Stranded temporary file")
            && orphan_row.message.contains("namechecker undo"),
        "OrphanedTempFile message must refresh to English: {}",
        orphan_row.message
    );
    if let Some(disc) = state.visible_discrepancies.first() {
        assert!(
            !disc.reason.contains("정렬") && !disc.reason.contains("인코딩"),
            "discrepancy reason must refresh to English: {}",
            disc.reason
        );
    }

    // External undo staging reads audit log operation count before execution
    let audit_dir = tempdir().unwrap();
    let audit_path = audit_dir.path().join("namechecker-rename-audit-test.json");
    fs::write(
        &audit_path,
        r#"{"schema_version":"1.0.0","operations":[{"timestamp_utc":"2026-11-23T00:00:00Z","parent_dir":".","old_name":"a","old_raw_os_name":{"UnixBytes":[97]},"file_id":null,"entry_type":"File","byte_size":1,"mtime_ns":1,"temp_name":null,"new_name":"b","status":"Applied","error_detail":null}]}"#,
    )
    .unwrap();
    state.request_external_undo(audit_path.clone());
    assert_eq!(
        state.pending_undo_path.as_deref(),
        Some(audit_path.as_path())
    );
    assert_eq!(state.pending_undo_count, 1);
}

#[test]
fn state_phase10_debounce_race_banner_preservation_bracket_ignore_and_profile_lang() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("⽊.txt"), b"kangxi").unwrap();
    fs::write(dir.path().join("[작가] 화보〜1.zip"), b"wave").unwrap();
    fs::write(dir.path().join("[미완성〜.zip"), b"wave2").unwrap();

    let ignore_dir = tempdir().unwrap();
    let ignore_path = ignore_dir.path().join("ignore.json");

    let mut state = NameCheckerAppState::new(UiLanguage::Ko);
    state.set_ignore_list_path(Some(ignore_path.clone()));
    state.start_scan(dir.path().to_path_buf());
    let deadline = Instant::now() + Duration::from_secs(5);
    while state.is_busy && Instant::now() < deadline {
        state.poll_worker_events();
        std::thread::sleep(Duration::from_millis(10));
    }

    // 1. Bracketed CJK filenames toggle ignore ON and OFF as exact !is_glob rules
    for fname in ["[작가] 화보〜1.zip", "[미완성〜.zip"] {
        let p = dir.path().join(fname);
        state.toggle_row_ignored(&p, true);
        assert!(state.rows.iter().find(|r| r.path == p).unwrap().ignored);
        state.toggle_row_ignored(&p, false);
        assert!(!state.rows.iter().find(|r| r.path == p).unwrap().ignored);
    }

    // 2. AutoFixable rows cannot be ignored; corrupted ignore.json must surface error without mutating row.ignored
    let kangxi_path = dir.path().join("⽊.txt");
    state.toggle_row_ignored(&kangxi_path, true);
    assert!(
        !state
            .rows
            .iter()
            .find(|r| r.path == kangxi_path)
            .unwrap()
            .ignored
    );

    let wave_path = dir.path().join("[작가] 화보〜1.zip");
    fs::write(&ignore_path, "{corrupted json").unwrap();
    state.toggle_row_ignored(&wave_path, true);
    assert!(
        !state
            .rows
            .iter()
            .find(|r| r.path == wave_path)
            .unwrap()
            .ignored,
        "corrupted ignore.json must not mutate row.ignored"
    );
    assert!(state.status_banner.is_some());

    // Manual start_scan clears stale error banner
    let _ = fs::remove_file(&ignore_path);
    state.start_scan(dir.path().to_path_buf());
    let deadline2 = Instant::now() + Duration::from_secs(5);
    while state.is_busy && Instant::now() < deadline2 {
        state.poll_worker_events();
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(state.status_banner.is_none());

    // 3. Pending debounce blocks can_apply_renames() and start_apply_renames(); load_report resets debounce
    state.update_row_edit(&kangxi_path, "edited_목.txt".to_string());
    assert!(
        !state.can_apply_renames(),
        "pending debounce must block can_apply_renames"
    );
    let audit_before = tempdir().unwrap().path().join("blocked_apply.json");
    state.start_apply_renames(audit_before.clone());
    assert!(
        !state.is_busy,
        "start_apply_renames must refuse stale/pending debounce"
    );

    let fresh_report = scan_target(&ScanTarget::new(dir.path().to_path_buf())).unwrap();
    state.load_report(fresh_report);
    assert!(
        state.can_apply_renames(),
        "load_report must reset stale pending debounce state"
    );

    // 4. Rename completion banner survives post-action rescan
    let audit_ok = tempdir().unwrap().path().join("ok_apply.json");
    state.start_apply_renames(audit_ok);
    let deadline3 = Instant::now() + Duration::from_secs(5);
    while (state.is_busy || state.status_banner.is_none()) && Instant::now() < deadline3 {
        state.poll_worker_events();
        std::thread::sleep(Duration::from_millis(10));
    }
    let deadline4 = Instant::now() + Duration::from_secs(5);
    while state.is_busy && Instant::now() < deadline4 {
        state.poll_worker_events();
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        state
            .status_banner
            .as_deref()
            .is_some_and(|s| s.contains("완료") || s.contains("Renamed")),
        "completion banner must survive post-rename rescan: {:?}",
        state.status_banner
    );

    // 5. set_language updates report.sort_discrepancies so set_active_profile retains the new language
    state.set_language(UiLanguage::En);
    state.set_active_profile(CollationProfile::LinuxCodePoint);
    if let Some(d) = state.visible_discrepancies.first() {
        assert!(
            !d.reason.contains("정렬") && !d.reason.contains("인코딩"),
            "profile switch after set_language(En) must retain English reason: {}",
            d.reason
        );
    }
}

#[test]
fn state_phase11_banner_counts_sanitization_audit_guard_and_relocalization() {
    use std::path::PathBuf;

    // 1. TOCTOU BlockedCollision yielding 0 recoverable ops must NOT overwrite last_audit_log_path,
    // must report blocked count alongside "완료"/"Renamed", and must re-localize on set_language()
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("⽊_a.txt"), b"a").unwrap();
    fs::write(dir.path().join("⽊_b.txt"), b"b").unwrap();

    let report = scan_target(&ScanTarget::new(dir.path().to_path_buf())).unwrap();
    let mut state = NameCheckerAppState::new(UiLanguage::Ko);
    state.target_dir = Some(dir.path().to_path_buf());
    state.load_report(report);

    let prev_audit = PathBuf::from("/tmp/previous_valid_audit.json");
    state.last_audit_log_path = Some(prev_audit.clone());

    // Simulate external/concurrent collision creation on disk after scan
    fs::write(dir.path().join("木_a.txt"), b"coll_a").unwrap();
    fs::write(dir.path().join("木_b.txt"), b"coll_b").unwrap();

    let new_audit = tempdir().unwrap().path().join("zero_recoverable.json");
    state.start_apply_renames(new_audit);
    let deadline = Instant::now() + Duration::from_secs(5);
    while (state.is_busy || state.status_banner.is_none()) && Instant::now() < deadline {
        state.poll_worker_events();
        std::thread::sleep(Duration::from_millis(10));
    }
    let deadline2 = Instant::now() + Duration::from_secs(5);
    while state.is_busy && Instant::now() < deadline2 {
        state.poll_worker_events();
        std::thread::sleep(Duration::from_millis(10));
    }

    assert_eq!(
        state.last_audit_log_path.as_ref(),
        Some(&prev_audit),
        "0-recoverable batch must NOT clobber prior undoable last_audit_log_path"
    );
    let banner_ko = state.status_banner.clone().expect("expected status banner");
    assert!(
        banner_ko.contains("완료") && banner_ko.contains("차단"),
        "Ko banner must preserve 완료 and surface blocked count: {banner_ko}"
    );

    state.set_language(UiLanguage::En);
    let banner_en = state
        .status_banner
        .clone()
        .expect("expected English banner");
    assert!(
        banner_en.contains("Renamed") && banner_en.to_lowercase().contains("blocked"),
        "set_language(En) must dynamically re-localize active Rename banner: {banner_en}"
    );

    // 2. UndoFinished failure on removed file surfaces localized failure count and re-localizes
    let undo_dir = tempdir().unwrap();
    let undo_audit_dir = tempdir().unwrap();
    let src_path = undo_dir.path().join("⽊_u.txt");
    fs::write(&src_path, b"u").unwrap();
    let u_report = scan_target(&ScanTarget::new(undo_dir.path().to_path_buf())).unwrap();
    let mut u_state = NameCheckerAppState::new(UiLanguage::En);
    u_state.target_dir = Some(undo_dir.path().to_path_buf());
    u_state.load_report(u_report);
    let u_audit = undo_audit_dir.path().join("u_audit.json");
    u_state.start_apply_renames(u_audit.clone());
    let d1 = Instant::now() + Duration::from_secs(5);
    while u_state.is_busy && Instant::now() < d1 {
        u_state.poll_worker_events();
        std::thread::sleep(Duration::from_millis(10));
    }
    let d2 = Instant::now() + Duration::from_secs(5);
    while u_state.is_busy && Instant::now() < d2 {
        u_state.poll_worker_events();
        std::thread::sleep(Duration::from_millis(10));
    }
    // Remove renamed target so Undo fails on disk
    fs::remove_file(undo_dir.path().join("木_u.txt")).unwrap();
    u_state.start_undo();
    let d3 = Instant::now() + Duration::from_secs(5);
    while (u_state.is_busy
        || u_state
            .status_banner
            .as_deref()
            .is_some_and(|s| s.contains("Renamed")))
        && Instant::now() < d3
    {
        u_state.poll_worker_events();
        std::thread::sleep(Duration::from_millis(10));
    }
    let d4 = Instant::now() + Duration::from_secs(5);
    while u_state.is_busy && Instant::now() < d4 {
        u_state.poll_worker_events();
        std::thread::sleep(Duration::from_millis(10));
    }
    let undo_banner_en = u_state.status_banner.clone().unwrap();
    assert!(
        undo_banner_en.contains("Restored") && undo_banner_en.to_lowercase().contains("failed"),
        "Undo banner must surface failure count alongside Restored: {undo_banner_en}"
    );
    u_state.set_language(UiLanguage::Ko);
    let undo_banner_ko = u_state.status_banner.clone().unwrap();
    assert!(
        undo_banner_ko.contains("복구") && undo_banner_ko.contains("실패"),
        "set_language(Ko) must re-localize Undo failure banner: {undo_banner_ko}"
    );

    // 3. summarize_rename_operations escapes ANSI/Bidi error_detail, caps samples <= 2 & <= 120 chars,
    // and surfaces FilesystemForcedNfd warnings
    use namechecker_core::model::{EntryType, RawOsName, RenameOperation, RenameStatus};
    let make_op = |name: &str, status: RenameStatus, detail: Option<&str>| RenameOperation {
        timestamp_utc: "2026-11-23T00:00:00Z".to_string(),
        parent_dir: undo_dir.path().to_path_buf(),
        old_name: name.to_string(),
        old_raw_os_name: RawOsName::UnixBytes(name.as_bytes().to_vec()),
        file_id: None,
        entry_type: EntryType::File,
        byte_size: Some(1),
        mtime_ns: Some(1),
        temp_name: None,
        new_name: "target.txt".to_string(),
        status,
        error_detail: detail.map(str::to_string),
    };
    let long_dirty = format!("err \x1b[31m\u{202E}{}", "x".repeat(200));
    let ops = vec![
        make_op(
            "forced.txt",
            RenameStatus::FilesystemForcedNfd,
            Some("HFS+ NFD"),
        ),
        make_op("p1.txt", RenameStatus::PermissionDenied, Some(&long_dirty)),
        make_op("p2.txt", RenameStatus::BlockedCollision, Some("coll")),
        make_op("p3.txt", RenameStatus::IoError, Some("io err")),
    ];
    let summary_ko = NameCheckerAppState::summarize_rename_operations(&ops, false, UiLanguage::Ko);
    assert!(!summary_ko.contains('\x1b') && !summary_ko.contains('\u{202E}'));
    assert!(summary_ko.contains("<U+001B>") && summary_ko.contains("<U+202E>"));
    assert!(
        summary_ko.contains("NFD") && summary_ko.contains("차단") && summary_ko.contains("실패")
    );
    assert!(
        summary_ko.contains("(+2건 더 있음)"),
        "expected capped remaining count: {summary_ko}"
    );
    assert!(!summary_ko.contains("p2.txt") && !summary_ko.contains("p3.txt"));

    let summary_en = NameCheckerAppState::summarize_rename_operations(&ops, false, UiLanguage::En);
    assert!(
        summary_en.contains("(+2 more)"),
        "expected En remaining count: {summary_en}"
    );
}

#[test]
fn state_phase12_static_error_banners_clear_stale_operation_banner_state() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("⽊_cleanme.txt"), b"x").unwrap();
    fs::write(dir.path().join("노트〜초안.txt"), b"wave").unwrap();

    let report = scan_target(&ScanTarget::new(dir.path().to_path_buf())).unwrap();
    let mut state = NameCheckerAppState::new(UiLanguage::Ko);
    state.target_dir = Some(dir.path().to_path_buf());
    state.load_report(report);

    let audit_dir = tempdir().unwrap();
    let audit_path = audit_dir.path().join("ok.json");
    state.start_apply_renames(audit_path);
    let d1 = Instant::now() + Duration::from_secs(5);
    while (state.is_busy || state.status_banner.is_none()) && Instant::now() < d1 {
        state.poll_worker_events();
        std::thread::sleep(Duration::from_millis(10));
    }
    let d2 = Instant::now() + Duration::from_secs(5);
    while state.is_busy && Instant::now() < d2 {
        state.poll_worker_events();
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        state
            .status_banner
            .as_deref()
            .is_some_and(|s| s.contains("완료"))
    );

    // 1. Trigger request_external_undo error after a completed rename, then switch language
    state.request_external_undo(audit_dir.path().join("does_not_exist.json"));
    let err_banner = state.status_banner.clone().unwrap();
    assert!(!err_banner.contains("완료"));

    state.set_language(UiLanguage::En);
    let after_lang = state.status_banner.clone().unwrap();
    assert!(
        !after_lang.contains("Renamed"),
        "set_language must NOT overwrite active error banner with stale operation banner: {after_lang}"
    );
    assert_eq!(after_lang, err_banner);

    // 2. Re-populate banner_state via 0-recoverable rename, trigger start_undo() missing log, then switch language
    fs::write(dir.path().join("⽊_coll.txt"), b"a").unwrap();
    fs::write(dir.path().join("木_coll.txt"), b"b").unwrap();
    let rep2 = scan_target(&ScanTarget::new(dir.path().to_path_buf())).unwrap();
    state.load_report(rep2);
    state.last_audit_log_path = None;
    let manual_path = dir.path().join("노트〜초안.txt");

    // Corrupted ignore.json after fresh rename
    let ignore_dir = tempdir().unwrap();
    let ignore_path = ignore_dir.path().join("ignore.json");
    fs::write(&ignore_path, "{invalid json").unwrap();
    state.set_ignore_list_path(Some(ignore_path));

    let dir3 = tempdir().unwrap();
    fs::write(dir3.path().join("⽊_two.txt"), b"x").unwrap();
    fs::write(dir3.path().join("노트〜초안.txt"), b"w").unwrap();
    let rep3 = scan_target(&ScanTarget::new(dir3.path().to_path_buf())).unwrap();
    state.target_dir = Some(dir3.path().to_path_buf());
    state.load_report(rep3);
    state.start_apply_renames(audit_dir.path().join("ok2.json"));
    let d3 = Instant::now() + Duration::from_secs(5);
    while state.is_busy && Instant::now() < d3 {
        state.poll_worker_events();
        std::thread::sleep(Duration::from_millis(10));
    }
    let d4 = Instant::now() + Duration::from_secs(5);
    while state.is_busy && Instant::now() < d4 {
        state.poll_worker_events();
        std::thread::sleep(Duration::from_millis(10));
    }
    let manual3 = dir3.path().join("노트〜초안.txt");
    let _ = manual_path;
    state.toggle_row_ignored(&manual3, true);
    let ig_err = state.status_banner.clone().unwrap();
    assert!(!ig_err.contains("완료") && !ig_err.contains("Renamed"));
    state.set_language(UiLanguage::Ko);
    assert_eq!(state.status_banner.as_deref(), Some(ig_err.as_str()));

    // 3. start_undo() missing-log banner after 0-recoverable rename stays cleared across set_language
    fs::remove_file(dir3.path().join("노트〜초안.txt")).unwrap();
    state.last_audit_log_path = None;
    fs::write(dir3.path().join("⽊_z.txt"), b"z").unwrap();
    let rep4 = scan_target(&ScanTarget::new(dir3.path().to_path_buf())).unwrap();
    state.load_report(rep4);
    fs::write(dir3.path().join("木_z.txt"), b"zcoll").unwrap();
    state.start_apply_renames(audit_dir.path().join("zero_rec.json"));
    let d5 = Instant::now() + Duration::from_secs(5);
    while state.is_busy && Instant::now() < d5 {
        state.poll_worker_events();
        std::thread::sleep(Duration::from_millis(10));
    }
    let d6 = Instant::now() + Duration::from_secs(5);
    while state.is_busy && Instant::now() < d6 {
        state.poll_worker_events();
        std::thread::sleep(Duration::from_millis(10));
    }
    state.start_undo();
    assert!(
        state
            .status_banner
            .as_deref()
            .is_some_and(|s| s.contains("로그가 없습니다"))
    );
    state.set_language(UiLanguage::En);
    assert!(
        !state
            .status_banner
            .as_deref()
            .unwrap_or_default()
            .contains("Renamed"),
        "start_undo missing-log banner must not revert to stale operation banner on set_language: {:?}",
        state.status_banner
    );
}

#[test]
fn state_phase13_modal_concurrency_and_recoverable_undo_count() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("⽊.txt"), b"k").unwrap();
    let report = scan_target(&ScanTarget::new(dir.path().to_path_buf())).unwrap();

    // 1. start_apply_renames() unconditionally clears show_rename_confirm even when !can_apply_renames()
    let mut state = NameCheckerAppState::new(UiLanguage::En);
    state.show_rename_confirm = true;
    state.start_apply_renames(dir.path().join("unused.json"));
    assert!(
        !state.show_rename_confirm,
        "start_apply_renames must clear show_rename_confirm even when !can_apply_renames()"
    );
    assert!(!state.is_busy);

    // 2. Build audit logs with pure/mixed FilesystemForcedNfd, Applied, and non-recoverable ops
    let audit_dir = tempdir().unwrap();
    let escaped_dir = dir
        .path()
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    let make_op_json = |old_name: &str, new_name: &str, status: &str| {
        format!(
            r#"{{
      "timestamp_utc": "2026-11-24T00:00:00Z",
      "parent_dir": "{escaped_dir}",
      "old_name": "{old_name}",
      "old_raw_os_name": {{ "UnixBytes": [97, 46, 116, 120, 116] }},
      "file_id": "1:2",
      "entry_type": "File",
      "byte_size": 1,
      "mtime_ns": 1,
      "temp_name": null,
      "new_name": "{new_name}",
      "status": "{status}",
      "error_detail": null
    }}"#
        )
    };

    let mixed_json = format!(
        r#"{{
  "schema_version": "1.0.0",
  "operations": [
    {op1},
    {op2},
    {op3},
    {op4}
  ]
}}"#,
        op1 = make_op_json("⽊_1.txt", "木_1.txt", "Applied"),
        op2 = make_op_json("blocked.txt", "existing.txt", "BlockedCollision"),
        op3 = make_op_json("\u{1112}\u{1161}.txt", "하.txt", "FilesystemForcedNfd"),
        op4 = make_op_json(
            "\u{1112}\u{1161}_⽊.txt",
            "하_木.txt",
            "FilesystemForcedNfd"
        ),
    );
    let mixed_log_path = audit_dir.path().join("mixed_undo.json");
    fs::write(&mixed_log_path, mixed_json).unwrap();

    // Staging external undo clears show_rename_confirm and counts ONLY recoverable ops (2)
    state.show_rename_confirm = true;
    state.request_external_undo(mixed_log_path.clone());
    assert!(
        !state.show_rename_confirm,
        "request_external_undo must enforce mutual exclusion by closing Confirm Rename modal"
    );
    assert_eq!(state.pending_undo_path.as_ref(), Some(&mixed_log_path));
    assert_eq!(
        state.pending_undo_count, 2,
        "pending_undo_count must exclude BlockedCollision and halted pure NFD->NFC ops"
    );

    // load_report clears both modals
    state.show_rename_confirm = true;
    state.load_report(report);
    assert!(!state.show_rename_confirm);
    assert!(state.pending_undo_path.is_none());
    assert_eq!(state.pending_undo_count, 0);

    // start_scan clears both modals when idle
    state.show_rename_confirm = true;
    state.pending_undo_path = Some(mixed_log_path.clone());
    state.pending_undo_count = 2;
    state.start_scan(dir.path().to_path_buf());
    assert!(!state.show_rename_confirm);
    assert!(state.pending_undo_path.is_none());
    assert_eq!(state.pending_undo_count, 0);
    let d1 = Instant::now() + Duration::from_secs(5);
    while state.is_busy && Instant::now() < d1 {
        state.poll_worker_events();
        std::thread::sleep(Duration::from_millis(10));
    }

    // 3. When is_busy == true, start_scan / start_undo / start_undo_from_path / confirm_external_undo refuse to spawn workers
    state.is_busy = true;
    state.target_dir = Some(dir.path().to_path_buf());
    state.last_audit_log_path = Some(mixed_log_path.clone());
    state.pending_undo_path = Some(mixed_log_path.clone());
    state.pending_undo_count = 2;

    let other_dir = tempdir().unwrap();
    state.start_scan(other_dir.path().to_path_buf());
    assert_eq!(
        state.target_dir.as_deref(),
        Some(dir.path()),
        "start_scan must be a no-op when is_busy == true"
    );
    state.start_undo();
    state.start_undo_from_path(mixed_log_path.clone());
    state.confirm_external_undo();
    assert_eq!(
        state.pending_undo_path.as_ref(),
        Some(&mixed_log_path),
        "confirm_external_undo must preserve staged path and refuse to run when is_busy == true"
    );
    assert_eq!(state.pending_undo_count, 2);
    std::thread::sleep(Duration::from_millis(50));
    assert!(
        !state.poll_worker_events(),
        "no background worker events may be emitted when busy guards block actions"
    );
    state.is_busy = false;

    // 4. Audit log with 0 recoverable ops shows localized banner and does NOT open modal
    let zero_json = format!(
        r#"{{
  "schema_version": "1.0.0",
  "operations": [
    {op1},
    {op2}
  ]
}}"#,
        op1 = make_op_json("blocked.txt", "existing.txt", "BlockedCollision"),
        op2 = make_op_json("err.txt", "err2.txt", "IoError"),
    );
    let zero_log_path = audit_dir.path().join("zero_recoverable.json");
    fs::write(&zero_log_path, zero_json).unwrap();
    state.pending_undo_path = None;
    state.pending_undo_count = 0;
    state.request_external_undo(zero_log_path);
    assert!(
        state.pending_undo_path.is_none(),
        "zero-recoverable audit log must NOT open Confirm Undo modal"
    );
    assert_eq!(state.pending_undo_count, 0);
    assert!(
        state.status_banner.is_some(),
        "zero-recoverable audit log must display localized status banner"
    );
}
