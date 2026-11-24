import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useState, type FormEvent } from "react";

import { Button, Switch, useToast } from "@/design-system";
import { FormError } from "@/features/auth/ui";
import { GLOBAL_VARIABLES_QUERY_KEY } from "@/features/variables/api";

import { DEPLOY_QUERY_KEY, fetchDeploySettings, saveDeploySettings, type DeploySettings } from "../api";
import { SecretModal } from "../SecretModal";
import { Card, Row, SECTION_FORM_ID, SectionState, Soon } from "../ui";

function DeployForm({ initial }: { initial: DeploySettings }) {
  const client = useQueryClient();
  const { flash } = useToast();
  const [auto, setAuto] = useState(initial.auto);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState("");
  const [secret, setSecret] = useState(false);

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (busy) return;
    setBusy(true);
    const result = await saveDeploySettings(auto);
    setBusy(false);
    if (!result.ok) {
      setErr(result.message);
      return;
    }
    client.setQueryData(DEPLOY_QUERY_KEY, result.data);
    flash("Paramètres enregistrés");
  };

  return (
    <>
      <form id={SECTION_FORM_ID} onSubmit={(e) => void submit(e)} noValidate style={{ display: "flex", flexDirection: "column", gap: 16 }}>
        <Card title="Déploiements" desc="Comportement par défaut pour tous les projets.">
          <Row t="Déploiement automatique" d="Chaque push sur la branche de production lance un build. S'applique aux nouveaux projets." first>
            <Switch checked={auto} onChange={(v) => { setAuto(v); setErr(""); }} />
          </Row>
          <Row t="Previews de pull request" d="Un environnement éphémère par PR.">
            <Soon />
            <Switch checked={false} disabled />
          </Row>
          <Row t="Rétention des builds" d="Images conservées pour revenir en arrière (rollback), par environnement.">
            <span style={{ font: "500 13px/1 var(--font-sans)", color: "var(--text-strong)" }}>{initial.retention} derniers déploiements réussis</span>
          </Row>
          <Row t="Secrets" d="Variables chiffrées transmises aux conteneurs de tous les projets.">
            <Button size="sm" icon="KeyRound" onClick={() => setSecret(true)}>
              Gérer les secrets
            </Button>
          </Row>
        </Card>
        {err && <FormError>{err}</FormError>}
        <div style={{ display: "flex", justifyContent: "flex-end" }}>
          <Button type="submit" variant="inverse" icon={busy ? "Loader" : undefined} disabled={busy} aria-busy={busy || undefined}>
            Enregistrer
          </Button>
        </div>
      </form>
      {secret && (
        <SecretModal
          onClose={() => setSecret(false)}
          onSaved={(key) => {
            setSecret(false);
            void client.invalidateQueries({ queryKey: GLOBAL_VARIABLES_QUERY_KEY });
            flash(`Secret ${key} enregistré`);
          }}
        />
      )}
    </>
  );
}

/** Parametres › Deploiements (Pages2.jsx#SettingsV2) : deploiement automatique, retention, secrets. */
export function DeploymentsSection() {
  const query = useQuery({ queryKey: DEPLOY_QUERY_KEY, queryFn: fetchDeploySettings });
  if (!query.data) return <SectionState error={query.error} what="les paramètres de déploiement" denied="Accès refusé : réservé aux administrateurs." />;
  return <DeployForm initial={query.data} />;
}
