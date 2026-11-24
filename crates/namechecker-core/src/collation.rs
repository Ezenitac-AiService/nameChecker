//! Cross-platform filename collation comparison and discrepancy detection.
//!
//! Uses an embedded ICU4X `.postcard` collation blob (`ko`, `ja`, `zh`) loaded via
//! `BlobDataProvider::try_new_from_static_blob` for `MacOsKo`, `Japanese`, and `Chinese`,
//! a distinct `WindowsKoKr` comparator matching Windows Explorer (`StrCmpLogicalW` +
//! Korean KS X 1001 / Unihan readings), and raw UTF-8 `LinuxCodePoint`.

use std::borrow::Cow;
use std::cmp::Ordering;
use std::collections::{BTreeMap, HashSet};
use std::path::Path;
use std::sync::LazyLock;

use icu_collator::options::CollatorOptions;
use icu_collator::preferences::CollationNumericOrdering;
use icu_collator::{Collator, CollatorBorrowed, CollatorPreferences};
use icu_locale_core::{Locale, locale};
use icu_provider_blob::BlobDataProvider;

use crate::model::{CollationProfile, DiscrepancyCause, SortDiscrepancy, UiLanguage};
use crate::unicode_data::{
    compat_ideograph_canonical, is_normalized_nfc, korean_hanja_reading, normalize_nfc,
    radical_compat_fix,
};

static COLLATION_BLOB: &[u8] = include_bytes!("../data/collation.postcard");

static COLLATOR_KO: LazyLock<Collator> = LazyLock::new(|| build_collator(locale!("ko")));
static COLLATOR_JA: LazyLock<Collator> = LazyLock::new(|| build_collator(locale!("ja")));
static COLLATOR_ZH: LazyLock<Collator> = LazyLock::new(|| build_collator(locale!("zh")));

fn build_collator(locale: Locale) -> Collator {
    let provider = BlobDataProvider::try_new_from_static_blob(COLLATION_BLOB)
        .expect("embedded ICU4X collation.postcard blob must deserialize");
    let mut prefs = CollatorPreferences::from(&locale);
    prefs.numeric_ordering = Some(CollationNumericOrdering::True);
    Collator::try_new_with_buffer_provider(&provider, prefs, CollatorOptions::default())
        .expect("embedded ICU4X collation locale must be available")
}

/// Returns the borrowed ICU4X collator for Korean macOS Finder ordering (`MacOsKo`).
#[must_use]
pub fn macos_ko_collator() -> CollatorBorrowed<'static> {
    COLLATOR_KO.as_borrowed()
}

/// Returns the borrowed ICU4X collator for Japanese ordering (`Japanese`).
#[must_use]
pub fn japanese_collator() -> CollatorBorrowed<'static> {
    COLLATOR_JA.as_borrowed()
}

/// Returns the borrowed ICU4X collator for Chinese ordering (`Chinese`).
#[must_use]
pub fn chinese_collator() -> CollatorBorrowed<'static> {
    COLLATOR_ZH.as_borrowed()
}

/// Compares two filenames according to the specified platform collation profile.
#[must_use]
pub fn compare_entries(profile: CollationProfile, a: &str, b: &str) -> Ordering {
    if a == b {
        return Ordering::Equal;
    }
    match profile {
        CollationProfile::WindowsKoKr => compare_windows_ko_kr(a, b),
        CollationProfile::MacOsKo => macos_ko_collator().compare(a, b),
        CollationProfile::LinuxCodePoint => a.cmp(b),
        CollationProfile::Japanese => japanese_collator().compare(a, b),
        CollationProfile::Chinese => chinese_collator().compare(a, b),
    }
}

/// Sorts mutable filename slices in-place using the specified collation profile.
pub fn sort_names(profile: CollationProfile, names: &mut [String]) {
    names.sort_by(|a, b| compare_entries(profile, a, b));
}

#[inline]
fn is_default_ignorable(ch: char) -> bool {
    matches!(
        ch,
        '\u{00AD}'
            | '\u{034F}'
            | '\u{180E}'
            | '\u{200B}'..='\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{206F}'
            | '\u{FE00}'..='\u{FE0F}'
            | '\u{FEFF}'
    )
}

#[inline]
fn is_word_sort_secondary(ch: char) -> bool {
    ch == '-' || ch == '\''
}

#[inline]
fn has_primary_content(s: &str) -> bool {
    s.chars()
        .any(|c| !is_default_ignorable(c) && !is_word_sort_secondary(c))
}

#[inline]
fn is_kana(ch: char) -> bool {
    matches!(
        ch,
        '\u{3040}'..='\u{309F}' | '\u{30A0}'..='\u{30FF}' | '\u{FF65}'..='\u{FF9F}'
    )
}

#[inline]
fn base_kana_key(ch: char) -> (char, u8) {
    // Fold Katakana (U+30A1..=U+30F6) to Hiragana for primary equality, keep script tag for secondary.
    let (hira, script_rank) = if ('\u{30A1}'..='\u{30F6}').contains(&ch) {
        (char::from_u32(ch as u32 - 0x60).unwrap_or(ch), 1u8)
    } else {
        (ch, 0u8)
    };
    (hira, script_rank)
}

#[inline]
fn is_hangul_syllable(ch: char) -> bool {
    ('\u{AC00}'..='\u{D7A3}').contains(&ch)
}

#[inline]
fn is_hangul_jamo(ch: char) -> bool {
    matches!(
        ch,
        '\u{1100}'..='\u{11FF}'
            | '\u{3130}'..='\u{318F}'
            | '\u{A960}'..='\u{A97F}'
            | '\u{D7B0}'..='\u{D7FF}'
            | '\u{FFA0}'..='\u{FFDC}'
    )
}

#[inline]
fn is_cjk_ideograph(ch: char) -> bool {
    matches!(
        ch,
        '\u{2E80}'..='\u{2EFF}'
            | '\u{2F00}'..='\u{2FDF}'
            | '\u{3400}'..='\u{4DBF}'
            | '\u{4E00}'..='\u{9FFF}'
            | '\u{F900}'..='\u{FAFF}'
            | '\u{20000}'..='\u{2A6DF}'
            | '\u{2A700}'..='\u{2B73F}'
            | '\u{2B740}'..='\u{2B81F}'
            | '\u{2B820}'..='\u{2CEAF}'
            | '\u{2CEB0}'..='\u{2EE5F}'
            | '\u{2F800}'..='\u{2FA1F}'
            | '\u{30000}'..='\u{323AF}'
    )
}

/// Primary script hierarchy for Windows Korean Explorer (`WindowsKoKr`):
/// 0 Whitespace -> 1 Symbols/Punctuation -> 2 Numbers -> 3 Latin -> 4 Hangul+Hanja -> 5 Kana -> 6 Other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WinToken<'a> {
    Whitespace(char),
    Symbol(char),
    Number(&'a str),
    Latin(char, char),
    /// `(tier: 0 = Hangul or KS X 1001 Hanja, 1 = other Hangul Jamo/Hanja, key, sub: 0 Hangul / 1 Hanja, ksx_rank)`
    HangulHanja(u8, u32, u8, u16),
    Kana(char, u8, char),
    Other(char),
}

impl<'a> WinToken<'a> {
    #[inline]
    fn group(self) -> u8 {
        match self {
            Self::Whitespace(_) => 0,
            Self::Symbol(_) => 1,
            Self::Number(_) => 2,
            Self::Latin(_, _) => 3,
            Self::HangulHanja(_, _, _, _) => 4,
            Self::Kana(_, _, _) => 5,
            Self::Other(_) => 6,
        }
    }
}

fn compare_numeric_slices(a: &str, b: &str) -> (Ordering, Ordering) {
    let trim_a = a.trim_start_matches('0');
    let trim_b = b.trim_start_matches('0');
    let primary = trim_a
        .len()
        .cmp(&trim_b.len())
        .then_with(|| trim_a.cmp(trim_b));
    // Win32 StrCmpLogicalW tie-breaker: more leading zeros sorts earlier ("01" < "1").
    let leading_tie = b.len().cmp(&a.len());
    (primary, leading_tie)
}

struct WinTokenIter<'a> {
    s: &'a str,
    bytes: &'a [u8],
    pos: usize,
    skip_word_sort: bool,
    secondary_marks: u32,
}

impl<'a> WinTokenIter<'a> {
    fn new(s: &'a str, skip_word_sort: bool) -> Self {
        Self {
            s,
            bytes: s.as_bytes(),
            pos: 0,
            skip_word_sort,
            secondary_marks: 0,
        }
    }
}

impl<'a> Iterator for WinTokenIter<'a> {
    type Item = WinToken<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        while self.pos < self.bytes.len() {
            let b = self.bytes[self.pos];
            if b.is_ascii_digit() {
                let start = self.pos;
                while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_digit() {
                    self.pos += 1;
                }
                return Some(WinToken::Number(&self.s[start..self.pos]));
            }

            let ch = self.s[self.pos..].chars().next()?;
            self.pos += ch.len_utf8();

            if is_default_ignorable(ch) {
                self.secondary_marks = self.secondary_marks.saturating_add(1);
                continue;
            }
            if self.skip_word_sort && is_word_sort_secondary(ch) {
                self.secondary_marks = self.secondary_marks.saturating_add(1);
                continue;
            }

            let mapped = radical_compat_fix(ch)
                .or_else(|| compat_ideograph_canonical(ch))
                .unwrap_or(ch);

            if mapped.is_whitespace() {
                return Some(WinToken::Whitespace(mapped));
            }
            if mapped.is_ascii_alphabetic() {
                return Some(WinToken::Latin(mapped.to_ascii_lowercase(), mapped));
            }
            if is_hangul_syllable(mapped) {
                return Some(WinToken::HangulHanja(0, mapped as u32, 0, 0));
            }
            if is_cjk_ideograph(mapped) {
                let (ksx_opt, reading_opt) = korean_hanja_reading(mapped);
                if let (Some(ksx_rank), Some(reading)) = (ksx_opt, reading_opt)
                    && let Some(first_syl) = reading.chars().next()
                {
                    return Some(WinToken::HangulHanja(0, first_syl as u32, 1, ksx_rank));
                }
                return Some(WinToken::HangulHanja(1, mapped as u32, 2, u16::MAX));
            }
            if is_hangul_jamo(mapped) {
                return Some(WinToken::HangulHanja(1, mapped as u32, 0, 0));
            }
            if is_kana(mapped) {
                let (base, script) = base_kana_key(mapped);
                return Some(WinToken::Kana(base, script, mapped));
            }
            if ('\u{00C0}'..='\u{024F}').contains(&mapped) {
                return Some(WinToken::Latin(
                    mapped.to_lowercase().next().unwrap_or(mapped),
                    mapped,
                ));
            }
            if mapped.is_ascii_punctuation() || !mapped.is_alphanumeric() {
                return Some(WinToken::Symbol(mapped));
            }
            return Some(WinToken::Other(mapped));
        }
        None
    }
}

#[inline]
fn nfc_cow(s: &str) -> Cow<'_, str> {
    if s.is_ascii() || is_normalized_nfc(s) {
        Cow::Borrowed(s)
    } else {
        Cow::Owned(normalize_nfc(s))
    }
}

/// Compares two filenames using Windows Korean Explorer (`CompareStringEx("ko-KR", SORT_DIGITSASNUMBERS)` + Word Sort).
#[must_use]
pub fn compare_windows_ko_kr(a: &str, b: &str) -> Ordering {
    if a == b {
        return Ordering::Equal;
    }

    let na = nfc_cow(a);
    let nb = nfc_cow(b);

    let skip_a = has_primary_content(&na);
    let skip_b = has_primary_content(&nb);

    let mut it_a = WinTokenIter::new(&na, skip_a);
    let mut it_b = WinTokenIter::new(&nb, skip_b);

    let mut secondary_tie = Ordering::Equal;

    loop {
        match (it_a.next(), it_b.next()) {
            (None, None) => break,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(ta), Some(tb)) => {
                let ga = ta.group();
                let gb = tb.group();
                if ga != gb {
                    return ga.cmp(&gb);
                }
                match (ta, tb) {
                    (WinToken::Whitespace(ca), WinToken::Whitespace(cb))
                    | (WinToken::Symbol(ca), WinToken::Symbol(cb))
                    | (WinToken::Other(ca), WinToken::Other(cb)) => {
                        let cmp = ca.cmp(&cb);
                        if cmp != Ordering::Equal {
                            return cmp;
                        }
                    }
                    (WinToken::Number(sa), WinToken::Number(sb)) => {
                        let (prim, tie) = compare_numeric_slices(sa, sb);
                        if prim != Ordering::Equal {
                            return prim;
                        }
                        if secondary_tie == Ordering::Equal && tie != Ordering::Equal {
                            secondary_tie = tie;
                        }
                    }
                    (WinToken::Latin(la, oa), WinToken::Latin(lb, ob)) => {
                        let prim = la.cmp(&lb);
                        if prim != Ordering::Equal {
                            return prim;
                        }
                        if secondary_tie == Ordering::Equal && oa != ob {
                            // Windows treats lowercase before uppercase when case differs only at tertiary level.
                            secondary_tie = match (oa.is_lowercase(), ob.is_lowercase()) {
                                (true, false) => Ordering::Less,
                                (false, true) => Ordering::Greater,
                                _ => oa.cmp(&ob),
                            };
                        }
                    }
                    (
                        WinToken::HangulHanja(t1, k1, s1, r1),
                        WinToken::HangulHanja(t2, k2, s2, r2),
                    ) => {
                        let prim = t1.cmp(&t2).then_with(|| k1.cmp(&k2));
                        if prim != Ordering::Equal {
                            return prim;
                        }
                        if secondary_tie == Ordering::Equal {
                            secondary_tie = s1.cmp(&s2).then_with(|| r1.cmp(&r2));
                        }
                    }
                    (WinToken::Kana(ba, sa, oa), WinToken::Kana(bb, sb, ob)) => {
                        let prim = ba.cmp(&bb);
                        if prim != Ordering::Equal {
                            return prim;
                        }
                        if secondary_tie == Ordering::Equal {
                            secondary_tie = sa.cmp(&sb).then_with(|| oa.cmp(&ob));
                        }
                    }
                    _ => unreachable!("same token group checked above"),
                }
            }
        }
    }

    secondary_tie
        .then_with(|| it_a.secondary_marks.cmp(&it_b.secondary_marks))
        .then_with(|| na.cmp(&nb))
        .then_with(|| a.cmp(b))
}

/// Detects $O(N \log N)$ sort order inversions and $O(N)$-bounded adjacent boundary discrepancies.
#[must_use]
pub fn detect_sort_discrepancies(
    parent_dir: &Path,
    entries: &[&str],
    issue_names: &HashSet<&str>,
    baseline: CollationProfile,
    active_profiles: &[CollationProfile],
    lang: UiLanguage,
) -> Vec<SortDiscrepancy> {
    if entries.len() < 2 || active_profiles.len() < 2 {
        return Vec::new();
    }

    let n = entries.len();
    let already_sorted = entries
        .windows(2)
        .all(|w| compare_entries(baseline, w[0], w[1]) != Ordering::Greater);
    let (baseline_order, baseline_rank): (Vec<usize>, Vec<usize>) = if already_sorted {
        ((0..n).collect(), (0..n).collect())
    } else {
        let mut order: Vec<usize> = (0..n).collect();
        order.sort_by(|&i, &j| compare_entries(baseline, entries[i], entries[j]));
        let mut rank = vec![0usize; n];
        for (r, &idx) in order.iter().enumerate() {
            rank[idx] = r;
        }
        (order, rank)
    };

    let other_profiles: Vec<CollationProfile> = active_profiles
        .iter()
        .copied()
        .filter(|&p| p != baseline)
        .collect();

    let compute_for_profile =
        |profile: CollationProfile| -> (CollationProfile, Vec<(usize, usize)>) {
            let mut prof_order: Vec<usize> = (0..n).collect();
            prof_order.sort_by(|&i, &j| compare_entries(profile, entries[i], entries[j]));

            let mut pairs = Vec::new();
            for win in prof_order.windows(2) {
                let left = win[0];
                let right = win[1];
                if baseline_rank[left] > baseline_rank[right]
                    && compare_entries(profile, entries[left], entries[right]) == Ordering::Less
                {
                    pairs.push((right, left));
                }
            }
            for win in baseline_order.windows(2) {
                let left = win[0];
                let right = win[1];
                if compare_entries(profile, entries[left], entries[right]) == Ordering::Greater {
                    pairs.push((left, right));
                }
            }
            (profile, pairs)
        };

    let per_profile_results: Vec<(CollationProfile, Vec<(usize, usize)>)> =
        if n >= 1000 && other_profiles.len() > 1 {
            std::thread::scope(|s| {
                let handles: Vec<_> = other_profiles
                    .iter()
                    .map(|&p| s.spawn(move || compute_for_profile(p)))
                    .collect();
                handles.into_iter().filter_map(|h| h.join().ok()).collect()
            })
        } else {
            other_profiles
                .into_iter()
                .map(compute_for_profile)
                .collect()
        };

    // Track divergent adjacent boundary pairs normalized as (earlier_baseline_idx, later_baseline_idx).
    let mut pair_profiles: BTreeMap<(usize, usize), Vec<CollationProfile>> = BTreeMap::new();
    for (profile, pairs) in per_profile_results {
        for key in pairs {
            let list = pair_profiles.entry(key).or_default();
            if !list.contains(&profile) {
                list.push(profile);
            }
        }
    }

    pair_profiles
        .into_iter()
        .map(|((idx_a, idx_b), divergent_profiles)| {
            let entry_a = entries[idx_a];
            let entry_b = entries[idx_b];
            let cause = if issue_names.contains(entry_a) || issue_names.contains(entry_b) {
                DiscrepancyCause::EncodingAnomaly
            } else {
                DiscrepancyCause::InherentPlatformCollation
            };
            let reason = format_discrepancy_reason(
                cause,
                baseline,
                &divergent_profiles,
                entry_a,
                entry_b,
                lang,
            );
            SortDiscrepancy {
                parent_dir: parent_dir.to_path_buf(),
                entry_a: entry_a.to_string(),
                entry_b: entry_b.to_string(),
                baseline_profile: baseline,
                divergent_profiles,
                cause,
                reason,
            }
        })
        .collect()
}

fn format_discrepancy_reason(
    cause: DiscrepancyCause,
    baseline: CollationProfile,
    divergent: &[CollationProfile],
    entry_a: &str,
    entry_b: &str,
    lang: UiLanguage,
) -> String {
    match (cause, lang) {
        (DiscrepancyCause::EncodingAnomaly, UiLanguage::Ko) => format!(
            "인코딩/정규화 이상으로 인해 `{entry_a}` ↔ `{entry_b}` 정렬 순서가 `{baseline:?}` 대비 `{divergent:?}`에서 뒤바뀝니다 (수정 후 정상화 가능)"
        ),
        (DiscrepancyCause::EncodingAnomaly, UiLanguage::En) => format!(
            "Encoding/normalization anomaly flips `{entry_a}` vs `{entry_b}` between `{baseline:?}` and `{divergent:?}` (fixable)"
        ),
        (DiscrepancyCause::InherentPlatformCollation, UiLanguage::Ko) => format!(
            "플랫폼 고유 문자셋 정렬 규칙 차이로 `{entry_a}` ↔ `{entry_b}` 순서가 `{baseline:?}` 대비 `{divergent:?}`에서 다릅니다 (참고 정보)"
        ),
        (DiscrepancyCause::InherentPlatformCollation, UiLanguage::En) => format!(
            "Inherent OS collation script ordering differs for `{entry_a}` vs `{entry_b}` between `{baseline:?}` and `{divergent:?}` (informational)"
        ),
    }
}
