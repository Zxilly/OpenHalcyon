/// Client's lookup2 variant for skin definition names. Its short final word
/// begins at bit zero, including when the tail occupies the third word.
pub fn skin_hash(name: &str) -> u32 {
    fn mix(a: &mut u32, b: &mut u32, c: &mut u32) {
        *a = a.wrapping_sub(*b).wrapping_sub(*c) ^ (*c >> 13);
        *b = b.wrapping_sub(*c).wrapping_sub(*a) ^ (*a << 8);
        *c = c.wrapping_sub(*a).wrapping_sub(*b) ^ (*b >> 13);
        *a = a.wrapping_sub(*b).wrapping_sub(*c) ^ (*c >> 12);
        *b = b.wrapping_sub(*c).wrapping_sub(*a) ^ (*a << 16);
        *c = c.wrapping_sub(*a).wrapping_sub(*b) ^ (*b >> 5);
        *a = a.wrapping_sub(*b).wrapping_sub(*c) ^ (*c >> 3);
        *b = b.wrapping_sub(*c).wrapping_sub(*a) ^ (*a << 10);
        *c = c.wrapping_sub(*a).wrapping_sub(*b) ^ (*b >> 15);
    }
    let (mut a, mut b, mut c) = (0x9e3779b9u32, 0x9e3779b9u32, 0x12345678u32);
    let mut chunks = name.as_bytes().chunks_exact(12);
    let add = |a: u32, bytes: &[u8]| a.wrapping_add(u32::from_le_bytes(bytes.try_into().unwrap()));
    for chunk in chunks.by_ref() {
        a = add(a, &chunk[..4]);
        b = add(b, &chunk[4..8]);
        c = add(c, &chunk[8..]);
        mix(&mut a, &mut b, &mut c);
    }
    let mut tail = [0; 12];
    let rem = chunks.remainder();
    tail[..rem.len()].copy_from_slice(rem);
    a = add(a, &tail[..4]);
    b = add(b, &tail[4..8]);
    c = add(c.wrapping_add(name.len() as u32), &tail[8..]);
    mix(&mut a, &mut b, &mut c);
    c
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn verified_default_skin_names() {
        assert_eq!(skin_hash("Amael_DefaultSkin"), 0x2fd7245d);
        assert_eq!(skin_hash("Baron_DefaultSkin"), 0x5b65dad4);
        assert_eq!(skin_hash("Ardan_DefaultSkin"), 0xe4a6ffc2);
    }
}
