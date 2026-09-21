use ffm::web;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "ffm=debug,tower_http=debug".into()),
        )
        .init();

    let app = web::router();

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

/// The address of the interface this machine would use to reach the rest of the
/// network. Opening a UDP socket sends nothing — it only asks the routing table
/// which local address a packet to that destination would leave from, which is
/// exactly the one a phone on the same LAN can reach back on.
fn lan_addr() -> Option<std::net::IpAddr> {
    let sock = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    sock.connect("192.0.2.1:80").ok()?; // TEST-NET-1, routed but never answered
    sock.local_addr().ok().map(|a| a.ip())
}
