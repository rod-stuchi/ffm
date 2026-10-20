//! The 11-palette theme system.
//!
//! A theme is **11 values, not 20**. Everything else — borders, soft fills,
//! the skeleton gradient, both shadows — is derived with `color-mix()`, which
//! is why overriding a single token stays coherent: change `accent` and the
//! focus rail, the due ring, the command caret and the NORMAL badge all move
//! together.
//!
//! Two things this module gets right that the mockup could not:
//!
//! 1. **It emits the full light/dark triple, not one resolved set.**
//!    `applyTheme()` reads `matchMedia` and writes inline styles. A server
//!    cannot, and `ffm.theme = system` means it genuinely does not know which
//!    mode the device is in — so [`RenderedTheme`] carries *both* and the
//!    template emits bare `:root`, the `prefers-color-scheme` block, and the
//!    `[data-theme="dark"]` block. No color ever has its only definition
//!    inside a media or attribute block, which is the rule from `CLAUDE.md`.
//!
//! 2. **One derivation function, so the preview cannot drift.** The theme
//!    picker previews by `hx-get`ting this same rendering. There is no second
//!    code path for a preview to disagree with.
//!
//! Palette and light/dark are two independent facts in two different places:
//! the palette is a personal preference (`member_settings.theme_name`), while
//! light/dark is a property of the *device* — the same person uses a bright
//! laptop and a dark phone — so it lives in the `ffm.theme` cookie.

use std::collections::BTreeMap;
use std::fmt;

/// The eleven tokens a theme is made of. Strongly typed on purpose: an unknown
/// key in `member_settings.theme_overrides` becomes a real error naming the
/// valid tokens, rather than a field silently ignored.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
pub enum Token {
    Paper,
    Surface,
    Surface2,
    Ink,
    Ink2,
    Ink3,
    Line,
    Accent,
    Pos,
    Neg,
    Info,
}

impl Token {
    pub const ALL: [Token; 11] = [
        Token::Paper,
        Token::Surface,
        Token::Surface2,
        Token::Ink,
        Token::Ink2,
        Token::Ink3,
        Token::Line,
        Token::Accent,
        Token::Pos,
        Token::Neg,
        Token::Info,
    ];

    /// The name as it appears in `theme_overrides` JSON and in CSS.
    pub fn key(self) -> &'static str {
        match self {
            Token::Paper => "paper",
            Token::Surface => "surface",
            Token::Surface2 => "surface_2",
            Token::Ink => "ink",
            Token::Ink2 => "ink_2",
            Token::Ink3 => "ink_3",
            Token::Line => "line",
            Token::Accent => "accent",
            Token::Pos => "pos",
            Token::Neg => "neg",
            Token::Info => "info",
        }
    }

    /// The CSS custom-property name, which uses hyphens where the JSON key
    /// uses underscores: `surface_2` -> `--surface-2`.
    pub fn css_var(self) -> &'static str {
        match self {
            Token::Surface2 => "surface-2",
            Token::Ink2 => "ink-2",
            Token::Ink3 => "ink-3",
            other => other.key(),
        }
    }

    pub fn parse(s: &str) -> Option<Token> {
        Token::ALL.into_iter().find(|t| t.key() == s)
    }
}

/// Light or dark. A property of the device, carried in the `ffm.theme` cookie
/// alongside `system` — not of the member, and not of the palette.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Hash, Default)]
pub enum Mode {
    #[default]
    Light,
    Dark,
}

/// What the viewer's cookie says. `System` stamps no attribute, which leaves
/// `prefers-color-scheme` in charge — the default state for most visitors.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub enum ModePref {
    #[default]
    System,
    Light,
    Dark,
}

impl ModePref {
    pub fn parse(s: &str) -> ModePref {
        match s {
            "light" => ModePref::Light,
            "dark" => ModePref::Dark,
            _ => ModePref::System,
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            ModePref::System => "system",
            ModePref::Light => "light",
            ModePref::Dark => "dark",
        }
    }

    /// The next mode in the cycle: System -> Light -> Dark -> System.
    ///
    /// One definition, because there are two controls now — the pre-auth
    /// chrome and the ledger bar — and a cycle that runs one way on the login
    /// page and another inside the app would be a small, maddening bug.
    pub fn next(self) -> &'static str {
        match self {
            ModePref::System => "light",
            ModePref::Light => "dark",
            ModePref::Dark => "system",
        }
    }

    /// The icon that names the current state — a screen for "whatever the
    /// system says", sun and moon for the two a member has pinned.
    pub fn icon(self) -> &'static str {
        match self {
            ModePref::System => "monitor",
            ModePref::Light => "sun",
            ModePref::Dark => "moon",
        }
    }

    /// The `data-theme` attribute value, if any. `System` deliberately stamps
    /// nothing: the un-stamped document is the common case.
    pub fn attr(self) -> Option<&'static str> {
        match self {
            ModePref::System => None,
            ModePref::Light => Some("light"),
            ModePref::Dark => Some("dark"),
        }
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Debug, Hash, Default)]
pub enum ThemeName {
    #[default]
    Ffm,
    Tokyonight,
    Catppuccin,
    Kanagawa,
    Everforest,
    Gruvbox,
    Rosepine,
    Onedark,
    Dracula,
    Nord,
    Solarized,
}

impl ThemeName {
    pub const ALL: [ThemeName; 11] = [
        ThemeName::Ffm,
        ThemeName::Tokyonight,
        ThemeName::Catppuccin,
        ThemeName::Kanagawa,
        ThemeName::Everforest,
        ThemeName::Gruvbox,
        ThemeName::Rosepine,
        ThemeName::Onedark,
        ThemeName::Dracula,
        ThemeName::Nord,
        ThemeName::Solarized,
    ];

    pub fn key(self) -> &'static str {
        match self {
            ThemeName::Ffm => "ffm",
            ThemeName::Tokyonight => "tokyonight",
            ThemeName::Catppuccin => "catppuccin",
            ThemeName::Kanagawa => "kanagawa",
            ThemeName::Everforest => "everforest",
            ThemeName::Gruvbox => "gruvbox",
            ThemeName::Rosepine => "rosepine",
            ThemeName::Onedark => "onedark",
            ThemeName::Dracula => "dracula",
            ThemeName::Nord => "nord",
            ThemeName::Solarized => "solarized",
        }
    }

    /// The label shown in the picker.
    pub fn label(self) -> &'static str {
        match self {
            ThemeName::Ffm => "FFM",
            ThemeName::Tokyonight => "Tokyo Night",
            ThemeName::Catppuccin => "Catppuccin",
            ThemeName::Kanagawa => "Kanagawa",
            ThemeName::Everforest => "Everforest",
            ThemeName::Gruvbox => "Gruvbox",
            ThemeName::Rosepine => "Rosé Pine",
            ThemeName::Onedark => "One Dark",
            ThemeName::Dracula => "Dracula",
            ThemeName::Nord => "Nord",
            ThemeName::Solarized => "Solarized",
        }
    }

    pub fn parse(s: &str) -> Option<ThemeName> {
        ThemeName::ALL.into_iter().find(|t| t.key() == s)
    }
}

/// Exactly the eleven values, in the order the mockup's `K` array declares
/// them. Nothing derived lives here.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Palette {
    pub paper: &'static str,
    pub surface: &'static str,
    pub surface_2: &'static str,
    pub ink: &'static str,
    pub ink_2: &'static str,
    pub ink_3: &'static str,
    pub line: &'static str,
    pub accent: &'static str,
    pub pos: &'static str,
    pub neg: &'static str,
    pub info: &'static str,
}

impl Palette {
    pub fn get(&self, t: Token) -> &'static str {
        match t {
            Token::Paper => self.paper,
            Token::Surface => self.surface,
            Token::Surface2 => self.surface_2,
            Token::Ink => self.ink,
            Token::Ink2 => self.ink_2,
            Token::Ink3 => self.ink_3,
            Token::Line => self.line,
            Token::Accent => self.accent,
            Token::Pos => self.pos,
            Token::Neg => self.neg,
            Token::Info => self.info,
        }
    }
}

pub struct Theme {
    pub name: ThemeName,
    pub dark: Palette,
    pub light: Palette,
}

/// Positional, mirroring the mockup's `TH(...)` helper and its `K` order, so
/// the extracted table below can be diffed against the original line by line.
const fn p(v: [&'static str; 11]) -> Palette {
    Palette {
        paper: v[0],
        surface: v[1],
        surface_2: v[2],
        ink: v[3],
        ink_2: v[4],
        ink_3: v[5],
        line: v[6],
        accent: v[7],
        pos: v[8],
        neg: v[9],
        info: v[10],
    }
}

/// Transcribed mechanically from `design/ffm-app-mockup.html`'s `THEMES`
/// table — not by eye. `palette_table_matches_the_mockup` re-extracts it from
/// that file and asserts every hex string still agrees, so the two cannot
/// drift apart silently.
///
/// Nord and Dracula have no canonical upstream light variant; those two light
/// palettes are derived (Dracula's from Alucard). Everything else uses the
/// real published values.
pub static PALETTES: [Theme; 11] = [
    Theme {
        name: ThemeName::Ffm,
        dark: p([
            "#171512", "#201D18", "#2A261E", "#EEE8DB", "#B2A995", "#807869", "#332E25", "#E9A63F",
            "#82B47D", "#E28470", "#8FB0D6",
        ]),
        light: p([
            "#F6F3ED", "#FFFDF8", "#EEE9DE", "#211E19", "#5C554A", "#8E8578", "#E1DACC", "#A8620A",
            "#3D7A4B", "#B23A2B", "#3F6288",
        ]),
    },
    Theme {
        name: ThemeName::Tokyonight,
        dark: p([
            "#1A1B26", "#1F2335", "#24283B", "#C0CAF5", "#A9B1D6", "#565F89", "#292E42", "#7AA2F7",
            "#9ECE6A", "#F7768E", "#BB9AF7",
        ]),
        light: p([
            "#E1E2E7", "#F0F0F5", "#D9DBE3", "#3760BF", "#6172B0", "#848CB5", "#CBCFDA", "#2E7DE9",
            "#587539", "#F52A65", "#9854F1",
        ]),
    },
    Theme {
        name: ThemeName::Catppuccin,
        dark: p([
            "#181825", "#1E1E2E", "#292A3B", "#CDD6F4", "#A6ADC8", "#6C7086", "#313244", "#CBA6F7",
            "#A6E3A1", "#F38BA8", "#89B4FA",
        ]),
        light: p([
            "#E6E9EF", "#EFF1F5", "#DCE0E8", "#4C4F69", "#6C6F85", "#9CA0B0", "#CCD0DA", "#8839EF",
            "#40A02B", "#D20F39", "#1E66F5",
        ]),
    },
    Theme {
        name: ThemeName::Kanagawa,
        dark: p([
            "#16161D", "#1F1F28", "#2A2A37", "#DCD7BA", "#C8C093", "#727169", "#363646", "#7E9CD8",
            "#98BB6C", "#FF5D62", "#E6C384",
        ]),
        light: p([
            "#F2ECBC", "#F7F1CB", "#E7DBA0", "#545464", "#43436C", "#8A8980", "#DDD2A4", "#4D699B",
            "#6F894E", "#C84053", "#624C83",
        ]),
    },
    Theme {
        name: ThemeName::Everforest,
        dark: p([
            "#2D353B", "#343F44", "#3D484D", "#D3C6AA", "#9DA9A0", "#7A8478", "#475258", "#DBBC7F",
            "#A7C080", "#E67E80", "#7FBBB3",
        ]),
        light: p([
            "#FDF6E3", "#F4F0D9", "#EFEBD4", "#5C6A72", "#829181", "#939F91", "#E6E2CC", "#DFA000",
            "#8DA101", "#F85552", "#3A94C5",
        ]),
    },
    Theme {
        name: ThemeName::Gruvbox,
        dark: p([
            "#1D2021", "#282828", "#3C3836", "#EBDBB2", "#BDAE93", "#928374", "#504945", "#FABD2F",
            "#B8BB26", "#FB4934", "#83A598",
        ]),
        light: p([
            "#F9F5D7", "#FBF1C7", "#EBDBB2", "#3C3836", "#504945", "#7C6F64", "#D5C4A1", "#B57614",
            "#79740E", "#9D0006", "#076678",
        ]),
    },
    Theme {
        name: ThemeName::Rosepine,
        dark: p([
            "#191724", "#1F1D2E", "#26233A", "#E0DEF4", "#908CAA", "#6E6A86", "#403D52", "#EBBCBA",
            "#9CCFD8", "#EB6F92", "#C4A7E7",
        ]),
        light: p([
            "#FAF4ED", "#FFFAF3", "#F2E9E1", "#575279", "#797593", "#9893A5", "#DFDAD9", "#D7827E",
            "#56949F", "#B4637A", "#907AA9",
        ]),
    },
    Theme {
        name: ThemeName::Onedark,
        dark: p([
            "#21252B", "#282C34", "#2C323C", "#ABB2BF", "#828997", "#5C6370", "#3E4451", "#61AFEF",
            "#98C379", "#E06C75", "#C678DD",
        ]),
        light: p([
            "#F0F0F1", "#FAFAFA", "#E5E5E6", "#383A42", "#696C77", "#A0A1A7", "#DCDCDD", "#4078F2",
            "#50A14F", "#E45649", "#A626A4",
        ]),
    },
    Theme {
        name: ThemeName::Dracula,
        dark: p([
            "#21222C", "#282A36", "#343746", "#F8F8F2", "#C4C9D4", "#6272A4", "#44475A", "#BD93F9",
            "#50FA7B", "#FF5555", "#8BE9FD",
        ]),
        light: p([
            "#F5F1E1", "#FFFBEB", "#E8E3D0", "#1F1F1F", "#4A4636", "#6C664B", "#DED9C4", "#644AC9",
            "#14710A", "#CB3A2A", "#036A96",
        ]),
    },
    Theme {
        name: ThemeName::Nord,
        dark: p([
            "#2E3440", "#3B4252", "#434C5E", "#ECEFF4", "#D8DEE9", "#7B88A1", "#4C566A", "#88C0D0",
            "#A3BE8C", "#BF616A", "#81A1C1",
        ]),
        light: p([
            "#E5E9F0", "#ECEFF4", "#D8DEE9", "#2E3440", "#434C5E", "#6B7794", "#C8D0DE", "#5E81AC",
            "#56753F", "#A54A52", "#4C6E8F",
        ]),
    },
    Theme {
        name: ThemeName::Solarized,
        dark: p([
            "#002B36", "#073642", "#0E4450", "#93A1A1", "#839496", "#586E75", "#14505C", "#B58900",
            "#859900", "#DC322F", "#268BD2",
        ]),
        light: p([
            "#EEE8D5", "#FDF6E3", "#E4DCC7", "#586E75", "#657B83", "#93A1A1", "#DDD6C1", "#B58900",
            "#859900", "#DC322F", "#268BD2",
        ]),
    },
];

pub fn theme(name: ThemeName) -> &'static Theme {
    PALETTES
        .iter()
        .find(|t| t.name == name)
        .unwrap_or(&PALETTES[0])
}

pub fn palette(name: ThemeName, mode: Mode) -> Palette {
    let t = theme(name);
    match mode {
        Mode::Dark => t.dark,
        Mode::Light => t.light,
    }
}

/// A member's per-token overrides. The one open-ended part of a theme: any of
/// the eleven, and nothing else.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Overrides(pub BTreeMap<Token, String>);

impl Overrides {
    /// Parses the `member_settings.theme_overrides` JSON column.
    ///
    /// An unknown key is an error naming the valid tokens, not a silent skip —
    /// a typo'd override that quietly does nothing is worse than one that says
    /// so, because the member sees no change and has nothing to go on.
    pub fn from_json(raw: &str) -> Result<Overrides, String> {
        let map: BTreeMap<String, String> = serde_json::from_str(raw)
            .map_err(|e| format!("theme_overrides is not valid JSON: {e}"))?;
        let mut out = BTreeMap::new();
        for (k, v) in map {
            let token = Token::parse(&k).ok_or_else(|| {
                format!(
                    "`{k}` is not a theme token. Valid tokens: {}",
                    Token::ALL
                        .iter()
                        .map(|t| t.key())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            })?;
            if !is_css_color(&v) {
                return Err(format!("`{k}` is not a color: {v}"));
            }
            out.insert(token, v);
        }
        Ok(Overrides(out))
    }

    fn apply(&self, base: Palette) -> ResolvedPalette {
        let mut out = ResolvedPalette::from(base);
        for (token, value) in &self.0 {
            out.set(*token, value.clone());
        }
        out
    }
}

/// A palette with overrides folded in, so the values are owned.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedPalette {
    values: BTreeMap<Token, String>,
}

impl From<Palette> for ResolvedPalette {
    fn from(p: Palette) -> Self {
        ResolvedPalette {
            values: Token::ALL
                .into_iter()
                .map(|t| (t, p.get(t).to_string()))
                .collect(),
        }
    }
}

impl ResolvedPalette {
    pub fn get(&self, t: Token) -> &str {
        &self.values[&t]
    }
    fn set(&mut self, t: Token, v: String) {
        self.values.insert(t, v);
    }
}

/// `color-mix(in srgb, <c> <pct>%, <with>)` — the same call the mockup makes.
fn mix(c: &str, pct: u8, with: &str) -> String {
    format!("color-mix(in srgb, {c} {pct}%, {with})")
}

fn shadow_sm(mode: Mode) -> &'static str {
    match mode {
        Mode::Dark => "0 1px 2px rgba(0,0,0,.42)",
        Mode::Light => "0 1px 2px rgba(20,16,10,.07)",
    }
}

fn shadow_md(mode: Mode) -> &'static str {
    match mode {
        Mode::Dark => "0 2px 4px rgba(0,0,0,.34),0 14px 32px rgba(0,0,0,.44)",
        Mode::Light => "0 2px 4px rgba(20,16,10,.05),0 12px 28px rgba(20,16,10,.10)",
    }
}

/// The eleven tokens plus every derivation, as `(css-var-name, value)` pairs
/// ready to emit into a `<style>` block.
///
/// **This is the only place a derived color is defined.** The app's `<head>`
/// and the picker's preview both come through here, which is what makes it
/// impossible for the preview to show something the real thing would not.
pub fn derive(p: &ResolvedPalette, mode: Mode) -> Vec<(&'static str, String)> {
    let ink = p.get(Token::Ink);
    let surface_2 = p.get(Token::Surface2);
    let accent = p.get(Token::Accent);
    let surface_3 = mix(surface_2, 84, ink);

    vec![
        ("paper", p.get(Token::Paper).to_string()),
        ("surface", p.get(Token::Surface).to_string()),
        ("surface-2", surface_2.to_string()),
        ("surface-3", surface_3.clone()),
        ("ink", ink.to_string()),
        ("ink-2", p.get(Token::Ink2).to_string()),
        ("ink-3", p.get(Token::Ink3).to_string()),
        ("line", p.get(Token::Line).to_string()),
        ("line-2", mix(p.get(Token::Line), 72, ink)),
        ("accent", accent.to_string()),
        ("accent-2", mix(accent, 74, ink)),
        ("accent-soft", mix(accent, 13, "transparent")),
        ("accent-line", mix(accent, 38, "transparent")),
        ("pos", p.get(Token::Pos).to_string()),
        ("pos-soft", mix(p.get(Token::Pos), 15, "transparent")),
        ("neg", p.get(Token::Neg).to_string()),
        ("neg-soft", mix(p.get(Token::Neg), 14, "transparent")),
        ("info", p.get(Token::Info).to_string()),
        ("shadow-sm", shadow_sm(mode).to_string()),
        ("shadow-md", shadow_md(mode).to_string()),
        (
            "skeleton",
            format!("linear-gradient(90deg,{surface_2} 0%,{surface_3} 45%,{surface_2} 90%)"),
        ),
    ]
}

/// Both modes, plus whatever the cookie stamped.
///
/// Carrying both is not redundancy: `ModePref::System` means the server does
/// not know which one the device will use, so it must ship the pair and let
/// `prefers-color-scheme` choose. Shipping only the "current" one is how a
/// color ends up defined solely inside a media block.
pub struct RenderedTheme {
    pub light: Vec<(&'static str, String)>,
    pub dark: Vec<(&'static str, String)>,
    pub attr: Option<&'static str>,
}

pub fn render(name: ThemeName, pref: ModePref, over: &Overrides) -> RenderedTheme {
    let t = theme(name);
    RenderedTheme {
        light: derive(&over.apply(t.light), Mode::Light),
        dark: derive(&over.apply(t.dark), Mode::Dark),
        attr: pref.attr(),
    }
}

impl RenderedTheme {
    /// The full triple, exactly as `CLAUDE.md` mandates: the complete light
    /// palette on bare `:root`; the dark tokens under `prefers-color-scheme`
    /// guarded by `:not([data-theme="light"])` so an explicit light choice
    /// beats a dark OS; and again under `[data-theme="dark"]` so the toggle
    /// wins in the other direction too.
    pub fn to_css(&self) -> String {
        let block = |vars: &[(&'static str, String)]| -> String {
            vars.iter()
                .map(|(k, v)| format!("--{k}:{v};"))
                .collect::<String>()
        };
        format!(
            ":root{{color-scheme:light;{light}}}\n\
             @media (prefers-color-scheme: dark){{:root:not([data-theme=\"light\"]){{color-scheme:dark;{dark}}}}}\n\
             :root[data-theme=\"dark\"]{{color-scheme:dark;{dark}}}\n",
            light = block(&self.light),
            dark = block(&self.dark),
        )
    }

    /// `<meta name="theme-color">` — the color a phone paints its own bar in
    /// (Android's status bar, an installed app's title bar), set to `var`, the
    /// token of whatever the page's top edge is. The same pair-or-one rule as
    /// the CSS: both modes behind `media` for `system`, only the one the
    /// cookie chose otherwise. Ids, so a palette change can swap them out of
    /// band beside the `<style>` block.
    pub fn meta_tags(&self, var: &str) -> String {
        let pick = |vars: &[(&'static str, String)]| {
            vars.iter()
                .find(|(k, _)| *k == var)
                .map(|(_, v)| v.clone())
                .unwrap_or_default()
        };
        let tag = |id: &str, color: String, media: &str| {
            format!(r#"<meta name="theme-color" id="ffm-tc-{id}" content="{color}"{media}>"#)
        };
        match self.attr {
            Some("light") => tag("light", pick(&self.light), ""),
            Some(_) => tag("dark", pick(&self.dark), ""),
            None => {
                tag(
                    "light",
                    pick(&self.light),
                    r#" media="(prefers-color-scheme: light)""#,
                ) + &tag(
                    "dark",
                    pick(&self.dark),
                    r#" media="(prefers-color-scheme: dark)""#,
                )
            }
        }
    }
}

impl fmt::Display for RenderedTheme {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_css())
    }
}

fn is_css_color(v: &str) -> bool {
    let h = v.strip_prefix('#');
    matches!(h, Some(h) if matches!(h.len(), 3 | 4 | 6 | 8) && h.chars().all(|c| c.is_ascii_hexdigit()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Re-extracts the palette table from the mockup and compares it against
    /// the transcription above, hex string by hex string.
    ///
    /// This is the test that matters most in this file. The mockup is the
    /// normative visual spec; a palette that drifts from it is a whole theme
    /// rendering subtly wrong, in a way nobody notices until they switch to it.
    #[test]
    fn palette_table_matches_the_mockup() {
        let src = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/design/ffm-app-mockup.html"
        ))
        .expect("the mockup is the normative source for this table");

        let block = {
            let start = src.find("const THEMES = {").expect("THEMES table moved");
            let rest = &src[start..];
            &rest[..rest.find("\n};").expect("unterminated THEMES table")]
        };

        // `name: { dark: TH('#..',...), light: TH('#..',...) }`
        let mut found = 0;
        for theme in &PALETTES {
            let key = theme.name.key();
            let at = block
                .find(&format!("{key}: {{"))
                .unwrap_or_else(|| panic!("`{key}` is not in the mockup's THEMES table"));
            let entry = &block[at..];

            for (which, expected) in [("dark", theme.dark), ("light", theme.light)] {
                let marker = format!("{which}:");
                let s = entry.find(&marker).expect("missing mode");
                let open = entry[s..].find("TH(").expect("missing TH(") + s + 3;
                let close = entry[open..].find(')').expect("unterminated TH(") + open;
                let hexes: Vec<&str> = entry[open..close]
                    .split(',')
                    .map(|v| v.trim().trim_matches('\''))
                    .collect();

                assert_eq!(hexes.len(), 11, "{key}.{which}: a theme is 11 values");
                for (i, token) in Token::ALL.into_iter().enumerate() {
                    assert_eq!(
                        expected.get(token).to_ascii_lowercase(),
                        hexes[i].to_ascii_lowercase(),
                        "{key}.{which}.{} drifted from the mockup",
                        token.key()
                    );
                }
                found += 1;
            }
        }
        assert_eq!(found, 22, "11 themes x 2 modes");
    }

    #[test]
    fn every_theme_is_present_and_unique() {
        assert_eq!(PALETTES.len(), ThemeName::ALL.len());
        for name in ThemeName::ALL {
            assert_eq!(theme(name).name, name, "{} is missing", name.key());
        }
        let mut keys: Vec<&str> = ThemeName::ALL.iter().map(|t| t.key()).collect();
        keys.sort_unstable();
        let n = keys.len();
        keys.dedup();
        assert_eq!(keys.len(), n);
    }

    #[test]
    fn derivations_match_the_mockup_formulas() {
        let p = ResolvedPalette::from(palette(ThemeName::Ffm, Mode::Dark));
        let vars: std::collections::HashMap<_, _> = derive(&p, Mode::Dark).into_iter().collect();

        // surface-3 = mix(surface_2 84%, ink)
        assert_eq!(
            vars["surface-3"],
            "color-mix(in srgb, #2A261E 84%, #EEE8DB)"
        );
        assert_eq!(vars["line-2"], "color-mix(in srgb, #332E25 72%, #EEE8DB)");
        assert_eq!(vars["accent-2"], "color-mix(in srgb, #E9A63F 74%, #EEE8DB)");
        assert_eq!(
            vars["accent-soft"],
            "color-mix(in srgb, #E9A63F 13%, transparent)"
        );
        assert_eq!(
            vars["accent-line"],
            "color-mix(in srgb, #E9A63F 38%, transparent)"
        );
        assert_eq!(
            vars["pos-soft"],
            "color-mix(in srgb, #82B47D 15%, transparent)"
        );
        assert_eq!(
            vars["neg-soft"],
            "color-mix(in srgb, #E28470 14%, transparent)"
        );
        assert!(vars["skeleton"].starts_with("linear-gradient(90deg,#2A261E 0%,"));
    }

    #[test]
    fn a_theme_is_eleven_values_and_ten_derivations() {
        let p = ResolvedPalette::from(palette(ThemeName::Gruvbox, Mode::Light));
        let vars = derive(&p, Mode::Light);
        assert_eq!(vars.len(), 21, "11 tokens + 10 derived");
        // Every one of the eleven appears under its CSS name.
        for t in Token::ALL {
            assert!(
                vars.iter().any(|(k, _)| *k == t.css_var()),
                "{} missing from the emitted block",
                t.css_var()
            );
        }
    }

    #[test]
    fn no_color_is_defined_only_inside_a_media_block() {
        // The CLAUDE.md rule, asserted mechanically: everything in the dark
        // block must also exist in the bare `:root` light block, or a viewer
        // whose OS is light and whose cookie says nothing gets an undefined var.
        let r = render(ThemeName::Nord, ModePref::System, &Overrides::default());
        let css = r.to_css();

        let light_names: Vec<&str> = r.light.iter().map(|(k, _)| *k).collect();
        for (k, _) in &r.dark {
            assert!(light_names.contains(k), "--{k} is only defined in dark");
        }

        assert!(css.starts_with(":root{color-scheme:light;"));
        assert!(
            css.contains("@media (prefers-color-scheme: dark){:root:not([data-theme=\"light\"])")
        );
        assert!(css.contains(":root[data-theme=\"dark\"]"));
    }

    #[test]
    fn system_stamps_no_attribute() {
        // The common case: nothing stamped, so prefers-color-scheme decides.
        assert_eq!(
            render(ThemeName::Ffm, ModePref::System, &Overrides::default()).attr,
            None
        );
        assert_eq!(
            render(ThemeName::Ffm, ModePref::Dark, &Overrides::default()).attr,
            Some("dark")
        );
    }

    #[test]
    fn overrides_replace_only_what_they_name() {
        let over = Overrides::from_json(r##"{"accent":"#ff0000"}"##).unwrap();
        let r = render(ThemeName::Ffm, ModePref::Dark, &over);
        let dark: std::collections::HashMap<_, _> = r.dark.into_iter().collect();

        assert_eq!(dark["accent"], "#ff0000");
        // And the derivations follow it, which is the whole point of deriving
        // rather than storing twenty values.
        assert_eq!(dark["accent-2"], "color-mix(in srgb, #ff0000 74%, #EEE8DB)");
        assert_eq!(
            dark["accent-soft"],
            "color-mix(in srgb, #ff0000 13%, transparent)"
        );
        // Untouched tokens keep the palette's values.
        assert_eq!(dark["pos"], "#82B47D");
    }

    #[test]
    fn an_unknown_override_key_names_the_valid_ones() {
        let err = Overrides::from_json(r##"{"accnt":"#ff0000"}"##).unwrap_err();
        assert!(err.contains("accnt"), "{err}");
        assert!(err.contains("accent") && err.contains("surface_2"), "{err}");

        assert!(Overrides::from_json(r#"{"accent":"red"}"#).is_err());
        assert!(Overrides::from_json("not json").is_err());
        assert!(Overrides::from_json("{}").unwrap().0.is_empty());
    }

    #[test]
    fn theme_color_follows_the_mode_the_cookie_chose() {
        let light = palette(ThemeName::Ffm, Mode::Light).get(Token::Surface);
        let dark = palette(ThemeName::Ffm, Mode::Dark).get(Token::Surface);
        let none = Overrides::default();

        // `system`: both, each behind its own media query.
        let both = render(ThemeName::Ffm, ModePref::System, &none).meta_tags("surface");
        assert!(
            both.contains(&format!(
                r#"content="{light}" media="(prefers-color-scheme: light)""#
            )),
            "{both}"
        );
        assert!(
            both.contains(&format!(
                r#"content="{dark}" media="(prefers-color-scheme: dark)""#
            )),
            "{both}"
        );

        // A forced mode: only that one, with no media to lose to the OS.
        let forced = render(ThemeName::Ffm, ModePref::Dark, &none).meta_tags("surface");
        assert_eq!(
            forced,
            format!(r#"<meta name="theme-color" id="ffm-tc-dark" content="{dark}">"#)
        );
    }
}
