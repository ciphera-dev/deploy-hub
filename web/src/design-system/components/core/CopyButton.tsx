import { useEffect, useRef, useState, type CSSProperties } from "react";
import { copyText } from "@/lib/clipboard";
import { IconButton } from "./IconButton";

export interface CopyButtonProps {
  value: string;
  /** Nom accessible, "Copier" par defaut */
  label?: string;
  size?: number;
  /** Appele apres une copie reussie */
  onCopied?: () => void;
  /** Appele quand le navigateur a refuse la copie (ou n'a pas de presse-papiers) */
  onFailed?: () => void;
  style?: CSSProperties;
}

export function CopyButton({ value, label = "Copier", size = 30, onCopied, onFailed, style }: CopyButtonProps) {
  const [done, setDone] = useState(false);
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  useEffect(() => () => clearTimeout(timer.current), []);
  const copy = () => {
    void copyText(value).then((ok) => {
      if (!ok) return onFailed?.();
      setDone(true);
      onCopied?.();
      clearTimeout(timer.current);
      timer.current = setTimeout(() => setDone(false), 1500);
    });
  };
  return <IconButton variant="ghost" size={size} icon={done ? "Check" : "Copy"} label={label} onClick={copy} style={style} />;
}
