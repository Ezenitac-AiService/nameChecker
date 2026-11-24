//! Portable JSON ignore-list persistence and pre-composition finding suppression.

use std::fs;
use std::path::Path;

use globset::Glob;

use crate::model::{
    CoreError, EntryFinding, IgnoreList, IgnoreRule, IssueCategory, RemediationTrack,
};
use crate::unicode_data::normalize_nfc;

/// Loads an `IgnoreList` from `path`, returning an empty list if the file does not exist.
pub fn load_ignore_list(path: &Path) -> Result<IgnoreList, CoreError> {
    match fs::read_to_string(path) {
        Ok(raw) => serde_json::from_str(&raw).map_err(CoreError::Serialization),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(IgnoreList::default()),
        Err(err) => Err(CoreError::Io(err)),
    }
}

/// Saves an `IgnoreList` as pretty JSON to `path`, creating parent directories as needed.
pub fn save_ignore_list(path: &Path, list: &IgnoreList) -> Result<(), CoreError> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(list).map_err(CoreError::Serialization)?;
    fs::write(path, json).map_err(CoreError::Io)
}

fn upsert_ignore_rule(list: &mut IgnoreList, rule: IgnoreRule) -> IgnoreRule {
    if let Some(existing) = list.rules.iter_mut().find(|r| {
        r.pattern == rule.pattern && r.category == rule.category && r.is_glob == rule.is_glob
    }) {
        *existing = rule.clone();
    } else {
        list.rules.push(rule.clone());
    }
    rule
}

/// Appends or replaces an exact single-component suppression rule without glob parsing.
pub fn add_exact_ignore_rule(
    list: &mut IgnoreList,
    name: &str,
    category: Option<IssueCategory>,
    reason: Option<String>,
) -> Result<IgnoreRule, CoreError> {
    let rule = IgnoreRule::exact_name(name, category, reason)?;
    Ok(upsert_ignore_rule(list, rule))
}

/// Appends or replaces a portable suppression rule extracted from `input_path_or_pattern`.
pub fn add_ignore_rule(
    list: &mut IgnoreList,
    input_path_or_pattern: &str,
    category: Option<IssueCategory>,
    reason: Option<String>,
) -> Result<IgnoreRule, CoreError> {
    let rule = IgnoreRule::from_path_or_pattern(input_path_or_pattern, category, reason)?;
    Ok(upsert_ignore_rule(list, rule))
}

fn rule_matches_name(rule: &IgnoreRule, raw_name: &str, nfc_name: &str) -> bool {
    let nfc_pat = normalize_nfc(&rule.pattern);
    if !rule.is_glob {
        return raw_name == rule.pattern
            || nfc_name == rule.pattern
            || raw_name == nfc_pat
            || nfc_name == nfc_pat;
    }

    let raw_match = Glob::new(&rule.pattern)
        .ok()
        .map(|g| g.compile_matcher())
        .is_some_and(|m| m.is_match(raw_name) || m.is_match(nfc_name));
    if raw_match {
        return true;
    }

    Glob::new(&nfc_pat)
        .ok()
        .map(|g| g.compile_matcher())
        .is_some_and(|m| m.is_match(raw_name) || m.is_match(nfc_name))
}

/// Returns `true` if `finding` is suppressed by `list`.
///
/// Never suppresses `OrphanedTempFile` warnings or `AutoFixable` deterministic issues.
#[must_use]
pub fn should_suppress_finding(list: &IgnoreList, finding: &EntryFinding) -> bool {
    if finding.category == IssueCategory::OrphanedTempFile
        || finding.track != RemediationTrack::ManualReview
    {
        return false;
    }

    let nfc_name = normalize_nfc(&finding.raw_name);
    list.rules.iter().any(|rule| {
        let cat_ok = rule.category.is_none_or(|c| c == finding.category);
        cat_ok && rule_matches_name(rule, &finding.raw_name, &nfc_name)
    })
}
