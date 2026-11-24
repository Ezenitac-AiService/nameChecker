//! Localized Korean (default) and English diagnostic and UI message catalog.

use crate::model::{DiscrepancyCause, IssueCategory, UiLanguage};

/// Localized labels and formatted explanations for reports and diagnostics.
pub struct MessageCatalog;

impl MessageCatalog {
    /// Returns localized human-readable category label (`ko` default, `en` alternative).
    #[must_use]
    pub const fn category_label(category: IssueCategory, lang: UiLanguage) -> &'static str {
        match (category, lang) {
            (IssueCategory::KangxiRadical, UiLanguage::Ko) => "강희자전/한자 부수 대체 문자",
            (IssueCategory::KangxiRadical, UiLanguage::En) => "Kangxi/CJK Radical Substitute",
            (IssueCategory::CompatibilityIdeograph, UiLanguage::Ko) => "CJK 호환 한자",
            (IssueCategory::CompatibilityIdeograph, UiLanguage::En) => {
                "CJK Compatibility Ideograph"
            }
            (IssueCategory::NonNfcNormalization, UiLanguage::Ko) => "비정규(NFD/혼용) 유니코드",
            (IssueCategory::NonNfcNormalization, UiLanguage::En) => "Non-NFC Unicode Normalization",
            (IssueCategory::InvisibleZeroWidth, UiLanguage::Ko) => "투명/제로폭/한글 채움 문자",
            (IssueCategory::InvisibleZeroWidth, UiLanguage::En) => {
                "Invisible/Zero-Width/Filler Character"
            }
            (IssueCategory::PunctuationVariant, UiLanguage::Ko) => "전각/물결표/문장부호 변형",
            (IssueCategory::PunctuationVariant, UiLanguage::En) => "Punctuation/Spacing Variant",
            (IssueCategory::InvalidUtf8, UiLanguage::Ko) => "비UTF-8 레거시 바이트/서로게이트",
            (IssueCategory::InvalidUtf8, UiLanguage::En) => "Invalid UTF-8 / Unpaired Surrogate",
            (IssueCategory::PrivateUseArea, UiLanguage::Ko) => "사용자 정의 영역(PUA/SFM)",
            (IssueCategory::PrivateUseArea, UiLanguage::En) => "Private Use Area (PUA/SFM)",
            (IssueCategory::VariationSelector, UiLanguage::Ko) => "이체자 선택자(IVS/SVS)",
            (IssueCategory::VariationSelector, UiLanguage::En) => "Variation Selector (IVS/SVS)",
            (IssueCategory::CrossScriptConfusable, UiLanguage::Ko) => {
                "혼동 가능 이종 문자/자모/탁점"
            }
            (IssueCategory::CrossScriptConfusable, UiLanguage::En) => {
                "Cross-Script Confusable / Jamo / Dakuten"
            }
            (IssueCategory::ReplacementChar, UiLanguage::Ko) => "손상 대체 문자 (U+FFFD)",
            (IssueCategory::ReplacementChar, UiLanguage::En) => "Replacement Character (U+FFFD)",
            (IssueCategory::WindowsSmbHazard, UiLanguage::Ko) => "Windows/SMB/NAS 이름 제한 위험",
            (IssueCategory::WindowsSmbHazard, UiLanguage::En) => "Windows/SMB/NAS Filename Hazard",
            (IssueCategory::OrphanedTempFile, UiLanguage::Ko) => "중단된 이름 변경 임시 파일",
            (IssueCategory::OrphanedTempFile, UiLanguage::En) => "Orphaned Rename Temp File",
        }
    }

    /// Formats a localized explanation for an `OrphanedTempFile` recovery prompt.
    #[must_use]
    pub fn orphaned_temp_message(entry_name: &str, lang: UiLanguage) -> String {
        match lang {
            UiLanguage::Ko => format!(
                "이전 이름 변경 작업이 중단되어 남은 임시 파일({entry_name})입니다. 감사 로그를 지정해 `namechecker undo` 또는 GUI `되돌리기`로 복구하세요."
            ),
            UiLanguage::En => format!(
                "Stranded temporary file ({entry_name}) from an interrupted two-step rename; run `namechecker undo <AUDIT_LOG_FILE>` or GUI Undo to recover."
            ),
        }
    }

    /// Returns localized explanation for a `SortDiscrepancy` cause.
    #[must_use]
    pub const fn discrepancy_reason(cause: DiscrepancyCause, lang: UiLanguage) -> &'static str {
        match (cause, lang) {
            (DiscrepancyCause::EncodingAnomaly, UiLanguage::Ko) => {
                "인코딩/정규화 이상으로 인해 플랫폼별 정렬 순서가 왜곡됨"
            }
            (DiscrepancyCause::EncodingAnomaly, UiLanguage::En) => {
                "Sort order distorted by character encoding or normalization anomaly"
            }
            (DiscrepancyCause::InherentPlatformCollation, UiLanguage::Ko) => {
                "OS/로케일 고유 정렬 규칙(스크립트 블록 또는 한자 독음 순서) 차이"
            }
            (DiscrepancyCause::InherentPlatformCollation, UiLanguage::En) => {
                "Inherent OS/locale collation rule difference (script-block or CJK reading order)"
            }
        }
    }

    /// Parses a `category_counts` map key back to its `IssueCategory` enum variant.
    #[must_use]
    pub fn parse_category_key(key: &str) -> Option<IssueCategory> {
        match key {
            "KangxiRadical" => Some(IssueCategory::KangxiRadical),
            "CompatibilityIdeograph" => Some(IssueCategory::CompatibilityIdeograph),
            "NonNfcNormalization" => Some(IssueCategory::NonNfcNormalization),
            "InvisibleZeroWidth" => Some(IssueCategory::InvisibleZeroWidth),
            "PunctuationVariant" => Some(IssueCategory::PunctuationVariant),
            "InvalidUtf8" => Some(IssueCategory::InvalidUtf8),
            "PrivateUseArea" => Some(IssueCategory::PrivateUseArea),
            "VariationSelector" => Some(IssueCategory::VariationSelector),
            "CrossScriptConfusable" => Some(IssueCategory::CrossScriptConfusable),
            "ReplacementChar" => Some(IssueCategory::ReplacementChar),
            "WindowsSmbHazard" => Some(IssueCategory::WindowsSmbHazard),
            "OrphanedTempFile" => Some(IssueCategory::OrphanedTempFile),
            _ => None,
        }
    }

    /// Returns localized report section header (`Auto-Fixable` / `Manual Review Required` / etc.).
    #[must_use]
    pub const fn section_heading(key: &str, lang: UiLanguage) -> &'static str {
        match (key.as_bytes(), lang) {
            (b"auto_fixable", UiLanguage::Ko) => "자동 수정 제안",
            (b"auto_fixable", UiLanguage::En) => "Auto-Fixable",
            (b"manual_review", UiLanguage::Ko) => "수동 확인 필요",
            (b"manual_review", UiLanguage::En) => "Manual Review Required",
            (b"sort_discrepancies", UiLanguage::Ko) => "크로스 플랫폼 정렬 불일치",
            (b"sort_discrepancies", UiLanguage::En) => "Sort Discrepancies",
            (b"collision_risks", UiLanguage::Ko) => "파일명 충돌 위험",
            (b"collision_risks", UiLanguage::En) => "Collision Risks",
            (b"io_errors", UiLanguage::Ko) => "접근/I/O 오류",
            (b"io_errors", UiLanguage::En) => "I/O Errors",
            (b"summary", UiLanguage::Ko) => "점검 요약",
            (b"summary", UiLanguage::En) => "Scan Summary",
            _ => "unknown",
        }
    }
}
