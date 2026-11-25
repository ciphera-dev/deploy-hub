import { Badge } from "@/design-system";

/**
 * Badge d'un domaine servi en HTTP, sans chiffrement (domaine généré
 * `<projet>.<ip>.sslip.io` : aucun certificat n'est émis). Réutilisable partout
 * où un domaine est listé ; `title` explique pourquoi.
 */
export function HttpOnlyBadge() {
  return (
    <Badge tone="negative" style={{ cursor: "help" }}>
      <span title="Ce domaine est servi en HTTP : le trafic n'est pas chiffré et aucun certificat n'est émis. Ajoutez un domaine à vous pour obtenir HTTPS.">HTTP, non chiffré</span>
    </Badge>
  );
}
