//! Algorithm lists: modern by default, legacy ones only in compatibility mode.

use std::borrow::Cow;

use russh::keys::{Algorithm, EcdsaCurve, HashAlg};
use russh::{Preferred, cipher, kex, mac};
use serde::Serialize;

const MODERN_KEX: &[kex::Name] = &[
    kex::MLKEM768X25519_SHA256,
    kex::CURVE25519,
    kex::CURVE25519_PRE_RFC_8731,
    kex::ECDH_SHA2_NISTP256,
    kex::ECDH_SHA2_NISTP384,
    kex::ECDH_SHA2_NISTP521,
    kex::DH_GEX_SHA256,
    kex::DH_G16_SHA512,
    kex::DH_G14_SHA256,
    // Protocol extensions, not real algorithms (strict KEX protects against Terrapin).
    kex::EXTENSION_SUPPORT_AS_CLIENT,
    kex::EXTENSION_OPENSSH_STRICT_KEX_AS_CLIENT,
];

/// Old Cisco IOS / Huawei VRP releases: SHA-1 key exchanges.
const LEGACY_KEX: &[kex::Name] = &[kex::DH_G14_SHA1, kex::DH_GEX_SHA1, kex::DH_G1_SHA1];

const MODERN_CIPHERS: &[cipher::Name] = &[
    cipher::CHACHA20_POLY1305,
    cipher::AES_256_GCM,
    cipher::AES_128_GCM,
    cipher::AES_256_CTR,
    cipher::AES_192_CTR,
    cipher::AES_128_CTR,
];

const LEGACY_CIPHERS: &[cipher::Name] = &[
    cipher::AES_256_CBC,
    cipher::AES_192_CBC,
    cipher::AES_128_CBC,
    cipher::TRIPLE_DES_CBC,
];

const MODERN_MACS: &[mac::Name] = &[
    mac::HMAC_SHA512_ETM,
    mac::HMAC_SHA256_ETM,
    mac::HMAC_SHA512,
    mac::HMAC_SHA256,
];

const LEGACY_MACS: &[mac::Name] = &[mac::HMAC_SHA1_ETM, mac::HMAC_SHA1];

/// Host key signature algorithms. Unlike russh's default list, plain `ssh-rsa`
/// (RSA signature with SHA-1) is *not* modern.
fn modern_keys() -> Vec<Algorithm> {
    vec![
        Algorithm::Ed25519,
        Algorithm::Ecdsa {
            curve: EcdsaCurve::NistP256,
        },
        Algorithm::Ecdsa {
            curve: EcdsaCurve::NistP384,
        },
        Algorithm::Ecdsa {
            curve: EcdsaCurve::NistP521,
        },
        Algorithm::Rsa {
            hash: Some(HashAlg::Sha512),
        },
        Algorithm::Rsa {
            hash: Some(HashAlg::Sha256),
        },
    ]
}

/// Modern algorithms first, then legacy ones: a recent server still gets a
/// modern negotiation even with the compatibility mode enabled.
pub fn preferred(legacy: bool) -> Preferred {
    // A generic function, not a closure: closures cannot be generic over `T`,
    // and the lists below have three different element types.
    fn extend<T: Clone>(legacy: bool, modern: &[T], old: &[T]) -> Vec<T> {
        if legacy {
            [modern, old].concat()
        } else {
            modern.to_vec()
        }
    }
    let mut keys = modern_keys();
    if legacy {
        keys.push(Algorithm::Rsa { hash: None });
    }
    Preferred {
        kex: Cow::Owned(extend(legacy, MODERN_KEX, LEGACY_KEX)),
        key: Cow::Owned(keys),
        cipher: Cow::Owned(extend(legacy, MODERN_CIPHERS, LEGACY_CIPHERS)),
        mac: Cow::Owned(extend(legacy, MODERN_MACS, LEGACY_MACS)),
        ..Preferred::DEFAULT
    }
}

/// Algorithms actually negotiated with the server, shown in the UI.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Negotiated {
    pub kex: String,
    pub host_key: String,
    pub cipher: String,
    pub mac: String,
    /// Subset of the above that is considered legacy (empty = all modern).
    pub legacy: Vec<String>,
}

impl Negotiated {
    pub fn from_names(names: &russh::Names) -> Self {
        let kex = names.kex.as_ref().to_string();
        let host_key = names.key.as_str().to_string();
        let cipher = names.cipher.as_ref().to_string();
        // With AEAD ciphers (GCM, ChaCha20-Poly1305) the MAC is not used.
        let aead = cipher.contains("gcm") || cipher.contains("poly1305");
        let mac = if aead {
            "(intégré au chiffrement)".to_string()
        } else {
            names.client_mac.as_ref().to_string()
        };

        let mut legacy = Vec::new();
        if LEGACY_KEX.iter().any(|k| k.as_ref() == kex) {
            legacy.push(kex.clone());
        }
        if names.key == (Algorithm::Rsa { hash: None }) {
            legacy.push(host_key.clone());
        }
        if LEGACY_CIPHERS.iter().any(|c| c.as_ref() == cipher) {
            legacy.push(cipher.clone());
        }
        if !aead && LEGACY_MACS.iter().any(|m| m.as_ref() == mac) {
            legacy.push(mac.clone());
        }
        Self {
            kex,
            host_key,
            cipher,
            mac,
            legacy,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modern_profile_offers_no_legacy_algorithm() {
        let p = preferred(false);
        assert!(!p.kex.iter().any(|k| LEGACY_KEX.contains(k)));
        assert!(!p.cipher.iter().any(|c| LEGACY_CIPHERS.contains(c)));
        assert!(!p.mac.iter().any(|m| LEGACY_MACS.contains(m)));
        assert!(
            !p.key.contains(&Algorithm::Rsa { hash: None }),
            "no ssh-rsa (SHA-1)"
        );
    }

    #[test]
    fn compatibility_mode_appends_legacy_after_modern() {
        let p = preferred(true);
        assert_eq!(p.kex[0], MODERN_KEX[0], "modern stays preferred");
        assert!(p.kex.contains(&kex::DH_G1_SHA1));
        assert!(p.cipher.contains(&cipher::TRIPLE_DES_CBC));
        assert!(p.mac.contains(&mac::HMAC_SHA1));
        assert_eq!(p.key.last(), Some(&Algorithm::Rsa { hash: None }));
    }
}
