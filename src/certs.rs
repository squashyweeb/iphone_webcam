use rcgen::{CertificateParams, DistinguishedName, DnType, KeyPair, SanType};
use std::fs;
use std::net::IpAddr;
use std::path::Path;

/// Returns cert/key paths, regenerating the self-signed certificate whenever the set of local
/// IP addresses changes. The iPhone connects by LAN IP, so that IP has to be in the SAN list.
pub fn ensure_certificates(ips: &[IpAddr]) -> Result<(String, String), Box<dyn std::error::Error>> {
    let cert_path = "cert.pem";
    let key_path = "key.pem";
    let ips_path = "cert-ips.txt";

    let wanted = ips.iter().map(|ip| ip.to_string()).collect::<Vec<_>>().join("\n");
    let current = fs::read_to_string(ips_path).unwrap_or_default();

    if Path::new(cert_path).exists() && Path::new(key_path).exists() && current == wanted {
        return Ok((cert_path.to_string(), key_path.to_string()));
    }

    println!("[*] Generating self-signed TLS certificate in pure Rust...");

    let mut params = CertificateParams::default();
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, "iphone-webcam");
    params.distinguished_name = dn;
    params.subject_alt_names = vec![SanType::DnsName("localhost".try_into()?)];
    params.subject_alt_names.extend(ips.iter().map(|ip| SanType::IpAddress(*ip)));

    let key_pair = KeyPair::generate()?;
    let cert = params.self_signed(&key_pair)?;

    fs::write(cert_path, cert.pem())?;
    fs::write(key_path, key_pair.serialize_pem())?;
    fs::write(ips_path, wanted)?;

    println!("[+] Certificate generated for: localhost, {}", ips.iter().map(|ip| ip.to_string()).collect::<Vec<_>>().join(", "));

    Ok((cert_path.to_string(), key_path.to_string()))
}
