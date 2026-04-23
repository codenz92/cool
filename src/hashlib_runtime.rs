use sha1::{Digest, Sha1};
use sha2::Sha256;

pub fn md5_hex(text: &str) -> String {
    format!("{:x}", md5::compute(text.as_bytes()))
}

pub fn sha1_hex(text: &str) -> String {
    let mut hasher = Sha1::new();
    hasher.update(text.as_bytes());
    format!("{:x}", hasher.finalize())
}

pub fn sha256_hex(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    format!("{:x}", hasher.finalize())
}

pub fn digest_hex(algo: &str, text: &str) -> Result<String, String> {
    match algo.to_ascii_lowercase().as_str() {
        "md5" => Ok(md5_hex(text)),
        "sha1" => Ok(sha1_hex(text)),
        "sha256" => Ok(sha256_hex(text)),
        other => Err(format!(
            "hashlib.digest() only supports md5/sha1/sha256, got '{}'",
            other
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::{digest_hex, md5_hex, sha1_hex, sha256_hex};

    #[test]
    fn known_hashes_match() {
        assert_eq!(md5_hex("abc"), "900150983cd24fb0d6963f7d28e17f72");
        assert_eq!(sha1_hex("abc"), "a9993e364706816aba3e25717850c26c9cd0d89d");
        assert_eq!(
            sha256_hex("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn digest_dispatches_and_rejects_unknown_algorithms() {
        assert_eq!(
            digest_hex("SHA256", "cool").unwrap(),
            "c34045c1a1db8d1b3fca8a692198466952daae07eaf6104b4c87ed3b55b6af1b"
        );
        let err = digest_hex("sha512", "cool").unwrap_err();
        assert!(err.contains("md5/sha1/sha256"));
    }
}
