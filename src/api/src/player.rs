//! Playback state shared by every player page: pause, volume, and the song clock.
//!
//! The clock is how several devices stay in sync. The "leader" player (the oldest
//! connected OBS/player page, see `live.rs`) reports when the song starts and where it
//! is; everyone else follows the server's idea of the position. A seek from the admin
//! page moves the clock itself, and every player (the leader too) jumps to it.

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
    /// Goes up by one on every seek. Players send back the number they have applied, so
    /// a report written before the jump can't pull the clock back to the old position.
    pub seek_seq: u32,
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

/// Where to move the current song.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Seek {
    /// To this position, in milliseconds from the start.
    To(i64),
    /// By this much from where it is now (negative goes back).
    By(i64),
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

    /// The leader player says the song started (or is at `pos_ms`). Ignored for other
    /// songs, and when the player has not caught up with the latest seek yet.
    pub fn sync(&mut self, request_id: i64, seek_seq: u32, pos_ms: i64, now_ms: i64) -> bool {
        if self.clock.request_id != Some(request_id) || self.clock.seek_seq != seek_seq {
            return false;
        }
        self.clock.started = true;
        self.clock.pos_ms = pos_ms.max(0);
        self.clock.at_ms = now_ms;
        true
    }

    /// Move song `request_id` to another position, kept between the start and
    /// `duration_ms` (when known). Returns the new position, or `None` when that song is
    /// not the one on air or no player has started it yet (there is nothing to move).
    pub fn seek(
        &mut self,
        request_id: i64,
        to: Seek,
        duration_ms: Option<i64>,
        now_ms: i64,
    ) -> Option<i64> {
        let c = &self.clock;
        if c.request_id != Some(request_id) || !c.started {
            return None;
        }
        let target = match to {
            Seek::To(ms) => ms,
            Seek::By(delta) => self.elapsed_ms(now_ms).saturating_add(delta),
        };
        let pos = target.clamp(0, duration_ms.unwrap_or(i64::MAX).max(0));
        let c = &mut self.clock;
        c.pos_ms = pos;
        c.at_ms = now_ms;
        c.seek_seq = c.seek_seq.wrapping_add(1);
        Some(pos)
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
        assert!(p.sync(7, 0, 0, 10_000));
        assert_eq!(p.elapsed_ms(12_500), 2_500);
        assert!(
            !p.sync(8, 0, 0, 12_500),
            "reports for another song are ignored"
        );
    }

    #[test]
    fn pause_freezes_the_clock() {
        let mut p = PlayerState::default();
        p.reset_for(Some(1));
        p.sync(1, 0, 1_000, 0);
        p.set_paused(true, 4_000);
        assert_eq!(p.elapsed_ms(60_000), 5_000);
        p.set_paused(false, 60_000);
        assert_eq!(p.elapsed_ms(61_000), 6_000);
    }

    #[test]
    fn seek_moves_the_clock_and_drops_older_reports() {
        let mut p = PlayerState::default();
        p.reset_for(Some(3));
        assert_eq!(
            p.seek(3, Seek::To(9_000), None, 1_000),
            None,
            "nothing to move before a player starts the song"
        );
        p.sync(3, 0, 0, 0);

        assert_eq!(
            p.seek(3, Seek::By(5_000), Some(200_000), 10_000),
            Some(15_000)
        );
        assert_eq!(p.elapsed_ms(11_000), 16_000);
        assert_eq!(p.clock.seek_seq, 1);
        assert!(
            !p.sync(3, 0, 10_500, 11_000),
            "a progress report sent before the jump is dropped"
        );
        assert_eq!(p.elapsed_ms(11_000), 16_000);
        assert!(p.sync(3, 1, 16_200, 11_000), "one that saw the jump counts");
        assert_eq!(p.elapsed_ms(11_000), 16_200);

        assert_eq!(p.seek(4, Seek::To(0), None, 11_000), None, "another song");
        assert_eq!(
            p.seek(3, Seek::By(-60_000), None, 11_000),
            Some(0),
            "not before the start"
        );
        assert_eq!(
            p.seek(3, Seek::To(500_000), Some(200_000), 11_000),
            Some(200_000),
            "not past the end"
        );
        p.reset_for(Some(5));
        assert_eq!(p.clock.seek_seq, 0, "a new song starts counting again");
    }

    #[test]
    fn seek_while_paused_stays_paused() {
        let mut p = PlayerState::default();
        p.reset_for(Some(1));
        p.sync(1, 0, 0, 0);
        p.set_paused(true, 30_000);
        assert_eq!(p.seek(1, Seek::By(-5_000), None, 40_000), Some(25_000));
        assert_eq!(p.elapsed_ms(90_000), 25_000);
        p.set_paused(false, 90_000);
        assert_eq!(p.elapsed_ms(92_000), 27_000);
    }
}
