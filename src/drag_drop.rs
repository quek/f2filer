#![allow(non_snake_case)]

use std::path::PathBuf;

use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::System::Com::*;
use windows::Win32::System::Memory::*;
use windows::Win32::System::Ole::*;
use windows::Win32::System::SystemServices::{MK_LBUTTON, MODIFIERKEYS_FLAGS};

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
                let _ = GlobalFree(self.hglobal);
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
        _padvsink: Option<&IAdviseSink>,
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

/// Start an OLE drag-and-drop operation with the given file paths.
/// Returns `true` if the drop result was MOVE (caller should refresh source panel).
pub fn start_drag(paths: &[PathBuf]) -> bool {
    if paths.is_empty() {
        return false;
    }

    unsafe {
        let hglobal = match build_hdrop(paths) {
            Ok(h) => h,
            Err(_) => return false,
        };

        let data_obj: IDataObject = FileDataObject { hglobal }.into();
        let drop_source: IDropSource = DropSource.into();

        let mut effect = DROPEFFECT_NONE;
        let hr = DoDragDrop(
            &data_obj,
            &drop_source,
            DROPEFFECT_COPY | DROPEFFECT_MOVE,
            &mut effect,
        );

        // HGLOBAL is freed by FileDataObject::drop when data_obj goes out of scope
        hr == DRAGDROP_S_DROP && effect == DROPEFFECT_MOVE
    }
}
