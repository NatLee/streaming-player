//! Playback state shared by every player page: pause, volume, and the song clock.
//!
//! The clock is how several devices stay in sync. The "leader" player (the oldest
//! connected OBS/player page, see `live.rs`) reports when the song starts and where it
//! is; everyone else follows the server's idea of the position.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use utoipa::ToSchema;

use crate::db;

const PLAYER_KEY: &str = "player";
/// Players further than this from the shared clock seek to catch up.
pub const DRIFT_TOLERANCE_MS: i64 = 3000;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Persisted {
    volume: u8,
    muted: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Clock {
    /// The request this clock belongs to.
    pub request_id: Option<i64>,
    /// False until a player reports that the video actually started.
    pub started: bool,
    pos_ms: i64,
    at_ms: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlayerState {
    pub paused: bool,
    pub volume: u8,
    pub muted: bool,
    pub clock: Clock,
}

impl Default for PlayerState {
    fn default() -> Self {
        Self {
            paused: false,
            volume: 100,
            muted: false,
            clock: Clock::default(),
        }
    }
}

/// What players see about playback.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct PlayerView {
    pub paused: bool,
    pub volume: u8,
    pub muted: bool,
}

impl PlayerState {
    pub async fn load(db: &SqlitePool) -> Result<Self> {
        let saved: Option<Persisted> = db::kv_get_json(db, PLAYER_KEY).await?;
        Ok(match saved {
            Some(p) => Self {
                volume: p.volume.min(100),
                muted: p.muted,
                ..Self::default()
            },
            None => Self::default(),
        })
    }

    pub async fn persist(&self, db: &SqlitePool) -> Result<()> {
        db::kv_set_json(
            db,
            PLAYER_KEY,
            &Persisted {
                volume: self.volume,
                muted: self.muted,
            },
        )
        .await
    }

    pub fn view(&self) -> PlayerView {
        PlayerView {
            paused: self.paused,
            volume: self.volume,
            muted: self.muted,
        }
    }

    /// Where the current song is, in milliseconds.
    pub fn elapsed_ms(&self, now_ms: i64) -> i64 {
        let c = &self.clock;
        if !c.started || self.paused {
            c.pos_ms
        } else {
            c.pos_ms + (now_ms - c.at_ms).max(0)
        }
    }

    /// A new song became current: its clock waits for a player to start it.
    pub fn reset_for(&mut self, request_id: Option<i64>) {
        self.clock = Clock {
            request_id,
            ..Clock::default()
        };
    }

    /// The leader player says the song started (or is at `pos_ms`). Ignored for other songs.
    pub fn sync(&mut self, request_id: i64, pos_ms: i64, now_ms: i64) -> bool {
        if self.clock.request_id != Some(request_id) {
            return false;
        }
        self.clock.started = true;
        self.clock.pos_ms = pos_ms.max(0);
        self.clock.at_ms = now_ms;
        true
    }

    pub fn set_paused(&mut self, paused: bool, now_ms: i64) {
        if paused == self.paused {
            return;
        }
        let pos = self.elapsed_ms(now_ms);
        self.paused = paused;
        self.clock.pos_ms = pos;
        self.clock.at_ms = now_ms;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_waits_for_start_then_runs() {
        let mut p = PlayerState::default();
        p.reset_for(Some(7));
        assert_eq!(p.elapsed_ms(10_000), 0, "not started yet");
        assert!(p.sync(7, 0, 10_000));
        assert_eq!(p.elapsed_ms(12_500), 2_500);
        assert!(
            !p.sync(8, 0, 12_500),
            "reports for another song are ignored"
        );
    }

    #[test]
    fn pause_freezes_the_clock() {
        let mut p = PlayerState::default();
        p.reset_for(Some(1));
        p.sync(1, 1_000, 0);
        p.set_paused(true, 4_000);
        assert_eq!(p.elapsed_ms(60_000), 5_000);
        p.set_paused(false, 60_000);
        assert_eq!(p.elapsed_ms(61_000), 6_000);
    }
}
