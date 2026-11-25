import { Badge } from "@/design-system";

/**
 * Badge d'un domaine servi en HTTP, sans chiffrement (domaine généré
 * `<projet>.<ip>.sslip.io` tant que son certificat n'est pas émis). Réutilisable partout
 * où un domaine est listé ; `title` explique pourquoi.
 */
export function HttpOnlyBadge() {
  return (
    <Badge tone="negative" style={{ cursor: "help" }}>
      <span title="Ce domaine est servi en HTTP : le trafic n'est pas chiffré tant que le certificat n'est pas émis (nouvelle tentative chaque jour). Ajoutez un domaine à vous pour obtenir HTTPS tout de suite.">HTTP, non chiffré</span>
    </Badge>
  );
}
