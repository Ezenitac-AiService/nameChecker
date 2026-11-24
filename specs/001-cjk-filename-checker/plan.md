# Implementation Plan: CJK Cross-Platform Filename & Folder Checker

**Branch**: `001-cjk-filename-checker` | **Date**: 2026-11-18 | **Spec**: [spec.md](file:///Users/dev/DEV/nameChecker/specs/001-cjk-filename-checker/spec.md)

**Input**: Feature specification from [spec.md](file:///Users/dev/DEV/nameChecker/specs/001-cjk-filename-checker/spec.md)

## Summary

Build a cross-platform Rust workspace (`namechecker`) delivering both a standalone CLI (`namechecker-cli`, including static `musl` builds for Synology NAS) and a pure-Rust desktop GUI (`namechecker-gui` via `eframe`/`egui`), powered by a shared core engine (`namechecker-core`). The engine inspects filenames and folder names for CJK Unicode anomalies (Kangxi Radicals, canonical CJK Compatibility Ideographs excluding the 12 UAX #15 non-decomposing unified ideographs, NFD/NFC mismatches, PUA, variation selectors, cross-script confusables, invalid UTF-8 bytes with legacy encoding hints, and Windows/SMB hazards), compares sibling sort orders across embedded OS-independent collation profiles (`WindowsKoKr`, `MacOsKo`, `LinuxCodePoint`, `Japanese`, `Chinese`), detects cross-platform collision risks, and supports a two-track remediation UX (batch-previewed `AutoFixable` renames + `ManualReview` hints/edits/ignore list) with pre-flight collision checks and reversible audit logs.

## Technical Context

**Language/Version**: Rust 2024 / `rustc 1.100.0` (`cargo 1.100.0`)

**Primary Dependencies**:
- Core: `icu_normalizer` (`"2.4"`, paired with build-time `BMP_ASSIGNED_MASK` / `ASSIGNED_UNICODE_16_RANGES` tables generated in `build.rs` from pinned `UnicodeData.txt` `16.0.0` and an `is_assigned_unicode_16()` runtime barrier in `unicode_data.rs`, verified by automated Unicode `16.0.0` parity assertions), `icu_collator` + embedded `.postcard` CLDR data (`ko`, `ja`, `zh`), `encoding_rs`, `chardetng`, `globset`, `serde`, `serde_json`, `csv`, `thiserror`, `chrono`, `uuid`, atomic non-overwriting OS rename (`libc` on Unix, `windows-sys` aligned to `0.61` (including `Win32_Security` for `CreateFileW`) on Windows)
- CLI: `clap` (derive)
- GUI: `eframe` (`egui`), `egui_extras` (`TableBuilder` virtual scrolling), `rfd` (native folder dialog), system/CJK font fallback loader
- Testing: `tempfile`, `assert_cmd`, `predicates`, `egui_kittest`

**Storage**: Local filesystem (read-only by default; optional ignore-list JSON and rename audit-log JSON files)

**Testing**: `cargo test --workspace` (TDD Red-Green-Refactor mandatory; unit, corpus, collation, collision, CLI contract, and filesystem integration tests)

**Target Platform**: macOS (Apple Silicon / Intel), Windows (x86_64), Linux (x86_64 / aarch64), Synology NAS Linux (`x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl`)

**Project Type**: Multi-crate Rust workspace (`core` library + `cli` binary + `gui` desktop application)

**Performance Goals**: Scan 50,000 local SSD entries in <5 seconds (SC-003) using single-folder default traversal (`max_depth = 1`, optional `--max-depth > 1` + cycle-safe `--follow-symlinks` with Windows `(dwVolumeSerialNumber, nFileIndex)` cycle fallback, extensible to `jwalk`), `O(N log N)` rank sorting, statically baked `CollatorBorrowed` instances, per-`parent_dir` sibling caching in `apply_safe_renames()`, and atomic audit-log writes

**Constraints**: Zero external runtime dependencies for `namechecker-cli` (static `musl` on Linux/Synology NAS) and no WebView/browser runtime for `namechecker-gui` (uses standard OS desktop graphics/windowing libraries); no host OS locale dependency; strictly read-only unless user explicitly confirms rename; lossless non-UTF-8/unpaired-surrogate handling

**Scale/Scope**: Single-node local drives and mounted NAS/SMB shares up to 100k+ files per scan

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

- **I. Bilingual Communication & Documentation Policy**:
  - Pre-design: PASS — All spec, research, data model, contracts, quickstart, and plan documents authored in English; user communication in Korean.
  - Post-design: PASS — Verified all Phase 0/1 artifacts are in English.
- **II. Test-Driven Development (NON-NEGOTIABLE)**:
  - Pre-design: PASS — Every module and requirement (`FR-001`–`FR-017`, `SC-001`–`SC-005`) will follow Red-Green-Refactor with failing tests written before implementation.
  - Post-design: PASS — `quickstart.md` and workspace test layout enforce unit, corpus, CLI (`cli_scan_mvp_test.rs`, `cli_test.rs`, `cli_fix_undo_test.rs`), GUI (`state_test.rs`, `ui_kittest.rs`), and integration tests across all 3 crates.
- **III. Implementation Integrity & Zero Workarounds (NON-NEGOTIABLE)**:
  - Pre-design: PASS — Core logic uses real Unicode UCD/UAX #15/UAX #39 tables, real ICU4X collation data, and real `tempfile` filesystem tests; zero mocks or hardcoded test shortcuts.
  - Post-design: PASS — No mocked core logic or hardcoded test outputs anywhere in the design.
- **IV. Evidence-Based Research & Adversarial Multi-Persona Verification (NON-NEGOTIABLE)**:
  - Pre-design: PASS — Unicode ranges, ICU4X/CLDR collation, Win32/macOS/Linux filesystem rename semantics, and empirical Windows 11 / macOS Finder behavior are researched and cited in `research.md`.
  - Post-design: PASS — Cross-artifact constrained fields and schemas are reconciled 1:1 across `spec.md`, `data-model.md`, `contracts/`, and `tasks.md`, and audited via adversarial multi-persona `/speckit-analyze` reviews.
- **V. Scope Discipline & Non-Destructive Default (NON-NEGOTIABLE)**:
  - Pre-design: PASS — Inspection defaults to read-only (`max_depth = 1`), separating deterministic `AutoFixable` items from `ManualReview` items and preserving non-destructive defaults.
  - Post-design: PASS — All filesystem mutations enforce pre-flight collision, length, and non-symlink checks, atomic non-overwriting rename with guarded SMB fallback, two-step rollback, `file_id` + fallback `undo`, external audit logging, and terminal/CSV/GUI injection sanitization.

## Project Structure

### Documentation (this feature)

```text
specs/001-cjk-filename-checker/
├── plan.md              # This file (/speckit-plan command output)
├── research.md          # Phase 0 output
├── data-model.md        # Phase 1 output
├── quickstart.md        # Phase 1 output
├── contracts/
│   ├── cli-contract.md
│   └── report-schema.json
├── checklists/
│   └── requirements.md
└── tasks.md             # Phase 2 output (/speckit-tasks command)
```

### Source Code (repository root)

```text
Cargo.toml
crates/
├── namechecker-core/
│   ├── Cargo.toml
│   ├── build.rs              # Generates pinned Unicode 16.0.0 UCD/UTS #51/KS X 1001 lookup tables
│   ├── src/
│   │   ├── lib.rs
│   │   ├── model.rs          # ScanTarget, EntryFinding, EntryRenamePlan, SortDiscrepancy, CollisionRisk, ScanReport
│   │   ├── unicode_data.rs   # Pinned Unicode 16.0.0 UCD tables (UAX #15 12 exceptions, Kangxi, CJKRadicals, UAX #39, UTS #51 emoji sets, KS X 1001)
│   │   ├── detector.rs       # Character, normalization, UTF-8, Windows/SMB/NAME_MAX hazard checks & EntryRenamePlan composer
│   │   ├── encoding_hint.rs  # Legacy CJK byte decoding hints (CP949/Shift-JIS/GB18030/Big5)
│   │   ├── collation.rs      # Embedded OS-independent sort profiles & DiscrepancyCause detector
│   │   ├── collision.rs      # Cross-platform NFC/compatibility/case collision detector
│   │   ├── ignore_list.rs    # Persistent ManualReview ignore-list store & matcher
│   │   ├── scanner.rs        # Single-folder traversal abstraction + exclusion + ignore list
│   │   ├── renamer.rs        # Pre-flight collision/length/symlink checks, atomic non-overwriting two-step rename, file_id/metadata audit log & undo
│   │   ├── report.rs         # Localized text, JSON, CSV formatters
│   │   └── i18n.rs           # Korean (default) & English message catalog
│   └── tests/
│       ├── model_test.rs        # Domain model serialization & constraint tests
│       ├── i18n_test.rs         # Korean/English catalog tests
│       ├── unicode_data_test.rs # UAX #15/UAX #39/CJKRadicals/KS X 1001 lookup tests
│       ├── corpus_test.rs       # SC-001 (100+ anomalies) & SC-002 (1000 clean NFC)
│       ├── encoding_hint_test.rs# Legacy byte detection, 1.5x length check, & decoding hints
│       ├── scanner_us1_test.rs  # Single-folder max_depth=1 & symlink non-traversal tests
│       ├── collation_test.rs    # Cross-profile & native OS sort discrepancy verification
│       ├── collision_test.rs    # NFC/NFD, compatibility, case collision tests
│       ├── scanner_us2_test.rs  # before_order, predicted_after_order, EntryRenamePlan, cause
│       ├── report_test.rs       # Sanitized text, JSON schema, flattened CSV BOM tests
│       ├── renamer_test.rs      # Atomic no-replace rename, two-step temp rename, SMB file_id/fallback undo
│       ├── ignore_list_test.rs  # Persistent ignore-list suppression tests
│       └── perf_test.rs         # SC-003 50,000-entry <5s performance benchmark
├── namechecker-cli/
│   ├── Cargo.toml
│   ├── src/
│   │   └── main.rs           # clap CLI: scan, fix, undo, ignore + exit codes (2 > 1 > 0)
│   └── tests/
│       ├── cli_scan_mvp_test.rs  # MVP scan CLI tests
│       ├── cli_test.rs           # Full scan filtering & export contract tests
│       └── cli_fix_undo_test.rs  # fix, undo, ignore CLI contract tests
└── namechecker-gui/
    ├── Cargo.toml
    ├── src/
    │   ├── lib.rs            # Library exports (`app`, `state`) for headless state/UI tests
    │   ├── main.rs           # eframe binary entry point launching `NameCheckerApp`
    │   ├── app.rs            # GUI layout, Pan-CJK fallback font setup, two-track UI, ignore toggle, rename/undo confirm
    │   └── state.rs          # Testable view-model state transitions (headless unit testable)
    └── tests/
        ├── state_test.rs     # Headless GUI state & two-track workflow tests
        └── ui_kittest.rs     # Headless egui_kittest 3-pane UI & AccessKit tests
```

**Structure Decision**: A 3-crate Cargo workspace (`namechecker-core`, `namechecker-cli`, `namechecker-gui`) cleanly isolates pure domain logic from CLI and desktop GUI layers, enables static `musl` compilation of `namechecker-cli` for Synology NAS without pulling in graphics dependencies, and keeps 100% of core and view-model logic unit-testable under TDD.

## Complexity Tracking

> No Constitution Check violations; standard 3-crate Rust workspace matches the dual CLI + Desktop GUI requirement directly.
