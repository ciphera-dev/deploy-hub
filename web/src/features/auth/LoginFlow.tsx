import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useState, type FormEvent } from "react";

import { Icon } from "@/design-system";

import { assertWithPasskey, fetchMfaMethods, fetchPasswordResetStatus, fetchWebauthnStatus, login, ME_QUERY_KEY, requestPasswordReset, verifySecondFactor, type LoginResult, type MfaMethods, type SecondFactor, type SecondFactorResult } from "./api";
import { AuthLayout, SIGNUP_COPY, type AuthStep } from "./AuthLayout";
import { CodeStep } from "./CodeStep";
import { AltMethod, FormError, LoginInput, MethodsDivider, PrimaryButton, StepTitle } from "./ui";

export type LoginStep = "login" | "register" | "forgot" | "sent" | "2fa" | "ok";

export interface LoginFlowProps {
  /** Arrivee apres un 401 (`/login?expired=1`) : bandeau « session expirée ». */
  expired?: boolean;
  /** Arrivee apres la creation du compte (`/login?created=1`, invitation acceptee) : bandeau « compte créé ». */
  created?: boolean;
  /** Arrivee apres une inscription ouverte (`/login?verify=1`) : un lien de confirmation a ete envoye par e-mail. */
  verifyEmail?: boolean;
  /** Arrivee apres une reinitialisation (`/login?reset=1`) : bandeau « mot de passe modifié ». */
  passwordReset?: boolean;
  /** `2fa` : session deja ouverte par le mot de passe, en attente du second facteur (route /2fa). */
  initialStep?: "login" | "register" | "2fa";
  /** Session ouverte (MFA comprise) : ouverture du tableau de bord. */
  onSignedIn: () => void;
}

/**
 * Controle minimal (quelque chose de part et d'autre d'un « @ ») : le serveur
 * decide. Plus strict, il refuserait des adresses acceptees a la creation du
 * compte (ex. domaine internationalise `example.xn--p1ai`).
 */
export function looksLikeEmail(value: string): boolean {
  const v = value.trim();
  const at = v.lastIndexOf("@");
  return at > 0 && at < v.length - 1;
}

/** Attente avant un nouvel essai : en minutes, ou en heures dès 1 h (jusqu'à 24 h pour le second facteur). */
export function tooManyAttempts(retryAfter = 900): string {
  const minutes = Math.max(1, Math.ceil(retryAfter / 60));
  if (minutes >= 60) return `Trop de tentatives. Réessayez dans ${Math.ceil(minutes / 60)} h.`;
  return `Trop de tentatives. Réessayez dans ${minutes} minute${minutes > 1 ? "s" : ""}.`;
}

/** Refus d'une cle de securite (connexion ou second facteur). */
function passkeyMessage(result: Extract<SecondFactorResult, { ok: false }>): string {
  if (result.reason === "invalid") return "Clé de sécurité refusée. Réessayez.";
  if (result.reason === "cancelled") return "Opération annulée ou délai dépassé. Réessayez.";
  if (result.reason === "rate_limited") return tooManyAttempts(result.retryAfter);
  return "Vérification impossible pour le moment. Réessayez.";
}

function failureMessage(result: Extract<LoginResult, { ok: false }>): string {
  if (result.reason === "invalid") return "E-mail ou mot de passe incorrect.";
  if (result.reason === "unverified") return "Confirmez votre adresse e-mail avec le lien reçu avant de vous connecter.";
  if (result.reason === "rate_limited") return tooManyAttempts(result.retryAfter);
  return "Connexion impossible pour le moment. Réessayez.";
}

/** Connexion, inscription et double authentification : un seul ecran a etapes, porte de login.html, register.html et 2fa.html. */
export function LoginFlow({ expired = false, created = false, verifyEmail = false, passwordReset = false, initialStep = "login", onSignedIn }: LoginFlowProps) {
  const queryClient = useQueryClient();
  const [step, setStep] = useState<LoginStep>(initialStep);
  const [email, setEmail] = useState("");
  const [pwd, setPwd] = useState("");
  const [show, setShow] = useState(false);
  const [remember, setRemember] = useState(false);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState("");

  const valid = looksLikeEmail(email) && pwd.length > 0;
  // Connexion sans mot de passe : seulement si l'hote le permet (pas d'adresse IP).
  const passkeys = useQuery({ queryKey: ["webauthn-status"], queryFn: fetchWebauthnStatus, staleTime: Infinity });
  // Mot de passe oublie : lien par e-mail seulement si l'instance peut l'envoyer.
  const resetByMail = useQuery({ queryKey: ["password-reset-status"], queryFn: fetchPasswordResetStatus, enabled: step === "forgot", staleTime: 60_000 });
  // Facteurs du compte en attente de MFA, relus a chaque passage par l'etape.
  const [methods, setMethods] = useState<MfaMethods>();
  useEffect(() => {
    if (step !== "2fa") return;
    let live = true;
    void fetchMfaMethods().then((m) => {
      if (live) setMethods(m);
    });
    return () => {
      live = false;
      setMethods(undefined);
    };
  }, [step]);

  const signedIn = () => {
    setPwd("");
    setStep("ok");
    // Le compte affiche par le shell vient de GET /api/me : rien de la session precedente.
    queryClient.removeQueries({ queryKey: ME_QUERY_KEY });
    onSignedIn();
  };

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (!valid || busy) return;
    setErr("");
    setBusy(true);
    const result = await login({ email: email.trim(), password: pwd, remember });
    setBusy(false);
    if (!result.ok) {
      setErr(failureMessage(result));
      return;
    }
    if (result.mfaRequired) {
      setPwd("");
      setStep("2fa");
    } else signedIn();
  };

  /** Second facteur : `undefined` si la session est ouverte (ou a refaire), sinon le refus a afficher. */
  const verifyFactor = async (factor: SecondFactor): Promise<string | undefined> => {
    const result = await verifySecondFactor(factor);
    if (result.ok) {
      signedIn();
      return undefined;
    }
    if (result.reason === "expired") {
      setErr("Session expirée, reconnectez-vous.");
      setStep("login");
      return undefined;
    }
    if (factor.kind === "webauthn") return passkeyMessage(result);
    if (result.reason === "invalid") return factor.kind === "totp" ? "Code incorrect, réessayez." : "Code de secours incorrect ou déjà utilisé.";
    if (result.reason === "rate_limited") return tooManyAttempts(result.retryAfter);
    return "Vérification impossible pour le moment. Réessayez.";
  };

  /** Connexion sans mot de passe avec une passkey. */
  const passkeyLogin = async () => {
    if (busy) return;
    setErr("");
    setBusy(true);
    const result = await assertWithPasskey("login", remember);
    setBusy(false);
    if (result.ok) signedIn();
    else setErr(passkeyMessage(result));
  };

  /** Demande d'un lien : meme suite que le compte existe ou non. */
  const sendResetLink = async (e: FormEvent) => {
    e.preventDefault();
    if (!looksLikeEmail(email) || busy) return;
    setErr("");
    setBusy(true);
    const result = await requestPasswordReset(email.trim());
    setBusy(false);
    if (result.ok) setStep("sent");
    else setErr(result.reason === "rate_limited" ? tooManyAttempts(result.retryAfter) : "Envoi impossible pour le moment. Réessayez.");
  };

  const cur = step === "2fa" ? 2 : step === "ok" ? 3 : 1;
  const steps: AuthStep[] = [
    [1, step === "register" ? "Inscription" : "Connexion", "Vos identifiants"],
    [2, "Vérification", "Confirmez votre identité"],
    [3, "Accès", "Bienvenue sur Ciphera"],
  ];
  const back = () => {
    setErr("");
    setStep("login");
  };

  return (
    <AuthLayout steps={steps} current={cur} {...(step === "register" ? SIGNUP_COPY : {})}>
      {expired && step === "login" && (
        <div role="status" style={{ display: "flex", alignItems: "flex-start", gap: 10, padding: "12px 14px", borderRadius: 12, border: "1px solid var(--border-default)", background: "var(--surface-raised)", font: "400 13px/1.45 var(--font-sans)", color: "var(--text-body)" }}>
          <Icon name="Clock" size={15} color="var(--text-muted)" style={{ marginTop: 2 }} />
          <span style={{ display: "flex", flexDirection: "column", gap: 4 }}>
            <h2 style={{ margin: 0, font: "600 13px/1.3 var(--font-sans)", color: "var(--text-strong)" }}>Session expirée</h2>
            Votre session a expiré ou a été révoquée. Reconnectez-vous pour continuer.
          </span>
        </div>
      )}

      {passwordReset && !expired && step === "login" && (
        <div role="status" style={{ display: "flex", alignItems: "flex-start", gap: 10, padding: "12px 14px", borderRadius: 12, border: "1px solid var(--border-default)", background: "var(--surface-raised)", font: "400 13px/1.45 var(--font-sans)", color: "var(--text-body)" }}>
          <Icon name="CircleCheck" size={15} color="var(--green-500)" style={{ marginTop: 2 }} />
          <span style={{ display: "flex", flexDirection: "column", gap: 4 }}>
            <h2 style={{ margin: 0, font: "600 13px/1.3 var(--font-sans)", color: "var(--text-strong)" }}>Mot de passe modifié</h2>
            Vos sessions ont été fermées. Connectez-vous avec le nouveau mot de passe.
          </span>
        </div>
      )}

      {verifyEmail && !expired && step === "login" && (
        <div role="status" style={{ display: "flex", alignItems: "flex-start", gap: 10, padding: "12px 14px", borderRadius: 12, border: "1px solid var(--border-default)", background: "var(--surface-raised)", font: "400 13px/1.45 var(--font-sans)", color: "var(--text-body)" }}>
          <Icon name="Mail" size={15} color="var(--green-500)" style={{ marginTop: 2 }} />
          <span style={{ display: "flex", flexDirection: "column", gap: 4 }}>
            <h2 style={{ margin: 0, font: "600 13px/1.3 var(--font-sans)", color: "var(--text-strong)" }}>Vérifiez votre boîte mail</h2>
            Si cette adresse peut s'inscrire, un lien de confirmation vient de lui être envoyé. Ouvrez-le (24 h), puis connectez-vous.
          </span>
        </div>
      )}

      {created && !expired && step === "login" && (
        <div role="status" style={{ display: "flex", alignItems: "flex-start", gap: 10, padding: "12px 14px", borderRadius: 12, border: "1px solid var(--border-default)", background: "var(--surface-raised)", font: "400 13px/1.45 var(--font-sans)", color: "var(--text-body)" }}>
          <Icon name="CircleCheck" size={15} color="var(--green-500)" style={{ marginTop: 2 }} />
          <span style={{ display: "flex", flexDirection: "column", gap: 4 }}>
            <h2 style={{ margin: 0, font: "600 13px/1.3 var(--font-sans)", color: "var(--text-strong)" }}>Compte créé</h2>
            Votre compte est prêt. Connectez-vous pour continuer.
          </span>
        </div>
      )}

      {step === "login" && (
        <form onSubmit={(e) => void submit(e)} noValidate className="login-step" style={{ display: "flex", flexDirection: "column", gap: 16 }}>
          <StepTitle>Connectez-vous à Ciphera</StepTitle>
          <div style={{ height: 4 }} />
          <LoginInput
            label="E-mail"
            type="email"
            autoComplete="username"
            value={email}
            onChange={(v) => {
              setEmail(v);
              setErr("");
            }}
            autoFocus
            error={!!err}
          />
          <LoginInput
            label="Mot de passe"
            type={show ? "text" : "password"}
            autoComplete="current-password"
            value={pwd}
            onChange={(v) => {
              setPwd(v);
              setErr("");
            }}
            error={!!err}
            right={
              <button type="button" onClick={() => setShow((s) => !s)} aria-label={show ? "Masquer le mot de passe" : "Afficher le mot de passe"} style={{ padding: 0, border: 0, background: "transparent", cursor: "pointer", display: "flex" }}>
                <Icon name={show ? "EyeOff" : "Eye"} size={16} color="var(--text-muted)" />
              </button>
            }
          />
          <label style={{ display: "flex", alignItems: "flex-start", gap: 10, cursor: "pointer", font: "400 13px/1.45 var(--font-sans)", color: "var(--text-body)", marginTop: 4 }}>
            <input type="checkbox" checked={remember} onChange={(e) => setRemember(e.target.checked)} style={{ accentColor: "#8b7cf6", width: 16, height: 16, marginTop: 1 }} />
            Rester connecté sur cet appareil pendant 30 jours
          </label>
          {err && <FormError>{err}</FormError>}
          <PrimaryButton busy={busy} disabled={!valid}>
            Se connecter
          </PrimaryButton>
          {passkeys.data && (
            <>
              <MethodsDivider>ou</MethodsDivider>
              <AltMethod icon="KeyRound" title="Clé de sécurité" text="Se connecter sans mot de passe" onClick={() => void passkeyLogin()} />
            </>
          )}
          <div style={{ display: "flex", flexDirection: "column", gap: 10, alignItems: "center", marginTop: 6, font: "400 13px/1 var(--font-sans)", color: "var(--text-muted)" }}>
            <span>
              Vous avez oublié votre{" "}
              <button
                type="button"
                className="login-link"
                onClick={() => {
                  setErr("");
                  setStep("forgot");
                }}
              >
                mot de passe
              </button>{" "}
              ?
            </span>
          </div>
        </form>
      )}

      {step === "register" && (
        <div className="login-step" style={{ display: "flex", flexDirection: "column", alignItems: "center", textAlign: "center", gap: 14 }}>
          <span style={{ width: 48, height: 48, borderRadius: 99, display: "flex", alignItems: "center", justifyContent: "center", background: "var(--surface-selected)" }}>
            <Icon name="MailPlus" size={20} color="var(--text-strong)" />
          </span>
          <StepTitle>Inscription sur invitation</StepTitle>
          <span style={{ font: "400 13px/1.5 var(--font-sans)", color: "var(--text-muted)" }}>Les comptes de cette instance sont créés par invitation. Ouvrez le lien reçu par e-mail, ou demandez une invitation à un administrateur.</span>
          <span style={{ marginTop: 6, font: "400 13px/1 var(--font-sans)", color: "var(--text-muted)" }}>
            Déjà un compte ?{" "}
            <button type="button" className="login-link" onClick={back}>
              Se connecter
            </button>
          </span>
        </div>
      )}

      {step === "forgot" && resetByMail.data === undefined && (
        <div role="status" className="login-step" style={{ display: "flex", flexDirection: "column", alignItems: "center", gap: 14, paddingTop: 40 }}>
          <Icon name="Loader" size={22} color="#a78bfa" className="login-spin" />
        </div>
      )}

      {step === "forgot" && resetByMail.data === true && (
        <form onSubmit={(e) => void sendResetLink(e)} noValidate className="login-step" style={{ display: "flex", flexDirection: "column", gap: 16 }}>
          <StepTitle>Réinitialiser le mot de passe</StepTitle>
          <span style={{ font: "400 13px/1.5 var(--font-sans)", color: "var(--text-muted)", textAlign: "center" }}>Un lien sera envoyé via le serveur SMTP configuré sur cette instance.</span>
          <LoginInput
            label="E-mail"
            type="email"
            autoComplete="username"
            value={email}
            onChange={(v) => {
              setEmail(v);
              setErr("");
            }}
            autoFocus
            error={!!err}
          />
          {err && <FormError>{err}</FormError>}
          <PrimaryButton busy={busy} disabled={!looksLikeEmail(email)}>
            Envoyer le lien
          </PrimaryButton>
          <button type="button" onClick={back} style={{ padding: 0, border: 0, background: "transparent", cursor: "pointer", font: "500 14px/1 var(--font-sans)", color: "var(--text-strong)" }}>
            Retour à la connexion
          </button>
        </form>
      )}

      {step === "forgot" && resetByMail.data === false && (
        <div className="login-step" style={{ display: "flex", flexDirection: "column", gap: 16 }}>
          <StepTitle>Réinitialiser le mot de passe</StepTitle>
          <span style={{ font: "400 13px/1.5 var(--font-sans)", color: "var(--text-muted)", textAlign: "center" }}>L'envoi d'e-mails n'est pas configuré sur cette instance : contactez un administrateur, qui peut vous transmettre un lien de réinitialisation.</span>
          <button type="button" onClick={back} style={{ padding: 0, border: 0, background: "transparent", cursor: "pointer", font: "500 14px/1 var(--font-sans)", color: "var(--text-strong)" }}>
            Retour à la connexion
          </button>
        </div>
      )}

      {step === "sent" && (
        <div className="login-step" style={{ display: "flex", flexDirection: "column", alignItems: "center", textAlign: "center", gap: 14 }}>
          <span style={{ width: 48, height: 48, borderRadius: 99, display: "flex", alignItems: "center", justifyContent: "center", background: "rgba(134,239,172,.12)" }}>
            <Icon name="MailCheck" size={20} color="#86efac" />
          </span>
          <StepTitle>Vérifiez votre boîte mail</StepTitle>
          <span style={{ font: "400 13px/1.5 var(--font-sans)", color: "var(--text-muted)" }}>Si un compte existe pour {email.trim()}, un lien valable 30 minutes vient d'être envoyé.</span>
          <button type="button" onClick={back} style={{ marginTop: 6, padding: 0, border: 0, background: "transparent", cursor: "pointer", color: "#a78bfa", font: "500 13px/1 var(--font-sans)" }}>
            Retour à la connexion
          </button>
        </div>
      )}

      {step === "2fa" && methods && <CodeStep onSubmit={verifyFactor} methods={methods} onBack={back} />}

      {step === "ok" && (
        <div role="status" className="login-step" style={{ display: "flex", flexDirection: "column", alignItems: "center", gap: 14, paddingTop: 40 }}>
          <Icon name="Loader" size={22} color="#a78bfa" className="login-spin" />
          <span style={{ font: "500 14px/1 var(--font-sans)", color: "var(--text-strong)" }}>Ouverture du dashboard…</span>
        </div>
      )}
    </AuthLayout>
  );
}
