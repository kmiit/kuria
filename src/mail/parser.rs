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
        .map(|a| format!("{} <{}>", a.name().unwrap_or(""), a.address().unwrap_or("")))
        .unwrap_or_default();

    let recipients: Vec<String> = message
        .to()
        .map(|addrs| {
            addrs
                .iter()
                .map(|a| format!("{} <{}>", a.name().unwrap_or(""), a.address().unwrap_or("")))
                .collect()
        })
        .unwrap_or_default();

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
    }
}
