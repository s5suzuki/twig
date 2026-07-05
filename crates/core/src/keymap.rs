use std::collections::BTreeMap;

macro_rules! define_keys {
    ($(($v:ident, $name:literal $(, $alias:literal)*)),* $(,)?) => {
        #[derive(Clone, Copy, PartialEq, Eq, Debug)]
        pub enum Key {
            $($v,)*
        }

        impl Key {
            pub const ALL: &'static [Key] = &[$(Key::$v,)*];

            pub fn from_name(s: &str) -> Option<Key> {
                match s {
                    $($name $(| $alias)* => Some(Key::$v),)*
                    _ => None,
                }
            }

            pub fn name(self) -> &'static str {
                match self {
                    $(Key::$v => $name,)*
                }
            }
        }
    };
}

define_keys![
    (ArrowDown, "Down", "ArrowDown", "⏷"),
    (ArrowLeft, "Left", "ArrowLeft", "⏴"),
    (ArrowRight, "Right", "ArrowRight", "⏵"),
    (ArrowUp, "Up", "ArrowUp", "⏶"),
    (Escape, "Escape", "Esc"),
    (Tab, "Tab"),
    (Backspace, "Backspace"),
    (Enter, "Enter", "Return"),
    (Space, "Space", " "),
    (Insert, "Insert", "Help"),
    (Delete, "Delete"),
    (Home, "Home"),
    (End, "End"),
    (PageUp, "PageUp"),
    (PageDown, "PageDown"),
    (Colon, "Colon", ":"),
    (Comma, "Comma", ","),
    (Backslash, "Backslash", "\\"),
    (Slash, "Slash", "/"),
    (Pipe, "Pipe", "|"),
    (Questionmark, "Questionmark", "?"),
    (Exclamationmark, "Exclamationmark", "!"),
    (OpenBracket, "OpenBracket", "["),
    (CloseBracket, "CloseBracket", "]"),
    (OpenCurlyBracket, "OpenCurlyBracket", "{"),
    (CloseCurlyBracket, "CloseCurlyBracket", "}"),
    (Backtick, "Backtick", "`", "Backquote", "Grave"),
    (Minus, "Minus", "-", "−"),
    (Period, "Period", "."),
    (Plus, "Plus", "+"),
    (Equals, "Equals", "=", "Equal", "NumpadEqual"),
    (Semicolon, "Semicolon", ";"),
    (Quote, "Quote", "'"),
    (Num0, "0", "Digit0", "Numpad0"),
    (Num1, "1", "Digit1", "Numpad1"),
    (Num2, "2", "Digit2", "Numpad2"),
    (Num3, "3", "Digit3", "Numpad3"),
    (Num4, "4", "Digit4", "Numpad4"),
    (Num5, "5", "Digit5", "Numpad5"),
    (Num6, "6", "Digit6", "Numpad6"),
    (Num7, "7", "Digit7", "Numpad7"),
    (Num8, "8", "Digit8", "Numpad8"),
    (Num9, "9", "Digit9", "Numpad9"),
    (A, "A", "a"),
    (B, "B", "b"),
    (C, "C", "c"),
    (D, "D", "d"),
    (E, "E", "e"),
    (F, "F", "f"),
    (G, "G", "g"),
    (H, "H", "h"),
    (I, "I", "i"),
    (J, "J", "j"),
    (K, "K", "k"),
    (L, "L", "l"),
    (M, "M", "m"),
    (N, "N", "n"),
    (O, "O", "o"),
    (P, "P", "p"),
    (Q, "Q", "q"),
    (R, "R", "r"),
    (S, "S", "s"),
    (T, "T", "t"),
    (U, "U", "u"),
    (V, "V", "v"),
    (W, "W", "w"),
    (X, "X", "x"),
    (Y, "Y", "y"),
    (Z, "Z", "z"),
    (F1, "F1"),
    (F2, "F2"),
    (F3, "F3"),
    (F4, "F4"),
    (F5, "F5"),
    (F6, "F6"),
    (F7, "F7"),
    (F8, "F8"),
    (F9, "F9"),
    (F10, "F10"),
    (F11, "F11"),
    (F12, "F12"),
];

impl Key {
    pub fn symbol_or_name(self) -> &'static str {
        match self {
            Key::ArrowDown => "⏷",
            Key::ArrowLeft => "⏴",
            Key::ArrowRight => "⏵",
            Key::ArrowUp => "⏶",
            Key::Colon => ":",
            Key::Comma => ",",
            Key::Minus => "−",
            Key::Period => ".",
            Key::Plus => "+",
            Key::Equals => "=",
            Key::Semicolon => ";",
            Key::Backslash => "\\",
            Key::Slash => "/",
            Key::Pipe => "|",
            Key::Questionmark => "?",
            Key::Exclamationmark => "!",
            Key::OpenBracket => "[",
            Key::CloseBracket => "]",
            Key::OpenCurlyBracket => "{",
            Key::CloseCurlyBracket => "}",
            Key::Backtick => "`",
            _ => self.name(),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Modifiers {
    pub alt: bool,
    pub ctrl: bool,
    pub shift: bool,
    pub command: bool,
}

impl Modifiers {
    pub const NONE: Self = Self {
        alt: false,
        ctrl: false,
        shift: false,
        command: false,
    };
    pub const ALT: Self = Self {
        alt: true,
        ..Self::NONE
    };
    pub const CTRL: Self = Self {
        ctrl: true,
        ..Self::NONE
    };
    pub const SHIFT: Self = Self {
        shift: true,
        ..Self::NONE
    };
    pub const COMMAND: Self = Self {
        command: true,
        ..Self::NONE
    };

    pub const fn plus(self, o: Self) -> Self {
        Self {
            alt: self.alt | o.alt,
            ctrl: self.ctrl | o.ctrl,
            shift: self.shift | o.shift,
            command: self.command | o.command,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Context {
    Global,
    Sidebar,
    Changes,
    Diff,
    Graph,
}

impl Context {
    fn index(self) -> usize {
        self as usize
    }

    fn from_name(s: &str) -> Option<Context> {
        Some(match s {
            "global" => Context::Global,
            "sidebar" => Context::Sidebar,
            "changes" => Context::Changes,
            "diff" => Context::Diff,
            "graph" => Context::Graph,
            _ => return None,
        })
    }

    pub fn title(self) -> &'static str {
        match self {
            Context::Global => "Global",
            Context::Sidebar => "Sidebar",
            Context::Changes => "Changes",
            Context::Diff => "Diff",
            Context::Graph => "Graph",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    FocusLeft,
    FocusRight,
    FocusUp,
    FocusDown,
    CycleTab,
    CycleTabFwd,
    CycleTabBack,
    ToggleShell,
    OpenSearch,
    NavBack,
    NavForward,

    DiffFind,
    DiffDown,
    DiffUp,
    DiffTop,
    DiffBottom,
    DiffNextHunk,
    DiffPrevHunk,
    DiffToggleVisual,
    DiffClearVisual,
    DiffStageSelection,
    DiffUnstageSelection,
    DiffDiscardSelection,
    DiffHalfPageDown,
    DiffHalfPageUp,
    DiffPageDown,
    DiffPageUp,
    DiffEditor,
    DiffCopySelection,

    ChangesTop,
    ChangesBottom,
    ChangesDown,
    ChangesUp,
    ChangesCollapse,
    ChangesExpand,
    ChangesActivate,
    ChangesStageToggle,
    ChangesEdit,
    ChangesDiscard,
    ChangesHalfPageDown,
    ChangesHalfPageUp,

    SidebarTop,
    SidebarBottom,
    SidebarDown,
    SidebarUp,
    SidebarSelect,
    SidebarExpand,
    SidebarCollapse,
    SidebarHalfPageDown,
    SidebarHalfPageUp,

    GraphDown,
    GraphUp,
    GraphTop,
    GraphBottom,
    GraphHalfPageDown,
    GraphHalfPageUp,
    GraphOpen,
    GraphEditor,
    GraphCollapse,
    GraphContextMenu,
    GraphReset,
    GraphCreateBranch,
    GraphCreateTag,
    GraphCherryPick,
    GraphRevert,
    GraphRebaseOnto,
    GraphRebaseInteractive,
    GraphCheckout,
    GraphPush,
    GraphForcePush,
    GraphFetch,
    GraphPull,
}

impl Action {
    const TABLE: &'static [(Action, &'static str, &'static str)] = &[
        (
            Action::FocusLeft,
            "focus-left",
            "Focus the pane on the left",
        ),
        (
            Action::FocusRight,
            "focus-right",
            "Focus the pane on the right",
        ),
        (Action::FocusUp, "focus-up", "Focus the pane above"),
        (
            Action::FocusDown,
            "focus-down",
            "Focus the pane below (terminal)",
        ),
        (Action::CycleTab, "cycle-tab", "Cycle the right-hand tab"),
        (Action::CycleTabFwd, "cycle-tab-fwd", "Next right-hand tab"),
        (
            Action::CycleTabBack,
            "cycle-tab-back",
            "Previous right-hand tab",
        ),
        (
            Action::ToggleShell,
            "toggle-shell",
            "Toggle the bottom terminal",
        ),
        (Action::OpenSearch, "open-search", "Open the Search tab"),
        (Action::NavBack, "nav-back", "Go back in navigation history"),
        (
            Action::NavForward,
            "nav-forward",
            "Go forward in navigation history",
        ),
        (
            Action::DiffFind,
            "diff-find",
            "Toggle the in-file find & replace bar",
        ),
        (Action::DiffDown, "diff-down", "Move cursor down one line"),
        (Action::DiffUp, "diff-up", "Move cursor up one line"),
        (Action::DiffTop, "diff-top", "Jump to the first line"),
        (Action::DiffBottom, "diff-bottom", "Jump to the last line"),
        (
            Action::DiffNextHunk,
            "diff-next-hunk",
            "Jump to the next hunk",
        ),
        (
            Action::DiffPrevHunk,
            "diff-prev-hunk",
            "Jump to the previous hunk",
        ),
        (
            Action::DiffToggleVisual,
            "diff-toggle-visual",
            "Toggle visual (line) selection",
        ),
        (
            Action::DiffClearVisual,
            "diff-clear-visual",
            "Clear the selection",
        ),
        (
            Action::DiffStageSelection,
            "diff-stage-selection",
            "Stage the selected lines",
        ),
        (
            Action::DiffUnstageSelection,
            "diff-unstage-selection",
            "Unstage the selected lines",
        ),
        (
            Action::DiffDiscardSelection,
            "diff-discard-selection",
            "Discard the selected lines from the working tree",
        ),
        (
            Action::DiffHalfPageDown,
            "diff-half-page-down",
            "Scroll half a page down",
        ),
        (
            Action::DiffHalfPageUp,
            "diff-half-page-up",
            "Scroll half a page up",
        ),
        (
            Action::DiffPageDown,
            "diff-page-down",
            "Scroll one page down",
        ),
        (Action::DiffPageUp, "diff-page-up", "Scroll one page up"),
        (
            Action::DiffEditor,
            "diff-editor",
            "Open the current file in the editor",
        ),
        (
            Action::DiffCopySelection,
            "diff-copy-selection",
            "Copy the selected lines to the clipboard",
        ),
        (Action::ChangesTop, "changes-top", "Move cursor to the top"),
        (
            Action::ChangesBottom,
            "changes-bottom",
            "Move cursor to the bottom",
        ),
        (Action::ChangesDown, "changes-down", "Move cursor down"),
        (Action::ChangesUp, "changes-up", "Move cursor up"),
        (
            Action::ChangesCollapse,
            "changes-collapse",
            "Collapse a folder/group, or step out",
        ),
        (
            Action::ChangesExpand,
            "changes-expand",
            "Expand a folder/group, or open a file",
        ),
        (
            Action::ChangesActivate,
            "changes-activate",
            "Open a file, or toggle a folder/group",
        ),
        (
            Action::ChangesStageToggle,
            "changes-stage-toggle",
            "Stage/unstage the item under the cursor",
        ),
        (
            Action::ChangesEdit,
            "changes-edit",
            "Open the file in the editor",
        ),
        (
            Action::ChangesDiscard,
            "changes-discard",
            "Discard changes to the file or folder",
        ),
        (
            Action::ChangesHalfPageDown,
            "changes-half-page-down",
            "Move cursor half a page down",
        ),
        (
            Action::ChangesHalfPageUp,
            "changes-half-page-up",
            "Move cursor half a page up",
        ),
        (Action::SidebarTop, "sidebar-top", "Move cursor to the top"),
        (
            Action::SidebarBottom,
            "sidebar-bottom",
            "Move cursor to the bottom",
        ),
        (Action::SidebarDown, "sidebar-down", "Move cursor down"),
        (Action::SidebarUp, "sidebar-up", "Move cursor up"),
        (
            Action::SidebarSelect,
            "sidebar-select",
            "Select the repository under the cursor",
        ),
        (
            Action::SidebarExpand,
            "sidebar-expand",
            "Expand a node, or select it",
        ),
        (
            Action::SidebarCollapse,
            "sidebar-collapse",
            "Collapse a node, or step out",
        ),
        (
            Action::SidebarHalfPageDown,
            "sidebar-half-page-down",
            "Move cursor half a page down",
        ),
        (
            Action::SidebarHalfPageUp,
            "sidebar-half-page-up",
            "Move cursor half a page up",
        ),
        (Action::GraphDown, "graph-down", "Move cursor down"),
        (Action::GraphUp, "graph-up", "Move cursor up"),
        (Action::GraphTop, "graph-top", "Jump to the newest commit"),
        (
            Action::GraphBottom,
            "graph-bottom",
            "Jump to the oldest commit",
        ),
        (
            Action::GraphHalfPageDown,
            "graph-half-page-down",
            "Move cursor half a page down",
        ),
        (
            Action::GraphHalfPageUp,
            "graph-half-page-up",
            "Move cursor half a page up",
        ),
        (
            Action::GraphOpen,
            "graph-open",
            "Open the commit / file under the cursor",
        ),
        (
            Action::GraphEditor,
            "graph-editor",
            "Open the file under the cursor in the editor",
        ),
        (
            Action::GraphCollapse,
            "graph-collapse",
            "Collapse the expanded commit",
        ),
        (
            Action::GraphContextMenu,
            "graph-context-menu",
            "Open the context menu",
        ),
        (
            Action::GraphReset,
            "graph-reset",
            "Reset the current branch to the commit",
        ),
        (
            Action::GraphCreateBranch,
            "graph-create-branch",
            "Create a branch at the commit",
        ),
        (
            Action::GraphCreateTag,
            "graph-create-tag",
            "Create a tag at the commit",
        ),
        (
            Action::GraphCherryPick,
            "graph-cherry-pick",
            "Cherry-pick the commit",
        ),
        (Action::GraphRevert, "graph-revert", "Revert the commit"),
        (
            Action::GraphRebaseOnto,
            "graph-rebase-onto",
            "Rebase the current branch onto the commit",
        ),
        (
            Action::GraphRebaseInteractive,
            "graph-rebase-interactive",
            "Interactively rebase onto the commit",
        ),
        (
            Action::GraphCheckout,
            "graph-checkout",
            "Check out the commit / branch",
        ),
        (Action::GraphPush, "graph-push", "Push the current branch"),
        (
            Action::GraphForcePush,
            "graph-force-push",
            "Force-push the current branch (with confirmation)",
        ),
        (Action::GraphFetch, "graph-fetch", "Fetch from the remote"),
        (Action::GraphPull, "graph-pull", "Pull the current branch"),
    ];

    fn from_name(s: &str) -> Option<Action> {
        Self::TABLE
            .iter()
            .find(|(_, name, _)| *name == s)
            .map(|(a, _, _)| *a)
    }

    fn describe(self) -> &'static str {
        Self::TABLE
            .iter()
            .find(|(a, _, _)| *a == self)
            .map(|(_, _, d)| *d)
            .unwrap_or("")
    }

    fn order(self) -> usize {
        Self::TABLE
            .iter()
            .position(|(a, _, _)| *a == self)
            .unwrap_or(usize::MAX)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Chord {
    pub mods: Modifiers,
    pub key: Key,
}

impl Chord {
    pub const fn new(mods: Modifiers, key: Key) -> Self {
        Chord { mods, key }
    }

    fn specificity(&self) -> u32 {
        let m = &self.mods;
        m.alt as u32 + m.shift as u32 + m.ctrl as u32 + m.command as u32
    }

    fn describe(&self) -> String {
        let mut s = String::new();
        let m = &self.mods;
        if m.ctrl {
            s.push_str("Ctrl+");
        }
        if m.command {
            s.push_str("Super+");
        }
        if m.alt {
            s.push_str("Alt+");
        }

        let name = self.key.symbol_or_name();
        let letter = name.len() == 1 && name.as_bytes()[0].is_ascii_alphabetic();
        let only_shift = m.shift && !m.ctrl && !m.alt && !m.command;
        if letter && only_shift {
            s.push_str(&name.to_ascii_uppercase());
        } else {
            if m.shift {
                s.push_str("Shift+");
            }
            if letter {
                s.push_str(&name.to_ascii_lowercase());
            } else {
                s.push_str(name);
            }
        }
        s
    }

    fn parse(s: &str) -> Option<Chord> {
        let mut mods = Modifiers::NONE;
        let mut key = None;
        for tok in s.split('+') {
            let tok = tok.trim();
            if tok.is_empty() {
                return None;
            }
            match tok.to_ascii_lowercase().as_str() {
                "alt" | "option" => mods.alt = true,
                "shift" => mods.shift = true,
                "ctrl" | "control" => mods.ctrl = true,
                "cmd" | "command" | "super" | "meta" => mods.command = true,
                _ => {
                    if key.is_some() {
                        return None;
                    }
                    key = Some(parse_key(tok)?);
                }
            }
        }
        Some(Chord::new(mods, key?))
    }
}

fn parse_key(tok: &str) -> Option<Key> {
    Key::from_name(tok).or_else(|| {
        let mut chars = tok.chars();
        let first = chars.next()?;
        let titled: String = first.to_uppercase().chain(chars).collect();
        Key::from_name(&titled)
    })
}

struct Binding {
    prefix: Option<Chord>,
    chord: Chord,
    action: Action,
}

impl Binding {
    fn describe(&self) -> String {
        match self.prefix {
            Some(p) => {
                let (a, b) = (p.describe(), self.chord.describe());
                if a.len() == 1 && b.len() == 1 {
                    format!("{a}{b}")
                } else {
                    format!("{a} {b}")
                }
            }
            None => self.chord.describe(),
        }
    }
}

pub struct HelpEntry {
    pub keys: String,
    pub desc: &'static str,
}

pub trait KeySource {
    fn consume(&mut self, mods: Modifiers, key: Key) -> bool;
}

pub struct Keymap {
    maps: [Vec<Binding>; 5],
}

impl Keymap {
    pub fn from_config(overrides: &BTreeMap<String, BTreeMap<String, String>>) -> Self {
        let mut km = Self::defaults();
        km.apply_overrides(overrides);
        for m in &mut km.maps {
            m.sort_by_key(|b| std::cmp::Reverse(b.chord.specificity()));
        }
        km
    }

    fn push(&mut self, ctx: Context, mods: Modifiers, key: Key, action: Action) {
        self.maps[ctx.index()].push(Binding {
            prefix: None,
            chord: Chord::new(mods, key),
            action,
        });
    }

    fn push_seq(&mut self, ctx: Context, prefix: Chord, chord: Chord, action: Action) {
        self.maps[ctx.index()].push(Binding {
            prefix: Some(prefix),
            chord,
            action,
        });
    }

    fn defaults() -> Self {
        use Action::*;
        use Context::*;
        use Key::*;
        let n = Modifiers::NONE;
        let alt = Modifiers::ALT;
        let shift = Modifiers::SHIFT;
        let ctrl = Modifiers::CTRL;
        let mut km = Keymap {
            maps: [Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new()],
        };

        km.push(Global, alt, H, FocusLeft);
        km.push(Global, alt, L, FocusRight);
        km.push(Global, alt, K, FocusUp);
        km.push(Global, alt, J, FocusDown);
        km.push(Global, n, Tab, CycleTabFwd);
        km.push(Global, shift, Tab, CycleTabBack);
        km.push(Global, ctrl, Tab, CycleTab);
        km.push(Global, ctrl, Backtick, ToggleShell);
        km.push(Global, ctrl.plus(shift), F, OpenSearch);
        km.push(Global, ctrl, O, NavBack);
        km.push(Global, ctrl, I, NavForward);

        km.push(Diff, n, Slash, DiffFind);
        km.push(Diff, n, J, DiffDown);
        km.push(Diff, n, K, DiffUp);
        km.push_seq(Diff, Chord::new(n, G), Chord::new(n, G), DiffTop);
        km.push(Diff, shift, G, DiffBottom);
        km.push_seq(
            Diff,
            Chord::new(n, CloseBracket),
            Chord::new(n, C),
            DiffNextHunk,
        );
        km.push_seq(
            Diff,
            Chord::new(n, OpenBracket),
            Chord::new(n, C),
            DiffPrevHunk,
        );
        km.push(Diff, n, V, DiffToggleVisual);
        km.push(Diff, n, Escape, DiffClearVisual);
        km.push(Diff, n, S, DiffStageSelection);
        km.push(Diff, n, U, DiffUnstageSelection);
        km.push(Diff, n, D, DiffDiscardSelection);
        km.push(Diff, ctrl, D, DiffHalfPageDown);
        km.push(Diff, ctrl, U, DiffHalfPageUp);
        km.push(Diff, ctrl, F, DiffPageDown);
        km.push(Diff, ctrl, B, DiffPageUp);
        km.push(Diff, n, E, DiffEditor);
        km.push(Diff, n, Y, DiffCopySelection);

        km.push_seq(Changes, Chord::new(n, G), Chord::new(n, G), ChangesTop);
        km.push(Changes, shift, G, ChangesBottom);
        km.push(Changes, n, J, ChangesDown);
        km.push(Changes, n, K, ChangesUp);
        km.push(Changes, n, H, ChangesCollapse);
        km.push(Changes, n, L, ChangesExpand);
        km.push(Changes, n, Enter, ChangesActivate);
        km.push(Changes, n, Space, ChangesStageToggle);
        km.push(Changes, n, E, ChangesEdit);
        km.push(Changes, n, D, ChangesDiscard);
        km.push(Changes, ctrl, D, ChangesHalfPageDown);
        km.push(Changes, ctrl, U, ChangesHalfPageUp);

        km.push_seq(Sidebar, Chord::new(n, G), Chord::new(n, G), SidebarTop);
        km.push(Sidebar, shift, G, SidebarBottom);
        km.push(Sidebar, n, J, SidebarDown);
        km.push(Sidebar, n, K, SidebarUp);
        km.push(Sidebar, n, Enter, SidebarSelect);
        km.push(Sidebar, n, L, SidebarExpand);
        km.push(Sidebar, n, H, SidebarCollapse);
        km.push(Sidebar, ctrl, D, SidebarHalfPageDown);
        km.push(Sidebar, ctrl, U, SidebarHalfPageUp);

        km.push(Graph, n, J, GraphDown);
        km.push(Graph, n, K, GraphUp);
        km.push_seq(Graph, Chord::new(n, G), Chord::new(n, G), GraphTop);
        km.push(Graph, shift, G, GraphBottom);
        km.push(Graph, ctrl, D, GraphHalfPageDown);
        km.push(Graph, ctrl, U, GraphHalfPageUp);
        km.push(Graph, n, L, GraphOpen);
        km.push(Graph, n, Enter, GraphOpen);
        km.push(Graph, n, E, GraphEditor);
        km.push(Graph, n, H, GraphCollapse);
        km.push(Graph, ctrl, Period, GraphContextMenu);
        km.push_seq(
            Graph,
            Chord::new(n, Space),
            Chord::new(n, Period),
            GraphContextMenu,
        );
        km.push(Graph, shift, R, GraphReset);
        km.push(Graph, n, B, GraphCreateBranch);
        km.push(Graph, n, T, GraphCreateTag);
        km.push(Graph, n, Y, GraphCherryPick);
        km.push(Graph, shift, V, GraphRevert);
        km.push(Graph, shift, B, GraphRebaseOnto);
        km.push(Graph, n, I, GraphRebaseInteractive);
        km.push(Graph, n, O, GraphCheckout);
        km.push(Graph, n, P, GraphPush);
        km.push(Graph, shift, P, GraphForcePush);
        km.push(Graph, n, F, GraphFetch);

        km
    }

    fn apply_overrides(&mut self, overrides: &BTreeMap<String, BTreeMap<String, String>>) {
        for (ctx_name, table) in overrides {
            let Some(ctx) = Context::from_name(ctx_name) else {
                eprintln!("keymap: unknown context [keys.{ctx_name}], ignored");
                continue;
            };
            for (chord_str, action_str) in table {
                let chords: Vec<&str> = chord_str.split_whitespace().collect();
                let parsed: Option<Vec<Chord>> = chords.iter().map(|c| Chord::parse(c)).collect();
                let Some(seq) = parsed.filter(|v| !v.is_empty() && v.len() <= 2) else {
                    eprintln!("keymap: bad chord \"{chord_str}\" in [keys.{ctx_name}], ignored");
                    continue;
                };
                let (prefix, chord) = if seq.len() == 2 {
                    (Some(seq[0]), seq[1])
                } else {
                    (None, seq[0])
                };

                let unbind = matches!(action_str.as_str(), "none" | "unbind" | "disabled");
                let action = Action::from_name(action_str);
                if !unbind && action.is_none() {
                    eprintln!(
                        "keymap: unknown action \"{action_str}\" in [keys.{ctx_name}], ignored"
                    );
                    continue;
                }

                let map = &mut self.maps[ctx.index()];
                map.retain(|b| !(b.prefix == prefix && b.chord == chord));
                if let Some(action) = action {
                    map.push(Binding {
                        prefix,
                        chord,
                        action,
                    });
                }
            }
        }
    }

    pub fn help_for(&self, ctx: Context) -> Vec<HelpEntry> {
        let mut bindings: Vec<&Binding> = self.maps[ctx.index()].iter().collect();
        bindings.sort_by_key(|b| (b.action.order(), b.chord.specificity()));
        bindings
            .iter()
            .map(|b| HelpEntry {
                keys: b.describe(),
                desc: b.action.describe(),
            })
            .collect()
    }

    pub fn resolve<S: KeySource, F: Fn(Action) -> bool>(
        &self,
        src: &mut S,
        ctx: Context,
        pending: &mut Option<Chord>,
        allowed: F,
    ) -> Vec<Action> {
        let mut out = Vec::new();
        let mut plain_fired = false;
        let mut established = false;

        for b in &self.maps[ctx.index()] {
            if !allowed(b.action) {
                continue;
            }
            match b.prefix {
                None => {
                    if src.consume(b.chord.mods, b.chord.key) {
                        out.push(b.action);
                        plain_fired = true;
                    }
                }
                Some(prefix) => {
                    let complete =
                        *pending == Some(prefix) && src.consume(b.chord.mods, b.chord.key);
                    if complete {
                        out.push(b.action);
                        *pending = None;
                    } else if src.consume(prefix.mods, prefix.key) {
                        *pending = Some(prefix);
                        established = true;
                    }
                }
            }
        }

        if plain_fired && !established {
            *pending = None;
        }
        out
    }
}

impl Default for Keymap {
    fn default() -> Self {
        Self::from_config(&BTreeMap::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_names_roundtrip() {
        for &key in Key::ALL {
            assert_eq!(Key::from_name(key.name()), Some(key));
            assert_eq!(Key::from_name(key.symbol_or_name()), Some(key));
        }
    }

    #[test]
    fn parses_simple_chords() {
        let c = Chord::parse("alt+h").unwrap();
        assert_eq!(c.key, Key::H);
        assert!(c.mods.alt);
        assert!(!c.mods.ctrl);

        let c = Chord::parse("ctrl+d").unwrap();
        assert_eq!(c.key, Key::D);
        assert!(c.mods.ctrl);

        let c = Chord::parse("Enter").unwrap();
        assert_eq!(c.key, Key::Enter);
        assert_eq!(c.mods, Modifiers::NONE);
    }

    #[test]
    fn rejects_garbage_chords() {
        assert!(Chord::parse("").is_none());
        assert!(Chord::parse("alt+").is_none());
        assert!(Chord::parse("wat+h").is_none());
        assert!(Chord::parse("h+j").is_none());
    }

    #[test]
    fn action_names_roundtrip() {
        for (action, name, _) in Action::TABLE {
            assert_eq!(Action::from_name(name), Some(*action));
        }
    }

    #[test]
    fn override_rebinds_and_unknown_is_ignored() {
        let mut over: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
        let mut diff = BTreeMap::new();
        diff.insert("ctrl+e".to_string(), "diff-half-page-down".to_string());
        diff.insert("ctrl+d".to_string(), "bogus-action".to_string());
        over.insert("diff".to_string(), diff);
        over.insert("nonsense".to_string(), BTreeMap::new());

        let km = Keymap::from_config(&over);
        let diff_map = &km.maps[Context::Diff.index()];
        assert!(
            diff_map
                .iter()
                .any(|b| b.chord.key == Key::E && b.action == Action::DiffHalfPageDown)
        );
        // ctrl+d default survives because the override for it was invalid
        assert!(diff_map.iter().any(|b| b.chord.key == Key::D
            && b.chord.mods.ctrl
            && b.action == Action::DiffHalfPageDown));
    }

    struct QueueSource(Vec<(Modifiers, Key)>);

    impl KeySource for QueueSource {
        fn consume(&mut self, mods: Modifiers, key: Key) -> bool {
            if let Some(i) = self.0.iter().position(|&(m, k)| m == mods && k == key) {
                self.0.remove(i);
                true
            } else {
                false
            }
        }
    }

    #[test]
    fn gg_sequence_fires_top_on_second_g() {
        let km = Keymap::default();
        let mut pending = None;

        let mut src = QueueSource(vec![(Modifiers::NONE, Key::G)]);
        let out = km.resolve(&mut src, Context::Changes, &mut pending, |_| true);
        assert!(out.is_empty());
        assert!(pending.is_some());

        let mut src = QueueSource(vec![(Modifiers::NONE, Key::G)]);
        let out = km.resolve(&mut src, Context::Changes, &mut pending, |_| true);
        assert_eq!(out, vec![Action::ChangesTop]);
        assert!(pending.is_none());
    }

    #[test]
    fn shift_g_does_not_start_gg_prefix() {
        let km = Keymap::default();
        let mut pending = None;

        let mut src = QueueSource(vec![(Modifiers::SHIFT, Key::G)]);
        let out = km.resolve(&mut src, Context::Changes, &mut pending, |_| true);
        assert_eq!(out, vec![Action::ChangesBottom]);
        assert!(pending.is_none());
    }

    #[test]
    fn help_lists_chords_and_descriptions() {
        let km = Keymap::default();

        let global = km.help_for(Context::Global);
        assert!(global.iter().any(|e| e.keys == "Alt+h"));
        assert!(global.iter().any(|e| e.keys == "Ctrl+Shift+f"));
        assert!(global.iter().all(|e| !e.desc.is_empty()));

        let graph = km.help_for(Context::Graph);
        assert!(graph.iter().any(|e| e.keys == "gg"));
        assert!(graph.iter().any(|e| e.keys == "G"));
        assert!(graph.iter().any(|e| e.keys == "Ctrl+d"));
        assert!(graph.iter().any(|e| e.keys == "Space ."));
    }

    #[test]
    fn unbind_removes_default() {
        let mut over: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
        let mut changes = BTreeMap::new();
        changes.insert("d".to_string(), "none".to_string());
        over.insert("changes".to_string(), changes);

        let km = Keymap::from_config(&over);
        let map = &km.maps[Context::Changes.index()];
        assert!(
            !map.iter()
                .any(|b| b.prefix.is_none() && b.chord.key == Key::D && !b.chord.mods.ctrl)
        );
    }
}
