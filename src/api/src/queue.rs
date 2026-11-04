//! The request queue: rules, ordering, votes and moving from one song to the next.
//!
//! Every change takes `queue_lock`, writes in one transaction, then publishes the new
//! state to all pages. The rule check (`check`) is a pure function so it can be tested
//! without a database.

use anyhow::Result;
use serde::Serialize;
use sqlx::{Sqlite, SqlitePool, Transaction};
use utoipa::ToSchema;

use crate::{
    db, events,
    live::{self, QueueItem},
    models::{EndReason, Entry, Requester, Song, Source, ENTRY_COLUMNS},
    settings::QueueRules,
    state::{AppState, Inner},
    util::now,
};

/// Shown as the requester of songs picked from the default playlist.
pub const AUTOPLAY_NAME: &str = "自動播放";
pub const AUTOPLAY_KEY: &str = "autoplay";

// ───────────────────────────── reading ─────────────────────────────

pub async fn current(db: &SqlitePool) -> Result<Option<Entry>> {
    let sql = format!(
        "SELECT {ENTRY_COLUMNS} FROM requests r JOIN songs s ON s.id = r.song_id \
         WHERE r.status = 'playing' LIMIT 1"
    );
    Ok(sqlx::query_as(&sql).fetch_optional(db).await?)
}

pub async fn queued(db: &SqlitePool) -> Result<Vec<Entry>> {
    let sql = format!(
        "SELECT {ENTRY_COLUMNS} FROM requests r JOIN songs s ON s.id = r.song_id \
         WHERE r.status = 'queued' ORDER BY r.position, r.id"
    );
    Ok(sqlx::query_as(&sql).fetch_all(db).await?)
}

pub async fn entry(db: &SqlitePool, request_id: i64) -> Result<Option<Entry>> {
    let sql = format!(
        "SELECT {ENTRY_COLUMNS} FROM requests r JOIN songs s ON s.id = r.song_id WHERE r.id = ?"
    );
    Ok(sqlx::query_as(&sql)
        .bind(request_id)
        .fetch_optional(db)
        .await?)
}

/// Songs a person has waiting or playing, in play order.
pub async fn pending_for(db: &SqlitePool, key: &str) -> Result<Vec<Entry>> {
    let sql = format!(
        "SELECT {ENTRY_COLUMNS} FROM requests r JOIN songs s ON s.id = r.song_id \
         WHERE r.status IN ('playing', 'queued') AND r.requester_key = ? \
         ORDER BY r.status = 'queued', r.position, r.id"
    );
    Ok(sqlx::query_as(&sql).bind(key).fetch_all(db).await?)
}

/// Turn the waiting list into numbered places with waiting times.
pub fn lineup(current: Option<&Entry>, elapsed_ms: i64, waiting: Vec<Entry>) -> Vec<QueueItem> {
    let offset = u32::from(current.is_some());
    let mut wait = current
        .and_then(|c| c.song.duration_secs)
        .map(|d| (d - elapsed_ms / 1000).max(0))
        .unwrap_or(0);
    waiting
        .into_iter()
        .enumerate()
        .map(|(i, entry)| {
            let item = QueueItem {
                position: offset + i as u32 + 1,
                wait_secs: wait,
                entry,
            };
            wait += item.entry.song.duration_secs.unwrap_or(0);
            item
        })
        .collect()
}

// ───────────────────────────── rules ─────────────────────────────

/// Why a request was refused. `outcome()` is the reply key for chat commands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rejection {
    Closed,
    UserBlocked,
    SongBlocked,
    Missing,
    TooLong {
        max_secs: u32,
    },
    Duplicate {
        id: i64,
        position: u32,
        requester: String,
        playing: bool,
    },
    QueueFull {
        max: u32,
    },
    UserLimit {
        max: u32,
        count: u32,
    },
}

impl Rejection {
    pub fn outcome(&self) -> &'static str {
        match self {
            Rejection::Closed => "closed",
            Rejection::UserBlocked => "blocked_user",
            Rejection::SongBlocked => "blocked_song",
            Rejection::Missing => "missing",
            Rejection::TooLong { .. } => "too_long",
            Rejection::Duplicate { playing: true, .. } => "duplicate_playing",
            Rejection::Duplicate { .. } => "duplicate",
            Rejection::QueueFull { .. } => "queue_full",
            Rejection::UserLimit { .. } => "user_limit",
        }
    }
}

/// What the database says about a request, gathered before `check`.
#[derive(Debug, Clone, Default)]
pub struct RequestFacts {
    pub user_blocked: bool,
    /// Songs waiting (not counting the one on air).
    pub queue_len: u32,
    /// Songs this person has waiting or playing.
    pub user_pending: u32,
    /// The same song already on air or waiting.
    pub duplicate: Option<(i64, u32, String, bool)>,
}

/// Decide whether a request may join the queue. `enforce = false` is for the admin page:
/// limits don't apply, but a blocked song is still refused.
pub fn check(
    rules: &QueueRules,
    who: &Requester,
    song: &Song,
    facts: &RequestFacts,
    enforce: bool,
) -> Result<(), Rejection> {
    if song.blocked {
        return Err(Rejection::SongBlocked);
    }
    if !enforce {
        return Ok(());
    }
    if !rules.requests_open && !who.privileged() {
        return Err(Rejection::Closed);
    }
    if facts.user_blocked {
        return Err(Rejection::UserBlocked);
    }
    if song.missing {
        return Err(Rejection::Missing);
    }
    if let Some(d) = song.duration_secs {
        if rules.max_duration_secs > 0 && d > i64::from(rules.max_duration_secs) {
            return Err(Rejection::TooLong {
                max_secs: rules.max_duration_secs,
            });
        }
    }
    if !rules.allow_duplicates {
        if let Some((id, position, requester, playing)) = &facts.duplicate {
            return Err(Rejection::Duplicate {
                id: *id,
                position: *position,
                requester: requester.clone(),
                playing: *playing,
            });
        }
    }
    if rules.max_queue > 0 && facts.queue_len >= rules.max_queue {
        return Err(Rejection::QueueFull {
            max: rules.max_queue,
        });
    }
    let bypass = rules.privileged_bypass && who.privileged();
    if rules.max_per_user > 0 && !bypass && facts.user_pending >= rules.max_per_user {
        return Err(Rejection::UserLimit {
            max: rules.max_per_user,
            count: facts.user_pending,
        });
    }
    Ok(())
}

pub async fn is_user_blocked(db: &SqlitePool, who: &Requester) -> Result<bool> {
    let hit: Option<i64> =
        sqlx::query_scalar("SELECT 1 FROM blocked_users WHERE name_key IN (?, ?) LIMIT 1")
            .bind(&who.key)
            .bind(who.name.to_lowercase())
            .fetch_optional(db)
            .await?;
    Ok(hit.is_some())
}

async fn gather_facts(state: &Inner, who: &Requester, song: &Song) -> Result<RequestFacts> {
    let db = &state.db;
    let queue_len: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM requests WHERE status = 'queued'")
            .fetch_one(db)
            .await?;
    let user_pending: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM requests WHERE status IN ('queued', 'playing') AND requester_key = ?",
    )
    .bind(&who.key)
    .fetch_one(db)
    .await?;
    let current = current(db).await?;
    let mut duplicate = None;
    if let Some(c) = current.as_ref().filter(|c| c.song.id == song.id) {
        duplicate = Some((c.id, 1, c.requester.clone(), true));
    } else {
        let waiting = queued(db).await?;
        let line = lineup(current.as_ref(), 0, waiting);
        if let Some(item) = line.iter().find(|i| i.entry.song.id == song.id) {
            duplicate = Some((
                item.entry.id,
                item.position,
                item.entry.requester.clone(),
                false,
            ));
        }
    }
    Ok(RequestFacts {
        user_blocked: is_user_blocked(db, who).await?,
        queue_len: queue_len as u32,
        user_pending: user_pending as u32,
        duplicate,
    })
}

// ───────────────────────────── changing ─────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, ToSchema, Default)]
#[serde(rename_all = "lowercase")]
pub enum Placement {
    /// Back of the queue.
    #[default]
    End,
    /// Plays after the current song.
    Next,
    /// Interrupts: plays right away, the interrupted song goes back to the front.
    Now,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Enqueued {
    pub id: i64,
    pub position: u32,
    pub wait_secs: i64,
    /// True when the song went on air right away.
    pub started: bool,
}

/// Renumber the waiting list 1..n, keeping its order.
async fn renumber(tx: &mut Transaction<'_, Sqlite>) -> Result<()> {
    sqlx::query(
        "UPDATE requests SET position = (\
           SELECT COUNT(*) FROM requests r2 WHERE r2.status = 'queued' \
           AND (r2.position < requests.position \
                OR (r2.position = requests.position AND r2.id <= requests.id))\
         ) WHERE status = 'queued'",
    )
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn front_position(tx: &mut Transaction<'_, Sqlite>) -> Result<i64> {
    Ok(sqlx::query_scalar(
        "SELECT COALESCE(MIN(position), 1) - 1 FROM requests WHERE status = 'queued'",
    )
    .fetch_one(&mut **tx)
    .await?)
}

/// Put `target` on air now; whatever was playing goes back to the front of the queue.
async fn put_on_air(
    tx: &mut Transaction<'_, Sqlite>,
    target: i64,
    interrupted: Option<i64>,
) -> Result<()> {
    if let Some(old) = interrupted {
        let front = front_position(tx).await?;
        sqlx::query(
            "UPDATE requests SET status = 'queued', position = ?, started_at = NULL WHERE id = ?",
        )
        .bind(front)
        .bind(old)
        .execute(&mut **tx)
        .await?;
    }
    sqlx::query("UPDATE requests SET status = 'playing', started_at = ? WHERE id = ?")
        .bind(now())
        .bind(target)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

/// Add a request. Returns `Ok(Err(rejection))` when the rules say no.
pub async fn enqueue(
    state: &AppState,
    who: &Requester,
    song: &Song,
    placement: Placement,
    enforce: bool,
) -> Result<Result<Enqueued, Rejection>> {
    let rules = state.settings().queue;
    let guard = state.queue_lock.lock().await;
    let facts = gather_facts(state, who, song).await?;
    if let Err(rejection) = check(&rules, who, song, &facts, enforce) {
        return Ok(Err(rejection));
    }

    let current = current(&state.db).await?;
    let mut tx = db::write_tx(&state.db).await?;
    let position: i64 = match placement {
        Placement::End => {
            sqlx::query_scalar(
                "SELECT COALESCE(MAX(position), 0) + 1 FROM requests WHERE status = 'queued'",
            )
            .fetch_one(&mut *tx)
            .await?
        }
        Placement::Next | Placement::Now => front_position(&mut tx).await?,
    };
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO requests (song_id, requester, requester_key, source, status, position, \
         autoplay, requested_at) VALUES (?, ?, ?, ?, 'queued', ?, 0, ?) RETURNING id",
    )
    .bind(song.id)
    .bind(&who.name)
    .bind(&who.key)
    .bind(who.source.as_str())
    .bind(position)
    .bind(now())
    .fetch_one(&mut *tx)
    .await?;

    let started = match (&current, placement) {
        (None, _) => {
            put_on_air(&mut tx, id, None).await?;
            true
        }
        (Some(c), Placement::Now) => {
            put_on_air(&mut tx, id, Some(c.id)).await?;
            true
        }
        _ => false,
    };
    renumber(&mut tx).await?;
    tx.commit().await?;
    if started {
        state.player.lock().unwrap().reset_for(Some(id));
    }
    drop(guard);

    let snap = live::snapshot(state).await?;
    let (position, wait_secs) = if started {
        (1, 0)
    } else {
        snap.queue
            .iter()
            .find(|i| i.entry.id == id)
            .map(|i| (i.position, i.wait_secs))
            .unwrap_or((0, 0))
    };
    events::record(
        &state.db,
        &who.name,
        "request",
        &format!(
            "點了『{}』（{}，#{position}）",
            song.title,
            who.source.as_str()
        ),
    )
    .await;
    live::publish(state).await;
    live::notice(
        state,
        "request",
        &format!("{} 點了『{}』", who.name, song.title),
    );
    Ok(Ok(Enqueued {
        id,
        position,
        wait_secs,
        started,
    }))
}

/// Close the current request and start the next one. Assumes `queue_lock` is held.
/// Returns the new current request id.
async fn finish_locked(state: &Inner, current: &Entry, reason: EndReason) -> Result<Option<i64>> {
    let settings = state.settings();
    let mut tx = db::write_tx(&state.db).await?;
    sqlx::query("UPDATE requests SET status = ?, ended_at = ? WHERE id = ?")
        .bind(reason.as_status())
        .bind(now())
        .bind(current.id)
        .execute(&mut *tx)
        .await?;
    if reason == EndReason::Played {
        sqlx::query("UPDATE songs SET last_played_at = ?, fail_count = 0 WHERE id = ?")
            .bind(now())
            .bind(current.song.id)
            .execute(&mut *tx)
            .await?;
    }
    let next = start_next(
        &mut tx,
        settings.playback.autoplay_favorites,
        Some(current.song.id),
    )
    .await?;
    tx.commit().await?;
    state.player.lock().unwrap().reset_for(next);
    Ok(next)
}

/// First waiting request goes on air; if none, a random favorite (the default playlist).
async fn start_next(
    tx: &mut Transaction<'_, Sqlite>,
    autoplay: bool,
    avoid_song: Option<i64>,
) -> Result<Option<i64>> {
    let next: Option<i64> = sqlx::query_scalar(
        "SELECT id FROM requests WHERE status = 'queued' ORDER BY position, id LIMIT 1",
    )
    .fetch_optional(&mut **tx)
    .await?;
    if let Some(id) = next {
        put_on_air(tx, id, None).await?;
        renumber(tx).await?;
        return Ok(Some(id));
    }
    if !autoplay {
        return Ok(None);
    }
    // The song that just ended sorts last, so it only repeats when it is the only favorite.
    let song: Option<i64> = sqlx::query_scalar(
        "SELECT id FROM songs WHERE favorite = 1 AND blocked = 0 AND missing = 0 \
         ORDER BY id = ?, RANDOM() LIMIT 1",
    )
    .bind(avoid_song.unwrap_or(0))
    .fetch_optional(&mut **tx)
    .await?;
    let Some(song) = song else {
        return Ok(None);
    };
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO requests (song_id, requester, requester_key, source, status, position, \
         autoplay, requested_at, started_at) VALUES (?, ?, ?, ?, 'playing', 0, 1, ?, ?) \
         RETURNING id",
    )
    .bind(song)
    .bind(AUTOPLAY_NAME)
    .bind(AUTOPLAY_KEY)
    .bind(Source::Autoplay.as_str())
    .bind(now())
    .bind(now())
    .fetch_one(&mut **tx)
    .await?;
    Ok(Some(id))
}

/// End the current song. `expected` makes reports idempotent: when two players report
/// the same ending, only the first one moves the queue.
pub async fn finish_current(
    state: &AppState,
    expected: Option<i64>,
    reason: EndReason,
) -> Result<Option<Entry>> {
    let guard = state.queue_lock.lock().await;
    let Some(cur) = current(&state.db).await? else {
        return Ok(None);
    };
    if expected.is_some_and(|e| e != cur.id) {
        return Ok(None);
    }
    finish_locked(state, &cur, reason).await?;
    drop(guard);
    live::publish(state).await;
    Ok(Some(cur))
}

/// Start something when nothing is on air (a player connected, or autoplay was switched on).
pub async fn ensure_playing(state: &AppState) -> Result<()> {
    let guard = state.queue_lock.lock().await;
    if current(&state.db).await?.is_some() {
        return Ok(());
    }
    let autoplay = state.settings().playback.autoplay_favorites;
    let mut tx = db::write_tx(&state.db).await?;
    let next = start_next(&mut tx, autoplay, None).await?;
    tx.commit().await?;
    drop(guard);
    if next.is_some() {
        state.player.lock().unwrap().reset_for(next);
        live::publish(state).await;
    }
    Ok(())
}

/// Take a request out of the queue. Removing the song on air skips it.
pub async fn remove(state: &AppState, request_id: i64) -> Result<Option<Entry>> {
    let guard = state.queue_lock.lock().await;
    let Some(e) = entry(&state.db, request_id).await? else {
        return Ok(None);
    };
    match e.status.as_str() {
        "playing" => {
            finish_locked(state, &e, EndReason::Skipped).await?;
        }
        "queued" => {
            let mut tx = db::write_tx(&state.db).await?;
            sqlx::query("UPDATE requests SET status = 'removed', ended_at = ? WHERE id = ?")
                .bind(now())
                .bind(request_id)
                .execute(&mut *tx)
                .await?;
            renumber(&mut tx).await?;
            tx.commit().await?;
        }
        _ => return Ok(None),
    }
    drop(guard);
    live::publish(state).await;
    Ok(Some(e))
}

/// Remove every waiting request (the song on air keeps playing).
pub async fn clear(state: &AppState) -> Result<u64> {
    let guard = state.queue_lock.lock().await;
    let n =
        sqlx::query("UPDATE requests SET status = 'removed', ended_at = ? WHERE status = 'queued'")
            .bind(now())
            .execute(&state.db)
            .await?
            .rows_affected();
    drop(guard);
    live::publish(state).await;
    Ok(n)
}

async fn write_order(state: &Inner, ids: &[i64]) -> Result<()> {
    let mut tx = db::write_tx(&state.db).await?;
    for (i, id) in ids.iter().enumerate() {
        sqlx::query("UPDATE requests SET position = ? WHERE id = ? AND status = 'queued'")
            .bind(i as i64 + 1)
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(())
}

async fn queued_ids(db: &SqlitePool) -> Result<Vec<i64>> {
    Ok(
        sqlx::query_scalar("SELECT id FROM requests WHERE status = 'queued' ORDER BY position, id")
            .fetch_all(db)
            .await?,
    )
}

/// Move a waiting request to `index` (0 = plays next). False if it isn't waiting.
pub async fn move_to(state: &AppState, request_id: i64, index: usize) -> Result<bool> {
    let guard = state.queue_lock.lock().await;
    let mut ids = queued_ids(&state.db).await?;
    let Some(from) = ids.iter().position(|id| *id == request_id) else {
        return Ok(false);
    };
    let id = ids.remove(from);
    ids.insert(index.min(ids.len()), id);
    write_order(state, &ids).await?;
    drop(guard);
    live::publish(state).await;
    Ok(true)
}

/// Apply a full new order. Unknown ids are ignored; waiting songs left out keep their
/// relative order at the end, so a stale page can't lose requests.
pub async fn reorder(state: &AppState, order: &[i64]) -> Result<()> {
    let guard = state.queue_lock.lock().await;
    let current_ids = queued_ids(&state.db).await?;
    let mut ids: Vec<i64> = order
        .iter()
        .copied()
        .filter(|id| current_ids.contains(id))
        .collect();
    ids.dedup();
    for id in &current_ids {
        if !ids.contains(id) {
            ids.push(*id);
        }
    }
    write_order(state, &ids).await?;
    drop(guard);
    live::publish(state).await;
    Ok(())
}

/// Interrupt: a waiting request goes on air now. False if it isn't waiting.
pub async fn play_now(state: &AppState, request_id: i64) -> Result<bool> {
    let guard = state.queue_lock.lock().await;
    let Some(e) = entry(&state.db, request_id).await? else {
        return Ok(false);
    };
    if e.status != "queued" {
        return Ok(false);
    }
    let cur = current(&state.db).await?;
    let mut tx = db::write_tx(&state.db).await?;
    put_on_air(&mut tx, request_id, cur.map(|c| c.id)).await?;
    renumber(&mut tx).await?;
    tx.commit().await?;
    state.player.lock().unwrap().reset_for(Some(request_id));
    drop(guard);
    live::publish(state).await;
    Ok(true)
}

// ───────────────────────────── player reports ─────────────────────────────

/// The song really started playing: it works, so its failure count resets.
pub async fn mark_started(state: &AppState, request_id: i64) -> Result<()> {
    sqlx::query(
        "UPDATE songs SET fail_count = 0 WHERE id = (SELECT song_id FROM requests WHERE id = ?)",
    )
    .bind(request_id)
    .execute(&state.db)
    .await?;
    Ok(())
}

/// The player could not play the current song. After `missing_after_failures` failures
/// in a row the song is marked missing and can't be requested again (issue #2).
pub async fn report_failure(state: &AppState, request_id: i64, reason: &str) -> Result<bool> {
    let limit = state.settings().queue.missing_after_failures;
    let guard = state.queue_lock.lock().await;
    let Some(cur) = current(&state.db).await? else {
        return Ok(false);
    };
    if cur.id != request_id {
        return Ok(false);
    }
    let fails: i64 = sqlx::query_scalar(
        "UPDATE songs SET fail_count = fail_count + 1 WHERE id = ? RETURNING fail_count",
    )
    .bind(cur.song.id)
    .fetch_one(&state.db)
    .await?;
    let missing = limit > 0 && fails >= i64::from(limit);
    if missing {
        sqlx::query("UPDATE songs SET missing = 1 WHERE id = ?")
            .bind(cur.song.id)
            .execute(&state.db)
            .await?;
    }
    finish_locked(state, &cur, EndReason::Failed).await?;
    drop(guard);
    let note = if missing {
        format!(
            "『{}』播放失敗 {fails} 次，標成「不見了」：{reason}",
            cur.song.title
        )
    } else {
        format!("『{}』播放失敗（第 {fails} 次）：{reason}", cur.song.title)
    };
    events::record(&state.db, "player", "failed", &note).await;
    live::publish(state).await;
    Ok(true)
}

/// A player learned the length of a song whose site didn't say. If the song is longer
/// than the limit (which couldn't be checked at request time), it is skipped.
pub async fn report_duration(state: &AppState, request_id: i64, secs: i64) -> Result<()> {
    let Some(e) = entry(&state.db, request_id).await? else {
        return Ok(());
    };
    if e.song.duration_secs.is_some() || secs <= 0 {
        return Ok(());
    }
    sqlx::query("UPDATE songs SET duration_secs = ? WHERE id = ?")
        .bind(secs)
        .bind(e.song.id)
        .execute(&state.db)
        .await?;
    let max = i64::from(state.settings().queue.max_duration_secs);
    let enforced = !e.autoplay && e.source != Source::Web.as_str();
    if enforced && max > 0 && secs > max && e.status == "playing" {
        finish_current(state, Some(request_id), EndReason::Skipped).await?;
        events::record(
            &state.db,
            "player",
            "skip",
            &format!("『{}』長 {secs} 秒，超過上限，已跳過", e.song.title),
        )
        .await;
    } else {
        live::publish(state).await;
    }
    Ok(())
}

// ───────────────────────────── votes ─────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoteKind {
    /// Skip the song on air
    Skip,
    /// Move a waiting song to play next
    Bump,
}

impl VoteKind {
    fn as_str(self) -> &'static str {
        match self {
            VoteKind::Skip => "skip",
            VoteKind::Bump => "bump",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tally {
    pub votes: u32,
    pub needed: u32,
    pub voters: Vec<String>,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VoteOutcome {
    Counted(Tally),
    Passed(Tally),
    Already(Tally),
    /// Skip: nothing on air. Bump: the id isn't waiting.
    NotFound,
    /// Bump: the song already plays next.
    AlreadyNext,
}

pub async fn vote(
    state: &AppState,
    kind: VoteKind,
    target: Option<i64>,
    voter: &Requester,
) -> Result<VoteOutcome> {
    let rules = state.settings().queue;
    let needed = match kind {
        VoteKind::Skip => rules.skip_votes,
        VoteKind::Bump => rules.bump_votes,
    }
    .max(1);
    let guard = state.queue_lock.lock().await;
    let db = &state.db;

    let subject = match kind {
        VoteKind::Skip => match current(db).await? {
            Some(c) => c,
            None => return Ok(VoteOutcome::NotFound),
        },
        VoteKind::Bump => {
            let Some(id) = target else {
                return Ok(VoteOutcome::NotFound);
            };
            let ids = queued_ids(db).await?;
            match ids.iter().position(|q| *q == id) {
                None => return Ok(VoteOutcome::NotFound),
                Some(0) => return Ok(VoteOutcome::AlreadyNext),
                Some(_) => match entry(db, id).await? {
                    Some(e) => e,
                    None => return Ok(VoteOutcome::NotFound),
                },
            }
        }
    };

    let t = now();
    sqlx::query("DELETE FROM votes WHERE request_id = ? AND kind = ? AND created_at < ?")
        .bind(subject.id)
        .bind(kind.as_str())
        .bind(t - i64::from(rules.vote_window_secs))
        .execute(db)
        .await?;
    let inserted = sqlx::query(
        "INSERT OR IGNORE INTO votes (request_id, kind, voter_key, voter_name, created_at) \
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(subject.id)
    .bind(kind.as_str())
    .bind(&voter.key)
    .bind(&voter.name)
    .bind(t)
    .execute(db)
    .await?
    .rows_affected()
        > 0;
    let voters: Vec<String> = sqlx::query_scalar(
        "SELECT voter_name FROM votes WHERE request_id = ? AND kind = ? ORDER BY created_at, rowid",
    )
    .bind(subject.id)
    .bind(kind.as_str())
    .fetch_all(db)
    .await?;
    let tally = Tally {
        votes: voters.len() as u32,
        needed,
        voters,
        title: subject.song.title.clone(),
    };
    if !inserted {
        return Ok(VoteOutcome::Already(tally));
    }
    if tally.votes < needed {
        return Ok(VoteOutcome::Counted(tally));
    }

    sqlx::query("DELETE FROM votes WHERE request_id = ? AND kind = ?")
        .bind(subject.id)
        .bind(kind.as_str())
        .execute(db)
        .await?;
    match kind {
        VoteKind::Skip => {
            finish_locked(state, &subject, EndReason::Skipped).await?;
        }
        VoteKind::Bump => {
            let mut ids = queued_ids(db).await?;
            ids.retain(|id| *id != subject.id);
            ids.insert(0, subject.id);
            write_order(state, &ids).await?;
        }
    }
    drop(guard);
    live::publish(state).await;
    Ok(VoteOutcome::Passed(tally))
}

// ───────────────────────────── statistics ─────────────────────────────

#[derive(Debug, Clone, Serialize, sqlx::FromRow, ToSchema)]
pub struct UserStats {
    pub requester: String,
    pub total: i64,
    pub played: i64,
    pub played_secs: i64,
    pub unique_songs: i64,
    pub top_title: String,
    pub top_count: i64,
}

/// v1's `order/{user}/count`: how many requests, how many finished, favourite song.
/// Matches by identity or by display name, so `!songstats SomeName` works on every platform.
pub async fn user_stats(db: &SqlitePool, who: &str) -> Result<Option<UserStats>> {
    let who = who.trim().to_lowercase();
    let sql = "WITH mine AS (
          SELECT r.*, s.title, COALESCE(s.duration_secs, 0) AS secs
          FROM requests r JOIN songs s ON s.id = r.song_id
          WHERE r.autoplay = 0 AND (r.requester_key = ?1 OR lower(r.requester) = ?1)
        ), top AS (
          SELECT title, COUNT(*) AS c FROM mine GROUP BY song_id ORDER BY c DESC, MAX(requested_at) DESC LIMIT 1
        )
        SELECT (SELECT requester FROM mine ORDER BY requested_at DESC LIMIT 1) AS requester,
               COUNT(*) AS total,
               COALESCE(SUM(status = 'played'), 0) AS played,
               COALESCE(SUM(CASE WHEN status = 'played' THEN secs ELSE 0 END), 0) AS played_secs,
               COUNT(DISTINCT song_id) AS unique_songs,
               (SELECT title FROM top) AS top_title,
               (SELECT c FROM top) AS top_count
        FROM mine";
    let row: (
        Option<String>,
        i64,
        i64,
        i64,
        i64,
        Option<String>,
        Option<i64>,
    ) = sqlx::query_as(sql).bind(&who).fetch_one(db).await?;
    if row.1 == 0 {
        return Ok(None);
    }
    Ok(Some(UserStats {
        requester: row.0.unwrap_or(who),
        total: row.1,
        played: row.2,
        played_secs: row.3,
        unique_songs: row.4,
        top_title: row.5.unwrap_or_default(),
        top_count: row.6.unwrap_or(0),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Level;

    fn song() -> Song {
        Song {
            id: 1,
            kind: "youtube".into(),
            media_key: "youtube:abc".into(),
            source_id: "abc".into(),
            url: "https://www.youtube.com/watch?v=abc".into(),
            title: "Song".into(),
            duration_secs: Some(200),
            thumbnail: None,
            site: "YouTube".into(),
            favorite: false,
            blocked: false,
            missing: false,
            fail_count: 0,
            created_at: 0,
            last_played_at: None,
        }
    }

    fn viewer() -> Requester {
        Requester::new("viewer", None, Level::Everyone, Source::Twitch)
    }

    fn moderator() -> Requester {
        Requester::new("mod", None, Level::Moderator, Source::Twitch)
    }

    #[test]
    fn accepts_a_normal_request() {
        let rules = QueueRules::default();
        assert_eq!(
            check(&rules, &viewer(), &song(), &RequestFacts::default(), true),
            Ok(())
        );
    }

    #[test]
    fn refuses_long_videos_like_v1() {
        let rules = QueueRules::default();
        let mut s = song();
        s.duration_secs = Some(15 * 60 + 1);
        assert_eq!(
            check(&rules, &viewer(), &s, &RequestFacts::default(), true),
            Err(Rejection::TooLong { max_secs: 900 })
        );
        s.duration_secs = None;
        assert_eq!(
            check(&rules, &viewer(), &s, &RequestFacts::default(), true),
            Ok(()),
            "unknown length is checked later by the player"
        );
    }

    #[test]
    fn refuses_full_queue_and_duplicates() {
        let rules = QueueRules::default();
        let facts = RequestFacts {
            queue_len: 30,
            ..Default::default()
        };
        assert_eq!(
            check(&rules, &viewer(), &song(), &facts, true),
            Err(Rejection::QueueFull { max: 30 })
        );
        let facts = RequestFacts {
            duplicate: Some((9, 1, "someone".into(), true)),
            ..Default::default()
        };
        let err = check(&rules, &viewer(), &song(), &facts, true).unwrap_err();
        assert_eq!(err.outcome(), "duplicate_playing");
        let mut open = rules.clone();
        open.allow_duplicates = true;
        assert_eq!(check(&open, &viewer(), &song(), &facts, true), Ok(()));
    }

    #[test]
    fn per_user_limit_with_moderator_bypass() {
        let rules = QueueRules {
            max_per_user: 2,
            ..Default::default()
        };
        let facts = RequestFacts {
            user_pending: 2,
            ..Default::default()
        };
        assert_eq!(
            check(&rules, &viewer(), &song(), &facts, true),
            Err(Rejection::UserLimit { max: 2, count: 2 })
        );
        assert_eq!(check(&rules, &moderator(), &song(), &facts, true), Ok(()));
        let strict = QueueRules {
            privileged_bypass: false,
            ..rules
        };
        assert!(check(&strict, &moderator(), &song(), &facts, true).is_err());
    }

    #[test]
    fn closed_requests_and_blocks() {
        let rules = QueueRules {
            requests_open: false,
            ..Default::default()
        };
        let facts = RequestFacts::default();
        assert_eq!(
            check(&rules, &viewer(), &song(), &facts, true),
            Err(Rejection::Closed)
        );
        assert_eq!(check(&rules, &moderator(), &song(), &facts, true), Ok(()));

        let blocked_user = RequestFacts {
            user_blocked: true,
            ..Default::default()
        };
        let open = QueueRules::default();
        assert_eq!(
            check(&open, &viewer(), &song(), &blocked_user, true),
            Err(Rejection::UserBlocked)
        );
        let mut s = song();
        s.blocked = true;
        assert_eq!(
            check(&open, &viewer(), &s, &facts, false),
            Err(Rejection::SongBlocked)
        );
        let mut s = song();
        s.missing = true;
        assert_eq!(
            check(&open, &viewer(), &s, &facts, true),
            Err(Rejection::Missing)
        );
        assert_eq!(
            check(&open, &viewer(), &s, &facts, false),
            Ok(()),
            "admin can retry"
        );
    }

    fn entry_with(id: i64, secs: Option<i64>) -> Entry {
        let mut s = song();
        s.duration_secs = secs;
        Entry {
            id,
            requester: "a".into(),
            requester_key: "a".into(),
            source: "twitch".into(),
            status: "queued".into(),
            autoplay: false,
            requested_at: 0,
            started_at: None,
            ended_at: None,
            song: s,
        }
    }

    #[test]
    fn lineup_counts_the_song_on_air_as_number_one() {
        let cur = entry_with(1, Some(100));
        let waiting = vec![
            entry_with(2, Some(60)),
            entry_with(3, None),
            entry_with(4, Some(30)),
        ];
        let line = lineup(Some(&cur), 40_000, waiting);
        let got: Vec<(u32, i64)> = line.iter().map(|i| (i.position, i.wait_secs)).collect();
        // 60 s left of the current song, then 60 s, then an unknown length counted as 0
        assert_eq!(got, vec![(2, 60), (3, 120), (4, 120)]);
        let line = lineup(None, 0, vec![entry_with(5, Some(10))]);
        assert_eq!((line[0].position, line[0].wait_secs), (1, 0));
    }
}
