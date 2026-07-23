use anyhow::{Context, anyhow};
use axum::http::HeaderMap;
use ipnet::IpNet;
use std::{collections::HashSet, net::IpAddr};

use crate::protocol::RelayIpPolicy;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CompiledRelayIpPolicy {
    pub allowed_cidrs: Vec<IpNet>,
    pub trusted_proxy_cidrs: Vec<IpNet>,
}

pub fn normalize_policy(policy: &RelayIpPolicy) -> RelayIpPolicy {
    RelayIpPolicy {
        allowed_cidrs: normalize_entries(&policy.allowed_cidrs),
        trusted_proxy_cidrs: normalize_entries(&policy.trusted_proxy_cidrs),
    }
}

pub fn compile_policy(policy: &RelayIpPolicy) -> anyhow::Result<CompiledRelayIpPolicy> {
    let policy = normalize_policy(policy);
    Ok(CompiledRelayIpPolicy {
        allowed_cidrs: parse_entries(&policy.allowed_cidrs, "allowed_cidrs")?,
        trusted_proxy_cidrs: parse_entries(&policy.trusted_proxy_cidrs, "trusted_proxy_cidrs")?,
    })
}

pub fn parse_entries(entries: &[String], field: &str) -> anyhow::Result<Vec<IpNet>> {
    entries
        .iter()
        .enumerate()
        .map(|(index, entry)| parse_entry(entry).with_context(|| format!("{field}[{index}]")))
        .collect()
}

pub fn parse_entry(value: &str) -> anyhow::Result<IpNet> {
    let value = value.trim();
    if value.is_empty() {
        return Err(anyhow!("value must not be empty"));
    }
    value
        .parse::<IpNet>()
        .or_else(|_| value.parse::<IpAddr>().map(IpNet::from))
        .map_err(|_| anyhow!("invalid IP or CIDR: {value}"))
}

pub fn normalize_entries(entries: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut normalized = Vec::new();
    for entry in entries {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        if seen.insert(entry.to_string()) {
            normalized.push(entry.to_string());
        }
    }
    normalized
}

pub fn resolve_client_ip(
    peer_ip: IpAddr,
    headers: &HeaderMap,
    trusted_proxy_cidrs: &[IpNet],
) -> Option<IpAddr> {
    if contains_ip(trusted_proxy_cidrs, peer_ip) {
        forwarded_for_ip(headers).or_else(|| header_ip(headers, "x-real-ip"))
    } else {
        Some(peer_ip)
    }
}

pub fn contains_ip(cidrs: &[IpNet], ip: IpAddr) -> bool {
    cidrs.iter().any(|cidr| cidr.contains(&ip))
}

fn forwarded_for_ip(headers: &HeaderMap) -> Option<IpAddr> {
    let value = headers.get("x-forwarded-for")?.to_str().ok()?;
    value
        .split(',')
        .find_map(|part| part.trim().parse::<IpAddr>().ok())
}

fn header_ip(headers: &HeaderMap, name: &str) -> Option<IpAddr> {
    headers
        .get(name)?
        .to_str()
        .ok()?
        .trim()
        .parse::<IpAddr>()
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_policy_trims_and_deduplicates() {
        let policy = RelayIpPolicy {
            allowed_cidrs: vec![
                " 127.0.0.1 ".to_string(),
                "".to_string(),
                "127.0.0.1".to_string(),
            ],
            trusted_proxy_cidrs: vec!["10.0.0.0/8".to_string(), "10.0.0.0/8".to_string()],
        };
        assert_eq!(
            normalize_policy(&policy),
            RelayIpPolicy {
                allowed_cidrs: vec!["127.0.0.1".to_string()],
                trusted_proxy_cidrs: vec!["10.0.0.0/8".to_string()],
            }
        );
    }

    #[test]
    fn parse_entry_accepts_single_ip() {
        let net = parse_entry("127.0.0.1").unwrap();
        let ip: IpAddr = "127.0.0.1".parse().unwrap();
        assert!(net.contains(&ip));
    }

    #[test]
    fn resolve_client_ip_ignores_spoofed_forwarded_for_for_untrusted_peers() {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", "10.0.0.5".parse().unwrap());
        assert_eq!(
            resolve_client_ip("127.0.0.1".parse().unwrap(), &headers, &[]),
            Some("127.0.0.1".parse().unwrap())
        );
    }

    #[test]
    fn resolve_client_ip_requires_forwarded_ip_for_trusted_proxy() {
        let trusted = vec![parse_entry("127.0.0.0/8").unwrap()];
        let headers = HeaderMap::new();
        assert_eq!(
            resolve_client_ip("127.0.0.1".parse().unwrap(), &headers, &trusted),
            None
        );
    }
}
