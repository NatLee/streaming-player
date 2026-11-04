use std::{
    collections::HashMap,
    sync::{Arc, Mutex, RwLock},
    time::Duration,
};

use sqlx::SqlitePool;

use crate::{
    chat, config::Config, live::Hub, media::stream::StreamCache, player::PlayerState,
    settings::Settings,
};

pub type AppState = Arc<Inner>;

pub struct Inner {
    pub db: SqlitePool,
    pub config: Config,
    pub http: reqwest::Client,
    /// Queue changes run one at a time, so two chat requests can't take the same slot.
    pub queue_lock: tokio::sync::Mutex<()>,
    pub player: Mutex<PlayerState>,
    pub hub: Hub,
    settings: RwLock<Settings>,
    /// Lower-case username → recent failed sign-in times (brute-force guard).
    pub login_failures: Mutex<HashMap<String, Vec<i64>>>,
    /// Google sign-ins waiting for the redirect back (state → verifier, nonce).
    pub oauth_pending: Mutex<HashMap<String, crate::google::PendingLogin>>,
    /// Client address → recent registration attempts (anti-flood).
    pub register_attempts: Mutex<Vec<(String, i64)>>,
    /// HS256 key for access tokens (`tokens.rs`).
    pub jwt_key: Vec<u8>,
    /// (command id, user key) → last time the user ran the command.
    pub cooldowns: Mutex<HashMap<(i64, String), i64>>,
    pub streams: StreamCache,
    pub chat: chat::Manager,
}

impl Inner {
    pub fn new(
        db: SqlitePool,
        config: Config,
        settings: Settings,
        player: PlayerState,
        jwt_key: Vec<u8>,
    ) -> anyhow::Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .user_agent(concat!("streaming-player/", env!("CARGO_PKG_VERSION")))
            .build()?;
        Ok(Self {
            db,
            config,
            http,
            queue_lock: tokio::sync::Mutex::new(()),
            player: Mutex::new(player),
            hub: Hub::new(),
            settings: RwLock::new(settings),
            login_failures: Mutex::new(HashMap::new()),
            oauth_pending: Mutex::new(HashMap::new()),
            register_attempts: Mutex::new(Vec::new()),
            jwt_key,
            cooldowns: Mutex::new(HashMap::new()),
            streams: StreamCache::default(),
            chat: chat::Manager::default(),
        })
    }

    pub fn settings(&self) -> Settings {
        self.settings.read().unwrap().clone()
    }

    pub async fn save_settings(&self, settings: Settings) -> anyhow::Result<()> {
        crate::settings::save(&self.db, &settings).await?;
        *self.settings.write().unwrap() = settings;
        Ok(())
    }
}
