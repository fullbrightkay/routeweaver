use crate::{config::Keys, error::RouteWeaverError};
use data_encoding::HEXLOWER_PERMISSIVE;
use routeweaver_common::PublicKey;
use serde::{Deserialize, Serialize};
use snow::{params::NoiseParams, HandshakeState};
use std::{fmt::Display, str::FromStr, sync::LazyLock};
use zeroize::ZeroizeOnDrop;

// Pattern used, unlikely to change
static NOISE_PATTERN: LazyLock<NoiseParams> =
    LazyLock::new(|| "Noise_XX_25519_ChaChaPoly_BLAKE2s".parse().unwrap());

pub fn generate_keys() -> Keys {
    let keypair = snow::Builder::new(NOISE_PATTERN.clone())
        .generate_keypair()
        .unwrap();

    Keys {
        public: PublicKey::new(keypair.public.try_into().unwrap()),
        private: PrivateKey::new(keypair.private.try_into().unwrap()),
    }
}

pub fn create_handshake_responder(key: &PrivateKey) -> HandshakeState {
    snow::Builder::new(NOISE_PATTERN.clone())
        .local_private_key(key.as_ref())
        .build_responder()
        .unwrap()
}

pub fn create_handshake_initiator(key: &PrivateKey) -> HandshakeState {
    snow::Builder::new(NOISE_PATTERN.clone())
        .local_private_key(key.as_ref())
        .build_initiator()
        .unwrap()
}

#[derive(Serialize, Deserialize, Debug, ZeroizeOnDrop)]
pub struct PrivateKey([u8; 32]);

impl PrivateKey {
    pub fn new(key: [u8; 32]) -> Self {
        PrivateKey(key)
    }
}

impl AsRef<[u8]> for PrivateKey {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl Display for PrivateKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&HEXLOWER_PERMISSIVE.encode(&self.0))
    }
}

impl FromStr for PrivateKey {
    type Err = RouteWeaverError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(PrivateKey(
            HEXLOWER_PERMISSIVE
                .decode(s.as_bytes())
                .map_err(|_| RouteWeaverError::InvalidKey)?
                .try_into()
                .map_err(|_| RouteWeaverError::InvalidKey)?,
        ))
    }
}
