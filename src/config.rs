//! `ffm.toml` — the operator's tier, and the only settings that cannot live in
//! the database.
//!
//! Three keys, and deliberately nothing else. Everything a *household* would
//! want to change is a column: a shared file that two people both want to edit
//! is the file disagreeing with itself. What is left here is what a running
//! process genuinely cannot change about itself — `db_path` most obviously,
//! since you cannot read a setting out of a database you have not opened.
//!
//! **Read once at startup, never watched.** The earlier `notify`-at-runtime
//! design is dropped because it is now false for everything remaining:
//! swapping `db_path` under open transactions is not a thing, and moving
//! `receipts_root` means relocating the whole vault.

use std::fmt;
use std::path::{Path, PathBuf};

/// Where each value actually came from, so an error message can point at the
/// thing the operator would edit — a file line, or an environment variable.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Source {
    /// 1-based line in `ffm.toml`.
    File(usize),
    Env(&'static str),
    Default,
}

impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Source::File(line) => write!(f, "line {line}"),
            Source::Env(var) => write!(f, "${var}"),
            Source::Default => write!(f, "default"),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Config {
    pub db_path: PathBuf,
    pub receipts_root: PathBuf,
    pub backup_dir: PathBuf,
    /// The resolved config file, when there was one. Shown read-only in
    /// `:household` and on `/setup`, with a line saying to edit it and restart.
    pub file: Option<PathBuf>,
}

/// A startup failure that names the line to edit. The process prints this and
/// exits rather than starting half-configured — for these three keys there is
/// no sensible partial state.
#[derive(Debug)]
pub struct ConfigError {
    pub source: Source,
    pub key: &'static str,
    pub problem: String,
    pub value: Option<String>,
}

impl fmt::Display for ConfigError {
    /// `✗ line 3 · storage.receipts_root does not exist — /disks/vault/ffm`
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "✗ {} · {} {}", self.source, self.key, self.problem)?;
        if let Some(v) = &self.value {
            write!(f, " — {v}")?;
        }
        Ok(())
    }
}

impl std::error::Error for ConfigError {}

/// The file's shape. Every field is optional here and required *after* the
/// environment has had its say, so `FFM_DB_PATH` alone is enough for a
/// container with no file at all.
#[derive(serde::Deserialize, Default)]
struct FileShape {
    #[serde(default)]
    storage: StorageShape,
}

#[derive(serde::Deserialize, Default)]
struct StorageShape {
    db_path: Option<String>,
    receipts_root: Option<String>,
    backup_dir: Option<String>,
}

/// One key, resolved, with a note of where it came from.
#[derive(Debug)]
struct Resolved {
    value: String,
    source: Source,
}

impl Config {
    /// Where the issuer profiles live — beside `ffm.toml`, because they are
    /// the same tier: data an operator edits, read at the moment it is needed
    /// rather than held in memory.
    pub fn issuers_path(&self) -> std::path::PathBuf {
        self.file
            .as_ref()
            .and_then(|f| f.parent())
            .unwrap_or_else(|| std::path::Path::new("."))
            .join("config/issuers.toml")
    }

    /// Reads `$FFM_CONFIG` (or `./ffm.toml`), lets the environment win, and
    /// validates. Any failure names the line to edit.
    pub fn load() -> Result<Config, ConfigError> {
        let path = std::env::var("FFM_CONFIG")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("ffm.toml"));
        let raw = std::fs::read_to_string(&path).ok();
        let found = raw.is_some();
        Config::from_parts(raw.as_deref(), found.then_some(path.as_path()))
    }

    /// The whole of `load`'s logic with the filesystem read lifted out, which
    /// is what makes every branch testable.
    pub fn from_parts(raw: Option<&str>, path: Option<&Path>) -> Result<Config, ConfigError> {
        let src = raw.unwrap_or("");
        let parsed: FileShape = toml::from_str(src).map_err(|e| ConfigError {
            source: e
                .span()
                .map(|s| Source::File(line_at(src, s.start)))
                .unwrap_or(Source::Default),
            key: "ffm.toml",
            problem: format!("is not valid TOML: {}", e.message()),
            value: None,
        })?;

        let db_path = resolve(src, "db_path", "FFM_DB_PATH", parsed.storage.db_path)?;
        let receipts_root = resolve(
            src,
            "receipts_root",
            "FFM_RECEIPTS_ROOT",
            parsed.storage.receipts_root,
        )?;
        let backup_dir = resolve(
            src,
            "backup_dir",
            "FFM_BACKUP_DIR",
            parsed.storage.backup_dir,
        )?;

        let cfg = Config {
            db_path: PathBuf::from(&db_path.value),
            receipts_root: PathBuf::from(&receipts_root.value),
            backup_dir: PathBuf::from(&backup_dir.value),
            file: path.map(Path::to_path_buf),
        };
        cfg.validate(&db_path, &receipts_root, &backup_dir)?;
        Ok(cfg)
    }

    fn validate(
        &self,
        db_path: &Resolved,
        receipts_root: &Resolved,
        backup_dir: &Resolved,
    ) -> Result<(), ConfigError> {
        // The database file itself is created on first connect, but its
        // directory is not: a typo'd path would otherwise surface as an opaque
        // SQLite error several layers down.
        if let Some(parent) = self.db_path.parent().filter(|p| !p.as_os_str().is_empty())
            && !parent.is_dir()
        {
            return Err(ConfigError {
                source: db_path.source,
                key: "storage.db_path",
                problem: "is in a directory that does not exist".into(),
                value: Some(parent.display().to_string()),
            });
        }

        // The vault must exist. It is not created here on purpose: an empty
        // vault silently conjured at a mistyped path is how a household
        // discovers months later that its receipts went somewhere else.
        if !self.receipts_root.is_dir() {
            return Err(ConfigError {
                source: receipts_root.source,
                key: "storage.receipts_root",
                problem: if self.receipts_root.exists() {
                    "is not a directory".into()
                } else {
                    "does not exist".into()
                },
                value: Some(self.receipts_root.display().to_string()),
            });
        }
        if let Err(e) = writable(&self.receipts_root) {
            return Err(ConfigError {
                source: receipts_root.source,
                key: "storage.receipts_root",
                problem: format!("is not writable: {e}"),
                value: Some(self.receipts_root.display().to_string()),
            });
        }

        // Backups are entirely ours, so this one is created rather than
        // demanded — there is nothing an operator could mean by it but "put
        // them here".
        if let Err(e) = std::fs::create_dir_all(&self.backup_dir) {
            return Err(ConfigError {
                source: backup_dir.source,
                key: "storage.backup_dir",
                problem: format!("cannot be created: {e}"),
                value: Some(self.backup_dir.display().to_string()),
            });
        }
        Ok(())
    }
}

/// Environment first, then the file. No default: a silent fallback for a path
/// is how data lands somewhere nobody meant.
fn resolve(
    src: &str,
    key: &'static str,
    var: &'static str,
    from_file: Option<String>,
) -> Result<Resolved, ConfigError> {
    if let Ok(v) = std::env::var(var)
        && !v.trim().is_empty()
    {
        return Ok(Resolved {
            value: v,
            source: Source::Env(var),
        });
    }
    match from_file {
        Some(v) if !v.trim().is_empty() => Ok(Resolved {
            source: Source::File(key_line(src, key).unwrap_or(0)),
            value: v,
        }),
        _ => Err(ConfigError {
            source: Source::Default,
            key: "storage",
            problem: format!("{key} is not set — put it in ffm.toml or set {var}"),
            value: None,
        }),
    }
}

fn line_at(src: &str, offset: usize) -> usize {
    src[..offset.min(src.len())].lines().count().max(1)
}

/// Which line an operator would edit to change this key. Naive on purpose: the
/// file is three keys in one table, so there is nothing to disambiguate.
fn key_line(src: &str, key: &str) -> Option<usize> {
    src.lines()
        .position(|l| {
            let l = l.trim_start();
            l.starts_with(key) && l[key.len()..].trim_start().starts_with('=')
        })
        .map(|i| i + 1)
}

/// Actually writable, not merely "the permission bits look right" — an NFS
/// mount or a read-only bind can disagree with the mode.
fn writable(dir: &Path) -> std::io::Result<()> {
    let probe = dir.join(".ffm-write-probe");
    std::fs::write(&probe, b"")?;
    std::fs::remove_file(&probe)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = r#"
[storage]
db_path       = "./ffm.db"
receipts_root = "/disks/vault/ffm/receipts"
backup_dir    = "./backups"
"#;

    #[test]
    fn reports_the_line_an_operator_would_edit() {
        assert_eq!(key_line(FILE, "db_path"), Some(3));
        assert_eq!(key_line(FILE, "receipts_root"), Some(4));
        assert_eq!(key_line(FILE, "backup_dir"), Some(5));
        assert_eq!(key_line(FILE, "nope"), None);
    }

    #[test]
    fn a_hash_inside_a_quoted_value_is_not_a_comment() {
        // The gotcha this project already paid for: a hand-rolled stripper cut
        // `accent = "#c4a7e7"` in half at the color's own '#'. The real toml
        // crate handles it, which is the reason it is a dependency.
        let v: FileShape = toml::from_str(
            r#"
[storage]
db_path = "./a#b.db"   # trailing comment
receipts_root = "/tmp"
backup_dir = "/tmp"
"#,
        )
        .unwrap();
        assert_eq!(v.storage.db_path.unwrap(), "./a#b.db");
    }

    #[test]
    fn the_error_names_the_line_and_the_value() {
        let e = ConfigError {
            source: Source::File(3),
            key: "storage.receipts_root",
            problem: "does not exist".into(),
            value: Some("/disks/vault/ffm".into()),
        };
        assert_eq!(
            e.to_string(),
            "✗ line 3 · storage.receipts_root does not exist — /disks/vault/ffm"
        );
    }

    #[test]
    fn an_env_sourced_error_points_at_the_variable() {
        let e = ConfigError {
            source: Source::Env("FFM_RECEIPTS_ROOT"),
            key: "storage.receipts_root",
            problem: "does not exist".into(),
            value: Some("/nope".into()),
        };
        assert_eq!(
            e.to_string(),
            "✗ $FFM_RECEIPTS_ROOT · storage.receipts_root does not exist — /nope"
        );
    }

    #[test]
    fn a_missing_key_says_both_places_it_could_be_set() {
        // Safe from a stray real environment: the test process does not set it.
        let err = resolve("[storage]\n", "receipts_root", "FFM_TEST_UNSET_VAR", None).unwrap_err();
        assert!(err.problem.contains("ffm.toml"), "{}", err.problem);
        assert!(
            err.problem.contains("FFM_TEST_UNSET_VAR"),
            "{}",
            err.problem
        );
    }
}
