import { render, screen, within } from "@testing-library/react";
import { expect, it } from "vitest";

import { DEPLOYMENTS } from "@/mocks/fixtures/deployments";

import { DeploymentTimeline } from "./DeploymentTimeline";

const byNumber = (n: number) => DEPLOYMENTS.find((d) => d.number === n)!;

function stages() {
  const list = screen.getByRole("list", { name: "Étapes du déploiement" });
  return within(list)
    .getAllByRole("listitem")
    .map((li) => ({ text: li.textContent ?? "", state: li.getAttribute("data-state") }));
}

it("réussi : les 6 étapes dans l'ordre, toutes terminées, avec leur durée", () => {
  render(<DeploymentTimeline deployment={byNumber(4818)} />);
  const items = stages();
  expect(items).toHaveLength(6);
  expect(items.map((i) => i.state)).toEqual(Array(6).fill("done"));
  const labels = ["Clonage", "Build", "Image", "Démarrage", "Healthcheck", "Bascule"];
  items.forEach((item, i) => expect(item.text).toContain(labels[i]));
  expect(items[1].text).toContain("62 s");
});

it("échec : l'étape en échec est rouge, les suivantes grisées, la cause est annoncée", () => {
  render(<DeploymentTimeline deployment={byNumber(4807)} />);
  expect(stages().map((i) => i.state)).toEqual(["done", "failed", "pending", "pending", "pending", "pending"]);
  expect(screen.getByRole("alert")).toHaveTextContent("Build : pnpm install a échoué (ERR_PNPM_LOCKFILE)");
});

it("échec présumé sans étape en échec : l'étape ouverte est close en échec avec la cause du déploiement", () => {
  const presumed = { ...byNumber(4821), status: "failed" as const, error: "Aucune nouvelle de l'agent depuis 10 minutes.", finished_at: "2026-11-22T12:10:00.000Z" };
  render(<DeploymentTimeline deployment={presumed} />);
  expect(stages().map((i) => i.state)).toEqual(["done", "failed", "pending", "pending", "pending", "pending"]);
  expect(screen.getByRole("alert")).toHaveTextContent("Aucune nouvelle de l'agent depuis 10 minutes.");
});

it("en cours : l'étape courante tourne, les autres attendent", () => {
  render(<DeploymentTimeline deployment={byNumber(4821)} />);
  expect(stages().map((i) => i.state)).toEqual(["done", "running", "pending", "pending", "pending", "pending"]);
  expect(screen.queryByRole("alert")).not.toBeInTheDocument();
});

it("annulé : l'étape interrompue est marquée annulée", () => {
  render(<DeploymentTimeline deployment={byNumber(4809)} />);
  // Un rollback commence à « Démarrage » : Clonage, Build et Image ne sont pas passées.
  expect(stages().map((i) => i.state)).toEqual(["pending", "pending", "pending", "cancelled", "pending", "pending"]);
});
