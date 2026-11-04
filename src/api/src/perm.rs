//! Permission points. Roles are sets of these, and every API checks one of them,
//! never a role name.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, ToSchema,
)]
pub enum Perm {
    /// Play, pause, skip, volume; send player reports.
    #[serde(rename = "player.control")]
    PlayerControl,
    /// See the queue, the player state and the live updates.
    #[serde(rename = "queue.view")]
    QueueView,
    /// Request songs from the web page (queue rules apply).
    #[serde(rename = "queue.request")]
    QueueRequest,
    /// Reorder, play next, interrupt, remove, clear; add songs without the limits.
    #[serde(rename = "queue.manage")]
    QueueManage,
    /// The song library and the default playlist, import and export.
    #[serde(rename = "library.manage")]
    LibraryManage,
    /// Block and unblock chatters and songs.
    #[serde(rename = "blocklist.manage")]
    BlocklistManage,
    /// Request history, statistics and the activity log.
    #[serde(rename = "history.view")]
    HistoryView,
    /// Chat commands and chat connections (Twitch, YouTube, bot key).
    #[serde(rename = "commands.manage")]
    CommandsManage,
    /// Read the settings, OBS links and system information.
    #[serde(rename = "settings.view")]
    SettingsView,
    /// Change queue rules, playback, viewer page, OBS key, yt-dlp.
    #[serde(rename = "settings.manage")]
    SettingsManage,
    /// Users, roles, registration and the audit log.
    #[serde(rename = "users.manage")]
    UsersManage,
}

pub struct PermInfo {
    pub perm: Perm,
    pub id: &'static str,
    pub label: &'static str,
}

/// Every permission point with its id and the label the admin page shows.
pub const ALL: &[PermInfo] = &[
    PermInfo {
        perm: Perm::PlayerControl,
        id: "player.control",
        label: "控制播放（播放、暫停、跳過、音量）",
    },
    PermInfo {
        perm: Perm::QueueView,
        id: "queue.view",
        label: "看佇列和播放狀態",
    },
    PermInfo {
        perm: Perm::QueueRequest,
        id: "queue.request",
        label: "從網頁點歌",
    },
    PermInfo {
        perm: Perm::QueueManage,
        id: "queue.manage",
        label: "管理佇列（排序、插播、刪除）",
    },
    PermInfo {
        perm: Perm::LibraryManage,
        id: "library.manage",
        label: "管理歌曲庫和預設播放清單",
    },
    PermInfo {
        perm: Perm::BlocklistManage,
        id: "blocklist.manage",
        label: "管理黑名單",
    },
    PermInfo {
        perm: Perm::HistoryView,
        id: "history.view",
        label: "看點歌歷史和動態",
    },
    PermInfo {
        perm: Perm::CommandsManage,
        id: "commands.manage",
        label: "管理聊天指令和聊天室連線",
    },
    PermInfo {
        perm: Perm::SettingsView,
        id: "settings.view",
        label: "看設定",
    },
    PermInfo {
        perm: Perm::SettingsManage,
        id: "settings.manage",
        label: "改設定",
    },
    PermInfo {
        perm: Perm::UsersManage,
        id: "users.manage",
        label: "管理使用者、角色和註冊",
    },
];

impl Perm {
    pub fn info(self) -> &'static PermInfo {
        ALL.iter()
            .find(|i| i.perm == self)
            .expect("every permission is listed in ALL")
    }

    pub fn id(self) -> &'static str {
        self.info().id
    }

    pub fn parse(id: &str) -> Option<Perm> {
        ALL.iter().find(|i| i.id == id).map(|i| i.perm)
    }

    pub fn all() -> Vec<Perm> {
        ALL.iter().map(|i| i.perm).collect()
    }
}

/// Permission ids from a stored JSON array; unknown ids are dropped.
pub fn parse_list(json: &str) -> Vec<Perm> {
    let ids: Vec<String> = serde_json::from_str(json).unwrap_or_default();
    let mut perms: Vec<Perm> = ids.iter().filter_map(|i| Perm::parse(i)).collect();
    perms.sort();
    perms.dedup();
    perms
}

pub fn to_json(perms: &[Perm]) -> String {
    serde_json::to_string(&perms.iter().map(|p| p.id()).collect::<Vec<_>>())
        .unwrap_or_else(|_| "[]".into())
}

/// Types that name a permission, so a handler can ask for it in its signature:
/// `Can<need::QueueManage>`.
pub trait PermMarker {
    const PERM: Perm;
}

pub mod need {
    use super::{Perm, PermMarker};

    macro_rules! markers {
        ($($name:ident),* $(,)?) => {
            $(
                pub struct $name;
                impl PermMarker for $name {
                    const PERM: Perm = Perm::$name;
                }
            )*
        };
    }

    markers!(
        PlayerControl,
        QueueView,
        QueueRequest,
        QueueManage,
        LibraryManage,
        BlocklistManage,
        HistoryView,
        CommandsManage,
        SettingsView,
        SettingsManage,
        UsersManage,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_match_serde_names() {
        for info in ALL {
            assert_eq!(serde_json::to_value(info.perm).unwrap(), info.id);
            assert_eq!(Perm::parse(info.id), Some(info.perm));
        }
        assert_eq!(ALL.len(), Perm::all().len());
    }

    #[test]
    fn stored_lists_ignore_unknown_ids() {
        assert_eq!(
            parse_list(r#"["queue.view","nope","player.control","queue.view"]"#),
            vec![Perm::PlayerControl, Perm::QueueView]
        );
        assert_eq!(parse_list("garbage"), Vec::<Perm>::new());
        assert_eq!(to_json(&[Perm::QueueView]), r#"["queue.view"]"#);
    }

    #[test]
    fn builtin_role_permissions_in_the_migration_are_known() {
        let sql = include_str!("../migrations/0002_roles_tokens.sql");
        for line in sql
            .lines()
            .filter(|l| l.contains("('moderator'") || l.contains("('viewer'"))
        {
            let start = line.find("'[").unwrap() + 1;
            let end = line.find("]'").unwrap() + 1;
            let json = &line[start..end];
            let ids: Vec<String> = serde_json::from_str(json).unwrap();
            assert_eq!(parse_list(json).len(), ids.len(), "unknown id in {json}");
        }
    }
}
