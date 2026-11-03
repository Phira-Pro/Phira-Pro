//! Bounded, asynchronous handoff to the native library adapter.
use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::{Duration, Instant};
use uuid::Uuid;

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "action", rename_all = "kebab-case", deny_unknown_fields)]
pub enum LibraryRequest {
    List,
    Read { key: String },
    Write { snapshot: String },
}

pub enum LibraryReply {
    Json(Value),
    Package(Vec<u8>),
}

struct Operation {
    id: String,
    started: Instant,
    reply: Option<std::result::Result<LibraryReply, String>>,
}

#[derive(Default)]
pub(crate) struct LibraryMailbox(Option<Operation>);
impl LibraryMailbox {
    pub fn begin(&mut self) -> Result<String> {
        ensure!(
            !self
                .0
                .as_ref()
                .is_some_and(|op| op.reply.is_none() && op.started.elapsed() < Duration::from_secs(120)),
            "Library operation is busy"
        );
        let id = Uuid::new_v4().to_string();
        self.0 = Some(Operation {
            id: id.clone(),
            started: Instant::now(),
            reply: None,
        });
        Ok(id)
    }
    pub fn complete(&mut self, id: &str, result: std::result::Result<LibraryReply, String>) {
        if let Some(op) = self.0.as_mut().filter(|op| op.id == id && op.reply.is_none()) {
            op.reply = Some(result);
        }
    }
    pub fn status(&self, id: &str) -> Result<Value> {
        let op = self
            .0
            .as_ref()
            .filter(|op| op.id == id)
            .ok_or_else(|| anyhow::anyhow!("Unknown library operation"))?;
        Ok(match &op.reply {
            None => json!({"state":"pending"}),
            Some(Err(message)) => json!({"state":"failed","message":message}),
            Some(Ok(LibraryReply::Json(value))) => json!({"state":"ready","kind":"json","value":value}),
            Some(Ok(LibraryReply::Package(bytes))) => json!({"state":"ready","kind":"package","bytes":bytes.len()}),
        })
    }
    pub fn take_package(&mut self, id: &str) -> Result<Vec<u8>> {
        ensure!(
            self.0
                .as_ref()
                .is_some_and(|op| op.id == id && matches!(op.reply, Some(Ok(LibraryReply::Package(_))))),
            "Library package is unavailable"
        );
        match self.0.take().unwrap().reply.unwrap().map_err(anyhow::Error::msg)? {
            LibraryReply::Package(bytes) => Ok(bytes),
            LibraryReply::Json(_) => unreachable!(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn handoff_rejects_stale_completion_and_consumes_package_once() {
        let mut mailbox = LibraryMailbox::default();
        let first = mailbox.begin().unwrap();
        assert!(mailbox.begin().is_err());
        mailbox.complete("other", Ok(LibraryReply::Package(vec![9])));
        assert_eq!(mailbox.status(&first).unwrap()["state"], "pending");
        mailbox.complete(&first, Ok(LibraryReply::Package(vec![1, 2, 3])));
        assert_eq!(mailbox.status(&first).unwrap()["bytes"], 3);
        assert!(mailbox.take_package("other").is_err());
        assert_eq!(mailbox.take_package(&first).unwrap(), vec![1, 2, 3]);
        assert!(mailbox.take_package(&first).is_err());
        let second = mailbox.begin().unwrap();
        mailbox.complete(&first, Ok(LibraryReply::Json(json!({"stale":true}))));
        assert_eq!(mailbox.status(&second).unwrap()["state"], "pending");
    }
}
