#[cfg(windows)]
pub fn show_file_properties(path: &std::path::Path) {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::UI::Shell::{ShellExecuteExW, SHELLEXECUTEINFOW, SEE_MASK_INVOKEIDLIST};

    let path_wide: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    let verb: Vec<u16> = "properties\0".encode_utf16().collect();

    let mut sei = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_INVOKEIDLIST,
        lpVerb: PCWSTR(verb.as_ptr()),
        lpFile: PCWSTR(path_wide.as_ptr()),
        ..Default::default()
    };

    unsafe {
        let _ = ShellExecuteExW(&mut sei);
    }
}

/// Open a file using the application associated with .txt extension.
#[cfg(windows)]
pub fn open_with_text_editor(path: &std::path::Path) {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::UI::Shell::{ShellExecuteExW, SHELLEXECUTEINFOW, SEE_MASK_CLASSNAME};

    let path_wide: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    let verb: Vec<u16> = "open\0".encode_utf16().collect();
    let class: Vec<u16> = ".txt\0".encode_utf16().collect();

    let mut sei = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_CLASSNAME,
        lpVerb: PCWSTR(verb.as_ptr()),
        lpFile: PCWSTR(path_wide.as_ptr()),
        lpClass: PCWSTR(class.as_ptr()),
        ..Default::default()
    };

    unsafe {
        let _ = ShellExecuteExW(&mut sei);
    }
}

/// Escape a path string for use inside PowerShell single-quoted strings.
/// Single quotes are escaped by doubling them: ' → ''
#[cfg(windows)]
fn ps_escape(s: &str) -> String {
    s.replace('\'', "''")
}

/// Run a PowerShell script with elevated (administrator) privileges via UAC.
/// Uses ShellExecuteExW with "runas" verb, waits for process completion.
#[cfg(windows)]
fn run_elevated_powershell(ps_script: &str) -> Result<(), String> {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
    use windows::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject, INFINITE};
    use windows::Win32::UI::Shell::{ShellExecuteExW, SHELLEXECUTEINFOW, SEE_MASK_NOCLOSEPROCESS};

    let params = format!(
        "-NoProfile -ExecutionPolicy Bypass -Command \"{}\"",
        ps_script
    );

    let file: Vec<u16> = "powershell.exe\0".encode_utf16().collect();
    let verb: Vec<u16> = "runas\0".encode_utf16().collect();
    let params_wide: Vec<u16> = params.encode_utf16().chain(std::iter::once(0)).collect();

    let mut sei = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS,
        lpVerb: PCWSTR(verb.as_ptr()),
        lpFile: PCWSTR(file.as_ptr()),
        lpParameters: PCWSTR(params_wide.as_ptr()),
        ..Default::default()
    };

    unsafe {
        if ShellExecuteExW(&mut sei).is_err() {
            return Err("UAC ダイアログがキャンセルされたか、昇格に失敗しました".to_string());
        }

        let process = sei.hProcess;
        if process.is_invalid() || process.0.is_null() {
            return Err("昇格プロセスのハンドルを取得できませんでした".to_string());
        }

        let wait_result = WaitForSingleObject(process, INFINITE);
        if wait_result != WAIT_OBJECT_0 {
            let _ = CloseHandle(process);
            return Err("昇格プロセスの完了待機に失敗しました".to_string());
        }

        let mut exit_code: u32 = 1;
        let _ = GetExitCodeProcess(process, &mut exit_code);
        let _ = CloseHandle(process);

        if exit_code != 0 {
            return Err(format!("昇格操作が失敗しました (exit code: {})", exit_code));
        }
    }

    Ok(())
}

/// Execute a copy or move operation with elevated (administrator) privileges.
#[cfg(windows)]
pub fn elevated_copy_or_move(
    sources: &[std::path::PathBuf],
    dest_dir: &std::path::Path,
    is_move: bool,
    overwrite: bool,
) -> Result<(), String> {
    let cmdlet = if is_move { "Move-Item" } else { "Copy-Item" };
    let force_flag = if overwrite { " -Force" } else { "" };
    let dest_escaped = ps_escape(&dest_dir.to_string_lossy());

    let mut ps_commands: Vec<String> = Vec::new();
    for src in sources {
        let src_escaped = ps_escape(&src.to_string_lossy());
        let recurse = if src.is_dir() { " -Recurse" } else { "" };
        ps_commands.push(format!(
            "{} -LiteralPath '{}' -Destination '{}'{}{}",
            cmdlet, src_escaped, dest_escaped, recurse, force_flag
        ));
    }

    run_elevated_powershell(&ps_commands.join("; "))
}

/// Execute a delete operation with elevated (administrator) privileges.
#[cfg(windows)]
pub fn elevated_delete(paths: &[std::path::PathBuf]) -> Result<(), String> {
    let mut ps_commands: Vec<String> = Vec::new();
    for path in paths {
        let path_escaped = ps_escape(&path.to_string_lossy());
        ps_commands.push(format!(
            "Remove-Item -LiteralPath '{}' -Recurse -Force",
            path_escaped
        ));
    }

    run_elevated_powershell(&ps_commands.join("; "))
}

/// Copy or cut file paths to the Windows clipboard using CF_HDROP format.
/// `is_cut` sets Preferred DropEffect to DROPEFFECT_MOVE, otherwise DROPEFFECT_COPY.
#[cfg(windows)]
pub fn copy_files_to_clipboard(paths: &[std::path::PathBuf], is_cut: bool) {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::DataExchange::*;
    use windows::Win32::System::Memory::*;
    use windows::Win32::System::Ole::CF_HDROP;

    // Build wide string file list: each path null-terminated, ending with extra null
    let mut file_list: Vec<u16> = Vec::new();
    for path in paths {
        file_list.extend(path.as_os_str().encode_wide());
        file_list.push(0);
    }
    file_list.push(0); // final null terminator

    // DROPFILES header: 20 bytes (pFiles=20, pt={0,0}, fNC=0, fWide=1)
    let header_size: usize = 20;
    let data_size = header_size + file_list.len() * 2;

    unsafe {
        if OpenClipboard(Some(HWND::default())).is_err() {
            return;
        }
        let _ = EmptyClipboard();

        // Allocate and fill CF_HDROP data
        let hmem = GlobalAlloc(GMEM_MOVEABLE | GMEM_ZEROINIT, data_size);
        let Ok(hmem) = hmem else {
            let _ = CloseClipboard();
            return;
        };

        let ptr = GlobalLock(hmem);
        if !ptr.is_null() {
            let header = ptr as *mut u8;
            // pFiles (offset to file list) = 20
            *(header as *mut u32) = header_size as u32;
            // fWide = 1 (at offset 16)
            *((header.add(16)) as *mut u32) = 1;
            // Copy file list after header
            std::ptr::copy_nonoverlapping(
                file_list.as_ptr() as *const u8,
                header.add(header_size),
                file_list.len() * 2,
            );
            let _ = GlobalUnlock(hmem);
        }

        let _ = SetClipboardData(CF_HDROP.0 as u32, Some(windows::Win32::Foundation::HANDLE(hmem.0)));

        // Set Preferred DropEffect
        let format_name: Vec<u16> = "Preferred DropEffect\0".encode_utf16().collect();
        let cf = RegisterClipboardFormatW(windows::core::PCWSTR(format_name.as_ptr()));
        if cf != 0 {
            let effect_mem = GlobalAlloc(GMEM_MOVEABLE | GMEM_ZEROINIT, 4);
            if let Ok(effect_mem) = effect_mem {
                let ptr = GlobalLock(effect_mem);
                if !ptr.is_null() {
                    let effect: u32 = if is_cut { 2 } else { 1 }; // DROPEFFECT_MOVE=2, DROPEFFECT_COPY=1
                    *(ptr as *mut u32) = effect;
                    let _ = GlobalUnlock(effect_mem);
                }
                let _ = SetClipboardData(cf, Some(windows::Win32::Foundation::HANDLE(effect_mem.0)));
            }
        }

        let _ = CloseClipboard();
    }
}

/// Read file paths from Windows clipboard (CF_HDROP format).
/// Returns (paths, is_cut) where is_cut indicates DROPEFFECT_MOVE.
#[cfg(windows)]
pub fn paste_files_from_clipboard() -> Option<(Vec<std::path::PathBuf>, bool)> {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::DataExchange::*;
    use windows::Win32::System::Memory::*;
    use windows::Win32::System::Ole::CF_HDROP;
    use windows::Win32::UI::Shell::*;

    unsafe {
        if OpenClipboard(Some(HWND::default())).is_err() {
            return None;
        }

        // Check CF_HDROP availability
        let hdrop_handle = GetClipboardData(CF_HDROP.0 as u32);
        let Ok(hdrop_handle) = hdrop_handle else {
            let _ = CloseClipboard();
            return None;
        };

        let hdrop = HDROP(hdrop_handle.0);
        let count = DragQueryFileW(hdrop, u32::MAX, None);
        if count == 0 {
            let _ = CloseClipboard();
            return None;
        }

        let mut paths = Vec::new();
        for i in 0..count {
            let len = DragQueryFileW(hdrop, i, None);
            let mut buf = vec![0u16; (len + 1) as usize];
            DragQueryFileW(hdrop, i, Some(&mut buf));
            // Remove trailing null
            if buf.last() == Some(&0) {
                buf.pop();
            }
            paths.push(std::path::PathBuf::from(String::from_utf16_lossy(&buf)));
        }

        // Check Preferred DropEffect
        let format_name: Vec<u16> = "Preferred DropEffect\0".encode_utf16().collect();
        let cf = RegisterClipboardFormatW(windows::core::PCWSTR(format_name.as_ptr()));
        let mut is_cut = false;
        if cf != 0 {
            if let Ok(effect_handle) = GetClipboardData(cf) {
                let hmem = windows::Win32::Foundation::HGLOBAL(effect_handle.0);
                let ptr = GlobalLock(hmem);
                if !ptr.is_null() {
                    let effect = *(ptr as *const u32);
                    is_cut = effect == 2; // DROPEFFECT_MOVE
                    let _ = GlobalUnlock(hmem);
                }
            }
        }

        let _ = CloseClipboard();
        Some((paths, is_cut))
    }
}

#[cfg(windows)]
pub fn show_context_menu(path: &std::path::Path) {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::*;
    use windows::Win32::System::Com::*;
    use windows::Win32::UI::Shell::Common::*;
    use windows::Win32::UI::Shell::*;
    use windows::Win32::UI::WindowsAndMessaging::*;

    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

        let path_wide: Vec<u16> = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

        let mut pidl: *mut ITEMIDLIST = std::ptr::null_mut();
        if SHParseDisplayName(PCWSTR(path_wide.as_ptr()), None, &mut pidl, 0, None).is_err() {
            return;
        }

        let mut child_pidl: *mut ITEMIDLIST = std::ptr::null_mut();
        let folder: IShellFolder =
            match SHBindToParent(pidl, Some(&mut child_pidl as *mut *mut _ as *mut *mut _)) {
                Ok(f) => f,
                Err(_) => {
                    CoTaskMemFree(Some(pidl as _));
                    return;
                }
            };

        let child_pidl = child_pidl as *const ITEMIDLIST;
        let ctx_menu: IContextMenu = match folder.GetUIObjectOf(
            windows::Win32::Foundation::HWND::default(),
            &[child_pidl],
            None,
        ) {
            Ok(m) => m,
            Err(_) => {
                CoTaskMemFree(Some(pidl as _));
                return;
            }
        };

        let hmenu = match CreatePopupMenu() {
            Ok(m) => m,
            Err(_) => {
                CoTaskMemFree(Some(pidl as _));
                return;
            }
        };

        let first_cmd: u32 = 1;
        if ctx_menu
            .QueryContextMenu(hmenu, 0, first_cmd, 0x7FFF, CMF_NORMAL)
            .is_err()
        {
            let _ = DestroyMenu(hmenu);
            CoTaskMemFree(Some(pidl as _));
            return;
        }

        let hwnd = GetForegroundWindow();
        let mut pt = windows::Win32::Foundation::POINT::default();
        let _ = GetCursorPos(&mut pt);
        let _ = SetForegroundWindow(hwnd);

        let cmd = TrackPopupMenuEx(
            hmenu,
            TPM_RETURNCMD.0 | TPM_RIGHTBUTTON.0,
            pt.x,
            pt.y,
            hwnd,
            None,
        );

        if cmd.0 != 0 {
            let verb = (cmd.0 as u32).wrapping_sub(first_cmd) as usize;
            let info = CMINVOKECOMMANDINFO {
                cbSize: std::mem::size_of::<CMINVOKECOMMANDINFO>() as u32,
                hwnd,
                lpVerb: windows::core::PCSTR(verb as *const u8),
                nShow: 1,
                ..Default::default()
            };
            let _ = ctx_menu.InvokeCommand(&info);
        }

        let _ = DestroyMenu(hmenu);
        CoTaskMemFree(Some(pidl as _));
    }
}
