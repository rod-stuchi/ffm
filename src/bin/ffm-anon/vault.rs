//! A vault of stand-in files, one wherever the copy says a file is.
//!
//! The real bills cannot be copied — they carry everything the shuffle hid —
//! but a vault with nothing in it reads every tracked row as unpaid, and the
//! watcher would log each missing file as removed by hand on the first boot.
//! So every `receipts` row gets a file at its path (or in its member's trash):
//! a bill is a page `barcode_page` draws from the row's fake code, so `P`
//! shows it and dropping it on the page finds its rule; a proof of payment is
//! a page of text; a photo is that page drawn by `pdftoppm`.

use crate::barcode;
use crate::shuffle::Shuffler;
use anyhow::{Context, bail};
use chrono::NaiveDate;
use ffm::domain::money::Cents;
use ffm::domain::receipt_path::{deleted_path, deleted_path_beside, deleted_path_for};
use ffm::services::barcode_page::{A4, PageInfo, page, text_page};
use ffm::services::{prune, receipts};
use ffm::ui::fmt;
use sqlx::SqliteConnection;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;

const FOOTER: &str =
    "Documento ficticio gerado pelo ffm-anon a partir de dados embaralhados - nao e uma conta real";

struct Job {
    rel: String,
    kind: String,
    ext: String,
    bill: Bill,
    /// The pages of a bill covering several months: (page, code, amount, due).
    pages: Vec<(i64, String, Option<i64>, Option<String>)>,
}

/// What a stand-in page says.
#[derive(Clone)]
struct Bill {
    issuer: String,
    cnpj: Option<String>,
    client: Option<String>,
    card: Option<String>,
    period: String,
    due: Option<String>,
    amount: Option<i64>,
    barcode: Option<String>,
}

pub struct Plan {
    real_vault: Option<PathBuf>,
    jobs: Vec<Job>,
}

impl Plan {
    pub fn new(real_vault: Option<PathBuf>) -> Plan {
        Plan {
            real_vault,
            jobs: Vec::new(),
        }
    }

    pub async fn collect(
        &mut self,
        conn: &mut SqliteConnection,
        s: &Shuffler,
    ) -> anyhow::Result<()> {
        // Files that left the disk: pruned or expired for good, or found
        // gone by the watcher. Nothing to write for those.
        let gone: HashSet<i64> = sqlx::query_scalar(
            "SELECT entity_id FROM change WHERE entity = 'file' AND note IN (?1, ?2, ?3)",
        )
        .bind(prune::PRUNED)
        .bind(prune::EXPIRED)
        .bind(receipts::REMOVED_BY_HAND)
        .fetch_all(&mut *conn)
        .await?
        .into_iter()
        .collect();

        let mut pages: HashMap<i64, Vec<(i64, String, Option<i64>, Option<String>)>> =
            HashMap::new();
        let codes: Vec<(i64, Option<i64>, String, Option<i64>, Option<String>)> = sqlx::query_as(
            "SELECT receipt_id, page, barcode_raw, amount_cents, due_date FROM rule_bill_code
             WHERE receipt_id IS NOT NULL ORDER BY id",
        )
        .fetch_all(&mut *conn)
        .await?;
        for (receipt, page, code, amount, due) in codes {
            pages
                .entry(receipt)
                .or_default()
                .push((page.unwrap_or(1).max(1), code, amount, due));
        }

        type Row = (
            i64,
            String,
            String,
            String,
            bool,
            String,
            String,
            Option<i64>,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<i64>,
        );
        let rows: Vec<Row> = sqlx::query_as(
            "SELECT r.id, r.kind, r.path, r.ext, r.deleted_at IS NOT NULL,
                    t.period, t.title, t.amount_cents, t.barcode_raw, t.due_date, t.issuer_id,
                    ru.bill_name, ru.client_code, ru.card_last4, ru.spans_period
             FROM receipts r
             JOIN transactions t ON t.id = r.transaction_id
             LEFT JOIN recurring_rules ru ON ru.id = t.rule_id
             ORDER BY r.id",
        )
        .fetch_all(&mut *conn)
        .await?;

        for r in rows {
            let (id, kind, path, ext, deleted) = (r.0, r.1, r.2, r.3, r.4);
            if gone.contains(&id) || !self.exists_in_real(id, deleted, s) {
                continue;
            }
            let rel = if deleted {
                deleted_path_for(&path, id)
            } else {
                path
            };
            let spans = r.14.unwrap_or(0) == 1;
            self.jobs.push(Job {
                rel,
                kind,
                ext,
                bill: Bill {
                    issuer: r.11.unwrap_or(r.6),
                    cnpj: r.10,
                    client: r.12,
                    card: r.13,
                    period: r.5,
                    due: r.9,
                    amount: r.7,
                    barcode: r.8,
                },
                pages: if spans {
                    pages.remove(&id).unwrap_or_default()
                } else {
                    Vec::new()
                },
            });
        }
        Ok(())
    }

    /// With the real vault named, only what is really there is made. It is
    /// asked whether a file exists — never what is in it.
    fn exists_in_real(&self, id: i64, deleted: bool, s: &Shuffler) -> bool {
        let (Some(root), Some(orig)) = (&self.real_vault, s.original_paths.get(&id)) else {
            return true;
        };
        if !deleted {
            return root.join(orig).is_file();
        }
        [
            deleted_path_for(orig, id),
            deleted_path_beside(orig, id),
            deleted_path(orig),
        ]
        .iter()
        .any(|p| root.join(p).is_file())
    }

    pub fn write(&self, vault: &Path, s: &Shuffler) -> anyhow::Result<usize> {
        let scratch = vault.with_extension("scratch");
        std::fs::create_dir_all(vault)?;
        std::fs::create_dir_all(&scratch)?;
        let result = self.write_all(vault, &scratch, s);
        let _ = std::fs::remove_dir_all(&scratch);
        result
    }

    fn write_all(&self, vault: &Path, scratch: &Path, s: &Shuffler) -> anyhow::Result<usize> {
        for (n, job) in self.jobs.iter().enumerate() {
            let target = vault.join(&job.rel);
            if let Some(dir) = target.parent() {
                std::fs::create_dir_all(dir)?;
            }
            let pdf = || -> anyhow::Result<Vec<u8>> {
                if job.kind == "barcode" {
                    bill_pdf(job, scratch, n, s)
                } else {
                    Ok(text_page(&proof_info(job, s), A4.0, A4.1))
                }
            };
            match job.ext.as_str() {
                "pdf" => std::fs::write(&target, pdf()?)?,
                "png" | "jpg" => {
                    let src = scratch.join(format!("{n}.pdf"));
                    std::fs::write(&src, pdf()?)?;
                    draw(&src, &target, job.ext == "png")?;
                }
                _ => std::fs::write(&target, proof_text(job, s))?,
            }
        }
        Ok(self.jobs.len())
    }
}

fn bill_pdf(job: &Job, scratch: &Path, n: usize, s: &Shuffler) -> anyhow::Result<Vec<u8>> {
    if job.pages.is_empty() {
        let info = bill_info(&job.bill, s);
        return Ok(match &job.bill.barcode {
            Some(_) => page(&info, A4.0, A4.1).context("drawing a stand-in bill")?,
            None => text_page(&info, A4.0, A4.1),
        });
    }
    // One page per page the real file had, each holding its own code, so
    // every month opens on the page its code is printed on.
    let last = job.pages.iter().map(|p| p.0).max().unwrap_or(1);
    let mut files = Vec::new();
    for p in 1..=last {
        let mut bill = job.bill.clone();
        bill.barcode = None;
        if let Some((_, code, amount, due)) = job.pages.iter().find(|x| x.0 == p) {
            bill.barcode = Some(code.clone());
            bill.amount = *amount;
            bill.due = due.clone();
        }
        let info = bill_info(&bill, s);
        let bytes = match &bill.barcode {
            Some(_) => page(&info, A4.0, A4.1).context("drawing a stand-in bill page")?,
            None => text_page(&info, A4.0, A4.1),
        };
        let f = scratch.join(format!("{n}-{p}.pdf"));
        std::fs::write(&f, bytes)?;
        files.push(f);
    }
    let out = scratch.join(format!("{n}.joined.pdf"));
    let status = Command::new("qpdf")
        .arg("--empty")
        .arg("--pages")
        .args(&files)
        .arg("--")
        .arg(&out)
        .status()
        .context("qpdf is needed to join a bill's pages")?;
    if !status.success() {
        bail!("qpdf could not join the pages of {}", job.rel);
    }
    Ok(std::fs::read(&out)?)
}

fn draw(pdf: &Path, target: &Path, png: bool) -> anyhow::Result<()> {
    let stem = target.with_extension("");
    let status = Command::new("pdftoppm")
        .arg(if png { "-png" } else { "-jpeg" })
        .args(["-r", "60", "-singlefile"])
        .arg(pdf)
        .arg(&stem)
        .status()
        .context("pdftoppm (poppler) is needed to draw a stand-in photo")?;
    if !status.success() {
        bail!("pdftoppm could not draw {}", target.display());
    }
    // pdftoppm names a JPEG `.jpg`, a PNG `.png` — the extensions the vault
    // uses — so the file is already where it belongs.
    anyhow::ensure!(
        target.is_file(),
        "pdftoppm wrote nothing at {}",
        target.display()
    );
    Ok(())
}

fn money(cents: i64, s: &Shuffler) -> String {
    fmt::money(Cents(cents), s.currency)
}

fn date(d: &str) -> String {
    NaiveDate::parse_from_str(d, "%Y-%m-%d")
        .map(fmt::date)
        .unwrap_or_else(|_| d.to_string())
}

fn month(period: &str) -> String {
    match period.split_once('-') {
        Some((y, m)) => format!("{m}/{y}"),
        None => period.to_string(),
    }
}

fn cnpj(d: &str) -> String {
    if d.len() != 14 {
        return d.to_string();
    }
    format!(
        "{}.{}.{}/{}-{}",
        &d[0..2],
        &d[2..5],
        &d[5..8],
        &d[8..12],
        &d[12..14]
    )
}

/// Labels without accents: the recipe patterns match them as typed.
fn bill_info(b: &Bill, s: &Shuffler) -> PageInfo {
    let mut details = Vec::new();
    if let Some(c) = &b.client {
        details.push(("Cliente".into(), c.clone()));
    }
    if let Some(c) = &b.cnpj {
        details.push(("CNPJ".into(), cnpj(c)));
    }
    if let Some(c) = &b.card {
        details.push(("Cartao final".into(), c.clone()));
    }
    details.push(("Referencia".into(), month(&b.period)));
    if let Some(d) = &b.due {
        details.push(("Vencimento".into(), date(d)));
    }
    if let Some(a) = b.amount {
        details.push(("Total a pagar".into(), money(a, s)));
    }
    PageInfo {
        heading: b.issuer.clone(),
        details,
        typed: b.barcode.as_deref().map(barcode::typed).unwrap_or_default(),
        barcode: b.barcode.clone().unwrap_or_default(),
        footer: FOOTER.into(),
    }
}

fn proof_info(job: &Job, s: &Shuffler) -> PageInfo {
    let b = &job.bill;
    let heading = if job.kind == "scheduled" {
        "Agendamento de pagamento"
    } else {
        "Comprovante de pagamento"
    };
    let mut details = vec![
        ("Favorecido".into(), b.issuer.clone()),
        ("Referencia".into(), month(&b.period)),
    ];
    if let Some(a) = b.amount {
        details.push(("Valor".into(), money(a, s)));
    }
    if let Some(d) = &b.due {
        details.push(("Data".into(), date(d)));
    }
    PageInfo {
        heading: heading.into(),
        details,
        footer: FOOTER.into(),
        ..PageInfo::default()
    }
}

fn proof_text(job: &Job, s: &Shuffler) -> String {
    let info = if job.kind == "barcode" {
        bill_info(&job.bill, s)
    } else {
        proof_info(job, s)
    };
    let mut out = format!("{}\n\n", info.heading);
    for (k, v) in &info.details {
        out.push_str(&format!("{k}: {v}\n"));
    }
    if !info.barcode.is_empty() {
        out.push_str(&format!("\n{}\n", info.typed));
    }
    out.push_str(&format!("\n{FOOTER}\n"));
    out
}
