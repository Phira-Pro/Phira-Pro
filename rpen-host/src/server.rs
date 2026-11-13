use crate::storage::{atomic_write, Store};
use crate::library::{LibraryMailbox, LibraryReply, LibraryRequest};
use anyhow::{ensure, Context, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fs,
    io::{Cursor, Read},
    net::Ipv4Addr,
    path::{Component, Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender},
        Arc, Mutex,
    },
    time::Duration,
};
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};
use uuid::Uuid;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlayRequest {
    pub snapshot: String,
    pub start: f64,
    pub end: Option<f64>,
}
pub enum EditorEvent {
    Play { request: PlayRequest, bytes: Vec<u8> },
    Close,
    Export { name: String, bytes: Vec<u8> },
    Library { id: String, request: LibraryRequest, bytes: Option<Vec<u8>> },
}
struct Shared {
    state: Mutex<&'static str>,
    message: Mutex<Option<String>>,
    stop: AtomicBool,
    library: Mutex<LibraryMailbox>,
    preview: crate::PreviewBridge,
}

pub struct EditorHost {
    pub url: String,
    events: Receiver<EditorEvent>,
    shared: Arc<Shared>,
    worker: Option<std::thread::JoinHandle<()>>,
}
impl EditorHost {
    pub fn start(assets: PathBuf, data: PathBuf) -> Result<Self> {
        ensure!(assets.join("index.html").is_file(), "RPEN 离线资源缺失，请重新安装完整程序包");
        let assets = assets.canonicalize()?;
        fs::create_dir_all(&data)?;
        let store = Store::new(data.join("store"))?;
        let server = Server::http((Ipv4Addr::LOCALHOST, 0)).map_err(|_| anyhow::anyhow!("无法启动本地编辑器服务"))?;
        let address = server.server_addr().to_ip().context("Missing editor service address")?;
        let origin = format!("http://{address}");
        let token = Uuid::new_v4().to_string();
        let prefix = format!("/{token}/");
        let url = format!("{origin}{prefix}index.html");
        let (sender, events) = mpsc::sync_channel(8);
        let shared = Arc::new(Shared {
            state: Mutex::new("visible"),
            message: Mutex::new(None),
            stop: AtomicBool::new(false),
            library: Mutex::new(LibraryMailbox::default()),
            preview: crate::PreviewBridge::default(),
        });
        let thread_shared = shared.clone();
        let worker = std::thread::Builder::new().name("rpen-storage".into()).spawn(move || {
            let mut service = Service {
                assets,
                data,
                store,
                prefix,
                origin,
                shared: thread_shared,
                sender,
                directory_picker: crate::linked::DirectoryPicker::default(),
            };
            while !service.shared.stop.load(Ordering::Relaxed) {
                if let Ok(Some(request)) = server.recv_timeout(Duration::from_millis(100)) {
                    service.handle(request);
                }
            }
        })?;
        Ok(Self {
            url,
            events,
            shared,
            worker: Some(worker),
        })
    }
    pub fn poll(&self) -> Option<EditorEvent> {
        self.events.try_recv().ok()
    }
    pub fn preview(&self) -> crate::PreviewBridge { self.shared.preview.clone() }
    pub fn playing(&self) {
        *self.shared.state.lock().unwrap() = "playing";
    }
    pub fn resume(&self, error: Option<String>) {
        *self.shared.message.lock().unwrap() = error;
        *self.shared.state.lock().unwrap() = "visible";
    }
    pub fn request_close(&self) {
        *self.shared.state.lock().unwrap() = "request-close";
    }
    pub fn complete_library(&self, id: &str, result: std::result::Result<LibraryReply, String>) {
        self.shared.library.lock().unwrap().complete(id, result);
    }
}
impl Drop for EditorHost {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

struct Service {
    assets: PathBuf,
    data: PathBuf,
    store: Store,
    prefix: String,
    origin: String,
    shared: Arc<Shared>,
    sender: SyncSender<EditorEvent>,
    directory_picker: crate::linked::DirectoryPicker,
}
fn header(name: &str, value: &str) -> Header {
    Header::from_bytes(name, value).unwrap()
}
impl Service {
    fn handle(&mut self, mut request: Request) {
        let result = self.route(&mut request);
        let (status, mime, bytes) = match result {
            Ok(response) => response,
            Err(error) => {
                let conflict = error.to_string().contains("ProjectConflictError");
                let linked = error.chain().any(|e| e.to_string().contains("LinkedAssetUnavailable"));
                (400,"application/json",serde_json::to_vec(&json!({"ok":false,"error":{"name":if conflict{"ProjectConflictError"}else if linked{"LinkedAssetUnavailable"}else{"EditorStorageError"},"message":if conflict{"工程已有新版本，请重新打开后保存"}else if linked{"关联的 RPE 素材已移动、被修改或无法读取，请恢复原目录或重新关联后再打开工程"}else{"编辑器请求失败，数据未确认保存"}}})).unwrap())
            }
        };
        let response = Response::from_data(bytes)
            .with_status_code(StatusCode(status))
            .with_header(header("Content-Type", mime))
            .with_header(header("Cache-Control", "no-store"))
            .with_header(header("X-Content-Type-Options", "nosniff"))
            .with_header(header("Referrer-Policy", "no-referrer"));
        let _ = request.respond(response);
    }
    fn route(&mut self, request: &mut Request) -> Result<(u16, &'static str, Vec<u8>)> {
        let uri = request.url().split('?').next().unwrap_or("");
        let Some(route) = uri.strip_prefix(&self.prefix).map(str::to_owned) else {
            return Ok((404, "text/plain", b"Not found".to_vec()));
        };
        let host = request.headers().iter().find(|h| h.field.equiv("Host")).map(|h| h.value.as_str());
        ensure!(host == self.origin.strip_prefix("http://"), "Invalid editor host");
        if request.method() == &Method::Post {
            let origin = request.headers().iter().find(|h| h.field.equiv("Origin")).map(|h| h.value.as_str());
            ensure!(origin == Some(self.origin.as_str()), "Invalid editor origin");
        }
        if route == "pro/control" && request.method() == &Method::Get {
            return json_response(json!({"state":*self.shared.state.lock().unwrap(),"message":self.shared.message.lock().unwrap().take()}));
        }
        if route == "pro/decode-audio" && request.method() == &Method::Post {
            ensure!(request.headers().iter().any(|h| h.field.equiv("Content-Type")
                && h.value.as_str() == "application/octet-stream"), "Invalid audio request");
            let bytes = read_body(request, crate::audio::MAX_INPUT)?;
            let wave = crate::audio::wave(bytes);
            return Ok(match wave {
                Ok(wave) => (200, "audio/wav", wave),
                Err(_) => (422, "text/plain", b"Unsupported or invalid audio".to_vec()),
            });
        }
        if route == "pro/preview" && request.method() == &Method::Post {
            let args: crate::preview::Request = serde_json::from_slice(&read_body(request, 8192)?)?;
            return match args {
                crate::preview::Request::Start { snapshot, width, height } => {
                    Uuid::parse_str(&snapshot)?;
                    let path = self.data.join("snapshots").join(format!("{snapshot}.pez"));
                    let bytes = fs::read(&path)?;
                    validate_snapshot(&bytes)?;
                    let result = self.shared.preview.start(bytes, width, height)?;
                    let _ = fs::remove_file(path);
                    json_response(result)
                }
                crate::preview::Request::Seek { id, time, show_hud } => json_response(self.shared.preview.seek(&id, time, show_hud)?),
                crate::preview::Request::Stop { id } => { self.shared.preview.stop(&id); json_response(json!({"ok":true})) }
            };
        }
        if route == "pro/preview-frame" && request.method() == &Method::Get {
            return Ok((200, "application/octet-stream", self.shared.preview.frame()));
        }
        if route == "pro/storage" && request.method() == &Method::Post {
            let args: Vec<Value> = serde_json::from_slice(&read_body(request, 8 * 1024 * 1024)?)?;
            return json_response(json!({"ok":true,"result":self.store.call(args)?}));
        }
        if route == "pro/linked" && request.method() == &Method::Post {
            let args: Value = serde_json::from_slice(&read_body(request, 8192)?)?;
            let result = self.directory_picker.call(args["action"].as_str().context("Missing directory action")?, args["id"].as_str(), self.store.linked.clone())?;
            return json_response(json!({"ok":true,"result":result}));
        }
        if route == "pro/library" && request.method() == &Method::Post {
            let action: LibraryRequest = serde_json::from_slice(&read_body(request, 8192)?)?;
            let bytes = match &action {
                LibraryRequest::List => None,
                LibraryRequest::Read { key } => { Uuid::parse_str(key)?; None },
                LibraryRequest::Write { snapshot } => {
                    Uuid::parse_str(snapshot)?;
                    let path = self.data.join("snapshots").join(format!("{snapshot}.pez"));
                    let bytes = fs::read(&path)?;
                    validate_snapshot(&bytes)?;
                    let _ = fs::remove_file(path);
                    Some(bytes)
                }
            };
            let id = self.shared.library.lock().unwrap().begin()?;
            if let Err(error) = self.sender.try_send(EditorEvent::Library { id: id.clone(), request: action, bytes }) {
                self.shared.library.lock().unwrap().complete(&id, Err("谱面库忙，请稍后重试".into()));
                return Err(error.into());
            }
            return json_response(json!({"ok":true,"id":id}));
        }
        if let Some(id) = route.strip_prefix("pro/library-status/") {
            ensure!(request.method() == &Method::Post, "Unsupported method");
            Uuid::parse_str(id)?;
            return json_response(json!({"ok":true,"result":self.shared.library.lock().unwrap().status(id)?}));
        }
        if let Some(id) = route.strip_prefix("pro/library-package/") {
            ensure!(request.method() == &Method::Get, "Unsupported method");
            Uuid::parse_str(id)?;
            return Ok((200, "application/zip", self.shared.library.lock().unwrap().take_package(id)?));
        }
        if route == "pro/snapshot" && request.method() == &Method::Post {
            let bytes = read_body(request, 512 * 1024 * 1024)?;
            validate_snapshot(&bytes)?;
            let id = Uuid::new_v4().to_string();
            atomic_write(&self.data.join("snapshots").join(format!("{id}.pez")), &bytes)?;
            return json_response(json!({"ok":true,"id":id}));
        }
        if route == "pro/play" && request.method() == &Method::Post {
            let body: PlayRequest = serde_json::from_slice(&read_body(request, 8192)?)?;
            Uuid::parse_str(&body.snapshot)?;
            ensure!(
                body.start.is_finite() && body.start >= 0. && body.end.is_none_or(|end| end.is_finite() && end > body.start),
                "Invalid practice interval"
            );
            let path = self.data.join("snapshots").join(format!("{}.pez", body.snapshot));
            let bytes = fs::read(&path)?;
            self.sender
                .try_send(EditorEvent::Play { request: body, bytes })
                .context("Editor is busy")?;
            let _ = fs::remove_file(path);
            return json_response(json!({"ok":true}));
        }
        if route == "pro/close" && request.method() == &Method::Post {
            self.sender.try_send(EditorEvent::Close).context("Editor is busy")?;
            return json_response(json!({"ok":true}));
        }
        if route.starts_with("pro/export/") && request.method() == &Method::Post {
            let name = String::from_utf8(URL_SAFE_NO_PAD.decode(route.strip_prefix("pro/export/").unwrap())?)?;
            ensure!(
                !name.is_empty() && name.len() <= 512 && !name.contains(['/', '\\', ':', '\0']) && name != "." && name != "..",
                "Invalid export name"
            );
            let bytes = read_body(request, 512 * 1024 * 1024)?;
            self.sender.try_send(EditorEvent::Export { name, bytes }).context("Editor is busy")?;
            return json_response(json!({"ok":true}));
        }
        ensure!(request.method() == &Method::Get, "Unsupported method");
        let route = resource_path(if route.is_empty() { "index.html" } else { &route })?;
        let path = self.assets.join(route);
        if !path.is_file() {
            return Ok((404, "text/plain", b"Not found".to_vec()));
        }
        let resolved = path.canonicalize()?;
        ensure!(resolved.starts_with(&self.assets), "Invalid editor resource path");
        let mime = match path.extension().and_then(|v| v.to_str()).unwrap_or("") {
            "html" => "text/html; charset=utf-8",
            "mjs" | "js" => "text/javascript; charset=utf-8",
            "css" => "text/css; charset=utf-8",
            "json" => "application/json",
            "png" => "image/png",
            "jpg" | "jpeg" => "image/jpeg",
            "webp" => "image/webp",
            "svg" => "image/svg+xml",
            "gif" => "image/gif",
            "ttf" => "font/ttf",
            "woff2" => "font/woff2",
            "wav" => "audio/wav",
            "mp3" => "audio/mpeg",
            "ogg" => "audio/ogg",
            "wasm" => "application/wasm",
            "md" | "txt" | "glsl" => "text/plain; charset=utf-8",
            _ => "application/octet-stream",
        };
        Ok((200, mime, fs::read(resolved)?))
    }
}
fn resource_path(encoded: &str) -> Result<String> {
    let path = percent_encoding::percent_decode_str(encoded).decode_utf8()?.into_owned();
    ensure!(
        !path.contains(['\\', ':', '\0', '%'])
            && Path::new(&path).components().all(|c| matches!(c, Component::Normal(_))),
        "Invalid editor resource path"
    );
    Ok(path)
}
fn json_response(value: Value) -> Result<(u16, &'static str, Vec<u8>)> {
    Ok((200, "application/json", serde_json::to_vec(&value)?))
}
fn read_body(request: &mut Request, limit: usize) -> Result<Vec<u8>> {
    ensure!(request.body_length().is_none_or(|length| length <= limit), "Editor request too large");
    let mut bytes = vec![];
    request.as_reader().take(limit as u64 + 1).read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= limit, "Editor request too large");
    Ok(bytes)
}
fn validate_snapshot(bytes: &[u8]) -> Result<()> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))?;
    ensure!(archive.len() <= 20000, "Too many snapshot resources");
    let mut total = 0u64;
    let mut info = false;
    for index in 0..archive.len() {
        let entry = archive.by_index(index)?;
        ensure!(entry.enclosed_name().is_some() && !entry.name().contains(['\\', ':']), "Invalid snapshot resource path");
        total = total.checked_add(entry.size()).context("Snapshot size overflow")?;
        ensure!(total <= 1024 * 1024 * 1024 && entry.size() <= 512 * 1024 * 1024, "Snapshot too large");
        info |= entry.name() == "info.txt";
    }
    ensure!(info, "Snapshot has no chart entry");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn encoded_resource_names_preserve_unicode_without_allowing_traversal() {
        assert_eq!(resource_path("assets/BY%E8%99%B9%E4%B9%8B%E6%A2%A6.png").unwrap(), "assets/BY虹之梦.png");
        assert_eq!(resource_path("assets/a%20b%23c.png").unwrap(), "assets/a b#c.png");
        for path in ["../index.html", "%2e%2e/index.html", "assets/%2e%2e/index.html", "%2findex.html", "a%5c..%5cb", "C%3a/x", "a%00b", "%252e%252e/index.html", "%ff"] {
            assert!(resource_path(path).is_err(), "accepted {path}");
        }
    }
    #[test]
    fn rejects_snapshot_traversal_and_missing_entry() {
        let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
        out.start_file("../info.txt", zip::write::SimpleFileOptions::default()).unwrap();
        let bytes = out.finish().unwrap().into_inner();
        assert!(validate_snapshot(&bytes).is_err());
        assert!(validate_snapshot(b"invalid").is_err());
    }
}
