import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useState, type CSSProperties } from "react";

import { Avatar, Badge, Button, CopyButton, Icon, Menu, Modal, TextField, useToast, type IconName, type MenuItem } from "@/design-system";
import { fetchMe, ME_QUERY_KEY, MFA_PENDING } from "@/features/auth/api";
import { FormError } from "@/features/auth/ui";

import {
  cancelInvitation,
  changeMemberRole,
  createResetLink,
  fetchTeamInvitations,
  fetchTeamMembers,
  inviteMember,
  removeMember,
  resetMemberMfa,
  TEAM_INVITATIONS_QUERY_KEY,
  TEAM_MEMBERS_QUERY_KEY,
  type PendingInvitation,
  type TeamMember,
  type TeamRole,
} from "../api";
import { SectionState } from "../ui";

// Team.jsx#MembersTab : memes cartes, memes colonnes (sans « Projets », que l'API ne porte pas encore).
const hair = "1px solid var(--border-subtle)";
const lbl: CSSProperties = { font: "400 12px/1 var(--font-sans)", color: "var(--text-subtle)" };
const card: CSSProperties = { borderRadius: "var(--radius-xl)", border: "1px solid var(--border-default)", background: "var(--surface-app)" };
const COLS = "minmax(0,1.8fr) 160px 100px 130px 120px";

const ROLES: TeamRole[] = ["owner", "admin", "developer", "ops", "viewer"];
// Team.jsx : RC (couleur, icone) et noms des roles.
const RC: Record<TeamRole, [string, IconName, string]> = {
  owner: ["#e5b454", "Crown", "Propriétaire"],
  admin: ["#a78bfa", "Shield", "Administrateur"],
  developer: ["#7dd3fc", "Code", "Développeur"],
  ops: ["#86efac", "Server", "Ops"],
  viewer: ["#a1a1aa", "Eye", "Lecteur"],
};

const isRole = (r: string): r is TeamRole => r in RC;
const roleName = (r: string) => (isRole(r) ? RC[r][2] : r);

/** Role de l'acteur : `Role::can_manage` cote serveur (un admin ne gere ni admin ni proprietaire). */
export function canManage(actor: string, target: string): boolean {
  if (actor === "owner") return true;
  if (actor === "admin") return target === "ops" || target === "developer" || target === "viewer";
  return false;
}

/** Roles que l'acteur peut attribuer (`Role::can_grant`) : un admin ne cree jamais de proprietaire. */
export function grantableRoles(actor: string): TeamRole[] {
  if (actor === "owner") return ROLES;
  if (actor === "admin") return ROLES.filter((r) => r !== "owner");
  return [];
}

function rgb(hex: string): string {
  const n = parseInt(hex.slice(1), 16);
  return `${(n >> 16) & 255},${(n >> 8) & 255},${n & 255}`;
}

function RoleBadge({ id, chev }: { id: string; chev?: boolean }) {
  const [c, icon] = isRole(id) ? RC[id] : (["#a1a1aa", "User"] as [string, IconName]);
  const r = rgb(c);
  return (
    <span style={{ display: "inline-flex", alignItems: "center", gap: 6, height: 26, padding: "0 9px 0 8px", borderRadius: 99, background: `rgba(${r},.10)`, border: `1px solid rgba(${r},.25)`, font: "500 12px/1 var(--font-sans)", color: c, whiteSpace: "nowrap" }}>
      <Icon name={icon} size={12} color={c} />
      {roleName(id)}
      {chev && <Icon name="ChevronDown" size={12} color={c} />}
    </span>
  );
}

/** Team.jsx#RolePicker : le badge ouvre la liste des seuls roles permis. */
function RolePicker({ member, roles, onChange }: { member: TeamMember; roles: TeamRole[]; onChange: (role: TeamRole) => void }) {
  const items: MenuItem[] = roles.map((r) => ({ icon: r === member.role ? "Check" : RC[r][1], label: RC[r][2], onSelect: () => r !== member.role && onChange(r) }));
  return (
    <Menu
      label={`Changer le rôle de ${member.name || member.email}`}
      align="start"
      width={200}
      items={items}
      trigger={
        <button type="button" aria-label={`Changer le rôle de ${member.name || member.email}`} style={{ padding: 0, border: 0, background: "transparent", cursor: "pointer" }}>
          <RoleBadge id={member.role} chev />
        </button>
      }
    />
  );
}

function ago(ts: number | null | undefined): string {
  if (!ts) return "Jamais connecté";
  const s = Math.max(0, Math.floor(Date.now() / 1000 - ts));
  if (s < 90) return "à l'instant";
  if (s < 3600) return `il y a ${Math.round(s / 60)} min`;
  if (s < 86_400) return `il y a ${Math.round(s / 3600)} h`;
  return `il y a ${Math.round(s / 86_400)} j`;
}

const plural = (n: number, one: string, many: string) => `${n} ${n > 1 ? many : one}`;

/** Le serveur peut repondre par un chemin (`/invite/<jeton>`) : le lien affiche est absolu. */
const absolute = (link: string) => (link.startsWith("/") ? window.location.origin + link : link);

/** Lien a copier : affiche une seule fois (le serveur ne le redonne jamais). */
function LinkModal({ title, description, link, onClose }: { title: string; description: string; link: string; onClose: () => void }) {
  const url = absolute(link);
  return (
    <Modal title={title} description={description} width={520} onClose={onClose} footer={<Button variant="inverse" onClick={onClose}>Terminé</Button>}>
      <div style={{ display: "flex", alignItems: "center", gap: 10, padding: "10px 12px", borderRadius: "var(--radius-md)", border: "1px solid var(--border-default)", background: "var(--gray-0)" }}>
        <Icon name="Link" size={13} color="var(--text-subtle)" />
        <span aria-label="Lien" style={{ flex: 1, minWidth: 0, font: "500 12px/1.4 var(--font-mono)", color: "var(--text-strong)", wordBreak: "break-all" }}>{url}</span>
        <CopyButton value={url} label="Copier le lien" size={28} />
      </div>
      <span style={{ ...lbl, lineHeight: 1.4 }}>Ce lien n'est affiché qu'une seule fois : copiez-le avant de fermer.</span>
    </Modal>
  );
}

function InviteModal({ roles, onClose, onCreated }: { roles: TeamRole[]; onClose: () => void; onCreated: (link: string) => void }) {
  const [email, setEmail] = useState("");
  const [name, setName] = useState("");
  const [role, setRole] = useState<TeamRole>(roles.includes("viewer") ? "viewer" : roles[0]!);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState("");
  const valid = /^\S+@\S+\.\S+$/.test(email.trim()) && !busy;

  const send = async () => {
    if (!valid) return;
    setBusy(true);
    const result = await inviteMember({ email: email.trim().toLowerCase(), name: name.trim() || undefined, role });
    setBusy(false);
    if (!result.ok) {
      setErr(result.message);
      return;
    }
    onCreated(result.data.link);
  };

  return (
    <Modal
      title="Inviter un membre"
      description="L'invité choisit son mot de passe depuis un lien valable 72 heures."
      width={520}
      onClose={onClose}
      footer={
        <>
          <Button onClick={onClose}>Annuler</Button>
          <Button variant="inverse" icon={busy ? "Loader" : "Send"} disabled={!valid} onClick={() => void send()}>
            Envoyer l'invitation
          </Button>
        </>
      }
    >
      <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 12 }}>
        <TextField label="Nom (facultatif)" placeholder="Nora Kaci" value={name} onChange={(v) => { setName(v); setErr(""); }} />
        <TextField label="E-mail" placeholder="nora@ciphera.com" value={email} onChange={(v) => { setEmail(v); setErr(""); }} />
      </div>
      <div role="radiogroup" aria-label="Rôle" style={{ display: "flex", flexDirection: "column", gap: 10 }}>
        <span style={{ font: "400 12px/1 var(--font-sans)", color: "var(--text-muted)" }}>Rôle</span>
        <div style={{ display: "grid", gridTemplateColumns: `repeat(${roles.length},minmax(0,1fr))`, gap: 6 }}>
          {roles.map((r) => {
            const on = role === r;
            return (
              <button key={r} type="button" role="radio" aria-checked={on} onClick={() => setRole(r)} style={{ height: 36, borderRadius: "var(--radius-md)", cursor: "pointer", font: "500 12px/1 var(--font-sans)", border: "1px solid " + (on ? "var(--gray-500)" : "var(--border-strong)"), background: on ? "var(--surface-selected)" : "var(--surface-control)", color: on ? "var(--text-strong)" : "var(--text-muted)" }}>
                {RC[r][2]}
              </button>
            );
          })}
        </div>
      </div>
      {err && <FormError>{err}</FormError>}
    </Modal>
  );
}

function RemoveModal({ member, onClose, onRemoved }: { member: TeamMember; onClose: () => void; onRemoved: (hooks: number, invitations: number) => void }) {
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState("");
  const who = member.name || member.email;
  const go = async () => {
    setBusy(true);
    const result = await removeMember(member.id);
    setBusy(false);
    if (!result.ok) {
      setErr(result.message);
      return;
    }
    onRemoved(result.data.deleted_hooks, result.data.deleted_invitations);
  };
  return (
    <Modal
      title={`Retirer ${who} ?`}
      description="Le compte est supprimé définitivement et ses sessions sont révoquées. Ses déploiements restent dans l'historique. Les deploy hooks et les invitations en attente qu'il a créés sont supprimés avec lui."
      width={460}
      onClose={onClose}
      footer={
        <>
          <Button onClick={onClose}>Annuler</Button>
          <Button variant="inverse" icon={busy ? "Loader" : "Trash"} disabled={busy} onClick={() => void go()}>
            Retirer
          </Button>
        </>
      }
    >
      {err && <FormError>{err}</FormError>}
    </Modal>
  );
}

/** Remise a zero du second facteur : le mot de passe de l'auteur est redemande (re-authentification). */
function ResetMfaModal({ member, onClose, onDone }: { member: TeamMember; onClose: () => void; onDone: () => void }) {
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState("");
  const who = member.name || member.email;
  const go = async () => {
    if (!password || busy) return;
    setBusy(true);
    const result = await resetMemberMfa(member.id, password);
    setBusy(false);
    if (!result.ok) {
      setErr(result.message);
      return;
    }
    onDone();
  };
  return (
    <Modal
      title={`Réinitialiser le 2FA de ${who} ?`}
      description="L'application d'authentification, les passkeys et les codes de secours sont supprimés, et ses sessions sont fermées. Il se reconnecte avec son mot de passe, puis ajoute un nouveau facteur."
      width={460}
      onClose={onClose}
      footer={
        <>
          <Button onClick={onClose}>Annuler</Button>
          <Button variant="inverse" icon={busy ? "Loader" : "ShieldX"} disabled={!password || busy} onClick={() => void go()}>
            Réinitialiser
          </Button>
        </>
      }
    >
      <TextField label="Votre mot de passe" type="password" autoComplete="current-password" value={password} onChange={(v) => { setPassword(v); setErr(""); }} />
      {err && <FormError>{err}</FormError>}
    </Modal>
  );
}

type Dialog =
  | { kind: "invite" }
  | { kind: "reset-mfa"; member: TeamMember }
  | { kind: "remove"; member: TeamMember }
  | { kind: "link"; title: string; description: string; link: string };

function MembersView({ me, members, invitations }: { me: { id: string; role: string }; members: TeamMember[]; invitations: PendingInvitation[] }) {
  const client = useQueryClient();
  const { flash } = useToast();
  const [q, setQ] = useState("");
  const [dialog, setDialog] = useState<Dialog | null>(null);
  const [err, setErr] = useState("");
  const roles = grantableRoles(me.role);
  const list = members.filter((m) => (m.name + " " + m.email).toLowerCase().includes(q.trim().toLowerCase()));

  const refresh = () => Promise.all([client.invalidateQueries({ queryKey: TEAM_MEMBERS_QUERY_KEY }), client.invalidateQueries({ queryKey: TEAM_INVITATIONS_QUERY_KEY })]);

  const setRole = async (m: TeamMember, role: TeamRole) => {
    setErr("");
    const result = await changeMemberRole(m.id, role);
    if (!result.ok) return setErr(result.message);
    flash(`${m.name || m.email} → ${roleName(role)}`);
    await refresh();
  };
  const resetLink = async (m: TeamMember) => {
    setErr("");
    const result = await createResetLink(m.id);
    if (!result.ok) return setErr(result.message);
    setDialog({ kind: "link", title: "Lien de réinitialisation", description: `À transmettre à ${m.name || m.email} : valable 30 minutes, à usage unique.`, link: result.data.link });
  };
  const cancel = async (i: PendingInvitation) => {
    setErr("");
    const result = await cancelInvitation(i.id);
    if (!result.ok) return setErr(result.message);
    flash("Invitation annulée");
    await refresh();
  };

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 14 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
        <div style={{ display: "flex", alignItems: "center", gap: 8, height: 32, padding: "0 10px", width: 260, borderRadius: "var(--radius-md)", border: "1px solid var(--border-strong)", background: "var(--surface-control)" }}>
          <Icon name="Search" size={14} color="var(--text-subtle)" />
          <input aria-label="Rechercher un membre" value={q} onChange={(e) => setQ(e.target.value)} placeholder="Rechercher un membre" style={{ flex: 1, minWidth: 0, background: "transparent", border: 0, outline: "none", color: "var(--text-strong)", font: "400 13px/1 var(--font-sans)" }} />
        </div>
        <span style={{ flex: 1 }} />
        <Button variant="inverse" icon="UserPlus" onClick={() => setDialog({ kind: "invite" })}>
          Inviter
        </Button>
      </div>
      {err && <FormError>{err}</FormError>}

      <div style={card}>
        <div style={{ display: "grid", gridTemplateColumns: COLS, gap: 16, padding: "12px 18px", ...lbl }}>
          <span>Membre</span>
          <span>Rôle</span>
          <span>2FA</span>
          <span>Activité</span>
          <span />
        </div>
        {list.map((m) => {
          const self = m.id === me.id;
          const manage = !self && canManage(me.role, m.role);
          // Compte d'inscription ouverte dont l'adresse n'est pas confirmee : ni role ni lien de reinitialisation.
          const removal: MenuItem = { icon: "Trash", label: "Retirer", danger: true, onSelect: () => setDialog({ kind: "remove", member: m }) };
          const items: MenuItem[] = m.pending
            ? [removal]
            : [
                { icon: "KeyRound", label: "Lien de réinitialisation", onSelect: () => void resetLink(m) },
                { icon: "ShieldX", label: "Réinitialiser le 2FA", onSelect: () => setDialog({ kind: "reset-mfa", member: m }) },
                "separator",
                removal,
              ];
          return (
            <div key={m.id} style={{ display: "grid", gridTemplateColumns: COLS, gap: 16, alignItems: "center", padding: "12px 18px", borderTop: hair }}>
              <div style={{ display: "flex", alignItems: "center", gap: 12, minWidth: 0 }}>
                <Avatar name={m.name || m.email} size={32} />
                <div style={{ display: "flex", flexDirection: "column", gap: 6, minWidth: 0 }}>
                  <span style={{ display: "flex", alignItems: "center", gap: 8, minWidth: 0 }}>
                    <span style={{ font: "500 13px/1 var(--font-sans)", color: "var(--text-strong)", whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>{m.name || m.email}</span>
                    {self && <Badge>Vous</Badge>}
                    {m.pending && <Badge>En attente</Badge>}
                  </span>
                  <span style={{ ...lbl, whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>{m.email}</span>
                </div>
              </div>
              {manage && !m.pending ? <RolePicker member={m} roles={roles} onChange={(r) => void setRole(m, r)} /> : <span><RoleBadge id={m.role} /></span>}
              <span>
                {m.two_factor ? (
                  <span style={{ display: "flex", alignItems: "center", gap: 6, font: "400 12px/1 var(--font-sans)", color: "var(--text-body)" }}>
                    <Icon name="ShieldCheck" size={13} color="var(--green-500)" />
                    Activée
                  </span>
                ) : (
                  <span style={{ display: "flex", alignItems: "center", gap: 6, font: "400 12px/1 var(--font-sans)", color: "#e5b454" }}>
                    <Icon name="ShieldX" size={13} />
                    Inactive
                  </span>
                )}
              </span>
              <span style={{ font: "400 12px/1 var(--font-sans)", color: "var(--text-body)" }}>{ago(m.last_active_at)}</span>
              <div style={{ justifySelf: "end" }}>{manage && <Menu label={`Actions de ${m.name || m.email}`} items={items} />}</div>
            </div>
          );
        })}
        {!list.length && <div style={{ padding: "28px 18px", borderTop: hair, ...lbl }}>Aucun membre ne correspond.</div>}
      </div>

      <div style={card}>
        <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "12px 18px", ...lbl }}>
          <span>Invitations en attente</span>
          <span style={{ fontSize: 11, color: "var(--text-faint)" }}>{invitations.length}</span>
        </div>
        {invitations.map((i) => (
          <div key={i.id} style={{ display: "grid", gridTemplateColumns: COLS, gap: 16, alignItems: "center", padding: "12px 18px", borderTop: hair }}>
            <div style={{ display: "flex", flexDirection: "column", gap: 6, minWidth: 0 }}>
              <span style={{ font: "500 13px/1 var(--font-sans)", color: "var(--text-muted)" }}>Invitation envoyée</span>
              <span style={{ ...lbl, whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>{i.email}</span>
            </div>
            <span><RoleBadge id={i.role} /></span>
            <span style={lbl}>—</span>
            <span style={{ font: "400 12px/1 var(--font-sans)", color: "var(--text-body)" }}>Envoyée {ago(i.created_at)}</span>
            <div style={{ justifySelf: "end" }}>
              {(roles as string[]).includes(i.role) && (
                <Button size="sm" aria-label={`Annuler l'invitation de ${i.email}`} onClick={() => void cancel(i)}>
                  Annuler
                </Button>
              )}
            </div>
          </div>
        ))}
        {!invitations.length && <div style={{ padding: "20px 18px", borderTop: hair, ...lbl }}>Aucune invitation en attente.</div>}
      </div>

      {dialog?.kind === "invite" && (
        <InviteModal
          roles={roles}
          onClose={() => setDialog(null)}
          onCreated={(link) => {
            setDialog({ kind: "link", title: "Invitation créée", description: "Transmettez ce lien à l'invité : il est valable 72 heures et à usage unique.", link });
            void refresh();
          }}
        />
      )}
      {dialog?.kind === "link" && <LinkModal title={dialog.title} description={dialog.description} link={dialog.link} onClose={() => setDialog(null)} />}
      {dialog?.kind === "reset-mfa" && (
        <ResetMfaModal
          member={dialog.member}
          onClose={() => setDialog(null)}
          onDone={() => {
            const who = dialog.member.name || dialog.member.email;
            setDialog(null);
            flash(`2FA de ${who} réinitialisé`);
            void refresh();
          }}
        />
      )}
      {dialog?.kind === "remove" && (
        <RemoveModal
          member={dialog.member}
          onClose={() => setDialog(null)}
          onRemoved={(hooks, invitationsDeleted) => {
            const who = dialog.member.name || dialog.member.email;
            setDialog(null);
            flash(`${who} retiré : ${plural(hooks, "deploy hook supprimé", "deploy hooks supprimés")}, ${plural(invitationsDeleted, "invitation supprimée", "invitations supprimées")}`);
            void refresh();
          }}
        />
      )}
    </div>
  );
}

/** Onglet non disponible : visible mais desactive, badge v0.2 (comme « Nouveau projet »). */
function TabRow({ count }: { count: number }) {
  const tab: CSSProperties = { height: 38, padding: 0, background: "none", border: 0, marginBottom: -1, font: "500 13px/1 var(--font-sans)", display: "flex", alignItems: "center", gap: 6 };
  return (
    <div role="tablist" style={{ display: "flex", gap: 22, borderBottom: "1px solid var(--border-subtle)", paddingLeft: 12 }}>
      <button role="tab" type="button" aria-selected tabIndex={0} style={{ ...tab, borderBottom: "1px solid var(--gray-950)", color: "var(--text-strong)", cursor: "pointer" }}>
        Membres
        <span style={{ font: "400 11px/1 var(--font-sans)", color: "var(--text-faint)" }}>{count}</span>
      </button>
      {["Rôles et permissions", "Journal d'accès"].map((label) => (
        <button key={label} role="tab" type="button" aria-selected={false} aria-disabled disabled style={{ ...tab, color: "var(--text-subtle)", cursor: "not-allowed", opacity: 0.7 }}>
          {label}
          <Badge>v0.2</Badge>
        </button>
      ))}
    </div>
  );
}

/** Parametres › Equipe (Team.jsx#MembersTab) : membres, roles permis, invitations. Roles et journal : v0.2. */
export function TeamSection() {
  const me = useQuery({ queryKey: ME_QUERY_KEY, queryFn: fetchMe, retryOnMount: false, refetchOnMount: false });
  const members = useQuery({ queryKey: TEAM_MEMBERS_QUERY_KEY, queryFn: fetchTeamMembers });
  const invitations = useQuery({ queryKey: TEAM_INVITATIONS_QUERY_KEY, queryFn: fetchTeamInvitations });
  const self = me.data && me.data !== MFA_PENDING ? me.data : null;
  const loaded = members.data && invitations.data && self;
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 14 }}>
      <TabRow count={members.data?.length ?? 0} />
      {loaded ? (
        <MembersView me={{ id: self.id, role: self.role }} members={members.data} invitations={invitations.data} />
      ) : (
        <SectionState error={members.error ?? invitations.error} what="l'équipe" denied="Accès refusé : réservé aux administrateurs." />
      )}
    </div>
  );
}
