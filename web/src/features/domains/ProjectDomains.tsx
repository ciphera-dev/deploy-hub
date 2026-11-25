import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useState, type CSSProperties, type ReactNode } from "react";

import cloudflareMark from "@/assets/images/cloudflare/cloudflare-mark.png";
import { Badge, Button, CopyButton, Icon, IconButton, Menu, Modal, Select, StatusDot, Switch, useToast, type IconName, type MenuItem } from "@/design-system";
import { fetchMe, ME_QUERY_KEY, MFA_PENDING } from "@/features/auth/api";
import type { Project } from "@/features/projects/api";

import { createDomain, daysLeft, deleteDomain, dnsRecords, domainsQueryKey, fetchDomains, formatDay, isApex, makePrimary, renewDomain, setGeneratedDomain, setRedirect, updateDomain, verifyDomain, type Domain, type ProjectDomains as Domains } from "./api";
import { HttpOnlyBadge } from "./HttpOnlyBadge";

// Port de prototypes/dashboard/ProjectDomains.jsx (structure, libellés, tokens).
// Écarts imposés par le backend v0.1 :
// - pas de garde Cloudflare : l'écran s'affiche directement ; le connecteur
//   Cloudflare est annoncé pour la v0.2 (bandeau de ProjectCloudflare.jsx) ;
// - DNS : enregistrement A (ou AAAA) vers l'IP du nœud, vérifié par le
//   serveur (« Vérifier le DNS ») ; pas de CNAME de bordure ;
// - certificat émis par le proxy du nœud (ACME HTTP-01, Let's Encrypt),
//   renouvelé à J-30 ; le domaine généré (sslip.io) est servi en HTTP ;
// - « Retirer le préfixe » et le choix du code de redirection ne sont pas
//   disponibles (redirection 308 seulement) ;
// - « Continuer » enregistre le domaine (DNS à vérifier) ; « Retour » le
//   supprime ; le port interne du service est requis pour un service ;
// - le domaine généré se désactive au lieu d'être supprimé ;
// - écriture réservée aux rôles développeur et plus (viewer : lecture).

const hair = "1px solid var(--border-subtle)";
const lbl: CSSProperties = { font: "400 12px/1 var(--font-sans)", color: "var(--text-subtle)" };
const COLS = "minmax(0,1.6fr) minmax(0,1fr) minmax(0,1.1fr) 150px 96px";
const DNS_COLS = "80px 140px minmax(0,1fr) 110px";
const CF = "#f38020";
const WRITERS = ["developer", "ops", "admin", "owner"];

const mono = (v: ReactNode, c?: string) => <span style={{ fontFamily: "var(--font-mono)", fontSize: 12, color: c }}>{v}</span>;

/** Rôle du compte chargé par le shell, sans nouvelle requête (voir useCanDeploy). */
function useRole(): string | null {
  const me = useQuery({ queryKey: ME_QUERY_KEY, queryFn: fetchMe, retryOnMount: false, refetchOnMount: false });
  return me.data && me.data !== MFA_PENDING ? me.data.role : null;
}

function dnsDot(status: Domain["dns_status"]): { status: "ok" | "error" | "pending"; label: string } {
  if (status === "ok") return { status: "ok", label: "Propagé" };
  if (status === "error") return { status: "error", label: "Incorrect" };
  return { status: "pending", label: "En attente" };
}

function DnsTable({ host, nodeIp, nodeIp6, dns }: { host: string; nodeIp: string | null | undefined; nodeIp6?: string | null; dns: Domain["dns_status"] }) {
  const records = dnsRecords(host, nodeIp, nodeIp6);
  const dot = dnsDot(dns);
  return (
    <div style={{ border: "1px solid var(--border-default)", borderRadius: "var(--radius-lg)", overflow: "hidden" }}>
      <div style={{ display: "grid", gridTemplateColumns: DNS_COLS, gap: 12, padding: "10px 14px", borderBottom: hair, ...lbl }}>
        <span>Type</span>
        <span>Nom</span>
        <span>Valeur</span>
        <span style={{ textAlign: "right" }}>État</span>
      </div>
      {records.length === 0 ? (
        <div style={{ padding: "10px 14px", font: "400 12px/1.5 var(--font-sans)", color: "var(--text-muted)" }}>IP publique du nœud inconnue : définissez CIPHERA_PUBLIC_IP sur le serveur.</div>
      ) : (
        records.map((r, i) => (
          <div key={i} style={{ display: "grid", gridTemplateColumns: DNS_COLS, gap: 12, alignItems: "center", padding: "8px 14px", borderTop: i ? hair : "none" }}>
            {mono(r.type, "var(--text-strong)")}
            <span style={{ display: "flex", alignItems: "center", gap: 4 }}>
              {mono(r.host)}
              <CopyButton value={r.host} label="Copier le nom" size={24} />
            </span>
            <span style={{ display: "flex", alignItems: "center", gap: 4, minWidth: 0 }}>
              <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{mono(r.value)}</span>
              <CopyButton value={r.value} label="Copier la valeur" size={24} />
            </span>
            <span style={{ justifySelf: "end" }}>
              <StatusDot status={dot.status} label={dot.label} />
            </span>
          </div>
        ))
      )}
    </div>
  );
}

function tlsState(d: Domain): { status: "ok" | "error" | "pending"; label: string } {
  if (d.generated) return d.dns_status === "ok" && d.internal_port ? { status: "ok", label: "HTTP (domaine généré)" } : { status: "pending", label: "En attente du port" };
  // DNS devenu incorrect (revérifié avant chaque émission) : domaine hors ligne, même avec un certificat encore valide.
  if (d.dns_status === "error") return { status: "error", label: "DNS incorrect" };
  if (d.cert_status === "active") return { status: "ok", label: "HTTPS actif" };
  if (d.cert_status === "issuing") return { status: "pending", label: "Émission du certificat…" };
  if (d.cert_status === "error") return { status: "error", label: "Échec du certificat" };
  return { status: "pending", label: "En attente du DNS" };
}

/** Message après un DNS correct : l'émission n'est annoncée que si elle est demandée. */
function certAfterCheck(d: Domain | null): string {
  if (d?.cert_status === "issuing") return "DNS propagé. Le certificat HTTPS est en cours d'émission.";
  if (d?.cert_status === "active") return "DNS propagé. Certificat HTTPS actif.";
  return "DNS propagé.";
}

type Act = "verify" | "primary" | "redirect" | "unredirect" | "renew" | "edit" | "delete" | "generated-off";

function DomainRow({ d, nodeIp, nodeIp6, open, onToggle, act, busy, canWrite }: { d: Domain; nodeIp: string | null | undefined; nodeIp6?: string | null; open: boolean; onToggle: () => void; act: (k: Act, d: Domain) => void; busy: boolean; canWrite: boolean }) {
  const [h, setH] = useState(false);
  const tls = tlsState(d);
  const days = daysLeft(d.cert_expires_at);
  const pct = days != null ? Math.max(4, Math.min(100, (days / 90) * 100)) : 0;
  const icon: IconName = d.redirect ? "CornerDownRight" : d.generated ? "Sparkles" : d.path_prefix !== "/" ? "Route" : "Globe";
  const scheme = d.generated ? "http://" : "https://";
  const items: MenuItem[] = [
    { icon: "ExternalLink", label: "Ouvrir", onSelect: () => window.open(scheme + d.host + (d.path_prefix === "/" ? "" : d.path_prefix), "_blank", "noopener,noreferrer") },
  ];
  if (canWrite) {
    if (!d.primary && !d.redirect && !d.generated) items.push({ icon: "Star", label: "Définir comme principal", onSelect: () => act("primary", d) });
    if (d.redirect) items.push({ icon: "Undo2", label: "Retirer la redirection", onSelect: () => act("unredirect", d) });
    else if (!d.primary && !d.generated) items.push({ icon: "CornerDownRight", label: "Rediriger vers le principal", onSelect: () => act("redirect", d) });
    // Renouvellement manuel : seulement à 30 jours ou moins de l'expiration (le serveur refuse avant).
    if (!d.generated && d.dns_status === "ok" && (days == null || days <= 30)) items.push({ icon: "RotateCw", label: "Renouveler le certificat", onSelect: () => act("renew", d) });
    items.push({ icon: "Pencil", label: "Modifier", onSelect: () => act("edit", d) });
    items.push("separator");
    if (d.generated) items.push({ icon: "Trash", label: "Désactiver le domaine généré", onSelect: () => act("generated-off", d), danger: true });
    else items.push({ icon: "Trash", label: "Supprimer le domaine", onSelect: () => act("delete", d), danger: true });
  }
  return (
    <div data-testid={`domain-${d.host}`} style={{ borderBottom: hair, background: open || h ? "var(--surface-raised)" : "transparent", transition: "background var(--dur-fast)" }} onMouseEnter={() => setH(true)} onMouseLeave={() => setH(false)}>
      <div onClick={onToggle} style={{ display: "grid", gridTemplateColumns: COLS, gap: 16, alignItems: "center", padding: "16px 12px", cursor: "pointer" }}>
        <div style={{ display: "flex", alignItems: "center", gap: 12, minWidth: 0 }}>
          <span style={{ width: 30, height: 30, borderRadius: 8, flex: "none", display: "flex", alignItems: "center", justifyContent: "center", background: "var(--surface-selected)", color: "var(--text-body)" }}>
            <Icon name={icon} size={14} />
          </span>
          <div style={{ display: "flex", flexDirection: "column", gap: 6, minWidth: 0 }}>
            <div style={{ display: "flex", alignItems: "center", gap: 8, minWidth: 0 }}>
              <span style={{ font: "500 14px/1.1 var(--font-sans)", color: "var(--text-strong)", whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>
                {d.host}
                {d.path_prefix !== "/" && <span style={{ color: "var(--text-muted)" }}>{d.path_prefix}</span>}
              </span>
              {d.primary && <Badge>Principal</Badge>}
              {d.generated && <Badge>Généré</Badge>}
              {d.generated && <HttpOnlyBadge />}
            </div>
            <span style={{ ...lbl, fontSize: 11 }}>
              {d.redirect ? (
                <>
                  Redirection 308 → <span style={{ color: "var(--text-body)" }}>{d.redirect_to ?? "—"}</span>
                </>
              ) : (
                <>
                  {d.path_prefix !== "/" ? mono(d.path_prefix + "/*", "var(--text-muted)") : "Toutes les routes"} → {mono((d.service_name ?? "—") + ":" + (d.internal_port ?? "—"), "var(--text-muted)")}
                </>
              )}
            </span>
          </div>
        </div>
        <div>
          <StatusDot status={tls.status} label={tls.label} />
        </div>
        <div style={{ display: "flex", flexDirection: "column", gap: 7 }}>
          <span style={{ font: "400 12px/1 var(--font-sans)", color: d.cert_expires_at ? "var(--text-body)" : "var(--text-faint)" }}>{d.cert_expires_at ? "Expire le " + formatDay(d.cert_expires_at) : "—"}</span>
          {d.cert_expires_at && (
            <span style={{ height: 3, width: "80%", borderRadius: 99, background: "var(--viz-4)", overflow: "hidden" }}>
              <span style={{ display: "block", height: "100%", width: pct + "%", background: days != null && days < 20 ? "#e5b454" : "var(--viz-1)" }} />
            </span>
          )}
        </div>
        <span style={{ ...lbl, fontSize: 11 }}>{d.cert_expires_at && days != null ? "Renouv. auto · " + days + " j" : d.cert_status === "issuing" ? "ACME HTTP-01…" : d.generated ? "—" : "Vérification DNS"}</span>
        <div style={{ display: "flex", gap: 2, justifyContent: "flex-end" }} onClick={(e) => e.stopPropagation()}>
          {canWrite && !d.generated && <IconButton size={28} variant="ghost" icon={busy ? "Loader" : "RefreshCw"} label="Vérifier" disabled={busy} onClick={() => act("verify", d)} style={busy ? { animation: "spin 1s linear infinite" } : undefined} />}
          <Menu items={items} label={`Actions pour ${d.host}`} width={230} />
        </div>
      </div>
      {open && (
        <div style={{ padding: "0 12px 20px 54px", display: "grid", gridTemplateColumns: "minmax(0,1.5fr) minmax(0,1fr)", gap: 28 }}>
          <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
            <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between" }}>
              <span style={{ font: "500 13px/1 var(--font-sans)", color: "var(--text-strong)" }}>Configuration DNS</span>
              {d.dns_status !== "ok" && <span style={lbl}>Ajoutez ces enregistrements chez votre registrar</span>}
            </div>
            {d.generated ? <span style={{ font: "400 12px/1.5 var(--font-sans)", color: "var(--text-muted)" }}>Domaine fourni par Ciphera, aucune configuration nécessaire.</span> : <DnsTable host={d.host} nodeIp={nodeIp} nodeIp6={nodeIp6} dns={d.dns_status} />}
            {d.dns_error && <span style={{ font: "400 12px/1.5 var(--font-sans)", color: "#f87171" }}>{d.dns_error}</span>}
          </div>
          <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
            <span style={{ font: "500 13px/1 var(--font-sans)", color: "var(--text-strong)" }}>Certificat</span>
            <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
              {[
                ["Émetteur", d.cert_status === "active" || d.cert_expires_at ? (d.cert_issuer ?? "—") : "—"],
                ["Géré par", d.generated ? "Aucun (HTTP)" : "Proxy Ciphera (ACME)"],
                ["Protocoles", d.generated ? "HTTP/1.1" : "TLS 1.2 · TLS 1.3"],
                ["HSTS", "Désactivé"],
              ].map(([k, v]) => (
                <div key={k} style={{ display: "flex", justifyContent: "space-between", font: "400 12px/1 var(--font-sans)" }}>
                  <span style={{ color: "var(--text-muted)" }}>{k}</span>
                  <span style={{ color: "var(--text-strong)" }}>{v}</span>
                </div>
              ))}
              {d.cert_error && <span style={{ font: "400 12px/1.5 var(--font-sans)", color: "#f87171" }}>{d.cert_error}</span>}
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

const seg = (on: boolean): CSSProperties => ({ flex: 1, display: "flex", alignItems: "center", justifyContent: "center", gap: 8, height: 30, border: 0, borderRadius: 7, cursor: "pointer", background: on ? "var(--surface-selected)" : "transparent", color: on ? "var(--text-strong)" : "var(--text-muted)", font: "500 13px/1 var(--font-sans)", boxShadow: on ? "inset 0 1px 0 rgba(255,255,255,.04)" : "none" });
const box: CSSProperties = { display: "flex", alignItems: "center", height: 40, borderRadius: "var(--radius-md)", border: "1px solid var(--border-strong)", background: "var(--surface-control)", boxShadow: "var(--shadow-control)", overflow: "hidden" };
const inp: CSSProperties = { flex: 1, minWidth: 0, height: "100%", padding: "0 12px", background: "transparent", border: 0, outline: "none", color: "var(--text-strong)", font: "400 13px/1 var(--font-mono)" };
const affix: CSSProperties = { display: "flex", alignItems: "center", height: "100%", padding: "0 10px", font: "400 12px/1 var(--font-mono)", color: "var(--text-faint)", whiteSpace: "nowrap" };

function Section({ n, title, hint, children }: { n: string; title: string; hint?: string | null; children: ReactNode }) {
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
      <div style={{ display: "flex", alignItems: "baseline", gap: 10 }}>
        <span style={{ font: "500 11px/1 var(--font-mono)", color: "var(--text-faint)" }}>{n}</span>
        <span style={{ font: "500 13px/1 var(--font-sans)", color: "var(--text-strong)" }}>{title}</span>
        {hint && <span style={{ ...lbl, marginLeft: "auto" }}>{hint}</span>}
      </div>
      {children}
    </div>
  );
}

const HOST_RE = /^([a-z0-9]([a-z0-9-]*[a-z0-9])?\.)+[a-z]([a-z0-9-]*[a-z0-9])?$/;
const normPath = (p: string) => "/" + (p || "").replace(/^\/+/, "").replace(/\s+/g, "-").toLowerCase();

type Form = { name: string; mode: "service" | "redirect"; svc: string; port: string; path: string; www: boolean };

function RouteForm({ fixedName, form, set, services, primary }: { fixedName?: boolean; form: Form; set: (f: Partial<Form>) => void; services: string[]; primary: string | null }) {
  const host = form.name.trim().toLowerCase() || "app.exemple.com";
  const valid = fixedName || HOST_RE.test(form.name.trim());
  const apex = valid && isApex(host);
  const sub = form.path === "/" ? "" : form.path;
  const opts = [form.mode === "service" && form.path !== "/" && "strip", !fixedName && apex && "www"].filter(Boolean) as string[];
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 24 }}>
      <Section n="01" title="Adresse" hint={fixedName ? null : "Domaine que vous possédez"}>
        <div style={{ ...box, borderColor: form.name && !valid ? "rgba(239,68,68,.5)" : "var(--border-strong)" }}>
          <span style={{ ...affix, paddingRight: 0 }}>
            <Icon name="Lock" size={12} color="var(--text-faint)" />
          </span>
          <span style={{ ...affix, paddingLeft: 6, paddingRight: 0 }}>{"https://"}</span>
          {fixedName ? (
            <span style={{ ...affix, color: "var(--text-strong)", padding: "0 2px", fontSize: 13 }}>{host}</span>
          ) : (
            <input autoFocus aria-label="Domaine" value={form.name} onChange={(e) => set({ name: e.target.value.toLowerCase().replace(/^https?:\/\//, "").replace(/\/.*$/, "") })} placeholder="app.exemple.com" style={{ ...inp, padding: "0 2px", flex: "1.6 1 0" }} />
          )}
          <span style={{ width: 1, height: 20, background: "var(--border-strong)" }} />
          <input aria-label="Chemin" value={form.path} onChange={(e) => set({ path: normPath(e.target.value) })} placeholder="/" title="Chemin" disabled={form.mode === "redirect"} style={{ ...inp, flex: fixedName ? 1 : "1 1 0", color: form.path === "/" ? "var(--text-muted)" : "var(--text-strong)" }} />
          {!fixedName && <span style={{ ...affix, paddingLeft: 0 }}>{valid ? <Icon name="CircleCheck" size={15} color="var(--green-500)" /> : form.name ? <Icon name="CircleExclamationPoint" size={15} color="#f87171" /> : null}</span>}
        </div>
        <span style={{ font: "400 11px/1.4 var(--font-sans)", color: form.name && !valid ? "#f87171" : "var(--text-faint)" }}>{form.name && !valid ? "Nom de domaine invalide." : form.path === "/" ? "Laissez « / » pour tout le domaine, ou saisissez un chemin comme /api pour ne router qu'une partie du trafic." : "Seules les requêtes " + host + form.path + "/* sont concernées."}</span>
      </Section>
      <Section n="02" title="Destination">
        <div style={{ display: "flex", gap: 2, padding: 2, borderRadius: 10, background: "var(--surface-control)", border: "1px solid var(--border-default)" }}>
          <button type="button" onClick={() => set({ mode: "service" })} style={seg(form.mode === "service")}>
            <Icon name="Box" size={14} />
            Service du projet
          </button>
          <button type="button" onClick={() => set({ mode: "redirect", path: "/" })} disabled={!primary && !fixedName} title={!primary ? "Ajoutez d'abord le domaine principal" : undefined} style={seg(form.mode === "redirect")}>
            <Icon name="CornerDownRight" size={14} />
            Redirection
          </button>
        </div>
        <div style={{ display: "grid", gridTemplateColumns: "minmax(0,1fr) 28px minmax(0,1fr)", alignItems: "center", gap: 8, padding: 14, borderRadius: "var(--radius-lg)", border: "1px solid var(--border-default)", background: "var(--gray-0)" }}>
          <div style={{ display: "flex", flexDirection: "column", gap: 8, minWidth: 0 }}>
            <span style={{ ...lbl, fontSize: 11 }}>Requête entrante</span>
            <div style={{ ...box, height: 36, background: "transparent", boxShadow: "none", padding: "0 12px" }}>
              <span style={{ font: "400 12px/1 var(--font-mono)", color: "var(--text-body)", whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>
                {host}
                <span style={{ color: "var(--text-strong)" }}>{sub}</span>
                <span style={{ color: "var(--text-faint)" }}>{"/*"}</span>
              </span>
            </div>
          </div>
          <span style={{ display: "flex", justifyContent: "center", paddingTop: 19 }}>
            <Icon name="ArrowRight" size={14} color="var(--text-muted)" />
          </span>
          {form.mode === "service" ? (
            <div style={{ display: "flex", flexDirection: "column", gap: 8, minWidth: 0 }}>
              <span style={{ ...lbl, fontSize: 11 }}>Service · port</span>
              <div style={{ display: "flex", gap: 6 }}>
                <Select options={services} value={form.svc} onChange={(svc) => set({ svc })} style={{ flex: 1, width: "100%" }} />
                <div style={{ ...box, height: 32, width: 84, flex: "none" }}>
                  <span style={{ ...affix, paddingRight: 0 }}>:</span>
                  <input aria-label="Port interne" value={form.port} onChange={(e) => set({ port: e.target.value.replace(/\D/g, "").slice(0, 5) })} style={{ ...inp, padding: "0 8px 0 4px", fontSize: 12 }} />
                </div>
              </div>
            </div>
          ) : (
            <div style={{ display: "flex", flexDirection: "column", gap: 8, minWidth: 0 }}>
              <span style={{ ...lbl, fontSize: 11 }}>Vers · code</span>
              <div style={{ display: "flex", gap: 6 }}>
                <Select options={[primary ?? "—"]} value={primary ?? "—"} style={{ flex: 1, width: "100%" }} />
                <Select options={["308"]} value="308" />
              </div>
            </div>
          )}
        </div>
      </Section>
      {opts.length > 0 && (
        <Section n="03" title="Options">
          <div style={{ display: "flex", flexDirection: "column", borderRadius: "var(--radius-lg)", border: "1px solid var(--border-default)" }}>
            {opts.map((o, i) => (
              <div key={o} style={{ padding: "14px 16px", borderTop: i ? "1px solid var(--border-subtle)" : "none" }}>
                {o === "strip" ? <Switch checked={false} disabled label="Retirer le préfixe" description={"Bientôt disponible : le service reçoit " + form.path + "/users."} /> : <Switch checked={form.www} onChange={(www) => set({ www })} label={"Ajouter www." + host} description={"Redirigé en 308 vers " + host + "."} />}
              </div>
            ))}
          </div>
        </Section>
      )}
      <div style={{ display: "flex", alignItems: "center", gap: 10, padding: "10px 12px", borderRadius: "var(--radius-md)", background: "var(--surface-raised)", font: "400 12px/1.3 var(--font-mono)", color: "var(--text-muted)", overflow: "hidden", whiteSpace: "nowrap" }}>
        <Icon name="Route" size={13} color="var(--text-subtle)" />
        <span style={{ overflow: "hidden", textOverflow: "ellipsis" }}>
          <span style={{ color: "var(--text-strong)" }}>{"https://" + host + sub + "/users"}</span>
          {"  →  "}
          {form.mode === "service" ? <span style={{ color: "var(--text-strong)" }}>{form.svc + ":" + (form.port || "—") + sub + "/users"}</span> : <span style={{ color: "var(--text-strong)" }}>{"308 https://" + (primary ?? "—") + sub + "/users"}</span>}
        </span>
      </div>
    </div>
  );
}

function AddDomainModal({ project, data, edit, onClose, onDone }: { project: Project; data: Domains; edit?: Domain; onClose: () => void; onDone: (message: string) => void }) {
  const services = project.services.map((s) => s.name);
  const primary = data.domains.find((d) => d.primary)?.host ?? null;
  const knownPort = (svc: string) => data.domains.find((d) => d.service_name === svc && d.internal_port)?.internal_port;
  const firstSvc = edit?.service_name ?? services[0] ?? "";
  const [form, setForm] = useState<Form>({ name: edit?.host ?? "", mode: edit?.redirect ? "redirect" : "service", svc: firstSvc, port: String(edit?.internal_port ?? knownPort(firstSvc) ?? ""), path: edit?.path_prefix ?? "/", www: !edit });
  const set = (f: Partial<Form>) => setForm((x) => ({ ...x, ...f, ...(f.svc && f.svc !== x.svc && !f.port ? { port: String(knownPort(f.svc) ?? "") } : {}) }));
  const [step, setStep] = useState(0);
  const [created, setCreated] = useState<Domain | null>(null);
  const [check, setCheck] = useState<"idle" | "loading" | "ok" | "error">("idle");
  const [dnsError, setDnsError] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [pending, setPending] = useState(false);
  const name = form.name.trim().toLowerCase();
  const valid = HOST_RE.test(name);
  const serviceId = project.services.find((s) => s.name === form.svc)?.id;
  const port = form.port ? Number(form.port) : undefined;

  async function submitEdit() {
    if (!edit) return;
    setPending(true);
    setError(null);
    const r = await updateDomain(edit.id, { path_prefix: form.path, service_id: serviceId ?? null, internal_port: port ?? null });
    setPending(false);
    if (!r.ok) return setError(r.message);
    onDone("Domaine mis à jour");
  }

  async function next() {
    setPending(true);
    setError(null);
    const redirect = form.mode === "redirect";
    const r = await createDomain(project.id, { host: name, path_prefix: redirect ? "/" : form.path, service_id: serviceId ?? null, internal_port: redirect ? null : (port ?? null), redirect, www: form.www && isApex(name) });
    setPending(false);
    if (!r.ok) return setError(r.message);
    setCreated(r.data as Domain);
    setStep(1);
  }

  async function back() {
    if (created) {
      // Domaine en attente : retiré pour revenir au formulaire.
      const r = await deleteDomain(created.id);
      if (!r.ok) return setError(r.message);
      if (form.www && isApex(name)) {
        const www = (await fetchDomains(project.id).catch(() => null))?.domains.find((d) => d.host === "www." + name);
        if (www) await deleteDomain(www.id);
      }
      setCreated(null);
    }
    setCheck("idle");
    setDnsError(null);
    setStep(0);
  }

  async function verify() {
    if (!created) return;
    setCheck("loading");
    const r = await verifyDomain(created.id);
    if (!r.ok) {
      setCheck("error");
      return setDnsError(r.message);
    }
    const d = r.data as Domain;
    setCreated(d);
    setCheck(d.dns_status === "ok" ? "ok" : "error");
    setDnsError(d.dns_error ?? null);
  }

  const errorLine = error && <span role="alert" style={{ font: "400 12px/1.4 var(--font-sans)", color: "#f87171" }}>{error}</span>;

  if (edit)
    return (
      <Modal title="Modifier le domaine" description={edit.host} onClose={onClose} width={520} footer={<><Button onClick={onClose}>Annuler</Button><Button variant="inverse" disabled={pending} onClick={submitEdit}>Enregistrer</Button></>}>
        <div style={{ display: "flex", flexDirection: "column", gap: 16 }}>
          <RouteForm fixedName form={{ ...form, www: false }} set={set} services={services} primary={primary} />
          {errorLine}
        </div>
      </Modal>
    );

  return (
    <Modal
      title="Ajouter un domaine"
      description={step === 0 ? "Pointez un domaine que vous possédez vers ce projet." : "Configurez le DNS de " + name}
      onClose={onClose}
      width={620}
      footer={
        step === 0 ? (
          <>
            <Button onClick={onClose}>Annuler</Button>
            <Button variant="inverse" disabled={!valid || pending} iconRight="ArrowRight" onClick={next}>
              Continuer
            </Button>
          </>
        ) : (
          <>
            <Button icon="ArrowLeft" onClick={back}>
              Retour
            </Button>
            <span style={{ flex: 1 }} />
            <Button onClick={() => onDone(name + " ajouté")}>Vérifier plus tard</Button>
            <Button variant="inverse" disabled={check !== "ok"} onClick={() => onDone(name + " ajouté")}>
              Ajouter le domaine
            </Button>
          </>
        )
      }
    >
      {step === 0 ? (
        <div style={{ display: "flex", flexDirection: "column", gap: 16 }}>
          <RouteForm form={form} set={set} services={services} primary={primary} />
          {errorLine}
        </div>
      ) : (
        <div style={{ display: "flex", flexDirection: "column", gap: 16 }}>
          <DnsTable host={name} nodeIp={data.node_ip} nodeIp6={data.node_ip6} dns={check === "ok" ? "ok" : check === "error" ? "error" : "pending"} />
          <div style={{ display: "flex", alignItems: "center", gap: 12, padding: "12px 14px", borderRadius: "var(--radius-lg)", background: "var(--surface-control)", border: "1px solid var(--border-default)" }}>
            {check === "ok" ? <Icon name="CircleCheck" size={16} color="var(--green-500)" /> : <Icon name={check === "loading" ? "Loader" : check === "error" ? "CircleExclamationPoint" : "Info"} size={16} color={check === "error" ? "#f87171" : "var(--text-muted)"} style={check === "loading" ? { animation: "spin 1s linear infinite" } : undefined} />}
            <span role="status" style={{ flex: 1, font: "400 12px/1.45 var(--font-sans)", color: check === "ok" ? "var(--text-strong)" : "var(--text-muted)" }}>
              {check === "ok" ? certAfterCheck(created) : check === "loading" ? "Interrogation des résolveurs DNS…" : check === "error" ? (dnsError ?? "DNS incorrect.") : "La propagation peut prendre de quelques minutes à 48 h."}
            </span>
            <Button size="sm" disabled={check === "loading"} onClick={verify}>
              {check === "ok" ? "Revérifier" : "Vérifier le DNS"}
            </Button>
          </div>
          {errorLine}
        </div>
      )}
    </Modal>
  );
}

/** Bandeau du connecteur Cloudflare (ProjectCloudflare.jsx), annoncé pour la v0.2. */
function CloudflareSoon() {
  return (
    <div style={{ display: "flex", alignItems: "center", gap: 14, padding: "12px 12px 12px 14px", borderRadius: "var(--radius-lg)", border: "1px solid rgba(243,128,32,.22)", background: "linear-gradient(90deg,rgba(243,128,32,.07),transparent 60%)" }}>
      <span style={{ width: 32, height: 32, borderRadius: 8, flex: "none", display: "flex", alignItems: "center", justifyContent: "center", background: "rgba(243,128,32,.1)", border: "1px solid rgba(243,128,32,.22)" }}>
        <img src={cloudflareMark} alt="Cloudflare" style={{ width: 21, height: "auto", display: "block" }} />
      </span>
      <div style={{ display: "flex", flexDirection: "column", gap: 6, flex: 1, minWidth: 0 }}>
        <span style={{ display: "flex", alignItems: "center", gap: 8, font: "500 13px/1 var(--font-sans)", color: "var(--text-strong)" }}>
          Cloudflare
          <Badge style={{ color: CF }}>v0.2</Badge>
        </span>
        <span style={{ ...lbl, fontSize: 11 }}>Connecteur Cloudflare (DNS automatique, proxy) prévu pour la v0.2. En attendant, créez l'enregistrement DNS chez votre registrar.</span>
      </div>
    </div>
  );
}

export function ProjectDomains({ project }: { project: Project }) {
  const client = useQueryClient();
  const { flash } = useToast();
  const role = useRole();
  const canWrite = role != null && WRITERS.includes(role);
  const key = domainsQueryKey(project.id);
  const query = useQuery({ queryKey: key, queryFn: () => fetchDomains(project.id) });
  const [open, setOpen] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [modal, setModal] = useState<{ edit?: Domain } | null>(null);
  const [confirm, setConfirm] = useState<Domain | null>(null);

  const refresh = () => client.invalidateQueries({ queryKey: key });
  const done = (r: { ok: true } | { ok: false; message: string }, message: string) => {
    flash(r.ok ? message : r.message);
    void refresh();
  };

  async function act(k: Act, d: Domain) {
    if (k === "verify") {
      setBusy(d.id);
      const r = await verifyDomain(d.id);
      setBusy(null);
      if (!r.ok) return done(r, "");
      const v = r.data as Domain;
      done(r, v.dns_status === "ok" ? (v.cert_status === "issuing" ? "DNS vérifié — émission du certificat" : d.host + " : configuration correcte") : (v.dns_error ?? "DNS incorrect"));
    }
    if (k === "primary") done(await makePrimary(d.id), d.host + " est le domaine principal");
    if (k === "redirect") done(await setRedirect(d.id, true), d.host + " redirige vers le domaine principal");
    if (k === "unredirect") done(await setRedirect(d.id, false), "Redirection retirée");
    if (k === "renew") done(await renewDomain(d.id), "Renouvellement demandé");
    if (k === "generated-off") done(await setGeneratedDomain(project.id, false), "Domaine généré désactivé");
    if (k === "edit") setModal({ edit: d });
    if (k === "delete") setConfirm(d);
  }

  const data = query.data;
  if (!data) return query.isError ? <span style={lbl}>Domaines indisponibles pour le moment.</span> : null;
  const list = data.domains;
  const ok = list.filter((x) => x.cert_status === "active").length;

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 18 }}>
      <CloudflareSoon />
      <div style={{ display: "flex", alignItems: "flex-end", justifyContent: "space-between", gap: 16, paddingLeft: 12 }}>
        <div style={{ display: "flex", gap: 28 }}>
          {[
            ["Domaines", list.length],
            ["HTTPS actif", ok + " / " + list.length],
            ["En attente", list.length - ok],
          ].map(([k, v]) => (
            <div key={k} style={{ display: "flex", flexDirection: "column", gap: 8 }}>
              <span style={lbl}>{k}</span>
              <span style={{ font: "500 18px/1 var(--font-sans)", color: "var(--text-strong)" }}>{v}</span>
            </div>
          ))}
        </div>
        {canWrite && (
          <div style={{ display: "flex", gap: 8 }}>
            {!data.generated_domain && data.node_ip && (
              <Button variant="ghost" icon="Sparkles" onClick={async () => done(await setGeneratedDomain(project.id, true), "Domaine généré activé")}>
                Activer le domaine généré
              </Button>
            )}
            <Button variant="inverse" icon="Plus" onClick={() => setModal({})}>
              Ajouter un domaine
            </Button>
          </div>
        )}
      </div>
      <div style={{ borderTop: hair }}>
        <div style={{ display: "grid", gridTemplateColumns: COLS, gap: 16, padding: "12px 12px", borderBottom: hair, ...lbl }}>
          <span>Domaine / chemin</span>
          <span>HTTPS</span>
          <span>Certificat</span>
          <span>Renouvellement</span>
          <span />
        </div>
        {list.length === 0 && <div style={{ padding: "20px 12px", ...lbl }}>Aucun domaine. Ajoutez un domaine que vous possédez.</div>}
        {list.map((d) => (
          <DomainRow key={d.id} d={d} nodeIp={data.node_ip} nodeIp6={data.node_ip6} open={open === d.id} onToggle={() => setOpen((o) => (o === d.id ? null : d.id))} act={act} busy={busy === d.id} canWrite={canWrite} />
        ))}
      </div>
      {modal && (
        <AddDomainModal
          project={project}
          data={data}
          edit={modal.edit}
          onClose={() => {
            setModal(null);
            void refresh();
          }}
          onDone={(m) => {
            setModal(null);
            flash(m);
            void refresh();
          }}
        />
      )}
      {confirm && (
        <Modal
          title="Supprimer le domaine ?"
          description={confirm.host + " cessera immédiatement de servir ce projet."}
          onClose={() => setConfirm(null)}
          width={460}
          footer={
            <>
              <Button onClick={() => setConfirm(null)}>Annuler</Button>
              <Button
                variant="inverse"
                icon="Trash"
                onClick={async () => {
                  const d = confirm;
                  setConfirm(null);
                  done(await deleteDomain(d.id), d.host + " supprimé");
                }}
              >
                Supprimer
              </Button>
            </>
          }
        />
      )}
    </div>
  );
}
