import { Button } from "@/design-system";

import { ErrorScreen } from "./ErrorScreen";

export function ServerError({ onRetry }: { onRetry?: () => void }) {
  return (
    <ErrorScreen
      icon="ServerCrash"
      code="500"
      title="Erreur du serveur"
      description="Le serveur a rencontré une erreur en traitant votre demande. Réessayez dans un instant ; si elle persiste, consultez les journaux du serveur."
    >
      {onRetry && (
        <Button variant="inverse" icon="RotateCw" onClick={onRetry}>
          Réessayer
        </Button>
      )}
    </ErrorScreen>
  );
}
