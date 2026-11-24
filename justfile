set windows-shell := ["pwsh", "-NoProfile", "-Command"]

default:
    @just --list

# Panneau en mode dev (maquette MSW) : branche dans la tache panneau
dev:
    cd web; npm run dev

# Genere openapi.json (control-plane) puis le schema TypeScript du panneau
gen:
    cargo run -q -p control-plane --bin gen-openapi -- openapi.json
    npx --prefix web openapi-typescript openapi.json -o web/src/lib/api/schema.d.ts

build:
    cargo build --workspace --release

test:
    cargo test --workspace

lint:
    cargo fmt --all --check
    cargo clippy --workspace --all-targets -- -D warnings

audit:
    cargo audit
    cargo deny check