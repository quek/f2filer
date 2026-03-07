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
