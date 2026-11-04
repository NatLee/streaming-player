//! Live updates over WebSocket: every page (OBS player, overlay, admin, viewers) gets the
//! same state snapshot whenever something changes, plus short notices for the overlay.
//!
//! Player pages also elect a leader: the oldest connected player reports start, progress
//! and end of each song. Only one device advances the queue, however many are open.

use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Mutex,
};

use anyhow::Result;
use serde::Serialize;
use serde_json::json;
use tokio::sync::broadcast;
use utoipa::ToSchema;

use crate::{
    models::Entry,
    player::PlayerView,
    queue,
    settings::Playback,
    state::{AppState, Inner},
    util::now_ms,
};

#[derive(Debug, Clone)]
pub enum HubMsg {
    /// Serialized `{"type":"state",...}`
    State(Arc<str>),
    /// Serialized `{"type":"notice",...}`
    Notice(Arc<str>),
    /// The player connection that leads now (or none).
    Leader(Option<u64>),
}

pub struct Hub {
    tx: broadcast::Sender<HubMsg>,
    next_id: AtomicU64,
    /// Connected player pages, oldest first. The first one leads.
    players: Mutex<Vec<u64>>,
}

impl Default for Hub {
    fn default() -> Self {
        Self::new()
    }
}

impl Hub {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(64);
        Self {
            tx,
            next_id: AtomicU64::new(1),
            players: Mutex::new(Vec::new()),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<HubMsg> {
        self.tx.subscribe()
    }

    pub fn connection_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    pub fn send(&self, msg: HubMsg) {
        // No receivers is fine: nobody has a page open.
        let _ = self.tx.send(msg);
    }

    /// Register a player page; returns the current leader.
    pub fn join_player(&self, id: u64) -> Option<u64> {
        let leader = {
            let mut players = self.players.lock().unwrap();
            players.push(id);
            players.first().copied()
        };
        self.send(HubMsg::Leader(leader));
        leader
    }

    pub fn leave_player(&self, id: u64) {
        let (changed, leader) = {
            let mut players = self.players.lock().unwrap();
            let was_leader = players.first() == Some(&id);
            players.retain(|p| *p != id);
            (was_leader, players.first().copied())
        };
        if changed {
            self.send(HubMsg::Leader(leader));
        }
    }

    pub fn leader(&self) -> Option<u64> {
        self.players.lock().unwrap().first().copied()
    }

    pub fn player_count(&self) -> usize {
        self.players.lock().unwrap().len()
    }
}

/// The song on air, with the shared clock.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct NowPlaying {
    #[serde(flatten)]
    pub entry: Entry,
    pub elapsed_ms: i64,
    /// False until a player reported that the video really started.
    pub clock_started: bool,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct QueueItem {
    #[serde(flatten)]
    pub entry: Entry,
    /// Place in line, counting the song on air as #1 (as v1 did).
    pub position: u32,
    /// Seconds until this song starts (songs of unknown length count as 0).
    pub wait_secs: i64,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct LiveState {
    pub now: Option<NowPlaying>,
    pub queue: Vec<QueueItem>,
    pub player: PlayerView,
    pub requests_open: bool,
    pub playback: Playback,
    pub players_connected: usize,
    pub server_time_ms: i64,
}

pub async fn snapshot(state: &Inner) -> Result<LiveState> {
    let current = queue::current(&state.db).await?;
    let waiting = queue::queued(&state.db).await?;
    let t = now_ms();
    let (player, elapsed, started) = {
        let p = state.player.lock().unwrap();
        let matches = current.as_ref().map(|c| c.id) == p.clock.request_id;
        (
            p.view(),
            if matches { p.elapsed_ms(t) } else { 0 },
            matches && p.clock.started,
        )
    };
    let settings = state.settings();
    let lineup = queue::lineup(current.as_ref(), elapsed, waiting);
    Ok(LiveState {
        now: current.map(|entry| NowPlaying {
            entry,
            elapsed_ms: elapsed,
            clock_started: started,
        }),
        queue: lineup,
        player,
        requests_open: settings.queue.requests_open,
        playback: settings.playback,
        players_connected: state.hub.player_count(),
        server_time_ms: t,
    })
}

/// Send the new state to every open page.
pub async fn publish(state: &AppState) {
    match snapshot(state).await {
        Ok(snap) => {
            let mut value = serde_json::to_value(&snap).unwrap_or_default();
            if let Some(obj) = value.as_object_mut() {
                obj.insert("type".into(), json!("state"));
            }
            state.hub.send(HubMsg::State(value.to_string().into()));
        }
        Err(err) => tracing::warn!("could not build live state: {err:#}"),
    }
}

/// A short message for overlays, e.g. "NatLee requested …".
pub fn notice(state: &Inner, kind: &str, text: &str) {
    let msg = json!({ "type": "notice", "kind": kind, "text": text, "at": now_ms() });
    state.hub.send(HubMsg::Notice(msg.to_string().into()));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oldest_player_leads_and_hands_over() {
        let hub = Hub::new();
        let mut rx = hub.subscribe();
        assert_eq!(hub.join_player(1), Some(1));
        assert_eq!(hub.join_player(2), Some(1));
        assert_eq!(hub.leader(), Some(1));
        hub.leave_player(2);
        assert_eq!(hub.leader(), Some(1), "a follower leaving changes nothing");
        hub.leave_player(1);
        assert_eq!(hub.leader(), None);
        // join, join, leave(leader) → three leader messages
        let mut leaders = Vec::new();
        while let Ok(HubMsg::Leader(l)) = rx.try_recv() {
            leaders.push(l);
        }
        assert_eq!(leaders, vec![Some(1), Some(1), None]);
    }
}
