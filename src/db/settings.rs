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

use crate::ui::i18n::Lang;
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
    LayoutMaxWidth,
    LayoutGap,
    LedgerSort,
    LedgerSearchScope,
    LedgerShowDelta,
    LedgerGroup,
    GroupFoldDefault,
    DefaultVisibility,
}

impl MemberKey {
    pub fn parse(s: &str) -> Option<MemberKey> {
        Some(match s {
            "language" => MemberKey::Language,
            "theme_name" => MemberKey::ThemeName,
            "line_numbers" => MemberKey::LineNumbers,
            "layout_panes" => MemberKey::LayoutPanes,
            "layout_min_split_width" => MemberKey::LayoutMinSplitWidth,
            "layout_max_width" => MemberKey::LayoutMaxWidth,
            "layout_gap" => MemberKey::LayoutGap,
            "ledger_sort" => MemberKey::LedgerSort,
            "ledger_search_scope" => MemberKey::LedgerSearchScope,
            "ledger_show_delta" => MemberKey::LedgerShowDelta,
            "ledger_group" => MemberKey::LedgerGroup,
            "group_fold_default" => MemberKey::GroupFoldDefault,
            "default_visibility" => MemberKey::DefaultVisibility,
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
    /// what lets `/login` honour your light/dark without knowing who you are.
    pub theme: ThemeName,
    pub theme_overrides: Overrides,
    pub line_numbers: String,
    pub layout_panes: String,
    pub layout_min_split_width: i64,
    pub layout_max_width: i64,
    pub layout_gap: i64,
    pub ledger_sort: String,
    pub ledger_search_scope: String,
    pub ledger_show_delta: bool,
    pub ledger_group: String,
    pub group_fold_default: String,
    /// `None` = follow the household's policy, which is a real third state and
    /// is rendered as one: three radios, the first labelled with what it
    /// currently resolves to.
    pub default_visibility: Option<String>,
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
            layout_max_width: 900,
            layout_gap: 16,
            ledger_sort: "manual".into(),
            ledger_search_scope: "pane".into(),
            ledger_show_delta: true,
            ledger_group: "none".into(),
            group_fold_default: "expanded".into(),
            default_visibility: None,
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
                  layout_max_width AS "layout_max_width!: i64",
                  layout_gap AS "layout_gap!: i64",
                  ledger_sort AS "ledger_sort!: String",
                  ledger_search_scope AS "ledger_search_scope!: String",
                  ledger_show_delta AS "ledger_show_delta!: i64",
                  ledger_group AS "ledger_group!: String",
                  group_fold_default AS "group_fold_default!: String",
                  default_visibility AS "default_visibility: String"
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
            layout_max_width: r.layout_max_width,
            layout_gap: r.layout_gap,
            ledger_sort: r.ledger_sort,
            ledger_search_scope: r.ledger_search_scope,
            ledger_show_delta: r.ledger_show_delta != 0,
            ledger_group: r.ledger_group,
            group_fold_default: r.group_fold_default,
            default_visibility: r.default_visibility,
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
    pub key: &'static str,
    pub problem: String,
}

impl std::fmt::Display for Invalid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "✗ {} {}", self.key, self.problem)
    }
}

fn one_of(key: &'static str, value: &str, allowed: &[&str]) -> Result<String, Invalid> {
    if allowed.contains(&value) {
        Ok(value.to_string())
    } else {
        Err(Invalid {
            key,
            problem: format!("must be one of {}", allowed.join(" · ")),
        })
    }
}

fn in_range(key: &'static str, value: &str, lo: i64, hi: i64) -> Result<i64, Invalid> {
    match value.trim().parse::<i64>() {
        Ok(n) if (lo..=hi).contains(&n) => Ok(n),
        _ => Err(Invalid {
            key,
            problem: format!("must be a number between {lo} and {hi}"),
        }),
    }
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
        problem: e.to_string(),
    };

    match key {
        MemberKey::Language => {
            let v = one_of("language", value, &["en", "pt-BR"])?;
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
            let v = ThemeName::parse(value).ok_or(Invalid {
                key: "theme",
                problem: "is not a bundled palette".into(),
            })?;
            let v = v.key();
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
            let v = one_of("layout.panes", value, &["auto", "split", "tabs"])?;
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
            let v = in_range("layout.min_split_width", value, 320, 4000)?;
            sqlx::query!(
                "UPDATE member_settings SET layout_min_split_width = ?2 WHERE member_id = ?1",
                member_id,
                v
            )
            .execute(pool)
            .await
            .map_err(db)?;
        }
        MemberKey::LayoutMaxWidth => {
            let v = in_range("layout.max_width", value, 0, 4000)?;
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
            let v = in_range("layout.gap", value, 0, 64)?;
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
            let v = one_of("ledger.sort", value, &["manual", "date", "amount"])?;
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
            let v = one_of("ledger.group", value, &["none", "bucket"])?;
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
}

impl HouseholdKey {
    pub fn parse(s: &str) -> Option<HouseholdKey> {
        Some(match s {
            "name" => HouseholdKey::Name,
            "locale_currency" | "currency" => HouseholdKey::Currency,
            "default_language" => HouseholdKey::DefaultLanguage,
            "default_visibility" => HouseholdKey::DefaultVisibility,
            "ledger_show_recurring" | "show_recurring" => HouseholdKey::ShowRecurring,
            _ => return None,
        })
    }
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
        problem: e.to_string(),
    };
    let at = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();

    match key {
        HouseholdKey::Name => {
            let v = value.trim();
            if v.is_empty() {
                return Err(Invalid {
                    key: "household name",
                    problem: "cannot be empty".into(),
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
                problem: "is not one we format".into(),
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
            let v = one_of("default_language", value, &["en", "pt-BR"])?;
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
    }
    Ok(())
}

#[cfg(test)]
mod tests {
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
            "layout_max_width",
            "layout_gap",
            "ledger_sort",
            "ledger_search_scope",
            "ledger_show_delta",
            "ledger_group",
            "group_fold_default",
            "default_visibility",
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
