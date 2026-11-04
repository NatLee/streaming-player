import { render, screen, within } from "@testing-library/react";
import { MemoryRouter, Route, Routes } from "react-router";
import { afterEach, describe, expect, it, vi } from "vitest";
import { AdminLayout, Require } from "../components/AdminLayout";
import { Register } from "../pages/Login";
import { UsersPage } from "../pages/Users";
import type { Perm, User } from "../types";

function user(permissions: Perm[], over: Partial<User> = {}): User {
  return {
    id: 2,
    username: "fan",
    role: "viewer",
    role_label: "觀眾",
    email: null,
    has_password: true,
    google_linked: false,
    disabled: false,
    created_at: 1_700_000_000,
    last_login_at: null,
    permissions,
    ...over,
  };
}

function mockApi(routes: Record<string, unknown>) {
  vi.stubGlobal(
    "fetch",
    vi.fn(async (path: string) => {
      const body = routes[path.split("?")[0]];
      return body === undefined ? new Response("{}", { status: 404 }) : new Response(JSON.stringify(body));
    }),
  );
}

afterEach(() => vi.unstubAllGlobals());

function renderAdmin(u: User, page = <p>page</p>, perm: Perm = "queue.view") {
  mockApi({ "/api/auth/state": { needs_setup: false, user: u, google_enabled: false, registration_open: true } });
  render(
    <MemoryRouter initialEntries={["/admin"]}>
      <Routes>
        <Route path="/admin" element={<AdminLayout />}>
          <Route index element={<Require perm={perm}>{page}</Require>} />
        </Route>
      </Routes>
    </MemoryRouter>,
  );
}

describe("permission-aware admin layout", () => {
  it("shows a viewer only the pages they may open", async () => {
    renderAdmin(user(["queue.view", "queue.request"]));
    const nav = await screen.findByRole("navigation", { name: "管理頁" });
    expect(within(nav).getByText("控制台")).toBeInTheDocument();
    expect(within(nav).getByText("我的帳號")).toBeInTheDocument();
    expect(within(nav).queryByText("使用者與權限")).toBeNull();
    expect(within(nav).queryByText("設定")).toBeNull();
    expect(screen.getByText("page")).toBeInTheDocument();
    expect(screen.getByText("角色：觀眾")).toBeInTheDocument();
  });

  it("explains a missing permission instead of showing the page", async () => {
    renderAdmin(user(["queue.view"]), <p>secret</p>, "users.manage");
    expect(await screen.findByText("沒有權限")).toBeInTheDocument();
    expect(screen.queryByText("secret")).toBeNull();
    // Names the permission and the page (its menu label) where an admin can grant it.
    const why = screen.getByText(/這一頁需要/).textContent ?? "";
    expect(why).toContain("管理使用者");
    expect(why).toContain("使用者與權限");
  });
});

describe("register page", () => {
  it("says when registration is closed", async () => {
    mockApi({ "/api/auth/state": { needs_setup: false, user: null, google_enabled: false, registration_open: false } });
    render(
      <MemoryRouter>
        <Register />
      </MemoryRouter>,
    );
    expect(await screen.findByText(/目前沒有開放註冊/)).toBeInTheDocument();
  });

  it("asks for the password twice when open", async () => {
    mockApi({ "/api/auth/state": { needs_setup: false, user: null, google_enabled: false, registration_open: true } });
    render(
      <MemoryRouter>
        <Register />
      </MemoryRouter>,
    );
    expect(await screen.findByRole("button", { name: "註冊並登入" })).toBeInTheDocument();
    expect(screen.getByLabelText("再輸入一次密碼")).toBeInTheDocument();
  });
});

describe("users page", () => {
  it("lists accounts, roles with their permissions, and the audit log", async () => {
    const admin = user(["users.manage"], { id: 1, username: "admin", role: "admin", role_label: "管理員" });
    mockApi({
      "/api/auth/state": { needs_setup: false, user: admin, google_enabled: false, registration_open: true },
      "/api/users": [admin, user(["queue.view"], { last_login_at: 1_700_000_100 })],
      "/api/roles": {
        default_role: "viewer",
        permissions: [
          { id: "queue.view", label: "看佇列和播放狀態" },
          { id: "users.manage", label: "管理使用者、角色和註冊" },
        ],
        roles: [
          { name: "admin", label: "管理員", permissions: ["queue.view", "users.manage"], builtin: true, locked: true, users: 1 },
          { name: "viewer", label: "觀眾", permissions: ["queue.view"], builtin: true, locked: false, users: 1 },
        ],
      },
      "/api/users/audit": [{ id: 1, at: 1_700_000_200, actor: "admin", kind: "user", detail: "fan：角色改成 viewer" }],
    });
    render(
      <MemoryRouter initialEntries={["/admin"]}>
        <Routes>
          <Route path="/admin" element={<AdminLayout />}>
            <Route index element={<UsersPage />} />
          </Route>
        </Routes>
      </MemoryRouter>,
    );
    expect(await screen.findByText("fan：角色改成 viewer")).toBeInTheDocument();
    expect(screen.getByLabelText("fan 的角色")).toHaveValue("viewer");
    expect(screen.getByText("這個角色固定擁有全部權限，不能修改。")).toBeInTheDocument();
    // The viewer role's editor shows its permission ticked; the admin can't delete themselves.
    expect(screen.getAllByLabelText("看佇列和播放狀態").some((c) => (c as HTMLInputElement).checked)).toBe(true);
    expect(screen.getAllByText("你")).toHaveLength(1);
  });
});
