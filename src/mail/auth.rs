use hickory_resolver::TokioResolver;
use hickory_resolver::proto::rr::RData;
use tracing::{debug, warn};

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

    // Look up SPF TXT record
    let txt_response = match resolver.txt_lookup(sender_domain).await {
        Ok(r) => r,
        Err(_) => return SpfResult::None,
    };

    let spf_record = txt_response.answers().iter().find_map(|r| {
        if let RData::TXT(txt) = &r.data {
            let txt_str = txt.to_string();
            if txt_str.starts_with("v=spf1") {
                Some(txt_str)
            } else {
                None
            }
        } else {
            None
        }
    });

    match spf_record {
        Some(record) => {
            debug!("SPF record for {}: {}", sender_domain, record);
            // Basic SPF check - a full implementation would parse all mechanisms
            if record.contains(&format!("ip4:{}", sender_ip)) {
                SpfResult::Pass
            } else if record.contains("include:_spf.google.com") && is_google_ip(sender_ip) {
                SpfResult::Pass
            } else if record.contains("+all") {
                SpfResult::Pass
            } else if record.contains("?all") {
                SpfResult::Neutral
            } else if record.contains("-all") {
                SpfResult::Fail
            } else if record.contains("~all") {
                SpfResult::SoftFail
            } else {
                SpfResult::Neutral
            }
        }
        None => SpfResult::None,
    }
}

fn is_google_ip(_ip: &str) -> bool {
    // Simplified check - a full implementation would check against Google's IP ranges
    false
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

/// Generate DKIM DNS record for a domain
pub fn generate_dkim_dns_record(selector: &str, domain: &str, public_key: &str) -> String {
    format!(
        "{}._domainkey.{} IN TXT \"v=DKIM1; k=rsa; p={}\"",
        selector, domain, public_key
    )
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
