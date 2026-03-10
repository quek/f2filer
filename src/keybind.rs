use std::collections::HashMap;

use eframe::egui;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Action – every configurable main-panel action
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    // Navigation
    SwitchPanel,
    CursorDown,
    CursorUp,
    CursorToTop,
    CursorToBottom,
    PageUp,
    PageDown,
    ToggleSelect,
    ToggleSelectUp,
    ToggleSelectAll,
    HistoryBack,
    HistoryForward,
    HistoryList,
    // File operations
    Open,
    OpenTextEditor,
    ParentDir,
    Copy,
    Move,
    Delete,
    DeletePermanent,
    OpenRecycleBin,
    FileProperties,
    ContextMenu,
    ZipCompress,
    Decompress,
    // Edit
    Rename,
    NewDirectory,
    // Misc
    Refresh,
    Quit,
    ToggleHidden,
    TogglePreview,
    SyncOppositePanel,
    CopyPathClipboard,
    ShowHelp,
    FocusFilter,
    RecursiveSearch,
    ExitRecursiveSearch,
    DriveSelect,
    RegisteredDirs,
    RegisterDir,
    Undo,
    Redo,
    FontSizeUp,
    FontSizeDown,
    Settings,
    CommandMode,
    // Sort
    SortMode,
    SortByName,
    SortByExtension,
    SortBySize,
    SortByDate,
}

/// Display order used for help text and settings UI.
pub const ACTION_DISPLAY_ORDER: &[Action] = &[
    Action::CursorDown,
    Action::CursorUp,
    Action::CursorToTop,
    Action::CursorToBottom,
    Action::PageUp,
    Action::PageDown,
    Action::Open,
    Action::OpenTextEditor,
    Action::ParentDir,
    Action::HistoryBack,
    Action::HistoryForward,
    Action::HistoryList,
    Action::SwitchPanel,
    Action::ToggleSelect,
    Action::ToggleSelectUp,
    Action::ToggleSelectAll,
    Action::FocusFilter,
    Action::RecursiveSearch,
    Action::ExitRecursiveSearch,
    Action::SortMode,
    Action::SyncOppositePanel,
    Action::Copy,
    Action::Move,
    Action::Delete,
    Action::DeletePermanent,
    Action::OpenRecycleBin,
    Action::Rename,
    Action::NewDirectory,
    Action::CopyPathClipboard,
    Action::ContextMenu,
    Action::FileProperties,
    Action::DriveSelect,
    Action::RegisteredDirs,
    Action::RegisterDir,
    Action::ZipCompress,
    Action::Decompress,
    Action::Undo,
    Action::Redo,
    Action::TogglePreview,
    Action::Refresh,
    Action::ToggleHidden,
    Action::CommandMode,
    Action::FontSizeUp,
    Action::FontSizeDown,
    Action::Settings,
    Action::Quit,
    Action::ShowHelp,
    // Sort sub-keys (not shown in main help, but configurable)
    Action::SortByName,
    Action::SortByExtension,
    Action::SortBySize,
    Action::SortByDate,
];

impl Action {
    pub fn description(&self) -> &'static str {
        match self {
            Action::SwitchPanel => "Switch panel",
            Action::CursorDown => "Cursor down",
            Action::CursorUp => "Cursor up",
            Action::CursorToTop => "Jump to top",
            Action::CursorToBottom => "Jump to bottom",
            Action::PageUp => "Page up",
            Action::PageDown => "Page down",
            Action::ToggleSelect => "Toggle select",
            Action::ToggleSelectUp => "Toggle select (move up)",
            Action::ToggleSelectAll => "Toggle select all / deselect",
            Action::Open => "Open dir / Execute file",
            Action::OpenTextEditor => "Open with text editor",
            Action::ParentDir => "Parent directory",
            Action::HistoryBack => "History back",
            Action::HistoryForward => "History forward",
            Action::HistoryList => "History list",
            Action::Copy => "Copy selected → opposite",
            Action::Move => "Move selected → opposite",
            Action::Delete => "Delete selected (trash)",
            Action::DeletePermanent => "Permanent delete (no undo)",
            Action::OpenRecycleBin => "Open recycle bin",
            Action::FileProperties => "File properties",
            Action::ContextMenu => "Context menu",
            Action::ZipCompress => "Zip compress selected",
            Action::Decompress => "Extract archive at cursor",
            Action::Rename => "Rename",
            Action::NewDirectory => "New directory",
            Action::Refresh => "Refresh",
            Action::Quit => "Quit",
            Action::ToggleHidden => "Toggle hidden files",
            Action::TogglePreview => "Preview (text/image/audio/video)",
            Action::SyncOppositePanel => "Sync opposite panel",
            Action::CopyPathClipboard => "Copy file path to clipboard",
            Action::ShowHelp => "This help",
            Action::FocusFilter => "Focus filter",
            Action::RecursiveSearch => "Recursive search (subdirectories)",
            Action::ExitRecursiveSearch => "Exit recursive search",
            Action::DriveSelect => "Drive select",
            Action::RegisteredDirs => "Registered directories",
            Action::RegisterDir => "Register current directory",
            Action::Undo => "Undo last operation",
            Action::Redo => "Redo",
            Action::FontSizeUp => "Font size up",
            Action::FontSizeDown => "Font size down",
            Action::Settings => "Settings",
            Action::CommandMode => "Command mode",
            Action::SortMode => "Sort mode (then n/e/s/d)",
            Action::SortByName => "Sort by name",
            Action::SortByExtension => "Sort by extension",
            Action::SortBySize => "Sort by size",
            Action::SortByDate => "Sort by date",
        }
    }
}

// ---------------------------------------------------------------------------
// KeyBinding – a single key + modifier combination
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct KeyBinding {
    pub key: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub ctrl: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub shift: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub alt: bool,
}

fn is_false(v: &bool) -> bool {
    !v
}

enum KeyKind {
    EguiKey(egui::Key),
    Text(String),
}

impl KeyBinding {
    fn parse_key(&self) -> Option<KeyKind> {
        if let Some(ch) = self.key.strip_prefix("text:") {
            Some(KeyKind::Text(ch.to_string()))
        } else {
            parse_egui_key(&self.key).map(KeyKind::EguiKey)
        }
    }

    pub fn is_pressed(&self, input: &egui::InputState) -> bool {
        match self.parse_key() {
            Some(KeyKind::EguiKey(key)) => {
                input.key_pressed(key)
                    && self.ctrl == input.modifiers.ctrl
                    && self.shift == input.modifiers.shift
                    && self.alt == input.modifiers.alt
            }
            Some(KeyKind::Text(t)) => input
                .events
                .iter()
                .any(|e| matches!(e, egui::Event::Text(s) if s == &t)),
            None => false,
        }
    }

    /// Human-readable display (e.g. "Ctrl+Shift+D", "?", "Enter").
    pub fn display(&self) -> String {
        let mut parts: Vec<&str> = Vec::new();
        if self.ctrl {
            parts.push("Ctrl");
        }
        if self.alt {
            parts.push("Alt");
        }
        if self.shift {
            parts.push("Shift");
        }
        let key_display = if let Some(ch) = self.key.strip_prefix("text:") {
            ch.to_string()
        } else {
            capitalize_key_name(&self.key)
        };
        let joined = if parts.is_empty() {
            key_display
        } else {
            format!("{}+{}", parts.join("+"), key_display)
        };
        joined
    }

    /// Create a KeyBinding from an egui key event.
    pub fn from_key_event(key: egui::Key, modifiers: &egui::Modifiers) -> Self {
        KeyBinding {
            key: egui_key_to_string(key),
            ctrl: modifiers.ctrl,
            shift: modifiers.shift,
            alt: modifiers.alt,
        }
    }
}

fn capitalize_key_name(s: &str) -> String {
    match s {
        "arrow_up" | "up" => "↑".to_string(),
        "arrow_down" | "down" => "↓".to_string(),
        "arrow_left" | "left" => "←".to_string(),
        "arrow_right" | "right" => "→".to_string(),
        "page_up" | "pageup" => "PgUp".to_string(),
        "page_down" | "pagedown" => "PgDn".to_string(),
        "space" => "Space".to_string(),
        "enter" => "Enter".to_string(),
        "escape" | "esc" => "Esc".to_string(),
        "tab" => "Tab".to_string(),
        "insert" => "Ins".to_string(),
        "delete" => "Del".to_string(),
        "home" => "Home".to_string(),
        "end" => "End".to_string(),
        "backslash" => "\\".to_string(),
        "comma" => ",".to_string(),
        "period" => ".".to_string(),
        "minus" => "-".to_string(),
        other => {
            let mut c = other.chars();
            match c.next() {
                None => String::new(),
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// KeyBindings – the full action → keys map
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyBindings {
    #[serde(flatten)]
    pub bindings: HashMap<Action, Vec<KeyBinding>>,
}

impl KeyBindings {
    /// Check if any binding for the given action is pressed.
    pub fn is_action_pressed(&self, action: Action, input: &egui::InputState) -> bool {
        self.bindings
            .get(&action)
            .map_or(false, |bs| bs.iter().any(|b| b.is_pressed(input)))
    }

    /// Generate help text from current bindings.
    pub fn help_text(&self) -> String {
        let mut lines = Vec::new();
        for &action in ACTION_DISPLAY_ORDER {
            // Skip sort sub-keys from main help
            if matches!(
                action,
                Action::SortByName
                    | Action::SortByExtension
                    | Action::SortBySize
                    | Action::SortByDate
            ) {
                continue;
            }
            if let Some(bindings) = self.bindings.get(&action) {
                let keys: Vec<String> = bindings.iter().map(|b| b.display()).collect();
                let keys_str = keys.join(" / ");
                lines.push(format!("{:<15}:  {}", keys_str, action.description()));
            }
        }
        // Hardcoded Ctrl+C/V (not configurable)
        lines.push(format!("{:<15}:  {}", "Ctrl+C", "Copy selected to clipboard"));
        lines.push(format!("{:<15}:  {}", "Ctrl+V", "Paste from clipboard"));
        lines.join("\n")
    }

    /// Default keybindings matching the original hardcoded keys.
    pub fn defaults() -> Self {
        let mut b = HashMap::new();

        // Navigation
        b.insert(
            Action::SwitchPanel,
            vec![kb("i", false, false, false)],
        );
        b.insert(
            Action::CursorDown,
            vec![
                kb("j", false, false, false),
                kb("down", false, false, false),
            ],
        );
        b.insert(
            Action::CursorUp,
            vec![kb("k", false, false, false), kb("up", false, false, false)],
        );
        b.insert(
            Action::CursorToTop,
            vec![
                kb("home", false, false, false),
                kb("k", false, false, true), // Alt+K
            ],
        );
        b.insert(
            Action::CursorToBottom,
            vec![
                kb("end", false, false, false),
                kb("j", false, false, true), // Alt+J
            ],
        );
        b.insert(
            Action::PageUp,
            vec![kb("page_up", false, false, false)],
        );
        b.insert(
            Action::PageDown,
            vec![kb("page_down", false, false, false)],
        );
        b.insert(
            Action::ToggleSelect,
            vec![
                kb("space", false, false, false),
                kb("insert", false, false, false),
            ],
        );
        b.insert(
            Action::ToggleSelectUp,
            vec![kb("space", false, true, false)],
        );
        b.insert(
            Action::ToggleSelectAll,
            vec![kb("a", false, false, false)],
        );

        // File operations
        b.insert(
            Action::Open,
            vec![
                kb("l", false, false, false),
                kb("enter", false, false, false),
            ],
        );
        b.insert(
            Action::OpenTextEditor,
            vec![kb("e", false, false, false)],
        );
        b.insert(Action::ParentDir, vec![kb("h", false, false, false)]);
        b.insert(
            Action::HistoryBack,
            vec![
                kb("left", false, false, true),  // Alt+Left
                kb("h", false, false, true),      // Alt+H
            ],
        );
        b.insert(
            Action::HistoryForward,
            vec![
                kb("right", false, false, true), // Alt+Right
                kb("l", false, false, true),      // Alt+L
            ],
        );
        b.insert(
            Action::HistoryList,
            vec![kb("h", true, false, false)],    // Ctrl+H
        );
        b.insert(Action::Copy, vec![kb("c", false, false, false)]);
        b.insert(Action::Move, vec![kb("m", false, false, false)]);
        b.insert(Action::Delete, vec![kb("d", false, false, false)]);
        b.insert(
            Action::DeletePermanent,
            vec![kb("d", false, true, false)], // Shift+D
        );
        b.insert(
            Action::OpenRecycleBin,
            vec![kb("x", false, true, false)], // Shift+X
        );
        b.insert(
            Action::FileProperties,
            vec![kb("enter", false, false, true)], // Alt+Enter
        );
        b.insert(
            Action::ContextMenu,
            vec![kb("backslash", false, false, false)],
        );
        b.insert(
            Action::ZipCompress,
            vec![kb("u", false, true, false)], // Shift+U
        );
        b.insert(Action::Decompress, vec![kb("u", false, false, false)]);

        // Edit
        b.insert(Action::Rename, vec![kb("r", false, false, false)]);
        b.insert(
            Action::NewDirectory,
            vec![kb("n", false, false, false)],
        );

        // Misc
        b.insert(
            Action::Refresh,
            vec![kb("r", true, false, false)], // Ctrl+R
        );
        b.insert(
            Action::Quit,
            vec![
                kb("q", false, false, false),
                kb("q", true, false, false), // Ctrl+Q
            ],
        );
        b.insert(
            Action::ToggleHidden,
            vec![kb("period", false, false, false)],
        );
        b.insert(
            Action::TogglePreview,
            vec![kb("v", false, false, false)],
        );
        b.insert(
            Action::SyncOppositePanel,
            vec![kb("o", false, false, false)],
        );
        b.insert(
            Action::CopyPathClipboard,
            vec![kb("y", false, false, false)],
        );
        b.insert(Action::ShowHelp, vec![kb_text("?")]);
        b.insert(
            Action::FocusFilter,
            vec![kb("f", false, false, false)],
        );
        b.insert(
            Action::RecursiveSearch,
            vec![kb("f", false, false, true)], // Alt+F
        );
        b.insert(
            Action::ExitRecursiveSearch,
            vec![kb("escape", false, false, false)],
        );
        b.insert(Action::DriveSelect, vec![kb("p", false, false, false)]);
        b.insert(
            Action::RegisteredDirs,
            vec![kb("g", false, false, false)],
        );
        b.insert(
            Action::RegisterDir,
            vec![kb("g", false, true, false)], // Shift+G
        );
        b.insert(Action::Undo, vec![kb("z", false, false, false)]);
        b.insert(
            Action::Redo,
            vec![kb("z", false, true, false)], // Shift+Z
        );
        b.insert(Action::FontSizeUp, vec![kb_text("+")]);
        b.insert(
            Action::FontSizeDown,
            vec![kb("minus", false, false, false)],
        );
        b.insert(
            Action::Settings,
            vec![kb("comma", true, false, false)], // Ctrl+,
        );
        b.insert(Action::CommandMode, vec![kb_text(":")]);

        // Sort
        b.insert(Action::SortMode, vec![kb("s", false, false, false)]);
        b.insert(
            Action::SortByName,
            vec![kb("n", false, false, false)],
        );
        b.insert(
            Action::SortByExtension,
            vec![kb("e", false, false, false)],
        );
        b.insert(
            Action::SortBySize,
            vec![kb("s", false, false, false)],
        );
        b.insert(
            Action::SortByDate,
            vec![kb("d", false, false, false)],
        );

        KeyBindings { bindings: b }
    }

    /// Merge user overrides on top of defaults.
    pub fn merge_with_defaults(overrides: &HashMap<Action, Vec<KeyBinding>>) -> Self {
        let mut result = Self::defaults();
        for (action, bindings) in overrides {
            result.bindings.insert(*action, bindings.clone());
        }
        result
    }

    /// Check if an action's bindings differ from defaults.
    pub fn is_customized(&self, action: Action) -> bool {
        let defaults = Self::defaults();
        self.bindings.get(&action) != defaults.bindings.get(&action)
    }
}

fn kb(key: &str, ctrl: bool, shift: bool, alt: bool) -> KeyBinding {
    KeyBinding {
        key: key.to_string(),
        ctrl,
        shift,
        alt,
    }
}

fn kb_text(text: &str) -> KeyBinding {
    KeyBinding {
        key: format!("text:{}", text),
        ctrl: false,
        shift: false,
        alt: false,
    }
}

// ---------------------------------------------------------------------------
// egui::Key ↔ string conversion
// ---------------------------------------------------------------------------

fn parse_egui_key(s: &str) -> Option<egui::Key> {
    match s.to_lowercase().as_str() {
        "a" => Some(egui::Key::A),
        "b" => Some(egui::Key::B),
        "c" => Some(egui::Key::C),
        "d" => Some(egui::Key::D),
        "e" => Some(egui::Key::E),
        "f" => Some(egui::Key::F),
        "g" => Some(egui::Key::G),
        "h" => Some(egui::Key::H),
        "i" => Some(egui::Key::I),
        "j" => Some(egui::Key::J),
        "k" => Some(egui::Key::K),
        "l" => Some(egui::Key::L),
        "m" => Some(egui::Key::M),
        "n" => Some(egui::Key::N),
        "o" => Some(egui::Key::O),
        "p" => Some(egui::Key::P),
        "q" => Some(egui::Key::Q),
        "r" => Some(egui::Key::R),
        "s" => Some(egui::Key::S),
        "t" => Some(egui::Key::T),
        "u" => Some(egui::Key::U),
        "v" => Some(egui::Key::V),
        "w" => Some(egui::Key::W),
        "x" => Some(egui::Key::X),
        "y" => Some(egui::Key::Y),
        "z" => Some(egui::Key::Z),
        "enter" => Some(egui::Key::Enter),
        "space" => Some(egui::Key::Space),
        "escape" | "esc" => Some(egui::Key::Escape),
        "tab" => Some(egui::Key::Tab),
        "arrow_up" | "up" => Some(egui::Key::ArrowUp),
        "arrow_down" | "down" => Some(egui::Key::ArrowDown),
        "arrow_left" | "left" => Some(egui::Key::ArrowLeft),
        "arrow_right" | "right" => Some(egui::Key::ArrowRight),
        "home" => Some(egui::Key::Home),
        "end" => Some(egui::Key::End),
        "page_up" | "pageup" => Some(egui::Key::PageUp),
        "page_down" | "pagedown" => Some(egui::Key::PageDown),
        "insert" => Some(egui::Key::Insert),
        "delete" => Some(egui::Key::Delete),
        "backslash" => Some(egui::Key::Backslash),
        "comma" => Some(egui::Key::Comma),
        "period" => Some(egui::Key::Period),
        "minus" => Some(egui::Key::Minus),
        "backspace" => Some(egui::Key::Backspace),
        "f1" => Some(egui::Key::F1),
        "f2" => Some(egui::Key::F2),
        "f3" => Some(egui::Key::F3),
        "f4" => Some(egui::Key::F4),
        "f5" => Some(egui::Key::F5),
        "f6" => Some(egui::Key::F6),
        "f7" => Some(egui::Key::F7),
        "f8" => Some(egui::Key::F8),
        "f9" => Some(egui::Key::F9),
        "f10" => Some(egui::Key::F10),
        "f11" => Some(egui::Key::F11),
        "f12" => Some(egui::Key::F12),
        _ => None,
    }
}

fn egui_key_to_string(key: egui::Key) -> String {
    match key {
        egui::Key::A => "a",
        egui::Key::B => "b",
        egui::Key::C => "c",
        egui::Key::D => "d",
        egui::Key::E => "e",
        egui::Key::F => "f",
        egui::Key::G => "g",
        egui::Key::H => "h",
        egui::Key::I => "i",
        egui::Key::J => "j",
        egui::Key::K => "k",
        egui::Key::L => "l",
        egui::Key::M => "m",
        egui::Key::N => "n",
        egui::Key::O => "o",
        egui::Key::P => "p",
        egui::Key::Q => "q",
        egui::Key::R => "r",
        egui::Key::S => "s",
        egui::Key::T => "t",
        egui::Key::U => "u",
        egui::Key::V => "v",
        egui::Key::W => "w",
        egui::Key::X => "x",
        egui::Key::Y => "y",
        egui::Key::Z => "z",
        egui::Key::Enter => "enter",
        egui::Key::Space => "space",
        egui::Key::Escape => "escape",
        egui::Key::Tab => "tab",
        egui::Key::ArrowUp => "up",
        egui::Key::ArrowDown => "down",
        egui::Key::ArrowLeft => "left",
        egui::Key::ArrowRight => "right",
        egui::Key::Home => "home",
        egui::Key::End => "end",
        egui::Key::PageUp => "page_up",
        egui::Key::PageDown => "page_down",
        egui::Key::Insert => "insert",
        egui::Key::Delete => "delete",
        egui::Key::Backslash => "backslash",
        egui::Key::Comma => "comma",
        egui::Key::Period => "period",
        egui::Key::Minus => "minus",
        egui::Key::Backspace => "backspace",
        egui::Key::F1 => "f1",
        egui::Key::F2 => "f2",
        egui::Key::F3 => "f3",
        egui::Key::F4 => "f4",
        egui::Key::F5 => "f5",
        egui::Key::F6 => "f6",
        egui::Key::F7 => "f7",
        egui::Key::F8 => "f8",
        egui::Key::F9 => "f9",
        egui::Key::F10 => "f10",
        egui::Key::F11 => "f11",
        egui::Key::F12 => "f12",
        _ => "unknown",
    }
    .to_string()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_cover_all_display_order_actions() {
        let defaults = KeyBindings::defaults();
        for action in ACTION_DISPLAY_ORDER {
            assert!(
                defaults.bindings.contains_key(action),
                "Action {:?} missing from defaults",
                action
            );
        }
    }

    #[test]
    fn keybinding_display_plain() {
        let kb = KeyBinding {
            key: "j".to_string(),
            ctrl: false,
            shift: false,
            alt: false,
        };
        assert_eq!(kb.display(), "J");
    }

    #[test]
    fn keybinding_display_modifiers() {
        let kb = KeyBinding {
            key: "d".to_string(),
            ctrl: false,
            shift: true,
            alt: false,
        };
        assert_eq!(kb.display(), "Shift+D");
    }

    #[test]
    fn keybinding_display_text() {
        let kb = KeyBinding {
            key: "text:?".to_string(),
            ctrl: false,
            shift: false,
            alt: false,
        };
        assert_eq!(kb.display(), "?");
    }

    #[test]
    fn keybinding_display_ctrl_combo() {
        let kb = KeyBinding {
            key: "comma".to_string(),
            ctrl: true,
            shift: false,
            alt: false,
        };
        assert_eq!(kb.display(), "Ctrl+,");
    }

    #[test]
    fn merge_with_defaults_overrides_action() {
        let mut overrides = HashMap::new();
        overrides.insert(
            Action::CursorDown,
            vec![KeyBinding {
                key: "n".to_string(),
                ctrl: false,
                shift: false,
                alt: false,
            }],
        );
        let merged = KeyBindings::merge_with_defaults(&overrides);
        let bindings = merged.bindings.get(&Action::CursorDown).unwrap();
        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0].key, "n");
        // Other actions should still have defaults
        assert!(merged.bindings.contains_key(&Action::CursorUp));
    }

    #[test]
    fn is_customized_detects_change() {
        let mut overrides = HashMap::new();
        overrides.insert(
            Action::Open,
            vec![KeyBinding {
                key: "space".to_string(),
                ctrl: false,
                shift: false,
                alt: false,
            }],
        );
        let merged = KeyBindings::merge_with_defaults(&overrides);
        assert!(merged.is_customized(Action::Open));
        assert!(!merged.is_customized(Action::CursorDown));
    }

    #[test]
    fn parse_and_serialize_roundtrip() {
        let kb = KeyBinding {
            key: "d".to_string(),
            ctrl: false,
            shift: true,
            alt: false,
        };
        let json = serde_json::to_string(&kb).unwrap();
        let parsed: KeyBinding = serde_json::from_str(&json).unwrap();
        assert_eq!(kb, parsed);
    }

    #[test]
    fn action_serialize_snake_case() {
        let json = serde_json::to_string(&Action::CursorDown).unwrap();
        assert_eq!(json, "\"cursor_down\"");
    }

    #[test]
    fn egui_key_roundtrip() {
        let key = egui::Key::Enter;
        let s = egui_key_to_string(key);
        let parsed = parse_egui_key(&s).unwrap();
        assert_eq!(key, parsed);
    }

    #[test]
    fn help_text_not_empty() {
        let kb = KeyBindings::defaults();
        let text = kb.help_text();
        assert!(text.contains("Cursor down"));
        assert!(text.contains("Ctrl+C"));
    }

    #[test]
    fn from_key_event_creates_binding() {
        let modifiers = egui::Modifiers {
            alt: true,
            ctrl: false,
            shift: false,
            mac_cmd: false,
            command: false,
        };
        let kb = KeyBinding::from_key_event(egui::Key::F, &modifiers);
        assert_eq!(kb.key, "f");
        assert!(kb.alt);
        assert!(!kb.ctrl);
        assert!(!kb.shift);
    }
}
