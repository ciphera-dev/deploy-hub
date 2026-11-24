import { useQueryClient } from "@tanstack/react-query";
import { useRef, useState, type ChangeEvent } from "react";

import { Avatar, Button, Icon, initials, Modal } from "@/design-system";
import { ME_QUERY_KEY } from "@/features/auth/api";

import { AVATAR_MAX_BYTES, AVATAR_TYPES, deleteAvatar, uploadAvatar } from "./api";

// Avatar.jsx : photo importee ou initiales. La galerie de personnages de la
// maquette est retiree ; l'image est recadree et re-encodee par le serveur.

const GRADIENT = "linear-gradient(135deg,oklch(0.78 0.09 250),oklch(0.62 0.12 280))";

/** Lit le fichier choisi en `data:` pour l'apercu (la CSP n'autorise pas `blob:`). */
function readAsDataUrl(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result));
    reader.onerror = () => reject(reader.error);
    reader.readAsDataURL(file);
  });
}

type Choice = { kind: "current" } | { kind: "initials" } | { kind: "file"; file: File; preview: string };

/** Fenetre « Photo de profil » (Picker de Avatar.jsx, sans galerie) : importer une image ou revenir aux initiales. */
export function AvatarPicker({ name, src, onClose }: { name: string; src?: string; onClose: () => void }) {
  const queryClient = useQueryClient();
  const input = useRef<HTMLInputElement>(null);
  const [choice, setChoice] = useState<Choice>({ kind: "current" });
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const preview = choice.kind === "file" ? choice.preview : choice.kind === "current" ? src : undefined;
  const initialsSelected = choice.kind === "initials" || (choice.kind === "current" && !src);

  const pick = async (e: ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    e.target.value = "";
    if (!file) return;
    setError("");
    if (!(AVATAR_TYPES as readonly string[]).includes(file.type)) return setError("Formats acceptés : PNG, JPEG ou WebP.");
    if (file.size > AVATAR_MAX_BYTES) return setError("L'image dépasse 1 Mo.");
    setChoice({ kind: "file", file, preview: await readAsDataUrl(file) });
  };

  const save = async () => {
    if (busy) return;
    let result: { ok: true } | { ok: false; message: string } = { ok: true };
    setBusy(true);
    setError("");
    if (choice.kind === "file") result = await uploadAvatar(choice.file);
    else if (choice.kind === "initials" && src) result = await deleteAvatar();
    setBusy(false);
    if (!result.ok) return setError(result.message);
    void queryClient.invalidateQueries({ queryKey: ME_QUERY_KEY, exact: true });
    onClose();
  };

  const tile = { aspectRatio: "1", borderRadius: 14, cursor: "pointer", display: "flex", flexDirection: "column", alignItems: "center", justifyContent: "center", gap: 8 } as const;
  return (
    <Modal
      title={
        <span style={{ display: "flex", alignItems: "center", gap: 16 }}>
          <span style={{ width: 56, height: 56, borderRadius: 99, overflow: "hidden", flex: "none", boxShadow: "0 0 0 2px var(--surface-sidebar), 0 0 0 3px rgba(255,255,255,.15)" }}>
            <Avatar name={name} src={preview} size={56} />
          </span>
          <span style={{ display: "flex", flexDirection: "column", gap: 6 }}>
            <span>Photo de profil</span>
            <span style={{ font: "400 12px/1 var(--font-sans)", color: "var(--text-subtle)" }}>Importez votre photo ou gardez vos initiales</span>
          </span>
        </span>
      }
      onClose={onClose}
      footer={
        <>
          <Button onClick={onClose}>Annuler</Button>
          <Button variant="inverse" disabled={busy} aria-busy={busy || undefined} onClick={() => void save()}>
            Enregistrer
          </Button>
        </>
      }
    >
      <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fill,minmax(96px,1fr))", gap: 10 }}>
        <button type="button" onClick={() => input.current?.click()} style={{ ...tile, border: "1px dashed rgba(255,255,255,.18)", background: "rgba(255,255,255,.02)", color: "var(--text-muted)", font: "500 12px/1 var(--font-sans)" }}>
          <Icon name="Upload" size={18} />
          Importer
        </button>
        <button
          type="button"
          aria-pressed={initialsSelected}
          onClick={() => setChoice({ kind: "initials" })}
          style={{ ...tile, position: "relative", border: "1px solid " + (initialsSelected ? "#ededed" : "rgba(255,255,255,.08)"), background: GRADIENT, color: "#fff", font: "600 22px/1 var(--font-sans)" }}
        >
          {initials(name)}
          <span style={{ position: "absolute", left: 0, right: 0, bottom: 8, font: "500 10px/1 var(--font-sans)", color: "rgba(255,255,255,.85)" }}>Initiales</span>
        </button>
        {preview && !initialsSelected && (
          <span style={{ ...tile, padding: 0, overflow: "hidden", border: "2px solid #ededed" }}>
            <img src={preview} alt="Aperçu" style={{ width: "100%", height: "100%", objectFit: "cover" }} />
          </span>
        )}
      </div>
      <span style={{ font: "400 12px/1.4 var(--font-sans)", color: "var(--text-subtle)" }}>PNG, JPEG ou WebP, 1 Mo au plus. L'image est recadrée en carré.</span>
      {error && (
        <span role="alert" style={{ display: "flex", alignItems: "center", gap: 6, font: "400 12px/1.3 var(--font-sans)", color: "var(--text-negative)" }}>
          <Icon name="CircleX" size={13} />
          {error}
        </span>
      )}
      <input ref={input} type="file" accept={AVATAR_TYPES.join(",")} aria-label="Importer une photo" style={{ display: "none" }} onChange={(e) => void pick(e)} />
    </Modal>
  );
}

export interface ProfileAvatarProps {
  name: string;
  /** URL de l'avatar (meme origine) ; initiales sinon */
  src?: string | null;
  size?: number;
  /** ouvre la fenetre « Photo de profil » au clic */
  editable?: boolean;
}

/** Avatar du compte (Avatar.jsx) : image importee ou initiales, modifiable sur la page Profil. */
export function ProfileAvatar({ name, src, size = 32, editable = false }: ProfileAvatarProps) {
  const [open, setOpen] = useState(false);
  const avatar = <Avatar name={name} src={src ?? undefined} size={size} />;
  if (!editable) return avatar;
  return (
    <>
      <button type="button" title="Changer la photo de profil" aria-label="Changer la photo de profil" onClick={() => setOpen(true)} style={{ display: "block", padding: 0, border: 0, background: "none", borderRadius: 99, cursor: "pointer" }}>
        {avatar}
      </button>
      {open && <AvatarPicker name={name} src={src ?? undefined} onClose={() => setOpen(false)} />}
    </>
  );
}
