//! CRC-32/ISO-HDLC (zlib): reflected 0x04C11DB7, init and xorout 0xFFFFFFFF (spec section 4.2).

const TABLE: [u32; 256] = make_table();

const fn make_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 { (c >> 1) ^ 0xEDB8_8320 } else { c >> 1 };
            k += 1;
        }
        table[i] = c;
        i += 1;
    }
    table
}

pub fn crc32(data: &[u8]) -> u32 {
    let mut c = 0xFFFF_FFFFu32;
    for &b in data {
        c = TABLE[((c ^ b as u32) & 0xFF) as usize] ^ (c >> 8);
    }
    c ^ 0xFFFF_FFFF
}

#[cfg(test)]
mod tests {
    #[test]
    fn crc_is_affine_for_equal_length_inputs_and_detects_single_bit_changes() {
        for len in 1..512 {
            let a: Vec<u8> = (0..len).map(|i| (i * 37 + len * 11) as u8).collect();
            let b: Vec<u8> = (0..len).map(|i| (i * 19 + len * 7) as u8).collect();
            let xor: Vec<u8> = a.iter().zip(&b).map(|(x, y)| x ^ y).collect();
            let zero = vec![0; len];
            assert_eq!(super::crc32(&xor), super::crc32(&a) ^ super::crc32(&b) ^ super::crc32(&zero));
            let mut changed = a.clone();
            changed[len / 2] ^= 1;
            assert_ne!(super::crc32(&changed), super::crc32(&a));
        }
    }
}
