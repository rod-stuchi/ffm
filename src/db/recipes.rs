//! A rule's **recipe**: how its bills are read — taught by the household's
//! assistant on the rule's first bill, approved by a person, and from then on
//! applied locally every month with no assistant at all (migration 0016).
//!
//! History is kept: storing a new recipe retires the live one rather than
//! overwriting it, so how an earlier month was read stays answerable.

use crate::db::changes::{Actor, Change, RuleAction, Value};
use crate::services::assistant::Recipe;
use sqlx::SqlitePool;

/// The log's note when a recipe is kept — `bill recipe taught by codex`.
/// Stored in English like every note; `ui::log::note_text` words it.
pub const TAUGHT_BY: &str = "bill recipe taught by ";

/// A recipe as stored.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RuleRecipe {
    pub id: i64,
    pub rule_id: i64,
    /// `codex` | `claude` | `gemini` | `migrated` | `manual`.
    pub source: String,
    pub created_at: String,
    pub cnpj: Option<String>,
    pub issuer_name: Option<String>,
    pub amount_regex: Option<String>,
    pub due_regex: Option<String>,
    pub client_regex: Option<String>,
    pub card_regex: Option<String>,
}

impl RuleRecipe {
    /// This recipe as the one-issuer profile the import pipeline reads a bill
    /// with — the rule's own, never a global list.
    pub fn profiles(&self, kind: Option<&str>) -> crate::services::issuers::Profiles {
        crate::services::issuers::Profiles {
            issuers: vec![crate::services::issuers::Issuer::from_recipe(
                self.cnpj.as_deref(),
                self.issuer_name.as_deref(),
                kind,
                self.amount_regex.as_deref(),
                self.due_regex.as_deref(),
                self.client_regex.as_deref(),
                self.card_regex.as_deref(),
            )],
        }
    }
}

/// The rule's live recipe, if it has been taught one.
pub async fn live(pool: &SqlitePool, rule_id: i64) -> Option<RuleRecipe> {
    sqlx::query!(
        r#"SELECT id AS "id!: i64", rule_id AS "rule_id!: i64", source AS "source!: String",
                  created_at AS "created_at!: String",
                  cnpj, issuer_name, amount_regex, due_regex, client_regex, card_regex
           FROM rule_recipe WHERE rule_id = ?1 AND retired_at IS NULL"#,
        rule_id
    )
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()
    .map(|r| RuleRecipe {
        id: r.id,
        rule_id: r.rule_id,
        source: r.source,
        created_at: r.created_at,
        cnpj: r.cnpj,
        issuer_name: r.issuer_name,
        amount_regex: r.amount_regex,
        due_regex: r.due_regex,
        client_regex: r.client_regex,
        card_regex: r.card_regex,
    })
}

/// Stores an approved recipe as the rule's live one, retiring whatever was
/// live before, and binds the rule to its CNPJ where nothing is bound yet.
/// Logged on the rule as an edit — the patterns themselves are plumbing, the
/// log says who taught it and which issuer it reads.
pub async fn store(
    pool: &SqlitePool,
    actor: Actor,
    rule_id: i64,
    source: &str,
    recipe: &Recipe,
) -> sqlx::Result<i64> {
    let mut tx = pool.begin().await?;
    let at = crate::clock::stamp();
    let by = match actor {
        Actor::User(id) | Actor::Import(Some(id)) => Some(id),
        _ => None,
    };
    let old: Option<Option<String>> = sqlx::query_scalar!(
        "SELECT issuer_name FROM rule_recipe WHERE rule_id = ?1 AND retired_at IS NULL",
        rule_id
    )
    .fetch_optional(&mut *tx)
    .await?;
    sqlx::query!(
        "UPDATE rule_recipe SET retired_at = ?2 WHERE rule_id = ?1 AND retired_at IS NULL",
        rule_id,
        at
    )
    .execute(&mut *tx)
    .await?;
    let id = sqlx::query_scalar!(
        r#"INSERT INTO rule_recipe
               (rule_id, created_at, created_by, source, cnpj, issuer_name,
                amount_regex, due_regex, client_regex, card_regex)
           VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
           RETURNING id AS "id!: i64""#,
        rule_id,
        at,
        by,
        source,
        recipe.cnpj,
        recipe.issuer_name,
        recipe.amount_regex,
        recipe.due_regex,
        recipe.client_regex,
        recipe.card_regex,
    )
    .fetch_one(&mut *tx)
    .await?;
    // The CNPJ is the rule's issuer binding — filled only where empty, like
    // everything a bill binds. Only on a rule the binding belongs to (a spend
    // with a barcode), where 0007's trigger allows it.
    sqlx::query!(
        "UPDATE recurring_rules SET issuer_id = coalesce(issuer_id, ?2)
         WHERE id = ?1 AND kind = 'out' AND no_barcode = 0",
        rule_id,
        recipe.cnpj
    )
    .execute(&mut *tx)
    .await?;
    Change::rule(actor, RuleAction::Edit, rule_id)
        .always(
            "recipe",
            Value::Text(old.flatten()),
            Value::Text(recipe.issuer_name.clone().or(Some(source.to_string()))),
        )
        .note(format!("{TAUGHT_BY}{source}"))
        .write(&mut tx)
        .await?;
    tx.commit().await?;
    Ok(id)
}

/// The last `config/issuers.toml` shipped, frozen — see the file's header.
const RETIRED: &str = include_str!("retired_issuers.toml");

/// What the one-time conversion did — for the boot log.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Converted {
    pub recipes: usize,
    pub unbound: u64,
}

/// **Retires `config/issuers.toml`** — once, at startup, a no-op after.
///
/// Every rule still bound to one of the file's profiles by its id
/// (`nubank-cartao`) gets that profile as a `migrated` recipe, and is re-bound
/// by the profile's CNPJ — the issuer binding is a CNPJ since 0016. A rule
/// bound to an id no CNPJ can be read from (a placeholder profile) is unbound
/// from it, since nothing could ever compare against it again. The file
/// itself is left where it is: the operator's, to delete.
pub async fn convert_profiles(
    pool: &SqlitePool,
    path: &std::path::Path,
) -> sqlx::Result<Converted> {
    let mut done = Converted::default();
    // The operator's copy when there is one — it may hold their own edits —
    // else the last one shipped, frozen, so a checkout whose file is already
    // gone still converts.
    let profiles = if path.exists() {
        crate::services::issuers::Profiles::load(path).unwrap_or_default()
    } else {
        toml::from_str(RETIRED).unwrap_or_default()
    };
    for p in &profiles.issuers {
        let digits: String = p
            .detect
            .first()
            .map(|d| d.chars().filter(char::is_ascii_digit).collect())
            .unwrap_or_default();
        let cnpj = (digits.len() == 14).then_some(digits);
        let bound: Vec<i64> = sqlx::query_scalar!(
            r#"SELECT id AS "id!: i64" FROM recurring_rules WHERE issuer_id = ?1"#,
            p.id
        )
        .fetch_all(pool)
        .await?;
        for rule_id in bound {
            if let Some(cnpj) = &cnpj
                && live(pool, rule_id).await.is_none()
            {
                let recipe = Recipe {
                    issuer_name: Some(p.name.clone()),
                    cnpj: Some(cnpj.clone()),
                    amount_regex: p.amount.as_ref().map(|r| r.regex.clone()),
                    due_regex: p.due_date.as_ref().map(|r| r.regex.clone()),
                    client_regex: p.client_code.as_ref().map(|r| r.regex.clone()),
                    card_regex: p.card.as_ref().map(|r| r.regex.clone()),
                };
                store(pool, Actor::System, rule_id, "migrated", &recipe).await?;
                done.recipes += 1;
            }
            sqlx::query!(
                "UPDATE recurring_rules SET issuer_id = ?2 WHERE id = ?1",
                rule_id,
                cnpj
            )
            .execute(pool)
            .await?;
        }
        sqlx::query!(
            "UPDATE transactions SET issuer_id = ?2 WHERE issuer_id = ?1",
            p.id,
            cnpj
        )
        .execute(pool)
        .await?;
    }
    // Whatever is left bound to something that is not a CNPJ can never be
    // compared again — unbound from it, rather than asked about forever.
    done.unbound = sqlx::query!(
        "UPDATE recurring_rules SET issuer_id = NULL
         WHERE issuer_id IS NOT NULL
           AND (length(issuer_id) <> 14 OR issuer_id GLOB '*[^0-9]*')"
    )
    .execute(pool)
    .await?
    .rows_affected();
    Ok(done)
}
