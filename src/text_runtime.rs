pub fn ord(value: &str) -> Result<i64, String> {
    let mut chars = value.chars();
    let ch = chars
        .next()
        .ok_or_else(|| "ord() requires a non-empty string".to_string())?;
    if chars.next().is_some() {
        return Err("ord() requires a single-character string".to_string());
    }
    Ok(ch as u32 as i64)
}

pub fn chr(value: i64) -> Result<String, String> {
    let codepoint =
        u32::try_from(value).map_err(|_| format!("chr() code point must be in range 0..=1114111, got {value}"))?;
    let ch = char::from_u32(codepoint)
        .ok_or_else(|| format!("chr() code point must be in range 0..=1114111, got {value}"))?;
    Ok(ch.to_string())
}

#[cfg(test)]
mod tests {
    use super::{chr, ord};

    #[test]
    fn ord_and_chr_handle_unicode_scalars() {
        assert_eq!(ord("A").unwrap(), 65);
        assert_eq!(ord("🙂").unwrap(), 0x1f642);
        assert_eq!(chr(65).unwrap(), "A");
        assert_eq!(chr(0x1f642).unwrap(), "🙂");
    }

    #[test]
    fn ord_and_chr_reject_invalid_inputs() {
        assert!(ord("").is_err());
        assert!(ord("ab").is_err());
        assert!(chr(-1).is_err());
        assert!(chr(0x110000).is_err());
    }
}
