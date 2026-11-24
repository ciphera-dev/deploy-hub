//! Genere le document OpenAPI (`just gen`).
//!
//! Sans argument : ecrit sur la sortie standard. Avec un chemin : ecrit ce
//! fichier directement (pas de redirection du shell, donc pas de conversion de
//! fins de ligne ni d'encodage). Sortie deterministe : cles triees (BTreeMap
//! d'utoipa), fins de ligne LF.

use std::io::Write;

fn main() {
    let mut json = api_types::openapi()
        .to_pretty_json()
        .expect("serialisation OpenAPI");
    json.push('\n');
    match std::env::args().nth(1) {
        Some(path) => std::fs::write(&path, json).expect("ecriture du fichier OpenAPI"),
        None => std::io::stdout()
            .lock()
            .write_all(json.as_bytes())
            .expect("ecriture de la sortie standard"),
    }
}
