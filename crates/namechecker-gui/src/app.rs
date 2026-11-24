//! Pure-Rust 3-pane desktop GUI built with `eframe`/`egui` and `egui_extras::TableBuilder`.

use egui_extras::{Column, TableBuilder};
use namechecker_core::i18n::MessageCatalog;
use namechecker_core::model::{CollationProfile, DiscrepancyCause, RemediationTrack, UiLanguage};
use namechecker_core::renamer::default_audit_log_dir;
use namechecker_core::report::sanitize_terminal_text;

use crate::state::NameCheckerAppState;

/// Installs OS Pan-CJK fallback font data (Korean/Japanese/Chinese) when available on disk.
pub fn configure_pan_cjk_fonts(ctx: &egui::Context) {
    let candidates: &[(&str, &str)] = &[
        // macOS
        (
            "macos_sd_gothic",
            "/System/Library/Fonts/AppleSDGothicNeo.ttc",
        ),
        (
            "macos_apple_gothic",
            "/System/Library/Fonts/Supplemental/AppleGothic.ttf",
        ),
        (
            "macos_hiragino_gb",
            "/System/Library/Fonts/Hiragino Sans GB.ttc",
        ),
        ("macos_arial_unicode", "/Library/Fonts/Arial Unicode.ttf"),
        // Windows
        ("win_malgun", r"C:\Windows\Fonts\malgun.ttf"),
        ("win_meiryo", r"C:\Windows\Fonts\meiryo.ttc"),
        ("win_msyh", r"C:\Windows\Fonts\msyh.ttc"),
        // Linux / NAS containers
        (
            "linux_noto_cjk",
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        ),
        (
            "linux_noto_kr",
            "/usr/share/fonts/truetype/noto/NotoSansKR-Regular.ttf",
        ),
    ];

    let mut fonts = egui::FontDefinitions::default();
    let mut loaded_names = Vec::new();

    for (key, path) in candidates {
        if let Ok(bytes) = std::fs::read(path) {
            fonts
                .font_data
                .insert((*key).to_string(), egui::FontData::from_owned(bytes).into());
            loaded_names.push((*key).to_string());
        }
    }

    if !loaded_names.is_empty() {
        for name in &loaded_names {
            fonts
                .families
                .entry(egui::FontFamily::Proportional)
                .or_default()
                .push(name.clone());
            fonts
                .families
                .entry(egui::FontFamily::Monospace)
                .or_default()
                .push(name.clone());
        }
        ctx.set_fonts(fonts);
    }
}

/// Main `eframe::App` implementing the 3-pane comparison and safe rename UI.
pub struct NameCheckerApp {
    pub state: NameCheckerAppState,
    context_attached: bool,
}

impl Default for NameCheckerApp {
    fn default() -> Self {
        Self {
            state: NameCheckerAppState::new(UiLanguage::Ko),
            context_attached: false,
        }
    }
}

impl NameCheckerApp {
    /// Creates a new app from `CreationContext`, registering Pan-CJK system fallback fonts.
    #[must_use]
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        configure_pan_cjk_fonts(&cc.egui_ctx);
        let mut state = NameCheckerAppState::new(UiLanguage::Ko);
        state.attach_context(cc.egui_ctx.clone());
        Self {
            state,
            context_attached: true,
        }
    }

    /// Wraps an existing pre-loaded `NameCheckerAppState` (primarily for headless testing).
    #[must_use]
    pub fn with_state(state: NameCheckerAppState) -> Self {
        Self {
            state,
            context_attached: false,
        }
    }

    /// Renders top toolbar and virtualized 3-pane UI inside `ui`.
    pub fn render_contents(&mut self, ui: &mut egui::Ui) {
        if !self.context_attached {
            self.state.attach_context(ui.ctx().clone());
            self.context_attached = true;
        }
        self.state.poll_worker_events();
        self.state.tick_debounce();

        self.render_toolbar(ui);
        self.render_left_before_pane(ui);
        self.render_right_after_pane(ui);
        self.render_center_diagnostic_pane(ui);
        self.render_confirm_modal(ui);
    }

    fn render_toolbar(&mut self, ui: &mut egui::Ui) {
        let lang = self.state.lang;
        egui::Panel::top("toolbar").show(ui, |ui| {
            ui.horizontal(|ui| {
                let can_select = !self.state.is_busy;
                if ui
                    .add_enabled(
                        can_select,
                        egui::Button::new(tr(lang, "폴더 선택", "Select Folder")),
                    )
                    .clicked()
                    && let Some(picked) = rfd::FileDialog::new().pick_folder()
                {
                    self.state.start_scan(picked);
                }

                let can_scan = self.state.target_dir.is_some() && !self.state.is_busy;
                if ui
                    .add_enabled(can_scan, egui::Button::new(tr(lang, "점검", "Inspect")))
                    .clicked()
                    && let Some(dir) = self.state.target_dir.clone()
                {
                    self.state.start_scan(dir);
                }

                let can_fix = self.state.can_apply_renames();
                if ui
                    .add_enabled(can_fix, egui::Button::new(tr(lang, "수정", "Fix")))
                    .clicked()
                {
                    self.state.cancel_external_undo();
                    self.state.show_rename_confirm = true;
                }

                let can_undo = !self.state.is_busy;
                if ui
                    .add_enabled(can_undo, egui::Button::new(tr(lang, "되돌리기", "Undo")))
                    .clicked()
                {
                    if self.state.last_audit_log_path.is_some() {
                        self.state.start_undo();
                    } else if let Some(picked) = rfd::FileDialog::new()
                        .set_directory(default_audit_log_dir())
                        .add_filter("JSON", &["json"])
                        .pick_file()
                    {
                        self.state.request_external_undo(picked);
                    }
                }

                ui.separator();

                let mut selected_profile = self.state.active_profile;
                egui::ComboBox::from_label(tr(lang, "정렬 프로필", "Collation Profile"))
                    .selected_text(profile_label(selected_profile, lang))
                    .show_ui(ui, |ui| {
                        for prof in CollationProfile::all() {
                            ui.selectable_value(
                                &mut selected_profile,
                                prof,
                                profile_label(prof, lang),
                            );
                        }
                    });
                if selected_profile != self.state.active_profile {
                    self.state.set_active_profile(selected_profile);
                }

                ui.separator();

                if ui
                    .selectable_label(self.state.lang == UiLanguage::Ko, "한국어")
                    .clicked()
                {
                    self.state.set_language(UiLanguage::Ko);
                }
                if ui
                    .selectable_label(self.state.lang == UiLanguage::En, "English")
                    .clicked()
                {
                    self.state.set_language(UiLanguage::En);
                }

                if self.state.is_busy {
                    ui.spinner();
                    ui.label(tr(lang, "작업 중...", "Working..."));
                }
            });

            if let Some(ref banner) = self.state.status_banner {
                ui.label(sanitize_terminal_text(banner));
            }
        });
    }

    fn render_left_before_pane(&mut self, ui: &mut egui::Ui) {
        let lang = self.state.lang;
        egui::Panel::left("left_before_pane")
            .resizable(true)
            .default_size(250.0)
            .show(ui, |ui| {
                ui.heading(format!(
                    "{} ({})",
                    tr(lang, "수정 전 정렬", "Before Order"),
                    profile_label(self.state.active_profile, lang)
                ));
                ui.separator();

                let row_count = self.state.before_order.len();
                let entries = self.state.before_order.clone();
                let mut clicked_name: Option<String> = None;
                let selected_name = self.state.selected_entry_name.as_deref();

                TableBuilder::new(ui)
                    .id_salt("before_order_table")
                    .striped(true)
                    .column(Column::initial(36.0))
                    .column(Column::remainder())
                    .header(20.0, |mut header| {
                        header.col(|ui| {
                            ui.strong("#");
                        });
                        header.col(|ui| {
                            ui.strong(tr(lang, "파일/폴더명", "Name"));
                        });
                    })
                    .body(|body| {
                        body.rows(20.0, row_count, |mut row| {
                            let idx = row.index();
                            let entry = &entries[idx];
                            let is_sel = selected_name == Some(entry.name.as_str());
                            let safe = sanitize_terminal_text(&entry.name);

                            row.col(|ui| {
                                ui.label(entry.rank.to_string());
                            });
                            row.col(|ui| {
                                let label = if entry.has_issue {
                                    format!("* {safe}")
                                } else {
                                    safe
                                };
                                if ui.selectable_label(is_sel, label).clicked() {
                                    clicked_name = Some(entry.name.clone());
                                }
                            });
                        });
                    });

                if let Some(name) = clicked_name {
                    self.state.select_entry(Some(&name));
                }
            });
    }

    fn render_right_after_pane(&mut self, ui: &mut egui::Ui) {
        let lang = self.state.lang;
        egui::Panel::right("right_after_pane")
            .resizable(true)
            .default_size(300.0)
            .show(ui, |ui| {
                ui.heading(tr(lang, "수정 후 예상 정렬", "Predicted After Order"));
                ui.separator();

                let predicted = self.state.predicted_after_order.clone();
                let discrepancies = self.state.visible_discrepancies.clone();
                let mut clicked_name: Option<String> = None;
                let selected_name = self.state.selected_entry_name.as_deref();

                let available_height = ui.available_height();
                let mut table = TableBuilder::new(ui)
                    .id_salt("after_order_table")
                    .striped(true)
                    .column(Column::initial(55.0))
                    .column(Column::remainder());
                if !discrepancies.is_empty() {
                    table = table.max_scroll_height((available_height * 0.55).max(120.0));
                }
                table
                    .header(20.0, |mut header| {
                        header.col(|ui| {
                            ui.strong(tr(lang, "이동", "Move"));
                        });
                        header.col(|ui| {
                            ui.strong(tr(lang, "예상 이름", "Predicted Name"));
                        });
                    })
                    .body(|body| {
                        body.rows(20.0, predicted.len(), |mut row| {
                            let idx = row.index();
                            let item = &predicted[idx];
                            let delta_text = if item.rank_delta > 0 {
                                format!("▲{}", item.rank_delta)
                            } else if item.rank_delta < 0 {
                                format!("▼{}", -item.rank_delta)
                            } else {
                                "-".to_string()
                            };
                            let mut label = sanitize_terminal_text(&item.new_name);
                            if item.has_collision {
                                let prefix = tr(lang, "[충돌]", "[Collision]");
                                label = format!("{prefix} {label}");
                            }
                            let is_sel = selected_name == Some(item.old_name.as_str());

                            row.col(|ui| {
                                ui.label(format!("{} ({delta_text})", item.new_rank));
                            });
                            row.col(|ui| {
                                if ui.selectable_label(is_sel, label).clicked() {
                                    clicked_name = Some(item.old_name.clone());
                                }
                            });
                        });
                    });

                if let Some(name) = clicked_name {
                    self.state.select_entry(Some(&name));
                }

                if !discrepancies.is_empty() {
                    ui.separator();
                    ui.heading(tr(lang, "정렬 불일치 내역", "Sort Discrepancies"));
                    egui::ScrollArea::vertical()
                        .id_salt("sort_discrepancies_scroll")
                        .show(ui, |ui| {
                            for d in &discrepancies {
                                let tag = match d.cause {
                                    DiscrepancyCause::EncodingAnomaly => {
                                        tr(lang, "[인코딩 이상]", "[Encoding Anomaly]")
                                    }
                                    DiscrepancyCause::InherentPlatformCollation => {
                                        tr(lang, "[OS 고유 정렬]", "[OS Collation]")
                                    }
                                };
                                ui.label(format!(
                                    "{tag} {} <-> {}",
                                    sanitize_terminal_text(&d.entry_a),
                                    sanitize_terminal_text(&d.entry_b)
                                ));
                                ui.label(
                                    egui::RichText::new(sanitize_terminal_text(&d.reason)).small(),
                                );
                            }
                        });
                }
            });
    }

    fn render_center_diagnostic_pane(&mut self, ui: &mut egui::Ui) {
        let lang = self.state.lang;
        egui::CentralPanel::default().show(ui, |ui| {
            ui.heading(tr(
                lang,
                "인코딩 요약 및 이름 변경 계획",
                "Encoding Summary & Rename Plan",
            ));

            if let Some(ref report) = self.state.report {
                ui.horizontal_wrapped(|ui| {
                    ui.label(format!(
                        "{}: {}",
                        tr(lang, "총 항목", "Total entries"),
                        report.total_entries
                    ));
                    ui.separator();
                    ui.label(format!(
                        "{}: {}",
                        tr(lang, "자동 수정 제안", "Auto-Fixable"),
                        report.auto_fixable_count
                    ));
                    ui.separator();
                    ui.label(format!(
                        "{}: {}",
                        tr(lang, "수동 확인 필요", "Manual Review"),
                        report.manual_review_count
                    ));
                    ui.separator();
                    for (enc, cnt) in &report.encoding_counts {
                        ui.label(format!("{}: {cnt}", sanitize_terminal_text(enc)));
                    }
                });
                if !report.category_counts.is_empty() {
                    ui.horizontal_wrapped(|ui| {
                        ui.strong(tr(lang, "이슈 유형:", "Categories:"));
                        for (cat, cnt) in &report.category_counts {
                            let cat_label = MessageCatalog::parse_category_key(cat).map_or_else(
                                || sanitize_terminal_text(cat),
                                |c| MessageCatalog::category_label(c, lang).to_string(),
                            );
                            ui.label(format!("[{cat_label}: {cnt}]"));
                        }
                    });
                }
                ui.separator();
            }

            let mut edited_updates: Vec<(std::path::PathBuf, String)> = Vec::new();
            let mut selected_updates: Vec<(std::path::PathBuf, bool)> = Vec::new();
            let mut ignore_updates: Vec<(std::path::PathBuf, bool)> = Vec::new();
            let mut clicked_name: Option<String> = None;
            let selected_name = self.state.selected_entry_name.clone();

            let row_count = self.state.rows.len();
            let mut rows_snapshot = self.state.rows.clone();
            let selected_row = selected_name
                .as_deref()
                .and_then(|sel| rows_snapshot.iter().find(|r| r.raw_name == sel))
                .cloned();
            let available_height = ui.available_height();

            let mut table = TableBuilder::new(ui)
                .id_salt("diagnostic_rows_table")
                .striped(true)
                .column(Column::initial(42.0))
                .column(Column::initial(85.0))
                .column(Column::initial(170.0))
                .column(Column::remainder())
                .column(Column::initial(55.0));
            if selected_row.is_some() {
                table = table.max_scroll_height((available_height * 0.58).max(140.0));
            }
            table
                .header(22.0, |mut header| {
                    header.col(|ui| {
                        ui.strong(tr(lang, "적용", "Apply"));
                    });
                    header.col(|ui| {
                        ui.strong(tr(lang, "트랙", "Track"));
                    });
                    header.col(|ui| {
                        ui.strong(tr(lang, "현재 이름", "Current Name"));
                    });
                    header.col(|ui| {
                        ui.strong(tr(lang, "대상 이름 (편집 가능)", "Target Name (Editable)"));
                    });
                    header.col(|ui| {
                        ui.strong(tr(lang, "무시", "Ignore"));
                    });
                })
                .body(|body| {
                    body.rows(26.0, row_count, |mut row_ui| {
                        let idx = row_ui.index();
                        let row = &mut rows_snapshot[idx];
                        let is_sel = selected_name.as_deref() == Some(row.raw_name.as_str());
                        let safe_raw = sanitize_terminal_text(&row.raw_name);

                        row_ui.col(|ui| {
                            let mut sel = row.selected;
                            if ui
                                .checkbox(&mut sel, "")
                                .on_hover_text(format!("{}: {safe_raw}", tr(lang, "적용", "Apply")))
                                .changed()
                            {
                                selected_updates.push((row.path.clone(), sel));
                            }
                        });

                        row_ui.col(|ui| {
                            let track_txt = match row.track {
                                RemediationTrack::AutoFixable => {
                                    tr(lang, "자동 수정", "Auto-Fixable")
                                }
                                RemediationTrack::ManualReview => {
                                    tr(lang, "수동 확인", "Manual Review")
                                }
                            };
                            ui.label(track_txt);
                        });

                        row_ui.col(|ui| {
                            if ui.selectable_label(is_sel, &safe_raw).clicked() {
                                clicked_name = Some(row.raw_name.clone());
                            }
                        });

                        row_ui.col(|ui| {
                            ui.horizontal(|ui| {
                                let mut buf = row.edited_target.clone();
                                let resp = ui
                                    .add(egui::TextEdit::singleline(&mut buf).hint_text(tr(
                                        lang,
                                        "대상 파일명",
                                        "Target filename",
                                    )))
                                    .on_hover_text(format!(
                                        "{}: {safe_raw}",
                                        tr(lang, "대상 이름", "Target name")
                                    ));
                                if resp.changed() {
                                    edited_updates.push((row.path.clone(), buf));
                                }
                                if let Some(ref warn) = row.length_warning {
                                    ui.colored_label(
                                        egui::Color32::RED,
                                        sanitize_terminal_text(warn),
                                    );
                                } else if row.has_collision {
                                    ui.colored_label(
                                        egui::Color32::RED,
                                        tr(lang, "충돌 위험", "Collision Risk"),
                                    );
                                }
                            });
                        });

                        row_ui.col(|ui| {
                            let mut ign = row.ignored;
                            if row.track == RemediationTrack::ManualReview
                                && !row.is_orphaned_temp
                                && ui
                                    .checkbox(&mut ign, "")
                                    .on_hover_text(format!(
                                        "{}: {safe_raw}",
                                        tr(lang, "무시", "Ignore")
                                    ))
                                    .changed()
                            {
                                ignore_updates.push((row.path.clone(), ign));
                            }
                        });
                    });
                });

            if let Some(sel_row) = selected_row {
                ui.separator();
                ui.group(|ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("selected_entry_details_scroll")
                        .show(ui, |ui| {
                            ui.strong(format!(
                                "{}: {}",
                                tr(lang, "선택 항목 상세", "Selected Entry Details"),
                                sanitize_terminal_text(&sel_row.raw_name)
                            ));
                            if !sel_row.category_summary.is_empty() {
                                ui.label(format!(
                                    "{}: {}",
                                    tr(lang, "유형", "Category"),
                                    sanitize_terminal_text(&sel_row.category_summary)
                                ));
                            }
                            if !sel_row.offending_summary.is_empty() {
                                ui.label(format!(
                                    "{}: {}",
                                    tr(lang, "문제 문자", "Offending Chars"),
                                    sel_row.offending_summary
                                ));
                            }
                            if !sel_row.hints_summary.is_empty() {
                                ui.label(format!(
                                    "{}: {}",
                                    tr(lang, "후보 제안", "Candidate Hints"),
                                    sel_row.hints_summary
                                ));
                            }
                            if !sel_row.message.is_empty() {
                                ui.label(sanitize_terminal_text(&sel_row.message));
                            }
                        });
                });
            }

            if let Some(name) = clicked_name {
                self.state.select_entry(Some(&name));
            }
            for (path, val) in edited_updates {
                self.state.update_row_edit(&path, val);
            }
            for (path, val) in selected_updates {
                self.state.toggle_row_selected(&path, val);
            }
            for (path, val) in ignore_updates {
                self.state.toggle_row_ignored(&path, val);
            }
        });
    }

    fn render_confirm_modal(&mut self, ui: &mut egui::Ui) {
        let lang = self.state.lang;
        if self.state.show_rename_confirm {
            let mut confirmed = false;
            let mut cancelled = false;
            egui::Window::new(tr(lang, "이름 변경 확인", "Confirm Rename"))
                .collapsible(false)
                .resizable(false)
                .show(ui.ctx(), |ui| {
                    ui.label(tr(
                        lang,
                        "선택된 항목의 파일명을 안전하게 변경하고 되돌리기 로그를 저장할까요?",
                        "Safely rename selected entries and save an undo audit log?",
                    ));
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(
                                self.state.can_apply_renames(),
                                egui::Button::new(tr(lang, "실행", "Apply")),
                            )
                            .clicked()
                        {
                            confirmed = true;
                        }
                        if ui.button(tr(lang, "취소", "Cancel")).clicked() {
                            cancelled = true;
                        }
                    });
                });

            if confirmed {
                self.state.start_apply_renames_default_audit();
            } else if cancelled {
                self.state.show_rename_confirm = false;
            }
        }

        if let Some(undo_path) = self.state.pending_undo_path.clone() {
            let op_count = self.state.pending_undo_count;
            let safe_path = sanitize_terminal_text(&undo_path.display().to_string());
            let mut confirmed = false;
            let mut cancelled = false;
            egui::Window::new(tr(lang, "되돌리기 확인", "Confirm Undo"))
                .collapsible(false)
                .resizable(false)
                .show(ui.ctx(), |ui| {
                    ui.label(match lang {
                        UiLanguage::Ko => {
                            format!("감사 로그({safe_path})의 {op_count}개 작업을 원상 복구할까요?")
                        }
                        UiLanguage::En => {
                            format!("Revert {op_count} operation(s) from audit log ({safe_path})?")
                        }
                    });
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(
                                !self.state.is_busy,
                                egui::Button::new(tr(lang, "실행", "Apply")),
                            )
                            .clicked()
                        {
                            confirmed = true;
                        }
                        if ui.button(tr(lang, "취소", "Cancel")).clicked() {
                            cancelled = true;
                        }
                    });
                });

            if confirmed {
                self.state.confirm_external_undo();
            } else if cancelled {
                self.state.cancel_external_undo();
            }
        }
    }
}

fn tr(lang: UiLanguage, ko: &'static str, en: &'static str) -> &'static str {
    match lang {
        UiLanguage::Ko => ko,
        UiLanguage::En => en,
    }
}

fn profile_label(profile: CollationProfile, lang: UiLanguage) -> &'static str {
    match (profile, lang) {
        (CollationProfile::WindowsKoKr, UiLanguage::Ko) => "Windows (한국어)",
        (CollationProfile::WindowsKoKr, UiLanguage::En) => "Windows (Korean)",
        (CollationProfile::MacOsKo, UiLanguage::Ko) => "macOS (한국어 Finder)",
        (CollationProfile::MacOsKo, UiLanguage::En) => "macOS (Korean Finder)",
        (CollationProfile::LinuxCodePoint, UiLanguage::Ko) => "Linux (UTF-8 코드 포인트)",
        (CollationProfile::LinuxCodePoint, UiLanguage::En) => "Linux (UTF-8 Code Point)",
        (CollationProfile::Japanese, UiLanguage::Ko) => "일본어 (Japanese)",
        (CollationProfile::Japanese, UiLanguage::En) => "Japanese",
        (CollationProfile::Chinese, UiLanguage::Ko) => "중국어 (Chinese)",
        (CollationProfile::Chinese, UiLanguage::En) => "Chinese",
    }
}

impl eframe::App for NameCheckerApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.render_contents(ui);
    }
}
