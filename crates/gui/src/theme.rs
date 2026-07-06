use egui::{Color32, Stroke, Visuals};

use twit_core::color::Rgb;
use twit_core::config::{Config, Theme};

pub fn c32(rgb: Rgb) -> Color32 {
    Color32::from_rgb(rgb.r, rgb.g, rgb.b)
}

pub fn visuals(config: &Config) -> Visuals {
    match config.theme {
        Theme::Dark => Visuals::dark(),
        Theme::Light => Visuals::light(),
        Theme::CatppuccinMocha => catppuccin_mocha(c32(config.accent.rgb())),
    }
}

fn blend(fg: Color32, bg: Color32, t: f32) -> Color32 {
    let l = |a: u8, b: u8| (a as f32 * (1.0 - t) + b as f32 * t).round() as u8;
    Color32::from_rgb(l(bg.r(), fg.r()), l(bg.g(), fg.g()), l(bg.b(), fg.b()))
}

fn catppuccin_mocha(accent: Color32) -> Visuals {
    let base = Color32::from_rgb(0x1e, 0x1e, 0x2e);
    let mantle = Color32::from_rgb(0x18, 0x18, 0x25);
    let crust = Color32::from_rgb(0x11, 0x11, 0x1b);
    let surface0 = Color32::from_rgb(0x31, 0x32, 0x44);
    let surface1 = Color32::from_rgb(0x45, 0x47, 0x5a);
    let surface2 = Color32::from_rgb(0x58, 0x5b, 0x70);
    let overlay0 = Color32::from_rgb(0x6c, 0x70, 0x86);
    let text = Color32::from_rgb(0xcd, 0xd6, 0xf4);
    let selection = blend(accent, base, 0.45);

    let mut v = Visuals::dark();
    v.dark_mode = true;
    v.override_text_color = Some(text);
    v.panel_fill = base;
    v.window_fill = base;
    v.extreme_bg_color = crust;
    v.faint_bg_color = mantle;
    v.code_bg_color = mantle;
    v.window_stroke = Stroke::new(1.0, surface0);
    v.hyperlink_color = accent;
    v.selection.bg_fill = selection;
    v.selection.stroke = Stroke::new(1.0, accent);

    let w = &mut v.widgets;
    w.noninteractive.bg_fill = base;
    w.noninteractive.weak_bg_fill = base;
    w.noninteractive.bg_stroke = Stroke::new(1.0, surface0);
    w.noninteractive.fg_stroke = Stroke::new(1.0, text);

    w.inactive.bg_fill = surface0;
    w.inactive.weak_bg_fill = surface0;
    w.inactive.bg_stroke = Stroke::NONE;
    w.inactive.fg_stroke = Stroke::new(1.0, text);

    w.hovered.bg_fill = surface1;
    w.hovered.weak_bg_fill = surface1;
    w.hovered.bg_stroke = Stroke::new(1.0, overlay0);
    w.hovered.fg_stroke = Stroke::new(1.5, text);

    w.active.bg_fill = surface2;
    w.active.weak_bg_fill = surface2;
    w.active.bg_stroke = Stroke::new(1.0, accent);
    w.active.fg_stroke = Stroke::new(2.0, text);

    w.open.bg_fill = surface0;
    w.open.weak_bg_fill = surface0;
    w.open.bg_stroke = Stroke::new(1.0, surface1);
    w.open.fg_stroke = Stroke::new(1.0, text);

    v
}
