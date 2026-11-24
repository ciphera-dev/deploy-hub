import { useState, type CSSProperties } from "react";

import { Icon, Menu } from "@/design-system";

import { rgb, type Connector } from "./catalog";

// Connectors.jsx#Card et #Mark. Seule la carte GitHub est active en v0.1 ; les
// autres affichent la version qui les apporte a la place de « Connecter ».
// Le volume d'evenements 24 h de la maquette n'a pas de source en v0.1.

const lbl: CSSProperties = { font: "400 12px/1 var(--font-sans)", color: "var(--text-subtle)" };
const line = "1px solid rgba(255,255,255,.06)";

export function Mark({ c, size = 40 }: { c: Connector; size?: number }) {
  return (
    <span style={{ width: size, height: size, borderRadius: size * 0.28, flex: "none", display: "flex", alignItems: "center", justifyContent: "center", background: "linear-gradient(145deg,rgba(" + rgb(c.col) + ",.22),rgba(" + rgb(c.col) + ",.06))", border: "1px solid rgba(" + rgb(c.col) + ",.28)", boxShadow: "inset 0 1px 0 rgba(255,255,255,.08)" }}>
      {c.img ? <img src={c.img} alt="" style={{ width: size * 0.58, height: size * 0.58, objectFit: "contain" }} /> : c.ic && <Icon name={c.ic} size={size * 0.46} color={c.col} />}
    </span>
  );
}

const POP = new Set(["github", "cloudflare", "openai", "claude"]);

export interface ConnectorCardProps {
  c: Connector;
  connected: boolean;
  onConnect?: () => void;
  onConfig?: () => void;
  onDisconnect?: () => void;
}

export function ConnectorCard({ c, connected, onConnect, onConfig, onDisconnect }: ConnectorCardProps) {
  const [h, setH] = useState(false);
  const C = rgb(c.col);
  return (
    <div
      data-testid={"connector-" + c.id}
      onMouseEnter={() => setH(true)}
      onMouseLeave={() => setH(false)}
      style={{ position: "relative", display: "flex", flexDirection: "column", borderRadius: 16, overflow: "hidden", isolation: "isolate", border: "1px solid " + (h ? "rgba(" + C + ",.32)" : "rgba(255,255,255,.08)"), background: connected ? "linear-gradient(115deg,#0f0f12 40%,rgba(" + C + ",.14))" : "#0e0e11", transform: h ? "translateY(-2px)" : "none", boxShadow: h ? "0 16px 36px -18px rgba(" + C + ",.55)" : "none", transition: "all .25s", opacity: c.soon ? 0.75 : 1 }}
    >
      <div style={{ display: "flex", alignItems: "flex-start", gap: 12, padding: "16px 16px 0" }}>
        <Mark c={c} size={40} />
        <div style={{ display: "flex", flexDirection: "column", gap: 6, flex: 1, minWidth: 0, paddingTop: 2 }}>
          <span style={{ font: "600 14px/1 var(--font-sans)", color: "var(--text-strong)" }}>{c.n}</span>
          <span style={{ ...lbl, fontSize: 11 }}>{c.org || c.cat}</span>
        </div>
        {connected ? (
          <Menu
            items={[
              { icon: "Settings2", label: "Configurer", onSelect: () => onConfig?.() },
              "separator",
              { icon: "Unplug", label: "Déconnecter", onSelect: () => onDisconnect?.(), danger: true },
            ]}
          />
        ) : c.soon ? (
          <span style={{ height: 20, display: "flex", alignItems: "center", padding: "0 8px", borderRadius: 99, background: "rgba(255,255,255,.06)", border: "1px solid rgba(255,255,255,.1)", font: "500 10px/1 var(--font-sans)", color: "var(--text-muted)" }}>{c.soon}</span>
        ) : (
          POP.has(c.id) && <span style={{ height: 20, display: "flex", alignItems: "center", padding: "0 8px", borderRadius: 99, background: "rgba(167,139,250,.12)", border: "1px solid rgba(167,139,250,.25)", font: "500 10px/1 var(--font-sans)", color: "#c4b5fd" }}>Populaire</span>
        )}
      </div>
      <span style={{ padding: "12px 16px 0", font: "400 12px/1.55 var(--font-sans)", color: "var(--text-muted)", textWrap: "pretty", minHeight: 38 }}>{c.d}</span>
      <div style={{ display: "flex", flexDirection: "column", gap: 8, padding: "14px 16px 16px" }}>
        <span style={{ ...lbl, fontSize: 11 }}>Fonctionnalités</span>
        <div style={{ display: "flex", gap: 5, flexWrap: "wrap" }}>
          {c.scopes.map((x) => (
            <span key={x} style={{ display: "flex", alignItems: "center", gap: 5, height: 22, padding: "0 8px", borderRadius: 7, background: "rgba(255,255,255,.045)", border: "1px solid rgba(255,255,255,.06)", font: "500 11px/1 var(--font-sans)", color: "var(--text-body)" }}>
              <Icon name="Check" size={10} color={c.col} />
              {x}
            </span>
          ))}
        </div>
      </div>
      <div style={{ display: "flex", alignItems: "center", gap: 10, padding: "12px 16px", marginTop: "auto", borderTop: line }}>
        <span style={{ display: "flex", flexDirection: "column", gap: 6, flex: 1, minWidth: 0 }}>
          <span style={{ ...lbl, fontSize: 10 }}>Connexion</span>
          <span style={{ display: "flex", alignItems: "center", gap: 6, font: "600 13px/1 var(--font-sans)", color: "var(--text-strong)" }}>
            {connected ? (
              <>
                <span style={{ width: 6, height: 6, borderRadius: 99, background: "#86efac", boxShadow: "0 0 6px rgba(134,239,172,.7)" }} />
                Connecté
              </>
            ) : c.key ? (
              "Clé d'API"
            ) : (
              "OAuth"
            )}
          </span>
        </span>
        {connected ? (
          <button type="button" onClick={onConfig} style={{ height: 34, padding: "0 14px", borderRadius: 10, cursor: "pointer", font: "500 12px/1 var(--font-sans)", border: "1px solid rgba(255,255,255,.12)", background: "rgba(255,255,255,.04)", color: "var(--text-strong)" }}>
            Configurer
          </button>
        ) : (
          <button
            type="button"
            onClick={onConnect}
            disabled={!!c.soon}
            title={c.soon ? "Disponible en " + c.soon : undefined}
            style={{ height: 34, padding: "0 16px", borderRadius: 10, cursor: c.soon ? "not-allowed" : "pointer", font: "600 12px/1 var(--font-sans)", border: 0, background: c.soon ? "rgba(255,255,255,.08)" : h ? "#fff" : "#ededed", color: c.soon ? "var(--text-faint)" : "#0b0b0b", transition: "background .2s" }}
          >
            Connecter
          </button>
        )}
      </div>
    </div>
  );
}
