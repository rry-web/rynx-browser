//! Encrypted Client Hello (ECH): the name of the site being visited, which the start of a TLS
//! connection otherwise shows anyone on the way, is encrypted to a key the site publishes in DNS
//! (the "ech" of its HTTPS record). Used only with encrypted DNS: from ordinary DNS, anyone on
//! the way could remove the key, or swap in their own.
//!
//! rustls does ECH, but ships its HPKE encryption only with aws-lc-rs, a C library rynx's
//! builds avoid; here it comes from the hpke crate (pure Rust, RFC 9180), for the suites sites
//! publish: X25519 with AES-128-GCM, AES-256-GCM or ChaCha20-Poly1305.

use hpke::aead::{Aead, AeadCtxR, AeadCtxS, AesGcm128, AesGcm256, ChaCha20Poly1305};
use hpke::kdf::HkdfSha256;
use hpke::kem::X25519HkdfSha256;
use hpke::{Deserializable, Kem, OpModeR, OpModeS, Serializable};
use rustls::crypto::hpke::{
    EncapsulatedSecret, Hpke, HpkeOpener, HpkePrivateKey, HpkePublicKey, HpkeSealer, HpkeSuite,
};
use rustls::internal::msgs::enums::{HpkeAead, HpkeKdf, HpkeKem};
use rustls::internal::msgs::handshake::HpkeSymmetricCipherSuite;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How long a site's key (or its having none) is remembered
const KEY_LIFETIME: Duration = Duration::from_secs(10 * 60);

/// The HPKE suites rynx can encrypt to
pub static SUITES: &[&dyn Hpke] = &[&AES_128, &AES_256, &CHACHA20];
static AES_128: X25519Suite = X25519Suite { aead: HpkeAead::AES_128_GCM };
static AES_256: X25519Suite = X25519Suite { aead: HpkeAead::AES_256_GCM };
static CHACHA20: X25519Suite = X25519Suite { aead: HpkeAead::CHACHA20_POLY_1305 };

/// HPKE with X25519 and HKDF-SHA256, and one of the AEADs
#[derive(Debug)]
struct X25519Suite {
    aead: HpkeAead,
}

type Kdf = HkdfSha256;
type Dh = X25519HkdfSha256;

fn failed(e: hpke::HpkeError) -> rustls::Error {
    rustls::Error::General(format!("HPKE: {}", e))
}

/// Calls `$body` with `$A` as the suite's AEAD type
macro_rules! with_aead {
    ($suite:expr, $A:ident => $body:expr) => {
        match $suite.aead {
            HpkeAead::AES_128_GCM => {
                type $A = AesGcm128;
                $body
            }
            HpkeAead::AES_256_GCM => {
                type $A = AesGcm256;
                $body
            }
            _ => {
                type $A = ChaCha20Poly1305;
                $body
            }
        }
    };
}

struct Sealer<A: Aead>(AeadCtxS<A, Kdf, Dh>);
struct Opener<A: Aead>(AeadCtxR<A, Kdf, Dh>);

impl<A: Aead> std::fmt::Debug for Sealer<A> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Sealer")
    }
}

impl<A: Aead> std::fmt::Debug for Opener<A> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Opener")
    }
}

impl<A: Aead + Send + Sync + 'static> HpkeSealer for Sealer<A>
where
    AeadCtxS<A, Kdf, Dh>: Send + Sync,
{
    fn seal(&mut self, aad: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, rustls::Error> {
        self.0.seal(plaintext, aad).map_err(failed)
    }
}

impl<A: Aead + Send + Sync + 'static> HpkeOpener for Opener<A>
where
    AeadCtxR<A, Kdf, Dh>: Send + Sync,
{
    fn open(&mut self, aad: &[u8], ciphertext: &[u8]) -> Result<Vec<u8>, rustls::Error> {
        self.0.open(ciphertext, aad).map_err(failed)
    }
}

fn public_key(key: &HpkePublicKey) -> Result<<Dh as Kem>::PublicKey, rustls::Error> {
    <Dh as Kem>::PublicKey::from_bytes(&key.0).map_err(failed)
}

impl Hpke for X25519Suite {
    fn seal(
        &self,
        info: &[u8],
        aad: &[u8],
        plaintext: &[u8],
        pub_key: &HpkePublicKey,
    ) -> Result<(EncapsulatedSecret, Vec<u8>), rustls::Error> {
        let (enc, mut sealer) = self.setup_sealer(info, pub_key)?;
        Ok((enc, sealer.seal(aad, plaintext)?))
    }

    fn setup_sealer(
        &self,
        info: &[u8],
        pub_key: &HpkePublicKey,
    ) -> Result<(EncapsulatedSecret, Box<dyn HpkeSealer + 'static>), rustls::Error> {
        let key = public_key(pub_key)?;
        with_aead!(self, A => {
            let (enc, context) = hpke::setup_sender::<A, Kdf, Dh>(&OpModeS::Base, &key, info).map_err(failed)?;
            Ok((EncapsulatedSecret(enc.to_bytes().to_vec()), Box::new(Sealer(context)) as Box<dyn HpkeSealer>))
        })
    }

    fn open(
        &self,
        enc: &EncapsulatedSecret,
        info: &[u8],
        aad: &[u8],
        ciphertext: &[u8],
        secret_key: &HpkePrivateKey,
    ) -> Result<Vec<u8>, rustls::Error> {
        self.setup_opener(enc, info, secret_key)?.open(aad, ciphertext)
    }

    fn setup_opener(
        &self,
        enc: &EncapsulatedSecret,
        info: &[u8],
        secret_key: &HpkePrivateKey,
    ) -> Result<Box<dyn HpkeOpener + 'static>, rustls::Error> {
        let key = <Dh as Kem>::PrivateKey::from_bytes(secret_key.secret_bytes()).map_err(failed)?;
        let enc = <Dh as Kem>::EncappedKey::from_bytes(&enc.0).map_err(failed)?;
        with_aead!(self, A => {
            let context = hpke::setup_receiver::<A, Kdf, Dh>(&OpModeR::Base, &key, &enc, info).map_err(failed)?;
            Ok(Box::new(Opener(context)) as Box<dyn HpkeOpener>)
        })
    }

    fn generate_key_pair(&self) -> Result<(HpkePublicKey, HpkePrivateKey), rustls::Error> {
        let (private, public) = <Dh as Kem>::gen_keypair();
        Ok((HpkePublicKey(public.to_bytes().to_vec()), HpkePrivateKey::from(private.to_bytes().to_vec())))
    }

    fn suite(&self) -> HpkeSuite {
        HpkeSuite {
            kem: HpkeKem::DHKEM_X25519_HKDF_SHA256,
            sym: HpkeSymmetricCipherSuite { kdf_id: HpkeKdf::HKDF_SHA256, aead_id: self.aead },
        }
    }
}

/// A site's ECH key (None: it publishes none), and when it was looked up
type Key = (Option<Arc<Vec<u8>>>, Instant);

/// Sites' ECH keys, by name
static KEYS: Mutex<Option<HashMap<String, Key>>> = Mutex::new(None);

/// The ECH key `host` publishes in its HTTPS record, looked up over encrypted DNS; None without
/// encrypted DNS, or if it publishes none
pub async fn key_for(host: &str) -> Option<Arc<Vec<u8>>> {
    use hickory_resolver::proto::rr::RData;
    use hickory_resolver::proto::rr::rdata::svcb::SvcParamValue;

    let resolver = crate::dns::encrypted().await?;
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    if let Some((key, at)) = KEYS.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_default().get(&host)
        && at.elapsed() < KEY_LIFETIME
    {
        return key.clone();
    }
    let answer = resolver.lookup(format!("{}.", host), hickory_resolver::proto::rr::RecordType::HTTPS).await;
    let key = answer.ok().and_then(|lookup| {
        lookup.answers().iter().find_map(|record| match &record.data {
            RData::HTTPS(https) => https.0.svc_params.iter().find_map(|(_, value)| match value {
                SvcParamValue::EchConfigList(list) if !list.0.is_empty() => Some(Arc::new(list.0.clone())),
                _ => None,
            }),
            _ => None,
        })
    });
    KEYS.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_default().insert(host, (key.clone(), Instant::now()));
    key
}

/// Forget that `host` has an ECH key, after it refused one: it's reached as before until the key
/// is looked up again
pub fn forget(host: &str) {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    KEYS.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_default().insert(host, (None, Instant::now()));
}

/// TLS settings encrypting the site's name to its ECH `key` (TLS 1.3 only, as ECH requires),
/// otherwise as rynx's clearweb connections are
pub fn tls_config(key: &[u8]) -> Result<rustls::ClientConfig, String> {
    use rustls_platform_verifier::BuilderVerifierExt;
    let config = rustls::client::EchConfig::new(rustls::pki_types::EchConfigListBytes::from(key.to_vec()), SUITES)
        .map_err(|e| format!("The site's ECH key can't be used: {}", e))?;
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let mut tls = rustls::ClientConfig::builder_with_provider(provider)
        .with_ech(rustls::client::EchMode::Enable(config))
        .map_err(|e| e.to_string())?
        .with_platform_verifier()
        .map_err(|e| e.to_string())?
        .with_no_client_auth();
    tls.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    Ok(tls)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each suite's encryption opens again with the matching private key, and rustls accepts them
    #[test]
    fn suites_seal_and_open() {
        for suite in SUITES {
            let (public, private) = suite.generate_key_pair().unwrap();
            let (enc, sealed) = suite.seal(b"info", b"aad", b"the site's name", &public).unwrap();
            assert_ne!(&sealed[..15], b"the site's name");
            assert_eq!(suite.open(&enc, b"info", b"aad", &sealed, &private).unwrap(), b"the site's name");
            assert!(suite.open(&enc, b"other", b"aad", &sealed, &private).is_err(), "bound to its info");

            // A context seals several messages, each opening in turn
            let (enc, mut sealer) = suite.setup_sealer(b"info", &public).unwrap();
            let mut opener = suite.setup_opener(&enc, b"info", &private).unwrap();
            for message in [&b"one"[..], b"two"] {
                let sealed = sealer.seal(b"", message).unwrap();
                assert_eq!(opener.open(b"", &sealed).unwrap(), message);
            }
        }
    }

    /// RFC 9180's test vector A.1.1 (X25519, HKDF-SHA256, AES-128-GCM, base mode): the recipient's
    /// key opens the vector's first ciphertext, so the suite is the standard one
    #[test]
    fn rfc_9180_test_vector_opens() {
        let hex = |s: &str| (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect();
        let enc: Vec<u8> = hex("37fda3567bdbd628e88668c3c8d7e97d1d1253b6d4ea6d44c150f741f1bf4431");
        let private: Vec<u8> = hex("4612c550263fc8ad58375df3f557aac531d26850903e55a9f23f21d8534e8ac8");
        let info: Vec<u8> = hex("4f6465206f6e2061204772656369616e2055726e");
        let aad: Vec<u8> = hex("436f756e742d30");
        let sealed: Vec<u8> =
            hex("f938558b5d72f1a23810b4be2ab4f84331acc02fc97babc53a52ae8218a355a96d8770ac83d07bea87e13c512a");
        let plaintext =
            AES_128.open(&EncapsulatedSecret(enc), &info, &aad, &sealed, &HpkePrivateKey::from(private)).unwrap();
        assert_eq!(plaintext, b"Beauty is truth, truth beauty");
    }
}
