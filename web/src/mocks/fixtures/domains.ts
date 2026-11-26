import type { components } from "@/lib/api/schema";

import { PROJECT_SUMMARIES } from "./projects";

type Domain = components["schemas"]["Domain"];
type ProjectDomains = components["schemas"]["ProjectDomains"];

// Donnees de prototypes/dashboard/ProjectDomains.jsx (`INIT`), ramenees au
// contrat de l'API v0.1 : DNS A vers l'IP du noeud, certificat ACME du proxy,
// domaine genere sslip.io servi en HTTP. Donnees factices (IP de
// documentation RFC 5737) pour MSW et les tests.

const [WEB] = PROJECT_SUMMARIES;
const SERVICE = WEB.id.replace(/1$/, "b");
export const NODE_IP = "203.0.113.10";
const T = "2026-05-15T10:00:00.000Z";
const d = (n: number, v: Partial<Domain> & Pick<Domain, "host">): Domain => ({
  id: `00000000-0000-4000-8000-0000000000${(0xd0 + n).toString(16)}`,
  path_prefix: "/",
  generated: false,
  primary: false,
  redirect: false,
  redirect_to: null,
  service_id: SERVICE,
  service_name: WEB.name,
  internal_port: 3000,
  dns_status: "ok",
  dns_error: null,
  cert_status: "active",
  cert_expires_at: "2026-08-13T10:00:00.000Z",
  cert_issuer: "Let's Encrypt R11",
  cert_error: null,
  created_at: T,
  ...v,
});

export const WEB_DOMAINS: ProjectDomains = {
  node_ip: NODE_IP,
  node_ip6: null,
  generated_domain: true,
  domains: [
    d(1, { host: "app.example.com", primary: true }),
    d(2, { host: "example.com", redirect: true, redirect_to: "app.example.com", service_id: null, service_name: null, internal_port: null }),
    d(3, { host: "api.example.com", path_prefix: "/v1", dns_status: "error", dns_error: "api.example.com résout vers 198.51.100.7, attendu 203.0.113.10.", cert_status: "none", cert_expires_at: null, cert_issuer: null }),
    d(4, { host: "ciphera-web.203.0.113.10.sslip.io", generated: true, cert_status: "none", cert_expires_at: null, cert_issuer: null }),
  ],
};

export const emptyDomains = (): ProjectDomains => ({ node_ip: NODE_IP, node_ip6: null, generated_domain: false, domains: [] });
