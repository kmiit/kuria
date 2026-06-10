#[derive(Debug)]
pub enum ImapCommand {
    Capability,
    Noop,
    Logout,
    StartTls,
    Login(String, String), // username, password
    AuthenticatePlain(Option<String>),
    List(String, String),  // reference, pattern
    Select(String),        // mailbox name
    Fetch(String, String), // sequence set, items
    Store(String, String), // sequence set, flags
    Search(String),        // criteria
    UidFetch(String, String),
    UidStore(String, String),
    UidSearch(String),
    Copy(String, String), // sequence set, destination mailbox
    UidCopy(String, String),
    Move(String, String),
    UidMove(String, String),
    Append(AppendCommand),
    Status(String, String), // mailbox, requested items
    Expunge,
    Unknown(String),
}

#[derive(Debug, Clone)]
pub struct AppendCommand {
    pub mailbox: String,
    pub flags: Option<String>,
    pub literal_size: usize,
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
            "AUTHENTICATE" => {
                if parts.len() >= 2 && parts[1].eq_ignore_ascii_case("PLAIN") {
                    ImapCommand::AuthenticatePlain(
                        parts
                            .get(2)
                            .map(|initial| initial.trim().to_string())
                            .filter(|initial| !initial.is_empty()),
                    )
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
            "SEARCH" => {
                let criteria = cmd_str
                    .split_once(' ')
                    .map(|(_, rest)| rest.trim().to_string())
                    .unwrap_or_else(|| "ALL".to_string());
                ImapCommand::Search(criteria)
            }
            "UID" => parse_uid_command(cmd_str),
            "COPY" => {
                if parts.len() >= 3 {
                    ImapCommand::Copy(parts[1].to_string(), unquote(parts[2].trim()))
                } else {
                    ImapCommand::Unknown(cmd_str.to_string())
                }
            }
            "MOVE" => {
                if parts.len() >= 3 {
                    ImapCommand::Move(parts[1].to_string(), unquote(parts[2].trim()))
                } else {
                    ImapCommand::Unknown(cmd_str.to_string())
                }
            }
            "APPEND" => parse_append_command(cmd_str),
            "STATUS" => parse_status_command(cmd_str),
            "EXPUNGE" => ImapCommand::Expunge,
            "STARTTLS" => ImapCommand::StartTls,
            _ => ImapCommand::Unknown(cmd_str.to_string()),
        }
    }
}

fn parse_uid_command(cmd_str: &str) -> ImapCommand {
    let rest = cmd_str
        .split_once(' ')
        .map(|(_, rest)| rest.trim())
        .unwrap_or("");
    let parts: Vec<&str> = rest.splitn(3, ' ').collect();
    if parts.is_empty() {
        return ImapCommand::Unknown(cmd_str.to_string());
    }

    match parts[0].to_uppercase().as_str() {
        "FETCH" if parts.len() >= 3 => {
            ImapCommand::UidFetch(parts[1].to_string(), parts[2].to_string())
        }
        "STORE" if parts.len() >= 3 => {
            ImapCommand::UidStore(parts[1].to_string(), parts[2].to_string())
        }
        "SEARCH" => {
            let criteria = rest
                .split_once(' ')
                .map(|(_, criteria)| criteria.trim().to_string())
                .filter(|criteria| !criteria.is_empty())
                .unwrap_or_else(|| "ALL".to_string());
            ImapCommand::UidSearch(criteria)
        }
        "COPY" if parts.len() >= 3 => {
            ImapCommand::UidCopy(parts[1].to_string(), unquote(parts[2].trim()))
        }
        "MOVE" if parts.len() >= 3 => {
            ImapCommand::UidMove(parts[1].to_string(), unquote(parts[2].trim()))
        }
        _ => ImapCommand::Unknown(cmd_str.to_string()),
    }
}

fn parse_append_command(cmd_str: &str) -> ImapCommand {
    let Some(literal_start) = cmd_str.rfind('{') else {
        return ImapCommand::Unknown(cmd_str.to_string());
    };
    let Some(literal_end) = cmd_str[literal_start..].find('}') else {
        return ImapCommand::Unknown(cmd_str.to_string());
    };
    let literal_end = literal_start + literal_end;
    let literal_spec = &cmd_str[literal_start + 1..literal_end];
    let literal_size = literal_spec.trim_end_matches('+').parse::<usize>().ok();
    let Some(literal_size) = literal_size else {
        return ImapCommand::Unknown(cmd_str.to_string());
    };

    let command_end = cmd_str
        .find(char::is_whitespace)
        .unwrap_or(cmd_str.len())
        .min(literal_start);
    let before_literal = cmd_str[command_end..literal_start].trim();
    let tokens = tokenize_imap_args(before_literal);
    if tokens.is_empty() {
        return ImapCommand::Unknown(cmd_str.to_string());
    }

    let flags = before_literal
        .find('(')
        .and_then(|start| {
            before_literal[start..]
                .find(')')
                .map(|end| (start, start + end))
        })
        .map(|(start, end)| before_literal[start..=end].to_string());

    ImapCommand::Append(AppendCommand {
        mailbox: tokens[0].clone(),
        flags,
        literal_size,
    })
}

fn parse_status_command(cmd_str: &str) -> ImapCommand {
    let rest = cmd_str
        .split_once(' ')
        .map(|(_, rest)| rest.trim())
        .unwrap_or("");
    let tokens = tokenize_imap_args(rest);
    if tokens.len() < 2 {
        return ImapCommand::Unknown(cmd_str.to_string());
    }

    ImapCommand::Status(tokens[0].clone(), tokens[1..].join(" "))
}

fn tokenize_imap_args(input: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_quote = false;
    let mut escaped = false;

    for ch in input.chars() {
        if escaped {
            current.push(ch);
            escaped = false;
            continue;
        }

        match ch {
            '\\' if in_quote => escaped = true,
            '"' => in_quote = !in_quote,
            ch if ch.is_whitespace() && !in_quote => {
                if !current.is_empty() {
                    tokens.push(current.clone());
                    current.clear();
                }
            }
            _ => current.push(ch),
        }
    }

    if !current.is_empty() {
        tokens.push(current);
    }

    tokens
}

pub fn unquote(s: &str) -> String {
    if s.starts_with('"') && s.ends_with('"') {
        s[1..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_uid_search_criteria() {
        match ImapCommand::parse("UID SEARCH UNSEEN") {
            ImapCommand::UidSearch(criteria) => assert_eq!(criteria, "UNSEEN"),
            other => panic!("unexpected command: {:?}", other),
        }
    }

    #[test]
    fn parses_append_literal_and_flags() {
        match ImapCommand::parse("APPEND \"Sent\" (\\Seen) {42}") {
            ImapCommand::Append(command) => {
                assert_eq!(command.mailbox, "Sent");
                assert_eq!(command.flags.as_deref(), Some("(\\Seen)"));
                assert_eq!(command.literal_size, 42);
            }
            other => panic!("unexpected command: {:?}", other),
        }
    }

    #[test]
    fn parses_authenticate_plain_with_optional_initial_response() {
        match ImapCommand::parse("AUTHENTICATE PLAIN") {
            ImapCommand::AuthenticatePlain(None) => {}
            other => panic!("unexpected command: {:?}", other),
        }

        match ImapCommand::parse("AUTHENTICATE PLAIN AHVzZXIAcGFzcw==") {
            ImapCommand::AuthenticatePlain(Some(initial)) => {
                assert_eq!(initial, "AHVzZXIAcGFzcw==");
            }
            other => panic!("unexpected command: {:?}", other),
        }
    }

    #[test]
    fn parses_move_commands() {
        match ImapCommand::parse("MOVE 1:3 Trash") {
            ImapCommand::Move(sequence, mailbox) => {
                assert_eq!(sequence, "1:3");
                assert_eq!(mailbox, "Trash");
            }
            other => panic!("unexpected command: {:?}", other),
        }

        match ImapCommand::parse("UID MOVE 44:55 \"Spam\"") {
            ImapCommand::UidMove(sequence, mailbox) => {
                assert_eq!(sequence, "44:55");
                assert_eq!(mailbox, "Spam");
            }
            other => panic!("unexpected command: {:?}", other),
        }
    }
}
