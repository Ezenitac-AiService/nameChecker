//! Pinned Unicode 16.0.0 UCD, UTS #51, UAX #39, and KS X 1001 / Unihan lookup tables.

use std::sync::LazyLock;

use icu_normalizer::{ComposingNormalizerBorrowed, DecomposingNormalizerBorrowed};

mod generated {
    #![allow(clippy::unreadable_literal, dead_code)]
    include!(concat!(env!("OUT_DIR"), "/generated_ucd.rs"));
}

static NFC_NORMALIZER: LazyLock<ComposingNormalizerBorrowed<'static>> =
    LazyLock::new(ComposingNormalizerBorrowed::new_nfc);

static NFD_NORMALIZER: LazyLock<DecomposingNormalizerBorrowed<'static>> =
    LazyLock::new(DecomposingNormalizerBorrowed::new_nfd);

fn lookup_pair(table: &[(u32, u32)], ch: char) -> Option<char> {
    let key = ch as u32;
    table
        .binary_search_by_key(&key, |&(k, _)| k)
        .ok()
        .and_then(|idx| char::from_u32(table[idx].1))
}

/// Returns `true` if `ch` is an assigned scalar in pinned Unicode `16.0.0` `UnicodeData.txt`.
#[inline]
#[must_use]
pub fn is_assigned_unicode_16(ch: char) -> bool {
    let cp = ch as u32;
    if cp <= 0xFFFF {
        return ((generated::BMP_ASSIGNED_MASK[(cp >> 6) as usize] >> (cp & 63)) & 1) != 0;
    }
    let idx = generated::ASSIGNED_UNICODE_16_RANGES.partition_point(|&(start, _)| start <= cp);
    idx > 0 && cp <= generated::ASSIGNED_UNICODE_16_RANGES[idx - 1].1
}

fn normalize_unicode_16(
    input: &str,
    normalize_slice: impl Fn(&str) -> std::borrow::Cow<'_, str>,
) -> String {
    if input.is_ascii() || input.chars().all(is_assigned_unicode_16) {
        return normalize_slice(input).into_owned();
    }
    let mut out = String::with_capacity(input.len());
    let mut start = 0usize;
    for (idx, ch) in input.char_indices() {
        if !is_assigned_unicode_16(ch) {
            if start < idx {
                out.push_str(&normalize_slice(&input[start..idx]));
            }
            out.push(ch);
            start = idx + ch.len_utf8();
        }
    }
    if start < input.len() {
        out.push_str(&normalize_slice(&input[start..]));
    }
    out
}

/// Performs Unicode 16.0.0 NFC normalization using `icu_normalizer`.
#[must_use]
pub fn normalize_nfc(input: &str) -> String {
    normalize_unicode_16(input, |s| NFC_NORMALIZER.normalize(s))
}

/// Checks if a UTF-8 string is already in Unicode 16.0.0 NFC form.
#[must_use]
pub fn is_normalized_nfc(input: &str) -> bool {
    if input.is_ascii() || input.chars().all(is_assigned_unicode_16) {
        return NFC_NORMALIZER.is_normalized(input);
    }
    let mut start = 0usize;
    for (idx, ch) in input.char_indices() {
        if !is_assigned_unicode_16(ch) {
            if start < idx && !NFC_NORMALIZER.is_normalized(&input[start..idx]) {
                return false;
            }
            start = idx + ch.len_utf8();
        }
    }
    start >= input.len() || NFC_NORMALIZER.is_normalized(&input[start..])
}

/// Performs Unicode 16.0.0 NFD normalization using `icu_normalizer`.
#[must_use]
pub fn normalize_nfd(input: &str) -> String {
    normalize_unicode_16(input, |s| NFD_NORMALIZER.normalize(s))
}

/// Returns the canonical decomposition of a CJK Compatibility Ideograph (`U+F900–U+FAFF`,
/// `U+2F800–U+2FA1F`), excluding the 12 UAX #15 standalone unified ideographs and unassigned slots.
#[must_use]
pub fn compat_ideograph_canonical(ch: char) -> Option<char> {
    lookup_pair(generated::COMPAT_IDEOGRAPH_CANON, ch)
}

/// Returns the `<compat>` replacement character for Kangxi Radicals (`U+2F00–U+2FD5`) and
/// CJK Radicals Supplement `<compat>` entries (`U+2E9F`, `U+2EF3`), excluding unassigned slots.
#[must_use]
pub fn radical_compat_fix(ch: char) -> Option<char> {
    lookup_pair(generated::RADICAL_COMPAT_FIX, ch)
}

/// Returns true if `ch` is an assigned CJK Radicals Supplement character (`U+2E80–U+2EF3`,
/// excluding unassigned slots like `U+2E9A`) in pinned Unicode 16.0.0 `UnicodeData.txt`.
#[must_use]
pub fn is_assigned_cjk_radical_supplement(ch: char) -> bool {
    let key = ch as u32;
    generated::ASSIGNED_RADICAL_SUPP.binary_search(&key).is_ok()
}

/// Returns a `ManualReview` candidate unified ideograph from `CJKRadicals.txt` for supplement
/// radicals without a UCD `<compat>` mapping (`U+2E80–U+2EF2` excluding `U+2E9F`, `U+2E9A`).
#[must_use]
pub fn radical_supplement_hint(ch: char) -> Option<char> {
    lookup_pair(generated::RADICAL_SUPP_HINT, ch)
}

/// Returns the target Hanzi character if `ch` is a Katakana lookalike from UAX #39 `confusables.txt`.
#[must_use]
pub fn katakana_hanzi_confusable(ch: char) -> Option<char> {
    lookup_pair(generated::KATAKANA_HANZI_CONFUSABLES, ch)
}

/// Returns the Latin/ASCII or CJK homoglyph skeleton if `ch` is a non-ASCII Cyrillic/Greek/Math
/// homoglyph in UAX #39 `confusables.txt` (never flagging plain ASCII alphanumerics).
#[must_use]
pub fn script_homoglyph_hint(ch: char) -> Option<char> {
    lookup_pair(generated::SCRIPT_HOMOGLYPHS, ch)
}

/// Returns true if `base` accepts `U+FE0E`/`U+FE0F` under UTS #51 `emoji-variation-sequences.txt`.
#[must_use]
pub fn is_valid_emoji_vs_base(base: char) -> bool {
    let key = base as u32;
    generated::EMOJI_VS_BASES.binary_search(&key).is_ok()
}

/// Returns true if `ch` has the `Extended_Pictographic` property in UTS #51 `emoji-data.txt`.
#[must_use]
pub fn is_extended_pictographic(ch: char) -> bool {
    let key = ch as u32;
    let idx = generated::EXTENDED_PICTOGRAPHIC_RANGES.partition_point(|&(start, _)| start <= key);
    if idx == 0 {
        return false;
    }
    let (start, end) = generated::EXTENDED_PICTOGRAPHIC_RANGES[idx - 1];
    (start..=end).contains(&key)
}

/// Maps a halfwidth Katakana scalar (`U+FF66–U+FF9D`) to its fullwidth equivalent via `<narrow>`.
#[must_use]
pub fn halfwidth_kana_to_fullwidth(ch: char) -> Option<char> {
    lookup_pair(generated::HALFWIDTH_KANA_TO_FULL, ch)
}

/// Returns the precomposed fullwidth Kana character when combining a base Hiragana/Katakana (or
/// halfwidth Katakana) with a combining/spacing/halfwidth voiced or semi-voiced mark
/// (`U+3099`, `U+309A`, `U+309B`, `U+309C`, `U+FF9E`, `U+FF9F`).
#[must_use]
pub fn compose_kana_dakuten_hint(base: char, mark: char) -> Option<char> {
    let norm_base = halfwidth_kana_to_fullwidth(base).unwrap_or(base) as u32;
    let norm_mark = match mark {
        '\u{3099}' | '\u{309B}' | '\u{FF9E}' => 0x3099,
        '\u{309A}' | '\u{309C}' | '\u{FF9F}' => 0x309A,
        _ => return None,
    };
    generated::KANA_PRECOMPOSED_DAKUTEN
        .binary_search_by_key(&(norm_base, norm_mark), |&(b, m, _)| (b, m))
        .ok()
        .and_then(|idx| char::from_u32(generated::KANA_PRECOMPOSED_DAKUTEN[idx].2))
}

/// Maps an isolated Conjoining Hangul Jamo (`U+1100–U+11FF`, `U+A960–U+A97F`, `U+D7B0–U+D7FF`) or
/// Halfwidth Hangul Jamo (`U+FFA1–U+FFDC`) to its Hangul Compatibility Jamo (`U+3131–U+318E`) hint.
#[must_use]
pub fn isolated_jamo_compat_hint(ch: char) -> Option<char> {
    lookup_pair(generated::ISOLATED_JAMO_COMPAT, ch)
}

/// Returns true if `ch` is an assigned Hangul Conjoining or Halfwidth Jamo code point in pinned
/// Unicode 16.0.0 `UnicodeData.txt` (excluding unassigned slots and Hangul Fillers `U+115F`/`U+1160`).
#[must_use]
pub fn is_assigned_isolated_jamo(ch: char) -> bool {
    let key = ch as u32;
    generated::ASSIGNED_ISOLATED_JAMO
        .binary_search(&key)
        .is_ok()
}

/// Returns true if `ch` is one of the four Hangul Filler characters (`U+115F`, `U+1160`,
/// `U+3164`, `U+FFA0`).
#[must_use]
pub const fn is_hangul_filler(ch: char) -> bool {
    matches!(ch, '\u{115F}' | '\u{1160}' | '\u{3164}' | '\u{FFA0}')
}

/// Maps macOS SMB SFM Private Use Area characters (`U+F001–U+F029`) back to their original
/// Windows-reserved character or trailing dot/space hint (matching Linux `cifs_unicode.h` / macOS `smbfs`).
#[must_use]
pub const fn sfm_pua_hint(ch: char) -> Option<&'static str> {
    match ch {
        '\u{F020}' => Some("\""),
        '\u{F021}' => Some("*"),
        '\u{F022}' => Some(":"),
        '\u{F023}' => Some("<"),
        '\u{F024}' => Some(">"),
        '\u{F025}' => Some("?"),
        '\u{F026}' => Some("\\"),
        '\u{F027}' => Some("|"),
        '\u{F028}' => Some(" "),
        '\u{F029}' => Some("."),
        '\u{F001}'..='\u{F01F}' => Some("<control>"),
        _ => None,
    }
}

/// Returns `(ksx1001_rank, primary_korean_hangul_reading)` for KS X 1001 Hanja, or `(None, Some)`
/// for non-KS X 1001 Unihan `kHangul` entries.
#[must_use]
pub fn korean_hanja_reading(ch: char) -> (Option<u16>, Option<&'static str>) {
    let key = ch as u32;
    if let Ok(idx) = generated::KSX1001_HANJA.binary_search_by_key(&key, |&(k, _, _)| k) {
        let (_, rank, reading) = generated::KSX1001_HANJA[idx];
        let opt_reading = if reading.is_empty() {
            None
        } else {
            Some(reading)
        };
        return (Some(rank), opt_reading);
    }
    if let Ok(idx) = generated::UNIHAN_KHANGUL_EXTRA.binary_search_by_key(&key, |&(k, _)| k) {
        return (None, Some(generated::UNIHAN_KHANGUL_EXTRA[idx].1));
    }
    (None, None)
}
