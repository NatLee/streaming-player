import { ExternalLink, KeyRound, LogIn, LogOut, RefreshCw } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { del, errorText, get, post, put } from "../api";
import { useCan } from "../components/AdminLayout";
import { ConfirmButton, CopyField, ErrorBox, Field, Loading, Notice, PageHeader, Section, StatusPill, Toggle } from "../components/ui";
import { botSnippets, botUrl } from "../lib/bots";
import { ACTION_LABEL } from "../lib/commands";
import type { ChatInfo, CommandList, Settings, SettingsView } from "../types";

const DOCS = "https://github.com/NatLee/streaming-player/blob/main/docs";
/** YouTube Data API default daily quota. */
const DAILY_QUOTA = 10_000;

export function ChatPage() {
  const [view, setView] = useState<SettingsView | null>(null);
  const [settings, setSettings] = useState<Settings | null>(null);
  const [info, setInfo] = useState<ChatInfo | null>(null);
  const [commands, setCommands] = useState<CommandList | null>(null);
  const [apiKey, setApiKey] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);
  const canEdit = useCan()("commands.manage");

  const load = useCallback(() => {
    get<SettingsView>("/api/settings")
      .then((v) => {
        setView(v);
        setSettings(v.settings);
      })
      .catch((e) => setError(errorText(e)));
    get<ChatInfo>("/api/chat").then(setInfo).catch((e) => setError(errorText(e)));
    get<CommandList>("/api/commands").then(setCommands).catch(() => undefined);
  }, []);
  useEffect(load, [load]);

  // Poll the connection state while a sign-in or connection is in progress.
  useEffect(() => {
    const t = window.setInterval(() => get<ChatInfo>("/api/chat").then(setInfo).catch(() => undefined), 3000);
    return () => window.clearInterval(t);
  }, []);

  if (!view || !settings || !info) return error ? <ErrorBox message={error} /> : <Loading />;

  const save = async (next: Settings) => {
    setError(null);
    try {
      await put("/api/settings", next);
      setSaved(true);
      window.setTimeout(() => setSaved(false), 1500);
      load();
    } catch (e) {
      setError(errorText(e));
    }
  };
  const chat = settings.chat;
  const setChat = (c: Partial<Settings["chat"]>) => setSettings({ ...settings, chat: { ...chat, ...c } });
  const request = commands?.commands.find((c) => c.action === "request")?.triggers[0] ?? "sr";
  const unitsPerHour = Math.floor(3600 / Math.max(1, chat.youtube.poll_secs)) * 5;

  return (
    <div className="mx-auto max-w-5xl space-y-5">
      <PageHeader
        title="聊天室連線"
        description="觀眾在聊天室打指令點歌。可以直接連 Twitch 和 YouTube 聊天室，也可以讓任何會呼叫網址的聊天機器人來用。"
        actions={
          canEdit && (
            <button className="btn btn-primary" onClick={() => save(settings)}>
              {saved ? "已儲存" : "儲存設定"}
            </button>
          )
        }
      />
      {error && <ErrorBox message={error} />}

      <Section title="指令前綴" description="聊天訊息要用這個開頭才會被當成指令。">
        <input className="input w-24 font-mono" value={chat.prefix} onChange={(e) => setChat({ prefix: e.target.value })} />
      </Section>

      <Section
        title="Twitch"
        description={
          <>
            用 Twitch 官方建議的 EventSub 讀聊天室、用 Twitch API 回覆。需要先在 Twitch 開發者後台建一個應用程式（類型選「公開」），步驟見{" "}
            <a className="underline" href={`${DOCS}/twitch.md`} target="_blank" rel="noreferrer">
              Twitch 設定說明 <ExternalLink className="inline size-3" />
            </a>
            。
          </>
        }
        actions={<StatusPill state={info.twitch.status.state} detail={info.twitch.status.detail} />}
      >
        <div className="grid gap-4 md:grid-cols-2">
          <Field label="應用程式的 Client ID" hint={info.twitch.client_id_set && !chat.twitch.client_id ? "目前用的是 SP_TWITCH_CLIENT_ID 環境變數" : undefined}>
            <input className="input font-mono" value={chat.twitch.client_id} onChange={(e) => setChat({ twitch: { ...chat.twitch, client_id: e.target.value } })} />
          </Field>
          <Field label="頻道" hint="要讀的頻道登入名稱。留空就是登入帳號自己的頻道。">
            <input className="input" value={chat.twitch.channel} placeholder={info.twitch.login ?? ""} onChange={(e) => setChat({ twitch: { ...chat.twitch, channel: e.target.value } })} />
          </Field>
        </div>
        <div className="mt-4 flex flex-wrap items-center gap-3">
          <Toggle checked={chat.twitch.enabled} onChange={(v) => setChat({ twitch: { ...chat.twitch, enabled: v } })} label="啟用 Twitch 聊天室" />
          <div className="ml-auto flex items-center gap-2">
            {info.twitch.login ? (
              <>
                <span className="text-sm text-zinc-400">
                  已登入 <span className="font-medium text-zinc-100">{info.twitch.login}</span>
                </span>
                <ConfirmButton className="btn btn-secondary" confirmLabel="確定登出？" onConfirm={() => del("/api/chat/twitch").then(load)}>
                  <LogOut className="size-4" /> 登出
                </ConfirmButton>
              </>
            ) : null}
            <button
              className="btn btn-secondary"
              disabled={!info.twitch.client_id_set && !chat.twitch.client_id}
              onClick={async () => {
                try {
                  await put("/api/settings", settings);
                  await post("/api/chat/twitch/device");
                  load();
                } catch (e) {
                  setError(errorText(e));
                }
              }}
            >
              <LogIn className="size-4" /> {info.twitch.login ? "換一個帳號" : "登入 Twitch"}
            </button>
          </div>
        </div>
        {info.twitch.device && (
          <div className="mt-4">
            <Notice>
              打開{" "}
              <a className="font-medium underline" href={info.twitch.device.verification_uri} target="_blank" rel="noreferrer">
                {info.twitch.device.verification_uri}
              </a>
              ，輸入代碼 <code className="rounded bg-white/10 px-2 py-0.5 font-mono text-base text-gold-300">{info.twitch.device.user_code}</code>{" "}
              並同意權限（{info.twitch.scopes}）。這個頁面會自動更新。
            </Notice>
          </div>
        )}
        {info.twitch.status.detail && <p className="hint mt-3">{info.twitch.status.detail}</p>}
      </Section>

      <Section
        title="YouTube 直播聊天室"
        description={
          <>
            用 YouTube Live Streaming API 讀聊天室，只需要 API key。API key 不能在聊天室發言，回覆會記在控制台的「最近動態」。步驟見{" "}
            <a className="underline" href={`${DOCS}/youtube.md`} target="_blank" rel="noreferrer">
              YouTube 設定說明 <ExternalLink className="inline size-3" />
            </a>
            。
          </>
        }
        actions={<StatusPill state={info.youtube.status.state} detail={info.youtube.status.detail} />}
      >
        <div className="grid gap-4 md:grid-cols-2">
          <Field label="API key" hint={view.youtube_api_key_set ? "已設定。點歌查 YouTube 影片資訊時也會用它（每次 1 單位）。" : "也會用來查 YouTube 影片的標題和長度。"}>
            <div className="flex gap-2">
              <input
                className="input font-mono"
                type="password"
                placeholder={view.youtube_api_key_set ? "••••••••（已設定）" : "AIza…"}
                value={apiKey}
                onChange={(e) => setApiKey(e.target.value)}
              />
              <button
                className="btn btn-secondary"
                disabled={!apiKey && !view.youtube_api_key_set}
                onClick={() => put("/api/settings/youtube-key", { api_key: apiKey || null }).then(() => { setApiKey(""); load(); }).catch((e) => setError(errorText(e)))}
              >
                <KeyRound className="size-4" /> {apiKey ? "儲存" : "清除"}
              </button>
            </div>
          </Field>
          <Field label="直播網址" hint="直播影片的網址或 ID，例如 https://www.youtube.com/live/xxxxxxxxxxx">
            <input className="input" value={chat.youtube.video} onChange={(e) => setChat({ youtube: { ...chat.youtube, video: e.target.value } })} />
          </Field>
          <Field
            label="多久讀一次聊天室"
            hint={`每次 5 單位，每小時約 ${unitsPerHour.toLocaleString()} 單位；每天 ${DAILY_QUOTA.toLocaleString()} 單位的預設配額大約夠 ${(DAILY_QUOTA / unitsPerHour).toFixed(1)} 小時。`}
          >
            <div className="flex items-center gap-2">
              <input
                type="number"
                min={5}
                className="input w-24"
                value={chat.youtube.poll_secs}
                onChange={(e) => setChat({ youtube: { ...chat.youtube, poll_secs: Math.max(5, Number(e.target.value) || 5) } })}
              />
              <span className="text-sm text-zinc-400">秒</span>
            </div>
          </Field>
          <div className="pt-6">
            <Toggle checked={chat.youtube.enabled} onChange={(v) => setChat({ youtube: { ...chat.youtube, enabled: v } })} label="啟用 YouTube 聊天室" />
          </div>
        </div>
        {info.youtube.status.detail && <p className="hint mt-3">{info.youtube.status.detail}</p>}
      </Section>

      <Section
        title="聊天機器人（Nightbot、StreamElements、Fossabot、Streamer.bot…）"
        description={
          <>
            任何能呼叫網址的機器人都能用：在機器人裡新增指令，內容貼下面的文字。回覆是純文字。細節見{" "}
            <a className="underline" href={`${DOCS}/chat-bots.md`} target="_blank" rel="noreferrer">
              聊天機器人說明 <ExternalLink className="inline size-3" />
            </a>
            。
          </>
        }
      >
        <div className="mb-4 flex flex-wrap items-end gap-3">
          <div className="min-w-72 flex-1">
            <span className="label">Bot key（網址裡的 key，等於密碼）</span>
            <CopyField value={view.bot_key} secret label="bot key" />
          </div>
          <ConfirmButton confirmLabel="舊網址會失效，確定？" onConfirm={() => post("/api/settings/keys/bot/rotate").then(load)}>
            <RefreshCw className="size-4" /> 重新產生
          </ConfirmButton>
        </div>
        <p className="mb-2 text-sm text-zinc-400">
          點歌指令（<code className="font-mono">{chat.prefix}{request}</code>）的內容：
        </p>
        <div className="space-y-3">
          {botSnippets(view.links.bot_base, view.bot_key, request).map((s) => (
            <div key={s.bot}>
              <div className="mb-1 text-sm font-medium">{s.bot}</div>
              <CopyField value={s.text} label={s.bot} />
              <p className="hint">{s.note}</p>
            </div>
          ))}
        </div>
        {commands && (
          <details className="mt-4">
            <summary className="cursor-pointer text-sm text-zinc-400">其他指令的網址</summary>
            <ul className="mt-2 space-y-1.5">
              {commands.commands.map((c) => (
                <li key={c.id} className="grid items-center gap-2 md:grid-cols-[9rem_1fr]">
                  <span className="text-sm text-zinc-300">
                    {chat.prefix}
                    {c.triggers[0]}（{ACTION_LABEL[c.action]?.name}）
                  </span>
                  <CopyField value={botUrl(view.links.bot_base, view.bot_key, c.triggers[0] ?? "")} label={c.action} />
                </li>
              ))}
            </ul>
          </details>
        )}
      </Section>

    </div>
  );
}
