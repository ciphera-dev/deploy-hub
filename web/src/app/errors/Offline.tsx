import { Button } from "@/design-system";

import { ErrorScreen } from "./ErrorScreen";

export function Offline({ onRetry }: { onRetry?: () => void }) {
  return (
    <ErrorScreen
      icon="WifiOff"
      title="API injoignable"
      description="Le panneau n'arrive pas à joindre le serveur Ciphera. Vérifiez votre connexion et que le service est démarré, puis réessayez."
    >
      {onRetry && (
        <Button variant="inverse" icon="RotateCw" onClick={onRetry}>
          Réessayer
        </Button>
      )}
    </ErrorScreen>
  );
}
