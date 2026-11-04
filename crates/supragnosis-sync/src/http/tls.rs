//! TLS for the sync transport's clients: the certificates a link trusts, and the one a join pins.

use super::*;

/// TLS that trusts the certificates in `ca_pem` (sync-correctness.md Section 10), two ways.
///
/// - A server that presents one of them exactly is that certificate's host, and is trusted as it.
///   This is the self-signed hub: its `tls_cert`, copied to the spoke. `openssl req -x509` marks
///   such a certificate as a CA by default, and a verifier that only builds chains refuses a CA
///   presented as the server's own certificate (`CaUsedAsEndEntity`) - the sandbox's hub did.
/// - Any other server certificate must chain to one of them, with its name checked: the file is
///   then a CA bundle.
///
/// Either way the handshake's signatures are verified against the certificate presented, so a
/// server that does not hold the key of the certificate it shows is refused.
///
/// Shared with every client that names a hub's certificate: the sync link, a remote MCP profile's
/// bridge, and `server`'s check of it - all reach the same `[server]` listener.
pub fn tls_trusting(ca_pem: &[u8]) -> Result<rustls::ClientConfig, TransportError> {
    use rustls::pki_types::{pem::PemObject, CertificateDer};
    let certs: Vec<CertificateDer<'static>> = CertificateDer::pem_slice_iter(ca_pem)
        .collect::<Result<_, _>>()
        .map_err(|e| TransportError::Tls(format!("the ca file is not PEM certificates: {e}")))?;
    if certs.is_empty() {
        return Err(TransportError::Tls("the ca file holds no certificate".into()));
    }
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let mut roots = rustls::RootCertStore::empty();
    for cert in &certs {
        // One webpki cannot take as a root is still trusted by exact match below.
        let _ = roots.add(cert.clone());
    }
    let chain = rustls::client::WebPkiServerVerifier::builder_with_provider(
        Arc::new(roots),
        provider.clone(),
    )
    .build()
    .ok();
    let verifier = Arc::new(NamedCertificates { pinned: certs, chain, provider: provider.clone() });
    Ok(rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| TransportError::Tls(e.to_string()))?
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth())
}

/// The DER of the first certificate in `pem` - what a pairing code's fingerprint is taken over
/// (docs/pairing.md Section 4.2).
pub fn certificate_der(pem: &[u8]) -> Result<Vec<u8>, TransportError> {
    use rustls::pki_types::{pem::PemObject, CertificateDer};
    CertificateDer::pem_slice_iter(pem)
        .next()
        .ok_or_else(|| TransportError::Tls("the PEM holds no certificate".into()))?
        .map(|c| c.as_ref().to_vec())
        .map_err(|e| TransportError::Tls(format!("the certificate is not PEM: {e}")))
}

#[derive(Debug)]
struct NamedCertificates {
    pinned: Vec<rustls::pki_types::CertificateDer<'static>>,
    chain: Option<Arc<rustls::client::WebPkiServerVerifier>>,
    provider: Arc<rustls::crypto::CryptoProvider>,
}

impl rustls::client::danger::ServerCertVerifier for NamedCertificates {
    fn verify_server_cert(
        &self,
        end_entity: &rustls::pki_types::CertificateDer<'_>,
        intermediates: &[rustls::pki_types::CertificateDer<'_>],
        server_name: &rustls::pki_types::ServerName<'_>,
        ocsp_response: &[u8],
        now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        if self.pinned.iter().any(|c| c.as_ref() == end_entity.as_ref()) {
            return Ok(rustls::client::danger::ServerCertVerified::assertion());
        }
        match &self.chain {
            Some(v) => {
                v.verify_server_cert(end_entity, intermediates, server_name, ocsp_response, now)
            }
            None => Err(rustls::Error::InvalidCertificate(rustls::CertificateError::UnknownIssuer)),
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.provider.signature_verification_algorithms.supported_schemes()
    }
}

/// Says yes to the DER of the one certificate a pin names.
pub type AcceptCertificate = Arc<dyn Fn(&[u8]) -> bool + Send + Sync>;

/// The HTTP client a join uses (docs/pairing.md Section 4.3): it accepts exactly the certificate the
/// code's fingerprint names, from any address - `accept` says yes to its DER.
pub fn join_client(accept: AcceptCertificate) -> Result<reqwest::Client, TransportError> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    // A first address that drops packets - the LAN one, off the LAN - is given up on quickly, so
    // the next is tried well inside the code's ten minutes.
    let builder = reqwest::Client::builder()
        .timeout(CALL_TIMEOUT)
        .connect_timeout(std::time::Duration::from_secs(5));
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let verifier = Arc::new(PinnedBy { accept, provider: provider.clone() });
    let tls = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| TransportError::Tls(e.to_string()))?
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth();
    Ok(builder.use_preconfigured_tls(tls).build()?)
}

/// Accepts the one certificate a pin names, whatever address presented it; the handshake's
/// signatures are still verified against it, so a server without its key is refused.
struct PinnedBy {
    accept: AcceptCertificate,
    provider: Arc<rustls::crypto::CryptoProvider>,
}

impl std::fmt::Debug for PinnedBy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PinnedBy")
    }
}

impl rustls::client::danger::ServerCertVerifier for PinnedBy {
    fn verify_server_cert(
        &self,
        end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        match (self.accept)(end_entity.as_ref()) {
            true => Ok(rustls::client::danger::ServerCertVerified::assertion()),
            false => Err(rustls::Error::General(NOT_THE_CODES_HUB.into())),
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.provider.signature_verification_algorithms.supported_schemes()
    }
}
