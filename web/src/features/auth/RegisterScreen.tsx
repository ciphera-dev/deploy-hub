import { useQuery } from "@tanstack/react-query";

import { Icon } from "@/design-system";

import { AccountForm } from "./AccountForm";
import { fetchPublicSignup, register } from "./api";
import { AuthLayout, SIGNUP_COPY, SIGNUP_STEPS } from "./AuthLayout";
import { SignInLink, signupMessage } from "./InviteAccept";
import { LoginFlow } from "./LoginFlow";

export interface RegisterScreenProps {
  /** Compte cree : la connexion suit. */
  onCreated: () => void;
  onSignIn: () => void;
  /** Session ouverte depuis l'etape de connexion de l'ecran « sur invitation ». */
  onSignedIn: () => void;
}

/** Inscription : « Inscription sur invitation » tant que l'inscription publique est fermee (defaut), sinon le formulaire de register.html. */
export function RegisterScreen({ onCreated, onSignIn, onSignedIn }: RegisterScreenProps) {
  const open = useQuery({ queryKey: ["public-signup"], queryFn: fetchPublicSignup, staleTime: Infinity });

  if (open.data === false) return <LoginFlow initialStep="register" onSignedIn={onSignedIn} />;

  return (
    <AuthLayout steps={SIGNUP_STEPS} current={1} {...SIGNUP_COPY}>
      {open.data ? (
        <AccountForm
          title="Créer un compte"
          submitLabel="Créer le compte"
          withoutPassword
          intro="Un lien de confirmation sera envoyé à cette adresse : vous y choisirez votre mot de passe."
          onSubmit={async (values) => {
            const result = await register({ name: values.name, email: values.email });
            if (result.ok) {
              onCreated();
              return undefined;
            }
            return signupMessage(result);
          }}
          footer={<SignInLink onSignIn={onSignIn} />}
        />
      ) : (
        <div role="status" aria-label="Chargement" className="login-step" style={{ display: "flex", justifyContent: "center", paddingTop: 40 }}>
          <Icon name="Loader" size={22} color="#a78bfa" className="login-spin" />
        </div>
      )}
    </AuthLayout>
  );
}
