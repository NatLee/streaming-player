import { useEffect, useState } from "react";
import { Link, useNavigate, useSearchParams } from "react-router";
import { errorText, get, post } from "../api";
import { ErrorBox, Loading } from "../components/ui";
import type { AuthState } from "../types";

const GOOGLE_ERRORS: Record<string, string> = {
  google_disabled: "伺服器沒有設定 Google 登入",
  google_denied: "Google 登入被取消了",
  google_state: "登入逾時，請再試一次",
  google_failed: "Google 登入失敗，請再試一次",
  google_unverified: "這個 Google 帳號的 Email 還沒驗證",
  google_not_allowed: "這個 Google 帳號沒有權限，請管理員在「使用者」加入你的 Email",
  user_disabled: "這個帳號已停用",
  register_limited: "註冊太頻繁，請晚點再試",
};

/** Sign in, or create the first admin account on a fresh install. */
export function Login() {
  const [auth, setAuth] = useState<AuthState | null>(null);
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const navigate = useNavigate();
  const [params] = useSearchParams();

  useEffect(() => {
    get<AuthState>("/api/auth/state")
      .then((s) => {
        if (s.user) navigate("/admin", { replace: true });
        else setAuth(s);
      })
      .catch((e) => setError(errorText(e)));
  }, [navigate]);

  const googleError = params.get("error");
  if (!auth && !error) return <Loading />;
  const setup = auth?.needs_setup ?? false;

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      await post(setup ? "/api/auth/setup" : "/api/auth/login", { username, password });
      navigate("/admin", { replace: true });
    } catch (err) {
      setError(errorText(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex min-h-screen items-center justify-center px-4">
      <div className="w-full max-w-sm">
        <div className="mb-6 flex items-center gap-3">
          <img src="/favicon.svg" alt="" className="size-11" />
          <div>
            <h1 className="text-xl font-semibold">Streaming Player</h1>
            <p className="text-sm text-zinc-400">{setup ? "第一次使用：建立管理員帳號" : "登入管理頁"}</p>
          </div>
        </div>
        <form onSubmit={submit} className="card space-y-4 p-6">
          {setup && (
            <p className="text-sm text-zinc-400">
              這是新安裝的系統。這裡建立的帳號是管理員，之後可以在「使用者」加入其他管理員或協助管理的人。
            </p>
          )}
          <div>
            <label className="label" htmlFor="username">
              帳號
            </label>
            <input id="username" className="input" autoComplete="username" value={username} onChange={(e) => setUsername(e.target.value)} required />
          </div>
          <div>
            <label className="label" htmlFor="password">
              密碼
            </label>
            <input
              id="password"
              type="password"
              className="input"
              autoComplete={setup ? "new-password" : "current-password"}
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              minLength={setup ? 8 : undefined}
              required
            />
            {setup && <p className="hint">至少 8 個字元</p>}
          </div>
          {(error || googleError) && <ErrorBox message={error ?? GOOGLE_ERRORS[googleError!] ?? googleError!} />}
          <button className="btn btn-primary w-full py-2" disabled={busy}>
            {setup ? "建立管理員並登入" : "登入"}
          </button>
          {auth?.google_enabled && !setup && (
            <a href="/api/auth/google/start" className="btn btn-secondary w-full py-2">
              使用 Google 登入
            </a>
          )}
          {auth?.registration_open && !setup && (
            <p className="text-center text-sm text-zinc-400">
              還沒有帳號？
              <Link to="/register" className="ml-1 text-gold-300 underline">
                註冊
              </Link>
            </p>
          )}
        </form>
      </div>
    </div>
  );
}

/** `/register`: create an account with the default role (when registration is open). */
export function Register() {
  const [auth, setAuth] = useState<AuthState | null>(null);
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [again, setAgain] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const navigate = useNavigate();

  useEffect(() => {
    get<AuthState>("/api/auth/state")
      .then((s) => {
        if (s.user) navigate("/admin", { replace: true });
        else setAuth(s);
      })
      .catch((e) => setError(errorText(e)));
  }, [navigate]);
  if (!auth && !error) return <Loading />;

  return (
    <div className="flex min-h-screen items-center justify-center px-4">
      <div className="w-full max-w-sm">
        <div className="mb-6 flex items-center gap-3">
          <img src="/favicon.svg" alt="" className="size-11" />
          <div>
            <h1 className="text-xl font-semibold">Streaming Player</h1>
            <p className="text-sm text-zinc-400">註冊帳號</p>
          </div>
        </div>
        {auth && !auth.registration_open ? (
          <div className="card space-y-3 p-6 text-sm text-zinc-300">
            <p>目前沒有開放註冊，請找實況主或管理員幫你開帳號。</p>
            <Link to="/login" className="text-gold-300 underline">
              回到登入
            </Link>
          </div>
        ) : (
          <form
            className="card space-y-4 p-6"
            onSubmit={async (e) => {
              e.preventDefault();
              if (password !== again) {
                setError("兩次輸入的密碼不一樣");
                return;
              }
              setBusy(true);
              setError(null);
              try {
                await post("/api/auth/register", { username, password });
                navigate("/admin", { replace: true });
              } catch (err) {
                setError(errorText(err));
              } finally {
                setBusy(false);
              }
            }}
          >
            <div>
              <label className="label" htmlFor="reg-username">
                帳號
              </label>
              <input id="reg-username" className="input" autoComplete="username" value={username} onChange={(e) => setUsername(e.target.value)} required />
            </div>
            <div>
              <label className="label" htmlFor="reg-password">
                密碼
              </label>
              <input id="reg-password" type="password" className="input" autoComplete="new-password" minLength={8} value={password} onChange={(e) => setPassword(e.target.value)} required />
              <p className="hint">至少 8 個字元</p>
            </div>
            <div>
              <label className="label" htmlFor="reg-again">
                再輸入一次密碼
              </label>
              <input id="reg-again" type="password" className="input" autoComplete="new-password" value={again} onChange={(e) => setAgain(e.target.value)} required />
            </div>
            {error && <ErrorBox message={error} />}
            <button className="btn btn-primary w-full py-2" disabled={busy}>
              註冊並登入
            </button>
            <p className="text-center text-sm text-zinc-400">
              已經有帳號？
              <Link to="/login" className="ml-1 text-gold-300 underline">
                登入
              </Link>
            </p>
          </form>
        )}
      </div>
    </div>
  );
}
