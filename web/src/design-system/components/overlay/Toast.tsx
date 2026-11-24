import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState, type CSSProperties, type ReactNode } from "react";
import { Icon } from "../core/Icon";

export interface ToastProps {
  message: string;
  style?: CSSProperties;
}

/** Pastille de confirmation fixee en bas a droite (annoncee aux lecteurs d'ecran). */
export function Toast({ message, style }: ToastProps) {
  return (
    <div
      role="status"
      style={{ position: "fixed", bottom: 24, right: 24, zIndex: 60, padding: "10px 14px", borderRadius: "var(--radius-md)", background: "var(--surface-tooltip)", boxShadow: "var(--shadow-tooltip)", font: "400 12px/1 var(--font-sans)", color: "var(--text-strong)", display: "flex", alignItems: "center", gap: 8, ...style }}
    >
      <Icon name="Check" size={12} color="var(--green-500)" />
      {message}
    </div>
  );
}

interface ToastContextValue {
  /** Affiche un message ; il disparait apres `duration` ms (2200 par defaut). */
  flash: (message: string, duration?: number) => void;
}

const ToastContext = createContext<ToastContextValue | null>(null);

export function ToastProvider({ children }: { children: ReactNode }) {
  const [message, setMessage] = useState("");
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  useEffect(() => () => clearTimeout(timer.current), []);
  const flash = useCallback((m: string, duration = 2200) => {
    setMessage(m);
    clearTimeout(timer.current);
    timer.current = setTimeout(() => setMessage(""), duration);
  }, []);
  const value = useMemo(() => ({ flash }), [flash]);
  return (
    <ToastContext.Provider value={value}>
      {children}
      {message && <Toast message={message} />}
    </ToastContext.Provider>
  );
}

export function useToast(): ToastContextValue {
  const ctx = useContext(ToastContext);
  if (!ctx) throw new Error("useToast doit etre utilise dans un <ToastProvider>");
  return ctx;
}
