// Locked notes, actually locked.
//
// The body of a locked note is encrypted before it touches the disk, so what
// sits in the notes folder — and on the WebDAV server — is ciphertext, not
// readable HTML. The passcode is never stored: a key is derived from it with
// PBKDF2 and used once to seal the body with AES-256-GCM, which also detects
// tampering.
//
// Format: `enc1:<salt>:<nonce>:<ciphertext>`, base64url without padding.

use aes_gcm::aead::{Aead, KeyInit, OsRng, rand_core::RngCore};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use base64::Engine;
use pbkdf2::pbkdf2_hmac;
use sha2::Sha256;

const PREFIX: &str = "enc1:";
const ROUNDS: u32 = 120_000;
const SALT: usize = 16;
const NONCE: usize = 12;

fn key(passcode: &str, salt: &[u8]) -> [u8; 32] {
    let mut out = [0u8; 32];
    pbkdf2_hmac::<Sha256>(passcode.as_bytes(), salt, ROUNDS, &mut out);
    out
}

/// Whether a body is already sealed.
pub fn is_encrypted(body: &str) -> bool {
    body.trim_start().starts_with(PREFIX)
}

/// Seal a body with the passcode.
pub fn encrypt(passcode: &str, plaintext: &str) -> String {
    let mut salt = [0u8; SALT];
    let mut nonce_bytes = [0u8; NONCE];
    OsRng.fill_bytes(&mut salt);
    OsRng.fill_bytes(&mut nonce_bytes);

    let cipher = Aes256Gcm::new_from_slice(&key(passcode, &salt)).expect("32-byte key");
    let sealed = match cipher.encrypt(Nonce::from_slice(&nonce_bytes), plaintext.as_bytes()) {
        Ok(bytes) => bytes,
        // Sealing only fails if the key length is wrong, which can't happen.
        Err(_) => return String::new(),
    };

    format!(
        "{PREFIX}{}:{}:{}",
        B64.encode(salt),
        B64.encode(nonce_bytes),
        B64.encode(sealed)
    )
}

/// Open a sealed body. `None` means the passcode is wrong or the text was
/// altered — the two cannot be told apart, by design.
pub fn decrypt(passcode: &str, blob: &str) -> Option<String> {
    let rest = blob.trim_start().strip_prefix(PREFIX)?;
    let mut parts = rest.split(':');
    let salt = B64.decode(parts.next()?).ok()?;
    let nonce_bytes = B64.decode(parts.next()?).ok()?;
    let sealed = B64.decode(parts.next()?).ok()?;
    if salt.len() != SALT || nonce_bytes.len() != NONCE || parts.next().is_some() {
        return None;
    }

    let cipher = Aes256Gcm::new_from_slice(&key(passcode, &salt)).ok()?;
    let plain = cipher
        .decrypt(Nonce::from_slice(&nonce_bytes), sealed.as_ref())
        .ok()?;
    String::from_utf8(plain).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_body_round_trips_with_the_right_passcode() {
        let sealed = encrypt("correct horse", "<p>secret</p>");
        assert!(is_encrypted(&sealed));
        assert!(!sealed.contains("secret"), "the text is not readable: {sealed}");
        assert_eq!(decrypt("correct horse", &sealed).as_deref(), Some("<p>secret</p>"));
    }

    #[test]
    fn the_wrong_passcode_gives_nothing() {
        let sealed = encrypt("correct horse", "<p>secret</p>");
        assert_eq!(decrypt("wrong horse", &sealed), None);
        assert_eq!(decrypt("", &sealed), None);
    }

    #[test]
    fn tampering_is_detected() {
        let sealed = encrypt("pw", "<p>secret</p>");
        let mut tampered = sealed.clone();
        // Flip one character of the ciphertext.
        let at = sealed.len() - 3;
        let replacement = if sealed.as_bytes()[at] == b'A' { "B" } else { "A" };
        tampered.replace_range(at..at + 1, replacement);
        assert_eq!(decrypt("pw", &tampered), None);
    }

    #[test]
    fn each_seal_is_different() {
        let a = encrypt("pw", "<p>same</p>");
        let b = encrypt("pw", "<p>same</p>");
        assert_ne!(a, b, "a fresh salt and nonce every time");
    }

    #[test]
    fn plain_bodies_are_left_alone() {
        assert!(!is_encrypted("<p>hello</p>"));
        assert_eq!(decrypt("pw", "<p>hello</p>"), None);
    }
}
