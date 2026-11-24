import type { ReactNode } from "react";
import { Dialog, DialogContent, DialogDescription, DialogTitle } from "@/components/ui/dialog";
import { IconButton } from "../core/IconButton";

export interface ModalProps {
  open?: boolean;
  title: ReactNode;
  description?: ReactNode;
  children?: ReactNode;
  /** boutons alignes a droite sous un filet */
  footer?: ReactNode;
  /** affiche le bouton de fermeture (x) ; appele aussi au clic sur l'overlay et sur Echap */
  onClose?: () => void;
  width?: number;
  /** rendu sans overlay (pour les apercus) */
  inline?: boolean;
}

export function Modal({ open = true, title, description, children, footer, onClose, width = 520, inline = false }: ModalProps) {
  if (!open) return null;
  const boxStyle = {
    width,
    maxWidth: "100%",
    maxHeight: inline ? undefined : "calc(100vh - 80px)",
    overflow: "auto",
    background: "var(--surface-sidebar)",
    border: "1px solid var(--border-strong)",
    borderRadius: "var(--radius-xl)",
    boxShadow: "0 24px 60px rgba(0,0,0,.6)",
    display: "flex",
    flexDirection: "column",
    gap: 0,
    padding: 0,
  } as const;
  const titleStyle = { margin: 0, font: "500 16px/1.2 var(--font-sans)", color: "var(--text-strong)" } as const;
  const descStyle = { margin: 0, font: "400 12px/1.45 var(--font-sans)", color: "var(--text-subtle)" } as const;
  const sections = (Title: typeof DialogTitle | "div", Desc: typeof DialogDescription | "div") => (
    <>
      <div style={{ display: "flex", alignItems: "flex-start", justifyContent: "space-between", gap: 16, padding: "20px 20px 0 24px" }}>
        <div style={{ display: "flex", flexDirection: "column", gap: 8, paddingTop: 4 }}>
          <Title style={titleStyle}>{title}</Title>
          {description && <Desc style={descStyle}>{description}</Desc>}
        </div>
        {onClose && <IconButton variant="ghost" size={28} icon="X" label="Fermer" onClick={onClose} />}
      </div>
      <div style={{ padding: "22px 24px", display: "flex", flexDirection: "column", gap: 18 }}>{children}</div>
      {footer && <div style={{ display: "flex", justifyContent: "flex-end", gap: 8, padding: "14px 24px", borderTop: "1px solid var(--border-default)" }}>{footer}</div>}
    </>
  );

  if (inline) {
    return (
      <div role="dialog" aria-modal="true" aria-label={typeof title === "string" ? title : undefined} style={boxStyle}>
        {sections("div", "div")}
      </div>
    );
  }
  return (
    <Dialog open onOpenChange={(o) => { if (!o) onClose?.(); }}>
      <DialogContent
        showCloseButton={false}
        overlayStyle={{ zIndex: 100, background: "rgba(0,0,0,.62)" }}
        {...(description ? {} : { "aria-describedby": undefined })}
        style={{ ...boxStyle, zIndex: 101, maxWidth: "calc(100% - 80px)" }}
      >
        {sections(DialogTitle, DialogDescription)}
      </DialogContent>
    </Dialog>
  );
}
