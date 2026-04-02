use std::sync::Arc;

use rcgen::{CertificateParams, KeyPair, PKCS_ED25519};
use ring::{
    rand::SystemRandom,
    signature::{
        ECDSA_P256_SHA256_ASN1_SIGNING, ECDSA_P384_SHA384_ASN1_SIGNING, EcdsaKeyPair,
        Ed25519KeyPair,
    },
};
use rustls::{
    ClientConfig as RustlsClientConfig, DigitallySignedStruct, DistinguishedName,
    Error as RustlsError, ServerConfig as RustlsServerConfig, SignatureScheme,
    client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    crypto::{CryptoProvider, verify_tls12_signature, verify_tls13_signature},
    pki_types::{
        CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName,
        SignatureVerificationAlgorithm, UnixTime,
    },
    server::danger::{ClientCertVerified, ClientCertVerifier},
};
use sha2::{Digest, Sha256};
use webpki::{EndEntityCert, ring as webpki_ring};

use crate::{
    constants::{
        app::LOCAL_HOSTNAME,
        crypto::{DEVICE_CERT_PURPOSE, DEVICE_KEY_PURPOSE},
    },
    core::{
        at_rest::{LocalDataCipher, load_sealed_bytes, save_sealed_bytes},
        error::{AppError, AppResult},
        paths::AppPaths,
    },
};

#[derive(Debug, Clone)]
pub struct DeviceIdentity {
    pub certificate_der: Vec<u8>,
    pub private_key_der: Vec<u8>,
    pub fingerprint: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageSignatureScheme {
    Ed25519,
    EcdsaP256Sha256Asn1,
    EcdsaP384Sha384Asn1,
}

#[derive(Debug, Clone)]
pub struct DetachedMessageSignature {
    pub certificate_der: Vec<u8>,
    pub scheme: MessageSignatureScheme,
    pub signature: Vec<u8>,
}

impl DeviceIdentity {
    pub fn certificate(&self) -> CertificateDer<'static> {
        CertificateDer::from(self.certificate_der.clone())
    }

    pub fn private_key(&self) -> PrivateKeyDer<'static> {
        PrivatePkcs8KeyDer::from(self.private_key_der.clone()).into()
    }
}

impl MessageSignatureScheme {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ed25519 => "ed25519",
            Self::EcdsaP256Sha256Asn1 => "ecdsa_p256_sha256_asn1",
            Self::EcdsaP384Sha384Asn1 => "ecdsa_p384_sha384_asn1",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim() {
            "ed25519" => Some(Self::Ed25519),
            "ecdsa_p256_sha256_asn1" => Some(Self::EcdsaP256Sha256Asn1),
            "ecdsa_p384_sha384_asn1" => Some(Self::EcdsaP384Sha384Asn1),
            _ => None,
        }
    }

    fn verification_algorithm(self) -> &'static dyn SignatureVerificationAlgorithm {
        match self {
            Self::Ed25519 => webpki_ring::ED25519,
            Self::EcdsaP256Sha256Asn1 => webpki_ring::ECDSA_P256_SHA256,
            Self::EcdsaP384Sha384Asn1 => webpki_ring::ECDSA_P384_SHA384,
        }
    }
}

pub fn sign_detached_message(
    identity: &DeviceIdentity,
    message: &[u8],
) -> AppResult<DetachedMessageSignature> {
    let rng = SystemRandom::new();
    if let Ok(key_pair) = Ed25519KeyPair::from_pkcs8(identity.private_key_der.as_slice()) {
        return Ok(DetachedMessageSignature {
            certificate_der: identity.certificate_der.clone(),
            scheme: MessageSignatureScheme::Ed25519,
            signature: key_pair.sign(message).as_ref().to_vec(),
        });
    }

    if let Ok(key_pair) = EcdsaKeyPair::from_pkcs8(
        &ECDSA_P256_SHA256_ASN1_SIGNING,
        identity.private_key_der.as_slice(),
        &rng,
    ) {
        return Ok(DetachedMessageSignature {
            certificate_der: identity.certificate_der.clone(),
            scheme: MessageSignatureScheme::EcdsaP256Sha256Asn1,
            signature: key_pair
                .sign(&rng, message)
                .map_err(|_| AppError::Crypto("Failed to sign detached message.".to_string()))?
                .as_ref()
                .to_vec(),
        });
    }

    if let Ok(key_pair) = EcdsaKeyPair::from_pkcs8(
        &ECDSA_P384_SHA384_ASN1_SIGNING,
        identity.private_key_der.as_slice(),
        &rng,
    ) {
        return Ok(DetachedMessageSignature {
            certificate_der: identity.certificate_der.clone(),
            scheme: MessageSignatureScheme::EcdsaP384Sha384Asn1,
            signature: key_pair
                .sign(&rng, message)
                .map_err(|_| AppError::Crypto("Failed to sign detached message.".to_string()))?
                .as_ref()
                .to_vec(),
        });
    }

    Err(AppError::Crypto(
        "Unsupported device identity key algorithm for detached message signing.".to_string(),
    ))
}

pub fn verify_detached_message_signature(
    certificate_der: &[u8],
    scheme: MessageSignatureScheme,
    message: &[u8],
    signature: &[u8],
) -> AppResult<()> {
    let certificate = CertificateDer::from(certificate_der.to_vec());
    let certificate = EndEntityCert::try_from(&certificate)
        .map_err(|error| AppError::Crypto(error.to_string()))?;
    certificate
        .verify_signature(scheme.verification_algorithm(), message, signature)
        .map_err(|error| AppError::Crypto(error.to_string()))
}

pub fn load_or_create_identity(
    paths: &AppPaths,
    device_id: &str,
    cipher: &LocalDataCipher,
) -> AppResult<DeviceIdentity> {
    paths.ensure()?;
    if paths.cert_der.exists() && paths.key_der.exists() {
        let certificate_der = load_sealed_bytes(&paths.cert_der, DEVICE_CERT_PURPOSE, cipher)?;
        let private_key_der = load_sealed_bytes(&paths.key_der, DEVICE_KEY_PURPOSE, cipher)?;
        return Ok(DeviceIdentity {
            fingerprint: certificate_fingerprint(&certificate_der),
            certificate_der,
            private_key_der,
        });
    }

    let params = CertificateParams::new(vec![
        LOCAL_HOSTNAME.to_string(),
        "localhost".to_string(),
        device_id.to_string(),
    ])?;
    let key_pair = KeyPair::generate_for(&PKCS_ED25519)?;
    let certified = params.self_signed(&key_pair)?;
    let certificate_der = certified.der().to_vec();
    let private_key_der = key_pair.serialize_der();

    save_sealed_bytes(
        &paths.cert_der,
        DEVICE_CERT_PURPOSE,
        &certificate_der,
        cipher,
    )?;
    save_sealed_bytes(&paths.key_der, DEVICE_KEY_PURPOSE, &private_key_der, cipher)?;

    Ok(DeviceIdentity {
        fingerprint: certificate_fingerprint(&certificate_der),
        certificate_der,
        private_key_der,
    })
}

pub fn certificate_fingerprint(cert: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(cert);
    let bytes = hasher.finalize();
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[derive(Debug)]
pub struct FingerprintServerVerifier {
    fingerprints: std::sync::Arc<parking_lot::RwLock<std::collections::HashSet<String>>>,
    provider: Arc<CryptoProvider>,
}

impl FingerprintServerVerifier {
    pub fn new(
        fingerprints: std::sync::Arc<parking_lot::RwLock<std::collections::HashSet<String>>>,
    ) -> Arc<Self> {
        Arc::new(Self {
            fingerprints,
            provider: Arc::new(rustls::crypto::ring::default_provider()),
        })
    }
}

impl ServerCertVerifier for FingerprintServerVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, RustlsError> {
        let fingerprint = certificate_fingerprint(end_entity.as_ref());
        if self.fingerprints.read().contains(&fingerprint) {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(RustlsError::General(
                "Untrusted peer certificate".to_string(),
            ))
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, RustlsError> {
        verify_tls12_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, RustlsError> {
        verify_tls13_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

#[derive(Debug)]
pub struct FingerprintClientVerifier {
    fingerprints: std::sync::Arc<parking_lot::RwLock<std::collections::HashSet<String>>>,
    provider: Arc<CryptoProvider>,
    hints: Vec<DistinguishedName>,
}

impl FingerprintClientVerifier {
    pub fn new(
        fingerprints: std::sync::Arc<parking_lot::RwLock<std::collections::HashSet<String>>>,
    ) -> Arc<Self> {
        Arc::new(Self {
            fingerprints,
            provider: Arc::new(rustls::crypto::ring::default_provider()),
            hints: Vec::new(),
        })
    }
}

impl ClientCertVerifier for FingerprintClientVerifier {
    fn root_hint_subjects(&self) -> &[DistinguishedName] {
        &self.hints
    }

    fn verify_client_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _now: UnixTime,
    ) -> Result<ClientCertVerified, RustlsError> {
        let fingerprint = certificate_fingerprint(end_entity.as_ref());
        if self.fingerprints.read().contains(&fingerprint) {
            Ok(ClientCertVerified::assertion())
        } else {
            Err(RustlsError::General(
                "Untrusted client certificate".to_string(),
            ))
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, RustlsError> {
        verify_tls12_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, RustlsError> {
        verify_tls13_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

pub fn build_rustls_server_config(
    identity: &DeviceIdentity,
    trusted_fingerprints: std::sync::Arc<parking_lot::RwLock<std::collections::HashSet<String>>>,
) -> AppResult<RustlsServerConfig> {
    RustlsServerConfig::builder()
        .with_client_cert_verifier(FingerprintClientVerifier::new(trusted_fingerprints))
        .with_single_cert(vec![identity.certificate()], identity.private_key())
        .map_err(|error| AppError::Crypto(error.to_string()))
}

pub fn build_rustls_client_config(
    identity: &DeviceIdentity,
    trusted_fingerprints: std::sync::Arc<parking_lot::RwLock<std::collections::HashSet<String>>>,
) -> AppResult<RustlsClientConfig> {
    RustlsClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(FingerprintServerVerifier::new(trusted_fingerprints))
        .with_client_auth_cert(vec![identity.certificate()], identity.private_key())
        .map_err(|error| AppError::Crypto(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detached_message_signature_roundtrip_works() {
        let temp_root =
            std::env::temp_dir().join(format!("cliplink-security-test-{}", uuid::Uuid::new_v4()));
        let paths = AppPaths::from_root(temp_root);
        paths.ensure().expect("paths");
        let cipher = LocalDataCipher::from_bytes([9_u8; 32]);
        let identity = load_or_create_identity(&paths, "test-device", &cipher).expect("identity");

        let signature = sign_detached_message(&identity, b"hello world").expect("sign");
        verify_detached_message_signature(
            &signature.certificate_der,
            signature.scheme,
            b"hello world",
            &signature.signature,
        )
        .expect("verify");
    }
}
