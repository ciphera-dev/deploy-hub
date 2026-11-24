import { fireEvent, render, screen } from "@testing-library/react";
import { vi } from "vitest";
import { RadioCard, Switch, TextField } from "@/design-system";

describe("TextField", () => {
  it("associe le libelle au champ et remonte la valeur", () => {
    const onChange = vi.fn();
    render(<TextField label="Domaine" hint="FQDN" onChange={onChange} />);
    fireEvent.change(screen.getByLabelText("Domaine"), { target: { value: "ciphera.app" } });
    expect(onChange).toHaveBeenCalledWith("ciphera.app");
    expect(screen.getByText("FQDN")).toBeInTheDocument();
  });
});

describe("Switch", () => {
  it("expose role switch et bascule via onChange", () => {
    const onChange = vi.fn();
    render(<Switch checked={false} onChange={onChange} label="Alertes" description="Par e-mail" />);
    const sw = screen.getByRole("switch", { name: "Alertes" });
    expect(sw).toHaveAttribute("aria-checked", "false");
    fireEvent.click(sw);
    expect(onChange).toHaveBeenCalledWith(true);
    expect(screen.getByText("Par e-mail")).toBeInTheDocument();
  });
  it("ne bascule pas quand il est desactive", () => {
    const onChange = vi.fn();
    render(<Switch checked disabled onChange={onChange} />);
    fireEvent.click(screen.getByRole("switch"));
    expect(onChange).not.toHaveBeenCalled();
  });
});

describe("RadioCard", () => {
  it("affiche titre et description et declenche onClick", () => {
    const onClick = vi.fn();
    render(<RadioCard icon="Server" title="Auto-heberge" description="Votre VPS" selected onClick={onClick} />);
    fireEvent.click(screen.getByRole("button", { name: /Auto-heberge/ }));
    expect(onClick).toHaveBeenCalled();
    expect(screen.getByText("Votre VPS")).toBeInTheDocument();
  });
});
