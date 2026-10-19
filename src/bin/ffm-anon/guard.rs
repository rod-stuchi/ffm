//! Refusing to run against a database somebody is using.
//!
//! The ffm server holds `ffm.db` open for as long as it runs, idle or not, so
//! the question "is it in use" is answered by asking the OS who has the file
//! open — `lsof` on the database, its WAL and its shared-memory file. An
//! SQLite lock would only catch a writer mid-transaction, and the process
//! that matters is the one sitting idle between requests, about to write.

use crate::Args;
use anyhow::{Context, bail};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Returns where an earlier copy was moved, so a run that fails can put it
/// back.
pub fn check(args: &Args) -> anyhow::Result<Option<PathBuf>> {
    if !args.from.is_file() {
        bail!("{} is not a database file", args.from.display());
    }
    let from = args.from.canonicalize()?;

    // The output must be somewhere else entirely: a mistyped --out must never
    // be able to put the shuffled copy over the real thing.
    let out = absolute(&args.out)?;
    if out == from || from.starts_with(&out) || out == from.parent().unwrap_or(Path::new("/")) {
        bail!(
            "--out {} would hold the source database — pick a directory of its own",
            args.out.display()
        );
    }

    holders(&from)?;
    let out_db = out.join("ffm.db");
    if out_db.exists() {
        holders(&out_db).context("the previous shuffled copy is open — stop `just dev-anon` first")?;
    }

    if let Some(r) = &args.receipts
        && !r.is_dir()
    {
        bail!("--receipts {} is not a directory", r.display());
    }

    if out.exists() && std::fs::read_dir(&out)?.next().is_some() {
        if !args.force {
            bail!(
                "{} already has a copy in it — --force moves it aside and writes a new one",
                args.out.display()
            );
        }
        let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
        let aside = PathBuf::from(format!("{}.old-{stamp}", out.display()));
        std::fs::rename(&out, &aside)?;
        println!("· the previous copy is now {}", aside.display());
        return Ok(Some(aside));
    }
    Ok(None)
}

/// Refuses when any process has the database, its WAL or its shm open.
fn holders(db: &Path) -> anyhow::Result<()> {
    let files: Vec<String> = ["", "-wal", "-shm"]
        .iter()
        .map(|s| format!("{}{s}", db.display()))
        .filter(|f| Path::new(f).exists())
        .collect();

    // -F pc: one `p<pid>` line and one `c<command>` line per process.
    let out = Command::new("lsof")
        .arg("-Fpc")
        .arg("--")
        .args(&files)
        .output()
        .context("lsof is needed to tell whether the database is in use, and it did not run")?;
    // lsof exits 1 both for "nobody has it open" and for some errors; an
    // empty stdout is the answer that matters.
    let text = String::from_utf8_lossy(&out.stdout);
    let mut found = Vec::new();
    let mut pid = String::new();
    for line in text.lines() {
        if let Some(p) = line.strip_prefix('p') {
            pid = p.to_string();
        } else if let Some(c) = line.strip_prefix('c') {
            found.push(format!("pid {pid} ({c})"));
        }
    }
    if !found.is_empty() {
        bail!(
            "{} is in use by {} — stop it first; a copy taken while it runs could be torn",
            db.display(),
            found.join(", ")
        );
    }
    Ok(())
}

fn absolute(p: &Path) -> anyhow::Result<PathBuf> {
    if p.exists() {
        return Ok(p.canonicalize()?);
    }
    // Not created yet: resolve what exists of it.
    let parent = p.parent().filter(|x| !x.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let name = p.file_name().context("--out has no name")?;
    Ok(absolute(parent)?.join(name))
}
