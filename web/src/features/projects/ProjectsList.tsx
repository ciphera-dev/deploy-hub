import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useState, type CSSProperties } from "react";

import cloudflareMark from "@/assets/images/cloudflare/cloudflare-mark.png";
import githubMark from "@/assets/images/connectors/github-mark.png";
import supabaseMark from "@/assets/images/connectors/supabase-mark.png";
import { Button, Icon, Menu, Tabs, type IconName } from "@/design-system";

import { fetchProjects, PROJECTS_QUERY_KEY, type ProjectSummary } from "./api";
import { branchOf, buildLabel, sourceLabel, STATUS_VIEW, timeAgo } from "./format";
import { NewProjectModal } from "./NewProjectModal";

// Pages2.jsx#ProjectsListV2. Differences avec la maquette, faute de donnees en
// v0.1 : pas de colonne Domaine (aucun domaine tant que les domaines ne sont
// pas branches), « Mis à jour » a la place de « Déployé », pas de serveur ;
// onglets Preview et Staging retires (un seul environnement : production) ;
// actions de menu sans endpoint desactivees.

const lbl: CSSProperties = { font: "400 12px/1 var(--font-sans)", color: "var(--text-subtle)" };
const hair = "1px solid var(--border-subtle)";
const card: CSSProperties = { borderRadius: "var(--radius-xl)", border: "1px solid var(--border-default)", background: "var(--surface-app)" };
const COLS = "28px minmax(0,1.6fr) minmax(0,1fr) 120px 136px";

const mono = (v: string, color = "var(--text-body)", size = 12) => <span style={{ font: `400 ${size}px/1 var(--font-mono)`, color, whiteSpace: "nowrap" }}>{v}</span>;

const FW: Record<string, IconName> = { railpack: "Package", dockerfile: "FileCode", compose: "Boxes" };
const ST_ICON = {
  none: ["CircleDashed", "var(--text-subtle)"],
  building: ["CircleExclamationPoint", "#e5b454"],
  running: ["CircleCheck", "var(--green-500)"],
  failed: ["CircleX", "#f87171"],
  stopped: ["CircleMinus", "var(--text-subtle)"],
  unreachable: ["Unplug", "#e5b454"],
  down: ["CircleX", "#f87171"],
} as const;

type Filter = "all" | "Production" | "err";
type SortKey = "name" | "status" | "updated_at";

export function ProjectsList({ open }: { open: (id: string) => void }) {
  const queryClient = useQueryClient();
  const projects = useQuery({ queryKey: PROJECTS_QUERY_KEY, queryFn: fetchProjects });
  const [tab, setTab] = useState<Filter>("all");
  const [expanded, setExpanded] = useState<string | null>(null);
  const [hover, setHover] = useState<string | null>(null);
  const [sort, setSort] = useState<[SortKey, 1 | -1]>(["name", 1]);
  const [creating, setCreating] = useState(false);

  const all = projects.data ?? [];
  const FILTERS: Record<Filter, (p: ProjectSummary) => boolean> = {
    all: () => true,
    Production: (p) => p.environment === "production",
    err: (p) => p.status === "failed" || p.status === "down" || p.status === "unreachable",
  };
  const key = (p: ProjectSummary) => (sort[0] === "status" ? p.status : p[sort[0]]);
  const rows = all.filter(FILTERS[tab]).sort((x, y) => (key(x) > key(y) ? 1 : -1) * sort[1]);

  const head = (k: SortKey, label: string) => (
    <button
      type="button"
      onClick={() => setSort((s) => [k, s[0] === k ? (-s[1] as 1 | -1) : 1])}
      style={{ display: "flex", alignItems: "center", gap: 6, padding: 0, border: 0, background: "transparent", cursor: "pointer", font: "500 12px/1 var(--font-sans)", color: sort[0] === k ? "var(--text-strong)" : "var(--text-muted)" }}
    >
      {label}
      <Icon name="ArrowDownUp" size={12} />
    </button>
  );

  return (
    <div style={{ padding: "20px 32px 40px 34px", display: "flex", flexDirection: "column", gap: 14 }}>
      <div style={{ ...card, borderRadius: 18, position: "relative", overflow: "hidden", display: "flex", alignItems: "center", gap: 24, padding: "26px 28px" }}>
        <span style={{ position: "absolute", inset: 0, background: "radial-gradient(50% 140% at 100% 0%,rgba(139,124,246,.16),transparent 70%)", pointerEvents: "none" }} />
        <span style={{ position: "relative", display: "flex", flexDirection: "column", gap: 10, flex: 1 }}>
          <span style={{ font: "600 18px/1.2 var(--font-sans)", color: "var(--text-strong)" }}>Déployez un dépôt en quelques secondes</span>
          <span style={{ font: "400 13px/1.55 var(--font-sans)", color: "var(--text-muted)", maxWidth: 520 }}>Importez depuis GitHub : Ciphera détecte le framework, construit l'image et la déploie sur vos serveurs.</span>
        </span>
        <div style={{ position: "relative", display: "flex", alignItems: "center", gap: 12 }}>
          <span style={{ display: "flex" }}>
            {[githubMark, supabaseMark, cloudflareMark].map((src, i) => (
              <span key={i} style={{ marginLeft: i ? -8 : 0, width: 34, height: 34, borderRadius: 10, display: "flex", alignItems: "center", justifyContent: "center", background: "#16161a", border: "1px solid rgba(255,255,255,.12)", boxShadow: "0 0 0 3px #0c0c0f" }}>
                <img src={src} alt="" style={{ width: 17, height: 17, objectFit: "contain" }} />
              </span>
            ))}
          </span>
          <Button variant="inverse" icon="Plus" onClick={() => setCreating(true)}>
            Importer un dépôt
          </Button>
        </div>
      </div>

      <div style={{ paddingLeft: 12, marginTop: 6 }}>
        <Tabs
          tabs={[
            { value: "all", label: "Tous", count: all.length },
            { value: "Production", label: "Production", count: all.filter(FILTERS.Production).length },
            { value: "err", label: "En erreur", count: all.filter(FILTERS.err).length },
          ]}
          value={tab}
          onChange={(v) => setTab(v as Filter)}
        />
      </div>

      <div style={{ ...card, overflow: "visible" }}>
        <div style={{ display: "grid", gridTemplateColumns: COLS, gap: 16, padding: "14px 16px" }}>
          <span />
          {head("name", "Détails")}
          {head("status", "Statut")}
          {head("updated_at", "Mis à jour")}
          <span />
        </div>
        {projects.isSuccess && rows.length === 0 && (
          <div style={{ borderTop: hair, padding: "28px 16px", ...lbl }}>{all.length === 0 ? "Aucun projet pour l'instant." : "Aucun projet dans cette vue."}</div>
        )}
        {rows.map((x) => {
          const isOpen = expanded === x.id;
          const hovered = hover === x.id;
          const [stIcon, stColor] = ST_ICON[x.status];
          const branch = branchOf(x.source);
          const detail = [x.environment, buildLabel(x.build), branch].filter(Boolean).join(" · ");
          const git = x.source.type === "git" ? x.source : null;
          return (
            <div key={x.id} data-project={x.name} style={{ borderTop: hair }} onMouseEnter={() => setHover(x.id)} onMouseLeave={() => setHover(null)}>
              <div
                onClick={() => setExpanded(isOpen ? null : x.id)}
                style={{ display: "grid", gridTemplateColumns: COLS, gap: 16, alignItems: "center", padding: "14px 16px", cursor: "pointer", background: hovered && !isOpen ? "var(--surface-raised)" : "transparent" }}
              >
                <Icon name="ChevronDown" size={15} color="var(--text-muted)" style={{ transform: isOpen ? "rotate(180deg)" : "none", transition: "transform var(--dur-fast)" }} />
                <div style={{ display: "flex", alignItems: "center", gap: 12, minWidth: 0 }}>
                  <span style={{ width: 40, height: 40, borderRadius: 10, flex: "none", display: "flex", alignItems: "center", justifyContent: "center", background: "var(--surface-selected)" }}>
                    <Icon name={x.build ? FW[x.build.kind] : "Container"} size={18} color="var(--text-strong)" />
                  </span>
                  <div style={{ display: "flex", flexDirection: "column", gap: 7, minWidth: 0 }}>
                    <span style={{ flex: "none", whiteSpace: "nowrap" }}>{mono(x.name, "var(--text-strong)", 13)}</span>
                    <span style={{ ...lbl, whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>{detail}</span>
                  </div>
                </div>
                <span style={{ display: "flex", alignItems: "center", gap: 8 }}>
                  <Icon name={stIcon} size={16} color={stColor} />
                  <span style={{ font: "400 13px/1.2 var(--font-sans)", color: "var(--text-body)" }}>{STATUS_VIEW[x.status].label}</span>
                </span>
                <span>
                  <span style={{ display: "inline-flex", alignItems: "center", height: 22, padding: "0 8px", borderRadius: 6, background: "var(--surface-selected)", font: "500 12px/1 var(--font-mono)", color: "var(--text-body)" }}>{timeAgo(x.updated_at)}</span>
                </span>
                <div style={{ display: "flex", gap: 6, justifyContent: "flex-end", position: "relative" }} onClick={(e) => e.stopPropagation()}>
                  <Button size="sm" onClick={() => open(x.id)}>
                    Gérer
                  </Button>
                  <Menu
                    label={`Actions de ${x.name}`}
                    width={210}
                    items={[
                      { icon: "RotateCw", label: "Redéployer", onSelect: () => undefined, disabled: true, tag: "Bientôt" },
                      { icon: "ScrollText", label: "Voir les logs", onSelect: () => undefined, disabled: true, tag: "Bientôt" },
                      { icon: "ArrowUpRight", label: "Ouvrir le site", onSelect: () => undefined, disabled: true, tag: "Bientôt" },
                      {
                        icon: "GitBranch",
                        label: "Voir le dépôt",
                        disabled: !git,
                        onSelect: () => {
                          if (git) window.open(`https://github.com/${git.repository}`, "_blank", "noopener,noreferrer");
                        },
                      },
                    ]}
                  />
                </div>
              </div>
              {isOpen && (
                <div style={{ padding: "0 16px 16px" }}>
                  {x.status === "failed" && (
                    <div style={{ display: "flex", alignItems: "center", gap: 12, padding: "14px 18px", borderRadius: 12, background: "rgba(239,68,68,.05)", border: "1px solid rgba(239,68,68,.18)", marginBottom: 10 }}>
                      <Icon name="CircleX" size={15} color="#f87171" />
                      <span style={{ flex: 1, font: "400 12px/1.4 var(--font-sans)", color: "var(--text-body)" }}>Le dernier build a échoué. La version précédente reste en ligne.</span>
                      <Button size="sm" disabled title="Disponible avec les déploiements">
                        Relancer
                      </Button>
                    </div>
                  )}
                  <div style={{ display: "grid", gridTemplateColumns: "repeat(4,minmax(0,1fr))", gap: 24, padding: "16px 22px 18px", borderRadius: 12, background: "var(--surface-raised)" }}>
                    {(
                      [
                        [x.source.type === "git" ? "Dépôt" : "Image", sourceLabel(x.source)],
                        ["Environnement", x.environment],
                        ["Services", String(x.services)],
                        ["Branche", branch ?? "—"],
                      ] as const
                    ).map(([k, v]) => (
                      <div key={k} style={{ display: "flex", flexDirection: "column", gap: 10, minWidth: 0 }}>
                        <span style={{ font: "400 13px/1 var(--font-sans)", color: "var(--text-body)" }}>{k}</span>
                        <span style={{ font: "500 14px/1 var(--font-sans)", color: "var(--text-strong)", whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>{v}</span>
                      </div>
                    ))}
                  </div>
                </div>
              )}
            </div>
          );
        })}
      </div>

      {creating && (
        <NewProjectModal
          onClose={() => setCreating(false)}
          onOpen={(id) => {
            setCreating(false);
            open(id);
          }}
          onCreated={() => void queryClient.invalidateQueries({ queryKey: PROJECTS_QUERY_KEY })}
        />
      )}
    </div>
  );
}
