import { useQuery } from "@tanstack/react-query";
import { useEffect, type ReactNode } from "react";

import { Icon, type IconName } from "@/design-system";

import { fetchChecks, rejectionMessage, type SetupCheck } from "./api";
import { RowCard, SetupButton, Spinner, StepHeading, lbl } from "./ui";

const META: Record<string, { label: string; icon: IconName }> = {
  database: { label: "Base de données", icon: "Database" },
  tls: { label: "Certificat HTTPS", icon: "ShieldCheck" },
  data_dir: { label: "Stockage", icon: "HardDrive" },
  docker: { label: "Docker", icon: "Container" },
};

export interface ChecksStepProps {
  token: string | null;
  /** Remonte l'etat des verifications : `true` quand aucune ne bloque. */
  onReady: (ready: boolean) => void;
  /** L'assistant est deja termine. */
  onClosed: () => void;
}

export function ChecksStep({ token, onReady, onClosed }: ChecksStepProps) {
  const query = useQuery({
    queryKey: ["setup", "checks"],
    queryFn: () => fetchChecks(token!),
    enabled: !!token,
    retry: false,
    gcTime: 0,
    staleTime: 0,
    refetchOnMount: "always",
  });
  const data = query.data;
  const refusal = data && "rejected" in data ? data : null;
  const rejected = refusal ? refusal.rejected : null;
  const checks = data && "checks" in data ? data.checks : null;
  const ready = !!checks && checks.every((c) => c.ok);

  useEffect(() => onReady(ready), [ready, onReady]);
  useEffect(() => {
    if (rejected === 410) onClosed();
  }, [rejected, onClosed]);

  return (
    <>
      <StepHeading title="Bienvenue sur Ciphera" subtitle="Nous vérifions que ce serveur peut faire tourner votre instance." />
      {!token && <Problem>Lien d'installation incomplet. Ouvrez l'adresse affichée sur le serveur (« ciphera init » ou « sudo ciphera admin setup-token ») : elle contient le jeton d'installation.</Problem>}
      {refusal && refusal.rejected !== 410 && <Problem>{rejectionMessage(refusal)}</Problem>}
      {query.isError && <Problem>Les vérifications n'ont pas pu être effectuées.</Problem>}
      {token && (query.isPending || checks) && (
        <RowCard>
          {checks ? checks.map((c, i) => <CheckRow key={c.id} check={c} first={i === 0} />) : <Row icon="Cpu" title="Vérifications en cours" detail="Lecture de l'état du serveur…" status={<Spinner />} first />}
        </RowCard>
      )}
      <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
        <span style={{ flex: 1, ...lbl }}>{checks ? (ready ? "Toutes les vérifications sont passées." : "Corrigez les points en échec, puis relancez les vérifications.") : " "}</span>
        {(query.isError || (checks && !ready)) && (
          <SetupButton ghost onClick={() => void query.refetch()}>
            Relancer
          </SetupButton>
        )}
      </div>
    </>
  );
}

function Problem({ children }: { children: string }) {
  return (
    <div role="alert" style={{ display: "flex", gap: 10, padding: "12px 14px", borderRadius: 12, background: "var(--surface-raised)", border: "1px solid var(--border-default)", font: "400 13px/1.5 var(--font-sans)", color: "var(--text-body)" }}>
      <Icon name="TriangleExclamationPoint" size={16} color="var(--text-strong)" />
      <span>{children}</span>
    </div>
  );
}

function CheckRow({ check, first }: { check: SetupCheck; first: boolean }) {
  const meta = META[check.id] ?? { label: check.id, icon: "Cpu" as IconName };
  const status = !check.ok ? (
    <>
      <Icon name="CircleX" size={16} color="var(--red-500, #ef4444)" />
      Échec
    </>
  ) : check.warning ? (
    <>
      <Icon name="TriangleExclamationPoint" size={16} color="var(--amber-500, #f59e0b)" />
      Attention
    </>
  ) : (
    <>
      <Icon name="CircleCheck" size={16} color="var(--green-500)" />
      Validé
    </>
  );
  return <Row icon={meta.icon} title={meta.label} detail={check.detail} status={status} first={first} />;
}

function Row({ icon, title, detail, status, first }: { icon: IconName; title: string; detail: string; status: ReactNode; first?: boolean }) {
  return (
    <div style={{ display: "flex", alignItems: "center", gap: 14, padding: "14px 16px", borderTop: first ? "none" : "1px solid var(--border-subtle)" }}>
      <span style={{ width: 40, height: 40, borderRadius: 10, flex: "none", display: "flex", alignItems: "center", justifyContent: "center", background: "var(--surface-selected)" }}>
        <Icon name={icon} size={17} color="var(--text-strong)" />
      </span>
      <span style={{ display: "flex", flexDirection: "column", gap: 7, flex: 1, minWidth: 0 }}>
        <span style={{ font: "500 14px/1 var(--font-sans)", color: "var(--text-strong)" }}>{title}</span>
        <span style={{ font: "400 12px/1.3 var(--font-sans)", color: "var(--text-subtle)" }}>{detail}</span>
      </span>
      <span style={{ minWidth: 80, display: "flex", alignItems: "center", justifyContent: "flex-end", gap: 7, font: "400 13px/1 var(--font-sans)", color: "var(--text-body)" }}>{status}</span>
    </div>
  );
}
