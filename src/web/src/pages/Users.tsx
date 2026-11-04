import { Trash2 } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { del, errorText, get, post, put } from "../api";
import { useUser } from "../components/AdminLayout";
import { ConfirmButton, ErrorBox, Field, PageHeader, Section } from "../components/ui";
import type { User } from "../types";

const ROLES = [
  { value: "admin", label: "管理員（全部功能）" },
  { value: "moderator", label: "協助管理（佇列和播放）" },
];

export function UsersPage() {
  const me = useUser();
  const [users, setUsers] = useState<User[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [form, setForm] = useState({ username: "", password: "", role: "moderator", email: "" });

  const load = useCallback(() => {
    get<User[]>("/api/users").then(setUsers).catch((e) => setError(errorText(e)));
  }, []);
  useEffect(load, [load]);

  const act = async (fn: () => Promise<unknown>) => {
    setError(null);
    try {
      await fn();
      load();
    } catch (e) {
      setError(errorText(e));
    }
  };

  return (
    <div className="mx-auto max-w-4xl space-y-5">
      <PageHeader
        title="使用者"
        description="可以登入管理頁的人。填了 Email 的人可以用 Google 登入（伺服器要先設定 Google 登入）。"
      />
      {error && <ErrorBox message={error} />}
      <Section title="新增">
        <form
          className="grid gap-3 md:grid-cols-5 md:items-end"
          onSubmit={(e) => {
            e.preventDefault();
            act(() => post("/api/users", { ...form, email: form.email || null, password: form.password || null })).then(() =>
              setForm({ username: "", password: "", role: "moderator", email: "" }),
            );
          }}
        >
          <Field label="帳號">
            <input className="input" value={form.username} onChange={(e) => setForm({ ...form, username: e.target.value })} required />
          </Field>
          <Field label="密碼">
            <input className="input" type="password" value={form.password} onChange={(e) => setForm({ ...form, password: e.target.value })} />
          </Field>
          <Field label="Email（Google 登入）">
            <input className="input" type="email" value={form.email} onChange={(e) => setForm({ ...form, email: e.target.value })} />
          </Field>
          <Field label="角色">
            <select className="input" value={form.role} onChange={(e) => setForm({ ...form, role: e.target.value })}>
              {ROLES.map((r) => (
                <option key={r.value} value={r.value}>
                  {r.label}
                </option>
              ))}
            </select>
          </Field>
          <button className="btn btn-primary">新增</button>
        </form>
      </Section>
      <Section title="所有使用者">
        <ul className="divide-y divide-white/5">
          {users.map((u) => (
            <li key={u.id} className="flex flex-wrap items-center gap-3 py-2.5">
              <span className="w-32 font-medium">{u.username}</span>
              <span className="flex-1 text-sm text-zinc-500">
                {u.email ?? "沒有 Email"} {u.google_linked && "· 已連結 Google"}
              </span>
              <select
                className="input w-56"
                value={u.role}
                disabled={u.id === me?.id}
                onChange={(e) => act(() => put(`/api/users/${u.id}`, { role: e.target.value }))}
              >
                {ROLES.map((r) => (
                  <option key={r.value} value={r.value}>
                    {r.label}
                  </option>
                ))}
              </select>
              {u.id !== me?.id && (
                <ConfirmButton className="icon-btn hover:text-red-400" confirmLabel="刪？" title="刪除" onConfirm={() => act(() => del(`/api/users/${u.id}`))}>
                  <Trash2 className="size-4" />
                </ConfirmButton>
              )}
            </li>
          ))}
        </ul>
      </Section>
    </div>
  );
}
