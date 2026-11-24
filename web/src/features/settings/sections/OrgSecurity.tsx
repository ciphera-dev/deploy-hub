import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useState, type FormEvent } from "react";

import { Button, Switch, useToast } from "@/design-system";
import { FormError } from "@/features/auth/ui";

import { fetchSecurity, GENERAL_QUERY_KEY, saveSecurity, SECURITY_QUERY_KEY, type OrgSecurity } from "../api";
import { Card, lbl, Note, Row, SECTION_FORM_ID, SectionState, Soon } from "../ui";

const lines = (text: string) =>
  text
    .split(/[\n,]/)
    .map((l) => l.trim())
    .filter(Boolean);

function SecurityForm({ initial }: { initial: OrgSecurity }) {
  const client = useQueryClient();
  const { flash } = useToast();
  const [mfa, setMfa] = useState(initial.mfa_required);
  const [ipAllow, setIpAllow] = useState(initial.ip_allowlist.length > 0);
  const [list, setList] = useState(initial.ip_allowlist.join("\n"));
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState("");
  const entries = lines(list);
  const here = initial.current_ip;

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (busy) return;
    if (ipAllow && entries.length === 0) {
      setErr("Ajoutez au moins une adresse, ou désactivez la restriction.");
      return;
    }
    setBusy(true);
    const result = await saveSecurity({ mfa_required: mfa, ip_allowlist: ipAllow ? entries : [] });
    setBusy(false);
    if (!result.ok) {
      setErr(result.message);
      return;
    }
    client.setQueryData(SECURITY_QUERY_KEY, result.data);
    void client.invalidateQueries({ queryKey: GENERAL_QUERY_KEY });
    setList(result.data.ip_allowlist.join("\n"));
    setIpAllow(result.data.ip_allowlist.length > 0);
    flash("Paramètres enregistrés");
  };

  return (
    <form id={SECTION_FORM_ID} onSubmit={(e) => void submit(e)} noValidate style={{ display: "flex", flexDirection: "column", gap: 16 }}>
      <Card title="Sécurité de l'organisation" desc="S'applique à tous les membres.">
        <Row t="Double authentification obligatoire" d="Appliquée à la prochaine connexion : un membre sans 2FA devra l'activer avant d'accéder au panneau." first>
          <Switch ariaLabel="Double authentification obligatoire" checked={mfa} onChange={(v) => { setMfa(v); setErr(""); }} />
        </Row>
        <Row t="Authentification unique (SSO)" d="SAML ou OIDC.">
          <Soon />
          <Switch ariaLabel="Authentification unique (SSO)" checked={false} disabled />
        </Row>
        <Row t="Restreindre l'accès par IP" d="Seules les IP autorisées accèdent au tableau de bord.">
          <Switch ariaLabel="Restreindre l'accès par IP" checked={ipAllow} onChange={(v) => { setIpAllow(v); setErr(""); }} />
        </Row>
        {ipAllow && (
          <div style={{ display: "flex", flexDirection: "column", gap: 10, padding: "0 20px 18px" }}>
            <label style={{ display: "flex", flexDirection: "column", gap: 8 }}>
              <span style={{ font: "500 13px/1.2 var(--font-sans)", color: "var(--text-strong)" }}>Adresses autorisées</span>
              <span style={{ ...lbl, lineHeight: 1.45 }}>Une adresse ou un bloc CIDR par ligne (ex. 203.0.113.7 ou 198.51.100.0/24).</span>
              <textarea
                aria-label="Adresses autorisées"
                value={list}
                rows={Math.min(8, Math.max(3, entries.length + 1))}
                spellCheck={false}
                onChange={(e) => { setList(e.target.value); setErr(""); }}
                style={{ width: "100%", boxSizing: "border-box", padding: "10px 12px", borderRadius: 10, border: "1px solid rgba(255,255,255,.1)", background: "rgba(255,255,255,.045)", color: "var(--text-strong)", font: "400 13px/1.5 var(--font-mono)", outline: "none", resize: "vertical" }}
              />
            </label>
            <span style={{ display: "flex", alignItems: "center", gap: 10, flexWrap: "wrap", ...lbl }}>
              Votre adresse : <span style={{ fontFamily: "var(--font-mono)", color: "var(--text-strong)" }}>{here}</span>
              {!entries.includes(here) && (
                <Button size="sm" icon="Plus" onClick={() => setList((l) => (l.trim() ? l.trimEnd() + "\n" : "") + here)}>
                  Ajouter mon adresse
                </Button>
              )}
            </span>
          </div>
        )}
        {ipAllow && (
          <Note>
            Les deploy hooks, le webhook GitHub et les certificats HTTPS restent joignables. Une liste qui exclut votre adresse est refusée ; si votre adresse change, lancez <code style={{ fontFamily: "var(--font-mono)" }}>sudo ciphera admin clear-ip-allowlist</code> sur le serveur.
          </Note>
        )}
      </Card>
      {err && <FormError>{err}</FormError>}
      <div style={{ display: "flex", justifyContent: "flex-end" }}>
        <Button type="submit" variant="inverse" icon={busy ? "Loader" : undefined} disabled={busy} aria-busy={busy || undefined}>
          Enregistrer
        </Button>
      </div>
    </form>
  );
}

/** Parametres › Securite de l'organisation (Pages2.jsx#SettingsV2) : 2FA obligatoire, SSO (v0.2), liste d'IP. */
export function OrgSecuritySection() {
  const query = useQuery({ queryKey: SECURITY_QUERY_KEY, queryFn: fetchSecurity });
  if (!query.data) return <SectionState error={query.error} what="la sécurité de l'organisation" denied="Accès refusé : réservé aux administrateurs." />;
  return <SecurityForm initial={query.data} />;
}
