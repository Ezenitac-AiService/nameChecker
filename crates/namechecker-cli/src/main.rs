//! Command-line interface entrypoint for `namechecker`.

use std::fs;
use std::io::{self, BufRead, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand, ValueEnum};
use namechecker_core::ignore_list::{add_ignore_rule, load_ignore_list, save_ignore_list};
use namechecker_core::model::{
    CollationProfile, DEFAULT_EXCLUDE_GLOBS, DiscrepancyCause, IssueCategory, RemediationTrack,
    RenameOperation, RenameStatus, ScanTarget, Severity, UiLanguage,
};
use namechecker_core::renamer::{
    SafeRenameItem, apply_safe_renames, default_audit_log_dir, default_audit_log_path,
    undo_audit_log, validate_audit_outside_dir,
};
use namechecker_core::report::{
    filter_report, format_csv, format_json, format_text, sanitize_terminal_text,
};
use namechecker_core::scanner::scan_target;

#[derive(Parser, Debug)]
#[command(
    name = "namechecker",
    version,
    about = "Cross-platform CJK filename & folder checker",
    args_conflicts_with_subcommands = true
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
    #[command(flatten)]
    default_scan: ScanArgs,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum OutputFormat {
    Text,
    Json,
    Csv,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum LangArg {
    Ko,
    En,
}

impl From<LangArg> for UiLanguage {
    fn from(value: LangArg) -> Self {
        match value {
            LangArg::Ko => UiLanguage::Ko,
            LangArg::En => UiLanguage::En,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum SeverityArg {
    Error,
    Warning,
    Info,
}

impl From<SeverityArg> for Severity {
    fn from(value: SeverityArg) -> Self {
        match value {
            SeverityArg::Error => Severity::Error,
            SeverityArg::Warning => Severity::Warning,
            SeverityArg::Info => Severity::Info,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum TrackArg {
    All,
    #[value(alias = "auto-fixable")]
    Auto,
    #[value(alias = "manual-review")]
    Manual,
}

impl TrackArg {
    fn into_filter(self) -> Option<RemediationTrack> {
        match self {
            Self::All => None,
            Self::Auto => Some(RemediationTrack::AutoFixable),
            Self::Manual => Some(RemediationTrack::ManualReview),
        }
    }
}

#[derive(Args, Debug)]
struct ScanArgs {
    /// Target directory path to inspect.
    path: Option<PathBuf>,
    /// Output format (`text`, `json`, `csv`).
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    format: OutputFormat,
    /// Write report to file instead of stdout.
    #[arg(long, short = 'o')]
    output: Option<PathBuf>,
    /// Output language (`ko` or `en`).
    #[arg(long, value_enum, default_value_t = LangArg::Ko)]
    lang: LangArg,
    /// Traversal depth (`1` inspects target folder immediate children).
    #[arg(long)]
    max_depth: Option<usize>,
    /// Follow directory symlinks with cycle-safe visited-directory tracking.
    #[arg(long)]
    follow_symlinks: bool,
    /// Additional glob pattern(s) to exclude.
    #[arg(long = "exclude")]
    excludes: Vec<String>,
    /// Disable default OS/NAS metadata exclusions (`@eaDir`, `#recycle`, `.DS_Store`, etc.).
    #[arg(long)]
    no_default_excludes: bool,
    /// Comma-separated collation profiles (`windows-ko,macos-ko,linux-c,ja,zh`).
    #[arg(long)]
    profiles: Option<String>,
    /// Minimum severity to include (`error`, `warning`, `info`).
    #[arg(long, value_enum)]
    severity: Option<SeverityArg>,
    /// Filter findings by issue category (repeatable).
    #[arg(long = "category")]
    categories: Vec<String>,
    /// Filter findings by remediation track (`all`, `auto`, `manual`).
    #[arg(long, value_enum)]
    track: Option<TrackArg>,
    /// Path to persistent portable ignore-list JSON file.
    #[arg(long)]
    ignore_file: Option<PathBuf>,
}

#[derive(Args, Debug)]
struct FixArgs {
    /// Target directory path to inspect and rename safely.
    path: PathBuf,
    /// Apply renames without interactive confirmation prompt.
    #[arg(long)]
    yes: bool,
    /// Preview rename operations without modifying the filesystem.
    #[arg(long)]
    dry_run: bool,
    /// Traversal depth (`1` inspects target folder immediate children).
    #[arg(long)]
    max_depth: Option<usize>,
    /// Additional glob pattern(s) to exclude.
    #[arg(long = "exclude")]
    excludes: Vec<String>,
    /// Disable default OS/NAS metadata exclusions (`@eaDir`, `#recycle`, `.DS_Store`, etc.).
    #[arg(long)]
    no_default_excludes: bool,
    /// Path to write the reversible JSON audit log (default: outside target tree).
    #[arg(long)]
    audit_log: Option<PathBuf>,
    /// Path to persistent portable ignore-list JSON file.
    #[arg(long)]
    ignore_file: Option<PathBuf>,
    /// Output language (`ko` or `en`).
    #[arg(long, value_enum, default_value_t = LangArg::Ko)]
    lang: LangArg,
}

#[derive(Args, Debug)]
struct UndoArgs {
    /// Audit log JSON file created by `fix`.
    audit_log: PathBuf,
    /// Preview rollback operations without modifying the filesystem.
    #[arg(long)]
    dry_run: bool,
    /// Force rollback even if file_id or fallback metadata changed (never bypasses path-traversal check).
    #[arg(long)]
    force: bool,
    /// Output language (`ko` or `en`).
    #[arg(long, value_enum, default_value_t = LangArg::Ko)]
    lang: LangArg,
}

#[derive(Args, Debug)]
struct IgnoreArgs {
    /// Path or pattern to add to the portable ignore list.
    path_or_pattern: String,
    /// Ignore list JSON file path.
    #[arg(long)]
    ignore_file: Option<PathBuf>,
    /// Optional single `IssueCategory` scope (e.g., `PunctuationVariant`).
    #[arg(long)]
    category: Option<String>,
    /// Optional reason documenting why this rule is intentional.
    #[arg(long)]
    reason: Option<String>,
    /// Output language (`ko` or `en`).
    #[arg(long, value_enum, default_value_t = LangArg::Ko)]
    lang: LangArg,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Scans a target folder in read-only mode and reports findings.
    Scan(ScanArgs),
    /// Safely renames deterministic Auto-Fixable entries and logs an undoable audit file.
    Fix(FixArgs),
    /// Reverses operations recorded in an audit log JSON file.
    Undo(UndoArgs),
    /// Adds a portable rule to `--ignore-file`.
    Ignore(IgnoreArgs),
}

fn parse_profiles(raw: &str) -> Result<Vec<CollationProfile>, String> {
    let mut out = Vec::new();
    for item in raw.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        let profile = match item.to_ascii_lowercase().as_str() {
            "windows" | "windows-ko" | "windows-ko-kr" | "windowskokr" => {
                CollationProfile::WindowsKoKr
            }
            "macos" | "macos-ko" | "macosko" => CollationProfile::MacOsKo,
            "linux" | "linux-c" | "linux-code-point" | "linuxcodepoint" => {
                CollationProfile::LinuxCodePoint
            }
            "ja" | "japanese" => CollationProfile::Japanese,
            "zh" | "chinese" => CollationProfile::Chinese,
            _ => return Err(format!("unknown collation profile '{item}'")),
        };
        if !out.contains(&profile) {
            out.push(profile);
        }
    }
    if out.is_empty() {
        return Err("at least one collation profile is required".to_string());
    }
    Ok(out)
}

fn parse_category(raw: &str) -> Result<IssueCategory, String> {
    let norm = raw.to_ascii_lowercase().replace(['-', '_'], "");
    match norm.as_str() {
        "kangxiradical" => Ok(IssueCategory::KangxiRadical),
        "compatibilityideograph" => Ok(IssueCategory::CompatibilityIdeograph),
        "nonnfcnormalization" => Ok(IssueCategory::NonNfcNormalization),
        "invisiblezerowidth" => Ok(IssueCategory::InvisibleZeroWidth),
        "punctuationvariant" => Ok(IssueCategory::PunctuationVariant),
        "invalidutf8" => Ok(IssueCategory::InvalidUtf8),
        "privateusearea" => Ok(IssueCategory::PrivateUseArea),
        "variationselector" => Ok(IssueCategory::VariationSelector),
        "crossscriptconfusable" => Ok(IssueCategory::CrossScriptConfusable),
        "replacementchar" => Ok(IssueCategory::ReplacementChar),
        "windowssmbhazard" => Ok(IssueCategory::WindowsSmbHazard),
        "orphanedtempfile" => Ok(IssueCategory::OrphanedTempFile),
        _ => Err(format!("unknown issue category '{raw}'")),
    }
}

fn run_scan(args: ScanArgs) -> u8 {
    let Some(path) = args.path else {
        eprintln!("error: target <PATH> is required (try `namechecker --help`)");
        return 2;
    };
    let lang: UiLanguage = args.lang.into();
    let mut target = ScanTarget::new(path);
    target.lang = lang;
    target.max_depth = Some(args.max_depth.unwrap_or(1));
    target.follow_symlinks = args.follow_symlinks;
    target.ignore_list_path = args.ignore_file;

    if args.no_default_excludes {
        target.exclude_globs = args.excludes;
    } else {
        target.exclude_globs = DEFAULT_EXCLUDE_GLOBS
            .iter()
            .map(|s| (*s).to_string())
            .chain(args.excludes)
            .collect();
    }

    if let Some(raw_profiles) = args.profiles.as_deref() {
        match parse_profiles(raw_profiles) {
            Ok(p) => target.profiles = p,
            Err(err) => {
                eprintln!("error: {err}");
                return 2;
            }
        }
    }

    let mut parsed_categories = Vec::new();
    for raw_cat in &args.categories {
        match parse_category(raw_cat) {
            Ok(cat) => {
                if !parsed_categories.contains(&cat) {
                    parsed_categories.push(cat);
                }
            }
            Err(err) => {
                eprintln!("error: {err}");
                return 2;
            }
        }
    }

    let report = match scan_target(&target) {
        Ok(r) => filter_report(
            r,
            args.severity.map(Into::into),
            args.track.and_then(TrackArg::into_filter),
            (!parsed_categories.is_empty()).then_some(parsed_categories.as_slice()),
        ),
        Err(err) => {
            eprintln!("error: {err}");
            return 2;
        }
    };

    let bytes = match args.format {
        OutputFormat::Text => format_text(&report, lang).into_bytes(),
        OutputFormat::Json => match format_json(&report) {
            Ok(s) => s.into_bytes(),
            Err(err) => {
                eprintln!("error: {err}");
                return 2;
            }
        },
        OutputFormat::Csv => match format_csv(&report) {
            Ok(b) => b,
            Err(err) => {
                eprintln!("error: {err}");
                return 2;
            }
        },
    };

    if let Some(out_path) = args.output {
        if let Err(err) = fs::write(&out_path, &bytes) {
            eprintln!("error writing output file {:?}: {err}", out_path);
            return 2;
        }
    } else {
        let mut stdout = io::stdout().lock();
        if let Err(err) = stdout.write_all(&bytes) {
            eprintln!("error writing stdout: {err}");
            return 2;
        }
        if !bytes.ends_with(b"\n") {
            let _ = stdout.write_all(b"\n");
        }
    }

    let has_encoding_discrepancy = report
        .sort_discrepancies
        .iter()
        .any(|d| d.cause == DiscrepancyCause::EncodingAnomaly);

    if !report.io_errors.is_empty() {
        2
    } else if !report.findings.is_empty()
        || report.collision_risk_count > 0
        || has_encoding_discrepancy
    {
        1
    } else {
        0
    }
}

fn run_fix(args: FixArgs) -> u8 {
    let mut target = ScanTarget::new(args.path);
    target.lang = args.lang.into();
    target.max_depth = Some(args.max_depth.unwrap_or(1));
    target.ignore_list_path = args.ignore_file;

    if args.no_default_excludes {
        target.exclude_globs = args.excludes;
    } else {
        target.exclude_globs = DEFAULT_EXCLUDE_GLOBS
            .iter()
            .map(|s| (*s).to_string())
            .chain(args.excludes)
            .collect();
    }

    let report = match scan_target(&target) {
        Ok(r) => r,
        Err(err) => {
            eprintln!("error: {err}");
            return 2;
        }
    };

    let lang: UiLanguage = args.lang.into();
    if !report.io_errors.is_empty() {
        for io_err in &report.io_errors {
            let safe_path = sanitize_terminal_text(&io_err.path.to_string_lossy());
            let safe_detail = sanitize_terminal_text(&io_err.message);
            match lang {
                UiLanguage::Ko => eprintln!("I/O 오류 ({safe_path}): {safe_detail}"),
                UiLanguage::En => eprintln!("I/O error ({safe_path}): {safe_detail}"),
            }
        }
        return 2;
    }

    let items: Vec<SafeRenameItem> = report
        .rename_plans
        .iter()
        .filter_map(|plan| {
            if plan.track != RemediationTrack::AutoFixable {
                return None;
            }
            let target_name = plan.composed_suggested_name.as_ref()?;
            let parent_dir = plan.path.parent()?.to_path_buf();
            Some(SafeRenameItem {
                parent_dir,
                old_raw_os_name: plan.raw_os_name.clone(),
                old_name: plan.raw_name.clone(),
                new_name: target_name.clone(),
            })
        })
        .collect();

    let audit_path = args.audit_log.unwrap_or_else(default_audit_log_path);
    if let Err(err) = validate_audit_outside_dir(&audit_path, &target.roots[0]) {
        eprintln!("error: {err}");
        return 2;
    }

    if items.is_empty() {
        match lang {
            UiLanguage::Ko => println!("자동 수정할 항목이 없습니다."),
            UiLanguage::En => println!("No auto-fixable renames needed."),
        }
        return 0;
    }

    if args.dry_run {
        let log = match apply_safe_renames(&items, Some(&audit_path), true) {
            Ok(l) => l,
            Err(err) => {
                eprintln!("error: {err}");
                return 2;
            }
        };
        for item in &items {
            println!(
                "  {} -> {}",
                sanitize_terminal_text(&item.old_name),
                sanitize_terminal_text(&item.new_name)
            );
        }
        let has_issues = report_fix_operation_issues(&log.operations, lang);
        return if has_issues { 1 } else { 0 };
    }

    for item in &items {
        println!(
            "  {} -> {}",
            sanitize_terminal_text(&item.old_name),
            sanitize_terminal_text(&item.new_name)
        );
    }

    if !args.yes {
        match lang {
            UiLanguage::Ko => print!("{}개 항목의 이름을 변경할까요? [y/N]: ", items.len()),
            UiLanguage::En => print!("Proceed with {} renames? [y/N]: ", items.len()),
        }
        let _ = io::stdout().flush();
        let mut line = String::new();
        if io::stdin().lock().read_line(&mut line).is_err()
            || !matches!(line.trim().to_ascii_lowercase().as_str(), "y" | "yes")
        {
            match lang {
                UiLanguage::Ko => println!("취소되었습니다."),
                UiLanguage::En => println!("Aborted."),
            }
            return 1;
        }
    }

    let log = match apply_safe_renames(&items, Some(&audit_path), false) {
        Ok(l) => l,
        Err(err) => {
            eprintln!("error: {err}");
            return 2;
        }
    };

    match lang {
        UiLanguage::Ko => println!("감사 로그 저장됨: {}", audit_path.display()),
        UiLanguage::En => println!("Audit log saved: {}", audit_path.display()),
    }
    let has_issues = report_fix_operation_issues(&log.operations, lang);
    if has_issues { 1 } else { 0 }
}

fn report_fix_operation_issues(operations: &[RenameOperation], lang: UiLanguage) -> bool {
    let mut has_issues = false;
    for op in operations {
        if op.status == RenameStatus::Applied {
            continue;
        }
        has_issues = true;
        let old_name = sanitize_terminal_text(&op.old_name);
        let new_name = sanitize_terminal_text(&op.new_name);
        let detail_suffix = op
            .error_detail
            .as_deref()
            .filter(|s| !s.is_empty())
            .map(|s| format!(" ({})", sanitize_terminal_text(s)))
            .unwrap_or_default();

        match (op.status, lang) {
            (RenameStatus::FilesystemForcedNfd, UiLanguage::Ko) => println!(
                "[{:?}] 경고: {} -> {} — 파일시스템이 NFC 대상 이름을 NFD로 강제 분해했습니다{}",
                op.status, old_name, new_name, detail_suffix
            ),
            (RenameStatus::FilesystemForcedNfd, UiLanguage::En) => println!(
                "[{:?}] warning: {} -> {} — filesystem forced NFC target name back to NFD{}",
                op.status, old_name, new_name, detail_suffix
            ),
            (RenameStatus::BlockedCollision, UiLanguage::Ko) => println!(
                "[{:?}] 차단됨: {} -> {} — 대상 이름 충돌로 변경이 차단되었습니다{}",
                op.status, old_name, new_name, detail_suffix
            ),
            (RenameStatus::BlockedCollision, UiLanguage::En) => println!(
                "[{:?}] blocked: {} -> {} — rename blocked due to target name collision{}",
                op.status, old_name, new_name, detail_suffix
            ),
            (RenameStatus::BlockedSymlink, UiLanguage::Ko) => println!(
                "[{:?}] 차단됨: {} -> {} — 심볼릭 링크는 안전을 위해 변경하지 않습니다{}",
                op.status, old_name, new_name, detail_suffix
            ),
            (RenameStatus::BlockedSymlink, UiLanguage::En) => println!(
                "[{:?}] blocked: {} -> {} — symlink entries are blocked from renaming{}",
                op.status, old_name, new_name, detail_suffix
            ),
            (RenameStatus::PermissionDenied, UiLanguage::Ko) => println!(
                "[{:?}] 실패: {} -> {} — 권한이 거부되었습니다{}",
                op.status, old_name, new_name, detail_suffix
            ),
            (RenameStatus::PermissionDenied, UiLanguage::En) => println!(
                "[{:?}] failed: {} -> {} — permission denied{}",
                op.status, old_name, new_name, detail_suffix
            ),
            (RenameStatus::IoError, UiLanguage::Ko) => println!(
                "[{:?}] 실패: {} -> {} — 입출력 오류가 발생했습니다{}",
                op.status, old_name, new_name, detail_suffix
            ),
            (RenameStatus::IoError, UiLanguage::En) => println!(
                "[{:?}] failed: {} -> {} — I/O error occurred{}",
                op.status, old_name, new_name, detail_suffix
            ),
            (RenameStatus::Applied, _) => {}
        }
    }
    has_issues
}

fn run_undo(args: UndoArgs) -> u8 {
    let lang: UiLanguage = args.lang.into();
    let ops = match undo_audit_log(&args.audit_log, args.dry_run, args.force) {
        Ok(o) => o,
        Err(err) => {
            eprintln!("error: {err}");
            return 2;
        }
    };
    let mut failed = false;
    if args.dry_run {
        match lang {
            UiLanguage::Ko => println!("되돌리기 미리보기 ({}건):", ops.len()),
            UiLanguage::En => println!("Undo dry-run preview ({} operations):", ops.len()),
        }
    }
    for op in &ops {
        println!(
            "[{:?}] {} -> {}",
            op.status,
            sanitize_terminal_text(&op.new_name),
            sanitize_terminal_text(&op.old_name)
        );
        if op.status != RenameStatus::Applied {
            failed = true;
        }
    }
    if !args.dry_run {
        match lang {
            UiLanguage::Ko => println!("되돌리기 처리 완료 ({}건)", ops.len()),
            UiLanguage::En => println!("Undo completed ({} operations)", ops.len()),
        }
    }
    if failed { 1 } else { 0 }
}

fn run_ignore(args: IgnoreArgs) -> u8 {
    let lang: UiLanguage = args.lang.into();
    let ignore_path = args
        .ignore_file
        .unwrap_or_else(|| default_audit_log_dir().join("ignore.json"));
    let cat = match args.category.as_deref() {
        Some(c) => match parse_category(c) {
            Ok(parsed) => Some(parsed),
            Err(err) => {
                eprintln!("error: {err}");
                return 2;
            }
        },
        None => None,
    };

    let mut list = match load_ignore_list(&ignore_path) {
        Ok(l) => l,
        Err(err) => {
            eprintln!("error loading ignore list: {err}");
            return 2;
        }
    };
    let rule = match add_ignore_rule(&mut list, &args.path_or_pattern, cat, args.reason) {
        Ok(r) => r,
        Err(err) => {
            eprintln!("error: {err}");
            return 2;
        }
    };
    if let Err(err) = save_ignore_list(&ignore_path, &list) {
        eprintln!("error saving ignore list: {err}");
        return 2;
    }
    let safe_pat = sanitize_terminal_text(&rule.pattern);
    match lang {
        UiLanguage::Ko => println!(
            "무시 규칙 추가됨 '{}' (glob={}, category={:?}) -> {}",
            safe_pat,
            rule.is_glob,
            rule.category,
            ignore_path.display()
        ),
        UiLanguage::En => println!(
            "Added rule '{}' (glob={}, category={:?}) -> {}",
            safe_pat,
            rule.is_glob,
            rule.category,
            ignore_path.display()
        ),
    }
    0
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let code = match cli.command {
        Some(Commands::Scan(args)) => run_scan(args),
        Some(Commands::Fix(args)) => run_fix(args),
        Some(Commands::Undo(args)) => run_undo(args),
        Some(Commands::Ignore(args)) => run_ignore(args),
        None => run_scan(cli.default_scan),
    };
    ExitCode::from(code)
}
