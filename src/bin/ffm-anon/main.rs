//! `ffm-anon` — a shuffled copy of a production database, for development.
//!
//! New features want a ledger shaped like the real one: months of history,
//! the real mix of rules, a bill covering several months, the trash, the
//! change log. Only production has that shape, and production also holds the
//! household's names, amounts, CNPJs, account numbers, barcodes and bills.
//! This writes a **separate copy** with all of that made up, plus a vault of
//! generated stand-in files, that `just dev-anon` runs against.
//!
//! The source is never opened: it is copied byte for byte while nothing holds
//! it, and every change is made to the copy. The copy is renamed into place
//! only once a scan for every original string comes back clean.
//!
//! Dev-only, behind the `anon` feature, so a release never builds it.

mod barcode;
mod guard;
mod ids;
mod leak;
mod money;
mod names;
mod paths;
mod scale;
mod shuffle;
mod sqlrow;
mod vault;

use anyhow::{Context, bail};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode};
use sqlx::{ConnectOptions, Connection};
use std::path::{Path, PathBuf};

const HELP: &str = "\
ffm-anon — a shuffled copy of a production database, for development

usage:
  ffm-anon --from <ffm.db> [options]

options:
  --from <path>       the database to copy (never written to)
  --out <dir>         where the copy goes                    [./anon]
  --receipts <dir>    the real vault, only to see WHICH files exist —
                      never read; without it every recorded file is made
  --seed <n>          the same seed gives the same copy      [random]
  --scale <n>         n copies of every member (n-1 new ones) [1]
  --years <n>         n more years of history, back in time   [0]
  --password <pw>     every member's password in the copy    [ffm]
  --force             replace an existing output (moved aside, not deleted)
  --keep-failed       leave a copy the leak check refused in place, to see
                      where the original is (it may hold real data)
  -h, --help          this
";

#[derive(Debug)]
pub struct Args {
    pub from: PathBuf,
    pub out: PathBuf,
    pub receipts: Option<PathBuf>,
    pub seed: u64,
    pub scale: u32,
    pub years: u32,
    pub password: String,
    pub force: bool,
    pub keep_failed: bool,
}

impl Args {
    fn parse(mut it: impl Iterator<Item = String>) -> anyhow::Result<Option<Args>> {
        let mut from = None;
        let mut out = PathBuf::from("./anon");
        let mut receipts = None;
        let mut seed = None;
        let mut scale = 1;
        let mut years = 0;
        let mut password = "ffm".to_string();
        let mut force = false;
        let mut keep_failed = false;
        while let Some(flag) = it.next() {
            let mut value = || it.next().with_context(|| format!("{flag} needs a value"));
            match flag.as_str() {
                "--from" => from = Some(PathBuf::from(value()?)),
                "--out" => out = PathBuf::from(value()?),
                "--receipts" => receipts = Some(PathBuf::from(value()?)),
                "--seed" => seed = Some(value()?.parse().context("--seed takes a number")?),
                "--scale" => scale = value()?.parse().context("--scale takes a number")?,
                "--years" => years = value()?.parse().context("--years takes a number")?,
                "--password" => password = value()?,
                "--force" => force = true,
                "--keep-failed" => keep_failed = true,
                "-h" | "--help" => return Ok(None),
                other => bail!("unknown option {other} — see --help"),
            }
        }
        let Some(from) = from else {
            bail!("--from is required: ffm-anon --from ./ffm.db");
        };
        if scale == 0 {
            bail!("--scale is at least 1 (1 means no extra members)");
        }
        if password.is_empty() {
            bail!("--password cannot be empty");
        }
        Ok(Some(Args {
            from,
            out,
            receipts,
            seed: seed.unwrap_or_else(rand::random),
            scale,
            years,
            password,
            force,
            keep_failed,
        }))
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let args = match Args::parse(std::env::args().skip(1)) {
        Ok(Some(a)) => a,
        Ok(None) => {
            print!("{HELP}");
            return;
        }
        Err(e) => {
            eprintln!("✗ {e:#}");
            std::process::exit(2);
        }
    };
    if let Err(e) = run(&args).await {
        eprintln!("✗ {e:#}");
        std::process::exit(1);
    }
}

/// The working names, beside where they end up. Nothing lands under the final
/// names until the leak check has passed.
struct Work {
    db: PathBuf,
    vault: PathBuf,
}

impl Work {
    fn new(out: &Path) -> Work {
        Work {
            db: out.join("ffm.db.tmp"),
            vault: out.join("receipts.tmp"),
        }
    }

    fn discard(&self) {
        for suffix in ["", "-wal", "-shm", "-journal"] {
            let _ = std::fs::remove_file(format!("{}{suffix}", self.db.display()));
        }
        let _ = std::fs::remove_dir_all(&self.vault);
    }
}

async fn run(args: &Args) -> anyhow::Result<()> {
    let aside = guard::check(args)?;
    std::fs::create_dir_all(&args.out)
        .with_context(|| format!("creating {}", args.out.display()))?;

    let work = Work::new(&args.out);
    work.discard();
    let result = build(args, &work).await;
    if result.is_err() && args.keep_failed {
        eprintln!(
            "· kept for inspection: {} and {}",
            work.db.display(),
            work.vault.display()
        );
        return result.map(|_| ());
    }
    if result.is_err() {
        work.discard();
        // A failed run leaves things as they were: the earlier copy back.
        if let Some(aside) = aside
            && std::fs::remove_dir(&args.out).is_ok()
        {
            let _ = std::fs::rename(&aside, &args.out);
        }
    }
    let summary = result?;

    let db = args.out.join("ffm.db");
    let vault = args.out.join("receipts");
    std::fs::rename(&work.db, &db)?;
    std::fs::rename(&work.vault, &vault)?;
    std::fs::create_dir_all(args.out.join("backups"))?;

    println!("✓ shuffled copy written");
    println!("  database   {}", db.display());
    println!("  vault      {}", vault.display());
    println!(
        "  seed       {}   (--seed {} makes this copy again)",
        args.seed, args.seed
    );
    println!("  password   {}   (every member)", args.password);
    println!("  members    {}", summary.members.join(", "));
    println!(
        "  rows       {} transactions · {} rules · {} files",
        summary.transactions, summary.rules, summary.files
    );
    println!();
    println!("  just dev-anon");
    Ok(())
}

pub struct Summary {
    pub members: Vec<String>,
    pub transactions: i64,
    pub rules: i64,
    pub files: usize,
}

async fn build(args: &Args, work: &Work) -> anyhow::Result<Summary> {
    // --- 1. the copy, always first ----------------------------------------
    // Byte for byte, not through SQLite: the guard has proven nothing holds
    // the source, so its files are whole, and a read-only open of a WAL
    // database with no -shm beside it fails outright. The WAL, if a crash left
    // one, comes along and is replayed when the copy is opened.
    std::fs::copy(&args.from, &work.db)
        .with_context(|| format!("copying {}", args.from.display()))?;
    let wal = PathBuf::from(format!("{}-wal", args.from.display()));
    if wal.exists() {
        std::fs::copy(&wal, format!("{}-wal", work.db.display()))?;
    }

    let mut conn = SqliteConnectOptions::new()
        .filename(&work.db)
        .foreign_keys(false)
        .journal_mode(SqliteJournalMode::Delete)
        .disable_statement_logging()
        .connect()
        .await
        .context("opening the copy")?;

    // --- 2. shuffle, scale, check — one transaction --------------------------
    let mut tx = conn.begin().await?;
    let triggers = shuffle::drop_guards(&mut tx).await?;
    let mut s = shuffle::Shuffler::load(&mut tx, args.seed).await?;
    let originals = s.originals();
    s.apply(&mut tx, &args.password).await?;
    scale::years(&mut tx, &mut s, args.years).await?;
    scale::members(&mut tx, &mut s, args.scale, &args.password).await?;
    let mut plan = vault::Plan::new(args.receipts.clone());
    plan.collect(&mut tx, &s).await?;
    shuffle::restore_guards(&mut tx, &triggers).await?;
    shuffle::finish(&mut tx).await?;
    tx.commit().await?;

    // --- 3. the vault, from what the copy now says -------------------------
    let files = plan.write(&work.vault, &s)?;

    // --- 4. purge freed pages, then look for anything left -------------------
    sqlx::query("PRAGMA secure_delete = ON")
        .execute(&mut conn)
        .await?;
    sqlx::query("VACUUM").execute(&mut conn).await?;
    let summary = shuffle::summary(&mut conn, files).await?;
    leak::check(&mut conn, &originals, &work.db, &work.vault).await?;
    conn.close().await?;
    Ok(summary)
}
