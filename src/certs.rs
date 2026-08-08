use rcgen::{CertificateParams, DistinguishedName, DnType, KeyPair, SanType};
use std::fs;
use std::path::Path;

pub fn ensure_certificates() -> Result<(String, String), Box<dyn std::error::Error>> {
    let cert_path = "cert.pem";
    let key_path = "key.pem";

    if Path::new(cert_path).exists() && Path::new(key_path).exists() {
        return Ok((cert_path.to_string(), key_path.to_string()));
    }

    println!("[*] Generating self-signed TLS certificate in pure Rust...");

    let mut params = CertificateParams::default();
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, "localhost");
    params.distinguished_name = dn;
    params.subject_alt_names = vec![
        SanType::DnsName("localhost".try_into()?),
        SanType::IpAddress(std::net::IpAddr::V4(std::net::Ipv4Addr::new(127, 0, 0, 1))),
    ];

    let key_pair = KeyPair::generate()?;
    let cert = params.self_signed(&key_pair)?;

    fs::write(cert_path, cert.pem())?;
    fs::write(key_path, key_pair.serialize_pem())?;

    println!("[+] Certificate generated successfully: cert.pem, key.pem");

    Ok((cert_path.to_string(), key_path.to_string()))
}
