//! The dev-only UI sandbox.
//!
//! Renders every row state against pure fixtures so UI and CSS can be iterated
//! without a database — and, per `CLAUDE.md`, it is how the row-state table
//! stays honest: a status with no fixture fails `cargo test`.
//!
//! `#![cfg(debug_assertions)]` at the module top, plus `#[cfg]` on the `mod`
//! declaration, means none of this exists in a release binary. That is stronger
//! than an auth guard, because there is no guard to forget.
//!
//! **No handler here takes a database pool, and none ever should.** From the
//! next phase there will be one, and the temptation is to seed a test database
//! — but the value of "iterate the UI without touching the real data" vanishes
//! the moment a migration is needed to look at a row.

pub mod fixtures;

use crate::domain::money::Currency;
use crate::ui::i18n::{Catalog, Lang};
use crate::ui::row::{LedgerRow, RowCtx, RowView};
use crate::ui::theme::{self, ModePref, ThemeName};
use crate::web::template::Html;
use askama::Template;
use axum::Router;
use axum::extract::Query;
use axum::response::{IntoResponse, Redirect};
use axum::routing::get;
use chrono::NaiveDate;
use serde::Deserialize;

/// Generic over the app state so it can be merged into the real router while
/// taking none of it. That is the "no handler here takes a pool" rule as a type
/// signature rather than as a comment.
pub fn router<S>() -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    Router::new()
        .route("/__sandbox", get(index))
        .route("/__sandbox/rows", get(rows))
        .route("/__sandbox/themes", get(themes))
        .route("/__sandbox/catalog", get(catalog))
        .route("/__sandbox/picker", get(picker))
}

// ---------------------------------------------------------------------------
// context
// ---------------------------------------------------------------------------

/// One URL describes one exact rendering, so a specimen can be linked to and
/// screenshot-diffed.
#[derive(Deserialize, Clone)]
pub struct Ctx {
    #[serde(default)]
    pub lang: LangSel,
    #[serde(default)]
    pub theme: String,
    #[serde(default)]
    pub mode: String,
    #[serde(default)]
    pub currency: String,
    /// Pinned by default — see `fixtures::SANDBOX_TODAY`.
    #[serde(default)]
    pub today: Option<String>,
    #[serde(default)]
    pub nu: String,
    #[serde(default)]
    pub narrow: Option<u8>,
    #[serde(default)]
    pub delta: Option<u8>,
    #[serde(default)]
    pub recurring: Option<u8>,
}

/// `both` is the default, not a debug toggle: a message that only reads
/// correctly in English is found by *looking*, and side by side makes that one
/// glance instead of two tab switches. It is also the cheapest way to catch a
/// pt-BR string blowing out a fixed-width control.
#[derive(Deserialize, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum LangSel {
    #[default]
    Both,
    En,
    #[serde(rename = "pt-BR")]
    PtBr,
}

impl LangSel {
    fn langs(self) -> Vec<Lang> {
        match self {
            LangSel::Both => vec![Lang::En, Lang::PtBr],
            LangSel::En => vec![Lang::En],
            LangSel::PtBr => vec![Lang::PtBr],
        }
    }
    fn code(self) -> &'static str {
        match self {
            LangSel::Both => "both",
            LangSel::En => "en",
            LangSel::PtBr => "pt-BR",
        }
    }
}

impl Ctx {
    fn theme_name(&self) -> ThemeName {
        ThemeName::parse(&self.theme).unwrap_or_default()
    }
    fn mode(&self) -> ModePref {
        if self.mode.is_empty() {
            ModePref::System
        } else {
            ModePref::parse(&self.mode)
        }
    }
    fn currency(&self) -> Currency {
        Currency::parse(&self.currency).unwrap_or(Currency::Brl)
    }
    fn today(&self) -> NaiveDate {
        self.today
            .as_deref()
            .and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
            .unwrap_or_else(fixtures::default_today)
    }
    fn row_ctx(&self) -> RowCtx {
        RowCtx {
            today: self.today(),
            sort: Default::default(),
            currency: self.currency(),
            show_recurring: self.recurring != Some(0),
            show_delta: self.delta != Some(0),
            narrow: self.narrow == Some(1),
            line_numbers: crate::ui::row::LineNumbers::parse(self.nu_code()),
        }
    }
    fn theme_css(&self) -> String {
        theme::render(self.theme_name(), self.mode(), &Default::default()).to_css()
    }
    fn attr(&self) -> Option<&'static str> {
        self.mode().attr()
    }
    /// Rebuilds the query string with one key changed, for the chrome bar's
    /// links. Plain `<a>` elements: the sandbox must never depend on the
    /// machinery it exists to test.
    fn with(&self, key: &str, value: impl AsRef<str>) -> String {
        let value = value.as_ref();
        let mut parts: Vec<(String, String)> = vec![
            ("lang".into(), self.lang.code().into()),
            ("theme".into(), self.theme_name().key().into()),
            ("mode".into(), self.mode().code().into()),
            ("currency".into(), self.currency().code().into()),
            ("nu".into(), self.nu_code().into()),
        ];
        if self.narrow == Some(1) {
            parts.push(("narrow".into(), "1".into()));
        }
        if self.delta == Some(0) {
            parts.push(("delta".into(), "0".into()));
        }
        if self.recurring == Some(0) {
            parts.push(("recurring".into(), "0".into()));
        }
        for p in parts.iter_mut() {
            if p.0 == key {
                p.1 = value.into();
            }
        }
        if !parts.iter().any(|p| p.0 == key) {
            parts.push((key.into(), value.into()));
        }
        let q: Vec<String> = parts.iter().map(|(k, v)| format!("{k}={v}")).collect();
        format!("?{}", q.join("&"))
    }
    fn nu_code(&self) -> &str {
        match self.nu.as_str() {
            "absolute" | "none" => &self.nu,
            _ => "relative",
        }
    }
}

// ---------------------------------------------------------------------------
// pages
// ---------------------------------------------------------------------------

async fn index() -> impl IntoResponse {
    Redirect::temporary("/__sandbox/rows")
}

/// One rendered specimen in one language.
pub struct Specimen {
    pub lang: &'static str,
    pub view: RowView,
}

pub struct SpecBlock {
    pub n: String,
    pub id: &'static str,
    pub heading: &'static str,
    pub why: &'static str,
    pub status: String,
    pub specimens: Vec<Specimen>,
}

#[derive(Template)]
#[template(path = "sandbox/rows.html")]
struct RowsPage {
    ctx: Ctx,
    theme_css: String,
    attr: Option<&'static str>,
    t: Catalog,
    specs: Vec<SpecBlock>,
    ledger: Vec<Specimen>,
    themes: Vec<(&'static str, &'static str, bool)>,
    langs: Vec<(&'static str, &'static str, bool)>,
    modes: Vec<(&'static str, &'static str, bool)>,
    currencies: Vec<(&'static str, &'static str, bool)>,
    nus: Vec<(&'static str, &'static str, bool)>,
}

async fn rows(Query(ctx): Query<Ctx>) -> impl IntoResponse {
    let rctx = ctx.row_ctx();
    let langs = ctx.lang.langs();

    let specs = fixtures::specs()
        .into_iter()
        .enumerate()
        .map(|(i, s)| {
            let row = (s.build)(rctx);
            SpecBlock {
                n: format!("{:02}", i + 1),
                id: s.id,
                heading: s.heading,
                why: s.why,
                status: row.status().slug().to_string(),
                specimens: langs
                    .iter()
                    .map(|l| Specimen {
                        lang: l.code(),
                        view: apply_gutter(&ctx, row.clone()).view(&Catalog::new(*l)),
                    })
                    .collect(),
            }
        })
        .collect();

    let ledger = fixtures::ledger(rctx)
        .into_iter()
        .map(|r| Specimen {
            lang: langs[0].code(),
            view: apply_gutter(&ctx, r).view(&Catalog::new(langs[0])),
        })
        .collect();

    Html(RowsPage {
        theme_css: ctx.theme_css(),
        attr: ctx.attr(),
        t: Catalog::new(langs[0]),
        specs,
        ledger,
        themes: ThemeName::ALL
            .iter()
            .map(|t| (t.key(), t.label(), *t == ctx.theme_name()))
            .collect(),
        langs: [("both", "both"), ("en", "en"), ("pt-BR", "pt-BR")]
            .iter()
            .map(|(k, l)| (*k, *l, *k == ctx.lang.code()))
            .collect(),
        modes: [("system", "system"), ("light", "light"), ("dark", "dark")]
            .iter()
            .map(|(k, l)| (*k, *l, *k == ctx.mode().code()))
            .collect(),
        currencies: Currency::ALL
            .iter()
            .map(|c| (c.code(), c.code(), *c == ctx.currency()))
            .collect(),
        nus: [("relative", "rnu"), ("absolute", "nu"), ("none", "nonu")]
            .iter()
            .map(|(k, l)| (*k, *l, *k == ctx.nu_code()))
            .collect(),
        ctx,
    })
}

fn apply_gutter(ctx: &Ctx, mut row: LedgerRow) -> LedgerRow {
    use crate::ui::row::Gutter;
    row.gutter = match ctx.nu_code() {
        "none" => Gutter::Hidden,
        "absolute" => Gutter::Absolute(3),
        _ => row.gutter,
    };
    row
}

#[derive(Template)]
#[template(path = "sandbox/themes.html")]
struct ThemesPage {
    theme_css: String,
    attr: Option<&'static str>,
    swatches: Vec<Swatch>,
}

pub struct Swatch {
    pub label: &'static str,
    pub mode: &'static str,
    pub css: String,
    pub chips: Vec<String>,
}

async fn themes(Query(ctx): Query<Ctx>) -> impl IntoResponse {
    let mut swatches = Vec::new();
    for name in ThemeName::ALL {
        for (mode, pref) in [("light", ModePref::Light), ("dark", ModePref::Dark)] {
            let r = theme::render(name, pref, &Default::default());
            let vars = if mode == "dark" { &r.dark } else { &r.light };
            swatches.push(Swatch {
                label: name.label(),
                mode,
                css: vars
                    .iter()
                    .map(|(k, v)| format!("--{k}:{v};"))
                    .collect::<String>(),
                chips: ["paper", "surface-2", "ink", "accent", "pos", "neg", "info"]
                    .iter()
                    .map(|k| (*k).to_string())
                    .collect(),
            });
        }
    }
    Html(ThemesPage {
        theme_css: ctx.theme_css(),
        attr: ctx.attr(),
        swatches,
    })
}

#[derive(Template)]
#[template(path = "sandbox/catalog.html")]
struct CatalogPage {
    theme_css: String,
    attr: Option<&'static str>,
    ids: Vec<&'static str>,
    count: usize,
}

async fn catalog(Query(ctx): Query<Ctx>) -> impl IntoResponse {
    Html(CatalogPage {
        theme_css: ctx.theme_css(),
        attr: ctx.attr(),
        ids: crate::ui::i18n::MESSAGE_IDS.to_vec(),
        count: crate::ui::i18n::MESSAGE_IDS.len(),
    })
}

#[derive(Template)]
#[template(path = "sandbox/picker.html")]
struct PickerPage {
    theme_css: String,
    attr: Option<&'static str>,
    /// `(lang, rendered popup)` — the real `partials/month_pop.html`, so the
    /// sandbox cannot drift from what `GET /months` answers with.
    pops: Vec<(&'static str, String)>,
}

/// The month picker with every cell state at once: the month on screen,
/// today's month, a deficit month, a projected month, a flagged month and the
/// future — plus the year's foot. Numbers are fixtures; nothing is summed.
async fn picker(Query(ctx): Query<Ctx>) -> impl IntoResponse {
    use crate::db::ledger::Unresolved;
    use crate::domain::money::Cents;
    use crate::domain::period::Period;
    use crate::web::ledger::{MonthPop, month_cells, year_summary};

    let today = ctx.today();
    let now = Period::of(today);
    let showing = now.pred_n(2);
    let flagged = [Unresolved {
        period: now.pred_n(4),
        count: 2,
    }];
    let currency = ctx.currency();
    let nets: [i64; 12] = [
        1_950_000, 1_812_000, -340_000, 1_700_000, 85_000, 1_690_000, 1_640_000, 1_700_000,
        2_210_000, 0, 0, 0,
    ];

    let pops = ctx
        .lang
        .langs()
        .into_iter()
        .map(|lang| {
            let t = Catalog::new(lang);
            let mut months = month_cells(&t, &flagged, showing, today);
            let mut counted = Vec::new();
            let (mut ins, mut outs) = (Cents::ZERO, Cents::ZERO);
            for (i, cell) in months.iter_mut().enumerate().filter(|(_, c)| !c.future) {
                let net = Cents(nets[i]);
                let compact = crate::ui::fmt::money_compact(net, currency);
                // The month on screen stands in for a varying income's month.
                cell.net = Some(if cell.current {
                    format!("~{compact}")
                } else {
                    compact
                });
                cell.negative = net.is_negative();
                ins += Cents(net.get().max(0) + 1_000_000);
                outs += Cents(1_000_000 - net.get().min(0));
                if let Some(p) = Period::parse(&cell.key) {
                    counted.push(p);
                }
            }
            let summary = year_summary(&t, &months, &counted, ins, outs, true, currency);
            let pop = MonthPop {
                year: showing.year(),
                months,
                summary,
                t,
            };
            let code = if lang == Lang::En { "en" } else { "pt-BR" };
            (code, pop.render().unwrap_or_default())
        })
        .collect();

    Html(PickerPage {
        theme_css: ctx.theme_css(),
        attr: ctx.attr(),
        pops,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::status::RowStatus;

    fn ctx() -> RowCtx {
        Ctx {
            lang: LangSel::Both,
            theme: String::new(),
            mode: String::new(),
            currency: String::new(),
            today: None,
            nu: String::new(),
            narrow: None,
            delta: None,
            recurring: None,
        }
        .row_ctx()
    }

    /// Every `RowStatus` variant must appear in some fixture.
    ///
    /// This is what keeps the row-state table honest: adding a status without a
    /// specimen fails here rather than quietly going unrendered forever.
    #[test]
    fn every_row_status_has_a_fixture() {
        let c = ctx();
        let covered: Vec<RowStatus> = fixtures::specs()
            .iter()
            .map(|s| (s.build)(c).status())
            .collect();

        for status in RowStatus::ALL {
            assert!(
                covered.contains(&status),
                "no sandbox fixture renders `{}` — add one to fixtures::specs()",
                status.slug()
            );
        }
    }

    /// And every lifecycle state, which the status enum does not cover.
    #[test]
    fn skeleton_error_and_placeholder_are_all_rendered() {
        let c = ctx();
        let states: Vec<&'static str> = fixtures::specs()
            .iter()
            .map(|s| (s.build)(c).state.slug())
            .collect();
        for want in ["skeleton", "error", "placeholder"] {
            assert!(states.contains(&want), "no fixture renders `{want}`");
        }
    }

    /// Household content is not app copy.
    ///
    /// "Água · Sabesp" and "Mesada" are things a household typed; they must
    /// never pass through the translation catalog. This asserts it mechanically
    /// rather than by convention.
    #[test]
    fn seed_data_never_leaks_into_the_catalog() {
        let c = ctx();
        let en = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/i18n/en.ftl"));
        let pt = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/i18n/pt-BR.ftl"));

        // Scan message VALUES, not raw bytes. A comment may legitimately name
        // the sample data in order to say it does not belong here — which is
        // exactly what tripped this assertion the first time it ran, and the
        // same trap build.rs's forbidden-content scan documents.
        let values = |src: &str| -> String {
            src.lines()
                .filter(|l| !l.trim_start().starts_with('#'))
                .collect::<Vec<_>>()
                .join("\n")
        };
        let en = values(en);
        let pt = values(pt);

        for spec in fixtures::specs() {
            let title = (spec.build)(c).title;
            // A placeholder row has no title at all — its label is app copy
            // rendered from the catalog. An empty needle matches everything,
            // so skipping it is required rather than lenient.
            if title.is_empty() {
                continue;
            }
            for (name, src) in [("en.ftl", &en), ("pt-BR.ftl", &pt)] {
                assert!(
                    !src.contains(&title),
                    "fixture title {title:?} appears in {name} — household content is not app copy"
                );
            }
        }
    }

    #[test]
    fn every_fixture_renders_without_panicking() {
        // The view is where every method gets exercised at once; a fixture that
        // cannot render is a broken specimen nobody would notice in a gallery.
        let c = ctx();
        for spec in fixtures::specs() {
            let v = (spec.build)(c).view(&Catalog::new(Lang::En));
            assert!(!v.classes.is_empty() || !v.title.is_empty(), "{}", spec.id);
        }
    }

    #[test]
    fn spec_ids_are_unique() {
        let mut ids: Vec<&str> = fixtures::specs().iter().map(|s| s.id).collect();
        let n = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), n, "duplicate fixture id");
    }
}
