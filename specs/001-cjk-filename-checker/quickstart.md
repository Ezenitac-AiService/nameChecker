# Quickstart & Validation Guide: CJK Cross-Platform Filename & Folder Checker

**Feature**: `001-cjk-filename-checker` | **Date**: 2026-11-18

## Prerequisites

- Rust toolchain (`rustc` / `cargo` >= 1.85; verified on `rustc 1.100.0`)
- Optional cross-compilation targets for Synology NAS: `x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl`

## Validation Scenarios

### 1. Run Full Test Suite (TDD Gate)
```bash
cargo test --workspace
```
Expected: All unit, corpus (100+ injected CJK anomalies, 1,000 clean NFC names), collation, collision, CLI contract, and safe-rename integration tests pass with zero skipped assertions.

### 2. Read-Only CLI Scan on Sample Directory
```bash
# Default scan shorthand (equivalent to `namechecker scan /path/to/target`)
cargo run -p namechecker-cli -- /path/to/target --format text

# Filter by specific issue category
cargo run -p namechecker-cli -- scan /path/to/target --category KangxiRadical --format text
```
Expected:
- Outputs Korean summary grouped into **Auto-Fixable** (`자동 수정 제안`) and **Manual Review Required** (`수동 확인 필요`), plus **Sort Discrepancies** and **Collision Risks**.
- Exits `0` if clean (or only `InherentPlatformCollation` profile differences exist on clean names), `1` if findings/`EncodingAnomaly`/collisions exist, `2` on invalid path or I/O errors.

### 3. Structured JSON & CSV Export + Ignore Rules
```bash
cargo run -p namechecker-cli -- scan /path/to/target --format json --output report.json
cargo run -p namechecker-cli -- scan /path/to/target --format csv --output report.csv
cargo run -p namechecker-cli -- ignore "노트〜초안.txt" --ignore-file ignore.json
```
Expected: Produces `report.json` conforming to [report-schema.json](file:///Users/dev/DEV/nameChecker/specs/001-cjk-filename-checker/contracts/report-schema.json) with fixed English keys, RFC 4180 UTF-8 BOM `report.csv` with 12 English header columns, and portable `ignore.json` rules.

### 4. Safe Preview & Confirmed Rename + Undo
```bash
# Preview only (no filesystem changes)
cargo run -p namechecker-cli -- fix /path/to/target --dry-run

# Apply confirmed AutoFixable renames (defaults audit log outside target tree to
# `<state_dir>/namechecker-rename-audit-<timestamp>-<unique>.json` unless `--audit-log` is specified)
cargo run -p namechecker-cli -- fix /path/to/target --yes --audit-log rename-log.json

# Revert applied renames
cargo run -p namechecker-cli -- undo rename-log.json
```
Expected: Selected entries in the target folder are renamed safely (using two-step temporary renames where needed); colliding targets are blocked; `undo` restores original names.

### 5. Launch Desktop GUI
```bash
cargo run -p namechecker-gui
```
Expected: Opens native 3-pane desktop window (`eframe`/`egui`) with Korean/Japanese/Chinese Pan-CJK font fallback, top action toolbar (`폴더 선택` supporting local drives and mounted SMB/NAS shares, `점검`, `수정`, `되돌리기`, sort-profile selector, and `한국어`/`English` language switcher), Left Pane (`before_order` current platform sort order), Center Pane (encoding/category counts + two-track `AutoFixable`/`ManualReview` diagnostic/edit table with ignore-list toggle), and Right Pane (real-time debounced `predicted_after_order` with `▲`/`▼` rank deltas, `has_collision` badges blocking `[수정]` on conflicts, and scrollable `SortDiscrepancy` details).
