//! Report formatters (sanitized terminal text, JSON schema v1.0.0, formula-safe CSV)
//! and severity/track report filtering.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Write as _;
use std::path::PathBuf;

use crate::i18n::MessageCatalog;
use crate::model::{CoreError, CsvReportRow, RemediationTrack, ScanReport, Severity, UiLanguage};

#[inline]
fn is_bidi_control(ch: char) -> bool {
    matches!(
        ch,
        '\u{061C}'
            | '\u{200E}'
            | '\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2066}'..='\u{2069}'
    )
}

/// Escapes control, ANSI/OSC escape, and Bidi override characters into safe `<U+XXXX>` tokens.
#[must_use]
pub fn sanitize_terminal_text(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        if ch.is_control() || is_bidi_control(ch) {
            let _ = write!(out, "<U+{:04X}>", ch as u32);
        } else {
            out.push(ch);
        }
    }
    out
}

/// Prefixes spreadsheet formula trigger characters (`=`, `+`, `-`, `@`, `\t`, `\r`) with `'`.
#[must_use]
pub fn escape_csv_formula(cell: &str) -> String {
    if cell.starts_with(['=', '+', '-', '@', '\t', '\r']) {
        format!("'{cell}")
    } else {
        cell.to_string()
    }
}

fn sanitize_and_escape_csv(cell: &str) -> String {
    escape_csv_formula(&sanitize_terminal_text(cell))
}

/// Filters a `ScanReport` by minimum severity (`Error >= Warning >= Info`), track, and/or categories.
#[must_use]
pub fn filter_report(
    mut report: ScanReport,
    min_severity: Option<Severity>,
    track: Option<RemediationTrack>,
    categories: Option<&[crate::model::IssueCategory]>,
) -> ScanReport {
    let has_cat_filter = categories.is_some_and(|c| !c.is_empty());
    if min_severity.is_none() && track.is_none() && !has_cat_filter {
        return report;
    }

    report.findings.retain(|f| {
        let sev_ok = min_severity.is_none_or(|min_sev| f.severity <= min_sev);
        let track_ok = track.is_none_or(|t| f.track == t);
        let cat_ok = categories.is_none_or(|cats| cats.is_empty() || cats.contains(&f.category));
        sev_ok && track_ok && cat_ok
    });

    let active_finding_ids: HashSet<&str> = report.findings.iter().map(|f| f.id.as_str()).collect();
    let active_paths: HashSet<PathBuf> = report.findings.iter().map(|f| f.path.clone()).collect();

    report.auto_fixable_count = report
        .findings
        .iter()
        .filter(|f| f.track == RemediationTrack::AutoFixable)
        .count();
    report.manual_review_count = report
        .findings
        .iter()
        .filter(|f| f.track == RemediationTrack::ManualReview)
        .count();

    let mut category_counts: BTreeMap<String, usize> = BTreeMap::new();
    for f in &report.findings {
        *category_counts
            .entry(format!("{:?}", f.category))
            .or_insert(0) += 1;
    }
    report.category_counts = category_counts;

    report.rename_plans.retain(|plan| {
        active_paths.contains(&plan.path)
            && plan
                .finding_ids
                .iter()
                .any(|id| active_finding_ids.contains(id.as_str()))
    });

    let active_names: HashSet<&str> = report
        .findings
        .iter()
        .map(|f| f.raw_name.as_str())
        .collect();
    for entry in &mut report.before_order {
        entry.has_issue = active_names.contains(entry.name.as_str());
    }

    report
}

fn write_finding_details(out: &mut String, finding: &crate::model::EntryFinding) {
    if !finding.offending_chars.is_empty() {
        let chars_str = finding
            .offending_chars
            .iter()
            .map(|c| format!("{}@{}", sanitize_terminal_text(&c.code_point), c.char_index))
            .collect::<Vec<_>>()
            .join(", ");
        let _ = writeln!(out, "      chars: {chars_str}");
    }
    if !finding.candidate_hints.is_empty() {
        let hints_str = finding
            .candidate_hints
            .iter()
            .map(|h| {
                format!(
                    "{}:{}",
                    sanitize_terminal_text(&h.encoding),
                    sanitize_terminal_text(&h.decoded_name)
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        let _ = writeln!(out, "      hints: {hints_str}");
    }
}

/// Formats a `ScanReport` as sanitized, localized terminal text.
#[must_use]
pub fn format_text(report: &ScanReport, lang: UiLanguage) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "=== {} ===",
        MessageCatalog::section_heading("summary", lang)
    );
    let summary_line = match lang {
        UiLanguage::Ko => format!(
            "총 항목: {}, 자동 수정 가능: {}, 수동 검토: {}, 정렬 불일치: {}, 충돌 위험: {}, I/O 오류: {}",
            report.total_entries,
            report.auto_fixable_count,
            report.manual_review_count,
            report.sort_discrepancy_count,
            report.collision_risk_count,
            report.io_errors.len()
        ),
        UiLanguage::En => format!(
            "Total entries: {}, Auto-Fixable: {}, Manual Review: {}, Sort Discrepancies: {}, Collision Risks: {}, I/O Errors: {}",
            report.total_entries,
            report.auto_fixable_count,
            report.manual_review_count,
            report.sort_discrepancy_count,
            report.collision_risk_count,
            report.io_errors.len()
        ),
    };
    let _ = writeln!(out, "{summary_line}");

    let auto_items: Vec<_> = report
        .findings
        .iter()
        .filter(|f| f.track == RemediationTrack::AutoFixable)
        .collect();
    if !auto_items.is_empty() {
        let _ = writeln!(
            out,
            "\n[{}]",
            MessageCatalog::section_heading("auto_fixable", lang)
        );
        for f in auto_items {
            let safe_name = sanitize_terminal_text(&f.raw_name);
            let safe_target = f.suggested_name.as_deref().map(sanitize_terminal_text);
            let _ = writeln!(
                out,
                "  - [{:?}] {:?} | {} -> {:?}",
                f.severity, f.category, safe_name, safe_target
            );
            write_finding_details(&mut out, f);
        }
    }

    let manual_items: Vec<_> = report
        .findings
        .iter()
        .filter(|f| f.track == RemediationTrack::ManualReview)
        .collect();
    if !manual_items.is_empty() {
        let _ = writeln!(
            out,
            "\n[{}]",
            MessageCatalog::section_heading("manual_review", lang)
        );
        for f in manual_items {
            let safe_name = sanitize_terminal_text(&f.raw_name);
            let safe_msg = sanitize_terminal_text(&f.message);
            let _ = writeln!(
                out,
                "  - [{:?}] {:?} | {} ({})",
                f.severity, f.category, safe_name, safe_msg
            );
            write_finding_details(&mut out, f);
        }
    }

    if !report.sort_discrepancies.is_empty() {
        let _ = writeln!(
            out,
            "\n[{}]",
            MessageCatalog::section_heading("sort_discrepancies", lang)
        );
        for d in &report.sort_discrepancies {
            let _ = writeln!(
                out,
                "  - [{:?}] {} <-> {} (divergent: {:?}) {}",
                d.cause,
                sanitize_terminal_text(&d.entry_a),
                sanitize_terminal_text(&d.entry_b),
                d.divergent_profiles,
                sanitize_terminal_text(&d.reason)
            );
        }
    }

    if !report.collision_risks.is_empty() {
        let _ = writeln!(
            out,
            "\n[{}]",
            MessageCatalog::section_heading("collision_risks", lang)
        );
        for c in &report.collision_risks {
            let entries: Vec<String> = c
                .entries
                .iter()
                .map(|e| sanitize_terminal_text(e))
                .collect();
            let _ = writeln!(
                out,
                "  - {:?} | {:?} ({})",
                c.collision_type,
                entries,
                sanitize_terminal_text(&c.normalized_key)
            );
        }
    }

    if !report.io_errors.is_empty() {
        let _ = writeln!(
            out,
            "\n[{}]",
            MessageCatalog::section_heading("io_errors", lang)
        );
        for e in &report.io_errors {
            let _ = writeln!(
                out,
                "  - {}: {}",
                sanitize_terminal_text(&e.path.to_string_lossy()),
                sanitize_terminal_text(&e.message)
            );
        }
    }

    out
}

/// Formats a `ScanReport` as pretty JSON conforming to `contracts/report-schema.json`.
pub fn format_json(report: &ScanReport) -> Result<String, CoreError> {
    serde_json::to_string_pretty(report).map_err(CoreError::Serialization)
}

/// Formats findings as UTF-8 BOM + RFC 4180 CSV bytes matching `CsvReportRow` (12 columns).
pub fn format_csv(report: &ScanReport) -> Result<Vec<u8>, CoreError> {
    let composed_by_path: HashMap<&std::path::Path, &str> = report
        .rename_plans
        .iter()
        .filter_map(|p| {
            p.composed_suggested_name
                .as_deref()
                .map(|s| (p.path.as_path(), s))
        })
        .collect();

    let mut out = Vec::with_capacity(256 + report.findings.len() * 128);
    // UTF-8 BOM required by data-model.md §6b for Windows Excel Korean compatibility.
    out.extend_from_slice(&[0xEF, 0xBB, 0xBF]);

    {
        let mut wtr = csv::WriterBuilder::new()
            .has_headers(false)
            .from_writer(&mut out);
        wtr.write_record([
            "path",
            "parent_dir",
            "raw_name",
            "entry_type",
            "track",
            "category",
            "severity",
            "offending_codepoints",
            "suggested_name",
            "composed_suggested_name",
            "candidate_hints",
            "message",
        ])
        .map_err(|err| CoreError::InvalidTarget(format!("CSV header write failed: {err}")))?;
        for f in &report.findings {
            let codepoints = f
                .offending_chars
                .iter()
                .map(|c| c.code_point.as_str())
                .collect::<Vec<_>>()
                .join(";");
            let hints = f
                .candidate_hints
                .iter()
                .map(|h| format!("{}:{}", h.encoding, h.decoded_name))
                .collect::<Vec<_>>()
                .join(";");
            let composed = composed_by_path
                .get(f.path.as_path())
                .copied()
                .unwrap_or("");

            let row = CsvReportRow {
                path: sanitize_and_escape_csv(&f.path.to_string_lossy()),
                parent_dir: sanitize_and_escape_csv(&f.parent_dir.to_string_lossy()),
                raw_name: sanitize_and_escape_csv(&f.raw_name),
                entry_type: sanitize_and_escape_csv(&format!("{:?}", f.entry_type)),
                track: sanitize_and_escape_csv(&format!("{:?}", f.track)),
                category: sanitize_and_escape_csv(&format!("{:?}", f.category)),
                severity: sanitize_and_escape_csv(&format!("{:?}", f.severity)),
                offending_codepoints: sanitize_and_escape_csv(&codepoints),
                suggested_name: sanitize_and_escape_csv(f.suggested_name.as_deref().unwrap_or("")),
                composed_suggested_name: sanitize_and_escape_csv(composed),
                candidate_hints: sanitize_and_escape_csv(&hints),
                message: sanitize_and_escape_csv(&f.message),
            };
            wtr.serialize(row)
                .map_err(|err| CoreError::InvalidTarget(format!("CSV serialize failed: {err}")))?;
        }
        wtr.flush().map_err(CoreError::Io)?;
    }

    Ok(out)
}
