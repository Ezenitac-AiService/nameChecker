use std::cmp::Ordering;
use std::collections::HashSet;
use std::path::Path;

use namechecker_core::collation::{compare_entries, detect_sort_discrepancies, sort_names};
use namechecker_core::detector::inspect_entry;
use namechecker_core::model::{
    CollationProfile, DiscrepancyCause, EntryType, RawOsName, UiLanguage,
};

#[test]
fn windows_ko_kr_hierarchy_and_word_sort_logic() {
    // Whitespace -> Symbols/Punctuation -> Numbers -> Latin -> Hangul+Hanja -> Kana
    let mut items = vec![
        "あう".to_string(),
        "日".to_string(),
        "가".to_string(),
        "A".to_string(),
        "10".to_string(),
        "2".to_string(),
        "01".to_string(),
        "1".to_string(),
        "~note".to_string(),
        "_under".to_string(),
        "[tag]".to_string(),
        "(paren)".to_string(),
        "!bang".to_string(),
        " leading".to_string(),
    ];
    sort_names(CollationProfile::WindowsKoKr, &mut items);

    assert_eq!(items[0], " leading");
    let a_pos = items.iter().position(|s| s == "A").unwrap();
    let ga_pos = items.iter().position(|s| s == "가").unwrap();
    let il_pos = items.iter().position(|s| s == "日").unwrap();
    let kana_pos = items.iter().position(|s| s == "あう").unwrap();
    let n01_pos = items.iter().position(|s| s == "01").unwrap();
    let n1_pos = items.iter().position(|s| s == "1").unwrap();
    let n2_pos = items.iter().position(|s| s == "2").unwrap();
    let n10_pos = items.iter().position(|s| s == "10").unwrap();
    let sym_pos = items.iter().position(|s| s == "_under").unwrap();

    assert!(sym_pos < n01_pos);
    assert!(n01_pos < n1_pos);
    assert!(n1_pos < n2_pos);
    assert!(n2_pos < n10_pos);
    assert!(n10_pos < a_pos);
    assert!(a_pos < ga_pos);
    assert!(ga_pos < il_pos);
    assert!(il_pos < kana_pos);

    // Pure Hangul + KS X 1001 Hanja ordering: 純(순) -> 애니포럼 -> 日(일) -> 화보집
    let mut ko_hanja = vec![
        "화보집".to_string(),
        "日".to_string(),
        "純".to_string(),
        "애니포럼".to_string(),
    ];
    sort_names(CollationProfile::WindowsKoKr, &mut ko_hanja);
    assert_eq!(ko_hanja, vec!["純", "애니포럼", "日", "화보집"]);

    // Win32 StrCmpLogicalW Word-Sort secondary weighting for '-' and '\''
    let mut hyphen_items = vec![
        "ac".to_string(),
        "a-b".to_string(),
        "ab".to_string(),
        "file's".to_string(),
        "filet".to_string(),
        "files".to_string(),
    ];
    sort_names(CollationProfile::WindowsKoKr, &mut hyphen_items);
    let ab = hyphen_items.iter().position(|s| s == "ab").unwrap();
    let a_b = hyphen_items.iter().position(|s| s == "a-b").unwrap();
    let ac = hyphen_items.iter().position(|s| s == "ac").unwrap();
    assert!(
        ab < a_b && a_b < ac,
        "hyphen order failed: {hyphen_items:?}"
    );

    let files = hyphen_items.iter().position(|s| s == "files").unwrap();
    let file_s = hyphen_items.iter().position(|s| s == "file's").unwrap();
    let filet = hyphen_items.iter().position(|s| s == "filet").unwrap();
    assert!(
        files < file_s && file_s < filet,
        "apostrophe order failed: {hyphen_items:?}"
    );

    // Overflow-free >40-digit integer comparison (beyond u64 and u128)
    let huge_small = format!("doc_{}.txt", "9".repeat(45));
    let huge_large = format!("doc_1{}.txt", "0".repeat(45));
    assert_eq!(
        compare_entries(CollationProfile::WindowsKoKr, &huge_small, &huge_large),
        Ordering::Less
    );
}

#[test]
fn macos_ko_and_other_collation_profiles() {
    // MacOsKo: Hangul+Hanja -> Latin -> Kana
    let mut basic = vec!["A".to_string(), "日".to_string(), "가".to_string()];
    sort_names(CollationProfile::MacOsKo, &mut basic);
    assert_eq!(basic, vec!["가", "日", "A"]);

    // CLDR ko [reorder Hang Hani] complex set
    let mut real_ko = vec![
        "あう".to_string(),
        "non".to_string(),
        "花".to_string(),
        "春".to_string(),
        "柊".to_string(),
        "音".to_string(),
        "유".to_string(),
        "슈퍼".to_string(),
        "鎖".to_string(),
        "鹿".to_string(),
    ];
    sort_names(CollationProfile::MacOsKo, &mut real_ko);
    assert_eq!(
        real_ko,
        vec![
            "鹿", "鎖", "슈퍼", "유", "音", "柊", "春", "花", "non", "あう"
        ]
    );

    // LinuxCodePoint: UTF-8 code-point order
    let mut linux = vec!["가".to_string(), "日".to_string(), "A".to_string()];
    sort_names(CollationProfile::LinuxCodePoint, &mut linux);
    assert_eq!(linux, vec!["A", "日", "가"]);

    // Japanese: numeric ordering + kana before latin or CJK per CLDR ja
    assert_eq!(
        compare_entries(CollationProfile::Japanese, "file2.txt", "file10.txt"),
        Ordering::Less
    );
    assert_eq!(
        compare_entries(CollationProfile::Japanese, "あいう", "かきく"),
        Ordering::Less
    );

    // Chinese: pinyin/locale-aware + numeric ordering
    assert_eq!(
        compare_entries(CollationProfile::Chinese, "doc2.txt", "doc10.txt"),
        Ordering::Less
    );
    assert_eq!(
        compare_entries(CollationProfile::Chinese, "安", "中"),
        Ordering::Less
    );
}

#[test]
fn discrepancy_cause_classification_and_post_fix_convergence() {
    let dir = Path::new("/tmp/test_collation");

    // 1. Clean folder: only InherentPlatformCollation discrepancies (no EncodingAnomaly)
    let clean_entries = ["A.txt", "가.txt", "日.txt"];
    let clean_issues: HashSet<&str> = HashSet::new();
    let clean_discrepancies = detect_sort_discrepancies(
        dir,
        &clean_entries,
        &clean_issues,
        CollationProfile::WindowsKoKr,
        &CollationProfile::all(),
        UiLanguage::En,
    );
    assert!(
        !clean_discrepancies.is_empty(),
        "clean A/가/日 must show platform inherent divergence"
    );
    assert!(
        clean_discrepancies
            .iter()
            .all(|d| d.cause == DiscrepancyCause::InherentPlatformCollation)
    );

    // 2. Encoding anomalies diverge as EncodingAnomaly before fix, converge after EntryRenamePlan
    let nfd_ha = "\u{1112}\u{1161}"; // NFD "하"
    let compat_cha = "\u{F902}"; // Compatibility ideograph for 車 (차)
    let zw_ga = "\u{200B}가"; // Leading zero-width space before 가
    let kana_nfd = "は\u{3099}"; // NFD "ば"

    let dirty_entries = [nfd_ha, "나", compat_cha, "한", zw_ga, "b", kana_nfd, "はあ"];
    let mut dirty_issue_names = HashSet::new();
    let mut fixed_names = Vec::new();
    for raw in dirty_entries {
        let raw_os = RawOsName::UnixBytes(raw.as_bytes().to_vec());
        let res = inspect_entry(&dir.join(raw), &raw_os, EntryType::File, UiLanguage::En);
        assert!(
            res.findings.is_empty() == (raw == "나" || raw == "한" || raw == "b" || raw == "はあ"),
            "unexpected findings for {raw:?}: {:?}",
            res.findings
        );
        if !res.findings.is_empty() {
            dirty_issue_names.insert(raw);
        }
        let next = res
            .rename_plan
            .and_then(|p| p.composed_suggested_name)
            .unwrap_or_else(|| raw.to_string());
        fixed_names.push(next);
    }

    let dirty_discrepancies = detect_sort_discrepancies(
        dir,
        &dirty_entries,
        &dirty_issue_names,
        CollationProfile::WindowsKoKr,
        &[
            CollationProfile::WindowsKoKr,
            CollationProfile::LinuxCodePoint,
        ],
        UiLanguage::En,
    );
    assert!(
        dirty_discrepancies
            .iter()
            .any(|d| d.cause == DiscrepancyCause::EncodingAnomaly),
        "expected EncodingAnomaly discrepancies before fix: {dirty_discrepancies:?}"
    );

    // After applying EntryRenamePlan fixes cleanly, WindowsKoKr vs LinuxCodePoint converges with zero EncodingAnomaly
    let fixed_refs: Vec<&str> = fixed_names.iter().map(String::as_str).collect();
    let no_issues: HashSet<&str> = HashSet::new();
    let post_discrepancies = detect_sort_discrepancies(
        dir,
        &fixed_refs,
        &no_issues,
        CollationProfile::WindowsKoKr,
        &[
            CollationProfile::WindowsKoKr,
            CollationProfile::LinuxCodePoint,
        ],
        UiLanguage::En,
    );
    assert!(
        post_discrepancies
            .iter()
            .all(|d| d.cause != DiscrepancyCause::EncodingAnomaly),
        "EncodingAnomaly persisted after fix: {post_discrepancies:?}"
    );
    // Clean Hangul+ASCII+Kana convergence check on WindowsKoKr vs LinuxCodePoint:
    // Fixed set: "하", "나", "車", "한", "가", "b", "ば", "はあ" -> WindowsKoKr puts Latin "b" before Hangul, LinuxCodePoint puts Latin "b" before "はあ"/"車"/"가", wait let's specifically verify Kangxi/Compat/NFD order converges!
    assert_eq!(
        compare_entries(CollationProfile::WindowsKoKr, "車", "한"),
        compare_entries(CollationProfile::LinuxCodePoint, "車", "한")
    );
    assert_eq!(
        compare_entries(CollationProfile::WindowsKoKr, "나", "하"),
        compare_entries(CollationProfile::LinuxCodePoint, "나", "하")
    );

    // 3. Negative clean-order case: zero discrepancies across all 5 profiles
    let uniform = ["01_alpha.txt", "02_beta.txt", "10_gamma.txt"];
    let empty = detect_sort_discrepancies(
        dir,
        &uniform,
        &clean_issues,
        CollationProfile::WindowsKoKr,
        &CollationProfile::all(),
        UiLanguage::Ko,
    );
    assert!(
        empty.is_empty(),
        "uniform numeric ASCII list must have 0 discrepancies: {empty:?}"
    );
}

#[cfg(target_os = "macos")]
#[test]
fn macos_native_finder_parity_sanity() {
    use core::ffi::{c_char, c_void};
    use std::ffi::CString;

    #[link(name = "Foundation", kind = "framework")]
    #[link(name = "objc")]
    unsafe extern "C" {
        fn objc_getClass(name: *const c_char) -> *const c_void;
        fn sel_registerName(name: *const c_char) -> *const c_void;
        fn objc_msgSend();
    }

    fn nsstring(s: &str) -> *const c_void {
        let cls = unsafe { objc_getClass(c"NSString".as_ptr()) };
        let sel = unsafe { sel_registerName(c"stringWithUTF8String:".as_ptr()) };
        let cstr = CString::new(s).unwrap();
        let f: unsafe extern "C" fn(*const c_void, *const c_void, *const c_char) -> *const c_void =
            unsafe { std::mem::transmute(objc_msgSend as *const c_void) };
        unsafe { f(cls, sel, cstr.as_ptr()) }
    }

    fn localized_standard_compare(a: &str, b: &str) -> Ordering {
        let sa = nsstring(a);
        let sb = nsstring(b);
        let sel = unsafe { sel_registerName(c"localizedStandardCompare:".as_ptr()) };
        let f: unsafe extern "C" fn(*const c_void, *const c_void, *const c_void) -> isize =
            unsafe { std::mem::transmute(objc_msgSend as *const c_void) };
        let res = unsafe { f(sa, sel, sb) };
        match res {
            x if x < 0 => Ordering::Less,
            x if x > 0 => Ordering::Greater,
            _ => Ordering::Equal,
        }
    }

    let sample = [
        "あう",
        "non",
        "花",
        "春",
        "柊",
        "音",
        "유",
        "슈퍼",
        "鎖",
        "鹿",
        "file10.txt",
        "file2.txt",
        "A",
        "日",
        "가",
    ];
    let mut ours: Vec<String> = sample.iter().map(|s| (*s).to_string()).collect();
    sort_names(CollationProfile::MacOsKo, &mut ours);

    let mut apple_native: Vec<String> = sample.iter().map(|s| (*s).to_string()).collect();
    apple_native.sort_by(|a, b| localized_standard_compare(a, b));

    assert_eq!(
        ours, apple_native,
        "MacOsKo diverged from macOS localizedStandardCompare:"
    );
}

#[cfg(windows)]
#[test]
fn windows_native_explorer_parity_sanity() {
    use windows_sys::Win32::Globalization::{
        CSTR_GREATER_THAN, CSTR_LESS_THAN, CompareStringEx, SORT_DIGITSASNUMBERS,
    };

    let wide = |s: &str| -> Vec<u16> { s.encode_utf16().collect() };
    let loc = wide("ko-KR\0");
    let win_cmp = |a: &str, b: &str| -> Ordering {
        let wa = wide(a);
        let wb = wide(b);
        let res = unsafe {
            CompareStringEx(
                loc.as_ptr(),
                SORT_DIGITSASNUMBERS,
                wa.as_ptr(),
                wa.len() as i32,
                wb.as_ptr(),
                wb.len() as i32,
                std::ptr::null(),
                std::ptr::null(),
                0,
            )
        };
        if res == CSTR_LESS_THAN {
            Ordering::Less
        } else if res == CSTR_GREATER_THAN {
            Ordering::Greater
        } else {
            Ordering::Equal
        }
    };

    for pair in [
        ("A", "가"),
        ("가", "日"),
        ("純", "애니포럼"),
        ("애니포럼", "日"),
        ("日", "화보집"),
        ("ab", "a-b"),
        ("a-b", "ac"),
        ("file2.txt", "file10.txt"),
    ] {
        assert_eq!(
            compare_entries(CollationProfile::WindowsKoKr, pair.0, pair.1),
            win_cmp(pair.0, pair.1),
            "WindowsKoKr parity mismatch for {pair:?}"
        );
    }
}

#[test]
fn windows_ko_kr_routes_cjk_extensions_f_g_h_i_into_hanja_block() {
    // Ext F (U+2CEB0), Ext I (U+2EBF0), Ext G (U+30000), Ext H (U+31350) must all sort after Latin Z and before Kana あ
    for ch in ['\u{2CEB0}', '\u{2EBF0}', '\u{30000}', '\u{31350}'] {
        let s = ch.to_string();
        assert_eq!(
            compare_entries(CollationProfile::WindowsKoKr, "Z", &s),
            Ordering::Less,
            "{ch:?} must sort after Latin Z in Hangul/Hanja block"
        );
        assert_eq!(
            compare_entries(CollationProfile::WindowsKoKr, &s, "あ"),
            Ordering::Less,
            "{ch:?} must sort before Kana あ in Hangul/Hanja block"
        );
    }
}
