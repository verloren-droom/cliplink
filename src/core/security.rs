use std::sync::Arc;

use rcgen::generate_simple_self_signed;
use rustls::{
    ClientConfig as RustlsClientConfig, DigitallySignedStruct, DistinguishedName,
    Error as RustlsError, ServerConfig as RustlsServerConfig, SignatureScheme,
    client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    crypto::{CryptoProvider, verify_tls12_signature, verify_tls13_signature},
    pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName, UnixTime},
    server::danger::{ClientCertVerified, ClientCertVerifier},
};
use sha2::{Digest, Sha256};

use crate::{
    constants::{
        app::LOCAL_HOSTNAME,
        crypto::{DEVICE_CERT_PURPOSE, DEVICE_KEY_PURPOSE},
    },
    core::{
        at_rest::{LocalDataCipher, load_or_legacy_bytes, save_sealed_bytes},
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

impl DeviceIdentity {
    pub fn certificate(&self) -> CertificateDer<'static> {
        CertificateDer::from(self.certificate_der.clone())
    }

    pub fn private_key(&self) -> PrivateKeyDer<'static> {
        PrivatePkcs8KeyDer::from(self.private_key_der.clone()).into()
    }
}

pub fn load_or_create_identity(
    paths: &AppPaths,
    device_id: &str,
    cipher: &LocalDataCipher,
) -> AppResult<DeviceIdentity> {
    if paths.cert_der.exists() && paths.key_der.exists() {
        let (certificate_der, legacy_cert) =
            load_or_legacy_bytes(&paths.cert_der, DEVICE_CERT_PURPOSE, cipher)?;
        let (private_key_der, legacy_key) =
            load_or_legacy_bytes(&paths.key_der, DEVICE_KEY_PURPOSE, cipher)?;
        if legacy_cert {
            save_sealed_bytes(
                &paths.cert_der,
                DEVICE_CERT_PURPOSE,
                &certificate_der,
                cipher,
            )?;
        }
        if legacy_key {
            save_sealed_bytes(&paths.key_der, DEVICE_KEY_PURPOSE, &private_key_der, cipher)?;
        }
        return Ok(DeviceIdentity {
            fingerprint: certificate_fingerprint(&certificate_der),
            certificate_der,
            private_key_der,
        });
    }

    let certified = generate_simple_self_signed(vec![
        LOCAL_HOSTNAME.to_string(),
        "localhost".to_string(),
        device_id.to_string(),
    ])?;
    let certificate_der = certified.cert.der().to_vec();
    let private_key_der = certified.key_pair.serialize_der();

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
