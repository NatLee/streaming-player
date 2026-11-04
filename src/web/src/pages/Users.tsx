import { Plus, Save, Trash2 } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { del, errorText, get, post, put } from "../api";
import { useUser } from "../components/AdminLayout";
import { ConfirmButton, ErrorBox, Field, Notice, PageHeader, Section } from "../components/ui";
import { ago, when } from "../lib/format";
import type { ActivityEvent, Perm, Role, RoleList, User } from "../types";

function UserRow({ u, roles, isMe, onChange }: { u: User; roles: Role[]; isMe: boolean; onChange: (body: object) => Promise<void> }) {
  const [password, setPassword] = useState("");
  return (
    <tr className={u.disabled ? "opacity-50" : ""}>
      <td className="td">
        <div className="font-medium">
          {u.username} {isMe && <span className="chip">你</span>}
        </div>
        <div className="text-xs text-zinc-500">
          {u.email ?? "沒有 Email"}
          {u.google_linked && " · 已連結 Google"}
        </div>
      </td>
      <td className="td">
        <select className="input w-40" aria-label={`${u.username} 的角色`} value={u.role} onChange={(e) => onChange({ role: e.target.value })}>
          {roles.map((r) => (
            <option key={r.name} value={r.name}>
              {r.label}
            </option>
          ))}
        </select>
      </td>
      <td className="td text-xs text-zinc-400">
        {u.last_login_at ? ago(u.last_login_at) : "還沒登入過"}
        <div className="text-zinc-600">建立 {when(u.created_at)}</div>
      </td>
      <td className="td">
        <div className="flex flex-wrap items-center justify-end gap-1.5">
          <input
            type="password"
            className="input w-36"
            placeholder="新密碼"
            aria-label={`${u.username} 的新密碼`}
            value={password}
            onChange={(e) => setPassword(e.target.value)}
          />
          <button className="btn btn-secondary" disabled={password.length < 8} onClick={() => onChange({ password }).then(() => setPassword(""))}>
            重設密碼
          </button>
          <button className="btn btn-secondary" onClick={() => onChange({ disabled: !u.disabled })}>
            {u.disabled ? "啟用" : "停用"}
          </button>
          {!isMe && (
            <ConfirmButton className="icon-btn hover:text-red-400" title="刪除" confirmLabel="刪？" onConfirm={() => del(`/api/users/${u.id}`).then(() => onChange({}))}>
              <Trash2 className="size-4" />
            </ConfirmButton>
          )}
        </div>
      </td>
    </tr>
  );
}

function RoleEditor({
  role,
  catalog,
  onSaved,
}: {
  role: Role | null;
  catalog: RoleList["permissions"];
  onSaved: () => void;
}) {
  const [name, setName] = useState(role?.name ?? "");
  const [label, setLabel] = useState(role?.label ?? "");
  const [perms, setPerms] = useState<Perm[]>(role?.permissions ?? []);
  const [error, setError] = useState<string | null>(null);
  const locked = role?.locked ?? false;
  const save = async () => {
    setError(null);
    try {
      if (role) await put(`/api/roles/${role.name}`, { label, permissions: perms });
      else await post("/api/roles", { name, label, permissions: perms });
      onSaved();
    } catch (e) {
      setError(errorText(e));
    }
  };
  return (
    <div className="card p-4">
      <div className="flex flex-wrap items-end gap-3">
        {role ? (
          <div className="font-mono text-sm text-zinc-400">{role.name}</div>
        ) : (
          <Field label="代號（英文小寫）">
            <input className="input w-40 font-mono" value={name} onChange={(e) => setName(e.target.value)} />
          </Field>
        )}
        <Field label="名稱">
          <input className="input w-44" value={label} disabled={locked} onChange={(e) => setLabel(e.target.value)} />
        </Field>
        {role && <span className="text-xs text-zinc-500">{role.users} 個帳號</span>}
        {role?.builtin && <span className="chip">內建</span>}
        <div className="ml-auto flex gap-2">
          {role && !role.builtin && (
            <ConfirmButton className="btn btn-ghost" confirmLabel="確定刪除？" onConfirm={() => del(`/api/roles/${role.name}`).then(onSaved).catch((e) => setError(errorText(e)))}>
              <Trash2 className="size-4" /> 刪除
            </ConfirmButton>
          )}
          {!locked && (
            <button className="btn btn-primary" onClick={save}>
              {role ? <Save className="size-4" /> : <Plus className="size-4" />} {role ? "儲存" : "新增角色"}
            </button>
          )}
        </div>
      </div>
      {locked ? (
        <p className="mt-3 text-sm text-zinc-400">這個角色固定擁有全部權限，不能修改。</p>
      ) : (
        <div className="mt-3 grid gap-1.5 sm:grid-cols-2">
          {catalog.map((p) => (
            <label key={p.id} className="flex items-center gap-2 text-sm text-zinc-300">
              <input
                type="checkbox"
                className="accent-gold-400"
                checked={perms.includes(p.id)}
                onChange={(e) => setPerms(e.target.checked ? [...perms, p.id] : perms.filter((x) => x !== p.id))}
              />
              {p.label}
            </label>
          ))}
        </div>
      )}
      {error && <div className="mt-3"><ErrorBox message={error} /></div>}
    </div>
  );
}

/** Accounts, roles with their permission points, and the audit log. */
export function UsersPage() {
  const me = useUser();
  const [users, setUsers] = useState<User[]>([]);
  const [roles, setRoles] = useState<RoleList | null>(null);
  const [audit, setAudit] = useState<ActivityEvent[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [form, setForm] = useState({ username: "", password: "", role: "viewer", email: "" });
  const [version, setVersion] = useState(0);

  const load = useCallback(() => {
    get<User[]>("/api/users").then(setUsers).catch((e) => setError(errorText(e)));
    get<RoleList>("/api/roles").then(setRoles).catch((e) => setError(errorText(e)));
    get<ActivityEvent[]>("/api/users/audit").then(setAudit).catch(() => undefined);
    setVersion((v) => v + 1);
  }, []);
  useEffect(load, [load]);

  const act = async (fn: () => Promise<unknown>) => {
    setError(null);
    try {
      await fn();
    } catch (e) {
      setError(errorText(e));
    }
    load();
  };

  return (
    <div className="mx-auto max-w-5xl space-y-5">
      <PageHeader
        title="使用者與權限"
        description="每個帳號有一個角色，角色決定能做哪些事（權限點）。至少要留一個能管理使用者的帳號：最後一個不能被停用、降級或刪除。"
      />
      {error && <ErrorBox message={error} />}

      <Section title="帳號">
        <div className="overflow-x-auto">
          <table className="w-full">
            <thead className="border-b border-white/6">
              <tr>
                <th className="th">帳號</th>
                <th className="th">角色</th>
                <th className="th">最後登入</th>
                <th className="th text-right">動作</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-white/5">
              {users.map((u) => (
                <UserRow key={u.id} u={u} roles={roles?.roles ?? []} isMe={u.id === me?.id} onChange={(body) => act(() => put(`/api/users/${u.id}`, body))} />
              ))}
            </tbody>
          </table>
        </div>
        <form
          className="mt-4 grid gap-3 border-t border-white/6 pt-4 md:grid-cols-5 md:items-end"
          onSubmit={(e) => {
            e.preventDefault();
            act(() => post("/api/users", { ...form, email: form.email || null, password: form.password || null })).then(() =>
              setForm({ username: "", password: "", role: roles?.default_role ?? "viewer", email: "" }),
            );
          }}
        >
          <Field label="新帳號">
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
              {(roles?.roles ?? []).map((r) => (
                <option key={r.name} value={r.name}>
                  {r.label}
                </option>
              ))}
            </select>
          </Field>
          <button className="btn btn-primary">新增</button>
        </form>
      </Section>

      <Section title="角色和權限點" description="內建角色可以調整權限，但不能刪除；新帳號拿到的預設角色在「設定 → 帳號與註冊」。">
        {roles && (
          <div className="space-y-3" key={version}>
            {roles.roles.map((r) => (
              <RoleEditor key={r.name} role={r} catalog={roles.permissions} onSaved={load} />
            ))}
            <RoleEditor role={null} catalog={roles.permissions} onSaved={load} />
          </div>
        )}
      </Section>

      <Section title="稽核紀錄" description="改角色、停用、刪除、改權限、API token 的紀錄。">
        {audit.length === 0 ? (
          <Notice>還沒有紀錄</Notice>
        ) : (
          <ul className="space-y-1.5 text-sm">
            {audit.map((e) => (
              <li key={e.id} className="flex gap-3">
                <span className="w-28 shrink-0 text-xs text-zinc-500">{when(e.at)}</span>
                <span className="font-medium text-gold-300">{e.actor}</span>
                <span className="text-zinc-300">{e.detail}</span>
              </li>
            ))}
          </ul>
        )}
      </Section>
    </div>
  );
}
