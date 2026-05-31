#[derive(Debug)]
pub enum ImapCommand {
    Capability,
    Noop,
    Logout,
    Login(String, String),      // username, password
    List(String, String),       // reference, pattern
    Select(String),             // mailbox name
    Fetch(String, String),      // sequence set, items
    Store(String, String),      // sequence set, flags
    Expunge,
    Unknown(String),
}

impl ImapCommand {
    pub fn parse(cmd_str: &str) -> Self {
        let parts: Vec<&str> = cmd_str.splitn(3, ' ').collect();
        let cmd = parts[0].to_uppercase();

        match cmd.as_str() {
            "CAPABILITY" => ImapCommand::Capability,
            "NOOP" => ImapCommand::Noop,
            "LOGOUT" => ImapCommand::Logout,
            "LOGIN" => {
                if parts.len() >= 3 {
                    let username = unquote(parts[1]);
                    let password = unquote(parts[2]);
                    ImapCommand::Login(username, password)
                } else {
                    ImapCommand::Unknown(cmd_str.to_string())
                }
            }
            "LIST" => {
                if parts.len() >= 3 {
                    let reference = unquote(parts[1]);
                    let pattern = unquote(parts[2]);
                    ImapCommand::List(reference, pattern)
                } else {
                    ImapCommand::Unknown(cmd_str.to_string())
                }
            }
            "SELECT" => {
                if parts.len() >= 2 {
                    ImapCommand::Select(unquote(parts[1]))
                } else {
                    ImapCommand::Unknown(cmd_str.to_string())
                }
            }
            "FETCH" => {
                if parts.len() >= 3 {
                    ImapCommand::Fetch(parts[1].to_string(), parts[2].to_string())
                } else {
                    ImapCommand::Unknown(cmd_str.to_string())
                }
            }
            "STORE" => {
                if parts.len() >= 3 {
                    ImapCommand::Store(parts[1].to_string(), parts[2].to_string())
                } else {
                    ImapCommand::Unknown(cmd_str.to_string())
                }
            }
            "EXPUNGE" => ImapCommand::Expunge,
            _ => ImapCommand::Unknown(cmd_str.to_string()),
        }
    }
}

fn unquote(s: &str) -> String {
    if s.starts_with('"') && s.ends_with('"') {
        s[1..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
}
