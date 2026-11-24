import type { CSSProperties, SVGAttributes } from "react";
import { icons } from "lucide-react";

export type IconName = keyof typeof icons;

export interface IconProps extends Omit<SVGAttributes<SVGSVGElement>, "name" | "color" | "style"> {
  /** Nom d'icone lucide-react en PascalCase (ex. "LayoutGrid", "Calendar", "Megaphone") */
  name: IconName;
  /** px, 16 par defaut */
  size?: number;
  /** Couleur CSS, currentColor par defaut */
  color?: string;
  style?: CSSProperties;
}

/** Icone rendue depuis le paquet lucide-react embarque : aucune requete reseau. */
export function Icon({ name, size = 16, color = "currentColor", style, ...rest }: IconProps) {
  const Glyph = icons[name];
  return <Glyph aria-hidden="true" {...rest} size={size} color={color} style={{ display: "inline-block", flex: "none", ...style }} />;
}
