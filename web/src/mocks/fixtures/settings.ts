import type { components } from "@/lib/api/schema";

type S = components["schemas"];

// Donnees factices des Parametres (Pages2.jsx#SettingsV2), ramenees au contrat de l'API.
// Aucun secret : l'API n'en renvoie jamais (mot de passe SMTP : `password_set` seulement).

export const GENERAL: S["GeneralSettings"] = { id: "7f2a4c10-1b2c-4d5e-8f90-a1b2c3d4e5f6", name: "Ciphera", slug: "ciphera", timezone: "Europe/Paris", language: "fr", members: 6, mfa_required: true };

export const DEPLOY: S["DeploySettings"] = { auto: true, retention: 5 };

export const SECURITY: S["OrgSecurity"] = { mfa_required: true, ip_allowlist: [], current_ip: "203.0.113.7" };

export const DANGER: S["DangerZone"] = { name: "Ciphera", projects: 4, members: 6, github_app: null };

export const PANEL_DOMAIN: S["PanelDomain"] = { domain: null, status: "none", error: null };

export const NOTIFICATIONS: S["NotificationSettings"] = {
  configured: true,
  enabled: true,
  host: "smtp.exemple.com",
  port: 587,
  tls: "starttls",
  username: "ciphera@exemple.com",
  password_set: true,
  from_address: "ciphera@exemple.com",
  from_name: "Ciphera",
  public_url: "https://panel.exemple.com",
  email_available: true,
};
