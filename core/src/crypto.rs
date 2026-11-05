// Locked notes, actually locked.
//
// The body of a locked note is encrypted before it touches the disk, so what
// sits in the notes folder — and on the WebDAV server — is ciphertext, not
// readable HTML. The passcode is never stored: a key is derived from it with
// PBKDF2 and used once to seal the body with AES-256-GCM, which also detects
// tampering.
//
// Formats: `enc1:<salt>:<nonce>:<ciphertext>` (fixed 120 000 rounds, kept so
// old notes still open) and `enc2:<rounds>:<salt>:<nonce>:<ciphertext>`,
// which carries its own work factor so it can be raised. Both are base64url
// without padding.

use aes_gcm::aead::{Aead, KeyInit, OsRng, rand_core::RngCore};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use base64::Engine;
use pbkdf2::pbkdf2_hmac;
use sha2::{Digest, Sha256};

/// Encryption format version prefixes.
const PREFIX_V1: &str = "enc1:";
const PREFIX_V2: &str = "enc2:";
/// PBKDF2 iteration count of the `enc1:` format, kept to open old notes.
const ROUNDS_V1: u32 = 120_000;
/// PBKDF2 iteration count for new seals (OWASP recommendation for
/// PBKDF2-HMAC-SHA256).
const ROUNDS: u32 = 600_000;
/// Upper bound on a round count read from a blob, so a hostile `enc2:` header
/// can't turn decryption into a denial of service.
const MAX_ROUNDS: u32 = 10_000_000;
/// Salt size in bytes for encryption.
const SALT: usize = 16;
/// Nonce size in bytes for AES-256-GCM.
const NONCE: usize = 12;
/// Rounds for the app-lock passcode verifier.
const PASS_ROUNDS: u32 = 600_000;

fn key(passcode: &str, salt: &[u8], rounds: u32) -> [u8; 32] {
    let mut out = [0u8; 32];
    pbkdf2_hmac::<Sha256>(passcode.as_bytes(), salt, rounds, &mut out);
    out
}

/// Whether a body is already sealed.
pub fn is_encrypted(body: &str) -> bool {
    let body = body.trim_start();
    body.starts_with(PREFIX_V1) || body.starts_with(PREFIX_V2)
}

/// Seal a body with the passcode, in the current `enc2:` format.
pub fn encrypt(passcode: &str, plaintext: &str) -> String {
    let mut salt = [0u8; SALT];
    let mut nonce_bytes = [0u8; NONCE];
    OsRng.fill_bytes(&mut salt);
    OsRng.fill_bytes(&mut nonce_bytes);

    let cipher = Aes256Gcm::new_from_slice(&key(passcode, &salt, ROUNDS)).expect("32-byte key");
    let sealed = match cipher.encrypt(Nonce::from_slice(&nonce_bytes), plaintext.as_bytes()) {
        Ok(bytes) => bytes,
        // Sealing only fails if the key length is wrong, which can't happen.
        Err(_) => return String::new(),
    };

    format!(
        "{PREFIX_V2}{ROUNDS}:{}:{}:{}",
        B64.encode(salt),
        B64.encode(nonce_bytes),
        B64.encode(sealed)
    )
}

/// Open a sealed body. `None` means the passcode is wrong or the text was
/// altered — the two cannot be told apart, by design.
pub fn decrypt(passcode: &str, blob: &str) -> Option<String> {
    let rest = blob.trim_start();
    let (rounds, mut parts) = if let Some(rest) = rest.strip_prefix(PREFIX_V2) {
        let mut parts = rest.split(':');
        let rounds: u32 = parts.next()?.parse().ok()?;
        if rounds == 0 || rounds > MAX_ROUNDS {
            return None;
        }
        (rounds, parts)
    } else {
        let rest = rest.strip_prefix(PREFIX_V1)?;
        (ROUNDS_V1, rest.split(':'))
    };
    let salt = B64.decode(parts.next()?).ok()?;
    let nonce_bytes = B64.decode(parts.next()?).ok()?;
    let sealed = B64.decode(parts.next()?).ok()?;
    if salt.len() != SALT || nonce_bytes.len() != NONCE || parts.next().is_some() {
        return None;
    }

    let cipher = Aes256Gcm::new_from_slice(&key(passcode, &salt, rounds)).ok()?;
    let plain = cipher
        .decrypt(Nonce::from_slice(&nonce_bytes), sealed.as_ref())
        .ok()?;
    String::from_utf8(plain).ok()
}

/// The app-lock verifier, stored in `app.json`: `pbkdf2$<rounds>$<salt>$<hash>`
/// (base64url). A salted, slow KDF, so a stolen file can't be brute-forced at
/// SHA-256 speed.
pub fn seal_passcode(passcode: &str) -> String {
    let mut salt = [0u8; SALT];
    OsRng.fill_bytes(&mut salt);
    let mut out = [0u8; 32];
    pbkdf2_hmac::<Sha256>(passcode.as_bytes(), &salt, PASS_ROUNDS, &mut out);
    format!(
        "pbkdf2${PASS_ROUNDS}${}${}",
        B64.encode(salt),
        B64.encode(out)
    )
}

/// Whether a stored verifier is the legacy single unsalted SHA-256 hex digest.
pub fn is_legacy_passcode(stored: &str) -> bool {
    stored.len() == 64 && stored.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Check a passcode against a stored verifier, in either format.
pub fn check_passcode(stored: &str, passcode: &str) -> bool {
    if let Some(rest) = stored.strip_prefix("pbkdf2$") {
        let mut parts = rest.split('$');
        let (Some(rounds), Some(salt), Some(want)) =
            (parts.next(), parts.next(), parts.next())
        else {
            return false;
        };
        if parts.next().is_some() {
            return false;
        }
        let Ok(rounds) = rounds.parse::<u32>() else {
            return false;
        };
        if rounds == 0 || rounds > MAX_ROUNDS {
            return false;
        }
        let (Ok(salt), Ok(want)) = (B64.decode(salt), B64.decode(want)) else {
            return false;
        };
        if want.len() != 32 {
            return false;
        }
        let mut out = [0u8; 32];
        pbkdf2_hmac::<Sha256>(passcode.as_bytes(), &salt, rounds, &mut out);
        // Constant-time compare.
        let mut diff = 0u8;
        for (a, b) in out.iter().zip(want.iter()) {
            diff |= a ^ b;
        }
        diff == 0
    } else {
        // Legacy verifier: one unsalted SHA-256 round.
        is_legacy_passcode(stored) && stored == hex_sha256(passcode)
    }
}

fn hex_sha256(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_body_round_trips_with_the_right_passcode() {
        let sealed = encrypt("correct horse", "<p>secret</p>");
        assert!(is_encrypted(&sealed));
        assert!(sealed.starts_with("enc2:"), "new seals use enc2: {sealed}");
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

    #[test]
    fn old_enc1_bodies_still_open() {
        // Build an enc1 blob by hand at the old work factor.
        let salt = [7u8; SALT];
        let nonce_bytes = [9u8; NONCE];
        let cipher = Aes256Gcm::new_from_slice(&key("pw", &salt, ROUNDS_V1)).unwrap();
        let sealed = cipher
            .encrypt(Nonce::from_slice(&nonce_bytes), "<p>old</p>".as_bytes())
            .unwrap();
        let blob = format!(
            "enc1:{}:{}:{}",
            B64.encode(salt),
            B64.encode(nonce_bytes),
            B64.encode(sealed)
        );
        assert!(is_encrypted(&blob));
        assert_eq!(decrypt("pw", &blob).as_deref(), Some("<p>old</p>"));
        assert_eq!(decrypt("nope", &blob), None);
    }

    #[test]
    fn a_hostile_round_count_is_rejected() {
        let blob = format!("enc2:999999999999:{}:{}:{}", B64.encode([0u8; 16]), B64.encode([0u8; 12]), B64.encode([0u8; 16]));
        assert_eq!(decrypt("pw", &blob), None);
    }

    #[test]
    fn passcode_verifier_round_trips_and_upgrades() {
        let stored = seal_passcode("1234");
        assert!(stored.starts_with("pbkdf2$"));
        assert!(!is_legacy_passcode(&stored));
        assert!(check_passcode(&stored, "1234"));
        assert!(!check_passcode(&stored, "1235"));
        // Legacy unsalted SHA-256 verifiers still verify.
        let legacy = hex_sha256("1234");
        assert!(is_legacy_passcode(&legacy));
        assert!(check_passcode(&legacy, "1234"));
        assert!(!check_passcode(&legacy, "0000"));
    }
}
