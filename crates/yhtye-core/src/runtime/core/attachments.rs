//! The attachment commands of [`Core`] (`core-design.md` §20): saving a file the
//! user wants to attach, discarding it, reading it back, and resolving the
//! staged attachments of a message. The files are handled outside the project's
//! loop (a project need not be open to stage one); the loop only reads them
//! when the prompt is sent.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;

use super::Core;
use crate::agents::AgentRole;
use crate::api::{ApiError, ApiErrorCode, ApiResponse};
use crate::attachments::{AttachmentError, AttachmentMeta, PromptCaps};
use crate::domain::orchestrator_session;

impl Core {
    /// What the orchestrator of `chat` is expected to accept in a prompt: the
    /// last capabilities its harness reported, else the harness's default.
    /// The harness is the one the chat's stored orchestrator session ran, else
    /// the one the orchestrator role would start now.
    pub(super) async fn predicted_prompt_caps(&self, project: &str, chat: &str) -> PromptCaps {
        let agents = &self.inner.agents;
        let key = orchestrator_session(chat);
        let stored = match self.inner.store.sessions(project).await {
            Ok(all) => all
                .into_iter()
                .find(|s| s.session_key == key)
                .and_then(|s| s.agent)
                .map(|a| a.harness)
                .filter(|h| agents.preset(h).is_some()),
            Err(e) => {
                tracing::warn!("could not read the sessions of {project}: {e}");
                None
            }
        };
        let harness = stored.or_else(|| {
            agents
                .resolve(project, AgentRole::Orchestrator, None)
                .ok()
                .map(|r| r.choice.harness)
        });
        let Some(harness) = harness else {
            return PromptCaps::default();
        };
        let observed = match self.inner.store.harness_prompt_caps().await {
            Ok(mut all) => all.remove(&harness),
            Err(e) => {
                tracing::warn!("could not read the prompt capabilities: {e}");
                None
            }
        };
        observed
            .or_else(|| agents.preset(&harness).map(|p| p.prompt_caps))
            .unwrap_or_default()
    }

    /// `stage_attachment`: decodes and saves the file.
    pub(super) async fn stage_attachment(
        &self,
        project: &str,
        chat: &str,
        name: String,
        mime: Option<String>,
        data_base64: String,
    ) -> Result<AttachmentMeta, ApiError> {
        self.known_chat(project, chat).await?;
        let caps = self.predicted_prompt_caps(project, chat).await;
        let store = self.inner.attachments.clone();
        let (project, chat) = (project.to_string(), chat.to_string());
        blocking(move || {
            let bytes = decode(&data_base64)?;
            drop(data_base64);
            store
                .stage(&project, &chat, &name, mime.as_deref(), &bytes, caps)
                .map_err(attachment_error)
        })
        .await
    }

    /// `discard_attachment`.
    pub(super) async fn discard_attachment(
        &self,
        project: &str,
        chat: &str,
        id: &str,
    ) -> Result<(), ApiError> {
        self.known_project(project).await?;
        let store = self.inner.attachments.clone();
        let (project, chat, id) = (project.to_string(), chat.to_string(), id.to_string());
        blocking(move || match store.discard(&project, &chat, &id) {
            Err(AttachmentError::Sent(id)) => Err(ApiError::new(
                ApiErrorCode::InvalidState,
                format!(
                    "attachment {id} was sent with a message; it stays part of the conversation"
                ),
            )),
            other => other.map_err(attachment_error),
        })
        .await
    }

    /// `get_attachment`: the metadata and the content, base64.
    pub(super) async fn get_attachment(
        &self,
        project: &str,
        chat: &str,
        id: &str,
    ) -> Result<ApiResponse, ApiError> {
        self.known_project(project).await?;
        let store = self.inner.attachments.clone();
        let (project, chat, id) = (project.to_string(), chat.to_string(), id.to_string());
        blocking(move || {
            let (attachment, bytes) = store.read(&project, &chat, &id).map_err(attachment_error)?;
            Ok(ApiResponse::Attachment {
                attachment,
                data_base64: STANDARD.encode(bytes),
            })
        })
        .await
    }

    /// The staged attachments `ids` of `chat` for a message, in order, marked
    /// as sent (so they can no longer be discarded; undo with
    /// [`Core::release_attachments`] if the message is refused). `not_found`
    /// for one that does not exist, `invalid_argument` for one sent before or
    /// named twice.
    pub(super) async fn take_attachments(
        &self,
        project: &str,
        chat: &str,
        ids: &[String],
    ) -> Result<Vec<AttachmentMeta>, ApiError> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let store = self.inner.attachments.clone();
        let (project, chat, ids) = (project.to_string(), chat.to_string(), ids.to_vec());
        blocking(move || {
            let mut metas = Vec::with_capacity(ids.len());
            for (at, id) in ids.iter().enumerate() {
                if ids[..at].contains(id) {
                    return Err(ApiError::invalid_argument(format!(
                        "attachment {id} is named twice"
                    )));
                }
                if store
                    .is_sent(&project, &chat, id)
                    .map_err(attachment_error)?
                {
                    return Err(ApiError::invalid_argument(format!(
                        "attachment {id} was already sent; attach the file again"
                    )));
                }
                metas.push(store.meta(&project, &chat, id).map_err(attachment_error)?);
            }
            for meta in &metas {
                store
                    .set_sent(&project, &chat, &meta.id, true)
                    .map_err(attachment_error)?;
            }
            Ok(metas)
        })
        .await
    }

    /// Takes back [`Core::take_attachments`] for a message that was refused.
    pub(super) async fn release_attachments(
        &self,
        project: &str,
        chat: &str,
        attachments: &[AttachmentMeta],
    ) {
        if attachments.is_empty() {
            return;
        }
        let store = self.inner.attachments.clone();
        let (project, chat) = (project.to_string(), chat.to_string());
        let ids: Vec<String> = attachments.iter().map(|a| a.id.clone()).collect();
        let _ = blocking(move || {
            for id in &ids {
                if let Err(e) = store.set_sent(&project, &chat, id, false) {
                    tracing::warn!("could not take back the sending of attachment {id}: {e}");
                }
            }
            Ok(())
        })
        .await;
    }

    /// `chat` of a registered `project`, as the store lists it.
    async fn known_chat(&self, project: &str, chat: &str) -> Result<(), ApiError> {
        self.known_project(project).await?;
        let chats = self.inner.store.chats(project).await?;
        if chats.iter().any(|c| c.id == chat) {
            Ok(())
        } else {
            Err(ApiError::not_found(format!("no chat {chat}")))
        }
    }
}

/// Runs blocking file work off the async runtime.
async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, ApiError> + Send + 'static,
) -> Result<T, ApiError> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|e| ApiError::internal(format!("attachment task failed: {e}")))?
}

/// Standard base64; line breaks and spaces (a wrapped encoding) are tolerated.
fn decode(data: &str) -> Result<Vec<u8>, ApiError> {
    let decoded = match STANDARD.decode(data) {
        Err(_) if data.contains(|c: char| c.is_ascii_whitespace()) => {
            let compact: String = data.split_ascii_whitespace().collect();
            STANDARD.decode(compact)
        }
        other => other,
    };
    decoded.map_err(|e| ApiError::invalid_argument(format!("data_base64 is not valid base64: {e}")))
}

fn attachment_error(e: AttachmentError) -> ApiError {
    match e {
        AttachmentError::NotFound(m) => ApiError::not_found(m),
        AttachmentError::Invalid(m) => ApiError::invalid_argument(m),
        AttachmentError::Sent(_) => ApiError::new(ApiErrorCode::InvalidState, e.to_string()),
        AttachmentError::Io(_) => ApiError::internal(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_is_read_strictly_but_a_wrapped_encoding_is_accepted() {
        assert_eq!(decode("aGVsbG8=").expect("plain"), b"hello");
        assert_eq!(decode("aGVs\nbG8=\n").expect("wrapped"), b"hello");
        assert_eq!(decode("aGVs bG8=").expect("spaced"), b"hello");
        assert_eq!(decode("").expect("empty"), b"");
        for bad in ["a", "aGVsbG8", "****", "aGVsbG8=="] {
            let e = decode(bad).expect_err(bad);
            assert_eq!(e.code, ApiErrorCode::InvalidArgument, "{bad}");
        }
    }
}
