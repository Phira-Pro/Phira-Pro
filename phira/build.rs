#[path = "../build_support/version.rs"]
mod version;

fn git_stdout(args: &[&str]) -> Option<String> {
    let output = std::process::Command::new("git").args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = std::str::from_utf8(&output.stdout).ok()?.trim().to_string();
    if stdout.is_empty() {
        None
    } else {
        Some(stdout)
    }
}

// A dirty checkout must not advertise the same identifier as an older binary.
// Hash only build inputs; user data, screenshots and signing credentials are excluded.
fn local_build_id(root: &std::path::Path) -> String {
    fn collect(path: &std::path::Path, files: &mut Vec<std::path::PathBuf>) {
        // 开源仓库里 `phira/src/censor` 是指向私有 `phira-inner` 的符号链接；
        // 没有那个仓库时它是断链的，这里要跳过而不是 panic。
        let Ok(meta) = std::fs::metadata(path) else {
            return;
        };
        if meta.is_dir() {
            let Ok(entries) = std::fs::read_dir(path) else {
                return;
            };
            for item in entries.flatten() {
                collect(&item.path(), files);
            }
        } else {
            files.push(path.to_owned());
        }
    }
    let mut files = Vec::new();
    for directory in ["phira/src", "phira/locales", "prpr/src", "phira-android/app/src/main"] {
        println!("cargo:rerun-if-changed={}", root.join(directory).display());
        collect(&root.join(directory), &mut files);
    }
    for file in [
        "version.json",
        "Cargo.toml",
        "Cargo.lock",
        "phira/build.rs",
        "phira/Cargo.toml",
        "prpr/Cargo.toml",
    ] {
        println!("cargo:rerun-if-changed={}", root.join(file).display());
        files.push(root.join(file));
    }
    files.sort();
    let mut hash = 0xcbf29ce484222325u64;
    for file in files {
        let name = file.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/");
        let Ok(bytes) = std::fs::read(&file) else {
            continue;
        };
        for byte in name.bytes().chain([0]).chain(bytes).chain([0]) {
            hash = (hash ^ byte as u64).wrapping_mul(0x100000001b3);
        }
    }
    format!("{:07x}", hash >> 36)
}

fn main() {
    let root = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap())
        .parent()
        .unwrap()
        .to_owned();
    version::Version::read(&root).emit();
    dotenv_build::output(dotenv_build::Config::default()).unwrap();

    let git_dir = git_stdout(&["rev-parse", "--git-dir"]).unwrap_or_else(|| ".git".to_string());
    println!("cargo:rerun-if-changed={}/HEAD", git_dir);
    println!("cargo:rerun-if-changed={}/packed-refs", git_dir);

    if let Some(ref_path) = git_stdout(&["symbolic-ref", "-q", "HEAD"]) {
        println!("cargo:rerun-if-changed={}/{}", git_dir, ref_path);
    }

    let git_hash = if git_stdout(&[
        "status",
        "--porcelain",
        "--",
        "src",
        "locales",
        "build.rs",
        "../prpr",
        "../version.json",
        "../phira-android/app/src/main",
        "../Cargo.toml",
        "../Cargo.lock",
    ])
    .is_some()
    {
        local_build_id(&root)
    } else {
        git_stdout(&["rev-parse", "--short=7", "HEAD"]).unwrap_or_else(|| local_build_id(&root))
    };
    println!("cargo:rustc-env=GIT_HASH={}", git_hash);
}
