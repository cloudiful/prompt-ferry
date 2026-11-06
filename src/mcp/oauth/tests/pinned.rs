use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use crate::mcp::transport::pinning::{is_blocked_host, is_blocked_ip, resolve_and_validate_with};

#[test]
fn blocks_non_global_and_shared_address_space() {
    let cases: &[(&str, bool)] = &[
        ("127.0.0.1", true),
        ("10.0.0.1", true),
        ("172.16.0.1", true),
        ("192.168.1.1", true),
        ("169.254.169.254", true),
        // Carrier-grade NAT: shared address space that can front internal
        // services.
        ("100.64.0.1", true),
        ("100.127.255.254", true),
        // IETF protocol assignments, 6to4 relay anycast, benchmarking and
        // TEST-NET documentation ranges.
        ("192.0.0.1", true),
        ("192.88.99.1", true),
        ("198.18.0.1", true),
        ("198.19.255.254", true),
        ("192.0.2.1", true),
        ("198.51.100.1", true),
        ("203.0.113.1", true),
        ("0.0.0.0", true),
        ("255.255.255.255", true),
        ("1.1.1.1", false),
        ("8.8.8.8", false),
        ("172.15.255.254", false),
        ("172.32.0.1", false),
        ("192.169.0.1", false),
        ("::1", true),
        ("::", true),
        ("fc00::1", true),
        ("fe80::1", true),
        ("ff02::1", true),
        // Discard-only, Teredo and 6to4 tunnel an arbitrary IPv4 hop.
        ("100::1", true),
        ("2001::1", true),
        ("2002:0a00:0001::1", true),
        // NAT64 embeds an arbitrary IPv4 destination.
        ("64:ff9b::c0a8:0101", true),
        ("64:ff9b:1::c0a8:0101", true),
        // IPv6 special-purpose/documentation/transition allocations.
        ("2001:2::1", true),
        ("2001:10::1", true),
        ("2001:20::1", true),
        ("2001:db8::1", true),
        ("3fff::1", true),
        ("5f00::1", true),
        ("fec0::1", true),
        ("4000::1", true),
        ("8000::1", true),
        ("fff0::1", true),
        ("2606:4700:4700::1111", false),
        ("2a00:1450:4001::1", false),
    ];
    for (text, expected) in cases {
        let ip: IpAddr = text.parse().unwrap();
        assert_eq!(is_blocked_ip(ip), *expected, "is_blocked_ip({ip})");
    }
}

#[test]
fn blocks_ipv4_mapped_ipv6_forms_of_blocked_addresses() {
    let mapped_loopback: IpAddr = "::ffff:127.0.0.1".parse().unwrap();
    let mapped_link_local: IpAddr = "::ffff:169.254.169.254".parse().unwrap();
    let mapped_private: IpAddr = "::ffff:10.0.0.1".parse().unwrap();
    let mapped_cgnat: IpAddr = "::ffff:100.64.0.1".parse().unwrap();
    let mapped_benchmark: IpAddr = "::ffff:198.18.0.1".parse().unwrap();
    let mapped_public: IpAddr = "::ffff:1.1.1.1".parse().unwrap();
    assert!(is_blocked_ip(mapped_loopback));
    assert!(is_blocked_ip(mapped_link_local));
    assert!(is_blocked_ip(mapped_private));
    assert!(is_blocked_ip(mapped_cgnat));
    assert!(is_blocked_ip(mapped_benchmark));
    assert!(!is_blocked_ip(mapped_public));
    assert!(is_blocked_ip(IpAddr::V4(Ipv4Addr::LOCALHOST)));
    assert!(is_blocked_ip(IpAddr::V6(Ipv6Addr::LOCALHOST)));
}

#[test]
fn cloud_metadata_hostnames_are_blocked_by_name() {
    assert!(is_blocked_host("metadata"));
    assert!(is_blocked_host("metadata.google.internal"));
    assert!(is_blocked_host("Metadata.Azure.Internal."));
    assert!(!is_blocked_host("mcp.example.com"));
}

/// The acceptance boundary is the *resolved answer set*, not only the
/// classifier: one blocked address in a mixed answer must reject the host.
#[tokio::test]
async fn a_mixed_resolved_answer_set_is_rejected_at_the_validation_boundary() {
    let mixed = resolve_and_validate_with("mcp.example.com", 443, |_host, port| async move {
        Ok(vec![
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1)), port),
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(100, 64, 0, 10)), port),
        ])
    })
    .await
    .expect("resolution itself succeeds");
    let blocked = mixed.expect_err("a mixed answer set must be rejected");
    assert!(
        blocked.to_string().contains("100.64.0.10"),
        "the rejected address must be named: {blocked}"
    );

    let mapped = resolve_and_validate_with("mcp.example.com", 443, |_host, port| async move {
        Ok(vec![
            SocketAddr::new(IpAddr::V6("2606:4700:4700::1111".parse().unwrap()), port),
            SocketAddr::new(IpAddr::V6("::ffff:198.18.0.1".parse().unwrap()), port),
        ])
    })
    .await
    .expect("resolution itself succeeds");
    assert!(mapped.is_err(), "a mapped blocked answer must be rejected");

    let nat64 = resolve_and_validate_with("mcp.example.com", 443, |_host, port| async move {
        Ok(vec![
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1)), port),
            SocketAddr::new(IpAddr::V6("64:ff9b::c0a8:0101".parse().unwrap()), port),
        ])
    })
    .await
    .expect("resolution itself succeeds");
    let nat64 = nat64.expect_err("a NAT64 answer must be rejected");
    assert!(
        nat64.to_string().contains("64:ff9b::c0a8:101"),
        "the rejected NAT64 address must be named: {nat64}"
    );

    let documentation =
        resolve_and_validate_with("mcp.example.com", 443, |_host, port| async move {
            Ok(vec![
                SocketAddr::new(IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1)), port),
                SocketAddr::new(IpAddr::V6("2001:db8::1".parse().unwrap()), port),
            ])
        })
        .await
        .expect("resolution itself succeeds");
    assert!(
        documentation.is_err(),
        "a documentation-range answer must be rejected"
    );

    let site_local = resolve_and_validate_with("mcp.example.com", 443, |_host, port| async move {
        Ok(vec![
            SocketAddr::new(IpAddr::V6("2606:4700:4700::1111".parse().unwrap()), port),
            SocketAddr::new(IpAddr::V6("fec0::1".parse().unwrap()), port),
        ])
    })
    .await
    .expect("resolution itself succeeds");
    let site_local = site_local.expect_err("a site-local answer must be rejected");
    assert!(
        site_local.to_string().contains("fec0::1"),
        "the rejected site-local address must be named: {site_local}"
    );

    let non_global = resolve_and_validate_with("mcp.example.com", 443, |_host, port| async move {
        Ok(vec![
            SocketAddr::new(IpAddr::V6("2606:4700:4700::1111".parse().unwrap()), port),
            SocketAddr::new(IpAddr::V6("4000::1".parse().unwrap()), port),
        ])
    })
    .await
    .expect("resolution itself succeeds");
    assert!(
        non_global.is_err(),
        "an address outside the supported global-unicast allocation must be rejected"
    );

    let public = resolve_and_validate_with("mcp.example.com", 443, |_host, port| async move {
        Ok(vec![
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1)), port),
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)), port),
        ])
    })
    .await
    .expect("resolution itself succeeds")
    .expect("only public addresses are accepted");
    assert_eq!(public.len(), 2, "every validated address stays pinned");

    let empty = resolve_and_validate_with("mcp.example.com", 443, |_host, _port| async move {
        Ok(Vec::new())
    })
    .await
    .expect("resolution itself succeeds");
    assert!(empty.is_err(), "an empty answer has nothing to pin");

    let metadata =
        resolve_and_validate_with("metadata.google.internal", 443, |_host, _port| async move {
            panic!("a metadata host must be rejected before resolution")
        })
        .await
        .expect("the host policy does not fail");
    assert!(metadata.is_err());
}

#[test]
fn the_blocked_ranges_cover_every_required_category() {
    // The frozen MiniMax regression reads the same list, and this test keeps a
    // later edit from silently dropping a category the contract names.
    let required = [
        IpAddr::V4(Ipv4Addr::new(100, 64, 0, 0)),
        IpAddr::V4(Ipv4Addr::new(192, 0, 0, 0)),
        IpAddr::V4(Ipv4Addr::new(192, 0, 2, 0)),
        IpAddr::V4(Ipv4Addr::new(198, 51, 100, 0)),
        IpAddr::V4(Ipv4Addr::new(203, 0, 113, 0)),
        IpAddr::V4(Ipv4Addr::new(198, 18, 0, 0)),
        IpAddr::V6(Ipv6Addr::new(0x0064, 0xff9b, 0, 0, 0, 0, 0, 0)),
        IpAddr::V6(Ipv6Addr::new(0x0064, 0xff9b, 1, 0, 0, 0, 0, 0)),
        IpAddr::V6(Ipv6Addr::new(0x2001, 0, 0, 0, 0, 0, 0, 0)),
        IpAddr::V6(Ipv6Addr::new(0x2001, 0x0db8, 0, 0, 0, 0, 0, 0)),
        IpAddr::V6(Ipv6Addr::new(0x2002, 0, 0, 0, 0, 0, 0, 0)),
        IpAddr::V6(Ipv6Addr::new(0x3fff, 0, 0, 0, 0, 0, 0, 0)),
        IpAddr::V6(Ipv6Addr::new(0x5f00, 0, 0, 0, 0, 0, 0, 0)),
        IpAddr::V6(Ipv6Addr::new(0xfec0, 0, 0, 0, 0, 0, 0, 0)),
    ];
    for ip in required {
        assert!(
            crate::mcp::transport::pinning::BLOCKED_NETS
                .iter()
                .any(|net| net.contains(&ip)),
            "{ip} must be inside a blocked range"
        );
    }
}
