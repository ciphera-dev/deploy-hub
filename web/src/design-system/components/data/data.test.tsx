import { fireEvent, render, screen } from "@testing-library/react";
import { vi } from "vitest";
import { Bar, DataTable, Panel, Spark, Stat, Tabs, Tile, type Column } from "@/design-system";

interface Row {
  name: string;
  status: string;
}
const columns: Column<Row>[] = [
  { key: "name", label: "Nom" },
  { key: "status", label: "Statut" },
];

describe("DataTable", () => {
  it("affiche 5 lignes squelette en chargement", () => {
    const { container } = render(<DataTable columns={columns} rows={[]} loading empty="Rien" />);
    expect(container.querySelectorAll('[data-skeleton="true"]')).toHaveLength(5);
    expect(screen.queryByText("Rien")).toBeNull();
  });
  it("affiche empty quand rows est vide", () => {
    render(<DataTable columns={columns} rows={[]} empty="Aucun projet" />);
    expect(screen.getByText("Aucun projet")).toBeInTheDocument();
  });
  it("rend les lignes et appelle onRowClick", () => {
    const onRowClick = vi.fn();
    const rows = [{ name: "api", status: "ok" }];
    render(<DataTable columns={columns} rows={rows} onRowClick={onRowClick} />);
    fireEvent.click(screen.getByText("api"));
    expect(onRowClick).toHaveBeenCalledWith(rows[0]);
  });
});

describe("Bar", () => {
  it("utilise le ton danger a 95 % quand il est donne", () => {
    render(<Bar value={95} max={100} tone="danger" />);
    const bar = screen.getByRole("progressbar");
    expect(bar).toHaveAttribute("data-tone", "danger");
    expect(bar).toHaveAttribute("aria-valuenow", "95");
    expect((bar.firstChild as HTMLElement).style.width).toBe("95%");
    expect((bar.firstChild as HTMLElement).style.background).toContain("--text-negative");
  });
});

describe("Spark", () => {
  it("produit un path non vide pour 3 points", () => {
    const { container } = render(<Spark points={[1, 3, 2]} />);
    const d = container.querySelector("path")?.getAttribute("d") ?? "";
    expect(d.length).toBeGreaterThan(0);
    expect(d.startsWith("M")).toBe(true);
  });
});

describe("Tile", () => {
  it("affiche libelle, valeur, variation et indication", () => {
    render(<Tile label="MRR" value="12 k" delta={-2.5} hint="30 jours" />);
    expect(screen.getByText("MRR")).toBeInTheDocument();
    expect(screen.getByText("12 k")).toBeInTheDocument();
    expect(screen.getByText(/2,5/)).toBeInTheDocument();
    expect(screen.getByText("30 jours")).toBeInTheDocument();
  });
});

describe("Stat et Panel", () => {
  it("rendent leur contenu", () => {
    render(
      <Panel title="Resume" icon="Wallet">
        <Stat label="Total" value="42" />
      </Panel>,
    );
    expect(screen.getByText("Resume")).toBeInTheDocument();
    expect(screen.getByText("42")).toBeInTheDocument();
  });
});

describe("Tabs", () => {
  const tabs = ["a", { value: "b", label: "B", count: 3 }, "c"];
  it("marque l'onglet actif et appelle onChange au clic sur un autre onglet", () => {
    const onChange = vi.fn();
    render(<Tabs tabs={tabs} value="a" onChange={onChange} />);
    expect(screen.getByRole("tab", { name: "a" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByRole("tab", { name: /B/ })).toHaveAttribute("aria-selected", "false");
    fireEvent.click(screen.getByRole("tab", { name: /B/ }));
    expect(onChange).toHaveBeenCalledWith("b");
  });
  it("ArrowRight donne le focus a l'onglet suivant", () => {
    const onChange = vi.fn();
    render(<Tabs tabs={tabs} value="a" onChange={onChange} />);
    fireEvent.keyDown(screen.getByRole("tablist"), { key: "ArrowRight" });
    expect(onChange).toHaveBeenCalledWith("b");
    expect(document.activeElement).toBe(screen.getByRole("tab", { name: /B/ }));
  });
  it("ArrowLeft depuis le premier onglet revient au dernier", () => {
    const onChange = vi.fn();
    render(<Tabs tabs={tabs} value="a" onChange={onChange} />);
    fireEvent.keyDown(screen.getByRole("tablist"), { key: "ArrowLeft" });
    expect(onChange).toHaveBeenCalledWith("c");
    expect(document.activeElement).toBe(screen.getByRole("tab", { name: "c" }));
  });
});
