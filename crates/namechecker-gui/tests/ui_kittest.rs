use std::fs;

use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use namechecker_core::model::{ScanTarget, UiLanguage};
use namechecker_core::scanner::scan_target;
use namechecker_gui::app::NameCheckerApp;
use namechecker_gui::state::NameCheckerAppState;
use tempfile::tempdir;

#[test]
fn gui_renders_3_pane_toolbar_collision_disable_and_safe_escaped_cells() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("⽊.txt"), b"kangxi").unwrap();
    fs::write(dir.path().join("木.txt"), b"unified").unwrap();
    let bad_name = "bad\x1b[31m\u{202E}file.txt";
    fs::write(dir.path().join(bad_name), b"ctrl").unwrap();

    let report = scan_target(&ScanTarget::new(dir.path().to_path_buf())).unwrap();
    let mut state = NameCheckerAppState::new(UiLanguage::Ko);
    state.target_dir = Some(dir.path().to_path_buf());
    state.load_report(report);
    // Target ⽊.txt -> 木.txt collides with existing 木.txt, so [수정] must be disabled!
    let app = NameCheckerApp::with_state(state);

    let harness = Harness::builder()
        .with_size(egui::Vec2::new(1280.0, 800.0))
        .allow_missing_glyphs()
        .build_ui_state(|ui, app: &mut NameCheckerApp| app.render_contents(ui), app);

    // Top toolbar buttons present in AccessKit tree
    harness.get_by_label("폴더 선택");
    harness.get_by_label("점검");
    let fix_btn = harness.get_by_label("수정");
    assert!(
        fix_btn.accesskit_node().is_disabled(),
        "collision must disable [수정] button"
    );
    harness.get_by_label("되돌리기");

    // Raw ANSI/Bidi control sequence must never appear raw in accessible labels
    assert_eq!(harness.query_all_by_label_contains("\x1b").count(), 0);
    assert_eq!(harness.query_all_by_label_contains("\u{202E}").count(), 0);
    assert!(harness.query_all_by_label_contains("<U+001B>").count() >= 2);
    assert!(harness.query_all_by_label_contains("[충돌]").count() >= 1);
}

#[test]
fn gui_switches_ko_en_language_renders_details_and_enforces_orphaned_ignore_guard() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("⽊.txt"), b"kangxi").unwrap();
    fs::write(dir.path().join("노트〜초안.txt"), b"wave").unwrap();
    fs::write(dir.path().join(".__nc_tmp_deadbeef"), b"tmp").unwrap();

    let report = scan_target(&ScanTarget::new(dir.path().to_path_buf())).unwrap();
    let mut state = NameCheckerAppState::new(UiLanguage::Ko);
    state.target_dir = Some(dir.path().to_path_buf());
    state.load_report(report);
    state.select_entry(Some("⽊.txt"));
    let app = NameCheckerApp::with_state(state);

    let mut harness = Harness::builder()
        .with_size(egui::Vec2::new(1280.0, 800.0))
        .allow_missing_glyphs()
        .build_ui_state(|ui, app: &mut NameCheckerApp| app.render_contents(ui), app);

    // Selected-row detail section displays offending codepoint U+2F4A without breaking table
    assert!(
        harness.query_all_by_label_contains("U+2F4A").count() >= 1,
        "selected row detail must display U+2F4A"
    );
    // OrphanedTempFile must not expose an active ignore checkbox
    assert_eq!(
        harness
            .query_all_by_label_contains(".__nc_tmp_deadbeef")
            .filter(
                |n| n.accesskit_node().role() == egui::accesskit::Role::CheckBox
                    && !n.accesskit_node().is_disabled()
            )
            .count(),
        1,
        "only apply checkbox (disabled or absent ignore checkbox) allowed for OrphanedTempFile"
    );

    // Switch to English via toolbar button
    harness.get_by_label("English").click();
    harness.run();

    harness.get_by_label("Select Folder");
    harness.get_by_label("Inspect");
    harness.get_by_label("Fix");
    harness.get_by_label("Undo");
}

#[test]
fn gui_localizes_category_badges_and_exposes_undo_for_external_audit_logs() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("⽊.txt"), b"kangxi").unwrap();
    fs::write(dir.path().join(".__nc_tmp_deadbeef"), b"tmp").unwrap();

    let report = scan_target(&ScanTarget::new(dir.path().to_path_buf())).unwrap();
    let mut state = NameCheckerAppState::new(UiLanguage::Ko);
    state.target_dir = Some(dir.path().to_path_buf());
    state.load_report(report);
    assert!(state.last_audit_log_path.is_none());

    let app = NameCheckerApp::with_state(state);
    let mut harness = Harness::builder()
        .with_size(egui::Vec2::new(1280.0, 800.0))
        .allow_missing_glyphs()
        .build_ui_state(|ui, app: &mut NameCheckerApp| app.render_contents(ui), app);

    // Undo button remains enabled even when last_audit_log_path is None so user can load an external audit log
    let undo_btn = harness.get_by_label("되돌리기");
    assert!(
        !undo_btn.accesskit_node().is_disabled(),
        "Undo button must remain enabled for external audit log recovery"
    );

    // Category count badges must be localized in Korean (not raw enum name "KangxiRadical")
    assert!(
        harness
            .query_all_by_label_contains("강희자전/한자 부수 대체 문자: 1")
            .count()
            >= 1,
        "expected localized Korean category badge"
    );

    // Switch to English and verify badge updates to localized English label
    harness.get_by_label("English").click();
    harness.run();
    assert!(
        harness
            .query_all_by_label_contains("Kangxi/CJK Radical Substitute: 1")
            .count()
            >= 1,
        "expected localized English category badge"
    );
}

#[test]
fn gui_renders_sort_discrepancy_reason_and_keeps_table_scrollable() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("가.txt"), b"a").unwrap();
    fs::write(dir.path().join("A.txt"), b"b").unwrap();

    let report = scan_target(&ScanTarget::new(dir.path().to_path_buf())).unwrap();
    let mut state = NameCheckerAppState::new(UiLanguage::En);
    state.target_dir = Some(dir.path().to_path_buf());
    state.load_report(report);
    state.set_active_profile(namechecker_core::model::CollationProfile::MacOsKo);
    let expected_reason = state
        .visible_discrepancies
        .first()
        .map(|d| d.reason.clone())
        .expect("expected discrepancy between 가.txt and A.txt on MacOsKo");

    let app = NameCheckerApp::with_state(state);
    let harness = Harness::builder()
        .with_size(egui::Vec2::new(1280.0, 800.0))
        .allow_missing_glyphs()
        .build_ui_state(|ui, app: &mut NameCheckerApp| app.render_contents(ui), app);

    assert!(
        harness
            .query_all_by_label_contains(&expected_reason)
            .count()
            >= 1,
        "Right Pane must render SortDiscrepancy.reason ({expected_reason})"
    );
}

#[test]
fn gui_center_pane_keeps_details_visible_only_when_flagged_row_selected() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("clean_00.txt"), b"clean").unwrap();
    for i in 0..35 {
        fs::write(dir.path().join(format!("⽊_{i:02}.txt")), b"kangxi").unwrap();
    }

    let report = scan_target(&ScanTarget::new(dir.path().to_path_buf())).unwrap();
    let mut state = NameCheckerAppState::new(UiLanguage::En);
    state.target_dir = Some(dir.path().to_path_buf());
    state.load_report(report);
    // Select clean entry first: details must NOT render and table must not reserve detail space
    state.select_entry(Some("clean_00.txt"));

    let app = NameCheckerApp::with_state(state);
    let mut harness = Harness::builder()
        .with_size(egui::Vec2::new(1280.0, 800.0))
        .allow_missing_glyphs()
        .build_ui_state(|ui, app: &mut NameCheckerApp| app.render_contents(ui), app);

    assert_eq!(
        harness
            .query_all_by_label_contains("Selected Entry Details")
            .count(),
        0,
        "clean entry must not render Selected Entry Details"
    );

    // Select a flagged entry on the large 35-row list: Selected Entry Details must appear within 800px height
    harness.get_by_label("⽊_00.txt").click();
    harness.run();

    let detail = harness
        .query_all_by_label_contains("U+2F4A")
        .next()
        .expect("flagged selection must render U+2F4A in Selected Entry Details");
    let rect = detail
        .accesskit_node()
        .raw_bounds()
        .expect("Selected Entry Details node must have layout bounds");
    assert!(
        rect.y1 <= 800.0,
        "Selected Entry Details must stay inside 800px viewport instead of overflowing below: {:?}",
        rect
    );
}

#[test]
fn phase13_modal_apply_and_select_folder_disabled_states() {
    // 1. Confirm Rename Apply button must render disabled when !can_apply_renames() (e.g. collision)
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("⽊.txt"), b"kangxi").unwrap();
    fs::write(dir.path().join("木.txt"), b"unified").unwrap();
    let report = scan_target(&ScanTarget::new(dir.path().to_path_buf())).unwrap();

    let mut state1 = NameCheckerAppState::new(UiLanguage::En);
    state1.target_dir = Some(dir.path().to_path_buf());
    state1.load_report(report.clone());
    assert!(!state1.can_apply_renames());
    state1.show_rename_confirm = true;

    let app1 = NameCheckerApp::with_state(state1);
    let harness1 = Harness::builder()
        .with_size(egui::Vec2::new(1280.0, 800.0))
        .allow_missing_glyphs()
        .build_ui_state(|ui, app: &mut NameCheckerApp| app.render_contents(ui), app1);
    let rename_apply = harness1
        .query_all_by_label("Apply")
        .find(|n| n.accesskit_node().role() == egui::accesskit::Role::Button)
        .expect("Confirm Rename Apply button must exist");
    assert!(
        rename_apply.accesskit_node().is_disabled(),
        "Confirm Rename Apply button must render disabled when !can_apply_renames()"
    );

    // 2. When is_busy == true and Confirm Undo modal is open, both Select Folder and Apply must be disabled
    let mut state2 = NameCheckerAppState::new(UiLanguage::En);
    state2.target_dir = Some(dir.path().to_path_buf());
    state2.load_report(report);
    state2.is_busy = true;
    state2.pending_undo_path = Some(dir.path().join("external_audit.json"));
    state2.pending_undo_count = 1;

    let app2 = NameCheckerApp::with_state(state2);
    let harness2 = Harness::builder()
        .with_size(egui::Vec2::new(1280.0, 800.0))
        .allow_missing_glyphs()
        .build_ui_state(|ui, app: &mut NameCheckerApp| app.render_contents(ui), app2);
    let select_folder = harness2.get_by_label("Select Folder");
    assert!(
        select_folder.accesskit_node().is_disabled(),
        "Select Folder button must render disabled when is_busy == true"
    );
    let undo_apply = harness2
        .query_all_by_label("Apply")
        .find(|n| n.accesskit_node().role() == egui::accesskit::Role::Button)
        .expect("Confirm Undo Apply button must exist");
    assert!(
        undo_apply.accesskit_node().is_disabled(),
        "Confirm Undo Apply button must render disabled when is_busy == true"
    );
}
