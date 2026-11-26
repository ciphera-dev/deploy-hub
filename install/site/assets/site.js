// Bouton « Copier » des commandes d'installation. La page fonctionne sans
// JavaScript : le bouton reste masque (attribut hidden) tant que ce script
// ne l'active pas, et seulement si l'API presse-papiers est disponible.
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
