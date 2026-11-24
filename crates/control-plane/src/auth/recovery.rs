//! Codes de secours : 10 codes de 10 caracteres base32 (50 bits chacun),
//! presentes `XXXXX-XXXXX`, affiches une seule fois a l'activation de la TOTP
//! ou a l'enregistrement d'une passkey sur un compte qui n'en a plus.
//! Seul leur hachage Argon2id est stocke ; chacun ne sert qu'une fois
//! (marquage conditionnel : deux soumissions simultanees, une seule passe).

use rand::TryRng;
use rand::rngs::SysRng;
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;
use zeroize::Zeroizing;

use super::password;

/// Nombre de codes emis a l'activation.
pub const COUNT: usize = 10;
/// Caracteres significatifs d'un code.
const LEN: usize = 10;
/// Alphabet base32 (RFC 4648) : pas de 0/O ni de 1/I a confondre.
const ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

/// Un code au format d'affichage `XXXXX-XXXXX`.
fn generate_one() -> Result<Zeroizing<String>, rand::rngs::SysError> {
    let mut bytes = Zeroizing::new([0u8; LEN]);
    SysRng.try_fill_bytes(bytes.as_mut())?;
    let mut code = Zeroizing::new(String::with_capacity(LEN + 1));
    for (i, b) in bytes.iter().enumerate() {
        if i == LEN / 2 {
            code.push('-');
        }
        // 256 est un multiple de 32 : tirage uniforme.
        code.push(char::from(ALPHABET[usize::from(*b) % 32]));
    }
    Ok(code)
}

/// `COUNT` codes distincts.
pub fn generate() -> Result<Vec<Zeroizing<String>>, rand::rngs::SysError> {
    let mut codes: Vec<Zeroizing<String>> = Vec::with_capacity(COUNT);
    while codes.len() < COUNT {
        let code = generate_one()?;
        if !codes.iter().any(|c| **c == *code) {
            codes.push(code);
        }
    }
    Ok(codes)
}

/// Forme canonique d'une saisie (majuscules, sans espaces ni tirets), ou
/// `None` si ce ne peut pas etre un code.
pub fn normalize(input: &str) -> Option<Zeroizing<String>> {
    let code: Zeroizing<String> = Zeroizing::new(
        input
            .chars()
            .filter(|c| !c.is_whitespace() && *c != '-')
            .map(|c| c.to_ascii_uppercase())
            .collect(),
    );
    (code.len() == LEN && code.bytes().all(|b| ALPHABET.contains(&b))).then_some(code)
}

/// Hachages Argon2id des codes (forme canonique). Couteux : hors du thread asynchrone.
pub fn hash_all(codes: &[Zeroizing<String>]) -> Result<Vec<String>, argon2::password_hash::Error> {
    codes
        .iter()
        .map(|c| {
            let canonical = normalize(c).expect("code genere valide");
            password::hash(&canonical)
        })
        .collect()
}

/// Remplace les codes du compte par ces hachages.
pub async fn replace(
    conn: &mut PgConnection,
    user_id: Uuid,
    hashes: &[String],
) -> Result<(), sqlx::Error> {
    delete_all(&mut *conn, user_id).await?;
    for hash in hashes {
        sqlx::query("INSERT INTO recovery_codes (id, user_id, code_hash) VALUES ($1, $2, $3)")
            .bind(Uuid::new_v4())
            .bind(user_id)
            .bind(hash)
            .execute(&mut *conn)
            .await?;
    }
    Ok(())
}

/// Le compte a encore au moins un code utilisable.
pub async fn has_unused(conn: &mut PgConnection, user_id: Uuid) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM recovery_codes WHERE user_id = $1 AND used_at IS NULL)",
    )
    .bind(user_id)
    .fetch_one(conn)
    .await
}

/// Supprime tous les codes du compte.
pub async fn delete_all(conn: &mut PgConnection, user_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM recovery_codes WHERE user_id = $1")
        .bind(user_id)
        .execute(conn)
        .await?;
    Ok(())
}

/// Codes encore utilisables du compte : (identifiant, hachage).
pub async fn unused(db: &PgPool, user_id: Uuid) -> Result<Vec<(Uuid, String)>, sqlx::Error> {
    sqlx::query_as(
        "SELECT id, code_hash FROM recovery_codes WHERE user_id = $1 AND used_at IS NULL",
    )
    .bind(user_id)
    .fetch_all(db)
    .await
}

/// Identifiant du code correspondant a la saisie canonique `code`, parmi `candidates`.
/// Couteux (Argon2 par code) : hors du thread asynchrone.
pub fn find(code: &str, candidates: &[(Uuid, String)]) -> Option<Uuid> {
    candidates
        .iter()
        .find(|(_, hash)| password::verify(code, hash))
        .map(|(id, _)| *id)
}

/// Marque le code utilise ; `false` s'il l'etait deja (soumission concurrente).
pub async fn mark_used(conn: &mut PgConnection, id: Uuid) -> Result<bool, sqlx::Error> {
    let done =
        sqlx::query("UPDATE recovery_codes SET used_at = now() WHERE id = $1 AND used_at IS NULL")
            .bind(id)
            .execute(conn)
            .await?;
    Ok(done.rows_affected() == 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ten_distinct_grouped_base32_codes() {
        let codes = generate().unwrap();
        assert_eq!(codes.len(), COUNT);
        for c in &codes {
            assert_eq!(c.len(), 11);
            assert_eq!(&c[5..6], "-");
            assert!(normalize(c).is_some(), "{}", **c);
        }
        let mut all: Vec<&str> = codes.iter().map(|c| c.as_str()).collect();
        all.sort();
        all.dedup();
        assert_eq!(all.len(), COUNT);
    }

    #[test]
    fn input_is_normalized() {
        assert_eq!(
            normalize(" abcde-fg234 ").as_deref().map(String::as_str),
            Some("ABCDEFG234")
        );
        assert_eq!(
            normalize("ABCDE FG234").as_deref().map(String::as_str),
            Some("ABCDEFG234")
        );
        for bad in [
            "",
            "ABCDE-FG23",
            "ABCDE-FG2345",
            "ABCDE-FG180",
            "ABCDE_FG234",
            "ÉBCDE-FG234",
        ] {
            assert!(normalize(bad).is_none(), "{bad:?}");
        }
    }

    #[test]
    fn hashes_are_argon2id_and_match_canonical_input() {
        let codes = generate().unwrap();
        let hashes = hash_all(&codes[..2]).unwrap();
        assert!(hashes.iter().all(|h| h.starts_with("$argon2id$")));
        let candidates: Vec<(Uuid, String)> =
            hashes.into_iter().map(|h| (Uuid::new_v4(), h)).collect();
        let typed = normalize(&codes[1].to_lowercase()).unwrap();
        assert_eq!(find(&typed, &candidates), Some(candidates[1].0));
        assert_eq!(find("AAAAAAAAAA", &candidates), None);
    }
}
