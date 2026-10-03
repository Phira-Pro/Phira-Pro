//! Phira Pro 授权：序列码 / 解密码
//!
//! - 序列码：首次启动时随机生成，之后永久固定。
//! - 解密码：作者用私钥对「规范化后的序列码」做 Ed25519 签名，再编码成 Base32。
//!   App 内只内置公钥，所以即使被逆向也无法伪造解密码。
//! - 校验方式：配置里存的只是解密码本身，每次启动都用 `序列码 + 解密码` 重新验签，
//!   因此把配置文件改成「已解锁」没有任何作用。

use ed25519_dalek::{Signature, VerifyingKey};

/// 作者公钥，对应 `tools/unlock-keygen` 里的私钥。
const PUBLIC_KEY: [u8; 32] = [
    0xd8, 0x1b, 0x70, 0x5e, 0x82, 0x03, 0x84, 0xb7, 0x51, 0x79, 0x39, 0xc1, 0x41, 0xeb, 0xb2, 0xc7, 0x85, 0xc4, 0x7f, 0x69, 0x70, 0x98, 0x69,
    0xab, 0x50, 0xce, 0xcf, 0xab, 0x1f, 0xc1, 0x55, 0x6e,
];

/// Crockford Base32：去掉了 I L O U，避免和 1 / 0 混淆。
const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

fn encode_base32(data: &[u8]) -> String {
    let mut out = String::new();
    let mut buf: u64 = 0;
    let mut bits: u32 = 0;
    for &b in data {
        buf = (buf << 8) | b as u64;
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(ALPHABET[((buf >> bits) & 0x1f) as usize] as char);
        }
        buf &= (1u64 << bits.max(1)) - 1;
    }
    if bits > 0 {
        out.push(ALPHABET[((buf << (5 - bits)) & 0x1f) as usize] as char);
    }
    out
}

fn decode_base32(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut buf: u64 = 0;
    let mut bits: u32 = 0;
    for c in s.chars() {
        let c = c.to_ascii_uppercase();
        let idx = ALPHABET.iter().position(|&a| a == c as u8)? as u64;
        buf = (buf << 5) | idx;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push(((buf >> bits) & 0xff) as u8);
        }
        buf &= (1u64 << bits.max(1)) - 1;
    }
    Some(out)
}

/// 去掉分隔符与空白并统一大写。序列码与解密码都按这个形式比较。
pub fn normalize(input: &str) -> String {
    input
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_uppercase())
        .collect()
}

/// 每 `n` 个字符插入一个 `-`，方便阅读与抄写。
pub fn group(s: &str, n: usize) -> String {
    s.as_bytes()
        .chunks(n)
        .map(|c| std::str::from_utf8(c).unwrap().to_owned())
        .collect::<Vec<_>>()
        .join("-")
}

/// 生成一个新的序列码：10 字节随机数 → 16 个 Base32 字符，每 4 个一组。
pub fn generate_serial() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 10];
    rand::thread_rng().fill_bytes(&mut bytes);
    group(&encode_base32(&bytes), 4)
}

/// 校验「序列码 + 解密码」是否匹配。
pub fn verify(serial: &str, code: &str) -> bool {
    let Ok(key) = VerifyingKey::from_bytes(&PUBLIC_KEY) else {
        return false;
    };
    let Some(bytes) = decode_base32(&normalize(code)) else {
        return false;
    };
    if bytes.len() != 64 {
        return false;
    }
    let mut arr = [0u8; 64];
    arr.copy_from_slice(&bytes);
    let sig = Signature::from_bytes(&arr);
    key.verify_strict(normalize(serial).as_bytes(), &sig).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 由 `tools/unlock-keygen --key <私钥> "A1B2-C3D4-E5F6-G7H8"` 生成。
    /// 这条测试的作用是锁死「发码工具」和「App 内验签」两套 Base32 实现的一致性。
    const SERIAL: &str = "A1B2-C3D4-E5F6-G7H8";
    const CODE: &str = "HTQQZ2X6-SJHXWQ1D-EVS4X2G0-R7JBSGSY-0KRX3CN7-SCF8V9ZN-PH16G71B-9HS6QJ6F-0091HP39-2J6KRWVE-JWYN9C4W-AMRCJ0WH-JYKY02G";

    #[test]
    fn generated_code_verifies() {
        assert!(verify(SERIAL, CODE));
    }

    #[test]
    fn other_serial_is_rejected() {
        assert!(!verify("A1B2-C3D4-E5F6-G7H9", CODE));
    }

    #[test]
    fn garbage_is_rejected() {
        assert!(!verify(SERIAL, ""));
        assert!(!verify(SERIAL, "NOT-A-REAL-CODE"));
    }

    #[test]
    fn serial_is_stable_shape() {
        let s = generate_serial();
        assert_eq!(s.len(), 19, "16 个字符 + 3 个分隔符");
        assert!(s.chars().all(|c| c == '-' || ALPHABET.contains(&(c as u8))));
    }
}
