import { Button, CodeBlock, CopyButton, Icon, Modal } from "@/design-system";

// Ecran manquant de la maquette (docs/design/screens.md, n° 11) : modale
// standard « jeton créé, copiez-le maintenant », reutilisable (deploy hook,
// jeton d'API, secret de webhook). Construite avec Modal + CodeBlock +
// CopyButton ; le secret n'est garde que dans l'etat du composant parent et
// disparait a la fermeture.

export interface TokenCreatedModalProps {
  title: string;
  description?: string;
  /** Libelle de la valeur (ex. « URL du deploy hook »). */
  label: string;
  /** Secret ou URL secrete, montre une seule fois. */
  value: string;
  /** Exemple d'utilisation (ex. commande curl), copiable. */
  example?: string;
  /** Mise en garde sous l'exemple (ex. voie déconseillée). */
  note?: string;
  onClose: () => void;
}

export function TokenCreatedModal({ title, description, label, value, example, note, onClose }: TokenCreatedModalProps) {
  return (
    <Modal
      title={title}
      description={description}
      onClose={onClose}
      width={560}
      footer={
        <Button variant="inverse" onClick={onClose}>
          J'ai copié le secret
        </Button>
      }
    >
      <div role="alert" style={{ display: "flex", alignItems: "flex-start", gap: 10, padding: "10px 12px", borderRadius: "var(--radius-md)", border: "1px solid rgba(229,180,84,.35)", background: "rgba(229,180,84,.08)", font: "400 12px/1.45 var(--font-sans)", color: "var(--text-body)" }}>
        <Icon name="TriangleExclamationPoint" size={14} color="#e5b454" style={{ marginTop: 1 }} />
        <span>Copiez-le maintenant : il ne sera plus jamais affiché. Ciphera n'en garde qu'une empreinte.</span>
      </div>
      <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
        <span style={{ font: "400 12px/1 var(--font-sans)", color: "var(--text-muted)" }}>{label}</span>
        <div style={{ display: "flex", alignItems: "center", gap: 8, height: 32, padding: "0 2px 0 10px", borderRadius: "var(--radius-md)", border: "1px solid var(--border-strong)", background: "var(--surface-control)", boxShadow: "var(--shadow-control)" }}>
          <span data-testid="token-value" style={{ flex: 1, minWidth: 0, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", font: "400 12px/1 var(--font-mono)", color: "var(--text-strong)" }}>
            {value}
          </span>
          <CopyButton value={value} size={28} label={`Copier : ${label}`} />
        </div>
      </div>
      {example && (
        <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
          <span style={{ font: "400 12px/1 var(--font-sans)", color: "var(--text-muted)" }}>Exemple</span>
          <CodeBlock code={example} />
          {note && <span style={{ font: "400 12px/1.45 var(--font-sans)", color: "var(--text-muted)" }}>{note}</span>}
        </div>
      )}
    </Modal>
  );
}
