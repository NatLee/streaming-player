//! Player controls shared by the admin page and chat commands.

use anyhow::Result;

use crate::{
    events, live,
    models::{EndReason, Entry},
    player::Seek,
    queue,
    state::AppState,
    util::now_ms,
};

pub async fn set_paused(state: &AppState, paused: bool, actor: &str) -> Result<()> {
    let snapshot = {
        let mut p = state.player.lock().unwrap();
        p.set_paused(paused, now_ms());
        p.clone()
    };
    snapshot.persist(&state.db).await?;
    events::record(
        &state.db,
        actor,
        if paused { "pause" } else { "resume" },
        if paused {
            "暫停播放"
        } else {
            "繼續播放"
        },
    )
    .await;
    live::publish(state).await;
    Ok(())
}

pub async fn set_volume(state: &AppState, volume: Option<u8>, muted: Option<bool>) -> Result<()> {
    let snapshot = {
        let mut p = state.player.lock().unwrap();
        if let Some(v) = volume {
            p.volume = v.min(100);
        }
        if let Some(m) = muted {
            p.muted = m;
        }
        p.clone()
    };
    snapshot.persist(&state.db).await?;
    live::publish(state).await;
    Ok(())
}

/// What a seek did.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Seeked {
    /// The song is now at this position (milliseconds).
    To(i64),
    NothingOnAir,
    /// No player has started the song yet, so there is no position to move.
    NotStarted,
}

/// Move the song on air (the seek bar and the back/forward buttons). Every player page,
/// the leader too, jumps to the new position when the state arrives.
pub async fn seek(state: &AppState, to: Seek) -> Result<Seeked> {
    let Some(current) = queue::current(&state.db).await? else {
        return Ok(Seeked::NothingOnAir);
    };
    let duration_ms = current.song.duration_secs.map(|s| s.saturating_mul(1000));
    let moved = state
        .player
        .lock()
        .unwrap()
        .seek(current.id, to, duration_ms, now_ms());
    let Some(pos) = moved else {
        return Ok(Seeked::NotStarted);
    };
    live::publish(state).await;
    Ok(Seeked::To(pos))
}

/// Skip the song on air; optionally put the song on the block list too
/// (v1's 「切歌並封鎖」).
pub async fn skip(state: &AppState, block: bool, actor: &str) -> Result<Option<Entry>> {
    let Some(e) = queue::finish_current(state, None, EndReason::Skipped).await? else {
        return Ok(None);
    };
    if block {
        sqlx::query("UPDATE songs SET blocked = 1 WHERE id = ?")
            .bind(e.song.id)
            .execute(&state.db)
            .await?;
    }
    let what = if block { "跳過並封鎖" } else { "跳過" };
    events::record(
        &state.db,
        actor,
        "skip",
        &format!("{what}『{}』", e.song.title),
    )
    .await;
    live::notice(
        state,
        "skip",
        &format!("{actor} {what}了『{}』", e.song.title),
    );
    Ok(Some(e))
}
