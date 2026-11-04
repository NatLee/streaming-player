import {
  Ban,
  BookText,
  CircleUser,
  History,
  LayoutDashboard,
  Library,
  Lock,
  LogOut,
  MessagesSquare,
  Settings,
  SquareTerminal,
  Users,
} from "lucide-react";
import { createContext, useCallback, useContext, useEffect, useState, type ReactNode } from "react";
import { NavLink, Outlet, useNavigate } from "react-router";
import { get, post } from "../api";
import { PERM_LABEL } from "../lib/perms";
import type { AuthState, Perm, User } from "../types";
import { Loading } from "./ui";

interface Session {
  user: User;
  /** Reload the signed-in user (after a role change, say). */
  refresh: () => void;
}

const SessionContext = createContext<Session | null>(null);
export const useUser = () => useContext(SessionContext)?.user ?? null;

/** Does the signed-in user hold this permission? The server checks again; this only hides buttons. */
export function useCan(): (p: Perm) => boolean {
  const user = useUser();
  return useCallback((p: Perm) => Boolean(user?.permissions.includes(p)), [user]);
}

/** Show the page only with the permission; otherwise say what is missing and where to get it. */
export function Require({ perm, children }: { perm: Perm; children: ReactNode }) {
  const can = useCan();
  if (can(perm)) return <>{children}</>;
  return (
    <div className="mx-auto mt-20 flex max-w-md flex-col items-center gap-3 text-center text-zinc-400">
      <Lock className="size-10 text-zinc-600" />
      <div className="text-lg font-medium text-zinc-200">沒有權限</div>
      <p className="text-sm">
        這一頁需要「{PERM_LABEL[perm]}」權限（<code>{perm}</code>），請管理員在「{USERS_PAGE}」幫你換一個有這個權限的角色。
      </p>
    </div>
  );
}

/** Navigation: each page and the permission it needs (none = every signed-in user). */
export const NAV: { to: string; label: string; icon: typeof Ban; end?: boolean; perm?: Perm }[] = [
  { to: "/admin", label: "控制台", icon: LayoutDashboard, end: true, perm: "queue.view" },
  { to: "/admin/library", label: "歌曲庫", icon: Library, perm: "library.manage" },
  { to: "/admin/history", label: "點歌歷史", icon: History, perm: "history.view" },
  { to: "/admin/blocklist", label: "黑名單", icon: Ban, perm: "blocklist.manage" },
  { to: "/admin/commands", label: "聊天指令", icon: SquareTerminal, perm: "settings.view" },
  { to: "/admin/chat", label: "聊天室連線", icon: MessagesSquare, perm: "settings.view" },
  { to: "/admin/settings", label: "設定", icon: Settings, perm: "settings.view" },
  { to: "/admin/users", label: "使用者與權限", icon: Users, perm: "users.manage" },
  { to: "/admin/account", label: "我的帳號", icon: CircleUser },
];

/** Where an admin changes roles (named after its menu item). */
const USERS_PAGE = NAV.find((n) => n.perm === "users.manage")?.label ?? "使用者";

export function SessionProvider({ user, refresh, children }: Session & { children: ReactNode }) {
  return <SessionContext.Provider value={{ user, refresh }}>{children}</SessionContext.Provider>;
}

export function AdminLayout() {
  const [user, setUser] = useState<User | null | undefined>(undefined);
  const [version, setVersion] = useState(0);
  const navigate = useNavigate();
  useEffect(() => {
    get<AuthState>("/api/auth/state")
      .then((s) => {
        if (!s.user) navigate("/login", { replace: true });
        else setUser(s.user);
      })
      .catch(() => navigate("/login", { replace: true }));
  }, [navigate, version]);
  const refresh = useCallback(() => setVersion((v) => v + 1), []);

  if (!user) return <Loading />;
  const items = NAV.filter((n) => !n.perm || user.permissions.includes(n.perm));
  return (
    <SessionProvider user={user} refresh={refresh}>
      <div className="flex min-h-screen">
        <aside className="sticky top-0 flex h-screen w-56 shrink-0 flex-col border-r border-white/6 bg-ink-900/60 px-3 py-5">
          <div className="mb-6 flex items-center gap-2 px-2">
            <img src="/favicon.svg" alt="" className="size-8" />
            <div>
              <div className="text-sm font-semibold leading-tight">Streaming Player</div>
              <div className="text-xs text-zinc-500">點歌播放器</div>
            </div>
          </div>
          <nav className="flex flex-1 flex-col gap-0.5" aria-label="管理頁">
            {items.map(({ to, label, icon: Icon, end }) => (
              <NavLink
                key={to}
                to={to}
                end={end}
                className={({ isActive }) =>
                  `flex items-center gap-2.5 rounded-lg px-2.5 py-2 text-sm transition-colors ${
                    isActive ? "bg-gold-400/12 font-medium text-gold-300" : "text-zinc-400 hover:bg-white/6 hover:text-zinc-100"
                  }`
                }
              >
                <Icon className="size-4" />
                {label}
              </NavLink>
            ))}
          </nav>
          <div className="space-y-1 border-t border-white/6 pt-3 text-sm">
            <a href="/queue" target="_blank" className="flex items-center gap-2.5 rounded-lg px-2.5 py-1.5 text-zinc-400 hover:bg-white/6">
              <BookText className="size-4" />
              觀眾頁
            </a>
            <a href="/api/docs/" target="_blank" className="flex items-center gap-2.5 rounded-lg px-2.5 py-1.5 text-zinc-400 hover:bg-white/6">
              <BookText className="size-4" />
              API 文件
            </a>
            <button
              className="flex w-full items-center gap-2.5 rounded-lg px-2.5 py-1.5 text-zinc-400 hover:bg-white/6"
              onClick={async () => {
                await post("/api/auth/logout");
                navigate("/login");
              }}
            >
              <LogOut className="size-4" />
              登出 {user.username}
            </button>
            <div className="px-2.5 text-xs text-zinc-600">角色：{user.role_label}</div>
          </div>
        </aside>
        <main className="min-w-0 flex-1 px-8 py-7">
          <Outlet />
        </main>
      </div>
    </SessionProvider>
  );
}
