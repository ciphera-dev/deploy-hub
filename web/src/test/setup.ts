import "@testing-library/jest-dom/vitest";
import { configure } from "@testing-library/react";

// findBy*/waitFor : 5 s au lieu d'1 s, pour le premier rendu d'un fichier
// sous charge (chargement paresseux des routes) ; reste sous testTimeout.
configure({ asyncUtilTimeout: 5_000 });


// jsdom ne fournit pas ces API utilisees par Radix (Select, Menu).
Element.prototype.hasPointerCapture ??= () => false;
Element.prototype.setPointerCapture ??= () => undefined;
Element.prototype.releasePointerCapture ??= () => undefined;
Element.prototype.scrollIntoView ??= () => undefined;
globalThis.ResizeObserver ??= class {
  observe() {}
  unobserve() {}
  disconnect() {}
};
window.scrollTo = () => undefined;
