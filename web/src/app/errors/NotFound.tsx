import { Link, useRouter, useRouterState } from "@tanstack/react-router";
import { useState } from "react";

import { CipheraLogo, Icon } from "@/design-system";
import { NAV } from "@/app/layout/nav";

// Pages proposees : les entrees disponibles de la navigation.
const PAGES = NAV.flatMap((g) => g.items).flatMap((it) => (it.to ? [{ icon: it.icon, label: it.label, to: it.to }] : []));

/** Portage de prototypes/dashboard/404.html (version serveur). */
export function NotFound() {
  const router = useRouter();
  const path = useRouterState({ select: (s) => s.location.pathname });
  const [q, setQ] = useState("");
  const results = PAGES.filter((p) => p.label.toLowerCase().includes(q.toLowerCase()));
  return (
    <div style={{ minHeight: "100vh", display: "flex", flexDirection: "column", background: "var(--surface-page)" }}>
      <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", padding: "24px 28px" }}>
        <CipheraLogo height={18} color="var(--text-strong)" />
        <Link to="/" style={{ display: "flex", alignItems: "center", gap: 7, height: 36, padding: "0 14px", borderRadius: 9, border: "1px solid var(--border-default)", font: "500 13px/1 var(--font-sans)", color: "var(--text-strong)", textDecoration: "none" }}>
          <Icon name="ArrowLeft" size={14} />
          Retour au dashboard
        </Link>
      </div>
      <div style={{ flex: 1, position: "relative", display: "flex", alignItems: "flex-start", justifyContent: "center", padding: "20px 20px 60px" }}>
        <span aria-hidden="true" style={{ position: "fixed", inset: 0, pointerEvents: "none", backgroundImage: "radial-gradient(rgba(255,255,255,.08) 1px,transparent 1px)", backgroundSize: "18px 18px", maskImage: "radial-gradient(45% 50% at 50% 45%,#000,transparent)", WebkitMaskImage: "radial-gradient(45% 50% at 50% 45%,#000,transparent)" }} />
        <div style={{ position: "relative", margin: "auto 0", width: "100%", maxWidth: 460, display: "flex", flexDirection: "column", alignItems: "center", textAlign: "center", gap: 18 }}>
          <span aria-hidden="true" style={{ font: "600 96px/1 var(--font-sans)", letterSpacing: "-0.06em", color: "transparent", WebkitTextStroke: "1px rgba(255,255,255,.22)", userSelect: "none" }}>404</span>
          <h1 style={{ margin: 0, font: "600 24px/1.2 var(--font-sans)", color: "var(--text-strong)", letterSpacing: "-0.01em" }}>Page introuvable</h1>
          <p style={{ margin: 0, font: "400 14px/1.55 var(--font-sans)", color: "var(--text-muted)", maxWidth: 380 }}>Cette page n'existe pas ou a été déplacée. Vérifiez l'adresse, ou rejoignez une autre section du dashboard.</p>
          <span style={{ display: "flex", alignItems: "center", gap: 8, height: 30, padding: "0 12px", borderRadius: 8, background: "var(--surface-raised)", border: "1px solid var(--border-subtle)", font: "400 12px/1 var(--font-mono)", color: "var(--text-subtle)", maxWidth: "100%", overflow: "hidden", whiteSpace: "nowrap", textOverflow: "ellipsis" }}>
            <Icon name="Link2" size={12} />
            {path}
          </span>
          <div style={{ width: "100%", marginTop: 10, borderRadius: 16, border: "1px solid var(--border-default)", background: "var(--surface-app)", overflow: "hidden", textAlign: "left" }}>
            <div style={{ display: "flex", alignItems: "center", gap: 10, padding: "0 14px", height: 46, borderBottom: "1px solid var(--border-subtle)" }}>
              <Icon name="Search" size={15} color="var(--text-subtle)" />
              <input
                aria-label="Aller à une page"
                value={q}
                onChange={(e) => setQ(e.target.value)}
                placeholder="Aller à une page…"
                style={{ flex: 1, minWidth: 0, background: "transparent", border: 0, outline: "none", color: "var(--text-strong)", font: "400 14px/1 var(--font-sans)" }}
              />
            </div>
            {results.map((p) => (
              <button
                key={p.label}
                type="button"
                onClick={() => void router.navigate({ to: p.to })}
                className="hover:bg-[var(--surface-raised)]"
                style={{ display: "flex", alignItems: "center", gap: 12, width: "100%", padding: "11px 14px", border: 0, background: "transparent", cursor: "pointer", textAlign: "left" }}
              >
                <span style={{ width: 30, height: 30, borderRadius: 8, display: "flex", alignItems: "center", justifyContent: "center", background: "var(--surface-selected)" }}>
                  <Icon name={p.icon} size={14} color="var(--text-strong)" />
                </span>
                <span style={{ flex: 1, font: "400 13px/1 var(--font-sans)", color: "var(--text-strong)" }}>{p.label}</span>
                <Icon name="ArrowRight" size={13} color="var(--text-subtle)" />
              </button>
            ))}
            {!results.length && <div style={{ padding: "18px 14px", font: "400 13px/1 var(--font-sans)", color: "var(--text-subtle)" }}>Aucune page ne correspond.</div>}
          </div>
        </div>
      </div>
    </div>
  );
}
