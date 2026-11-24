import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { vi } from "vitest";
import { Menu, Modal, ToastProvider, useToast } from "@/design-system";

describe("Modal", () => {
  it("expose un dialogue nomme par son titre", () => {
    render(<Modal title="Supprimer" description="Action definitive">corps</Modal>);
    expect(screen.getByRole("dialog", { name: "Supprimer" })).toBeInTheDocument();
    expect(screen.getByText("Action definitive")).toBeInTheDocument();
  });
  it("se ferme sur Echap", () => {
    const onClose = vi.fn();
    render(<Modal title="Supprimer" onClose={onClose}>corps</Modal>);
    fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(1);
  });
  it("se ferme avec le bouton Fermer", () => {
    const onClose = vi.fn();
    render(<Modal title="Supprimer" onClose={onClose}>corps</Modal>);
    fireEvent.click(screen.getByRole("button", { name: "Fermer" }));
    expect(onClose).toHaveBeenCalled();
  });
  it("ne rend rien quand open est faux", () => {
    render(<Modal open={false} title="Supprimer">corps</Modal>);
    expect(screen.queryByRole("dialog")).toBeNull();
  });
  it("en mode inline rend le dialogue sans overlay", () => {
    render(<Modal inline title="Apercu">corps</Modal>);
    expect(screen.getByRole("dialog")).toBeInTheDocument();
  });
});

describe("Menu", () => {
  it("se navigue au clavier et declenche l'action choisie", async () => {
    const copy = vi.fn();
    const del = vi.fn();
    render(<Menu items={[{ icon: "Copy", label: "Copier", onSelect: copy }, "separator", { icon: "Trash", label: "Supprimer", onSelect: del, danger: true }]} />);
    const trigger = screen.getByRole("button", { name: "Actions" });
    fireEvent.keyDown(trigger, { key: "ArrowDown" });
    const items = screen.getAllByRole("menuitem");
    expect(items).toHaveLength(2);
    expect(items[0]).toHaveFocus();
    fireEvent.keyDown(items[0], { key: "ArrowDown" });
    await waitFor(() => expect(items[1]).toHaveFocus());
    fireEvent.keyDown(items[1], { key: "Enter" });
    expect(del).toHaveBeenCalledTimes(1);
    expect(copy).not.toHaveBeenCalled();
  });
});

function Demo() {
  const { flash } = useToast();
  return <button onClick={() => flash("Commande copiee")}>go</button>;
}

describe("Toast", () => {
  it("affiche le message puis le retire", () => {
    vi.useFakeTimers();
    render(<ToastProvider><Demo /></ToastProvider>);
    fireEvent.click(screen.getByText("go"));
    expect(screen.getByRole("status")).toHaveTextContent("Commande copiee");
    act(() => { vi.advanceTimersByTime(2300); });
    expect(screen.queryByRole("status")).toBeNull();
    vi.useRealTimers();
  });
});
