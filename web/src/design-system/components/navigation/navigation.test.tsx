import { fireEvent, render, screen } from "@testing-library/react";
import { vi } from "vitest";
import { WorkspaceSwitcher } from "@/design-system";

describe("WorkspaceSwitcher", () => {
  it("est une pilule compacte : jamais etiree sur la largeur de l'en-tete", () => {
    render(
      <div style={{ display: "flex", width: 1650 }}>
        <WorkspaceSwitcher name="Ciphera" />
      </div>,
    );
    const btn = screen.getByRole("button", { name: /Ciphera/ });
    expect(btn.style.width).toBe("fit-content");
    expect(btn.style.width).not.toBe("100%");
    expect(btn.style.maxWidth).toBe("240px");
    expect(btn.style.height).toBe("30px");
    expect(btn.style.borderRadius).toBe("99px");
  });
  it("tronque un nom long avec ellipse et garde le nom complet en infobulle", () => {
    const name = "Une entreprise au nom vraiment tres long pour un en-tete";
    render(<WorkspaceSwitcher name={name} />);
    const btn = screen.getByRole("button", { name: new RegExp(name) });
    expect(btn).toHaveAttribute("title", expect.stringContaining(name));
    const label = screen.getByText(name);
    expect(label.style.textOverflow).toBe("ellipsis");
    expect(label.style.overflow).toBe("hidden");
  });
  it("replie en avatar seul avec un nom accessible", () => {
    render(<WorkspaceSwitcher name="Ciphera" collapsed />);
    expect(screen.getByRole("button", { name: "Ciphera" })).toBeInTheDocument();
  });
  it("declenche onClick", () => {
    const onClick = vi.fn();
    render(<WorkspaceSwitcher onClick={onClick} />);
    fireEvent.click(screen.getByRole("button"));
    expect(onClick).toHaveBeenCalledTimes(1);
  });
});
