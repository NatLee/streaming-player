import { Download, ExternalLink, RefreshCw } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { errorText, get, post, put } from "../api";
import { ConfirmButton, CopyField, ErrorBox, Field, Loading, Notice, NumberField, PageHeader, Section, Toggle } from "../components/ui";
import type { QueueRules, Settings, SettingsView, SystemInfo, YoutubeMode } from "../types";

const DOCS = "https://github.com/NatLee/streaming-player/blob/main/docs";

export function SettingsPage() {
  const [view, setView] = useState<SettingsView | null>(null);
  const [settings, setSettings] = useState<Settings | null>(null);
  const [system, setSystem] = useState<SystemInfo | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);
  const [updating, setUpdating] = useState(false);
  const [updateOut, setUpdateOut] = useState<string | null>(null);
  const [pw, setPw] = useState({ current: "", next: "" });
  const [pwMsg, setPwMsg] = useState<string | null>(null);

  const load = useCallback(() => {
    get<SettingsView>("/api/settings")
      .then((v) => {
        setView(v);
        setSettings(v.settings);
      })
      .catch((e) => setError(errorText(e)));
    get<SystemInfo>("/api/system").then(setSystem).catch(() => undefined);
  }, []);
  useEffect(load, [load]);

  if (!view || !settings) return error ? <ErrorBox message={error} /> : <Loading />;
  const q = settings.queue;
  const setQ = (patch: Partial<QueueRules>) => setSettings({ ...settings, queue: { ...q, ...patch } });

  const save = async () => {
    setError(null);
    try {
      await put("/api/settings", settings);
      setSaved(true);
      window.setTimeout(() => setSaved(false), 1500);
    } catch (e) {
      setError(errorText(e));
    }
  };

  return (
    <div className="mx-auto max-w-4xl space-y-5">
      <PageHeader
        title="設定"
        description="點歌規則、播放方式、OBS 網址和系統資訊。改完按「儲存」，所有播放頁會立刻套用。"
        actions={
          <button className="btn btn-primary" onClick={save}>
            {saved ? "已儲存" : "儲存設定"}
          </button>
        }
      />
      {error && <ErrorBox message={error} />}

      <Section title="點歌規則" description="0 代表不限制。管理員和實況主不受「每人上限」和指令冷卻限制（可以關掉）。">
        <div className="grid gap-5 sm:grid-cols-2 lg:grid-cols-3">
          <NumberField label="佇列最多幾首" value={q.max_queue} onChange={(v) => setQ({ max_queue: v })} suffix="首" />
          <NumberField label="每人最多排幾首" value={q.max_per_user} onChange={(v) => setQ({ max_per_user: v })} suffix="首" />
          <NumberField
            label="影片最長"
            value={Math.round(q.max_duration_secs / 60)}
            onChange={(v) => setQ({ max_duration_secs: v * 60 })}
            suffix="分鐘"
          />
          <NumberField label="投票跳過門檻" value={q.skip_votes} min={1} onChange={(v) => setQ({ skip_votes: v })} suffix="票" />
          <NumberField label="投票插歌門檻" value={q.bump_votes} min={1} onChange={(v) => setQ({ bump_votes: v })} suffix="票" />
          <NumberField
            label="投票有效時間"
            value={Math.round(q.vote_window_secs / 60)}
            onChange={(v) => setQ({ vote_window_secs: v * 60 })}
            suffix="分鐘"
          />
          <NumberField
            label="連續失敗幾次標成「不見了」"
            hint="不見了的歌不能再點，可以在歌曲庫恢復"
            value={q.missing_after_failures}
            onChange={(v) => setQ({ missing_after_failures: v })}
            suffix="次"
          />
        </div>
        <div className="mt-5 grid gap-3 sm:grid-cols-2">
          <Toggle checked={q.requests_open} onChange={(v) => setQ({ requests_open: v })} label="開放點歌" />
          <Toggle checked={q.allow_duplicates} onChange={(v) => setQ({ allow_duplicates: v })} label="允許重複點同一首" hint="關掉時，正在播或已經在佇列的歌不能再點" />
          <Toggle checked={q.privileged_bypass} onChange={(v) => setQ({ privileged_bypass: v })} label="管理員不受上限和冷卻限制" />
        </div>
        <p className="hint mt-4">每個指令的冷卻時間在「聊天指令」設定。</p>
      </Section>

      <Section title="播放">
        <div className="space-y-4">
          <Field label="YouTube 怎麼播">
            <select
              className="input w-80"
              value={settings.playback.youtube_mode}
              onChange={(e) => setSettings({ ...settings, playback: { ...settings.playback, youtube_mode: e.target.value as YoutubeMode } })}
            >
              <option value="embed">官方嵌入播放器（建議）</option>
              <option value="stream">yt-dlp 直接連結（v1 的方式，實驗性）</option>
            </select>
          </Field>
          {settings.playback.youtube_mode === "stream" && (
            <Notice tone="warn">
              YouTube 的直接連結常常播不了：YouTube 會擋掉不是它自己播放器的請求，查太多次還會暫時要求「確認不是機器人」。只有在嵌入播放器不能用時才建議改這個。
            </Notice>
          )}
          <Toggle
            checked={settings.playback.stream_fallback}
            onChange={(v) => setSettings({ ...settings, playback: { ...settings.playback, stream_fallback: v } })}
            label="嵌入失敗時改用 yt-dlp 直接連結"
            hint="讓作者不允許嵌入的影片有機會播放（實驗性，YouTube 不一定給連結）"
          />
          <Toggle
            checked={settings.playback.autoplay_favorites}
            onChange={(v) => setSettings({ ...settings, playback: { ...settings.playback, autoplay_favorites: v } })}
            label="佇列空了就播預設播放清單"
            hint="從歌曲庫裡打星號的歌隨機挑"
          />
        </div>
      </Section>

      <Section
        title="OBS"
        description={
          <>
            在 OBS 新增「瀏覽器」來源，網址貼下面的連結。播放頁建議 1920×1080，小卡 640×160 左右，背景是透明的。細節見{" "}
            <a className="underline" href={`${DOCS}/obs.md`} target="_blank" rel="noreferrer">
              OBS 設定說明 <ExternalLink className="inline size-3" />
            </a>
            。
          </>
        }
        actions={
          <ConfirmButton confirmLabel="OBS 網址會失效，確定？" onConfirm={() => post("/api/settings/keys/obs/rotate").then(load)}>
            <RefreshCw className="size-4" /> 重新產生金鑰
          </ConfirmButton>
        }
      >
        <div className="space-y-3">
          <Field label="播放頁（影片和聲音）">
            <CopyField value={view.links.player} label="player url" />
          </Field>
          <Field label="正在播放小卡（透明背景）">
            <CopyField value={view.links.overlay} label="overlay url" />
          </Field>
          <Notice>
            OBS 裡請勾選「透過 OBS 控制音訊」，音量就能在 OBS 混音器調整。網址裡有金鑰，不要放到畫面上。
          </Notice>
        </div>
      </Section>

      <Section title="觀眾頁">
        <Toggle
          checked={settings.viewer.public_queue}
          onChange={(v) => setSettings({ ...settings, viewer: { public_queue: v } })}
          label="開放觀眾頁"
          hint="任何人都能看佇列、查自己點的歌；聊天室的 !queue 會附上這個網址"
        />
        <div className="mt-3">
          <CopyField value={view.links.viewer} label="viewer url" />
        </div>
      </Section>

      <Section title="系統">
        {system ? (
          <div className="space-y-3 text-sm">
            <div>
              版本 <span className="font-mono">{system.version}</span>
            </div>
            <div className="flex flex-wrap items-center gap-3">
              <span>
                yt-dlp{" "}
                {system.ytdlp_version ? (
                  <span className="font-mono">{system.ytdlp_version}</span>
                ) : (
                  <span className="text-red-300">無法執行：{system.ytdlp_error}</span>
                )}
              </span>
              <button
                className="btn btn-secondary"
                disabled={updating}
                onClick={async () => {
                  setUpdating(true);
                  setUpdateOut(null);
                  try {
                    const r = await post<{ output: string; version: string | null }>("/api/system/ytdlp/update");
                    setUpdateOut(r.output);
                    load();
                  } catch (e) {
                    setUpdateOut(errorText(e));
                  } finally {
                    setUpdating(false);
                  }
                }}
              >
                <Download className="size-4" /> {updating ? "更新中…" : "更新 yt-dlp"}
              </button>
            </div>
            <p className="hint">網站常改版，yt-dlp 太舊就會抓不到。這裡更新的版本在容器重建後會回到映像檔內建的版本。</p>
            {updateOut && <pre className="overflow-x-auto rounded-lg bg-black/40 p-3 text-xs whitespace-pre-wrap">{updateOut}</pre>}
          </div>
        ) : (
          <Loading />
        )}
      </Section>

      <Section title="我的密碼">
        <form
          className="flex flex-wrap items-end gap-3"
          onSubmit={async (e) => {
            e.preventDefault();
            setPwMsg(null);
            try {
              await post("/api/auth/password", { current: pw.current, new: pw.next });
              setPw({ current: "", next: "" });
              setPwMsg("密碼已更新");
            } catch (err) {
              setPwMsg(errorText(err));
            }
          }}
        >
          <Field label="目前的密碼">
            <input type="password" className="input w-52" autoComplete="current-password" value={pw.current} onChange={(e) => setPw({ ...pw, current: e.target.value })} />
          </Field>
          <Field label="新密碼（至少 8 個字元）">
            <input type="password" className="input w-52" autoComplete="new-password" value={pw.next} onChange={(e) => setPw({ ...pw, next: e.target.value })} />
          </Field>
          <button className="btn btn-secondary">更新密碼</button>
          {pwMsg && <span className="text-sm text-zinc-400">{pwMsg}</span>}
        </form>
      </Section>
    </div>
  );
}
