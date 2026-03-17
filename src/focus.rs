//! SetWinEventHook によるフォアグラウンドウィンドウ変更のイベント駆動検出。
//!
//! 外部プロセス（AutoHotkey等）が SetForegroundWindow を呼ぶと、OS上はフォアグラウンドに
//! なるが winit/egui が視覚的なZ順序を更新しない場合がある。
//! EVENT_SYSTEM_FOREGROUND フックで自プロセスがフォアグラウンドになった瞬間を検出し、
//! AtomicBool フラグをセット。メインループの update() でフラグを確認してZ順序を強制更新する。

use std::sync::atomic::{AtomicBool, Ordering};

use windows::Win32::Foundation::HWND;
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::Accessibility::SetWinEventHook;
use windows::Win32::Graphics::Gdi::InvalidateRect;
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowThreadProcessId,
    EVENT_SYSTEM_FOREGROUND, WINEVENT_OUTOFCONTEXT,
};

/// 自プロセスがフォアグラウンドになったときにコールバックがセットするフラグ。
static FOREGROUND_GAINED: AtomicBool = AtomicBool::new(false);

/// フラグを読み取ってクリアする。前回呼び出し以降にフォアグラウンドになっていれば true。
pub fn take_foreground_flag() -> bool {
    FOREGROUND_GAINED.swap(false, Ordering::Relaxed)
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
            FOREGROUND_GAINED.store(true, Ordering::Relaxed);
            // eframe の update() を起動してフラグを処理させるためリペイントを要求
            let _ = InvalidateRect(Some(hwnd), None, false);
        }
    }
}
