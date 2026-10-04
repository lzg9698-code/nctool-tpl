//! Reject non-finite NC numbers without interpreting comments and quoted names as code.
pub(crate) fn non_finite_nc(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    let mut i = 0;
    let mut comment_depth = 0usize;
    let mut quote = None;
    while i < bytes.len() {
        let c = bytes[i];
        if let Some(delimiter) = quote {
            if c == b'\\' {
                i += 2;
                continue;
            }
            if c == delimiter {
                quote = None;
            }
            i += 1;
            continue;
        }
        if comment_depth > 0 {
            if c == b'(' {
                comment_depth += 1;
            }
            if c == b')' {
                comment_depth -= 1;
            }
            i += 1;
            continue;
        }
        if c == b'(' {
            comment_depth = 1;
            i += 1;
            continue;
        }
        if c == b'\'' || c == b'"' {
            quote = Some(c);
            i += 1;
            continue;
        }
        if c == b';' {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        let literal = |start: usize| {
            let start = start + usize::from(matches!(bytes.get(start), Some(b'+' | b'-')));
            if !matches!(bytes.get(start..start + 3), Some(b"inf" | b"nan")) {
                return false;
            }
            let end = start + 3;
            match bytes.get(end) {
                None => true,
                Some(c) if !c.is_ascii_alphabetic() && *c != b'_' => true,
                Some(c) if c.is_ascii_alphabetic() => {
                    matches!(
                        bytes.get(end + 1),
                        Some(b'0'..=b'9' | b'+' | b'-' | b'[' | b'=')
                    ) || matches!(bytes.get(end + 1..end + 4), Some(b"inf" | b"nan"))
                }
                _ => false,
            }
        };
        // Standalone expression values, or immediately after an address letter.
        let boundary =
            i == 0 || !bytes[i - 1].is_ascii_alphanumeric() && !b"_.".contains(&bytes[i - 1]);
        if boundary && literal(i) || c.is_ascii_alphabetic() && literal(i + 1) {
            return true;
        }
        i += 1;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tight_addresses_and_expressions_are_checked_but_comments_and_names_are_not() {
        for text in [
            "Xinf", "XinfY0", "X-infZ1", "XnanYinf", "X+NaNY2", "R1=INF", "inf", "nan",
        ] {
            assert!(non_finite_nc(text), "{text}");
        }
        for text in [
            "G1 X1.5Y2",
            "(XinfY0)",
            "; Xnan\nG0 X1",
            "MSG(\"information\")",
            "CALL \"inf\"",
            "information",
        ] {
            assert!(!non_finite_nc(text), "{text}");
        }
    }
}
