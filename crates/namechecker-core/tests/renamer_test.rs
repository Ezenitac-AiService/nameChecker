use std::fs;
use std::path::Path;

use namechecker_core::model::{
    CoreError, EntryType, RawOsName, RenameAuditLog, RenameOperation, RenameStatus, SCHEMA_VERSION,
    ScanTarget,
};
use namechecker_core::renamer::{
    SafeRenameItem, apply_safe_renames, capture_file_id_and_metadata, to_win32_verbatim_path,
    undo_audit_log,
};
use namechecker_core::scanner::{os_str_to_raw, scan_target};
use tempfile::tempdir;

#[test]
fn formats_win32_verbatim_local_and_unc_paths() {
    assert_eq!(
        to_win32_verbatim_path(Path::new(r"C:\folder\trailing ")),
        r"\\?\C:\folder\trailing "
    );
    assert_eq!(
        to_win32_verbatim_path(Path::new(r"\\nas\share\한글")),
        r"\\?\UNC\nas\share\한글"
    );
}

#[test]
fn applies_safe_renames_blocks_collisions_and_overflows_and_supports_two_step_nfd_and_undo() {
    let dir = tempdir().unwrap();
    let audit_dir = tempdir().unwrap();
    let audit_path = audit_dir.path().join("audit.json");

    // 1. Safe non-overwriting rename: Kangxi ⽊.txt -> 木.txt
    fs::write(dir.path().join("⽊.txt"), b"kangxi-data").unwrap();
    // 2. NFD -> NFC two-step rename: 하.txt -> 하.txt
    let nfd_ha = "\u{1112}\u{1161}.txt";
    fs::write(dir.path().join(nfd_ha), b"nfd-data").unwrap();
    // 3. Collision against unchanged sibling
    fs::write(dir.path().join("existing.txt"), b"original-existing").unwrap();
    fs::write(dir.path().join("wants_existing.txt"), b"colliding-src").unwrap();
    // 4. Intra-batch duplicate target
    fs::write(dir.path().join("dup_a.txt"), b"a").unwrap();
    fs::write(dir.path().join("dup_b.txt"), b"b").unwrap();
    // 5. Overflow >255 bytes
    fs::write(dir.path().join("overflow_src.txt"), b"ov").unwrap();

    #[cfg(unix)]
    std::os::unix::fs::symlink(dir.path().join("existing.txt"), dir.path().join("sym.txt"))
        .unwrap();

    let items = vec![
        SafeRenameItem {
            parent_dir: dir.path().to_path_buf(),
            old_raw_os_name: os_str_to_raw(std::ffi::OsStr::new("⽊.txt")),
            old_name: "⽊.txt".to_string(),
            new_name: "木.txt".to_string(),
        },
        SafeRenameItem {
            parent_dir: dir.path().to_path_buf(),
            old_raw_os_name: os_str_to_raw(std::ffi::OsStr::new(nfd_ha)),
            old_name: nfd_ha.to_string(),
            new_name: "하.txt".to_string(),
        },
        SafeRenameItem {
            parent_dir: dir.path().to_path_buf(),
            old_raw_os_name: os_str_to_raw(std::ffi::OsStr::new("wants_existing.txt")),
            old_name: "wants_existing.txt".to_string(),
            new_name: "existing.txt".to_string(),
        },
        SafeRenameItem {
            parent_dir: dir.path().to_path_buf(),
            old_raw_os_name: os_str_to_raw(std::ffi::OsStr::new("dup_a.txt")),
            old_name: "dup_a.txt".to_string(),
            new_name: "shared.txt".to_string(),
        },
        SafeRenameItem {
            parent_dir: dir.path().to_path_buf(),
            old_raw_os_name: os_str_to_raw(std::ffi::OsStr::new("dup_b.txt")),
            old_name: "dup_b.txt".to_string(),
            new_name: "shared.txt".to_string(),
        },
        SafeRenameItem {
            parent_dir: dir.path().to_path_buf(),
            old_raw_os_name: os_str_to_raw(std::ffi::OsStr::new("overflow_src.txt")),
            old_name: "overflow_src.txt".to_string(),
            new_name: format!("{}.txt", "a".repeat(255)),
        },
    ];

    let log = apply_safe_renames(&items, Some(&audit_path), false).expect("apply_safe_renames");
    assert!(audit_path.exists(), "incremental audit log must be written");

    assert_eq!(log.operations[0].status, RenameStatus::Applied);
    assert_eq!(fs::read(dir.path().join("木.txt")).unwrap(), b"kangxi-data");
    assert!(!dir.path().join("⽊.txt").exists());
    assert!(log.operations[0].file_id.is_some() || log.operations[0].byte_size == Some(11));

    assert!(
        matches!(
            log.operations[1].status,
            RenameStatus::Applied | RenameStatus::FilesystemForcedNfd
        ),
        "NFD->NFC two-step status: {:?}",
        log.operations[1]
    );
    assert_eq!(fs::read(dir.path().join("하.txt")).unwrap(), b"nfd-data");

    assert_eq!(log.operations[2].status, RenameStatus::BlockedCollision);
    assert_eq!(
        fs::read(dir.path().join("existing.txt")).unwrap(),
        b"original-existing"
    );
    assert_eq!(log.operations[3].status, RenameStatus::BlockedCollision);
    assert_eq!(log.operations[4].status, RenameStatus::BlockedCollision);
    assert_ne!(log.operations[5].status, RenameStatus::Applied);
    assert!(dir.path().join("overflow_src.txt").exists());

    // Symlink check
    #[cfg(unix)]
    {
        let sym_item = vec![SafeRenameItem {
            parent_dir: dir.path().to_path_buf(),
            old_raw_os_name: os_str_to_raw(std::ffi::OsStr::new("sym.txt")),
            old_name: "sym.txt".to_string(),
            new_name: "sym_renamed.txt".to_string(),
        }];
        let sym_log = apply_safe_renames(&sym_item, None, false).unwrap();
        assert_eq!(sym_log.operations[0].status, RenameStatus::BlockedSymlink);
    }

    // Undo dry-run leaves 木.txt on disk
    let dry_ops = undo_audit_log(&audit_path, true, false).unwrap();
    assert!(!dry_ops.is_empty());
    assert!(dir.path().join("木.txt").exists());

    // Real undo restores exact original filenames
    undo_audit_log(&audit_path, false, false).unwrap();
    assert!(dir.path().join("⽊.txt").exists());
    assert!(!dir.path().join("木.txt").exists());
    assert!(dir.path().join(nfd_ha).exists());
}

#[test]
fn undo_rejects_file_id_mismatch_unless_forced_and_always_rejects_path_traversal() {
    let dir = tempdir().unwrap();
    let audit_dir = tempdir().unwrap();
    let audit_path = audit_dir.path().join("undo_safety.json");

    fs::write(dir.path().join("before.txt"), b"original").unwrap();
    let item = vec![SafeRenameItem {
        parent_dir: dir.path().to_path_buf(),
        old_raw_os_name: os_str_to_raw(std::ffi::OsStr::new("before.txt")),
        old_name: "before.txt".to_string(),
        new_name: "after.txt".to_string(),
    }];
    apply_safe_renames(&item, Some(&audit_path), false).unwrap();

    // Replace after.txt with an unrelated file (new inode + new byte_size)
    fs::remove_file(dir.path().join("after.txt")).unwrap();
    fs::write(dir.path().join("after.txt"), b"replaced-external-content").unwrap();

    let err_ops = undo_audit_log(&audit_path, false, false).unwrap();
    assert_ne!(
        err_ops[0].status,
        RenameStatus::Applied,
        "undo must refuse changed file_id/metadata without --force"
    );
    assert!(dir.path().join("after.txt").exists());

    let forced_ops = undo_audit_log(&audit_path, false, true).unwrap();
    assert_eq!(forced_ops[0].status, RenameStatus::Applied);
    assert!(dir.path().join("before.txt").exists());

    // Tampered audit log with path traversal must ALWAYS return CoreError::UnsafePath even with force=true
    let evil_log = RenameAuditLog {
        schema_version: SCHEMA_VERSION.to_string(),
        operations: vec![RenameOperation {
            timestamp_utc: "2026-11-19T00:00:00Z".to_string(),
            parent_dir: dir.path().to_path_buf(),
            old_name: "../escape.txt".to_string(),
            old_raw_os_name: RawOsName::UnixBytes(b"../escape.txt".to_vec()),
            file_id: None,
            entry_type: EntryType::File,
            byte_size: None,
            mtime_ns: None,
            temp_name: None,
            new_name: "before.txt".to_string(),
            status: RenameStatus::Applied,
            error_detail: None,
        }],
    };
    let evil_path = audit_dir.path().join("evil.json");
    fs::write(&evil_path, serde_json::to_vec(&evil_log).unwrap()).unwrap();
    let err = undo_audit_log(&evil_path, false, true).unwrap_err();
    assert!(matches!(err, CoreError::UnsafePath(_)));

    // Verify capture_file_id_and_metadata + scan integration for EntryRenamePlan -> SafeRenameItem
    fs::write(dir.path().join("⽊_multi\u{200B}.txt"), b"m").unwrap();
    let report = scan_target(&ScanTarget::new(dir.path().to_path_buf())).unwrap();
    assert!(
        report
            .rename_plans
            .iter()
            .any(|p| p.composed_suggested_name.as_deref() == Some("木_multi.txt"))
    );
    let _meta = capture_file_id_and_metadata(&dir.path().join("before.txt")).unwrap();
}

#[test]
fn recovers_stranded_temp_file_via_undo_audit_log() {
    let dir = tempdir().unwrap();
    let audit_dir = tempdir().unwrap();
    let audit_path = audit_dir.path().join("stranded.json");

    let tmp_name = ".__nc_tmp_12345678".to_string();
    fs::write(dir.path().join(&tmp_name), b"precious").unwrap();
    let (file_id, entry_type, byte_size, mtime_ns) =
        capture_file_id_and_metadata(&dir.path().join(&tmp_name)).unwrap();

    let log = RenameAuditLog {
        schema_version: SCHEMA_VERSION.to_string(),
        operations: vec![RenameOperation {
            timestamp_utc: "2026-11-19T00:00:00Z".to_string(),
            parent_dir: dir.path().to_path_buf(),
            old_name: "원본.txt".to_string(),
            old_raw_os_name: RawOsName::UnixBytes("원본.txt".as_bytes().to_vec()),
            file_id,
            entry_type,
            byte_size,
            mtime_ns,
            temp_name: Some(tmp_name.clone()),
            new_name: "target.txt".to_string(),
            status: RenameStatus::IoError,
            error_detail: Some("Step 2 failed".to_string()),
        }],
    };
    fs::write(&audit_path, serde_json::to_vec(&log).unwrap()).unwrap();

    let ops = undo_audit_log(&audit_path, false, false).unwrap();
    assert_eq!(ops[0].status, RenameStatus::Applied);
    assert!(dir.path().join("원본.txt").exists());
    assert!(!dir.path().join(tmp_name).exists());
}

#[test]
fn verifies_audit_filename_prefix_backoff_schedule_and_forced_nfd_volume_halt() {
    use namechecker_core::renamer::{
        RETRY_BACKOFF_DELAYS_MS, default_audit_log_path, should_halt_forced_nfd_item,
    };
    use std::collections::HashSet;

    let default_path = default_audit_log_path();
    let fname = default_path.file_name().unwrap().to_str().unwrap();
    assert!(
        fname.starts_with("namechecker-rename-audit-") && fname.ends_with(".json"),
        "unexpected default audit filename: {fname}"
    );

    assert_eq!(RETRY_BACKOFF_DELAYS_MS, [25, 50, 100, 200, 400]);

    let mut forced = HashSet::new();
    forced.insert("dev:42".to_string());

    // NFD->NFC on forced volume halts
    assert!(should_halt_forced_nfd_item(
        &forced,
        "dev:42",
        "\u{1100}\u{1161}.txt",
        "가.txt"
    ));
    // Unrelated non-NFD rename on same volume continues
    assert!(!should_halt_forced_nfd_item(
        &forced, "dev:42", "⽊.txt", "木.txt"
    ));
    // NFD->NFC on a different volume continues
    assert!(!should_halt_forced_nfd_item(
        &forced,
        "dev:99",
        "\u{1100}\u{1161}.txt",
        "가.txt"
    ));
}

#[test]
fn undo_smb_fallback_succeeds_for_directory_with_matching_mtime_and_rejects_null_mtime() {
    let dir = tempdir().unwrap();
    let audit_dir = tempdir().unwrap();
    let audit_path = audit_dir.path().join("dir_smb_audit.json");

    let renamed_dir = dir.path().join("木_folder");
    fs::create_dir(&renamed_dir).unwrap();
    let (_real_id, entry_type, byte_size, mtime_ns) =
        capture_file_id_and_metadata(&renamed_dir).unwrap();
    assert_eq!(entry_type, EntryType::Directory);
    assert_eq!(byte_size, None);
    assert!(mtime_ns.is_some());

    // 1. Vacuous None mtime_ns with changed file_id must be REJECTED without --force
    let null_mtime_log = RenameAuditLog {
        schema_version: SCHEMA_VERSION.to_string(),
        operations: vec![RenameOperation {
            timestamp_utc: "2026-11-23T00:00:00Z".to_string(),
            parent_dir: dir.path().to_path_buf(),
            old_name: "⽊_folder".to_string(),
            old_raw_os_name: RawOsName::UnixBytes("⽊_folder".as_bytes().to_vec()),
            file_id: Some("99999:99999".to_string()),
            entry_type: EntryType::Directory,
            byte_size: None,
            mtime_ns: None,
            temp_name: None,
            new_name: "木_folder".to_string(),
            status: RenameStatus::Applied,
            error_detail: None,
        }],
    };
    fs::write(&audit_path, serde_json::to_vec(&null_mtime_log).unwrap()).unwrap();
    let rejected = undo_audit_log(&audit_path, false, false).unwrap();
    assert_eq!(rejected[0].status, RenameStatus::IoError);
    assert!(renamed_dir.exists());

    // 2. Matching non-null mtime_ns with changed SMB file_id must SUCCEED without --force
    let smb_dir_log = RenameAuditLog {
        schema_version: SCHEMA_VERSION.to_string(),
        operations: vec![RenameOperation {
            timestamp_utc: "2026-11-23T00:00:00Z".to_string(),
            parent_dir: dir.path().to_path_buf(),
            old_name: "⽊_folder".to_string(),
            old_raw_os_name: RawOsName::UnixBytes("⽊_folder".as_bytes().to_vec()),
            file_id: Some("99999:99999".to_string()),
            entry_type: EntryType::Directory,
            byte_size: None,
            mtime_ns,
            temp_name: None,
            new_name: "木_folder".to_string(),
            status: RenameStatus::Applied,
            error_detail: None,
        }],
    };
    fs::write(&audit_path, serde_json::to_vec(&smb_dir_log).unwrap()).unwrap();
    let restored = undo_audit_log(&audit_path, false, false).unwrap();
    assert_eq!(restored[0].status, RenameStatus::Applied);
    assert!(dir.path().join("⽊_folder").exists());
    assert!(!renamed_dir.exists());
}

#[test]
fn apply_safe_renames_rejects_old_raw_traversal_and_non_utf8_parent_dir_before_disk_write() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("valid_⽊.txt"), b"ok").unwrap();

    // 1. Traversal in old_raw_os_name must record IoError and NOT cause false BlockedCollision
    // on a legitimate sibling targeting the same new_name ("木.txt").
    let items = vec![
        SafeRenameItem {
            parent_dir: dir.path().to_path_buf(),
            old_raw_os_name: RawOsName::UnixBytes(b"../escape.txt".to_vec()),
            old_name: "../escape.txt".to_string(),
            new_name: "木.txt".to_string(),
        },
        SafeRenameItem {
            parent_dir: dir.path().to_path_buf(),
            old_raw_os_name: RawOsName::UnixBytes("valid_⽊.txt".as_bytes().to_vec()),
            old_name: "valid_⽊.txt".to_string(),
            new_name: "木.txt".to_string(),
        },
    ];
    let audit_path = tempdir().unwrap().path().join("traversal_audit.json");
    let log = apply_safe_renames(&items, Some(&audit_path), false).unwrap();
    assert_eq!(log.operations[0].status, RenameStatus::IoError);
    assert_eq!(log.operations[1].status, RenameStatus::Applied);
    assert!(dir.path().join("木.txt").exists());

    // 2. On Unix, non-UTF-8 parent_dir with audit_log_path = Some(...) must fail BEFORE mutating disk
    #[cfg(unix)]
    {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;
        let bad_parent = dir.path().join(OsStr::from_bytes(b"bad_\xff_dir"));
        let created_bad_dir = fs::create_dir(&bad_parent).is_ok();
        if created_bad_dir {
            fs::write(bad_parent.join("⽊.txt"), b"x").unwrap();
        }
        let bad_audit_dir = tempdir().unwrap();
        let bad_audit = bad_audit_dir.path().join("audit.json");
        let bad_items = vec![SafeRenameItem {
            parent_dir: bad_parent.clone(),
            old_raw_os_name: RawOsName::UnixBytes("⽊.txt".as_bytes().to_vec()),
            old_name: "⽊.txt".to_string(),
            new_name: "木.txt".to_string(),
        }];
        let res = apply_safe_renames(&bad_items, Some(&bad_audit), false);
        assert!(
            matches!(res, Err(CoreError::UnsafePath(_))),
            "non-UTF-8 parent_dir with audit log must fail pre-flight with UnsafePath: {res:?}"
        );
        if created_bad_dir {
            assert!(
                bad_parent.join("⽊.txt").exists(),
                "file on disk must remain untouched when audit pre-flight fails"
            );
            assert!(!bad_parent.join("木.txt").exists());
        }
    }
}

#[test]
fn audit_log_serialization_rejects_non_utf8_parent_dir_on_unix() {
    #[cfg(unix)]
    {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;
        use std::path::PathBuf;
        let bad_parent = PathBuf::from(OsStr::from_bytes(b"/tmp/\xff/sub"));
        let log = RenameAuditLog {
            schema_version: SCHEMA_VERSION.to_string(),
            operations: vec![RenameOperation {
                timestamp_utc: "2026-11-23T00:00:00Z".to_string(),
                parent_dir: bad_parent,
                old_name: "a.txt".to_string(),
                old_raw_os_name: RawOsName::UnixBytes(b"a.txt".to_vec()),
                file_id: None,
                entry_type: EntryType::File,
                byte_size: Some(1),
                mtime_ns: Some(1),
                temp_name: None,
                new_name: "b.txt".to_string(),
                status: RenameStatus::Applied,
                error_detail: None,
            }],
        };
        assert!(
            serde_json::to_string(&log).is_err(),
            "RenameOperation.parent_dir must never lossily write U+FFFD"
        );
    }
}

#[test]
fn undo_handles_mixed_forced_nfd_skips_halted_and_non_recoverable_io_errors() {
    let dir = tempdir().unwrap();
    let audit_dir = tempdir().unwrap();
    let audit_path = audit_dir.path().join("mixed_undo.json");

    // 1. Valid Applied entry to restore
    fs::write(dir.path().join("restored_new.txt"), b"valid").unwrap();
    let (id1, ty1, sz1, mt1) =
        capture_file_id_and_metadata(&dir.path().join("restored_new.txt")).unwrap();

    // 2. Halted pure NFD->NFC entry (untouched disk file "가_untouched.txt" in NFD)
    let untouched_nfd = "\u{1100}\u{1161}_untouched.txt";
    fs::write(dir.path().join(untouched_nfd), b"halted").unwrap();
    let (id2, ty2, sz2, mt2) =
        capture_file_id_and_metadata(&dir.path().join(untouched_nfd)).unwrap();

    // 3. Executed mixed FilesystemForcedNfd: old="⽊_가.txt", target new="木_가.txt",
    // on disk stored as decomposed "木_가.txt" (forcing NFD resolution on Linux/Unix)
    let mixed_disk_nfd = "木_\u{1100}\u{1161}.txt";
    fs::write(dir.path().join(mixed_disk_nfd), b"mixed-nfd").unwrap();
    let (id3, ty3, sz3, mt3) =
        capture_file_id_and_metadata(&dir.path().join(mixed_disk_nfd)).unwrap();

    let log = RenameAuditLog {
        schema_version: SCHEMA_VERSION.to_string(),
        operations: vec![
            // Non-recoverable IoError with bad new_name & bad old_raw_os_name must NOT abort valid rollbacks!
            RenameOperation {
                timestamp_utc: "2026-11-23T00:00:00Z".to_string(),
                parent_dir: dir.path().to_path_buf(),
                old_name: "../bad.txt".to_string(),
                old_raw_os_name: RawOsName::UnixBytes(b"../bad.txt".to_vec()),
                file_id: None,
                entry_type: EntryType::File,
                byte_size: None,
                mtime_ns: None,
                temp_name: None,
                new_name: "../bad_target.txt".to_string(),
                status: RenameStatus::IoError,
                error_detail: Some("unsafe path".to_string()),
            },
            RenameOperation {
                timestamp_utc: "2026-11-23T00:00:00Z".to_string(),
                parent_dir: dir.path().to_path_buf(),
                old_name: "restored_old.txt".to_string(),
                old_raw_os_name: RawOsName::UnixBytes(b"restored_old.txt".to_vec()),
                file_id: id1,
                entry_type: ty1,
                byte_size: sz1,
                mtime_ns: mt1,
                temp_name: None,
                new_name: "restored_new.txt".to_string(),
                status: RenameStatus::Applied,
                error_detail: None,
            },
            RenameOperation {
                timestamp_utc: "2026-11-23T00:00:00Z".to_string(),
                parent_dir: dir.path().to_path_buf(),
                old_name: untouched_nfd.to_string(),
                old_raw_os_name: RawOsName::UnixBytes(untouched_nfd.as_bytes().to_vec()),
                file_id: id2,
                entry_type: ty2,
                byte_size: sz2,
                mtime_ns: mt2,
                temp_name: None,
                new_name: "가_untouched.txt".to_string(),
                status: RenameStatus::FilesystemForcedNfd,
                error_detail: Some("halted".to_string()),
            },
            RenameOperation {
                timestamp_utc: "2026-11-23T00:00:00Z".to_string(),
                parent_dir: dir.path().to_path_buf(),
                old_name: "⽊_\u{1100}\u{1161}.txt".to_string(),
                old_raw_os_name: RawOsName::UnixBytes(
                    "⽊_\u{1100}\u{1161}.txt".as_bytes().to_vec(),
                ),
                file_id: id3,
                entry_type: ty3,
                byte_size: sz3,
                mtime_ns: mt3,
                temp_name: None,
                new_name: "木_가.txt".to_string(),
                status: RenameStatus::FilesystemForcedNfd,
                error_detail: Some("volume driver forcibly decomposed target to NFD".to_string()),
            },
        ],
    };
    fs::write(&audit_path, serde_json::to_vec(&log).unwrap()).unwrap();

    let outcomes = undo_audit_log(&audit_path, false, false).unwrap();
    // Only recoverable ops (Applied + executed mixed FilesystemForcedNfd) should be returned
    assert_eq!(
        outcomes.len(),
        2,
        "expected 2 recoverable ops: {outcomes:?}"
    );
    assert!(outcomes.iter().all(|o| o.status == RenameStatus::Applied));
    assert!(dir.path().join("restored_old.txt").exists());
    assert!(dir.path().join("⽊_\u{1100}\u{1161}.txt").exists());
    assert!(dir.path().join(untouched_nfd).exists());

    // Multi-sibling NFC ambiguity on normalization-sensitive filesystems (e.g., Linux ext4/btrfs):
    // When two distinct siblings normalize to the same NFC ("dup_\u{91D1}.txt"), `--force` must reject
    // ambiguous NFD fallback as IoError rather than picking one non-deterministically.
    // (On normalization-insensitive macOS APFS, U+F90A and U+91D1 coalesce into a single inode.)
    let amb_dir = tempdir().unwrap();
    let amb1 = amb_dir.path().join("dup_\u{F90A}.txt");
    let amb2 = amb_dir.path().join("dup_\u{91D1}.txt");
    fs::write(&amb1, b"1").unwrap();
    fs::write(&amb2, b"2").unwrap();
    let disk_count = fs::read_dir(amb_dir.path()).unwrap().count();
    let amb_audit_dir = tempdir().unwrap();
    let amb_audit = amb_audit_dir.path().join("amb_audit.json");
    let amb_log = RenameAuditLog {
        schema_version: SCHEMA_VERSION.to_string(),
        operations: vec![RenameOperation {
            timestamp_utc: "2026-11-23T00:00:00Z".to_string(),
            parent_dir: amb_dir.path().to_path_buf(),
            old_name: "⽊dup_金.txt".to_string(),
            old_raw_os_name: RawOsName::UnixBytes("⽊dup_金.txt".as_bytes().to_vec()),
            file_id: None,
            entry_type: EntryType::File,
            byte_size: Some(1),
            mtime_ns: Some(1),
            temp_name: None,
            new_name: "dup_金.txt".to_string(),
            status: RenameStatus::FilesystemForcedNfd,
            error_detail: None,
        }],
    };
    fs::write(&amb_audit, serde_json::to_vec(&amb_log).unwrap()).unwrap();
    if disk_count == 2 {
        let amb_res = undo_audit_log(&amb_audit, false, true).unwrap();
        assert_eq!(
            amb_res[0].status,
            RenameStatus::IoError,
            "ambiguous multi-sibling NFD fallback must be rejected even with --force"
        );
    } else {
        // Without --force, mismatched metadata still rejects safely even when APFS coalesced them
        let mismatch_res = undo_audit_log(&amb_audit, false, false).unwrap();
        assert_eq!(mismatch_res[0].status, RenameStatus::IoError);
    }
}

#[test]
fn formats_win32_verbatim_wide_losslessly_with_idempotency_clamping_and_surrogates() {
    use namechecker_core::renamer::format_win32_verbatim_wide;

    let enc = |s: &str| -> Vec<u16> { s.encode_utf16().collect() };

    // 1. Unpaired surrogate 0xD800 + trailing dot/space preserved losslessly, slashes normalized
    let mut raw_with_surrogate = enc("C:/dir/../leaf_");
    raw_with_surrogate.push(0xD800);
    raw_with_surrogate.extend(enc(". "));
    let mut expected = enc(r"\\?\C:\leaf_");
    expected.push(0xD800);
    expected.extend(enc(". "));
    assert_eq!(format_win32_verbatim_wide(&raw_with_surrogate), expected);

    // 2. Idempotency on already-verbatim local and UNC paths (normalizing / and non-leaf ..)
    assert_eq!(
        format_win32_verbatim_wide(&enc(r"\\?\C:\a/b/../name. ")),
        enc(r"\\?\C:\a\name. ")
    );
    assert_eq!(
        format_win32_verbatim_wide(&enc(r"\\?\UNC\srv\sh\a/../name.")),
        enc(r"\\?\UNC\srv\sh\name.")
    );

    // 3. Drive root & UNC share root clamping on ..
    assert_eq!(
        format_win32_verbatim_wide(&enc(r"C:\a\..\..\..\name")),
        enc(r"\\?\C:\name")
    );
    assert_eq!(
        format_win32_verbatim_wide(&enc(r"\\srv\sh\..\name")),
        enc(r"\\?\UNC\srv\sh\name")
    );

    // 4. Device paths, root-relative, relative, and drive-relative paths remain unprefixed & non-NUL
    for untouched in [r"\\.\pipe\nc", r"\foo\bar", r"foo\bar", r"C:foo.txt"] {
        let wide = enc(untouched);
        let out = format_win32_verbatim_wide(&wide);
        assert_eq!(out, wide, "path {untouched:?} must remain unprefixed");
        assert_ne!(out.last(), Some(&0u16));
    }
}

#[test]
fn phase11_win32_verbatim_long_parent_dir_and_logical_paths() {
    use namechecker_core::renamer::format_win32_verbatim_wide;

    let long_seg = "한글폴더_".repeat(60); // > 260 UTF-16 code units
    let local_long = format!(r"C:\{long_seg}\trailing ");
    let local_wide: Vec<u16> = local_long.encode_utf16().collect();
    assert!(local_wide.len() > 260);
    let local_verbatim = format_win32_verbatim_wide(&local_wide);
    let expected_local: Vec<u16> = format!(r"\\?\C:\{long_seg}\trailing ")
        .encode_utf16()
        .collect();
    assert_eq!(local_verbatim, expected_local);

    let unc_long = format!(r"\\nas01\media\{long_seg}\dot.");
    let unc_wide: Vec<u16> = unc_long.encode_utf16().collect();
    assert!(unc_wide.len() > 260);
    let unc_verbatim = format_win32_verbatim_wide(&unc_wide);
    let expected_unc: Vec<u16> = format!(r"\\?\UNC\nas01\media\{long_seg}\dot.")
        .encode_utf16()
        .collect();
    assert_eq!(unc_verbatim, expected_unc);

    #[cfg(windows)]
    {
        use std::ffi::OsString;
        use std::os::windows::ffi::{OsStrExt, OsStringExt};

        let root = tempdir().unwrap();
        let nested = root
            .path()
            .join("a".repeat(90))
            .join("b".repeat(90))
            .join("c".repeat(90));
        let nested_verbatim =
            format_win32_verbatim_wide(&nested.as_os_str().encode_wide().collect::<Vec<u16>>());
        let nested_vp = std::path::PathBuf::from(OsString::from_wide(&nested_verbatim));
        fs::create_dir_all(&nested_vp).unwrap();
        fs::write(nested_vp.join("⽊_long.txt"), b"x").unwrap();

        let report = scan_target(&ScanTarget::new(nested.clone())).unwrap();
        assert_eq!(report.total_entries, 1);
        assert!(
            report
                .findings
                .iter()
                .all(|f| !f.path.to_string_lossy().starts_with(r"\\?\")),
            "logical ScanReport path must not leak verbatim prefix: {:?}",
            report.findings
        );

        let audit_dir = tempdir().unwrap();
        let audit_path = audit_dir.path().join("long_path_audit.json");
        let item = SafeRenameItem {
            parent_dir: nested.clone(),
            old_raw_os_name: report.rename_plans[0].raw_os_name.clone(),
            old_name: "⽊_long.txt".to_string(),
            new_name: "木_long.txt".to_string(),
        };
        let log = apply_safe_renames(&[item], Some(&audit_path), false).unwrap();
        assert_eq!(log.operations[0].status, RenameStatus::Applied);
        assert!(nested_vp.join("木_long.txt").exists());
        let undone = undo_audit_log(&audit_path, false, false).unwrap();
        assert_eq!(undone[0].status, RenameStatus::Applied);
        assert!(nested_vp.join("⽊_long.txt").exists());
    }
}

#[test]
fn phase11_renamer_security_permission_symlink_mount_audit_and_dryrun_parity() {
    use namechecker_core::renamer::default_audit_log_path;

    // 1. Back-to-back default_audit_log_path() calls within 1 second must not collide
    let p1 = default_audit_log_path();
    let p2 = default_audit_log_path();
    assert_ne!(
        p1, p2,
        "default_audit_log_path must append a unique suffix to prevent 1s overwrite races"
    );

    // 2. Audit log path inside item.parent_dir must be rejected upfront (UnsafePath)
    let dir = tempdir().unwrap();
    let src = dir.path().join("⽊_inside.txt");
    fs::write(&src, b"data").unwrap();
    let item = SafeRenameItem {
        parent_dir: dir.path().to_path_buf(),
        old_raw_os_name: os_str_to_raw(std::ffi::OsStr::new("⽊_inside.txt")),
        old_name: "⽊_inside.txt".to_string(),
        new_name: "木_inside.txt".to_string(),
    };
    let inside_audit = dir.path().join("audit_inside.json");
    let res_inside = apply_safe_renames(std::slice::from_ref(&item), Some(&inside_audit), false);
    assert!(
        matches!(res_inside, Err(CoreError::UnsafePath(_))),
        "audit log inside target parent_dir must fail with CoreError::UnsafePath: {res_inside:?}"
    );
    assert!(
        !inside_audit.exists(),
        "audit log must not be written inside target folder"
    );

    // 3. Batch renames in a single directory commit atomic JSON audit log cleanly (no leftover temp files)
    let audit_dir = tempdir().unwrap();
    let batch_dir = tempdir().unwrap();
    fs::write(batch_dir.path().join("⽊_1.txt"), b"1").unwrap();
    fs::write(batch_dir.path().join("⽊_2.txt"), b"2").unwrap();
    let batch_items = vec![
        SafeRenameItem {
            parent_dir: batch_dir.path().to_path_buf(),
            old_raw_os_name: os_str_to_raw(std::ffi::OsStr::new("⽊_1.txt")),
            old_name: "⽊_1.txt".to_string(),
            new_name: "木_1.txt".to_string(),
        },
        SafeRenameItem {
            parent_dir: batch_dir.path().to_path_buf(),
            old_raw_os_name: os_str_to_raw(std::ffi::OsStr::new("⽊_2.txt")),
            old_name: "⽊_2.txt".to_string(),
            new_name: "木_2.txt".to_string(),
        },
    ];
    let batch_audit = audit_dir.path().join("atomic_batch.json");
    let batch_log = apply_safe_renames(&batch_items, Some(&batch_audit), false).unwrap();
    assert!(
        batch_log
            .operations
            .iter()
            .all(|o| o.status == RenameStatus::Applied)
    );
    let tmp_leftovers: Vec<_> = fs::read_dir(audit_dir.path())
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().contains(".tmp."))
        .collect();
    assert!(
        tmp_leftovers.is_empty(),
        "atomic audit flush must not leave temp sibling files: {tmp_leftovers:?}"
    );

    #[cfg(unix)]
    {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;
        use std::os::unix::fs::{PermissionsExt, symlink};

        // 4. dry_run with audit_log_path.is_some() must match live mode rejecting non-UTF-8 parent_dir
        let bad_parent = dir
            .path()
            .join(OsString::from_vec(b"bad_\xff_dir".to_vec()));
        let bad_item = SafeRenameItem {
            parent_dir: bad_parent,
            old_raw_os_name: RawOsName::UnixBytes(b"a.txt".to_vec()),
            old_name: "a.txt".to_string(),
            new_name: "b.txt".to_string(),
        };
        let dry_non_utf8 = apply_safe_renames(
            &[bad_item],
            Some(&audit_dir.path().join("dry_utf8.json")),
            true,
        );
        assert!(
            matches!(dry_non_utf8, Err(CoreError::UnsafePath(_))),
            "dry_run must enforce UTF-8 parent_dir pre-flight consistently with live mode: {dry_non_utf8:?}"
        );

        // 5. Execute-only (0o111) parent_dir where symlink_metadata succeeds but read_dir fails EACCES
        // must record RenameStatus::PermissionDenied, NOT BlockedCollision
        let perm_dir = tempdir().unwrap();
        fs::write(perm_dir.path().join("⽊_perm.txt"), b"p").unwrap();
        fs::set_permissions(perm_dir.path(), fs::Permissions::from_mode(0o111)).unwrap();
        if fs::read_dir(perm_dir.path()).is_err() {
            let perm_item = SafeRenameItem {
                parent_dir: perm_dir.path().to_path_buf(),
                old_raw_os_name: os_str_to_raw(std::ffi::OsStr::new("⽊_perm.txt")),
                old_name: "⽊_perm.txt".to_string(),
                new_name: "木_perm.txt".to_string(),
            };
            let perm_audit = audit_dir.path().join("perm_audit.json");
            let perm_log = apply_safe_renames(&[perm_item], Some(&perm_audit), false).unwrap();
            let _ = fs::set_permissions(perm_dir.path(), fs::Permissions::from_mode(0o755));
            assert_eq!(
                perm_log.operations[0].status,
                RenameStatus::PermissionDenied,
                "read_dir EACCES must propagate PermissionDenied instead of BlockedCollision"
            );
        } else {
            let _ = fs::set_permissions(perm_dir.path(), fs::Permissions::from_mode(0o755));
        }

        // 6. undo_audit_log must succeed when parent_dir is a valid symlinked directory mount point,
        // while child symlink targets inside it remain blocked
        let mount_base = tempdir().unwrap();
        let real_mount = mount_base.path().join("real_nas");
        let link_mount = mount_base.path().join("nas_link");
        fs::create_dir(&real_mount).unwrap();
        fs::write(real_mount.join("⽊_nas.txt"), b"nas").unwrap();
        symlink(&real_mount, &link_mount).unwrap();

        let mount_item = SafeRenameItem {
            parent_dir: link_mount.clone(),
            old_raw_os_name: os_str_to_raw(std::ffi::OsStr::new("⽊_nas.txt")),
            old_name: "⽊_nas.txt".to_string(),
            new_name: "木_nas.txt".to_string(),
        };
        let mount_audit = audit_dir.path().join("mount_audit.json");
        let mount_log = apply_safe_renames(&[mount_item], Some(&mount_audit), false).unwrap();
        assert_eq!(mount_log.operations[0].status, RenameStatus::Applied);
        let undone = undo_audit_log(&mount_audit, false, false)
            .expect("undo must succeed when parent_dir is a symlinked directory mount");
        assert_eq!(undone[0].status, RenameStatus::Applied);
        assert!(real_mount.join("⽊_nas.txt").exists());
    }
}

fn extract_windows_sys_features(toml_src: &str) -> Vec<String> {
    let mut in_win_deps = false;
    let mut in_features = false;
    let mut out = Vec::new();
    let mut bracket_buf = String::new();

    for line in toml_src.lines() {
        let code = line.split('#').next().unwrap_or("").trim();
        if code.is_empty() {
            continue;
        }
        if code.starts_with('[') && code.ends_with(']') {
            in_win_deps = code == "[target.'cfg(windows)'.dependencies]";
            in_features = false;
            bracket_buf.clear();
            continue;
        }
        if !in_win_deps {
            continue;
        }
        if !in_features && code.starts_with("windows-sys") && code.contains("features") {
            if let Some(idx) = code.find("features")
                && let Some(open) = code[idx..].find('[')
            {
                in_features = true;
                bracket_buf.push_str(&code[idx + open + 1..]);
            }
        } else if in_features {
            bracket_buf.push(' ');
            bracket_buf.push_str(code);
        }

        if in_features && let Some(end) = bracket_buf.find(']') {
            let inside = &bracket_buf[..end];
            for (i, part) in inside.split('"').enumerate() {
                if i % 2 == 1 {
                    out.push(part.to_string());
                }
            }
            in_features = false;
            bracket_buf.clear();
        }
    }
    out
}

#[test]
fn phase13_windows_sys_feature_and_pointer_regressions() {
    // Negative guards: commented feature or feature under wrong section must NOT satisfy parser
    let commented_only = r#"
[target.'cfg(windows)'.dependencies]
windows-sys = { version = "0.61", features = [
    # "Win32_Security",
    "Win32_Foundation",
] }
"#;
    assert!(
        !extract_windows_sys_features(commented_only)
            .iter()
            .any(|f| f == "Win32_Security"),
        "commented Win32_Security must not satisfy windows-sys feature parser"
    );

    let wrong_section = r#"
[dependencies]
windows-sys = { version = "0.61", features = ["Win32_Security"] }

[target.'cfg(windows)'.dependencies]
windows-sys = { version = "0.61", features = ["Win32_Foundation"] }
"#;
    assert!(
        !extract_windows_sys_features(wrong_section)
            .iter()
            .any(|f| f == "Win32_Security"),
        "feature under [dependencies] must not satisfy [target.'cfg(windows)'.dependencies]"
    );

    // Positive assertion on real Cargo.toml
    let cargo_toml = include_str!("../Cargo.toml");
    let features = extract_windows_sys_features(cargo_toml);
    assert!(
        features.iter().any(|f| f == "Win32_Security"),
        "crates/namechecker-core/Cargo.toml must enable Win32_Security for CreateFileW: {features:?}"
    );

    // Source-level pointer regressions in CreateFileW (renamer.rs) and CompareStringEx (collation_test.rs)
    let renamer_src = include_str!("../src/renamer.rs");
    let cf_start = renamer_src
        .find("CreateFileW(")
        .expect("CreateFileW call must exist in src/renamer.rs");
    let cf_end = renamer_src[cf_start..]
        .find(");")
        .expect("CreateFileW call terminator");
    let cf_call = &renamer_src[cf_start..cf_start + cf_end];
    assert!(
        !cf_call.contains("0 as _"),
        "CreateFileW must not use `0 as _` (triggers clippy::zero_ptr): {cf_call}"
    );
    assert!(
        cf_call.contains("null()") && cf_call.contains("null_mut()"),
        "CreateFileW must pass null() for lpsecurityattributes and null_mut() for htemplatefile: {cf_call}"
    );

    let collation_src = include_str!("collation_test.rs");
    let cs_start = collation_src
        .find("CompareStringEx(")
        .expect("CompareStringEx call must exist in tests/collation_test.rs");
    let cs_end = collation_src[cs_start..]
        .find(");")
        .expect("CompareStringEx call terminator");
    let cs_call = &collation_src[cs_start..cs_start + cs_end];
    assert!(
        !cs_call.contains("null_mut()"),
        "CompareStringEx lpreserved expects `*const c_void` (std::ptr::null()), not null_mut(): {cs_call}"
    );
}
