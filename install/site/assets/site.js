// Bouton « Copier » des commandes d'installation et bandeau defilant. La
// page fonctionne sans JavaScript : les boutons restent masques (attribut
// hidden) et le bandeau reste statique tant que ce script ne les active pas.
"use strict";
(function () {
  if (!navigator.clipboard || !window.isSecureContext) return;
  var buttons = document.querySelectorAll("button[data-copy]");
  Array.prototype.forEach.call(buttons, function (button) {
    var target = document.getElementById(button.getAttribute("data-copy"));
    if (!target) return;
    var label = button.querySelector(".lbl");
    var status = document.getElementById(button.getAttribute("data-status") || "");
    button.hidden = false;
    button.addEventListener("click", function () {
      navigator.clipboard.writeText(target.textContent.trim()).then(
        function () {
          button.classList.add("done");
          if (label) label.textContent = "Copié";
          if (status) status.textContent = "Commande copiée dans le presse-papiers.";
          window.setTimeout(function () {
            button.classList.remove("done");
            if (label) label.textContent = "Copier";
            if (status) status.textContent = "";
          }, 2000);
        },
        function () {
          if (status) status.textContent = "Copie impossible : sélectionnez la commande à la main.";
        },
      );
    });
  });
})();

// Bandeau des technologies : defilement seulement avec JavaScript et sans
// preference « moins d'animations » ; bouton pause/reprise accessible.
(function () {
  if (window.matchMedia && window.matchMedia("(prefers-reduced-motion: reduce)").matches) return;
  var buttons = document.querySelectorAll("button[data-marquee]");
  Array.prototype.forEach.call(buttons, function (button) {
    var marquee = document.getElementById(button.getAttribute("data-marquee"));
    if (!marquee) return;
    marquee.classList.add("anim");
    button.hidden = false;
    button.addEventListener("click", function () {
      var paused = marquee.classList.toggle("paused");
      button.setAttribute("aria-pressed", paused ? "true" : "false");
      button.textContent = paused ? "Reprendre le défilement" : "Mettre en pause le défilement";
    });
  });
})();

// Menu mobile (details/summary, utilisable sans JavaScript) : se ferme avec
// Echap (le focus revient sur « Menu »), au choix d'un lien et au clic ou au
// focus hors du menu.
(function () {
  var menus = document.querySelectorAll("details.menu");
  Array.prototype.forEach.call(menus, function (menu) {
    var summary = menu.querySelector("summary");
    function close(refocus) {
      if (!menu.open) return;
      menu.open = false;
      if (refocus && summary) summary.focus();
    }
    menu.addEventListener("keydown", function (e) {
      if (e.key === "Escape") {
        e.preventDefault();
        close(true);
      }
    });
    menu.addEventListener("click", function (e) {
      if (e.target.closest && e.target.closest("a")) close(false);
    });
    document.addEventListener("click", function (e) {
      if (!menu.contains(e.target)) close(false);
    });
    menu.addEventListener("focusout", function (e) {
      if (e.relatedTarget && !menu.contains(e.relatedTarget)) close(false);
    });
  });
})();
