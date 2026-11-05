//! Stages the editor frontend from the pinned RPEN checkout; shared by build scripts.
//! The generated tree stays out of version control: everything in the checkout ships
//! except the development-only paths listed below, so merging upstream needs no list.
//! Local patches live in `scripts/editor/overlay` and apply on top of the checkout;
//! `overlay.lock.json` pins the upstream revisions they were written against.
use serde_json::Value;
use std::{
    env, fs,
    path::{Component, Path, PathBuf},
    process::Command,
    sync::OnceLock,
};

const PIN: &str = "scripts/editor/upstream.json";
const MANIFEST: &str = "phira-pro-exclude.json";
const OVERLAY: &str = "scripts/editor/overlay";
const LOCK: &str = "scripts/editor/overlay.lock.json";
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
    println!("cargo:rerun-if-changed={}", root.join(OVERLAY).display());
    println!("cargo:rerun-if-changed={}", root.join(LOCK).display());

    let Ok(pin) = fs::read_to_string(&pin_path) else {
        println!("cargo:warning=缺少 {PIN}，跳过制谱器前端准备");
        return;
    };
    let overlay = overlay_fingerprint(root);
    let mut staged_now = false;
    if !staged(&target, &pin, &overlay) {
        if env::var_os("PHIRA_SKIP_EDITOR_ASSETS").is_some() {
            println!("cargo:warning=已跳过制谱器前端准备（PHIRA_SKIP_EDITOR_ASSETS）");
            return;
        }
        stage(root, &target, &pin, &overlay);
        staged_now = true;
    }
    // 索引只随生成树变化；否则每次构建脚本重跑都要重新读一遍几百个文件。
    if staged_now || !target.join("files.json").is_file() {
        index(&target);
    }
}

fn staged(target: &Path, pin: &str, overlay: &str) -> bool {
    let Ok(stamp) = fs::read_to_string(target.join(STAMP)) else {
        return false;
    };
    let Ok(stamp) = serde_json::from_str::<Value>(&stamp) else {
        return false;
    };
    stamp["pin"].as_str() == Some(pin)
        && stamp["overlay"].as_str() == Some(overlay)
        && REQUIRED.iter().all(|name| target.join(name).is_file())
}

fn stage(root: &Path, target: &Path, pin: &str, overlay: &str) {
    let upstream: Value = serde_json::from_str(pin).unwrap_or_else(|error| panic!("{PIN} 不是合法 JSON: {error}"));
    let repository = field(&upstream, "repository");
    let revision = field(&upstream, "revision");
    let cache = work_dir(root).join("rpe-next-upstream");
    let staging = work_dir(root).join("rpe-next-staging");
    println!("cargo:warning=正在从 {repository} 拉取制谱器前端 {revision}（首次构建需要联网）");
    checkout(&cache, &repository, &revision).unwrap_or_else(|error| panic!("制谱器前端准备失败: {error}"));
    let rules = exclusions(&cache);
    let names = shipped(&cache, &rules);
    remove(&staging);
    for name in &names {
        let destination = staging.join(name);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent).unwrap_or_else(|error| panic!("创建 {} 失败: {error}", parent.display()));
        }
        fs::copy(cache.join(name), &destination).unwrap_or_else(|error| panic!("复制制谱器资源 {name} 失败: {error}"));
    }
    apply_overlay(root, &cache, &staging);
    let manifest = serde_json::to_string_pretty(&upstream).unwrap() + "\n";
    fs::write(staging.join("UPSTREAM.json"), manifest).unwrap_or_else(|error| panic!("写入 UPSTREAM.json 失败: {error}"));
    remove(target);
    copy_tree(&staging, target);
    remove(&staging);
    let stamp = serde_json::json!({ "pin": pin, "revision": revision, "overlay": overlay });
    let stamp = serde_json::to_string_pretty(&stamp).unwrap() + "\n";
    fs::write(target.join(STAMP), stamp).unwrap_or_else(|error| panic!("写入 {STAMP} 失败: {error}"));
}

/// Copies the local patches onto a staged checkout. Every patch mirrors the path it
/// replaces; files created by `derive` keep the pinned upstream file available under
/// a second name so a patch can wrap it instead of forking it. The lock pins the
/// upstream bytes each patch was written against, so bumping the pin fails loudly.
fn apply_overlay(root: &Path, checkout: &Path, staging: &Path) {
    let source = root.join(OVERLAY);
    if !source.is_dir() {
        return;
    }
    let text = fs::read_to_string(root.join(LOCK)).unwrap_or_else(|error| panic!("读取 {LOCK} 失败: {error}"));
    let lock: Value = serde_json::from_str(&text).unwrap_or_else(|error| panic!("{LOCK} 不是合法 JSON: {error}"));
    if lock["schema"].as_u64() != Some(1) {
        panic!("{LOCK} 的 schema 不受支持");
    }
    let base = lock["base"].as_object().unwrap_or_else(|| panic!("{LOCK} 缺少 base 表"));
    for (name, expected) in base {
        if !valid_path(name) {
            panic!("{LOCK} 含非法路径: {name}");
        }
        let bytes = fs::read(checkout.join(name)).unwrap_or_else(|error| panic!("读取上游 {name} 失败: {error}"));
        let size = expected["bytes"].as_u64();
        let digest = expected["crc32"].as_str();
        if size != Some(bytes.len() as u64) || digest != Some(format!("{:08x}", crc32(0, &bytes)).as_str()) {
            panic!("{name} 与 {LOCK} 记录的上游版本不一致；复核 {OVERLAY} 后更新基线");
        }
    }
    let derive = lock["derive"].as_object().unwrap_or_else(|| panic!("{LOCK} 缺少 derive 表"));
    for (destination, original) in derive {
        let (Some(original), destination) = (original.as_str(), destination.as_str()) else {
            panic!("{LOCK} 的 derive 表只能包含字符串");
        };
        if !valid_path(destination) || !valid_path(original) {
            panic!("{LOCK} 含非法路径: {destination} <- {original}");
        }
        let target = staging.join(destination);
        create_parent(&target);
        fs::copy(staging.join(original), &target)
            .unwrap_or_else(|error| panic!("复制 {original} 为 {destination} 失败: {error}"));
    }
    let mut names = Vec::new();
    collect_files(&source, "", &[], &mut names);
    if names.is_empty() {
        panic!("{OVERLAY} 目录为空");
    }
    names.sort_unstable();
    for name in &names {
        if !valid_path(name) || derive.contains_key(name) {
            panic!("{OVERLAY} 含冲突路径: {name}");
        }
        if checkout.join(name).is_file() && !base.contains_key(name) {
            panic!("{name} 覆盖了未登记基线的上游文件；在 {LOCK} 中补上 base 条目");
        }
        let target = staging.join(name);
        create_parent(&target);
        fs::copy(source.join(name), &target).unwrap_or_else(|error| panic!("应用补丁 {name} 失败: {error}"));
    }
}

fn create_parent(path: &Path) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap_or_else(|error| panic!("创建 {} 失败: {error}", parent.display()));
    }
}

/// Stable across checkouts and Rust versions, unlike `DefaultHasher`.
fn overlay_fingerprint(root: &Path) -> String {
    let directory = root.join(OVERLAY);
    let mut names = Vec::new();
    if directory.is_dir() {
        collect_files(&directory, "", &[], &mut names);
        names.sort_unstable();
    }
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for name in &names {
        hash = fnv1a(hash, name.as_bytes());
        hash = fnv1a(hash, b"\0");
        let bytes = fs::read(directory.join(name)).unwrap_or_else(|error| panic!("读取 {name} 失败: {error}"));
        hash = fnv1a(hash, &(bytes.len() as u64).to_le_bytes());
        hash = fnv1a(hash, &bytes);
    }
    match fs::read(root.join(LOCK)) {
        Ok(lock) => hash = fnv1a(hash, &lock),
        Err(_) => hash = fnv1a(hash, b"no-lock"),
    }
    format!("{hash:016x}")
}

fn fnv1a(mut hash: u64, data: &[u8]) -> u64 {
    for byte in data {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
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

fn shipped(checkout: &Path, rules: &[String]) -> Vec<String> {
    let mut names = Vec::new();
    collect_files(checkout, "", rules, &mut names);
    names.sort_unstable();
    for required in REQUIRED {
        if !names.iter().any(|name| name == required) {
            panic!("上游提交缺少 {required}；检查 {MANIFEST} 的剔除规则");
        }
    }
    names
}

fn collect_files(root: &Path, prefix: &str, rules: &[String], names: &mut Vec<String>) {
    let directory = if prefix.is_empty() { root.to_path_buf() } else { root.join(prefix) };
    let entries = fs::read_dir(&directory).unwrap_or_else(|error| panic!("读取 {} 失败: {error}", directory.display()));
    for entry in entries {
        let entry = entry.unwrap_or_else(|error| panic!("读取 {} 失败: {error}", directory.display()));
        let name = entry
            .file_name()
            .into_string()
            .unwrap_or_else(|name| panic!("文件名不是合法 UTF-8: {name:?}"));
        if prefix.is_empty() && (name == ".git" || name == MANIFEST) {
            continue;
        }
        let relative = if prefix.is_empty() { name } else { format!("{prefix}/{name}") };
        if excluded(&relative, rules) {
            continue;
        }
        if entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false) {
            collect_files(root, &relative, rules, names);
        } else {
            names.push(relative);
        }
    }
}

fn excluded(path: &str, rules: &[String]) -> bool {
    rules
        .iter()
        .any(|rule| path == rule || path.strip_prefix(rule.as_str()).is_some_and(|rest| rest.starts_with('/')))
}

fn exclusions(checkout: &Path) -> Vec<String> {
    let text = fs::read_to_string(checkout.join(MANIFEST)).unwrap_or_else(|error| panic!("上游提交缺少 {MANIFEST}: {error}"));
    let manifest: Value = serde_json::from_str(&text).unwrap_or_else(|error| panic!("{MANIFEST} 不是合法 JSON: {error}"));
    if manifest["schema"].as_u64() != Some(1) {
        panic!("{MANIFEST} 的 schema 不受支持");
    }
    manifest["exclude"]
        .as_array()
        .unwrap_or_else(|| panic!("{MANIFEST} 缺少 exclude 列表"))
        .iter()
        .map(|value| value.as_str().unwrap_or_else(|| panic!("{MANIFEST} 含非字符串条目: {value}")))
        .map(|rule| {
            if !valid_path(rule) {
                panic!("{MANIFEST} 含非法路径: {rule}");
            }
            rule.to_owned()
        })
        .collect()
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
