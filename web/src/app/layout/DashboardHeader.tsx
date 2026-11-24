import type { ReactNode } from "react";

import { Button, ButtonGroup, GroupButton, IconButton, Select } from "@/design-system";

export interface DashboardHeaderProps {
  title: string;
  /** Remplace les controles de periode (ex. un bouton « Nouveau projet ») */
  actions?: ReactNode;
  /** Controles de periode de la maquette : affiches seulement si `periods` est fourni */
  periods?: string[];
  period?: string;
  onPeriodChange?: (period: string) => void;
  range?: string;
}

export function DashboardHeader({ title, actions, periods, period, onPeriodChange, range }: DashboardHeaderProps) {
  return (
    <header style={{ display: "flex", alignItems: "center", justifyContent: "space-between", padding: "24px 30px 22px 32px" }}>
      <h1 style={{ margin: 0, font: "500 24px/1.2 var(--font-sans)", color: "var(--text-strong)", letterSpacing: "-0.01em", whiteSpace: "nowrap" }}>{title}</h1>
      {actions ? (
        <div style={{ display: "flex", gap: 8, alignItems: "center" }}>{actions}</div>
      ) : (
        periods && (
          <div style={{ display: "flex", gap: 16, alignItems: "center" }}>
            <ButtonGroup>
              <Select bare options={periods} value={period} onChange={onPeriodChange} />
              {range && <GroupButton icon="Calendar">{range}</GroupButton>}
            </ButtonGroup>
            <div style={{ display: "flex", gap: 8 }}>
              <Button icon="SlidersHorizontal">Personnaliser</Button>
              <IconButton icon="Ellipsis" label="Plus" />
            </div>
          </div>
        )
      )}
    </header>
  );
}
