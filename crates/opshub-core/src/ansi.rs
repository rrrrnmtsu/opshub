use regex::Regex;
use std::sync::OnceLock;

/// Strip ANSI escape sequences (CSI, OSC, single-char ESC) from byte slices.
///
/// The goal is to produce a searchable plain-text representation. Malformed or
/// truncated sequences are left in place rather than fighting the parser - we
/// prefer to lose a little UX polish over discarding content.
pub fn strip(input: &[u8]) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(concat!(
            r"\x1B\[[0-?]*[ -/]*[@-~]",
            r"|\x1B\][^\x07\x1B]*(?:\x07|\x1B\\)",
            r"|\x1B[@-Z\\-_]",
        ))
        .expect("static regex compiles")
    });
    let text = String::from_utf8_lossy(input);
    re.replace_all(&text, "").into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_csi_color() {
        let s = b"\x1b[31mhello\x1b[0m world";
        assert_eq!(strip(s), "hello world");
    }

    #[test]
    fn strips_osc_title() {
        let s = b"\x1b]0;tab title\x07plain";
        assert_eq!(strip(s), "plain");
    }

    #[test]
    fn leaves_plain_text() {
        assert_eq!(strip(b"nothing to strip"), "nothing to strip");
    }
}
