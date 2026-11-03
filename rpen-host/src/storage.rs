use anyhow::{bail, ensure, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::{Duration, Instant},
    sync::{Arc, Mutex},
};
use uuid::Uuid;

const KINDS: &[&str] = &[
    "projects",
    "drafts",
    "preferences",
    "mods",
    "editor-skins",
    "note-skins",
    "version-repos",
    "version-commits",
    "version-trees",
    "version-blobs",
];
const MAX_RECORD: usize = 256 * 1024 * 1024;
const MAX_BINARY: u64 = 512 * 1024 * 1024;

pub(crate) fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    fs::create_dir_all(path.parent().context("Missing storage directory")?)?;
    let temporary = path.with_extension(format!("{}.tmp", Uuid::new_v4()));
    let result = (|| {
        let mut file = fs::OpenOptions::new().write(true).create_new(true).open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&temporary, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn str_arg(args: &[Value], index: usize) -> Result<&str> {
    args.get(index).and_then(Value::as_str).context("Missing string argument")
}
fn kind(value: &str) -> Result<&str> {
    ensure!(KINDS.contains(&value), "Unknown editor storage kind");
    Ok(value)
}
fn hash(value: &str) -> Result<&str> {
    ensure!(value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit()), "Invalid blob reference");
    Ok(value)
}

struct Binary {
    metadata: Value,
    bytes: Vec<u8>,
    expected: u64,
    hash: Option<String>,
}
struct Transaction {
    kind: String,
    id: String,
    options: Value,
    data: Value,
    binaries: Vec<Binary>,
    created: Instant,
    weight: usize,
}
pub(crate) struct Store {
    root: PathBuf,
    transactions: HashMap<String, Transaction>,
    pub(crate) linked: Arc<Mutex<crate::linked::LinkedFiles>>,
}

impl Store {
    pub fn new(root: PathBuf) -> Result<Self> {
        fs::create_dir_all(&root)?;
        Ok(Self {
            linked: Arc::new(Mutex::new(crate::linked::LinkedFiles::new(root.join("linked-files.json"))?)),
            root,
            transactions: HashMap::new(),
        })
    }
    fn record_path(&self, k: &str, id: &str) -> Result<PathBuf> {
        ensure!(!id.is_empty() && id.len() <= 2048, "Invalid editor record identifier");
        Ok(self.root.join(kind(k)?).join(format!("{}.json", digest(id.as_bytes()))))
    }
    fn read(&self, k: &str, id: &str) -> Result<Value> {
        let path = self.record_path(k, id)?;
        if !path.exists() {
            return Ok(Value::Null);
        }
        ensure!(fs::metadata(&path)?.len() <= MAX_RECORD as u64, "Editor record is too large");
        let mut value: Value = serde_json::from_slice(&fs::read(path)?)?;
        if k == "projects" && value["data"].is_object() {
            value["data"]["storageRevision"] = value["revision"].clone();
        }
        Ok(value)
    }
    fn list(&self, k: &str, deleted: bool) -> Result<Vec<Value>> {
        let path = self.root.join(kind(k)?);
        if !path.exists() {
            return Ok(vec![]);
        }
        let mut values = vec![];
        for file in fs::read_dir(path)? {
            let path = file?.path();
            if path.extension().is_none_or(|ext| ext != "json") {
                continue;
            }
            ensure!(fs::metadata(&path)?.len() <= MAX_RECORD as u64, "Editor record is too large");
            let mut value: Value = serde_json::from_slice(&fs::read(path)?)?;
            if (value["deleted"] == true) != deleted {
                continue;
            }
            // The library requests summaries; loading all chart bodies here would
            // multiply memory use by the entire library size.
            if k == "projects" || k == "drafts" {
                if let Some(data) = value["data"].as_object_mut() {
                    if let Some(chart) = data.remove("chart") {
                        for field in ["name", "level", "charter", "composer"] {
                            data.insert(field.into(), chart["META"][field].clone());
                        }
                        let lines = chart["judgeLineList"].as_array();
                        data.insert("lines".into(), json!(lines.map_or(0, Vec::len)));
                        data.insert(
                            "notes".into(),
                            json!(lines.map_or(0, |lines| lines.iter().map(|line| line["notes"].as_array().map_or(0, Vec::len)).sum::<usize>())),
                        );
                    }
                    data.remove("assets");
                }
                value["binaries"] = json!([]);
                if k == "projects" {
                    value["data"]["storageRevision"] = value["revision"].clone();
                }
            }
            values.push(value);
        }
        Ok(values)
    }
    pub fn call(&mut self, args: Vec<Value>) -> Result<Value> {
        self.transactions.retain(|_, item| item.created.elapsed() < Duration::from_secs(600));
        let command = str_arg(&args, 0)?;
        match command {
            "legacyMigrationPending" => Ok(json!(false)),
            "migrationComplete" | "markMigrationDone" | "markMigration" => Ok(Value::Null),
            "migrationDone" => Ok(json!(false)),
            "begin" => {
                ensure!(self.transactions.len() < 16, "Too many pending editor saves");
                let k = str_arg(&args, 1)?.to_owned();
                let id = str_arg(&args, 2)?.to_owned();
                self.record_path(&k, &id)?;
                let token = Uuid::new_v4().to_string();
                self.transactions.insert(
                    token.clone(),
                    Transaction {
                        kind: k,
                        id,
                        options: args.get(3).cloned().unwrap_or(json!({})),
                        data: Value::Null,
                        binaries: vec![],
                        created: Instant::now(),
                        weight: 0,
                    },
                );
                Ok(json!(token))
            }
            "parts" => {
                let tx = self.transactions.get_mut(str_arg(&args, 1)?).context("Editor save expired")?;
                let parts = args.get(2).and_then(Value::as_array).context("Invalid editor save parts")?;
                tx.weight += serde_json::to_vec(parts)?.len();
                ensure!(tx.weight <= MAX_RECORD, "Editor save is too large");
                for part in parts {
                    apply_part(&mut tx.data, part)?;
                }
                Ok(Value::Null)
            }
            "binaryBegin" => {
                let tx = self.transactions.get_mut(str_arg(&args, 1)?).context("Editor save expired")?;
                let metadata = args.get(2).context("Missing editor asset metadata")?.clone();
                let expected = metadata["length"].as_u64().context("Invalid editor asset length")?;
                ensure!(expected <= MAX_BINARY && tx.binaries.len() < 20000, "Editor asset limit exceeded");
                ensure!(metadata["path"].is_array(), "Invalid editor asset location");
                let known = metadata["hash"]
                    .as_str()
                    .filter(|h| hash(h).is_ok())
                    .filter(|h| fs::metadata(self.root.join("blobs").join(h)).is_ok_and(|m| m.len() == expected)
                        || self.linked.lock().unwrap().matches(h, expected))
                    .map(str::to_owned);
                let existing = known.is_some();
                let index = tx.binaries.len();
                tx.binaries.push(Binary {
                    metadata,
                    expected,
                    bytes: vec![],
                    hash: known.clone(),
                });
                Ok(json!({"index":index,"existing":existing,"hash":known}))
            }
            "binaryChunk" => {
                let tx = self.transactions.get_mut(str_arg(&args, 1)?).context("Editor save expired")?;
                let index = args.get(2).and_then(Value::as_u64).context("Invalid asset index")? as usize;
                let entry = tx.binaries.get_mut(index).context("Unknown editor asset")?;
                let offset = args.get(3).and_then(Value::as_u64).context("Invalid asset offset")?;
                ensure!(entry.hash.is_none() && offset == entry.bytes.len() as u64, "Out of order asset chunk");
                let bytes = STANDARD.decode(args.get(4).and_then(Value::as_str).context("Invalid asset bytes")?)?;
                ensure!(bytes.len() <= 1024 * 1024 && offset + bytes.len() as u64 <= entry.expected, "Editor asset chunk exceeds declared size");
                entry.bytes.extend(bytes);
                if entry.bytes.len() as u64 == entry.expected {
                    let key = digest(&entry.bytes);
                    let path = self.root.join("blobs").join(&key);
                    if !path.exists() && !self.linked.lock().unwrap().matches(&key, entry.expected) {
                        atomic_write(&path, &entry.bytes)?;
                    }
                    entry.hash = Some(key);
                    entry.bytes.clear();
                }
                Ok(json!({"hash":entry.hash}))
            }
            "commit" => {
                let token = str_arg(&args, 1)?;
                let tx = self.transactions.remove(token).context("Editor save expired")?;
                let previous = self.read(&tx.kind, &tx.id)?;
                let actual = previous["revision"].clone();
                if tx.kind == "projects" {
                    ensure!(tx.options.get("expectedRevision").unwrap_or(&Value::Null) == &actual, "ProjectConflictError");
                }
                let mut binaries = vec![];
                for entry in tx.binaries {
                    let key = match entry.hash {
                        Some(key) => key,
                        None if entry.expected == 0 => {
                            let key = digest(&[]);
                            let path = self.root.join("blobs").join(&key);
                            if !path.exists() {
                                atomic_write(&path, &[])?;
                            }
                            key
                        }
                        _ => bail!("Incomplete editor asset"),
                    };
                    ensure!(self.root.join("blobs").join(&key).is_file() || self.linked.lock().unwrap().matches(&key, entry.expected), "LinkedAssetUnavailable");
                    let mut metadata = entry.metadata;
                    metadata["hash"] = json!(key);
                    binaries.push(metadata);
                }
                let revision = Uuid::new_v4().to_string();
                let envelope = json!({"data":tx.data,"binaries":binaries,"revision":revision,"updated":std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_millis() as u64});
                atomic_write(&self.record_path(&tx.kind, &tx.id)?, &serde_json::to_vec(&envelope)?)?;
                // Retention affects recovery snapshots only, never projects or mods.
                if tx.kind == "drafts" {
                    self.prune_drafts(envelope["data"]["projectId"].as_str(), tx.options["limit"].as_u64().unwrap_or(10).clamp(1, 100) as usize)?;
                }
                Ok(json!({"revision":revision}))
            }
            "abort" => {
                self.transactions.remove(str_arg(&args, 1)?);
                Ok(Value::Null)
            }
            "read" => {
                let value = self.read(str_arg(&args, 1)?, str_arg(&args, 2)?)?;
                Ok(if value["deleted"] == true { Value::Null } else { value })
            }
            "list" => Ok(json!(self.list(str_arg(&args, 1)?, args.get(2) == Some(&json!(true)))?)),
            "listDrafts" => {
                let project = args.get(1).and_then(Value::as_str);
                let mut drafts = self
                    .list("drafts", false)?
                    .into_iter()
                    .filter(|v| project.is_none_or(|p| v["data"]["projectId"] == p))
                    .map(|v| {
                        let mut draft = v["data"].clone();
                        draft["title"] = draft["name"].clone();
                        draft
                    })
                    .collect::<Vec<_>>();
                drafts.sort_by_key(|v| std::cmp::Reverse(v["updated"].as_u64().unwrap_or(0)));
                Ok(json!(drafts))
            }
            "readRevision" => {
                let value = self.read("projects", str_arg(&args, 1)?)?;
                Ok(if value.is_null() {
                    Value::Null
                } else {
                    json!({"id":str_arg(&args,1)?,"revision":value["revision"],"deleted":value["deleted"]==true})
                })
            }
            "remove" => {
                let k = str_arg(&args, 1)?;
                let id = str_arg(&args, 2)?;
                let mut value = self.read(k, id)?;
                if let Some(expected) = args.get(3) {
                    ensure!(expected == &value["revision"], "ProjectConflictError");
                }
                value["deleted"] = json!(true);
                value["revision"] = json!(Uuid::new_v4().to_string());
                atomic_write(&self.record_path(k, id)?, &serde_json::to_vec(&value)?)?;
                Ok(Value::Null)
            }
            "blob" => {
                use std::io::{Read, Seek, SeekFrom};
                let key = hash(str_arg(&args, 1)?)?;
                let offset = args.get(2).and_then(Value::as_u64).context("Invalid blob offset")?;
                let length = args.get(3).and_then(Value::as_u64).context("Invalid blob length")?.min(1024 * 1024);
                let path = self.root.join("blobs").join(key);
                if !path.exists() && self.linked.lock().unwrap().contains(key) {
                    let bytes = self.linked.lock().unwrap().read(key, offset, length).context("LinkedAssetUnavailable")?;
                    return Ok(json!({"base64":STANDARD.encode(bytes)}));
                }
                let mut file = fs::File::open(path)?;
                file.seek(SeekFrom::Start(offset))?;
                let mut bytes = vec![];
                file.take(length).read_to_end(&mut bytes)?;
                Ok(json!({"base64":STANDARD.encode(bytes)}))
            }
            "setLocalPreference" => {
                let key = str_arg(&args, 1)?;
                ensure!(key.starts_with("rpe") && key.len() <= 1024, "Unknown preference key");
                let mut value = self.read("preferences", "ui-settings")?;
                if value.is_null() {
                    value = json!({"data":{"id":"ui-settings","values":{}},"binaries":[]});
                }
                if args.get(2).is_none_or(Value::is_null) {
                    value["data"]["values"].as_object_mut().context("Invalid preferences")?.remove(key);
                } else {
                    value["data"]["values"][key] = args[2].clone();
                }
                let bytes = serde_json::to_vec(&value)?;
                ensure!(bytes.len() <= 8 * 1024 * 1024, "Editor preferences too large");
                atomic_write(&self.record_path("preferences", "ui-settings")?, &bytes)?;
                Ok(Value::Null)
            }
            "status" => {
                let mut usage = 0u64;
                let mut blobs = 0;
                for k in KINDS.iter().copied().chain(std::iter::once("blobs")) {
                    if let Ok(entries) = fs::read_dir(self.root.join(k)) {
                        for entry in entries {
                            let metadata = entry?.metadata()?;
                            if metadata.is_file() {
                                usage = usage.saturating_add(metadata.len());
                                if k == "blobs" {
                                    blobs += 1;
                                }
                            }
                        }
                    }
                }
                Ok(
                    json!({"native":true,"root":"应用内编辑器目录","persistent":true,"usage":usage,"free":null,"blobs":blobs,"projects":self.list("projects",false)?.len(),"trash":self.list("projects",true)?.len(),"warnings":[]}),
                )
            }
            "restore" | "purge" => {
                let id = str_arg(&args, 1)?;
                let mut value = self.read("projects", id)?;
                ensure!(value["deleted"] == true && args.get(2) == Some(&value["revision"]), "ProjectConflictError");
                if command == "purge" {
                    fs::remove_file(self.record_path("projects", id)?)?;
                } else {
                    value["deleted"] = json!(false);
                    value["revision"] = json!(Uuid::new_v4().to_string());
                    atomic_write(&self.record_path("projects", id)?, &serde_json::to_vec(&value)?)?;
                }
                Ok(Value::Null)
            }
            "rebuild" => {
                for k in KINDS {
                    self.list(k, false)?;
                    self.list(k, true)?;
                }
                Ok(Value::Null)
            }
            "collect" => {
                // Keep all committed assets, including the recycle bin. Only
                // unreferenced blobs from interrupted writes are eligible.
                let mut referenced = std::collections::HashSet::new();
                for k in KINDS {
                    for deleted in [false, true] {
                        let path = self.root.join(k);
                        if !path.exists() {
                            continue;
                        }
                        for entry in fs::read_dir(path)? {
                            let path = entry?.path();
                            if path.extension().is_none_or(|v| v != "json") {
                                continue;
                            }
                            let value: Value = serde_json::from_slice(&fs::read(path)?)?;
                            if (value["deleted"] == true) != deleted {
                                continue;
                            }
                            if let Some(entries) = value["binaries"].as_array() {
                                for binary in entries {
                                    if let Some(key) = binary["hash"].as_str() {
                                        referenced.insert(key.to_owned());
                                    }
                                }
                            }
                        }
                    }
                }
                for tx in self.transactions.values() {
                    for binary in &tx.binaries {
                        if let Some(key) = &binary.hash {
                            referenced.insert(key.clone());
                        }
                    }
                }
                let mut removed = 0;
                let mut bytes = 0;
                if let Ok(entries) = fs::read_dir(self.root.join("blobs")) {
                    for entry in entries {
                        let entry = entry?;
                        let key = entry.file_name().to_string_lossy().into_owned();
                        if hash(&key).is_ok() && !referenced.contains(&key) {
                            let size = entry.metadata()?.len();
                            fs::remove_file(entry.path())?;
                            bytes += size;
                            removed += 1;
                        }
                    }
                }
                Ok(json!({"removed":removed,"bytes":bytes}))
            }
            _ => bail!("Unsupported editor storage operation"),
        }
    }
    fn prune_drafts(&self, project: Option<&str>, limit: usize) -> Result<()> {
        let mut items = self
            .list("drafts", false)?
            .into_iter()
            .filter(|v| v["data"]["projectId"].as_str() == project)
            .collect::<Vec<_>>();
        items.sort_by_key(|v| std::cmp::Reverse(v["updated"].as_u64().unwrap_or(0)));
        for item in items.into_iter().skip(limit) {
            if let Some(id) = item["data"]["id"].as_str() {
                fs::remove_file(self.record_path("drafts", id)?)?;
            }
        }
        Ok(())
    }
}

fn apply_part(root: &mut Value, part: &Value) -> Result<()> {
    let path = part["path"].as_array().context("Invalid snapshot path")?;
    ensure!(path.len() <= 128, "Snapshot nesting limit exceeded");
    let mut node = root;
    for key in path {
        node = if let Some(index) = key.as_u64() {
            node.as_array_mut()
                .and_then(|a| a.get_mut(index as usize))
                .context("Invalid snapshot index")?
        } else {
            let key = key.as_str().context("Invalid snapshot key")?;
            ensure!(!["__proto__", "prototype", "constructor"].contains(&key), "Invalid snapshot key");
            node.as_object_mut().context("Invalid snapshot parent")?.entry(key).or_insert(Value::Null)
        };
    }
    if let Some(offset) = part["offset"].as_u64() {
        let values = part["values"].as_array().context("Invalid array part")?;
        let target = node.as_array_mut().context("Missing snapshot array")?;
        let start = offset as usize;
        ensure!(start <= target.len() && values.len() <= target.len() - start, "Snapshot array exceeds bounds");
        target[start..start + values.len()].clone_from_slice(values);
    } else if let Some(length) = part["length"].as_u64() {
        ensure!(length <= 2_000_000, "Snapshot array too large");
        *node = Value::Array(vec![Value::Null; length as usize]);
    } else {
        *node = part.get("value").context("Missing snapshot value")?.clone();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn linked_chart_descriptions_are_saved_locally_before_original_rpe_changes() {
        let temp = std::env::temp_dir().join(format!("editor-linked-description-{}", Uuid::new_v4()));
        let source = temp.join("rpe/Resources/example/chart.json");
        fs::create_dir_all(source.parent().unwrap()).unwrap(); fs::write(&source, b"{}").unwrap();
        let mut store = Store::new(temp.join("store")).unwrap();
        let selection = store.linked.lock().unwrap().register(&temp.join("rpe")).unwrap();
        let hash = selection["entries"][0]["hash"].clone();
        assert!(!store.linked.lock().unwrap().matches(hash.as_str().unwrap(), 2));
        let token = store.call(vec![json!("begin"),json!("projects"),json!("description"),json!({"expectedRevision":null})]).unwrap();
        store.call(vec![json!("parts"),token.clone(),json!([{"path":[],"value":{"id":"description","asset":null}}])]).unwrap();
        let entry = store.call(vec![json!("binaryBegin"),token.clone(),json!({"path":["asset"],"length":2,"kind":"bytes","hash":hash})]).unwrap();
        assert_eq!(entry["existing"], false);
        store.call(vec![json!("binaryChunk"),token.clone(),json!(0),json!(0),json!(STANDARD.encode(b"{}"))]).unwrap();
        store.call(vec![json!("commit"),token]).unwrap();
        fs::write(&source, b"{\"editedInRpe\":true}").unwrap();
        let bytes = store.call(vec![json!("blob"),hash,json!(0),json!(2)]).unwrap();
        assert_eq!(bytes["base64"], STANDARD.encode(b"{}"));
        fs::remove_dir_all(temp).unwrap();
    }
    #[test]
    fn linked_assets_are_not_copied_by_project_draft_history_or_collection() {
        let temp = std::env::temp_dir().join(format!("editor-linked-{}", Uuid::new_v4()));
        let source = temp.join("rpe/Resources/example/music.wav");
        fs::create_dir_all(source.parent().unwrap()).unwrap(); fs::write(&source, [1, 2, 3]).unwrap();
        let root = temp.join("store"); let mut store = Store::new(root.clone()).unwrap();
        let selection = store.linked.lock().unwrap().register(&temp.join("rpe")).unwrap();
        let hash = selection["entries"][0]["hash"].clone();
        for kind in ["projects", "drafts", "version-blobs"] {
            let token = store.call(vec![json!("begin"),json!(kind),json!("linked"),json!({"expectedRevision":null})]).unwrap();
            store.call(vec![json!("parts"),token.clone(),json!([{"path":[],"value":{"id":"linked","chart":{"META":{"name":"linked"}},"asset":null}}])]).unwrap();
            // Cloned arrays without a cached renderer hash also remain references.
            let entry = store.call(vec![json!("binaryBegin"),token.clone(),json!({"path":["asset"],"length":3,"kind":"bytes"})]).unwrap();
            assert_eq!(entry["existing"], false);
            store.call(vec![json!("binaryChunk"),token.clone(),json!(0),json!(0),json!(STANDARD.encode([1,2,3]))]).unwrap();
            store.call(vec![json!("commit"),token]).unwrap();
        }
        assert!(!root.join("blobs").exists()); drop(store);
        let mut store = Store::new(root.clone()).unwrap();
        let chunk = store.call(vec![json!("blob"),hash.clone(),json!(0),json!(3)]).unwrap();
        assert_eq!(chunk["base64"], STANDARD.encode([1,2,3]));
        store.call(vec![json!("collect")]).unwrap();
        assert_eq!(fs::read(&source).unwrap(), [1,2,3]);
        let token = store.call(vec![json!("begin"),json!("drafts"),json!("modified")]).unwrap();
        store.call(vec![json!("parts"),token.clone(),json!([{"path":[],"value":{"id":"modified","asset":null}}])]).unwrap();
        let changed = store.call(vec![json!("binaryBegin"),token.clone(),json!({"path":["asset"],"length":3,"kind":"bytes","hash":hash})]).unwrap();
        assert_eq!(changed["existing"], true);
        // Changed bytes have a new digest and must use owned storage.
        let token2 = store.call(vec![json!("begin"),json!("drafts"),json!("new-media")]).unwrap();
        store.call(vec![json!("parts"),token2.clone(),json!([{"path":[],"value":{"id":"new-media","asset":null}}])]).unwrap();
        store.call(vec![json!("binaryBegin"),token2.clone(),json!({"path":["asset"],"length":3,"kind":"bytes"})]).unwrap();
        let uploaded = store.call(vec![json!("binaryChunk"),token2.clone(),json!(0),json!(0),json!(STANDARD.encode([4,5,6]))]).unwrap();
        store.call(vec![json!("commit"),token2]).unwrap();
        assert!(root.join("blobs").join(uploaded["hash"].as_str().unwrap()).is_file());
        assert_eq!(fs::read(&source).unwrap(), [1,2,3]);
        fs::remove_dir_all(temp).unwrap();
    }
    #[test]
    fn multipart_save_reopen_conflict_and_asset_roundtrip() {
        let root = std::env::temp_dir().join(format!("editor-store-{}", Uuid::new_v4()));
        let mut store = Store::new(root.clone()).unwrap();
        let token = store
            .call(vec![json!("begin"), json!("projects"), json!("../safe-id"), json!({"expectedRevision":null})])
            .unwrap();
        store.call(vec![json!("parts"),token.clone(),json!([{"path":[],"value":{"id":"../safe-id","chart":{"META":{"name":"test"}},"assets":null}},{"path":["assets"],"length":1},{"path":["assets"],"offset":0,"values":[["song.ogg",null]]}])]).unwrap();
        store
            .call(vec![
                json!("binaryBegin"),
                token.clone(),
                json!({"path":["assets",0,1],"length":3,"kind":"bytes"}),
            ])
            .unwrap();
        store
            .call(vec![json!("binaryChunk"), token.clone(), json!(0), json!(0), json!(STANDARD.encode([1, 2, 3]))])
            .unwrap();
        let revision = store.call(vec![json!("commit"), token]).unwrap()["revision"].clone();
        drop(store);
        let mut store = Store::new(root.clone()).unwrap();
        let record = store.call(vec![json!("read"), json!("projects"), json!("../safe-id")]).unwrap();
        assert_eq!(record["revision"], revision);
        let bytes = store
            .call(vec![json!("blob"), record["binaries"][0]["hash"].clone(), json!(0), json!(3)])
            .unwrap();
        assert_eq!(STANDARD.decode(bytes["base64"].as_str().unwrap()).unwrap(), [1, 2, 3]);
        let token = store
            .call(vec![json!("begin"), json!("projects"), json!("../safe-id"), json!({"expectedRevision":null})])
            .unwrap();
        assert!(store
            .call(vec![json!("commit"), token])
            .unwrap_err()
            .to_string()
            .contains("ProjectConflictError"));
        assert!(!root.parent().unwrap().join("safe-id.json").exists());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn draft_summaries_match_recovery_contract_and_newest_first() {
        let root = std::env::temp_dir().join(format!("editor-drafts-{}", Uuid::new_v4()));
        let mut store = Store::new(root.clone()).unwrap();
        for (id, project, updated) in [("old", "one", 1), ("new", "one", 3), ("other", "two", 2)] {
            let value = json!({"data":{"id":id,"projectId":project,"updated":updated,"name":"chart.json","chart":{"META":{"name":id}},"assets":[]},"binaries":[],"revision":"test"});
            atomic_write(&store.record_path("drafts", id).unwrap(), &serde_json::to_vec(&value).unwrap()).unwrap();
        }
        let drafts = store.call(vec![json!("listDrafts"), json!("one")]).unwrap();
        assert_eq!(drafts[0]["title"], "new");
        assert_eq!(drafts[1]["title"], "old");
        assert_eq!(drafts.as_array().unwrap().len(), 2);
        assert!(drafts[0].get("chart").is_none());
        let all = store.call(vec![json!("listDrafts"), Value::Null]).unwrap();
        assert_eq!(all[1]["title"], "other");
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn snapshot_rejects_invalid_indices_and_prototype_keys() {
        let mut root = json!({"a":[null]});
        assert!(apply_part(&mut root, &json!({"path":["a"],"offset":2,"values":[1]})).is_err());
        assert!(apply_part(&mut root, &json!({"path":["__proto__"],"value":1})).is_err());
    }
}
