import { Outlet, useMatches, useRouter, useRouterState } from "@tanstack/react-router";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Dialog } from "radix-ui";
import { useEffect, useRef, useState } from "react";

import { Forbidden } from "@/app/errors/Forbidden";
import { Offline } from "@/app/errors/Offline";
import { ServerError } from "@/app/errors/ServerError";
import { MUTATION_ERROR_MESSAGES, fatalError, mutationErrors, useFatalError } from "@/app/errors/fatal-error-store";
import { useToast } from "@/design-system";
import { fetchMe, logout, ME_QUERY_KEY, MFA_PENDING } from "@/features/auth/api";

import { AppHeader } from "./AppHeader";
import { AppSidebar } from "./AppSidebar";
import { MobileTabBar } from "./MobileTabBar";
import { MOBILE_QUERY, TABLET_QUERY, useMedia } from "./useMedia";
import { DashboardHeader } from "./DashboardHeader";

// Fond du theme par defaut (maquette, cipheraThemeBg) : cinq lueurs radiales sur #050505.
const PAGE_BACKGROUND = [
  "radial-gradient(38% 60% at 8% 0%,rgba(124,77,255,.28),transparent 70%)",
  "radial-gradient(30% 55% at 52% -10%,rgba(56,189,248,.16),transparent 70%)",
  "radial-gradient(34% 60% at 96% 4%,rgba(249,115,22,.22),transparent 70%)",
  "radial-gradient(40% 50% at 70% 110%,rgba(236,72,153,.14),transparent 70%)",
  "radial-gradient(35% 50% at 0% 100%,rgba(34,192,138,.10),transparent 70%)",
  "#050505",
].join(",");

// Affiche seulement si GET /api/me echoue autrement que par un 401 (ecran d'erreur plein page).
const UNKNOWN_USER = { name: "Compte", email: "" };

function isEditable(target: EventTarget | null): boolean {
  return target instanceof HTMLElement && (target.isContentEditable || ["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName));
}

export function AppShell() {
  const router = useRouter();
  const queryClient = useQueryClient();
  const fatal = useFatalError();
  const pathname = useRouterState({ select: (s) => s.location.pathname });
  const title = useMatches({ select: (m) => m.findLast((x) => x.staticData.title)?.staticData.title });
  const mobile = useMedia(MOBILE_QUERY);
  // Rail d'icones force en tablette ; au-dela, c'est la preference de l'utilisateur (repli explicite).
  const tablet = useMedia(TABLET_QUERY);
  const [preferred, setPreferred] = useState<boolean | null>(null);
  const collapsed = preferred ?? tablet;
  const toggleCollapsed = () => setPreferred(!collapsed);
  const moreRef = useRef<HTMLButtonElement>(null);
  // Feuille « Plus » (mobile) : ouverte pour une page donnee, donc fermee d'office des qu'on navigue
  // ou que l'ecran s'elargit.
  const [sheetPath, setSheetPath] = useState<string | null>(null);
  const drawerOpen = mobile && sheetPath === pathname;
  const setDrawerOpen = (open: boolean) => setSheetPath(open ? pathname : null);
  const { flash } = useToast();
  const me = useQuery({ queryKey: ME_QUERY_KEY, queryFn: fetchMe });
  // Session connue une fois (donnee ou erreur) : un rechargement ulterieur de
  // `me` (statut repasse a « pending », par exemple quand une page qui
  // l'observe se monte apres un echec) ne demonte plus le shell ; sinon la
  // page se demonte, se remonte a l'echec et relance la requete, en boucle.
  const [settled, setSettled] = useState(false);
  if (!settled && !me.isPending) setSettled(true);
  const signOut = useMutation({
    mutationFn: logout,
    onSuccess: async () => {
      await router.navigate({ to: "/login" });
      // Plus aucune donnee de la session ne doit rester en cache.
      queryClient.clear();
    },
  });

  // Pas de session (401) : direction la connexion ; second facteur attendu :
  // sa verification ; 2FA imposee par l'organisation et absente : l'enrolement.
  const redirect = me.data === null ? "/login" : me.data === MFA_PENDING ? "/2fa" : me.data?.mfa_enrollment_required ? "/2fa-setup" : null;
  useEffect(() => {
    if (redirect) void router.navigate({ to: redirect, replace: true });
  }, [redirect, router]);

  // Une mutation en echec ne demonte jamais la page : message ephemere seulement.
  useEffect(() => mutationErrors.subscribe((kind) => flash(MUTATION_ERROR_MESSAGES[kind], 5000)), [flash]);

  // ⌘B (macOS) ou Ctrl+B replie la barre laterale.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && !e.altKey && !e.shiftKey && e.key.toLowerCase() === "b" && !isEditable(e.target)) {
        e.preventDefault();
        setPreferred(!collapsed);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [collapsed]);

  // Une erreur plein ecran ne survit pas a un changement de page.
  useEffect(() => {
    fatalError.clear();
  }, [pathname]);

  const retry = () => {
    fatalError.clear();
    void queryClient.invalidateQueries();
  };

  // Rien du tableau de bord n'est rendu avant de connaitre la session.
  if ((me.isPending && !settled) || redirect || me.data === MFA_PENDING) return null;
  const user = me.data ? { name: me.data.name || me.data.email, email: me.data.email, avatar: me.data.avatar_url ?? undefined } : UNKNOWN_USER;

  let body = <Outlet />;
  if (fatal === "forbidden") body = <Forbidden onBack={() => void router.navigate({ to: "/" })} />;
  else if (fatal === "offline") body = <Offline onRetry={retry} />;
  else if (fatal === "server") body = <ServerError onRetry={retry} />;

  return (
    <div style={{ display: "flex", flexDirection: "column", height: "100dvh", background: PAGE_BACKGROUND }}>
      <AppHeader user={user} collapsed={collapsed} mobile={mobile} onToggle={toggleCollapsed} onLogout={() => signOut.mutate()} />
      <div style={{ flex: 1, minHeight: 0, display: "flex", margin: mobile ? "0 4px 0" : "0 8px 8px", borderRadius: mobile ? 16 : 20, border: "1px solid var(--border-default)", overflow: "hidden", background: "var(--surface-app)" }}>
        {mobile ? (
          <Dialog.Root open={drawerOpen} onOpenChange={setDrawerOpen}>
            <Dialog.Portal>
              <Dialog.Overlay className="app-sheet__overlay" />
              <Dialog.Content
                className="app-sheet"
                aria-describedby={undefined}
                onCloseAutoFocus={(e) => {
                  // Safari/iOS ne rend pas toujours le focus au declencheur : on le fait nous-memes.
                  e.preventDefault();
                  moreRef.current?.focus();
                }}
              >
                <span className="app-sheet__grip" aria-hidden="true" />
                <Dialog.Title className="sr-only">Plus de navigation</Dialog.Title>
                <AppSidebar user={user} collapsed={false} fill navLabel="Navigation complète" onNavigate={() => setDrawerOpen(false)} onLogout={() => signOut.mutate()} />
              </Dialog.Content>
            </Dialog.Portal>
          </Dialog.Root>
        ) : (
          <div style={{ flex: "none", height: "100%", background: "var(--surface-sidebar)", borderRight: "1px solid var(--border-subtle)", overflow: "auto" }}>
            <AppSidebar user={user} collapsed={collapsed} onLogout={() => signOut.mutate()} />
          </div>
        )}
        <main style={{ flex: 1, minWidth: 0, overflowY: "auto", overflowX: "hidden" }}>
          <div style={{ maxWidth: 1180, margin: "0 auto", padding: "14px 8px 0" }}>
            {title && !fatal && <DashboardHeader title={title} />}
            {body}
          </div>
        </main>
      </div>
      {mobile && <MobileTabBar moreRef={moreRef} moreOpen={drawerOpen} onMore={() => setDrawerOpen(true)} />}
    </div>
  );
}
