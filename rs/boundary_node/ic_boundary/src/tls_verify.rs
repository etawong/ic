use std::{sync::Arc, time::SystemTime};

use arc_swap::ArcSwapOption;
use rustls::{
    client::{ServerCertVerified, ServerCertVerifier},
    Certificate, CertificateError, Error as RustlsError, ServerName,
};
use x509_parser::{
    prelude::{FromDer, X509Certificate},
    time::ASN1Time,
};

use crate::snapshot::RegistrySnapshot;

pub struct TlsVerifier {
    rs: Arc<ArcSwapOption<RegistrySnapshot>>,
}

impl TlsVerifier {
    pub fn new(rs: Arc<ArcSwapOption<RegistrySnapshot>>) -> Self {
        Self { rs }
    }
}

// Implement the certificate verifier which ensures that the certificate
// that was provided by node during TLS handshake matches its public key from the registry
// This trait is used by Rustls in reqwest under the hood
// We don't really check CommonName since the resolver makes sure we connect to the right IP
impl ServerCertVerifier for TlsVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &Certificate,
        _intermediates: &[Certificate],
        server_name: &ServerName,
        _scts: &mut dyn Iterator<Item = &[u8]>,
        _ocsp_response: &[u8],
        now: SystemTime,
    ) -> Result<ServerCertVerified, RustlsError> {
        // Load a routing table if we have one
        let rs = self
            .rs
            .load_full()
            .ok_or_else(|| RustlsError::General("no routing table published".into()))?;

        // Look up a node in the routing table based on the hostname provided by rustls
        let node = match server_name {
            // Currently support only DnsName
            ServerName::DnsName(v) => {
                match rs.nodes.get(v.as_ref()) {
                    // If the requested node is not in the routing table
                    None => {
                        return Err(RustlsError::General(format!(
                            "Node '{}' not found in a routing table",
                            v.as_ref()
                        )));
                    }

                    // Found
                    Some(v) => v,
                }
            }

            // Unsupported for now, can be removed later if not needed at all
            ServerName::IpAddress(_) => return Err(RustlsError::UnsupportedNameType),

            // Enum is marked non_exhaustive
            &_ => return Err(RustlsError::UnsupportedNameType),
        };

        // Cert is parsed & checked when we read it from the registry - if we got here then it's correct
        // It's a zero-copy view over byte array
        // Storing X509Certificate directly in Node is problematic since it does not own the data
        let (_, node_cert) = X509Certificate::from_der(&node.tls_certificate).unwrap();

        // Parse the certificate provided by server
        let (_, provided_cert) = X509Certificate::from_der(&end_entity.0)
            .map_err(|_x| RustlsError::InvalidCertificate(CertificateError::BadEncoding))?;

        // Verify the provided self-signed certificate using the public key from registry
        provided_cert
            .verify_signature(Some(&node_cert.tbs_certificate.subject_pki))
            .map_err(|_x| RustlsError::InvalidCertificate(CertificateError::BadSignature))?;

        // Check if the certificate is valid at provided `now` time
        if !provided_cert.validity.is_valid_at(
            ASN1Time::from_timestamp(
                now.duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap()
                    .as_secs() as i64,
            )
            .unwrap(),
        ) {
            return Err(RustlsError::InvalidCertificate(CertificateError::Expired));
        }

        Ok(ServerCertVerified::assertion())
    }
}

#[cfg(test)]
pub mod test;
