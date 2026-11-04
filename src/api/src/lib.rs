//! streaming-player: a self-hosted song request player for live streams.
//!
//! Chat (Twitch, YouTube, or any chat bot) requests songs; they wait in a queue; OBS
//! plays them through a browser source; the admin page controls everything.

pub mod auth;
pub mod chat;
pub mod commands;
pub mod config;
pub mod control;
pub mod db;
pub mod error;
pub mod events;
pub mod google;
pub mod legacy;
pub mod live;
pub mod media;
pub mod models;
pub mod player;
pub mod queue;
pub mod routes;
pub mod settings;
pub mod state;
pub mod util;

use std::sync::Arc;

use anyhow::Result;
use axum::Router;

pub use config::Config;
pub use state::{AppState, Inner};

/// TLS for outgoing connections uses ring. Several dependencies pull in rustls, so pick
/// the crypto provider once, before anything connects.
pub fn init_crypto() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

/// Open the database, load settings and commands, and prepare the shared state.
/// Chat connections are not started here; call `chat::restart` (the server does).
pub async fn build_state(config: Config) -> Result<AppState> {
    init_crypto();
    let db = db::connect(&config.database_path).await?;
    commands::seed_defaults(&db).await?;
    let settings = settings::load(&db).await?;
    let mut player = player::PlayerState::load(&db).await?;
    // A song left "playing" by the last run continues from the start.
    player.reset_for(queue::current(&db).await?.map(|e| e.id));
    Ok(Arc::new(Inner::new(db, config, settings, player)?))
}

pub fn app(state: AppState) -> Router {
    routes::router(state)
}
