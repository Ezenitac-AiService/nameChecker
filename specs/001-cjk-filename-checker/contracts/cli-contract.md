# CLI Interface Contract: `namechecker`

**Feature**: `001-cjk-filename-checker` | **Date**: 2026-11-18

## Commands

### 1. `namechecker scan <PATH>` (Default subcommand)
Scans a target folder in read-only mode and reports findings. Invoking `namechecker <PATH> [scan options]` without an explicit subcommand defaults to `scan` (implemented via `clap` 4 `args_conflicts_with_subcommands = true` + `Option<Commands>`). Directories literally named `scan`, `fix`, `undo`, or `ignore` are disambiguated with `./fix` or `namechecker scan fix`; invoking `namechecker` with neither a subcommand nor `<PATH>` exits with code `2`.

**Options**:
- `--format <text|json|csv>` (default: `text`; `json` conforms to [report-schema.json](file:///Users/dev/DEV/nameChecker/specs/001-cjk-filename-checker/contracts/report-schema.json); `csv` emits flattened `CsvReportRow` records with UTF-8 BOM, RFC 4180 quoting, spreadsheet formula-prefix escaping, and fixed English header columns—always emitted even when `findings` is empty: `path,parent_dir,raw_name,entry_type,track,category,severity,offending_codepoints,suggested_name,composed_suggested_name,candidate_hints,message`)
- `--output <FILE>` (optional; writes report to file instead of stdout)
- `--lang <ko|en>` (default: `ko` for text mode; JSON/CSV keys always English)
- `--max-depth <N>` (default: `1` in v1 — immediate files and child folders of target directory)
- `--exclude <GLOB>` (repeatable; defaults include `@eaDir`, `#recycle`, `.DS_Store`, `._*`, `Thumbs.db`, `.git`)
- `--no-default-excludes` (disable built-in NAS/OS metadata exclusions)
- `--follow-symlinks` (default: `false`)
- `--profiles <LIST>` (comma-separated: `windows-ko`, `macos-ko`, `linux-c`, `ja`, `zh`; default: all)
- `--severity <error|warning|info>` (minimum severity filter)
- `--category <CATEGORY>` (repeatable `IssueCategory` filter; updates `auto_fixable_count`, `manual_review_count`, `category_counts`, `rename_plans`, and `before_order.has_issue`)
- `--track <all|auto|manual>` (filter by remediation track)
- `--ignore-file <PATH>` (path to ignore-list JSON file)

**Exit Codes** (precedence: `2` > `1` > `0`):
- `0`: Scan completed with zero findings, zero `EncodingAnomaly` sort discrepancies, zero collision risks, and zero I/O errors (pure `InherentPlatformCollation` differences on clean NFC filenames do not trigger exit code `1`).
- `1`: Scan completed with one or more character/encoding findings, `EncodingAnomaly` sort discrepancies, or collision risks (and zero execution/I/O errors).
- `2`: Execution error (invalid arguments, nonexistent root path, or I/O/permission errors encountered—takes precedence even if findings are also present).

### 2. `namechecker fix <PATH>`
Interactive or confirmed batch rename for `AutoFixable` findings in the target folder.

**Options**:
- `--yes` (explicit confirmation flag required for non-interactive batch application of `AutoFixable` items)
- `--dry-run` (preview without writing; prints exact `old -> new` plan, while enforcing the same pre-flight safety gates as live `fix`—target-external `--audit-log` validation against the scanned target root even when there are zero eligible `AutoFixable` items or `--max-depth > 1`, plus UTF-8 `parent_dir` validation—exiting `2` on violation without printing a rename plan or creating files/directories on disk; without `--dry-run` or `--yes`, prompts interactively for confirmation)
- `--audit-log <FILE>` (default: OS user data/state directory `namechecker-rename-audit-<timestamp>-<unique>.json`, outside the scanned target tree)
- Inherits `--exclude`, `--no-default-excludes`, `--max-depth`, `--lang`, `--ignore-file`

**Safety Guarantees**:
- Never auto-renames `ManualReview` items.
- Never follows symbolic links when renaming.
- Performs pre-flight collision, symlink, and two-step temporary rename checks on target folder entries.

**Output & Exit Codes**:
- Both `--dry-run` and live `fix` print localized per-operation block/failure reasons (`BlockedCollision`, `BlockedSymlink`, `PermissionDenied`, `IoError`, `FilesystemForcedNfd`) and warn explicitly on `FilesystemForcedNfd`.
- Exit codes: `0` when all eligible renames succeed cleanly; `1` when aborted by user or when any operation is blocked, fails, or triggers `FilesystemForcedNfd`; `2` on execution, scan, or I/O errors, including pre-flight `--audit-log` inside-target or non-UTF-8 `parent_dir` violations in both `--dry-run` and live `fix`.

### 3. `namechecker undo <AUDIT_LOG_FILE> [--dry-run] [--force] [--lang <ko|en>]`
Reverts `Applied`, stranded mid-two-step `temp_name`, and executed mixed `FilesystemForcedNfd` (`normalize_nfc(old_name) != normalize_nfc(new_name)`) rename operations recorded in an audit log in reverse order (skipping halted pure NFD→NFC `FilesystemForcedNfd` operations where `normalize_nfc(old_name) == normalize_nfc(new_name)` and other non-recoverable skipped entries), validating that recoverable `old_name`, `temp_name`, and `new_name` values are single-component filenames (no path separators or `..`), verifying `file_id` when stable or falling back to `(entry_type, byte_size, mtime_ns)` for files and `(entry_type, mtime_ns)` (with mandatory non-null `mtime_ns`) for directories when synthetic `st_dev:st_ino` values change on SMB mounts (`smbfs`/`cifs`)—with `--force` allowing explicit override if OS metadata or timestamps were touched—and performing atomic non-overwriting collision checks. Supports `--dry-run` to preview the revert plan prior to applying changes and `--lang <ko|en>` (default: `ko`) for localized status output.

**Exit Codes**:
- `0` when all recoverable operations revert cleanly (or `--dry-run` shows zero blocked or failed operations).
- `1` when any recoverable operation is blocked, fails `file_id`/metadata verification, or encounters a collision or I/O error.
- `2` on invalid CLI arguments, unreadable or corrupt audit log, or path-traversal/`parent_dir` safety violation.

**`RenameAuditLog` JSON Format**:
```json
{
  "schema_version": "1.0.0",
  "operations": [
    {
      "timestamp_utc": "2026-11-18T13:30:00Z",
      "parent_dir": "/path/to/dir",
      "old_name": "original",
      "old_raw_os_name": { "UnixBytes": [111, 114, 105, 103] },
      "file_id": "16777220:123456",
      "entry_type": "File",
      "byte_size": 1024,
      "mtime_ns": 1795008600000000000,
      "temp_name": null,
      "new_name": "canonical",
      "status": "Applied",
      "error_detail": null
    }
  ]
}
```

### 4. `namechecker ignore <PATH_OR_PATTERN> [--ignore-file <PATH>] [--category <CATEGORY>] [--reason <TEXT>] [--lang <ko|en>]`
Adds a portable suppression rule (`IgnoreRule`) to the `--ignore-file` JSON (`IgnoreList` `schema_version: "1.0.0"`) to suppress repeated `ManualReview` warnings across macOS, Windows, and Linux/Synology NAS without hardcoding host mount paths (when given a filesystem path, stores its single-component entry name; when given wildcards like `*?[`, stores a `globset` pattern), outputting localized status messages per `--lang <ko|en>` (default: `ko`).

**Exit Codes**:
- `0` when the ignore rule is added and persisted cleanly.
- `2` on invalid CLI arguments, unknown `--category`, malformed glob pattern, path traversal, or ignore-file I/O error.
