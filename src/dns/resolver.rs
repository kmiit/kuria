use hickory_resolver::TokioResolver;
use hickory_resolver::proto::rr::RData;
use tracing::debug;

/// DNS resolver for mail-related lookups
pub struct DnsResolver {
    resolver: TokioResolver,
}

impl DnsResolver {
    pub fn new() -> anyhow::Result<Self> {
        let resolver = TokioResolver::builder_tokio()?.build()?;
        Ok(Self { resolver })
    }

    /// Resolve MX records for a domain, returns the highest-priority mail server
    pub async fn resolve_mx(&self, domain: &str) -> Option<String> {
        let response = self.resolver.mx_lookup(domain).await.ok()?;
        for record in response.answers() {
            if let RData::MX(mx) = &record.data {
                let host = mx.exchange.to_string();
                let host = host.trim_end_matches('.').to_string();
                debug!("MX record for {}: {}", domain, host);
                return Some(host);
            }
        }
        None
    }

    /// Resolve A record for a hostname
    pub async fn resolve_a(&self, hostname: &str) -> Option<String> {
        let response = self.resolver.ipv4_lookup(hostname).await.ok()?;
        for record in response.answers() {
            if let RData::A(addr) = &record.data {
                return Some(addr.to_string());
            }
        }
        None
    }

    /// Check if a domain has valid MX records
    pub async fn has_mx(&self, domain: &str) -> bool {
        self.resolve_mx(domain).await.is_some()
    }

    /// Resolve SPF record for a domain
    pub async fn resolve_spf(&self, domain: &str) -> Option<String> {
        let response = self.resolver.txt_lookup(domain).await.ok()?;
        for record in response.answers() {
            if let RData::TXT(txt) = &record.data {
                let txt_str = txt.to_string();
                if txt_str.starts_with("v=spf1") {
                    return Some(txt_str);
                }
            }
        }
        None
    }
}
