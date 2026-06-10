use std::future::Future;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::pin::Pin;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use hickory_resolver::TokioResolver;
use hickory_resolver::proto::rr::RData;
use hickory_resolver::proto::rr::rdata::TXT;
use rsa::pkcs1::DecodeRsaPublicKey;
use rsa::pkcs1::{EncodeRsaPrivateKey, LineEnding as Pkcs1LineEnding};
use rsa::pkcs1v15::{Signature as RsaPkcs1v15Signature, VerifyingKey};
use rsa::pkcs8::DecodePrivateKey;
use rsa::pkcs8::DecodePublicKey;
use rsa::signature::Verifier;
use rsa::{RsaPrivateKey, RsaPublicKey};
use sha2::{Digest, Sha256};
use tracing::{debug, warn};

const MAX_SPF_INCLUDE_DEPTH: u8 = 10;

/// Check SPF record for the sender
pub async fn check_spf(sender_domain: &str, sender_ip: &str) -> SpfResult {
    let resolver = match TokioResolver::builder_tokio() {
        Ok(builder) => match builder.build() {
            Ok(r) => r,
            Err(e) => {
                warn!("Failed to build DNS resolver: {}", e);
                return SpfResult::TempError;
            }
        },
        Err(e) => {
            warn!("Failed to create DNS resolver: {}", e);
            return SpfResult::TempError;
        }
    };

    let sender_ip = match sender_ip.parse::<IpAddr>() {
        Ok(ip) => ip,
        Err(_) => return SpfResult::Neutral,
    };

    match lookup_spf_record(&resolver, sender_domain).await {
        Some(record) => {
            debug!("SPF record for {}: {}", sender_domain, record);
            evaluate_spf_record(&resolver, sender_domain, &record, sender_ip, 0).await
        }
        None => SpfResult::None,
    }
}

async fn lookup_spf_record(resolver: &TokioResolver, domain: &str) -> Option<String> {
    let txt_response = resolver.txt_lookup(domain).await.ok()?;
    txt_response.answers().iter().find_map(|r| {
        if let RData::TXT(txt) = &r.data {
            let txt_str = txt_record_text(txt);
            if txt_str.to_ascii_lowercase().starts_with("v=spf1") {
                Some(txt_str)
            } else {
                None
            }
        } else {
            None
        }
    })
}

fn txt_record_text(txt: &TXT) -> String {
    txt.txt_data
        .iter()
        .map(|chunk| String::from_utf8_lossy(chunk))
        .collect::<String>()
}

pub async fn check_dkim(raw_message: &[u8]) -> DkimResult {
    let message = String::from_utf8_lossy(raw_message);
    let Some(parsed) = ParsedMessage::parse(&message) else {
        return DkimResult::PermError;
    };

    let Some(signature_header) = parsed.last_header("DKIM-Signature") else {
        return DkimResult::None;
    };

    let signature = match DkimSignature::parse(&signature_header.unfolded_value()) {
        Some(signature) => signature,
        None => return DkimResult::PermError,
    };

    if signature.algorithm != "rsa-sha256" {
        return DkimResult::Neutral;
    }

    let body_hash = STANDARD.encode(Sha256::digest(canonicalize_dkim_body(
        parsed.body.as_bytes(),
        signature.body_canonicalization,
    )));
    if body_hash != signature.body_hash {
        return DkimResult::Fail;
    }

    let resolver = match TokioResolver::builder_tokio().and_then(|builder| builder.build()) {
        Ok(resolver) => resolver,
        Err(error) => {
            warn!("Failed to build DNS resolver for DKIM: {}", error);
            return DkimResult::TempError;
        }
    };

    let public_key =
        match lookup_dkim_public_key(&resolver, &signature.domain, &signature.selector).await {
            Ok(Some(public_key)) => public_key,
            Ok(None) => return DkimResult::PermError,
            Err(error) => {
                warn!(
                    "Failed to look up DKIM key {}._domainkey.{}: {}",
                    signature.selector, signature.domain, error
                );
                return DkimResult::TempError;
            }
        };

    if verify_dkim_signature(&parsed, signature_header, &signature, &public_key) {
        DkimResult::Pass
    } else {
        DkimResult::Fail
    }
}

async fn lookup_dkim_public_key(
    resolver: &TokioResolver,
    domain: &str,
    selector: &str,
) -> anyhow::Result<Option<RsaPublicKey>> {
    let query_name = format!("{}._domainkey.{}", selector, domain);
    let txt_response = resolver.txt_lookup(query_name).await?;

    for record in txt_response.answers() {
        if let RData::TXT(txt) = &record.data {
            let txt_str = txt_record_text(txt);
            if let Some(public_key) = dkim_public_key_from_dns_record(&txt_str) {
                return Ok(Some(public_key));
            }
        }
    }

    Ok(None)
}

fn dkim_public_key_from_dns_record(record: &str) -> Option<RsaPublicKey> {
    let tags = parse_dkim_tag_list(record);
    if !tags
        .get("v")
        .is_none_or(|version| version.eq_ignore_ascii_case("DKIM1"))
    {
        return None;
    }
    if !tags
        .get("k")
        .is_none_or(|key_type| key_type.eq_ignore_ascii_case("rsa"))
    {
        return None;
    }

    let public_key = tags.get("p")?;
    if public_key.trim().is_empty() {
        return None;
    }

    let der = STANDARD
        .decode(public_key.split_whitespace().collect::<String>())
        .ok()?;

    RsaPublicKey::from_public_key_der(&der)
        .or_else(|_| RsaPublicKey::from_pkcs1_der(&der))
        .ok()
}

fn verify_rsa_sha256(public_key: &RsaPublicKey, message: &[u8], signature: &str) -> bool {
    let Ok(signature_bytes) = STANDARD.decode(signature.split_whitespace().collect::<String>())
    else {
        return false;
    };
    let Ok(signature) = RsaPkcs1v15Signature::try_from(signature_bytes.as_slice()) else {
        return false;
    };
    let verifying_key = VerifyingKey::<Sha256>::new(public_key.clone());
    verifying_key.verify(message, &signature).is_ok()
}

fn verify_dkim_signature(
    message: &ParsedMessage<'_>,
    signature_header: &ParsedHeader<'_>,
    signature: &DkimSignature,
    public_key: &RsaPublicKey,
) -> bool {
    let Some(signing_data) = dkim_signing_data(message, signature_header, signature) else {
        return false;
    };
    verify_rsa_sha256(public_key, signing_data.as_bytes(), &signature.signature)
}

fn dkim_signing_data(
    message: &ParsedMessage<'_>,
    signature_header: &ParsedHeader<'_>,
    signature: &DkimSignature,
) -> Option<String> {
    let mut values = Vec::new();
    let dkim_header_name = "dkim-signature";
    let mut used_indices: Vec<usize> = Vec::new();

    for header_name in &signature.signed_headers {
        if header_name.eq_ignore_ascii_case(dkim_header_name) {
            continue;
        }

        let (index, header) = message.last_header_excluding_used(header_name, &used_indices)?;
        used_indices.push(index);
        values.push(canonicalize_dkim_header(
            header,
            signature.header_canonicalization,
            None,
        ));
    }

    values.push(canonicalize_dkim_header(
        signature_header,
        signature.header_canonicalization,
        Some(&signature.header_without_signature),
    ));

    Some(values.join(""))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DkimCanonicalization {
    Simple,
    Relaxed,
}

#[derive(Debug)]
struct DkimSignature {
    algorithm: String,
    body_hash: String,
    domain: String,
    selector: String,
    signed_headers: Vec<String>,
    signature: String,
    header_without_signature: String,
    header_canonicalization: DkimCanonicalization,
    body_canonicalization: DkimCanonicalization,
}

impl DkimSignature {
    fn parse(value: &str) -> Option<Self> {
        let tags = parse_dkim_tag_list(value);
        let algorithm = tags.get("a")?.trim().to_ascii_lowercase();
        let body_hash = tags.get("bh")?.trim().to_string();
        let domain = normalize_domain(tags.get("d")?);
        let selector = normalize_domain(tags.get("s")?);
        let signed_headers = tags
            .get("h")?
            .split(':')
            .map(|header| header.trim().to_ascii_lowercase())
            .filter(|header| !header.is_empty())
            .collect::<Vec<_>>();
        let signature = tags.get("b").cloned().unwrap_or_default();
        let header_without_signature = dkim_header_without_b_tag_value(value)?;
        let (header_canonicalization, body_canonicalization) =
            parse_dkim_canonicalization(tags.get("c").map(String::as_str));

        if domain.is_empty()
            || selector.is_empty()
            || signed_headers.is_empty()
            || body_hash.is_empty()
        {
            return None;
        }

        Some(Self {
            algorithm,
            body_hash,
            domain,
            selector,
            signed_headers,
            signature,
            header_without_signature,
            header_canonicalization,
            body_canonicalization,
        })
    }
}

fn parse_dkim_tag_list(value: &str) -> std::collections::HashMap<String, String> {
    value
        .split(';')
        .filter_map(|part| {
            let (name, value) = part.split_once('=')?;
            Some((name.trim().to_ascii_lowercase(), value.trim().to_string()))
        })
        .collect()
}

fn parse_dkim_canonicalization(
    value: Option<&str>,
) -> (DkimCanonicalization, DkimCanonicalization) {
    let Some(value) = value else {
        return (DkimCanonicalization::Simple, DkimCanonicalization::Simple);
    };

    let mut parts = value
        .split('/')
        .map(|part| part.trim().to_ascii_lowercase());
    let header = match parts.next().as_deref() {
        Some("relaxed") => DkimCanonicalization::Relaxed,
        _ => DkimCanonicalization::Simple,
    };
    let body = match parts.next().as_deref() {
        Some("relaxed") => DkimCanonicalization::Relaxed,
        _ => DkimCanonicalization::Simple,
    };

    (header, body)
}

fn dkim_header_without_b_tag_value(value: &str) -> Option<String> {
    let b_index = find_dkim_b_tag_value_start(value)?;
    let b_end = value[b_index..]
        .find(';')
        .map(|offset| b_index + offset)
        .unwrap_or(value.len());
    let mut cleaned = String::with_capacity(value.len());
    cleaned.push_str(&value[..b_index]);
    cleaned.push_str(&value[b_end..]);
    Some(cleaned)
}

fn find_dkim_b_tag_value_start(value: &str) -> Option<usize> {
    let mut offset = 0usize;
    for part in value.split(';') {
        let trimmed_start = part.trim_start();
        let leading_ws = part.len() - trimmed_start.len();
        if let Some(eq_index) = trimmed_start.find('=') {
            let name = trimmed_start[..eq_index].trim();
            if name.eq_ignore_ascii_case("b") {
                return Some(offset + leading_ws + eq_index + 1);
            }
        }
        offset += part.len() + 1;
    }
    None
}

fn canonicalize_dkim_body(body: &[u8], canonicalization: DkimCanonicalization) -> Vec<u8> {
    match canonicalization {
        DkimCanonicalization::Simple => canonicalize_dkim_body_simple(body),
        DkimCanonicalization::Relaxed => canonicalize_dkim_body_relaxed(body),
    }
}

fn canonicalize_dkim_body_simple(body: &[u8]) -> Vec<u8> {
    let normalized = normalize_crlf(String::from_utf8_lossy(body).as_ref());
    let mut lines = normalized
        .split("\r\n")
        .map(str::to_string)
        .collect::<Vec<_>>();

    while lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }

    if lines.is_empty() {
        return Vec::new();
    }

    let mut result = lines.join("\r\n");
    result.push_str("\r\n");
    result.into_bytes()
}

fn canonicalize_dkim_body_relaxed(body: &[u8]) -> Vec<u8> {
    let normalized = normalize_crlf(String::from_utf8_lossy(body).as_ref());
    let mut lines = normalized
        .split("\r\n")
        .map(|line| line.split_ascii_whitespace().collect::<Vec<_>>().join(" "))
        .collect::<Vec<_>>();

    while lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }

    if lines.is_empty() {
        return Vec::new();
    }

    let mut result = lines.join("\r\n");
    result.push_str("\r\n");
    result.into_bytes()
}

fn canonicalize_dkim_header(
    header: &ParsedHeader<'_>,
    canonicalization: DkimCanonicalization,
    override_value: Option<&str>,
) -> String {
    let value = override_value.unwrap_or(header.value);
    match canonicalization {
        DkimCanonicalization::Simple => {
            let mut result = String::new();
            result.push_str(header.raw_name);
            result.push(':');
            result.push_str(value);
            result.push_str("\r\n");
            result
        }
        DkimCanonicalization::Relaxed => {
            let value = unfold_header_value(value);
            let value = value.split_ascii_whitespace().collect::<Vec<_>>().join(" ");
            format!(
                "{}:{}\r\n",
                header.raw_name.trim().to_ascii_lowercase(),
                value
            )
        }
    }
}

fn normalize_crlf(value: &str) -> String {
    value
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\n', "\r\n")
}

fn unfold_header_value(value: &str) -> String {
    let mut result = String::with_capacity(value.len());
    let mut chars = value.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\r' {
            if chars.peek() == Some(&'\n') {
                chars.next();
            }
            if matches!(chars.peek(), Some(' ' | '\t')) {
                result.push(' ');
                while matches!(chars.peek(), Some(' ' | '\t')) {
                    chars.next();
                }
            } else {
                result.push(ch);
            }
        } else if ch == '\n' && matches!(chars.peek(), Some(' ' | '\t')) {
            result.push(' ');
            while matches!(chars.peek(), Some(' ' | '\t')) {
                chars.next();
            }
        } else {
            result.push(ch);
        }
    }
    result
}

#[derive(Debug)]
struct ParsedMessage<'a> {
    headers: Vec<ParsedHeader<'a>>,
    body: &'a str,
}

impl<'a> ParsedMessage<'a> {
    fn parse(message: &'a str) -> Option<Self> {
        let split_at = message
            .find("\r\n\r\n")
            .map(|index| (index, 4))
            .or_else(|| message.find("\n\n").map(|index| (index, 2)));
        let (headers, body) = match split_at {
            Some((index, separator_len)) => (&message[..index], &message[index + separator_len..]),
            None => (message, ""),
        };
        Some(Self {
            headers: parse_headers(headers)?,
            body,
        })
    }

    fn last_header(&self, name: &str) -> Option<&ParsedHeader<'a>> {
        self.headers
            .iter()
            .rev()
            .find(|header| header.raw_name.eq_ignore_ascii_case(name))
    }

    fn last_header_excluding_used(
        &self,
        name: &str,
        used_indices: &[usize],
    ) -> Option<(usize, &ParsedHeader<'a>)> {
        self.headers
            .iter()
            .enumerate()
            .rev()
            .find(|(index, header)| {
                header.raw_name.eq_ignore_ascii_case(name) && !used_indices.contains(index)
            })
    }
}

#[derive(Debug)]
struct ParsedHeader<'a> {
    raw_name: &'a str,
    value: &'a str,
}

impl ParsedHeader<'_> {
    fn unfolded_value(&self) -> String {
        unfold_header_value(self.value).trim().to_string()
    }
}

fn parse_headers(headers: &str) -> Option<Vec<ParsedHeader<'_>>> {
    let mut parsed = Vec::new();
    let mut current_name_start = 0usize;
    let mut current_colon: Option<usize> = None;
    let mut current_value_start = 0usize;
    let mut line_start = 0usize;

    for line in headers.split_inclusive('\n') {
        let line_end = line_start + line.trim_end_matches(['\r', '\n']).len();
        let next_line_start = line_start + line.len();
        let is_continuation = line.starts_with(' ') || line.starts_with('\t');

        if is_continuation {
            current_colon?;
            line_start = next_line_start;
            continue;
        }

        if let Some(colon) = current_colon {
            parsed.push(ParsedHeader {
                raw_name: &headers[current_name_start..colon],
                value: &headers[current_value_start..line_start],
            });
        }

        let relative_colon = headers[line_start..line_end].find(':')?;
        current_name_start = line_start;
        let colon = line_start + relative_colon;
        current_colon = Some(colon);
        current_value_start = colon + 1;
        line_start = next_line_start;
    }

    if let Some(colon) = current_colon {
        parsed.push(ParsedHeader {
            raw_name: &headers[current_name_start..colon],
            value: &headers[current_value_start..],
        });
    }

    Some(parsed)
}

fn evaluate_spf_record<'a>(
    resolver: &'a TokioResolver,
    domain: &'a str,
    record: &'a str,
    sender_ip: IpAddr,
    include_depth: u8,
) -> Pin<Box<dyn Future<Output = SpfResult> + Send + 'a>> {
    Box::pin(async move {
        if include_depth > MAX_SPF_INCLUDE_DEPTH {
            return SpfResult::TempError;
        }

        let mut redirect: Option<&str> = None;

        for raw_token in record.split_whitespace().skip(1) {
            let token = raw_token.trim();
            if token.is_empty() {
                continue;
            }

            if let Some((name, value)) = token.split_once('=') {
                if name.eq_ignore_ascii_case("redirect") {
                    redirect = Some(value);
                }
                continue;
            }

            let (qualifier, mechanism) = split_spf_qualifier(token);
            let matched =
                match_spf_mechanism(resolver, domain, mechanism, sender_ip, include_depth).await;

            if matched == SpfMechanismMatch::TempError {
                return SpfResult::TempError;
            }

            if matched == SpfMechanismMatch::Match {
                return spf_result_for_qualifier(qualifier);
            }
        }

        if let Some(redirect_domain) = redirect
            && let Some(record) = lookup_spf_record(resolver, redirect_domain).await
        {
            return evaluate_spf_record(
                resolver,
                redirect_domain,
                &record,
                sender_ip,
                include_depth + 1,
            )
            .await;
        }

        SpfResult::Neutral
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SpfMechanismMatch {
    Match,
    NoMatch,
    TempError,
}

async fn match_spf_mechanism(
    resolver: &TokioResolver,
    domain: &str,
    mechanism: &str,
    sender_ip: IpAddr,
    include_depth: u8,
) -> SpfMechanismMatch {
    let lower_mechanism = mechanism.to_ascii_lowercase();

    if lower_mechanism == "all" {
        return SpfMechanismMatch::Match;
    }

    if let Some(spec) = lower_mechanism.strip_prefix("ip4:") {
        return bool_match(spf_ip4_matches(spec, sender_ip));
    }

    if let Some(spec) = lower_mechanism.strip_prefix("ip6:") {
        return bool_match(spf_ip6_matches(spec, sender_ip));
    }

    if lower_mechanism.starts_with("a")
        && lower_mechanism
            .get(1..2)
            .is_none_or(|ch| ch == ":" || ch == "/")
    {
        return bool_match(
            spf_host_matches(
                resolver,
                domain,
                lower_mechanism.strip_prefix('a').unwrap_or_default(),
                sender_ip,
            )
            .await,
        );
    }

    if lower_mechanism.starts_with("mx")
        && lower_mechanism
            .get(2..3)
            .is_none_or(|ch| ch == ":" || ch == "/")
    {
        return bool_match(
            spf_mx_matches(
                resolver,
                domain,
                lower_mechanism.strip_prefix("mx").unwrap_or_default(),
                sender_ip,
            )
            .await,
        );
    }

    if let Some(include_domain) = lower_mechanism.strip_prefix("include:") {
        if include_depth >= MAX_SPF_INCLUDE_DEPTH {
            return SpfMechanismMatch::TempError;
        }
        let Some(record) = lookup_spf_record(resolver, include_domain).await else {
            return SpfMechanismMatch::NoMatch;
        };
        return match evaluate_spf_record(
            resolver,
            include_domain,
            &record,
            sender_ip,
            include_depth + 1,
        )
        .await
        {
            SpfResult::Pass => SpfMechanismMatch::Match,
            SpfResult::TempError => SpfMechanismMatch::TempError,
            _ => SpfMechanismMatch::NoMatch,
        };
    }

    SpfMechanismMatch::NoMatch
}

fn bool_match(value: bool) -> SpfMechanismMatch {
    if value {
        SpfMechanismMatch::Match
    } else {
        SpfMechanismMatch::NoMatch
    }
}

fn split_spf_qualifier(token: &str) -> (char, &str) {
    match token.chars().next() {
        Some(first @ ('+' | '-' | '~' | '?')) => (first, &token[first.len_utf8()..]),
        _ => ('+', token),
    }
}

fn spf_result_for_qualifier(qualifier: char) -> SpfResult {
    match qualifier {
        '-' => SpfResult::Fail,
        '~' => SpfResult::SoftFail,
        '?' => SpfResult::Neutral,
        _ => SpfResult::Pass,
    }
}

fn spf_ip4_matches(spec: &str, sender_ip: IpAddr) -> bool {
    let IpAddr::V4(sender_ip) = sender_ip else {
        return false;
    };
    let Some((addr, prefix)) = parse_ip_mechanism::<Ipv4Addr>(spec, 32) else {
        return false;
    };
    ipv4_in_cidr(sender_ip, addr, prefix)
}

fn spf_ip6_matches(spec: &str, sender_ip: IpAddr) -> bool {
    let IpAddr::V6(sender_ip) = sender_ip else {
        return false;
    };
    let Some((addr, prefix)) = parse_ip_mechanism::<Ipv6Addr>(spec, 128) else {
        return false;
    };
    ipv6_in_cidr(sender_ip, addr, prefix)
}

fn parse_ip_mechanism<T>(spec: &str, max_prefix: u8) -> Option<(T, u8)>
where
    T: std::str::FromStr,
{
    let (address, prefix) = split_domain_and_cidr(spec, max_prefix)?;
    let address = address.parse::<T>().ok()?;
    Some((address, prefix.unwrap_or(max_prefix)))
}

async fn spf_host_matches(
    resolver: &TokioResolver,
    domain: &str,
    spec: &str,
    sender_ip: IpAddr,
) -> bool {
    let Some((host, prefix)) = parse_domain_mechanism(domain, spec, sender_ip) else {
        return false;
    };
    host_has_ip(resolver, &host, sender_ip, prefix).await
}

async fn spf_mx_matches(
    resolver: &TokioResolver,
    domain: &str,
    spec: &str,
    sender_ip: IpAddr,
) -> bool {
    let Some((mx_domain, prefix)) = parse_domain_mechanism(domain, spec, sender_ip) else {
        return false;
    };
    let Ok(mx_lookup) = resolver.mx_lookup(mx_domain).await else {
        return false;
    };

    for record in mx_lookup.answers() {
        if let RData::MX(mx) = &record.data
            && host_has_ip(resolver, &mx.exchange.to_utf8(), sender_ip, prefix).await
        {
            return true;
        }
    }

    false
}

async fn host_has_ip(
    resolver: &TokioResolver,
    host: &str,
    sender_ip: IpAddr,
    prefix: Option<u8>,
) -> bool {
    let Ok(lookup) = resolver.lookup_ip(host).await else {
        return false;
    };

    lookup
        .iter()
        .any(|candidate| ip_matches_with_prefix(sender_ip, candidate, prefix))
}

fn parse_domain_mechanism(
    default_domain: &str,
    spec: &str,
    sender_ip: IpAddr,
) -> Option<(String, Option<u8>)> {
    let spec = spec.strip_prefix(':').unwrap_or(spec);
    let max_prefix = match sender_ip {
        IpAddr::V4(_) => 32,
        IpAddr::V6(_) => 128,
    };
    let (domain, prefix) = split_domain_and_cidr(spec, max_prefix)?;
    let domain = if domain.is_empty() {
        default_domain.to_string()
    } else {
        normalize_domain(domain)
    };
    Some((domain, prefix))
}

fn split_domain_and_cidr(spec: &str, max_prefix: u8) -> Option<(&str, Option<u8>)> {
    let Some((value, prefix)) = spec.split_once('/') else {
        return Some((spec, None));
    };
    let prefix = prefix.parse::<u8>().ok()?;
    if prefix > max_prefix {
        return None;
    }
    Some((value, Some(prefix)))
}

fn ip_matches_with_prefix(sender_ip: IpAddr, candidate: IpAddr, prefix: Option<u8>) -> bool {
    match (sender_ip, candidate) {
        (IpAddr::V4(sender), IpAddr::V4(candidate)) => {
            ipv4_in_cidr(sender, candidate, prefix.unwrap_or(32))
        }
        (IpAddr::V6(sender), IpAddr::V6(candidate)) => {
            ipv6_in_cidr(sender, candidate, prefix.unwrap_or(128))
        }
        _ => false,
    }
}

fn ipv4_in_cidr(ip: Ipv4Addr, network: Ipv4Addr, prefix: u8) -> bool {
    if prefix == 0 {
        return true;
    }
    let mask = u32::MAX << (32 - prefix);
    (u32::from(ip) & mask) == (u32::from(network) & mask)
}

fn ipv6_in_cidr(ip: Ipv6Addr, network: Ipv6Addr, prefix: u8) -> bool {
    if prefix == 0 {
        return true;
    }
    let mask = u128::MAX << (128 - prefix);
    (u128::from(ip) & mask) == (u128::from(network) & mask)
}

pub async fn check_dmarc(
    header_from_domain: Option<&str>,
    envelope_from_domain: Option<&str>,
    spf_result: &SpfResult,
    dkim_result: Option<&str>,
) -> DmarcResult {
    let Some(header_from_domain) = header_from_domain
        .map(normalize_domain)
        .filter(|d| !d.is_empty())
    else {
        return DmarcResult::None;
    };

    if dkim_result == Some("pass") {
        return DmarcResult::Pass;
    }

    if matches!(spf_result, SpfResult::Pass)
        && envelope_from_domain
            .map(normalize_domain)
            .is_some_and(|domain| domains_align(&domain, &header_from_domain))
    {
        return DmarcResult::Pass;
    }

    match lookup_dmarc_record(&header_from_domain).await {
        Ok(Some(_record)) => DmarcResult::Fail,
        Ok(None) => DmarcResult::None,
        Err(error) => {
            warn!(
                "Failed to look up DMARC record for {}: {}",
                header_from_domain, error
            );
            DmarcResult::TempError
        }
    }
}

async fn lookup_dmarc_record(domain: &str) -> anyhow::Result<Option<String>> {
    let resolver = TokioResolver::builder_tokio()?.build()?;

    for candidate in dmarc_lookup_domains(domain) {
        let query_name = format!("_dmarc.{}", candidate);
        let txt_response = match resolver.txt_lookup(query_name).await {
            Ok(response) => response,
            Err(_) => continue,
        };

        if let Some(record) = txt_response.answers().iter().find_map(|record| {
            if let RData::TXT(txt) = &record.data {
                let txt_str = txt_record_text(txt);
                if txt_str.to_ascii_lowercase().starts_with("v=dmarc1") {
                    Some(txt_str)
                } else {
                    None
                }
            } else {
                None
            }
        }) {
            return Ok(Some(record));
        }
    }

    Ok(None)
}

fn dmarc_lookup_domains(domain: &str) -> Vec<String> {
    let domain = normalize_domain(domain);
    let labels: Vec<&str> = domain
        .split('.')
        .filter(|label| !label.is_empty())
        .collect();
    if labels.len() <= 2 {
        return vec![domain];
    }

    let organizational_domain = labels[labels.len() - 2..].join(".");
    vec![domain, organizational_domain]
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DmarcResult {
    Pass,
    Fail,
    None,
    TempError,
}

impl DmarcResult {
    pub fn as_str(&self) -> &'static str {
        match self {
            DmarcResult::Pass => "pass",
            DmarcResult::Fail => "fail",
            DmarcResult::None => "none",
            DmarcResult::TempError => "temperror",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DkimResult {
    Pass,
    Fail,
    Neutral,
    None,
    TempError,
    PermError,
}

impl DkimResult {
    pub fn as_str(&self) -> &'static str {
        match self {
            DkimResult::Pass => "pass",
            DkimResult::Fail => "fail",
            DkimResult::Neutral => "neutral",
            DkimResult::None => "none",
            DkimResult::TempError => "temperror",
            DkimResult::PermError => "permerror",
        }
    }
}

fn normalize_domain(value: &str) -> String {
    value.trim().trim_end_matches('.').to_ascii_lowercase()
}

fn domains_align(authenticated_domain: &str, header_from_domain: &str) -> bool {
    authenticated_domain == header_from_domain
        || authenticated_domain.ends_with(&format!(".{}", header_from_domain))
}

#[derive(Debug, Clone, PartialEq)]
pub enum SpfResult {
    Pass,
    Fail,
    SoftFail,
    Neutral,
    None,
    TempError,
}

impl SpfResult {
    pub fn as_str(&self) -> &'static str {
        match self {
            SpfResult::Pass => "pass",
            SpfResult::Fail => "fail",
            SpfResult::SoftFail => "softfail",
            SpfResult::Neutral => "neutral",
            SpfResult::None => "none",
            SpfResult::TempError => "temperror",
        }
    }
}

pub fn generate_dkim_dns_record(selector: &str, domain: &str, public_key: &str) -> String {
    let record = format!("v=DKIM1; k=rsa; p={}", public_key);

    // Split at 255 character boundaries for DNS TXT record compliance
    let chunks: Vec<String> = record
        .as_bytes()
        .chunks(255)
        .map(|chunk| String::from_utf8_lossy(chunk).to_string())
        .collect();

    let value = chunks
        .iter()
        .map(|chunk| format!("\"{}\"", chunk))
        .collect::<Vec<_>>()
        .join(" ");

    format!("{}._domainkey.{} IN TXT {}", selector, domain, value)
}

/// Generate SPF record for a domain
pub fn generate_spf_record(domain: &str, extra_ips: &[String]) -> String {
    let mut parts = vec!["v=spf1".to_string()];
    parts.push(format!("mx:{} -all", domain));
    for ip in extra_ips {
        parts.insert(1, format!("ip4:{}", ip));
    }
    parts.join(" ")
}

pub fn normalize_dkim_private_key_pem(private_key: &str) -> anyhow::Result<String> {
    if private_key.contains("BEGIN RSA PRIVATE KEY") {
        return Ok(private_key.to_string());
    }

    if private_key.contains("BEGIN PRIVATE KEY") {
        let key = RsaPrivateKey::from_pkcs8_pem(private_key)?;
        return Ok(key.to_pkcs1_pem(Pkcs1LineEnding::LF)?.to_string());
    }

    anyhow::bail!("unsupported DKIM private key format")
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand_core::OsRng;
    use rsa::pkcs1v15::SigningKey;
    use rsa::pkcs8::EncodePrivateKey;
    use rsa::pkcs8::EncodePublicKey;
    use rsa::signature::{SignatureEncoding, Signer};

    #[test]
    fn dkim_private_key_normalization_keeps_pkcs1_pem() {
        let key = RsaPrivateKey::new(&mut OsRng, 2048).expect("key");
        let pkcs1 = key
            .to_pkcs1_pem(Pkcs1LineEnding::LF)
            .expect("pkcs1")
            .to_string();

        assert_eq!(
            normalize_dkim_private_key_pem(&pkcs1).expect("normalized"),
            pkcs1
        );
    }

    #[test]
    fn dkim_private_key_normalization_converts_pkcs8_pem_to_pkcs1() {
        let key = RsaPrivateKey::new(&mut OsRng, 2048).expect("key");
        let pkcs8 = key
            .to_pkcs8_pem(rsa::pkcs8::LineEnding::LF)
            .expect("pkcs8")
            .to_string();
        let converted = normalize_dkim_private_key_pem(&pkcs8).expect("converted");

        assert!(converted.contains("BEGIN RSA PRIVATE KEY"));
        assert!(!converted.contains("BEGIN PRIVATE KEY-----"));
    }

    #[test]
    fn dmarc_spf_alignment_allows_exact_and_subdomain_matches() {
        assert!(domains_align("example.com", "example.com"));
        assert!(domains_align("mail.example.com", "example.com"));
        assert!(!domains_align("badexample.com", "example.com"));
    }

    #[test]
    fn dmarc_result_strings_are_stable() {
        assert_eq!(DmarcResult::Pass.as_str(), "pass");
        assert_eq!(DmarcResult::Fail.as_str(), "fail");
        assert_eq!(DmarcResult::None.as_str(), "none");
        assert_eq!(DmarcResult::TempError.as_str(), "temperror");
    }

    #[test]
    fn dkim_result_strings_are_stable() {
        assert_eq!(DkimResult::Pass.as_str(), "pass");
        assert_eq!(DkimResult::Fail.as_str(), "fail");
        assert_eq!(DkimResult::Neutral.as_str(), "neutral");
        assert_eq!(DkimResult::None.as_str(), "none");
        assert_eq!(DkimResult::TempError.as_str(), "temperror");
        assert_eq!(DkimResult::PermError.as_str(), "permerror");
    }

    #[test]
    fn dkim_body_canonicalization_matches_relaxed_rules() {
        assert_eq!(
            canonicalize_dkim_body(
                b"Line  \t one\r\n\r\nSecond\t\tline \r\n\r\n\r\n",
                DkimCanonicalization::Relaxed
            ),
            b"Line one\r\n\r\nSecond line\r\n"
        );
        assert_eq!(
            canonicalize_dkim_body(b"", DkimCanonicalization::Simple),
            b""
        );
    }

    #[test]
    fn dkim_public_key_record_accepts_generated_spki_key() {
        let key = RsaPrivateKey::new(&mut OsRng, 2048).expect("key");
        let public_key = key.to_public_key();
        let public_key_der = public_key.to_public_key_der().expect("public key");
        let record = format!(
            "v=DKIM1; k=rsa; p={}",
            STANDARD.encode(public_key_der.as_bytes())
        );

        assert!(dkim_public_key_from_dns_record(&record).is_some());
    }

    #[test]
    fn verifies_generated_dkim_signature() {
        let private_key = RsaPrivateKey::new(&mut OsRng, 2048).expect("key");
        let public_key = private_key.to_public_key();
        let body = "Hello DKIM\r\n";
        let body_hash = STANDARD.encode(Sha256::digest(canonicalize_dkim_body(
            body.as_bytes(),
            DkimCanonicalization::Simple,
        )));
        let dkim_without_signature = format!(
            " v=1; a=rsa-sha256; c=relaxed/simple; d=example.com; s=kuria; h=from:to:subject; bh={}; b=",
            body_hash
        );
        let unsigned = format!(
            "From: Sender <sender@example.com>\r\nTo: User <user@example.net>\r\nSubject: Test\r\nDKIM-Signature:{}\r\n\r\n{}",
            dkim_without_signature, body
        );
        let parsed = ParsedMessage::parse(&unsigned).expect("parsed unsigned");
        let signature_header = parsed.last_header("DKIM-Signature").expect("dkim");
        let signature =
            DkimSignature::parse(&signature_header.unfolded_value()).expect("signature");
        let signing_data =
            dkim_signing_data(&parsed, signature_header, &signature).expect("signing data");
        let signing_key = SigningKey::<Sha256>::new(private_key);
        let signature_value = STANDARD.encode(signing_key.sign(signing_data.as_bytes()).to_bytes());
        let raw = unsigned.replace("b=\r\n", &format!("b={}\r\n", signature_value));

        let parsed = ParsedMessage::parse(&raw).expect("parsed signed");
        let signature_header = parsed.last_header("DKIM-Signature").expect("dkim");
        let signature =
            DkimSignature::parse(&signature_header.unfolded_value()).expect("signature");

        assert_eq!(
            STANDARD.encode(Sha256::digest(canonicalize_dkim_body(
                parsed.body.as_bytes(),
                signature.body_canonicalization
            ))),
            signature.body_hash
        );
        assert!(verify_dkim_signature(
            &parsed,
            signature_header,
            &signature,
            &public_key
        ));
    }

    #[test]
    fn dmarc_lookup_domains_include_parent_fallback_for_subdomains() {
        assert_eq!(dmarc_lookup_domains("example.com"), vec!["example.com"]);
        assert_eq!(
            dmarc_lookup_domains("mail.example.com"),
            vec!["mail.example.com", "example.com"]
        );
        assert_eq!(
            dmarc_lookup_domains("A.B.Example.COM."),
            vec!["a.b.example.com", "example.com"]
        );
    }

    #[test]
    fn spf_ip_mechanisms_support_cidr_and_ip_versions() {
        assert!(spf_ip4_matches(
            "203.0.113.0/24",
            "203.0.113.42".parse().expect("ip")
        ));
        assert!(!spf_ip4_matches(
            "203.0.113.0/24",
            "203.0.114.42".parse().expect("ip")
        ));
        assert!(spf_ip6_matches(
            "2001:db8::/32",
            "2001:db8::1".parse().expect("ip")
        ));
        assert!(!spf_ip6_matches(
            "2001:db8::/32",
            "2001:db9::1".parse().expect("ip")
        ));
    }

    #[test]
    fn spf_qualifiers_map_to_results() {
        assert_eq!(split_spf_qualifier("-all"), ('-', "all"));
        assert_eq!(
            split_spf_qualifier("ip4:203.0.113.1"),
            ('+', "ip4:203.0.113.1")
        );
        assert_eq!(spf_result_for_qualifier('+'), SpfResult::Pass);
        assert_eq!(spf_result_for_qualifier('-'), SpfResult::Fail);
        assert_eq!(spf_result_for_qualifier('~'), SpfResult::SoftFail);
        assert_eq!(spf_result_for_qualifier('?'), SpfResult::Neutral);
    }

    #[test]
    fn spf_domain_mechanisms_parse_optional_domain_and_cidr() {
        assert_eq!(
            parse_domain_mechanism(
                "example.com",
                ":mail.example.com/24",
                "203.0.113.4".parse().expect("ip")
            ),
            Some(("mail.example.com".to_string(), Some(24)))
        );
        assert_eq!(
            parse_domain_mechanism("example.com", "/32", "203.0.113.4".parse().expect("ip")),
            Some(("example.com".to_string(), Some(32)))
        );
        assert_eq!(
            parse_domain_mechanism("example.com", "/129", "2001:db8::1".parse().expect("ip")),
            None
        );
    }
}
