#![allow(non_snake_case)]

use std::path::PathBuf;
use std::time::{Duration, Instant};

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::System::Com::*;
use windows::Win32::System::Memory::*;
use windows::Win32::System::Ole::*;
use windows::Win32::System::SystemServices::{MK_LBUTTON, MODIFIERKEYS_FLAGS};
use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowThreadProcessId, SetForegroundWindow,
};

/// `DoDragDrop` がこれより早く戻った場合、ユーザー操作ではなく開始失敗（キャプチャ不可等）と
/// みなして診断メッセージを返す。egui のドラッグ閾値を越えてから指を離すには最低でも
/// 数十ミリ秒かかるため、正常なドラッグがこの時間内に完了することはない。
const INSTANT_RETURN: Duration = Duration::from_millis(100);

const CF_HDROP_VALUE: u16 = 15;

/// DROPFILES header for CF_HDROP format.
#[repr(C)]
struct DropFilesHeader {
    p_files: u32,
    pt_x: i32,
    pt_y: i32,
    f_nc: i32,
    f_wide: i32,
}

// ─── IDropSource ───

#[implement(IDropSource)]
struct DropSource;

impl IDropSource_Impl for DropSource_Impl {
    fn QueryContinueDrag(
        &self,
        fescapepressed: BOOL,
        grfkeystate: MODIFIERKEYS_FLAGS,
    ) -> HRESULT {
        if fescapepressed.as_bool() {
            DRAGDROP_S_CANCEL
        } else if (grfkeystate & MK_LBUTTON).0 == 0 {
            DRAGDROP_S_DROP
        } else {
            S_OK
        }
    }

    fn GiveFeedback(&self, _dweffect: DROPEFFECT) -> HRESULT {
        DRAGDROP_S_USEDEFAULTCURSORS
    }
}

// ─── IDataObject ───

#[implement(IDataObject)]
struct FileDataObject {
    hglobal: HGLOBAL,
}

impl Drop for FileDataObject {
    fn drop(&mut self) {
        unsafe {
            if !self.hglobal.0.is_null() {
                let _ = GlobalFree(Some(self.hglobal));
            }
        }
    }
}

impl IDataObject_Impl for FileDataObject_Impl {
    fn GetData(&self, pformatetcin: *const FORMATETC) -> Result<STGMEDIUM> {
        unsafe {
            let fmt = &*pformatetcin;
            if fmt.cfFormat != CF_HDROP_VALUE {
                return Err(Error::new(DV_E_FORMATETC, "unsupported format"));
            }

            // Clone the HGLOBAL data
            let size = GlobalSize(self.hglobal);
            let new_hglobal = GlobalAlloc(GMEM_MOVEABLE, size)?;
            let src = GlobalLock(self.hglobal);
            let dst = GlobalLock(new_hglobal);
            std::ptr::copy_nonoverlapping(src as *const u8, dst as *mut u8, size);
            let _ = GlobalUnlock(self.hglobal);
            let _ = GlobalUnlock(new_hglobal);

            let mut medium: STGMEDIUM = std::mem::zeroed();
            medium.tymed = TYMED_HGLOBAL.0 as u32;
            medium.u.hGlobal = new_hglobal;
            Ok(medium)
        }
    }

    fn GetDataHere(
        &self,
        _pformatetc: *const FORMATETC,
        _pmedium: *mut STGMEDIUM,
    ) -> Result<()> {
        Err(Error::new(E_NOTIMPL, ""))
    }

    fn QueryGetData(&self, pformatetc: *const FORMATETC) -> HRESULT {
        unsafe {
            let fmt = &*pformatetc;
            if fmt.cfFormat == CF_HDROP_VALUE {
                S_OK
            } else {
                DV_E_FORMATETC
            }
        }
    }

    fn GetCanonicalFormatEtc(
        &self,
        _pformatectin: *const FORMATETC,
        _pformatetcout: *mut FORMATETC,
    ) -> HRESULT {
        E_NOTIMPL
    }

    fn SetData(
        &self,
        _pformatetc: *const FORMATETC,
        _pmedium: *const STGMEDIUM,
        _frelease: BOOL,
    ) -> Result<()> {
        Err(Error::new(E_NOTIMPL, ""))
    }

    fn EnumFormatEtc(&self, dwdirection: u32) -> Result<IEnumFORMATETC> {
        if dwdirection != 1 {
            // DATADIR_GET = 1
            return Err(Error::new(E_NOTIMPL, ""));
        }

        let format = FORMATETC {
            cfFormat: CF_HDROP_VALUE,
            ptd: std::ptr::null_mut(),
            dwAspect: 1, // DVASPECT_CONTENT
            lindex: -1,
            tymed: 1, // TYMED_HGLOBAL
        };

        let enumerator = FormatEnumerator {
            formats: vec![format],
            index: std::cell::Cell::new(0),
        };
        Ok(enumerator.into())
    }

    fn DAdvise(
        &self,
        _pformatetc: *const FORMATETC,
        _advf: u32,
        _padvsink: windows_core::Ref<'_, IAdviseSink>,
    ) -> Result<u32> {
        Err(Error::new(E_NOTIMPL, ""))
    }

    fn DUnadvise(&self, _dwconnection: u32) -> Result<()> {
        Err(Error::new(E_NOTIMPL, ""))
    }

    fn EnumDAdvise(&self) -> Result<IEnumSTATDATA> {
        Err(Error::new(E_NOTIMPL, ""))
    }
}

// ─── IEnumFORMATETC ───

#[implement(IEnumFORMATETC)]
struct FormatEnumerator {
    formats: Vec<FORMATETC>,
    index: std::cell::Cell<usize>,
}

impl IEnumFORMATETC_Impl for FormatEnumerator_Impl {
    fn Next(&self, celt: u32, rgelt: *mut FORMATETC, pceltfetched: *mut u32) -> HRESULT {
        unsafe {
            let remaining = self.formats.len() - self.index.get();
            let count = (celt as usize).min(remaining);
            for i in 0..count {
                *rgelt.add(i) = self.formats[self.index.get() + i];
            }
            self.index.set(self.index.get() + count);
            if !pceltfetched.is_null() {
                *pceltfetched = count as u32;
            }
            if count == celt as usize {
                S_OK
            } else {
                S_FALSE
            }
        }
    }

    fn Skip(&self, celt: u32) -> Result<()> {
        self.index
            .set((self.index.get() + celt as usize).min(self.formats.len()));
        Ok(())
    }

    fn Reset(&self) -> Result<()> {
        self.index.set(0);
        Ok(())
    }

    fn Clone(&self) -> Result<IEnumFORMATETC> {
        let enumerator = FormatEnumerator {
            formats: self.formats.clone(),
            index: std::cell::Cell::new(self.index.get()),
        };
        Ok(enumerator.into())
    }
}

// ─── HDROP builder ───

fn build_hdrop(paths: &[PathBuf]) -> Result<HGLOBAL> {
    let header_size = std::mem::size_of::<DropFilesHeader>();
    let wide_paths: Vec<Vec<u16>> = paths
        .iter()
        .map(|p| {
            let s = p.to_string_lossy();
            let mut wide: Vec<u16> = s.encode_utf16().collect();
            wide.push(0); // null terminator per path
            wide
        })
        .collect();

    let data_size: usize = wide_paths.iter().map(|w| w.len() * 2).sum::<usize>() + 2;
    let total_size = header_size + data_size;

    unsafe {
        let hglobal = GlobalAlloc(GMEM_MOVEABLE | GMEM_ZEROINIT, total_size)?;
        let ptr = GlobalLock(hglobal) as *mut u8;

        // Write DROPFILES header
        let header = ptr as *mut DropFilesHeader;
        (*header).p_files = header_size as u32;
        (*header).f_wide = 1; // Unicode paths

        // Write file paths (wide char, null-terminated each, double-null at end)
        let mut offset = header_size;
        for wide in &wide_paths {
            let dst = ptr.add(offset) as *mut u16;
            std::ptr::copy_nonoverlapping(wide.as_ptr(), dst, wide.len());
            offset += wide.len() * 2;
        }
        // Final double-null is already zeroed by GMEM_ZEROINIT

        let _ = GlobalUnlock(hglobal);
        Ok(hglobal)
    }
}

// ─── Public API ───

/// eframe の `Frame` からメインウィンドウの HWND を取得する。
///
/// フォアグラウンドフックが記録した HWND は、winit の補助ウィンドウ
/// （"Winit Thread Event Target"）がフォアグラウンドになった場合にそちらを指すため
/// 使わない。`Frame` の raw handle は常にメインウィンドウを指す。
pub fn window_hwnd(frame: &eframe::Frame) -> Option<HWND> {
    let handle = frame.window_handle().ok()?;
    match handle.as_raw() {
        RawWindowHandle::Win32(h) => Some(HWND(h.hwnd.get() as *mut core::ffi::c_void)),
        _ => None,
    }
}

/// 現在のフォアグラウンドウィンドウが自スレッドのものか。
/// マウスキャプチャの可否はフォアグラウンド「スレッド」で決まるため、HWND の一致ではなく
/// スレッド ID で判定する。
unsafe fn is_foreground_thread() -> bool {
    unsafe {
        let fg = GetForegroundWindow();
        !fg.is_invalid() && GetWindowThreadProcessId(fg, None) == GetCurrentThreadId()
    }
}

/// 自ウィンドウをフォアグラウンドにし、実際に切り替わったかを返す。
///
/// `SetForegroundWindow` はフォアグラウンドロック（他プロセスが直前に入力を受けていた等）で
/// 黙って拒否されることがある。その場合は現フォアグラウンドスレッドに入力キューを一時接続して
/// 同一スレッド扱いにし、再試行する（`AttachThreadInput` による定番の回避策）。
unsafe fn ensure_foreground(hwnd: HWND) -> bool {
    unsafe {
        if is_foreground_thread() {
            return true;
        }
        let _ = SetForegroundWindow(hwnd);
        if is_foreground_thread() {
            return true;
        }

        let fg = GetForegroundWindow();
        let fg_tid = GetWindowThreadProcessId(fg, None);
        let cur_tid = GetCurrentThreadId();
        if fg_tid != 0 && fg_tid != cur_tid && AttachThreadInput(cur_tid, fg_tid, true).as_bool() {
            let _ = SetForegroundWindow(hwnd);
            let _ = AttachThreadInput(cur_tid, fg_tid, false);
        }
        is_foreground_thread()
    }
}

/// Start an OLE drag-and-drop operation with the given file paths.
///
/// `hwnd` は自メインウィンドウ（`window_hwnd()`）。`None` ならフォアグラウンド化をスキップする。
/// Returns `Ok(true)` if the drop result was MOVE (caller should refresh source panel).
/// ドラッグが開始できなかったと判断した場合は診断メッセージを `Err` で返す。
pub fn start_drag(paths: &[PathBuf], hwnd: Option<HWND>) -> std::result::Result<bool, String> {
    if paths.is_empty() {
        return Ok(false);
    }

    unsafe {
        let hglobal = build_hdrop(paths).map_err(|e| format!("Drag failed: {e}"))?;

        // DoDragDrop はマウスをキャプチャするが、キャプチャできるのはフォアグラウンド
        // ウィンドウのみ（MSDN: "Only the foreground window can capture the mouse"）。
        // フォーカス追従（前面に移動しない）設定では、ドラッグ開始時に自ウィンドウが
        // フォアグラウンドでないことがあり、その場合キャプチャに失敗してドラッグできない。
        // ドラッグは明示操作なので、ここで自ウィンドウをフォアグラウンド化してから開始する。
        let foreground_ok = match hwnd {
            Some(hwnd) => ensure_foreground(hwnd),
            None => is_foreground_thread(),
        };

        let data_obj: IDataObject = FileDataObject { hglobal }.into();
        let drop_source: IDropSource = DropSource.into();

        let mut effect = DROPEFFECT_NONE;
        let started = Instant::now();
        let hr = DoDragDrop(
            &data_obj,
            &drop_source,
            DROPEFFECT_COPY | DROPEFFECT_MOVE,
            &mut effect,
        );
        let elapsed = started.elapsed();

        // HGLOBAL is freed by FileDataObject::drop when data_obj goes out of scope
        if hr == DRAGDROP_S_DROP && effect != DROPEFFECT_NONE {
            return Ok(effect == DROPEFFECT_MOVE);
        }
        if elapsed < INSTANT_RETURN {
            return Err(format!(
                "Drag did not start (hr={:#x}, {}ms, foreground={}{}) - try again",
                hr.0,
                elapsed.as_millis(),
                foreground_ok,
                if hwnd.is_none() { ", no hwnd" } else { "" },
            ));
        }
        Ok(false)
    }
}
