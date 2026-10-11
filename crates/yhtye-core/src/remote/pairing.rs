//! Pairing codes and device tokens (`core-design.md` §18.3).
//!
//! A code is 8 characters from an alphabet without look-alikes (no 0/O, 1/I/L),
//! valid for 5 minutes, used once, and dropped after 5 wrong tries. Only one
//! code is valid at a time: issuing a new one drops the old one.

use sha2::{Digest, Sha256};

pub const CODE_LEN: usize = 8;
/// How long a pairing code is valid.
pub const CODE_TTL_MS: u64 = 5 * 60 * 1000;
/// Wrong tries after which the code is dropped.
pub const MAX_FAILURES: u32 = 5;
/// Upper-case letters and digits without 0, O, 1, I and L.
const ALPHABET: &[u8] = b"ABCDEFGHJKMNPQRSTUVWXYZ23456789";
/// Bytes of a device token (256 bits).
const TOKEN_BYTES: usize = 32;

/// The valid code, if any.
#[derive(Debug, Default)]
pub struct Pairing {
    active: Option<ActiveCode>,
}

#[derive(Debug)]
struct ActiveCode {
    code: String,
    expires_at_ms: u64,
    failures: u32,
}

/// Why a code was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PairRefused {
    /// No code is valid (none issued, expired, used, or too many wrong tries).
    NoCode,
    Wrong,
}

impl Pairing {
    /// Issues a new code (dropping the old one); returns it and its expiry.
    pub fn issue(&mut self, now_ms: u64) -> Result<(String, u64), getrandom::Error> {
        let code = random_code()?;
        let expires_at_ms = now_ms + CODE_TTL_MS;
        self.active = Some(ActiveCode {
            code: code.clone(),
            expires_at_ms,
            failures: 0,
        });
        Ok((code, expires_at_ms))
    }

    /// Uses the code: `Ok` drops it (it was right); a wrong one counts a try.
    pub fn redeem(&mut self, input: &str, now_ms: u64) -> Result<(), PairRefused> {
        let Some(active) = self.active.as_mut() else {
            return Err(PairRefused::NoCode);
        };
        if now_ms >= active.expires_at_ms {
            self.active = None;
            return Err(PairRefused::NoCode);
        }
        if constant_time_eq(normalize(input).as_bytes(), active.code.as_bytes()) {
            self.active = None;
            return Ok(());
        }
        active.failures += 1;
        if active.failures >= MAX_FAILURES {
            self.active = None;
        }
        Err(PairRefused::Wrong)
    }

    /// The expiry of the valid code, if one is.
    #[must_use]
    pub fn expires_at_ms(&self, now_ms: u64) -> Option<u64> {
        self.active
            .as_ref()
            .map(|a| a.expires_at_ms)
            .filter(|&at| now_ms < at)
    }

    pub fn clear(&mut self) {
        self.active = None;
    }
}

/// What a person typed, in the code's form: upper case, without spaces or dashes.
fn normalize(input: &str) -> String {
    input
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .map(|c| c.to_ascii_uppercase())
        .collect()
}

fn random_code() -> Result<String, getrandom::Error> {
    // Rejection sampling keeps every character equally likely.
    let limit = u8::try_from(256 - 256 % ALPHABET.len()).unwrap_or(u8::MAX);
    let mut code = String::with_capacity(CODE_LEN);
    let mut buf = [0u8; 32];
    while code.len() < CODE_LEN {
        getrandom::fill(&mut buf)?;
        for &b in buf.iter().filter(|&&b| b < limit) {
            if code.len() == CODE_LEN {
                break;
            }
            code.push(char::from(ALPHABET[usize::from(b) % ALPHABET.len()]));
        }
    }
    Ok(code)
}

/// A new device token (256 random bits, hex).
pub fn new_token() -> Result<String, getrandom::Error> {
    random_hex(TOKEN_BYTES)
}

/// `n` random bytes as hex (ids).
pub fn random_hex(n: usize) -> Result<String, getrandom::Error> {
    let mut bytes = vec![0u8; n];
    getrandom::fill(&mut bytes)?;
    Ok(hex(&bytes))
}

/// SHA-256 of a token (hex): what the database keeps.
#[must_use]
pub fn token_hash(token: &str) -> String {
    hex(&Sha256::digest(token.as_bytes()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Compares without stopping at the first difference.
#[must_use]
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_eight_unambiguous_characters() {
        for _ in 0..200 {
            let code = random_code().expect("code");
            assert_eq!(code.len(), CODE_LEN);
            assert!(
                code.bytes().all(|c| ALPHABET.contains(&c)),
                "unexpected character in {code}"
            );
            assert!(!code.contains(['0', 'O', '1', 'I', 'L', 'l']), "{code}");
        }
    }

    #[test]
    fn a_code_works_once_and_is_case_and_space_insensitive() {
        let mut p = Pairing::default();
        let (code, expires) = p.issue(1000).expect("issue");
        assert_eq!(expires, 1000 + CODE_TTL_MS);
        let typed = format!(" {}-{} ", &code[..4], code[4..].to_lowercase());
        assert_eq!(p.redeem(&typed, 2000), Ok(()));
        assert_eq!(p.redeem(&code, 2001), Err(PairRefused::NoCode), "used");
    }

    #[test]
    fn a_code_expires_after_five_minutes() {
        let mut p = Pairing::default();
        let (code, _) = p.issue(0).expect("issue");
        assert_eq!(p.expires_at_ms(CODE_TTL_MS - 1), Some(CODE_TTL_MS));
        assert_eq!(p.expires_at_ms(CODE_TTL_MS), None);
        assert_eq!(p.redeem(&code, CODE_TTL_MS), Err(PairRefused::NoCode));
    }

    #[test]
    fn five_wrong_tries_drop_the_code() {
        let mut p = Pairing::default();
        let (code, _) = p.issue(0).expect("issue");
        for _ in 0..MAX_FAILURES {
            assert_eq!(p.redeem("WRONGCDE", 1), Err(PairRefused::Wrong));
        }
        assert_eq!(p.redeem(&code, 2), Err(PairRefused::NoCode));
        assert_eq!(p.expires_at_ms(2), None);
    }

    #[test]
    fn a_new_code_replaces_the_old_one() {
        let mut p = Pairing::default();
        let (old, _) = p.issue(0).expect("issue");
        let (new, _) = p.issue(1).expect("issue");
        if old != new {
            assert_eq!(p.redeem(&old, 2), Err(PairRefused::Wrong));
        }
        assert_eq!(p.redeem(&new, 3), Ok(()));
    }

    #[test]
    fn tokens_are_256_bit_hex_and_hashed_with_sha256() {
        let t = new_token().expect("token");
        assert_eq!(t.len(), 64);
        assert!(t.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(t, new_token().expect("token"));
        assert_eq!(
            token_hash("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert!(constant_time_eq(b"ab", b"ab"));
        assert!(!constant_time_eq(b"ab", b"ac"));
        assert!(!constant_time_eq(b"ab", b"abc"));
    }
}
