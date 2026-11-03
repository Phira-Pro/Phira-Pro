//! Read-only references granted by the native RPE directory picker.
use crate::storage::atomic_write;
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{collections::HashMap, fs, io::{Read, Seek, SeekFrom}, path::{Component, Path, PathBuf}, sync::{Arc, Mutex}};

const MAX_FILE: u64 = 512 * 1024 * 1024;
#[derive(Clone, Serialize, Deserialize)]
struct Reference { root: PathBuf, path: String, length: u64 }
struct Selection { references: HashMap<String, Reference>, description: Value }
pub(crate) struct LinkedFiles {
    index: PathBuf,
    references: HashMap<String, Reference>,
    verified: Mutex<HashMap<String, (PathBuf, std::time::SystemTime, u64)>>,
}
impl LinkedFiles {
    pub fn new(index: PathBuf) -> Result<Self> {
        let references = if index.exists() {
            ensure!(fs::metadata(&index)?.len() <= 32 * 1024 * 1024, "Linked index too large");
            serde_json::from_slice(&fs::read(&index)?)?
        } else { HashMap::new() };
        Ok(Self { index, references, verified: Mutex::new(HashMap::new()) })
    }
    #[cfg(test)]
    pub fn register(&mut self, root: &Path) -> Result<Value> { self.install(Self::prepare(root)?) }
    fn prepare(root: &Path) -> Result<Selection> {
        let root = root.canonicalize()?;
        let mut files = Vec::new();
        for item in fs::read_dir(&root)? {
            let item = item?; let path = item.path();
            if item.file_type()?.is_symlink() { continue; }
            let name = item.file_name().to_string_lossy().into_owned();
            if item.file_type()?.is_dir() && name.eq_ignore_ascii_case("resources") {
                Self::scan(&root, &path, &mut files)?;
            } else if item.file_type()?.is_file() && ["settings.json", "settings.txt", "hotkey.txt", "ui.txt", "chartlist.txt"].contains(&name.to_lowercase().as_str()) {
                files.push(path);
            }
        }
        ensure!(files.iter().any(|p| p.strip_prefix(&root).is_ok_and(|p| p.components().next().is_some_and(|c| c.as_os_str().eq_ignore_ascii_case("resources")))), "请选择包含 Resources 的 RPE 主目录");
        let mut next = HashMap::new(); let mut entries = Vec::new();
        for file in files {
            ensure!(entries.len() < 20000, "RPE 目录文件数量过多");
            let path = file.strip_prefix(&root)?.to_string_lossy().replace('\\', "/");
            let length = fs::metadata(&file)?.len();
            ensure!(length <= MAX_FILE, "RPE 素材超过单文件大小限制");
            let hash = file_hash(&file)?;
            next.insert(hash.clone(), Reference { root: root.clone(), path: path.clone(), length });
            entries.push(json!({"path":path,"hash":hash,"length":length}));
        }
        Ok(Selection { references: next, description: json!({"name":root.file_name().unwrap_or_default().to_string_lossy(),"entries":entries}) })
    }
    fn install(&mut self, selection: Selection) -> Result<Value> {
        let mut next = self.references.clone(); next.extend(selection.references);
        let bytes = serde_json::to_vec(&next)?;
        ensure!(bytes.len() <= 32 * 1024 * 1024, "关联目录索引过大");
        atomic_write(&self.index, &bytes)?;
        self.references = next;
        self.verified.lock().unwrap().clear();
        Ok(selection.description)
    }
    fn scan(root: &Path, folder: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
        ensure!(folder.strip_prefix(root)?.components().count() <= 32, "RPE 目录层级过深");
        for item in fs::read_dir(folder)? {
            let item = item?; let kind = item.file_type()?;
            if kind.is_symlink() { continue; }
            let path = item.path();
            ensure!(path.canonicalize()?.starts_with(root), "RPE 资源超出所选目录");
            if kind.is_dir() { Self::scan(root, &path, files)?; }
            else if kind.is_file() { files.push(path); }
            ensure!(files.len() <= 20000, "RPE 目录文件数量过多");
        }
        Ok(())
    }
    fn resolve(&self, key: &str) -> Result<(PathBuf, &Reference)> {
        let reference = self.references.get(key).context("LinkedAssetUnavailable")?;
        ensure!(Path::new(&reference.path).components().all(|c| matches!(c, Component::Normal(_))), "LinkedAssetUnavailable");
        let root = reference.root.canonicalize().context("LinkedAssetUnavailable")?;
        ensure!(root == reference.root, "LinkedAssetUnavailable");
        let path = root.join(&reference.path).canonicalize().context("LinkedAssetUnavailable")?;
        ensure!(path.starts_with(&root) && fs::metadata(&path)?.len() == reference.length, "LinkedAssetUnavailable");
        Ok((path, reference))
    }
    pub fn contains(&self, key: &str) -> bool { self.references.contains_key(key) }
    fn verify(&self, key: &str, path: &Path) -> Result<()> {
        let metadata = fs::metadata(path).context("LinkedAssetUnavailable")?;
        let stamp = (path.to_path_buf(), metadata.modified()?, metadata.len());
        let mut cache = self.verified.lock().unwrap();
        if cache.get(key) != Some(&stamp) {
            ensure!(file_hash(path)? == key, "LinkedAssetUnavailable");
            cache.insert(key.to_owned(), stamp);
        }
        Ok(())
    }
    pub fn matches(&self, key: &str, length: u64) -> bool {
        self.resolve(key).is_ok_and(|(path, r)| {
            // Keep chart descriptions and bindings independent from later RPE edits.
            let name = r.path.to_lowercase();
            let description = name.ends_with(".json") || name.ends_with(".pec") || name.ends_with(".txt");
            !description && r.length == length && self.verify(key, &path).is_ok()
        })
    }
    pub fn read(&self, key: &str, offset: u64, length: u64) -> Result<Vec<u8>> {
        let (path, reference) = self.resolve(key)?;
        ensure!(offset <= reference.length, "LinkedAssetUnavailable");
        self.verify(key, &path)?;
        let mut file = fs::File::open(path)?; file.seek(SeekFrom::Start(offset))?;
        let mut bytes = Vec::new(); file.take(length.min(1024 * 1024)).read_to_end(&mut bytes)?;
        Ok(bytes)
    }
}
fn file_hash(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path)?; let mut hash = Sha256::new(); let mut buffer = [0; 64 * 1024];
    loop { let count = file.read(&mut buffer)?; if count == 0 { break; } hash.update(&buffer[..count]); }
    Ok(format!("{:x}", hash.finalize()))
}

#[derive(Default)]
pub(crate) struct DirectoryPicker { pending: Arc<Mutex<Option<(String, Value)>>> }
impl DirectoryPicker {
    pub fn call(&self, action: &str, id: Option<&str>, files: Arc<Mutex<LinkedFiles>>) -> Result<Value> {
        if action == "available" { return Ok(json!({"available":cfg!(target_os = "windows")})); }
        if action == "status" {
            let mut pending = self.pending.lock().unwrap();
            let (current, result) = pending.as_ref().context("目录选择已结束")?;
            ensure!(Some(current.as_str()) == id, "无效的目录选择请求");
            let result = result.clone(); if result["pending"] != true { pending.take(); }
            return Ok(result);
        }
        ensure!(action == "pick", "Unsupported directory action");
        #[cfg(target_os = "windows")]
        {
            let mut pending = self.pending.lock().unwrap(); ensure!(pending.is_none(), "请先完成当前目录选择");
            let id = uuid::Uuid::new_v4().to_string(); *pending = Some((id.clone(), json!({"pending":true})));
            let slot = self.pending.clone();
            std::thread::spawn(move || {
                let result = match rfd::FileDialog::new().set_title("关联 RPE 主目录").pick_folder() {
                    None => json!({"cancelled":true}),
                    Some(root) => match LinkedFiles::prepare(&root).and_then(|selection| files.lock().unwrap().install(selection)) {
                        Ok(result) => json!({"selection":result}),
                        Err(_) => json!({"error":"无法关联该目录，请选择包含 Resources 的 RPE 主目录，检查读取权限及文件大小"}),
                    },
                };
                if let Some((_, value)) = slot.lock().unwrap().as_mut() { *value = result; }
            });
            return Ok(json!({"id":id}));
        }
        #[cfg(not(target_os = "windows"))]
        { let _ = files; anyhow::bail!("当前平台不支持关联目录") }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_references_survive_restart_and_reject_changed_missing_or_escaping_files() {
        let temp = std::env::temp_dir().join(format!("linked-rpe-{}", uuid::Uuid::new_v4()));
        let root = temp.join("source"); let source = root.join("Resources/chart/song.wav");
        fs::create_dir_all(source.parent().unwrap()).unwrap(); fs::write(&source, b"original music").unwrap();
        let index = temp.join("store/index.json");
        let mut linked = LinkedFiles::new(index.clone()).unwrap();
        let selected = linked.register(&root).unwrap(); let key = selected["entries"][0]["hash"].as_str().unwrap();
        assert_eq!(linked.read(key, 2, 4).unwrap(), b"igin");
        drop(linked); let mut linked = LinkedFiles::new(index).unwrap();
        assert_eq!(linked.read(key, 0, 100).unwrap(), b"original music");
        fs::write(&source, b"changed music!").unwrap();
        assert!(linked.read(key, 0, 100).is_err());
        fs::remove_file(&source).unwrap(); assert!(linked.read(key, 0, 100).is_err());
        fs::write(&source, b"original music").unwrap(); assert!(linked.matches(key, 14));
        linked.references.get_mut(key).unwrap().path = "../outside.wav".into();
        assert!(linked.read(key, 0, 100).is_err());
        fs::remove_dir_all(temp).unwrap();
    }
    #[test]
    fn invalid_root_does_not_grant_access_or_replace_existing_index() {
        let temp = std::env::temp_dir().join(format!("linked-invalid-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&temp).unwrap(); fs::write(temp.join("private.txt"), b"unrelated").unwrap();
        let mut linked = LinkedFiles::new(temp.join("index.json")).unwrap();
        assert!(linked.register(&temp).is_err()); assert!(linked.references.is_empty());
        assert!(!temp.join("index.json").exists());
        fs::remove_dir_all(temp).unwrap();
    }
}
