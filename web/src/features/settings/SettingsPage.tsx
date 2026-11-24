import { useQuery } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import type { CSSProperties, ReactNode } from "react";

import cipheraIcon from "@/assets/brand/ciphera-icon.svg";
import { Button, Icon, type IconName } from "@/design-system";
import { fetchMe, ME_QUERY_KEY, MFA_PENDING } from "@/features/auth/api";
import { FormError } from "@/features/auth/ui";

import { fetchGeneral, GENERAL_QUERY_KEY } from "./api";
import { DangerSection } from "./sections/Danger";
import { DeploymentsSection } from "./sections/Deployments";
import { GeneralSection } from "./sections/General";
import { NotificationsSection } from "./sections/Notifications";
import { OrgSecuritySection } from "./sections/OrgSecurity";
import { PanelDomainSection } from "./sections/PanelDomain";
import { TeamSection } from "./sections/Team";
import { lbl, SECTION_FORM_ID } from "./ui";

export type SectionKey = "general" | "panel-domain" | "deployments" | "notifications" | "security" | "team" | "danger";

interface SectionDef {
  key: SectionKey;
  name: string;
  icon: IconName;
  /** Proprietaire seulement (l'administrateur ne voit pas l'entree). */
  owner?: boolean;
  render: () => ReactNode;
}

// Navigation de Pages2.jsx#SettingsV2 (NAV) ; les URL nomment les sections.
export const SECTIONS: SectionDef[] = [
  { key: "general", name: "Général", icon: "Settings", render: () => <GeneralSection /> },
  { key: "panel-domain", name: "Domaine du panel", icon: "Globe", owner: true, render: () => <PanelDomainSection /> },
  { key: "deployments", name: "Déploiements", icon: "Rocket", render: () => <DeploymentsSection /> },
  { key: "notifications", name: "Notifications", icon: "Bell", render: () => <NotificationsSection /> },
  { key: "security", name: "Sécurité", icon: "Lock", render: () => <OrgSecuritySection /> },
  { key: "team", name: "Équipe", icon: "Users", render: () => <TeamSection /> },
  { key: "danger", name: "Zone de danger", icon: "TriangleExclamationPoint", owner: true, render: () => <DangerSection /> },
];

export function isSection(value: string): value is SectionKey {
  return SECTIONS.some((s) => s.key === value);
}

const CARD: CSSProperties = { border: "1px solid var(--border-default)", background: "var(--surface-app)" };
const sep = <span style={{ width: 1, height: 14, background: "var(--border-strong)" }} />;

/** Role du compte charge par le shell, sans nouvelle requete (voir useCanDeploy). */
function useRole(): string | null {
  const me = useQuery({ queryKey: ME_QUERY_KEY, queryFn: fetchMe, retryOnMount: false, refetchOnMount: false });
  return me.data && me.data !== MFA_PENDING ? me.data.role : null;
}

/** En-tete de SettingsV2 : organisation, identifiant, membres, fuseau, 2FA ; « Enregistrer » soumet la section. */
function Header({ section }: { section: SectionKey }) {
  const general = useQuery({ queryKey: GENERAL_QUERY_KEY, queryFn: fetchGeneral });
  const g = general.data;
  return (
    <div style={{ ...CARD, borderRadius: 20, padding: "22px 24px", display: "flex", flexDirection: "column", gap: 18 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 16 }}>
        <span style={{ width: 52, height: 52, borderRadius: 13, flex: "none", display: "flex", alignItems: "center", justifyContent: "center", background: "#fff" }}>
          <img src={cipheraIcon} alt="" style={{ width: 28, filter: "invert(1)" }} />
        </span>
        <span style={{ display: "flex", flexDirection: "column", gap: 8, flex: 1, minWidth: 0 }}>
          <span style={{ font: "600 18px/1 var(--font-sans)", color: "var(--text-strong)" }}>{g?.name ?? "Paramètres"}</span>
          <span style={lbl}>{g ? [g.slug, g.timezone].filter(Boolean).join(" · ") : " "}</span>
        </span>
        {section !== "danger" && section !== "team" && (
          <Button type="submit" form={SECTION_FORM_ID} variant="inverse">
            Enregistrer
          </Button>
        )}
      </div>
      {g && (
        <div style={{ display: "flex", alignItems: "center", gap: 18, flexWrap: "wrap", padding: "14px 18px", borderRadius: 12, background: "var(--surface-raised)", font: "400 13px/1 var(--font-sans)", color: "var(--text-muted)" }}>
          <span>
            ID : <span style={{ fontFamily: "var(--font-mono)", fontWeight: 600, color: "var(--text-strong)" }}>{g.id.slice(0, 8)}</span>
          </span>
          {sep}
          <span>
            Membres : <span style={{ fontWeight: 600, color: "var(--text-strong)" }}>{g.members}</span>
          </span>
          {sep}
          <span>
            Fuseau : <span style={{ color: "var(--text-strong)" }}>{g.timezone}</span>
          </span>
          {sep}
          <span>
            2FA : <span style={{ fontWeight: 600, color: g.mfa_required ? "var(--green-500)" : "#e5b454" }}>{g.mfa_required ? "obligatoire" : "facultative"}</span>
          </span>
        </div>
      )}
    </div>
  );
}

/** Parametres (Pages2.jsx#SettingsV2) : en-tete, navigation laterale, section affichee. */
export function SettingsPage({ section }: { section: SectionKey }) {
  const role = useRole();
  if (role !== null && role !== "owner" && role !== "admin") {
    return (
      <div style={{ padding: "20px 32px 40px 34px" }}>
        <FormError>Accès refusé : les paramètres sont réservés aux administrateurs.</FormError>
      </div>
    );
  }
  const visible = SECTIONS.filter((s) => !s.owner || role === "owner");
  const current = SECTIONS.find((s) => s.key === section) ?? SECTIONS[0];
  return (
    <div style={{ padding: "20px 32px 40px 34px", display: "flex", flexDirection: "column", gap: 20 }}>
      <Header section={current.key} />
      <div style={{ display: "grid", gridTemplateColumns: "220px minmax(0,1fr)", gap: 24, alignItems: "start" }}>
        <nav aria-label="Sections des paramètres" style={{ position: "sticky", top: 12, display: "flex", flexDirection: "column", gap: 2 }}>
          {visible.map((s) => {
            const on = s.key === current.key;
            return (
              <Link
                key={s.key}
                to="/settings/$section"
                params={{ section: s.key }}
                aria-current={on ? "page" : undefined}
                style={{ display: "flex", alignItems: "center", gap: 10, height: 36, padding: "0 12px", borderRadius: 9, textDecoration: "none", background: on ? "rgba(255,255,255,.07)" : "transparent", font: "500 13px/1 var(--font-sans)", color: s.key === "danger" ? "#f87171" : on ? "var(--text-strong)" : "var(--text-muted)" }}
              >
                <Icon name={s.icon} size={14} />
                {s.name}
              </Link>
            );
          })}
        </nav>
        <div style={{ display: "flex", flexDirection: "column", gap: 16, maxWidth: 860, minWidth: 0 }}>{current.render()}</div>
      </div>
    </div>
  );
}
