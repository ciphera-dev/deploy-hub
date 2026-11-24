import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { vi } from "vitest";
import { Avatar, Badge, Button, ButtonGroup, CopyButton, Delta, GroupButton, Icon, IconButton, LegendItem, Select } from "@/design-system";

describe("Icon", () => {
  it("rend un SVG masque aux lecteurs d'ecran sans aucune requete reseau", () => {
    const fetchSpy = vi.spyOn(globalThis, "fetch");
    const { container } = render(<Icon name="Calendar" size={20} />);
    const svg = container.querySelector("svg");
    expect(svg).not.toBeNull();
    expect(svg).toHaveAttribute("aria-hidden", "true");
    expect(svg).toHaveAttribute("width", "20");
    expect(fetchSpy).not.toHaveBeenCalled();
    fetchSpy.mockRestore();
  });
});

describe("Button", () => {
  it("expose le role bouton, le libelle et declenche onClick", () => {
    const onClick = vi.fn();
    render(<Button icon="Plus" onClick={onClick}>Ajouter</Button>);
    fireEvent.click(screen.getByRole("button", { name: "Ajouter" }));
    expect(onClick).toHaveBeenCalledTimes(1);
  });
  it("ne declenche pas onClick quand il est desactive", () => {
    const onClick = vi.fn();
    render(<Button disabled onClick={onClick}>Ajouter</Button>);
    fireEvent.click(screen.getByRole("button", { name: "Ajouter" }));
    expect(onClick).not.toHaveBeenCalled();
  });
});

describe("IconButton", () => {
  it("utilise label comme nom accessible", () => {
    render(<IconButton icon="X" label="Fermer" />);
    expect(screen.getByRole("button", { name: "Fermer" })).toBeInTheDocument();
  });
});

describe("GroupButton / ButtonGroup", () => {
  it("rend les boutons du groupe", () => {
    render(<ButtonGroup><GroupButton>Jour</GroupButton><GroupButton>Mois</GroupButton></ButtonGroup>);
    expect(screen.getAllByRole("button")).toHaveLength(2);
  });
});

describe("Badge, Delta, LegendItem", () => {
  it("affichent leur texte", () => {
    render(<><Badge tone="positive">Termine</Badge><Delta value="2,7 %" suffix="sur 30 jours" /><LegendItem label="CPU" /></>);
    expect(screen.getByText("Termine")).toBeInTheDocument();
    expect(screen.getByText("2,7 %")).toBeInTheDocument();
    expect(screen.getByText("sur 30 jours")).toBeInTheDocument();
    expect(screen.getByText("CPU")).toBeInTheDocument();
  });
});

describe("Avatar", () => {
  it("affiche les initiales AM sans src", () => {
    render(<Avatar name="Alex Martin" />);
    expect(screen.getByText("AM")).toBeInTheDocument();
  });
  it("affiche l'image quand src est fourni", () => {
    render(<Avatar name="Alex Martin" src="/a.png" />);
    expect(screen.getByRole("img", { name: "Alex Martin" })).toHaveAttribute("src", "/a.png");
  });
});

describe("CopyButton", () => {
  it("ecrit la valeur dans le presse-papiers", () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
    render(<CopyButton value="secret-123" />);
    fireEvent.click(screen.getByRole("button", { name: "Copier" }));
    expect(writeText).toHaveBeenCalledWith("secret-123");
  });
});

describe("Select", () => {
  it("affiche la valeur courante dans un combobox", () => {
    render(<Select options={["7 jours", "30 jours"]} value="30 jours" />);
    expect(screen.getByRole("combobox")).toHaveTextContent("30 jours");
  });
  it("retombe sur la premiere option sans value", () => {
    render(<Select options={["7 jours", "30 jours"]} />);
    expect(screen.getByRole("combobox")).toHaveTextContent("7 jours");
  });
  it("ouvre la liste et remonte l'option choisie via onChange", async () => {
    const onChange = vi.fn();
    render(<Select options={["7 jours", "30 jours"]} value="7 jours" onChange={onChange} />);
    fireEvent.keyDown(screen.getByRole("combobox"), { key: "Enter" });
    const option = await screen.findByRole("option", { name: "30 jours" });
    fireEvent.click(option);
    await waitFor(() => expect(onChange).toHaveBeenCalledWith("30 jours"));
  });
  it("ne plante pas sans options", () => {
    render(<Select options={[]} />);
    expect(screen.getByRole("combobox")).toBeInTheDocument();
  });
  it("prend la hauteur 30 imposee par ButtonGroup", () => {
    render(<ButtonGroup><Select bare options={["7 jours"]} /></ButtonGroup>);
    expect(screen.getByRole("combobox")).toHaveStyle({ height: "30px" });
  });
});
