import type { CSSProperties } from "react";

export interface AvatarProps {
  name: string;
  /** URL de l'image ; initiales si absent */
  src?: string;
  /** Diametre en px, 32 par defaut */
  size?: number;
  style?: CSSProperties;
}

export function initials(name: string): string {
  const parts = name.trim().split(/\s+/).filter(Boolean);
  if (parts.length === 0) return "";
  const first = parts[0][0];
  const last = parts.length > 1 ? parts[parts.length - 1][0] : "";
  return (first + last).toUpperCase();
}

export function Avatar({ name, src, size = 32, style }: AvatarProps) {
  if (src) {
    return <img src={src} alt={name} style={{ width: size, height: size, borderRadius: 99, objectFit: "cover", display: "block", flex: "none", ...style }} />;
  }
  return (
    <span
      role="img"
      aria-label={name}
      style={{ width: size, height: size, borderRadius: 99, flex: "none", display: "flex", alignItems: "center", justifyContent: "center", background: "linear-gradient(135deg,oklch(0.78 0.09 250),oklch(0.62 0.12 280))", color: "#fff", font: "600 " + size * 0.38 + "px/1 var(--font-sans)", ...style }}
    >
      <span aria-hidden="true">{initials(name)}</span>
    </span>
  );
}
