//! Compatibility implementation of the `strEnc` routine shipped with the web
//! login page.
//!
//! The routine is a textbook DES-ECB encoder with two deviations from NIST
//! FIPS 46-3: the final `L16/R16` swap is omitted and the key is zero-padded
//! to eight bytes rather than refused. The output is uppercase hexadecimal.
//! This exists only to drive the web login and must not be reused as general
//! cryptography.

const IP: [u8; 64] = [
    58, 50, 42, 34, 26, 18, 10, 2, 60, 52, 44, 36, 28, 20, 12, 4, 62, 54, 46, 38, 30, 22, 14, 6, 64, 56, 48,
    40, 32, 24, 16, 8, 57, 49, 41, 33, 25, 17, 9, 1, 59, 51, 43, 35, 27, 19, 11, 3, 61, 53, 45, 37, 29, 21,
    13, 5, 63, 55, 47, 39, 31, 23, 15, 7,
];
const FP: [u8; 64] = [
    40, 8, 48, 16, 56, 24, 64, 32, 39, 7, 47, 15, 55, 23, 63, 31, 38, 6, 46, 14, 54, 22, 62, 30, 37, 5, 45,
    13, 53, 21, 61, 29, 36, 4, 44, 12, 52, 20, 60, 28, 35, 3, 43, 11, 51, 19, 59, 27, 34, 2, 42, 10, 50, 18,
    58, 26, 33, 1, 41, 9, 49, 17, 57, 25,
];
const E_TABLE: [u8; 48] = [
    32, 1, 2, 3, 4, 5, 4, 5, 6, 7, 8, 9, 8, 9, 10, 11, 12, 13, 12, 13, 14, 15, 16, 17, 16, 17, 18, 19, 20,
    21, 20, 21, 22, 23, 24, 25, 24, 25, 26, 27, 28, 29, 28, 29, 30, 31, 32, 1,
];
const P_TABLE: [u8; 32] = [
    16, 7, 20, 21, 29, 12, 28, 17, 1, 15, 23, 26, 5, 18, 31, 10, 2, 8, 24, 14, 32, 27, 3, 9, 19, 13, 30, 6,
    22, 11, 4, 25,
];
const PC1: [u8; 56] = [
    57, 49, 41, 33, 25, 17, 9, 1, 58, 50, 42, 34, 26, 18, 10, 2, 59, 51, 43, 35, 27, 19, 11, 3, 60, 52, 44,
    36, 63, 55, 47, 39, 31, 23, 15, 7, 62, 54, 46, 38, 30, 22, 14, 6, 61, 53, 45, 37, 29, 21, 13, 5, 28, 20,
    12, 4,
];
const PC2: [u8; 48] = [
    14, 17, 11, 24, 1, 5, 3, 28, 15, 6, 21, 10, 23, 19, 12, 4, 26, 8, 16, 7, 27, 20, 13, 2, 41, 52, 31, 37,
    47, 55, 30, 40, 51, 45, 33, 48, 44, 49, 39, 56, 34, 53, 46, 42, 50, 36, 29, 32,
];
const SHIFT: [u8; 16] = [1, 1, 2, 2, 2, 2, 2, 2, 1, 2, 2, 2, 2, 2, 2, 1];
#[rustfmt::skip]
const S_BOXES: [[u8; 64]; 8] = [
    [14,4,13,1,2,15,11,8,3,10,6,12,5,9,0,7, 0,15,7,4,14,2,13,1,10,6,12,11,9,5,3,8,
     4,1,14,8,13,6,2,11,15,12,9,7,3,10,5,0, 15,12,8,2,4,9,1,7,5,11,3,14,10,0,6,13],
    [15,1,8,14,6,11,3,4,9,7,2,13,12,0,5,10, 3,13,4,7,15,2,8,14,12,0,1,10,6,9,11,5,
     0,14,7,11,10,4,13,1,5,8,12,6,9,3,2,15, 13,8,10,1,3,15,4,2,11,6,7,12,0,5,14,9],
    [10,0,9,14,6,3,15,5,1,13,12,7,11,4,2,8, 13,7,0,9,3,4,6,10,2,8,5,14,12,11,15,1,
     13,6,4,9,8,15,3,0,11,1,2,12,5,10,14,7, 1,10,13,0,6,9,8,7,4,15,14,3,11,5,2,12],
    [7,13,14,3,0,6,9,10,1,2,8,5,11,12,4,15, 13,8,11,5,6,15,0,3,4,7,2,12,1,10,14,9,
     10,6,9,0,12,11,7,13,15,1,3,14,5,2,8,4, 3,15,0,6,10,1,13,8,9,4,5,11,12,7,2,14],
    [2,12,4,1,7,10,11,6,8,5,3,15,13,0,14,9, 14,11,2,12,4,7,13,1,5,0,15,10,3,9,8,6,
     4,2,1,11,10,13,7,8,15,9,12,5,6,3,0,14, 11,8,12,7,1,14,2,13,6,15,0,9,10,4,5,3],
    [12,1,10,15,9,2,6,8,0,13,3,4,14,7,5,11, 10,15,4,2,7,12,9,5,6,1,13,14,0,11,3,8,
     9,14,15,5,2,8,12,3,7,0,4,10,1,13,11,6, 4,3,2,12,9,5,15,10,11,14,1,7,6,0,8,13],
    [4,11,2,14,15,0,8,13,3,12,9,7,5,10,6,1, 13,0,11,7,4,9,1,10,14,3,5,12,2,15,8,6,
     1,4,11,13,12,3,7,14,10,15,6,8,0,5,9,2, 6,11,13,8,1,4,10,7,9,5,0,15,14,2,3,12],
    [13,2,8,4,6,15,11,1,10,9,3,14,5,0,12,7, 1,15,13,8,10,3,7,4,12,5,6,11,0,14,9,2,
     7,11,4,1,9,12,14,2,0,6,10,13,15,3,5,8, 2,1,14,7,4,10,8,13,15,12,9,0,3,5,6,11],
];

fn bits(buf: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(buf.len() * 8);
    for &b in buf {
        for i in 0..8 {
            v.push((b >> (7 - i)) & 1);
        }
    }
    v
}

fn perm(src: &[u8], table: &[u8]) -> Vec<u8> {
    table.iter().map(|&t| src[(t - 1) as usize]).collect()
}

fn subkeys(key: &[u8; 8]) -> [Vec<u8>; 16] {
    let k = perm(&bits(key), &PC1);
    let (mut c, mut d) = (k[..28].to_vec(), k[28..].to_vec());
    std::array::from_fn(|i| {
        let s = SHIFT[i] as usize;
        c.rotate_left(s);
        d.rotate_left(s);
        let cd: Vec<u8> = c.iter().chain(d.iter()).copied().collect();
        perm(&cd, &PC2)
    })
}

fn des_block_no_swap(block: &[u8; 8], key: &[u8; 8]) -> [u8; 8] {
    let b = perm(&bits(block), &IP);
    let (mut left, mut right) = (b[..32].to_vec(), b[32..].to_vec());
    for sk in subkeys(key) {
        let x: Vec<u8> = perm(&right, &E_TABLE)
            .into_iter()
            .zip(sk)
            .map(|(a, c)| a ^ c)
            .collect();
        let mut s = Vec::with_capacity(32);
        for i in 0..8 {
            let ch = &x[i * 6..i * 6 + 6];
            let row = (ch[0] << 1 | ch[5]) as usize;
            let col =
                ((ch[1] as usize) << 3) | ((ch[2] as usize) << 2) | ((ch[3] as usize) << 1) | ch[4] as usize;
            let v = S_BOXES[i][row * 16 + col];
            s.push((v >> 3) & 1);
            s.push((v >> 2) & 1);
            s.push((v >> 1) & 1);
            s.push(v & 1);
        }
        let pv = perm(&s, &P_TABLE);
        let new_right: Vec<u8> = left.iter().zip(pv).map(|(a, c)| a ^ c).collect();
        left = right;
        right = new_right;
    }
    let mut lr = Vec::with_capacity(64);
    lr.extend_from_slice(&left);
    lr.extend_from_slice(&right);
    let f = perm(&lr, &FP);
    let mut out = [0u8; 8];
    for i in 0..8 {
        let mut v = 0u8;
        for j in 0..8 {
            v = (v << 1) | f[i * 8 + j];
        }
        out[i] = v;
    }
    out
}

/// Encode `data` with `key` using the login routine shipped by the device.
pub fn str_enc(data: &str, key: &str) -> String {
    let mut k = [0u8; 8];
    for (i, b) in key.bytes().take(8).enumerate() {
        k[i] = b;
    }
    let data_bytes = data.as_bytes();
    let mut out = String::new();
    for chunk in data_bytes.chunks(8) {
        let mut block = [0u8; 8];
        block[..chunk.len()].copy_from_slice(chunk);
        let c = des_block_no_swap(&block, &k);
        for b in c {
            use std::fmt::Write;
            let _ = write!(&mut out, "{:02X}", b);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::str_enc;

    #[test]
    fn empty_data_empty_output() {
        assert_eq!(str_enc("", "123456789"), "");
    }

    #[test]
    fn output_is_uppercase_hex_multiple_of_sixteen() {
        let out = str_enc("admin", "123456789");
        assert_eq!(out.len(), 16);
        assert!(out
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_lowercase()));
    }

    #[test]
    fn multi_block_encoding_concatenates() {
        // Two-block input: each 8-byte slab is encoded independently.
        let short = str_enc("12345678", "key");
        let long = str_enc("1234567890ABCDEF", "key");
        assert_eq!(&long[..16], &short[..16]);
        assert_eq!(long.len(), 32);
    }
}
