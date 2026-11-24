//! Trames : longueur sur 4 octets (u32 gros-boutiste) puis charge postcard.
//! Une trame ne depasse jamais `MAX_FRAME_LEN` octets de charge ; la longueur
//! est verifiee avant toute allocation. Le decodage ne panique jamais.

use std::fmt;

use bytes::{BufMut, Bytes, BytesMut};
use serde::Serialize;
use serde::de::DeserializeOwned;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// Taille maximale de la charge d'une trame : 4 Mio.
pub const MAX_FRAME_LEN: usize = 4 * 1024 * 1024;

const PREFIX_LEN: usize = 4;

#[derive(Debug)]
pub enum Error {
    /// Charge annoncee ou produite au-dela de `MAX_FRAME_LEN` (longueur en octets).
    Oversize(usize),
    /// Trame incomplete ou suivie d'octets en trop.
    Framing,
    /// Charge illisible pour le type attendu.
    Malformed,
    /// Message impossible a encoder.
    Encode,
    Io(std::io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Oversize(n) => write!(f, "trame de {n} octets, maximum {MAX_FRAME_LEN}"),
            Self::Framing => f.write_str("trame incomplete ou octets en trop"),
            Self::Malformed => f.write_str("message illisible"),
            Self::Encode => f.write_str("message impossible a encoder"),
            Self::Io(e) => write!(f, "lecture ou ecriture du flux : {e}"),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

/// Longueur de charge annoncee par un prefixe, refusee si trop grande.
pub fn payload_len(prefix: [u8; PREFIX_LEN]) -> Result<usize> {
    let len = u32::from_be_bytes(prefix) as usize;
    if len > MAX_FRAME_LEN {
        return Err(Error::Oversize(len));
    }
    Ok(len)
}

/// Encode `msg` en une trame complete (prefixe compris).
pub fn encode<T: Serialize>(msg: &T) -> Result<Bytes> {
    let payload = postcard::to_stdvec(msg).map_err(|_| Error::Encode)?;
    if payload.len() > MAX_FRAME_LEN {
        return Err(Error::Oversize(payload.len()));
    }
    let mut frame = BytesMut::with_capacity(PREFIX_LEN + payload.len());
    // payload.len() <= MAX_FRAME_LEN < u32::MAX : conversion sans perte.
    frame.put_u32(payload.len() as u32);
    frame.put_slice(&payload);
    Ok(frame.freeze())
}

/// Decode une trame complete (prefixe compris, rien apres).
pub fn decode<T: DeserializeOwned>(frame: &[u8]) -> Result<T> {
    let (prefix, payload) = frame
        .split_first_chunk::<PREFIX_LEN>()
        .ok_or(Error::Framing)?;
    let len = payload_len(*prefix)?;
    if payload.len() != len {
        return Err(Error::Framing);
    }
    decode_payload(payload)
}

fn decode_payload<T: DeserializeOwned>(payload: &[u8]) -> Result<T> {
    match postcard::take_from_bytes(payload) {
        Ok((msg, [])) => Ok(msg),
        Ok(_) => Err(Error::Framing),
        Err(_) => Err(Error::Malformed),
    }
}

/// Lit la trame suivante d'un flux. `None` si le flux se termine proprement
/// entre deux trames. La charge n'est allouee qu'apres controle de sa taille.
pub async fn read_frame<R, T>(reader: &mut R) -> Result<Option<T>>
where
    R: AsyncRead + Unpin + ?Sized,
    T: DeserializeOwned,
{
    let mut prefix = [0u8; PREFIX_LEN];
    let mut filled = 0;
    while filled < PREFIX_LEN {
        match reader
            .read(&mut prefix[filled..])
            .await
            .map_err(Error::Io)?
        {
            0 if filled == 0 => return Ok(None),
            0 => return Err(Error::Framing),
            n => filled += n,
        }
    }
    let len = payload_len(prefix)?;
    // Tampon qui grandit avec les octets recus (borne par `len`) : annoncer
    // 4 Mio sans les envoyer ne fait rien allouer.
    let mut payload = Vec::new();
    let mut limited = (&mut *reader).take(len as u64);
    limited.read_to_end(&mut payload).await.map_err(Error::Io)?;
    if payload.len() != len {
        return Err(Error::Framing);
    }
    decode_payload(&payload).map(Some)
}

/// Ecrit `msg` en une trame.
pub async fn write_frame<W, T>(writer: &mut W, msg: &T) -> Result<()>
where
    W: AsyncWrite + Unpin + ?Sized,
    T: Serialize,
{
    let frame = encode(msg)?;
    writer.write_all(&frame).await.map_err(Error::Io)?;
    writer.flush().await.map_err(Error::Io)
}
