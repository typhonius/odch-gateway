/// Parsed NMDC protocol messages.
#[derive(Debug, Clone, PartialEq)]
pub enum NmdcMessage {
    Lock {
        lock: Vec<u8>,
        pk: Option<String>,
    },
    Hello {
        nick: String,
    },
    Quit {
        nick: String,
    },
    MyInfo {
        nick: String,
        description: String,
        speed: String,
        email: String,
        share: u64,
    },
    Chat {
        nick: String,
        message: String,
    },
    HubName {
        name: String,
    },
    OpList {
        nicks: Vec<String>,
    },
    GetPass,
    ValidateDenide,
    Supports {
        features: Vec<String>,
    },
    NickList {
        nicks: Vec<String>,
    },
    PrivateMessage {
        from: String,
        to: String,
        message: String,
    },
    /// Admin event stream messages ($Event TYPE data|)
    Event {
        event_type: String,
        data: String,
    },
    /// Status response from $GetStatus
    Status {
        key: String,
        value: String,
    },
    /// User entry from $GetUserList
    UserEntry {
        nick: String,
        ip: String,
        share: String,
        user_type: String,
        description: String,
        email: String,
        speed: String,
    },
    /// Unknown/unhandled message
    Unknown(String),
}

/// Parse a single pipe-delimited NMDC message.
pub fn parse_message(raw: &str) -> NmdcMessage {
    let msg = raw.trim_end_matches('|');

    if let Some(rest) = msg.strip_prefix("$Lock ") {
        parse_lock(rest)
    } else if let Some(rest) = msg.strip_prefix("$Hello ") {
        NmdcMessage::Hello {
            nick: rest.to_string(),
        }
    } else if let Some(rest) = msg.strip_prefix("$Quit ") {
        NmdcMessage::Quit {
            nick: rest.to_string(),
        }
    } else if let Some(rest) = msg.strip_prefix("$MyINFO $ALL ") {
        parse_myinfo(rest)
    } else if let Some(rest) = msg.strip_prefix("$HubName ") {
        NmdcMessage::HubName {
            name: rest.to_string(),
        }
    } else if let Some(rest) = msg.strip_prefix("$OpList ") {
        let nicks = rest
            .split("$$")
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect();
        NmdcMessage::OpList { nicks }
    } else if msg == "$GetPass" {
        NmdcMessage::GetPass
    } else if msg.starts_with("$ValidateDenide") {
        NmdcMessage::ValidateDenide
    } else if let Some(rest) = msg.strip_prefix("$Supports ") {
        let features = rest.split_whitespace().map(|s| s.to_string()).collect();
        NmdcMessage::Supports { features }
    } else if let Some(rest) = msg.strip_prefix("$NickList ") {
        let nicks = rest
            .split("$$")
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect();
        NmdcMessage::NickList { nicks }
    } else if let Some(rest) = msg.strip_prefix("$To: ") {
        parse_private_message(rest)
    } else if let Some(rest) = msg.strip_prefix("$Event ") {
        parse_event(rest)
    } else if let Some(rest) = msg.strip_prefix("STATUS ") {
        parse_status(rest)
    } else if let Some(rest) = msg.strip_prefix("USER ") {
        if rest.contains('|') {
            parse_user_entry(rest)
        } else {
            NmdcMessage::Unknown(msg.to_string())
        }
    } else if msg.starts_with('<') {
        parse_chat(msg)
    } else {
        NmdcMessage::Unknown(msg.to_string())
    }
}

fn parse_lock(rest: &str) -> NmdcMessage {
    // rest = "EXTENDEDPROTOCOL_hub Pk=test"
    let (lock_str, pk) = if let Some(pos) = rest.find(" Pk=") {
        (&rest[..pos], Some(rest[pos + 4..].to_string()))
    } else {
        (rest, None)
    };
    NmdcMessage::Lock {
        lock: lock_str.as_bytes().to_vec(),
        pk,
    }
}

fn parse_myinfo(rest: &str) -> NmdcMessage {
    // rest = "nick description<tag>$ $speed\x01$email$share$"
    let nick_end = rest.find(' ').unwrap_or(rest.len());
    let nick = rest[..nick_end].to_string();

    let after_nick = if nick_end < rest.len() {
        &rest[nick_end + 1..]
    } else {
        ""
    };

    // Format: desc$$$speed\x01$email$share$
    // Split by $ gives: [desc, "", "", "speed\x01", email, share, ""]
    // Or with "$ $": [desc, " ", "speed\x01", email, share, ""]
    let parts: Vec<&str> = after_nick.split('$').collect();

    let description = parts.first().unwrap_or(&"").to_string();

    // Find speed: first non-empty part after description (skip empty separators)
    let mut speed_idx = 1;
    while speed_idx < parts.len() && parts[speed_idx].trim().is_empty() {
        speed_idx += 1;
    }

    let speed = parts
        .get(speed_idx)
        .unwrap_or(&"")
        .trim_end_matches('\x01')
        .trim_end_matches('>')
        .trim_start_matches('>')
        .to_string();
    let email = parts.get(speed_idx + 1).unwrap_or(&"").to_string();
    let share = parts
        .get(speed_idx + 2)
        .unwrap_or(&"0")
        .parse::<u64>()
        .unwrap_or(0);

    NmdcMessage::MyInfo {
        nick,
        description,
        speed,
        email,
        share,
    }
}

fn parse_chat(msg: &str) -> NmdcMessage {
    // <nick> message
    if let Some(end) = msg.find('>') {
        let nick = msg[1..end].to_string();
        let message = msg[end + 1..].trim_start().to_string();
        NmdcMessage::Chat { nick, message }
    } else {
        NmdcMessage::Unknown(msg.to_string())
    }
}

fn parse_private_message(rest: &str) -> NmdcMessage {
    // rest = "target From: sender $<sender> message"
    if let Some(from_pos) = rest.find(" From: ") {
        let to = rest[..from_pos].to_string();
        let after_from = &rest[from_pos + 7..];
        if let Some(msg_start) = after_from.find(" $") {
            let from = after_from[..msg_start].to_string();
            let message = after_from[msg_start + 2..].to_string();
            return NmdcMessage::PrivateMessage { from, to, message };
        }
    }
    NmdcMessage::Unknown(format!("$To: {}", rest))
}

fn parse_event(rest: &str) -> NmdcMessage {
    // rest = "TYPE data"
    if let Some(space) = rest.find(' ') {
        NmdcMessage::Event {
            event_type: rest[..space].to_string(),
            data: rest[space + 1..].to_string(),
        }
    } else {
        NmdcMessage::Event {
            event_type: rest.to_string(),
            data: String::new(),
        }
    }
}

fn parse_status(rest: &str) -> NmdcMessage {
    // rest = "key|value"
    if let Some(pipe) = rest.find('|') {
        NmdcMessage::Status {
            key: rest[..pipe].to_string(),
            value: rest[pipe + 1..].to_string(),
        }
    } else {
        NmdcMessage::Status {
            key: rest.to_string(),
            value: String::new(),
        }
    }
}

fn parse_user_entry(rest: &str) -> NmdcMessage {
    // rest = "nick|ip|share|type|desc|email|speed"
    let parts: Vec<&str> = rest.split('|').collect();
    NmdcMessage::UserEntry {
        nick: parts.first().unwrap_or(&"").to_string(),
        ip: parts.get(1).unwrap_or(&"").to_string(),
        share: parts.get(2).unwrap_or(&"").to_string(),
        user_type: parts.get(3).unwrap_or(&"").to_string(),
        description: parts.get(4).unwrap_or(&"").to_string(),
        email: parts.get(5).unwrap_or(&"").to_string(),
        speed: parts.get(6).unwrap_or(&"").to_string(),
    }
}

/// Split a raw TCP buffer into individual pipe-delimited messages.
/// Returns (parsed messages as owned strings, remaining incomplete data).
pub fn split_messages(buf: &str) -> (Vec<String>, String) {
    let mut messages = Vec::new();
    let mut last_end = 0;

    for (i, c) in buf.char_indices() {
        if c == '|' {
            let msg = &buf[last_end..=i];
            if msg.len() > 1 {
                messages.push(msg.to_string());
            }
            last_end = i + 1;
        }
    }

    let remainder = buf[last_end..].to_string();
    (messages, remainder)
}

/// Split admin port buffer into messages delimited by newlines.
///
/// The admin port uses `\n` as message delimiters, unlike the NMDC protocol
/// which uses `|`. STATUS responses contain `|` as a field separator
/// (e.g. `STATUS hub_name|Chaotic Neutral\n`), so splitting on `|` would
/// incorrectly fragment them.
pub fn split_admin_messages(buf: &str) -> (Vec<String>, String) {
    let mut messages = Vec::new();
    let mut last_end = 0;

    for (i, c) in buf.char_indices() {
        if c == '\n' {
            let msg = buf[last_end..i].trim();
            if !msg.is_empty() {
                messages.push(msg.to_string());
            }
            last_end = i + 1;
        }
    }

    let remainder = buf[last_end..].to_string();
    (messages, remainder)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_lock() {
        let msg = parse_message("$Lock EXTENDEDPROTOCOL_hub Pk=test|");
        match msg {
            NmdcMessage::Lock { lock, pk } => {
                assert_eq!(lock, b"EXTENDEDPROTOCOL_hub");
                assert_eq!(pk, Some("test".to_string()));
            }
            _ => panic!("Expected Lock, got {:?}", msg),
        }
    }

    #[test]
    fn test_parse_lock_no_pk() {
        let msg = parse_message("$Lock SOMELOCK|");
        match msg {
            NmdcMessage::Lock { lock, pk } => {
                assert_eq!(lock, b"SOMELOCK");
                assert_eq!(pk, None);
            }
            _ => panic!("Expected Lock, got {:?}", msg),
        }
    }

    #[test]
    fn test_parse_hello() {
        let msg = parse_message("$Hello TestUser|");
        assert_eq!(
            msg,
            NmdcMessage::Hello {
                nick: "TestUser".to_string()
            }
        );
    }

    #[test]
    fn test_parse_quit() {
        let msg = parse_message("$Quit SomeUser|");
        assert_eq!(
            msg,
            NmdcMessage::Quit {
                nick: "SomeUser".to_string()
            }
        );
    }

    #[test]
    fn test_parse_myinfo() {
        let msg = parse_message(
            "$MyINFO $ALL TestUser Test Desc<TestClient>$$$LAN(T1)\x01$test@email.com$12345$|",
        );
        match msg {
            NmdcMessage::MyInfo {
                nick,
                description,
                speed,
                email,
                share,
            } => {
                assert_eq!(nick, "TestUser");
                assert!(description.contains("Test Desc"));
                assert_eq!(speed, "LAN(T1)");
                assert_eq!(email, "test@email.com");
                assert_eq!(share, 12345);
            }
            _ => panic!("Expected MyInfo, got {:?}", msg),
        }
    }

    #[test]
    fn test_parse_chat() {
        let msg = parse_message("<Alice> Hello everyone!|");
        assert_eq!(
            msg,
            NmdcMessage::Chat {
                nick: "Alice".to_string(),
                message: "Hello everyone!".to_string(),
            }
        );
    }

    #[test]
    fn test_parse_hubname() {
        let msg = parse_message("$HubName My Cool Hub|");
        assert_eq!(
            msg,
            NmdcMessage::HubName {
                name: "My Cool Hub".to_string()
            }
        );
    }

    #[test]
    fn test_parse_oplist() {
        let msg = parse_message("$OpList Admin$$Bot$$|");
        match msg {
            NmdcMessage::OpList { nicks } => {
                assert_eq!(nicks, vec!["Admin", "Bot"]);
            }
            _ => panic!("Expected OpList, got {:?}", msg),
        }
    }

    #[test]
    fn test_parse_getpass() {
        assert_eq!(parse_message("$GetPass|"), NmdcMessage::GetPass);
    }

    #[test]
    fn test_parse_validatenied() {
        assert_eq!(
            parse_message("$ValidateDenide|"),
            NmdcMessage::ValidateDenide
        );
    }

    #[test]
    fn test_parse_supports() {
        let msg = parse_message("$Supports UserCommand NoGetINFO NoHello|");
        match msg {
            NmdcMessage::Supports { features } => {
                assert_eq!(features, vec!["UserCommand", "NoGetINFO", "NoHello"]);
            }
            _ => panic!("Expected Supports, got {:?}", msg),
        }
    }

    #[test]
    fn test_parse_event() {
        let msg = parse_message("$Event JOIN TestUser|");
        assert_eq!(
            msg,
            NmdcMessage::Event {
                event_type: "JOIN".to_string(),
                data: "TestUser".to_string(),
            }
        );
    }

    #[test]
    fn test_parse_event_chat() {
        let msg = parse_message("$Event CHAT Alice Hello world!|");
        assert_eq!(
            msg,
            NmdcMessage::Event {
                event_type: "CHAT".to_string(),
                data: "Alice Hello world!".to_string(),
            }
        );
    }

    #[test]
    fn test_parse_status() {
        let msg = parse_message("STATUS hub_name|TestHub|");
        assert_eq!(
            msg,
            NmdcMessage::Status {
                key: "hub_name".to_string(),
                value: "TestHub".to_string(),
            }
        );
    }

    #[test]
    fn test_parse_private_message() {
        let msg = parse_message("$To: Bob From: Alice $<Alice> Hey there!|");
        assert_eq!(
            msg,
            NmdcMessage::PrivateMessage {
                from: "Alice".to_string(),
                to: "Bob".to_string(),
                message: "<Alice> Hey there!".to_string(),
            }
        );
    }

    #[test]
    fn test_split_messages() {
        let buf = "$Hello Alice|$Hello Bob|$MyINFO partial";
        let (msgs, remainder) = split_messages(buf);
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0], "$Hello Alice|");
        assert_eq!(msgs[1], "$Hello Bob|");
        assert_eq!(remainder, "$MyINFO partial");
    }

    #[test]
    fn test_split_messages_empty() {
        let (msgs, remainder) = split_messages("");
        assert!(msgs.is_empty());
        assert!(remainder.is_empty());
    }

    #[test]
    fn test_parse_unknown() {
        let msg = parse_message("SomeRandomGarbage|");
        match msg {
            NmdcMessage::Unknown(s) => assert_eq!(s, "SomeRandomGarbage"),
            _ => panic!("Expected Unknown"),
        }
    }
}
