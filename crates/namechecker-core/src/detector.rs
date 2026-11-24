//! Per-entry Unicode, CJK normalization, confusable, hazard, and orphaned temp-file detector.

use std::collections::BTreeSet;
use std::path::Path;

use crate::encoding_hint::{detect_legacy_encoding_hints, exceeds_cross_platform_name_limit};
use crate::i18n::MessageCatalog;
use crate::model::{
    CharDetail, EncodingHint, EntryFinding, EntryRenamePlan, EntryType, IssueCategory, RawOsName,
    RemediationTrack, Severity, UiLanguage,
};
use crate::unicode_data::{
    compat_ideograph_canonical, compose_kana_dakuten_hint, halfwidth_kana_to_fullwidth,
    is_assigned_cjk_radical_supplement, is_assigned_isolated_jamo, is_extended_pictographic,
    is_hangul_filler, is_normalized_nfc, is_valid_emoji_vs_base, isolated_jamo_compat_hint,
    katakana_hanzi_confusable, normalize_nfc, radical_compat_fix, radical_supplement_hint,
    script_homoglyph_hint, sfm_pua_hint,
};

const RESERVED_WINDOWS_DEVICES: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Result of inspecting a single directory entry name.
#[derive(Debug, Clone)]
pub struct EntryDetectionResult {
    /// Classification bucket (`"UTF-8 NFC"`, `"UTF-8 NFD"`, or `"Non-UTF-8 Legacy"`).
    pub encoding_type: &'static str,
    /// Detected findings on the entry.
    pub findings: Vec<EntryFinding>,
    /// Unified rename plan if any fixes or manual reviews apply.
    pub rename_plan: Option<EntryRenamePlan>,
}

fn deterministic_finding_id(
    path: &Path,
    category: IssueCategory,
    track: RemediationTrack,
) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in path.to_string_lossy().as_bytes() {
        hash ^= u64::from(*b);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    for b in format!("{category:?}:{track:?}").as_bytes() {
        hash ^= u64::from(*b);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{hash:016x}")
}

fn format_code_point(ch: char) -> String {
    format!("U+{:04X}", ch as u32)
}

fn visible_glyph_for(ch: char) -> Option<String> {
    if ch.is_control()
        || is_invisible_control(ch)
        || matches!(ch, '\u{FE00}'..='\u{FE0F}' | '\u{E0100}'..='\u{E01EF}')
    {
        None
    } else {
        Some(ch.to_string())
    }
}

const fn is_invisible_control(ch: char) -> bool {
    matches!(
        ch,
        '\u{200B}'
            | '\u{FEFF}'
            | '\u{00AD}'
            | '\u{200C}'..='\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2066}'..='\u{2069}'
    ) || is_hangul_filler(ch)
}

const fn is_han_scalar(ch: char) -> bool {
    matches!(
        ch as u32,
        0x4E00..=0x9FFF
            | 0x3400..=0x4DBF
            | 0x20000..=0x2A6DF
            | 0x2A700..=0x2CEAF
            | 0x2CEB0..=0x2EE5F
            | 0x30000..=0x323AF
            | 0xF900..=0xFAFF
            | 0x2F800..=0x2FA1F
    )
}

const fn is_hangul_scalar(ch: char) -> bool {
    matches!(
        ch as u32,
        0xAC00..=0xD7A3 | 0x1100..=0x11FF | 0x3131..=0x318E | 0xA960..=0xA97F | 0xD7B0..=0xD7FF
    )
}

const fn is_kana_scalar(ch: char) -> bool {
    matches!(ch as u32, 0x3040..=0x30FF | 0x31F0..=0x31FF | 0xFF66..=0xFF9F)
}

const fn is_token_separator(ch: char) -> bool {
    ch.is_ascii_whitespace()
        || matches!(
            ch,
            '.' | '_' | '-' | '/' | '\\' | '(' | ')' | '[' | ']' | '{' | '}' | ',' | ':' | ';'
        )
}

fn token_bounds(chars: &[char], idx: usize) -> (usize, usize) {
    let mut start = idx;
    while start > 0 && !is_token_separator(chars[start - 1]) {
        start -= 1;
    }
    let mut end = idx + 1;
    while end < chars.len() && !is_token_separator(chars[end]) {
        end += 1;
    }
    (start, end)
}

fn compute_emoji_protection_mask(chars: &[char]) -> Vec<bool> {
    let mut mask = vec![false; chars.len()];
    for (i, &ch) in chars.iter().enumerate() {
        if matches!(ch, '\u{FE0E}' | '\u{FE0F}')
            && i > 0
            && (is_valid_emoji_vs_base(chars[i - 1]) || is_extended_pictographic(chars[i - 1]))
        {
            mask[i] = true;
        }
    }
    for (i, &ch) in chars.iter().enumerate() {
        if ch == '\u{200D}' && i > 0 && i + 1 < chars.len() {
            let mut left = i - 1;
            while left > 0 && matches!(chars[left], '\u{FE0F}' | '\u{1F3FB}'..='\u{1F3FF}') {
                left -= 1;
            }
            let left_ok = is_extended_pictographic(chars[left]);
            let right_ok = is_extended_pictographic(chars[i + 1]);
            if left_ok && right_ok {
                mask[i] = true;
            }
        }
    }
    mask
}

/// Checks if a target single-component filename triggers any Windows/SMB/NAS hazard.
#[must_use]
pub fn has_windows_smb_hazard(name: &str) -> bool {
    !collect_windows_smb_hazards(name, UiLanguage::En).is_empty()
}

fn collect_windows_smb_hazards(name: &str, lang: UiLanguage) -> Vec<CharDetail> {
    let mut details = Vec::new();
    if name.is_empty() || name == "." || name == ".." {
        details.push(CharDetail {
            char_index: 0,
            glyph: None,
            code_point: "U+0000".to_string(),
            reason: match lang {
                UiLanguage::Ko => {
                    "빈 문자열이거나 예약된 디렉터리 경로명('.', '..')입니다.".to_string()
                }
                UiLanguage::En => "Filename cannot be empty, '.', or '..'.".to_string(),
            },
        });
        return details;
    }

    for (idx, ch) in name.chars().enumerate() {
        if matches!(ch, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') {
            details.push(CharDetail {
                char_index: idx,
                glyph: Some(ch.to_string()),
                code_point: format_code_point(ch),
                reason: match lang {
                    UiLanguage::Ko => format!("Windows/SMB 예약 금지 문자 '{}'", ch),
                    UiLanguage::En => format!("Windows/SMB reserved character '{ch}'"),
                },
            });
        } else if (ch as u32) <= 0x1F {
            details.push(CharDetail {
                char_index: idx,
                glyph: None,
                code_point: format_code_point(ch),
                reason: match lang {
                    UiLanguage::Ko => "ASCII 제어 문자(U+0000–U+001F)".to_string(),
                    UiLanguage::En => "ASCII control character (U+0000–U+001F)".to_string(),
                },
            });
        }
    }

    if let Some(last) = name.chars().last()
        && (last == ' ' || last == '.')
    {
        let idx = name.chars().count().saturating_sub(1);
        details.push(CharDetail {
            char_index: idx,
            glyph: Some(last.to_string()),
            code_point: format_code_point(last),
            reason: match lang {
                UiLanguage::Ko => "Windows/SMB 후행 공백 또는 마침표 금지".to_string(),
                UiLanguage::En => "Windows/SMB trailing space or period hazard".to_string(),
            },
        });
    }

    let stem = name.split('.').next().unwrap_or(name);
    let upper_stem = stem.trim_end_matches([' ', '.']).to_ascii_uppercase();
    if RESERVED_WINDOWS_DEVICES.contains(&upper_stem.as_str()) {
        details.push(CharDetail {
            char_index: 0,
            glyph: Some(stem.to_string()),
            code_point: "DEVICE".to_string(),
            reason: match lang {
                UiLanguage::Ko => format!("Windows 예약 장치 이름 '{upper_stem}'"),
                UiLanguage::En => format!("Windows reserved device name '{upper_stem}'"),
            },
        });
    }

    let (exceeds, utf8_bytes, utf16_units) = exceeds_cross_platform_name_limit(name);
    if exceeds {
        details.push(CharDetail {
            char_index: 0,
            glyph: None,
            code_point: "LEN>255".to_string(),
            reason: match lang {
                UiLanguage::Ko => format!(
                    "파일명 길이 제한 초과 (UTF-8 {utf8_bytes}B / UTF-16 {utf16_units} units > 255)"
                ),
                UiLanguage::En => format!(
                    "Filename exceeds 255-byte/unit limit (UTF-8 {utf8_bytes}B / UTF-16 {utf16_units} units)"
                ),
            },
        });
    }

    details
}

fn is_combining_mark(ch: char) -> bool {
    matches!(
        ch as u32,
        0x0300..=0x036F | 0x3099..=0x309A | 0x1AB0..=0x1AFF | 0x1DC0..=0x1DFF | 0x20D0..=0x20FF
    )
}

fn strip_compat_and_radicals_and_invisible(raw_name: &str, emoji_mask: &[bool]) -> String {
    let mut out = String::with_capacity(raw_name.len());
    for (idx, ch) in raw_name.chars().enumerate() {
        let protected = emoji_mask.get(idx).copied().unwrap_or(false);
        if is_invisible_control(ch) && !protected {
            continue;
        }
        if let Some(rep) = radical_compat_fix(ch) {
            out.push(rep);
        } else if let Some(canon) = compat_ideograph_canonical(ch) {
            out.push(canon);
        } else {
            out.push(ch);
        }
    }
    normalize_nfc(&out)
}

fn apply_single_category_fix(
    raw_name: &str,
    cat: IssueCategory,
    emoji_mask: &[bool],
) -> Option<String> {
    match cat {
        IssueCategory::InvisibleZeroWidth => {
            let mut out = String::with_capacity(raw_name.len());
            for (idx, ch) in raw_name.chars().enumerate() {
                if is_invisible_control(ch) && !emoji_mask.get(idx).copied().unwrap_or(false) {
                    continue;
                }
                out.push(ch);
            }
            Some(out)
        }
        IssueCategory::KangxiRadical => {
            let mut out = String::with_capacity(raw_name.len());
            let mut changed = false;
            for ch in raw_name.chars() {
                if let Some(rep) = radical_compat_fix(ch) {
                    out.push(rep);
                    changed = true;
                } else {
                    out.push(ch);
                }
            }
            changed.then_some(out)
        }
        IssueCategory::CompatibilityIdeograph => {
            let mut out = String::with_capacity(raw_name.len());
            for ch in raw_name.chars() {
                out.push(compat_ideograph_canonical(ch).unwrap_or(ch));
            }
            Some(out)
        }
        IssueCategory::NonNfcNormalization => Some(normalize_nfc(raw_name)),
        _ => None,
    }
}

struct FindingTarget<'a> {
    path: &'a Path,
    parent_dir: &'a Path,
    raw_name: &'a str,
    raw_os_name: &'a RawOsName,
    entry_type: EntryType,
}

#[allow(clippy::too_many_arguments)]
fn build_finding(
    target: FindingTarget<'_>,
    track: RemediationTrack,
    category: IssueCategory,
    severity: Severity,
    offending_chars: Vec<CharDetail>,
    suggested_name: Option<String>,
    candidate_hints: Vec<EncodingHint>,
    message: String,
) -> EntryFinding {
    EntryFinding {
        id: deterministic_finding_id(target.path, category, track),
        path: target.path.to_path_buf(),
        parent_dir: target.parent_dir.to_path_buf(),
        raw_name: target.raw_name.to_string(),
        raw_os_name: target.raw_os_name.clone(),
        entry_type: target.entry_type,
        track,
        category,
        severity,
        offending_chars,
        suggested_name,
        candidate_hints,
        message,
    }
}

/// Inspects a single directory entry and returns its findings and composed rename plan.
#[must_use]
pub fn inspect_entry(
    path: &Path,
    raw_os_name: &RawOsName,
    entry_type: EntryType,
    lang: UiLanguage,
) -> EntryDetectionResult {
    inspect_entry_with_ignore(path, raw_os_name, entry_type, lang, None)
}

/// Inspects a single directory entry applying `ignore_list` suppression before composing `EntryRenamePlan`.
#[must_use]
pub fn inspect_entry_with_ignore(
    path: &Path,
    raw_os_name: &RawOsName,
    entry_type: EntryType,
    lang: UiLanguage,
    ignore_list: Option<&crate::model::IgnoreList>,
) -> EntryDetectionResult {
    let parent_dir = path
        .parent()
        .map_or_else(|| path.to_path_buf(), Path::to_path_buf);

    let legacy_hints = detect_legacy_encoding_hints(raw_os_name);
    if !legacy_hints.is_empty() {
        let raw_name = match raw_os_name {
            RawOsName::UnixBytes(b) => String::from_utf8_lossy(b).into_owned(),
            RawOsName::WindowsWide(w) => String::from_utf16_lossy(w),
        };
        let mut msg = match lang {
            UiLanguage::Ko => {
                "UTF-8로 유효하지 않은 레거시 바이트 또는 짝 없는 UTF-16 서로게이트입니다. 후보 인코딩을 확인해 수동 변환하세요.".to_string()
            }
            UiLanguage::En => {
                "Invalid UTF-8 byte sequence or unpaired UTF-16 surrogate; review legacy decoding hints.".to_string()
            }
        };
        if let Some(first_hint) = legacy_hints.first() {
            let (exceeds, utf8_bytes, _) =
                exceeds_cross_platform_name_limit(&first_hint.decoded_name);
            if exceeds {
                msg.push_str(&format!(" (UTF-8 expanded length {utf8_bytes}B > 255B)"));
            }
        }
        let finding = build_finding(
            FindingTarget {
                path,
                parent_dir: &parent_dir,
                raw_name: &raw_name,
                raw_os_name,
                entry_type,
            },
            RemediationTrack::ManualReview,
            IssueCategory::InvalidUtf8,
            Severity::Error,
            vec![CharDetail {
                char_index: 0,
                glyph: None,
                code_point: "RAW-BYTES".to_string(),
                reason: msg.clone(),
            }],
            None,
            legacy_hints,
            msg,
        );
        let mut findings = vec![finding];
        if let Some(list) = ignore_list {
            findings.retain(|f| !crate::ignore_list::should_suppress_finding(list, f));
        }
        let (plan, updated_findings) =
            compose_rename_plan(path, &raw_name, raw_os_name, &findings, lang);
        return EntryDetectionResult {
            encoding_type: "Non-UTF-8 Legacy",
            findings: updated_findings,
            rename_plan: plan,
        };
    }

    let raw_name = match raw_os_name {
        RawOsName::UnixBytes(b) => String::from_utf8_lossy(b).into_owned(),
        RawOsName::WindowsWide(w) => String::from_utf16_lossy(w),
    };

    if raw_name.starts_with(".__nc_tmp_") && raw_name.len() > ".__nc_tmp_".len() {
        let msg = MessageCatalog::orphaned_temp_message(&raw_name, lang);
        let finding = build_finding(
            FindingTarget {
                path,
                parent_dir: &parent_dir,
                raw_name: &raw_name,
                raw_os_name,
                entry_type,
            },
            RemediationTrack::ManualReview,
            IssueCategory::OrphanedTempFile,
            Severity::Warning,
            vec![],
            None,
            vec![EncodingHint {
                encoding: "UndoRecovery".to_string(),
                decoded_name: "namechecker undo <AUDIT_LOG_FILE>".to_string(),
            }],
            msg,
        );
        let mut findings = vec![finding];
        if let Some(list) = ignore_list {
            findings.retain(|f| !crate::ignore_list::should_suppress_finding(list, f));
        }
        let (plan, updated_findings) =
            compose_rename_plan(path, &raw_name, raw_os_name, &findings, lang);
        return EntryDetectionResult {
            encoding_type: "UTF-8 NFC",
            findings: updated_findings,
            rename_plan: plan,
        };
    }

    if raw_name.is_ascii() {
        let smb_hazards = collect_windows_smb_hazards(&raw_name, lang);
        if smb_hazards.is_empty() {
            return EntryDetectionResult {
                encoding_type: "UTF-8 NFC",
                findings: Vec::new(),
                rename_plan: None,
            };
        }
        let mut findings = vec![build_finding(
            FindingTarget {
                path,
                parent_dir: &parent_dir,
                raw_name: &raw_name,
                raw_os_name,
                entry_type,
            },
            RemediationTrack::ManualReview,
            IssueCategory::WindowsSmbHazard,
            Severity::Error,
            smb_hazards,
            None,
            vec![],
            MessageCatalog::category_label(IssueCategory::WindowsSmbHazard, lang).to_string(),
        )];
        if let Some(list) = ignore_list {
            findings.retain(|f| !crate::ignore_list::should_suppress_finding(list, f));
        }
        let (plan, updated_findings) =
            compose_rename_plan(path, &raw_name, raw_os_name, &findings, lang);
        return EntryDetectionResult {
            encoding_type: "UTF-8 NFC",
            findings: updated_findings,
            rename_plan: plan,
        };
    }

    let chars: Vec<char> = raw_name.chars().collect();
    let emoji_mask = compute_emoji_protection_mask(&chars);

    let mut compat_chars = Vec::new();
    let mut kangxi_auto_chars = Vec::new();
    let mut kangxi_manual_chars = Vec::new();
    let mut kangxi_hints = Vec::new();
    let mut invisible_chars = Vec::new();
    let mut vs_chars = Vec::new();
    let mut pua_chars = Vec::new();
    let mut pua_hints = Vec::new();
    let mut repl_chars = Vec::new();
    let mut punct_chars = Vec::new();
    let mut punct_hints = Vec::new();
    let mut confusable_chars = Vec::new();
    let mut confusable_hints = Vec::new();

    for (idx, &ch) in chars.iter().enumerate() {
        let protected = emoji_mask[idx];

        if compat_ideograph_canonical(ch).is_some() {
            compat_chars.push(CharDetail {
                char_index: idx,
                glyph: Some(ch.to_string()),
                code_point: format_code_point(ch),
                reason: MessageCatalog::category_label(IssueCategory::CompatibilityIdeograph, lang)
                    .to_string(),
            });
        } else if radical_compat_fix(ch).is_some() {
            kangxi_auto_chars.push(CharDetail {
                char_index: idx,
                glyph: Some(ch.to_string()),
                code_point: format_code_point(ch),
                reason: MessageCatalog::category_label(IssueCategory::KangxiRadical, lang)
                    .to_string(),
            });
        } else if is_assigned_cjk_radical_supplement(ch) {
            kangxi_manual_chars.push(CharDetail {
                char_index: idx,
                glyph: Some(ch.to_string()),
                code_point: format_code_point(ch),
                reason: match lang {
                    UiLanguage::Ko => "CJK 부수 보충 문자",
                    UiLanguage::En => "CJK Radicals Supplement",
                }
                .to_string(),
            });
            if let Some(target) = radical_supplement_hint(ch) {
                kangxi_hints.push(EncodingHint {
                    encoding: "CJKRadicals.txt".to_string(),
                    decoded_name: raw_name.replacen(ch, &target.to_string(), 1),
                });
            }
        }

        if is_invisible_control(ch) && !protected {
            invisible_chars.push(CharDetail {
                char_index: idx,
                glyph: None,
                code_point: format_code_point(ch),
                reason: MessageCatalog::category_label(IssueCategory::InvisibleZeroWidth, lang)
                    .to_string(),
            });
        }

        if (matches!(ch, '\u{FE00}'..='\u{FE0D}' | '\u{E0100}'..='\u{E01EF}')
            || (matches!(ch, '\u{FE0E}' | '\u{FE0F}') && !protected))
        {
            vs_chars.push(CharDetail {
                char_index: idx,
                glyph: None,
                code_point: format_code_point(ch),
                reason: MessageCatalog::category_label(IssueCategory::VariationSelector, lang)
                    .to_string(),
            });
        }

        if matches!(
            ch as u32,
            0xE000..=0xF8FF | 0xF0000..=0xFFFFD | 0x100000..=0x10FFFD
        ) {
            pua_chars.push(CharDetail {
                char_index: idx,
                glyph: visible_glyph_for(ch),
                code_point: format_code_point(ch),
                reason: MessageCatalog::category_label(IssueCategory::PrivateUseArea, lang)
                    .to_string(),
            });
            if let Some(mapped) = sfm_pua_hint(ch) {
                pua_hints.push(EncodingHint {
                    encoding: "SFM-PUA".to_string(),
                    decoded_name: format!("{mapped} (macOS SMB SFM)"),
                });
            }
        }

        if ch == '\u{FFFD}' {
            repl_chars.push(CharDetail {
                char_index: idx,
                glyph: Some(ch.to_string()),
                code_point: format_code_point(ch),
                reason: MessageCatalog::category_label(IssueCategory::ReplacementChar, lang)
                    .to_string(),
            });
        }

        if matches!(ch, '\u{301C}' | '\u{FF5E}' | '\u{3000}') {
            punct_chars.push(CharDetail {
                char_index: idx,
                glyph: Some(ch.to_string()),
                code_point: format_code_point(ch),
                reason: MessageCatalog::category_label(IssueCategory::PunctuationVariant, lang)
                    .to_string(),
            });
            let alt = match ch {
                '\u{301C}' => raw_name.replacen(ch, "\u{FF5E}", 1),
                '\u{FF5E}' => raw_name.replacen(ch, "~", 1),
                '\u{3000}' => raw_name.replacen(ch, " ", 1),
                _ => raw_name.clone(),
            };
            punct_hints.push(EncodingHint {
                encoding: "Punctuation".to_string(),
                decoded_name: alt,
            });
        }

        // Context-Gated Confusable Detection
        if katakana_hanzi_confusable(ch).is_some() {
            let (t_start, t_end) = token_bounds(&chars, idx);
            let has_han_or_hangul = chars[t_start..t_end]
                .iter()
                .any(|&c| is_han_scalar(c) || is_hangul_scalar(c));
            let left_kana = idx > t_start && is_kana_scalar(chars[idx - 1]);
            let right_kana = idx + 1 < t_end && is_kana_scalar(chars[idx + 1]);
            if has_han_or_hangul && !left_kana && !right_kana {
                confusable_chars.push(CharDetail {
                    char_index: idx,
                    glyph: Some(ch.to_string()),
                    code_point: format_code_point(ch),
                    reason: match lang {
                        UiLanguage::Ko => "한자/한글 토큰 내부에 고립된 가타카나 유사 문자",
                        UiLanguage::En => "Katakana lookalike isolated inside Han/Hangul token",
                    }
                    .to_string(),
                });
                if let Some(han) = katakana_hanzi_confusable(ch) {
                    confusable_hints.push(EncodingHint {
                        encoding: "UAX#39-Confusable".to_string(),
                        decoded_name: raw_name.replacen(ch, &han.to_string(), 1),
                    });
                }
            }
        } else if let Some(skel) = script_homoglyph_hint(ch) {
            let (t_start, t_end) = token_bounds(&chars, idx);
            let mixed_with_latin_or_cjk = chars[t_start..t_end]
                .iter()
                .any(|&c| c.is_ascii_alphabetic() || is_han_scalar(c) || is_hangul_scalar(c));
            if mixed_with_latin_or_cjk {
                confusable_chars.push(CharDetail {
                    char_index: idx,
                    glyph: Some(ch.to_string()),
                    code_point: format_code_point(ch),
                    reason: match lang {
                        UiLanguage::Ko => format!("'{skel}'와 혼동 가능한 이종 문자"),
                        UiLanguage::En => format!("Cross-script homoglyph for '{skel}'"),
                    },
                });
                confusable_hints.push(EncodingHint {
                    encoding: "UAX#39-Homoglyph".to_string(),
                    decoded_name: raw_name.replacen(ch, &skel.to_string(), 1),
                });
            }
        } else if matches!(ch, '\u{309B}' | '\u{309C}' | '\u{FF9E}' | '\u{FF9F}') {
            confusable_chars.push(CharDetail {
                char_index: idx,
                glyph: Some(ch.to_string()),
                code_point: format_code_point(ch),
                reason: match lang {
                    UiLanguage::Ko => "분리형 또는 반각 가나 탁점/반탁점 기호",
                    UiLanguage::En => "Spacing or halfwidth Kana voiced/semi-voiced sound mark",
                }
                .to_string(),
            });
            if idx > 0
                && let Some(composed) = compose_kana_dakuten_hint(chars[idx - 1], ch)
            {
                let mut pair = String::new();
                pair.push(chars[idx - 1]);
                pair.push(ch);
                confusable_hints.push(EncodingHint {
                    encoding: "KanaDakuten".to_string(),
                    decoded_name: raw_name.replacen(&pair, &composed.to_string(), 1),
                });
            }
        } else if let Some(full) = halfwidth_kana_to_fullwidth(ch) {
            let followed_by_hw_mark =
                idx + 1 < chars.len() && matches!(chars[idx + 1], '\u{FF9E}' | '\u{FF9F}');
            if !followed_by_hw_mark {
                confusable_chars.push(CharDetail {
                    char_index: idx,
                    glyph: Some(ch.to_string()),
                    code_point: format_code_point(ch),
                    reason: match lang {
                        UiLanguage::Ko => "반각 가타카나 변형 문자",
                        UiLanguage::En => "Halfwidth Katakana variant",
                    }
                    .to_string(),
                });
                confusable_hints.push(EncodingHint {
                    encoding: "KanaFullwidth".to_string(),
                    decoded_name: raw_name.replacen(ch, &full.to_string(), 1),
                });
            }
        } else if ('\u{FF01}'..='\u{FF5D}').contains(&ch) {
            confusable_chars.push(CharDetail {
                char_index: idx,
                glyph: Some(ch.to_string()),
                code_point: format_code_point(ch),
                reason: match lang {
                    UiLanguage::Ko => "전각 ASCII 변형 문자",
                    UiLanguage::En => "Fullwidth ASCII variant",
                }
                .to_string(),
            });
            let is_forbidden_fullwidth = matches!(
                ch,
                '\u{FF02}'
                    | '\u{FF0A}'
                    | '\u{FF0F}'
                    | '\u{FF1A}'
                    | '\u{FF1C}'
                    | '\u{FF1E}'
                    | '\u{FF1F}'
                    | '\u{FF3C}'
                    | '\u{FF5C}'
            );
            if !is_forbidden_fullwidth
                && !confusable_hints
                    .iter()
                    .any(|h| h.encoding == "FullwidthAscii")
            {
                let candidate: String = raw_name
                    .chars()
                    .map(|c| {
                        if ('\u{FF01}'..='\u{FF5D}').contains(&c)
                            && !matches!(
                                c,
                                '\u{FF02}'
                                    | '\u{FF0A}'
                                    | '\u{FF0F}'
                                    | '\u{FF1A}'
                                    | '\u{FF1C}'
                                    | '\u{FF1E}'
                                    | '\u{FF1F}'
                                    | '\u{FF3C}'
                                    | '\u{FF5C}'
                            )
                        {
                            char::from_u32((c as u32) - 0xFEE0).unwrap_or(c)
                        } else {
                            c
                        }
                    })
                    .collect();
                if !candidate.is_empty()
                    && candidate != "."
                    && candidate != ".."
                    && !has_windows_smb_hazard(&candidate)
                {
                    confusable_hints.push(EncodingHint {
                        encoding: "FullwidthAscii".to_string(),
                        decoded_name: candidate,
                    });
                }
            }
        }
    }

    // Check Non-NFC Normalization (excluding single-char compatibility ideograph substitutions
    // and excluding non-composable remnants checked right after)
    let without_compat: String = raw_name
        .chars()
        .map(|c| compat_ideograph_canonical(c).unwrap_or(c))
        .collect();
    let nfc_after_compat = normalize_nfc(&without_compat);
    let is_utf8_nfc = is_normalized_nfc(&without_compat);
    let encoding_type = if is_utf8_nfc {
        "UTF-8 NFC"
    } else {
        "UTF-8 NFD"
    };

    let mut non_nfc_chars = Vec::new();
    if !is_utf8_nfc {
        for (idx, &ch) in chars.iter().enumerate() {
            if matches!(ch as u32, 0x1100..=0x11FF | 0xA960..=0xA97F | 0xD7B0..=0xD7FF | 0x3099..=0x309A | 0x0300..=0x036F)
                && !is_hangul_filler(ch)
            {
                non_nfc_chars.push(CharDetail {
                    char_index: idx,
                    glyph: visible_glyph_for(ch),
                    code_point: format_code_point(ch),
                    reason: MessageCatalog::category_label(
                        IssueCategory::NonNfcNormalization,
                        lang,
                    )
                    .to_string(),
                });
            }
        }
        if non_nfc_chars.is_empty() {
            non_nfc_chars.push(CharDetail {
                char_index: 0,
                glyph: None,
                code_point: "NFD".to_string(),
                reason: MessageCatalog::category_label(IssueCategory::NonNfcNormalization, lang)
                    .to_string(),
            });
        }
    }

    // Post-NFC residue check for isolated Conjoining/Halfwidth Hangul Jamo,
    // non-composable Kana dakuten, and UAX #15 defective combining sequences
    let nfc_chars: Vec<char> = nfc_after_compat.chars().collect();
    for (idx, &ch) in nfc_chars.iter().enumerate() {
        if is_assigned_isolated_jamo(ch) {
            confusable_chars.push(CharDetail {
                char_index: idx,
                glyph: Some(ch.to_string()),
                code_point: format_code_point(ch),
                reason: match lang {
                    UiLanguage::Ko => "고립된 조합형 또는 반각 한글 자모",
                    UiLanguage::En => "Isolated conjoining or halfwidth Hangul Jamo",
                }
                .to_string(),
            });
            if let Some(compat_jamo) = isolated_jamo_compat_hint(ch) {
                confusable_hints.push(EncodingHint {
                    encoding: "HangulJamo".to_string(),
                    decoded_name: nfc_after_compat.replacen(ch, &compat_jamo.to_string(), 1),
                });
            }
        } else if is_combining_mark(ch) && !is_hangul_filler(ch) {
            confusable_chars.push(CharDetail {
                char_index: idx,
                glyph: Some(ch.to_string()),
                code_point: format_code_point(ch),
                reason: match lang {
                    UiLanguage::Ko => "NFC 정규화 후에도 남은 비결합/결함 결합 문자",
                    UiLanguage::En => {
                        "Defective or non-composable combining mark remaining after NFC"
                    }
                }
                .to_string(),
            });
            if idx > 0
                && let Some(composed) = compose_kana_dakuten_hint(nfc_chars[idx - 1], ch)
            {
                let mut pair = String::new();
                pair.push(nfc_chars[idx - 1]);
                pair.push(ch);
                confusable_hints.push(EncodingHint {
                    encoding: "KanaDakuten".to_string(),
                    decoded_name: nfc_after_compat.replacen(&pair, &composed.to_string(), 1),
                });
            }
        }
    }

    let mut findings = Vec::new();
    let add_finding = |findings: &mut Vec<EntryFinding>,
                       track: RemediationTrack,
                       cat: IssueCategory,
                       sev: Severity,
                       details: Vec<CharDetail>,
                       hints: Vec<EncodingHint>| {
        if details.is_empty() {
            return;
        }
        let suggested = if track == RemediationTrack::AutoFixable {
            apply_single_category_fix(&raw_name, cat, &emoji_mask)
        } else {
            None
        };
        let msg = MessageCatalog::category_label(cat, lang).to_string();
        findings.push(build_finding(
            FindingTarget {
                path,
                parent_dir: &parent_dir,
                raw_name: &raw_name,
                raw_os_name,
                entry_type,
            },
            track,
            cat,
            sev,
            details,
            suggested,
            hints,
            msg,
        ));
    };

    add_finding(
        &mut findings,
        RemediationTrack::AutoFixable,
        IssueCategory::CompatibilityIdeograph,
        Severity::Warning,
        compat_chars,
        vec![],
    );
    add_finding(
        &mut findings,
        RemediationTrack::AutoFixable,
        IssueCategory::KangxiRadical,
        Severity::Warning,
        kangxi_auto_chars,
        vec![],
    );
    add_finding(
        &mut findings,
        RemediationTrack::ManualReview,
        IssueCategory::KangxiRadical,
        Severity::Warning,
        kangxi_manual_chars,
        kangxi_hints,
    );
    add_finding(
        &mut findings,
        RemediationTrack::AutoFixable,
        IssueCategory::NonNfcNormalization,
        Severity::Warning,
        non_nfc_chars,
        vec![],
    );
    add_finding(
        &mut findings,
        RemediationTrack::AutoFixable,
        IssueCategory::InvisibleZeroWidth,
        Severity::Warning,
        invisible_chars,
        vec![],
    );
    add_finding(
        &mut findings,
        RemediationTrack::ManualReview,
        IssueCategory::VariationSelector,
        Severity::Warning,
        vs_chars,
        vec![],
    );
    add_finding(
        &mut findings,
        RemediationTrack::ManualReview,
        IssueCategory::PrivateUseArea,
        Severity::Warning,
        pua_chars,
        pua_hints,
    );
    add_finding(
        &mut findings,
        RemediationTrack::ManualReview,
        IssueCategory::ReplacementChar,
        Severity::Error,
        repl_chars,
        vec![],
    );
    add_finding(
        &mut findings,
        RemediationTrack::ManualReview,
        IssueCategory::PunctuationVariant,
        Severity::Warning,
        punct_chars,
        punct_hints,
    );
    add_finding(
        &mut findings,
        RemediationTrack::ManualReview,
        IssueCategory::CrossScriptConfusable,
        Severity::Warning,
        confusable_chars,
        confusable_hints,
    );

    let smb_hazards = collect_windows_smb_hazards(&raw_name, lang);
    add_finding(
        &mut findings,
        RemediationTrack::ManualReview,
        IssueCategory::WindowsSmbHazard,
        Severity::Error,
        smb_hazards,
        vec![],
    );

    if let Some(list) = ignore_list {
        findings.retain(|f| !crate::ignore_list::should_suppress_finding(list, f));
    }

    let (rename_plan, final_findings) =
        compose_rename_plan(path, &raw_name, raw_os_name, &findings, lang);

    EntryDetectionResult {
        encoding_type,
        findings: final_findings,
        rename_plan,
    }
}

/// Re-computes `EntryRenamePlan` and Post-Composition Target Validation Gate over unsuppressed findings.
#[must_use]
pub fn compose_rename_plan(
    path: &Path,
    raw_name: &str,
    raw_os_name: &RawOsName,
    active_findings: &[EntryFinding],
    lang: UiLanguage,
) -> (Option<EntryRenamePlan>, Vec<EntryFinding>) {
    if active_findings.is_empty() {
        return (None, Vec::new());
    }

    let parent_dir = path
        .parent()
        .map_or_else(|| path.to_path_buf(), Path::to_path_buf);

    let has_non_draftable = active_findings.iter().any(|f| {
        matches!(
            f.category,
            IssueCategory::InvalidUtf8 | IssueCategory::OrphanedTempFile
        )
    });

    if has_non_draftable {
        let finding_ids = active_findings.iter().map(|f| f.id.clone()).collect();
        let plan = EntryRenamePlan {
            path: path.to_path_buf(),
            raw_name: raw_name.to_string(),
            raw_os_name: raw_os_name.clone(),
            composed_suggested_name: None,
            track: RemediationTrack::ManualReview,
            finding_ids,
        };
        return (Some(plan), active_findings.to_vec());
    }

    let mut updated: Vec<EntryFinding> = active_findings.to_vec();
    let chars: Vec<char> = raw_name.chars().collect();
    let emoji_mask = compute_emoji_protection_mask(&chars);

    let active_cats: BTreeSet<IssueCategory> = updated
        .iter()
        .filter(|f| f.track == RemediationTrack::AutoFixable)
        .map(|f| f.category)
        .collect();

    let draft_target = if active_cats.is_empty() {
        None
    } else {
        let mut s = raw_name.to_string();
        if active_cats.contains(&IssueCategory::InvisibleZeroWidth) {
            let mut stripped = String::with_capacity(s.len());
            for (idx, ch) in s.chars().enumerate() {
                if !(is_invisible_control(ch) && !emoji_mask.get(idx).copied().unwrap_or(false)) {
                    stripped.push(ch);
                }
            }
            s = stripped;
        }
        if active_cats.contains(&IssueCategory::KangxiRadical) {
            s = s
                .chars()
                .map(|c| radical_compat_fix(c).unwrap_or(c))
                .collect();
        }
        if active_cats.contains(&IssueCategory::CompatibilityIdeograph) {
            s = s
                .chars()
                .map(|c| compat_ideograph_canonical(c).unwrap_or(c))
                .collect();
        }
        s = normalize_nfc(&s);
        Some(s)
    };

    // Post-Composition Target Validation Gate (G1):
    // Check single-finding suggestions AND composed target for "", ".", "..", or WindowsSmbHazard
    let mut gate_triggered = false;
    let mut exposed_hazards = Vec::new();
    for f in &mut updated {
        if let Some(ref cand) = f.suggested_name {
            let hazards = collect_windows_smb_hazards(cand, lang);
            if !hazards.is_empty() {
                gate_triggered = true;
                exposed_hazards.extend(hazards);
                f.track = RemediationTrack::ManualReview;
                f.suggested_name = None;
            }
        }
    }

    let mut final_composed = if let Some(ref draft) = draft_target {
        let hazards = collect_windows_smb_hazards(draft, lang);
        if !hazards.is_empty() {
            gate_triggered = true;
            exposed_hazards.extend(hazards);
            None
        } else if draft == raw_name {
            None
        } else {
            Some(draft.clone())
        }
    } else {
        None
    };

    if gate_triggered {
        final_composed = None;
        for f in &mut updated {
            if f.track == RemediationTrack::AutoFixable {
                f.track = RemediationTrack::ManualReview;
                f.suggested_name = None;
            }
        }
        if !updated
            .iter()
            .any(|f| f.category == IssueCategory::WindowsSmbHazard)
            && !exposed_hazards.is_empty()
        {
            let entry_type = updated.first().map_or(EntryType::File, |f| f.entry_type);
            updated.push(build_finding(
                FindingTarget {
                    path,
                    parent_dir: &parent_dir,
                    raw_name,
                    raw_os_name,
                    entry_type,
                },
                RemediationTrack::ManualReview,
                IssueCategory::WindowsSmbHazard,
                Severity::Error,
                exposed_hazards,
                None,
                vec![],
                MessageCatalog::category_label(IssueCategory::WindowsSmbHazard, lang).to_string(),
            ));
        }
    }

    // Preserve strip helper usage for unused checks if needed; keep final_composed clean
    let _ = strip_compat_and_radicals_and_invisible;

    let has_manual = updated
        .iter()
        .any(|f| f.track == RemediationTrack::ManualReview);
    let plan_track = if has_manual || gate_triggered {
        RemediationTrack::ManualReview
    } else {
        RemediationTrack::AutoFixable
    };

    let finding_ids = updated.iter().map(|f| f.id.clone()).collect();
    let plan = EntryRenamePlan {
        path: path.to_path_buf(),
        raw_name: raw_name.to_string(),
        raw_os_name: raw_os_name.clone(),
        composed_suggested_name: final_composed,
        track: plan_track,
        finding_ids,
    };

    (Some(plan), updated)
}
