//! Build script generating static Unicode 16.0.0 UCD, UTS #51, UAX #39, and
//! KS X 1001 / Unihan `kHangul` lookup tables from pinned data files under `data/`.

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

fn is_cjk_unified(cp: u32) -> bool {
    matches!(
        cp,
        0x4E00..=0x9FFF
            | 0x3400..=0x4DBF
            | 0x20000..=0x2A6DF
            | 0x2A700..=0x2B73F
            | 0x2B740..=0x2B81F
            | 0x2B820..=0x2CEAF
            | 0x2CEB0..=0x2EE5F
            | 0x30000..=0x323AF
    )
}

fn is_katakana_scalar(cp: u32) -> bool {
    matches!(cp, 0x30A0..=0x30FF)
}

fn is_homoglyph_source(cp: u32) -> bool {
    matches!(
        cp,
        0x0370..=0x03FF | 0x0400..=0x052F | 0x1D400..=0x1D7FF
    )
}

fn parse_hex(s: &str) -> Option<u32> {
    u32::from_str_radix(s.trim(), 16).ok()
}

fn parse_single_hex(s: &str) -> Option<u32> {
    let mut parts = s.split_whitespace();
    let first = parts.next()?;
    if parts.next().is_some() {
        return None;
    }
    parse_hex(first)
}

fn lis_indices(values: &[&str]) -> Vec<usize> {
    if values.is_empty() {
        return Vec::new();
    }
    let mut tails: Vec<usize> = Vec::new();
    let mut prev = vec![None; values.len()];
    for (idx, val) in values.iter().enumerate() {
        // Upper-bound binary search to find non-decreasing insertion point
        let pos = tails.partition_point(|&tail_idx| values[tail_idx] <= *val);
        if pos > 0 {
            prev[idx] = Some(tails[pos - 1]);
        }
        if pos == tails.len() {
            tails.push(idx);
        } else {
            tails[pos] = idx;
        }
    }
    let mut result = Vec::with_capacity(tails.len());
    let mut cur = tails.last().copied();
    while let Some(idx) = cur {
        result.push(idx);
        cur = prev[idx];
    }
    result.reverse();
    result
}

fn enumerate_ksx1001_hanja() -> Vec<u32> {
    let mut out = Vec::with_capacity(4888);
    let mut buf = [0u8; 2];
    for b1 in 0xCAu8..=0xFDu8 {
        for b2 in 0xA1u8..=0xFEu8 {
            buf[0] = b1;
            buf[1] = b2;
            let (cow, _, had_errors) = encoding_rs::EUC_KR.decode(&buf);
            if !had_errors {
                let mut chars = cow.chars();
                if let (Some(ch), None) = (chars.next(), chars.next()) {
                    out.push(ch as u32);
                }
            }
        }
    }
    out
}

fn write_pair_table(w: &mut BufWriter<File>, name: &str, pairs: &[(u32, u32)]) {
    writeln!(w, "pub static {name}: &[(u32, u32)] = &[").expect("write pair header");
    for &(a, b) in pairs {
        writeln!(w, "    (0x{a:04X}, 0x{b:04X}),").expect("write pair entry");
    }
    writeln!(w, "];").expect("write pair footer");
}

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let data_dir = manifest_dir.join("data");
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));

    let ucd_path = data_dir.join("UnicodeData.txt");
    let rad_path = data_dir.join("CJKRadicals.txt");
    let emoji_data_path = data_dir.join("emoji-data.txt");
    let emoji_vs_path = data_dir.join("emoji-variation-sequences.txt");
    let confusables_path = data_dir.join("confusables.txt");
    let unihan_path = data_dir.join("Unihan_Readings.txt");

    for p in [
        &ucd_path,
        &rad_path,
        &emoji_data_path,
        &emoji_vs_path,
        &confusables_path,
        &unihan_path,
    ] {
        println!("cargo:rerun-if-changed={}", p.display());
    }

    let ucd_raw = fs::read_to_string(&ucd_path).expect("read UnicodeData.txt");
    let mut assigned_ucd: BTreeMap<u32, (String, String)> = BTreeMap::new();
    let mut raw_assigned_ranges: Vec<(u32, u32)> = Vec::new();
    let mut ucd_range_start: Option<u32> = None;
    for line in ucd_raw.lines() {
        if line.is_empty() {
            continue;
        }
        let fields: Vec<&str> = line.split(';').collect();
        if fields.len() < 6 {
            continue;
        }
        if let Some(cp) = parse_hex(fields[0]) {
            if fields[1].ends_with(", First>") {
                ucd_range_start = Some(cp);
            } else if fields[1].ends_with(", Last>") {
                let start = ucd_range_start.take().unwrap_or(cp);
                raw_assigned_ranges.push((start, cp));
            } else {
                raw_assigned_ranges.push((cp, cp));
            }
            assigned_ucd.insert(cp, (fields[1].to_string(), fields[5].to_string()));
        }
    }
    raw_assigned_ranges.sort_unstable();
    let mut bmp_assigned_mask = [0u64; 1024];
    let mut assigned_unicode_16_ranges: Vec<(u32, u32)> = Vec::new();
    for (s, e) in raw_assigned_ranges {
        let bmp_end = e.min(0xFFFF);
        if s <= bmp_end {
            for cp in s..=bmp_end {
                bmp_assigned_mask[(cp >> 6) as usize] |= 1u64 << (cp & 63);
            }
        }
        if e > 0xFFFF {
            let supp_s = s.max(0x10000);
            if let Some(last) = assigned_unicode_16_ranges.last_mut()
                && supp_s <= last.1.saturating_add(1)
            {
                last.1 = last.1.max(e);
            } else {
                assigned_unicode_16_ranges.push((supp_s, e));
            }
        }
    }

    let mut compat_ideograph_canon: Vec<(u32, u32)> = Vec::new();
    let mut radical_compat_fix: Vec<(u32, u32)> = Vec::new();
    let mut assigned_radical_supp: Vec<u32> = Vec::new();
    let mut halfwidth_kana_to_full: Vec<(u32, u32)> = Vec::new();
    let mut kana_precomposed_dakuten: Vec<(u32, u32, u32)> = Vec::new();
    let mut compat_jamo_by_suffix: BTreeMap<String, u32> = BTreeMap::new();
    let mut isolated_jamo_map: BTreeMap<u32, u32> = BTreeMap::new();

    for (&cp, (name, decomp)) in &assigned_ucd {
        // 1. Canonical Compatibility Ideographs (automatically excluding the 12 UAX #15 exceptions)
        if (matches!(cp, 0xF900..=0xFAFF | 0x2F800..=0x2FA1F))
            && !decomp.is_empty()
            && !decomp.starts_with('<')
            && let Some(target) = parse_single_hex(decomp)
        {
            compat_ideograph_canon.push((cp, target));
        }

        // 2. Kangxi Radicals & UCD <compat> Supplement Radicals (+ assigned supplement slots)
        if (0x2E80..=0x2EF3).contains(&cp) {
            assigned_radical_supp.push(cp);
        }
        if (matches!(cp, 0x2F00..=0x2FD5 | 0x2E80..=0x2EF3))
            && let Some(rest) = decomp.strip_prefix("<compat> ")
            && let Some(target) = parse_single_hex(rest)
        {
            radical_compat_fix.push((cp, target));
        }

        // 3. Halfwidth Katakana -> Fullwidth Katakana
        if (0xFF66..=0xFF9D).contains(&cp)
            && let Some(rest) = decomp.strip_prefix("<narrow> ")
            && let Some(target) = parse_single_hex(rest)
        {
            halfwidth_kana_to_full.push((cp, target));
        }

        // 4. Canonical Kana Dakuten / Handakuten composition pairs
        if !decomp.is_empty() && !decomp.starts_with('<') {
            let mut parts = decomp.split_whitespace();
            if let (Some(base_hex), Some(mark_hex), None) =
                (parts.next(), parts.next(), parts.next())
                && let (Some(base_cp), Some(mark_cp)) = (parse_hex(base_hex), parse_hex(mark_hex))
                && (mark_cp == 0x3099 || mark_cp == 0x309A)
            {
                kana_precomposed_dakuten.push((base_cp, mark_cp, cp));
            }
        }

        // 5. Compatibility Hangul Jamo map
        if (0x3131..=0x318E).contains(&cp)
            && cp != 0x3164
            && let Some(suffix) = name.strip_prefix("HANGUL LETTER ")
        {
            compat_jamo_by_suffix.insert(suffix.to_string(), cp);
            if let Some(rest) = decomp.strip_prefix("<compat> ")
                && let Some(src_cp) = parse_single_hex(rest)
            {
                isolated_jamo_map.insert(src_cp, cp);
            }
        }

        // 6. Halfwidth Hangul Jamo
        if (0xFFA1..=0xFFDC).contains(&cp)
            && let Some(rest) = decomp.strip_prefix("<narrow> ")
            && let Some(target_cp) = parse_single_hex(rest)
        {
            isolated_jamo_map.insert(cp, target_cp);
        }
    }

    // Collect all assigned isolated Conjoining/Halfwidth Hangul Jamo excluding fillers U+115F/U+1160
    let mut assigned_isolated_jamo: Vec<u32> = Vec::new();
    for (&cp, (name, _)) in &assigned_ucd {
        if matches!(
            cp,
            0x1100..=0x11FF | 0xA960..=0xA97F | 0xD7B0..=0xD7FF | 0xFFA1..=0xFFDC
        ) && !matches!(cp, 0x115F | 0x1160)
        {
            assigned_isolated_jamo.push(cp);
            let suffix = name
                .strip_prefix("HANGUL CHOSEONG ")
                .or_else(|| name.strip_prefix("HANGUL JUNGSEONG "))
                .or_else(|| name.strip_prefix("HANGUL JONGSEONG "));
            if let Some(suf) = suffix
                && let Some(&compat_cp) = compat_jamo_by_suffix.get(suf)
            {
                isolated_jamo_map.entry(cp).or_insert(compat_cp);
            }
        }
    }
    assigned_isolated_jamo.sort_unstable();

    kana_precomposed_dakuten.sort_unstable();
    let compat_fixed_set: BTreeSet<u32> = radical_compat_fix.iter().map(|&(c, _)| c).collect();

    // Parse CJKRadicals.txt for supplement hints not in RADICAL_COMPAT_FIX (excluding unassigned)
    let rad_raw = fs::read_to_string(&rad_path).expect("read CJKRadicals.txt");
    let mut radical_supp_hint: Vec<(u32, u32)> = Vec::new();
    for line in rad_raw.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let parts: Vec<&str> = line.split(';').collect();
        if parts.len() < 3 {
            continue;
        }
        let rad_str = parts[1].trim();
        let uni_str = parts[2].trim();
        if rad_str.is_empty() || uni_str.is_empty() {
            continue;
        }
        if let (Some(rad_cp), Some(uni_cp)) = (parse_hex(rad_str), parse_hex(uni_str))
            && (0x2E80..=0x2EF3).contains(&rad_cp)
            && assigned_ucd.contains_key(&rad_cp)
            && !compat_fixed_set.contains(&rad_cp)
        {
            radical_supp_hint.push((rad_cp, uni_cp));
        }
    }
    radical_supp_hint.sort_unstable();
    radical_supp_hint.dedup_by_key(|item| item.0);

    // Parse emoji-variation-sequences.txt
    let emoji_vs_raw =
        fs::read_to_string(&emoji_vs_path).expect("read emoji-variation-sequences.txt");
    let mut emoji_vs_set: BTreeSet<u32> = BTreeSet::new();
    for line in emoji_vs_raw.lines() {
        let clean = line.split('#').next().unwrap_or("").trim();
        if clean.is_empty() {
            continue;
        }
        if let Some(seq_part) = clean.split(';').next()
            && let Some(first_hex) = seq_part.split_whitespace().next()
            && let Some(cp) = parse_hex(first_hex)
        {
            emoji_vs_set.insert(cp);
        }
    }
    let emoji_vs_bases: Vec<u32> = emoji_vs_set.into_iter().collect();

    // Parse emoji-data.txt for Extended_Pictographic ranges
    let emoji_data_raw = fs::read_to_string(&emoji_data_path).expect("read emoji-data.txt");
    let mut ext_pic_raw: Vec<(u32, u32)> = Vec::new();
    for line in emoji_data_raw.lines() {
        let clean = line.split('#').next().unwrap_or("").trim();
        if clean.is_empty() {
            continue;
        }
        let Some((range_str, prop_str)) = clean.split_once(';') else {
            continue;
        };
        if prop_str.trim() != "Extended_Pictographic" {
            continue;
        }
        let r = range_str.trim();
        if let Some((start_s, end_s)) = r.split_once("..") {
            if let (Some(s), Some(e)) = (parse_hex(start_s), parse_hex(end_s)) {
                ext_pic_raw.push((s, e));
            }
        } else if let Some(single) = parse_hex(r) {
            ext_pic_raw.push((single, single));
        }
    }
    ext_pic_raw.sort_unstable();
    let mut ext_pic_ranges: Vec<(u32, u32)> = Vec::new();
    for (s, e) in ext_pic_raw {
        if let Some(last) = ext_pic_ranges.last_mut()
            && s <= last.1 + 1
        {
            last.1 = last.1.max(e);
        } else {
            ext_pic_ranges.push((s, e));
        }
    }

    // Parse confusables.txt
    let conf_raw = fs::read_to_string(&confusables_path).expect("read confusables.txt");
    let mut skeleton_buckets: BTreeMap<Vec<u32>, Vec<u32>> = BTreeMap::new();
    let mut script_homoglyphs: BTreeMap<u32, u32> = BTreeMap::new();
    for line in conf_raw.lines() {
        let clean = line.split('#').next().unwrap_or("").trim();
        if clean.is_empty() {
            continue;
        }
        let parts: Vec<&str> = clean.split(';').collect();
        if parts.len() < 2 {
            continue;
        }
        let Some(src_cp) = parse_single_hex(parts[0]) else {
            continue;
        };
        let target_cps: Vec<u32> = parts[1].split_whitespace().filter_map(parse_hex).collect();
        if target_cps.is_empty() {
            continue;
        }

        if target_cps.len() == 1 && is_homoglyph_source(src_cp) {
            let target_cp = target_cps[0];
            if matches!(target_cp, 0x30..=0x39 | 0x41..=0x5A | 0x61..=0x7A)
                || is_cjk_unified(target_cp)
            {
                script_homoglyphs.entry(src_cp).or_insert(target_cp);
            }
        }

        let bucket = skeleton_buckets.entry(target_cps.clone()).or_default();
        bucket.push(src_cp);
        if target_cps.len() == 1 && !bucket.contains(&target_cps[0]) {
            bucket.push(target_cps[0]);
        }
    }

    let mut katakana_hanzi: BTreeMap<u32, u32> = BTreeMap::new();
    for members in skeleton_buckets.values() {
        let primary_han = members
            .iter()
            .copied()
            .filter(|&cp| (0x4E00..=0x9FFF).contains(&cp))
            .min()
            .or_else(|| {
                members
                    .iter()
                    .copied()
                    .filter(|&cp| is_cjk_unified(cp))
                    .min()
            });
        if let Some(han_cp) = primary_han {
            for &m in members {
                if is_katakana_scalar(m) {
                    katakana_hanzi.entry(m).or_insert(han_cp);
                }
            }
        }
    }

    let katakana_hanzi_vec: Vec<(u32, u32)> = katakana_hanzi.into_iter().collect();
    let script_homoglyphs_vec: Vec<(u32, u32)> = script_homoglyphs.into_iter().collect();
    let isolated_jamo_vec: Vec<(u32, u32)> = isolated_jamo_map.into_iter().collect();

    // Parse Unihan_Readings.txt + KS X 1001 monotonic Hanja readings
    let unihan_raw = fs::read_to_string(&unihan_path).expect("read Unihan_Readings.txt");
    let mut khangul: BTreeMap<u32, Vec<String>> = BTreeMap::new();
    for line in unihan_raw.lines() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.split('\t');
        let (Some(cp_str), Some(tag), Some(val)) = (parts.next(), parts.next(), parts.next())
        else {
            continue;
        };
        if tag != "kHangul" {
            continue;
        }
        let Some(cp) = cp_str.strip_prefix("U+").and_then(parse_hex) else {
            continue;
        };
        let mut cands = Vec::new();
        for tok in val.split_whitespace() {
            let syl = tok.split(':').next().unwrap_or("");
            if !syl.is_empty() && !cands.iter().any(|existing| existing == syl) {
                cands.push(syl.to_string());
            }
        }
        if !cands.is_empty() {
            khangul.insert(cp, cands);
        }
    }

    let ksx_hanja = enumerate_ksx1001_hanja();
    let mut single_indices: Vec<usize> = Vec::new();
    let mut single_vals: Vec<&str> = Vec::new();
    for (idx, cp) in ksx_hanja.iter().enumerate() {
        if let Some(cands) = khangul.get(cp)
            && cands.len() == 1
        {
            single_indices.push(idx);
            single_vals.push(cands[0].as_str());
        }
    }
    let anchor_pos = lis_indices(&single_vals);
    let mut anchors: BTreeMap<usize, String> = BTreeMap::new();
    for pos in anchor_pos {
        anchors.insert(single_indices[pos], single_vals[pos].to_string());
    }
    let anchor_keys: Vec<usize> = anchors.keys().copied().collect();

    let mut ksx_resolved: Vec<(u32, u16, String)> = Vec::with_capacity(ksx_hanja.len());
    let mut ksx_set: BTreeSet<u32> = BTreeSet::new();
    for (idx, &cp) in ksx_hanja.iter().enumerate() {
        ksx_set.insert(cp);
        let reading = if let Some(exact) = anchors.get(&idx) {
            exact.clone()
        } else {
            let pos = anchor_keys.partition_point(|&a_idx| a_idx < idx);
            let lo = if pos > 0 {
                anchors[&anchor_keys[pos - 1]].as_str()
            } else {
                ""
            };
            let hi = if pos < anchor_keys.len() {
                anchors[&anchor_keys[pos]].as_str()
            } else {
                "\u{10FFFF}"
            };
            khangul
                .get(&cp)
                .and_then(|cands| {
                    cands
                        .iter()
                        .find(|s| s.as_str() >= lo && s.as_str() <= hi)
                        .or_else(|| cands.first())
                        .cloned()
                })
                .unwrap_or_default()
        };
        ksx_resolved.push((cp, idx as u16, reading));
    }
    ksx_resolved.sort_unstable_by_key(|item| item.0);

    let mut extra_readings: Vec<(u32, String)> = khangul
        .iter()
        .filter(|&(cp, _)| !ksx_set.contains(cp))
        .filter_map(|(&cp, cands)| cands.first().map(|s| (cp, s.clone())))
        .collect();
    extra_readings.sort_unstable_by_key(|item| item.0);

    // Emit OUT_DIR/generated_ucd.rs
    let dest_path = Path::new(&out_dir).join("generated_ucd.rs");
    let file = File::create(&dest_path).expect("create generated_ucd.rs");
    let mut w = BufWriter::new(file);

    write_pair_table(&mut w, "COMPAT_IDEOGRAPH_CANON", &compat_ideograph_canon);
    write_pair_table(
        &mut w,
        "ASSIGNED_UNICODE_16_RANGES",
        &assigned_unicode_16_ranges,
    );
    writeln!(w, "pub static BMP_ASSIGNED_MASK: [u64; 1024] = [").expect("bmp hdr");
    for word in &bmp_assigned_mask {
        writeln!(w, "    0x{word:016X},").expect("bmp word");
    }
    writeln!(w, "];").expect("bmp ftr");
    write_pair_table(&mut w, "RADICAL_COMPAT_FIX", &radical_compat_fix);
    write_pair_table(&mut w, "RADICAL_SUPP_HINT", &radical_supp_hint);
    write_pair_table(&mut w, "HALFWIDTH_KANA_TO_FULL", &halfwidth_kana_to_full);
    write_pair_table(&mut w, "ISOLATED_JAMO_COMPAT", &isolated_jamo_vec);
    write_pair_table(&mut w, "KATAKANA_HANZI_CONFUSABLES", &katakana_hanzi_vec);
    write_pair_table(&mut w, "SCRIPT_HOMOGLYPHS", &script_homoglyphs_vec);
    write_pair_table(&mut w, "EXTENDED_PICTOGRAPHIC_RANGES", &ext_pic_ranges);

    writeln!(
        w,
        "pub static KANA_PRECOMPOSED_DAKUTEN: &[(u32, u32, u32)] = &["
    )
    .expect("kana hdr");
    for &(b, m, c) in &kana_precomposed_dakuten {
        writeln!(w, "    (0x{b:04X}, 0x{m:04X}, 0x{c:04X}),").expect("kana item");
    }
    writeln!(w, "];").expect("kana ftr");

    writeln!(w, "pub static EMOJI_VS_BASES: &[u32] = &[").expect("vs hdr");
    for &cp in &emoji_vs_bases {
        writeln!(w, "    0x{cp:04X},").expect("vs item");
    }
    writeln!(w, "];").expect("vs ftr");

    writeln!(w, "pub static ASSIGNED_RADICAL_SUPP: &[u32] = &[").expect("supp hdr");
    for &cp in &assigned_radical_supp {
        writeln!(w, "    0x{cp:04X},").expect("supp item");
    }
    writeln!(w, "];").expect("supp ftr");

    writeln!(w, "pub static ASSIGNED_ISOLATED_JAMO: &[u32] = &[").expect("jamo hdr");
    for &cp in &assigned_isolated_jamo {
        writeln!(w, "    0x{cp:04X},").expect("jamo item");
    }
    writeln!(w, "];").expect("jamo ftr");

    writeln!(w, "pub static KSX1001_HANJA: &[(u32, u16, &str)] = &[").expect("ksx hdr");
    for (cp, rank, reading) in &ksx_resolved {
        writeln!(w, "    (0x{cp:04X}, {rank}, {reading:?}),").expect("ksx item");
    }
    writeln!(w, "];").expect("ksx ftr");

    writeln!(w, "pub static UNIHAN_KHANGUL_EXTRA: &[(u32, &str)] = &[").expect("ext hdr");
    for (cp, reading) in &extra_readings {
        writeln!(w, "    (0x{cp:04X}, {reading:?}),").expect("ext item");
    }
    writeln!(w, "];").expect("ext ftr");

    w.flush().expect("flush generated_ucd.rs");
}
