/// Standard Bob Jenkins `lookup2` hash (`0x81ed5c`, initval `0x12345678`) used by
/// `libGameKindred.so` (`0xc9ff34`/`0xc9ff9c`) for skin definition names in
/// `KindredSkinManifest.def`. Tail bytes `8..=10` shift into bits `8..=31` of `c`,
/// reserving the lowest byte of `c` for `length`.
pub fn skin_hash(name: &str) -> u32 {
    const GOLDEN_RATIO: u32 = 0x9e37_79b9;
    const SKIN_SEED: u32 = 0x1234_5678;

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
    let (mut a, mut b, mut c) = (GOLDEN_RATIO, GOLDEN_RATIO, SKIN_SEED);
    let (chunks, rem) = name.as_bytes().as_chunks::<12>();
    let add = |a: u32, bytes: &[u8]| a.wrapping_add(u32::from_le_bytes(bytes.try_into().unwrap()));
    for chunk in chunks {
        a = add(a, &chunk[..4]);
        b = add(b, &chunk[4..8]);
        c = add(c, &chunk[8..]);
        mix(&mut a, &mut b, &mut c);
    }
    let mut tail = [0u8; 12];
    tail[..rem.len()].copy_from_slice(rem);
    a = add(a, &tail[..4]);
    b = add(b, &tail[4..8]);
    // Client `0x81ee3c..0x81ee50` adds tail bytes 8, 9, 10 shifted by 8, 16, 24 bits.
    let tail_c = [0, tail[8], tail[9], tail[10]];
    c = add(c.wrapping_add(name.len() as u32), &tail_c);
    mix(&mut a, &mut b, &mut c);
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verified_default_skin_names() {
        for (name, expected) in [
            ("Amael_DefaultSkin", 0x2fd7_245d),
            ("Baron_DefaultSkin", 0x5b65_dad4),
            ("Ardan_DefaultSkin", 0xe4a6_ffc2),
            // Tail lengths 9, 10, 11 mod 12 — client `0x81ee3c..0x81ee50` shifts by 8, 16, 24.
            ("Catherine_DefaultSkin", 0x62db_dc96),
            ("Silvernail_DefaultSkin", 0xf4ec_ac3f),
            ("Churnwalker_DefaultSkin", 0xc73e_ab15),
            // Aliased legacy hero default skins from `KindredSkinManifest.def`.
            ("Krul_DefaultSkin", 0x9d1a_d5d3),
            ("Rona_DefaultSkin", 0x7bd2_186c),
            ("Skaarf_DefaultSkin", 0x09ea_f99f),
            ("Taka_DefaultSkin", 0xf7a1_e642),
        ] {
            assert_eq!(skin_hash(name), expected, "{name}");
        }
    }

    #[test]
    fn lookup2_all_tail_remainders_match_client_0x81ed5c() {
        for (input, expected) in [
            ("", 0x3df6_41a9),
            ("a", 0xdf46_2a3e),
            ("ab", 0xb269_f9cf),
            ("abc", 0x4648_dcca),
            ("abcd", 0x6220_8b4d),
            ("abcde", 0x429e_9ff5),
            ("abcdef", 0x5042_5dd0),
            ("abcdefg", 0x5c8a_23bf),
            ("abcdefgh", 0x41f3_c809),
            ("abcdefghi", 0xc7fd_15fe),
            ("abcdefghij", 0xfb0b_30b7),
            ("abcdefghijk", 0xaeaa_a6a3),
            ("abcdefghijkl", 0xe7f9_a00e),
        ] {
            assert_eq!(skin_hash(input), expected, "len={}", input.len());
        }
    }
}
