//! TLS for the DSO link in the spirit of IEC 62351-3: TLS 1.2/1.3, the
//! station authenticates with its certificate, and the control centre must
//! present a client certificate signed by the DSO's CA — an attacker who can
//! reach port 2404 cannot even complete the handshake, let alone send commands.

use std::sync::Arc;

use rustls_pki_types::pem::PemObject;
use rustls_pki_types::{CertificateDer, PrivateKeyDer};
use tokio_rustls::TlsAcceptor;
use tokio_rustls::rustls::server::WebPkiClientVerifier;
use tokio_rustls::rustls::{self, RootCertStore, ServerConfig};

use crate::config::Tls;

pub fn acceptor(cfg: &Tls) -> Result<TlsAcceptor, String> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());

    let certs: Vec<CertificateDer<'static>> = CertificateDer::pem_file_iter(&cfg.cert)
        .and_then(|it| it.collect())
        .map_err(|e| format!("{}: {e}", cfg.cert.display()))?;
    let key = PrivateKeyDer::from_pem_file(&cfg.key).map_err(|e| format!("{}: {e}", cfg.key.display()))?;

    let mut roots = RootCertStore::empty();
    for ca in CertificateDer::pem_file_iter(&cfg.client_ca).map_err(|e| format!("{}: {e}", cfg.client_ca.display()))? {
        let ca = ca.map_err(|e| format!("{}: {e}", cfg.client_ca.display()))?;
        roots.add(ca).map_err(|e| format!("{}: {e}", cfg.client_ca.display()))?;
    }
    let verifier = WebPkiClientVerifier::builder_with_provider(Arc::new(roots), provider.clone())
        .build()
        .map_err(|e| format!("client verifier: {e}"))?;

    let config = ServerConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13, &rustls::version::TLS12])
        .map_err(|e| e.to_string())?
        .with_client_cert_verifier(verifier)
        .with_single_cert(certs, key)
        .map_err(|e| format!("station certificate: {e}"))?;
    Ok(TlsAcceptor::from(Arc::new(config)))
}
