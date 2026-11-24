//! Core domain entities, validation constraints, and schema contracts.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize, Serializer};
use thiserror::Error;

fn serialize_path_lossy<S: Serializer>(path: &Path, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(&path.to_string_lossy())
}

/// Current JSON schema version shared by reports, audit logs, and ignore lists.
pub const SCHEMA_VERSION: &str = "1.0.0";

/// Pinned Unicode Character Database version used for tables and normalization.
pub const UNICODE_VERSION: &str = "16.0.0";

/// Domain errors returned by core configuration, scan, and rename operations.
#[derive(Debug, Error)]
pub enum CoreError {
    /// Target validation failed (e.g., empty roots, >1 root in v1, missing directory).
    #[error("invalid scan target: {0}")]
    InvalidTarget(String),
    /// Unsafe path or invalid single-component filename (path traversal rejected).
    #[error("unsafe path or filename: {0}")]
    UnsafePath(String),
    /// Filesystem or serialization error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    /// JSON serialization/deserialization error.
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

/// UI and diagnostic message language.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum UiLanguage {
    /// Korean explanations (default).
    #[default]
    Ko,
    /// English explanations.
    En,
}

/// Embedded cross-platform collation profile identifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum CollationProfile {
    /// Korean Windows Explorer order (`Whitespace -> Symbols -> Numbers -> Latin -> Hangul+Hanja -> Kana`).
    WindowsKoKr,
    /// Korean macOS Finder order (`CLDR ko.xml [reorder Hang Hani]`).
    MacOsKo,
    /// Raw UTF-8 / POSIX `C.UTF-8` code-point order.
    LinuxCodePoint,
    /// Locale-aware Japanese collation profile.
    Japanese,
    /// Locale-aware Simplified/Traditional Chinese collation profile.
    Chinese,
}

impl CollationProfile {
    /// Returns all five default collation profiles in canonical order.
    #[must_use]
    pub const fn all() -> [Self; 5] {
        [
            Self::WindowsKoKr,
            Self::MacOsKo,
            Self::LinuxCodePoint,
            Self::Japanese,
            Self::Chinese,
        ]
    }
}

/// Default exclusion globs applied to prevent NAS/OS metadata false positives.
pub const DEFAULT_EXCLUDE_GLOBS: &[&str] = &[
    "@eaDir",
    "#recycle",
    ".DS_Store",
    "._*",
    "Thumbs.db",
    ".git",
];

/// Scan configuration adhering to v1 single-folder constraints.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanTarget {
    /// Exactly 1 existing target directory in v1 (`roots.len() == 1`).
    pub roots: Vec<PathBuf>,
    /// Traversal depth (`Some(1)` default in v1 for immediate children only).
    pub max_depth: Option<usize>,
    /// Exclusion glob patterns.
    pub exclude_globs: Vec<String>,
    /// Whether to follow symbolic links (default `false`).
    pub follow_symlinks: bool,
    /// Collation profiles to evaluate.
    pub profiles: Vec<CollationProfile>,
    /// Output explanation language (`Ko` default).
    pub lang: UiLanguage,
    /// Optional path to persistent portable ignore-list JSON file.
    pub ignore_list_path: Option<PathBuf>,
}

impl ScanTarget {
    /// Constructs a v1 single-folder scan target with defaults.
    #[must_use]
    pub fn new(root: PathBuf) -> Self {
        Self {
            roots: vec![root],
            max_depth: Some(1),
            exclude_globs: DEFAULT_EXCLUDE_GLOBS
                .iter()
                .map(|s| (*s).to_string())
                .collect(),
            follow_symlinks: false,
            profiles: CollationProfile::all().to_vec(),
            lang: UiLanguage::Ko,
            ignore_list_path: None,
        }
    }

    /// Validates v1 constraints (`roots.len() == 1`, existing directory).
    pub fn validate(&self) -> Result<(), CoreError> {
        if self.roots.len() != 1 {
            return Err(CoreError::InvalidTarget(format!(
                "v1 requires exactly 1 target directory, got {}",
                self.roots.len()
            )));
        }
        let root = &self.roots[0];
        let vp_root = crate::renamer::verbatim_path_buf(root);
        let meta = std::fs::metadata(vp_root.as_ref()).map_err(|err| {
            CoreError::InvalidTarget(format!("target directory unreachable ({root:?}): {err}"))
        })?;
        if !meta.is_dir() {
            return Err(CoreError::InvalidTarget(format!(
                "target path must be a directory: {root:?}"
            )));
        }
        Ok(())
    }
}

/// Lossless OS-native filename bytes or UTF-16 code units.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RawOsName {
    /// Raw Unix filename bytes (`Vec<u8>`).
    UnixBytes(Vec<u8>),
    /// Raw Windows wide UTF-16 code units (`Vec<u16>`).
    WindowsWide(Vec<u16>),
}

/// Filesystem entry type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EntryType {
    /// Regular file.
    File,
    /// Directory / subfolder.
    Directory,
    /// Symbolic link.
    Symlink,
}

/// Remediation UX track separating safe deterministic fixes from manual reviews.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum RemediationTrack {
    /// Deterministic 1:1 safe canonical conversion.
    AutoFixable,
    /// Ambiguous or hazardous condition requiring manual user review.
    ManualReview,
}

/// All 12 deterministic anomaly categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum IssueCategory {
    /// Kangxi Radicals (`U+2F00–U+2FD5`) or CJK Radicals Supplement (`U+2E80–U+2EF3`).
    KangxiRadical,
    /// CJK Compatibility Ideographs (`U+F900–U+FAFF`, `U+2F800–U+2FA1F` excluding 12 UAX #15 exceptions).
    CompatibilityIdeograph,
    /// Non-NFC Unicode sequence (e.g., NFD Hangul or NFD Kana).
    NonNfcNormalization,
    /// Invisible/zero-width control characters and Hangul Fillers outside valid UTS #51 sequences.
    InvisibleZeroWidth,
    /// CJK/Japanese punctuation or spacing variants (`U+301C`, `U+FF5E`, `U+3000`).
    PunctuationVariant,
    /// Invalid UTF-8 bytes on Unix/NAS or unpaired UTF-16 surrogates on Windows.
    InvalidUtf8,
    /// Unicode Private Use Area code points (including macOS SMB SFM `U+F000–U+F0FF`).
    PrivateUseArea,
    /// Non-emoji Standard or Ideographic Variation Selectors (`U+FE00–U+FE0F`, `U+E0100–U+E01EF`).
    VariationSelector,
    /// Context-gated cross-script lookalikes, non-NFC Kana dakuten, isolated Hangul Jamo, defective combining sequences.
    CrossScriptConfusable,
    /// Unicode Replacement Character (`U+FFFD`).
    ReplacementChar,
    /// Cross-platform Windows/SMB/NAS filename hazards (`<>:"/\|?*`, reserved device names, trailing space/dot, >255 length).
    WindowsSmbHazard,
    /// Stranded `.__nc_tmp_<uuid>` file left behind by an interrupted two-step rename.
    OrphanedTempFile,
}

impl IssueCategory {
    /// Returns all 12 variants in canonical order.
    #[must_use]
    pub const fn all() -> [Self; 12] {
        [
            Self::KangxiRadical,
            Self::CompatibilityIdeograph,
            Self::NonNfcNormalization,
            Self::InvisibleZeroWidth,
            Self::PunctuationVariant,
            Self::InvalidUtf8,
            Self::PrivateUseArea,
            Self::VariationSelector,
            Self::CrossScriptConfusable,
            Self::ReplacementChar,
            Self::WindowsSmbHazard,
            Self::OrphanedTempFile,
        ]
    }

    /// Returns the default deterministic `RemediationTrack` for non-branching categories.
    #[must_use]
    pub const fn default_track(self) -> RemediationTrack {
        match self {
            Self::NonNfcNormalization
            | Self::CompatibilityIdeograph
            | Self::KangxiRadical
            | Self::InvisibleZeroWidth => RemediationTrack::AutoFixable,
            Self::PunctuationVariant
            | Self::InvalidUtf8
            | Self::PrivateUseArea
            | Self::VariationSelector
            | Self::CrossScriptConfusable
            | Self::ReplacementChar
            | Self::WindowsSmbHazard
            | Self::OrphanedTempFile => RemediationTrack::ManualReview,
        }
    }
}

/// Finding severity level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Severity {
    /// Critical compatibility or data hazard.
    Error,
    /// Significant encoding, sorting, or portability issue.
    Warning,
    /// Informational finding.
    Info,
}

/// Code-point-level detail of an offending character within an entry name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CharDetail {
    /// 0-based Unicode scalar index in `raw_name`.
    pub char_index: usize,
    /// Sanitized visible glyph (`None` if non-printable or raw invalid byte).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub glyph: Option<String>,
    /// Unicode scalar notation (e.g., `"U+2F47"`).
    pub code_point: String,
    /// Localized character-level anomaly reason.
    pub reason: String,
}

/// Candidate decoding or rule-based replacement/recovery hint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EncodingHint {
    /// Legacy encoding name or rule source (`CP949`, `CJKRadicals.txt`, `SFM-PUA`, etc.).
    pub encoding: String,
    /// Decoded or suggested replacement/recovery string.
    pub decoded_name: String,
}

/// Single character, encoding, or compatibility issue on a file or directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntryFinding {
    /// Deterministic hash of `(path, category, track)`.
    pub id: String,
    /// Full path to the filesystem entry.
    #[serde(serialize_with = "serialize_path_lossy")]
    pub path: PathBuf,
    /// Parent directory path.
    #[serde(serialize_with = "serialize_path_lossy")]
    pub parent_dir: PathBuf,
    /// Lossy sanitized display representation of the filename.
    pub raw_name: String,
    /// Lossless OS-native filename representation.
    pub raw_os_name: RawOsName,
    /// Entry kind (`File`, `Directory`, `Symlink`).
    pub entry_type: EntryType,
    /// Remediation track (`AutoFixable` or `ManualReview`).
    pub track: RemediationTrack,
    /// Issue category (one of 12 variants).
    pub category: IssueCategory,
    /// Severity level.
    pub severity: Severity,
    /// Code-point breakdown of offending characters.
    pub offending_chars: Vec<CharDetail>,
    /// Populated for `AutoFixable`; `None` for `ManualReview`.
    pub suggested_name: Option<String>,
    /// Optional decoding/replacement/recovery hints.
    #[serde(default)]
    pub candidate_hints: Vec<EncodingHint>,
    /// Localized explanation (`ko` or `en`).
    pub message: String,
}

/// Composes all findings on a single filesystem entry into one target rename plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntryRenamePlan {
    /// Full path to the entry.
    #[serde(serialize_with = "serialize_path_lossy")]
    pub path: PathBuf,
    /// Lossy display name of the entry.
    pub raw_name: String,
    /// Lossless OS-native filename.
    pub raw_os_name: RawOsName,
    /// Unified target name composing `AutoFixable` fixes (`None` if none apply, invalid UTF-8, `OrphanedTempFile`, or rejected by safety gate).
    pub composed_suggested_name: Option<String>,
    /// Track (`ManualReview` if ANY contributing finding is `ManualReview` or safety gate downgrades target).
    pub track: RemediationTrack,
    /// IDs of contributing `EntryFinding` records.
    pub finding_ids: Vec<String>,
}

/// Root cause classification for cross-platform sort discrepancies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DiscrepancyCause {
    /// Fixable ordering distortion caused by encoding/normalization/character anomalies.
    EncodingAnomaly,
    /// Inherent OS script or locale ordering rule on clean filenames.
    InherentPlatformCollation,
}

/// Adjacent boundary sibling pair whose sort order diverges across profiles.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SortDiscrepancy {
    /// Parent directory containing both sibling entries.
    #[serde(serialize_with = "serialize_path_lossy")]
    pub parent_dir: PathBuf,
    /// First sibling filename.
    pub entry_a: String,
    /// Second sibling filename.
    pub entry_b: String,
    /// Reference collation profile (defaults to `WindowsKoKr`).
    pub baseline_profile: CollationProfile,
    /// Profiles whose ordering of `entry_a` vs `entry_b` flips relative to baseline.
    pub divergent_profiles: Vec<CollationProfile>,
    /// Discrepancy cause (`EncodingAnomaly` vs `InherentPlatformCollation`).
    pub cause: DiscrepancyCause,
    /// Localized explanation of the divergence.
    pub reason: String,
}

/// Mechanism causing distinct filenames on disk to collide on another platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CollisionType {
    /// Collides under NFD/NFC normalization (macOS APFS/HFS+ vs Linux/NAS).
    NormalizationNfcNfd,
    /// Collides under canonical compatibility decomposition.
    CanonicalCompatibility,
    /// Collides under ASCII/Unicode case folding (Windows/macOS/SMB vs Linux).
    CaseInsensitive,
}

/// Two or more sibling entries that collide under target platform normalization/case rules.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CollisionRisk {
    /// Parent directory.
    #[serde(serialize_with = "serialize_path_lossy")]
    pub parent_dir: PathBuf,
    /// Colliding sibling entry names (2 or more).
    pub entries: Vec<String>,
    /// Canonical NFC + case-folded key shared by the entries.
    pub normalized_key: String,
    /// Collision category.
    pub collision_type: CollisionType,
    /// Affected target platforms.
    pub affected_platforms: Vec<String>,
}

/// Status of an attempted single-entry safe rename operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RenameStatus {
    /// Rename completed cleanly on disk.
    Applied,
    /// Blocked pre-flight or atomically because target name already exists or collides in batch.
    BlockedCollision,
    /// Blocked because entry is a symbolic link.
    BlockedSymlink,
    /// Volume driver forcibly decomposed NFC back into NFD (e.g., legacy HFS+).
    FilesystemForcedNfd,
    /// OS permission denied.
    PermissionDenied,
    /// Other OS/filesystem I/O failure.
    IoError,
}

/// Single rename operation persisted inside the reversible external audit log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenameOperation {
    /// ISO-8601 UTC timestamp.
    pub timestamp_utc: String,
    /// Parent directory path.
    pub parent_dir: PathBuf,
    /// Lossy display representation of the original filename.
    pub old_name: String,
    /// Lossless original OS filename for exact `undo` restoration.
    pub old_raw_os_name: RawOsName,
    /// OS file identifier (`dev:ino` or Windows FileId) when available.
    pub file_id: Option<String>,
    /// Entry kind recorded for SMB metadata fallback verification.
    pub entry_type: EntryType,
    /// File byte size recorded for SMB metadata fallback verification.
    pub byte_size: Option<u64>,
    /// Modification timestamp (nanoseconds since Unix epoch) for SMB fallback verification.
    pub mtime_ns: Option<i128>,
    /// Stranded temporary filename if Step 2 and Step-1 rollback both failed.
    pub temp_name: Option<String>,
    /// Single-component target filename.
    pub new_name: String,
    /// Operation outcome status.
    pub status: RenameStatus,
    /// Optional localized error detail when `status != Applied`.
    pub error_detail: Option<String>,
}

/// Reversible audit log written outside the scanned target folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenameAuditLog {
    /// Schema version (`"1.0.0"`).
    pub schema_version: String,
    /// Chronological list of rename operations.
    pub operations: Vec<RenameOperation>,
}

/// Ordered entry displayed in the Left Pane (`Before Fix`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrderedEntry {
    /// 1-based sort rank.
    pub rank: usize,
    /// Display name.
    pub name: String,
    /// Entry kind.
    pub entry_type: EntryType,
    /// Whether this entry has any unsuppressed finding.
    pub has_issue: bool,
}

/// Predicted post-fix entry displayed in the Right Pane (`After Fix`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PredictedEntry {
    /// 1-based rank in `before_order`.
    pub old_rank: usize,
    /// 1-based rank after applying selected fixes.
    pub new_rank: usize,
    /// Signed rank movement (`old_rank as isize - new_rank as isize`; positive = moved up `▲`).
    pub rank_delta: isize,
    /// Original display name.
    pub old_name: String,
    /// Predicted post-fix display name.
    pub new_name: String,
    /// Whether `old_name != new_name`.
    pub changed: bool,
    /// Whether `new_name` collides with an unchanged sibling or another batch target.
    pub has_collision: bool,
}

impl PredictedEntry {
    /// Computes signed `rank_delta` (`old_rank as isize - new_rank as isize`).
    #[must_use]
    pub const fn compute_rank_delta(old_rank: usize, new_rank: usize) -> isize {
        (old_rank as isize).saturating_sub(new_rank as isize)
    }
}

/// Recorded filesystem permission or I/O error during scanning.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IoErrorRecord {
    /// Unreadable path.
    #[serde(serialize_with = "serialize_path_lossy")]
    pub path: PathBuf,
    /// Error description.
    pub message: String,
}

/// Complete aggregated scan report matching `contracts/report-schema.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanReport {
    /// Schema version (`"1.0.0"`).
    pub schema_version: String,
    /// Pinned Unicode version (`"16.0.0"`).
    pub unicode_version: String,
    /// ISO-8601 UTC scan timestamp.
    pub scanned_at_utc: String,
    /// Total inspected entries.
    pub total_entries: usize,
    /// Count of `AutoFixable` findings.
    pub auto_fixable_count: usize,
    /// Count of `ManualReview` findings.
    pub manual_review_count: usize,
    /// Count of `SortDiscrepancy` records.
    pub sort_discrepancy_count: usize,
    /// Count of `CollisionRisk` records.
    pub collision_risk_count: usize,
    /// Aggregated counts by encoding/normalization type.
    pub encoding_counts: BTreeMap<String, usize>,
    /// Aggregated counts by `IssueCategory`.
    pub category_counts: BTreeMap<String, usize>,
    /// Entries in active baseline/selected pre-fix sort order.
    pub before_order: Vec<OrderedEntry>,
    /// Entries in predicted post-fix sort order.
    pub predicted_after_order: Vec<PredictedEntry>,
    /// Unreadable/permission error records.
    pub io_errors: Vec<IoErrorRecord>,
    /// All unsuppressed findings.
    pub findings: Vec<EntryFinding>,
    /// Composed per-entry rename plans.
    pub rename_plans: Vec<EntryRenamePlan>,
    /// Adjacent sort discrepancy pairs.
    pub sort_discrepancies: Vec<SortDiscrepancy>,
    /// Cross-platform collision risks.
    pub collision_risks: Vec<CollisionRisk>,
}

/// Flattened per-finding row serialized for `--format csv` exports.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CsvReportRow {
    /// Sanitized full path.
    pub path: String,
    /// Parent directory path.
    pub parent_dir: String,
    /// Escaped display filename.
    pub raw_name: String,
    /// Entry type string (`File`, `Directory`, `Symlink`).
    pub entry_type: String,
    /// Track string (`AutoFixable`, `ManualReview`).
    pub track: String,
    /// One of the 12 `IssueCategory` names.
    pub category: String,
    /// Severity string (`Error`, `Warning`, `Info`).
    pub severity: String,
    /// Semicolon-delimited `U+XXXX` code points.
    pub offending_codepoints: String,
    /// Per-finding `suggested_name` or empty string.
    pub suggested_name: String,
    /// Entry-level `composed_suggested_name` or empty string.
    pub composed_suggested_name: String,
    /// Semicolon-delimited `encoding:decoded_name` hints or empty string.
    pub candidate_hints: String,
    /// Sanitized localized message.
    pub message: String,
}

/// Portable suppression rule stored in `--ignore-file`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IgnoreRule {
    /// Single-component entry name or relative glob pattern.
    pub pattern: String,
    /// `true` when `pattern` uses glob wildcards, `false` for exact filename match.
    pub is_glob: bool,
    /// Optional single category filter (`None` matches all `ManualReview` categories except `OrphanedTempFile`).
    pub category: Option<IssueCategory>,
    /// Optional user annotation.
    pub reason: Option<String>,
    /// ISO-8601 UTC timestamp when added.
    pub added_at_utc: String,
}

impl IgnoreRule {
    /// Creates an exact single-component ignore rule (`is_glob: false`) without glob parsing
    /// so filenames containing `[`, `?`, or `*` remain literal matches.
    pub fn exact_name(
        name: &str,
        category: Option<IssueCategory>,
        reason: Option<String>,
    ) -> Result<Self, CoreError> {
        if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\\']) {
            return Err(CoreError::UnsafePath(format!(
                "invalid exact single-component ignore name: {name:?}"
            )));
        }
        Ok(Self {
            pattern: name.to_string(),
            is_glob: false,
            category,
            reason,
            added_at_utc: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        })
    }

    /// Extracts a portable single-component rule from a CLI path or glob pattern string.
    pub fn from_path_or_pattern(
        input: &str,
        category: Option<IssueCategory>,
        reason: Option<String>,
    ) -> Result<Self, CoreError> {
        let trimmed = input.trim_end_matches(['/', '\\']);
        if trimmed.is_empty() {
            return Err(CoreError::UnsafePath(
                "ignore pattern cannot be empty".to_string(),
            ));
        }
        let component = trimmed.rsplit(['/', '\\']).next().unwrap_or("").to_string();
        if component.is_empty() || component == "." || component == ".." {
            return Err(CoreError::UnsafePath(format!(
                "invalid single-component ignore pattern: {input:?}"
            )));
        }
        let is_glob = component.contains('*') || component.contains('?') || component.contains('[');
        if is_glob && let Err(err) = globset::Glob::new(&component) {
            return Err(CoreError::InvalidTarget(format!(
                "invalid glob pattern '{component}': {err}"
            )));
        }
        Ok(Self {
            pattern: component,
            is_glob,
            category,
            reason,
            added_at_utc: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        })
    }
}

/// Persistent portable ignore list JSON document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IgnoreList {
    /// Schema version (`"1.0.0"`).
    pub schema_version: String,
    /// Ordered suppression rules.
    pub rules: Vec<IgnoreRule>,
}

impl Default for IgnoreList {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION.to_string(),
            rules: Vec::new(),
        }
    }
}
