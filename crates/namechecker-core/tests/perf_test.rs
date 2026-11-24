use std::fs::File;
use std::time::{Duration, Instant};

use namechecker_core::model::ScanTarget;
use namechecker_core::scanner::scan_target;
use tempfile::tempdir;

#[test]
fn sc003_scans_and_predicts_sort_order_for_50000_entries_under_5_seconds() {
    let dir = tempdir().unwrap();

    for i in 0..50_000usize {
        let name = match i % 5 {
            0 => format!("문서_{i:05}.txt"),
            1 => format!("File_{i:05}.txt"),
            2 => format!("⽊_항목_{i:05}.txt"),
            3 => format!("ファイル_{i:05}.txt"),
            _ => format!("資料_{i:05}.txt"),
        };
        File::create(dir.path().join(name)).unwrap();
    }

    let target = ScanTarget::new(dir.path().to_path_buf());
    let start = Instant::now();
    let report = scan_target(&target).unwrap();
    let elapsed = start.elapsed();

    assert_eq!(report.total_entries, 50_000);
    assert_eq!(report.before_order.len(), 50_000);
    assert_eq!(report.predicted_after_order.len(), 50_000);
    assert_eq!(report.auto_fixable_count, 10_000);

    assert!(
        elapsed < Duration::from_secs(5),
        "SC-003 budget exceeded: 50k scan + 5-profile collation/collision took {:?}",
        elapsed
    );
    eprintln!("SC-003 50k scan + collation elapsed: {:?}", elapsed);
}
