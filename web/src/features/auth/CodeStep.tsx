import { useEffect, useRef, useState, type ClipboardEvent, type KeyboardEvent } from "react";

import { Icon, type IconName } from "@/design-system";

import type { MfaMethods, SecondFactor } from "./api";
import { AltMethod, FormError, MethodsDivider, PrimaryButton } from "./ui";

export interface CodeStepProps {
  /** Verification du second facteur : renvoie le message a afficher en cas de refus. */
  onSubmit: (factor: SecondFactor) => Promise<string | undefined>;
  /** Facteurs du compte (`GET /api/auth/mfa/methods`) : seuls ceux-ci sont proposes. */
  methods?: MfaMethods;
  onBack: () => void;
}

const DEFAULT_METHODS: MfaMethods = { totp: true, webauthn: false, recovery: true };

type Mode = "totp" | "key" | "backup" | "none";

const EMPTY = ["", "", "", "", "", ""];

const secondsLeft = () => 30 - (Math.floor(Date.now() / 1000) % 30);

const HEAD: Record<Mode, { icon: IconName; title: string; text: string }> = {
  totp: { icon: "Smartphone", title: "Vérification en deux étapes", text: "Ouvrez votre application d'authentification et saisissez le code affiché." },
  key: { icon: "KeyRound", title: "Clé de sécurité", text: "Insérez votre clé ou utilisez la biométrie de votre appareil." },
  backup: { icon: "LifeBuoy", title: "Code de secours", text: "Saisissez l'un des codes de secours reçus lors de l'activation." },
  none: { icon: "KeyRound", title: "Clé de sécurité", text: "Votre compte se vérifie avec une clé de sécurité." },
};

/** Cle du compte inutilisable : panneau ouvert par son adresse IP (WebAuthn exige un nom de domaine). */
const IP_HINT = "Votre clé de sécurité ne peut pas être utilisée ici : ouvrez le panneau par son nom de domaine (et non par son adresse IP).";

function Hint({ children }: { children: string }) {
  return <p style={{ margin: 0, padding: "12px 14px", borderRadius: 12, border: "1px solid var(--border-default)", background: "var(--surface-raised)", font: "400 12px/1.5 var(--font-sans)", color: "var(--text-muted)" }}>{children}</p>;
}

/**
 * Etape de double authentification de LoginFlow, portee de 2fa.html (sans
 * « Simuler la validation » ni « appareil de confiance », non pris en charge
 * par le serveur). Ne propose que les facteurs du compte ; un compte sans
 * application d'authentification commence par sa cle de securite, ou, si
 * elle est inutilisable (panneau ouvert par une adresse IP), par ses codes de
 * secours avec une explication.
 */
export function CodeStep({ onSubmit, methods = DEFAULT_METHODS, onBack }: CodeStepProps) {
  const [digits, setDigits] = useState(EMPTY);
  const refs = useRef<(HTMLInputElement | null)[]>([]);
  // Compte sans application d'authentification : la cle est la methode principale.
  // Sans TOTP ni cle utilisable (compte a passkey, panneau joint par IP) :
  // codes de secours s'il en reste, sinon seulement l'explication.
  const keyUnusable = !methods.totp && !methods.webauthn;
  const home: Mode = methods.totp ? "totp" : methods.webauthn ? "key" : methods.recovery ? "backup" : "none";
  const [mode, setMode] = useState<Mode>(home);
  const [sec, setSec] = useState(secondsLeft);
  const [backup, setBackup] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  useEffect(() => {
    const t = setInterval(() => setSec(secondsLeft()), 1000);
    return () => clearInterval(t);
  }, []);

  const submit = async (factor: SecondFactor) => {
    if (busy) return;
    setError("");
    setBusy(true);
    const refused = await onSubmit(factor);
    setBusy(false);
    if (!refused) return;
    setError(refused);
    if (factor.kind === "totp") {
      setDigits(EMPTY);
      refs.current[0]?.focus();
    }
  };

  const submitTotp = (code: string) => void submit({ kind: "totp", code });

  const set = (i: number, c: string) => {
    if (!/^\d?$/.test(c)) return;
    const next = [...digits];
    next[i] = c;
    setDigits(next);
    setError("");
    if (c && i < 5) refs.current[i + 1]?.focus();
    if (next.every(Boolean)) submitTotp(next.join(""));
  };

  const onKeyDown = (i: number, c: string) => (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "Backspace" && !c && i > 0) refs.current[i - 1]?.focus();
    if (e.key === "ArrowLeft" && i > 0) refs.current[i - 1]?.focus();
    if (e.key === "ArrowRight" && i < 5) refs.current[i + 1]?.focus();
  };

  const onPaste = (e: ClipboardEvent<HTMLInputElement>) => {
    const t = e.clipboardData.getData("text").replace(/\D/g, "").slice(0, 6);
    if (t.length === 6) {
      e.preventDefault();
      setDigits(t.split(""));
      submitTotp(t);
    }
  };

  const R = 14;
  const C = 2 * Math.PI * R;
  const box = (c: string, i: number) => (
    <input
      key={i}
      ref={(el) => {
        refs.current[i] = el;
      }}
      aria-label={`Chiffre ${i + 1}`}
      autoFocus={i === 0}
      value={c}
      inputMode="numeric"
      autoComplete={i === 0 ? "one-time-code" : "off"}
      maxLength={1}
      onChange={(e) => set(i, e.target.value.slice(-1))}
      onKeyDown={onKeyDown(i, c)}
      onPaste={onPaste}
      onFocus={(e) => e.target.select()}
      style={{
        width: 0,
        flex: 1,
        height: 56,
        textAlign: "center",
        borderRadius: 12,
        border: "1px solid " + (error ? "rgba(248,113,113,.6)" : c ? "var(--border-strong)" : "var(--border-default)"),
        background: c ? "var(--surface-selected)" : "var(--surface-raised)",
        color: "var(--text-strong)",
        font: "600 24px/1 var(--font-mono)",
        outline: "none",
        caretColor: "#a78bfa",
        transition: "all .15s",
      }}
    />
  );

  const startKey = () => {
    setError("");
    setMode("key");
    void submit({ kind: "webauthn" });
  };

  const others = [
    mode !== "key" && methods.webauthn && <AltMethod key="key" icon="KeyRound" title="Clé de sécurité" text="Clé physique ou biométrie" onClick={startKey} />,
    methods.recovery && (
      <AltMethod
        key="backup"
        icon="LifeBuoy"
        title="Code de secours"
        text="Un des 10 codes de récupération"
        onClick={() => {
          setError("");
          setMode("backup");
        }}
      />
    ),
  ].filter(Boolean);
  const alternatives = others.length > 0 && (
    <>
      <MethodsDivider />
      <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>{others}</div>
    </>
  );

  const head = HEAD[mode];

  return (
    <div className="login-step" style={{ display: "flex", flexDirection: "column", gap: 20 }}>
      <div style={{ display: "flex", flexDirection: "column", alignItems: "center", textAlign: "center", gap: 14 }}>
        <span style={{ position: "relative", width: 60, height: 60 }}>
          <span style={{ position: "absolute", left: 8, right: 8, bottom: -8, height: 10, borderRadius: "50%", background: "rgba(0,0,0,.6)", filter: "blur(5px)" }} />
          <span style={{ position: "absolute", inset: 0, borderRadius: 16, transform: "rotate(-8deg)", background: "linear-gradient(145deg,#4a4a52,#26262c)", boxShadow: "inset 0 1px 0 rgba(255,255,255,.18),inset 0 -2px 0 rgba(0,0,0,.35)", display: "flex", alignItems: "center", justifyContent: "center" }}>
            <Icon name={head.icon} size={24} color="#f2f2f2" />
          </span>
        </span>
        <h1 style={{ margin: "4px 0 0", font: "600 22px/1.2 var(--font-sans)", color: "var(--text-strong)" }}>{head.title}</h1>
        <span style={{ font: "400 13px/1.5 var(--font-sans)", color: "var(--text-muted)", maxWidth: 320 }}>{head.text}</span>
      </div>

      {mode === "totp" && (
        <>
          <div className={error ? "login-shake" : undefined} style={{ display: "flex", alignItems: "center", gap: 8 }}>
            {digits.slice(0, 3).map((c, i) => box(c, i))}
            <span style={{ width: 10, height: 2, borderRadius: 2, background: "var(--border-strong)", flex: "none" }} />
            {digits.slice(3).map((c, i) => box(c, i + 3))}
          </div>
          <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", marginTop: -4 }}>
            {error ? <FormError>{error}</FormError> : <span style={{ font: "400 12px/1 var(--font-sans)", color: "var(--text-subtle)" }}>Collez le code ou saisissez-le</span>}
            <span style={{ display: "flex", alignItems: "center", gap: 7, font: "500 12px/1 var(--font-mono)", color: sec <= 5 ? "#e5b454" : "var(--text-muted)" }}>
              <svg width="18" height="18" viewBox="0 0 32 32" aria-hidden="true" style={{ transform: "rotate(-90deg)" }}>
                <circle cx="16" cy="16" r={R} fill="none" stroke="var(--surface-selected)" strokeWidth="3" />
                <circle cx="16" cy="16" r={R} fill="none" stroke={sec <= 5 ? "#e5b454" : "#ededed"} strokeWidth="3" strokeLinecap="round" strokeDasharray={C} strokeDashoffset={C * (1 - sec / 30)} style={{ transition: "stroke-dashoffset 1s linear" }} />
              </svg>
              {sec} s
            </span>
          </div>
          <PrimaryButton type="button" busy={busy} disabled={!digits.every(Boolean)} onClick={() => submitTotp(digits.join(""))}>
            Vérifier
          </PrimaryButton>
          {alternatives}
        </>
      )}

      {mode === "key" && (
        <>
          <div style={{ display: "flex", flexDirection: "column", alignItems: "center", gap: 12, padding: "26px 16px", borderRadius: 14, border: "1px dashed var(--border-strong)", background: "var(--surface-raised)" }}>
            <Icon name={busy ? "Loader" : "KeyRound"} size={20} color="var(--text-muted)" className={busy ? "login-spin" : undefined} />
            <span role={busy ? "status" : undefined} style={{ font: "400 13px/1 var(--font-sans)", color: "var(--text-muted)" }}>
              {busy ? "En attente de votre clé…" : "Votre navigateur vous demandera votre clé."}
            </span>
          </div>
          {error && <FormError>{error}</FormError>}
          <PrimaryButton type="button" busy={busy} onClick={startKey}>
            {error ? "Réessayer" : "Utiliser ma clé"}
          </PrimaryButton>
          {home === "key" && alternatives}
        </>
      )}

      {mode === "none" && <Hint>{IP_HINT}</Hint>}

      {mode === "backup" && (
        <>
          {keyUnusable && <Hint>{IP_HINT}</Hint>}
          <label style={{ display: "flex", flexDirection: "column", gap: 8 }}>
            <span style={{ font: "500 13px/1 var(--font-sans)", color: "var(--text-strong)" }}>Code de secours</span>
            <input
              className="login-field"
              autoFocus
              autoComplete="off"
              value={backup}
              onChange={(e) => {
                setBackup(e.target.value.toUpperCase());
                setError("");
              }}
              placeholder="XXXXX-XXXXX"
              style={{ height: 46, padding: "0 14px", borderRadius: 10, border: "1px solid var(--border-default)", background: "var(--surface-raised)", color: "var(--text-strong)", font: "500 16px/1 var(--font-mono)", letterSpacing: ".12em", outline: "none" }}
            />
          </label>
          {error && <FormError>{error}</FormError>}
          <PrimaryButton type="button" busy={busy} disabled={backup.replace(/\W/g, "").length < 10} onClick={() => void submit({ kind: "recovery", code: backup.trim() })}>
            Vérifier
          </PrimaryButton>
        </>
      )}

      <button
        type="button"
        onClick={
          mode === home
            ? onBack
            : () => {
                setError("");
                setMode(home);
              }
        }
        style={{ alignSelf: "center", padding: 0, border: 0, background: "transparent", cursor: "pointer", color: "var(--text-muted)", display: "flex", alignItems: "center", gap: 6, font: "400 13px/1 var(--font-sans)" }}
      >
        <Icon name="ArrowLeft" size={13} />
        {mode === home ? "Retour" : home === "totp" ? "Utiliser l'application d'authentification" : home === "key" ? "Utiliser ma clé de sécurité" : "Utiliser un code de secours"}
      </button>
    </div>
  );
}
