//! Chat commands. Every source (Twitch, YouTube, chat bots, the v1 Nightbot URLs) goes
//! through the same table of commands, so triggers, permissions, cooldowns and reply
//! texts are configured once on the admin page.

pub mod engine;
pub mod template;

use std::collections::BTreeMap;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use utoipa::ToSchema;

use crate::models::Level;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Request,
    Current,
    Queue,
    Mine,
    Stats,
    Remove,
    VoteSkip,
    Skip,
    VoteBump,
    Pause,
    Resume,
    Help,
}

pub const ALL_ACTIONS: &[Action] = &[
    Action::Request,
    Action::Current,
    Action::Queue,
    Action::Mine,
    Action::Stats,
    Action::Remove,
    Action::VoteSkip,
    Action::Skip,
    Action::VoteBump,
    Action::Pause,
    Action::Resume,
    Action::Help,
];

/// Outcomes any command can have.
pub const COMMON_OUTCOMES: &[(&str, &str)] = &[
    ("cooldown", "{user}，請等 {seconds} 秒再用這個指令。"),
    ("no_permission", "{user}，這個指令只有{level}可以用。"),
    ("disabled", "這個指令目前關閉中。"),
];

/// Default reply for every (action, outcome). Texts follow v1's chat replies.
const DEFAULT_REPLIES: &[(Action, &str, &str)] = &[
    (Action::Request, "ok", "{user} 無情點播了『{title}』，播放順位是#{position}，ID是『{id}』，還要再等{wait}！"),
    (Action::Request, "ok_now", "{user} 點播了『{title}』，馬上開始播放！ID是『{id}』"),
    (Action::Request, "empty", "哪有人點歌不輸入網址的！"),
    (Action::Request, "not_found", "{user}，找不到這首歌：{reason}"),
    (Action::Request, "live", "{user}，直播沒辦法點喔！"),
    (Action::Request, "closed", "現在沒有開放點歌喔！"),
    (Action::Request, "blocked_user", "{user}，你目前不能點歌。"),
    (Action::Request, "blocked_song", "Sorry! This song has been blocked ;/"),
    (Action::Request, "missing", "Sad! This song has lost :("),
    (Action::Request, "too_long", "這歌怎麼能超過{max}分鐘！"),
    (Action::Request, "duplicate_playing", "這首歌被 {requester} 點過，順位 {position}，正在放送中！"),
    (Action::Request, "duplicate", "這首歌被 {requester} 點過，在佇列還沒播放！目前順位 {position}！"),
    (Action::Request, "queue_full", "要播的歌太多了！再點我要罷工了！"),
    (Action::Request, "user_limit", "{user}，你已經有 {count} 首在排了，每人最多 {max} 首！"),
    (Action::Current, "ok", "現在播放的是 {requester} 點的『{title}』，傳送門 -> {url}"),
    (Action::Current, "autoplay", "現在播放的歌由自動點播提供『{title}』，傳送門 -> {url}"),
    (Action::Current, "idle", "現在佇列沒有歌哦！"),
    (Action::Queue, "ok", "佇列共 {count} 首，下一首是『{next}』 {link}"),
    (Action::Queue, "empty", "佇列是空的，快來點歌！ {link}"),
    (Action::Mine, "ok", "{user} 排隊中的歌：{list}"),
    (Action::Mine, "none", "{user}，你現在沒有排隊中的歌。"),
    (Action::Stats, "ok", "{user} 點歌次數有 {total} 次，播完的比例有 {percent} %，總共 {duration}，不重複的歌有 {unique} 首，最常點播的歌是 [{top_title}] ，高達 {top_count} 次"),
    (Action::Stats, "none", "{user} 這個人沒有點過歌哦！"),
    (Action::Remove, "ok", "{user} 無情砍了 #{position} 『{title}』"),
    (Action::Remove, "not_found", "{user}，現在的佇列沒有這首歌啊！"),
    (Action::Remove, "not_owner", "{user}，不要砍別人點的歌！這首是 {requester} 點的"),
    (Action::Remove, "bad_id", "ID是點歌時會給的一組數字ㄛ！"),
    (Action::Remove, "none", "{user}，你沒有排隊中的歌可以刪。"),
    (Action::VoteSkip, "voted", "[ {voters} ] 投票要卡掉當前的歌 [{title}]，達成人數 [{votes}/{needed}]"),
    (Action::VoteSkip, "passed", "當前的歌 [{title}] 已經被 [ {voters} ] 投票卡掉！"),
    (Action::VoteSkip, "already", "{user}，天啊！不要重複投票好嗎！"),
    (Action::VoteSkip, "idle", "{user}，天啊！現在剛好沒歌！"),
    (Action::Skip, "ok", "{user} 跳過了『{title}』"),
    (Action::Skip, "idle", "現在沒有在播的歌。"),
    (Action::VoteBump, "voted", "[ {voters} ] 投票要插歌 [{title}]，達成人數 [{votes}/{needed}]"),
    (Action::VoteBump, "passed", "[{title}] 由 [ {voters} ] 投票成功插歌！"),
    (Action::VoteBump, "already", "{user}，天啊！不要重複投票插歌好嗎！"),
    (Action::VoteBump, "not_found", "無法插歌！因爲這首已經不存在！"),
    (Action::VoteBump, "already_next", "無法插歌！因爲這首已經是下一首了！"),
    (Action::VoteBump, "bad_id", "ID是點歌時會給的一組數字ㄛ！"),
    (Action::Pause, "ok", "已暫停播放。"),
    (Action::Resume, "ok", "繼續播放！"),
    (Action::Help, "ok", "點歌指令：{commands}"),
];

/// Variables each action fills in, shown on the admin page next to the templates.
const VARIABLES: &[(Action, &[&str])] = &[
    (
        Action::Request,
        &[
            "user",
            "title",
            "url",
            "id",
            "position",
            "wait",
            "requester",
            "reason",
            "max",
            "count",
        ],
    ),
    (
        Action::Current,
        &["user", "title", "url", "requester", "id"],
    ),
    (Action::Queue, &["user", "count", "next", "link"]),
    (Action::Mine, &["user", "list", "count"]),
    (
        Action::Stats,
        &[
            "user",
            "total",
            "played",
            "percent",
            "duration",
            "unique",
            "top_title",
            "top_count",
        ],
    ),
    (
        Action::Remove,
        &["user", "title", "id", "position", "requester"],
    ),
    (
        Action::VoteSkip,
        &["user", "title", "votes", "needed", "voters"],
    ),
    (Action::Skip, &["user", "title"]),
    (
        Action::VoteBump,
        &["user", "title", "id", "votes", "needed", "voters"],
    ),
    (Action::Pause, &["user"]),
    (Action::Resume, &["user"]),
    (Action::Help, &["user", "commands"]),
];

impl Action {
    pub fn default_reply(self, outcome: &str) -> Option<&'static str> {
        DEFAULT_REPLIES
            .iter()
            .find(|(a, o, _)| *a == self && *o == outcome)
            .map(|(_, _, t)| *t)
            .or_else(|| {
                COMMON_OUTCOMES
                    .iter()
                    .find(|(o, _)| *o == outcome)
                    .map(|(_, t)| *t)
            })
    }

    pub fn outcomes(self) -> Vec<(&'static str, &'static str)> {
        DEFAULT_REPLIES
            .iter()
            .filter(|(a, _, _)| *a == self)
            .map(|(_, o, t)| (*o, *t))
            .chain(COMMON_OUTCOMES.iter().copied())
            .collect()
    }

    pub fn variables(self) -> &'static [&'static str] {
        VARIABLES
            .iter()
            .find(|(a, _)| *a == self)
            .map(|(_, v)| *v)
            .unwrap_or(&[])
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Action::Request => "request",
            Action::Current => "current",
            Action::Queue => "queue",
            Action::Mine => "mine",
            Action::Stats => "stats",
            Action::Remove => "remove",
            Action::VoteSkip => "vote_skip",
            Action::Skip => "skip",
            Action::VoteBump => "vote_bump",
            Action::Pause => "pause",
            Action::Resume => "resume",
            Action::Help => "help",
        }
    }

    pub fn parse(s: &str) -> Option<Action> {
        ALL_ACTIONS.iter().copied().find(|a| a.as_str() == s)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq)]
pub struct Command {
    #[serde(default)]
    pub id: i64,
    pub action: Action,
    /// Words that start the command, without the prefix (`sr`, `點歌`).
    pub triggers: Vec<String>,
    pub min_level: Level,
    /// Seconds a person has to wait before using this command again.
    #[serde(default)]
    pub cooldown_secs: i64,
    #[serde(default = "yes")]
    pub enabled: bool,
    /// outcome → template; outcomes not listed use the built-in text.
    #[serde(default)]
    pub replies: BTreeMap<String, String>,
}

fn yes() -> bool {
    true
}

impl Command {
    pub fn reply_template(&self, outcome: &str) -> String {
        self.replies
            .get(outcome)
            .filter(|t| !t.trim().is_empty())
            .cloned()
            .or_else(|| self.action.default_reply(outcome).map(str::to_string))
            .unwrap_or_default()
    }
}

/// Commands a new install starts with. v1's Nightbot setup used `!sr`.
pub fn defaults() -> Vec<Command> {
    let c = |action, triggers: &[&str], min_level| Command {
        id: 0,
        action,
        triggers: triggers.iter().map(|t| t.to_string()).collect(),
        min_level,
        cooldown_secs: 0,
        enabled: true,
        replies: BTreeMap::new(),
    };
    vec![
        c(
            Action::Request,
            &["sr", "點歌", "songrequest"],
            Level::Everyone,
        ),
        c(
            Action::Current,
            &["song", "現在", "nowplaying"],
            Level::Everyone,
        ),
        c(Action::Queue, &["queue", "歌單"], Level::Everyone),
        c(Action::Mine, &["mysongs", "我的歌"], Level::Everyone),
        c(Action::Stats, &["songstats", "點歌統計"], Level::Everyone),
        c(Action::Remove, &["wrongsong", "刪歌"], Level::Everyone),
        c(Action::VoteSkip, &["voteskip", "卡歌"], Level::Everyone),
        c(Action::Skip, &["skip", "切歌"], Level::Moderator),
        c(Action::VoteBump, &["bump", "插歌"], Level::Everyone),
        c(Action::Pause, &["pause", "暫停"], Level::Moderator),
        c(Action::Resume, &["resume", "繼續"], Level::Moderator),
        c(Action::Help, &["songhelp", "點歌說明"], Level::Everyone),
    ]
}

#[derive(sqlx::FromRow)]
struct Row {
    id: i64,
    action: String,
    triggers: String,
    min_level: String,
    cooldown_secs: i64,
    enabled: bool,
    replies: String,
}

impl Row {
    fn into_command(self) -> Option<Command> {
        Some(Command {
            id: self.id,
            action: Action::parse(&self.action)?,
            triggers: serde_json::from_str(&self.triggers).unwrap_or_default(),
            min_level: Level::parse(&self.min_level),
            cooldown_secs: self.cooldown_secs,
            enabled: self.enabled,
            replies: serde_json::from_str(&self.replies).unwrap_or_default(),
        })
    }
}

pub async fn list(db: &SqlitePool) -> Result<Vec<Command>> {
    let rows: Vec<Row> = sqlx::query_as("SELECT * FROM commands ORDER BY id")
        .fetch_all(db)
        .await?;
    Ok(rows.into_iter().filter_map(Row::into_command).collect())
}

pub async fn get(db: &SqlitePool, id: i64) -> Result<Option<Command>> {
    let row: Option<Row> = sqlx::query_as("SELECT * FROM commands WHERE id = ?")
        .bind(id)
        .fetch_optional(db)
        .await?;
    Ok(row.and_then(Row::into_command))
}

pub async fn seed_defaults(db: &SqlitePool) -> Result<()> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM commands")
        .fetch_one(db)
        .await?;
    if count == 0 {
        for cmd in defaults() {
            insert(db, &cmd).await?;
        }
    }
    Ok(())
}

pub async fn insert(db: &SqlitePool, cmd: &Command) -> Result<i64> {
    sqlx::query_scalar(
        "INSERT INTO commands (action, triggers, min_level, cooldown_secs, enabled, replies) \
         VALUES (?, ?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(cmd.action.as_str())
    .bind(serde_json::to_string(&cmd.triggers)?)
    .bind(cmd.min_level.as_str())
    .bind(cmd.cooldown_secs)
    .bind(cmd.enabled)
    .bind(serde_json::to_string(&cmd.replies)?)
    .fetch_one(db)
    .await
    .context("insert command")
}

pub async fn update(db: &SqlitePool, cmd: &Command) -> Result<bool> {
    Ok(sqlx::query(
        "UPDATE commands SET action = ?, triggers = ?, min_level = ?, cooldown_secs = ?, \
         enabled = ?, replies = ? WHERE id = ?",
    )
    .bind(cmd.action.as_str())
    .bind(serde_json::to_string(&cmd.triggers)?)
    .bind(cmd.min_level.as_str())
    .bind(cmd.cooldown_secs)
    .bind(cmd.enabled)
    .bind(serde_json::to_string(&cmd.replies)?)
    .bind(cmd.id)
    .execute(db)
    .await?
    .rows_affected()
        > 0)
}

pub async fn delete(db: &SqlitePool, id: i64) -> Result<bool> {
    Ok(sqlx::query("DELETE FROM commands WHERE id = ?")
        .bind(id)
        .execute(db)
        .await?
        .rows_affected()
        > 0)
}

/// Clean up what the admin typed: strip the prefix, drop blanks, lower-case.
/// Errors when a trigger is used by another command.
pub fn normalize(cmd: &mut Command, prefix: &str, others: &[Command]) -> Result<(), String> {
    let mut seen = Vec::new();
    for t in &cmd.triggers {
        let t = t.trim().trim_start_matches(prefix).trim().to_lowercase();
        if t.is_empty() {
            continue;
        }
        if t.contains(char::is_whitespace) {
            return Err(format!("指令「{t}」不能有空白"));
        }
        if !seen.contains(&t) {
            seen.push(t);
        }
    }
    if seen.is_empty() {
        return Err("至少要有一個指令文字".into());
    }
    for other in others.iter().filter(|o| o.id != cmd.id) {
        if let Some(dup) = seen.iter().find(|t| other.triggers.contains(t)) {
            return Err(format!("「{prefix}{dup}」已經是別的指令"));
        }
    }
    cmd.triggers = seen;
    cmd.cooldown_secs = cmd.cooldown_secs.max(0);
    cmd.replies.retain(|_, v| !v.trim().is_empty());
    Ok(())
}

/// Characters chat clients add that aren't part of the message (7TV appends U+E0000 to
/// repeat a message, some clients add zero-width spaces).
const INVISIBLE: &[char] = &[
    '\u{E0000}',
    '\u{200B}',
    '\u{200C}',
    '\u{200D}',
    '\u{2060}',
    '\u{FEFF}',
];

pub fn clean(text: &str) -> String {
    text.chars().filter(|c| !INVISIBLE.contains(c)).collect()
}

/// `"!sr https://…"` → `("sr", "https://…")`. None when it isn't a command.
pub fn split_command<'a>(text: &'a str, prefix: &str) -> Option<(String, &'a str)> {
    let rest = text.trim().strip_prefix(prefix)?;
    let (word, args) = match rest.find(char::is_whitespace) {
        Some(i) => (&rest[..i], rest[i..].trim()),
        None => (rest, ""),
    };
    (!word.is_empty()).then(|| (word.to_lowercase(), args))
}

pub fn find<'a>(commands: &'a [Command], trigger: &str) -> Option<&'a Command> {
    let trigger = trigger.trim().to_lowercase();
    commands.iter().find(|c| c.triggers.contains(&trigger))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_action_has_defaults_and_names_round_trip() {
        for a in ALL_ACTIONS {
            assert!(
                a.default_reply("ok").is_some() || a.default_reply("voted").is_some(),
                "{a:?}"
            );
            assert_eq!(Action::parse(a.as_str()), Some(*a));
            let json = serde_json::to_value(a).unwrap();
            assert_eq!(json.as_str(), Some(a.as_str()), "serde name matches as_str");
            assert!(!a.variables().is_empty());
        }
        assert_eq!(
            Action::Request.default_reply("cooldown"),
            Some(COMMON_OUTCOMES[0].1)
        );
    }

    #[test]
    fn splits_commands() {
        assert_eq!(
            split_command("!sr https://youtu.be/x", "!"),
            Some(("sr".into(), "https://youtu.be/x"))
        );
        assert_eq!(
            split_command("  !SR   a b  ", "!"),
            Some(("sr".into(), "a b"))
        );
        assert_eq!(
            split_command("!點歌 小幸運", "!"),
            Some(("點歌".into(), "小幸運"))
        );
        assert_eq!(split_command("hello !sr", "!"), None);
        assert_eq!(split_command("!", "!"), None);
        assert_eq!(split_command("?sr x", "?"), Some(("sr".into(), "x")));
    }

    #[test]
    fn strips_invisible_characters() {
        assert_eq!(clean("!sr abc \u{E0000}"), "!sr abc ");
        assert_eq!(clean("!s\u{200B}r"), "!sr");
    }

    #[test]
    fn normalize_cleans_and_rejects_duplicates() {
        let existing = defaults()
            .into_iter()
            .enumerate()
            .map(|(i, mut c)| {
                c.id = i as i64 + 1;
                c
            })
            .collect::<Vec<_>>();
        let mut cmd = Command {
            id: 0,
            action: Action::Request,
            triggers: vec!["!Play".into(), " ".into(), "play".into()],
            min_level: Level::Everyone,
            cooldown_secs: -5,
            enabled: true,
            replies: BTreeMap::from([("ok".into(), "  ".into())]),
        };
        normalize(&mut cmd, "!", &existing).unwrap();
        assert_eq!(cmd.triggers, vec!["play"]);
        assert_eq!(cmd.cooldown_secs, 0);
        assert!(cmd.replies.is_empty());

        let mut dup = cmd.clone();
        dup.triggers = vec!["sr".into()];
        assert!(normalize(&mut dup, "!", &existing).is_err());
        let mut same = existing[0].clone();
        assert!(
            normalize(&mut same, "!", &existing).is_ok(),
            "a command may keep its own triggers"
        );
    }

    #[test]
    fn custom_reply_overrides_default() {
        let mut cmd = defaults().remove(0);
        assert!(cmd.reply_template("ok").contains("無情點播"));
        cmd.replies.insert("ok".into(), "收到 {title}".into());
        assert_eq!(cmd.reply_template("ok"), "收到 {title}");
        assert_eq!(cmd.reply_template("nope"), "");
    }
}
