use std::collections::BTreeMap;

use egui::{Key, Modifiers};

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
    DiffToggleVisual,
    DiffClearVisual,
    DiffStageSelection,
    DiffUnstageSelection,
    DiffHalfPageDown,
    DiffHalfPageUp,
    DiffPageDown,
    DiffPageUp,

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
}

impl Action {
    const TABLE: &'static [(Action, &'static str)] = &[
        (Action::FocusLeft, "focus-left"),
        (Action::FocusRight, "focus-right"),
        (Action::FocusUp, "focus-up"),
        (Action::FocusDown, "focus-down"),
        (Action::CycleTab, "cycle-tab"),
        (Action::CycleTabFwd, "cycle-tab-fwd"),
        (Action::CycleTabBack, "cycle-tab-back"),
        (Action::ToggleShell, "toggle-shell"),
        (Action::OpenSearch, "open-search"),
        (Action::NavBack, "nav-back"),
        (Action::NavForward, "nav-forward"),
        (Action::DiffFind, "diff-find"),
        (Action::DiffDown, "diff-down"),
        (Action::DiffUp, "diff-up"),
        (Action::DiffTop, "diff-top"),
        (Action::DiffBottom, "diff-bottom"),
        (Action::DiffToggleVisual, "diff-toggle-visual"),
        (Action::DiffClearVisual, "diff-clear-visual"),
        (Action::DiffStageSelection, "diff-stage-selection"),
        (Action::DiffUnstageSelection, "diff-unstage-selection"),
        (Action::DiffHalfPageDown, "diff-half-page-down"),
        (Action::DiffHalfPageUp, "diff-half-page-up"),
        (Action::DiffPageDown, "diff-page-down"),
        (Action::DiffPageUp, "diff-page-up"),
        (Action::ChangesTop, "changes-top"),
        (Action::ChangesBottom, "changes-bottom"),
        (Action::ChangesDown, "changes-down"),
        (Action::ChangesUp, "changes-up"),
        (Action::ChangesCollapse, "changes-collapse"),
        (Action::ChangesExpand, "changes-expand"),
        (Action::ChangesActivate, "changes-activate"),
        (Action::ChangesStageToggle, "changes-stage-toggle"),
        (Action::ChangesEdit, "changes-edit"),
        (Action::ChangesDiscard, "changes-discard"),
        (Action::ChangesHalfPageDown, "changes-half-page-down"),
        (Action::ChangesHalfPageUp, "changes-half-page-up"),
        (Action::SidebarTop, "sidebar-top"),
        (Action::SidebarBottom, "sidebar-bottom"),
        (Action::SidebarDown, "sidebar-down"),
        (Action::SidebarUp, "sidebar-up"),
        (Action::SidebarSelect, "sidebar-select"),
        (Action::SidebarExpand, "sidebar-expand"),
        (Action::SidebarCollapse, "sidebar-collapse"),
        (Action::SidebarHalfPageDown, "sidebar-half-page-down"),
        (Action::SidebarHalfPageUp, "sidebar-half-page-up"),
        (Action::GraphDown, "graph-down"),
        (Action::GraphUp, "graph-up"),
        (Action::GraphTop, "graph-top"),
        (Action::GraphBottom, "graph-bottom"),
        (Action::GraphHalfPageDown, "graph-half-page-down"),
        (Action::GraphHalfPageUp, "graph-half-page-up"),
        (Action::GraphOpen, "graph-open"),
        (Action::GraphEditor, "graph-editor"),
        (Action::GraphCollapse, "graph-collapse"),
        (Action::GraphContextMenu, "graph-context-menu"),
        (Action::GraphReset, "graph-reset"),
        (Action::GraphCreateBranch, "graph-create-branch"),
        (Action::GraphCreateTag, "graph-create-tag"),
        (Action::GraphCherryPick, "graph-cherry-pick"),
        (Action::GraphRevert, "graph-revert"),
        (Action::GraphRebaseOnto, "graph-rebase-onto"),
        (Action::GraphRebaseInteractive, "graph-rebase-interactive"),
        (Action::GraphCheckout, "graph-checkout"),
    ];

    fn from_name(s: &str) -> Option<Action> {
        Self::TABLE
            .iter()
            .find(|(_, name)| *name == s)
            .map(|(a, _)| *a)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Chord {
    pub mods: Modifiers,
    pub key: Key,
}

impl Chord {
    const fn new(mods: Modifiers, key: Key) -> Self {
        Chord { mods, key }
    }

    fn specificity(&self) -> u32 {
        let m = &self.mods;
        m.alt as u32 + m.shift as u32 + m.ctrl as u32 + m.command as u32 + m.mac_cmd as u32
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
            maps: [
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
            ],
        };

        km.push(Global, alt, H, FocusLeft);
        km.push(Global, alt, L, FocusRight);
        km.push(Global, alt, K, FocusUp);
        km.push(Global, alt, J, FocusDown);
        km.push(Global, n, Tab, CycleTab);
        km.push(Global, ctrl, Tab, CycleTabFwd);
        km.push(Global, ctrl.plus(shift), Tab, CycleTabBack);
        km.push(Global, ctrl, Backtick, ToggleShell);
        km.push(Global, ctrl.plus(shift), F, OpenSearch);
        km.push(Global, ctrl, O, NavBack);
        km.push(Global, ctrl, I, NavForward);

        km.push(Diff, n, Slash, DiffFind);
        km.push(Diff, n, J, DiffDown);
        km.push(Diff, n, K, DiffUp);
        km.push_seq(Diff, Chord::new(n, G), Chord::new(n, G), DiffTop);
        km.push(Diff, shift, G, DiffBottom);
        km.push(Diff, n, V, DiffToggleVisual);
        km.push(Diff, n, Escape, DiffClearVisual);
        km.push(Diff, n, S, DiffStageSelection);
        km.push(Diff, n, U, DiffUnstageSelection);
        km.push(Diff, ctrl, D, DiffHalfPageDown);
        km.push(Diff, ctrl, U, DiffHalfPageUp);
        km.push(Diff, ctrl, F, DiffPageDown);
        km.push(Diff, ctrl, B, DiffPageUp);

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
        km.push_seq(Graph, Chord::new(n, Space), Chord::new(n, Period), GraphContextMenu);
        km.push(Graph, shift, R, GraphReset);
        km.push(Graph, n, B, GraphCreateBranch);
        km.push(Graph, n, T, GraphCreateTag);
        km.push(Graph, n, Y, GraphCherryPick);
        km.push(Graph, shift, V, GraphRevert);
        km.push(Graph, shift, B, GraphRebaseOnto);
        km.push(Graph, n, I, GraphRebaseInteractive);
        km.push(Graph, n, O, GraphCheckout);

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

    pub fn poll<F: Fn(Action) -> bool>(
        &self,
        ui: &mut egui::Ui,
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
                    if ui.input_mut(|i| i.consume_key(b.chord.mods, b.chord.key)) {
                        out.push(b.action);
                        plain_fired = true;
                    }
                }
                Some(prefix) => {
                    let complete = *pending == Some(prefix)
                        && ui.input_mut(|i| i.consume_key(b.chord.mods, b.chord.key));
                    if complete {
                        out.push(b.action);
                        *pending = None;
                    } else if ui.input_mut(|i| i.consume_key(prefix.mods, prefix.key)) {
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
        for (action, name) in Action::TABLE {
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
        assert!(
            diff_map
                .iter()
                .any(|b| b.chord.key == Key::D
                    && b.chord.mods.ctrl
                    && b.action == Action::DiffHalfPageDown)
        );
    }

    fn key_event(key: Key, mods: Modifiers) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: mods,
        }
    }

    fn poll_events(
        km: &Keymap,
        ctx: &egui::Context,
        context: Context,
        pending: &mut Option<Chord>,
        events: Vec<egui::Event>,
    ) -> Vec<Action> {
        ctx.begin_pass(egui::RawInput {
            events,
            ..Default::default()
        });
        let mut ui = egui::Ui::new(
            ctx.clone(),
            egui::Id::new("keys_test"),
            egui::UiBuilder::new(),
        );
        let out = km.poll(&mut ui, context, pending, |_| true);
        let _ = ctx.end_pass();
        out
    }

    #[test]
    fn ctrl_d_scrolls_and_plain_keys_are_distinct() {
        let km = Keymap::default();
        let ctx = egui::Context::default();
        let mut pending = None;

        let out = poll_events(
            &km,
            &ctx,
            Context::Diff,
            &mut pending,
            vec![key_event(Key::D, Modifiers::CTRL)],
        );
        assert_eq!(out, vec![Action::DiffHalfPageDown]);

        let out = poll_events(
            &km,
            &ctx,
            Context::Diff,
            &mut pending,
            vec![key_event(Key::U, Modifiers::NONE)],
        );
        assert_eq!(out, vec![Action::DiffUnstageSelection]);
    }

    #[test]
    fn gg_sequence_fires_top_on_second_g() {
        let km = Keymap::default();
        let ctx = egui::Context::default();
        let mut pending = None;

        let out = poll_events(
            &km,
            &ctx,
            Context::Changes,
            &mut pending,
            vec![key_event(Key::G, Modifiers::NONE)],
        );
        assert!(out.is_empty());
        assert!(pending.is_some());

        let out = poll_events(
            &km,
            &ctx,
            Context::Changes,
            &mut pending,
            vec![key_event(Key::G, Modifiers::NONE)],
        );
        assert_eq!(out, vec![Action::ChangesTop]);
        assert!(pending.is_none());
    }

    #[test]
    fn shift_g_does_not_start_gg_prefix() {
        let km = Keymap::default();
        let ctx = egui::Context::default();
        let mut pending = None;

        let out = poll_events(
            &km,
            &ctx,
            Context::Changes,
            &mut pending,
            vec![key_event(Key::G, Modifiers::SHIFT)],
        );
        assert_eq!(out, vec![Action::ChangesBottom]);
        assert!(pending.is_none());
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
