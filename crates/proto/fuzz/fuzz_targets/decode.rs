//! `codec::decode` sur octets arbitraires, pour chaque message lu sur un
//! lien : `ControlToAgent` (agent, proxy), `AgentToControl` (plan de
//! controle), `ProxyReply` (agent). Le decodage ne doit jamais paniquer, et
//! un message decode doit se reencoder puis se relire a l'identique.

#![no_main]

use libfuzzer_sys::fuzz_target;
use proto::codec::{MAX_FRAME_LEN, decode, encode};
use proto::{AgentToControl, ControlToAgent, ProxyReply};

/// Decode `frame`, puis verifie l'aller-retour du message obtenu.
fn check<T>(frame: &[u8], same: fn(&T, &T) -> bool)
where
    T: serde::Serialize + serde::de::DeserializeOwned,
{
    if let Ok(msg) = decode::<T>(frame) {
        let again = encode(&msg).expect("un message decode se reencode");
        let back = decode::<T>(&again).expect("un message reencode se relit");
        assert!(same(&msg, &back), "aller-retour different");
    }
}

fn all(frame: &[u8]) {
    check::<ControlToAgent>(frame, |a, b| a == b);
    check::<ProxyReply>(frame, |a, b| a == b);
    // Contient des flottants (NaN != NaN) : seule la relecture est exigee.
    check::<AgentToControl>(frame, |_, _| true);
}

fuzz_target!(|data: &[u8]| {
    // Trame brute (prefixe de longueur compris, tel que recu).
    all(data);
    // Charge seule, prefixee de sa vraie longueur : atteint postcard.
    if data.len() <= MAX_FRAME_LEN {
        let mut frame = (data.len() as u32).to_be_bytes().to_vec();
        frame.extend_from_slice(data);
        all(&frame);
    }
});
