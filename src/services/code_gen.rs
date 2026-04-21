use rand::seq::SliceRandom;
use rand::thread_rng;

// No 0/O/1/I/L — avoid handwriting / QR confusion.
const CHARSET: &[u8] = b"ABCDEFGHJKMNPQRSTUVWXYZ23456789";

pub fn generate_code(length: usize, prefix: Option<&str>) -> String {
    let length = length.clamp(4, 32);
    let mut rng = thread_rng();
    let body: String = (0..length)
        .map(|_| *CHARSET.choose(&mut rng).unwrap() as char)
        .collect();

    match prefix {
        Some(p) if !p.is_empty() => format!("{}{}", p.to_uppercase(), body),
        _ => body,
    }
}

pub fn normalize_code(input: &str) -> String {
    input
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .collect::<String>()
        .to_uppercase()
}
