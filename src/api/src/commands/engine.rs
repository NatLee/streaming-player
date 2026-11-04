//! Run a chat command and produce the reply text.

use anyhow::Result;
use serde::Serialize;
use utoipa::ToSchema;

use super::{clean, find, split_command, template::render, Action, Command};
use crate::{
    commands, control, live,
    media::resolve::{self, ResolveError},
    models::{Level, Requester},
    queue::{self, Placement, Rejection, VoteKind, VoteOutcome},
    state::AppState,
    util::{format_total, format_wait, now, truncate_chars},
};

/// Nightbot shows at most 400 characters of a urlfetch response; Twitch allows 500.
pub const MAX_REPLY_CHARS: usize = 400;

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Reply {
    pub outcome: String,
    pub text: String,
}

/// Chinese names for levels, used in the "no permission" reply.
fn level_name(level: Level) -> &'static str {
    match level {
        Level::Everyone => "所有人",
        Level::Subscriber => "訂閱者",
        Level::Vip => "VIP",
        Level::Moderator => "管理員",
        Level::Broadcaster => "實況主",
    }
}

/// A whole chat message, e.g. from Twitch. None when it isn't one of our commands.
pub async fn handle_message(
    state: &AppState,
    who: &Requester,
    text: &str,
) -> Result<Option<Reply>> {
    let text = clean(text);
    let prefix = state.settings().chat.prefix;
    let Some((trigger, args)) = split_command(&text, &prefix) else {
        return Ok(None);
    };
    run_trigger(state, who, &trigger, args).await
}

/// A command word (with or without the prefix) and its arguments, e.g. from a bot URL.
pub async fn run_trigger(
    state: &AppState,
    who: &Requester,
    trigger: &str,
    args: &str,
) -> Result<Option<Reply>> {
    let prefix = state.settings().chat.prefix;
    let all = commands::list(&state.db).await?;
    let word = trigger.trim().trim_start_matches(prefix.as_str());
    let Some(cmd) = find(&all, word) else {
        return Ok(None);
    };
    Ok(Some(execute(state, cmd, &all, who, &clean(args)).await?))
}

/// Run by action, for callers that don't speak in triggers (the v1 Nightbot URLs).
/// Uses the first command configured for the action, so its texts and limits apply.
pub async fn run_action(
    state: &AppState,
    who: &Requester,
    action: Action,
    args: &str,
) -> Result<Reply> {
    let all = commands::list(&state.db).await?;
    let cmd = all
        .iter()
        .find(|c| c.action == action)
        .cloned()
        .or_else(|| {
            commands::defaults()
                .into_iter()
                .find(|c| c.action == action)
        })
        .expect("every action has a default command");
    execute(state, &cmd, &all, who, &clean(args)).await
}

fn cooldown_left(state: &AppState, cmd: &Command, who: &Requester) -> Option<i64> {
    if cmd.cooldown_secs <= 0 {
        return None;
    }
    let bypass = state.settings().queue.privileged_bypass && who.privileged();
    if bypass {
        return None;
    }
    let cooldowns = state.cooldowns.lock().unwrap();
    let last = *cooldowns.get(&(cmd.id, who.key.clone()))?;
    let left = last + cmd.cooldown_secs - now();
    (left > 0).then_some(left)
}

fn start_cooldown(state: &AppState, cmd: &Command, who: &Requester) {
    if cmd.cooldown_secs > 0 {
        let mut cooldowns = state.cooldowns.lock().unwrap();
        let t = now();
        let longest = 24 * 3600;
        cooldowns.retain(|_, at| t - *at < longest);
        cooldowns.insert((cmd.id, who.key.clone()), t);
    }
}

type Vars = Vec<(&'static str, String)>;

async fn execute(
    state: &AppState,
    cmd: &Command,
    all: &[Command],
    who: &Requester,
    args: &str,
) -> Result<Reply> {
    let mut vars: Vars = vec![("user", who.name.clone())];
    let outcome: &'static str = if !cmd.enabled {
        "disabled"
    } else if who.level < cmd.min_level {
        vars.push(("level", level_name(cmd.min_level).into()));
        "no_permission"
    } else if let Some(left) = cooldown_left(state, cmd, who) {
        vars.push(("seconds", left.to_string()));
        "cooldown"
    } else {
        start_cooldown(state, cmd, who);
        act(state, cmd.action, all, who, args, &mut vars).await?
    };
    let text = render(&cmd.reply_template(outcome), &vars);
    Ok(Reply {
        outcome: outcome.into(),
        text: truncate_chars(text.trim(), MAX_REPLY_CHARS),
    })
}

fn parse_id(args: &str) -> Option<Result<i64, ()>> {
    let first = args.split_whitespace().next()?;
    Some(first.trim_start_matches('#').parse::<i64>().map_err(|_| ()))
}

async fn position_of(state: &AppState, request_id: i64) -> Result<u32> {
    let snap = live::snapshot(state).await?;
    if snap.now.as_ref().is_some_and(|n| n.entry.id == request_id) {
        return Ok(1);
    }
    Ok(snap
        .queue
        .iter()
        .find(|i| i.entry.id == request_id)
        .map(|i| i.position)
        .unwrap_or(0))
}

async fn act(
    state: &AppState,
    action: Action,
    all: &[Command],
    who: &Requester,
    args: &str,
    vars: &mut Vars,
) -> Result<&'static str> {
    let settings = state.settings();
    Ok(match action {
        Action::Request => {
            if args.is_empty() {
                return Ok("empty");
            }
            // Cheap refusals first, before asking a website about the song.
            if !settings.queue.requests_open && !who.privileged() {
                return Ok("closed");
            }
            if queue::is_user_blocked(&state.db, who).await? {
                return Ok("blocked_user");
            }
            let song = match resolve::song_for(state, args).await? {
                Ok(song) => song,
                Err(ResolveError::Empty) => return Ok("empty"),
                Err(ResolveError::Live) => return Ok("live"),
                Err(e) => {
                    vars.push(("reason", e.reason()));
                    return Ok("not_found");
                }
            };
            vars.push(("title", song.title.clone()));
            vars.push(("url", song.url.clone()));
            match queue::enqueue(state, who, &song, Placement::End, true).await? {
                Ok(done) => {
                    vars.push(("id", done.id.to_string()));
                    vars.push(("position", done.position.to_string()));
                    vars.push(("wait", format_wait(done.wait_secs)));
                    if done.started {
                        "ok_now"
                    } else {
                        "ok"
                    }
                }
                Err(rejection) => {
                    match &rejection {
                        Rejection::Duplicate {
                            id,
                            position,
                            requester,
                            ..
                        } => {
                            vars.push(("id", id.to_string()));
                            vars.push(("position", position.to_string()));
                            vars.push(("requester", requester.clone()));
                        }
                        Rejection::TooLong { max_secs } => {
                            vars.push(("max", (max_secs / 60).to_string()))
                        }
                        Rejection::QueueFull { max } => vars.push(("max", max.to_string())),
                        Rejection::UserLimit { max, count } => {
                            vars.push(("max", max.to_string()));
                            vars.push(("count", count.to_string()));
                        }
                        _ => {}
                    }
                    rejection.outcome()
                }
            }
        }
        Action::Current => match queue::current(&state.db).await? {
            None => "idle",
            Some(e) => {
                vars.push(("title", e.song.title.clone()));
                vars.push(("url", e.song.url.clone()));
                vars.push(("requester", e.requester.clone()));
                vars.push(("id", e.id.to_string()));
                if e.autoplay {
                    "autoplay"
                } else {
                    "ok"
                }
            }
        },
        Action::Queue => {
            let snap = live::snapshot(state).await?;
            let link = if settings.viewer.public_queue {
                format!("{}/queue", state.config.public_url)
            } else {
                String::new()
            };
            vars.push(("link", link));
            vars.push(("count", snap.queue.len().to_string()));
            match snap.queue.first() {
                None => "empty",
                Some(next) => {
                    vars.push(("next", next.entry.song.title.clone()));
                    "ok"
                }
            }
        }
        Action::Mine => {
            let snap = live::snapshot(state).await?;
            let mut mine: Vec<String> = Vec::new();
            if let Some(n) = snap
                .now
                .as_ref()
                .filter(|n| n.entry.requester_key == who.key)
            {
                mine.push(format!("#1『{}』(ID {})", n.entry.song.title, n.entry.id));
            }
            for item in snap
                .queue
                .iter()
                .filter(|i| i.entry.requester_key == who.key)
            {
                mine.push(format!(
                    "#{}『{}』(ID {})",
                    item.position, item.entry.song.title, item.entry.id
                ));
            }
            vars.push(("count", mine.len().to_string()));
            vars.push(("list", mine.join("、")));
            if mine.is_empty() {
                "none"
            } else {
                "ok"
            }
        }
        Action::Stats => {
            let target = args.split_whitespace().next().map(str::to_string);
            let lookup = target.clone().unwrap_or_else(|| who.key.clone());
            match queue::user_stats(&state.db, &lookup).await? {
                None => {
                    if let Some(name) = target {
                        vars[0].1 = name;
                    }
                    "none"
                }
                Some(s) => {
                    if target.is_some() {
                        vars[0].1 = s.requester.clone();
                    }
                    let percent = if s.total > 0 {
                        100.0 * s.played as f64 / s.total as f64
                    } else {
                        0.0
                    };
                    vars.push(("total", s.total.to_string()));
                    vars.push(("played", s.played.to_string()));
                    vars.push(("percent", format!("{percent:.2}")));
                    vars.push(("duration", format_total(s.played_secs)));
                    vars.push(("unique", s.unique_songs.to_string()));
                    vars.push(("top_title", s.top_title));
                    vars.push(("top_count", s.top_count.to_string()));
                    "ok"
                }
            }
        }
        Action::Remove => {
            let target = match parse_id(args) {
                Some(Err(())) => return Ok("bad_id"),
                Some(Ok(id)) => match queue::entry(&state.db, id).await? {
                    Some(e) if matches!(e.status.as_str(), "queued" | "playing") => e,
                    _ => return Ok("not_found"),
                },
                None => match queue::pending_for(&state.db, &who.key)
                    .await?
                    .into_iter()
                    .filter(|e| e.status == "queued")
                    .max_by_key(|e| e.requested_at)
                {
                    Some(e) => e,
                    None => return Ok("none"),
                },
            };
            vars.push(("title", target.song.title.clone()));
            vars.push(("id", target.id.to_string()));
            vars.push(("requester", target.requester.clone()));
            if target.requester_key != who.key && !who.privileged() {
                return Ok("not_owner");
            }
            vars.push(("position", position_of(state, target.id).await?.to_string()));
            match queue::remove(state, target.id).await? {
                Some(_) => "ok",
                None => "not_found",
            }
        }
        Action::VoteSkip => tally_outcome(
            queue::vote(state, VoteKind::Skip, None, who).await?,
            vars,
            "idle",
        ),
        Action::VoteBump => match parse_id(args) {
            None | Some(Err(())) => "bad_id",
            Some(Ok(id)) => {
                vars.push(("id", id.to_string()));
                tally_outcome(
                    queue::vote(state, VoteKind::Bump, Some(id), who).await?,
                    vars,
                    "not_found",
                )
            }
        },
        Action::Skip => match control::skip(state, false, &who.name).await? {
            Some(e) => {
                vars.push(("title", e.song.title));
                "ok"
            }
            None => "idle",
        },
        Action::Pause => {
            control::set_paused(state, true, &who.name).await?;
            "ok"
        }
        Action::Resume => {
            control::set_paused(state, false, &who.name).await?;
            "ok"
        }
        Action::Help => {
            let prefix = &settings.chat.prefix;
            let list: Vec<String> = all
                .iter()
                .filter(|c| c.enabled && c.min_level <= who.level)
                .filter_map(|c| c.triggers.first().map(|t| format!("{prefix}{t}")))
                .collect();
            vars.push(("commands", list.join(" ")));
            "ok"
        }
    })
}

fn tally_outcome(outcome: VoteOutcome, vars: &mut Vars, not_found: &'static str) -> &'static str {
    let (key, tally) = match outcome {
        VoteOutcome::NotFound => return not_found,
        VoteOutcome::AlreadyNext => return "already_next",
        VoteOutcome::Counted(t) => ("voted", t),
        VoteOutcome::Passed(t) => ("passed", t),
        VoteOutcome::Already(t) => ("already", t),
    };
    vars.push(("title", tally.title));
    vars.push(("votes", tally.votes.to_string()));
    vars.push(("needed", tally.needed.to_string()));
    vars.push(("voters", tally.voters.join(" , ")));
    key
}
