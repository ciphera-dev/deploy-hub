import type { components } from "@/lib/api/schema";

type S = components["schemas"];

// Donnees factices des Parametres (Pages2.jsx#SettingsV2), ramenees au contrat de l'API.
// Aucun secret : l'API n'en renvoie jamais (mot de passe SMTP : `password_set` seulement).

export const GENERAL: S["GeneralSettings"] = { id: "7f2a4c10-1b2c-4d5e-8f90-a1b2c3d4e5f6", name: "Ciphera", slug: "ciphera", timezone: "Europe/Paris", language: "fr", members: 6, mfa_required: true };

export const DEPLOY: S["DeploySettings"] = { auto: true, retention: 5 };

export const SECURITY: S["OrgSecurity"] = { mfa_required: true, ip_allowlist: [], current_ip: "203.0.113.7", self_enrollment_required: false };

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

const NOW = Math.floor(Date.now() / 1000);

export const TEAM_MEMBERS: S["TeamMember"][] = [
  { id: "00000000-0000-4000-8000-0000000000a1", name: "Alex Martin", email: "alex@ciphera.com", role: "owner", two_factor: true, created_at: NOW - 86_400 * 300, last_active_at: NOW, pending: false },
  { id: "00000000-0000-4000-8000-0000000000a2", name: "Camille Roux", email: "camille@ciphera.com", role: "admin", two_factor: true, created_at: NOW - 86_400 * 200, last_active_at: NOW - 3_600, pending: false },
  { id: "00000000-0000-4000-8000-0000000000a3", name: "Nora Kaci", email: "nora@ciphera.com", role: "developer", two_factor: false, created_at: NOW - 86_400 * 40, last_active_at: NOW - 86_400 * 2, pending: false },
];

export const TEAM_INVITATIONS: S["PendingInvitation"][] = [
  { id: "00000000-0000-4000-8000-0000000000b1", email: "lea@ciphera.com", name: "Léa", role: "viewer", invited_by: "Alex Martin", created_at: NOW - 7_200, expires_at: NOW + 86_400 * 3 },
];
