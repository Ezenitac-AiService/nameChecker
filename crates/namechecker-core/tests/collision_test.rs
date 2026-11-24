use std::path::Path;

use namechecker_core::collision::detect_collisions;
use namechecker_core::model::CollisionType;

#[test]
fn detects_nfd_nfc_compatibility_and_case_collisions() {
    let dir = Path::new("/tmp/collision_test");
    let nfd_hangul = "\u{1100}\u{1161}\u{11AB}.txt"; // 간 in NFD
    let nfc_hangul = "간.txt";

    let entries = [
        nfd_hangul,
        nfc_hangul,
        "車.txt", // U+F902 compatibility ideograph
        "車.txt", // U+8ECA unified ideograph
        "Report.txt",
        "report.txt",
        "REPORT.TXT",
    ];

    let risks = detect_collisions(dir, &entries);
    assert_eq!(
        risks.len(),
        3,
        "expected 3 distinct collision groups: {risks:?}"
    );

    let nfd_risk = risks
        .iter()
        .find(|r| r.collision_type == CollisionType::NormalizationNfcNfd)
        .expect("missing NormalizationNfcNfd risk");
    assert_eq!(nfd_risk.entries.len(), 2);
    assert!(nfd_risk.entries.contains(&nfd_hangul.to_string()));
    assert!(nfd_risk.entries.contains(&nfc_hangul.to_string()));

    let compat_risk = risks
        .iter()
        .find(|r| r.collision_type == CollisionType::CanonicalCompatibility)
        .expect("missing CanonicalCompatibility risk");
    assert_eq!(compat_risk.entries.len(), 2);
    assert!(compat_risk.entries.contains(&"車.txt".to_string()));
    assert!(compat_risk.entries.contains(&"車.txt".to_string()));

    let case_risk = risks
        .iter()
        .find(|r| r.collision_type == CollisionType::CaseInsensitive)
        .expect("missing CaseInsensitive risk");
    assert_eq!(case_risk.entries.len(), 3);
    assert!(!case_risk.affected_platforms.is_empty());
}

#[test]
fn handles_unicode_case_folding_boundary_without_false_positive_on_distinct_names() {
    let dir = Path::new("/tmp/collision_boundary");
    // Distinct names including clean Unicode punctuation / distinct Hangul syllables must NOT collide
    let entries = [
        "가.txt",
        "각.txt",
        "01.txt",
        "1.txt",
        "東京.txt",
        "京都.txt",
    ];
    let risks = detect_collisions(dir, &entries);
    assert!(
        risks.is_empty(),
        "distinct filenames must never produce collision risks: {risks:?}"
    );
}
