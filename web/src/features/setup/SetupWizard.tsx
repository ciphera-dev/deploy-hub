import { useMutation, useQuery } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { useCallback, useEffect, useState } from "react";

import { CipheraLogo, Icon } from "@/design-system";
import { ApiError } from "@/lib/api/errors";

import { fetchStatus, rejectionMessage, submitSetup, type SetupPayload } from "./api";
import { ChecksStep } from "./ChecksStep";
import { OrganizationStep } from "./OrganizationStep";
import { languageOf, REGIONS, slugify, type Region } from "./regions";
import { AccessStep, AdminStep, adminValid, FinalizeStep, type AdminForm } from "./steps";
import { SetupButton, lbl } from "./ui";

const STEPS: readonly (readonly [string, string])[] = [
  ["Vérifications", "Prérequis du serveur"],
  ["Administrateur", "Premier compte"],
  ["Organisation", "Nom et région"],
  ["Accès", "Domaine plus tard"],
  ["Terminé", "Prêt à l'emploi"],
];
const FINAL = STEPS.length - 1;

const EMPTY_ADMIN: AdminForm = { first: "", last: "", email: "", pwd: "", pwd2: "" };

export interface SetupWizardProps {
  /** Jeton d'installation lu une fois dans l'URL ; garde en memoire seulement. */
  token: string | null;
}

export function SetupWizard({ token }: SetupWizardProps) {
  const navigate = useNavigate();
  const [step, setStep] = useState(0);
  const [checksReady, setChecksReady] = useState(false);
  const [admin, setAdmin] = useState<AdminForm>(EMPTY_ADMIN);
  const [orgName, setOrgName] = useState("");
  const [region, setRegion] = useState<Region>(REGIONS[0]!);
  const [error, setError] = useState<string | null>(null);

  const status = useQuery({ queryKey: ["setup", "status"], queryFn: fetchStatus, retry: false, staleTime: 0, gcTime: 0 });
  const goLogin = useCallback(() => void navigate({ to: "/login", replace: true }), [navigate]);
  useEffect(() => {
    if (status.data?.initialized) goLogin();
  }, [status.data, goLogin]);

  const install = useMutation({
    mutationFn: (body: SetupPayload) => submitSetup(token!, body),
    onSuccess: (result) => {
      if ("created" in result) {
        setAdmin(EMPTY_ADMIN);
        void navigate({ to: "/" });
      } else if (result.rejected === 410) goLogin();
      else setError(rejectionMessage(result));
    },
    onError: (e) => setError(e instanceof ApiError && e.status === 422 ? "Certains champs sont invalides. Revenez en arrière pour les corriger." : "L'installation n'a pas pu aboutir. Réessayez."),
  });

  if (!status.data || status.data.initialized) return null;

  const valid = [checksReady && !!token, adminValid(admin), orgName.trim().length > 1, true, false][step]!;
  const finishing = step === FINAL;

  const submit = () => {
    setError(null);
    setStep(FINAL);
    install.mutate({
      admin: { name: `${admin.first.trim()} ${admin.last.trim()}`, email: admin.email.trim(), password: admin.pwd },
      organization: { name: orgName.trim(), slug: slugify(orgName), timezone: region.timezone, language: languageOf(region) },
    });
  };

  return (
    <div style={{ minHeight: "100vh", display: "grid", gridTemplateColumns: "380px minmax(0,1fr)", padding: 12, boxSizing: "border-box", background: "var(--surface-page)" }}>
      <aside style={{ borderRadius: 24, border: "1px solid var(--border-default)", background: "radial-gradient(70% 50% at 30% 20%,rgba(255,255,255,.06),transparent 70%),linear-gradient(170deg,#18181c,#0d0d10 70%)", padding: "32px 30px", display: "flex", flexDirection: "column" }}>
        <CipheraLogo height={18} color="var(--text-strong)" style={{ alignSelf: "flex-start" }} />
        <div style={{ display: "flex", flexDirection: "column", gap: 8, marginTop: 40 }}>
          <span style={{ font: "600 22px/1.2 var(--font-sans)", color: "var(--text-strong)" }}>Installation</span>
          <span style={{ font: "400 13px/1.5 var(--font-sans)", color: "var(--text-muted)" }}>Configurez votre instance en quelques minutes.</span>
        </div>
        <ol aria-label="Étapes de l'installation" style={{ display: "flex", flexDirection: "column", marginTop: 32, padding: 0, listStyle: "none" }}>
          {STEPS.map(([title, sub], i) => {
            const on = i === step;
            const done = i < step;
            return (
              <li key={title} aria-current={on ? "step" : undefined} style={{ display: "flex", gap: 14, minHeight: 62 }}>
                <span style={{ display: "flex", flexDirection: "column", alignItems: "center", width: 26 }}>
                  <span style={{ width: 26, height: 26, borderRadius: 99, flex: "none", display: "flex", alignItems: "center", justifyContent: "center", background: done ? "var(--green-500)" : on ? "#ededed" : "transparent", border: done || on ? "none" : "1px solid var(--border-strong)", font: "600 12px/1 var(--font-sans)", color: done || on ? "#0b0b0b" : "var(--text-muted)" }}>
                    {done ? <Icon name="Check" size={13} color="#0b0b0b" /> : i + 1}
                  </span>
                  {i < STEPS.length - 1 && <span style={{ flex: 1, width: 1, margin: "4px 0", background: done ? "rgba(34,197,94,.5)" : "var(--border-subtle)" }} />}
                </span>
                <span style={{ display: "flex", flexDirection: "column", gap: 6, paddingTop: 4 }}>
                  <span style={{ font: "500 14px/1 var(--font-sans)", color: on || done ? "var(--text-strong)" : "var(--text-muted)" }}>{title}</span>
                  <span style={lbl}>{sub}</span>
                </span>
              </li>
            );
          })}
        </ol>
      </aside>
      <main style={{ display: "flex", flexDirection: "column", overflow: "auto" }}>
        <div style={{ flex: 1, display: "flex", justifyContent: "center", padding: "40px 40px" }}>
          <div key={step} className="setup-step" style={{ width: "100%", maxWidth: 480, display: "flex", flexDirection: "column", gap: 18, margin: "auto 0" }}>
            {step === 0 && <ChecksStep token={token} onReady={setChecksReady} onClosed={goLogin} />}
            {step === 1 && <AdminStep value={admin} onChange={setAdmin} />}
            {step === 2 && <OrganizationStep name={orgName} region={region} onName={setOrgName} onRegion={setRegion} />}
            {step === 3 && <AccessStep />}
            {finishing && <FinalizeStep pending={install.isPending} error={error} />}
            <div style={{ display: "flex", gap: 10, marginTop: 10 }}>
              {step > 0 && (!finishing || error) && (
                <SetupButton ghost onClick={() => setStep(finishing ? FINAL - 1 : step - 1)}>
                  Retour
                </SetupButton>
              )}
              <span style={{ flex: 1 }} />
              {!finishing && (
                <SetupButton disabled={!valid} icon="ArrowRight" onClick={step === FINAL - 1 ? submit : () => setStep(step + 1)}>
                  {step === FINAL - 1 ? "Installer" : "Continuer"}
                </SetupButton>
              )}
            </div>
          </div>
        </div>
      </main>
    </div>
  );
}
