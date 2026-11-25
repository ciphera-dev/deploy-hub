import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { useRef, useState, type CSSProperties } from "react";

import { Button, Icon, Modal, useToast } from "@/design-system";
import { fetchMe, ME_QUERY_KEY, MFA_PENDING } from "@/features/auth/api";
import { FormError } from "@/features/auth/ui";

import { deleteProject, PROJECTS_QUERY_KEY, projectQueryKey, type Project } from "./api";

// Confirmation de la suppression d'un projet (owner et admin) : nom du projet
// tape pour confirmer ; les volumes (donnees) ne sont supprimes que si la case
// est cochee (decochee par defaut).

const fld: CSSProperties = { height: 42, padding: "0 12px", borderRadius: 10, border: "1px solid var(--border-default)", background: "var(--surface-raised)", color: "var(--text-strong)", font: "400 14px/1 var(--font-sans)", outline: "none", width: "100%", boxSizing: "border-box" };

const ERASED = ["Les services et leurs déploiements (conteneurs, images)", "Les domaines, certificats et variables du projet", "Le réseau du projet sur ses serveurs"];

/** Suppression d'un projet : owner et admin. */
export function useCanDeleteProject(): boolean {
  const me = useQuery({ queryKey: ME_QUERY_KEY, queryFn: fetchMe, retryOnMount: false, refetchOnMount: false });
  return !!me.data && me.data !== MFA_PENDING && (me.data.role === "owner" || me.data.role === "admin");
}

export function DeleteProjectModal({ project, onClose }: { project: Project; onClose: () => void }) {
  const { flash } = useToast();
  const client = useQueryClient();
  const navigate = useNavigate();
  const [name, setName] = useState("");
  const [purge, setPurge] = useState(false);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState("");
  const inFlight = useRef(false);
  const ok = name === project.name && !busy;

  const submit = async () => {
    if (!ok || inFlight.current) return;
    inFlight.current = true;
    setBusy(true);
    try {
      const r = await deleteProject(project.id, purge);
      if (!r.ok) {
        setErr(r.message);
        return;
      }
      flash("Projet supprimé");
      client.removeQueries({ queryKey: projectQueryKey(project.id) });
      await client.invalidateQueries({ queryKey: PROJECTS_QUERY_KEY });
      void navigate({ to: "/projects" });
    } finally {
      inFlight.current = false;
      setBusy(false);
    }
  };

  return (
    <Modal
      title={"Supprimer " + project.name + " ?"}
      description="Cette action est définitive et ne peut pas être annulée."
      onClose={onClose}
      width={480}
      footer={
        <>
          <Button onClick={onClose}>Annuler</Button>
          <button type="button" disabled={!ok} onClick={() => void submit()} style={{ display: "flex", alignItems: "center", gap: 8, height: 36, padding: "0 14px", borderRadius: 10, border: 0, cursor: ok ? "pointer" : "default", background: ok ? "#dc2626" : "rgba(220,38,38,.35)", color: "#fff", font: "600 13px/1 var(--font-sans)" }}>
            <Icon name={busy ? "Loader" : "Trash"} size={14} color="#fff" className={busy ? "login-spin" : undefined} />
            Supprimer le projet
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
        <label style={{ display: "flex", alignItems: "flex-start", gap: 10, cursor: "pointer", font: "400 13px/1.45 var(--font-sans)", color: "var(--text-body)" }}>
          <input type="checkbox" checked={purge} onChange={(e) => setPurge(e.target.checked)} style={{ accentColor: "#8b7cf6", width: 16, height: 16, marginTop: 1 }} />
          <span>
            Supprimer aussi les volumes (données)
            <span style={{ display: "block", color: "var(--text-subtle)", fontSize: 12 }}>
              {purge ? "Les données enregistrées par les services seront effacées." : "Les volumes restent sur les serveurs ; un nouveau projet ne les réutilise jamais."}
            </span>
          </span>
        </label>
        <label style={{ display: "flex", flexDirection: "column", gap: 8 }}>
          <span style={{ font: "500 13px/1.4 var(--font-sans)", color: "var(--text-strong)" }}>
            Tapez <span style={{ fontFamily: "var(--font-mono)", padding: "1px 6px", borderRadius: 5, background: "var(--surface-selected)", color: "#f87171" }}>{project.name}</span> pour confirmer
          </span>
          <input autoFocus aria-label="Nom du projet" value={name} onChange={(e) => { setName(e.target.value); setErr(""); }} placeholder={project.name} spellCheck={false} autoComplete="off" style={{ ...fld, fontFamily: "var(--font-mono)", borderColor: name && name !== project.name ? "rgba(248,113,113,.5)" : "var(--border-default)" }} />
        </label>
        {err && <FormError>{err}</FormError>}
      </div>
    </Modal>
  );
}
