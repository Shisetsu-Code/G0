//! Opaque native password verifiers. No API exports verifier bytes or originals.
use crate::{gir::SemanticType, store_engine::StoreError, value::Value};
use ring::{
    pbkdf2,
    rand::{SecureRandom, SystemRandom},
};
use std::{fmt, num::NonZeroU32};

const ITERATIONS: NonZeroU32 = NonZeroU32::new(600_000).unwrap();
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Verifier {
    text: bool,
    salt: [u8; 16],
    derived: [u8; 32],
}
impl fmt::Debug for Verifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("CredentialVerifier(<redacted>)")
    }
}
impl Verifier {
    fn input(value: &Value) -> Result<(bool, &[u8]), StoreError> {
        let Value::Credential(value) = value else {
            return Err(StoreError::TypeMismatch);
        };
        let (text, bytes) = match value.as_ref() {
            Value::Text(text) => (true, text.as_bytes()),
            Value::Bytes(bytes) => (false, bytes.as_ref()),
            _ => return Err(StoreError::TypeMismatch),
        };
        if bytes.is_empty() || bytes.len() > 4096 {
            return Err(StoreError::Limit);
        }
        Ok((text, bytes))
    }
    pub(crate) fn create(value: &Value) -> Result<Self, StoreError> {
        let (text, bytes) = Self::input(value)?;
        let mut salt = [0u8; 16];
        SystemRandom::new()
            .fill(&mut salt)
            .map_err(|_| StoreError::Entropy)?;
        let mut derived = [0u8; 32];
        pbkdf2::derive(
            pbkdf2::PBKDF2_HMAC_SHA256,
            ITERATIONS,
            &salt,
            bytes,
            &mut derived,
        );
        Ok(Self {
            text,
            salt,
            derived,
        })
    }
    pub(crate) fn verify(&self, value: &Value) -> Result<bool, StoreError> {
        let (text, bytes) = Self::input(value)?;
        if text != self.text {
            return Err(StoreError::TypeMismatch);
        }
        Ok(pbkdf2::verify(
            pbkdf2::PBKDF2_HMAC_SHA256,
            ITERATIONS,
            &self.salt,
            bytes,
            &self.derived,
        )
        .is_ok())
    }
    pub(crate) fn fits(&self, ty: &SemanticType) -> bool {
        matches!(ty, SemanticType::Text) && self.text
            || matches!(ty, SemanticType::Bytes) && !self.text
    }
    pub(crate) fn encode(&self) -> [u8; 54] {
        let mut bytes = [0u8; 54];
        bytes[0] = 1; // fixed PBKDF2-HMAC-SHA256 profile
        bytes[1] = u8::from(self.text);
        bytes[2..6].copy_from_slice(&ITERATIONS.get().to_le_bytes());
        bytes[6..22].copy_from_slice(&self.salt);
        bytes[22..].copy_from_slice(&self.derived);
        bytes
    }
    pub(crate) fn decode(bytes: &[u8]) -> Result<Self, StoreError> {
        if bytes.len() != 54
            || bytes[0] != 1
            || bytes[1] > 1
            || bytes[2..6] != ITERATIONS.get().to_le_bytes()
        {
            return Err(StoreError::Integrity);
        }
        Ok(Self {
            text: bytes[1] == 1,
            salt: bytes[6..22].try_into().unwrap(),
            derived: bytes[22..].try_into().unwrap(),
        })
    }
}
