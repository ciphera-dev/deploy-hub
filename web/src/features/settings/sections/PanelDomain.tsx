import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useState, type FormEvent, type ReactNode } from "react";

import { Button, Icon, type IconName, Modal, Switch, useToast } from "@/design-system";
import { FormError } from "@/features/auth/ui";

import { confirmPanelDomain, fetchPanelDomain, PANEL_DOMAIN_QUERY_KEY, removePanelDomain, setPanelDomain, type PanelDomain } from "../api";
import { Card, Input, Note, Pill, Row, SECTION_FORM_ID, SectionState, Soon } from "../ui";

/** Cartes d'acces de SettingsV2 : domaine public (v0.1) ou acces prive Tailscale (v0.2). */
function AccessCards() {
  const cards: [string, IconName, string, string, boolean][] = [
    ["public", "Globe", "Domaine public", "Accessible sur Internet via votre domaine et HTTPS.", true],
    ["vpn", "Shield", "Accès privé (VPN)", "Uniquement depuis les appareils de votre tailnet Tailscale. Disponible en v0.2.", false],
  ];
  return (
    <div role="radiogroup" aria-label="Accès au panneau" style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 10 }}>
      {cards.map(([k, ic, t, d, on]) => (
        <div key={k} role="radio" aria-checked={on} aria-disabled={!on} style={{ display: "flex", alignItems: "center", gap: 14, padding: "16px 18px", borderRadius: 16, textAlign: "left", border: "1px solid " + (on ? "var(--border-strong)" : "var(--border-default)"), background: on ? "var(--surface-raised)" : "var(--surface-app)", opacity: on ? 1 : 0.6 }}>
          <span style={{ width: 40, height: 40, borderRadius: 10, flex: "none", display: "flex", alignItems: "center", justifyContent: "center", background: "var(--surface-selected)" }}>
            <Icon name={ic} size={18} color="var(--text-strong)" />
          </span>
          <span style={{ display: "flex", flexDirection: "column", gap: 7, flex: 1 }}>
            <span style={{ font: "600 14px/1 var(--font-sans)", color: "var(--text-strong)" }}>{t}</span>
            <span style={{ font: "400 12px/1.4 var(--font-sans)", color: "var(--text-subtle)" }}>{d}</span>
          </span>
          <span style={{ width: 18, height: 18, borderRadius: 99, flex: "none", border: "2px solid " + (on ? "#8b7cf6" : "var(--border-strong)"), display: "flex", alignItems: "center", justifyContent: "center" }}>
            {on && <span style={{ width: 8, height: 8, borderRadius: 99, background: "#8b7cf6" }} />}
          </span>
        </div>
      ))}
    </div>
  );
}

function dnsPill(p: PanelDomain): ReactNode {
  if (p.status === "none") return <Pill tone="muted" icon="CircleMinus" mono>non vérifié</Pill>;
  if (p.status === "error") return <Pill tone="error" icon="CircleX" mono>erreur</Pill>;
  return <Pill tone="ok" icon="CircleCheck" mono>pointe vers ce serveur</Pill>;
}

function httpsPill(p: PanelDomain): ReactNode {
  if (p.status === "awaiting_confirmation" || p.status === "active") return <Pill tone="ok" icon="Lock">Certificat valide</Pill>;
  if (p.status === "issuing") return <Pill tone="muted" icon="Loader">Émission du certificat…</Pill>;
  if (p.status === "error") return <Pill tone="error" icon="LockOpen">Indisponible</Pill>;
  return <Pill tone="muted" icon="LockOpen">En attente du DNS</Pill>;
}

/** Confirmation depuis le nouveau domaine ; avec une GitHub App, la Setup URL doit d'abord etre changee a la main. */
function ConfirmBlock({ domain, onDone }: { domain: string; onDone: (p: PanelDomain) => void }) {
  const [busy, setBusy] = useState(false);
  const [setupUrl, setSetupUrl] = useState<string | null>(null);
  const [updated, setUpdated] = useState(false);
  const [err, setErr] = useState("");
  const onDomain = window.location.hostname.toLowerCase() === domain;

  const confirm = async () => {
    if (busy) return;
    setBusy(true);
    const r = await confirmPanelDomain(updated);
    setBusy(false);
    if (r.ok) return onDone(r.data);
    if (r.code === "github_setup_url") {
      setSetupUrl(r.message);
      return;
    }
    setErr(r.message);
  };

  if (!onDomain) {
    return (
      <Note icon="ArrowUpRight">
        Le certificat est actif. Ouvrez{" "}
        <a href={`https://${domain}/settings/panel-domain`} style={{ color: "var(--text-strong)" }}>
          https://{domain}/settings/panel-domain
        </a>{" "}
        et confirmez depuis ce domaine : l'accès actuel reste ouvert jusque-là.
      </Note>
    );
  }
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 10, padding: "0 20px 18px" }}>
      <span style={{ font: "400 12px/1.45 var(--font-sans)", color: "var(--text-muted)" }}>Vous êtes sur le nouveau domaine. Confirmer ferme l'accès direct (port 8443).</span>
      {setupUrl && (
        <div role="alert" style={{ display: "flex", flexDirection: "column", gap: 8, padding: "10px 12px", borderRadius: 10, background: "var(--surface-raised)", font: "400 12px/1.45 var(--font-sans)", color: "var(--text-body)" }}>
          <span>{setupUrl}</span>
          <label style={{ display: "flex", alignItems: "center", gap: 8, cursor: "pointer" }}>
            <input type="checkbox" checked={updated} onChange={(e) => setUpdated(e.target.checked)} />
            J'ai remplacé l'URL de configuration (Setup URL) de la GitHub App
          </label>
        </div>
      )}
      {err && <FormError>{err}</FormError>}
      <div>
        <Button variant="inverse" icon={busy ? "Loader" : "Check"} disabled={busy || (setupUrl !== null && !updated)} onClick={() => void confirm()}>
          Confirmer le domaine
        </Button>
      </div>
    </div>
  );
}

function RemoveModal({ domain, active, onClose, onDone }: { domain: string; active: boolean; onClose: () => void; onDone: () => void }) {
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState("");
  const remove = async () => {
    if (busy) return;
    setBusy(true);
    const r = await removePanelDomain();
    setBusy(false);
    if (!r.ok) {
      setErr(r.message);
      return;
    }
    onDone();
  };
  return (
    <Modal
      title={active ? `Retirer ${domain} ?` : "Annuler la bascule ?"}
      description="Le panneau redevient accessible sur l'accès direct (port 8443, certificat auto-signé)."
      onClose={onClose}
      width={480}
      footer={
        <>
          <Button onClick={onClose}>Annuler</Button>
          <Button variant="inverse" icon={busy ? "Loader" : "Trash"} disabled={busy} onClick={() => void remove()}>
            {active ? "Retirer le domaine" : "Annuler la bascule"}
          </Button>
        </>
      }
    >
      <div style={{ display: "flex", flexDirection: "column", gap: 10, font: "400 13px/1.5 var(--font-sans)", color: "var(--text-body)" }}>
        <span>La route et le certificat de {domain} sont retirés.</span>
        {active && <span>Si une GitHub App est connectée, son webhook revient automatiquement sur l'accès direct. Sur GitHub, remettez aussi son URL de configuration (Setup URL) sur l'adresse 8443 du panneau suivie de /connectors.</span>}
        {err && <FormError>{err}</FormError>}
      </div>
    </Modal>
  );
}

function PanelDomainForm({ current }: { current: PanelDomain }) {
  const client = useQueryClient();
  const { flash } = useToast();
  const [domain, setDomain] = useState(current.domain ?? "");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState("");
  const [removing, setRemoving] = useState(false);
  const locked = current.status === "active";
  const update = (p: PanelDomain) => client.setQueryData(PANEL_DOMAIN_QUERY_KEY, p);

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (busy || locked || !domain) return;
    setBusy(true);
    const r = await setPanelDomain(domain);
    setBusy(false);
    if (!r.ok) {
      setErr(r.message);
      return;
    }
    setErr("");
    update(r.data);
    flash("DNS vérifié : certificat demandé");
  };

  const dnsHint = current.status === "error" ? current.error ?? "Le domaine ne pointe pas vers ce serveur." : current.status === "none" ? "Pas encore vérifié." : "Le domaine pointe bien vers ce serveur.";

  return (
    <>
      <AccessCards />
      <form id={SECTION_FORM_ID} onSubmit={(e) => void submit(e)} noValidate style={{ display: "flex", flexDirection: "column", gap: 16 }}>
        <Card title="Domaine principal" desc="Adresse à laquelle l'équipe accède au dashboard.">
          <Row t="Domaine du panel" d="Pointez un enregistrement A (et AAAA si IPv6) vers l'IP de ce serveur." first>
            <Input label="Domaine du panel" value={domain} onChange={(v) => { setDomain(v.toLowerCase().replace(/^https?:\/\//, "").replace(/\/.*$/, "")); setErr(""); }} placeholder="panel.exemple.com" mono />
          </Row>
          <Row t="Vérification DNS" d={dnsHint}>
            {dnsPill(current)}
            {!locked && (
              <Button type="submit" size="sm" icon={busy ? "Loader" : "RefreshCw"} disabled={busy || !domain}>
                Vérifier le DNS
              </Button>
            )}
          </Row>
          <Row t="Cloudflare" d="Créez l'enregistrement automatiquement dans votre zone Cloudflare.">
            <Soon />
          </Row>
          <Row t="HTTPS">{httpsPill(current)}</Row>
          <Row t="Certificat HTTPS automatique" d="Émis et renouvelé via Let's Encrypt.">
            <Switch ariaLabel="Certificat HTTPS automatique" checked disabled />
          </Row>
          {current.status === "awaiting_confirmation" && current.domain && <ConfirmBlock domain={current.domain} onDone={(p) => { update(p); flash("Domaine confirmé : accès direct fermé"); }} />}
          {current.status === "active" && <Note icon="CircleCheck" color="var(--green-500)">Le panneau est servi en HTTPS sur {current.domain} ; l'accès direct (port 8443) est fermé.</Note>}
        </Card>
        {err && <FormError>{err}</FormError>}
        {current.domain && (
          <div style={{ display: "flex", justifyContent: "flex-end" }}>
            <Button icon="Trash" onClick={() => setRemoving(true)}>
              {locked ? "Retirer le domaine" : "Annuler la bascule"}
            </Button>
          </div>
        )}
      </form>
      {removing && current.domain && (
        <RemoveModal
          domain={current.domain}
          active={locked}
          onClose={() => setRemoving(false)}
          onDone={() => {
            setRemoving(false);
            setDomain("");
            update({ domain: null, status: "none", error: null });
            flash("Domaine du panneau retiré");
          }}
        />
      )}
    </>
  );
}

/** Parametres › Domaine du panel (Pages2.jsx#SettingsV2, proprietaire seulement) : HTTPS, confirmation, retrait. */
export function PanelDomainSection() {
  const query = useQuery({
    queryKey: PANEL_DOMAIN_QUERY_KEY,
    queryFn: fetchPanelDomain,
    // Emission en cours : etat relu jusqu'au certificat.
    refetchInterval: (q) => (q.state.data?.status === "issuing" ? 5000 : false),
  });
  if (!query.data) return <SectionState error={query.error} what="le domaine du panneau" denied="Accès refusé : réservé au propriétaire de l'organisation." />;
  return <PanelDomainForm key={query.data.domain ?? ""} current={query.data} />;
}
