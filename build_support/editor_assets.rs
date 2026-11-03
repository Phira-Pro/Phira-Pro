//! Stages the editor frontend from the pinned RPEN checkout; shared by build scripts.
//! The generated tree stays out of version control: only the pin and the upstream
//! bundle manifest decide what ships.
use serde_json::Value;
use std::{
    env, fs,
    path::{Component, Path, PathBuf},
    process::Command,
    sync::OnceLock,
};

const PIN: &str = "scripts/editor/upstream.json";
const BUNDLE: &str = "phira-pro-bundle.json";
const STAMP: &str = ".staged.json";
const TARGET: &str = "assets/rpen-editor";
const REQUIRED: [&str; 6] = [
    "index.html",
    "LICENSE",
    "NOTICE",
    "pro/bridge.js",
    "pro/editor.mjs",
    "src/ui/mod-runtime.mjs",
];

pub fn ensure(root: &Path) {
    let pin_path = root.join(PIN);
    let target = root.join(TARGET);
    println!("cargo:rerun-if-changed={}", pin_path.display());
    println!("cargo:rerun-if-changed={}", root.join("build_support/editor_assets.rs").display());
    println!("cargo:rerun-if-changed={}", target.join(STAMP).display());

    let Ok(pin) = fs::read_to_string(&pin_path) else {
        println!("cargo:warning=缺少 {PIN}，跳过制谱器前端准备");
        return;
    };
    let mut staged_now = false;
    if !staged(&target, &pin) {
        if env::var_os("PHIRA_SKIP_EDITOR_ASSETS").is_some() {
            println!("cargo:warning=已跳过制谱器前端准备（PHIRA_SKIP_EDITOR_ASSETS）");
            return;
        }
        stage(root, &target, &pin);
        staged_now = true;
    }
    // 索引只随生成树变化；否则每次构建脚本重跑都要重新读一遍几百个文件。
    if staged_now || !target.join("files.json").is_file() {
        index(&target);
    }
}

fn staged(target: &Path, pin: &str) -> bool {
    let Ok(stamp) = fs::read_to_string(target.join(STAMP)) else {
        return false;
    };
    let Ok(stamp) = serde_json::from_str::<Value>(&stamp) else {
        return false;
    };
    stamp["pin"].as_str() == Some(pin) && REQUIRED.iter().all(|name| target.join(name).is_file())
}

fn stage(root: &Path, target: &Path, pin: &str) {
    let upstream: Value = serde_json::from_str(pin).unwrap_or_else(|error| panic!("{PIN} 不是合法 JSON: {error}"));
    let repository = field(&upstream, "repository");
    let revision = field(&upstream, "revision");
    let cache = work_dir(root).join("rpe-next-upstream");
    let staging = work_dir(root).join("rpe-next-staging");
    println!("cargo:warning=正在从 {repository} 拉取制谱器前端 {revision}（首次构建需要联网）");
    checkout(&cache, &repository, &revision).unwrap_or_else(|error| panic!("制谱器前端准备失败: {error}"));
    let names = bundle(&cache);
    remove(&staging);
    for name in &names {
        let destination = staging.join(name);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent).unwrap_or_else(|error| panic!("创建 {} 失败: {error}", parent.display()));
        }
        fs::copy(cache.join(name), &destination).unwrap_or_else(|error| panic!("复制制谱器资源 {name} 失败: {error}"));
    }
    let manifest = serde_json::to_string_pretty(&upstream).unwrap() + "\n";
    fs::write(staging.join("UPSTREAM.json"), manifest).unwrap_or_else(|error| panic!("写入 UPSTREAM.json 失败: {error}"));
    remove(target);
    copy_tree(&staging, target);
    remove(&staging);
    let stamp = serde_json::json!({ "pin": pin, "revision": revision });
    let stamp = serde_json::to_string_pretty(&stamp).unwrap() + "\n";
    fs::write(target.join(STAMP), stamp).unwrap_or_else(|error| panic!("写入 {STAMP} 失败: {error}"));
}

fn field<'a>(upstream: &'a Value, name: &str) -> &'a str {
    upstream[name]
        .as_str()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| panic!("{PIN} 缺少 {name}"))
}

fn checkout(cache: &Path, repository: &str, revision: &str) -> Result<(), String> {
    if cache.join(".git").is_dir() && run_git(cache, &["rev-parse", "HEAD"]).ok().as_deref() == Some(revision) {
        return Ok(());
    }
    remove(cache);
    fs::create_dir_all(cache).map_err(|error| format!("创建 {} 失败: {error}", cache.display()))?;
    run_git(cache, &["init", "-q"])?;
    run_git(cache, &["remote", "add", "origin", repository])?;
    run_git(cache, &["fetch", "-q", "--depth", "1", "origin", revision])?;
    run_git(cache, &["checkout", "-q", "--detach", "FETCH_HEAD"])?;
    let head = run_git(cache, &["rev-parse", "HEAD"])?;
    if head != revision {
        return Err(format!("检出 {head}，期望 {revision}"));
    }
    Ok(())
}

fn run_git(directory: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(directory)
        .output()
        .map_err(|error| format!("无法运行 git: {error}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("git {} 失败: {}", args.join(" "), stderr.trim()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn bundle(root: &Path) -> Vec<String> {
    let text = fs::read_to_string(root.join(BUNDLE)).unwrap_or_else(|error| panic!("读取 {BUNDLE} 失败: {error}"));
    let bundle: Value = serde_json::from_str(&text).unwrap_or_else(|error| panic!("{BUNDLE} 不是合法 JSON: {error}"));
    if bundle["schema"].as_u64() != Some(1) {
        panic!("{BUNDLE} 的 schema 不受支持");
    }
    let files = bundle["files"].as_array().unwrap_or_else(|| panic!("{BUNDLE} 缺少 files 列表"));
    if files.is_empty() {
        panic!("{BUNDLE} 未列出任何文件");
    }
    let mut names: Vec<String> = files
        .iter()
        .map(|value| value.as_str().unwrap_or_else(|| panic!("{BUNDLE} 含非字符串条目: {value}")))
        .map(|name| {
            if !valid_path(name) {
                panic!("{BUNDLE} 含非法路径: {name}");
            }
            if !root.join(name).is_file() {
                panic!("{BUNDLE} 列出的文件不存在: {name}");
            }
            name.to_owned()
        })
        .collect();
    let mut unique = names.clone();
    unique.sort_unstable();
    unique.dedup();
    if unique.len() != names.len() {
        panic!("{BUNDLE} 含重复路径");
    }
    names
}

fn valid_path(name: &str) -> bool {
    !name.is_empty()
        && !name.contains('\\')
        && !name.starts_with('/')
        && !name.split('/').any(str::is_empty)
        && Path::new(name).components().all(|component| matches!(component, Component::Normal(_)))
}

fn index(target: &Path) {
    let mut names = Vec::new();
    collect(target, target, &mut names);
    names.sort_unstable();
    let mut revision = 0u32;
    for name in &names {
        revision = crc32(revision, name.as_bytes());
        revision = crc32(revision, b"\0");
        let bytes = fs::read(target.join(name)).unwrap_or_else(|error| panic!("读取 {name} 失败: {error}"));
        revision = crc32(revision, &bytes);
    }
    let manifest = serde_json::json!({ "schema": 1, "contentId": format!("{revision:08x}"), "files": names });
    let content = serde_json::to_string_pretty(&manifest).unwrap() + "\n";
    let path = target.join("files.json");
    if fs::read_to_string(&path).ok().as_deref() != Some(content.as_str()) {
        fs::write(&path, content).unwrap_or_else(|error| panic!("写入 files.json 失败: {error}"));
    }
}

fn collect(root: &Path, directory: &Path, names: &mut Vec<String>) {
    let entries = fs::read_dir(directory).unwrap_or_else(|error| panic!("读取 {} 失败: {error}", directory.display()));
    for entry in entries {
        let entry = entry.unwrap_or_else(|error| panic!("读取 {} 失败: {error}", directory.display()));
        let path = entry.path();
        if path.is_dir() {
            collect(root, &path, names);
            continue;
        }
        let name = path.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/");
        if name == "files.json" || name == STAMP {
            continue;
        }
        names.push(name);
    }
}

fn crc32(seed: u32, data: &[u8]) -> u32 {
    static TABLE: OnceLock<[u32; 256]> = OnceLock::new();
    let table = TABLE.get_or_init(|| {
        let mut table = [0u32; 256];
        for (index, slot) in table.iter_mut().enumerate() {
            let mut value = index as u32;
            for _ in 0..8 {
                value = if value & 1 == 0 { value >> 1 } else { 0xedb8_8320 ^ (value >> 1) };
            }
            *slot = value;
        }
        table
    });
    let mut revision = !seed;
    for byte in data {
        revision = table[((revision ^ *byte as u32) & 0xff) as usize] ^ (revision >> 8);
    }
    !revision
}

fn work_dir(root: &Path) -> PathBuf {
    match env::var_os("CARGO_TARGET_DIR").map(PathBuf::from) {
        Some(path) if path.is_absolute() => path,
        Some(path) => root.join(path),
        None => root.join("target"),
    }
}

fn copy_tree(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).unwrap_or_else(|error| panic!("创建 {} 失败: {error}", destination.display()));
    let entries = fs::read_dir(source).unwrap_or_else(|error| panic!("读取 {} 失败: {error}", source.display()));
    for entry in entries {
        let entry = entry.unwrap_or_else(|error| panic!("读取 {} 失败: {error}", source.display()));
        let path = entry.path();
        let target = destination.join(entry.file_name());
        if path.is_dir() {
            copy_tree(&path, &target);
        } else {
            fs::copy(&path, &target).unwrap_or_else(|error| panic!("复制 {} 失败: {error}", path.display()));
        }
    }
}

fn remove(path: &Path) {
    match fs::remove_dir_all(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => panic!("清理 {} 失败: {error}", path.display()),
    }
}
