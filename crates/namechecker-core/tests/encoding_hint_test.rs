//! Triangulated unit tests for legacy CJK byte detection and cross-platform length limits (T011).

use encoding_rs::{EUC_KR, GB18030, SHIFT_JIS};
use namechecker_core::encoding_hint::{
    detect_legacy_encoding_hints, exceeds_cross_platform_name_limit,
};
use namechecker_core::model::RawOsName;

#[test]
fn detects_cp949_and_shift_jis_even_on_short_raw_byte_filenames() {
    // Positive: Korean CP949 ("가" = B0 A1, "한글" = C7 D1 B1 DB)
    let (short_cp949, _, _) = EUC_KR.encode("가");
    let raw_short = RawOsName::UnixBytes(short_cp949.into_owned());
    let hints_short = detect_legacy_encoding_hints(&raw_short);
    assert!(
        hints_short
            .iter()
            .any(|h| h.encoding == "CP949" && h.decoded_name == "가"),
        "expected CP949 hint for short byte sequence, got {hints_short:?}"
    );

    // Positive: Shift_JIS Japanese ("日本語" in Shift_JIS)
    let (sjis_bytes, _, _) = SHIFT_JIS.encode("日本語テスト.txt");
    let raw_sjis = RawOsName::UnixBytes(sjis_bytes.into_owned());
    let hints_sjis = detect_legacy_encoding_hints(&raw_sjis);
    assert!(
        hints_sjis
            .iter()
            .any(|h| h.decoded_name == "日本語テスト.txt"),
        "expected Shift_JIS decoded candidate, got {hints_sjis:?}"
    );

    // Positive: GB18030 Chinese
    let (gb_bytes, _, _) = GB18030.encode("中文测试文件.txt");
    let raw_gb = RawOsName::UnixBytes(gb_bytes.into_owned());
    let hints_gb = detect_legacy_encoding_hints(&raw_gb);
    assert!(
        hints_gb
            .iter()
            .any(|h| h.decoded_name == "中文测试文件.txt"),
        "expected GB18030 decoded candidate, got {hints_gb:?}"
    );
}

#[test]
fn handles_unpaired_windows_utf16_surrogates_and_valid_utf8_negative_case() {
    // Boundary / Positive: unpaired UTF-16 high surrogate on Windows NTFS
    let wide = RawOsName::WindowsWide(vec![0x0041, 0xD800, 0x002E, 0x0074, 0x0078, 0x0074]);
    let hints = detect_legacy_encoding_hints(&wide);
    assert!(
        !hints.is_empty(),
        "unpaired UTF-16 surrogate must produce diagnostic hint"
    );

    // Negative: valid UTF-8 and valid UTF-16 return empty hints
    let valid_utf8 = RawOsName::UnixBytes("정상한글파일.txt".as_bytes().to_vec());
    assert!(detect_legacy_encoding_hints(&valid_utf8).is_empty());

    let valid_utf16: Vec<u16> = "정상한글파일.txt".encode_utf16().collect();
    assert!(detect_legacy_encoding_hints(&RawOsName::WindowsWide(valid_utf16)).is_empty());
}

#[test]
fn flags_cp949_to_utf8_1_5x_expansion_crossing_255_byte_limit() {
    // 100 Hangul syllables = 200 bytes in CP949 (fits old CP949), but 300 bytes in UTF-8 (> 255 NAME_MAX!)
    let hundred_hangul = "가".repeat(100);
    let (cp949_bytes, _, _) = EUC_KR.encode(&hundred_hangul);
    assert_eq!(cp949_bytes.len(), 200);

    let hints = detect_legacy_encoding_hints(&RawOsName::UnixBytes(cp949_bytes.into_owned()));
    let cp949_hint = hints
        .iter()
        .find(|h| h.encoding == "CP949")
        .expect("CP949 hint present");
    let (exceeds, utf8_bytes, utf16_units) =
        exceeds_cross_platform_name_limit(&cp949_hint.decoded_name);
    assert!(exceeds);
    assert_eq!(utf8_bytes, 300);
    assert_eq!(utf16_units, 100);

    // Exact boundary: 255 bytes passes, 256 bytes fails
    let exact_255 = "a".repeat(255);
    assert!(!exceeds_cross_platform_name_limit(&exact_255).0);
    let over_256 = "a".repeat(256);
    assert!(exceeds_cross_platform_name_limit(&over_256).0);
}
