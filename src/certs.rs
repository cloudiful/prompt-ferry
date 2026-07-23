use crate::cli::CertInitArgs;
use anyhow::Context;
use rcgen::{
    BasicConstraints, CertificateParams, CertifiedIssuer, DistinguishedName, DnType,
    ExtendedKeyUsagePurpose, IsCa, KeyPair, KeyUsagePurpose,
};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub fn init(args: CertInitArgs) -> anyhow::Result<()> {
    let out = PathBuf::from(args.out);
    fs::create_dir_all(&out).with_context(|| format!("failed to create {}", out.display()))?;

    let ca_key = KeyPair::generate().context("failed to generate CA key")?;
    let mut ca_params = CertificateParams::new(vec!["prompt-ferry local CA".to_string()])
        .context("failed to build CA certificate params")?;
    ca_params.distinguished_name = distinguished_name("prompt-ferry local CA");
    ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    ca_params.key_usages = vec![
        KeyUsagePurpose::KeyCertSign,
        KeyUsagePurpose::DigitalSignature,
        KeyUsagePurpose::CrlSign,
    ];
    let ca = CertifiedIssuer::self_signed(ca_params, ca_key).context("failed to sign CA")?;

    let relay_key = KeyPair::generate().context("failed to generate relay key")?;
    let mut relay_params = CertificateParams::new(vec![args.host.clone()])
        .context("failed to build relay certificate params")?;
    relay_params.distinguished_name = distinguished_name(&args.host);
    relay_params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    relay_params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    let relay_cert = relay_params
        .signed_by(&relay_key, &ca)
        .context("failed to sign relay certificate")?;

    let worker_key = KeyPair::generate().context("failed to generate worker key")?;
    let mut worker_params = CertificateParams::new(vec!["prompt-ferry-worker".to_string()])
        .context("failed to build worker certificate params")?;
    worker_params.distinguished_name = distinguished_name("prompt-ferry-worker");
    worker_params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    worker_params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ClientAuth];
    let worker_cert = worker_params
        .signed_by(&worker_key, &ca)
        .context("failed to sign worker certificate")?;

    write(&out, "ca.crt", ca.pem())?;
    write(&out, "relay.crt", relay_cert.pem())?;
    write(&out, "relay.key", relay_key.serialize_pem())?;
    write(&out, "worker.crt", worker_cert.pem())?;
    write(&out, "worker.key", worker_key.serialize_pem())?;

    println!("wrote certificates to {}", out.display());
    Ok(())
}

fn distinguished_name(common_name: &str) -> DistinguishedName {
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, common_name);
    dn
}

fn write(out: &Path, name: &str, contents: String) -> anyhow::Result<()> {
    let path = out.join(name);
    fs::write(&path, contents).with_context(|| format!("failed to write {}", path.display()))
}
