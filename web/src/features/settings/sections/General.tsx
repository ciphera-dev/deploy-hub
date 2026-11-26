import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useState, type FormEvent } from "react";

import { Button, Select, useToast } from "@/design-system";
import { FormError } from "@/features/auth/ui";
import { REGIONS } from "@/features/setup/regions";

import { fetchGeneral, GENERAL_QUERY_KEY, saveGeneral, type GeneralSettings } from "../api";
import { Card, Input, Row, SECTION_FORM_ID, SectionState } from "../ui";
import { UpdateStatus } from "./UpdateStatus";

const LANGUAGES: [string, string][] = [
  ["fr", "Français"],
  ["en", "English"],
];

/** Fuseaux proposes par l'assistant d'installation, UTC et le fuseau enregistre. */
function timezones(current: string): string[] {
  const list = [...new Set([...REGIONS.map((r) => r.timezone), "UTC"])].sort();
  return list.includes(current) ? list : [current, ...list];
}

function GeneralForm({ initial }: { initial: GeneralSettings }) {
  const client = useQueryClient();
  const { flash } = useToast();
  const [s, setS] = useState({ name: initial.name, slug: initial.slug ?? "", timezone: initial.timezone, language: initial.language });
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState("");
  const u = (k: keyof typeof s, v: string) => {
    setS((x) => ({ ...x, [k]: v }));
    setErr("");
  };
  const valid = s.name.trim() !== "" && /^[a-z0-9](?:[a-z0-9-]{0,48}[a-z0-9])?$/.test(s.slug);

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (!valid || busy) return;
    setBusy(true);
    const result = await saveGeneral({ name: s.name.trim(), slug: s.slug, timezone: s.timezone, language: s.language });
    setBusy(false);
    if (!result.ok) {
      setErr(result.message);
      return;
    }
    client.setQueryData(GENERAL_QUERY_KEY, result.data);
    flash("Paramètres enregistrés");
  };

  return (
    <form id={SECTION_FORM_ID} onSubmit={(e) => void submit(e)} noValidate style={{ display: "flex", flexDirection: "column", gap: 16 }}>
      <Card title="Organisation" desc="Informations visibles par les membres de l'espace.">
        <Row t="Nom de l'organisation" first>
          <Input label="Nom de l'organisation" value={s.name} onChange={(v) => u("name", v)} placeholder="Ciphera" />
        </Row>
        <Row t="Identifiant" d="Utilisé dans les URL et l'API.">
          <Input label="Identifiant" value={s.slug} onChange={(v) => u("slug", v.toLowerCase().replace(/[^a-z0-9-]/g, ""))} placeholder="ciphera" mono />
        </Row>
        <Row t="Fuseau horaire">
          <Select options={timezones(s.timezone)} value={s.timezone} onChange={(v) => u("timezone", v)} />
        </Row>
        <Row t="Langue">
          <Select options={LANGUAGES.map(([, n]) => n)} value={LANGUAGES.find(([c]) => c === s.language)?.[1]} onChange={(v) => u("language", LANGUAGES.find(([, n]) => n === v)?.[0] ?? "fr")} />
        </Row>
      </Card>
      {err && <FormError>{err}</FormError>}
      <div style={{ display: "flex", justifyContent: "flex-end" }}>
        <Button type="submit" variant="inverse" icon={busy ? "Loader" : undefined} disabled={!valid || busy} aria-busy={busy || undefined}>
          Enregistrer
        </Button>
      </div>
    </form>
  );
}

/** Parametres › General (Pages2.jsx#SettingsV2) : nom, identifiant, fuseau, langue ; etat de la mise a jour. */
export function GeneralSection() {
  const query = useQuery({ queryKey: GENERAL_QUERY_KEY, queryFn: fetchGeneral });
  if (!query.data) return <SectionState error={query.error} what="les paramètres généraux" denied="Accès refusé : réservé aux administrateurs." />;
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 16 }}>
      <GeneralForm initial={query.data} />
      <UpdateStatus />
    </div>
  );
}
