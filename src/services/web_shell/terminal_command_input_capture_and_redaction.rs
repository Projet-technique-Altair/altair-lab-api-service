//! Capture and redact commands entered through the terminal WebSocket stream.

#[derive(Default)]
pub(super) struct TerminalCommandInputCapture {
    buffer: String,
}

impl TerminalCommandInputCapture {
    pub(super) fn capture_redacted_commands(&mut self, input: &[u8]) -> Vec<String> {
        self.extract_commands(input)
            .into_iter()
            .map(|command| redact_command(&command))
            .filter(|command| !command.is_empty())
            .collect()
    }

    fn extract_commands(&mut self, input: &[u8]) -> Vec<String> {
        let mut commands = Vec::new();

        for byte in input {
            match *byte {
                b'\r' | b'\n' => {
                    let command = self.buffer.trim().to_string();
                    self.buffer.clear();
                    if !command.is_empty() {
                        commands.push(command);
                    }
                }
                0x03 => self.buffer.clear(),
                0x08 | 0x7f => {
                    self.buffer.pop();
                }
                0x1b => {}
                b'\t' => self.buffer.push(' '),
                byte if byte.is_ascii_graphic() || byte == b' ' => {
                    self.buffer.push(byte as char);
                }
                _ => {}
            }
        }

        commands
    }
}

fn redact_command(command: &str) -> String {
    let trimmed = command.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    let mut redact_next = false;
    trimmed
        .split_whitespace()
        .map(|part| {
            let lower = part.to_ascii_lowercase();
            if redact_next {
                redact_next = false;
                return "[redacted]".to_string();
            }

            if matches!(
                lower.as_str(),
                "-p" | "--password" | "--token" | "--secret" | "--api-key" | "--key"
            ) {
                redact_next = true;
                return part.to_string();
            }

            if let Some((key, _)) = part.split_once('=') {
                let key_lower = key.to_ascii_lowercase();
                if key_lower.contains("password")
                    || key_lower.contains("passwd")
                    || key_lower.contains("secret")
                    || key_lower.contains("token")
                    || key_lower.contains("api_key")
                    || key_lower.contains("apikey")
                {
                    return format!("{key}=[redacted]");
                }
            }

            part.to_string()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::{redact_command, TerminalCommandInputCapture};

    #[test]
    fn extracts_entered_commands_from_terminal_input() {
        let mut capture = TerminalCommandInputCapture::default();
        let commands = capture.capture_redacted_commands(b"ls -la\rwhoami\n");

        assert_eq!(commands, vec!["ls -la", "whoami"]);
    }

    #[test]
    fn keeps_partial_command_until_enter_is_received() {
        let mut capture = TerminalCommandInputCapture::default();

        assert!(capture.capture_redacted_commands(b"ec").is_empty());
        assert_eq!(
            capture.capture_redacted_commands(b"ho test\r"),
            vec!["echo test"]
        );
    }

    #[test]
    fn redacts_sensitive_assignment_values() {
        assert_eq!(
            redact_command("curl --token abc SECRET_KEY=value"),
            "curl --token [redacted] SECRET_KEY=[redacted]"
        );
    }
}
