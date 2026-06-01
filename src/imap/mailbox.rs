#![allow(dead_code)]

/// Standard mailbox names
pub const INBOX: &str = "INBOX";
pub const SENT: &str = "Sent";
pub const DRAFTS: &str = "Drafts";
pub const TRASH: &str = "Trash";
pub const SPAM: &str = "Spam";

pub fn standard_mailboxes() -> Vec<&'static str> {
    vec![INBOX, SENT, DRAFTS, TRASH, SPAM]
}

/// Get IMAP flag string for an email
pub fn get_flags(is_read: bool, is_answered: bool, is_flagged: bool) -> String {
    let mut flags = Vec::new();
    if is_read {
        flags.push("\\Seen");
    }
    if is_answered {
        flags.push("\\Answered");
    }
    if is_flagged {
        flags.push("\\Flagged");
    }
    format!("({})", flags.join(" "))
}
