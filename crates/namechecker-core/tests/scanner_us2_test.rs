use std::fs;

use namechecker_core::model::{DiscrepancyCause, ScanTarget};
use namechecker_core::scanner::scan_target;
use tempfile::tempdir;

#[test]
fn scanner_populates_before_after_orders_rank_deltas_collisions_and_causes() {
    let dir = tempdir().unwrap();

    // 1. NFD Hangul "하" (U+1112 U+1161) vs "나" + Kangxi radical "⽊" (U+2F4A) vs existing "木" (U+6728)
    let nfd_ha = "\u{1112}\u{1161}.txt";
    fs::write(dir.path().join(nfd_ha), b"nfd").unwrap();
    fs::write(dir.path().join("나.txt"), b"nfc").unwrap();
    fs::write(dir.path().join("⽊.txt"), b"kangxi").unwrap();
    fs::write(dir.path().join("木.txt"), b"unified").unwrap();
    fs::write(dir.path().join("A_clean.txt"), b"ascii").unwrap();
    fs::write(dir.path().join("가_clean.txt"), b"hangul").unwrap();

    let mut target = ScanTarget::new(dir.path().to_path_buf());
    target.max_depth = Some(1);
    let report = scan_target(&target).unwrap();

    assert_eq!(report.total_entries, 6);
    assert!(!report.before_order.is_empty());
    assert_eq!(
        report.predicted_after_order.len(),
        report.before_order.len()
    );

    // Baseline order is WindowsKoKr (Latin A_clean -> Hangul+Hanja)
    let a_rank = report
        .before_order
        .iter()
        .find(|e| e.name == "A_clean.txt")
        .unwrap()
        .rank;
    let ga_rank = report
        .before_order
        .iter()
        .find(|e| e.name == "가_clean.txt")
        .unwrap()
        .rank;
    assert!(a_rank < ga_rank);

    // Collision between ⽊.txt -> 木.txt and existing 木.txt must mark has_collision in predicted_after_order
    let kangxi_pred = report
        .predicted_after_order
        .iter()
        .find(|p| p.old_name == "⽊.txt")
        .expect("missing predicted entry for ⽊.txt");
    assert_eq!(kangxi_pred.new_name, "木.txt");
    assert!(kangxi_pred.changed);
    assert!(kangxi_pred.has_collision);

    // Rank delta formula verification: old_rank as isize - new_rank as isize
    for pred in &report.predicted_after_order {
        assert_eq!(
            pred.rank_delta,
            (pred.old_rank as isize) - (pred.new_rank as isize)
        );
    }

    assert!(report.sort_discrepancy_count > 0);
    assert_eq!(
        report.sort_discrepancy_count,
        report.sort_discrepancies.len()
    );
    assert!(report.collision_risk_count >= 1);
    assert_eq!(report.collision_risk_count, report.collision_risks.len());
    assert!(
        report
            .sort_discrepancies
            .iter()
            .any(|d| d.cause == DiscrepancyCause::EncodingAnomaly)
    );
}
