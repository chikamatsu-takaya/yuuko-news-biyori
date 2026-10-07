//! URL validation guard for all outbound network access.
//!
//! This slice validates the URL text itself and enforces the allowlist boundary
//! before any socket is opened.
//!
//! Follow-up slices must keep two additional rules:
//! - DNS resolution results must also reject private, loopback, link-local, and
//!   unspecified IPs before connecting.
//! - HTTP redirects must be followed manually, and every `Location` hop must be
//!   resolved and re-validated through this guard instead of enabling automatic
//!   redirect following.
#![allow(dead_code)]

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use url::{Host, Url};

use super::allowlist::NetworkAllowlist;
use crate::error::AppError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UrlPurpose {
    Rss,
    Article,
    AiEndpoint,
}

impl UrlPurpose {
    fn label(self) -> &'static str {
        match self {
            Self::Rss => "RSS",
            Self::Article => "article",
            Self::AiEndpoint => "AI endpoint",
        }
    }

    fn allowed_hosts(self, allowlist: &NetworkAllowlist) -> &[String] {
        match self {
            Self::Rss => &allowlist.allowed_rss_domains,
            Self::Article => &allowlist.allowed_article_domains,
            Self::AiEndpoint => &allowlist.allowed_ai_endpoints,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum NormalizedHost {
    Domain(String),
    Ipv4(Ipv4Addr),
    Ipv6(Ipv6Addr),
}

impl NormalizedHost {
    fn display(&self) -> String {
        match self {
            Self::Domain(domain) => domain.clone(),
            Self::Ipv4(ip) => ip.to_string(),
            Self::Ipv6(ip) => ip.to_string(),
        }
    }
}

pub fn validate_url(
    raw_url: &str,
    purpose: UrlPurpose,
    allowlist: &NetworkAllowlist,
) -> Result<Url, AppError> {
    let trimmed = raw_url.trim();
    if trimmed.is_empty() {
        return Err(AppError::Validation(format!(
            "{} URL must not be empty",
            purpose.label()
        )));
    }

    let url = Url::parse(trimmed).map_err(|error| {
        AppError::Validation(format!("invalid {} URL: {error}", purpose.label()))
    })?;

    validate_parsed_url(&url, purpose, allowlist)?;
    Ok(url)
}

pub fn validate_parsed_url(
    url: &Url,
    purpose: UrlPurpose,
    allowlist: &NetworkAllowlist,
) -> Result<(), AppError> {
    validate_scheme(url, allowlist)?;
    validate_userinfo(url, purpose)?;

    let host = url.host().ok_or_else(|| {
        AppError::Validation(format!("{} URL must include a host", purpose.label()))
    })?;
    let normalized_host = normalize_host(host)?;
    validate_host(&normalized_host, purpose, allowlist)
}

fn validate_scheme(url: &Url, allowlist: &NetworkAllowlist) -> Result<(), AppError> {
    let scheme = url.scheme().to_ascii_lowercase();
    if allowlist
        .blocked_schemes
        .iter()
        .any(|blocked| blocked.eq_ignore_ascii_case(&scheme))
    {
        return Err(AppError::Validation(format!(
            "scheme '{scheme}' is blocked by network allowlist"
        )));
    }

    if scheme != "http" && scheme != "https" {
        return Err(AppError::Validation(format!(
            "scheme '{scheme}' is not allowed; only http and https are supported"
        )));
    }

    Ok(())
}

fn validate_userinfo(url: &Url, purpose: UrlPurpose) -> Result<(), AppError> {
    if !url.username().is_empty() || url.password().is_some() {
        return Err(AppError::Validation(format!(
            "{} URL must not include username or password",
            purpose.label()
        )));
    }

    Ok(())
}

fn normalize_host(host: Host<&str>) -> Result<NormalizedHost, AppError> {
    match host {
        Host::Domain(domain) => {
            let normalized = domain.trim_end_matches('.').to_ascii_lowercase();
            if normalized.is_empty() {
                return Err(AppError::Validation(
                    "URL host must not be empty after normalization".to_string(),
                ));
            }
            Ok(NormalizedHost::Domain(normalized))
        }
        Host::Ipv4(ip) => Ok(NormalizedHost::Ipv4(ip)),
        Host::Ipv6(ip) => Ok(NormalizedHost::Ipv6(ip)),
    }
}

fn validate_host(
    host: &NormalizedHost,
    purpose: UrlPurpose,
    allowlist: &NetworkAllowlist,
) -> Result<(), AppError> {
    match host {
        NormalizedHost::Domain(domain) => {
            if domain == "localhost" || domain.ends_with(".localhost") {
                return Err(AppError::Validation(format!(
                    "{} URL host '{domain}' is not allowed",
                    purpose.label()
                )));
            }
        }
        NormalizedHost::Ipv4(ip) => {
            if is_disallowed_ip_addr(IpAddr::V4(*ip)) {
                return Err(AppError::Validation(format!(
                    "{} URL host '{}' is not allowed",
                    purpose.label(),
                    ip
                )));
            }
        }
        NormalizedHost::Ipv6(ip) => {
            if is_disallowed_ip_addr(IpAddr::V6(*ip)) {
                return Err(AppError::Validation(format!(
                    "{} URL host '{}' is not allowed",
                    purpose.label(),
                    ip
                )));
            }
        }
    }

    let allowed_hosts = normalized_allowed_hosts(purpose.allowed_hosts(allowlist));
    if allowed_hosts.is_empty() {
        return Err(AppError::Validation(format!(
            "{} URL access is denied by default because no allowlist entries are configured",
            purpose.label()
        )));
    }

    if allowed_hosts
        .iter()
        .any(|allowed| host_matches_allowed(host, allowed))
    {
        return Ok(());
    }

    Err(AppError::Validation(format!(
        "{} URL host '{}' is not present in the allowlist",
        purpose.label(),
        host.display()
    )))
}

fn normalized_allowed_hosts(entries: &[String]) -> Vec<String> {
    entries
        .iter()
        .filter_map(|entry| normalize_allowed_host(entry))
        .collect()
}

fn normalize_allowed_host(entry: &str) -> Option<String> {
    let trimmed = entry.trim();
    if trimmed.is_empty() {
        return None;
    }

    let without_brackets = trimmed
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
        .unwrap_or(trimmed);

    let normalized = without_brackets.trim_end_matches('.').to_ascii_lowercase();
    if normalized.is_empty() {
        return None;
    }

    Some(normalized)
}

fn host_matches_allowed(host: &NormalizedHost, allowed: &str) -> bool {
    match host {
        NormalizedHost::Domain(domain) => {
            domain == allowed || domain.ends_with(&format!(".{allowed}"))
        }
        NormalizedHost::Ipv4(ip) => ip.to_string() == allowed,
        NormalizedHost::Ipv6(ip) => ip.to_string() == allowed,
    }
}

/// 接続先として拒否する IPv4 か（セキュリティ詳細設計書 §6.3）。
/// `is_global` は nightly 限定のため、拒否帯を明示的に列挙する。
/// 「公開インターネット上の通常ホストではない帯」はすべて拒否側へ倒す。
fn is_disallowed_ipv4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    ip.is_private() // 10/8, 172.16/12, 192.168/16
        || ip.is_loopback() // 127/8
        || ip.is_link_local() // 169.254/16
        || ip.is_documentation() // 192.0.2/24, 198.51.100/24, 203.0.113/24
        || ip.is_multicast() // 224/4
        || ip.is_broadcast() // 255.255.255.255（240/4 にも含まれる）
        || a == 0 // 0/8（"this network"。0.0.0.0 を含む）
        || (a == 100 && (b & 0xc0) == 64) // 100.64/10（CGNAT 共有アドレス）
        || (a == 192 && b == 0 && c == 0) // 192.0.0/24（IETF プロトコル割当）
        || (a == 198 && (b & 0xfe) == 18) // 198.18/15（ベンチマーク用）
        || a >= 240 // 240/4（予約帯）
}

/// IPv6 アドレスに IPv4 が埋め込まれている場合、その IPv4 を返す。
/// IPv4射影（::ffff:a.b.c.d）・IPv4互換（::a.b.c.d）・NAT64 既知プレフィックス
/// （64:ff9b::/96）は IPv4 として判定しないと、::ffff:127.0.0.1 等で内部宛て拒否をすり抜けられる。
fn embedded_ipv4(ip: Ipv6Addr) -> Option<Ipv4Addr> {
    if let Some(ipv4) = ip.to_ipv4_mapped() {
        return Some(ipv4);
    }
    let segments = ip.segments();
    let tail = Ipv4Addr::new(
        (segments[6] >> 8) as u8,
        segments[6] as u8,
        (segments[7] >> 8) as u8,
        segments[7] as u8,
    );
    if segments[..6] == [0, 0, 0, 0, 0, 0] {
        // IPv4互換（非推奨）。:: と ::1 もここに入るが、0/8 として拒否される。
        return Some(tail);
    }
    if segments[..6] == [0x64, 0xff9b, 0, 0, 0, 0] {
        return Some(tail);
    }
    None
}

/// 接続先として拒否する IPv6 か（セキュリティ詳細設計書 §6.3）。
fn is_disallowed_ipv6(ip: Ipv6Addr) -> bool {
    if let Some(ipv4) = embedded_ipv4(ip) {
        return is_disallowed_ipv4(ipv4);
    }
    let segments = ip.segments();
    ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_unique_local() // fc00::/7
        || ip.is_unicast_link_local() // fe80::/10
        || ip.is_multicast() // ff00::/8
        || (segments[0] & 0xffc0) == 0xfec0 // fec0::/10（廃止済みサイトローカル）
        || (segments[0] == 0x2001 && segments[1] == 0x0db8) // 2001:db8::/32（文書用）
        // 64:ff9b:1::/48（ローカル用 NAT64）
        || (segments[0] == 0x0064 && segments[1] == 0xff9b && segments[2] == 0x0001)
}

/// IP の拒否判定の唯一の入口。URL リテラルの検証と、RSS・記事HTML取得での
/// DNS 解決後の再検証の両方がこの関数を使う（判定ロジックを複製しない）。
pub(crate) fn is_disallowed_ip_addr(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ipv4) => is_disallowed_ipv4(ipv4),
        IpAddr::V6(ipv6) => is_disallowed_ipv6(ipv6),
    }
}

#[cfg(test)]
mod tests {
    use super::{is_disallowed_ip_addr, validate_url, NetworkAllowlist, UrlPurpose};
    use std::net::IpAddr;

    fn ip(text: &str) -> IpAddr {
        text.parse().expect("test IP must parse")
    }

    fn assert_all_disallowed(cases: &[&str]) {
        for case in cases {
            assert!(is_disallowed_ip_addr(ip(case)), "{case} must be rejected");
        }
    }

    fn assert_all_allowed(cases: &[&str]) {
        for case in cases {
            assert!(!is_disallowed_ip_addr(ip(case)), "{case} must be allowed");
        }
    }

    #[test]
    fn allows_normal_public_ipv4_and_ipv6() {
        assert_all_allowed(&[
            "8.8.8.8",
            "1.1.1.1",
            "142.250.196.110",
            "100.63.255.255",
            "100.128.0.0",
            "198.17.255.255",
            "198.20.0.0",
            "223.255.255.255",
            "2001:4860:4860::8888",
            "2606:4700:4700::1111",
            "2001:db9::1",
        ]);
    }

    #[test]
    fn rejects_existing_private_loopback_link_local_ranges() {
        assert_all_disallowed(&[
            "10.0.0.1",
            "172.16.0.1",
            "172.31.255.255",
            "192.168.0.1",
            "127.0.0.1",
            "127.255.255.254",
            "169.254.169.254",
            "::1",
            "::",
            "fc00::1",
            "fd12:3456::1",
            "fe80::1",
        ]);
    }

    #[test]
    fn rejects_this_network_0_0_0_0_slash_8() {
        assert_all_disallowed(&["0.0.0.0", "0.1.2.3", "0.255.255.255"]);
    }

    #[test]
    fn rejects_cgnat_100_64_slash_10() {
        assert_all_disallowed(&["100.64.0.0", "100.100.100.100", "100.127.255.255"]);
    }

    #[test]
    fn rejects_ietf_protocol_assignments_192_0_0_slash_24() {
        assert_all_disallowed(&["192.0.0.0", "192.0.0.8", "192.0.0.255"]);
        assert_all_allowed(&["192.0.1.1"]);
    }

    #[test]
    fn rejects_ipv4_documentation_ranges() {
        assert_all_disallowed(&["192.0.2.1", "198.51.100.7", "203.0.113.42", "203.0.113.255"]);
    }

    #[test]
    fn rejects_benchmarking_198_18_slash_15() {
        assert_all_disallowed(&["198.18.0.0", "198.19.255.255"]);
    }

    #[test]
    fn rejects_ipv4_multicast_224_slash_4() {
        assert_all_disallowed(&["224.0.0.1", "239.255.255.250"]);
    }

    #[test]
    fn rejects_reserved_240_slash_4_and_broadcast() {
        assert_all_disallowed(&["240.0.0.1", "250.1.2.3", "255.255.255.255"]);
    }

    #[test]
    fn judges_ipv4_mapped_ipv6_as_ipv4() {
        assert_all_disallowed(&[
            "::ffff:127.0.0.1",
            "::ffff:10.0.0.1",
            "::ffff:192.168.1.1",
            "::ffff:169.254.169.254",
            "::ffff:100.64.0.1",
            "::ffff:0.0.0.0",
        ]);
        assert_all_allowed(&["::ffff:8.8.8.8"]);
    }

    #[test]
    fn judges_ipv4_compatible_ipv6_as_ipv4() {
        assert_all_disallowed(&["::127.0.0.1", "::10.0.0.1", "::192.168.0.1"]);
        assert_all_allowed(&["::8.8.8.8"]);
    }

    #[test]
    fn judges_nat64_well_known_prefix_by_embedded_ipv4() {
        assert_all_disallowed(&[
            "64:ff9b::127.0.0.1",
            "64:ff9b::10.0.0.1",
            "64:ff9b::a9fe:a9fe",
        ]);
        assert_all_allowed(&["64:ff9b::8.8.8.8"]);
        // ローカル用 NAT64（RFC 8215）は埋め込み先に関わらず拒否する。
        assert_all_disallowed(&["64:ff9b:1::8.8.8.8"]);
    }

    #[test]
    fn rejects_ipv6_multicast_ff00_slash_8() {
        assert_all_disallowed(&["ff02::1", "ff05::2", "ff0e::1"]);
    }

    #[test]
    fn rejects_ipv6_site_local_fec0_slash_10() {
        assert_all_disallowed(&["fec0::1", "feff:ffff::1"]);
    }

    #[test]
    fn rejects_ipv6_documentation_2001_db8_slash_32() {
        assert_all_disallowed(&["2001:db8::1", "2001:db8:ffff::1"]);
    }

    #[test]
    fn rejects_new_ranges_as_url_literals() {
        let allowlist = allowlist_for(
            UrlPurpose::Article,
            &["100.64.0.1", "::ffff:127.0.0.1", "ff02::1", "203.0.113.1"],
        );

        for url in [
            "http://100.64.0.1/article",
            "http://[::ffff:127.0.0.1]/article",
            "http://[ff02::1]/article",
            "http://203.0.113.1/article",
        ] {
            let error = validate_url(url, UrlPurpose::Article, &allowlist)
                .expect_err("reserved ranges must be rejected even when allowlisted");
            assert!(error.to_string().contains("is not allowed"));
        }
    }

    fn allowlist_for(purpose: UrlPurpose, allowed_hosts: &[&str]) -> NetworkAllowlist {
        let mut allowlist = NetworkAllowlist::default();
        let values = allowed_hosts
            .iter()
            .map(|value| value.to_string())
            .collect();
        match purpose {
            UrlPurpose::Rss => allowlist.allowed_rss_domains = values,
            UrlPurpose::Article => allowlist.allowed_article_domains = values,
            UrlPurpose::AiEndpoint => allowlist.allowed_ai_endpoints = values,
        }
        allowlist
    }

    #[test]
    fn allows_exact_domain_match() {
        let allowlist = allowlist_for(UrlPurpose::Rss, &["news.example.com"]);
        let url = validate_url(
            "https://news.example.com/feed.xml",
            UrlPurpose::Rss,
            &allowlist,
        )
        .expect("expected exact match to pass");

        assert_eq!(url.host_str(), Some("news.example.com"));
    }

    #[test]
    fn allows_subdomain_match_after_normalization() {
        let allowlist = allowlist_for(UrlPurpose::Rss, &["EXAMPLE.COM."]);
        validate_url(
            "https://News.Example.com./feed.xml",
            UrlPurpose::Rss,
            &allowlist,
        )
        .expect("uppercase host with trailing dot should normalize and pass");
    }

    #[test]
    fn rejects_partial_domain_match() {
        let allowlist = allowlist_for(UrlPurpose::Rss, &["example.com"]);
        let error = validate_url(
            "https://evil-example.com/feed.xml",
            UrlPurpose::Rss,
            &allowlist,
        )
        .expect_err("partial domain match must be rejected");

        assert!(error.to_string().contains("not present in the allowlist"));
    }

    #[test]
    fn rejects_username_and_password_in_url() {
        let allowlist = allowlist_for(UrlPurpose::Article, &["example.com"]);

        for url in [
            "https://reader@example.com/article",
            "https://reader:secret@example.com/article",
            "https://:secret@example.com/article",
        ] {
            let error = validate_url(url, UrlPurpose::Article, &allowlist)
                .expect_err("userinfo must be rejected");
            assert!(error
                .to_string()
                .contains("must not include username or password"));
        }
    }

    #[test]
    fn rejects_blocked_and_non_http_schemes() {
        let allowlist = allowlist_for(UrlPurpose::Rss, &["example.com"]);

        let blocked = validate_url("file://example.com/feed.xml", UrlPurpose::Rss, &allowlist)
            .expect_err("file scheme must be blocked");
        assert!(blocked.to_string().contains("is blocked"));

        let unsupported = validate_url("ws://example.com/feed.xml", UrlPurpose::Rss, &allowlist)
            .expect_err("non-http scheme must be rejected");
        assert!(unsupported
            .to_string()
            .contains("only http and https are supported"));
    }

    #[test]
    fn rejects_localhost_domains() {
        let allowlist = allowlist_for(UrlPurpose::Article, &["localhost"]);

        for url in [
            "http://localhost/article",
            "http://LOCALHOST./article",
            "http://api.localhost/article",
        ] {
            let error = validate_url(url, UrlPurpose::Article, &allowlist)
                .expect_err("localhost targets must be rejected");
            assert!(error.to_string().contains("is not allowed"));
        }
    }

    #[test]
    fn rejects_private_and_loopback_ipv4_literals() {
        let allowlist = allowlist_for(
            UrlPurpose::Article,
            &[
                "127.0.0.1",
                "10.0.0.8",
                "172.16.0.5",
                "192.168.1.10",
                "169.254.0.2",
                "0.0.0.0",
            ],
        );

        for url in [
            "http://127.0.0.1/article",
            "http://10.0.0.8/article",
            "http://172.16.0.5/article",
            "http://192.168.1.10/article",
            "http://169.254.0.2/article",
            "http://0.0.0.0/article",
        ] {
            let error = validate_url(url, UrlPurpose::Article, &allowlist)
                .expect_err("private IPv4 literals must be rejected");
            assert!(error.to_string().contains("is not allowed"));
        }
    }

    #[test]
    fn rejects_private_and_loopback_ipv6_literals() {
        let allowlist = allowlist_for(UrlPurpose::AiEndpoint, &["::1", "fe80::1", "fc00::1", "::"]);

        for url in [
            "https://[::1]/v1beta/models",
            "https://[fe80::1]/v1beta/models",
            "https://[fc00::1]/v1beta/models",
            "https://[::]/v1beta/models",
        ] {
            let error = validate_url(url, UrlPurpose::AiEndpoint, &allowlist)
                .expect_err("private IPv6 literals must be rejected");
            assert!(error.to_string().contains("is not allowed"));
        }
    }

    #[test]
    fn allows_public_ip_literal_only_when_explicitly_allowlisted() {
        let allowlist = allowlist_for(
            UrlPurpose::AiEndpoint,
            &["8.8.8.8", "[2001:4860:4860::8888]"],
        );

        validate_url(
            "https://8.8.8.8/v1beta/models",
            UrlPurpose::AiEndpoint,
            &allowlist,
        )
        .expect("public IPv4 literal should pass when explicitly allowlisted");
        validate_url(
            "https://[2001:4860:4860::8888]/v1beta/models",
            UrlPurpose::AiEndpoint,
            &allowlist,
        )
        .expect("public IPv6 literal should pass when explicitly allowlisted");
    }

    #[test]
    fn denies_access_when_allowlist_is_empty() {
        let allowlist = NetworkAllowlist::default();
        let error = validate_url(
            "https://news.example.com/feed.xml",
            UrlPurpose::Rss,
            &allowlist,
        )
        .expect_err("empty allowlist must deny by default");

        assert!(error.to_string().contains("denied by default"));
    }

    #[test]
    fn uses_purpose_specific_allowlist() {
        let allowlist = NetworkAllowlist {
            allowed_ai_endpoints: vec!["generativelanguage.googleapis.com".to_string()],
            allowed_rss_domains: vec!["rss.example.com".to_string()],
            ..NetworkAllowlist::default()
        };

        validate_url(
            "https://generativelanguage.googleapis.com/v1beta/models",
            UrlPurpose::AiEndpoint,
            &allowlist,
        )
        .expect("AI endpoint should pass with AI allowlist");

        let error = validate_url(
            "https://generativelanguage.googleapis.com/v1beta/models",
            UrlPurpose::Rss,
            &allowlist,
        )
        .expect_err("RSS allowlist must not inherit AI entries");
        assert!(error.to_string().contains("not present in the allowlist"));
    }
}
