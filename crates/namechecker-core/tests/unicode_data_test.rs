//! Triangulated unit tests for pinned Unicode 16.0.0 UCD, UTS #51, UAX #39, and KS X 1001 lookups (T008).

use namechecker_core::unicode_data::{
    compat_ideograph_canonical, compose_kana_dakuten_hint, is_assigned_cjk_radical_supplement,
    is_extended_pictographic, is_hangul_filler, is_valid_emoji_vs_base, isolated_jamo_compat_hint,
    katakana_hanzi_confusable, korean_hanja_reading, normalize_nfc, normalize_nfd,
    radical_compat_fix, radical_supplement_hint, script_homoglyph_hint, sfm_pua_hint,
};

#[test]
fn uax15_12_standalone_exceptions_never_decompose_while_canonical_compat_ideographs_do() {
    // Positive canonical compatibility ideographs (BMP & SIP)
    assert_eq!(compat_ideograph_canonical('\u{F90A}'), Some('\u{91D1}')); // 金 -> 金
    assert_eq!(compat_ideograph_canonical('\u{2F800}'), Some('\u{4E3D}')); // 丽 -> 丽

    // Boundary: all 12 UAX #15 standalone unified ideographs MUST return None
    let twelve_exceptions = [
        '\u{FA0E}', '\u{FA0F}', '\u{FA11}', '\u{FA13}', '\u{FA14}', '\u{FA1F}', '\u{FA21}',
        '\u{FA23}', '\u{FA24}', '\u{FA27}', '\u{FA28}', '\u{FA29}',
    ];
    for ch in twelve_exceptions {
        assert_eq!(
            compat_ideograph_canonical(ch),
            None,
            "UAX #15 exception {ch:?} ({:X}) must not decompose",
            ch as u32
        );
    }

    // Negative: unassigned code points in Compatibility Ideographs block must return None
    assert_eq!(compat_ideograph_canonical('\u{FA6E}'), None);
    assert_eq!(compat_ideograph_canonical('\u{FAFF}'), None);
    assert_eq!(compat_ideograph_canonical('日'), None);
}

#[test]
fn kangxi_and_cjk_radicals_supplement_split_deterministically_and_skip_unassigned() {
    // Positive UCD <compat> mappings -> AutoFixable
    assert_eq!(radical_compat_fix('\u{2F47}'), Some('\u{65E5}')); // ⽇ -> 日
    assert_eq!(radical_compat_fix('\u{2E9F}'), Some('\u{6BCD}')); // ⺟ -> 母
    assert_eq!(radical_compat_fix('\u{2EF3}'), Some('\u{9F9F}')); // ⻳ -> 龟

    // Negative: unassigned slot U+2E9A must not appear in either table
    assert!(!is_assigned_cjk_radical_supplement('\u{2E9A}'));
    assert_eq!(radical_compat_fix('\u{2E9A}'), None);
    assert_eq!(radical_supplement_hint('\u{2E9A}'), None);

    // Positive: assigned CJKRadicals Supplement (2E85 without unified mapping, and 2EF2/2ED4 with CJKRadicals.txt hints)
    assert!(is_assigned_cjk_radical_supplement('\u{2E85}'));
    assert!(radical_compat_fix('\u{2E81}').is_none());
    assert_eq!(radical_supplement_hint('\u{2EF2}'), Some('\u{4E80}')); // ⻲ -> 亀
    assert_eq!(radical_supplement_hint('\u{2ED4}'), Some('\u{95E8}')); // ⻔ -> 门
}

#[test]
fn icu_normalizer_nfc_nfd_and_hangul_kana_rules_match_unicode_16() {
    let nfd_korean = normalize_nfd("가나다");
    assert_ne!(nfd_korean, "가나다");
    assert_eq!(normalize_nfc(&nfd_korean), "가나다");

    let nfd_kana = "\u{304B}\u{3099}"; // か + U+3099
    assert_eq!(normalize_nfc(nfd_kana), "が");
}

#[test]
fn uts51_emoji_sets_and_context_gated_confusables_prevent_false_positives() {
    // Positive UTS #51 checks for ❤️ (U+2764 U+FE0F) and 👨‍👩‍👧 (Extended_Pictographic around ZWJ)
    assert!(is_valid_emoji_vs_base('\u{2764}'));
    assert!(is_extended_pictographic('👨'));
    assert!(is_extended_pictographic('👩'));

    // Negative UTS #51 checks for CJK ideographs and ASCII
    assert!(!is_valid_emoji_vs_base('日'));
    assert!(!is_extended_pictographic('한'));

    // Context-gated confusable lookup tables
    assert_eq!(katakana_hanzi_confusable('エ'), Some('工'));
    assert_eq!(katakana_hanzi_confusable('ロ'), Some('口'));
    assert_eq!(katakana_hanzi_confusable('カ'), Some('力'));
    assert_eq!(katakana_hanzi_confusable('ー'), Some('一'));

    // Cyrillic / Greek homoglyphs
    assert_eq!(script_homoglyph_hint('\u{0430}'), Some('a')); // Cyrillic а -> a
    assert_eq!(script_homoglyph_hint('\u{03BF}'), Some('o')); // Greek ο -> o

    // Negative: standard ASCII alphanumerics MUST NOT appear in homoglyph table
    for ascii in ['0', 'O', '1', 'l', 'I'] {
        assert_eq!(script_homoglyph_hint(ascii), None);
    }
}

#[test]
fn kana_dakuten_hangul_jamo_fillers_sfm_and_ksx1001_lookups_work() {
    // Kana dakuten / handakuten hints
    assert_eq!(compose_kana_dakuten_hint('か', '\u{309B}'), Some('が'));
    assert_eq!(compose_kana_dakuten_hint('は', '\u{309C}'), Some('ぱ'));
    assert_eq!(
        compose_kana_dakuten_hint('\u{FF8A}', '\u{FF9E}'),
        Some('バ')
    ); // ﾊﾞ -> バ
    assert_eq!(
        compose_kana_dakuten_hint('\u{FF8A}', '\u{FF9F}'),
        Some('パ')
    ); // ﾊﾟ -> パ
    assert_eq!(compose_kana_dakuten_hint('か', '\u{3099}'), Some('が'));

    // Isolated / halfwidth Hangul Jamo -> Compatibility Jamo
    assert_eq!(isolated_jamo_compat_hint('\u{1100}'), Some('ㄱ'));
    assert_eq!(isolated_jamo_compat_hint('\u{1161}'), Some('ㅏ'));
    assert_eq!(isolated_jamo_compat_hint('\u{FFA1}'), Some('ㄱ'));
    assert_eq!(isolated_jamo_compat_hint('가'), None);

    // Hangul Fillers
    assert!(is_hangul_filler('\u{115F}'));
    assert!(is_hangul_filler('\u{1160}'));
    assert!(is_hangul_filler('\u{3164}'));
    assert!(is_hangul_filler('\u{FFA0}'));
    assert!(!is_hangul_filler(' '));

    // macOS SMB SFM PUA hints (Linux cifs_unicode.h / Apple smbfs SFM table)
    assert_eq!(sfm_pua_hint('\u{F021}'), Some("*"));
    assert_eq!(sfm_pua_hint('\u{F022}'), Some(":"));
    assert_eq!(sfm_pua_hint('\u{F028}'), Some(" "));
    assert_eq!(sfm_pua_hint('\u{F029}'), Some("."));

    // KS X 1001 + Unihan kHangul readings
    assert_eq!(korean_hanja_reading('日').1, Some("일"));
    assert_eq!(korean_hanja_reading('純').1, Some("순"));
    assert_eq!(korean_hanja_reading('鎖').1, Some("쇄"));
    assert_eq!(korean_hanja_reading('音').1, Some("음"));
    assert_eq!(korean_hanja_reading('春').1, Some("춘"));
    assert_eq!(korean_hanja_reading('花').1, Some("화"));
    assert!(korean_hanja_reading('日').0.is_some());
    assert_eq!(korean_hanja_reading('柊'), (None, Some("종"))); // KS X 1002 Unihan kHangul fallback
    assert_eq!(korean_hanja_reading('A'), (None, None));
}

#[test]
fn assigned_isolated_jamo_and_ext_i_han_classification() {
    use namechecker_core::detector::inspect_entry;
    use namechecker_core::model::{EntryType, IssueCategory, RawOsName, UiLanguage};
    use std::path::Path;

    // Katakana lookalike 'エ' isolated next to Extension I Han character U+2EBF0 must trigger CrossScriptConfusable
    let name = "\u{2EBF0}エ.txt";
    let res = inspect_entry(
        Path::new(name),
        &RawOsName::UnixBytes(name.as_bytes().to_vec()),
        EntryType::File,
        UiLanguage::En,
    );
    assert!(
        res.findings
            .iter()
            .any(|f| f.category == IssueCategory::CrossScriptConfusable),
        "expected U+2EBF0 Extension I to count as Han for confusable gating: {:?}",
        res.findings
    );
}

#[test]
fn unicode_16_0_0_normalization_parity_with_pinned_ucd() {
    use std::collections::HashSet;

    let raw = include_str!("../data/UnicodeData.txt");
    let mut assigned = HashSet::new();
    let mut range_start: Option<u32> = None;
    let mut canonical_decomps = Vec::new();

    for line in raw.lines() {
        if line.is_empty() {
            continue;
        }
        let mut fields = line.split(';');
        let cp = u32::from_str_radix(fields.next().unwrap(), 16).unwrap();
        let name = fields.next().unwrap();
        let _cat = fields.next().unwrap();
        let _ccc = fields.next().unwrap();
        let _bidi = fields.next().unwrap();
        let decomp = fields.next().unwrap();

        if name.ends_with(", First>") {
            range_start = Some(cp);
        } else if name.ends_with(", Last>") {
            let start = range_start.take().unwrap();
            for c in start..=cp {
                assigned.insert(c);
            }
        } else {
            assigned.insert(cp);
        }

        if !decomp.is_empty() && !decomp.starts_with('<') {
            canonical_decomps.push(cp);
        }
    }

    // 1. Canonical decompositions present in UnicodeData.txt 16.0.0 must decompose under normalize_nfd
    for cp in canonical_decomps {
        if let Some(ch) = char::from_u32(cp) {
            let s = ch.to_string();
            assert_ne!(
                normalize_nfd(&s),
                s,
                "expected U+{cp:04X} from Unicode 16.0.0 to decompose in NFD"
            );
        }
    }

    // 2. Code points UNASSIGNED in UnicodeData.txt 16.0.0 must NOT decompose, compose,
    // or carry non-zero Canonical_Combining_Class under Unicode 16.0.0 normalization rules.
    let mut skew_points = Vec::new();
    for cp in 0x0080u32..=0x323AF {
        if assigned.contains(&cp) {
            continue;
        }
        if let Some(ch) = char::from_u32(cp) {
            let s = ch.to_string();
            let pair = format!("\u{0345}{ch}");
            if normalize_nfd(&s) != s
                || normalize_nfc(&s) != s
                || normalize_nfd(&pair) != pair
                || normalize_nfc(&pair) != pair
            {
                skew_points.push(format!("U+{cp:04X}"));
            }
        }
    }
    assert!(
        skew_points.is_empty(),
        "found post-16.0.0 scalars normalizing under runtime icu_normalizer: {:?}",
        &skew_points[..skew_points.len().min(10)]
    );
}
