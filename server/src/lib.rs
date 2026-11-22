//! Razdor's API server: the analytics (`2026-11-22-analytics-design.md`) and the cloud saves
//! (`2026-11-22-cloud-saves-design.md`), one binary behind nginx on `api.<domain>`. The shared
//! formats and limits are in the `razdor-wire` crate; every body is checked with its `validate`.

pub mod abuse;
pub mod admin;
pub mod analytics;
pub mod budget;
pub mod civil;
pub mod clock;
pub mod cloud;
pub mod config;
pub mod db;
pub mod files;
pub mod http;
pub mod retention;
pub mod schema;
pub mod state;
pub mod summary;

use std::net::SocketAddr;

use axum::extract::{DefaultBodyLimit, State};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use razdor_wire::{limits, paths};

use crate::http::ApiError;
use crate::state::AppState;

/// Every route with its body cap. A body over its cap is refused by the framework (413) before any
/// handler reads it.
pub fn router(state: AppState) -> Router {
    let save_body = limits::MAX_SAVE_BODY + limits::MAX_FRAME_HEADER + 1024;
    let battle_body = limits::MAX_BATTLE_BODY + limits::MAX_FRAME_HEADER + 1024;
    let small = DefaultBodyLimit::max(limits::MAX_SMALL_BODY);
    Router::new()
        .route("/healthz", get(healthz))
        .route(paths::PING, post(analytics::ping).layer(small))
        .route(paths::EVENTS, post(analytics::events).layer(DefaultBodyLimit::max(limits::MAX_EVENT_BODY)))
        .route(paths::PERF, post(analytics::perf).layer(DefaultBodyLimit::max(limits::MAX_PERF_BODY)))
        .route(paths::CRASH, post(analytics::crash).layer(DefaultBodyLimit::max(limits::MAX_CRASH_BODY)))
        .route(paths::CRASH_SAVE, post(analytics::crash_save).layer(DefaultBodyLimit::max(save_body)))
        .route(paths::BATTLES, post(analytics::battles).layer(DefaultBodyLimit::max(battle_body)))
        .route(paths::VERDICTS, post(analytics::verdict).layer(small))
        .route("/v1/battles/pending", get(analytics::battles_pending))
        .route("/v1/battles/{id}", get(analytics::battle_get))
        .route(paths::INSTALL_DELETE, post(analytics::install_delete).layer(small))
        .route(paths::ACCOUNT, post(cloud::new_account).layer(small).delete(cloud::delete_account))
        .route(paths::LINK, post(cloud::link_issue))
        .route(paths::LINK_REDEEM, post(cloud::link_redeem).layer(small))
        .route(
            paths::SAVES,
            get(cloud::list_saves).put(cloud::put_save).layer(DefaultBodyLimit::max(save_body)),
        )
        .route("/v1/saves/{id}", get(cloud::get_save).delete(cloud::delete_save))
        .route(
            paths::MARKS,
            get(cloud::get_marks).put(cloud::put_marks).layer(DefaultBodyLimit::max(limits::MAX_MARKS_BODY)),
        )
        .route(
            paths::SETTINGS,
            get(cloud::get_settings).put(cloud::put_settings).layer(DefaultBodyLimit::max(limits::MAX_SETTINGS_BODY)),
        )
        .route(paths::HUB, get(cloud::get_hub).put(cloud::put_hub).layer(DefaultBodyLimit::max(limits::MAX_HUB_BODY)))
        .route(
            paths::BENCH,
            get(cloud::get_bench).post(cloud::post_bench).layer(DefaultBodyLimit::max(limits::MAX_BENCH_BODY)),
        )
        .route("/admin", get(admin::page_get))
        .route("/admin/login", post(admin::login).layer(small))
        .route("/admin/logout", post(admin::logout))
        .route("/admin/unban", post(admin::unban).layer(small))
        .route("/admin/api/summary.json", get(admin::summary_json))
        .with_state(state)
}

async fn healthz(State(st): State<AppState>) -> Response {
    match st.stats.read(|c| Ok(c.query_row("SELECT 1", [], |r| r.get::<_, i64>(0))?)).await {
        Ok(_) => "ok".into_response(),
        Err(_) => ApiError::Internal("health check".into()).into_response(),
    }
}

/// Runs the service until SIGINT or SIGTERM, then folds the WALs into their files.
pub async fn serve(state: AppState) -> std::io::Result<()> {
    let listener = tokio::net::TcpListener::bind(state.cfg.bind).await?;
    tracing::info!("listening on {}", state.cfg.bind);
    retention::spawn(state.clone());
    retention::spawn_minutely(state.clone());
    let app = router(state.clone()).into_make_service_with_connect_info::<SocketAddr>();
    axum::serve(listener, app).with_graceful_shutdown(shutdown_signal()).await?;
    for db in [&state.stats, &state.cloud] {
        if let Err(e) = db.checkpoint().await {
            tracing::warn!("checkpoint at shutdown: {e:?}");
        }
    }
    tracing::info!("stopped");
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut s) => {
                s.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
    tracing::info!("shutting down");
}
