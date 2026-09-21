//! `member_settings` — the personal tier.
//!
//! One row per member, created in the same transaction as the member, so no
//! read path ever handles a missing row. The defaults in the schema exist for
//! columns, not for the absence of a row.
//!
//! Only the fields the rendered page needs today are loaded. `:profile` (the
//! settings phase) is what grows this to the whole row; adding them before
//! anything reads them would just be columns nobody can check.

use crate::ui::i18n::Lang;
use crate::ui::theme::{Overrides, ThemeName};
use sqlx::SqlitePool;

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
}

impl Default for MemberSettings {
    fn default() -> Self {
        MemberSettings {
            language: Lang::En,
            theme: ThemeName::default(),
            theme_overrides: Overrides::default(),
            line_numbers: "relative".into(),
        }
    }
}

pub async fn member_settings(pool: &SqlitePool, member_id: i64) -> sqlx::Result<MemberSettings> {
    let row = sqlx::query!(
        r#"SELECT language AS "language!: String",
                  theme_name AS "theme_name!: String",
                  theme_overrides AS "theme_overrides!: String",
                  line_numbers AS "line_numbers!: String"
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
        })
        .unwrap_or_default())
}
