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

use std::net::{Ipv4Addr, Ipv6Addr};

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
            if is_disallowed_ipv4(*ip) {
                return Err(AppError::Validation(format!(
                    "{} URL host '{}' is not allowed",
                    purpose.label(),
                    ip
                )));
            }
        }
        NormalizedHost::Ipv6(ip) => {
            if is_disallowed_ipv6(*ip) {
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

fn is_disallowed_ipv4(ip: Ipv4Addr) -> bool {
    ip.is_private() || ip.is_loopback() || ip.is_link_local() || ip.is_unspecified()
}

fn is_disallowed_ipv6(ip: Ipv6Addr) -> bool {
    ip.is_loopback() || ip.is_unique_local() || ip.is_unicast_link_local() || ip.is_unspecified()
}

#[cfg(test)]
mod tests {
    use super::{validate_url, NetworkAllowlist, UrlPurpose};

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
