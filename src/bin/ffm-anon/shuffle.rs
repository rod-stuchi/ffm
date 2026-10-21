//! The shuffle itself: every name, amount and identifier in the copy replaced,
//! consistently, in one transaction.
//!
//! Everything is read first, so each map sees the originals; then written
//! back row by row, in id order, so a seed always gives the same copy.

use crate::barcode;
use crate::ids::{self, IdMap};
use crate::money::{AmountMap, Scope};
use crate::names::{Names, Pool};
use crate::paths::PathMap;
use anyhow::Context;
use chrono::NaiveDate;
use ffm::domain::money::Currency;
use ffm::domain::slug::slugify;
use ffm::services::boleto;
use rand::SeedableRng;
use rand::rngs::StdRng;
use sqlx::SqliteConnection;
use std::collections::{HashMap, HashSet};

/// The patterns a recipe gets in the copy: they read the stand-in bills
/// `vault` writes, so recipes keep working on the fake files.
pub const AMOUNT_RE: &str = r"Total a pagar\s+\D*([\d.,]+)";
pub const DUE_RE: &str = r"Vencimento\s+(\d{2}/\d{2}/\d{4})";
pub const CLIENT_RE: &str = r"Cliente\s+(\S+)";
pub const CARD_RE: &str = r"Cartao final\s+(\d{4})";

/// Who taught a recipe — an identifier, not household content.
const ASSISTANTS: &[&str] = &["codex", "claude", "gemini", "migrated", "manual", "none"];

struct Member {
    id: i64,
    name: String,
    slug: String,
}

struct Rule {
    id: i64,
    kind: String,
    title: String,
    bucket: Option<String>,
    bill_name: Option<String>,
    instance: Option<String>,
    amount: Option<i64>,
    client_code: Option<String>,
    issuer_id: Option<String>,
    company: Option<String>,
    card: Option<String>,
    pdf_password: Option<String>,
}

#[derive(Clone)]
struct Tx {
    id: i64,
    rule_id: Option<i64>,
    period: String,
    kind: String,
    title: String,
    bucket: Option<String>,
    bill_name: Option<String>,
    amount: Option<i64>,
    barcode: Option<String>,
    issuer_id: Option<String>,
    due_date: Option<String>,
}

struct Receipt {
    id: i64,
    tx: i64,
    kind: String,
    path: String,
    ext: String,
    deleted_from: Option<String>,
}

struct Change {
    entity: String,
    entity_id: i64,
    rule_id: Option<i64>,
    period: Option<String>,
}

struct Field {
    id: i64,
    change_id: i64,
    field: String,
    old: Option<String>,
    new: Option<String>,
    kind: String,
}

/// Every original string the copy must not contain, for the leak check.
pub struct Originals {
    /// Typed text: titles, buckets, bill names, people.
    pub text: Vec<String>,
    /// Identifiers: CNPJs, account numbers, barcodes.
    pub numbers: Vec<String>,
}

pub struct Shuffler {
    pub rng: StdRng,
    pub currency: Currency,
    pub money: AmountMap,
    pub names: Names,
    pub paths: PathMap,
    pub cnpj: IdMap,
    pub client: IdMap,
    pub company: IdMap,
    pub card: IdMap,
    barcodes: HashMap<String, String>,
    pub used_barcodes: HashSet<String>,
    forbidden_ids: HashSet<String>,
    /// Each receipt's original path — only ever to ask the real vault whether
    /// the file exists.
    pub original_paths: HashMap<i64, String>,
    household: String,
    members: Vec<Member>,
    rules: Vec<Rule>,
    txs: Vec<Tx>,
    receipts: Vec<Receipt>,
    changes: HashMap<i64, Change>,
    fields: Vec<Field>,
    originals_text: HashSet<String>,
}

/// Drops every guard trigger except the search index's, and hands back their
/// SQL to put them back verbatim. The change log is append-only in the
/// household's database; this is a copy whose log is being rewritten.
pub async fn drop_guards(conn: &mut SqliteConnection) -> anyhow::Result<Vec<(String, String)>> {
    let triggers: Vec<(String, String)> = sqlx::query_as(
        "SELECT name, sql FROM sqlite_master WHERE type = 'trigger' AND name NOT LIKE 'transactions_fts_%' ORDER BY name",
    )
    .fetch_all(&mut *conn)
    .await?;
    for (name, _) in &triggers {
        sqlx::query(&format!("DROP TRIGGER \"{name}\""))
            .execute(&mut *conn)
            .await?;
    }
    Ok(triggers)
}

pub async fn restore_guards(
    conn: &mut SqliteConnection,
    triggers: &[(String, String)],
) -> anyhow::Result<()> {
    for (_, sql) in triggers {
        sqlx::query(sql).execute(&mut *conn).await?;
    }
    Ok(())
}

pub async fn finish(conn: &mut SqliteConnection) -> anyhow::Result<()> {
    sqlx::query("INSERT INTO transactions_fts (transactions_fts) VALUES ('rebuild')")
        .execute(&mut *conn)
        .await?;
    let fk: Vec<(String, i64)> =
        sqlx::query_as("SELECT \"table\", rowid FROM pragma_foreign_key_check")
            .fetch_all(&mut *conn)
            .await?;
    anyhow::ensure!(fk.is_empty(), "foreign keys broken in the copy: {fk:?}");
    let ok: String = sqlx::query_scalar("PRAGMA integrity_check")
        .fetch_one(&mut *conn)
        .await?;
    anyhow::ensure!(ok == "ok", "integrity check failed: {ok}");
    Ok(())
}

pub async fn summary(conn: &mut SqliteConnection, files: usize) -> anyhow::Result<crate::Summary> {
    let members: Vec<String> =
        sqlx::query_scalar("SELECT name FROM members WHERE deleted_at IS NULL ORDER BY id")
            .fetch_all(&mut *conn)
            .await?;
    let transactions = sqlx::query_scalar("SELECT count(*) FROM transactions")
        .fetch_one(&mut *conn)
        .await?;
    let rules = sqlx::query_scalar("SELECT count(*) FROM recurring_rules")
        .fetch_one(&mut *conn)
        .await?;
    Ok(crate::Summary {
        members: members.iter().map(|n| ffm::ui::fmt::name(n)).collect(),
        transactions,
        rules,
        files,
    })
}

impl Shuffler {
    pub async fn load(conn: &mut SqliteConnection, seed: u64) -> anyhow::Result<Shuffler> {
        let (household, currency): (String, String) =
            sqlx::query_as("SELECT name, locale_currency FROM household_settings WHERE id = 1")
                .fetch_one(&mut *conn)
                .await?;
        let currency = Currency::parse(&currency).context("the household's currency")?;

        let members = sqlx::query_as::<_, (i64, String, String)>(
            "SELECT id, name, slug FROM members ORDER BY id",
        )
        .fetch_all(&mut *conn)
        .await?
        .into_iter()
        .map(|(id, name, slug)| Member { id, name, slug })
        .collect::<Vec<_>>();

        type RuleRow = (
            i64,
            String,
            String,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<i64>,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
        );
        let rules = sqlx::query_as::<_, RuleRow>(
            "SELECT id, kind, title, bucket, bill_name, instance, amount_cents, client_code, issuer_id,
                    barcode_company, card_last4, pdf_password
             FROM recurring_rules ORDER BY id",
        )
        .fetch_all(&mut *conn)
        .await?
        .into_iter()
        .map(|r| Rule {
            id: r.0,
            kind: r.1,
            title: r.2,
            bucket: r.3,
            bill_name: r.4,
            instance: r.5,
            amount: r.6,
            client_code: r.7,
            issuer_id: r.8,
            company: r.9,
            card: r.10,
            pdf_password: r.11,
        })
        .collect::<Vec<_>>();

        type TxRow = (
            i64,
            Option<i64>,
            String,
            String,
            String,
            Option<String>,
            Option<String>,
            Option<i64>,
            Option<String>,
            Option<String>,
            Option<String>,
        );
        let txs = sqlx::query_as::<_, TxRow>(
            "SELECT id, rule_id, period, kind, title, bucket, bill_name, amount_cents, barcode_raw,
                    issuer_id, due_date
             FROM transactions ORDER BY id",
        )
        .fetch_all(&mut *conn)
        .await?
        .into_iter()
        .map(|r| Tx {
            id: r.0,
            rule_id: r.1,
            period: r.2,
            kind: r.3,
            title: r.4,
            bucket: r.5,
            bill_name: r.6,
            amount: r.7,
            barcode: r.8,
            issuer_id: r.9,
            due_date: r.10,
        })
        .collect::<Vec<_>>();

        let receipts = sqlx::query_as::<_, (i64, i64, String, String, String, Option<String>)>(
            "SELECT id, transaction_id, kind, path, ext, deleted_from_path FROM receipts ORDER BY id",
        )
        .fetch_all(&mut *conn)
        .await?
        .into_iter()
        .map(|r| Receipt {
            id: r.0,
            tx: r.1,
            kind: r.2,
            path: r.3,
            ext: r.4,
            deleted_from: r.5,
        })
        .collect::<Vec<_>>();

        let changes = sqlx::query_as::<_, (i64, String, i64, Option<i64>, Option<String>)>(
            "SELECT id, entity, entity_id, rule_id, period FROM change",
        )
        .fetch_all(&mut *conn)
        .await?
        .into_iter()
        .map(|r| {
            (
                r.0,
                Change {
                    entity: r.1,
                    entity_id: r.2,
                    rule_id: r.3,
                    period: r.4,
                },
            )
        })
        .collect::<HashMap<_, _>>();

        let fields = sqlx::query_as::<_, (i64, i64, String, Option<String>, Option<String>, String)>(
            "SELECT id, change_id, field, old_value, new_value, kind FROM change_field ORDER BY id",
        )
        .fetch_all(&mut *conn)
        .await?
        .into_iter()
        .map(|r| Field { id: r.0, change_id: r.1, field: r.2, old: r.3, new: r.4, kind: r.5 })
        .collect::<Vec<_>>();

        let recipes: Vec<(Option<String>, Option<String>)> =
            sqlx::query_as("SELECT cnpj, issuer_name FROM rule_recipe")
                .fetch_all(&mut *conn)
                .await?;
        let codes: Vec<String> = sqlx::query_scalar("SELECT barcode_raw FROM rule_bill_code")
            .fetch_all(&mut *conn)
            .await?;

        // --- the originals: what no fake may be, and what the copy may not hold
        let mut text: HashSet<String> = HashSet::new();
        let mut numbers: HashSet<String> = HashSet::new();
        let typed = |s: &Option<String>, text: &mut HashSet<String>| {
            if let Some(s) = s {
                text.insert(s.trim().to_string());
                for p in s.split(" · ") {
                    text.insert(p.trim().to_string());
                }
            }
        };
        text.insert(household.clone());
        for m in &members {
            text.insert(m.name.clone());
            text.insert(m.slug.clone());
        }
        for r in &rules {
            for s in [
                Some(r.title.clone()),
                r.bucket.clone(),
                r.bill_name.clone(),
                r.instance.clone(),
            ] {
                typed(&s, &mut text);
            }
            for s in [&r.client_code, &r.issuer_id].into_iter().flatten() {
                numbers.insert(s.clone());
            }
        }
        for t in &txs {
            for s in [Some(t.title.clone()), t.bucket.clone(), t.bill_name.clone()] {
                typed(&s, &mut text);
            }
            for s in [&t.barcode, &t.issuer_id].into_iter().flatten() {
                numbers.insert(s.clone());
            }
        }
        for (cnpj, issuer) in &recipes {
            typed(issuer, &mut text);
            if let Some(c) = cnpj {
                numbers.insert(c.clone());
            }
        }
        numbers.extend(codes);
        for f in &fields {
            for v in [&f.old, &f.new].into_iter().flatten() {
                match f.field.as_str() {
                    "title" | "bucket" | "instance" | "bill_name" => {
                        typed(&Some(v.clone()), &mut text)
                    }
                    "recipe" if !ASSISTANTS.contains(&v.as_str()) => {
                        typed(&Some(v.clone()), &mut text)
                    }
                    "barcode" | "issuer" | "client_code" => {
                        numbers.insert(v.clone());
                    }
                    _ => {}
                }
            }
        }
        text.retain(|s| !s.is_empty());
        numbers.retain(|s| !s.is_empty());

        let mut rng = StdRng::seed_from_u64(seed);
        let names = Names::new(&mut rng, &text);
        let original_paths = receipts.iter().map(|r| (r.id, r.path.clone())).collect();
        Ok(Shuffler {
            rng,
            currency,
            money: AmountMap::new(seed),
            names,
            paths: PathMap::default(),
            cnpj: IdMap::default(),
            client: IdMap::default(),
            company: IdMap::default(),
            card: IdMap::default(),
            barcodes: HashMap::new(),
            used_barcodes: HashSet::new(),
            forbidden_ids: numbers,
            original_paths,
            household,
            members,
            rules,
            txs,
            receipts,
            changes,
            fields,
            originals_text: text,
        })
    }

    pub fn originals(&self) -> Originals {
        let mut text: Vec<String> = self.originals_text.iter().cloned().collect();
        text.sort();
        let mut numbers: Vec<String> = self.forbidden_ids.iter().cloned().collect();
        numbers.sort();
        Originals { text, numbers }
    }

    // ------------------------------------------------------------------
    // small maps
    // ------------------------------------------------------------------

    fn map_cnpj(&mut self, v: &str) -> String {
        let rng = &mut self.rng;
        self.cnpj.get_or(v, &self.forbidden_ids, || ids::cnpj(rng))
    }

    fn map_client(&mut self, v: &str) -> String {
        let rng = &mut self.rng;
        self.client
            .get_or(v, &self.forbidden_ids, || ids::same_shape(v, rng))
    }

    pub fn map_company(&mut self, v: &str) -> String {
        let rng = &mut self.rng;
        self.company
            .get_or(v, &self.forbidden_ids, || ids::digits(v.len().max(4), rng))
    }

    fn map_card(&mut self, v: &str) -> String {
        let rng = &mut self.rng;
        self.card
            .get_or(v, &self.forbidden_ids, || ids::digits(4, rng))
    }

    /// A title: each part reuses what it already became as a bill name or a
    /// bucket, else takes a name from the pool its row calls for.
    fn map_title(&mut self, title: &str, kind: &str, recurring: bool) -> String {
        let pool = if kind == "in" {
            Pool::Income
        } else if recurring {
            Pool::Company
        } else {
            Pool::Spend
        };
        title
            .split(" · ")
            .map(|p| match self.names.known(p) {
                Some(v) => v.clone(),
                None => self.names.part(p, pool),
            })
            .collect::<Vec<_>>()
            .join(" · ")
    }

    fn scope_of_tx(&self, id: i64) -> Option<Scope> {
        let t = self.txs.iter().find(|t| t.id == id)?;
        Some(tx_scope(t))
    }

    /// The scope a change log entry's amounts were in.
    fn scope_of_change(&self, change_id: i64) -> Scope {
        let Some(c) = self.changes.get(&change_id) else {
            return Scope::Title(String::new());
        };
        match c.entity.as_str() {
            "transaction" => self
                .scope_of_tx(c.entity_id)
                .unwrap_or(Scope::Title(String::new())),
            "rule" => Scope::Rule(c.entity_id),
            _ => match c.rule_id {
                Some(r) => Scope::Rule(r),
                None => Scope::Title(String::new()),
            },
        }
    }

    fn change_kind(&self, change_id: i64) -> (String, bool) {
        let Some(c) = self.changes.get(&change_id) else {
            return ("out".into(), false);
        };
        let t = match c.entity.as_str() {
            "transaction" => self
                .txs
                .iter()
                .find(|t| t.id == c.entity_id)
                .map(|t| (t.kind.clone(), t.rule_id.is_some())),
            "rule" => self
                .rules
                .iter()
                .find(|r| r.id == c.entity_id)
                .map(|r| (r.kind.clone(), true)),
            _ => None,
        };
        t.unwrap_or(("out".into(), c.rule_id.is_some()))
    }

    /// One original barcode → one fake, carrying the amount its scope maps
    /// the original's to. `reference` disambiguates a cobrança's due factor.
    pub fn map_barcode(
        &mut self,
        raw: &str,
        scope: &Scope,
        reference: NaiveDate,
    ) -> anyhow::Result<String> {
        if let Some(v) = self.barcodes.get(raw) {
            return Ok(v.clone());
        }
        let Ok(parsed) = boleto::parse(raw, reference) else {
            // Not a code the app could have read: no structure to keep.
            let fake = ids::same_shape(raw, &mut self.rng);
            self.barcodes.insert(raw.to_string(), fake.clone());
            return Ok(fake);
        };
        let cents = match parsed.amount {
            boleto::Amount::Fixed(c) => Some(self.money.map(0, scope, c.get())),
            _ => None,
        };
        let company = parsed.company.as_deref().map(|c| self.map_company(c));
        let target = barcode::Target {
            cents,
            due: None,
            company,
        };
        let fake = self.fresh_barcode(raw, reference, &target)?;
        self.barcodes.insert(raw.to_string(), fake.clone());
        Ok(fake)
    }

    /// A rebuilt code no other row has, and that is no original.
    pub fn fresh_barcode(
        &mut self,
        raw: &str,
        reference: NaiveDate,
        target: &barcode::Target,
    ) -> anyhow::Result<String> {
        for _ in 0..20 {
            let fake = barcode::rebuild(raw, reference, target, &mut self.rng)?;
            let shared = boleto::names_an_account(boleto::Kind::Cobranca, &fake);
            if shared
                || (!self.used_barcodes.contains(&fake) && !self.forbidden_ids.contains(&fake))
            {
                self.used_barcodes.insert(fake.clone());
                return Ok(fake);
            }
        }
        anyhow::bail!("no unused barcode could be built from {raw}")
    }

    fn map_path(&mut self, path: &str, scope: &Scope) -> String {
        let currency = self.currency;
        let money = &self.money;
        let names = &mut self.names;
        self.paths.map(
            path,
            currency,
            |c| money.map(0, scope, c),
            |seg| slugify(&names.part(seg, Pool::Bucket)),
        )
    }

    /// Replaces every original inside free text — a log note.
    fn scrub(&self, s: &str) -> String {
        let mut out = s.to_string();
        for (orig, fake) in self.names.pairs() {
            if orig.chars().count() >= 3 {
                out = replace_ci(&out, &orig, &fake);
            }
        }
        for (orig, fake) in self
            .cnpj
            .pairs()
            .chain(self.client.pairs())
            .chain(self.barcodes.iter())
        {
            out = out.replace(orig.as_str(), fake);
        }
        out
    }

    // ------------------------------------------------------------------
    // the writes
    // ------------------------------------------------------------------

    pub async fn apply(
        &mut self,
        conn: &mut SqliteConnection,
        password: &str,
    ) -> anyhow::Result<()> {
        self.members(conn, password).await?;
        self.rules(conn).await?;
        self.transactions(conn).await?;
        self.bill_codes(conn).await?;
        self.recipes(conn).await?;
        self.receipts(conn).await?;
        self.windows(conn).await?;
        self.log(conn).await?;
        sqlx::query("DELETE FROM sessions")
            .execute(&mut *conn)
            .await?;
        sqlx::query("DELETE FROM member_invites")
            .execute(&mut *conn)
            .await?;
        Ok(())
    }

    async fn members(&mut self, conn: &mut SqliteConnection, password: &str) -> anyhow::Result<()> {
        let hash = ffm::db::auth::hash_password(password)?;
        let household = self.names.part(&self.household.clone(), Pool::Household);
        sqlx::query("UPDATE household_settings SET name = ?1 WHERE id = 1")
            .bind(household)
            .execute(&mut *conn)
            .await?;
        for i in 0..self.members.len() {
            let (id, old_name, old_slug) = {
                let m = &self.members[i];
                (m.id, m.name.clone(), m.slug.clone())
            };
            let name = ffm::db::auth::normal_name(&self.names.part(&old_name, Pool::Person));
            let slug = slugify(&name);
            self.paths.members.insert(old_slug, slug.clone());
            sqlx::query("UPDATE members SET name = ?1, slug = ?2, uuid = ?3, email = NULL, password_hash = ?4 WHERE id = ?5")
                .bind(&name)
                .bind(&slug)
                .bind(ids::uuid(&mut self.rng))
                .bind(&hash)
                .bind(id)
                .execute(&mut *conn)
                .await?;
        }
        Ok(())
    }

    async fn rules(&mut self, conn: &mut SqliteConnection) -> anyhow::Result<()> {
        for i in 0..self.rules.len() {
            let r = &self.rules[i];
            let (id, kind, title) = (r.id, r.kind.clone(), r.title.clone());
            let (bucket, bill, instance) =
                (r.bucket.clone(), r.bill_name.clone(), r.instance.clone());
            let (amount, client, issuer, company, card, pw) = (
                r.amount,
                r.client_code.clone(),
                r.issuer_id.clone(),
                r.company.clone(),
                r.card.clone(),
                r.pdf_password.clone(),
            );
            let bucket = bucket.map(|b| self.taxonomy(&b, Pool::Bucket));
            let bill_pool = if kind == "in" {
                Pool::Income
            } else {
                Pool::Company
            };
            let bill = bill.map(|b| self.taxonomy(&b, bill_pool));
            let instance = instance.map(|v| self.taxonomy(&v, Pool::Instance));
            let title = self.map_title(&title, &kind, true);
            let amount = amount.map(|c| self.money.map(0, &Scope::Rule(id), c));
            let client = client.map(|v| self.map_client(&v));
            let issuer = issuer.map(|v| self.map_cnpj(&v));
            let company = company.map(|v| self.map_company(&v));
            let card = card.map(|v| self.map_card(&v));
            let pw = pw.map(|_| "senha".to_string());
            sqlx::query(
                "UPDATE recurring_rules SET title = ?1, bucket = ?2, bill_name = ?3, instance = ?4,
                    amount_cents = ?5, client_code = ?6, issuer_id = ?7, barcode_company = ?8,
                    card_last4 = ?9, pdf_password = ?10
                 WHERE id = ?11",
            )
            .bind(title)
            .bind(bucket)
            .bind(bill)
            .bind(instance)
            .bind(amount)
            .bind(client)
            .bind(issuer)
            .bind(company)
            .bind(card)
            .bind(pw)
            .bind(id)
            .execute(&mut *conn)
            .await?;
        }
        Ok(())
    }

    /// A value that becomes a vault folder: mapped, and its slug learned.
    fn taxonomy(&mut self, v: &str, pool: Pool) -> String {
        let new = self.names.map(v, pool);
        self.paths.learn(v, &new);
        new
    }

    async fn transactions(&mut self, conn: &mut SqliteConnection) -> anyhow::Result<()> {
        for i in 0..self.txs.len() {
            let t = self.txs[i].clone();
            let scope = tx_scope(&t);
            let bucket = t.bucket.as_ref().map(|b| self.taxonomy(b, Pool::Bucket));
            let bill_pool = if t.kind == "in" {
                Pool::Income
            } else {
                Pool::Company
            };
            let bill = t.bill_name.as_ref().map(|b| self.taxonomy(b, bill_pool));
            let title = self.map_title(&t.title, &t.kind, t.rule_id.is_some());
            let amount = t.amount.map(|c| self.money.map(0, &scope, c));
            let issuer = t.issuer_id.as_ref().map(|v| self.map_cnpj(v));
            let reference = reference_of(t.due_date.as_deref(), &t.period);
            let barcode = match &t.barcode {
                Some(raw) => Some(
                    self.map_barcode(raw, &scope, reference)
                        .with_context(|| format!("transaction {}", t.id))?,
                ),
                None => None,
            };
            sqlx::query(
                "UPDATE transactions SET title = ?1, bucket = ?2, bill_name = ?3, amount_cents = ?4,
                    issuer_id = ?5, barcode_raw = ?6
                 WHERE id = ?7",
            )
            .bind(title)
            .bind(bucket)
            .bind(bill)
            .bind(amount)
            .bind(issuer)
            .bind(barcode)
            .bind(t.id)
            .execute(&mut *conn)
            .await?;
        }
        Ok(())
    }

    async fn bill_codes(&mut self, conn: &mut SqliteConnection) -> anyhow::Result<()> {
        let codes: Vec<(i64, i64, String, String, Option<i64>, Option<String>)> = sqlx::query_as(
            "SELECT id, rule_id, period, barcode_raw, amount_cents, due_date FROM rule_bill_code ORDER BY id",
        )
        .fetch_all(&mut *conn)
        .await?;
        for (id, rule, period, raw, amount, due) in codes {
            let scope = Scope::Rule(rule);
            let barcode = self.map_barcode(&raw, &scope, reference_of(due.as_deref(), &period))?;
            let amount = amount.map(|c| self.money.map(0, &scope, c));
            sqlx::query(
                "UPDATE rule_bill_code SET barcode_raw = ?1, amount_cents = ?2 WHERE id = ?3",
            )
            .bind(barcode)
            .bind(amount)
            .bind(id)
            .execute(&mut *conn)
            .await?;
        }
        Ok(())
    }

    async fn recipes(&mut self, conn: &mut SqliteConnection) -> anyhow::Result<()> {
        type Row = (
            i64,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
        );
        let rows: Vec<Row> = sqlx::query_as(
            "SELECT id, cnpj, issuer_name, amount_regex, due_regex, client_regex, card_regex FROM rule_recipe ORDER BY id",
        )
        .fetch_all(&mut *conn)
        .await?;
        for (id, cnpj, issuer, amount, due, client, card) in rows {
            let cnpj = cnpj.map(|v| self.map_cnpj(&v));
            let issuer = issuer.map(|v| self.names.map(&v, Pool::Company));
            sqlx::query(
                "UPDATE rule_recipe SET cnpj = ?1, issuer_name = ?2, amount_regex = ?3, due_regex = ?4,
                    client_regex = ?5, card_regex = ?6 WHERE id = ?7",
            )
            .bind(cnpj)
            .bind(issuer)
            .bind(amount.map(|_| AMOUNT_RE))
            .bind(due.map(|_| DUE_RE))
            .bind(client.map(|_| CLIENT_RE))
            .bind(card.map(|_| CARD_RE))
            .bind(id)
            .execute(&mut *conn)
            .await?;
        }
        Ok(())
    }

    async fn receipts(&mut self, conn: &mut SqliteConnection) -> anyhow::Result<()> {
        for i in 0..self.receipts.len() {
            let (id, tx, kind, path, ext, from) = {
                let r = &self.receipts[i];
                (
                    r.id,
                    r.tx,
                    r.kind.clone(),
                    r.path.clone(),
                    r.ext.clone(),
                    r.deleted_from.clone(),
                )
            };
            let scope = self.scope_of_tx(tx).unwrap_or(Scope::Title(String::new()));
            let path = self.map_path(&path, &scope);
            let from = from.map(|p| self.map_path(&p, &scope));
            sqlx::query(
                "UPDATE receipts SET path = ?1, deleted_from_path = ?2, original_filename = ?3, text_hash = NULL
                 WHERE id = ?4",
            )
            .bind(path)
            .bind(from)
            .bind(format!("{kind}.{ext}"))
            .bind(id)
            .execute(&mut *conn)
            .await?;
        }
        Ok(())
    }

    async fn windows(&mut self, conn: &mut SqliteConnection) -> anyhow::Result<()> {
        let rows: Vec<(i64, i64, Option<String>)> =
            sqlx::query_as("SELECT id, rule_id, file_path FROM rule_barcode_window")
                .fetch_all(&mut *conn)
                .await?;
        for (id, rule, path) in rows {
            let path = path.map(|p| self.map_path(&p, &Scope::Rule(rule)));
            sqlx::query("UPDATE rule_barcode_window SET file_path = ?1 WHERE id = ?2")
                .bind(path)
                .bind(id)
                .execute(&mut *conn)
                .await?;
        }
        Ok(())
    }

    async fn log(&mut self, conn: &mut SqliteConnection) -> anyhow::Result<()> {
        let today = ffm::clock::today();
        let fields = std::mem::take(&mut self.fields);
        for f in &fields {
            let scope = self.scope_of_change(f.change_id);
            let (kind, recurring) = self.change_kind(f.change_id);
            let reference = self
                .changes
                .get(&f.change_id)
                .and_then(|c| c.period.as_deref())
                .map(|p| reference_of(None, p))
                .unwrap_or(today);
            let mut map = |v: &str| -> anyhow::Result<String> {
                Ok(match (f.kind.as_str(), f.field.as_str()) {
                    ("money", _) => match v.parse::<i64>() {
                        Ok(c) => self.money.map(0, &scope, c).to_string(),
                        Err(_) => v.to_string(),
                    },
                    (_, "title") => self.map_title(v, &kind, recurring),
                    (_, "bucket") => self.taxonomy(v, Pool::Bucket),
                    (_, "instance") => self.taxonomy(v, Pool::Instance),
                    (_, "bill_name") => {
                        let pool = if kind == "in" {
                            Pool::Income
                        } else {
                            Pool::Company
                        };
                        self.taxonomy(v, pool)
                    }
                    (_, "issuer") if v.chars().all(|c| c.is_ascii_digit()) => self.map_cnpj(v),
                    (_, "issuer") => self.names.map(v, Pool::Company),
                    (_, "client_code") => self.map_client(v),
                    (_, "company_code") => self.map_company(v),
                    (_, "card" | "card_last4") => self.map_card(v),
                    (_, "barcode") => self.map_barcode(v, &scope, reference)?,
                    (_, "recipe") if ASSISTANTS.contains(&v) => v.to_string(),
                    (_, "recipe") => self.names.map(v, Pool::Company),
                    (_, "original_filename") => match v.rsplit_once('.') {
                        Some((_, ext)) => format!("arquivo.{ext}"),
                        None => "arquivo".to_string(),
                    },
                    (_, "path" | "kept") => self.map_path(v, &scope),
                    _ => v.to_string(),
                })
            };
            let old = f
                .old
                .as_deref()
                .filter(|v| !v.is_empty())
                .map(&mut map)
                .transpose()?;
            let new = f
                .new
                .as_deref()
                .filter(|v| !v.is_empty())
                .map(&mut map)
                .transpose()?;
            if old.as_deref() != f.old.as_deref().filter(|v| !v.is_empty())
                || new.as_deref() != f.new.as_deref().filter(|v| !v.is_empty())
            {
                sqlx::query("UPDATE change_field SET old_value = coalesce(?1, old_value), new_value = coalesce(?2, new_value) WHERE id = ?3")
                    .bind(old)
                    .bind(new)
                    .bind(f.id)
                    .execute(&mut *conn)
                    .await?;
            }
        }
        self.fields = fields;

        let notes: Vec<(i64, String)> =
            sqlx::query_as("SELECT id, note FROM change WHERE note IS NOT NULL ORDER BY id")
                .fetch_all(&mut *conn)
                .await?;
        for (id, note) in notes {
            let new = self.scrub(&note);
            if new != note {
                sqlx::query("UPDATE change SET note = ?1 WHERE id = ?2")
                    .bind(new)
                    .bind(id)
                    .execute(&mut *conn)
                    .await?;
            }
        }
        Ok(())
    }
}

fn tx_scope(t: &Tx) -> Scope {
    match t.rule_id {
        Some(r) => Scope::Rule(r),
        None => Scope::Title(t.title.trim().to_lowercase()),
    }
}

/// The date a cobrança's due factor is read near: the row's due date, else
/// the middle of its month.
pub fn reference_of(due: Option<&str>, period: &str) -> NaiveDate {
    due.and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
        .or_else(|| NaiveDate::parse_from_str(&format!("{period}-15"), "%Y-%m-%d").ok())
        .unwrap_or_else(ffm::clock::today)
}

/// `s` with every case-insensitive occurrence of `needle` replaced.
fn replace_ci(s: &str, needle: &str, with: &str) -> String {
    let lower = s.to_lowercase();
    let n = needle.to_lowercase();
    if n.is_empty() || !lower.contains(&n) || lower.len() != s.len() {
        // Lowercasing changed the byte length (a rare letter): fall back to
        // an exact match rather than slice on the wrong boundaries.
        return s.replace(needle, with);
    }
    let mut out = String::new();
    let mut i = 0;
    while let Some(at) = lower[i..].find(&n) {
        out.push_str(&s[i..i + at]);
        out.push_str(with);
        i += at + n.len();
    }
    out.push_str(&s[i..]);
    out
}
