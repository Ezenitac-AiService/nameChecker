# Data Model: CJK Cross-Platform Filename & Folder Checker

**Feature**: `001-cjk-filename-checker` | **Date**: 2026-11-18

## Core Entities

### 1. `ScanTarget`
Configuration for a scan execution.

| Field | Type | Constraints / Validation |
|-------|------|--------------------------|
| `roots` | `Vec<PathBuf>` | Exactly 1 path in v1 (`roots.len() == 1`, validated as an existing directory at scan start to match `cli-contract.md` `<PATH>` and single-folder `before_order`/`predicted_after_order` rank lists; `Vec<PathBuf>` retained for future v2 multi-root expansion) |
| `max_depth` | `Option<usize>` | Default `Some(1)` in v1 (immediate children of target folder only; `None` reserved for future hierarchical expansion) |
| `exclude_globs` | `Vec<String>` | Defaults include `@eaDir`, `#recycle`, `.DS_Store`, `._*`, `Thumbs.db`, `.git` |
| `follow_symlinks` | `bool` | Default `false` (cycle-safe tracking if `true`) |
| `profiles` | `Vec<CollationProfile>` | Default: `[WindowsKoKr, MacOsKo, LinuxCodePoint, Japanese, Chinese]` |
| `lang` | `UiLanguage` | `Ko` (default) or `En` |
| `ignore_list_path` | `Option<PathBuf>` | Optional path to persistent ignore-list file |

### 2. `EntryFinding`
Represents a character, encoding, or compatibility issue on a single file or directory.

| Field | Type | Constraints / Validation |
|-------|------|--------------------------|
| `id` | `String` | Deterministic hash of `(path, category, track)` so entries emitting both `AutoFixable` and `ManualReview` `KangxiRadical` findings never collide |
| `path` | `PathBuf` | Full path to entry (serialized as lossy UTF-8 string in JSON reports when path contains non-UTF-8 Unix bytes) |
| `parent_dir` | `PathBuf` | Parent directory path (serialized as lossy UTF-8 string in JSON reports when path contains non-UTF-8 Unix bytes) |
| `raw_name` | `String` | Lossy display representation if invalid UTF-8 / unpaired UTF-16 surrogate |
| `raw_os_name` | `RawOsName` | Lossless OS name: `UnixBytes(Vec<u8>)` \| `WindowsWide(Vec<u16>)` |
| `entry_type` | `EntryType` | `File` \| `Directory` \| `Symlink` |
| `track` | `RemediationTrack` | `AutoFixable` \| `ManualReview` |
| `category` | `IssueCategory` | `KangxiRadical` \| `CompatibilityIdeograph` \| `NonNfcNormalization` \| `InvisibleZeroWidth` \| `PunctuationVariant` \| `InvalidUtf8` \| `PrivateUseArea` \| `VariationSelector` \| `CrossScriptConfusable` \| `ReplacementChar` \| `WindowsSmbHazard` \| `OrphanedTempFile` |
| `severity` | `Severity` | `Error` \| `Warning` \| `Info` |
| `offending_chars` | `Vec<CharDetail>` | Character index, glyph, Unicode `U+XXXX`, and reason |
| `suggested_name` | `Option<String>` | Populated for `AutoFixable`; `None` for `ManualReview` |
| `candidate_hints` | `Vec<EncodingHint>` | Optional candidate hints `{ encoding, decoded_name }` covering legacy byte decodings (`CP949`, `Shift_JIS`, `GB18030`, `Big5`) and rule-based suggestions (`CJKRadicals.txt`, `SFM-PUA`, `Punctuation`, `FullwidthAscii`, `KanaDakuten`, `HangulJamo`, `UndoRecovery`) |
| `message` | `String` | Localized explanation (`ko` or `en`) |

**Deterministic `IssueCategory -> RemediationTrack` Mapping (`FR-016`) & Post-Composition Target Validation Gate (`FR-010`)**:
- **`AutoFixable` (`suggested_name: Some(...)` ONLY if Post-Composition Target Validation Gate passes)**:
  - `NonNfcNormalization` → canonical UTF-8 NFC normalization (when no unmerged/defective combining marks remain)
  - `CompatibilityIdeograph` → canonical UAX #15 CJK Unified Ideograph (the 12 UAX #15 standalone unified ideographs `U+FA0E`–`U+FA29` are never flagged)
  - `KangxiRadical` (UCD `<compat>` decomposing radicals only: `U+2F00–U+2FD5`, `U+2E9F`, `U+2EF3`) → UCD `<compat>` CJK Unified Ideograph
  - `InvisibleZeroWidth` (non-emoji `U+200B`, `U+FEFF`, `U+00AD`, `U+200E–U+200F`, `U+202A–U+202E`, `U+2066–U+2069`, standalone/non-emoji `U+200C`/`U+200D`, and Hangul Fillers `U+115F`, `U+1160`, `U+3164`, `U+FFA0`) → deterministic stripping, subject to Post-Composition Target Validation Gate: if the stripped/composed result becomes `""`, `"."`, `".."`, or triggers any `WindowsSmbHazard`, it MUST downgrade to `ManualReview` (`suggested_name: None`) and emit the exposed `WindowsSmbHazard`
- **`ManualReview` (`suggested_name: None`, hints/actions provided)**:
  - `KangxiRadical` (`CJKRadicals.txt`-only supplement `U+2E80–U+2EF2` excluding `U+2E9F` and unassigned `U+2E9A`, with `CJKRadicals.txt` candidate hints)
  - `PunctuationVariant` (`U+301C` Wave Dash vs `U+FF5E` Fullwidth Tilde, `U+3000` Ideographic Space, with `Punctuation` replacement hints)
  - `VariationSelector` (non-emoji `U+FE00–U+FE0D`, standalone/non-emoji `U+FE0E–U+FE0F`, and `U+E0100–U+E01EF`, preserving valid UTS #51 emoji sequences)
  - `CrossScriptConfusable` (context-gated UAX #39 confusables: Katakana/CJK lookalikes flagged ONLY when isolated inside contiguous Han/Hangul tokens without adjacent Kana—preserving clean Japanese Kana words—plus Cyrillic/Greek/mathematical homoglyphs mixed into Latin or CJK/Hangul tokens while exempting standard ASCII `0`/`O` and `1`/`l`/`I`; fullwidth/halfwidth variants; non-NFC Japanese voiced/semi-voiced sound marks including spacing `U+309B`/`U+309C`, halfwidth `U+FF9E`/`U+FF9F`, and non-composable combining `U+3099`/`U+309A` with fullwidth `KanaDakuten` hints; isolated/unpaired Conjoining Hangul Jamo `U+1100–U+11FF`/`U+A960–U+A97F`/`U+D7B0–U+D7FF` and Halfwidth Hangul Jamo `U+FFA1–U+FFDC` remaining uncomposed after NFC with compatibility-Jamo `U+3131–U+318E` `HangulJamo` hints; and UAX #15 defective combining sequences lacking a composable base)
  - `PrivateUseArea` (`U+E000–U+F8FF`, `U+F0000–U+FFFFD`, `U+100000–U+10FFFD`, with macOS SMB SFM `U+F000–U+F0FF` replacement hints)
  - `InvalidUtf8` (invalid UTF-8 bytes on Unix/NAS or unpaired UTF-16 surrogates on Windows, with lossless `encoding_rs` candidate hints)
  - `ReplacementChar` (`U+FFFD`)
  - `WindowsSmbHazard` (reserved chars `<>:"/\|?*`, control chars `U+0000–U+001F`, trailing space/period, reserved device names `CON`/`PRN`/`AUX`/`NUL`/`COM1–9`/`LPT1–9` bare or with extensions, `>255` UTF-8 bytes or `>255` UTF-16 units)
  - `OrphanedTempFile` (stranded `.__nc_tmp_<uuid>` temporary rename artifact from an interrupted two-step rename, flagged as `Warning` instructing the user to run `namechecker undo` or GUI `되돌리기`)

### 2b. `EntryRenamePlan`
Composes multiple `EntryFinding` items for a single file or directory into one unified target name.

| Field | Type | Constraints / Validation |
|-------|------|--------------------------|
| `path` | `PathBuf` | Full path to entry (serialized as lossy UTF-8 string in JSON reports when path contains non-UTF-8 Unix bytes) |
| `raw_name` | `String` | Lossy display name of entry |
| `raw_os_name` | `RawOsName` | Lossless OS name (`UnixBytes(Vec<u8>)` \| `WindowsWide(Vec<u16>)`) |
| `composed_suggested_name` | `Option<String>` | Unified draft target name composing all `AutoFixable` fixes on valid UTF-8 entries; MUST be `None` if no `AutoFixable` findings apply, on `InvalidUtf8`/`OrphanedTempFile`, or if Post-Composition Target Validation Gate rejects the composed string (`""`, `"."`, `".."`, or `WindowsSmbHazard`) |
| `track` | `RemediationTrack` | `AutoFixable` \| `ManualReview` (MUST be `ManualReview` whenever ANY `ManualReview` finding exists on the entry or when Post-Composition Target Validation Gate downgrades the composed target—blocking unattended `fix --yes` batch renames) |
| `finding_ids` | `Vec<String>` | IDs of `EntryFinding` records contributing to this plan |

### 3. `SortDiscrepancy`
Represents $O(N)$-bounded adjacent boundary sibling entries in the same directory whose relative order diverges across collation profiles (avoiding $O(N^2)$ pair materialization).

| Field | Type | Constraints / Validation |
|-------|------|--------------------------|
| `parent_dir` | `PathBuf` | Directory containing both entries (serialized as lossy UTF-8 string in JSON reports when path contains non-UTF-8 Unix bytes) |
| `entry_a` | `String` | First filename |
| `entry_b` | `String` | Second filename |
| `baseline_profile` | `CollationProfile` | `WindowsKoKr` (default baseline; variants: `WindowsKoKr` \| `MacOsKo` \| `LinuxCodePoint` \| `Japanese` \| `Chinese`) |
| `divergent_profiles` | `Vec<CollationProfile>` | Profiles (`WindowsKoKr` \| `MacOsKo` \| `LinuxCodePoint` \| `Japanese` \| `Chinese`) where `entry_a` vs `entry_b` ordering flips |
| `cause` | `DiscrepancyCause` | `EncodingAnomaly` (fixable via normalization/replacement) \| `InherentPlatformCollation` (unfixable OS script/locale rule) |
| `reason` | `String` | Localized explanation (e.g., Hanja reading vs code-point order, script-block order, non-KS X 1001 fallback) |

### 4. `CollisionRisk`
Represents two or more sibling entries that are distinct on the current filesystem but collide under target platform rules.

| Field | Type | Constraints / Validation |
|-------|------|--------------------------|
| `parent_dir` | `PathBuf` | Parent directory (serialized as lossy UTF-8 string in JSON reports when path contains non-UTF-8 Unix bytes) |
| `entries` | `Vec<String>` | 2+ colliding sibling names |
| `normalized_key` | `String` | Canonical NFC/compatibility key (case-folded for `CaseInsensitive`) |
| `collision_type` | `CollisionType` | `NormalizationNfcNfd` \| `CanonicalCompatibility` \| `CaseInsensitive` |
| `affected_platforms` | `Vec<String>` | e.g., `["macOS (HFS+/APFS)", "Windows (NTFS/SMB)"]` |

### 5. `RenameOperation` & `RenameAuditLog`
State and audit record for user-confirmed safe renaming (FR-014). `RenameAuditLog` wraps `{ schema_version: String ("1.0.0"), operations: Vec<RenameOperation> }` as persisted JSON (see [cli-contract.md](file:///Users/dev/DEV/nameChecker/specs/001-cjk-filename-checker/contracts/cli-contract.md)).

| Field | Type | Description |
|-------|------|-------------|
| `timestamp_utc` | `String` | ISO-8601 timestamp |
| `parent_dir` | `PathBuf` | Parent directory |
| `old_name` | `String` | Lossy display name of original filename |
| `old_raw_os_name` | `RawOsName` | Lossless original OS filename (`UnixBytes(Vec<u8>)` \| `WindowsWide(Vec<u16>)`) persisted for exact `undo` restoration |
| `file_id` | `Option<String>` | OS file identifier (`dev:ino` on Unix or Windows volume/file index) paired with fallback metadata `(entry_type, byte_size, mtime_ns)` so `undo` remains reliable on SMB mounts (`smbfs`/`cifs`) where `st_dev:st_ino` can change across renames or remounts |
| `entry_type` | `EntryType` | `File` \| `Directory` \| `Symlink` fallback metadata recorded at rename time for SMB `undo` verification |
| `byte_size` | `Option<u64>` | File byte size fallback metadata recorded at rename time for SMB `undo` verification (`Some(len)` for `EntryType::File`; `None` for `EntryType::Directory` and `EntryType::Symlink`, where SMB fallback verifies `(entry_type, mtime_ns)` with non-null `mtime_ns`) |
| `mtime_ns` | `Option<i128>` | Modification timestamp (nanoseconds since Unix epoch) fallback metadata recorded at rename time for SMB `undo` verification |
| `temp_name` | `Option<String>` | Intermediate temporary name (`Some` if Step 2 of a two-step rename failed and Step-1 rollback also failed, allowing `undo` recovery) |
| `new_name` | `String` | Target single-component filename (validated to contain no path separators or `..`) |
| `status` | `RenameStatus` | `Applied` \| `BlockedCollision` \| `BlockedSymlink` \| `FilesystemForcedNfd` \| `PermissionDenied` \| `IoError` |
| `error_detail` | `Option<String>` | Populated when status is not `Applied` |

### 6. `ScanReport`
Aggregated output of a scan run.

| Field | Type | Description |
|-------|------|-------------|
| `schema_version` | `String` | `"1.0.0"` |
| `unicode_version` | `String` | Pinned UCD version (e.g., `"16.0.0"`) used for scan tables and normalization |
| `scanned_at_utc` | `String` | ISO-8601 timestamp |
| `total_entries` | `usize` | Total files + directories inspected |
| `auto_fixable_count` | `usize` | Count of `AutoFixable` findings |
| `manual_review_count` | `usize` | Count of `ManualReview` findings |
| `sort_discrepancy_count` | `usize` | Count of `SortDiscrepancy` records |
| `collision_risk_count` | `usize` | Count of `CollisionRisk` records |
| `encoding_counts` | `BTreeMap<String, usize>` | Aggregated counts by encoding/normalization type (`UTF-8 NFC`, `UTF-8 NFD`, `Non-UTF-8 Legacy`) |
| `category_counts` | `BTreeMap<String, usize>` | Aggregated counts by `IssueCategory` |
| `before_order` | `Vec<OrderedEntry>` | Target folder entries ordered by active platform/baseline sort profile (`rank`, `name`, `entry_type`, `has_issue`; computed after applying `IgnoreList` suppressions) |
| `predicted_after_order` | `Vec<PredictedEntry>` | Predicted sort order after applying selected fixes (`old_rank`, `new_rank`, `rank_delta = old_rank as isize - new_rank as isize`, `old_name`, `new_name`, `changed`, `has_collision`) |
| `io_errors` | `Vec<IoErrorRecord>` | Permission denied / unreadable paths |
| `findings` | `Vec<EntryFinding>` | All `EntryFinding` records |
| `rename_plans` | `Vec<EntryRenamePlan>` | Per-entry composed rename plans aggregating multi-finding entries |
| `sort_discrepancies` | `Vec<SortDiscrepancy>` | All `SortDiscrepancy` records |
| `collision_risks` | `Vec<CollisionRisk>` | All `CollisionRisk` records |

### 6a. Sub-Entities (`EntryFinding` & `ScanReport` Nested Records)
Nested structs serialized inside `EntryFinding` and `ScanReport` conforming 1:1 with [report-schema.json](file:///Users/dev/DEV/nameChecker/specs/001-cjk-filename-checker/contracts/report-schema.json).

| Sub-Entity | Field | Type | Constraints / Validation |
|------------|-------|------|--------------------------|
| `CharDetail` | `char_index` | `usize` | 0-based Unicode scalar index inside `raw_name` |
| `CharDetail` | `glyph` | `Option<String>` | Sanitized visible character display (`None` if non-printable/invalid; omitted in JSON when `None`) |
| `CharDetail` | `code_point` | `String` | Unicode scalar representation (e.g., `"U+2F47"`) |
| `CharDetail` | `reason` | `String` | Localized character-level anomaly explanation |
| `EncodingHint` | `encoding` | `String` | Decoding or rule hint source (`CP949`, `Shift_JIS`, `CJKRadicals.txt`, `SFM-PUA`, `Punctuation`, `FullwidthAscii`, `KanaDakuten`, `HangulJamo`, `UndoRecovery`, etc.) |
| `EncodingHint` | `decoded_name` | `String` | Candidate replacement string or recovery guidance |
| `OrderedEntry` | `rank` | `usize` | 1-based sort rank under active baseline/selected `CollationProfile` |
| `OrderedEntry` | `name` | `String` | Escaped/lossy display filename |
| `OrderedEntry` | `entry_type` | `EntryType` | `File` \| `Directory` \| `Symlink` |
| `OrderedEntry` | `has_issue` | `bool` | `true` if the entry has any unsuppressed `EntryFinding` |
| `PredictedEntry` | `old_rank` | `usize` | 1-based rank in `before_order` |
| `PredictedEntry` | `new_rank` | `usize` | 1-based rank after applying selected fixes/edits |
| `PredictedEntry` | `rank_delta` | `isize` | `old_rank as isize - new_rank as isize` (`> 0` = moved up `▲`, `< 0` = moved down `▼`, `0` = unchanged) |
| `PredictedEntry` | `old_name` | `String` | Original display filename |
| `PredictedEntry` | `new_name` | `String` | Target post-fix filename |
| `PredictedEntry` | `changed` | `bool` | `true` when `old_name != new_name` |
| `PredictedEntry` | `has_collision` | `bool` | `true` when `new_name` collides with another target or unchanged sibling (blocks GUI `[수정]`) |
| `IoErrorRecord` | `path` | `PathBuf` | Unreadable / permission-denied entry path (serialized as lossy UTF-8 string in JSON reports when path contains non-UTF-8 Unix bytes) |
| `IoErrorRecord` | `message` | `String` | Localized I/O error detail |

### 6b. `CsvReportRow`
Flattened per-finding row serialized for `--format csv` exports (prefixed with UTF-8 BOM `0xEF 0xBB 0xBF`, RFC 4180 quoted, and prefix-escaped when starting with `=`, `+`, `-`, `@`, `\t`, or `\r` to prevent spreadsheet formula injection).

| Field / Column Name | Type | Description |
|---------------------|------|-------------|
| `path` | `String` | Full sanitized entry path |
| `parent_dir` | `String` | Parent directory path |
| `raw_name` | `String` | Escaped/lossy display filename |
| `entry_type` | `String` | `File` \| `Directory` \| `Symlink` |
| `track` | `String` | `AutoFixable` \| `ManualReview` |
| `category` | `String` | One of the 12 `IssueCategory` variant names |
| `severity` | `String` | `Error` \| `Warning` \| `Info` |
| `offending_codepoints` | `String` | Semicolon-delimited `U+XXXX` code points from `offending_chars` |
| `suggested_name` | `String` | Per-finding `suggested_name` or empty string |
| `composed_suggested_name` | `String` | Entry-level `EntryRenamePlan.composed_suggested_name` or empty string |
| `candidate_hints` | `String` | Semicolon-delimited `encoding:decoded_name` hints or empty string |
| `message` | `String` | Sanitized localized explanation (`ko` or `en`) |

### 7. `IgnoreList` & `IgnoreRule`
Portable JSON schema persisted via `--ignore-file` (defaulting outside scanned target folders) so suppressions work identically across macOS (`/Volumes/...`), Windows (`Z:\...` / `\\nas\...`), and Linux/Synology NAS (`/volume1/...`).

| Entity / Field | Type | Constraints / Validation |
|----------------|------|--------------------------|
| `IgnoreList.schema_version` | `String` | `"1.0.0"` |
| `IgnoreList.rules` | `Vec<IgnoreRule>` | Ordered list of portable suppression rules |
| `IgnoreRule.pattern` | `String` | Single-component entry name (or relative glob pattern when `is_glob == true`, validated eagerly via `globset::Glob::new` in CLI `IgnoreRule::from_path_or_pattern`; GUI exact-name ignore toggles construct `IgnoreRule::exact_name` with `is_glob = false` and bypass glob parsing even when literal filenames contain `[`, `?`, or `*` such as `"[작가] 화보〜1.zip"`); literal path arguments passed to CLI extract `file_name()` instead of host-specific drive/mount prefixes, and matching compares against BOTH original `raw_name` and NFC-normalized `nfc(raw_name)` vs `nfc(pattern)` so macOS (NFD) and Windows/Linux/NAS (NFC) rules match identically |
| `IgnoreRule.is_glob` | `bool` | `false` for exact entry-name matching (including all GUI ignore-checkbox entries), `true` for CLI `globset` pattern matching |
| `IgnoreRule.category` | `Option<IssueCategory>` | `Some(...)` to suppress only one `IssueCategory`, or `None` to suppress all `ManualReview` warnings for matching entries (excluding `OrphanedTempFile`, which is never suppressed; never suppresses `AutoFixable` unless matched on `ManualReview` findings before `EntryRenamePlan` composition, `manual_review_count`, `category_counts`, and `before_order.has_issue` aggregation) |
| `IgnoreRule.reason` | `Option<String>` | Optional user annotation explaining why the name is intentional |
| `IgnoreRule.added_at_utc` | `String` | ISO-8601 timestamp |

## State Transitions (Safe Rename Workflow)

```text
[Scanned EntryFinding / EntryRenamePlan]
   ├── track == AutoFixable ──▶ [User Selects & Previews] ──▶ [Pre-Flight Collision & Symlink Check]
   │                                                               ├── Pass ──▶ [Atomic No-Replace Rename] ──▶ [Applied + Audit Log (file_id)]
   │                                                               └── Fail ──▶ [Blocked (Collision/Symlink/Perm)]
   └── track == ManualReview ─▶ [User Edits Target Name OR Adds to Ignore List]
                                   ├── Manual Target Entered ──▶ [Pre-Flight Check] ──▶ [Applied + Audit Log]
                                   └── Ignored ──▶ [Persisted in Ignore List]
```
