import { useState } from "react";

import { Icon } from "@/design-system";

import { REGIONS, slugify, type Region } from "./regions";
import { SetupInput, StepHeading, lbl } from "./ui";

export interface OrganizationStepProps {
  name: string;
  region: Region;
  onName: (name: string) => void;
  onRegion: (region: Region) => void;
}

export function OrganizationStep({ name, region, onName, onRegion }: OrganizationStepProps) {
  return (
    <>
      <StepHeading title="Votre organisation" subtitle="Visible par les membres que vous inviterez." />
      <SetupInput label="Nom de l'organisation" value={name} onChange={onName} placeholder="Ciphera" autoFocus hint={name.trim() ? "Identifiant : " + slugify(name) : undefined} />
      <RegionPicker region={region} onChange={onRegion} />
    </>
  );
}

function RegionPicker({ region, onChange }: { region: Region; onChange: (r: Region) => void }) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const list = REGIONS.filter((r) => (r.city + " " + r.country).toLowerCase().includes(query.toLowerCase()));
  return (
    <div style={{ position: "relative", display: "flex", flexDirection: "column", gap: 8 }}>
      <span style={{ font: "500 13px/1 var(--font-sans)", color: "var(--text-strong)" }}>Région</span>
      <button
        type="button"
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-label={"Région : " + region.city}
        onClick={() => setOpen((v) => !v)}
        style={{ display: "flex", alignItems: "center", gap: 10, height: 44, padding: "0 12px", borderRadius: 10, border: "1px solid " + (open ? "rgba(167,139,250,.7)" : "var(--border-default)"), boxShadow: open ? "0 0 0 3px rgba(139,124,246,.15)" : "none", background: "var(--surface-raised)", cursor: "pointer", textAlign: "left" }}
      >
        <CountryBadge cc={region.cc} selected />
        <span style={{ flex: 1, font: "500 14px/1 var(--font-sans)", color: "var(--text-strong)" }}>
          {region.city}
          <span style={{ fontWeight: 400, color: "var(--text-subtle)" }}> · {region.country}</span>
        </span>
        <Icon name="ChevronDown" size={15} color="var(--text-muted)" style={{ transform: open ? "rotate(180deg)" : "none", transition: "transform .15s" }} />
      </button>
      {open && (
        <div style={{ position: "absolute", top: 76, left: 0, right: 0, zIndex: 20, borderRadius: 12, background: "var(--surface-overlay, #141417)", border: "1px solid var(--border-default)", boxShadow: "0 18px 40px -12px rgba(0,0,0,.8)", overflow: "hidden" }}>
          <div style={{ display: "flex", alignItems: "center", gap: 8, height: 40, padding: "0 12px", borderBottom: "1px solid var(--border-subtle)" }}>
            <Icon name="Search" size={14} color="var(--text-subtle)" />
            <input
              className="setup-field"
              aria-label="Rechercher une ville"
              autoFocus
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder="Rechercher une ville…"
              style={{ flex: 1, minWidth: 0, background: "transparent", border: 0, outline: "none", color: "var(--text-strong)", font: "400 13px/1 var(--font-sans)" }}
            />
          </div>
          <div role="listbox" aria-label="Villes" style={{ maxHeight: 240, overflowY: "auto", padding: 4 }}>
            {list.map((r) => {
              const on = r.city === region.city;
              return (
                <button
                  key={r.city}
                  type="button"
                  role="option"
                  aria-selected={on}
                  onClick={() => {
                    onChange(r);
                    setOpen(false);
                    setQuery("");
                  }}
                  style={{ display: "flex", alignItems: "center", gap: 10, width: "100%", height: 38, padding: "0 10px", borderRadius: 8, border: 0, cursor: "pointer", textAlign: "left", background: on ? "var(--surface-selected)" : "transparent" }}
                >
                  <CountryBadge cc={r.cc} />
                  <span style={{ flex: 1, font: "400 13px/1 var(--font-sans)", color: "var(--text-strong)" }}>{r.city}</span>
                  <span style={{ font: "400 12px/1 var(--font-sans)", color: "var(--text-subtle)" }}>{r.country}</span>
                  {on && <Icon name="Check" size={14} color="var(--text-strong)" />}
                </button>
              );
            })}
            {!list.length && <div style={{ padding: "14px 10px", font: "400 13px/1 var(--font-sans)", color: "var(--text-subtle)" }}>Aucune ville.</div>}
          </div>
        </div>
      )}
      <span style={{ ...lbl, fontSize: 11 }}>Utilisée pour la langue et le fuseau horaire ({region.timezone}).</span>
    </div>
  );
}

function CountryBadge({ cc, selected = false }: { cc: string; selected?: boolean }) {
  return (
    <span style={{ width: 26, height: 20, borderRadius: 5, flex: "none", display: "flex", alignItems: "center", justifyContent: "center", background: selected ? "var(--surface-selected)" : "var(--surface-app)", border: selected ? "none" : "1px solid var(--border-subtle)", font: "600 10px/1 var(--font-mono)", color: "var(--text-muted)" }}>{cc}</span>
  );
}
