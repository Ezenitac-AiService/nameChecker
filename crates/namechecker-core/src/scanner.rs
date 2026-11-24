//! Non-destructive directory scanner enforcing configurable depth (`max_depth = 1` default in v1),
//! exclusion globs, symlink boundaries, and per-directory collation/collision analysis.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

use globset::{Glob, GlobSet, GlobSetBuilder};

use crate::collation::{compare_entries, detect_sort_discrepancies};
use crate::collision::detect_collisions;
use crate::detector::{EntryDetectionResult, inspect_entry_with_ignore};
use crate::ignore_list::load_ignore_list;
use crate::model::{
    CollationProfile, CollisionRisk, CoreError, EntryFinding, EntryRenamePlan, EntryType,
    IgnoreList, IoErrorRecord, OrderedEntry, PredictedEntry, RawOsName, RemediationTrack,
    SCHEMA_VERSION, ScanReport, ScanTarget, SortDiscrepancy, UNICODE_VERSION,
};
use crate::unicode_data::normalize_nfc;

fn build_exclude_matcher(globs: &[String]) -> Result<GlobSet, CoreError> {
    let mut builder = GlobSetBuilder::new();
    for pat in globs {
        let g = Glob::new(pat).map_err(|err| {
            CoreError::InvalidTarget(format!("invalid exclude glob '{pat}': {err}"))
        })?;
        builder.add(g);
    }
    builder
        .build()
        .map_err(|err| CoreError::InvalidTarget(format!("failed building globset: {err}")))
}

/// Extracts lossless `RawOsName` from an `OsStr`.
#[must_use]
pub fn os_str_to_raw(name: &OsStr) -> RawOsName {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        RawOsName::UnixBytes(name.as_bytes().to_vec())
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        RawOsName::WindowsWide(name.encode_wide().collect())
    }
}

struct ScannedRawEntry {
    name: String,
    entry_type: EntryType,
    auto_suggested: Option<String>,
    has_issue: bool,
}

#[allow(clippy::too_many_arguments)]
fn accumulate_detection(
    detected: EntryDetectionResult,
    display_name: String,
    entry_type: EntryType,
    encoding_counts: &mut BTreeMap<String, usize>,
    category_counts: &mut BTreeMap<String, usize>,
    auto_fixable_count: &mut usize,
    manual_review_count: &mut usize,
    findings: &mut Vec<EntryFinding>,
    rename_plans: &mut Vec<EntryRenamePlan>,
    issue_names: &mut HashSet<String>,
) -> ScannedRawEntry {
    *encoding_counts
        .entry(detected.encoding_type.to_string())
        .or_insert(0) += 1;

    let has_issue = !detected.findings.is_empty();
    if has_issue {
        issue_names.insert(display_name.clone());
    }

    for f in detected.findings {
        match f.track {
            RemediationTrack::AutoFixable => *auto_fixable_count += 1,
            RemediationTrack::ManualReview => *manual_review_count += 1,
        }
        *category_counts
            .entry(format!("{:?}", f.category))
            .or_insert(0) += 1;
        findings.push(f);
    }

    let auto_suggested = detected.rename_plan.as_ref().and_then(|p| {
        if p.track == RemediationTrack::AutoFixable {
            p.composed_suggested_name.clone()
        } else {
            None
        }
    });
    if let Some(plan) = detected.rename_plan {
        rename_plans.push(plan);
    }

    ScannedRawEntry {
        name: display_name,
        entry_type,
        auto_suggested,
        has_issue,
    }
}

/// Computes `predicted_after_order` given `(old_name, optional_target_name)` pairs ordered by `old_rank`.
#[must_use]
pub fn simulate_predicted_after_order(
    ordered_before: &[(String, Option<String>)],
    baseline: CollationProfile,
) -> Vec<PredictedEntry> {
    let n = ordered_before.len();
    if n == 0 {
        return Vec::new();
    }

    let targets: Vec<(usize, &str, String, bool)> = ordered_before
        .iter()
        .enumerate()
        .map(|(i, (old_name, opt_target))| {
            let new_name = opt_target
                .as_ref()
                .filter(|s| !s.is_empty())
                .cloned()
                .unwrap_or_else(|| old_name.clone());
            let changed = new_name != *old_name;
            (i + 1, old_name.as_str(), new_name, changed)
        })
        .collect();

    let mut nfc_counts: HashMap<String, usize> = HashMap::with_capacity(n);
    let mut case_counts: HashMap<String, usize> = HashMap::with_capacity(n);
    for (_, _, new_name, _) in &targets {
        *nfc_counts.entry(normalize_nfc(new_name)).or_insert(0) += 1;
        *case_counts.entry(new_name.to_lowercase()).or_insert(0) += 1;
    }

    let mut sorted_indices: Vec<usize> = (0..n).collect();
    sorted_indices.sort_by(|&i, &j| compare_entries(baseline, &targets[i].2, &targets[j].2));

    sorted_indices
        .into_iter()
        .enumerate()
        .map(|(new_pos, orig_idx)| {
            let new_rank = new_pos + 1;
            let (old_rank, old_name, ref new_name, changed) = targets[orig_idx];
            let nfc = normalize_nfc(new_name);
            let has_collision = nfc_counts.get(&nfc).copied().unwrap_or(0) > 1
                || (changed
                    && case_counts
                        .get(&new_name.to_lowercase())
                        .copied()
                        .unwrap_or(0)
                        > 1);
            PredictedEntry {
                old_rank,
                new_rank,
                rank_delta: PredictedEntry::compute_rank_delta(old_rank, new_rank),
                old_name: old_name.to_string(),
                new_name: new_name.clone(),
                changed,
                has_collision,
            }
        })
        .collect()
}

/// Scans the target directory in read-only mode and aggregates report entities.
pub fn scan_target(target: &ScanTarget) -> Result<ScanReport, CoreError> {
    target.validate()?;
    if target.max_depth == Some(0) {
        return Err(CoreError::InvalidTarget(
            "max_depth must be at least 1".to_string(),
        ));
    }

    let root = &target.roots[0];
    let exclude_matcher = build_exclude_matcher(&target.exclude_globs)?;
    let ignore_list: Option<IgnoreList> = match target.ignore_list_path.as_deref() {
        Some(p) => Some(load_ignore_list(p)?),
        None => None,
    };
    let baseline = target
        .profiles
        .first()
        .copied()
        .unwrap_or(CollationProfile::WindowsKoKr);

    let mut total_entries: usize = 0;
    let mut auto_fixable_count: usize = 0;
    let mut manual_review_count: usize = 0;
    let mut encoding_counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut category_counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut io_errors: Vec<IoErrorRecord> = Vec::new();
    let mut findings = Vec::new();
    let mut rename_plans = Vec::new();
    let mut sort_discrepancies: Vec<SortDiscrepancy> = Vec::new();
    let mut collision_risks: Vec<CollisionRisk> = Vec::new();

    let mut root_raw_entries: Vec<ScannedRawEntry> = Vec::new();
    let mut dir_queue: VecDeque<(PathBuf, usize)> = VecDeque::new();
    let mut visited_canon: HashSet<PathBuf> = HashSet::new();
    #[cfg(any(unix, windows))]
    let mut visited_dev_ino: HashSet<(u64, u64)> = HashSet::new();
    dir_queue.push_back((root.clone(), 1));

    while let Some((dir, depth)) = dir_queue.pop_front() {
        if target.follow_symlinks {
            let vp_dir = crate::renamer::verbatim_path_buf(&dir);
            let mut cycle_id_ok = false;
            if let Ok(canon) = std::fs::canonicalize(vp_dir.as_ref()) {
                cycle_id_ok = true;
                if !visited_canon.insert(canon) {
                    continue;
                }
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                if let Ok(meta) = std::fs::metadata(vp_dir.as_ref()) {
                    cycle_id_ok = true;
                    if !visited_dev_ino.insert((meta.dev(), meta.ino())) {
                        continue;
                    }
                }
            }
            #[cfg(windows)]
            {
                if let Some(win_id) = crate::renamer::windows_dir_file_id(&dir) {
                    cycle_id_ok = true;
                    if !visited_dev_ino.insert(win_id) {
                        continue;
                    }
                }
            }
            if !cycle_id_ok {
                io_errors.push(IoErrorRecord {
                    path: dir,
                    message: "unable to verify directory identity for cycle detection".to_string(),
                });
                continue;
            }
        }
        let mut dir_issue_names: HashSet<String> = HashSet::new();
        let mut dir_entries: Vec<ScannedRawEntry> = Vec::new();

        scan_single_directory(
            &dir,
            depth,
            target,
            &exclude_matcher,
            ignore_list.as_ref(),
            &mut total_entries,
            &mut auto_fixable_count,
            &mut manual_review_count,
            &mut encoding_counts,
            &mut category_counts,
            &mut io_errors,
            &mut findings,
            &mut rename_plans,
            &mut dir_issue_names,
            &mut dir_entries,
            &mut dir_queue,
        );

        dir_entries.sort_by(|a, b| compare_entries(baseline, &a.name, &b.name));

        let dir_names: Vec<&str> = dir_entries.iter().map(|e| e.name.as_str()).collect();
        let dir_issue_refs: HashSet<&str> = dir_issue_names.iter().map(String::as_str).collect();
        sort_discrepancies.extend(detect_sort_discrepancies(
            &dir,
            &dir_names,
            &dir_issue_refs,
            baseline,
            &target.profiles,
            target.lang,
        ));
        collision_risks.extend(detect_collisions(&dir, &dir_names));

        if depth == 1 {
            root_raw_entries = dir_entries;
        }
    }

    let before_order: Vec<OrderedEntry> = root_raw_entries
        .iter()
        .enumerate()
        .map(|(i, e)| OrderedEntry {
            rank: i + 1,
            name: e.name.clone(),
            entry_type: e.entry_type,
            has_issue: e.has_issue,
        })
        .collect();

    let sim_inputs: Vec<(String, Option<String>)> = root_raw_entries
        .iter()
        .map(|e| (e.name.clone(), e.auto_suggested.clone()))
        .collect();
    let predicted_after_order = simulate_predicted_after_order(&sim_inputs, baseline);

    Ok(ScanReport {
        schema_version: SCHEMA_VERSION.to_string(),
        unicode_version: UNICODE_VERSION.to_string(),
        scanned_at_utc: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        total_entries,
        auto_fixable_count,
        manual_review_count,
        sort_discrepancy_count: sort_discrepancies.len(),
        collision_risk_count: collision_risks.len(),
        encoding_counts,
        category_counts,
        before_order,
        predicted_after_order,
        io_errors,
        findings,
        rename_plans,
        sort_discrepancies,
        collision_risks,
    })
}

#[allow(clippy::too_many_arguments)]
fn scan_single_directory(
    dir: &Path,
    depth: usize,
    target: &ScanTarget,
    exclude_matcher: &GlobSet,
    ignore_list: Option<&IgnoreList>,
    total_entries: &mut usize,
    auto_fixable_count: &mut usize,
    manual_review_count: &mut usize,
    encoding_counts: &mut BTreeMap<String, usize>,
    category_counts: &mut BTreeMap<String, usize>,
    io_errors: &mut Vec<IoErrorRecord>,
    findings: &mut Vec<EntryFinding>,
    rename_plans: &mut Vec<EntryRenamePlan>,
    dir_issue_names: &mut HashSet<String>,
    dir_entries: &mut Vec<ScannedRawEntry>,
    dir_queue: &mut VecDeque<(PathBuf, usize)>,
) {
    let vp_dir = crate::renamer::verbatim_path_buf(dir);
    let entries = match fs::read_dir(vp_dir.as_ref()) {
        Ok(rd) => rd,
        Err(err) => {
            io_errors.push(IoErrorRecord {
                path: dir.to_path_buf(),
                message: err.to_string(),
            });
            return;
        }
    };

    let can_descend = target.max_depth.is_none_or(|limit| depth < limit);

    for entry_res in entries {
        let entry = match entry_res {
            Ok(e) => e,
            Err(err) => {
                io_errors.push(IoErrorRecord {
                    path: dir.to_path_buf(),
                    message: err.to_string(),
                });
                continue;
            }
        };

        let os_name = entry.file_name();
        let display_name = os_name.to_string_lossy().into_owned();
        let is_orphaned_temp =
            display_name.starts_with(".__nc_tmp_") && display_name.len() > ".__nc_tmp_".len();
        if !is_orphaned_temp && exclude_matcher.is_match(&display_name) {
            continue;
        }

        // Preserve logical user-facing path without Windows \\?\ prefix leak.
        let entry_path = dir.join(&os_name);
        let file_type = match entry.file_type() {
            Ok(ft) => ft,
            Err(err) => {
                io_errors.push(IoErrorRecord {
                    path: entry_path,
                    message: err.to_string(),
                });
                continue;
            }
        };

        let entry_type = if file_type.is_symlink() {
            EntryType::Symlink
        } else if file_type.is_dir() {
            EntryType::Directory
        } else {
            EntryType::File
        };

        if can_descend {
            match entry_type {
                EntryType::Directory => {
                    dir_queue.push_back((entry_path.clone(), depth + 1));
                }
                EntryType::Symlink if target.follow_symlinks => {
                    let vp_entry = crate::renamer::verbatim_path_buf(&entry_path);
                    match fs::metadata(vp_entry.as_ref()) {
                        Ok(meta) if meta.is_dir() => {
                            dir_queue.push_back((entry_path.clone(), depth + 1));
                        }
                        Ok(_) => {}
                        Err(err) => {
                            io_errors.push(IoErrorRecord {
                                path: entry_path.clone(),
                                message: err.to_string(),
                            });
                        }
                    }
                }
                _ => {}
            }
        }

        *total_entries += 1;
        let raw_os_name = os_str_to_raw(&os_name);
        let detected = inspect_entry_with_ignore(
            &entry_path,
            &raw_os_name,
            entry_type,
            target.lang,
            ignore_list,
        );
        let rec = accumulate_detection(
            detected,
            display_name,
            entry_type,
            encoding_counts,
            category_counts,
            auto_fixable_count,
            manual_review_count,
            findings,
            rename_plans,
            dir_issue_names,
        );
        dir_entries.push(rec);
    }
}
