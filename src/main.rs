use ffm::{config::Config, db, state::AppState, web};

#[tokio::main]
async fn main() {
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

    // "Today" is the process's LOCAL date, and every date-shaped decision in
    // the app reads it: the due ring, `scheduled overdue`, the unresolved
    // badge, and the materializer's current month. ffm.toml has no timezone key
    // and should not grow one — one process serves one household in one place,
    // so the deployment sets TZ. Logging it is what gives a household watching
    // a bill go overdue at 21:00 any way to find out why.
    tracing::info!(
        "ffm · timezone {} · today {}",
        timezone_name(),
        ffm::ui::fmt::date(chrono::Local::now().date_naive())
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

    // One pass now, then one an hour. Startup matters as much as the tick: a
    // process that was down across a month boundary has to catch up the moment
    // it is back, not up to an hour later.
    ffm::services::materializer::spawn(pool.clone());

    let state = AppState {
        pool,
        cfg: std::sync::Arc::new(cfg),
    };

    let app = web::router(state);

    #[cfg(feature = "dev-reload")]
    let app = app.layer(tower_livereload::LiveReloadLayer::new());

    let addr = std::env::var("FFM_ADDR").unwrap_or_else(|_| "127.0.0.1:3000".into());
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .unwrap_or_else(|e| panic!("cannot bind {addr}: {e}"));

    // Bound to a wildcard, "http://0.0.0.0:3000" is not something anybody can
    // type into a phone. Resolve the address this machine actually answers on
    // and print that instead, so the URL in the log is the URL to open.
    let shown = match listener.local_addr() {
        Ok(local) if local.ip().is_unspecified() => lan_addr()
            .map(|ip| format!("{ip}:{}", local.port()))
            .unwrap_or_else(|| addr.clone()),
        _ => addr.clone(),
    };

    tracing::info!("ffm · http://{shown}");
    #[cfg(debug_assertions)]
    tracing::info!("sandbox · http://{shown}/__sandbox/rows");

    axum::serve(listener, app).await.unwrap();
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
