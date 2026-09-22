//! File:     stratum-networking/src/tls.rs
//! Project:  Stratum Networking
//! Author:   Jacob Chacko
//!
//! The server's TLS certificate and key, and the settings rustls needs to
//! use them.  tcp.rs asks for those settings every time the server starts.
//!
//! Two files, in constellations::ssl_folder():
//!
//!   - key.pem, the private key.  It never leaves this machine, and only our
//!     own user can read it.  Whoever has it can pretend to be us.
//!   - cert.pem, the certificate.  It holds the public half of the key, and
//!     it is the one clients get.  Probe keeps a copy and refuses any server
//!     that shows it a different one (that is "pinning").
//!
//! The first time the server starts there are neither, so we make both.
//! Self-signed, which means nobody but us vouches for it.  That is fine,
//! because the clients never ask anybody else -- they only trust the copy
//! they were given.
//!
//! If only one of the two is there, we refuse to start rather than make a
//! new pair.  A new certificate would quietly lock out every client that
//! pinned the old one.

use std::io;
use std::net::IpAddr;
use std::path::Path;
use std::sync::Arc;

use rcgen::{CertificateParams, DistinguishedName, DnType, KeyPair};
use rustls::ServerConfig;
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};

use stratum_tools::constellations;
use stratum_tools::diskman::{self, StratumFile};
use stratum_tools::scribe::{self, Channel};

const CERT_FILE: &str = "cert.pem";
const KEY_FILE: &str = "key.pem";

/// What the certificate says it was issued to.
const COMMON_NAME: &str = "Stratum";

/// Reads the certificate and key (making them the first time), and hands
/// back the settings every TLS connection gets built from.  An `Err` is a
/// sentence for the admin.  It never has any of the key in it.
pub fn server_config() -> Result<Arc<ServerConfig>, String> {
    let folder = constellations::ssl_folder();
    let cert_path = folder.join(CERT_FILE);
    let key_path = folder.join(KEY_FILE);

    let (cert_pem, key_pem) = match (read_if_there(&cert_path)?, read_if_there(&key_path)?) {
        (Some(cert), Some(key)) => (cert, key),
        (None, None) => make_pair(&folder, &cert_path, &key_path)?,
        (Some(_), None) => return Err(one_missing(&cert_path, &key_path)),
        (None, Some(_)) => return Err(one_missing(&key_path, &cert_path)),
    };

    let cert = CertificateDer::from_pem_slice(&cert_pem)
        .map_err(|error| format!("{} isn't a certificate we can read: {}",
                                 cert_path.display(), error))?;
    // No details on this one.  Whatever is wrong with the file, the reason
    // could quote part of it, and part of a key is still a key.
    let key = PrivateKeyDer::from_pem_slice(&key_pem)
        .map_err(|_| format!("{} isn't a private key we can read.", key_path.display()))?;

    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|error| format!("TLS won't start: {}", error))?
        .with_no_client_auth()
        .with_single_cert(vec![cert], key)
        .map_err(|error| format!("TLS won't use the files in {}: {}", folder.display(), error))?;
    Ok(Arc::new(config))
}

/// A file's bytes, or `None` if it isn't there.  Anything else that goes
/// wrong is an `Err`, and DiskMan has already logged the details.
fn read_if_there(path: &Path) -> Result<Option<Vec<u8>>, String> {
    match diskman::read_file(path) {
        Ok(file) => Ok(Some(file.contents)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("Couldn't read {}: {}", path.display(), error)),
    }
}

fn one_missing(found: &Path, missing: &Path) -> String {
    format!("Found {} but not {}.  Put the missing one back, or delete both \
    and a new pair gets made at the next start (and every client needs the \
    new {}).", found.display(), missing.display(), CERT_FILE)
}

/// Makes a new key and a certificate for it, and saves both.  Hands back
/// the two files' contents, so the caller reads them the same way it reads
/// a pair that was already there.
fn make_pair(folder: &Path, cert_path: &Path, key_path: &Path) -> Result<(Vec<u8>, Vec<u8>), String> {
    let failed = |error: rcgen::Error| format!("Couldn't make a TLS certificate: {}", error);

    let mut params = CertificateParams::new(names()).map_err(failed)?;
    let mut issued_to = DistinguishedName::new();
    issued_to.push(DnType::CommonName, COMMON_NAME);
    params.distinguished_name = issued_to;
    let key_pair = KeyPair::generate().map_err(failed)?;
    let cert = params.self_signed(&key_pair).map_err(failed)?;

    let key_file = StratumFile {
        path: key_path.to_path_buf(),
        contents: key_pair.serialize_pem().into_bytes(),
    };
    let cert_file = StratumFile {
        path: cert_path.to_path_buf(),
        contents: cert.pem().into_bytes(),
    };

    // The key first, and privately.  If the certificate then fails, the key
    // comes back out, so we never leave half a pair for the next start to
    // trip over.
    if let Err(error) = diskman::write_private_file(&key_file) {
        return Err(format!("Couldn't save {}: {}", key_path.display(), error));
    }
    if let Err(error) = diskman::write_file(&cert_file) {
        let _ = diskman::delete_file(key_path);
        return Err(format!("Couldn't save {}: {}", cert_path.display(), error));
    }

    scribe::info(Channel::NetTcp,
                 &format!("Made a new TLS certificate and key in {}.  Clients need a copy of {}.",
                          folder.display(), CERT_FILE));
    Ok((cert_file.contents, key_file.contents))
}

/// The names the certificate answers to.  localhost always, and the
/// address the TCP side listens on, unless that is "everything" or already
/// localhost.  They are baked in when the certificate is made, so a changed
/// address later doesn't change them.  A client that pins the certificate
/// shouldn't care.
fn names() -> Vec<String> {
    let mut names = vec!["localhost".to_string(), "127.0.0.1".to_string()];
    let address: IpAddr = constellations::get().tcp_host_address;
    if !address.is_unspecified() && !address.is_loopback() {
        names.push(address.to_string());
    }
    names
}