//! TLS helpers for connecting from the reverse-socks agent to the relay.
//!
//! TLS is opt-in (`--tls`). Verification follows the usual rules unless
//! `--tls-insecure` is set, in which case any server cert is accepted —
//! useful for self-signed deployments and lab testing only.
use std::fs::File;
use std::io::{self, BufReader};
use std::sync::Arc;

use rustls::ClientConfig;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{DigitallySignedStruct, RootCertStore, SignatureScheme};
use tokio_rustls::TlsConnector;

pub fn build_connector(ca_path: Option<&str>, insecure: bool) -> io::Result<TlsConnector> {
    let config = if insecure {
        ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(NoVerify))
            .with_no_client_auth()
    } else {
        let mut roots = RootCertStore::empty();
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        if let Some(path) = ca_path {
            for cert in load_certs(path)? {
                roots.add(cert).map_err(|e| {
                    io::Error::new(io::ErrorKind::InvalidData, format!("bad CA cert: {e}"))
                })?;
            }
        }
        ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth()
    };
    Ok(TlsConnector::from(Arc::new(config)))
}

/// Parse `host:port` and return the host portion as a `ServerName` for SNI.
/// Falls back to whatever the user passes via `--tls-domain`.
pub fn server_name(domain: &str) -> io::Result<ServerName<'static>> {
    ServerName::try_from(domain.to_string())
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, format!("bad TLS domain: {e}")))
}

fn load_certs(path: &str) -> io::Result<Vec<CertificateDer<'static>>> {
    let mut reader = BufReader::new(File::open(path)?);
    rustls_pemfile::certs(&mut reader).collect()
}

/// Verifier that accepts any server certificate. Used by `--tls-insecure`.
#[derive(Debug)]
struct NoVerify;

impl ServerCertVerifier for NoVerify {
    fn verify_server_cert(
        &self,
        _: &CertificateDer<'_>,
        _: &[CertificateDer<'_>],
        _: &ServerName<'_>,
        _: &[u8],
        _: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _: &[u8],
        _: &CertificateDer<'_>,
        _: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _: &[u8],
        _: &CertificateDer<'_>,
        _: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![
            SignatureScheme::RSA_PKCS1_SHA256,
            SignatureScheme::RSA_PKCS1_SHA384,
            SignatureScheme::RSA_PKCS1_SHA512,
            SignatureScheme::ECDSA_NISTP256_SHA256,
            SignatureScheme::ECDSA_NISTP384_SHA384,
            SignatureScheme::ECDSA_NISTP521_SHA512,
            SignatureScheme::RSA_PSS_SHA256,
            SignatureScheme::RSA_PSS_SHA384,
            SignatureScheme::RSA_PSS_SHA512,
            SignatureScheme::ED25519,
        ]
    }
}
