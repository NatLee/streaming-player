import {
  Ban,
  BookText,
  History,
  LayoutDashboard,
  Library,
  LogOut,
  MessagesSquare,
  Settings,
  SquareTerminal,
  Users,
} from "lucide-react";
import { createContext, useContext, useEffect, useState } from "react";
import { NavLink, Outlet, useNavigate } from "react-router";
import { get, post } from "../api";
import type { AuthState, User } from "../types";
import { Loading } from "./ui";

const UserContext = createContext<User | null>(null);
export const useUser = () => useContext(UserContext);

const NAV = [
  { to: "/admin", label: "控制台", icon: LayoutDashboard, end: true },
  { to: "/admin/library", label: "歌曲庫", icon: Library },
  { to: "/admin/history", label: "點歌歷史", icon: History },
  { to: "/admin/blocklist", label: "黑名單", icon: Ban },
  { to: "/admin/commands", label: "聊天指令", icon: SquareTerminal, admin: true },
  { to: "/admin/chat", label: "聊天室連線", icon: MessagesSquare, admin: true },
  { to: "/admin/settings", label: "設定", icon: Settings, admin: true },
  { to: "/admin/users", label: "使用者", icon: Users, admin: true },
];

export function AdminLayout() {
  const [user, setUser] = useState<User | null | undefined>(undefined);
  const navigate = useNavigate();
  useEffect(() => {
    get<AuthState>("/api/auth/state")
      .then((s) => {
        if (!s.user) navigate("/login", { replace: true });
        else setUser(s.user);
      })
      .catch(() => navigate("/login", { replace: true }));
  }, [navigate]);

  if (!user) return <Loading />;
  const items = NAV.filter((n) => !n.admin || user.role === "admin");
  return (
    <UserContext.Provider value={user}>
      <div className="flex min-h-screen">
        <aside className="sticky top-0 flex h-screen w-56 shrink-0 flex-col border-r border-white/6 bg-ink-900/60 px-3 py-5">
          <div className="mb-6 flex items-center gap-2 px-2">
            <img src="/favicon.svg" alt="" className="size-8" />
            <div>
              <div className="text-sm font-semibold leading-tight">Streaming Player</div>
              <div className="text-xs text-zinc-500">點歌播放器</div>
            </div>
          </div>
          <nav className="flex flex-1 flex-col gap-0.5">
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
          </div>
        </aside>
        <main className="min-w-0 flex-1 px-8 py-7">
          <Outlet />
        </main>
      </div>
    </UserContext.Provider>
  );
}
