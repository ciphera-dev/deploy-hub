import type { IconName } from "@/design-system";

import type { KpiSeries, OverviewIssue, OverviewPeriod } from "./api";

/** Salutation selon l'heure (Pages2.jsx#OverviewV2). */
export function greeting(hour: number): string {
  return hour < 6 ? "Bonne nuit" : hour < 18 ? "Bonjour" : "Bonsoir";
}

/** « lundi 23 novembre ». */
export const longDate = (d: Date): string => d.toLocaleDateString("fr-FR", { weekday: "long", day: "numeric", month: "long" });

/** Prénom affiché dans la salutation. */
export const firstName = (name: string | undefined): string => (name ?? "").trim().split(/\s+/)[0] ?? "";

/** Libellé d'un jour du graphe (`AAAA-MM-JJ`, UTC) : « 23 nov. ». */
export function dayLabel(date: string): string {
  const d = new Date(`${date}T00:00:00Z`);
  return Number.isNaN(d.getTime()) ? date : d.toLocaleDateString("fr-FR", { day: "numeric", month: "short", timeZone: "UTC" });
}

export const PERIOD_LABEL: Record<OverviewPeriod, string> = { "7d": "7 derniers jours", "30d": "30 derniers jours" };

/** Ligne « À traiter » : icône, couleur, description et écran cible. */
export const ISSUE_VIEW: Record<OverviewIssue["kind"], { icon: IconName; color: string; label: string }> = {
  project_failed: { icon: "CircleX", color: "#f87171", label: "Dernier build en échec" },
  project_down: { icon: "CircleX", color: "#f87171", label: "Service arrêté hors du panneau" },
  server_unreachable: { icon: "Unplug", color: "#e5b454", label: "Agent injoignable" },
};

const fr = (v: number, digits = 0) => v.toLocaleString("fr-FR", { minimumFractionDigits: digits, maximumFractionDigits: digits });

/** Total d'un indicateur ; « — » sans mesure. */
export function kpiTotal(s: KpiSeries, fmt: (v: number) => string): string {
  return s.total == null ? "—" : fmt(s.total);
}

/** Variation « +12,4 % » ; rien sans base de comparaison. */
export function kpiChange(s: KpiSeries): string | null {
  if (s.change_percent == null) return null;
  return `${s.change_percent > 0 ? "+" : ""}${fr(s.change_percent, 1)} %`;
}

export const formatCount = (v: number): string => (v >= 1_000_000 ? `${fr(v / 1_000_000, 2)} M` : v >= 1000 ? `${fr(v / 1000, 0)} k` : fr(Math.round(v)));
export const formatMs = (v: number): string => `${fr(Math.round(v))} ms`;
