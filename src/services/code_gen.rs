use rand::seq::SliceRandom;
use rand::thread_rng;

// No 0/O/1/I/L — avoid handwriting / QR confusion.
const CHARSET: &[u8] = b"ABCDEFGHJKMNPQRSTUVWXYZ23456789";

/// Longest code a manager can create. It is the redeem box's `maxlength`
/// (verification.rs), so every code we store can also be typed in full.
pub const MAX_CODE_LEN: usize = 40;
/// Prefix cap: the longest generated body (20) plus this fills `MAX_CODE_LEN`.
pub const MAX_PREFIX_LEN: usize = 20;

/// The part of a code that identifies it: ASCII letters (upper-cased) and
/// digits, nothing else. Hyphens, spaces and any other separator are display
/// only, so "LAT-ABCD", "lat abcd" and "LATABCD" are the same code.
///
/// Redemption matches on this. It must stay equal to the `codes.code_key`
/// generated column in migrations/006_code_key.sql, which computes the same
/// thing for every stored code.
pub fn code_key(input: &str) -> String {
    input
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_uppercase())
        .collect()
}

/// The form a manager's text is stored and shown in: whitespace removed,
/// letters upper-cased, hyphens kept. Anything else is refused, because the
/// redeem box drops it as people type and the code would look different
/// from what they were handed.
fn display_form(raw: &str) -> Result<String, char> {
    let mut out = String::with_capacity(raw.len());
    for c in raw.chars().filter(|c| !c.is_whitespace()) {
        if c.is_ascii_alphanumeric() || c == '-' {
            out.push(c.to_ascii_uppercase());
        } else {
            return Err(c);
        }
    }
    Ok(out)
}

/// A pasted custom code, ready to store. `None` for a blank line.
pub fn custom_code(raw: &str) -> Result<Option<String>, String> {
    let code = display_form(raw).map_err(|c| {
        format!(
            "\"{}\" contains '{c}'. Codes can use letters, numbers and hyphens.",
            raw.trim()
        )
    })?;
    if code.is_empty() {
        return Ok(None);
    }
    if code_key(&code).is_empty() {
        return Err(format!("\"{code}\" needs at least one letter or number."));
    }
    if code.chars().count() > MAX_CODE_LEN {
        return Err(format!(
            "\"{code}\" is longer than {MAX_CODE_LEN} characters."
        ));
    }
    Ok(Some(code))
}

/// A generation prefix, ready to prepend. `None` when left blank.
pub fn prefix(raw: &str) -> Result<Option<String>, String> {
    let p = display_form(raw)
        .map_err(|c| format!("The prefix contains '{c}'. Use letters, numbers and hyphens."))?;
    if p.is_empty() {
        return Ok(None);
    }
    if p.chars().count() > MAX_PREFIX_LEN {
        return Err(format!(
            "Keep the prefix to {MAX_PREFIX_LEN} characters or fewer."
        ));
    }
    Ok(Some(p))
}

/// `prefix` must already have been through [`prefix()`].
pub fn generate_code(length: usize, prefix: Option<&str>) -> String {
    let length = length.clamp(4, 32);
    let mut rng = thread_rng();
    let body: String = (0..length)
        .map(|_| *CHARSET.choose(&mut rng).unwrap() as char)
        .collect();

    match prefix {
        Some(p) => format!("{p}{body}"),
        None => body,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_ignores_separators_and_case() {
        assert_eq!(code_key("LAT-7XK2-M9QH"), "LAT7XK2M9QH");
        assert_eq!(code_key("lat 7xk2_m9qh"), "LAT7XK2M9QH");
        assert_eq!(code_key("  ks--abc  "), "KSABC");
        assert_eq!(code_key("---"), "");
        // Non-ASCII never reaches a key. These match what the SQL column
        // computes for the same text (checked on Postgres 16, including under
        // a Turkish collation, where upper('i') is not 'I').
        assert_eq!(code_key("CAFÉ-1"), "CAF1");
        assert_eq!(code_key("ＡＢ-1"), "1");
        assert_eq!(code_key("straße"), "STRAE");
        assert_eq!(code_key("ıi-9"), "I9");
        assert_eq!(code_key("😀ab"), "AB");
    }

    #[test]
    fn a_prefixed_code_redeems_however_it_is_typed() {
        let p = prefix("lat-").unwrap();
        let code = generate_code(12, p.as_deref());
        assert!(code.starts_with("LAT-"), "{code}");
        let key = code_key(&code);
        assert_eq!(code_key(&code.to_lowercase()), key);
        assert_eq!(code_key(&code.replace('-', "")), key);
        assert_eq!(code_key(&format!(" {} ", code.replace('-', " - "))), key);
    }

    #[test]
    fn prefix_keeps_hyphens_and_refuses_what_cannot_be_typed() {
        assert_eq!(prefix("  KS- ").unwrap().as_deref(), Some("KS-"));
        assert_eq!(prefix("lat-30d-").unwrap().as_deref(), Some("LAT-30D-"));
        assert_eq!(prefix("   ").unwrap(), None);
        assert!(prefix("LAT_").is_err());
        assert!(prefix("CAFÉ-").is_err());
        assert!(prefix(&"A".repeat(MAX_PREFIX_LEN + 1)).is_err());
        assert!(prefix(&"A".repeat(MAX_PREFIX_LEN)).is_ok());
    }

    #[test]
    fn custom_codes_keep_their_hyphens() {
        assert_eq!(
            custom_code("lat-trial-30d").unwrap().as_deref(),
            Some("LAT-TRIAL-30D")
        );
        assert_eq!(
            custom_code(" pod code ").unwrap().as_deref(),
            Some("PODCODE")
        );
        assert_eq!(custom_code("  ").unwrap(), None);
        assert!(custom_code("---").is_err());
        assert!(custom_code("HALLOWEEN_2026").is_err());
        assert!(custom_code(&"A".repeat(MAX_CODE_LEN + 1)).is_err());
    }

    #[test]
    fn longest_generated_code_fits_the_redeem_box() {
        let p = prefix(&"A".repeat(MAX_PREFIX_LEN)).unwrap();
        assert!(generate_code(20, p.as_deref()).len() <= MAX_CODE_LEN);
    }
}
