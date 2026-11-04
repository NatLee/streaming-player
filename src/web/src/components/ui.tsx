import { AlertTriangle, Check, Copy, Eye, EyeOff, Loader2 } from "lucide-react";
import { useState, type ReactNode } from "react";

export function PageHeader({
  title,
  description,
  actions,
}: {
  title: string;
  description?: ReactNode;
  actions?: ReactNode;
}) {
  return (
    <div className="mb-6 flex flex-wrap items-end justify-between gap-3">
      <div className="min-w-0">
        <h1 className="text-2xl font-semibold tracking-tight">{title}</h1>
        {description && <p className="mt-1 max-w-3xl text-sm text-zinc-400">{description}</p>}
      </div>
      {actions && <div className="flex flex-wrap items-center gap-2">{actions}</div>}
    </div>
  );
}

export function Section({
  title,
  description,
  actions,
  children,
}: {
  title: string;
  description?: ReactNode;
  actions?: ReactNode;
  children: ReactNode;
}) {
  return (
    <section className="card p-5">
      <div className="mb-4 flex flex-wrap items-start justify-between gap-2">
        <div>
          <h2 className="font-semibold">{title}</h2>
          {description && <p className="mt-0.5 text-sm text-zinc-400">{description}</p>}
        </div>
        {actions}
      </div>
      {children}
    </section>
  );
}

export function ErrorBox({ message }: { message: string }) {
  return (
    <div className="flex items-start gap-2 rounded-lg border border-red-900/70 bg-red-950/50 px-3 py-2 text-sm text-red-200">
      <AlertTriangle className="mt-0.5 size-4 shrink-0" />
      <div className="min-w-0 flex-1 break-words">{message}</div>
    </div>
  );
}

export function Notice({ children, tone = "info" }: { children: ReactNode; tone?: "info" | "ok" | "warn" }) {
  const cls = {
    info: "border-sky-900/60 bg-sky-950/40 text-sky-200",
    ok: "border-emerald-900/60 bg-emerald-950/40 text-emerald-200",
    warn: "border-amber-900/60 bg-amber-950/40 text-amber-200",
  }[tone];
  return <div className={`rounded-lg border px-3 py-2 text-sm ${cls}`}>{children}</div>;
}

export function Loading({ label = "載入中…" }: { label?: string }) {
  return (
    <div className="flex items-center gap-2 py-8 text-sm text-zinc-400">
      <Loader2 className="size-4 animate-spin" />
      {label}
    </div>
  );
}

export function Empty({ children }: { children: ReactNode }) {
  return (
    <div className="rounded-xl border border-dashed border-white/10 px-4 py-10 text-center text-sm text-zinc-500">
      {children}
    </div>
  );
}

export function Toggle({
  checked,
  onChange,
  label,
  hint,
  disabled,
}: {
  checked: boolean;
  onChange: (v: boolean) => void;
  label: ReactNode;
  hint?: ReactNode;
  disabled?: boolean;
}) {
  return (
    <label className={`flex items-start gap-3 ${disabled ? "opacity-50" : "cursor-pointer"}`}>
      <button
        type="button"
        role="switch"
        aria-checked={checked}
        disabled={disabled}
        onClick={() => onChange(!checked)}
        className={`relative mt-0.5 h-5 w-9 shrink-0 rounded-full transition-colors ${checked ? "bg-gold-400" : "bg-ink-700"}`}
      >
        <span
          className={`absolute top-0.5 left-0.5 size-4 rounded-full bg-white shadow transition-transform ${checked ? "translate-x-4" : ""}`}
        />
      </button>
      <span>
        <span className="text-sm font-medium text-zinc-200">{label}</span>
        {hint && <span className="hint block">{hint}</span>}
      </span>
    </label>
  );
}

export function Field({ label, hint, children }: { label: ReactNode; hint?: ReactNode; children: ReactNode }) {
  return (
    <div>
      <span className="label">{label}</span>
      {children}
      {hint && <p className="hint">{hint}</p>}
    </div>
  );
}

export function NumberField({
  label,
  hint,
  value,
  onChange,
  min = 0,
  suffix,
}: {
  label: ReactNode;
  hint?: ReactNode;
  value: number;
  onChange: (v: number) => void;
  min?: number;
  suffix?: string;
}) {
  return (
    <Field label={label} hint={hint}>
      <div className="flex items-center gap-2">
        <input
          type="number"
          className="input w-28"
          min={min}
          value={Number.isFinite(value) ? value : 0}
          onChange={(e) => onChange(Math.max(min, Math.floor(Number(e.target.value) || 0)))}
        />
        {suffix && <span className="text-sm text-zinc-400">{suffix}</span>}
      </div>
    </Field>
  );
}

export function CopyField({ value, secret = false, label }: { value: string; secret?: boolean; label?: string }) {
  const [copied, setCopied] = useState(false);
  const [shown, setShown] = useState(!secret);
  return (
    <div className="flex items-center gap-1.5">
      <input
        readOnly
        aria-label={label}
        className="input font-mono text-xs"
        type={shown ? "text" : "password"}
        value={value}
        onFocus={(e) => e.currentTarget.select()}
      />
      {secret && (
        <button type="button" className="icon-btn" title={shown ? "隱藏" : "顯示"} onClick={() => setShown(!shown)}>
          {shown ? <EyeOff className="size-4" /> : <Eye className="size-4" />}
        </button>
      )}
      <button
        type="button"
        className="icon-btn"
        title="複製"
        onClick={() => {
          void navigator.clipboard?.writeText(value);
          setCopied(true);
          window.setTimeout(() => setCopied(false), 1500);
        }}
      >
        {copied ? <Check className="size-4 text-emerald-400" /> : <Copy className="size-4" />}
      </button>
    </div>
  );
}

const DOT: Record<string, string> = {
  connected: "bg-emerald-400",
  connecting: "bg-sky-400 animate-pulse",
  waiting: "bg-amber-400",
  needs_auth: "bg-amber-400",
  error: "bg-red-500",
  off: "bg-zinc-600",
};

const STATE_LABEL: Record<string, string> = {
  connected: "已連線",
  connecting: "連線中",
  waiting: "等待中",
  needs_auth: "需要設定",
  error: "錯誤",
  off: "關閉",
};

export function StatusPill({ state, detail }: { state: string; detail?: string }) {
  return (
    <span className="inline-flex items-center gap-2 rounded-full bg-white/6 px-2.5 py-1 text-xs text-zinc-300" title={detail}>
      <span className={`size-2 rounded-full ${DOT[state] ?? DOT.off}`} />
      {STATE_LABEL[state] ?? state}
    </span>
  );
}

/** A button that asks once more before doing something destructive. */
export function ConfirmButton({
  onConfirm,
  children,
  confirmLabel = "確定？",
  className = "btn btn-secondary",
  title,
}: {
  onConfirm: () => void;
  children: ReactNode;
  confirmLabel?: string;
  className?: string;
  title?: string;
}) {
  const [armed, setArmed] = useState(false);
  return (
    <button
      type="button"
      title={title}
      className={armed ? "btn btn-danger" : className}
      onBlur={() => setArmed(false)}
      onClick={() => {
        if (armed) {
          setArmed(false);
          onConfirm();
        } else {
          setArmed(true);
        }
      }}
    >
      {armed ? confirmLabel : children}
    </button>
  );
}

export function Thumb({ src, className = "h-12 w-20" }: { src: string | null | undefined; className?: string }) {
  return src ? (
    <img src={src} alt="" loading="lazy" referrerPolicy="no-referrer" className={`${className} shrink-0 rounded-md bg-ink-800 object-cover`} />
  ) : (
    <div className={`${className} shrink-0 rounded-md bg-ink-800`} />
  );
}

export function Equalizer({ active = true }: { active?: boolean }) {
  return (
    <span className="inline-flex h-3.5 items-end gap-[2px]" aria-hidden>
      {[0, 0.2, 0.4].map((d) => (
        <span
          key={d}
          className={`w-[3px] rounded-sm bg-gold-400 ${active ? "eq-bar" : ""}`}
          style={{ height: "100%", animationDelay: `${d}s` }}
        />
      ))}
    </span>
  );
}
