//! Phira Pro 发码工具（作者侧；私钥只存在于你本地）
//!
//! 用法：
//!   unlock-keygen              交互模式：粘贴序列码 → 输出解密码（并自动复制到剪贴板）
//!   unlock-keygen <序列码>      直接发码
//!   unlock-keygen --gen-key     生成新密钥对，并写入同目录的 unlock-key.txt
//!
//! 私钥读取顺序：环境变量 PHIRA_UNLOCK_KEY → 同目录 unlock-key.txt → 交互式询问一次并保存。

use anyhow::Result;
use ed25519_dalek::{Signer, SigningKey};
use rand::rngs::OsRng;
use std::io::Write;
use std::path::PathBuf;

const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
const KEY_FILE: &str = "unlock-key.txt";

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

fn normalize(input: &str) -> String {
    input
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_uppercase())
        .collect()
}

fn group(s: &str, n: usize) -> String {
    s.as_bytes()
        .chunks(n)
        .map(|c| std::str::from_utf8(c).unwrap().to_owned())
        .collect::<Vec<_>>()
        .join("-")
}

fn sign(secret: &[u8; 32], serial: &str) -> String {
    let key = SigningKey::from_bytes(secret);
    let sig = key.sign(normalize(serial).as_bytes());
    group(&encode_base32(&sig.to_bytes()), 8)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn parse_hex(s: &str) -> Result<[u8; 32]> {
    let s: String = s.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    let bytes = s
        .as_bytes()
        .chunks(2)
        .map(|c| u8::from_str_radix(std::str::from_utf8(c).unwrap(), 16))
        .collect::<Result<Vec<u8>, _>>()?;
    let mut out = [0u8; 32];
    anyhow::ensure!(bytes.len() == 32, "私钥必须是 32 字节（64 个 hex 字符）");
    out.copy_from_slice(&bytes);
    Ok(out)
}

fn rust_const(public: &[u8; 32]) -> String {
    let body = public.iter().map(|b| format!("{b:#04x}")).collect::<Vec<_>>().join(", ");
    format!("const PUBLIC_KEY: [u8; 32] = [{body}];")
}

fn key_path() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|it| it.parent().map(|d| d.join(KEY_FILE)))
        .unwrap_or_else(|| PathBuf::from(KEY_FILE))
}

fn prompt(msg: &str) -> Result<String> {
    print!("{msg}");
    std::io::stdout().flush()?;
    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;
    Ok(line.trim().to_owned())
}

/// 把结果塞进系统剪贴板（Windows 用自带的 clip.exe；其它平台静默失败）。
fn copy_to_clipboard(text: &str) {
    if let Ok(mut child) = std::process::Command::new("clip")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .spawn()
    {
        if let Some(stdin) = child.stdin.as_mut() {
            let _ = stdin.write_all(text.as_bytes());
        }
        let _ = child.wait();
    }
}

/// 结束前停住，避免双击运行时窗口一闪就没。若 stdin 已到末尾则直接等一会儿。
fn pause() {
    println!("\n按回车键关闭窗口…");
    let mut line = String::new();
    let n = std::io::stdin().read_line(&mut line).unwrap_or(0);
    if n == 0 {
        std::thread::sleep(std::time::Duration::from_secs(15));
    }
}

fn load_key() -> Result<[u8; 32]> {
    if let Ok(k) = std::env::var("PHIRA_UNLOCK_KEY") {
        return parse_hex(&k);
    }
    let path = key_path();
    if let Ok(s) = std::fs::read_to_string(&path) {
        return parse_hex(&s);
    }
    println!("没找到私钥文件：{}", path.display());
    println!("请粘贴你的私钥（64 个 hex 字符）后回车 —— 只会问这一次，之后自动记住：");
    let key = parse_hex(&prompt("私钥> ")?)?;
    std::fs::write(&path, hex(&key))?;
    println!("已保存到 {}\n", path.display());
    Ok(key)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // 双击运行（无参数）或生成密钥时，结束时停住等按键；带序列码参数的脚本调用则直接退出。
    let keep_open = args.is_empty() || args.first().map(|s| s.as_str()) == Some("--gen-key");
    if let Err(err) = run(&args) {
        println!("\n出错：{err:#}");
    }
    if keep_open {
        pause();
    }
}

fn run(args: &[String]) -> Result<()> {
    match args.first().map(|s| s.as_str()) {
        Some("--gen-key") => gen_key(),
        Some("--key") => {
            let secret = args.get(1).ok_or_else(|| anyhow::anyhow!("缺少私钥"))?;
            let serial = args.get(2).ok_or_else(|| anyhow::anyhow!("缺少序列码"))?;
            println!("{}", sign(&parse_hex(secret)?, serial));
            Ok(())
        }
        Some(serial) => {
            let key = load_key()?;
            println!("{}", sign(&key, serial));
            Ok(())
        }
        None => interactive(),
    }
}

fn gen_key() -> Result<()> {
    let key = SigningKey::generate(&mut OsRng);
    let secret = key.to_bytes();
    let public = key.verifying_key().to_bytes();
    let path = key_path();
    std::fs::write(&path, hex(&secret))?;
    println!("已生成新密钥对。");
    println!("私钥（已保存到 {}，请自行备份，切勿外传）:", path.display());
    println!("  {}\n", hex(&secret));
    println!("公钥 —— 请把下面这一行整体替换到 prpr/src/activation.rs 里的 PUBLIC_KEY：");
    println!("{}\n", rust_const(&public));
    println!("（替换后三端都要重新打包，否则新码对不上旧包）");
    Ok(())
}

fn interactive() -> Result<()> {
    let path = key_path();
    println!("Phira Pro 发码工具");
    println!("私钥文件：{}（{}）\n", path.display(), if path.is_file() { "已找到" } else { "未找到，稍后会让你粘贴一次" });
    let key = load_key()?;
    println!("把序列码粘进来按回车 → 输出解密码（并自动复制到剪贴板）。");
    println!("直接回车退出。\n");
    loop {
        let s = prompt("序列码> ")?;
        if s.is_empty() {
            break;
        }
        let code = sign(&key, &s);
        println!("\n解密码> {}\n", code);
        copy_to_clipboard(&code);
        println!("（已复制到剪贴板，可直接粘到 Phira Pro 的「解密码」输入框）\n");
    }
    Ok(())
}
