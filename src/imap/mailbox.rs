/// Standard mailbox names
pub const INBOX: &str = "INBOX";
pub const SENT: &str = "Sent";
pub const DRAFTS: &str = "Drafts";
pub const TRASH: &str = "Trash";
pub const SPAM: &str = "Spam";

pub fn standard_mailboxes() -> Vec<&'static str> {
    vec![INBOX, SENT, DRAFTS, TRASH, SPAM]
}
