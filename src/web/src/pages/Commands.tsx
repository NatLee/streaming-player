import { ChevronDown, ChevronRight, Plus, RotateCcw, Save, Trash2 } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { del, errorText, get, post, put } from "../api";
import { useCan } from "../components/AdminLayout";
import { ConfirmButton, ErrorBox, Loading, Notice, PageHeader, Toggle } from "../components/ui";
import { ACTION_LABEL, OUTCOME_LABEL } from "../lib/commands";
import { LEVELS } from "../lib/format";
import type { Action, ActionInfo, Command, CommandList, Level } from "../types";

function CommandCard({
  cmd,
  info,
  prefix,
  onSaved,
  readOnly = false,
}: {
  cmd: Command;
  info: ActionInfo | undefined;
  prefix: string;
  onSaved: () => void;
  readOnly?: boolean;
}) {
  const [draft, setDraft] = useState(cmd);
  const [triggers, setTriggers] = useState(cmd.triggers.join(", "));
  const [open, setOpen] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);
  const label = ACTION_LABEL[cmd.action];
  const dirty = JSON.stringify({ ...draft, triggers: triggers.split(/[,，\s]+/).filter(Boolean) }) !== JSON.stringify(cmd);

  const save = async () => {
    setError(null);
    try {
      const body = { ...draft, triggers: triggers.split(/[,，\s]+/).filter(Boolean) };
      if (cmd.id) await put(`/api/commands/${cmd.id}`, body);
      else await post("/api/commands", body);
      setSaved(true);
      window.setTimeout(() => setSaved(false), 1500);
      onSaved();
    } catch (e) {
      setError(errorText(e));
    }
  };

  return (
    <div className={`card p-4 ${draft.enabled ? "" : "opacity-60"}`}>
      <div className="flex flex-wrap items-start gap-4">
        <div className="w-40 shrink-0">
          <div className="font-semibold">{label?.name ?? cmd.action}</div>
          <div className="text-xs text-zinc-500">{label?.what}</div>
        </div>
        <div className="min-w-56 flex-1">
          <label className="label text-xs">指令文字（用逗號分開）</label>
          <div className="flex items-center gap-1">
            <span className="font-mono text-zinc-500">{prefix}</span>
            <input className="input font-mono" value={triggers} onChange={(e) => setTriggers(e.target.value)} />
          </div>
        </div>
        <div>
          <label className="label text-xs">誰能用</label>
          <select className="input w-36" value={draft.min_level} onChange={(e) => setDraft({ ...draft, min_level: e.target.value as Level })}>
            {LEVELS.map((l) => (
              <option key={l.value} value={l.value}>
                {l.label}
                {l.value !== "everyone" && l.value !== "broadcaster" ? "以上" : ""}
              </option>
            ))}
          </select>
        </div>
        <div>
          <label className="label text-xs">冷卻（秒／每人）</label>
          <input
            type="number"
            min={0}
            className="input w-24"
            value={draft.cooldown_secs}
            onChange={(e) => setDraft({ ...draft, cooldown_secs: Math.max(0, Number(e.target.value) || 0) })}
          />
        </div>
        <div className="pt-6">
          <Toggle checked={draft.enabled} onChange={(v) => setDraft({ ...draft, enabled: v })} label="啟用" />
        </div>
      </div>
      <div className="mt-3 flex items-center gap-2">
        <button className="inline-flex items-center gap-1 text-sm text-zinc-400 hover:text-zinc-200" onClick={() => setOpen(!open)}>
          {open ? <ChevronDown className="size-4" /> : <ChevronRight className="size-4" />}
          回覆文字 {Object.keys(draft.replies).length > 0 && <span className="chip">已自訂 {Object.keys(draft.replies).length} 則</span>}
        </button>
        <div className={`ml-auto flex items-center gap-2 ${readOnly ? "hidden" : ""}`}>
          {cmd.id > 0 && (
            <ConfirmButton className="btn btn-ghost" confirmLabel="確定刪除？" onConfirm={() => del(`/api/commands/${cmd.id}`).then(onSaved)}>
              <Trash2 className="size-4" /> 刪除
            </ConfirmButton>
          )}
          <button className="btn btn-primary" disabled={!dirty && cmd.id > 0} onClick={save}>
            <Save className="size-4" /> {saved ? "已儲存" : "儲存"}
          </button>
        </div>
      </div>
      {open && info && (
        <div className="mt-3 space-y-3">
          <p className="text-xs text-zinc-500">
            留空就用預設文字。可以用的變數：{info.variables.map((v) => <code key={v} className="mx-0.5 rounded bg-white/8 px-1">{`{${v}}`}</code>)}
          </p>
          {info.outcomes.map((o) => (
            <div key={o.key} className="grid gap-1 md:grid-cols-[10rem_1fr] md:items-start md:gap-3">
              <span className="pt-2 text-sm text-zinc-400">{OUTCOME_LABEL[o.key] ?? o.key}</span>
              <textarea
                rows={1}
                className="input min-h-9 resize-y"
                placeholder={o.default_text}
                value={draft.replies[o.key] ?? ""}
                onChange={(e) => {
                  const replies = { ...draft.replies };
                  if (e.target.value) replies[o.key] = e.target.value;
                  else delete replies[o.key];
                  setDraft({ ...draft, replies });
                }}
              />
            </div>
          ))}
        </div>
      )}
      {error && <div className="mt-3"><ErrorBox message={error} /></div>}
    </div>
  );
}

export function CommandsPage() {
  const [data, setData] = useState<CommandList | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [adding, setAdding] = useState<Action | "">("");
  const [version, setVersion] = useState(0);
  const readOnly = !useCan()("commands.manage");

  const load = useCallback(() => {
    get<CommandList>("/api/commands")
      .then((d) => {
        setData(d);
        setVersion((v) => v + 1);
      })
      .catch((e) => setError(errorText(e)));
  }, []);
  useEffect(load, [load]);

  if (!data) return error ? <ErrorBox message={error} /> : <Loading />;
  const info = (a: Action) => data.actions.find((x) => x.action === a);

  return (
    <div className="mx-auto max-w-5xl">
      <PageHeader
        title="聊天指令"
        description="Twitch、YouTube 聊天室和所有聊天機器人共用這份指令表。每個指令都能改觸發文字、權限、冷卻時間和回覆文字。"
        actions={
          !readOnly && (
            <ConfirmButton confirmLabel="確定恢復預設？" onConfirm={() => post("/api/commands/reset").then(load)}>
              <RotateCcw className="size-4" /> 恢復預設
            </ConfirmButton>
          )
        }
      />
      <div className="mb-4">
        <Notice>
          指令前綴是 <code className="font-mono">{data.prefix}</code>，在「聊天室連線」可以改。聊天機器人的網址用不加前綴的指令文字，例如{" "}
          <code className="font-mono">/api/bot/{data.commands[0]?.triggers[0] ?? "sr"}</code>。
        </Notice>
      </div>
      <div className="space-y-3" key={version}>
        {data.commands.map((c) => (
          <CommandCard key={c.id} cmd={c} info={info(c.action)} prefix={data.prefix} onSaved={load} readOnly={readOnly} />
        ))}
        {adding && (
          <CommandCard
            cmd={{ id: 0, action: adding, triggers: [], min_level: "everyone", cooldown_secs: 0, enabled: true, replies: {} }}
            info={info(adding)}
            prefix={data.prefix}
            onSaved={() => {
              setAdding("");
              load();
            }}
          />
        )}
      </div>
      <div className={`mt-4 flex items-center gap-2 ${readOnly ? "hidden" : ""}`}>
        <select className="input w-48" value={adding} onChange={(e) => setAdding(e.target.value as Action)}>
          <option value="">新增指令…</option>
          {data.actions.map((a) => (
            <option key={a.action} value={a.action}>
              {ACTION_LABEL[a.action]?.name ?? a.action}
            </option>
          ))}
        </select>
        {adding && (
          <span className="inline-flex items-center gap-1 text-sm text-zinc-400">
            <Plus className="size-4" /> 填好指令文字後按儲存
          </span>
        )}
      </div>
    </div>
  );
}
