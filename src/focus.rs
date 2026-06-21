//! SetWinEventHook によるフォアグラウンドウィンドウ変更のイベント駆動検出。
//!
//! 外部プロセス（AutoHotkey等）が SetForegroundWindow を呼ぶと、OS上はフォアグラウンドに
//! なるが winit/egui が視覚的なZ順序を更新しない場合がある。
//! EVENT_SYSTEM_FOREGROUND フックで自プロセスがフォアグラウンドになった瞬間を検出し、
//! AtomicBool フラグをセット。メインループの update() でフラグを確認してZ順序を更新する。
//!
//! ただし「マウスホバーでフォーカス（アクティブウィンドウ追跡）かつ前面に移動しない」設定
//! （SPI_GETACTIVEWNDTRKZORDER = FALSE）の場合は Z順序を持ち上げない。
//! 判定は `should_raise_on_foreground()` で行う。

use core::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};

use windows::Win32::Foundation::HWND;
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::Accessibility::SetWinEventHook;
use windows::Win32::Graphics::Gdi::InvalidateRect;
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowThreadProcessId, SystemParametersInfoW,
    EVENT_SYSTEM_FOREGROUND, SPI_GETACTIVEWINDOWTRACKING, SPI_GETACTIVEWNDTRKZORDER,
    SYSTEM_PARAMETERS_INFO_ACTION, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, WINEVENT_OUTOFCONTEXT,
};

/// 自プロセスがフォアグラウンドになったときにコールバックがセットするフラグ。
static FOREGROUND_GAINED: AtomicBool = AtomicBool::new(false);

/// 自プロセスのメインウィンドウ HWND（フォアグラウンド化時にコールバックが記録）。
/// `0` は未記録。OLE ドラッグ開始時の `SetForegroundWindow` 用にハンドルを保持する。
static MAIN_HWND: AtomicIsize = AtomicIsize::new(0);

/// フラグを読み取ってクリアする。前回呼び出し以降にフォアグラウンドになっていれば true。
pub fn take_foreground_flag() -> bool {
    FOREGROUND_GAINED.swap(false, Ordering::Relaxed)
}

/// 記録済みの自プロセスのメインウィンドウハンドルを返す。未記録なら `None`。
pub fn main_hwnd() -> Option<HWND> {
    let raw = MAIN_HWND.load(Ordering::Relaxed);
    if raw == 0 {
        None
    } else {
        Some(HWND(raw as *mut c_void))
    }
}

/// EVENT_SYSTEM_FOREGROUND の SetWinEventHook を登録する。
/// UIスレッドから呼ぶこと。コールバックはメッセージディスパッチ中に同一スレッドで実行される。
pub fn install_foreground_hook() {
    unsafe {
        SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_FOREGROUND,
            None,
            Some(on_foreground_change),
            0,
            0,
            WINEVENT_OUTOFCONTEXT,
        );
    }
}

/// `SystemParametersInfoW` で BOOL 系のシステム設定を取得する。
/// 取得失敗時は `None`。
fn get_spi_bool(action: SYSTEM_PARAMETERS_INFO_ACTION) -> Option<bool> {
    let mut value: i32 = 0;
    let result = unsafe {
        SystemParametersInfoW(
            action,
            0,
            Some(&mut value as *mut i32 as *mut c_void),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    };
    result.ok().map(|()| value != 0)
}

/// 自プロセスがフォアグラウンドになったとき、ウィンドウを Z 順序の最前面へ
/// 持ち上げるべきかをシステム設定から判定する。
///
/// - アクティブウィンドウ追跡（マウスホバーでフォーカス）が無効、または設定取得に
///   失敗した場合: フォアグラウンド取得はクリック等の明示操作なので `true`（従来動作）。
/// - 追跡が有効な場合: `SPI_GETACTIVEWNDTRKZORDER`（「ウィンドウを前面に移動」設定）を
///   尊重する。ユーザーが「前面に移動しない」を選んでいれば `false` を返し、持ち上げを抑止する。
pub fn should_raise_on_foreground() -> bool {
    // 追跡が無効、または取得失敗 → 従来どおり持ち上げる
    if get_spi_bool(SPI_GETACTIVEWINDOWTRACKING) != Some(true) {
        return true;
    }
    // 追跡が有効 → Z 順序設定を尊重（取得失敗時は安全側で持ち上げる）
    get_spi_bool(SPI_GETACTIVEWNDTRKZORDER).unwrap_or(true)
}

unsafe extern "system" fn on_foreground_change(
    _h_win_event_hook: windows::Win32::UI::Accessibility::HWINEVENTHOOK,
    _event: u32,
    hwnd: HWND,
    _id_object: i32,
    _id_child: i32,
    _id_event_thread: u32,
    _dwms_event_time: u32,
) {
    unsafe {
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == GetCurrentProcessId() {
            MAIN_HWND.store(hwnd.0 as isize, Ordering::Relaxed);
            FOREGROUND_GAINED.store(true, Ordering::Relaxed);
            // eframe の update() を起動してフラグを処理させるためリペイントを要求
            let _ = InvalidateRect(Some(hwnd), None, false);
        }
    }
}
