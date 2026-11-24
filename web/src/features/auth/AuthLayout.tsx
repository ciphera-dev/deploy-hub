import type { ReactNode } from "react";

import { CipheraLogo, Icon } from "@/design-system";

/** Etape du panneau de gauche : numero, titre, sous-titre. */
export type AuthStep = [number, string, string];

export interface AuthLayoutProps {
  steps: AuthStep[];
  /** Numero de l'etape en cours (les precedentes sont cochees). */
  current: number;
  /** Phrase sous le titre ; l'hote du panneau est ajoute a la fin. */
  subtitle?: string;
  /** Nom accessible de la liste des etapes. */
  stepsLabel?: string;
  children: ReactNode;
}

/** Mise en page des ecrans d'authentification (login.html, register.html) : panneau d'etapes a gauche, formulaire a droite. */
export function AuthLayout({ steps, current, subtitle = "Connectez-vous en trois étapes pour accéder à", stepsLabel = "Étapes de connexion", children }: AuthLayoutProps) {
  return (
    <div style={{ height: "100vh", display: "grid", gridTemplateColumns: "minmax(0,1.05fr) minmax(0,1fr)", padding: 12, boxSizing: "border-box", background: "#070708" }}>
      <div style={{ position: "relative", borderRadius: 24, overflow: "hidden", display: "flex", flexDirection: "column", padding: "34px 38px", border: "1px solid var(--border-default)", background: "radial-gradient(70% 55% at 30% 25%,rgba(255,255,255,.07),transparent 70%),linear-gradient(170deg,#18181c,#0d0d10 70%)" }}>
        <span style={{ position: "absolute", inset: 0, backgroundImage: "radial-gradient(rgba(255,255,255,.09) 1px,transparent 1px)", backgroundSize: "18px 18px", maskImage: "radial-gradient(80% 60% at 50% 35%,#000,transparent)", WebkitMaskImage: "radial-gradient(80% 60% at 50% 35%,#000,transparent)", opacity: 0.8 }} />
        <CipheraLogo height={18} color="var(--text-strong)" style={{ position: "relative", alignSelf: "flex-start" }} />
        <div style={{ flex: 1 }} />
        <div style={{ position: "relative", display: "flex", flexDirection: "column", gap: 18 }}>
          <span style={{ width: "fit-content", display: "flex", alignItems: "center", gap: 8, height: 32, padding: "0 14px", borderRadius: 99, background: "var(--surface-raised)", border: "1px solid var(--border-default)", font: "500 13px/1 var(--font-sans)", color: "var(--text-body)" }}>
            <span style={{ width: 6, height: 6, borderRadius: 99, background: "var(--green-500)" }} />
            Instance auto-hébergée
          </span>
          <span style={{ font: "600 40px/1.08 var(--font-sans)", color: "var(--text-strong)", letterSpacing: "-0.03em", maxWidth: 460 }}>Votre infrastructure, sur vos serveurs</span>
          <span style={{ font: "400 15px/1.5 var(--font-sans)", color: "var(--text-muted)", maxWidth: 440 }}>
            {subtitle} {window.location.host}.
          </span>
          <ol aria-label={stepsLabel} style={{ listStyle: "none", margin: "14px 0 0", padding: 0, display: "grid", gridTemplateColumns: "repeat(3,minmax(0,1fr))", gap: 12 }}>
            {steps.map(([n, title, text]) => {
              const on = n === current;
              const done = n < current;
              return (
                <li key={n} aria-current={on ? "step" : undefined} style={{ display: "flex", flexDirection: "column", gap: 28, padding: "16px 16px 18px", minHeight: 118, boxSizing: "border-box", borderRadius: 16, background: on ? "var(--surface-raised)" : "transparent", border: "1px solid " + (on ? "var(--border-strong)" : "var(--border-subtle)"), transition: "all .3s" }}>
                  <span style={{ width: 26, height: 26, borderRadius: 99, display: "flex", alignItems: "center", justifyContent: "center", background: on ? "#ededed" : done ? "var(--green-500)" : "transparent", border: on || done ? "none" : "1px solid var(--border-strong)", font: "600 12px/1 var(--font-sans)", color: on || done ? "#0b0b0b" : "var(--text-muted)" }}>
                    {done ? <Icon name="Check" size={13} color="#0b0b0b" /> : n}
                  </span>
                  <span style={{ display: "flex", flexDirection: "column", gap: 5 }}>
                    <span style={{ font: "600 13px/1.25 var(--font-sans)", color: on ? "var(--text-strong)" : "var(--text-body)" }}>{title}</span>
                    <span style={{ font: "400 12px/1.3 var(--font-sans)", color: "var(--text-subtle)" }}>{text}</span>
                  </span>
                </li>
              );
            })}
          </ol>
        </div>
      </div>

      <div style={{ display: "flex", flexDirection: "column", minHeight: 0, overflow: "auto" }}>
        <div style={{ flex: 1, display: "flex", alignItems: "center", justifyContent: "center", padding: "20px 20px 40px" }}>
          <div style={{ width: "100%", maxWidth: 356, display: "flex", flexDirection: "column", gap: 18 }}>{children}</div>
        </div>
      </div>
    </div>
  );
}

/** Textes du panneau de gauche pour une creation de compte (invitation, inscription). */
export const SIGNUP_COPY = { subtitle: "Créez votre compte en trois étapes pour accéder à", stepsLabel: "Étapes de l'inscription" } as const;

/** Etapes d'une creation de compte (invitation, inscription) : le compte cree, la connexion suit. */
export const SIGNUP_STEPS: AuthStep[] = [
  [1, "Inscription", "Vos identifiants"],
  [2, "Vérification", "Confirmez votre identité"],
  [3, "Accès", "Bienvenue sur Ciphera"],
];
