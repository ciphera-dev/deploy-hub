import { useState, type CSSProperties } from "react";
import { Icon } from "../core/Icon";

export interface WorkspaceSwitcherProps {
  name?: string;
  /** avatar seul */
  collapsed?: boolean;
  onClick?: () => void;
  style?: CSSProperties;
}

/** Largeur maximale de la pilule : au-dela, le nom est tronque (nom complet en infobulle). */
const MAX_WIDTH = 240;

/**
 * Pilule compacte (maquette dashboard/Workspace.jsx) : elle prend la largeur de son contenu,
 * jamais celle de l'en-tete qui la contient.
 */
export function WorkspaceSwitcher({ name = "Ciphera", collapsed = false, onClick, style }: WorkspaceSwitcherProps) {
  const [h, setH] = useState(false);
  return (
    <button
      type="button"
      title={onClick ? name : `${name} : changement d'espace bientôt disponible`}
      aria-label={name}
      aria-disabled={onClick ? undefined : true}
      onClick={onClick}
      onMouseEnter={() => setH(true)}
      onMouseLeave={() => setH(false)}
      style={{
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        gap: 8,
        flex: "0 1 auto",
        width: "fit-content",
        maxWidth: MAX_WIDTH,
        minWidth: 0,
        height: 30,
        padding: collapsed ? "0 6px" : "0 10px 0 6px",
        borderRadius: 99,
        background: h ? "rgba(10,10,14,.7)" : "var(--glass-bg)",
        backdropFilter: "blur(10px)",
        WebkitBackdropFilter: "blur(10px)",
        border: "1px solid var(--glass-border)",
        color: "var(--text-strong)",
        font: "500 12px/1 var(--font-sans)",
        cursor: onClick ? "pointer" : "default",
        ...style,
      }}
    >
      <span style={{ width: 18, height: 18, borderRadius: 99, background: "var(--grad-brand)", flex: "none" }} />
      {!collapsed && (
        <>
          <span style={{ minWidth: 0, overflow: "hidden", textOverflow: "ellipsis", textAlign: "left", whiteSpace: "nowrap" }}>{name}</span>
          <Icon name="ChevronsUpDown" size={12} color="var(--text-subtle)" />
        </>
      )}
    </button>
  );
}
