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

    tracing::info!("ffm · http://{addr}");
    #[cfg(debug_assertions)]
    tracing::info!("sandbox · http://{addr}/__sandbox/rows");

    axum::serve(listener, app).await.unwrap();
}
