import { render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router";
import { afterEach, describe, expect, it, vi } from "vitest";
import { botSnippets } from "../lib/bots";
import { Login } from "../pages/Login";
import { parseCsv } from "../pages/Library";
import { Viewer } from "../pages/Viewer";
import { youtubeId } from "../pages/Watch";

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

describe("Login", () => {
  it("offers to create the first admin on a fresh install", async () => {
    mockApi({ "/api/auth/state": { needs_setup: true, user: null, google_enabled: false } });
    render(
      <MemoryRouter>
        <Login />
      </MemoryRouter>,
    );
    expect(await screen.findByRole("button", { name: "建立管理員並登入" })).toBeInTheDocument();
    expect(screen.queryByText("使用 Google 登入")).toBeNull();
  });

  it("shows Google sign-in and its errors", async () => {
    mockApi({ "/api/auth/state": { needs_setup: false, user: null, google_enabled: true } });
    render(
      <MemoryRouter initialEntries={["/login?error=google_not_allowed"]}>
        <Login />
      </MemoryRouter>,
    );
    expect(await screen.findByRole("button", { name: "登入" })).toBeInTheDocument();
    expect(screen.getByText("使用 Google 登入")).toHaveAttribute("href", "/api/auth/google/start");
    expect(screen.getByText(/沒有權限/)).toBeInTheDocument();
  });
});

describe("Viewer page", () => {
  it("says when the streamer turned it off", async () => {
    mockApi({ "/api/meta": { version: "2.0.0", public_queue: false, google_enabled: false } });
    render(<Viewer />);
    expect(await screen.findByText("實況主沒有開放觀眾頁")).toBeInTheDocument();
  });
});

describe("helpers", () => {
  it("builds bot commands with an encoded trigger", () => {
    const s = botSnippets("https://p.example/api/bot", "k 1", "點歌");
    const nightbot = s.find((x) => x.bot === "Nightbot")!;
    expect(nightbot.text).toBe("$(urlfetch https://p.example/api/bot/%E9%BB%9E%E6%AD%8C?key=k%201&args=$(querystring))");
    expect(s.find((x) => x.bot === "StreamElements")!.text).toContain("user=$(queryescape $(sender))");
    expect(s.map((x) => x.bot)).toEqual(["Nightbot", "StreamElements", "Fossabot", "Streamer.bot"]);
  });

  it("reads YouTube ids like the server", () => {
    expect(youtubeId("https://youtu.be/JJo-zUi9E5U?si=1")).toBe("JJo-zUi9E5U");
    expect(youtubeId("https://www.youtube.com/watch?v=JJo-zUi9E5U")).toBe("JJo-zUi9E5U");
    expect(youtubeId("https://m.youtube.com/shorts/JJo-zUi9E5U")).toBe("JJo-zUi9E5U");
    expect(youtubeId("JJo-zUi9E5U")).toBe("JJo-zUi9E5U");
    expect(youtubeId("https://example.com/watch?v=x")).toBeNull();
  });

  it("parses the exported CSV", () => {
    const csv = 'url,title,duration_secs,favorite,blocked\nhttps://a,"Hello, ""world""",200,true,false\nhttps://b,x,,false,true\n';
    expect(parseCsv(csv)).toEqual([
      { url: "https://a", title: 'Hello, "world"', duration_secs: 200, favorite: true, blocked: false },
      { url: "https://b", title: "x", duration_secs: null, favorite: false, blocked: true },
    ]);
  });
});
