//! Traitement des avatars envoyes par les comptes : le format est lu dans les
//! octets (jamais dans `Content-Type`), seuls PNG, JPEG et WebP sont decodes,
//! avec des limites explicites contre les bombes de decompression : les
//! dimensions sont verifiees des la lecture de l'en-tete, avant tout decodage
//! des pixels (c'est ce plafond de 4096 x 4096 qui borne la memoire), et au
//! plus `MAX_CONCURRENT` decodages tournent en meme temps. L'image est orientee, recadree au centre, reduite a
//! `SIZE` x `SIZE` puis re-encodee en PNG : aucune metadonnee d'origine (EXIF,
//! XMP, ICC, commentaires) ne survit, seuls les pixels sont gardes.

use std::collections::{HashMap, VecDeque};
use std::io::Cursor;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use image::imageops::FilterType;
use image::{
    DynamicImage, ExtendedColorType, ImageDecoder, ImageEncoder, ImageFormat, ImageReader, Limits,
};
use tokio::sync::Semaphore;
use uuid::Uuid;

/// Taille maximale du fichier envoye (1 Mo).
pub const MAX_BYTES: usize = 1024 * 1024;
/// Cote de l'avatar enregistre, en pixels.
pub const SIZE: u32 = 256;
/// Dimensions maximales de l'image envoyee.
pub const MAX_DIMENSION: u32 = 4096;
/// Allocation maximale annoncee au decodeur (`Limits::max_alloc`). Les
/// decodeurs ne la respectent pas tous : la borne effective est le plafond de
/// dimensions, controle a l'en-tete (4096 x 4096 RGBA = 64 Mio).
pub const MAX_ALLOC: u64 = 64 * 1024 * 1024;
/// Decodages simultanes, tous comptes confondus.
pub const MAX_CONCURRENT: usize = 2;
/// Envois decodes par compte dans `UPLOAD_WINDOW`.
pub const MAX_UPLOADS: usize = 5;
/// Fenetre glissante de la limite par compte.
pub const UPLOAD_WINDOW: Duration = Duration::from_secs(60);

#[derive(Debug, PartialEq, Eq)]
pub enum AvatarError {
    /// Ni PNG, ni JPEG, ni WebP (SVG, GIF, texte...).
    Unsupported,
    /// Format reconnu mais image illisible, ou dimensions hors limites.
    Invalid,
}

/// Format accepte, lu dans les premiers octets.
pub fn sniff(bytes: &[u8]) -> Result<ImageFormat, AvatarError> {
    match image::guess_format(bytes) {
        Ok(format @ (ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::WebP)) => Ok(format),
        _ => Err(AvatarError::Unsupported),
    }
}

/// Decode dans une tache bloquante en tenant un jeton de `slots` jusqu'a la
/// fin du calcul : si la requete est abandonnee (client parti), le jeton reste
/// pris tant que le decodage tourne, la limite de simultaneite tient donc.
pub async fn process_limited(
    slots: Arc<Semaphore>,
    bytes: impl AsRef<[u8]> + Send + 'static,
) -> Result<Result<Vec<u8>, AvatarError>, tokio::task::JoinError> {
    let permit = slots
        .acquire_owned()
        .await
        .expect("le semaphore des avatars n'est jamais ferme");
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        process(bytes.as_ref())
    })
    .await
}

/// Limite par compte des envois a decoder (fenetre glissante, en memoire :
/// remise a zero au redemarrage, un seul processus sert l'API).
#[derive(Default)]
pub struct UploadLimiter(Mutex<HashMap<Uuid, VecDeque<Instant>>>);

impl UploadLimiter {
    /// `Ok` (envoi compte) ou `Err(secondes avant le prochain essai)`.
    pub fn admit(&self, user: Uuid, now: Instant) -> Result<(), u64> {
        let mut map = self.0.lock().unwrap_or_else(|e| e.into_inner());
        map.retain(|_, hits| {
            while hits
                .front()
                .is_some_and(|t| now.duration_since(*t) >= UPLOAD_WINDOW)
            {
                hits.pop_front();
            }
            !hits.is_empty()
        });
        let hits = map.entry(user).or_default();
        if hits.len() >= MAX_UPLOADS {
            let oldest = hits.front().copied().unwrap_or(now);
            let wait = UPLOAD_WINDOW.saturating_sub(now.duration_since(oldest));
            return Err(wait.as_secs().max(1));
        }
        hits.push_back(now);
        Ok(())
    }
}

fn limits() -> Limits {
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_DIMENSION);
    limits.max_image_height = Some(MAX_DIMENSION);
    limits.max_alloc = Some(MAX_ALLOC);
    limits
}

/// Decode, oriente, recadre et re-encode l'image en PNG `SIZE` x `SIZE`.
/// Calcul couteux : a appeler hors de la boucle asynchrone (`spawn_blocking`).
pub fn process(bytes: &[u8]) -> Result<Vec<u8>, AvatarError> {
    if bytes.len() > MAX_BYTES {
        return Err(AvatarError::Invalid);
    }
    let format = sniff(bytes)?;
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    reader.limits(limits());
    let mut decoder = reader.into_decoder().map_err(|_| AvatarError::Invalid)?;
    let (width, height) = decoder.dimensions();
    if width == 0 || height == 0 || width > MAX_DIMENSION || height > MAX_DIMENSION {
        return Err(AvatarError::Invalid);
    }
    // Orientation EXIF lue avant de jeter les metadonnees (photo de telephone).
    let orientation = decoder.orientation().ok();
    let mut img = DynamicImage::from_decoder(decoder).map_err(|_| AvatarError::Invalid)?;
    if let Some(orientation) = orientation {
        img.apply_orientation(orientation);
    }
    let square = img
        .resize_to_fill(SIZE, SIZE, FilterType::Lanczos3)
        .to_rgba8();
    let mut out = Vec::new();
    image::codecs::png::PngEncoder::new(&mut out)
        .write_image(square.as_raw(), SIZE, SIZE, ExtendedColorType::Rgba8)
        .map_err(|_| AvatarError::Invalid)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode(format: ImageFormat, width: u32, height: u32) -> Vec<u8> {
        let mut out = Vec::new();
        DynamicImage::ImageRgb8(image::RgbImage::new(width, height))
            .write_to(&mut Cursor::new(&mut out), format)
            .unwrap();
        out
    }

    #[test]
    fn only_png_jpeg_webp_are_accepted() {
        assert_eq!(sniff(&encode(ImageFormat::Png, 2, 2)), Ok(ImageFormat::Png));
        assert_eq!(
            sniff(&encode(ImageFormat::Jpeg, 2, 2)),
            Ok(ImageFormat::Jpeg)
        );
        assert_eq!(
            sniff(b"<svg xmlns=\"http://www.w3.org/2000/svg\"/>"),
            Err(AvatarError::Unsupported)
        );
        assert_eq!(
            sniff(b"<?xml version=\"1.0\"?><svg/>"),
            Err(AvatarError::Unsupported)
        );
        assert_eq!(sniff(b"GIF89a"), Err(AvatarError::Unsupported));
        assert_eq!(sniff(b"BM\0\0"), Err(AvatarError::Unsupported));
        assert_eq!(sniff(b""), Err(AvatarError::Unsupported));
    }

    #[test]
    fn output_is_a_square_png() {
        let out = process(&encode(ImageFormat::Jpeg, 300, 120)).unwrap();
        assert_eq!(image::guess_format(&out).unwrap(), ImageFormat::Png);
        let img = image::load_from_memory(&out).unwrap();
        assert_eq!((img.width(), img.height()), (SIZE, SIZE));
    }

    fn crc32(data: &[u8]) -> u32 {
        let mut crc = !0u32;
        for &b in data {
            crc ^= u32::from(b);
            for _ in 0..8 {
                crc = if crc & 1 == 1 {
                    (crc >> 1) ^ 0xEDB8_8320
                } else {
                    crc >> 1
                };
            }
        }
        !crc
    }

    fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut out = (data.len() as u32).to_be_bytes().to_vec();
        out.extend_from_slice(kind);
        out.extend_from_slice(data);
        let mut crc_input = kind.to_vec();
        crc_input.extend_from_slice(data);
        out.extend_from_slice(&crc32(&crc_input).to_be_bytes());
        out
    }

    /// PNG dont l'en-tete (CRC valide) annonce 65535 x 65535, sans pixels.
    fn forged_png() -> Vec<u8> {
        let mut ihdr = 65535u32.to_be_bytes().to_vec();
        ihdr.extend_from_slice(&65535u32.to_be_bytes());
        ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
        let mut out = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
        out.extend(chunk(b"IHDR", &ihdr));
        out.extend(chunk(b"IDAT", &[0x78, 0x9c, 0x03, 0x00]));
        out.extend(chunk(b"IEND", &[]));
        out
    }

    #[test]
    fn forged_header_is_refused_before_decoding() {
        let png = forged_png();
        assert_eq!(sniff(&png), Ok(ImageFormat::Png));
        // Refus a la lecture de l'en-tete : aucun decodeur n'est construit.
        let mut reader = ImageReader::with_format(Cursor::new(&png), ImageFormat::Png);
        reader.limits(limits());
        assert!(reader.into_decoder().is_err());
        assert_eq!(process(&png), Err(AvatarError::Invalid));
    }

    #[test]
    fn uploads_are_limited_per_account() {
        let limiter = UploadLimiter::default();
        let (alice, bob) = (Uuid::new_v4(), Uuid::new_v4());
        let t0 = Instant::now();
        for _ in 0..MAX_UPLOADS {
            assert_eq!(limiter.admit(alice, t0), Ok(()));
        }
        assert_eq!(limiter.admit(alice, t0), Err(UPLOAD_WINDOW.as_secs()));
        assert_eq!(limiter.admit(bob, t0), Ok(()), "par compte");
        assert_eq!(
            limiter.admit(alice, t0 + UPLOAD_WINDOW),
            Ok(()),
            "fenetre glissante"
        );
    }

    #[tokio::test]
    async fn permit_is_held_until_blocking_work_ends() {
        let slots = Arc::new(Semaphore::new(1));
        let (release, wait) = std::sync::mpsc::channel::<()>();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel::<()>();
        // Corps qui bloque le decodage jusqu'au signal `release`.
        struct Gate(
            std::sync::mpsc::Receiver<()>,
            Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
        );
        impl AsRef<[u8]> for Gate {
            fn as_ref(&self) -> &[u8] {
                if let Some(tx) = self.1.lock().unwrap().take() {
                    let _ = tx.send(());
                }
                let _ = self.0.recv();
                b"pas une image"
            }
        }
        let gate = Gate(wait, Mutex::new(Some(started_tx)));
        let request = tokio::spawn(process_limited(slots.clone(), gate));
        started_rx.await.unwrap();
        // Le client abandonne la requete : le decodage continue, jeton tenu.
        request.abort();
        let _ = request.await;
        assert_eq!(
            slots.available_permits(),
            0,
            "jeton tenu pendant le decodage"
        );
        release.send(()).unwrap();
        for _ in 0..200 {
            if slots.available_permits() == 1 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(slots.available_permits(), 1, "jeton rendu a la fin");
    }

    #[test]
    fn dimension_limits_are_enforced() {
        assert_eq!(
            process(&encode(ImageFormat::Png, MAX_DIMENSION + 1, 1)),
            Err(AvatarError::Invalid)
        );
        assert_eq!(
            process(&encode(ImageFormat::Png, 1, MAX_DIMENSION + 1)),
            Err(AvatarError::Invalid)
        );
        assert!(process(&encode(ImageFormat::Png, 1, 1)).is_ok());
    }
}
