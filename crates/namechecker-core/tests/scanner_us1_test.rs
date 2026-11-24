//! Triangulated integration tests for single-folder US1 scanner (max_depth=1, symlinks, empty, missing root) (T012).

use std::fs;
use tempfile::TempDir;

use namechecker_core::model::{CoreError, IssueCategory, RemediationTrack, ScanTarget};
use namechecker_core::scanner::scan_target;
use namechecker_core::unicode_data::normalize_nfd;

#[test]
fn scans_immediate_entries_only_without_recursing_into_subdirectories_in_v1() {
    let dir = TempDir::new().expect("tempdir");
    let root = dir.path();

    // Immediate anomalous file & immediate child folder with NFD
    let top_file = root.join(format!("{}.txt", normalize_nfd("가나다")));
    let child_dir = root.join("하위폴더_\u{2F47}");
    fs::write(&top_file, b"top").expect("write top file");
    fs::create_dir(&child_dir).expect("create child dir");

    // Deep nested file inside child_dir MUST NOT be inspected under v1 max_depth = 1
    let deep_file = child_dir.join("deep_\u{200B}hidden.txt");
    fs::write(&deep_file, b"deep").expect("write deep file");

    let target = ScanTarget::new(root.to_path_buf());
    let report = scan_target(&target).expect("scan succeeds");

    assert_eq!(report.total_entries, 2);
    assert_eq!(report.auto_fixable_count, 2);
    assert!(
        report
            .findings
            .iter()
            .all(|f| !f.raw_name.contains("hidden")),
        "nested grandchild files must be excluded in v1 max_depth = 1"
    );
    assert!(
        report
            .findings
            .iter()
            .any(|f| f.category == IssueCategory::KangxiRadical
                && f.track == RemediationTrack::AutoFixable)
    );
}

#[cfg(unix)]
#[test]
fn skips_symlink_target_traversal_by_default() {
    use std::os::unix::fs::symlink;

    let dir = TempDir::new().expect("tempdir");
    let external = TempDir::new().expect("external");
    fs::write(external.path().join("external_\u{200B}.txt"), b"x").expect("write ext");

    let link_path = dir.path().join("symlink_dir");
    symlink(external.path(), &link_path).expect("symlink");

    let target = ScanTarget::new(dir.path().to_path_buf());
    let report = scan_target(&target).expect("scan succeeds");
    assert_eq!(report.total_entries, 1);
    assert!(
        report
            .findings
            .iter()
            .all(|f| !f.raw_name.contains("external"))
    );
}

#[test]
fn empty_and_missing_root_behave_cleanly() {
    let dir = TempDir::new().expect("tempdir");
    let empty_target = ScanTarget::new(dir.path().to_path_buf());
    let empty_report = scan_target(&empty_target).expect("empty scan succeeds");
    assert_eq!(empty_report.total_entries, 0);
    assert_eq!(empty_report.auto_fixable_count, 0);
    assert_eq!(empty_report.manual_review_count, 0);
    assert!(empty_report.findings.is_empty());

    // Negative: nonexistent root returns InvalidTarget error
    let missing_target = ScanTarget::new(dir.path().join("nonexistent-subdir"));
    assert!(matches!(
        scan_target(&missing_target),
        Err(CoreError::InvalidTarget(_))
    ));
}

#[cfg(unix)]
#[test]
fn follow_symlinks_breaks_directory_cycles_safely() {
    use std::os::unix::fs::symlink;

    let dir = TempDir::new().expect("tempdir");
    let sub = dir.path().join("sub");
    fs::create_dir(&sub).expect("sub");
    fs::write(sub.join("nested_\u{200B}.txt"), b"x").expect("write");
    symlink(dir.path(), sub.join("loop")).expect("symlink cycle");

    let mut target = ScanTarget::new(dir.path().to_path_buf());
    target.follow_symlinks = true;
    target.max_depth = Some(3);

    let report = scan_target(&target).expect("cycle-safe scan completes");
    assert!(
        report
            .findings
            .iter()
            .any(|f| f.raw_name.contains("nested_")),
        "expected nested finding under symlinked traversal"
    );
    assert_eq!(
        report.total_entries, 3,
        "cycle must terminate cleanly without re-entering visited dirs"
    );
}

#[test]
fn follow_symlinks_records_io_error_on_symlink_loop_metadata_failure() {
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        let dir = TempDir::new().expect("tempdir");
        symlink(dir.path().join("self_loop"), dir.path().join("self_loop")).expect("self loop");

        let mut target = ScanTarget::new(dir.path().to_path_buf());
        target.follow_symlinks = true;
        target.max_depth = Some(2);

        let report = scan_target(&target).expect("scan completes with io_errors");
        assert!(
            !report.io_errors.is_empty(),
            "expected IoErrorRecord on symlink metadata ELOOP error"
        );
    }
}

#[test]
fn logical_report_paths_never_leak_win32_verbatim_prefix() {
    let dir = TempDir::new().expect("tempdir");
    fs::write(dir.path().join("⽊_scan.txt"), b"x").expect("write");
    let target = ScanTarget::new(dir.path().to_path_buf());
    let report = scan_target(&target).expect("scan succeeds");
    assert!(
        report
            .findings
            .iter()
            .all(|f| !f.path.to_string_lossy().starts_with(r"\\?\"))
            && report
                .rename_plans
                .iter()
                .all(|p| !p.path.to_string_lossy().starts_with(r"\\?\")),
        "scan report paths must remain logical user-facing paths"
    );
}
