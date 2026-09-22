//! [`Core`]: the transport-independent API facade (`core-design.md` §2). It owns
//! the shared database and one [`Orchestration`] per open project, runs
//! [`ApiCommand`]s and merges every project's [`ApiEvent`]s into one broadcast
//! stream. The Tauri command and the WebSocket bridge (Stage 5) only forward
//! `command` / `subscribe` to it.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use tokio::sync::broadcast;

use super::orchestration::{OrchError, Orchestration, OrchestrationConfig, UserActionError};
use crate::acp::HarnessConfig;
use crate::api::{
    ApiCommand, ApiError, ApiEvent, ApiResponse, DEFAULT_EVENT_PAGE, LoggedEvent, MAX_EVENT_PAGE,
    ProjectInfo, Snapshot,
};
use crate::domain::DomainConfig;
use crate::git::{GitCli, worktree_root};
use crate::store::{ProjectRecord, Store, db_path};

/// How many events a slow subscriber may fall behind before it misses some
/// (it then sees a `seq` gap and catches up with `ListEvents`).
const EVENT_BUFFER: usize = 4096;

/// Reason recorded when the user cancels without giving one.
const USER_CANCEL_REASON: &str = "cancelled by the user";

#[derive(Debug, Clone)]
pub struct CoreConfig {
    /// Database (`yhtye.sqlite3`) and task worktrees (`worktrees/`).
    pub data_dir: PathBuf,
    pub orchestrator: HarnessConfig,
    pub implementer: HarnessConfig,
    pub reviewer: HarnessConfig,
    /// Where each project's MCP server listens (`127.0.0.1:0`).
    pub mcp_bind: SocketAddr,
    pub domain: DomainConfig,
}

impl CoreConfig {
    /// Claude Code for every role with `model` (tests and development use Haiku).
    #[must_use]
    pub fn claude_code(data_dir: impl Into<PathBuf>, model: &str) -> Self {
        Self {
            data_dir: data_dir.into(),
            orchestrator: HarnessConfig::claude_code_orchestrator(model),
            implementer: HarnessConfig::claude_code(model),
            reviewer: HarnessConfig::claude_code(model),
            mcp_bind: SocketAddr::from(([127, 0, 0, 1], 0)),
            domain: DomainConfig::default(),
        }
    }
}

/// The API facade. Cheap to clone.
#[derive(Clone)]
pub struct Core {
    inner: Arc<Inner>,
}

struct Inner {
    cfg: CoreConfig,
    store: Store,
    open: Mutex<HashMap<String, Arc<Orchestration>>>,
    /// Serializes `OpenProject` (starting an orchestrator takes a while).
    opening: tokio::sync::Mutex<()>,
    events: broadcast::Sender<ApiEvent>,
}

impl Core {
    /// Opens the database. Projects are started by [`ApiCommand::OpenProject`].
    pub async fn start(cfg: CoreConfig) -> Result<Self, ApiError> {
        let store = Store::open(&db_path(&cfg.data_dir)).await?;
        let (events, _) = broadcast::channel(EVENT_BUFFER);
        Ok(Self {
            inner: Arc::new(Inner {
                cfg,
                store,
                open: Mutex::new(HashMap::new()),
                opening: tokio::sync::Mutex::new(()),
                events,
            }),
        })
    }

    /// Every project's events (durable and live), in order per project.
    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<ApiEvent> {
        self.inner.events.subscribe()
    }

    pub async fn command(&self, cmd: ApiCommand) -> Result<ApiResponse, ApiError> {
        match cmd {
            ApiCommand::ListProjects => Ok(ApiResponse::Projects {
                projects: self.list_projects().await?,
            }),
            ApiCommand::OpenProject { path } => Ok(ApiResponse::Project {
                project: self.open_project(&path).await?,
            }),
            ApiCommand::GetSnapshot { project } => Ok(ApiResponse::Snapshot {
                snapshot: self.snapshot(&project).await?,
            }),
            ApiCommand::ListEvents {
                project,
                after_seq,
                limit,
            } => self.list_events(&project, after_seq, limit).await,
            ApiCommand::SendUserMessage { project, text } => {
                if text.trim().is_empty() {
                    return Err(ApiError::invalid_argument("the message is empty"));
                }
                self.orchestration(&project)?
                    .send_user_message(text)
                    .map_err(orch_error)?;
                Ok(ApiResponse::Accepted)
            }
            ApiCommand::CancelOrchestratorTurn { project } => {
                self.orchestration(&project)?
                    .cancel_orchestrator_turn()
                    .map_err(orch_error)?;
                Ok(ApiResponse::Accepted)
            }
            ApiCommand::CancelTask {
                project,
                task,
                reason,
            } => {
                let orch = self.orchestration(&project)?;
                orch.cancel_task(task, reason_or_default(reason))
                    .await
                    .map_err(user_action_error)?;
                Ok(ApiResponse::Accepted)
            }
            ApiCommand::CancelGroup {
                project,
                group,
                reason,
            } => {
                let orch = self.orchestration(&project)?;
                orch.cancel_group(group, reason_or_default(reason))
                    .await
                    .map_err(user_action_error)?;
                Ok(ApiResponse::Accepted)
            }
        }
    }

    /// The state of an open project (for tests and the app shell).
    pub async fn snapshot(&self, project: &str) -> Result<Snapshot, ApiError> {
        self.orchestration(project)?
            .snapshot()
            .await
            .map_err(orch_error)
    }

    /// Stops every open project's agents and MCP server.
    pub async fn shutdown(&self) {
        let open: Vec<Arc<Orchestration>> = self.lock_open().drain().map(|(_, o)| o).collect();
        for orch in open {
            orch.shutdown().await;
        }
    }

    async fn list_projects(&self) -> Result<Vec<ProjectInfo>, ApiError> {
        let records = self.inner.store.projects().await?;
        let open = self.lock_open();
        Ok(records
            .iter()
            .map(|r| project_info(r, open.contains_key(&r.id)))
            .collect())
    }

    async fn open_project(&self, path: &str) -> Result<ProjectInfo, ApiError> {
        let dir = repository_root(path).await?;
        let _guard = self.inner.opening.lock().await;
        let records = self.inner.store.projects().await?;
        let record = match records.iter().find(|r| r.path == dir) {
            Some(r) => r.clone(),
            None => ProjectRecord {
                id: new_project_id(&dir, &records),
                path: dir.clone(),
                created_ms: 0,
            },
        };
        if self.lock_open().contains_key(&record.id) {
            return Ok(project_info(&record, true));
        }
        let (orch, mut rx) = Orchestration::start(self.orchestration_config(&record))
            .await
            .map_err(|e| ApiError::internal(format!("could not open {}: {e}", dir.display())))?;
        let tx = self.inner.events.clone();
        tokio::spawn(async move {
            while let Some(ev) = rx.recv().await {
                // No subscriber is fine: clients catch up with ListEvents.
                let _ = tx.send(ev);
            }
        });
        self.lock_open().insert(record.id.clone(), Arc::new(orch));
        Ok(project_info(&record, true))
    }

    async fn list_events(
        &self,
        project: &str,
        after_seq: u64,
        limit: Option<u32>,
    ) -> Result<ApiResponse, ApiError> {
        let known = self.inner.store.projects().await?;
        if !known.iter().any(|r| r.id == project) {
            return Err(ApiError::not_found(format!("unknown project {project}")));
        }
        let limit = limit.unwrap_or(DEFAULT_EVENT_PAGE).clamp(1, MAX_EVENT_PAGE);
        let stored = self
            .inner
            .store
            .events_after(project, after_seq, limit)
            .await?;
        let more = stored.len() == limit as usize;
        let events = stored
            .into_iter()
            .map(|e| LoggedEvent::from_stored(project, e))
            .collect();
        Ok(ApiResponse::Events { events, more })
    }

    fn orchestration(&self, project: &str) -> Result<Arc<Orchestration>, ApiError> {
        self.lock_open().get(project).cloned().ok_or_else(|| {
            ApiError::not_found(format!("project {project} is not open (open it first)"))
        })
    }

    fn orchestration_config(&self, record: &ProjectRecord) -> OrchestrationConfig {
        let cfg = &self.inner.cfg;
        OrchestrationConfig {
            project: record.id.clone(),
            project_dir: record.path.clone(),
            orchestrator: cfg.orchestrator.clone(),
            implementer: cfg.implementer.clone(),
            reviewer: cfg.reviewer.clone(),
            mcp_bind: cfg.mcp_bind,
            domain: cfg.domain,
            git: Arc::new(GitCli::new(
                &record.path,
                worktree_root(&cfg.data_dir, &record.id),
            )),
            db_path: db_path(&cfg.data_dir),
        }
    }

    fn lock_open(&self) -> std::sync::MutexGuard<'_, HashMap<String, Arc<Orchestration>>> {
        self.inner
            .open
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }
}

fn project_info(r: &ProjectRecord, open: bool) -> ProjectInfo {
    ProjectInfo {
        id: r.id.clone(),
        name: dir_name(&r.path),
        path: r.path.display().to_string(),
        open,
    }
}

fn dir_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

/// A readable id from the directory name (`my-app`, `my-app-2`, ...).
fn new_project_id(dir: &Path, known: &[ProjectRecord]) -> String {
    let slug: String = dir_name(dir)
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let slug = slug.trim_matches('-');
    let base = if slug.is_empty() { "project" } else { slug };
    let taken = |id: &str| known.iter().any(|r| r.id == id);
    if !taken(base) {
        return base.to_string();
    }
    (2..)
        .map(|n| format!("{base}-{n}"))
        .find(|id| !taken(id))
        .unwrap_or_else(|| base.to_string())
}

/// The top-level directory of the git repository at `path` (which must be it).
async fn repository_root(path: &str) -> Result<PathBuf, ApiError> {
    if path.trim().is_empty() {
        return Err(ApiError::invalid_argument("the project path is empty"));
    }
    let dir = std::fs::canonicalize(path)
        .map_err(|e| ApiError::invalid_argument(format!("{path}: {e}")))?;
    if !dir.is_dir() {
        return Err(ApiError::invalid_argument(format!(
            "{} is not a directory",
            dir.display()
        )));
    }
    let out = tokio::process::Command::new("git")
        .arg("-C")
        .arg(&dir)
        .args(["rev-parse", "--show-toplevel"])
        .env("LC_ALL", "C")
        .output()
        .await
        .map_err(|e| ApiError::internal(format!("could not run git: {e}")))?;
    let top = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let top = std::fs::canonicalize(&top).unwrap_or_else(|_| PathBuf::from(&top));
    if !out.status.success() || top != dir {
        return Err(ApiError::invalid_argument(format!(
            "{} is not the top directory of a git repository",
            dir.display()
        )));
    }
    Ok(dir)
}

fn reason_or_default(reason: Option<String>) -> String {
    reason
        .filter(|r| !r.trim().is_empty())
        .unwrap_or_else(|| USER_CANCEL_REASON.to_string())
}

fn orch_error(e: OrchError) -> ApiError {
    match e {
        OrchError::Closed => ApiError::unavailable("the project's orchestration has stopped"),
        other => ApiError::internal(other.to_string()),
    }
}

fn user_action_error(e: UserActionError) -> ApiError {
    match e {
        UserActionError::Rejected(e) => e.into(),
        UserActionError::Orchestration(e) => orch_error(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(id: &str) -> ProjectRecord {
        ProjectRecord {
            id: id.into(),
            path: PathBuf::from("/x"),
            created_ms: 0,
        }
    }

    #[test]
    fn project_ids_come_from_the_directory_name() {
        assert_eq!(new_project_id(Path::new("/a/My App"), &[]), "my-app");
        assert_eq!(new_project_id(Path::new("/a/日本"), &[]), "project");
        let known = [record("app"), record("app-2")];
        assert_eq!(new_project_id(Path::new("/b/app"), &known), "app-3");
    }

    #[test]
    fn empty_cancel_reasons_get_the_default() {
        assert_eq!(reason_or_default(None), USER_CANCEL_REASON);
        assert_eq!(reason_or_default(Some("  ".into())), USER_CANCEL_REASON);
        assert_eq!(reason_or_default(Some("why".into())), "why");
    }
}
