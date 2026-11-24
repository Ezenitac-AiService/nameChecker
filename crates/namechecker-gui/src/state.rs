//! Testable headless view-model and background worker coordinator for the 3-pane GUI.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use namechecker_core::collation::compare_entries;
use namechecker_core::detector::has_windows_smb_hazard;
use namechecker_core::encoding_hint::exceeds_cross_platform_name_limit;
use namechecker_core::i18n::MessageCatalog;
use namechecker_core::ignore_list::{add_exact_ignore_rule, load_ignore_list, save_ignore_list};
use namechecker_core::model::{
    CollationProfile, DiscrepancyCause, IssueCategory, OrderedEntry, PredictedEntry, RawOsName,
    RemediationTrack, RenameAuditLog, RenameOperation, RenameStatus, ScanReport, ScanTarget,
    SortDiscrepancy, UiLanguage,
};
use namechecker_core::renamer::{
    SafeRenameItem, apply_safe_renames, default_audit_log_dir, default_audit_log_path,
    load_audit_log, undo_audit_log,
};
use namechecker_core::report::sanitize_terminal_text;
use namechecker_core::scanner::{scan_target, simulate_predicted_after_order};
use namechecker_core::unicode_data::normalize_nfc;

/// Editable table row representing an entry inside the Center diagnostic pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditableEntryRow {
    pub path: PathBuf,
    pub parent_dir: PathBuf,
    pub raw_os_name: RawOsName,
    pub raw_name: String,
    pub track: RemediationTrack,
    pub category_summary: String,
    pub offending_summary: String,
    pub hints_summary: String,
    pub message: String,
    pub selected: bool,
    pub edited_target: String,
    pub ignored: bool,
    pub is_orphaned_temp: bool,
    pub has_length_overflow: bool,
    pub length_warning: Option<String>,
    pub has_collision: bool,
}

/// Messages delivered from background scan/prediction/rename/undo threads.
#[derive(Debug)]
pub enum WorkerEvent {
    ScanFinished(Box<Result<ScanReport, String>>),
    PredictionReady {
        generation: u64,
        predicted: Vec<PredictedEntry>,
    },
    RenameFinished {
        audit_path: PathBuf,
        result: Result<RenameAuditLog, String>,
    },
    UndoFinished(Result<Vec<RenameOperation>, String>),
}

/// Pure-Rust view-model driving the 3-pane desktop GUI.
#[derive(Debug)]
pub struct NameCheckerAppState {
    pub target_dir: Option<PathBuf>,
    pub lang: UiLanguage,
    pub active_profile: CollationProfile,
    pub show_all_discrepancies: bool,
    pub is_busy: bool,
    pub report: Option<ScanReport>,
    pub rows: Vec<EditableEntryRow>,
    pub selected_entry_name: Option<String>,
    pub before_order: Vec<OrderedEntry>,
    pub predicted_after_order: Vec<PredictedEntry>,
    pub visible_discrepancies: Vec<SortDiscrepancy>,
    pub show_rename_confirm: bool,
    pub pending_undo_path: Option<PathBuf>,
    pub pending_undo_count: usize,
    pub last_audit_log_path: Option<PathBuf>,
    pub ignore_list_path: Option<PathBuf>,
    pub status_banner: Option<String>,

    banner_state: Option<BannerState>,
    tx: mpsc::Sender<WorkerEvent>,
    rx: mpsc::Receiver<WorkerEvent>,
    repaint_ctx: Option<egui::Context>,
    prediction_generation: u64,
    applied_prediction_generation: u64,
    pending_prediction_at: Option<Instant>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum BannerState {
    Operations {
        is_undo: bool,
        applied: usize,
        forced_nfd: usize,
        blocked: usize,
        failed: usize,
        audit_saved: bool,
        samples: Vec<(RenameStatus, String)>,
        extra_issues: usize,
    },
}

impl BannerState {
    fn from_operations(ops: &[RenameOperation], is_undo: bool, audit_saved: bool) -> Self {
        let mut applied = 0usize;
        let mut forced_nfd = 0usize;
        let mut blocked = 0usize;
        let mut failed = 0usize;
        let mut samples = Vec::new();
        let mut total_issues = 0usize;

        for op in ops {
            match op.status {
                RenameStatus::Applied => applied += 1,
                RenameStatus::FilesystemForcedNfd => forced_nfd += 1,
                RenameStatus::BlockedCollision | RenameStatus::BlockedSymlink => blocked += 1,
                RenameStatus::PermissionDenied | RenameStatus::IoError => failed += 1,
            }
            if op.status != RenameStatus::Applied {
                total_issues += 1;
                if samples.len() < 2 {
                    let safe_name = sanitize_terminal_text(&op.old_name);
                    let snippet = if let Some(detail) = op.error_detail.as_deref()
                        && !detail.is_empty()
                    {
                        truncate_sanitized_banner_text(&format!("{safe_name}: {detail}"), 120)
                    } else {
                        truncate_sanitized_banner_text(&safe_name, 120)
                    };
                    samples.push((op.status, snippet));
                }
            }
        }

        let extra_issues = total_issues.saturating_sub(samples.len());
        Self::Operations {
            is_undo,
            applied,
            forced_nfd,
            blocked,
            failed,
            audit_saved,
            samples,
            extra_issues,
        }
    }

    fn format(&self, lang: UiLanguage) -> String {
        let Self::Operations {
            is_undo,
            applied,
            forced_nfd,
            blocked,
            failed,
            audit_saved,
            samples,
            extra_issues,
        } = self;

        let mut parts = Vec::new();
        let header = match (*is_undo, *audit_saved, lang) {
            (false, true, UiLanguage::Ko) => {
                format!("{applied}개 항목 이름 변경 완료 (감사 로그 저장됨)")
            }
            (false, false, UiLanguage::Ko) => format!("{applied}개 항목 이름 변경 완료"),
            (false, true, UiLanguage::En) => {
                format!("Renamed {applied} entries (audit log persisted)")
            }
            (false, false, UiLanguage::En) => format!("Renamed {applied} entries"),
            (true, _, UiLanguage::Ko) => format!("{applied}개 항목 원상 복구 완료"),
            (true, _, UiLanguage::En) => format!("Restored {applied} entries"),
        };
        parts.push(header);

        let mut counts = Vec::new();
        if *forced_nfd > 0 {
            counts.push(match lang {
                UiLanguage::Ko => format!("NFD 강제 분해 경고 {forced_nfd}건"),
                UiLanguage::En => format!("NFD forced warning: {forced_nfd}"),
            });
        }
        if *blocked > 0 {
            counts.push(match lang {
                UiLanguage::Ko => format!("차단 {blocked}건"),
                UiLanguage::En => format!("blocked: {blocked}"),
            });
        }
        if *failed > 0 {
            counts.push(match lang {
                UiLanguage::Ko => format!("실패 {failed}건"),
                UiLanguage::En => format!("failed: {failed}"),
            });
        }
        if !counts.is_empty() {
            parts.push(counts.join(", "));
        }

        if !samples.is_empty() {
            let mut detail_items: Vec<String> = samples
                .iter()
                .map(|(status, snippet)| {
                    format!("[{}] {snippet}", localized_rename_status(*status, lang))
                })
                .collect();
            if *extra_issues > 0 {
                detail_items.push(match lang {
                    UiLanguage::Ko => format!("(+{extra_issues}건 더 있음)"),
                    UiLanguage::En => format!("(+{extra_issues} more)"),
                });
            }
            parts.push(detail_items.join("; "));
        }

        parts.join(" | ")
    }
}

fn localized_rename_status(status: RenameStatus, lang: UiLanguage) -> &'static str {
    match (status, lang) {
        (RenameStatus::Applied, UiLanguage::Ko) => "완료",
        (RenameStatus::Applied, UiLanguage::En) => "applied",
        (RenameStatus::FilesystemForcedNfd, UiLanguage::Ko) => "NFD 강제분해",
        (RenameStatus::FilesystemForcedNfd, UiLanguage::En) => "NFD forced",
        (RenameStatus::BlockedCollision, UiLanguage::Ko) => "충돌 차단",
        (RenameStatus::BlockedCollision, UiLanguage::En) => "collision blocked",
        (RenameStatus::BlockedSymlink, UiLanguage::Ko) => "심볼릭 링크 차단",
        (RenameStatus::BlockedSymlink, UiLanguage::En) => "symlink blocked",
        (RenameStatus::PermissionDenied, UiLanguage::Ko) => "권한 거부",
        (RenameStatus::PermissionDenied, UiLanguage::En) => "permission denied",
        (RenameStatus::IoError, UiLanguage::Ko) => "I/O 오류",
        (RenameStatus::IoError, UiLanguage::En) => "I/O error",
    }
}

fn truncate_sanitized_banner_text(raw: &str, max_chars: usize) -> String {
    let clean = sanitize_terminal_text(raw);
    if clean.chars().count() <= max_chars {
        return clean;
    }
    let keep = max_chars.saturating_sub(3);
    let mut out: String = clean.chars().take(keep).collect();
    out.push_str("...");
    out
}

impl Default for NameCheckerAppState {
    fn default() -> Self {
        Self::new(UiLanguage::Ko)
    }
}

impl NameCheckerAppState {
    /// Creates a new GUI view-model initialized to `lang`.
    #[must_use]
    pub fn new(lang: UiLanguage) -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            target_dir: None,
            lang,
            active_profile: CollationProfile::WindowsKoKr,
            show_all_discrepancies: false,
            is_busy: false,
            report: None,
            rows: Vec::new(),
            selected_entry_name: None,
            before_order: Vec::new(),
            predicted_after_order: Vec::new(),
            visible_discrepancies: Vec::new(),
            show_rename_confirm: false,
            pending_undo_path: None,
            pending_undo_count: 0,
            last_audit_log_path: None,
            ignore_list_path: None,
            status_banner: None,
            banner_state: None,
            tx,
            rx,
            repaint_ctx: None,
            prediction_generation: 0,
            applied_prediction_generation: 0,
            pending_prediction_at: None,
        }
    }

    /// Formats a sanitized, bounded localized summary banner for completed rename or undo batches.
    #[must_use]
    pub fn summarize_rename_operations(
        ops: &[RenameOperation],
        is_undo: bool,
        lang: UiLanguage,
    ) -> String {
        BannerState::from_operations(ops, is_undo, !is_undo).format(lang)
    }

    /// Attaches an `egui::Context` so background threads can request immediate repaints.
    pub fn attach_context(&mut self, ctx: egui::Context) {
        self.repaint_ctx = Some(ctx);
    }

    /// Populates view-model panes from a completed `ScanReport`.
    pub fn load_report(&mut self, report: ScanReport) {
        self.show_rename_confirm = false;
        self.cancel_external_undo();
        let mut findings_by_path: HashMap<PathBuf, Vec<_>> = HashMap::new();
        for f in &report.findings {
            findings_by_path.entry(f.path.clone()).or_default().push(f);
        }

        let mut rows = Vec::with_capacity(report.rename_plans.len());
        for plan in &report.rename_plans {
            let matching = findings_by_path.get(&plan.path);
            let category_summary = matching
                .map(|list| {
                    list.iter()
                        .map(|f| MessageCatalog::category_label(f.category, self.lang))
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default();
            let offending_summary = matching
                .map(|list| {
                    list.iter()
                        .flat_map(|f| f.offending_chars.iter())
                        .map(|c| {
                            format!("{}@{}", sanitize_terminal_text(&c.code_point), c.char_index)
                        })
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default();
            let hints_summary = matching
                .map(|list| {
                    list.iter()
                        .flat_map(|f| f.candidate_hints.iter())
                        .map(|h| {
                            format!(
                                "{}: {}",
                                sanitize_terminal_text(&h.encoding),
                                sanitize_terminal_text(&h.decoded_name)
                            )
                        })
                        .collect::<Vec<_>>()
                        .join(" | ")
                })
                .unwrap_or_default();
            let is_orphaned_temp = matching.is_some_and(|list| {
                list.iter()
                    .any(|f| f.category == IssueCategory::OrphanedTempFile)
            });
            let message = matching
                .and_then(|list| list.first().map(|f| f.message.clone()))
                .unwrap_or_default();
            let edited_target = plan.composed_suggested_name.clone().unwrap_or_default();
            let selected = plan.track == RemediationTrack::AutoFixable && !edited_target.is_empty();
            let parent_dir = plan
                .path
                .parent()
                .map_or_else(|| PathBuf::from("."), Path::to_path_buf);

            let mut row = EditableEntryRow {
                path: plan.path.clone(),
                parent_dir,
                raw_os_name: plan.raw_os_name.clone(),
                raw_name: plan.raw_name.clone(),
                track: plan.track,
                category_summary,
                offending_summary,
                hints_summary,
                message,
                selected,
                edited_target,
                ignored: false,
                is_orphaned_temp,
                has_length_overflow: false,
                length_warning: None,
                has_collision: false,
            };
            validate_row_target(&mut row, self.lang);
            rows.push(row);
        }

        self.rows = rows;
        self.report = Some(report);
        self.pending_prediction_at = None;
        self.recompute_orders_for_profile();
        self.update_visible_discrepancies();
    }

    fn recompute_orders_for_profile(&mut self) {
        let Some(ref report) = self.report else {
            return;
        };

        self.pending_prediction_at = None;
        self.prediction_generation = self.prediction_generation.wrapping_add(1);

        let mut ordered = report.before_order.clone();
        ordered.sort_by(|a, b| compare_entries(self.active_profile, &a.name, &b.name));
        for (i, entry) in ordered.iter_mut().enumerate() {
            entry.rank = i + 1;
        }
        self.before_order = ordered;

        let inputs = self.build_prediction_inputs();
        let predicted = simulate_predicted_after_order(&inputs, self.active_profile);
        self.apply_prediction_results(predicted);
        self.applied_prediction_generation = self.prediction_generation;
    }

    fn build_prediction_inputs(&self) -> Vec<(String, Option<String>)> {
        let target_by_raw: HashMap<&str, Option<String>> = self
            .rows
            .iter()
            .map(|r| {
                let target = if r.selected
                    && !r.ignored
                    && !r.edited_target.is_empty()
                    && r.edited_target != r.raw_name
                {
                    Some(r.edited_target.clone())
                } else {
                    None
                };
                (r.raw_name.as_str(), target)
            })
            .collect();

        self.before_order
            .iter()
            .map(|entry| {
                let target = target_by_raw
                    .get(entry.name.as_str())
                    .cloned()
                    .unwrap_or(None);
                (entry.name.clone(), target)
            })
            .collect()
    }

    fn apply_prediction_results(&mut self, predicted: Vec<PredictedEntry>) {
        let colliding_old_names: std::collections::HashSet<&str> = predicted
            .iter()
            .filter(|p| p.has_collision)
            .map(|p| p.old_name.as_str())
            .collect();
        for row in &mut self.rows {
            row.has_collision = colliding_old_names.contains(row.raw_name.as_str());
        }
        self.predicted_after_order = predicted;
    }

    fn update_visible_discrepancies(&mut self) {
        let Some(ref report) = self.report else {
            self.visible_discrepancies.clear();
            return;
        };
        if self.show_all_discrepancies {
            self.visible_discrepancies = report.sort_discrepancies.clone();
        } else {
            self.visible_discrepancies = report
                .sort_discrepancies
                .iter()
                .filter(|d| d.cause == DiscrepancyCause::EncodingAnomaly)
                .cloned()
                .collect();
        }
    }

    /// Returns the effective `IgnoreList` JSON path (custom override or default user state dir).
    #[must_use]
    pub fn effective_ignore_list_path(&self) -> PathBuf {
        self.ignore_list_path
            .clone()
            .unwrap_or_else(|| default_audit_log_dir().join("ignore.json"))
    }

    /// Switches UI display language (`Ko` or `En`) and refreshes localized row summaries,
    /// messages, and discrepancy reasons.
    pub fn set_language(&mut self, lang: UiLanguage) {
        self.lang = lang;
        if let Some(ref mut report) = self.report {
            for d in &mut report.sort_discrepancies {
                d.reason = MessageCatalog::discrepancy_reason(d.cause, lang).to_string();
            }
            let mut findings_by_path: HashMap<&Path, Vec<_>> = HashMap::new();
            for f in &report.findings {
                findings_by_path
                    .entry(f.path.as_path())
                    .or_default()
                    .push(f);
            }
            for row in &mut self.rows {
                if let Some(list) = findings_by_path.get(row.path.as_path()) {
                    row.category_summary = list
                        .iter()
                        .map(|f| MessageCatalog::category_label(f.category, lang))
                        .collect::<Vec<_>>()
                        .join(", ");
                    if row.is_orphaned_temp {
                        row.message = MessageCatalog::orphaned_temp_message(&row.raw_name, lang);
                    } else if let Some(first) = list.first() {
                        row.message =
                            MessageCatalog::category_label(first.category, lang).to_string();
                    }
                }
                validate_row_target(row, lang);
            }
        } else {
            for row in &mut self.rows {
                validate_row_target(row, lang);
            }
        }
        for d in &mut self.visible_discrepancies {
            d.reason = MessageCatalog::discrepancy_reason(d.cause, lang).to_string();
        }
        if let Some(ref banner) = self.banner_state {
            self.status_banner = Some(banner.format(lang));
        }
    }

    /// Sets a custom `IgnoreList` JSON path (used in tests or `--ignore-file`).
    pub fn set_ignore_list_path(&mut self, path: Option<PathBuf>) {
        self.ignore_list_path = path;
    }

    /// Switches the active collation comparison profile and reveals inherent platform discrepancies.
    pub fn set_active_profile(&mut self, profile: CollationProfile) {
        self.active_profile = profile;
        self.show_all_discrepancies = profile != CollationProfile::WindowsKoKr;
        self.recompute_orders_for_profile();
        self.update_visible_discrepancies();
    }

    /// Synchronizes selected entry name across all 3 panes.
    pub fn select_entry(&mut self, name: Option<&str>) {
        self.selected_entry_name = name.map(str::to_string);
    }

    /// Updates an inline target edit and validates length/hazard constraints synchronously.
    pub fn update_row_edit(&mut self, path: &Path, new_target: String) {
        if let Some(row) = self.rows.iter_mut().find(|r| r.path == path) {
            row.edited_target = new_target;
            row.selected =
                !row.edited_target.is_empty() && row.edited_target != row.raw_name && !row.ignored;
            validate_row_target(row, self.lang);
            self.prediction_generation = self.prediction_generation.wrapping_add(1);
            self.pending_prediction_at = Some(Instant::now() + Duration::from_millis(80));
        }
    }

    /// Toggles selection for renaming on a row.
    pub fn toggle_row_selected(&mut self, path: &Path, selected: bool) {
        if let Some(row) = self.rows.iter_mut().find(|r| r.path == path) {
            row.selected = selected;
            self.prediction_generation = self.prediction_generation.wrapping_add(1);
            self.pending_prediction_at = Some(Instant::now() + Duration::from_millis(80));
        }
    }

    /// Toggles ignore flag on a `ManualReview` row (excluding `OrphanedTempFile`) and persists it.
    pub fn toggle_row_ignored(&mut self, path: &Path, ignored: bool) {
        let raw_name = if let Some(row) = self.rows.iter().find(|r| r.path == path) {
            if row.track != RemediationTrack::ManualReview || row.is_orphaned_temp {
                return;
            }
            row.raw_name.clone()
        } else if !ignored {
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                return;
            };
            name.to_string()
        } else {
            return;
        };

        let ignore_path = self.effective_ignore_list_path();
        let mut list = match load_ignore_list(&ignore_path) {
            Ok(list) => list,
            Err(err) => {
                self.banner_state = None;
                self.status_banner = Some(err.to_string());
                return;
            }
        };
        if ignored {
            if let Err(err) = add_exact_ignore_rule(&mut list, &raw_name, None, None) {
                self.banner_state = None;
                self.status_banner = Some(err.to_string());
                return;
            }
        } else {
            let nfc_raw = normalize_nfc(&raw_name);
            list.rules
                .retain(|r| r.is_glob || (r.pattern != raw_name && r.pattern != nfc_raw));
        }
        if let Err(err) = save_ignore_list(&ignore_path, &list) {
            self.banner_state = None;
            self.status_banner = Some(err.to_string());
            return;
        }

        if let Some(row) = self.rows.iter_mut().find(|r| r.path == path) {
            row.ignored = ignored;
            if ignored {
                row.selected = false;
            }
            self.prediction_generation = self.prediction_generation.wrapping_add(1);
            self.pending_prediction_at = Some(Instant::now() + Duration::from_millis(80));
        }
    }

    /// Flushes debounce timer if elapsed and spawns worker prediction.
    pub fn tick_debounce(&mut self) {
        if let Some(deadline) = self.pending_prediction_at
            && Instant::now() >= deadline
        {
            self.pending_prediction_at = None;
            self.request_debounced_prediction();
        }
    }

    /// Spawns a background prediction worker to recompute `predicted_after_order`.
    pub fn request_debounced_prediction(&mut self) {
        self.pending_prediction_at = None;
        self.prediction_generation = self.prediction_generation.wrapping_add(1);
        let generation = self.prediction_generation;
        let inputs = self.build_prediction_inputs();
        let profile = self.active_profile;
        let tx = self.tx.clone();
        let ctx = self.repaint_ctx.clone();

        std::thread::spawn(move || {
            let predicted = simulate_predicted_after_order(&inputs, profile);
            let _ = tx.send(WorkerEvent::PredictionReady {
                generation,
                predicted,
            });
            if let Some(c) = ctx {
                c.request_repaint();
            }
        });
    }

    /// Returns `true` only if there is valid non-overflow, collision-free work ready to rename.
    #[must_use]
    pub fn can_apply_renames(&self) -> bool {
        if self.is_busy
            || self.pending_prediction_at.is_some()
            || self.applied_prediction_generation != self.prediction_generation
        {
            return false;
        }
        let active: Vec<&EditableEntryRow> = self
            .rows
            .iter()
            .filter(|r| {
                r.selected
                    && !r.ignored
                    && !r.edited_target.is_empty()
                    && r.edited_target != r.raw_name
            })
            .collect();

        if active.is_empty() {
            return false;
        }
        if active
            .iter()
            .any(|r| r.has_length_overflow || has_windows_smb_hazard(&r.edited_target))
        {
            return false;
        }
        if self.predicted_after_order.iter().any(|p| p.has_collision) {
            return false;
        }
        true
    }

    /// Dispatches a read-only background scan of `dir`, clearing stale status banners.
    pub fn start_scan(&mut self, dir: PathBuf) {
        if self.is_busy {
            return;
        }
        self.show_rename_confirm = false;
        self.cancel_external_undo();
        self.start_scan_internal(dir, true);
    }

    fn start_scan_internal(&mut self, dir: PathBuf, clear_banner: bool) {
        self.target_dir = Some(dir.clone());
        self.is_busy = true;
        if clear_banner {
            self.status_banner = None;
            self.banner_state = None;
        }

        let mut target = ScanTarget::new(dir);
        target.lang = self.lang;
        target.ignore_list_path = Some(self.effective_ignore_list_path());
        let tx = self.tx.clone();
        let ctx = self.repaint_ctx.clone();

        std::thread::spawn(move || {
            let res = scan_target(&target).map_err(|e| e.to_string());
            let _ = tx.send(WorkerEvent::ScanFinished(Box::new(res)));
            if let Some(c) = ctx {
                c.request_repaint();
            }
        });
    }

    /// Dispatches safe renaming of selected rows in a background worker, writing an external audit log.
    pub fn start_apply_renames(&mut self, audit_path: PathBuf) {
        self.show_rename_confirm = false;
        if !self.can_apply_renames() {
            return;
        }
        let items: Vec<SafeRenameItem> = self
            .rows
            .iter()
            .filter(|r| {
                r.selected
                    && !r.ignored
                    && !r.edited_target.is_empty()
                    && r.edited_target != r.raw_name
                    && !r.has_length_overflow
                    && !has_windows_smb_hazard(&r.edited_target)
            })
            .map(|r| SafeRenameItem {
                parent_dir: r.parent_dir.clone(),
                old_raw_os_name: r.raw_os_name.clone(),
                old_name: r.raw_name.clone(),
                new_name: r.edited_target.clone(),
            })
            .collect();

        if items.is_empty() {
            return;
        }

        self.is_busy = true;
        let tx = self.tx.clone();
        let ctx = self.repaint_ctx.clone();

        std::thread::spawn(move || {
            let res =
                apply_safe_renames(&items, Some(&audit_path), false).map_err(|e| e.to_string());
            let _ = tx.send(WorkerEvent::RenameFinished {
                audit_path,
                result: res,
            });
            if let Some(c) = ctx {
                c.request_repaint();
            }
        });
    }

    /// Convenience helper using OS user state/data directory outside the target tree.
    pub fn start_apply_renames_default_audit(&mut self) {
        self.start_apply_renames(default_audit_log_path());
    }

    /// Reverses the last recorded rename audit log on a background worker.
    pub fn start_undo(&mut self) {
        if self.is_busy {
            return;
        }
        let Some(audit_path) = self.last_audit_log_path.clone() else {
            self.banner_state = None;
            self.status_banner = Some(match self.lang {
                UiLanguage::Ko => "되돌릴 작업 로그가 없습니다.".to_string(),
                UiLanguage::En => "No audit log available to undo.".to_string(),
            });
            return;
        };
        self.start_undo_from_path(audit_path);
    }

    /// Stages an external audit log for user confirmation prior to rollback.
    pub fn request_external_undo(&mut self, audit_path: PathBuf) {
        match load_audit_log(&audit_path) {
            Ok(log) => {
                let recoverable = log
                    .operations
                    .iter()
                    .filter(|o| is_recoverable_operation(o))
                    .count();
                if recoverable == 0 {
                    self.pending_undo_path = None;
                    self.pending_undo_count = 0;
                    self.banner_state = None;
                    self.status_banner = Some(match self.lang {
                        UiLanguage::Ko => "감사 로그에 되돌릴 수 있는 작업이 없습니다.".to_string(),
                        UiLanguage::En => "No recoverable operations in audit log.".to_string(),
                    });
                    return;
                }
                self.show_rename_confirm = false;
                self.pending_undo_count = recoverable;
                self.pending_undo_path = Some(audit_path);
            }
            Err(err) => {
                self.pending_undo_path = None;
                self.pending_undo_count = 0;
                self.banner_state = None;
                self.status_banner = Some(err.to_string());
            }
        }
    }

    /// Confirms and starts rollback of the staged external audit log.
    pub fn confirm_external_undo(&mut self) {
        if self.is_busy {
            return;
        }
        if let Some(path) = self.pending_undo_path.take() {
            self.pending_undo_count = 0;
            self.start_undo_from_path(path);
        }
    }

    /// Cancels a pending external audit log rollback confirmation.
    pub fn cancel_external_undo(&mut self) {
        self.pending_undo_path = None;
        self.pending_undo_count = 0;
    }

    /// Reverses operations recorded in an explicit audit log path on a background worker.
    pub fn start_undo_from_path(&mut self, audit_path: PathBuf) {
        if self.is_busy {
            return;
        }
        self.is_busy = true;
        let tx = self.tx.clone();
        let ctx = self.repaint_ctx.clone();

        std::thread::spawn(move || {
            let res = undo_audit_log(&audit_path, false, false).map_err(|e| e.to_string());
            let _ = tx.send(WorkerEvent::UndoFinished(res));
            if let Some(c) = ctx {
                c.request_repaint();
            }
        });
    }

    /// Drains worker messages without blocking; returns `true` if any state changed.
    pub fn poll_worker_events(&mut self) -> bool {
        let mut changed = false;
        while let Ok(ev) = self.rx.try_recv() {
            changed = true;
            match ev {
                WorkerEvent::ScanFinished(res_box) => match *res_box {
                    Ok(report) => {
                        self.is_busy = false;
                        self.load_report(report);
                    }
                    Err(err) => {
                        self.is_busy = false;
                        self.banner_state = None;
                        self.status_banner = Some(err);
                    }
                },
                WorkerEvent::PredictionReady {
                    generation,
                    predicted,
                } => {
                    if generation == self.prediction_generation {
                        self.apply_prediction_results(predicted);
                        self.applied_prediction_generation = generation;
                    }
                }
                WorkerEvent::RenameFinished { audit_path, result } => {
                    self.is_busy = false;
                    match result {
                        Ok(log) => {
                            let has_recoverable =
                                log.operations.iter().any(is_recoverable_operation);
                            if has_recoverable {
                                self.last_audit_log_path = Some(audit_path);
                            }
                            let banner = BannerState::from_operations(
                                &log.operations,
                                false,
                                has_recoverable,
                            );
                            self.status_banner = Some(banner.format(self.lang));
                            self.banner_state = Some(banner);
                            if let Some(dir) = self.target_dir.clone() {
                                self.start_scan_internal(dir, false);
                            }
                        }
                        Err(err) => {
                            self.banner_state = None;
                            self.status_banner = Some(err);
                        }
                    }
                }
                WorkerEvent::UndoFinished(result) => {
                    self.is_busy = false;
                    match result {
                        Ok(ops) => {
                            let banner = BannerState::from_operations(&ops, true, false);
                            self.status_banner = Some(banner.format(self.lang));
                            self.banner_state = Some(banner);
                            if let Some(dir) = self.target_dir.clone() {
                                self.start_scan_internal(dir, false);
                            }
                        }
                        Err(err) => {
                            self.banner_state = None;
                            self.status_banner = Some(err);
                        }
                    }
                }
            }
        }
        changed
    }
}

fn validate_row_target(row: &mut EditableEntryRow, lang: UiLanguage) {
    if row.edited_target.is_empty() {
        row.has_length_overflow = false;
        row.length_warning = None;
        return;
    }
    let (exceeds, utf8_bytes, utf16_units) = exceeds_cross_platform_name_limit(&row.edited_target);
    let hazard = has_windows_smb_hazard(&row.edited_target);
    row.has_length_overflow = exceeds;
    if exceeds {
        row.length_warning = Some(match lang {
            UiLanguage::Ko => {
                format!("길이 제한 초과 ({utf8_bytes}B UTF-8 / {utf16_units} UTF-16 > 255)")
            }
            UiLanguage::En => {
                format!("Length limit exceeded ({utf8_bytes}B UTF-8 / {utf16_units} UTF-16 > 255)")
            }
        });
    } else if hazard {
        row.length_warning = Some(match lang {
            UiLanguage::Ko => "Windows/SMB 금지 문자 또는 예약 이름 포함".to_string(),
            UiLanguage::En => "Contains Windows/SMB forbidden chars or reserved name".to_string(),
        });
    } else {
        row.length_warning = None;
    }
}

fn is_recoverable_operation(o: &RenameOperation) -> bool {
    o.status == RenameStatus::Applied
        || o.temp_name.is_some()
        || (o.status == RenameStatus::FilesystemForcedNfd
            && normalize_nfc(&o.old_name) != normalize_nfc(&o.new_name))
}
