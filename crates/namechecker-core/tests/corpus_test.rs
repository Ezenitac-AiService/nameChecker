//! Triangulated corpus tests covering 100+ CJK anomalies (SC-001), 1,000 clean NFC files (SC-002),
//! UTS #51 emoji preservation, Context-Gated Confusable Detection, and Post-Composition Safety Gate (T010).

use std::path::PathBuf;

use namechecker_core::detector::inspect_entry;
use namechecker_core::model::{EntryType, IssueCategory, RawOsName, RemediationTrack, UiLanguage};
use namechecker_core::unicode_data::normalize_nfd;

fn inspect_utf8(name: &str) -> namechecker_core::detector::EntryDetectionResult {
    inspect_entry(
        &PathBuf::from(format!("/tmp/{name}")),
        &RawOsName::UnixBytes(name.as_bytes().to_vec()),
        EntryType::File,
        UiLanguage::Ko,
    )
}

#[test]
fn sc001_detects_over_100_diverse_cjk_encoding_and_hazard_anomalies() {
    let mut injected_cases: Vec<(String, IssueCategory)> = Vec::new();

    // 25 Kangxi Radicals (U+2F00..U+2FD5) + U+2E9F + U+2EF3 -> AutoFixable
    for code in 0x2F00u32..0x2F19u32 {
        let ch = char::from_u32(code).expect("kangxi");
        injected_cases.push((format!("부수_{ch}.txt"), IssueCategory::KangxiRadical));
    }
    injected_cases.push(("모친_\u{2E9F}.txt".into(), IssueCategory::KangxiRadical));
    injected_cases.push(("거북_\u{2EF3}.txt".into(), IssueCategory::KangxiRadical));
    injected_cases.push(("간체문_\u{2ED4}.txt".into(), IssueCategory::KangxiRadical));

    // 25 Canonical CJK Compatibility Ideographs (excluding UAX #15 12 exceptions) -> AutoFixable
    for code in 0xF900u32..0xF919u32 {
        let ch = char::from_u32(code).expect("compat");
        injected_cases.push((
            format!("호환_{ch}.txt"),
            IssueCategory::CompatibilityIdeograph,
        ));
    }

    // 20 Non-NFC Hangul & Kana NFD names -> AutoFixable
    for i in 0..10 {
        let nfd = normalize_nfd(&format!("한글문서_{i}.txt"));
        injected_cases.push((nfd, IssueCategory::NonNfcNormalization));
    }
    for i in 0..10 {
        injected_cases.push((
            format!("か\u{3099}ぞう_{i}.png"),
            IssueCategory::NonNfcNormalization,
        ));
    }

    // 10 Invisible / Zero-Width / Hangul Fillers -> AutoFixable (when safe after strip)
    for (i, ch) in [
        '\u{200B}', '\u{FEFF}', '\u{00AD}', '\u{200C}', '\u{200D}', '\u{202E}', '\u{115F}',
        '\u{1160}', '\u{3164}', '\u{FFA0}',
    ]
    .into_iter()
    .enumerate()
    {
        injected_cases.push((
            format!("투명{ch}파일_{i}.txt"),
            IssueCategory::InvisibleZeroWidth,
        ));
    }

    // Punctuation variants, IVS/SVS, PUA/SFM, U+FFFD
    injected_cases.push((
        "파도\u{301C}물결.txt".into(),
        IssueCategory::PunctuationVariant,
    ));
    injected_cases.push((
        "전각\u{FF5E}틸드.txt".into(),
        IssueCategory::PunctuationVariant,
    ));
    injected_cases.push((
        "전각\u{3000}공백.txt".into(),
        IssueCategory::PunctuationVariant,
    ));
    injected_cases.push((
        "한자이체자_\u{8FBA}\u{E0100}.txt".into(),
        IssueCategory::VariationSelector,
    ));
    injected_cases.push((
        "한글VS16_가\u{FE0F}.txt".into(),
        IssueCategory::VariationSelector,
    ));
    injected_cases.push((
        "개인문자_\u{E000}.txt".into(),
        IssueCategory::PrivateUseArea,
    ));
    injected_cases.push(("SFM별표_\u{F021}.txt".into(), IssueCategory::PrivateUseArea));
    injected_cases.push(("SFM콜론_\u{F022}.txt".into(), IssueCategory::PrivateUseArea));
    injected_cases.push(("SFM점_\u{F029}".into(), IssueCategory::PrivateUseArea));
    injected_cases.push((
        "깨진\u{FFFD}문자.txt".into(),
        IssueCategory::ReplacementChar,
    ));

    // Context-gated cross-script confusables, Jamo, Kana dakuten
    injected_cases.push(("エ作.txt".into(), IssueCategory::CrossScriptConfusable));
    injected_cases.push(("화보ー집.txt".into(), IssueCategory::CrossScriptConfusable));
    injected_cases.push((
        "p\u{0430}ss.txt".into(),
        IssueCategory::CrossScriptConfusable,
    ));
    injected_cases.push((
        "자모_\u{1100}가.txt".into(),
        IssueCategory::CrossScriptConfusable,
    ));
    injected_cases.push((
        "반각자모_\u{FFA1}.txt".into(),
        IssueCategory::CrossScriptConfusable,
    ));
    injected_cases.push((
        "탁점_か\u{309B}.txt".into(),
        IssueCategory::CrossScriptConfusable,
    ));
    injected_cases.push((
        "반각탁점_\u{FF8A}\u{FF9E}.txt".into(),
        IssueCategory::CrossScriptConfusable,
    ));
    injected_cases.push((
        "불완전_あ\u{3099}.txt".into(),
        IssueCategory::CrossScriptConfusable,
    ));

    // Windows / SMB hazards & Orphaned temp file
    injected_cases.push(("CON.txt".into(), IssueCategory::WindowsSmbHazard));
    injected_cases.push(("nul.mp3".into(), IssueCategory::WindowsSmbHazard));
    injected_cases.push(("trailing_space ".into(), IssueCategory::WindowsSmbHazard));
    injected_cases.push(("trailing_dot.".into(), IssueCategory::WindowsSmbHazard));
    injected_cases.push(("bad:name?.txt".into(), IssueCategory::WindowsSmbHazard));
    injected_cases.push(("¡special.txt".into(), IssueCategory::PunctuationVariant)); // or allowed? Wait check special punctuation requirement!
    injected_cases.push((
        ".__nc_tmp_550e8400-e29b-41d4-a716-446655440000".into(),
        IssueCategory::OrphanedTempFile,
    ));

    // Verify >= 100 injected anomalies detected cleanly (exclude optional non-anomalous special punctuation before checking!)
    // Actually wait: is `¡` supposed to be clean or flagged? Wait: in corpus tests check special chars don't crash!
    // Remove `¡special.txt` from injected anomaly list so we test 100+ true anomalies cleanly, and test `¡` separately as clean punctuation or boundary.
    injected_cases.retain(|(name, _)| !name.starts_with('¡'));
    assert!(
        injected_cases.len() >= 100,
        "SC-001 requires 100+ anomalous entries, got {}",
        injected_cases.len()
    );

    for (name, expected_cat) in &injected_cases {
        let res = inspect_utf8(name);
        assert!(
            res.findings.iter().any(|f| f.category == *expected_cat),
            "expected category {expected_cat:?} for {name:?}, got {:?}",
            res.findings
        );
    }
}

#[test]
fn detects_multi_finding_composition_and_mixed_track_safety() {
    // Pure AutoFixable composition: NFD Hangul + Kangxi Radical + Canonical Compat + Zero-Width
    let mixed_auto = format!("{}_\u{2F47}_\u{F90A}\u{200B}.txt", normalize_nfd("가나다"));
    let res = inspect_utf8(&mixed_auto);
    let plan = res.rename_plan.expect("rename plan present");
    assert_eq!(plan.track, RemediationTrack::AutoFixable);
    assert_eq!(
        plan.composed_suggested_name.as_deref(),
        Some("가나다_日_金.txt")
    );
    assert!(plan.finding_ids.len() >= 4);

    // Mixed AutoFixable + ManualReview -> MUST resolve deterministically to ManualReview while keeping AutoFixable draft
    let mixed_manual = format!("{}_エ作.txt", normalize_nfd("작품"));
    let res_manual = inspect_utf8(&mixed_manual);
    let plan_manual = res_manual.rename_plan.expect("rename plan");
    assert_eq!(plan_manual.track, RemediationTrack::ManualReview);
    assert_eq!(
        plan_manual.composed_suggested_name.as_deref(),
        Some("작품_エ作.txt")
    );
}

#[test]
fn post_composition_target_validation_gate_blocks_empty_and_smb_hazards() {
    // Empty after stripping zero-width
    let empty_strip = inspect_utf8("\u{200B}\u{FEFF}");
    let plan_empty = empty_strip.rename_plan.expect("plan");
    assert_eq!(plan_empty.track, RemediationTrack::ManualReview);
    assert_eq!(plan_empty.composed_suggested_name, None);
    assert!(
        empty_strip
            .findings
            .iter()
            .any(|f| f.category == IssueCategory::WindowsSmbHazard)
    );

    // "." or ".." after stripping zero-width
    let dot_strip = inspect_utf8(".\u{200B}");
    assert_eq!(
        dot_strip.rename_plan.as_ref().unwrap().track,
        RemediationTrack::ManualReview
    );
    assert_eq!(
        dot_strip
            .rename_plan
            .as_ref()
            .unwrap()
            .composed_suggested_name,
        None
    );

    // Hidden reserved device name CON.txt after stripping zero-width
    let con_strip = inspect_utf8("CON\u{200B}.txt");
    assert_eq!(
        con_strip.rename_plan.as_ref().unwrap().track,
        RemediationTrack::ManualReview
    );
    assert_eq!(
        con_strip
            .rename_plan
            .as_ref()
            .unwrap()
            .composed_suggested_name,
        None
    );
    assert!(
        con_strip
            .findings
            .iter()
            .any(|f| f.category == IssueCategory::WindowsSmbHazard)
    );

    // Hidden trailing period after stripping zero-width
    let trailing_dot = inspect_utf8("folder.\u{200B}");
    assert_eq!(
        trailing_dot.rename_plan.as_ref().unwrap().track,
        RemediationTrack::ManualReview
    );
    assert_eq!(
        trailing_dot
            .rename_plan
            .as_ref()
            .unwrap()
            .composed_suggested_name,
        None
    );
}

#[test]
fn sc002_preserves_1000_clean_nfc_files_valid_emojis_12_exceptions_and_kana_words() {
    // 1. UTS #51 emoji sequences (U+FE0F VS16 & U+200D ZWJ) preserved cleanly
    for emoji_name in ["❤️화보집.zip", "👨‍👩‍👧가족사진.jpg", "☀️여행.png"]
    {
        let res = inspect_utf8(emoji_name);
        assert!(
            res.findings.is_empty(),
            "valid UTS #51 emoji {emoji_name:?} must not trigger warnings: {:?}",
            res.findings
        );
    }

    // 2. UAX #15 12 standalone Compatibility Ideographs preserved cleanly
    for ch in [
        '\u{FA0E}', '\u{FA0F}', '\u{FA11}', '\u{FA13}', '\u{FA14}', '\u{FA1F}', '\u{FA21}',
        '\u{FA23}', '\u{FA24}', '\u{FA27}', '\u{FA28}', '\u{FA29}',
    ] {
        let name = format!("표준한자_{ch}.txt");
        let res = inspect_utf8(&name);
        assert!(
            res.findings.is_empty(),
            "UAX #15 exception {name:?} must not trigger warnings: {:?}",
            res.findings
        );
    }

    // 3. Clean Japanese Katakana words, special safe punctuation, and ASCII 0/O/1/l/I preserved cleanly
    for clean in [
        "エラー.txt",
        "カタログ.zip",
        "コード.png",
        "01_Illusion_0O.log",
        "¡Hola_세계_日本_中国.txt",
    ] {
        let res = inspect_utf8(clean);
        assert!(
            res.findings.is_empty(),
            "clean entry {clean:?} must have zero findings: {:?}",
            res.findings
        );
    }

    // 4. 1,000 clean NFC CJK + Hangul + Kana + ASCII files (SC-002)
    for i in 0..1000 {
        let name = format!("00{i:04}_가나다_한국어_日本語_カタログ_中文_File.txt");
        let res = inspect_utf8(&name);
        assert!(
            res.findings.is_empty(),
            "SC-002 false positive on clean NFC file {name:?}: {:?}",
            res.findings
        );
        assert_eq!(res.encoding_type, "UTF-8 NFC");
    }
}

#[test]
fn detects_safe_fullwidth_ascii_variants_while_gating_windows_posix_hazards() {
    let fw_safe = inspect_utf8("ＡＢＣ１２３（테스트）.txt");
    let conf = fw_safe
        .findings
        .iter()
        .find(|f| f.category == IssueCategory::CrossScriptConfusable)
        .expect("fullwidth ASCII must be flagged as CrossScriptConfusable");
    assert_eq!(conf.track, RemediationTrack::ManualReview);
    assert!(
        conf.candidate_hints
            .iter()
            .any(|h| h.encoding == "FullwidthAscii" && h.decoded_name == "ABC123(테스트).txt"),
        "expected safe FullwidthAscii hint: {:?}",
        conf.candidate_hints
    );

    // 9 Windows/POSIX forbidden fullwidth substitutes must be flagged without emitting unsafe / \ : * ? < > " | hints
    let fw_forbid = inspect_utf8("제목：테스트／안내.txt");
    let conf_forbid = fw_forbid
        .findings
        .iter()
        .find(|f| f.category == IssueCategory::CrossScriptConfusable)
        .expect("forbidden fullwidth substitute flagged");
    assert!(
        conf_forbid
            .candidate_hints
            .iter()
            .all(|h| !h.decoded_name.contains(':') && !h.decoded_name.contains('/')),
        "unsafe ASCII hint must never be emitted: {:?}",
        conf_forbid.candidate_hints
    );

    // Fullwidth period / dotdot / CON hazard candidates must be blocked by !has_windows_smb_hazard
    for hazard_case in ["제목．", "．．", "ＣＯＮ.txt"] {
        let res = inspect_utf8(hazard_case);
        for f in &res.findings {
            assert!(
                f.candidate_hints
                    .iter()
                    .all(|h| h.encoding != "FullwidthAscii"),
                "hazard case {hazard_case:?} must not emit FullwidthAscii hint: {:?}",
                f.candidate_hints
            );
        }
    }

    // U+FF5E stays PunctuationVariant
    let wave = inspect_utf8("노래～초안.txt");
    assert!(
        wave.findings
            .iter()
            .any(|f| f.category == IssueCategory::PunctuationVariant)
    );
}

#[test]
fn char_detail_reasons_localize_in_korean_and_english() {
    let cases = [
        "⺁_radical.txt",
        "エ作.txt",
        "A\u{0430}test.txt",
        "か\u{309B}.txt",
        "ｶﾀｶﾅ.txt",
        "ＡＢＣ.txt",
        "\u{1100}test.txt",
        "a\u{0301}\u{0301}.txt",
    ];
    for name in cases {
        let ko = inspect_entry(
            &PathBuf::from(name),
            &RawOsName::UnixBytes(name.as_bytes().to_vec()),
            EntryType::File,
            UiLanguage::Ko,
        );
        let en = inspect_entry(
            &PathBuf::from(name),
            &RawOsName::UnixBytes(name.as_bytes().to_vec()),
            EntryType::File,
            UiLanguage::En,
        );
        let ko_reasons: Vec<&str> = ko
            .findings
            .iter()
            .flat_map(|f| f.offending_chars.iter())
            .map(|c| c.reason.as_str())
            .collect();
        let en_reasons: Vec<&str> = en
            .findings
            .iter()
            .flat_map(|f| f.offending_chars.iter())
            .map(|c| c.reason.as_str())
            .collect();
        assert!(!ko_reasons.is_empty(), "expected reasons for {name}");
        assert_ne!(
            ko_reasons, en_reasons,
            "CharDetail.reason for {name:?} must localize in Ko vs En: {ko_reasons:?}"
        );
    }
}

#[test]
fn restricts_zwj_protection_detects_all_assigned_isolated_jamo_and_normalizes_composed_draft() {
    // T074: ZWJ between ASCII digits/#/* must flag InvisibleZeroWidth, while valid Extended_Pictographic ZWJ stays clean
    for bad_zwj in ["2026\u{200D}01.txt", "#\u{200D}*", "1\u{200D}2.txt"] {
        let res = inspect_utf8(bad_zwj);
        assert!(
            res.findings
                .iter()
                .any(|f| f.category == IssueCategory::InvisibleZeroWidth),
            "{bad_zwj:?} must flag InvisibleZeroWidth: {:?}",
            res.findings
        );
    }
    for good_emoji in [
        "❤️\u{200D}🔥.txt",
        "👩🏻\u{200D}💻.txt",
        "👨\u{200D}👩\u{200D}👧가족사진.jpg",
    ] {
        let res = inspect_utf8(good_emoji);
        assert!(
            res.findings.is_empty(),
            "valid UTS #51 emoji {good_emoji:?} must stay clean: {:?}",
            res.findings
        );
    }

    // T075: Unmapped assigned isolated Jamo (U+A960, U+D7B0, U+1113) flagged; unassigned slots (U+A97F, U+D7C7, U+FFC0) NOT flagged
    for assigned_unmapped in ["\u{A960}test.txt", "\u{D7B0}test.txt", "\u{1113}test.txt"] {
        let res = inspect_utf8(assigned_unmapped);
        assert!(
            res.findings
                .iter()
                .any(|f| f.category == IssueCategory::CrossScriptConfusable
                    && f.track == RemediationTrack::ManualReview),
            "assigned isolated jamo {assigned_unmapped:?} must trigger CrossScriptConfusable: {:?}",
            res.findings
        );
    }
    for unassigned_slot in ["\u{A97F}test.txt", "\u{D7C7}test.txt", "\u{FFC0}test.txt"] {
        let res = inspect_utf8(unassigned_slot);
        assert!(
            !res.findings
                .iter()
                .any(|f| f.category == IssueCategory::CrossScriptConfusable),
            "unassigned jamo slot {unassigned_slot:?} must not be flagged as CrossScriptConfusable: {:?}",
            res.findings
        );
    }

    // T076: InvisibleZeroWidth removal between composable Jamo or combining marks produces NFC draft + ManualReview track
    let jamo_split = inspect_utf8("\u{1100}\u{200B}\u{1161}.txt");
    let jamo_plan = jamo_split.rename_plan.as_ref().unwrap();
    assert_eq!(jamo_plan.track, RemediationTrack::ManualReview);
    assert_eq!(jamo_plan.composed_suggested_name.as_deref(), Some("가.txt"));

    let mark_split = inspect_utf8("e\u{200B}\u{0301}.txt");
    let mark_plan = mark_split.rename_plan.as_ref().unwrap();
    assert_eq!(mark_plan.track, RemediationTrack::ManualReview);
    assert_eq!(mark_plan.composed_suggested_name.as_deref(), Some("é.txt"));

    // Coexisting AutoFixable KangxiRadical (U+2F4A ⽊) and ManualReview Supplement (U+2E81 ⺁) produce distinct IDs
    let dual_radical = inspect_utf8("⽊⺁.txt");
    let kangxi_ids: Vec<&str> = dual_radical
        .findings
        .iter()
        .filter(|f| f.category == IssueCategory::KangxiRadical)
        .map(|f| f.id.as_str())
        .collect();
    assert_eq!(kangxi_ids.len(), 2);
    assert_ne!(
        kangxi_ids[0], kangxi_ids[1],
        "finding_id must include track"
    );
}
