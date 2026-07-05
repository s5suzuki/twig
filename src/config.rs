use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::color::Rgb;

pub const BASE_FONT_SIZE: f32 = 13.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Theme {
    #[default]
    Dark,
    Light,
    CatppuccinMocha,
}

impl Theme {
    pub fn label(self) -> &'static str {
        match self {
            Theme::Dark => "Dark",
            Theme::Light => "Light",
            Theme::CatppuccinMocha => "Catppuccin Mocha",
        }
    }

    pub const ALL: [Theme; 3] = [Theme::Dark, Theme::Light, Theme::CatppuccinMocha];
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Accent {
    Rosewater,
    Flamingo,
    Pink,
    Mauve,
    Red,
    Maroon,
    Peach,
    Yellow,
    Green,
    Teal,
    Sky,
    Sapphire,
    Blue,
    #[default]
    Lavender,
}

impl Accent {
    pub fn rgb(self) -> Rgb {
        let (r, g, b) = match self {
            Accent::Rosewater => (0xf5, 0xe0, 0xdc),
            Accent::Flamingo => (0xf2, 0xcd, 0xcd),
            Accent::Pink => (0xf5, 0xc2, 0xe7),
            Accent::Mauve => (0xcb, 0xa6, 0xf7),
            Accent::Red => (0xf3, 0x8b, 0xa8),
            Accent::Maroon => (0xeb, 0xa0, 0xac),
            Accent::Peach => (0xfa, 0xb3, 0x87),
            Accent::Yellow => (0xf9, 0xe2, 0xaf),
            Accent::Green => (0xa6, 0xe3, 0xa1),
            Accent::Teal => (0x94, 0xe2, 0xd5),
            Accent::Sky => (0x89, 0xdc, 0xeb),
            Accent::Sapphire => (0x74, 0xc7, 0xec),
            Accent::Blue => (0x89, 0xb4, 0xfa),
            Accent::Lavender => (0xb4, 0xbe, 0xfe),
        };
        Rgb::new(r, g, b)
    }

    pub fn label(self) -> &'static str {
        match self {
            Accent::Rosewater => "Rosewater",
            Accent::Flamingo => "Flamingo",
            Accent::Pink => "Pink",
            Accent::Mauve => "Mauve",
            Accent::Red => "Red",
            Accent::Maroon => "Maroon",
            Accent::Peach => "Peach",
            Accent::Yellow => "Yellow",
            Accent::Green => "Green",
            Accent::Teal => "Teal",
            Accent::Sky => "Sky",
            Accent::Sapphire => "Sapphire",
            Accent::Blue => "Blue",
            Accent::Lavender => "Lavender",
        }
    }

    pub const ALL: [Accent; 14] = [
        Accent::Rosewater,
        Accent::Flamingo,
        Accent::Pink,
        Accent::Mauve,
        Accent::Red,
        Accent::Maroon,
        Accent::Peach,
        Accent::Yellow,
        Accent::Green,
        Accent::Teal,
        Accent::Sky,
        Accent::Sapphire,
        Accent::Blue,
        Accent::Lavender,
    ];
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub font_size: f32,
    pub theme: Theme,
    pub accent: Accent,
    pub graph_commit_limit: usize,
    pub graph_show_author: bool,
    pub graph_show_date: bool,
    pub graph_files_tree: bool,
    pub mono_font: String,
    pub show_title_bar: bool,
    pub show_files: bool,
    #[serde(alias = "confirm_delete")]
    pub confirm_discard: bool,
    pub commit_message_guide: bool,
    #[serde(default)]
    pub keys: BTreeMap<String, BTreeMap<String, String>>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            font_size: BASE_FONT_SIZE,
            theme: Theme::Dark,
            accent: Accent::Lavender,
            graph_commit_limit: 200,
            graph_show_author: true,
            graph_show_date: true,
            graph_files_tree: true,
            mono_font: "hackgen-console-nf".to_string(),
            show_title_bar: true,
            show_files: false,
            confirm_discard: true,
            commit_message_guide: true,
            keys: BTreeMap::new(),
        }
    }
}

impl Config {
    pub fn load() -> Self {
        config_path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| toml::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let Some(path) = config_path() else {
            return;
        };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(s) = toml::to_string_pretty(self) {
            let _ = std::fs::write(path, s);
        }
    }
}

fn config_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("twig").join("config.toml"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_preserves_values() {
        let cfg = Config {
            font_size: 18.0,
            theme: Theme::CatppuccinMocha,
            accent: Accent::Mauve,
            ..Config::default()
        };
        let s = toml::to_string_pretty(&cfg).unwrap();
        assert!(s.contains("catppuccin-mocha"), "got:\n{s}");
        assert!(s.contains("mauve"), "got:\n{s}");
        let back: Config = toml::from_str(&s).unwrap();
        assert_eq!(back.font_size, 18.0);
        assert_eq!(back.theme, Theme::CatppuccinMocha);
        assert_eq!(back.accent, Accent::Mauve);
    }

    #[test]
    fn partial_config_fills_defaults() {
        let back: Config = toml::from_str("font_size = 15.0").unwrap();
        assert_eq!(back.font_size, 15.0);
        assert_eq!(back.theme, Theme::Dark);
        assert_eq!(back.accent, Accent::Lavender);
        assert_eq!(back.graph_commit_limit, 200);
        assert!(back.graph_show_author);
        assert!(back.graph_show_date);
        assert_eq!(back.mono_font, "hackgen-console-nf");
        assert!(back.confirm_discard);
        assert!(back.commit_message_guide);
    }

    #[test]
    fn keys_table_deserializes() {
        let src = "\
font_size = 14.0

[keys.diff]
\"ctrl+e\" = \"diff-half-page-down\"

[keys.global]
\"ctrl+t\" = \"toggle-shell\"
";
        let cfg: Config = toml::from_str(src).unwrap();
        assert_eq!(
            cfg.keys["diff"]["ctrl+e"],
            "diff-half-page-down".to_string()
        );
        assert_eq!(cfg.keys["global"]["ctrl+t"], "toggle-shell".to_string());
    }

    #[test]
    fn missing_keys_table_defaults_empty() {
        let cfg: Config = toml::from_str("font_size = 14.0").unwrap();
        assert!(cfg.keys.is_empty());
    }

    #[test]
    fn corrupt_toml_is_a_parse_error() {
        assert!(toml::from_str::<Config>("this is not toml = = =").is_err());
        assert!(toml::from_str::<Config>("theme = \"solarized\"").is_err());
    }
}
