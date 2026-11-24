# Feature Specification: CJK Cross-Platform Filename & Folder Checker

**Feature Branch**: `001-cjk-filename-checker`

**Created**: 2026-11-18

**Status**: Active (Post-Convergence Reviewed)

**Input**: User description: "동아시아의 3국은 각각 중국어, 한국어, 일본어를 사용하는데, 공통적으로 한자를 사용하고 있음 파일명, 폴더 이름을 생성할때에, 이름순 정렬에 플렛폼별 문제가 생기는 경우가 있는데, 같은 모양의 한자가 3개의 언어의 인코딩들에 따라 다른 문자로 인식되는것 예를 들면 날일자는 한국어로 '일' 이라는 발음이지만 일본어로는 '히'라는 발음임 파일명이나 폴더명에 문자를 입력할때에 '입력기'를 이용해서 키보드로 입력하면 문제가 적어지나, 웹페이지에서 복사 붙여넣기 했을때에, 웹페이지의 언어 인코딩 설정을 따라가는것 같음 그래서, 맥os, 리눅스, 리눅스기반 나스(시놀리지), 윈도우 에서 폴더와 파일 정렬이 각각 다른 문제가 있음, rust 기반으로 멀티플렛폼이거나, 혹은 각각의 플렛폼용의 별개 실행파일로, 문제가 되는 파일명 폴더명을 점검할수 있는 프로그램이 있으면 좋겠는데, 이 요구사항에 대한 리서치를 진행해서 스펙을 작성해줘"

## Clarifications

### Session 2026-11-18

- Q: Should regional CJK script variants (e.g., Traditional `國`, Simplified `国`, Japanese Shinjitai `竜` vs `龍`) be flagged in addition to same-shape/canonical-equivalent code-point anomalies? → A: Excluded from v1 scope; v1 detects only visually identical or canonically equivalent code-point anomalies (Kangxi radicals, compatibility ideographs, NFD/NFC, confusables).
- Q: Should cross-platform sort comparison rely on host OS locales or embedded OS-independent collation rules, and which order is the primary baseline? → A: Embedded OS-independent collation rules, using Korean Windows Explorer sort order (Korean locale collation + natural numeric/logical ordering) as the primary reference baseline.
- Q: What language should human-readable CLI output and structured export keys use? → A: Human-readable terminal reports and issue explanations default to Korean (with an English switch option), while structured JSON/CSV field names remain fixed in English.
- Q: Should the application provide a CLI, a desktop GUI, or both in v1? → A: Provide both a CLI (for headless Linux/Synology NAS and scripting) and a desktop GUI (for Windows/macOS/Linux, including scanning mounted Synology NAS shared folders) in v1.
- Q: Should v1 include renaming/fixing flagged filenames and folders? → A: Read-only inspection by default, plus user-confirmed safe renaming (with pre-rename collision checks and a reversible rename log) in v1.
- Q: Should v1 add numeric sort prefixes (e.g., `001_`) to force identical cross-OS sort order, and how should unconvertible/ambiguous characters be handled in UX? → A: Numeric sort prefixes are out of scope; v1 uses a two-track UX separating (1) deterministic 1:1 canonical UTF-8/NFC fixes (batch preview & confirm) from (2) manual-review items (with legacy encoding candidate hints such as CP949/Shift-JIS, manual edit/rename, and ignore-list support).
- Q: What is the directory traversal scope and desktop GUI layout for v1? → A: v1 checks and fixes immediate files and child folders inside the currently selected target directory (`max_depth = 1`, non-recursive by default, with collapsible hierarchical subdirectory tree expansion deferred to v2 after performance/security validation). The desktop GUI uses a top toolbar (`폴더 선택`, `점검`, `수정`, `되돌리기`, sort-profile selector, and `한국어`/`English` language switcher) and a 3-pane layout: Left (current platform sort order), Center (encoding/normalization/anomaly counts + diagnostic check results and two-track editing), and Right (predicted sort order after applying fixes with rank deltas).
- Q: How should canonical CJK Compatibility Ideographs (`U+F900–U+FAFF`, `U+2F800–U+2FA1F` excluding the 12 UAX #15 exceptions) be classified (`ManualReview` vs `AutoFixable`), and what is the empirically verified `WindowsKoKr` vs `MacOsKo` script ordering? → A: Canonical CJK Compatibility Ideographs are `AutoFixable` (converted to canonical CJK Unified Ideographs via NFC, prioritizing cross-platform compatibility over legacy KS X 1001 duplicate-reading distinction). Empirical Windows 11 Explorer (`ko-KR`) verification (`A -> 가 -> 日` and `純 -> 애니포럼 -> 日 -> 화보집`) confirms `WindowsKoKr` orders `Numbers -> Latin (A–Z) -> Hangul + Korean Hanja (interleaved by reading) -> Kana`, whereas `MacOsKo` (CLDR `ko.xml` `[reorder Hang Hani]`) orders `Numbers -> Hangul + Korean Hanja (interleaved by reading) -> Latin (A–Z) -> Kana`.
- Q: Which image/comic viewer's sorting behavior drives the viewer next-file/folder navigation issue (`N14`), and should a separate viewer sort profile be added? → A: No separate image/comic viewer sort profile is added. While external apps (such as Honeyview) and OS file managers have unalterable built-in sorting engines, the project's primary priority is inspecting and fixing mixed encodings and character anomalies in filenames/folder names so cleaned names naturally align as closely as possible with Korean Windows Explorer (`WindowsKoKr`) visible sort order.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Scan Directories for CJK Encoding & Character Anomalies (Priority: P1)

A user managing shared folders across macOS, Windows, Linux, and a Synology NAS runs the checker
against a target directory to identify filenames and immediate child folder names containing
problematic East Asian (CJK) characters—such as characters copied from web pages that look
identical to standard Hanja/Kanji/Hanzi but use different Unicode code points (e.g., Kangxi
Radicals, CJK Radicals Supplement, CJK Compatibility Ideographs, Private Use Area characters, or
NFD-decomposed Hangul).

**Why this priority**: Visually identical characters with different underlying code points are the
root cause of unexpected sorting, search failures, and hidden duplicates when names are copied from
web pages or transferred between operating systems. Detecting these anomalies safely (read-only)
delivers immediate diagnostic value.

**Independent Test**: Can be fully tested by pointing the checker at a sample directory containing
known problematic filenames (e.g., Kangxi radical `⽇` vs unified ideograph `日`, compatibility
ideograph `金` vs `金`, NFD vs NFC Hangul, Katakana lookalike `エ` in a Hanzi title) and verifying
that every problematic entry is flagged with its exact path, offending character(s), code point(s),
issue category, and canonical replacement suggestion.

**Acceptance Scenarios**:

1. **Given** a directory containing a file named with a Kangxi Radical (`⽇`, U+2F47) instead of
   the CJK Unified Ideograph (`日`, U+65E5), **When** the user scans the directory, **Then** the
   checker flags the file, identifies the exact character position and code point, explains that it
   is a radical lookalike, and suggests the standard CJK Unified Ideograph (`日`, U+65E5).
2. **Given** a directory containing filenames with CJK Compatibility Ideographs (e.g., U+F900–U+FAFF)
   or CJK Compatibility Ideographs Supplement characters copied from legacy/Korean/Japanese web
   sources, **When** the user scans the directory, **Then** the checker flags each compatibility
   character and reports its canonical CJK Unified Ideograph equivalent where one exists.
3. **Given** a directory containing filenames with mixed Unicode normalization forms (NFD Hangul
   from macOS alongside NFC Hangul from Windows/Linux/Synology), **When** the user scans the
   directory, **Then** the checker flags the NFD/non-NFC entries and reports the normalization
   mismatch.
4. **Given** a directory containing only clean, standard NFC filenames with standard CJK Unified
   Ideographs, **When** the user scans the directory, **Then** the checker reports zero character
   anomaly issues.

---

### User Story 2 - Detect Cross-Platform Sorting Discrepancies & Name Collisions (Priority: P2)

A user wants to know why a folder's files and subfolders appear in different orders on macOS
Finder, Windows Explorer, Linux CLI/desktop, and Synology NAS File Station—and whether any sibling
entries will collide on case-insensitive or normalization-insensitive filesystems. The user runs
the checker to compare how sibling names in each directory sort across target platform/locale
profiles and to detect potential collisions.

**Why this priority**: Even when filenames use valid CJK Unified Ideographs (such as `日`), East
Asian locales sort the same character differently by pronunciation or stroke/pinyin (e.g., Korean
`일`, Japanese `ひ`/`にち`, Chinese `rì`), while Linux/NAS raw code-point sorting differs from
all three. Identifying sibling pairs that diverge in sort order or collide across filesystems
directly solves the user's cross-platform sorting pain point.

**Independent Test**: Can be fully tested by scanning a directory containing sibling folders/files
whose relative order changes between code-point order, Korean locale order, Japanese locale order,
and Chinese locale order (or whose names normalize to the same string), and verifying that the
checker reports the divergent pairs and collision risks accurately.

**Acceptance Scenarios**:

1. **Given** a directory containing sibling entries whose relative sort order differs between
   raw code-point order (Linux/Synology default) and locale-based collation (Korean, Japanese,
   Chinese) or natural numeric order (Windows/macOS), **When** the user runs a cross-platform
   sort check, **Then** the checker reports the affected directory and the specific sibling pairs
   whose order diverges across the selected platform/locale profiles.
2. **Given** a directory on a case-sensitive/normalization-preserving filesystem (Linux/Synology)
   that contains two sibling entries whose names are distinct byte sequences (e.g., one in NFD and
   one in NFC, or one using a canonically equivalent compatibility ideograph and one using its
   unified ideograph, or differing only in ASCII case), **When** the user scans the directory,
   **Then** the checker flags a cross-platform collision warning indicating that macOS (HFS+
   modified-NFD / APFS normalization-insensitive lookups) or Windows (NTFS/SMB case-insensitive
   matching) will treat them as conflicting names.
3. **Given** filenames starting with Hanja/Kanji/Hanzi characters (e.g., `日...`), **When** the
   user enables locale-divergence inspection across Korean, Japanese, and Chinese profiles,
   **Then** the checker highlights entries whose sort position depends on OS language/locale
   settings rather than universal code-point order.

---

### User Story 3 - Filter, Inspect Details, and Export Audit Reports (Priority: P3)

A user auditing a large Synology NAS volume or shared team drive needs to exclude irrelevant
system directories (such as `@eaDir`, `.git`, `#recycle`), filter results by severity or issue
type, and export the inspection results in both human-readable and machine-readable formats for
review or downstream cleanup workflows.

**Why this priority**: Real-world NAS and workstation storage often contains tens or hundreds of
thousands of files plus NAS metadata folders. Filtering and structured reporting make the tool
practical for large-scale audits without modifying any files.

**Independent Test**: Can be fully tested by running the checker on a target directory containing
system/hidden entries, applying exclusion patterns and output format options (human-readable
summary vs. structured JSON/CSV), and verifying that excluded entries are skipped and the exported
report contains complete, parseable findings.

**Acceptance Scenarios**:

1. **Given** a target directory containing NAS/OS metadata entries (e.g., `@eaDir`, `#recycle`,
   `.DS_Store`, `._*`, `Thumbs.db`), **When** the user scans with default or custom exclusion
   patterns, **Then** excluded entries are skipped and not reported as false positives.
2. **Given** a completed scan with multiple findings, **When** the user requests a structured
   report (JSON or CSV), **Then** the tool outputs valid, parseable records containing path, entry
   type (file/directory), issue category, severity, offending code points, and suggested canonical
   name.
3. **Given** a target directory containing unreadable or permission-denied entries, **When** the
   user runs a scan, **Then** the checker logs the inaccessible path as a warning/error, continues
   scanning remaining accessible entries, and exits with a clear status code.

---

### User Story 4 - Safely Rename Flagged Entries After Confirmation (Priority: P3)

A user inspecting a local folder or a writable Synology NAS shared folder (e.g., mounted via SMB
on Windows or macOS) reviews the suggested canonical names and explicitly confirms renaming
selected files or immediate child folders—with pre-rename collision checks and a recorded rename
history log so changes can be audited or reverted.

**Why this priority**: While read-only scanning is always the safe default, users with write
permissions on shared NAS folders benefit from fixing confirmed encoding anomalies directly
without manually retyping characters.

**Independent Test**: Can be fully tested by scanning a temporary directory with known anomalous
names, confirming a rename action, verifying that the target entries are renamed to their
canonical forms (using two-step temporary renames where required), confirming that colliding
targets are blocked before any write occurs, and verifying that a rename log is written.

**Acceptance Scenarios**:

1. **Given** a directory with flagged entries and write permissions, **When** the user explicitly
   selects and confirms renaming to the suggested canonical names, **Then** the system checks for
   target collisions first, renames the selected entries safely, and writes a reversible rename
   audit log.
2. **Given** a rename target that would collide with an existing sibling file or folder (or a
   read-only/permission-denied path), **When** the user attempts the rename, **Then** the system
   blocks the conflicting rename, reports the reason clearly, and leaves the existing entry intact.
3. **Given** a directory containing both deterministic 1:1 convertible anomalies (e.g., NFD Hangul,
   Kangxi radicals) and unconvertible/ambiguous entries (e.g., raw CP949/Shift-JIS bytes, PUA, or
   cross-script lookalikes), **When** the user reviews the scan results, **Then** the system
   separates them into "Auto-Fixable" and "Manual Review Required" groups, displays legacy-encoding
   decoding hints where applicable, allows manual target-name editing or ignore-list registration,
   and never auto-converts ambiguous entries.

---

### Edge Cases

- **Broken / Non-UTF-8 Filenames**: On Linux and Synology NAS, filenames can contain raw byte
  sequences that are not valid UTF-8. The checker MUST detect and report invalid UTF-8 filenames
  without crashing.
- **Symlinks**: Symbolic links inside the target folder MUST NOT be followed during inspection or
  rename operations by default.
- **Empty Directories & Zero-Issue Scans**: Scanning an empty directory or a directory with no
  issues MUST succeed cleanly and clearly report zero findings.
- **Long Paths & Special Characters**: Long paths (including Windows paths exceeding 260
  UTF-16 code units, handled via Win32 extended-length verbatim `\\?\` and `\\?\UNC\` paths) and
  filenames containing spaces, trailing spaces/periods, emojis, or combining marks MUST be
  processed accurately.
- **Ideographic Variation Selectors (IVS/SVS) & Emoji Sequences**: Filenames containing variation
  selectors (`U+FE00–U+FE0F`, `U+E0100–U+E01EF`) MUST be detected and reported when attached to
  non-emoji characters (such as CJK/Hangul/ASCII text) or standing alone, because they create
  visually identical or subtly different glyphs that sort/match inconsistently across OSes.
  However, per Unicode **UTS #51**, `U+FE0E`/`U+FE0F` (VS15/VS16) and `U+200D` (ZWJ) inside valid
  emoji presentation or emoji ZWJ sequences (verified deterministically against pinned Unicode
  `16.0.0` `emoji-data.txt` and `emoji-variation-sequences.txt`, e.g., `❤️` = `U+2764 U+FE0F`,
  `👨‍👩‍👧`) MUST be preserved intact without false-positive warnings.
- **Cross-Script Confusables**: Context-Gated Confusable Detection MUST apply so visually similar
  characters across scripts do not cause false positives on clean files (`SC-002`): (1) standard
  ASCII alphanumerics (`0–9`, `A–Z`, `a–z`, e.g., `0`/`O`, `1`/`l`/`I`) MUST NEVER be flagged as
  confusables; (2) Katakana/Hanzi lookalikes (`エ` vs `工`, `ロ` vs `口`, `カ` vs `力`, `ニ` vs
  `二`, `ハ` vs `八`, `ト` vs `卜`, `タ` vs `夕`, prolonged sound mark `ー` vs `一`) MUST be
  flagged ONLY when isolated inside a contiguous Han or Hangul token without adjacent Hiragana or
  Katakana characters (e.g., `エ作.txt`, `화보ー집.txt`), while legitimate Japanese Kana words
  (such as `エラー.txt`, `カタログ.zip`, `コード.png`) MUST be preserved cleanly; and (3)
  Cyrillic, Greek, or mathematical homoglyphs MUST be flagged when mixed into Latin or CJK/Hangul
  tokens.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: System MUST inspect immediate files and child directories within a user-specified
  target folder (`max_depth = 1` non-recursive default in v1, extensible for future hierarchical
  tree expansion) in read-only mode by default (making zero filesystem modifications unless the
  user explicitly triggers and confirms a rename action).
- **FR-002**: System MUST provide both a standalone CLI executable (for macOS, Windows, Linux,
  and Linux-based Synology NAS, with zero external runtime dependencies) and a desktop GUI
  application (for macOS, Windows, and Linux desktops, requiring no external WebView/browser
  runtime and supporting local directories as well as mounted network/SMB shared folders).
- **FR-003**: System MUST detect CJK Compatibility Ideographs (`U+F900–U+FAFF` and
  `U+2F800–U+2FA1F`) in filenames and folder names, distinguishing characters that have Unicode
  UAX #15 canonical decompositions to CJK Unified Ideographs—which MUST be classified as
  `AutoFixable` and converted to their canonical CJK Unified Ideographs (prioritizing
  cross-platform compatibility over legacy KS X 1001 duplicate-reading distinctions)—from the 12
  standalone unified ideographs in the compatibility block (`U+FA0E`, `U+FA0F`, `U+FA11`,
  `U+FA13`, `U+FA14`, `U+FA1F`, `U+FA21`, `U+FA23`, `U+FA24`, `U+FA27`, `U+FA28`, `U+FA29`) which
  have no canonical decomposition and MUST NOT be falsely flagged as decomposing.
- **FR-004**: System MUST detect Kangxi Radicals (`U+2F00–U+2FD5`) and CJK Radicals Supplement
  (`U+2E80–U+2EF3`) in filenames and folder names deterministically from pinned UCD data
  (`UnicodeData.txt` and `CJKRadicals.txt`), excluding unassigned code points inside those blocks
  (such as `U+2E9A` in `U+2E80–U+2EF3`, and `U+FA6E–U+FA6F`, `U+FADA–U+FAFF` in `U+F900–U+FAFF`)
  because lookup tables are generated strictly from assigned UCD entries, classifying radicals with
  `<compat>` decompositions in `UnicodeData.txt` (all `U+2F00–U+2FD5` Kangxi Radicals plus
  `U+2E9F` `⺟→母` and `U+2EF3` `⻳→龟`) as `AutoFixable`, while routing the remaining
  `CJK Radicals Supplement` (`U+2E80–U+2EF2`) characters mapped only in `CJKRadicals.txt` to
  `ManualReview` with candidate hints from `CJKRadicals.txt` (never applying blanket NFKC, which
  would unintentionally alter width or formatting characters).
- **FR-005**: System MUST detect Unicode normalization anomalies in filenames and folder names,
  identifying non-NFC sequences (such as macOS NFD-decomposed Hangul, NFD-decomposed Japanese
  Kana with combining dakuten/handakuten like `か` + `U+3099`, or decomposed Latin/CJK
  sequences) and reporting the exact NFC-normalized form, as well as UAX #15 defective combining
  character sequences (leading or unmerged combining marks lacking a composable base).
- **FR-006**: System MUST detect Private Use Area (`U+E000–U+F8FF`, `U+F0000–U+FFFFD`,
  `U+100000–U+10FFFD`, specifically identifying macOS SMB SFM-remapped Windows-illegal characters
  in `U+F000–U+F0FF` with replacement hints), non-emoji invisible/zero-width Unicode characters
  (`U+200B` ZWSP, `U+FEFF` BOM, `U+00AD` Soft Hyphen, `U+200E–U+200F`, `U+202A–U+202E`,
  `U+2066–U+2069`, standalone/non-emoji `U+200C`/`U+200D`, and Hangul Fillers `U+115F`, `U+1160`,
  `U+3164`, `U+FFA0`), Unicode replacement characters (`U+FFFD`), invalid UTF-8 byte sequences (on
  Unix/NAS) and unpaired UTF-16 surrogates (on Windows NTFS), and non-emoji Ideographic/Standard
  Variation Selectors (`U+FE00–U+FE0D`, non-emoji/standalone `U+FE0E–U+FE0F`, and
  `U+E0100–U+E01EF`) in filenames—preserving valid Unicode UTS #51 emoji presentation
  (`U+FE0E`/`U+FE0F`) and emoji ZWJ (`U+200D`) sequences verified via pinned Unicode `16.0.0`
  `emoji-data.txt` and `emoji-variation-sequences.txt`.
- **FR-007**: System MUST detect cross-script visually confusable characters using
  Context-Gated Confusable Detection (flagging Katakana lookalikes ONLY when isolated inside a
  contiguous Han or Hangul token without adjacent Hiragana or Katakana characters—preserving clean
  Japanese Kana words such as `エラー.txt`—flagging Cyrillic/Greek/mathematical homoglyphs when
  mixed into Latin or CJK/Hangul tokens, and NEVER flagging standard ASCII alphanumerics `0`/`O` or
  `1`/`l`/`I`), fullwidth ASCII variants, halfwidth Katakana, Japanese Wave Dash `U+301C` vs.
  Fullwidth Tilde `U+FF5E` mismatches, Ideographic Space `U+3000` vs. ASCII space, non-NFC Japanese
  voiced/semi-voiced sound marks including standalone spacing marks `U+309B` `゛` / `U+309C` `゜`,
  halfwidth Katakana marks `U+FF9E` `ﾞ` / `U+FF9F` `ﾟ`, and non-composable combining
  `U+3099`/`U+309A` marks remaining after NFC with candidate fullwidth Kana replacement hints,
  isolated/unpaired Conjoining Hangul Jamo `U+1100–U+11FF`/`U+A960–U+A97F`/`U+D7B0–U+D7FF` and
  Halfwidth Hangul Jamo `U+FFA1–U+FFDC` remaining uncomposed after NFC with `HangulJamo`
  compatibility-Jamo `U+3131–U+318E` hints, and defective combining character sequences lacking a
  composable base, and flag them with code-point inspection details.
- **FR-008**: System MUST analyze sibling entries within each scanned directory for cross-platform
  sorting discrepancies using embedded, OS-independent collation profiles—at minimum: Korean
  Windows Explorer order (`WindowsKoKr`: `Whitespace -> Symbols/Punctuation (with Win32
  StrCmpLogicalW Word-Sort secondary weighting for '-' and '\'') -> Numbers (0–9, natural numeric)
  -> Latin (A–Z) -> Hangul + Korean Hanja interleaved by reading -> Kana`, case-insensitive) as
  the primary reference baseline, macOS Korean Finder order (`MacOsKo`: CLDR `ko.xml` `[reorder Hang
  Hani]`, `Whitespace -> Symbols/Punctuation -> Numbers (0–9) -> Hangul + Korean Hanja
  interleaved by reading -> Latin (A–Z) -> Kana`), raw UTF-8 code-point order (Linux/Synology
  `C.UTF-8`), and Japanese/Chinese locale-aware CJK collation
  profiles—reporting $O(N)$-bounded adjacent boundary inversion pairs (preventing $O(N^2)$ memory
  blowup on large directories) and classifying each discrepancy's `cause` as `EncodingAnomaly`
  (fixable via normalization/replacement) vs `InherentPlatformCollation` (unfixable OS script or
  locale ordering difference). Default scan warnings and exit code `1` MUST trigger only on
  `EncodingAnomaly` sort discrepancies (where character/encoding anomalies distort sort order
  relative to the clean normalized name), while `InherentPlatformCollation` differences on clean
  filenames remain informational-only when switching comparison profiles and MUST NOT produce
  false-positive warnings or exit code `1` (preserving `SC-002`).
- **FR-009**: System MUST detect potential cross-platform filename collisions within each
  directory—specifically sibling entries whose names are distinct on case-sensitive or
  normalization-preserving filesystems (Linux/Synology) but collide under NFC normalization,
  canonical compatibility folding, or case-insensitive matching (macOS/Windows/SMB).
- **FR-010**: System MUST generate non-destructive remediation suggestions for each flagged entry,
  composing all `AutoFixable` character fixes across multiple findings on the same entry into one
  unified per-entry `EntryRenamePlan` suggested canonical name without altering the filesystem,
  followed by a **Post-Composition Target Validation Gate**: if any generated `suggested_name` or
  `composed_suggested_name` (such as after stripping `InvisibleZeroWidth`) yields an empty string
  (`""`), `"."`, `".."`, or exposes a `WindowsSmbHazard` (for example `"CON\u{200B}.txt"` →
  `"CON.txt"`, `"folder.\u{200B}"` → `"folder."`, `"name \u{200B}"` → `"name "`, or exceeding
  255 UTF-8 bytes / 255 UTF-16 units), the finding and `EntryRenamePlan` MUST automatically downgrade
  to `ManualReview` (`suggested_name: None`, `composed_suggested_name: None`) and emit the exposed
  `WindowsSmbHazard` finding so it is never auto-applied.
  If an entry has ANY `ManualReview` finding alongside `AutoFixable` findings (and passes target
  safety validation), the entry's `EntryRenamePlan.track` MUST deterministically resolve to
  `ManualReview` (blocking unattended `namechecker fix --yes` auto-renaming), while
  `composed_suggested_name` holds the partial `AutoFixable`-cleaned draft (`None` when no
  `AutoFixable` fixes apply or on `InvalidUtf8` / `OrphanedTempFile`) as the starting value for
  interactive manual review.
- **FR-011**: System MUST support both human-readable terminal reports (defaulting to Korean
  explanations with an option to switch to English, grouped/summarized with severity and
  code-point breakdown, and sanitizing/escaping control, ANSI/OSC escape, and Bidi override
  characters into visible safe tokens to prevent terminal injection or visual spoofing) and
  machine-readable structured output (JSON and CSV with fixed English field names).
- **FR-012**: System MUST support path exclusion patterns (e.g., glob patterns), traversal depth
  control (defaulting to immediate directory entries `max_depth = 1` in v1), severity/category
  filtering, and safe symlink handling (no symlink following by default).
- **FR-013**: System MUST handle permission errors or unreadable entries gracefully by recording
  the error, continuing the scan of remaining entries, and returning distinct exit codes for
  clean scans, scans with findings, and execution/I/O errors.
- **FR-014**: System MUST support user-confirmed safe renaming of selected flagged entries in the
  target folder to their canonical names (including on writable mounted network/SMB shares),
  performing pre-rename target-collision checks—verifying both (a) against existing unchanged
  sibling entries on disk (using a cached per-`parent_dir` sibling index and NFC + case-folding
  `normalize_nfc(...).to_lowercase()` matching, propagating any `read_dir`/`symlink_metadata`
  I/O or permission error via `map_io_status()` rather than misclassifying it as
  `BlockedCollision`) and (b) pairwise deduplication (under NFC + case-folding) among all
  target `new_name`s within the selected rename batch—and target-length checks (blocking names
  exceeding 255 UTF-8 bytes `NAME_MAX` on Unix/APFS/NAS or 255 UTF-16 code units on Windows),
  routing all Win32 filesystem reads and writes uniformly through the extended-length verbatim
  namespace `\\?\C:\...` and `\\?\UNC\server\share\...` (`scanner.rs` `fs::read_dir`/
  `fs::metadata`/`fs::canonicalize` while constructing logical report paths via `dir.join(&os_name)`
  so `\\?\` never leaks into reports; `renamer.rs` `fs::read_dir`/`symlink_metadata`/
  `volume_key_for_dir`/`MoveFileExW`/`CreateFileW`; and audit-log I/O so Win32 path normalization
  never strips trailing spaces/periods or fails on `>260`-unit `MAX_PATH` paths),
  trying atomic non-overwriting OS rename primitives first (`renameat2` with `RENAME_NOREPLACE` on
  Linux, `renamex_np` with `RENAME_EXCL` on macOS, `MoveFileExW` without
  `MOVEFILE_REPLACE_EXISTING` on Windows—never falling back to overwriting `std::fs::rename`
  (`ReplaceIfExists = TRUE`) on Windows),
  falling back—ONLY when a Unix network/SMB mount (`smbfs`/`cifs`) returns `ENOTSUP`, `EINVAL`,
  or `ENOSYS` for the exclusive flag—to an immediate pre-rename
  `symlink_metadata(dst)` `NotFound` (`ENOENT`) verification prior to `std::fs::rename` (with the
  residual TOCTOU window on mounts lacking kernel exclusive rename primitives explicitly
  documented), using a two-step temporary rename (`old` → temporary name → `new`)
  for case-only, NFD→NFC normalization-only, and canonical-equivalence changes on case- or
  normalization-insensitive filesystems (APFS/HFS+/NTFS/SMB) with bounded exponential-backoff
  retries (up to 5 retries / 6 total attempts: 25ms, 50ms, 100ms, 200ms, 400ms) on transient sharing violations (`ERROR_SHARING_VIOLATION`
  / `EBUSY`) during Step 2 and Step-1 rollback plus automatic Step-1 rollback (`temp` → `old`) and
  `temp_name` audit logging if Step 2 still fails (and flagging any orphaned `.__nc_tmp_<uuid>`
  file left behind by an interrupted two-step rename during subsequent scans as an
  `OrphanedTempFile` `ManualReview` warning instructing the user to run `namechecker undo` or
  GUI `되돌리기`), verifying post-rename on-disk bytes for NFD→NFC normalization targets
  (scoped to NFD→NFC changes with cached sibling state to avoid $O(M \times N)$ directory scans)
  so that if an underlying volume (such as legacy macOS HFS+) forcibly decomposes
  NFC back to NFD at the driver layer the system records `FilesystemForcedNfd`, warns the user,
  and halts redundant NFD→NFC renames on that volume, never following symbolic links during child
  rename operations while validating `parent_dir` via `fs::metadata()` during `undo` (allowing
  valid symlinked directory mount points such as `~/nas_share -> /Volumes/NAS`), handling write
  permission errors safely, enforcing the non-UTF-8 `parent_dir` audit-log check identically across
  `--dry-run` and live modes as well as verifying `audit_log_path` lies outside every
  `item.parent_dir`, generating unique collision-free default audit-log filenames (preventing
  1-second overwrite races), recording `file_id` (inode / Windows FileId) paired with fallback
  metadata `(entry_type, byte_size, mtime_ns)` in the audit log so `undo` verifies `file_id` on
  local filesystems while falling back to `(entry_type, byte_size, mtime_ns)` verification (or
  explicit `--force`) when synthetic `st_dev:st_ino` values change on SMB mounts (`smbfs`/`cifs`)
  or across remounts, and atomically flushing the reversible rename audit log (via sibling
  temporary file + rename) stored outside the target scan folder.
- **FR-015**: System MUST detect cross-platform Windows/SMB and NAS filename hazards (reserved
  characters `<>:"/\|?*`, control characters `U+0000–U+001F`, trailing spaces or periods, reserved
  Windows device names `CON`, `PRN`, `AUX`, `NUL`, `COM1–9`, `LPT1–9` matched case-insensitively
  both as bare names and when followed by an extension such as `CON.txt` or `nul.mp3`, and
  single-component filename length overflow exceeding 255 UTF-8 bytes on Unix/macOS/Synology NAS
  `ext4`/`btrfs` or 255 UTF-16 code units on Windows NTFS—including when 2-byte `CP949`/`Shift_JIS`
  legacy decoding candidates expand into 3-byte UTF-8).
- **FR-016**: System MUST separate findings deterministically across all 12 `IssueCategory` values
  into two distinct UX tracks: (1) `AutoFixable` conversions eligible for batch preview and
  confirmed renaming (`NonNfcNormalization` → UTF-8 NFC; canonical `CompatibilityIdeograph` →
  Unified Ideograph, excluding the 12 UAX #15 standalone unified ideographs; UCD `<compat>`
  `KangxiRadical` `U+2F00–U+2FD5`, `U+2E9F`, `U+2EF3`; and non-emoji `InvisibleZeroWidth` removal
  when validated safe by the Post-Composition Target Validation Gate),
  and (2) `ManualReview` items (`CJKRadicals.txt`-only `KangxiRadical` supplement `U+2E80–U+2EF2`
  excluding `U+2E9F`/`U+2E9A`, `PunctuationVariant` `U+301C`/`U+FF5E`/`U+3000`, non-emoji
  `VariationSelector`, `CrossScriptConfusable` incl. isolated/halfwidth Hangul Jamo and non-NFC Kana
  dakuten, `PrivateUseArea` incl. SFM `U+F000–U+F0FF`, `InvalidUtf8`, `ReplacementChar`,
  `WindowsSmbHazard`, `OrphanedTempFile`, and any finding downgraded by the Post-Composition Target
  Validation Gate) for which the system provides candidate hints (including legacy CJK encoding
  detection such as CP949/EUC-KR, Shift-JIS, and GB18030 for non-UTF-8 bytes, `HangulJamo`,
  `KanaDakuten`, or `undo` recovery hints for `OrphanedTempFile`), supports user-entered manual
  renames, and allows registering intentional names in an ignore list to suppress repeated
  warnings. Numeric sort-order prefixes are out of scope.
- **FR-017**: The Desktop GUI MUST invoke native folder selection (`rfd`) on the main UI thread
  while executing filesystem scan and rename tasks asynchronously on a background worker thread
  (keeping the UI responsive on high-latency SMB shares), sanitize/escape control and Bidi
  override characters into visible safe tokens across all table cells and status banners, and
  provide a top action toolbar (`폴더 선택` Folder Select, `점검` Inspect, `수정` Fix/Rename,
  `되돌리기` Undo, sort-profile selector, and `한국어`/`English` language switcher) plus a
  responsive status banner (`status_banner`) for Rename/Undo completion that surfaces separated
  localized `Applied`, `FilesystemForcedNfd` warning, blocked, and failed counts alongside
  `sanitize_terminal_text()`-escaped, bounded-length sample `error_detail` snippets (`<= 2`
  samples, `<= 120` chars each + remaining count), preserves `"완료"`/`"Renamed"` and
  `"복구"`/`"Restored"` cues, dynamically re-localizes when `set_language()` switches languages, and
  updates `last_audit_log_path` only when at least one operation is recoverable, above a 3-pane
  main view: (1) **Left Pane** showing the target folder's files and folders in the current
  platform's sort order (Before Fix), (2) **Center Pane** showing aggregated counts by
  encoding/normalization type (UTF-8 NFC, UTF-8 NFD, non-UTF-8 legacy candidates) and anomaly
  category alongside the two-track diagnostic/edit list (bounding `diagnostic_rows_table` height via
  `max_scroll_height` only when an actual flagged row is selected `selected_row.is_some()`, never
  shrinking on clean-file selection, and wrapping `Selected Entry Details` in a vertical
  `ScrollArea`), and (3) **Right Pane** showing the real-time predicted sort order after applying
  selected fixes (After Fix) with rank-change indicators (`▲`/`▼`), post-fix collision warnings
  (`has_collision` blocking `[수정]`), and synchronized cross-pane row highlighting.

### Key Entities

- **ScanTarget**: Represents a user-provided root directory or file path along with scan options
  (recursion depth, exclusion patterns, enabled check categories, target platform/locale profiles).
- **EntryFinding**: Represents a single detected issue on a file or directory, including full path,
  entry type (file/directory), issue category, severity level, offending character positions and
  Unicode code points, human-readable explanation, and suggested canonical name.
- **EntryRenamePlan**: Aggregates multiple findings on the same entry into a single composed
  suggested target name, combined remediation track, and linked finding IDs.
- **SortDiscrepancy**: Represents an $O(N)$-bounded adjacent boundary sibling pair within a parent
  directory whose relative sort order diverges between collation profiles, classified by `cause`
  (`EncodingAnomaly` vs `InherentPlatformCollation`).
- **CollisionRisk**: Represents two or more sibling entries in the same directory that are distinct
  byte sequences on the current filesystem but resolve to an identical name under target
  platform normalization or case-folding rules.
- **ScanReport**: Aggregated output of a scan run, containing summary counts (total entries
  scanned, findings by category and severity, sort discrepancies, collision risks, skipped/errored
  paths) and detailed records.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: Given a test corpus of 100 known problematic CJK filenames (covering Kangxi radicals,
  compatibility ideographs, NFD/NFC Hangul, variation selectors, PUA, invalid UTF-8, and Katakana
  lookalikes), the checker detects 100% of injected anomalies with zero false negatives.
- **SC-002**: Given a directory of 1,000 clean, standard NFC filenames (mixed ASCII, Hangul,
  Kana, and standard CJK Unified Ideographs), the checker produces zero false-positive character
  anomaly findings.
- **SC-003**: Users can scan a target directory of 50,000 files and folders in under 5 seconds on
  a typical local SSD, producing a complete summary and structured report.
- **SC-004**: Every flagged entry includes exact Unicode code-point details and a deterministic
  canonical name suggestion so users can immediately identify and resolve the issue without
  guessing which character caused the mismatch.
- **SC-005**: The checker executes identically (producing consistent findings for the same
  logical test corpus) across macOS, Windows, Linux, and Synology NAS Linux environments without
  requiring external runtime installations.

## Assumptions

- **Single-Folder Default Scope in v1 with Future GUI Tree Expansion**: v1 checks and fixes
  immediate files and child folders by default (`max_depth = 1`) to prioritize performance,
  predictability, and rename safety, while the CLI already supports opt-in `--max-depth <N>`
  (`> 1`) and cycle-safe `--follow-symlinks` recursive traversal in v1; only the GUI collapsible
  hierarchical subdirectory tree inspection UI is deferred to future expansion once baseline
  performance and security are validated.
- **Read-Only Default with Explicit Safe Rename**: v1 operates in read-only inspection mode by
  default to prevent accidental data loss on shared drives and NAS volumes; actual renaming
  requires explicit user selection and confirmation, pre-rename collision checks, and audit
  logging.
- **Regional Script Variants Out of Scope for v1**: Distinct regional character forms (such as
  Traditional Chinese `國`, Simplified Chinese `国`, or Japanese Shinjitai `竜` vs `龍`) are
  treated as intentional linguistic variants and are excluded from v1 anomaly detection.
- **Dual CLI & Desktop GUI Interface**: v1 delivers both a CLI (for headless NAS/server
  execution via SSH or scheduled tasks and automated scripting) and a desktop GUI (for visual
  folder selection, mounted NAS share inspection, and interactive review on Windows/macOS/Linux).
- **Unicode Standard Baseline**: Character classification, canonical decomposition, and confusable
  mappings follow the official Unicode Character Database (UCD) and Unicode Security Mechanisms
  (UTS #39) / Collation Algorithm (UCA) standards rather than ad-hoc heuristics.
- **Locale-Dependent Hanzi Sorting**: For standard CJK Unified Ideographs (e.g., `日`, U+65E5)
  where the exact same code point sorts differently in Korean (`일`), Japanese (`ひ`/`にち`), and
  Chinese (`rì`), no single "correct" character replacement exists; the checker therefore reports
  this as a cross-locale sort divergence rather than an invalid character encoding.
