import { KeyRound, Trash2 } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { del, errorText, get, post } from "../api";
import { useUser } from "../components/AdminLayout";
import { ConfirmButton, CopyField, ErrorBox, Field, Notice, PageHeader, Section } from "../components/ui";
import { PERM_LABEL } from "../lib/perms";
import { when } from "../lib/format";
import type { ApiTokenInfo, Perm } from "../types";

/** Your own account: password, and personal API tokens for programs. */
export function AccountPage() {
  const user = useUser();
  const [pw, setPw] = useState({ current: "", next: "" });
  const [pwMsg, setPwMsg] = useState<{ ok: boolean; text: string } | null>(null);
  const [tokens, setTokens] = useState<ApiTokenInfo[]>([]);
  const [name, setName] = useState("");
  const [picked, setPicked] = useState<Perm[]>(["player.control"]);
  const [days, setDays] = useState(0);
  const [made, setMade] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(() => {
    get<ApiTokenInfo[]>("/api/auth/api-tokens").then(setTokens).catch((e) => setError(errorText(e)));
  }, []);
  useEffect(load, [load]);
  if (!user) return null;

  return (
    <div className="mx-auto max-w-3xl space-y-5">
      <PageHeader title="我的帳號" description={`${user.username}，角色：${user.role_label}`} />
      {error && <ErrorBox message={error} />}

      <Section title="密碼" description="改密碼後，你在其他地方的登入和 API 的 refresh token 都會登出，這個頁面保持登入。">
        <form
          className="flex flex-wrap items-end gap-3"
          onSubmit={async (e) => {
            e.preventDefault();
            setPwMsg(null);
            try {
              await post("/api/auth/password", { current: pw.current, new: pw.next });
              setPw({ current: "", next: "" });
              setPwMsg({ ok: true, text: "密碼已更新" });
            } catch (err) {
              setPwMsg({ ok: false, text: errorText(err) });
            }
          }}
        >
          {user.has_password && (
            <Field label="目前的密碼">
              <input type="password" className="input w-52" autoComplete="current-password" value={pw.current} onChange={(e) => setPw({ ...pw, current: e.target.value })} />
            </Field>
          )}
          <Field label="新密碼（至少 8 個字元）">
            <input type="password" className="input w-52" autoComplete="new-password" value={pw.next} onChange={(e) => setPw({ ...pw, next: e.target.value })} />
          </Field>
          <button className="btn btn-secondary">更新密碼</button>
        </form>
        {pwMsg && <div className="mt-3">{pwMsg.ok ? <Notice tone="ok">{pwMsg.text}</Notice> : <ErrorBox message={pwMsg.text} />}</div>}
      </Section>

      <Section
        title="個人 API token"
        description="給 Streamer.bot、Stream Deck 或自己的程式用：在請求加上 Authorization: Bearer <token>。token 只能做你勾選的事，也不會超過你自己的權限。"
      >
        <form
          className="space-y-3"
          onSubmit={async (e) => {
            e.preventDefault();
            setError(null);
            try {
              const r = await post<{ token: string }>("/api/auth/api-tokens", {
                name,
                permissions: picked,
                expires_days: days || null,
              });
              setMade(r.token);
              setName("");
              load();
            } catch (err) {
              setError(errorText(err));
            }
          }}
        >
          <div className="flex flex-wrap items-end gap-3">
            <Field label="名稱">
              <input className="input w-56" placeholder="例如 stream deck" value={name} onChange={(e) => setName(e.target.value)} required />
            </Field>
            <Field label="幾天後失效（0 = 不失效）">
              <input type="number" min={0} className="input w-28" value={days} onChange={(e) => setDays(Math.max(0, Number(e.target.value) || 0))} />
            </Field>
          </div>
          <div className="grid gap-1.5 sm:grid-cols-2">
            {user.permissions.map((p) => (
              <label key={p} className="flex items-center gap-2 text-sm text-zinc-300">
                <input
                  type="checkbox"
                  className="accent-gold-400"
                  checked={picked.includes(p)}
                  onChange={(e) => setPicked(e.target.checked ? [...picked, p] : picked.filter((x) => x !== p))}
                />
                {PERM_LABEL[p] ?? p}
              </label>
            ))}
          </div>
          <button className="btn btn-primary" disabled={!picked.length}>
            <KeyRound className="size-4" /> 產生 token
          </button>
        </form>
        {made && (
          <div className="mt-4 space-y-2">
            <Notice tone="warn">這個 token 只會顯示這一次，請現在複製起來。</Notice>
            <CopyField value={made} label="new api token" />
          </div>
        )}
        <ul className="mt-4 divide-y divide-white/5">
          {tokens.map((t) => (
            <li key={t.id} className="flex flex-wrap items-center gap-3 py-2 text-sm">
              <span className="font-medium">{t.name}</span>
              <span className="flex-1 text-xs text-zinc-500">{t.permissions.map((p) => PERM_LABEL[p] ?? p).join("、")}</span>
              <span className="text-xs text-zinc-500">
                {t.last_used_at ? `最後使用 ${when(t.last_used_at)}` : "還沒用過"}
                {t.expires_at ? ` · ${when(t.expires_at)} 失效` : ""}
              </span>
              <ConfirmButton className="icon-btn hover:text-red-400" title="撤銷" confirmLabel="撤銷？" onConfirm={() => del(`/api/auth/api-tokens/${t.id}`).then(load)}>
                <Trash2 className="size-4" />
              </ConfirmButton>
            </li>
          ))}
          {tokens.length === 0 && <li className="py-2 text-sm text-zinc-500">還沒有 token</li>}
        </ul>
      </Section>
    </div>
  );
}
