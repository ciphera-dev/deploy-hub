import { Button } from "@/design-system";

import { ErrorScreen } from "./ErrorScreen";

export function Forbidden({ onBack }: { onBack?: () => void }) {
  return (
    <ErrorScreen
      icon="ShieldX"
      code="403"
      title="Accès refusé"
      description="Votre rôle ne vous permet pas d'ouvrir cette page ou d'effectuer cette action. Demandez l'accès à un administrateur de l'organisation."
    >
      {onBack && (
        <Button variant="secondary" icon="ArrowLeft" onClick={onBack}>
          Retour à la vue d'ensemble
        </Button>
      )}
    </ErrorScreen>
  );
}
