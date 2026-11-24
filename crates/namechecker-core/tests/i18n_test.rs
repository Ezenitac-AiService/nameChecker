//! Triangulated unit tests for Korean (default) and English i18n formatting (T005).

use namechecker_core::i18n::MessageCatalog;
use namechecker_core::model::{IssueCategory, UiLanguage};

#[test]
fn category_labels_cover_all_12_categories_in_ko_and_en() {
    for category in IssueCategory::all() {
        let ko = MessageCatalog::category_label(category, UiLanguage::Ko);
        let en = MessageCatalog::category_label(category, UiLanguage::En);
        assert!(
            !ko.is_empty(),
            "Korean label must not be empty for {category:?}"
        );
        assert!(
            !en.is_empty(),
            "English label must not be empty for {category:?}"
        );
        assert_ne!(
            ko, en,
            "Korean and English labels should differ for {category:?}"
        );
    }
}

#[test]
fn orphaned_temp_message_mentions_undo_recovery() {
    let ko = MessageCatalog::orphaned_temp_message(".__nc_tmp_1234", UiLanguage::Ko);
    assert!(ko.contains(".__nc_tmp_1234"));
    assert!(ko.contains("되돌리기") || ko.contains("undo"));

    let en = MessageCatalog::orphaned_temp_message(".__nc_tmp_1234", UiLanguage::En);
    assert!(en.contains(".__nc_tmp_1234"));
    assert!(en.contains("undo"));
}

#[test]
fn section_headings_handle_valid_and_unknown_keys() {
    let auto_ko = MessageCatalog::section_heading("auto_fixable", UiLanguage::Ko);
    let auto_en = MessageCatalog::section_heading("auto_fixable", UiLanguage::En);
    assert_eq!(auto_ko, "자동 수정 제안");
    assert_eq!(auto_en, "Auto-Fixable");

    let manual_ko = MessageCatalog::section_heading("manual_review", UiLanguage::Ko);
    let manual_en = MessageCatalog::section_heading("manual_review", UiLanguage::En);
    assert_eq!(manual_ko, "수동 확인 필요");
    assert_eq!(manual_en, "Manual Review Required");

    // Negative/unknown key returns stable fallback without panicking
    assert_eq!(
        MessageCatalog::section_heading("nonexistent_key", UiLanguage::En),
        "unknown"
    );
}
