import { Panel } from "@/design-system";

/** Emplacement d'un ecran livre par une tache suivante : garde la route, le titre et l'entree de menu valides. */
export function ScreenPlaceholder({ title }: { title: string }) {
  return (
    <div style={{ padding: "0 32px 32px" }}>
      <Panel title={title} icon="Construction">
        <p style={{ margin: 0, font: "400 13px/1.5 var(--font-sans)", color: "var(--text-muted)" }}>Cet écran sera branché sur les données du serveur dans une prochaine étape.</p>
      </Panel>
    </div>
  );
}
