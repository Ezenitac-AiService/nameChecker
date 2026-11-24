//! Safe non-overwriting filesystem rename engine with two-step recovery and `undo`.
//!
//! Uses atomic non-replacing OS primitives (`renamex_np(RENAME_EXCL)`, `renameat2(RENAME_NOREPLACE)`,
//! `MoveFileExW(flags = 0)` over `\\?\` / `\\?\UNC\` verbatim namespaces), guarded `ENOTSUP`/`EINVAL`/`ENOSYS`
//! SMB fallback, bounded transient lock backoff retries, Step-1 rollback on Step-2 failure, post-rename
//! `FilesystemForcedNfd` detection, incremental audit log persistence, and `file_id` + SMB metadata-verified `undo`.

use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::detector::has_windows_smb_hazard;
use crate::encoding_hint::exceeds_cross_platform_name_limit;
use crate::model::{
    CoreError, EntryType, RawOsName, RenameAuditLog, RenameOperation, RenameStatus, SCHEMA_VERSION,
};
use crate::unicode_data::{is_normalized_nfc, normalize_nfc};

/// Planned single-entry rename request executed by `apply_safe_renames`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SafeRenameItem {
    /// Parent directory holding the entry.
    pub parent_dir: PathBuf,
    /// Lossless original OS-native filename.
    pub old_raw_os_name: RawOsName,
    /// Display string of the original filename.
    pub old_name: String,
    /// Single-component target filename.
    pub new_name: String,
}

fn collapse_non_leaf_wide<'a>(segments: &[&'a [u16]], min_depth: usize) -> Vec<&'a [u16]> {
    let mut stack: Vec<&'a [u16]> = Vec::with_capacity(segments.len());
    if segments.is_empty() {
        return stack;
    }
    let last_idx = segments.len() - 1;
    for (idx, &seg) in segments.iter().enumerate() {
        if idx < min_depth {
            stack.push(seg);
            continue;
        }
        if idx == last_idx {
            stack.push(seg);
            break;
        }
        if seg.is_empty() || (seg.len() == 1 && seg[0] == u16::from(b'.')) {
            continue;
        }
        if seg.len() == 2 && seg[0] == u16::from(b'.') && seg[1] == u16::from(b'.') {
            if stack.len() > min_depth {
                stack.pop();
            }
            continue;
        }
        stack.push(seg);
    }
    stack
}

fn join_wide_segments(prefix: &[u16], segments: &[&[u16]]) -> Vec<u16> {
    let mut out = Vec::with_capacity(prefix.len() + segments.len() * 8);
    out.extend_from_slice(prefix);
    for (i, seg) in segments.iter().enumerate() {
        if i > 0 {
            out.push(0x005C);
        }
        out.extend_from_slice(seg);
    }
    out
}

fn starts_with_unc_verbatim_ci(norm: &[u16]) -> bool {
    norm.len() >= 8
        && norm[0] == 0x005C
        && norm[1] == 0x005C
        && norm[2] == u16::from(b'?')
        && norm[3] == 0x005C
        && norm[4].to_ascii_lowercase_u16() == u16::from(b'u')
        && norm[5].to_ascii_lowercase_u16() == u16::from(b'n')
        && norm[6].to_ascii_lowercase_u16() == u16::from(b'c')
        && norm[7] == 0x005C
}

trait AsciiLowerU16 {
    fn to_ascii_lowercase_u16(self) -> u16;
}

impl AsciiLowerU16 for u16 {
    fn to_ascii_lowercase_u16(self) -> u16 {
        if (u16::from(b'A')..=u16::from(b'Z')).contains(&self) {
            self + 32
        } else {
            self
        }
    }
}

fn is_ascii_alpha_u16(u: u16) -> bool {
    (u16::from(b'A')..=u16::from(b'Z')).contains(&u)
        || (u16::from(b'a')..=u16::from(b'z')).contains(&u)
}

/// Converts raw UTF-16 Windows path units into an extended-length verbatim `\\?\` / `\\?\UNC\`
/// UTF-16 sequence losslessly without NUL termination.
#[must_use]
pub fn format_win32_verbatim_wide(wide: &[u16]) -> Vec<u16> {
    let norm: Vec<u16> = wide
        .iter()
        .map(|&u| if u == 0x002F { 0x005C } else { u })
        .collect();

    // Leave Win32 device namespace `\\.\...` untouched.
    if norm.len() >= 4
        && norm[0] == 0x005C
        && norm[1] == 0x005C
        && norm[2] == u16::from(b'.')
        && norm[3] == 0x005C
    {
        return wide.to_vec();
    }

    // Already `\\?\UNC\server\share\...`: normalize slashes and non-leaf `.`/`..` idempotently.
    if starts_with_unc_verbatim_ci(&norm) {
        let rest = &norm[8..];
        let segs: Vec<&[u16]> = rest.split(|&u| u == 0x005C).collect();
        if segs.len() < 2 || segs[0].is_empty() || segs[1].is_empty() {
            return norm;
        }
        let collapsed = collapse_non_leaf_wide(&segs, 2);
        let unc_prefix: Vec<u16> = r"\\?\UNC\".encode_utf16().collect();
        return join_wide_segments(&unc_prefix, &collapsed);
    }

    // Already `\\?\C:\...`: normalize slashes and non-leaf `.`/`..` idempotently.
    if norm.len() >= 7
        && norm[0] == 0x005C
        && norm[1] == 0x005C
        && norm[2] == u16::from(b'?')
        && norm[3] == 0x005C
    {
        let rest = &norm[4..];
        if rest.len() >= 3
            && is_ascii_alpha_u16(rest[0])
            && rest[1] == u16::from(b':')
            && rest[2] == 0x005C
        {
            let segs: Vec<&[u16]> = rest.split(|&u| u == 0x005C).collect();
            let collapsed = collapse_non_leaf_wide(&segs, 1);
            let local_prefix: Vec<u16> = r"\\?\".encode_utf16().collect();
            return join_wide_segments(&local_prefix, &collapsed);
        }
        return norm;
    }

    // Standard UNC `\\server\share\...` -> `\\?\UNC\server\share\...`
    if norm.len() >= 2 && norm[0] == 0x005C && norm[1] == 0x005C {
        let rest = &norm[2..];
        let segs: Vec<&[u16]> = rest.split(|&u| u == 0x005C).collect();
        if segs.len() < 2 || segs[0].is_empty() || segs[1].is_empty() {
            return wide.to_vec();
        }
        let collapsed = collapse_non_leaf_wide(&segs, 2);
        let unc_prefix: Vec<u16> = r"\\?\UNC\".encode_utf16().collect();
        return join_wide_segments(&unc_prefix, &collapsed);
    }

    // Standard local drive absolute path `C:\...` -> `\\?\C:\...`
    if norm.len() >= 3
        && is_ascii_alpha_u16(norm[0])
        && norm[1] == u16::from(b':')
        && norm[2] == 0x005C
    {
        let segs: Vec<&[u16]> = norm.split(|&u| u == 0x005C).collect();
        let collapsed = collapse_non_leaf_wide(&segs, 1);
        let local_prefix: Vec<u16> = r"\\?\".encode_utf16().collect();
        return join_wide_segments(&local_prefix, &collapsed);
    }

    // Root-relative `\foo`, drive-relative `C:foo`, or plain relative `foo\bar`: leave unprefixed.
    wide.to_vec()
}

/// Converts a Windows local/UNC path to the extended-length verbatim namespace (`\\?\` / `\\?\UNC\`).
#[must_use]
pub fn to_win32_verbatim_path(path: &Path) -> String {
    #[cfg(windows)]
    let wide_in: Vec<u16> = {
        use std::os::windows::ffi::OsStrExt;
        path.as_os_str().encode_wide().collect()
    };
    #[cfg(not(windows))]
    let wide_in: Vec<u16> = path.to_string_lossy().encode_utf16().collect();

    let out = format_win32_verbatim_wide(&wide_in);
    String::from_utf16_lossy(&out)
}

/// Converts a `RawOsName` back to a native `OsString`.
pub fn raw_os_to_os_string(raw: &RawOsName) -> Result<OsString, CoreError> {
    match raw {
        RawOsName::UnixBytes(bytes) => {
            #[cfg(unix)]
            {
                use std::os::unix::ffi::OsStringExt;
                Ok(OsString::from_vec(bytes.clone()))
            }
            #[cfg(not(unix))]
            {
                Ok(OsString::from(String::from_utf8_lossy(bytes).into_owned()))
            }
        }
        RawOsName::WindowsWide(wide) => {
            #[cfg(windows)]
            {
                use std::os::windows::ffi::OsStringExt;
                Ok(OsString::from_wide(wide))
            }
            #[cfg(not(windows))]
            {
                let s = String::from_utf16(wide).map_err(|err| {
                    CoreError::UnsafePath(format!("non-UTF16 WindowsWide on non-Windows: {err}"))
                })?;
                Ok(OsString::from(s))
            }
        }
    }
}

fn validate_no_traversal_str(name: &str) -> Result<(), CoreError> {
    if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\\', '\0']) {
        return Err(CoreError::UnsafePath(format!(
            "unsafe multi-component or empty filename: {name:?}"
        )));
    }
    Ok(())
}

fn validate_no_traversal_raw(raw: &RawOsName) -> Result<(), CoreError> {
    match raw {
        RawOsName::UnixBytes(bytes) => {
            if bytes.is_empty()
                || bytes == b"."
                || bytes == b".."
                || bytes.contains(&b'/')
                || bytes.contains(&b'\\')
                || bytes.contains(&0)
            {
                return Err(CoreError::UnsafePath(format!(
                    "unsafe UnixBytes filename: {:?}",
                    String::from_utf8_lossy(bytes)
                )));
            }
            Ok(())
        }
        RawOsName::WindowsWide(units) => {
            let dot = '.' as u16;
            let slash = '/' as u16;
            let bslash = '\\' as u16;
            if units.is_empty()
                || units.as_slice() == [dot]
                || units.as_slice() == [dot, dot]
                || units.contains(&slash)
                || units.contains(&bslash)
                || units.contains(&0)
            {
                return Err(CoreError::UnsafePath(
                    "unsafe WindowsWide filename".to_string(),
                ));
            }
            Ok(())
        }
    }
}

/// Validates that `name` is a safe single-component target filename within cross-platform limits.
pub fn validate_single_component_name(name: &str) -> Result<(), CoreError> {
    validate_no_traversal_str(name)?;
    let (exceeds, utf8_bytes, utf16_units) = exceeds_cross_platform_name_limit(name);
    if exceeds {
        return Err(CoreError::UnsafePath(format!(
            "filename exceeds cross-platform limit ({utf8_bytes} UTF-8 bytes, {utf16_units} UTF-16 units): {name:?}"
        )));
    }
    if has_windows_smb_hazard(name) {
        return Err(CoreError::UnsafePath(format!(
            "filename violates Windows/SMB constraints: {name:?}"
        )));
    }
    Ok(())
}

fn system_time_to_ns(t: SystemTime) -> i128 {
    match t.duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_nanos() as i128,
        Err(e) => -(e.duration().as_nanos() as i128),
    }
}

fn metadata_details(
    meta: &fs::Metadata,
    _path: &Path,
) -> (Option<String>, EntryType, Option<u64>, Option<i128>) {
    let entry_type = if meta.file_type().is_symlink() {
        EntryType::Symlink
    } else if meta.is_dir() {
        EntryType::Directory
    } else {
        EntryType::File
    };
    let byte_size = if entry_type == EntryType::File {
        Some(meta.len())
    } else {
        None
    };
    let mtime_ns = meta.modified().ok().map(system_time_to_ns);

    #[cfg(unix)]
    let file_id = {
        use std::os::unix::fs::MetadataExt;
        Some(format!("{}:{}", meta.dev(), meta.ino()))
    };

    #[cfg(windows)]
    let file_id = windows_file_id(_path);

    #[cfg(not(any(unix, windows)))]
    let file_id: Option<String> = None;

    (file_id, entry_type, byte_size, mtime_ns)
}

#[cfg(windows)]
fn windows_volume_and_index(path: &Path, follow_reparse: bool) -> Option<(u32, u64)> {
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, CreateFileW, FILE_FLAG_BACKUP_SEMANTICS,
        FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
        GetFileInformationByHandle, OPEN_EXISTING,
    };

    let flags = if follow_reparse {
        FILE_FLAG_BACKUP_SEMANTICS
    } else {
        FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT
    };
    let wide = to_verbatim_utf16(path);
    let handle = unsafe {
        CreateFileW(
            wide.as_ptr(),
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            null(),
            OPEN_EXISTING,
            flags,
            null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return None;
    }
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    let ok = unsafe { GetFileInformationByHandle(handle, &mut info) };
    unsafe { CloseHandle(handle) };
    if ok != 0 {
        let index = ((info.nFileIndexHigh as u64) << 32) | (info.nFileIndexLow as u64);
        Some((info.dwVolumeSerialNumber, index))
    } else {
        None
    }
}

#[cfg(windows)]
fn windows_file_id(path: &Path) -> Option<String> {
    windows_volume_and_index(path, false).map(|(vol, idx)| format!("{vol}:{idx}"))
}

#[cfg(windows)]
pub(crate) fn windows_dir_file_id(path: &Path) -> Option<(u64, u64)> {
    windows_volume_and_index(path, true).map(|(vol, idx)| (u64::from(vol), idx))
}

#[cfg(windows)]
pub(crate) fn verbatim_path_buf(path: &Path) -> std::borrow::Cow<'_, Path> {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    let abs_wide: Vec<u16> = if path.is_absolute() {
        path.as_os_str().encode_wide().collect()
    } else if let Ok(cwd) = std::env::current_dir() {
        let mut w: Vec<u16> = cwd.as_os_str().encode_wide().collect();
        if !w.ends_with(&[0x005C]) && !w.ends_with(&[0x002F]) {
            w.push(0x005C);
        }
        w.extend(path.as_os_str().encode_wide());
        w
    } else {
        path.as_os_str().encode_wide().collect()
    };
    let verbatim = format_win32_verbatim_wide(&abs_wide);
    std::borrow::Cow::Owned(PathBuf::from(OsString::from_wide(&verbatim)))
}

#[cfg(windows)]
fn to_verbatim_utf16(path: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    let vp = verbatim_path_buf(path);
    vp.as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

#[cfg(not(windows))]
pub(crate) fn verbatim_path_buf(path: &Path) -> std::borrow::Cow<'_, Path> {
    std::borrow::Cow::Borrowed(path)
}

/// Captured `(file_id, entry_type, byte_size, mtime_ns)` identity tuple for safe `undo`.
pub type FileIdentityMeta = (Option<String>, EntryType, Option<u64>, Option<i128>);

/// Captures platform file identity (`dev:ino` or Windows FileId) and fallback metadata.
#[must_use]
pub fn capture_file_id_and_metadata(path: &Path) -> Option<FileIdentityMeta> {
    let vp = verbatim_path_buf(path);
    let meta = fs::symlink_metadata(vp.as_ref()).ok()?;
    Some(metadata_details(&meta, path))
}

#[cfg(not(windows))]
fn guarded_smb_fallback_rename(src: &Path, dst: &Path) -> std::io::Result<()> {
    let vp_src = verbatim_path_buf(src);
    let vp_dst = verbatim_path_buf(dst);
    match fs::symlink_metadata(vp_dst.as_ref()) {
        Ok(_) => Err(std::io::Error::new(
            ErrorKind::AlreadyExists,
            "destination exists during pre-rename SMB check",
        )),
        Err(e) if e.kind() == ErrorKind::NotFound => fs::rename(vp_src.as_ref(), vp_dst.as_ref()),
        Err(e) => Err(e),
    }
}

fn no_replace_rename_once(src: &Path, dst: &Path) -> std::io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        let c_src = CString::new(src.as_os_str().as_bytes())
            .map_err(|e| std::io::Error::new(ErrorKind::InvalidInput, e))?;
        let c_dst = CString::new(dst.as_os_str().as_bytes())
            .map_err(|e| std::io::Error::new(ErrorKind::InvalidInput, e))?;
        let rc = unsafe { libc::renamex_np(c_src.as_ptr(), c_dst.as_ptr(), libc::RENAME_EXCL) };
        if rc == 0 {
            return Ok(());
        }
        let err = std::io::Error::last_os_error();
        let raw_errno = err.raw_os_error();
        if matches!(raw_errno, Some(libc::ENOTSUP | libc::EINVAL | libc::ENOSYS)) {
            return guarded_smb_fallback_rename(src, dst);
        }
        Err(err)
    }

    #[cfg(target_os = "linux")]
    {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        let c_src = CString::new(src.as_os_str().as_bytes())
            .map_err(|e| std::io::Error::new(ErrorKind::InvalidInput, e))?;
        let c_dst = CString::new(dst.as_os_str().as_bytes())
            .map_err(|e| std::io::Error::new(ErrorKind::InvalidInput, e))?;
        let rc = unsafe {
            libc::syscall(
                libc::SYS_renameat2,
                libc::AT_FDCWD,
                c_src.as_ptr(),
                libc::AT_FDCWD,
                c_dst.as_ptr(),
                libc::RENAME_NOREPLACE,
            )
        };
        if rc == 0 {
            return Ok(());
        }
        let err = std::io::Error::last_os_error();
        if matches!(
            err.raw_os_error(),
            Some(libc::ENOTSUP | libc::EINVAL | libc::ENOSYS)
        ) {
            return guarded_smb_fallback_rename(src, dst);
        }
        Err(err)
    }

    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::{ERROR_ALREADY_EXISTS, ERROR_FILE_EXISTS};
        use windows_sys::Win32::Storage::FileSystem::MoveFileExW;

        let w_src = to_verbatim_utf16(src);
        let w_dst = to_verbatim_utf16(dst);
        let ok = unsafe { MoveFileExW(w_src.as_ptr(), w_dst.as_ptr(), 0) };
        if ok != 0 {
            return Ok(());
        }
        let err = std::io::Error::last_os_error();
        match err.raw_os_error().map(|c| c as u32) {
            Some(ERROR_FILE_EXISTS | ERROR_ALREADY_EXISTS) => Err(std::io::Error::new(
                ErrorKind::AlreadyExists,
                err.to_string(),
            )),
            _ => Err(err),
        }
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
    {
        guarded_smb_fallback_rename(src, dst)
    }
}

fn is_transient_busy(err: &std::io::Error) -> bool {
    #[cfg(unix)]
    if matches!(err.raw_os_error(), Some(libc::EBUSY)) {
        return true;
    }
    #[cfg(windows)]
    if err.raw_os_error() == Some(32) {
        // ERROR_SHARING_VIOLATION (32)
        return true;
    }
    false
}

/// Bounded exponential-backoff retry schedule in milliseconds (5 retries / 6 total attempts).
pub const RETRY_BACKOFF_DELAYS_MS: [u64; 5] = [25, 50, 100, 200, 400];

/// Returns `true` if `item` is an NFD→NFC normalization-only rename on a volume that already
/// forced NFD back onto disk.
#[must_use]
pub fn should_halt_forced_nfd_item(
    forced_volumes: &std::collections::HashSet<String>,
    volume_key: &str,
    old_name: &str,
    new_name: &str,
) -> bool {
    forced_volumes.contains(volume_key) && normalize_nfc(old_name) == normalize_nfc(new_name)
}

fn volume_key_for_dir(parent_dir: &Path) -> String {
    let vp = verbatim_path_buf(parent_dir);
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if let Ok(meta) = fs::metadata(vp.as_ref()) {
            return format!("dev:{}", meta.dev());
        }
    }
    #[cfg(windows)]
    {
        if let Some((vol, _)) = windows_volume_and_index(parent_dir, true) {
            return format!("vol:{vol:08x}");
        }
    }
    fs::canonicalize(vp.as_ref())
        .unwrap_or_else(|_| parent_dir.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

fn atomic_rename_with_retry(src: &Path, dst: &Path) -> std::io::Result<()> {
    for &delay_ms in &RETRY_BACKOFF_DELAYS_MS {
        match no_replace_rename_once(src, dst) {
            Ok(()) => return Ok(()),
            Err(e) if is_transient_busy(&e) => {
                std::thread::sleep(Duration::from_millis(delay_ms));
            }
            Err(e) => return Err(e),
        }
    }
    no_replace_rename_once(src, dst)
}

fn is_already_exists(err: &std::io::Error) -> bool {
    if err.kind() == ErrorKind::AlreadyExists {
        return true;
    }
    #[cfg(unix)]
    if matches!(err.raw_os_error(), Some(libc::EEXIST | libc::ENOTEMPTY)) {
        return true;
    }
    false
}

fn map_io_status(err: &std::io::Error) -> RenameStatus {
    if is_already_exists(err) {
        RenameStatus::BlockedCollision
    } else if err.kind() == ErrorKind::PermissionDenied {
        RenameStatus::PermissionDenied
    } else {
        RenameStatus::IoError
    }
}

fn is_two_step_candidate(old_name: &str, new_name: &str) -> bool {
    old_name != new_name
        && (old_name.to_lowercase() == new_name.to_lowercase()
            || normalize_nfc(old_name) == normalize_nfc(new_name))
}

struct SiblingIndex {
    by_nfc_lower: HashMap<String, Vec<OsString>>,
}

impl SiblingIndex {
    fn load(parent_dir: &Path) -> Result<Self, std::io::Error> {
        let vp = verbatim_path_buf(parent_dir);
        let mut by_nfc_lower: HashMap<String, Vec<OsString>> = HashMap::new();
        for entry_res in fs::read_dir(vp.as_ref())? {
            let entry = entry_res?;
            let fname = entry.file_name();
            let key = normalize_nfc(&fname.to_string_lossy()).to_lowercase();
            by_nfc_lower.entry(key).or_default().push(fname);
        }
        Ok(Self { by_nfc_lower })
    }

    fn record_rename(&mut self, old_os: &OsStr, new_name: &str) {
        let old_key = normalize_nfc(&old_os.to_string_lossy()).to_lowercase();
        if let Some(list) = self.by_nfc_lower.get_mut(&old_key) {
            list.retain(|item| item != old_os);
            if list.is_empty() {
                self.by_nfc_lower.remove(&old_key);
            }
        }
        let new_key = normalize_nfc(new_name).to_lowercase();
        self.by_nfc_lower
            .entry(new_key)
            .or_default()
            .push(OsString::from(new_name));
    }
}

fn check_disk_sibling_collision(
    parent_dir: &Path,
    sibling_cache: &mut HashMap<PathBuf, SiblingIndex>,
    src_file_id: Option<&str>,
    old_raw: &OsStr,
    new_name: &str,
) -> Result<bool, std::io::Error> {
    let dst_path = parent_dir.join(new_name);
    let vp_dst = verbatim_path_buf(&dst_path);

    match fs::symlink_metadata(vp_dst.as_ref()) {
        Ok(dst_meta) => {
            let (dst_id, _, _, _) = metadata_details(&dst_meta, &dst_path);
            if src_file_id.is_none() || dst_id.as_deref() != src_file_id {
                return Ok(true);
            }
        }
        Err(e) if e.kind() == ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }

    if !sibling_cache.contains_key(parent_dir) {
        let index = SiblingIndex::load(parent_dir)?;
        sibling_cache.insert(parent_dir.to_path_buf(), index);
    }
    let index = sibling_cache
        .get(parent_dir)
        .expect("sibling index inserted above");
    let target_key = normalize_nfc(new_name).to_lowercase();
    if let Some(candidates) = index.by_nfc_lower.get(&target_key) {
        for fname in candidates {
            if fname == old_raw {
                continue;
            }
            let cand_path = parent_dir.join(fname);
            let (other_id, _, _, _) = capture_file_id_and_metadata(&cand_path).unwrap_or((
                None,
                EntryType::File,
                None,
                None,
            ));
            if src_file_id.is_none() || other_id.as_deref() != src_file_id {
                return Ok(true);
            }
        }
    }

    Ok(false)
}

fn check_forced_nfd(parent_dir: &Path, new_name: &str) -> bool {
    if !is_normalized_nfc(new_name) {
        return false;
    }
    let target_nfc = normalize_nfc(new_name);
    let vp_parent = verbatim_path_buf(parent_dir);
    let Ok(entries) = fs::read_dir(vp_parent.as_ref()) else {
        return false;
    };
    let mut saw_nfc = false;
    let mut saw_non_nfc = false;
    for entry_res in entries.flatten() {
        let name = entry_res.file_name().to_string_lossy().into_owned();
        if name == new_name {
            saw_nfc = true;
        } else if normalize_nfc(&name) == target_nfc {
            saw_non_nfc = true;
        }
    }
    !saw_nfc && saw_non_nfc
}

fn resolve_path_with_ancestor_canon(path: &Path) -> PathBuf {
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else if let Ok(cwd) = std::env::current_dir() {
        cwd.join(path)
    } else {
        path.to_path_buf()
    };
    let mut suffix: Vec<OsString> = Vec::new();
    let mut probe = abs.as_path();
    loop {
        let vp = verbatim_path_buf(probe);
        if let Ok(canon) = fs::canonicalize(vp.as_ref()) {
            let mut out = canon;
            for comp in suffix.iter().rev() {
                out.push(comp);
            }
            return out;
        }
        if let (Some(parent), Some(name)) = (probe.parent(), probe.file_name()) {
            suffix.push(name.to_os_string());
            probe = parent;
        } else {
            return abs;
        }
    }
}

fn validate_audit_canon_outside_dir(
    audit_canon: &Path,
    audit_path: &Path,
    dir: &Path,
) -> Result<(), CoreError> {
    let vp_dir = verbatim_path_buf(dir);
    let dir_canon =
        fs::canonicalize(vp_dir.as_ref()).unwrap_or_else(|_| resolve_path_with_ancestor_canon(dir));
    if audit_canon == dir_canon || audit_canon.starts_with(&dir_canon) {
        return Err(CoreError::UnsafePath(format!(
            "audit log path must be stored outside target directory ({}): {}",
            dir.display(),
            audit_path.display()
        )));
    }
    Ok(())
}

/// Validates that `audit_path` resolves strictly outside `dir`.
pub fn validate_audit_outside_dir(audit_path: &Path, dir: &Path) -> Result<(), CoreError> {
    let audit_canon = resolve_path_with_ancestor_canon(audit_path);
    validate_audit_canon_outside_dir(&audit_canon, audit_path, dir)
}

fn validate_audit_outside_targets(
    audit_path: &Path,
    items: &[SafeRenameItem],
) -> Result<(), CoreError> {
    let audit_canon = resolve_path_with_ancestor_canon(audit_path);
    let mut checked_parents: std::collections::HashSet<&Path> = std::collections::HashSet::new();
    for item in items {
        if !checked_parents.insert(item.parent_dir.as_path()) {
            continue;
        }
        validate_audit_canon_outside_dir(&audit_canon, audit_path, &item.parent_dir)?;
    }
    Ok(())
}

fn flush_audit_log(path: Option<&Path>, log: &RenameAuditLog) -> Result<(), CoreError> {
    let Some(p) = path else {
        return Ok(());
    };
    let parent = p
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    if parent != Path::new(".") {
        let vp_parent = verbatim_path_buf(parent);
        fs::create_dir_all(vp_parent.as_ref())?;
    }
    let fname = p
        .file_name()
        .map(|n| n.to_string_lossy())
        .unwrap_or_else(|| std::borrow::Cow::Borrowed("audit.json"));
    let tmp_path = parent.join(format!(".{fname}.tmp.{}", uuid::Uuid::new_v4().simple()));
    let vp_tmp = verbatim_path_buf(&tmp_path);
    let vp_dst = verbatim_path_buf(p);

    let json = serde_json::to_string_pretty(log).map_err(CoreError::Serialization)?;
    fs::write(vp_tmp.as_ref(), &json)?;
    if let Err(err) = fs::rename(vp_tmp.as_ref(), vp_dst.as_ref()) {
        let _ = fs::remove_file(vp_tmp.as_ref());
        return Err(CoreError::Io(err));
    }
    Ok(())
}

fn now_utc_iso() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// Applies safe non-overwriting renames across `items`, persisting incremental audit logs.
pub fn apply_safe_renames(
    items: &[SafeRenameItem],
    audit_log_path: Option<&Path>,
    dry_run: bool,
) -> Result<RenameAuditLog, CoreError> {
    if let Some(audit_path) = audit_log_path {
        if let Some(bad) = items.iter().find(|it| it.parent_dir.to_str().is_none()) {
            return Err(CoreError::UnsafePath(format!(
                "audit log requires valid UTF-8 parent_dir: {:?}",
                bad.parent_dir
            )));
        }
        validate_audit_outside_targets(audit_path, items)?;
    }

    let mut target_counts: HashMap<(PathBuf, String), usize> = HashMap::new();
    for item in items {
        if validate_single_component_name(&item.new_name).is_ok()
            && validate_no_traversal_raw(&item.old_raw_os_name).is_ok()
        {
            let key = (
                item.parent_dir.clone(),
                normalize_nfc(&item.new_name).to_lowercase(),
            );
            *target_counts.entry(key).or_insert(0) += 1;
        }
    }

    let mut log = RenameAuditLog {
        schema_version: SCHEMA_VERSION.to_string(),
        operations: Vec::with_capacity(items.len()),
    };
    let mut forced_nfd_volumes: std::collections::HashSet<String> =
        std::collections::HashSet::new();
    let mut sibling_cache: HashMap<PathBuf, SiblingIndex> = HashMap::new();
    let mut pending_flush = false;

    for item in items {
        let timestamp_utc = now_utc_iso();
        if let Err(err) = validate_no_traversal_raw(&item.old_raw_os_name)
            .and_then(|()| validate_single_component_name(&item.new_name))
        {
            log.operations.push(RenameOperation {
                timestamp_utc,
                parent_dir: item.parent_dir.clone(),
                old_name: item.old_name.clone(),
                old_raw_os_name: item.old_raw_os_name.clone(),
                file_id: None,
                entry_type: EntryType::File,
                byte_size: None,
                mtime_ns: None,
                temp_name: None,
                new_name: item.new_name.clone(),
                status: RenameStatus::IoError,
                error_detail: Some(err.to_string()),
            });
            if !dry_run {
                flush_audit_log(audit_log_path, &log)?;
                pending_flush = false;
            }
            continue;
        }

        let old_os = raw_os_to_os_string(&item.old_raw_os_name)?;
        let src_path = item.parent_dir.join(&old_os);
        let dst_path = item.parent_dir.join(&item.new_name);
        let vp_src = verbatim_path_buf(&src_path);

        let src_meta = match fs::symlink_metadata(vp_src.as_ref()) {
            Ok(m) => m,
            Err(err) => {
                log.operations.push(RenameOperation {
                    timestamp_utc,
                    parent_dir: item.parent_dir.clone(),
                    old_name: item.old_name.clone(),
                    old_raw_os_name: item.old_raw_os_name.clone(),
                    file_id: None,
                    entry_type: EntryType::File,
                    byte_size: None,
                    mtime_ns: None,
                    temp_name: None,
                    new_name: item.new_name.clone(),
                    status: map_io_status(&err),
                    error_detail: Some(err.to_string()),
                });
                if !dry_run {
                    flush_audit_log(audit_log_path, &log)?;
                    pending_flush = false;
                }
                continue;
            }
        };

        let (file_id, entry_type, byte_size, mtime_ns) = metadata_details(&src_meta, &src_path);
        if entry_type == EntryType::Symlink {
            log.operations.push(RenameOperation {
                timestamp_utc,
                parent_dir: item.parent_dir.clone(),
                old_name: item.old_name.clone(),
                old_raw_os_name: item.old_raw_os_name.clone(),
                file_id,
                entry_type,
                byte_size,
                mtime_ns,
                temp_name: None,
                new_name: item.new_name.clone(),
                status: RenameStatus::BlockedSymlink,
                error_detail: Some("refusing to rename symlink".to_string()),
            });
            if !dry_run {
                flush_audit_log(audit_log_path, &log)?;
                pending_flush = false;
            }
            continue;
        }

        let batch_key = (
            item.parent_dir.clone(),
            normalize_nfc(&item.new_name).to_lowercase(),
        );
        let intra_batch_collision = target_counts.get(&batch_key).copied().unwrap_or(0) > 1;
        let disk_collision = match check_disk_sibling_collision(
            &item.parent_dir,
            &mut sibling_cache,
            file_id.as_deref(),
            &old_os,
            &item.new_name,
        ) {
            Ok(collided) => collided,
            Err(err) => {
                log.operations.push(RenameOperation {
                    timestamp_utc,
                    parent_dir: item.parent_dir.clone(),
                    old_name: item.old_name.clone(),
                    old_raw_os_name: item.old_raw_os_name.clone(),
                    file_id,
                    entry_type,
                    byte_size,
                    mtime_ns,
                    temp_name: None,
                    new_name: item.new_name.clone(),
                    status: map_io_status(&err),
                    error_detail: Some(err.to_string()),
                });
                if !dry_run {
                    flush_audit_log(audit_log_path, &log)?;
                    pending_flush = false;
                }
                continue;
            }
        };

        if intra_batch_collision || disk_collision {
            log.operations.push(RenameOperation {
                timestamp_utc,
                parent_dir: item.parent_dir.clone(),
                old_name: item.old_name.clone(),
                old_raw_os_name: item.old_raw_os_name.clone(),
                file_id,
                entry_type,
                byte_size,
                mtime_ns,
                temp_name: None,
                new_name: item.new_name.clone(),
                status: RenameStatus::BlockedCollision,
                error_detail: Some(
                    "target filename collides with sibling or batch entry".to_string(),
                ),
            });
            if !dry_run {
                flush_audit_log(audit_log_path, &log)?;
                pending_flush = false;
            }
            continue;
        }

        if dry_run {
            log.operations.push(RenameOperation {
                timestamp_utc,
                parent_dir: item.parent_dir.clone(),
                old_name: item.old_name.clone(),
                old_raw_os_name: item.old_raw_os_name.clone(),
                file_id,
                entry_type,
                byte_size,
                mtime_ns,
                temp_name: None,
                new_name: item.new_name.clone(),
                status: RenameStatus::Applied,
                error_detail: None,
            });
            continue;
        }

        let vol_key = volume_key_for_dir(&item.parent_dir);
        if should_halt_forced_nfd_item(
            &forced_nfd_volumes,
            &vol_key,
            &item.old_name,
            &item.new_name,
        ) {
            log.operations.push(RenameOperation {
                timestamp_utc,
                parent_dir: item.parent_dir.clone(),
                old_name: item.old_name.clone(),
                old_raw_os_name: item.old_raw_os_name.clone(),
                file_id,
                entry_type,
                byte_size,
                mtime_ns,
                temp_name: None,
                new_name: item.new_name.clone(),
                status: RenameStatus::FilesystemForcedNfd,
                error_detail: Some(
                    "halted NFD-to-NFC rename after volume driver forced NFD".to_string(),
                ),
            });
            flush_audit_log(audit_log_path, &log)?;
            pending_flush = false;
            continue;
        }

        let should_probe_forced_nfd =
            is_normalized_nfc(&item.new_name) && !is_normalized_nfc(&item.old_name);
        let two_step = is_two_step_candidate(&item.old_name, &item.new_name);
        let (status, error_detail, temp_name) = if two_step {
            let tmp = format!(".__nc_tmp_{}", uuid::Uuid::new_v4().simple());
            let tmp_path = item.parent_dir.join(&tmp);
            match atomic_rename_with_retry(&src_path, &tmp_path) {
                Ok(()) => match atomic_rename_with_retry(&tmp_path, &dst_path) {
                    Ok(()) => {
                        if should_probe_forced_nfd
                            && check_forced_nfd(&item.parent_dir, &item.new_name)
                        {
                            forced_nfd_volumes.insert(vol_key);
                            (
                                RenameStatus::FilesystemForcedNfd,
                                Some("volume driver forcibly decomposed target to NFD".to_string()),
                                None,
                            )
                        } else {
                            (RenameStatus::Applied, None, None)
                        }
                    }
                    Err(step2_err) => {
                        let st = map_io_status(&step2_err);
                        if atomic_rename_with_retry(&tmp_path, &src_path).is_ok() {
                            (
                                st,
                                Some(format!("Step 2 failed (rolled back): {step2_err}")),
                                None,
                            )
                        } else {
                            (
                                st,
                                Some(format!(
                                    "Step 2 and Step 1 rollback both failed: {step2_err}"
                                )),
                                Some(tmp),
                            )
                        }
                    }
                },
                Err(step1_err) => (map_io_status(&step1_err), Some(step1_err.to_string()), None),
            }
        } else {
            match atomic_rename_with_retry(&src_path, &dst_path) {
                Ok(()) => {
                    if should_probe_forced_nfd && check_forced_nfd(&item.parent_dir, &item.new_name)
                    {
                        forced_nfd_volumes.insert(vol_key);
                        (
                            RenameStatus::FilesystemForcedNfd,
                            Some("volume driver forcibly decomposed target to NFD".to_string()),
                            None,
                        )
                    } else {
                        (RenameStatus::Applied, None, None)
                    }
                }
                Err(err) => (map_io_status(&err), Some(err.to_string()), None),
            }
        };

        let (post_id, post_type, post_size, post_mtime) = if matches!(
            status,
            RenameStatus::Applied | RenameStatus::FilesystemForcedNfd
        ) {
            if let Some(index) = sibling_cache.get_mut(&item.parent_dir) {
                index.record_rename(&old_os, &item.new_name);
            }
            capture_file_id_and_metadata(&dst_path)
                .unwrap_or((file_id, entry_type, byte_size, mtime_ns))
        } else {
            (file_id, entry_type, byte_size, mtime_ns)
        };

        let flush_now = status != RenameStatus::Applied
            || temp_name.is_some()
            || log.operations.len().is_multiple_of(64);
        log.operations.push(RenameOperation {
            timestamp_utc,
            parent_dir: item.parent_dir.clone(),
            old_name: item.old_name.clone(),
            old_raw_os_name: item.old_raw_os_name.clone(),
            file_id: post_id,
            entry_type: post_type,
            byte_size: post_size,
            mtime_ns: post_mtime,
            temp_name,
            new_name: item.new_name.clone(),
            status,
            error_detail,
        });
        if flush_now {
            flush_audit_log(audit_log_path, &log)?;
            pending_flush = false;
        } else {
            pending_flush = true;
        }
    }

    if !dry_run && pending_flush {
        flush_audit_log(audit_log_path, &log)?;
    }

    Ok(log)
}

fn identity_matches(op: &RenameOperation, cur_path: &Path) -> bool {
    let Some((cur_id, cur_type, cur_size, cur_mtime)) = capture_file_id_and_metadata(cur_path)
    else {
        return false;
    };
    if cur_type != op.entry_type {
        return false;
    }
    if let (Some(expected_id), Some(actual_id)) = (op.file_id.as_deref(), cur_id.as_deref()) {
        if expected_id == actual_id {
            return op.byte_size.is_none() || op.byte_size == cur_size;
        }
        // On SMB/CIFS mounts st_dev:st_ino can change; verify fallback metadata.
        if op.mtime_ns.is_none() || op.mtime_ns != cur_mtime {
            return false;
        }
        return match op.entry_type {
            EntryType::File => op.byte_size.is_some() && op.byte_size == cur_size,
            EntryType::Directory | EntryType::Symlink => true,
        };
    }
    if op.mtime_ns.is_none() || op.mtime_ns != cur_mtime {
        return false;
    }
    match op.entry_type {
        EntryType::File => op.byte_size.is_some() && op.byte_size == cur_size,
        EntryType::Directory | EntryType::Symlink => true,
    }
}

/// Loads and parses a `RenameAuditLog` JSON file from disk.
pub fn load_audit_log(audit_log_path: &Path) -> Result<RenameAuditLog, CoreError> {
    let vp = verbatim_path_buf(audit_log_path);
    let raw = fs::read_to_string(vp.as_ref())?;
    serde_json::from_str(&raw).map_err(CoreError::Serialization)
}

fn is_recoverable_op(op: &RenameOperation) -> bool {
    op.status == RenameStatus::Applied
        || op.temp_name.is_some()
        || (op.status == RenameStatus::FilesystemForcedNfd
            && normalize_nfc(&op.old_name) != normalize_nfc(&op.new_name))
}

fn resolve_forced_nfd_sibling(
    op: &RenameOperation,
    force: bool,
) -> Result<(PathBuf, String), String> {
    let target_nfc = normalize_nfc(&op.new_name);
    let vp_parent = verbatim_path_buf(&op.parent_dir);
    let entries = fs::read_dir(vp_parent.as_ref())
        .map_err(|err| format!("failed to inspect parent_dir for NFD rollback: {err}"))?;
    let mut nfc_candidates: Vec<(PathBuf, String)> = Vec::new();
    for entry_res in entries {
        let entry = entry_res.map_err(|err| format!("failed reading directory entry: {err}"))?;
        let os_name = entry.file_name();
        let name = os_name.to_string_lossy().into_owned();
        if normalize_nfc(&name) == target_nfc {
            nfc_candidates.push((op.parent_dir.join(&os_name), name));
        }
    }

    if nfc_candidates.is_empty() {
        return Err("target NFD sibling not found on disk".to_string());
    }

    if force {
        if nfc_candidates.len() > 1 {
            return Err("ambiguous multiple NFC-equivalent siblings on disk".to_string());
        }
        return Ok(nfc_candidates.remove(0));
    }

    let matched: Vec<(PathBuf, String)> = nfc_candidates
        .into_iter()
        .filter(|(cand_path, _)| identity_matches(op, cand_path))
        .collect();
    match matched.len() {
        1 => Ok(matched.into_iter().next().expect("single match")),
        0 => Err("file_id/metadata verification failed (use --force to override)".to_string()),
        _ => Err("ambiguous multiple NFC-equivalent siblings matched metadata".to_string()),
    }
}

/// Reverses operations recorded in an audit log using `file_id` + SMB metadata verification.
pub fn undo_audit_log(
    audit_log_path: &Path,
    dry_run: bool,
    force: bool,
) -> Result<Vec<RenameOperation>, CoreError> {
    let log = load_audit_log(audit_log_path)?;

    // Reject path traversal and tampered parent_dir strictly on recoverable operations BEFORE mutating disk, even with force.
    // Use `fs::metadata` on `parent_dir` so valid symlinked mount points pass while child entries use `symlink_metadata`.
    for op in log.operations.iter().filter(|op| is_recoverable_op(op)) {
        let vp_parent = verbatim_path_buf(&op.parent_dir);
        let meta = fs::metadata(vp_parent.as_ref()).map_err(|err| {
            CoreError::UnsafePath(format!(
                "audit parent_dir unreachable ({:?}): {err}",
                op.parent_dir
            ))
        })?;
        if !meta.is_dir() {
            return Err(CoreError::UnsafePath(format!(
                "audit parent_dir is not a directory: {:?}",
                op.parent_dir
            )));
        }
        validate_no_traversal_str(&op.new_name)?;
        validate_no_traversal_raw(&op.old_raw_os_name)?;
        if let Some(ref tmp) = op.temp_name {
            validate_no_traversal_str(tmp)?;
        }
    }

    let mut results = Vec::new();
    for op in log.operations.iter().rev() {
        if !is_recoverable_op(op) {
            continue;
        }

        let mut cur_name = op
            .temp_name
            .as_deref()
            .unwrap_or(op.new_name.as_str())
            .to_string();
        let mut cur_path = op.parent_dir.join(&cur_name);
        let old_os = raw_os_to_os_string(&op.old_raw_os_name)?;
        let restore_path = op.parent_dir.join(&old_os);

        let mut identity_verified = false;
        if op.status == RenameStatus::FilesystemForcedNfd && op.temp_name.is_none() {
            match resolve_forced_nfd_sibling(op, force) {
                Ok((resolved_path, resolved_name)) => {
                    cur_path = resolved_path;
                    cur_name = resolved_name;
                    identity_verified = !force;
                }
                Err(detail) => {
                    let mut failed = op.clone();
                    failed.status = RenameStatus::IoError;
                    failed.error_detail = Some(detail);
                    results.push(failed);
                    continue;
                }
            }
        }

        if !force && !identity_verified && !identity_matches(op, &cur_path) {
            let mut failed = op.clone();
            failed.status = RenameStatus::IoError;
            failed.error_detail =
                Some("file_id/metadata verification failed (use --force to override)".to_string());
            results.push(failed);
            continue;
        }

        if dry_run {
            let mut preview = op.clone();
            preview.status = RenameStatus::Applied;
            preview.error_detail = None;
            results.push(preview);
            continue;
        }

        let two_step = is_two_step_candidate(&cur_name, &op.old_name);
        let rename_res = if two_step {
            let tmp = op
                .parent_dir
                .join(format!(".__nc_tmp_{}", uuid::Uuid::new_v4().simple()));
            match atomic_rename_with_retry(&cur_path, &tmp) {
                Ok(()) => match atomic_rename_with_retry(&tmp, &restore_path) {
                    Ok(()) => Ok(()),
                    Err(e) => {
                        let _ = atomic_rename_with_retry(&tmp, &cur_path);
                        Err(e)
                    }
                },
                Err(e) => Err(e),
            }
        } else {
            atomic_rename_with_retry(&cur_path, &restore_path)
        };

        let mut outcome = op.clone();
        match rename_res {
            Ok(()) => {
                outcome.status = RenameStatus::Applied;
                outcome.temp_name = None;
                outcome.error_detail = None;
            }
            Err(err) => {
                outcome.status = map_io_status(&err);
                outcome.error_detail = Some(err.to_string());
            }
        }
        results.push(outcome);
    }

    Ok(results)
}

/// Returns the default OS user state/data directory path for audit logs outside target folders.
#[must_use]
pub fn default_audit_log_dir() -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_STATE_HOME")
        && !xdg.is_empty()
    {
        return PathBuf::from(xdg).join("namechecker");
    }
    #[cfg(target_os = "macos")]
    if let Ok(home) = std::env::var("HOME")
        && !home.is_empty()
    {
        return PathBuf::from(home)
            .join("Library")
            .join("Application Support")
            .join("namechecker");
    }
    #[cfg(windows)]
    if let Ok(local) = std::env::var("LOCALAPPDATA")
        && !local.is_empty()
    {
        return PathBuf::from(local).join("namechecker");
    }
    if let Ok(home) = std::env::var("HOME")
        && !home.is_empty()
    {
        return PathBuf::from(home)
            .join(".local")
            .join("state")
            .join("namechecker");
    }
    std::env::temp_dir().join("namechecker-state")
}

/// Returns a unique timestamped audit-log file path inside `default_audit_log_dir()`.
#[must_use]
pub fn default_audit_log_path() -> PathBuf {
    let ts = chrono::Utc::now().format("%Y%m%d-%H%M%S");
    let uid = uuid::Uuid::new_v4().simple().to_string();
    default_audit_log_dir().join(format!("namechecker-rename-audit-{ts}-{}.json", &uid[..8]))
}
