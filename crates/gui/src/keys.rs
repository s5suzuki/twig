pub use twig_core::keymap::{Action, Chord, Context, Key, Keymap, Modifiers};

use twig_core::keymap::KeySource;

struct UiSource<'a>(&'a mut egui::Ui);

impl KeySource for UiSource<'_> {
    fn consume(&mut self, mods: Modifiers, key: Key) -> bool {
        self.0
            .input_mut(|i| i.consume_key(egui_mods(mods), egui_key(key)))
    }
}

fn egui_mods(m: Modifiers) -> egui::Modifiers {
    egui::Modifiers {
        alt: m.alt,
        ctrl: m.ctrl,
        shift: m.shift,
        mac_cmd: false,
        command: m.command,
    }
}

fn egui_key(k: Key) -> egui::Key {
    egui::Key::from_name(k.name()).expect("every core key name maps to an egui key")
}

pub trait KeymapPoll {
    fn poll<F: Fn(Action) -> bool>(
        &self,
        ui: &mut egui::Ui,
        ctx: Context,
        pending: &mut Option<Chord>,
        allowed: F,
    ) -> Vec<Action>;
}

impl KeymapPoll for Keymap {
    fn poll<F: Fn(Action) -> bool>(
        &self,
        ui: &mut egui::Ui,
        ctx: Context,
        pending: &mut Option<Chord>,
        allowed: F,
    ) -> Vec<Action> {
        self.resolve(&mut UiSource(ui), ctx, pending, allowed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_core_key_maps_to_egui() {
        for &key in Key::ALL {
            assert_eq!(egui_key(key).name(), key.name());
        }
    }

    fn key_event(key: egui::Key, mods: egui::Modifiers) -> egui::Event {
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
            vec![key_event(egui::Key::D, egui::Modifiers::CTRL)],
        );
        assert_eq!(out, vec![Action::DiffHalfPageDown]);

        let out = poll_events(
            &km,
            &ctx,
            Context::Diff,
            &mut pending,
            vec![key_event(egui::Key::U, egui::Modifiers::NONE)],
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
            vec![key_event(egui::Key::G, egui::Modifiers::NONE)],
        );
        assert!(out.is_empty());
        assert!(pending.is_some());

        let out = poll_events(
            &km,
            &ctx,
            Context::Changes,
            &mut pending,
            vec![key_event(egui::Key::G, egui::Modifiers::NONE)],
        );
        assert_eq!(out, vec![Action::ChangesTop]);
        assert!(pending.is_none());
    }

    #[test]
    fn bracket_c_sequence_jumps_hunks() {
        let km = Keymap::default();
        let ctx = egui::Context::default();
        let mut pending = None;

        let out = poll_events(
            &km,
            &ctx,
            Context::Diff,
            &mut pending,
            vec![key_event(egui::Key::CloseBracket, egui::Modifiers::NONE)],
        );
        assert!(out.is_empty());
        assert!(pending.is_some());

        let out = poll_events(
            &km,
            &ctx,
            Context::Diff,
            &mut pending,
            vec![key_event(egui::Key::C, egui::Modifiers::NONE)],
        );
        assert_eq!(out, vec![Action::DiffNextHunk]);
        assert!(pending.is_none());

        let out = poll_events(
            &km,
            &ctx,
            Context::Diff,
            &mut pending,
            vec![key_event(egui::Key::OpenBracket, egui::Modifiers::NONE)],
        );
        assert!(out.is_empty());
        let out = poll_events(
            &km,
            &ctx,
            Context::Diff,
            &mut pending,
            vec![key_event(egui::Key::C, egui::Modifiers::NONE)],
        );
        assert_eq!(out, vec![Action::DiffPrevHunk]);
    }
}
