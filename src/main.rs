use ffm::db::auth;
use ffm::db::changes::Actor;
use ffm::{config::Config, db, state::AppState, web};

#[tokio::main]
async fn main() {
    // Parsed before anything else is set up, so `--help` and a typo both
    // answer instantly instead of after opening a database they never needed.
    let cmd = match Cmd::parse(std::env::args().skip(1)) {
        Ok(Cmd::Help) => {
            println!("{USAGE}");
            return;
        }
        Ok(c) => c,
        Err(e) => {
            eprintln!("✗ {e}");
            eprintln!();
            eprintln!("{USAGE}");
            std::process::exit(2);
        }
    };

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "ffm=debug,tower_http=debug".into()),
        )
        .init();

    // Config failures name the line to edit and stop the process. For three
    // paths there is no sensible partial state: starting with the vault
    // pointing nowhere would file receipts into a directory nobody meant.
    let cfg = match Config::load() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{e}");
            eprintln!(
                "  see ffm.example.toml, or set FFM_DB_PATH / FFM_RECEIPTS_ROOT / FFM_BACKUP_DIR"
            );
            std::process::exit(1);
        }
    };

    // `FFM_TODAY` runs a dev server as of another date — `just dev 2026-10-15`
    // sets it, against a throwaway copy of the database in ./demo. Debug
    // builds only: a release binary says it is ignoring it and keeps today.
    match ffm::clock::demo_from_env() {
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
        Ok(None) => {}
        #[cfg(debug_assertions)]
        Ok(Some(date)) => {
            let real = chrono::Local::now().date_naive();
            ffm::clock::pretend(date);
            tracing::warn!(
                "ffm · DEMO · today {} (really {}) · the clock runs from there",
                ffm::ui::fmt::date(date),
                ffm::ui::fmt::date(real)
            );
        }
        #[cfg(not(debug_assertions))]
        Ok(Some(_)) => {
            tracing::warn!("FFM_TODAY is a dev-only setting — ignored in a release build");
        }
    }

    // "Today" is the process's LOCAL date, and every date-shaped decision in
    // the app reads it: the due ring, `scheduled overdue`, the unresolved
    // badge, and the materializer's current month. ffm.toml has no timezone key
    // and should not grow one — one process serves one household in one place,
    // so the deployment sets TZ. Logging it is what gives a household watching
    // a bill go overdue at 21:00 any way to find out why.
    tracing::info!(
        "ffm · timezone {} · today {}",
        timezone_name(),
        ffm::ui::fmt::date(ffm::clock::today())
    );
    tracing::info!(
        "db {} · receipts {} · backups {}",
        cfg.db_path.display(),
        cfg.receipts_root.display(),
        cfg.backup_dir.display()
    );

    let pool = match db::connect(&cfg).await {
        Ok(p) => p,
        Err(e) => {
            eprintln!("✗ cannot open {} — {e}", cfg.db_path.display());
            std::process::exit(1);
        }
    };

    // A console command runs against the same config and the same migrated
    // schema the server would have used, then exits. It deliberately happens
    // after the two log lines above: "which database did that touch" is the
    // first thing anyone asks afterwards.
    if let Cmd::ResetLink(name) = &cmd {
        std::process::exit(reset_link(&pool, name).await);
    }

    // Before anything reads a receipt path: folders named `{uuid}-{slug}` by
    // an earlier version move to `{slug}`. A no-op once done.
    if let Err(e) = ffm::services::vault::drop_uuid_prefix(&pool, &cfg.receipts_root).await {
        eprintln!("{e}");
        std::process::exit(1);
    }
    // And into the one-trash-per-member layout: a `.delete/` beside a file
    // moves into the member's trash, and a deleted row's still-live files
    // follow it there. Also a no-op once done.
    if let Err(e) = ffm::services::vault::tidy_trash(&pool, &cfg.receipts_root).await {
        eprintln!("{e}");
        std::process::exit(1);
    }
    // And every file where its row now looks: a rule renamed, or a row given
    // to another member, by v0.0.5 or earlier left its files behind. A no-op
    // once done; a file whose new path is taken stays put and is logged.
    match ffm::services::vault::refile_all(&pool, &cfg.receipts_root).await {
        Ok((0, _)) => {}
        Ok((n, blocked)) => tracing::info!(
            "vault · {n} file(s) re-filed where their row now looks, {} left in place",
            blocked.len()
        ),
        Err(e) => tracing::warn!("{e}"),
    }
    // And `config/issuers.toml` retires: a rule still bound to one of its
    // profiles gets it as its own recipe, bound by CNPJ. A no-op once done.
    match ffm::db::recipes::convert_profiles(&pool, &cfg.issuers_path()).await {
        Ok(c) if c.recipes > 0 || c.unbound > 0 => tracing::info!(
            "issuers.toml · {} rule(s) given their recipe, {} unbound from a profile with no CNPJ",
            c.recipes,
            c.unbound
        ),
        Ok(_) => {}
        Err(e) => {
            eprintln!("✗ converting config/issuers.toml: {e}");
            std::process::exit(1);
        }
    }

    // One pass now, then one an hour. Startup matters as much as the tick: a
    // process that was down across a month boundary has to catch up the moment
    // it is back, not up to an hour later.
    ffm::services::materializer::spawn(pool.clone());
    // One snapshot now, then one a day. Startup matters for the same reason it
    // does for the materializer: a process that was down for a week should not
    // also be a week without a backup.
    ffm::services::backup::spawn(pool.clone(), cfg.backup_dir.clone());

    let state_pool = pool.clone();
    let receipts_root = cfg.receipts_root.clone();
    let (live, stop_live) = ffm::services::live::Live::new();
    let state = AppState {
        pool,
        cfg: std::sync::Arc::new(cfg),
        live: live.clone(),
    };

    let app = web::router(state);

    let addr = std::env::var("FFM_ADDR").unwrap_or_else(|_| "127.0.0.1:3000".into());
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .unwrap_or_else(|e| panic!("cannot bind {addr}: {e}"));
    // dev or dev-lan is decided by what was actually bound, not by the flag
    // that asked for it — the badge must say what the house can reach.
    if let Ok(local) = listener.local_addr() {
        ffm::run::bound(local.ip());
    }

    // Bound to a wildcard, "http://0.0.0.0:3000" is not something anybody can
    // type into a phone. Resolve the address this machine actually answers on
    // and print that instead, so the URL in the log is the URL to open.
    let shown = match listener.local_addr() {
        Ok(local) if local.ip().is_unspecified() => lan_addr()
            .map(|ip| format!("{ip}:{}", local.port()))
            .unwrap_or_else(|| addr.clone()),
        _ => addr.clone(),
    };

    tracing::info!(
        "ffm {} · {} · http://{shown}",
        ffm::version(),
        ffm::run::line()
    );
    #[cfg(debug_assertions)]
    tracing::info!("sandbox · http://{shown}/__sandbox/rows");

    // Files changed by hand: watched from now on, and caught up on for the
    // time the process was down. Both are their own tasks, started after the
    // bind, so the server answers from the first moment.
    // Every change in the log, to every open page — the watcher only adds
    // what it finds changed by hand.
    ffm::services::live_feed::spawn(state_pool.clone(), live.clone());
    ffm::services::watch::spawn(state_pool.clone(), receipts_root.clone(), live.clone());
    ffm::services::watch::catch_up(state_pool.clone(), receipts_root, live);

    // WAL mode means `ffm.db` is only the whole database when read WITH its
    // sidecar. Checkpointing on the way out leaves a file that is complete on
    // its own — which is what whoever copies it will assume.
    let shutdown = {
        let pool = state_pool.clone();
        async move {
            let ctrl_c = tokio::signal::ctrl_c();
            #[cfg(unix)]
            {
                let mut term =
                    tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                        .expect("SIGTERM handler");
                tokio::select! {
                    _ = ctrl_c => {}
                    _ = term.recv() => {}
                }
            }
            #[cfg(not(unix))]
            let _ = ctrl_c.await;

            tracing::info!("shutting down");
            // Every open live stream ends here, or graceful shutdown would
            // wait on them forever — an SSE response never finishes by itself.
            let _ = stop_live.send(true);
            ffm::services::backup::checkpoint(&pool).await;
            pool.close().await;
        }
    };

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown)
        .await
        .unwrap();
}

/// The IANA name, for the boot line. `chrono::Local` knows the offset but not
/// the name, so this reads the two places the name actually lives: `TZ`, then
/// what `/etc/localtime` points at.
fn timezone_name() -> String {
    if let Ok(tz) = std::env::var("TZ")
        && !tz.trim().is_empty()
    {
        return tz;
    }
    if let Ok(link) = std::fs::read_link("/etc/localtime") {
        let p = link.to_string_lossy();
        if let Some(i) = p.find("zoneinfo/") {
            return p[i + "zoneinfo/".len()..].to_string();
        }
    }
    // Not knowing the name is not a reason to hide the offset — that is the
    // number that actually decides whether a bill is overdue.
    format!("UTC{}", chrono::Local::now().offset())
}

/// The address of the interface this machine would use to reach the rest of the
/// network. Opening a UDP socket sends nothing — it only asks the routing table
/// which local address a packet to that destination would leave from, which is
/// exactly the one a phone on the same LAN can reach back on.
fn lan_addr() -> Option<std::net::IpAddr> {
    let sock = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    sock.connect("192.0.2.1:80").ok()?; // TEST-NET-1, routed but never answered
    sock.local_addr().ok().map(|a| a.ip())
}

// ---------------------------------------------------------------------------
// the console
// ---------------------------------------------------------------------------

const USAGE: &str = "\
ffm — a shared household ledger

  ffm                          serve the ledger (the default)
  ffm serve                    the same thing, said out loud
  ffm reset-link <name>        issue a password-reset link for a member
  ffm --help                   this

`reset-link` is the way back in when the only admin has forgotten their own
password — the one account nobody else can reset. It prints the same link
:members issues: single-use, good for 5 days, and it signs that member out
everywhere. It never prints or sets a password; the member picks their own on
the link, exactly as they would have.

Which database it touches comes from ffm.toml or FFM_DB_PATH, and is logged.";

/// What this invocation is. Hand-rolled rather than pulled from an argument
/// parser: there are two commands and one argument between them, and a
/// dependency that generates `--help` for that is more machinery than the
/// thing it parses.
enum Cmd {
    Serve,
    ResetLink(String),
    Help,
}

impl Cmd {
    fn parse(mut args: impl Iterator<Item = String>) -> Result<Cmd, String> {
        let Some(first) = args.next() else {
            return Ok(Cmd::Serve);
        };

        match first.as_str() {
            "serve" => Ok(Cmd::Serve),
            "help" | "-h" | "--help" => Ok(Cmd::Help),
            "reset-link" => {
                // A member name can contain spaces ("Maria Clara"), so the
                // shell has to quote it — and an unquoted one arrives here as
                // two arguments. Saying which is wrong beats resetting the
                // wrong account because only the first word was read.
                let name = args
                    .next()
                    .ok_or("reset-link needs a member name: ffm reset-link rodrigo")?;
                let rest: Vec<String> = args.collect();
                if !rest.is_empty() {
                    return Err(format!(
                        "reset-link takes one name. Quote it if it has spaces: \
                         ffm reset-link \"{name} {}\"",
                        rest.join(" ")
                    ));
                }
                Ok(Cmd::ResetLink(name))
            }
            other => Err(format!("not a command: {other}")),
        }
    }
}

/// Issues a reset link for `name` and prints it. Returns the process's exit
/// code, so a script can tell "no such member" from "here is your link".
///
/// This is deliberately the *same* call `:members` makes rather than a
/// shortcut that sets `password_hash` directly. Two reasons, both of which get
/// quietly lost if somebody later "simplifies" this into an UPDATE:
///
/// * There stays exactly one code path that writes a password — the accept
///   form — so a change to how passwords are stored cannot reach one and miss
///   the other.
/// * A reset hands over the ability to write to the ledger as somebody else,
///   which is the one thing preview-as-non-admin was built to refuse. The web
///   path pays for that with two change-log entries and every session revoked;
///   a console path that skipped them would be the same power, unrecorded.
///
/// A password on `argv` was never on the table either: it lands in the shell's
/// history file and in `ps` output for every user on the machine.
async fn reset_link(pool: &sqlx::SqlitePool, name: &str) -> i32 {
    let member = match auth::member_by_name(pool, name).await {
        Ok(Some(m)) => m,
        Ok(None) => {
            eprintln!("✗ no member called \"{name}\"");
            match ffm::db::ledger::members(pool).await {
                Ok(all) if !all.is_empty() => {
                    let names: Vec<String> =
                        all.iter().map(|m| ffm::ui::fmt::name(&m.name)).collect();
                    eprintln!("  this household has: {}", names.join(", "));
                }
                // An empty members table is not a typo, it is a household that
                // was never set up — and /setup only exists while it is empty,
                // so that is genuinely the next thing to do.
                Ok(_) => eprintln!("  this household has no members yet — open /setup"),
                Err(e) => eprintln!("  (could not list the others — {e})"),
            }
            return 1;
        }
        Err(e) => {
            eprintln!("✗ cannot read members — {e}");
            return 1;
        }
    };

    let role = if member.is_admin { 0 } else { 1 };
    let token = match auth::issue_link(pool, Actor::System, Some(member.id), None, role).await {
        Ok(t) => t,
        Err(e) => {
            eprintln!("✗ could not issue the link — {e}");
            return 1;
        }
    };

    let expires = (ffm::clock::now() + chrono::Duration::days(auth::LINK_DAYS)).date_naive();

    println!();
    println!(
        "✓ reset link for {} · expires {}",
        ffm::ui::fmt::name(&member.name),
        ffm::ui::fmt::date(expires)
    );
    println!();
    println!("  http://{}/invite/{token}", console_addr());
    println!();
    println!(
        "  It works once. {} is now signed out everywhere, and",
        ffm::ui::fmt::name(&member.name)
    );
    println!("  sets their own password on that page — nobody types it for them.");
    println!();

    0
}

/// The address to put in the printed link. The server resolves this from the
/// listener it actually bound; there is no listener here, so `FFM_ADDR` is read
/// the same way and a wildcard is resolved the same way — a link saying
/// `0.0.0.0:3000` is one nobody can open.
fn console_addr() -> String {
    let raw = std::env::var("FFM_ADDR").unwrap_or_else(|_| "127.0.0.1:3000".into());
    match raw.parse::<std::net::SocketAddr>() {
        Ok(s) if s.ip().is_unspecified() => lan_addr()
            .map(|ip| format!("{ip}:{}", s.port()))
            .unwrap_or(raw),
        _ => raw,
    }
}
