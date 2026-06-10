use mail_parser::{MessageParser, MimeHeaders};

/// Parse an email message from raw bytes
pub fn parse_email(raw: &[u8]) -> anyhow::Result<ParsedEmail> {
    let message = MessageParser::default()
        .parse(raw)
        .ok_or_else(|| anyhow::anyhow!("Failed to parse email"))?;

    let from_address = message
        .from()
        .and_then(|f| f.first())
        .and_then(|a| a.address())
        .map(str::to_ascii_lowercase);

    let sender = message
        .from()
        .and_then(|f| f.first())
        .map(|a| format_address(a.name(), a.address()))
        .unwrap_or_default();

    let mut recipients = collect_addresses(message.to());
    recipients.extend(collect_addresses(message.cc()));

    let subject = message.subject().map(|s| s.to_string());

    let body_text = message.body_text(0).map(|s| s.to_string());

    let body_html = message.body_html(0).map(|s| s.to_string());

    let message_id = message.message_id().map(|s| s.to_string());

    // Extract attachments with content
    let attachments: Vec<AttachmentInfo> = message
        .attachments()
        .map(|att| AttachmentInfo {
            filename: att.attachment_name().map(|s| s.to_string()),
            content_type: att
                .content_type()
                .map(|ct| format!("{}/{}", ct.ctype(), ct.subtype().unwrap_or("octet-stream"))),
            data: att.contents().to_vec(),
        })
        .collect();

    Ok(ParsedEmail {
        sender,
        recipients,
        subject,
        body_text,
        body_html,
        message_id,
        from_address,
        attachments,
    })
}

fn format_address(name: Option<&str>, address: Option<&str>) -> String {
    let name = name.unwrap_or_default().trim();
    let address = address.unwrap_or_default().trim();

    match (name.is_empty(), address.is_empty()) {
        (_, true) => name.to_string(),
        (true, false) => address.to_string(),
        (false, false) => format!("{} <{}>", name, address),
    }
}

fn collect_addresses(addresses: Option<&mail_parser::Address<'_>>) -> Vec<String> {
    addresses
        .map(|addrs| {
            addrs
                .iter()
                .map(|a| format_address(a.name(), a.address()))
                .collect()
        })
        .unwrap_or_default()
}

#[derive(Debug)]
pub struct ParsedEmail {
    pub sender: String,
    pub recipients: Vec<String>,
    pub subject: Option<String>,
    pub body_text: Option<String>,
    pub body_html: Option<String>,
    pub message_id: Option<String>,
    pub from_address: Option<String>,
    pub attachments: Vec<AttachmentInfo>,
}

#[derive(Debug)]
pub struct AttachmentInfo {
    pub filename: Option<String>,
    pub content_type: Option<String>,
    pub data: Vec<u8>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_email_extracts_lowercase_from_address() {
        let parsed = parse_email(
            b"From: User <User@Example.COM>\r\nTo: dest@example.com\r\nSubject: Hi\r\n\r\nBody",
        )
        .expect("parsed");

        assert_eq!(parsed.from_address.as_deref(), Some("user@example.com"));
        assert_eq!(parsed.sender, "User <User@Example.COM>");
        assert_eq!(parsed.recipients, vec!["dest@example.com".to_string()]);
    }

    #[test]
    fn parse_email_recipients_include_cc_but_not_bcc() {
        let parsed = parse_email(
            b"From: sender@example.com\r\nTo: dest@example.com\r\nCc: Copy <copy@example.com>\r\nBcc: hidden@example.com\r\nSubject: Hi\r\n\r\nBody",
        )
        .expect("parsed");

        assert_eq!(
            parsed.recipients,
            vec![
                "dest@example.com".to_string(),
                "Copy <copy@example.com>".to_string(),
            ]
        );
    }

    #[test]
    fn format_address_omits_empty_display_name() {
        assert_eq!(
            format_address(None, Some("user@example.com")),
            "user@example.com"
        );
        assert_eq!(
            format_address(Some("User"), Some("user@example.com")),
            "User <user@example.com>"
        );
        assert_eq!(format_address(Some("User"), None), "User");
    }
}
