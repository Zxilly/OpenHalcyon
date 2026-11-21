//! Fixed-width NUL-padded ASCII/UTF-8 wire text buffers.
use binrw::{BinRead, BinWrite};
use std::fmt;

/// Fixed-width `N`-byte NUL-padded wire text field.
///
/// Retains all `N` bytes losslessly on decode and re-encode while exposing
/// NUL-trimmed views for callers.
#[derive(Clone, Copy, PartialEq, Eq, Debug, BinRead, BinWrite)]
#[brw(big)]
pub struct FixedAscii<const N: usize>(pub [u8; N]);

impl<const N: usize> Default for FixedAscii<N> {
    fn default() -> Self {
        Self([0u8; N])
    }
}

impl<const N: usize> FixedAscii<N> {
    pub const LEN: usize = N;

    /// Encodes `s` into a zero-padded `N`-byte buffer, truncating at `N` bytes.
    pub fn from_str_trunc(s: &str) -> Self {
        let mut buf = [0u8; N];
        let len = s.len().min(N);
        buf[..len].copy_from_slice(&s.as_bytes()[..len]);
        Self(buf)
    }

    pub const fn as_bytes(&self) -> &[u8; N] {
        &self.0
    }

    pub fn as_mut_bytes(&mut self) -> &mut [u8; N] {
        &mut self.0
    }

    /// Returns the prefix before the first NUL (`0x00`) byte.
    pub fn trimmed_bytes(&self) -> &[u8] {
        let end = self.0.iter().position(|&b| b == 0).unwrap_or(N);
        &self.0[..end]
    }

    /// Returns the NUL-trimmed prefix as UTF-8 if valid.
    pub fn trimmed_str(&self) -> Option<&str> {
        std::str::from_utf8(self.trimmed_bytes()).ok()
    }
}

impl<const N: usize> From<[u8; N]> for FixedAscii<N> {
    fn from(bytes: [u8; N]) -> Self {
        Self(bytes)
    }
}

impl<const N: usize> From<FixedAscii<N>> for [u8; N] {
    fn from(text: FixedAscii<N>) -> Self {
        text.0
    }
}

impl<const N: usize> AsRef<[u8]> for FixedAscii<N> {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl<const N: usize> fmt::Display for FixedAscii<N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.trimmed_str() {
            Some(s) => f.write_str(s),
            None => write!(f, "{:?}", self.trimmed_bytes()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn fixed_ascii_roundtrips_and_trims_at_first_nul() {
        let text = FixedAscii::<16>::from_str_trunc("Guest");
        assert_eq!(text.trimmed_bytes(), b"Guest");
        assert_eq!(text.trimmed_str(), Some("Guest"));

        let mut raw = [0xabu8; 16];
        raw[..5].copy_from_slice(b"Ringo");
        raw[5] = 0;
        let parsed = FixedAscii::<16>::read_be(&mut Cursor::new(&raw)).unwrap();
        assert_eq!(parsed.trimmed_bytes(), b"Ringo");
        assert_eq!(parsed.trimmed_str(), Some("Ringo"));

        let mut out = Vec::new();
        parsed.write_be(&mut Cursor::new(&mut out)).unwrap();
        assert_eq!(out, raw);

        let truncated = FixedAscii::<4>::from_str_trunc("ABCDEF");
        assert_eq!(truncated.as_bytes(), b"ABCD");
        assert_eq!(truncated.trimmed_str(), Some("ABCD"));
    }
}
