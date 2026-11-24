use std::fs;

use namechecker_core::ignore_list::{
    add_ignore_rule, load_ignore_list, save_ignore_list, should_suppress_finding,
};
use namechecker_core::model::{
    EntryFinding, EntryType, IgnoreList, IssueCategory, RawOsName, RemediationTrack, ScanTarget,
    Severity,
};
use namechecker_core::scanner::scan_target;
use tempfile::tempdir;

#[test]
fn ignore_list_persists_portably_and_matches_nfc_and_globs() {
    let dir = tempdir().unwrap();
    let ignore_path = dir.path().join("ignore.json");

    let mut list = load_ignore_list(&ignore_path).unwrap();
    assert!(list.rules.is_empty());

    // Host-mounted paths collapse to portable single-component rules
    let rule = add_ignore_rule(
        &mut list,
        "/Volumes/SynologyNAS/Music/가나다〜노래.txt",
        Some(IssueCategory::PunctuationVariant),
        Some("intentional wave dash".to_string()),
    )
    .unwrap();
    assert_eq!(rule.pattern, "가나다〜노래.txt");
    assert!(!rule.is_glob);

    add_ignore_rule(&mut list, "Z:\\shared\\*.legacy", None, None).unwrap();
    save_ignore_list(&ignore_path, &list).unwrap();

    let reloaded = load_ignore_list(&ignore_path).unwrap();
    assert_eq!(reloaded.rules.len(), 2);

    // NFD raw_name must match NFC pattern rule
    let nfd_name = "\u{1100}\u{1161}\u{1102}\u{1161}\u{1103}\u{1161}\u{301C}\u{1102}\u{1169}\u{1105}\u{1162}.txt";
    let finding = EntryFinding {
        id: "f1".to_string(),
        path: dir.path().join(nfd_name),
        parent_dir: dir.path().to_path_buf(),
        raw_name: nfd_name.to_string(),
        raw_os_name: RawOsName::UnixBytes(nfd_name.as_bytes().to_vec()),
        entry_type: EntryType::File,
        track: RemediationTrack::ManualReview,
        category: IssueCategory::PunctuationVariant,
        severity: Severity::Warning,
        offending_chars: Vec::new(),
        suggested_name: None,
        candidate_hints: Vec::new(),
        message: String::new(),
    };
    assert!(should_suppress_finding(&reloaded, &finding));

    // AutoFixable and OrphanedTempFile are NEVER suppressed
    let mut orphaned = finding.clone();
    orphaned.raw_name = "foo.legacy".to_string();
    orphaned.category = IssueCategory::OrphanedTempFile;
    assert!(!should_suppress_finding(&reloaded, &orphaned));

    let mut auto_f = finding.clone();
    auto_f.raw_name = "foo.legacy".to_string();
    auto_f.track = RemediationTrack::AutoFixable;
    auto_f.category = IssueCategory::KangxiRadical;
    assert!(!should_suppress_finding(&reloaded, &auto_f));
}

#[test]
fn scanner_applies_ignore_list_before_rename_plan_composition() {
    let target_dir = tempdir().unwrap();
    let cfg_dir = tempdir().unwrap();
    let ignore_path = cfg_dir.path().join("ignore.json");

    // Multi-issue file: AutoFixable KangxiRadical '⽊' + ManualReview PunctuationVariant '〜'
    let combo = "⽊〜노래.txt";
    fs::write(target_dir.path().join(combo), b"x").unwrap();
    // OrphanedTempFile must never be ignored
    fs::write(target_dir.path().join(".__nc_tmp_deadbeef"), b"tmp").unwrap();

    let mut list = IgnoreList::default();
    add_ignore_rule(
        &mut list,
        combo,
        Some(IssueCategory::PunctuationVariant),
        None,
    )
    .unwrap();
    add_ignore_rule(&mut list, ".__nc_tmp_*", None, None).unwrap();
    save_ignore_list(&ignore_path, &list).unwrap();

    let mut target = ScanTarget::new(target_dir.path().to_path_buf());
    target.ignore_list_path = Some(ignore_path);
    let report = scan_target(&target).unwrap();

    // Only OrphanedTempFile remains as ManualReview; combo's ManualReview was suppressed before plan composition!
    assert_eq!(report.manual_review_count, 1);
    assert_eq!(report.auto_fixable_count, 1);

    let combo_plan = report
        .rename_plans
        .iter()
        .find(|p| p.raw_name == combo)
        .expect("missing composed plan for combo");
    assert_eq!(
        combo_plan.track,
        RemediationTrack::AutoFixable,
        "suppressing ManualReview before composition promotes combo plan to AutoFixable"
    );
    assert_eq!(
        combo_plan.composed_suggested_name.as_deref(),
        Some("木〜노래.txt")
    );
}

#[test]
fn ignore_rule_validates_glob_syntax_while_exact_name_preserves_brackets() {
    use namechecker_core::ignore_list::add_exact_ignore_rule;
    use namechecker_core::model::{CoreError, IgnoreRule};

    let bad = IgnoreRule::from_path_or_pattern("[unclosed", None, None);
    assert!(
        matches!(bad, Err(CoreError::InvalidTarget(_))),
        "malformed glob must be rejected with InvalidTarget: {bad:?}"
    );

    let exact = IgnoreRule::exact_name("[작가] 화보〜1.zip", None, None).unwrap();
    assert!(!exact.is_glob);
    assert_eq!(exact.pattern, "[작가] 화보〜1.zip");

    let mut list = IgnoreList::default();
    add_exact_ignore_rule(&mut list, "[미완성〜.zip", None, None).unwrap();
    assert_eq!(list.rules.len(), 1);
    assert!(!list.rules[0].is_glob);
}
