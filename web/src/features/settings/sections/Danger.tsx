import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useState, type CSSProperties, type ReactNode } from "react";

import { Button, Icon, Modal, useToast } from "@/design-system";
import { asksForCode, hasSecondFactor } from "@/features/auth/step-up";
import { FormError } from "@/features/auth/ui";

import { DANGER_QUERY_KEY, deleteOrganization, download, exportOrganization, fetchDangerZone, type DangerZone, type OrganizationDeleted } from "../api";
import { lbl, SectionState, Soon } from "../ui";

const PHRASE = "SUPPRIMER";

const fld: CSSProperties = { height: 42, padding: "0 12px", borderRadius: 10, border: "1px solid var(--border-default)", background: "var(--surface-raised)", color: "var(--text-strong)", font: "400 14px/1 var(--font-sans)", outline: "none", width: "100%", boxSizing: "border-box" };

const ERASED = ["Tous les projets et leurs déploiements (conteneurs, domaines, certificats)", "Les données et volumes de tous les projets, sur tous les serveurs", "Les variables et secrets", "Les accès de tous les membres (vous restez propriétaire)", "La GitHub App de l'instance (à supprimer aussi sur GitHub)"];

/** Issue de la suppression : projets gardés (agent injoignable), GitHub App à supprimer à la main. */
function Outcome({ result }: { result: OrganizationDeleted }) {
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 12, font: "400 13px/1.5 var(--font-sans)", color: "var(--text-body)" }}>
      <span>
        {result.projects_deleted} projet(s) supprimé(s). Toutes les sessions ont été fermées : reconnectez-vous.
      </span>
      {!result.complete && (
        <div role="alert" style={{ display: "flex", flexDirection: "column", gap: 6, padding: "12px 14px", borderRadius: 12, border: "1px solid rgba(248,113,113,.25)", background: "rgba(248,113,113,.05)" }}>
          <span style={{ fontWeight: 600, color: "#f87171" }}>Suppression incomplète</span>
          <span>Le serveur de ces projets ne répond pas : ils sont conservés. Relancez la suppression quand il est en ligne.</span>
          <ul style={{ margin: 0, paddingLeft: 18 }}>
            {result.remaining_projects.map((p) => (
              <li key={p.id} style={{ fontFamily: "var(--font-mono)" }}>{p.name}</li>
            ))}
          </ul>
        </div>
      )}
      {result.github_app && (
        <div style={{ display: "flex", flexDirection: "column", gap: 6, padding: "12px 14px", borderRadius: 12, background: "var(--surface-raised)" }}>
          <span style={{ fontWeight: 600, color: "var(--text-strong)" }}>GitHub App à supprimer sur GitHub</span>
          <span>
            L'App « {result.github_app.name} » a été retirée de Ciphera mais existe encore sur GitHub. Supprimez-la : <a href={result.github_app.html_url} target="_blank" rel="noreferrer noopener" style={{ color: "var(--text-strong)" }}>{result.github_app.html_url}</a>, puis Settings → Advanced → Delete GitHub App.
          </span>
        </div>
      )}
    </div>
  );
}

function DeleteModal({ zone, onClose }: { zone: DangerZone; onClose: () => void }) {
  const queryClient = useQueryClient();
  const [pwd, setPwd] = useState("");
  const [needsCode, setNeedsCode] = useState(() => hasSecondFactor(queryClient));
  const [code, setCode] = useState("");
  const [word, setWord] = useState("");
  const [show, setShow] = useState(false);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState("");
  const [result, setResult] = useState<OrganizationDeleted | null>(null);
  const ok = pwd.length > 0 && (!needsCode || code.trim().length > 0) && word === PHRASE && !busy;

  const submit = async () => {
    if (!ok) return;
    setBusy(true);
    const r = await deleteOrganization(pwd, word, needsCode ? code.trim() : undefined);
    setBusy(false);
    if (!r.ok) {
      setErr(r.message);
      setCode("");
      if (asksForCode(r.code)) setNeedsCode(true);
      return;
    }
    setPwd("");
    setCode("");
    setResult(r.data);
  };

  if (result) {
    return (
      <Modal title="Organisation supprimée" width={480} footer={<Button variant="inverse" onClick={() => window.location.assign("/login")}>Se reconnecter</Button>}>
        <Outcome result={result} />
      </Modal>
    );
  }

  return (
    <Modal
      title={"Supprimer " + zone.name + " ?"}
      description="Cette action est définitive et ne peut pas être annulée."
      onClose={onClose}
      width={480}
      footer={
        <>
          <Button onClick={onClose}>Annuler</Button>
          <button type="button" disabled={!ok} onClick={() => void submit()} style={{ display: "flex", alignItems: "center", gap: 8, height: 36, padding: "0 14px", borderRadius: 10, border: 0, cursor: ok ? "pointer" : "default", background: ok ? "#dc2626" : "rgba(220,38,38,.35)", color: "#fff", font: "600 13px/1 var(--font-sans)" }}>
            <Icon name={busy ? "Loader" : "Trash"} size={14} color="#fff" className={busy ? "login-spin" : undefined} />
            Supprimer définitivement
          </button>
        </>
      }
    >
      <div style={{ display: "flex", flexDirection: "column", gap: 16 }}>
        <div style={{ display: "flex", flexDirection: "column", gap: 8, padding: "12px 14px", borderRadius: 12, border: "1px solid rgba(248,113,113,.25)", background: "rgba(248,113,113,.05)" }}>
          {ERASED.map((x) => (
            <span key={x} style={{ display: "flex", alignItems: "center", gap: 8, font: "400 13px/1.3 var(--font-sans)", color: "var(--text-body)" }}>
              <Icon name="X" size={13} color="#f87171" />
              {x}
            </span>
          ))}
        </div>
        <span style={{ ...lbl, lineHeight: 1.45 }}>Conservés : l'instance et ses serveurs, le domaine du panneau, le serveur SMTP et le journal d'audit.</span>
        <label style={{ display: "flex", flexDirection: "column", gap: 8 }}>
          <span style={{ font: "500 13px/1 var(--font-sans)", color: "var(--text-strong)" }}>Mot de passe du compte</span>
          <span style={{ position: "relative", display: "flex", alignItems: "center" }}>
            <input autoFocus type={show ? "text" : "password"} value={pwd} autoComplete="current-password" onChange={(e) => { setPwd(e.target.value); setErr(""); }} style={{ ...fld, paddingRight: 40 }} />
            <button type="button" aria-label={show ? "Masquer le mot de passe" : "Afficher le mot de passe"} onClick={() => setShow((v) => !v)} style={{ position: "absolute", right: 10, padding: 0, border: 0, background: "transparent", cursor: "pointer", display: "flex" }}>
              <Icon name={show ? "EyeOff" : "Eye"} size={16} color="var(--text-muted)" />
            </button>
          </span>
        </label>
        {needsCode && (
          <label style={{ display: "flex", flexDirection: "column", gap: 8 }}>
            <span style={{ font: "500 13px/1 var(--font-sans)", color: "var(--text-strong)" }}>Code de vérification (application ou code de secours)</span>
            <input type="text" autoComplete="one-time-code" maxLength={64} value={code} onChange={(e) => { setCode(e.target.value); setErr(""); }} style={fld} />
          </label>
        )}
        <label style={{ display: "flex", flexDirection: "column", gap: 8 }}>
          <span style={{ font: "500 13px/1.4 var(--font-sans)", color: "var(--text-strong)" }}>
            Tapez <span style={{ fontFamily: "var(--font-mono)", padding: "1px 6px", borderRadius: 5, background: "var(--surface-selected)", color: "#f87171" }}>{PHRASE}</span> pour confirmer
          </span>
          <input value={word} onChange={(e) => { setWord(e.target.value); setErr(""); }} placeholder={PHRASE} spellCheck={false} autoComplete="off" style={{ ...fld, fontFamily: "var(--font-mono)", letterSpacing: ".04em", borderColor: word && word !== PHRASE ? "rgba(248,113,113,.5)" : "var(--border-default)" }} />
        </label>
        {err && <FormError>{err}</FormError>}
      </div>
    </Modal>
  );
}

const btn = (danger: boolean): CSSProperties => ({ height: 34, padding: "0 14px", borderRadius: 10, cursor: "pointer", font: "600 12px/1 var(--font-sans)", border: "1px solid rgba(248,113,113,.4)", background: danger ? "#dc2626" : "transparent", color: danger ? "#fff" : "#f87171" });

function DangerRows({ zone }: { zone: DangerZone }) {
  const { flash } = useToast();
  const [del, setDel] = useState(false);
  const [exporting, setExporting] = useState(false);

  const runExport = async () => {
    if (exporting) return;
    setExporting(true);
    const r = await exportOrganization();
    setExporting(false);
    if (!r.ok) {
      flash(r.message);
      return;
    }
    download("ciphera-export.json", r.data);
    flash("Export JSON téléchargé");
  };

  const rows: { t: string; d: string; action: ReactNode }[] = [
    {
      t: "Transférer l'organisation",
      d: "Donner la propriété à un autre administrateur.",
      action: (
        <>
          <Soon />
          <button type="button" disabled style={{ ...btn(false), cursor: "default", opacity: 0.5 }}>
            Transférer
          </button>
        </>
      ),
    },
    {
      t: "Exporter toutes les données",
      d: "Projets, membres, variables (sans leurs valeurs) et configuration au format JSON. Aucun secret n'est exporté.",
      action: (
        <button type="button" disabled={exporting} onClick={() => void runExport()} style={btn(false)}>
          {exporting ? "Export…" : "Exporter"}
        </button>
      ),
    },
    {
      t: "Supprimer l'organisation",
      d: `Supprime définitivement les ${zone.projects} projet(s) avec leurs données et volumes, les variables et l'accès des ${Math.max(zone.members - 1, 0)} autre(s) membre(s).`,
      action: (
        <button type="button" onClick={() => setDel(true)} style={btn(true)}>
          Supprimer
        </button>
      ),
    },
  ];

  return (
    <>
      <div style={{ borderRadius: 16, border: "1px solid rgba(248,113,113,.3)", background: "rgba(248,113,113,.03)", overflow: "hidden" }}>
        {rows.map(({ t, d, action }, i) => (
          <div key={t} style={{ display: "flex", alignItems: "center", gap: 16, padding: "18px 20px", borderTop: i ? "1px solid rgba(248,113,113,.15)" : "none" }}>
            <span style={{ display: "flex", flexDirection: "column", gap: 6, flex: 1 }}>
              <span style={{ font: "500 13px/1 var(--font-sans)", color: "var(--text-strong)" }}>{t}</span>
              <span style={{ ...lbl, lineHeight: 1.4 }}>{d}</span>
            </span>
            <span style={{ display: "flex", alignItems: "center", gap: 10 }}>{action}</span>
          </div>
        ))}
      </div>
      {del && <DeleteModal zone={zone} onClose={() => setDel(false)} />}
    </>
  );
}

/** Parametres › Zone de danger (Pages2.jsx#SettingsV2, proprietaire seulement) : export JSON, suppression. */
export function DangerSection() {
  const query = useQuery({ queryKey: DANGER_QUERY_KEY, queryFn: fetchDangerZone });
  if (!query.data) return <SectionState error={query.error} what="la zone de danger" denied="Accès refusé : réservé au propriétaire de l'organisation." />;
  return <DangerRows zone={query.data} />;
}
