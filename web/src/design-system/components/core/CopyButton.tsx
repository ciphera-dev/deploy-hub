import { useEffect, useRef, useState, type CSSProperties } from "react";
import { IconButton } from "./IconButton";

export interface CopyButtonProps {
  value: string;
  /** Nom accessible, "Copier" par defaut */
  label?: string;
  size?: number;
  /** Appele apres une copie reussie */
  onCopied?: () => void;
  style?: CSSProperties;
}

export function CopyButton({ value, label = "Copier", size = 30, onCopied, style }: CopyButtonProps) {
  const [done, setDone] = useState(false);
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  useEffect(() => () => clearTimeout(timer.current), []);
  const copy = () => {
    const clip = typeof navigator !== "undefined" ? navigator.clipboard : undefined;
    if (!clip) return;
    clip.writeText(value).then(
      () => {
        setDone(true);
        onCopied?.();
        clearTimeout(timer.current);
        timer.current = setTimeout(() => setDone(false), 1500);
      },
      () => undefined,
    );
  };
  return <IconButton variant="ghost" size={size} icon={done ? "Check" : "Copy"} label={label} onClick={copy} style={style} />;
}
