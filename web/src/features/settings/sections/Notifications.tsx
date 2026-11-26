import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useState, type FormEvent, type ReactNode } from "react";

import { Button, Icon, Switch, useToast } from "@/design-system";
import { FormError } from "@/features/auth/ui";
import { ApiError } from "@/lib/api/errors";

import { fetchNotifications, NOTIFICATIONS_QUERY_KEY, saveNotifications, type NotificationSettings, type NotificationUpdate, type SmtpUpdate } from "../api";
import { StepUpModal } from "../StepUpModal";
import { Card, Input, Row, SECTION_FORM_ID } from "../ui";

/** Chiffrement (selecteur de Connectors.jsx, carte SMTP) : STARTTLS ou SSL/TLS, jamais en clair. */
const TLS_MODES: [NotificationUpdate["tls"], string, number][] = [
  ["starttls", "STARTTLS", 587],
  ["tls", "SSL/TLS", 465],
];

function TlsPicker({ value, onChange, disabled }: { value: string; onChange: (tls: string, port: number) => void; disabled?: boolean }) {
  return (
    <div role="radiogroup" aria-label="Chiffrement" style={{ width: "100%", maxWidth: 340, display: "grid", gridTemplateColumns: "repeat(2,1fr)", gap: 3, padding: 3, height: 38, boxSizing: "border-box", borderRadius: 10, border: "1px solid rgba(255,255,255,.09)", background: "rgba(255,255,255,.045)" }}>
      {TLS_MODES.map(([mode, name, port]) => {
        const on = value === mode;
        return (
          <button key={mode} type="button" role="radio" aria-checked={on} disabled={disabled} onClick={() => onChange(mode, port)} style={{ display: "flex", alignItems: "center", justifyContent: "center", borderRadius: 8, border: 0, cursor: disabled ? "default" : "pointer", font: "500 12px/1 var(--font-sans)", background: on ? "rgba(255,255,255,.12)" : "transparent", color: on ? "var(--text-strong)" : "var(--text-muted)", transition: "all .15s" }}>
            {name}
          </button>
        );
      })}
    </div>
  );
}

interface Draft {
  enabled: boolean;
  host: string;
  port: string;
  tls: string;
  username: string;
  password: string;
  from_address: string;
  from_name: string;
}

const draftOf = (s: NotificationSettings): Draft => ({
  enabled: s.enabled,
  host: s.host,
  port: String(s.port),
  tls: s.tls,
  username: s.username,
  password: "",
  from_address: s.from_address,
  from_name: s.from_name,
});

function SmtpForm({ initial, readOnly }: { initial: NotificationSettings; readOnly: boolean }) {
  const queryClient = useQueryClient();
  const { flash } = useToast();
  const [saved, setSaved] = useState(initial);
  const [s, setDraft] = useState(() => draftOf(initial));
  const [pending, setPending] = useState<SmtpUpdate | null>(null);
  const u = <K extends keyof Draft>(k: K, v: Draft[K]) => {
    setDraft((x) => ({ ...x, [k]: v }));
  };

  const port = Number(s.port);
  const valid = s.host.trim() !== "" && /^\d{1,5}$/.test(s.port) && port >= 1 && port <= 65535 && s.from_address.trim() !== "" && s.from_name.trim() !== "";

  // Enregistrement confirme par le step-up (mot de passe du compte, code du second facteur) : ces
  // reglages decident si les avis de securite aux proprietaires partent.
  const submit = (e: FormEvent) => {
    e.preventDefault();
    if (readOnly || !valid || pending) return;
    setPending({
      enabled: s.enabled,
      host: s.host.trim(),
      port,
      tls: s.tls,
      username: s.username.trim(),
      from_address: s.from_address.trim(),
      from_name: s.from_name.trim(),
      // Champ vide : le mot de passe enregistre est conserve.
      ...(s.password ? { password: s.password } : {}),
    });
  };
  const done = (settings: NotificationSettings) => {
    setPending(null);
    setSaved(settings);
    setDraft((x) => ({ ...x, password: "" }));
    queryClient.setQueryData(NOTIFICATIONS_QUERY_KEY, settings);
    flash("Paramètres enregistrés");
  };

  const status: [string, string, ReactNode] = saved.email_available
    ? ["CircleCheck", "var(--green-500)", <>Les e-mails partent via {saved.host}.</>]
    : !saved.public_url
      ? ["Info", "var(--text-muted)", <>Définissez <code style={{ fontFamily: "var(--font-mono)" }}>CIPHERA_PUBLIC_URL</code> sur le serveur (adresse publique du panneau) : sans elle, aucun lien n'est envoyé par e-mail et « mot de passe oublié » invite à contacter un administrateur.</>]
      : ["Info", "var(--text-muted)", <>Aucun e-mail n'est envoyé : activez l'envoi et enregistrez un serveur SMTP. Les invitations et réinitialisations restent possibles par lien à copier.</>];

  return (
    <form id={SECTION_FORM_ID} onSubmit={submit} noValidate style={{ display: "flex", flexDirection: "column", gap: 16 }}>
      <Card title="Notifications" desc="Où l'équipe est prévenue des événements importants.">
        <Row t="E-mail" d="Invitations d'équipe et liens de réinitialisation du mot de passe." first>
          <Switch checked={s.enabled} onChange={(v) => u("enabled", v)} disabled={readOnly} ariaLabel="Envoi des e-mails" />
        </Row>
        <div style={{ display: "flex", alignItems: "flex-start", gap: 10, margin: "0 20px 16px", padding: "10px 12px", borderRadius: 10, background: "var(--surface-raised)", font: "400 12px/1.45 var(--font-sans)", color: "var(--text-muted)" }}>
          <Icon name={status[0] as "Info"} size={14} color={status[1]} style={{ marginTop: 1 }} />
          <span>{status[2]}</span>
        </div>
      </Card>

      <Card title="Serveur SMTP" desc="Envoi des e-mails de l'instance. Chiffrement obligatoire.">
        <Row t="Serveur SMTP" d="Nom d'hôte du serveur d'envoi." first>
          <Input disabled={readOnly} label="Serveur SMTP" value={s.host} onChange={(v) => u("host", v.trim().toLowerCase())} placeholder="smtp.exemple.com" mono />
        </Row>
        <Row t="Chiffrement" d="L'envoi en clair n'est pas proposé.">
          <TlsPicker
            disabled={readOnly}
            value={s.tls}
            onChange={(tls, p) => {
              u("tls", tls);
              u("port", String(p));
            }}
          />
        </Row>
        <Row t="Port" d="587 pour STARTTLS, 465 pour SSL/TLS en général.">
          <Input disabled={readOnly} label="Port" value={s.port} onChange={(v) => u("port", v.replace(/\D/g, "").slice(0, 5))} placeholder="587" mono />
        </Row>
        <Row t="Utilisateur" d="Laissez vide si le serveur n'exige pas d'authentification.">
          <Input disabled={readOnly} label="Utilisateur SMTP" value={s.username} onChange={(v) => u("username", v)} placeholder="utilisateur" autoComplete="off" />
        </Row>
        <Row t="Mot de passe" d={saved.password_set ? "Un mot de passe est enregistré (chiffré, jamais réaffiché). Laissez vide pour le conserver." : "Chiffré sur le serveur, jamais réaffiché."}>
          <Input disabled={readOnly} label="Mot de passe SMTP" type="password" value={s.password} onChange={(v) => u("password", v)} placeholder={saved.password_set ? "••••••••" : ""} autoComplete="new-password" />
        </Row>
        <Row t="Adresse d'expédition">
          <Input disabled={readOnly} label="Adresse d'expédition" value={s.from_address} onChange={(v) => u("from_address", v)} placeholder="ciphera@exemple.com" mono />
        </Row>
        <Row t="Nom affiché" d={<>Apparaît comme <span style={{ color: "var(--text-body)" }}>{(s.from_name || "Ciphera") + " <" + (s.from_address || "adresse") + ">"}</span></>}>
          <Input disabled={readOnly} label="Nom affiché" value={s.from_name} onChange={(v) => u("from_name", v)} placeholder="Ciphera" />
        </Row>
      </Card>

      {pending && (
        <StepUpModal
          title="Confirmer les réglages SMTP"
          description="Ces réglages décident si les avis de sécurité partent. Confirmez avec votre mot de passe."
          action="Confirmer"
          icon="Check"
          run={(stepUp) => saveNotifications(pending, stepUp)}
          onClose={() => setPending(null)}
          onDone={done}
        />
      )}
      {readOnly ? (
        <div style={{ display: "flex", justifyContent: "flex-end", alignItems: "center", gap: 8, font: "400 12px/1.4 var(--font-sans)", color: "var(--text-muted)" }}>
          <Icon name="Lock" size={13} />
          Réservé au propriétaire
        </div>
      ) : (
        <div style={{ display: "flex", justifyContent: "flex-end" }}>
          <Button type="submit" variant="inverse" disabled={!valid || !!pending}>
            Enregistrer
          </Button>
        </div>
      )}
    </form>
  );
}

/** Parametres › Notifications (Pages2.jsx#SettingsV2) : envoi d'e-mails et serveur SMTP. */
export function NotificationsSection({ readOnly = false }: { readOnly?: boolean }) {
  const query = useQuery({ queryKey: NOTIFICATIONS_QUERY_KEY, queryFn: fetchNotifications });
  if (query.isError) {
    const status = query.error instanceof ApiError ? query.error.status : 0;
    return (
      <FormError>
        {status === 403 || status === 401
          ? "Accès refusé : seuls les administrateurs gèrent les notifications."
          : "Impossible de charger les paramètres de notification. Réessayez plus tard."}
      </FormError>
    );
  }
  if (!query.data) {
    return (
      <div role="status" style={{ display: "flex", alignItems: "center", gap: 10, padding: 20, font: "400 13px/1 var(--font-sans)", color: "var(--text-muted)" }}>
        <Icon name="Loader" size={14} className="login-spin" />
        Chargement des paramètres…
      </div>
    );
  }
  return <SmtpForm initial={query.data} readOnly={readOnly} />;
}
