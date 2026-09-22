//! [`Core`]: the transport-independent API facade (`core-design.md` §2). It owns
//! the shared database and one [`Orchestration`] per open project, runs
//! [`ApiCommand`]s and merges every project's [`ApiEvent`]s into one broadcast
//! stream. The Tauri command and the WebSocket bridge (Stage 5) only forward
//! `command` / `subscribe` to it.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use tokio::sync::broadcast;

use super::orchestration::{OrchError, Orchestration, OrchestrationConfig, UserActionError};
use crate::acp::HarnessConfig;
use crate::api::{
    ApiCommand, ApiError, ApiEvent, ApiResponse, DEFAULT_EVENT_PAGE, DEFAULT_GRAPH_COMMITS,
    LoggedEvent, MAX_EVENT_PAGE, ProjectInfo, Snapshot,
};
use crate::domain::DomainConfig;
use crate::git::{self, GitCli, GitOverview, MAX_GRAPH_COMMITS, worktree_root};
use crate::store::{ProjectRecord, Store, db_path};
use crate::usage::UsageService;

/// How many events a slow subscriber may fall behind before it misses some
/// (it then sees a `seq` gap and catches up with `ListEvents`).
const EVENT_BUFFER: usize = 4096;

/// Working directory of the usage probe agent, under the data directory.
const USAGE_PROBE_DIR: &str = "usage-probe";

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
    /// Harness asked for subscription usage (`ApiCommand::GetUsage`); `None`
    /// answers `unavailable`.
    pub usage: Option<HarnessConfig>,
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
            usage: Some(HarnessConfig::claude_code_usage_probe()),
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
    usage: UsageService,
    /// Set by `shutdown`: no project may be opened any more.
    closed: AtomicBool,
}

impl Core {
    /// Opens the database. Projects are started by [`ApiCommand::OpenProject`].
    pub async fn start(cfg: CoreConfig) -> Result<Self, ApiError> {
        let store = Store::open(&db_path(&cfg.data_dir)).await?;
        let (events, _) = broadcast::channel(EVENT_BUFFER);
        let usage = UsageService::new(cfg.usage.clone(), cfg.data_dir.join(USAGE_PROBE_DIR));
        Ok(Self {
            inner: Arc::new(Inner {
                cfg,
                store,
                open: Mutex::new(HashMap::new()),
                opening: tokio::sync::Mutex::new(()),
                events,
                usage,
                closed: AtomicBool::new(false),
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
            ApiCommand::RetryGroupMerge { project, group } => {
                let orch = self.orchestration(&project)?;
                orch.retry_group_merge(group)
                    .await
                    .map_err(user_action_error)?;
                Ok(ApiResponse::Accepted)
            }
            ApiCommand::GetGitOverview { project, limit } => Ok(ApiResponse::GitOverview {
                git: self.git_overview(&project, limit).await?,
            }),
            ApiCommand::GetUsage { refresh } => {
                let usage = self
                    .inner
                    .usage
                    .get(refresh.unwrap_or(false))
                    .await
                    .map_err(|e| ApiError::unavailable(format!("usage: {e}")))?;
                Ok(ApiResponse::Usage { usage })
            }
        }
    }

    /// Opens every known project that has unfinished work — an active or
    /// finishing group, or undelivered inbox entries — so its interrupted tasks
    /// resume right away instead of waiting for the UI to open it (Stage 5
    /// decision, `core-design.md` §2). Projects are opened one after another;
    /// the result of each is returned (and failures logged).
    pub async fn resume_unfinished(&self) -> Vec<(String, Result<ProjectInfo, ApiError>)> {
        let records = match self.inner.store.projects().await {
            Ok(r) => r,
            Err(e) => {
                tracing::error!("could not list the projects to resume: {e}");
                return Vec::new();
            }
        };
        let mut out = Vec::new();
        for r in records {
            match self.inner.store.load_state(&r.id).await {
                Ok(Some(state)) if state.open_group().is_some() || !state.inbox.is_empty() => {}
                Ok(_) => continue,
                Err(e) => {
                    tracing::error!("could not read the state of {}: {e}", r.id);
                    continue;
                }
            }
            let path = r.path.display().to_string();
            let result = self.open_project(&path).await;
            match &result {
                Ok(_) => tracing::info!("resumed project {} ({path})", r.id),
                Err(e) => tracing::error!("could not resume project {} ({path}): {e}", r.id),
            }
            out.push((r.id, result));
        }
        out
    }

    /// The state of an open project (for tests and the app shell).
    pub async fn snapshot(&self, project: &str) -> Result<Snapshot, ApiError> {
        self.orchestration(project)?
            .snapshot()
            .await
            .map_err(orch_error)
    }

    /// Stops every open project's agents and MCP server, and a running usage
    /// probe. Waits for a project that is being opened (it is not left running)
    /// and refuses later opens.
    pub async fn shutdown(&self) {
        self.inner.closed.store(true, Ordering::SeqCst);
        self.inner.usage.close().await;
        let _opening = self.inner.opening.lock().await;
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
        if self.inner.closed.load(Ordering::SeqCst) {
            return Err(ApiError::unavailable("Yhtye is shutting down"));
        }
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

    async fn git_overview(
        &self,
        project: &str,
        limit: Option<u32>,
    ) -> Result<GitOverview, ApiError> {
        let known = self.inner.store.projects().await?;
        let record = known
            .iter()
            .find(|r| r.id == project)
            .ok_or_else(|| ApiError::not_found(format!("unknown project {project}")))?;
        let limit = limit.unwrap_or(DEFAULT_GRAPH_COMMITS) as usize;
        git::overview(&record.path, limit.min(MAX_GRAPH_COMMITS))
            .await
            .map_err(|e| ApiError::unavailable(format!("could not read the repository: {e}")))
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
    let top = git::show_toplevel(&dir)
        .await
        .map_err(|e| ApiError::internal(format!("could not run git: {e}")))?
        .map(|top| std::fs::canonicalize(&top).unwrap_or(top));
    if top.as_deref() != Some(dir.as_path()) {
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
