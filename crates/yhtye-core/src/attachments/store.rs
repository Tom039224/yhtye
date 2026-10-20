//! The attachment files on disk:
//! `<data dir>/attachments/<project>/<chat>/<id>/<file name>`, each with a
//! `<id>.json` beside its directory (the [`AttachmentMeta`] and whether the
//! attachment was sent), written last so a half-staged file is never listed.
//!
//! All operations are blocking file I/O; callers on an async runtime run them
//! with `spawn_blocking`.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{AttachmentMeta, PromptCaps, classify, delivery_for, sanitize_file_name};

/// Directory of the attachments inside the app data directory.
const DIR_NAME: &str = "attachments";

/// Bytes of an attachment id (it is hex, twice as long).
const ID_BYTES: usize = 8;

/// The directory of all attachments for an app data directory.
#[must_use]
pub fn attachments_root(data_dir: &Path) -> PathBuf {
    data_dir.join(DIR_NAME)
}

#[derive(Debug, thiserror::Error)]
pub enum AttachmentError {
    /// No such attachment (never staged, discarded, or its chat was deleted).
    #[error("{0}")]
    NotFound(String),
    /// A name that cannot be a project, chat or attachment id.
    #[error("{0}")]
    Invalid(String),
    /// The attachment was already sent with a message.
    #[error("attachment {0} was already sent")]
    Sent(String),
    #[error("could not access the attachment files: {0}")]
    Io(#[from] std::io::Error),
}

/// What is written beside an attachment's directory.
#[derive(Debug, Serialize, Deserialize)]
struct Sidecar {
    meta: AttachmentMeta,
    /// A message with it was accepted: it is part of the conversation now and
    /// can no longer be discarded.
    #[serde(default)]
    sent: bool,
}

/// The attachment files of one app data directory.
#[derive(Debug, Clone)]
pub struct AttachmentStore {
    root: PathBuf,
}

impl AttachmentStore {
    /// A store under `root` (made absolute: prompts and `file://` URIs name
    /// absolute paths).
    #[must_use]
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        let root = std::path::absolute(&root).unwrap_or(root);
        Self { root }
    }

    /// The store of an app data directory.
    #[must_use]
    pub fn for_data_dir(data_dir: &Path) -> Self {
        Self::new(attachments_root(data_dir))
    }

    /// Saves `bytes` as a new attachment of `chat`. `mime` is the client's
    /// claim; `caps` are what the chat's orchestrator is expected to accept,
    /// which gives the predicted delivery.
    pub fn stage(
        &self,
        project: &str,
        chat: &str,
        name: &str,
        mime: Option<&str>,
        bytes: &[u8],
        caps: PromptCaps,
    ) -> Result<AttachmentMeta, AttachmentError> {
        let chat_dir = self.chat_dir(project, chat)?;
        let name = sanitize_file_name(name);
        let classified = classify(&name, mime, bytes);
        let id = new_id()?;
        let meta = AttachmentMeta {
            id,
            size: bytes.len() as u64,
            kind: classified.kind,
            delivery: delivery_for(classified.kind, caps),
            mime: classified.mime,
            name,
        };
        let dir = chat_dir.join(&meta.id);
        let written = std::fs::create_dir_all(&dir)
            .and_then(|()| std::fs::write(dir.join(&meta.name), bytes))
            .map_err(AttachmentError::from)
            .and_then(|()| {
                write_sidecar(
                    &chat_dir,
                    &Sidecar {
                        meta: meta.clone(),
                        sent: false,
                    },
                )
            });
        if let Err(e) = written {
            let _ = std::fs::remove_dir_all(&dir);
            return Err(e);
        }
        Ok(meta)
    }

    /// The attachment `id` of `chat`.
    pub fn meta(
        &self,
        project: &str,
        chat: &str,
        id: &str,
    ) -> Result<AttachmentMeta, AttachmentError> {
        Ok(self.sidecar(project, chat, id)?.meta)
    }

    /// The attachment `id` of `chat` and its content.
    pub fn read(
        &self,
        project: &str,
        chat: &str,
        id: &str,
    ) -> Result<(AttachmentMeta, Vec<u8>), AttachmentError> {
        let meta = self.meta(project, chat, id)?;
        let bytes = std::fs::read(self.file_path(project, chat, &meta)?)
            .map_err(|e| missing_or_io(e, || format!("attachment {id} of {chat} has no file")))?;
        Ok((meta, bytes))
    }

    /// The absolute path of the file of `meta` (which belongs to `chat`).
    pub fn file_path(
        &self,
        project: &str,
        chat: &str,
        meta: &AttachmentMeta,
    ) -> Result<PathBuf, AttachmentError> {
        check_id(&meta.id)?;
        Ok(self
            .chat_dir(project, chat)?
            .join(&meta.id)
            .join(sanitize_file_name(&meta.name)))
    }

    /// Records whether a message with attachment `id` was accepted; a later
    /// [`AttachmentStore::discard`] refuses one that was.
    pub fn set_sent(
        &self,
        project: &str,
        chat: &str,
        id: &str,
        sent: bool,
    ) -> Result<(), AttachmentError> {
        let mut sidecar = self.sidecar(project, chat, id)?;
        if sidecar.sent != sent {
            sidecar.sent = sent;
            write_sidecar(&self.chat_dir(project, chat)?, &sidecar)?;
        }
        Ok(())
    }

    /// Whether attachment `id` was sent with a message.
    pub fn is_sent(&self, project: &str, chat: &str, id: &str) -> Result<bool, AttachmentError> {
        Ok(self.sidecar(project, chat, id)?.sent)
    }

    /// Deletes a staged attachment that was not sent. `Sent` for one that was.
    pub fn discard(&self, project: &str, chat: &str, id: &str) -> Result<(), AttachmentError> {
        if self.sidecar(project, chat, id)?.sent {
            return Err(AttachmentError::Sent(id.to_string()));
        }
        let chat_dir = self.chat_dir(project, chat)?;
        // The sidecar goes first: without it the attachment no longer exists
        // for anyone, whatever happens to the files.
        remove_file(&sidecar_path(&chat_dir, id))?;
        remove_dir(&chat_dir.join(id))
    }

    /// Deletes every attachment of `chat` (the chat was deleted).
    pub fn remove_chat(&self, project: &str, chat: &str) -> Result<(), AttachmentError> {
        remove_dir(&self.chat_dir(project, chat)?)
    }

    fn sidecar(&self, project: &str, chat: &str, id: &str) -> Result<Sidecar, AttachmentError> {
        check_id(id)?;
        let path = sidecar_path(&self.chat_dir(project, chat)?, id);
        let text = std::fs::read_to_string(&path)
            .map_err(|e| missing_or_io(e, || format!("no attachment {id} in chat {chat}")))?;
        serde_json::from_str(&text).map_err(|e| {
            std::io::Error::new(ErrorKind::InvalidData, format!("{}: {e}", path.display())).into()
        })
    }

    /// `<root>/<project>/<chat>`; `project` and `chat` must be plain names.
    fn chat_dir(&self, project: &str, chat: &str) -> Result<PathBuf, AttachmentError> {
        check_component("project", project)?;
        check_component("chat", chat)?;
        Ok(self.root.join(project).join(chat))
    }
}

fn sidecar_path(chat_dir: &Path, id: &str) -> PathBuf {
    chat_dir.join(format!("{id}.json"))
}

/// Writes the sidecar through a temporary file, so a reader never sees half of it.
fn write_sidecar(chat_dir: &Path, sidecar: &Sidecar) -> Result<(), AttachmentError> {
    let id = &sidecar.meta.id;
    let tmp = chat_dir.join(format!("{id}.json.tmp"));
    let json = serde_json::to_vec(sidecar)
        .map_err(|e| std::io::Error::new(ErrorKind::InvalidData, e.to_string()))?;
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, sidecar_path(chat_dir, id))?;
    Ok(())
}

fn remove_file(path: &Path) -> Result<(), AttachmentError> {
    match std::fs::remove_file(path) {
        Err(e) if e.kind() != ErrorKind::NotFound => Err(e.into()),
        _ => Ok(()),
    }
}

fn remove_dir(path: &Path) -> Result<(), AttachmentError> {
    match std::fs::remove_dir_all(path) {
        Err(e) if e.kind() != ErrorKind::NotFound => Err(e.into()),
        _ => Ok(()),
    }
}

fn missing_or_io(e: std::io::Error, what: impl FnOnce() -> String) -> AttachmentError {
    if e.kind() == ErrorKind::NotFound {
        AttachmentError::NotFound(what())
    } else {
        e.into()
    }
}

/// A new random id (hex).
fn new_id() -> Result<String, AttachmentError> {
    let mut bytes = [0u8; ID_BYTES];
    getrandom::fill(&mut bytes)
        .map_err(|e| std::io::Error::other(format!("no random bytes for an id: {e}")))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// An attachment id is the hex [`new_id`] makes; anything else cannot name a
/// file below the chat's directory.
fn check_id(id: &str) -> Result<(), AttachmentError> {
    let ok = id.len() == ID_BYTES * 2 && id.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
    if ok {
        Ok(())
    } else {
        Err(AttachmentError::Invalid(format!(
            "{id:?} is not an attachment id"
        )))
    }
}

/// A project or chat id as a directory name: letters, digits, `-`, `_` and
/// inner dots, nothing that moves out of the directory.
fn check_component(what: &str, name: &str) -> Result<(), AttachmentError> {
    let ok = !name.is_empty()
        && name.len() <= 128
        && !name.starts_with('.')
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'));
    if ok {
        Ok(())
    } else {
        Err(AttachmentError::Invalid(format!(
            "{name:?} is not a valid {what} id"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attachments::{AttachmentDelivery, AttachmentKind};

    const BOTH: PromptCaps = PromptCaps {
        image: true,
        embedded_context: true,
    };

    fn store() -> (tempfile::TempDir, AttachmentStore) {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = AttachmentStore::for_data_dir(dir.path());
        (dir, store)
    }

    #[test]
    fn a_staged_file_is_saved_under_project_chat_and_id() {
        let (dir, store) = store();
        let meta = store
            .stage("repo", "C-1", "notes.txt", None, b"hello", BOTH)
            .expect("staged");
        assert_eq!(
            (meta.name.as_str(), meta.mime.as_str(), meta.size),
            ("notes.txt", "text/plain", 5)
        );
        assert_eq!(
            (meta.kind, meta.delivery),
            (AttachmentKind::Text, AttachmentDelivery::Native)
        );
        let path = store.file_path("repo", "C-1", &meta).expect("path");
        assert_eq!(
            path,
            std::path::absolute(dir.path())
                .expect("abs")
                .join("attachments/repo/C-1")
                .join(&meta.id)
                .join("notes.txt")
        );
        assert_eq!(std::fs::read(&path).expect("file"), b"hello");
        let (again, bytes) = store.read("repo", "C-1", &meta.id).expect("read");
        assert_eq!((again, bytes.as_slice()), (meta, b"hello".as_slice()));
    }

    #[test]
    fn the_file_name_cannot_leave_the_attachment_directory() {
        let (dir, store) = store();
        let meta = store
            .stage("repo", "C-1", "../../../escape.txt", None, b"x", BOTH)
            .expect("staged");
        assert_eq!(meta.name, "escape.txt");
        let path = store.file_path("repo", "C-1", &meta).expect("path");
        assert!(path.starts_with(dir.path().join("attachments/repo/C-1")));
        assert!(!dir.path().join("escape.txt").exists());
        assert!(
            !dir.path()
                .parent()
                .expect("parent")
                .join("escape.txt")
                .exists()
        );
    }

    #[test]
    fn ids_and_chat_names_cannot_climb_out_of_the_store() {
        let (_dir, store) = store();
        for project in ["", "..", "../x", "a/b", ".hidden", "a\\b"] {
            assert!(
                matches!(
                    store.stage(project, "C-1", "a.txt", None, b"x", BOTH),
                    Err(AttachmentError::Invalid(_))
                ),
                "project {project:?}"
            );
        }
        for chat in ["", "..", "../C-1", "C-1/x"] {
            assert!(
                matches!(
                    store.stage("repo", chat, "a.txt", None, b"x", BOTH),
                    Err(AttachmentError::Invalid(_))
                ),
                "chat {chat:?}"
            );
        }
        for id in [
            "",
            "..",
            "../C-2/abcdef0123456789",
            "ABCDEF0123456789",
            "abc",
        ] {
            assert!(
                matches!(
                    store.meta("repo", "C-1", id),
                    Err(AttachmentError::Invalid(_))
                ),
                "id {id:?}"
            );
        }
    }

    #[test]
    fn an_unknown_attachment_is_not_found() {
        let (_dir, store) = store();
        assert!(matches!(
            store.meta("repo", "C-1", "0123456789abcdef"),
            Err(AttachmentError::NotFound(_))
        ));
        assert!(matches!(
            store.read("repo", "C-1", "0123456789abcdef"),
            Err(AttachmentError::NotFound(_))
        ));
    }

    #[test]
    fn a_staged_attachment_can_be_discarded_but_a_sent_one_cannot() {
        let (_dir, store) = store();
        let a = store
            .stage("repo", "C-1", "a.txt", None, b"a", BOTH)
            .expect("a");
        let b = store
            .stage("repo", "C-1", "b.txt", None, b"b", BOTH)
            .expect("b");
        store.discard("repo", "C-1", &a.id).expect("discarded");
        assert!(matches!(
            store.meta("repo", "C-1", &a.id),
            Err(AttachmentError::NotFound(_))
        ));
        assert!(!store.file_path("repo", "C-1", &a).expect("path").exists());

        assert!(!store.is_sent("repo", "C-1", &b.id).expect("flag"));
        store.set_sent("repo", "C-1", &b.id, true).expect("marked");
        assert!(store.is_sent("repo", "C-1", &b.id).expect("flag"));
        assert!(matches!(
            store.discard("repo", "C-1", &b.id),
            Err(AttachmentError::Sent(_))
        ));
        assert_eq!(store.read("repo", "C-1", &b.id).expect("kept").1, b"b");
    }

    #[test]
    fn attachments_of_other_chats_are_not_reachable() {
        let (_dir, store) = store();
        let a = store
            .stage("repo", "C-1", "a.txt", None, b"a", BOTH)
            .expect("a");
        assert!(matches!(
            store.meta("repo", "C-2", &a.id),
            Err(AttachmentError::NotFound(_))
        ));
        assert!(matches!(
            store.meta("other", "C-1", &a.id),
            Err(AttachmentError::NotFound(_))
        ));
    }

    #[test]
    fn removing_a_chat_removes_its_attachments_only() {
        let (dir, store) = store();
        let a = store
            .stage("repo", "C-1", "a.txt", None, b"a", BOTH)
            .expect("a");
        let b = store
            .stage("repo", "C-2", "b.txt", None, b"b", BOTH)
            .expect("b");
        store.remove_chat("repo", "C-1").expect("removed");
        assert!(matches!(
            store.meta("repo", "C-1", &a.id),
            Err(AttachmentError::NotFound(_))
        ));
        assert!(!dir.path().join("attachments/repo/C-1").exists());
        assert!(store.meta("repo", "C-2", &b.id).is_ok());
        // Removing again is fine.
        store.remove_chat("repo", "C-1").expect("again");
    }

    #[test]
    fn staging_predicts_the_delivery_from_the_capabilities() {
        let (_dir, store) = store();
        let png = b"\x89PNG\r\n\x1a\n....";
        let none = PromptCaps::default();
        let image = store
            .stage("repo", "C-1", "shot.png", Some("image/png"), png, BOTH)
            .expect("image");
        assert_eq!(
            (image.kind, image.delivery),
            (AttachmentKind::Image, AttachmentDelivery::Native)
        );
        let image = store
            .stage("repo", "C-1", "shot.png", Some("image/png"), png, none)
            .expect("image");
        assert_eq!(image.delivery, AttachmentDelivery::Path);
        let pdf = store
            .stage("repo", "C-1", "r.pdf", None, b"%PDF-1.4", BOTH)
            .expect("pdf");
        assert_eq!(
            (pdf.kind, pdf.delivery, pdf.mime.as_str()),
            (
                AttachmentKind::File,
                AttachmentDelivery::Path,
                "application/pdf"
            )
        );
    }
}
