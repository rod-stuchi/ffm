//! `member_settings` — the personal tier.
//!
//! One row per member, created in the same transaction as the member, so no
//! read path ever handles a missing row. The defaults in the schema exist for
//! columns, not for the absence of a row.
//!
//! **Every setting is written by one function, and validated there.** `:set`,
//! `:sort`, `:theme`, a clicked statusline cell and the `:profile` pane are
//! five gestures writing *one row* — which is what the single-source-of-truth
//! rule actually asks for. The thing it forbids is a second, pending copy, and
//! that is why these panes have no Save button: every control writes on
//! `change`, so there is no moment when the form and the row disagree.

use crate::ui::i18n::{Catalog, Lang};
use crate::ui::theme::{Overrides, ThemeName};
use sqlx::SqlitePool;

/// One editable setting: the column, and the value as the form sends it.
///
/// A closed enum rather than a `&str` key, because the alternative is a
/// handler taking a column name from a form and putting it into SQL.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum MemberKey {
    Language,
    ThemeName,
    LineNumbers,
    LayoutPanes,
    LayoutMinSplitWidth,
    LayoutMinWidth,
    LayoutMaxWidth,
    LayoutGap,
    LedgerSort,
    LedgerSearchScope,
    LedgerShowDelta,
    LedgerGroup,
    GroupFoldDefault,
    GroupOrder,
    DefaultVisibility,
    Density,
    PdfView,
}

impl MemberKey {
    pub fn parse(s: &str) -> Option<MemberKey> {
        Some(match s {
            "language" => MemberKey::Language,
            "theme_name" => MemberKey::ThemeName,
            "line_numbers" => MemberKey::LineNumbers,
            "layout_panes" => MemberKey::LayoutPanes,
            "layout_min_split_width" => MemberKey::LayoutMinSplitWidth,
            "layout_min_width" => MemberKey::LayoutMinWidth,
            "layout_max_width" => MemberKey::LayoutMaxWidth,
            "layout_gap" => MemberKey::LayoutGap,
            "ledger_sort" => MemberKey::LedgerSort,
            "ledger_search_scope" => MemberKey::LedgerSearchScope,
            "ledger_show_delta" => MemberKey::LedgerShowDelta,
            "ledger_group" => MemberKey::LedgerGroup,
            "group_fold_default" => MemberKey::GroupFoldDefault,
            "group_order" => MemberKey::GroupOrder,
            "default_visibility" => MemberKey::DefaultVisibility,
            "density" => MemberKey::Density,
            "pdf_view" => MemberKey::PdfView,
            _ => return None,
        })
    }
}

#[derive(Clone, Debug)]
pub struct MemberSettings {
    /// Words only. Dates stay dd/mm/yyyy and money follows the household's
    /// currency regardless of what this says.
    pub language: Lang,
    /// The palette. Light-versus-dark is **not** here: that is a property of
    /// the device, so it lives in the `ffm.theme` cookie. Keeping them apart is
    /// what lets `/login` honor your light/dark without knowing who you are.
    pub theme: ThemeName,
    pub theme_overrides: Overrides,
    pub line_numbers: String,
    pub layout_panes: String,
    pub layout_min_split_width: i64,
    /// The floor a pane will not be squeezed below while it shares the stage
    /// with another; 0 = no floor, mirroring `layout_max_width`'s 0.
    pub layout_min_width: i64,
    pub layout_max_width: i64,
    pub layout_gap: i64,
    pub ledger_sort: String,
    pub ledger_search_scope: String,
    pub ledger_show_delta: bool,
    pub ledger_group: String,
    pub group_fold_default: String,
    /// How `:group bucket` orders its groups: `appearance` | `name` |
    /// `subtotal`. `Uncategorized` is last in all three.
    pub group_order: String,
    /// `None` = follow the household's policy, which is a real third state and
    /// is rendered as one: three radios, the first labeled with what it
    /// currently resolves to.
    pub default_visibility: Option<String>,
    /// How much room the ledger's rows take: `default` (the roomiest — a
    /// value, Gmail's name for it, never "follow the household") |
    /// `comfortable` | `compact`. Applied in CSS off one attribute.
    pub density: String,
    /// How `P` first shows a PDF: `png` (the server's page images) | `pdf`
    /// (the browser's own viewer). `v` flips it while the carousel is open.
    pub pdf_view: String,
}

impl Default for MemberSettings {
    fn default() -> Self {
        MemberSettings {
            language: Lang::En,
            theme: ThemeName::default(),
            theme_overrides: Overrides::default(),
            line_numbers: "relative".into(),
            layout_panes: "auto".into(),
            layout_min_split_width: 900,
            layout_min_width: 420,
            layout_max_width: 900,
            layout_gap: 16,
            ledger_sort: "manual".into(),
            ledger_search_scope: "pane".into(),
            ledger_show_delta: true,
            ledger_group: "none".into(),
            group_fold_default: "expanded".into(),
            group_order: "appearance".into(),
            default_visibility: None,
            density: "comfortable".into(),
            pdf_view: "png".into(),
        }
    }
}

pub async fn member_settings(pool: &SqlitePool, member_id: i64) -> sqlx::Result<MemberSettings> {
    let row = sqlx::query!(
        r#"SELECT language AS "language!: String",
                  theme_name AS "theme_name!: String",
                  theme_overrides AS "theme_overrides!: String",
                  line_numbers AS "line_numbers!: String",
                  layout_panes AS "layout_panes!: String",
                  layout_min_split_width AS "layout_min_split_width!: i64",
                  layout_min_width AS "layout_min_width!: i64",
                  layout_max_width AS "layout_max_width!: i64",
                  layout_gap AS "layout_gap!: i64",
                  ledger_sort AS "ledger_sort!: String",
                  ledger_search_scope AS "ledger_search_scope!: String",
                  ledger_show_delta AS "ledger_show_delta!: i64",
                  ledger_group AS "ledger_group!: String",
                  group_fold_default AS "group_fold_default!: String",
                  group_order AS "group_order!: String",
                  default_visibility AS "default_visibility: String",
                  density AS "density!: String",
                  pdf_view AS "pdf_view!: String"
           FROM member_settings WHERE member_id = ?1"#,
        member_id
    )
    .fetch_optional(pool)
    .await?;

    Ok(row
        .map(|r| MemberSettings {
            // Every one of these is a closed set with a CHECK behind it, so an
            // unparseable value means the schema was bypassed. Falling back
            // renders the ledger in the wrong theme; refusing would not render
            // it at all.
            language: Lang::parse(&r.language).unwrap_or(Lang::En),
            theme: ThemeName::parse(&r.theme_name).unwrap_or_default(),
            theme_overrides: Overrides::from_json(&r.theme_overrides).unwrap_or_default(),
            line_numbers: r.line_numbers,
            layout_panes: r.layout_panes,
            layout_min_split_width: r.layout_min_split_width,
            layout_min_width: r.layout_min_width,
            layout_max_width: r.layout_max_width,
            layout_gap: r.layout_gap,
            ledger_sort: r.ledger_sort,
            ledger_search_scope: r.ledger_search_scope,
            ledger_show_delta: r.ledger_show_delta != 0,
            ledger_group: r.ledger_group,
            group_fold_default: r.group_fold_default,
            group_order: r.group_order,
            default_visibility: r.default_visibility,
            density: r.density,
            pdf_view: r.pdf_view,
        })
        .unwrap_or_default())
}

/// Why a setting was refused, in the `✗ …` dialect the pane's footer speaks.
///
/// Every constraint here is also a `CHECK` in the schema, so a bad value
/// cannot reach the table even if a handler forgets — this exists so the
/// member sees a sentence rather than a constraint violation.
#[derive(Debug, PartialEq, Eq)]
pub struct Invalid {
    /// The setting, as the identifier a member types back (`ledger.sort`) —
    /// never translated, like the values it may take.
    pub key: &'static str,
    pub problem: Problem,
}

/// What was wrong with a value. Worded by the catalog where a member reads it
/// (`Invalid::say`); `Display` stays English, for logs and tests.
#[derive(Debug, PartialEq, Eq)]
pub enum Problem {
    /// Not one of these values, joined ` · `.
    OneOf(String),
    Between(i64, i64),
    NotAPalette,
    Empty,
    NotACurrency,
    NotAnAssistant,
    /// The database refused it.
    Storage(String),
}

impl std::fmt::Display for Invalid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let problem = match &self.problem {
            Problem::OneOf(allowed) => format!("must be one of {allowed}"),
            Problem::Between(lo, hi) => format!("must be a number between {lo} and {hi}"),
            Problem::NotAPalette => "is not a bundled palette".into(),
            Problem::Empty => "cannot be empty".into(),
            Problem::NotACurrency => "is not one we format".into(),
            Problem::NotAnAssistant => "is not one this app runs".into(),
            Problem::Storage(e) => format!("could not be saved — {e}"),
        };
        write!(f, "✗ {} {problem}", self.key)
    }
}

impl Invalid {
    /// The refusal in the member's language — the key and the values it may
    /// take stay as typed, the words around them are translated.
    pub fn say(&self, t: &Catalog) -> String {
        let key = self.key;
        match &self.problem {
            Problem::OneOf(allowed) => t.settings_invalid_one_of(key, allowed),
            Problem::Between(lo, hi) => {
                t.settings_invalid_between(key, &lo.to_string(), &hi.to_string())
            }
            Problem::NotAPalette => t.settings_invalid_palette(key),
            Problem::Empty => t.settings_invalid_empty(key),
            Problem::NotACurrency => t.settings_invalid_currency(key),
            Problem::NotAnAssistant => t.settings_invalid_assistant(key),
            Problem::Storage(e) => t.settings_invalid_storage(key, e),
        }
    }
}

/// The closed sets a setting accepts. Public because the `:` bar completes
/// against the same lists (`web::commands`) — a completion offering a value
/// the write then refuses would be the hint lying all over again.
pub const LANGUAGES: [&str; 2] = ["en", "pt-BR"];
pub const LEDGER_SORTS: [&str; 4] = ["manual", "name", "amount", "date"];
pub const LEDGER_GROUPS: [&str; 2] = ["none", "bucket"];
/// Roomiest first, the order `:profile` shows them in.
pub const DENSITIES: [&str; 3] = ["default", "comfortable", "compact"];

fn one_of(key: &'static str, value: &str, allowed: &[&str]) -> Result<String, Invalid> {
    if allowed.contains(&value) {
        Ok(value.to_string())
    } else {
        Err(Invalid {
            key,
            problem: Problem::OneOf(allowed.join(" · ")),
        })
    }
}

fn in_range(key: &'static str, value: &str, lo: i64, hi: i64) -> Result<i64, Invalid> {
    match value.trim().parse::<i64>() {
        Ok(n) if (lo..=hi).contains(&n) => Ok(n),
        _ => Err(Invalid {
            key,
            problem: Problem::Between(lo, hi),
        }),
    }
}

// One validator per value that a member sets for themselves AND the household
// sets as the default a new member starts with. Both writes call the same
// function, so a bound can never differ between `:profile` and `:household`.

fn theme_value(value: &str) -> Result<&'static str, Invalid> {
    ThemeName::parse(value).map(ThemeName::key).ok_or(Invalid {
        key: "theme",
        problem: Problem::NotAPalette,
    })
}

fn panes_value(value: &str) -> Result<String, Invalid> {
    one_of("layout.panes", value, &["auto", "split", "tabs"])
}

fn min_split_value(value: &str) -> Result<i64, Invalid> {
    in_range("layout.min_split_width", value, 320, 4000)
}

/// The upper bound is the same 4000 every layout value uses. The lower one is
/// 0 rather than a usable minimum: 0 means "no floor at all", which is a real
/// choice — the same one `layout_max_width` spells as "unlimited".
fn min_width_value(value: &str) -> Result<i64, Invalid> {
    in_range("layout.min_width", value, 0, 4000)
}

fn max_width_value(value: &str) -> Result<i64, Invalid> {
    in_range("layout.max_width", value, 0, 4000)
}

fn gap_value(value: &str) -> Result<i64, Invalid> {
    in_range("layout.gap", value, 0, 64)
}

/// Days a removed file is kept before `:prune` offers it. 0 is "forever" —
/// what the app did before there was a limit — and ten years is the most: a
/// number past it is a typo, not a policy. The schema's CHECK says the same.
fn trash_keep_value(value: &str) -> Result<i64, Invalid> {
    in_range("trash.keep_days", value, 0, 3650)
}

/// Writes one member setting, validated.
///
/// The value arrives as text because a form sends text; it leaves as the right
/// type or as a refusal. The field keeps its previous value on a refusal —
/// which falls out of not writing it.
pub async fn set_member(
    pool: &SqlitePool,
    member_id: i64,
    key: MemberKey,
    value: &str,
) -> Result<(), Invalid> {
    let db = |e: sqlx::Error| Invalid {
        key: "setting",
        problem: Problem::Storage(e.to_string()),
    };

    match key {
        MemberKey::Language => {
            let v = one_of("language", value, &LANGUAGES)?;
            sqlx::query!(
                "UPDATE member_settings SET language = ?2 WHERE member_id = ?1",
                member_id,
                v
            )
            .execute(pool)
            .await
            .map_err(db)?;
        }
        MemberKey::ThemeName => {
            let v = theme_value(value)?;
            sqlx::query!(
                "UPDATE member_settings SET theme_name = ?2 WHERE member_id = ?1",
                member_id,
                v
            )
            .execute(pool)
            .await
            .map_err(db)?;
        }
        MemberKey::LineNumbers => {
            let v = one_of("line_numbers", value, &["relative", "absolute", "none"])?;
            sqlx::query!(
                "UPDATE member_settings SET line_numbers = ?2 WHERE member_id = ?1",
                member_id,
                v
            )
            .execute(pool)
            .await
            .map_err(db)?;
        }
        MemberKey::LayoutPanes => {
            let v = panes_value(value)?;
            sqlx::query!(
                "UPDATE member_settings SET layout_panes = ?2 WHERE member_id = ?1",
                member_id,
                v
            )
            .execute(pool)
            .await
            .map_err(db)?;
        }
        MemberKey::LayoutMinSplitWidth => {
            let v = min_split_value(value)?;
            sqlx::query!(
                "UPDATE member_settings SET layout_min_split_width = ?2 WHERE member_id = ?1",
                member_id,
                v
            )
            .execute(pool)
            .await
            .map_err(db)?;
        }
        MemberKey::LayoutMinWidth => {
            let v = min_width_value(value)?;
            sqlx::query!(
                "UPDATE member_settings SET layout_min_width = ?2 WHERE member_id = ?1",
                member_id,
                v
            )
            .execute(pool)
            .await
            .map_err(db)?;
        }
        MemberKey::LayoutMaxWidth => {
            let v = max_width_value(value)?;
            sqlx::query!(
                "UPDATE member_settings SET layout_max_width = ?2 WHERE member_id = ?1",
                member_id,
                v
            )
            .execute(pool)
            .await
            .map_err(db)?;
        }
        MemberKey::LayoutGap => {
            let v = gap_value(value)?;
            sqlx::query!(
                "UPDATE member_settings SET layout_gap = ?2 WHERE member_id = ?1",
                member_id,
                v
            )
            .execute(pool)
            .await
            .map_err(db)?;
        }
        MemberKey::LedgerSort => {
            let v = one_of("ledger.sort", value, &LEDGER_SORTS)?;
            sqlx::query!(
                "UPDATE member_settings SET ledger_sort = ?2 WHERE member_id = ?1",
                member_id,
                v
            )
            .execute(pool)
            .await
            .map_err(db)?;
        }
        MemberKey::LedgerSearchScope => {
            let v = one_of("ledger.search_scope", value, &["pane", "household"])?;
            sqlx::query!(
                "UPDATE member_settings SET ledger_search_scope = ?2 WHERE member_id = ?1",
                member_id,
                v
            )
            .execute(pool)
            .await
            .map_err(db)?;
        }
        MemberKey::LedgerShowDelta => {
            let v = i64::from(is_on(value));
            sqlx::query!(
                "UPDATE member_settings SET ledger_show_delta = ?2 WHERE member_id = ?1",
                member_id,
                v
            )
            .execute(pool)
            .await
            .map_err(db)?;
        }
        MemberKey::LedgerGroup => {
            let v = one_of("ledger.group", value, &LEDGER_GROUPS)?;
            sqlx::query!(
                "UPDATE member_settings SET ledger_group = ?2 WHERE member_id = ?1",
                member_id,
                v
            )
            .execute(pool)
            .await
            .map_err(db)?;
        }
        MemberKey::GroupFoldDefault => {
            let v = one_of("group_fold_default", value, &["expanded", "collapsed"])?;
            sqlx::query!(
                "UPDATE member_settings SET group_fold_default = ?2 WHERE member_id = ?1",
                member_id,
                v
            )
            .execute(pool)
            .await
            .map_err(db)?;
        }
        MemberKey::GroupOrder => {
            let v = one_of("group_order", value, &["appearance", "name", "subtotal"])?;
            sqlx::query!(
                "UPDATE member_settings SET group_order = ?2 WHERE member_id = ?1",
                member_id,
                v
            )
            .execute(pool)
            .await
            .map_err(db)?;
        }
        MemberKey::Density => {
            let v = one_of("density", value, &DENSITIES)?;
            sqlx::query!(
                "UPDATE member_settings SET density = ?2 WHERE member_id = ?1",
                member_id,
                v
            )
            .execute(pool)
            .await
            .map_err(db)?;
        }
        MemberKey::PdfView => {
            let v = one_of("pdf_view", value, &["png", "pdf"])?;
            sqlx::query!(
                "UPDATE member_settings SET pdf_view = ?2 WHERE member_id = ?1",
                member_id,
                v
            )
            .execute(pool)
            .await
            .map_err(db)?;
        }
        MemberKey::DefaultVisibility => {
            // The empty string is the third state, not a missing one: NULL
            // means "follow the household", which is a choice a member makes.
            let v = match value {
                "" | "household_default" => None,
                other => Some(one_of("privacy.default", other, &["household", "private"])?),
            };
            sqlx::query!(
                "UPDATE member_settings SET default_visibility = ?2 WHERE member_id = ?1",
                member_id,
                v
            )
            .execute(pool)
            .await
            .map_err(db)?;
        }
    }
    Ok(())
}

/// A checkbox sends `on` when ticked and nothing at all when not, so presence
/// is the value — but a radio or a command may send a word instead.
fn is_on(value: &str) -> bool {
    matches!(value, "on" | "1" | "true" | "yes")
}

// ---------------------------------------------------------------------------
// the household tier
// ---------------------------------------------------------------------------

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum HouseholdKey {
    Name,
    Currency,
    DefaultLanguage,
    DefaultVisibility,
    ShowRecurring,
    // What a new member's own settings start as — see `MemberDefaults`.
    DefaultTheme,
    DefaultPanes,
    DefaultMinSplitWidth,
    DefaultMinWidth,
    DefaultMaxWidth,
    DefaultGap,
    DefaultDensity,
    /// Which CLI on the server teaches a rule how to read its bills.
    Assistant,
    /// How many days a removed file stays in the trash before `:prune`
    /// offers it for removal for good; 0 keeps it forever.
    TrashKeepDays,
}

impl HouseholdKey {
    pub fn parse(s: &str) -> Option<HouseholdKey> {
        Some(match s {
            "name" => HouseholdKey::Name,
            "locale_currency" | "currency" => HouseholdKey::Currency,
            "default_language" => HouseholdKey::DefaultLanguage,
            "default_visibility" => HouseholdKey::DefaultVisibility,
            "ledger_show_recurring" | "show_recurring" => HouseholdKey::ShowRecurring,
            "default_theme" | "default_theme_name" => HouseholdKey::DefaultTheme,
            "default_layout_panes" => HouseholdKey::DefaultPanes,
            "default_layout_min_split_width" => HouseholdKey::DefaultMinSplitWidth,
            "default_layout_min_width" => HouseholdKey::DefaultMinWidth,
            "default_layout_max_width" => HouseholdKey::DefaultMaxWidth,
            "default_layout_gap" => HouseholdKey::DefaultGap,
            "default_density" => HouseholdKey::DefaultDensity,
            "assistant" => HouseholdKey::Assistant,
            "trash_keep_days" => HouseholdKey::TrashKeepDays,
            _ => return None,
        })
    }

    /// Whether writing this key changes what is on anybody's screen. The
    /// new-member defaults do not — they are read only when somebody joins —
    /// so they write quietly, with no refresh.
    pub fn changes_the_screen(self) -> bool {
        !matches!(
            self,
            HouseholdKey::DefaultTheme
                | HouseholdKey::DefaultPanes
                | HouseholdKey::DefaultMinSplitWidth
                | HouseholdKey::DefaultMinWidth
                | HouseholdKey::DefaultMaxWidth
                | HouseholdKey::DefaultGap
                | HouseholdKey::DefaultDensity
                // Read by `:prune` when it runs, and by nothing on screen.
                | HouseholdKey::TrashKeepDays
        )
    }
}

/// What a new member's palette and layout start as. The household **seeds**
/// these into the member's own row when they join and never reads them for
/// that member again: changing one changes nobody's current screen. A member
/// adopts them later only by asking ([`adopt_member_defaults`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemberDefaults {
    pub theme: ThemeName,
    pub panes: String,
    pub min_split: i64,
    pub min_width: i64,
    pub max_width: i64,
    pub gap: i64,
    pub density: String,
}

impl Default for MemberDefaults {
    /// The schema's own column defaults — what a household that never set
    /// any hands out, and what its members had before these existed.
    fn default() -> Self {
        MemberDefaults {
            theme: ThemeName::default(),
            panes: "auto".into(),
            min_split: 900,
            min_width: 420,
            max_width: 900,
            gap: 16,
            density: "comfortable".into(),
        }
    }
}

impl MemberDefaults {
    /// Whether this member's own values already are the household's — when
    /// they are, `:profile` has nothing to offer.
    pub fn matches(&self, s: &MemberSettings) -> bool {
        self.theme == s.theme
            && self.panes == s.layout_panes
            && self.min_split == s.layout_min_split_width
            && self.min_width == s.layout_min_width
            && self.max_width == s.layout_max_width
            && self.gap == s.layout_gap
            && self.density == s.density
    }
}

/// The household's defaults for a new member. Takes any executor so the
/// invite's accept can read them inside its own transaction.
pub async fn member_defaults<'e, E>(ex: E) -> sqlx::Result<MemberDefaults>
where
    E: sqlx::SqliteExecutor<'e>,
{
    let row = sqlx::query!(
        r#"SELECT default_theme AS "theme!: String",
                  default_layout_panes AS "panes!: String",
                  default_layout_min_split_width AS "min_split!: i64",
                  default_layout_min_width AS "min_width!: i64",
                  default_layout_max_width AS "max_width!: i64",
                  default_layout_gap AS "gap!: i64",
                  default_density AS "density!: String"
           FROM household_settings WHERE id = 1"#
    )
    .fetch_optional(ex)
    .await?;
    Ok(row
        .map(|r| MemberDefaults {
            theme: ThemeName::parse(&r.theme).unwrap_or_default(),
            panes: r.panes,
            min_split: r.min_split,
            min_width: r.min_width,
            max_width: r.max_width,
            gap: r.gap,
            density: r.density,
        })
        .unwrap_or_default())
}

/// `:profile`'s "Use the household's defaults": the household's palette and
/// layout, copied into this member's own row — which stays theirs afterwards.
pub async fn adopt_member_defaults(pool: &SqlitePool, member_id: i64) -> sqlx::Result<()> {
    let d = member_defaults(pool).await?;
    let theme = d.theme.key();
    sqlx::query!(
        r#"UPDATE member_settings
           SET theme_name = ?2, layout_panes = ?3, layout_min_split_width = ?4,
               layout_min_width = ?5, layout_max_width = ?6, layout_gap = ?7,
               density = ?8
           WHERE member_id = ?1"#,
        member_id,
        theme,
        d.panes,
        d.min_split,
        d.min_width,
        d.max_width,
        d.gap,
        d.density
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// The assistant the household chose — `none` when the row is missing, which
/// is the safe answer: nothing leaves the machine.
pub async fn assistant(pool: &SqlitePool) -> crate::services::assistant::Provider {
    use crate::services::assistant::Provider;
    sqlx::query_scalar!(
        r#"SELECT assistant AS "assistant!: String" FROM household_settings WHERE id = 1"#
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()
    .and_then(|a| Provider::parse(&a))
    .unwrap_or(Provider::None)
}

/// How long a removed file stays in the trash before `:prune` offers it for
/// removal for good, in days; 0 keeps it forever. 90 when the row is missing —
/// the column's own default.
pub async fn trash_keep_days(pool: &SqlitePool) -> i64 {
    sqlx::query_scalar!(
        r#"SELECT trash_keep_days AS "days!: i64" FROM household_settings WHERE id = 1"#
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()
    .unwrap_or(90)
}

/// Writes one household setting. **Admin only** — enforced by the caller,
/// because "may this viewer" is a question about the session, not about the
/// value.
pub async fn set_household(
    pool: &SqlitePool,
    key: HouseholdKey,
    value: &str,
) -> Result<(), Invalid> {
    let db = |e: sqlx::Error| Invalid {
        key: "setting",
        problem: Problem::Storage(e.to_string()),
    };
    let at = crate::clock::stamp();

    match key {
        HouseholdKey::Name => {
            let v = value.trim();
            if v.is_empty() {
                return Err(Invalid {
                    key: "household.name",
                    problem: Problem::Empty,
                });
            }
            sqlx::query!(
                "UPDATE household_settings SET name = ?1, updated_at = ?2 WHERE id = 1",
                v,
                at
            )
            .execute(pool)
            .await
            .map_err(db)?;
        }
        HouseholdKey::Currency => {
            // Picking a currency picks its whole number format as one preset.
            let v = crate::domain::money::Currency::parse(value).ok_or(Invalid {
                key: "currency",
                problem: Problem::NotACurrency,
            })?;
            let v = v.code();
            sqlx::query!(
                "UPDATE household_settings SET locale_currency = ?1, updated_at = ?2 WHERE id = 1",
                v,
                at
            )
            .execute(pool)
            .await
            .map_err(db)?;
        }
        HouseholdKey::DefaultLanguage => {
            let v = one_of("default_language", value, &LANGUAGES)?;
            sqlx::query!(
                "UPDATE household_settings SET default_language = ?1, updated_at = ?2 WHERE id = 1",
                v,
                at
            )
            .execute(pool)
            .await
            .map_err(db)?;
        }
        HouseholdKey::DefaultVisibility => {
            let v = one_of("default_visibility", value, &["household", "private"])?;
            sqlx::query!("UPDATE household_settings SET default_visibility = ?1, updated_at = ?2 WHERE id = 1", v, at)
                .execute(pool).await.map_err(db)?;
        }
        HouseholdKey::ShowRecurring => {
            let v = i64::from(is_on(value));
            sqlx::query!("UPDATE household_settings SET ledger_show_recurring = ?1, updated_at = ?2 WHERE id = 1", v, at)
                .execute(pool).await.map_err(db)?;
        }
        // The new-member defaults, through the same validators a member's own
        // setting goes through — one bound per value, whoever writes it.
        HouseholdKey::DefaultTheme => {
            let v = theme_value(value)?;
            sqlx::query!(
                "UPDATE household_settings SET default_theme = ?1, updated_at = ?2 WHERE id = 1",
                v,
                at
            )
            .execute(pool)
            .await
            .map_err(db)?;
        }
        HouseholdKey::DefaultPanes => {
            let v = panes_value(value)?;
            sqlx::query!("UPDATE household_settings SET default_layout_panes = ?1, updated_at = ?2 WHERE id = 1", v, at)
                .execute(pool).await.map_err(db)?;
        }
        HouseholdKey::DefaultMinSplitWidth => {
            let v = min_split_value(value)?;
            sqlx::query!("UPDATE household_settings SET default_layout_min_split_width = ?1, updated_at = ?2 WHERE id = 1", v, at)
                .execute(pool).await.map_err(db)?;
        }
        HouseholdKey::DefaultMinWidth => {
            let v = min_width_value(value)?;
            sqlx::query!("UPDATE household_settings SET default_layout_min_width = ?1, updated_at = ?2 WHERE id = 1", v, at)
                .execute(pool).await.map_err(db)?;
        }
        HouseholdKey::DefaultMaxWidth => {
            let v = max_width_value(value)?;
            sqlx::query!("UPDATE household_settings SET default_layout_max_width = ?1, updated_at = ?2 WHERE id = 1", v, at)
                .execute(pool).await.map_err(db)?;
        }
        HouseholdKey::DefaultGap => {
            let v = gap_value(value)?;
            sqlx::query!("UPDATE household_settings SET default_layout_gap = ?1, updated_at = ?2 WHERE id = 1", v, at)
                .execute(pool).await.map_err(db)?;
        }
        HouseholdKey::TrashKeepDays => {
            let v = trash_keep_value(value)?;
            sqlx::query!(
                "UPDATE household_settings SET trash_keep_days = ?1, updated_at = ?2 WHERE id = 1",
                v,
                at
            )
            .execute(pool)
            .await
            .map_err(db)?;
        }
        HouseholdKey::Assistant => {
            let v = crate::services::assistant::Provider::parse(value)
                .ok_or(Invalid {
                    key: "assistant",
                    problem: Problem::NotAnAssistant,
                })?
                .code();
            sqlx::query!(
                "UPDATE household_settings SET assistant = ?1, updated_at = ?2 WHERE id = 1",
                v,
                at
            )
            .execute(pool)
            .await
            .map_err(db)?;
        }
        HouseholdKey::DefaultDensity => {
            let v = one_of("density", value, &DENSITIES)?;
            sqlx::query!(
                "UPDATE household_settings SET default_density = ?1, updated_at = ?2 WHERE id = 1",
                v,
                at
            )
            .execute(pool)
            .await
            .map_err(db)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    /// The key and the values stay as typed; the words around them are the
    /// member's.
    #[test]
    fn a_refusal_is_said_in_the_members_language() {
        let pt = Catalog::new(Lang::PtBr);
        let e = one_of("ledger.sort", "sideways", &["manual", "date"]).unwrap_err();
        assert_eq!(
            e.say(&pt),
            "✗ ledger.sort precisa ser um destes: manual · date"
        );
        let e = in_range("layout.gap", "900", 0, 64).unwrap_err();
        assert_eq!(
            e.say(&pt),
            "✗ layout.gap precisa ser um número entre 0 e 64"
        );
    }

    use super::*;

    #[test]
    fn a_refusal_names_the_setting_and_what_would_be_valid() {
        let e = one_of("ledger.sort", "sideways", &["manual", "date", "amount"]).unwrap_err();
        assert_eq!(
            e.to_string(),
            "✗ ledger.sort must be one of manual · date · amount"
        );

        let e = in_range("layout.gap", "900", 0, 64).unwrap_err();
        assert_eq!(
            e.to_string(),
            "✗ layout.gap must be a number between 0 and 64"
        );
        assert!(in_range("layout.gap", "16", 0, 64).is_ok());
        assert!(in_range("layout.gap", "", 0, 64).is_err());
    }

    #[test]
    fn every_key_a_form_can_send_is_one_the_writer_knows() {
        for k in [
            "language",
            "theme_name",
            "line_numbers",
            "layout_panes",
            "layout_min_split_width",
            "layout_min_width",
            "layout_max_width",
            "layout_gap",
            "ledger_sort",
            "ledger_search_scope",
            "ledger_show_delta",
            "ledger_group",
            "group_fold_default",
            "group_order",
            "default_visibility",
            "density",
            "pdf_view",
        ] {
            assert!(MemberKey::parse(k).is_some(), "{k}");
        }
        // And a column name that is not a setting cannot be spelled — which is
        // the point of the enum: the alternative is a handler putting a form
        // field straight into SQL.
        assert!(MemberKey::parse("password_hash").is_none());
        assert!(MemberKey::parse("role").is_none());
    }

    #[test]
    fn the_household_accepts_its_two_aliases() {
        // `:currency usd` and the form's own field name are the same setting.
        assert_eq!(
            HouseholdKey::parse("currency"),
            Some(HouseholdKey::Currency)
        );
        assert_eq!(
            HouseholdKey::parse("locale_currency"),
            Some(HouseholdKey::Currency)
        );
        assert_eq!(
            HouseholdKey::parse("show_recurring"),
            Some(HouseholdKey::ShowRecurring)
        );
        assert!(
            HouseholdKey::parse("db_path").is_none(),
            "the operator's, not the admin's"
        );
    }
}
