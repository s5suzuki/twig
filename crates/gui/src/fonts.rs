use std::path::Path;
use std::sync::Arc;

use egui::{FontData, FontDefinitions, FontFamily};

const PROPORTIONAL_JP: &[&str] = &[
    "/home/shun/.local/share/fonts/n/NotoSansJP-Regular.ttf",
    "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/noto/NotoSansCJK-Regular.ttc",
];

const NF_FALLBACK: &[&str] = &[
    "/usr/share/fonts/TTF/HackGenConsoleNF-Regular.ttf",
    "/home/shun/.local/share/fonts/h/HackGenConsoleNF_Regular.ttf",
    "/usr/share/fonts/TTF/HackGenConsole-Regular.ttf",
    "/usr/share/fonts/TTF/HackGen-Regular.ttf",
];

pub struct MonoFont {
    pub key: &'static str,
    pub label: &'static str,
    candidates: &'static [&'static str],
}

impl MonoFont {
    fn path(&self) -> Option<&'static str> {
        self.candidates
            .iter()
            .copied()
            .find(|p| Path::new(p).exists())
    }
    pub fn installed(&self) -> bool {
        self.path().is_some()
    }
}

pub const MONO_FONTS: &[MonoFont] = &[
    MonoFont {
        key: "hackgen-console-nf",
        label: "HackGen Console NF",
        candidates: &[
            "/usr/share/fonts/TTF/HackGenConsoleNF-Regular.ttf",
            "/home/shun/.local/share/fonts/h/HackGenConsoleNF_Regular.ttf",
        ],
    },
    MonoFont {
        key: "hackgen-console",
        label: "HackGen Console",
        candidates: &["/usr/share/fonts/TTF/HackGenConsole-Regular.ttf"],
    },
    MonoFont {
        key: "hackgen",
        label: "HackGen",
        candidates: &["/usr/share/fonts/TTF/HackGen-Regular.ttf"],
    },
];

pub fn available_mono() -> Vec<&'static MonoFont> {
    MONO_FONTS.iter().filter(|f| f.installed()).collect()
}

fn pick_mono(key: &str) -> Option<&'static MonoFont> {
    MONO_FONTS
        .iter()
        .find(|f| f.key == key && f.installed())
        .or_else(|| MONO_FONTS.iter().find(|f| f.installed()))
}

pub fn install(ctx: &egui::Context, mono_key: &str) {
    let mut fonts = FontDefinitions::default();

    if let Some((name, data)) = load_first("jp_prop", PROPORTIONAL_JP) {
        fonts.font_data.insert(name.clone(), Arc::new(data));
        fonts
            .families
            .entry(FontFamily::Proportional)
            .or_default()
            .push(name);
    }

    if let Some(path) = pick_mono(mono_key).and_then(MonoFont::path)
        && let Ok(bytes) = std::fs::read(path)
    {
        let name = "mono_primary".to_string();
        fonts
            .font_data
            .insert(name.clone(), Arc::new(FontData::from_owned(bytes)));
        fonts
            .families
            .entry(FontFamily::Monospace)
            .or_default()
            .insert(0, name);
    }

    if let Some((name, data)) = load_first("nf_fallback", NF_FALLBACK) {
        fonts.font_data.insert(name.clone(), Arc::new(data));
        fonts
            .families
            .entry(FontFamily::Monospace)
            .or_default()
            .push(name.clone());
        fonts
            .families
            .entry(FontFamily::Proportional)
            .or_default()
            .push(name);
    }

    ctx.set_fonts(fonts);
}

fn load_first(name: &str, candidates: &[&str]) -> Option<(String, FontData)> {
    for path in candidates {
        if let Ok(bytes) = std::fs::read(path) {
            return Some((name.to_string(), FontData::from_owned(bytes)));
        }
    }
    eprintln!("Warning: font not found ({name}); text may render as tofu.");
    None
}
