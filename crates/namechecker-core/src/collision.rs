//! Cross-platform filename collision risk detection across normalization, compatibility,
//! and case-folding boundaries.

use std::borrow::Cow;
use std::collections::{BTreeSet, HashMap};
use std::path::Path;

use crate::model::{CollisionRisk, CollisionType};
use crate::unicode_data::{
    compat_ideograph_canonical, is_normalized_nfc, normalize_nfc, radical_compat_fix,
};

fn canonical_compat_key(name: &str) -> (Cow<'_, str>, Cow<'_, str>, bool) {
    if name.is_ascii() {
        return (Cow::Borrowed(name), Cow::Borrowed(name), false);
    }

    let nfc_key: Cow<'_, str> = if is_normalized_nfc(name) {
        Cow::Borrowed(name)
    } else {
        Cow::Owned(normalize_nfc(name))
    };

    let mut buf = String::with_capacity(name.len());
    let mut had_compat = false;
    for ch in name.chars() {
        if let Some(rep) = radical_compat_fix(ch) {
            had_compat = true;
            buf.push(rep);
        } else if let Some(rep) = compat_ideograph_canonical(ch) {
            had_compat = true;
            buf.push(rep);
        } else {
            buf.push(ch);
        }
    }

    let compat_key: Cow<'_, str> = if had_compat {
        if is_normalized_nfc(&buf) {
            Cow::Owned(buf)
        } else {
            Cow::Owned(normalize_nfc(&buf))
        }
    } else {
        nfc_key.clone()
    };

    (nfc_key, compat_key, had_compat)
}

/// Detects sibling entry groups within `parent_dir` that collide on Windows, macOS, or Linux/NAS.
#[must_use]
pub fn detect_collisions(parent_dir: &Path, entries: &[&str]) -> Vec<CollisionRisk> {
    if entries.len() < 2 {
        return Vec::new();
    }

    let mut unique_entries: Vec<&str> = entries.to_vec();
    unique_entries.sort_unstable();
    unique_entries.dedup();

    struct KeyInfo<'a> {
        raw: &'a str,
        nfc_key: String,
        compat_key: String,
        had_compat: bool,
    }

    let mut infos = Vec::with_capacity(unique_entries.len());
    let mut compat_groups: HashMap<String, (bool, Vec<usize>)> =
        HashMap::with_capacity(unique_entries.len());
    let mut nfc_groups: HashMap<String, Vec<usize>> = HashMap::with_capacity(unique_entries.len());
    let mut case_groups: HashMap<String, Vec<usize>> = HashMap::with_capacity(unique_entries.len());

    for (idx, &raw) in unique_entries.iter().enumerate() {
        let (nfc_cow, compat_cow, had_compat) = canonical_compat_key(raw);
        let compat_key = compat_cow.into_owned();
        let nfc_key = nfc_cow.into_owned();
        let case_key = compat_key.to_lowercase();

        let compat_slot = compat_groups
            .entry(compat_key.clone())
            .or_insert_with(|| (false, Vec::new()));
        compat_slot.0 |= had_compat;
        compat_slot.1.push(idx);

        nfc_groups.entry(nfc_key.clone()).or_default().push(idx);
        case_groups.entry(case_key).or_default().push(idx);

        infos.push(KeyInfo {
            raw,
            nfc_key,
            compat_key,
            had_compat,
        });
    }

    let mut risks = Vec::new();
    let mut emitted_compat = vec![false; infos.len()];

    // 1. Canonical/Compatibility collisions (Kangxi radicals & Compatibility ideographs).
    for (ckey, (had_compat, member_idxs)) in compat_groups {
        if member_idxs.len() >= 2 && had_compat {
            let mut members = Vec::with_capacity(member_idxs.len());
            for idx in member_idxs {
                emitted_compat[idx] = true;
                members.push(infos[idx].raw.to_string());
            }
            members.sort();
            risks.push(CollisionRisk {
                parent_dir: parent_dir.to_path_buf(),
                entries: members,
                normalized_key: ckey,
                collision_type: CollisionType::CanonicalCompatibility,
                affected_platforms: vec![
                    "Windows (NTFS/SMB)".to_string(),
                    "macOS (HFS+/APFS)".to_string(),
                ],
            });
        }
    }

    // 2. Pure NFC/NFD normalization collisions (excluding groups already covered by tier 1).
    for (nfc_key, member_idxs) in nfc_groups {
        if member_idxs.len() >= 2
            && !member_idxs.iter().any(|&idx| emitted_compat[idx])
            && !member_idxs.iter().any(|&idx| infos[idx].had_compat)
        {
            let mut members: Vec<String> = member_idxs
                .into_iter()
                .map(|idx| infos[idx].raw.to_string())
                .collect();
            members.sort();
            risks.push(CollisionRisk {
                parent_dir: parent_dir.to_path_buf(),
                entries: members,
                normalized_key: nfc_key,
                collision_type: CollisionType::NormalizationNfcNfd,
                affected_platforms: vec!["macOS (APFS/HFS+)".to_string(), "NAS/SMB".to_string()],
            });
        }
    }

    // 3. Case-folding collisions across >= 2 distinct canonical compatibility keys.
    for (case_key, member_idxs) in case_groups {
        if member_idxs.len() >= 2 {
            let distinct_ckeys: BTreeSet<&str> = member_idxs
                .iter()
                .map(|&idx| infos[idx].compat_key.as_str())
                .collect();
            let _ = &infos[member_idxs[0]].nfc_key;
            if distinct_ckeys.len() >= 2 {
                let mut members: Vec<String> = member_idxs
                    .into_iter()
                    .map(|idx| infos[idx].raw.to_string())
                    .collect();
                members.sort();
                risks.push(CollisionRisk {
                    parent_dir: parent_dir.to_path_buf(),
                    entries: members,
                    normalized_key: case_key,
                    collision_type: CollisionType::CaseInsensitive,
                    affected_platforms: vec![
                        "Windows (NTFS)".to_string(),
                        "macOS (APFS Case-Insensitive)".to_string(),
                        "SMB/CIFS".to_string(),
                    ],
                });
            }
        }
    }

    risks.sort_by(|a, b| a.normalized_key.cmp(&b.normalized_key));
    risks
}
