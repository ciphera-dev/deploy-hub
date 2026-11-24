import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Fragment, useRef, useState } from "react";

import { Button, Icon, Switch, useToast, type IconName } from "@/design-system";

import { fetchPrefs, PREFS_QUERY_KEY, savePrefs, type NotificationPrefs, type NotificationPrefsView } from "./api";
import { Alert, card, hair, Tile, Title } from "./ui";

// ProfileNotif.jsx limite au canal e-mail (v0.1) : retires le canal
// « Dashboard », « Ne pas déranger », les alertes de securite et le rapport
// hebdomadaire (aucune fonction derriere en v0.1).

type EventKey = Exclude<keyof NotificationPrefs, "email_enabled">;
const GROUPS: [string, [EventKey, IconName, string, string][]][] = [
  [
    "Déploiements",
    [
      ["deploy_succeeded", "CircleCheck", "Déploiement réussi", "Quand une version passe en production"],
      ["deploy_failed", "CircleX", "Échec de build ou de déploiement", "Avec l'étape en échec et le lien vers les logs"],
    ],
  ],
  ["Infrastructure", [["certificates", "Globe", "Certificats", "Expiration et renouvellement"]]],
  ["Organisation", [["team", "Users", "Équipe", "Invitations, changements de rôle"]]],
];
const EVENTS = GROUPS.flatMap(([, rows]) => rows.map(([key]) => key));

function Check({ on, label, disabled, onClick }: { on: boolean; label: string; disabled: boolean; onClick: () => void }) {
  return (
    <button
      type="button"
      role="checkbox"
      aria-checked={on}
      aria-label={label}
      disabled={disabled}
      onClick={onClick}
      style={{ width: 22, height: 22, borderRadius: 7, padding: 0, cursor: disabled ? "default" : "pointer", outline: "none", display: "flex", alignItems: "center", justifyContent: "center", border: on ? "none" : "1.5px solid var(--gray-500)", background: on ? "#ededed" : "transparent", opacity: disabled ? 0.35 : 1, transition: "all .15s" }}
    >
      {on && <Icon name="Check" size={13} color="#0b0b0b" />}
    </button>
  );
}

function prefsOf(view: NotificationPrefsView): NotificationPrefs {
  const { email_enabled, deploy_succeeded, deploy_failed, certificates, team } = view;
  return { email_enabled, deploy_succeeded, deploy_failed, certificates, team };
}

/** Onglet Notifications du Profil : chaque modification est enregistree aussitot. */
export function ProfileNotif({ email }: { email: string }) {
  const queryClient = useQueryClient();
  const { flash } = useToast();
  const prefs = useQuery({ queryKey: PREFS_QUERY_KEY, queryFn: fetchPrefs });
  const [error, setError] = useState("");
  // Les enregistrements partent un par un (chaine de promesses) ; chacun porte
  // l'etat cumule, et seule la reponse du dernier est appliquee.
  const queue = useRef<Promise<void>>(Promise.resolve());
  const seq = useRef(0);
  const view = prefs.data;

  if (!view) {
    if (prefs.isError) {
      return (
        <div style={{ ...card, display: "flex", alignItems: "center", justifyContent: "space-between", gap: 12, padding: "12px 16px" }}>
          <Alert>Impossible de charger vos préférences.</Alert>
          <Button onClick={() => void prefs.refetch()}>Réessayer</Button>
        </div>
      );
    }
    return (
      <div role="status" style={{ ...card, padding: "12px 16px", font: "400 12px/1.4 var(--font-sans)", color: "var(--text-subtle)" }}>
        Chargement…
      </div>
    );
  }
  const n = EVENTS.filter((k) => view[k]).length;

  const update = (change: (current: NotificationPrefsView) => Partial<NotificationPrefs>) => {
    const current = queryClient.getQueryData<NotificationPrefsView>(PREFS_QUERY_KEY) ?? view;
    const patch = change(current);
    const next = { ...prefsOf(current), ...patch };
    queryClient.setQueryData(PREFS_QUERY_KEY, { ...current, ...patch });
    setError("");
    const mine = ++seq.current;
    queue.current = queue.current.then(async () => {
      try {
        const saved = await savePrefs(next);
        if (mine !== seq.current) return;
        queryClient.setQueryData(PREFS_QUERY_KEY, saved);
        flash("Préférences enregistrées");
      } catch {
        setError("Enregistrement impossible pour le moment. Réessayez.");
        if (mine === seq.current) await queryClient.invalidateQueries({ queryKey: PREFS_QUERY_KEY });
      }
    });
  };

  return (
    <>
      {!view.email_available && (
        <div role="note" style={{ ...card, display: "flex", alignItems: "center", gap: 10, padding: "12px 16px", font: "400 12px/1.4 var(--font-sans)", color: "var(--text-subtle)" }}>
          <Icon name="Info" size={14} />
          L'envoi d'e-mails n'est pas configuré sur cette instance (Paramètres › Notifications) : vos préférences seront appliquées dès qu'il le sera.
        </div>
      )}
      <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fill,minmax(200px,1fr))", gap: 12 }}>
        <div style={{ ...card, padding: 18, display: "flex", flexDirection: "column", gap: 14, opacity: view.email_enabled ? 1 : 0.6, transition: "opacity .2s" }}>
          <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between" }}>
            <Tile icon="Mail" size={40} />
            <Switch checked={view.email_enabled} onChange={(v) => update(() => ({ email_enabled: v }))} ariaLabel="Notifications par e-mail" />
          </div>
          <span style={{ display: "flex", flexDirection: "column", gap: 6 }}>
            <span style={{ font: "500 14px/1 var(--font-sans)", color: "var(--text-strong)" }}>E-mail</span>
            <span style={{ font: "400 12px/1 var(--font-sans)", color: "var(--text-subtle)" }}>{email}</span>
          </span>
          <span style={{ font: "400 11px/1 var(--font-sans)", color: "var(--text-muted)" }}>
            {n} / {EVENTS.length} événements
          </span>
        </div>
      </div>
      {error && <Alert>{error}</Alert>}
      <Title t="Événements" d="Cochez les événements à recevoir par e-mail." />
      <div style={card}>
        <div style={{ display: "grid", gridTemplateColumns: "minmax(0,1fr) 92px", alignItems: "center", padding: "12px 18px", borderBottom: hair, background: "var(--surface-raised)" }}>
          <span style={{ font: "500 12px/1 var(--font-sans)", color: "var(--text-subtle)" }}>Événement</span>
          <span style={{ display: "flex", alignItems: "center", justifyContent: "center", gap: 6, font: "500 12px/1 var(--font-sans)", color: "var(--text-muted)" }}>
            <Icon name="Mail" size={12} color="#38bdf8" />
            E-mail
          </span>
        </div>
        {GROUPS.map(([group, rows], gi) => (
          <Fragment key={group}>
            <div style={{ padding: "14px 18px 6px", borderTop: gi ? hair : "none", font: "500 11px/1 var(--font-sans)", letterSpacing: ".06em", textTransform: "uppercase", color: "var(--text-faint)" }}>{group}</div>
            {rows.map(([key, icon, title, desc]) => (
              <div key={key} style={{ display: "grid", gridTemplateColumns: "minmax(0,1fr) 92px", alignItems: "center", padding: "10px 18px" }}>
                <span style={{ display: "flex", alignItems: "center", gap: 12, minWidth: 0 }}>
                  <Tile icon={icon} size={32} />
                  <span style={{ display: "flex", flexDirection: "column", gap: 5, minWidth: 0 }}>
                    <span style={{ font: "500 13px/1 var(--font-sans)", color: "var(--text-strong)" }}>{title}</span>
                    <span style={{ font: "400 12px/1.2 var(--font-sans)", color: "var(--text-subtle)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{desc}</span>
                  </span>
                </span>
                <span style={{ display: "flex", justifyContent: "center" }}>
                  <Check on={view[key] && view.email_enabled} label={`${title} par e-mail`} disabled={!view.email_enabled} onClick={() => update((current) => ({ [key]: !current[key] }))} />
                </span>
              </div>
            ))}
          </Fragment>
        ))}
      </div>
    </>
  );
}
