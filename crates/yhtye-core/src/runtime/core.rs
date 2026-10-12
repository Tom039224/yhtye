//! [`Core`]: the transport-independent API facade (`core-design.md` §2). It owns
//! the shared database and one [`Orchestration`] per open project, runs
//! [`ApiCommand`]s and merges every project's [`ApiEvent`]s into one broadcast
//! stream. The Tauri command and the WebSocket bridge (Stage 5) only forward
//! `command` / `subscribe` to it.

mod harnesses;
mod settings;

pub use harnesses::DetectionConfig;

use std::collections::HashMap;
use std::future::Future;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use futures::future::join_all;
use tokio::sync::broadcast;

use super::chats::ChatTarget;
use super::host::host_name;
use super::orchestration::{OrchError, Orchestration, OrchestrationConfig, UserActionError};
use crate::acp::HarnessConfig;
use crate::agents::{AgentCatalog, AgentChoice, HarnessPreset, ModelEfforts, ModelService};
use crate::api::{
    ApiCommand, ApiError, ApiEvent, ApiEventBody, ApiResponse, DEFAULT_EVENT_PAGE,
    DEFAULT_GRAPH_COMMITS, LoggedEvent, MAX_EVENT_PAGE, ProjectInfo, Snapshot,
};
use crate::clock::now_ms;
use crate::domain::DomainConfig;
use crate::git::{self, GitCli, GitOverview, MAX_GRAPH_COMMITS, worktree_root};
use crate::remote::{RemoteService, Tailscale, TailscaleCli};
use crate::secrets::{KeyringBackend, SecretBackend, Secrets};
use crate::store::{ProjectRecord, Store, db_path};
use crate::usage::UsageService;

/// How many events a slow subscriber may fall behind before it misses some
/// (it then sees a `seq` gap and catches up with `ListEvents`).
const EVENT_BUFFER: usize = 4096;

/// Working directory of the usage probe agent, under the data directory.
const USAGE_PROBE_DIR: &str = "usage-probe";
/// Working directory of the model-listing sessions, under the data directory.
const MODEL_PROBE_DIR: &str = "model-probe";

/// How long a project may take to tell whether work is on in it (see `is_running`).
const RUNNING_QUERY_TIMEOUT: Duration = Duration::from_secs(1);

/// Reason recorded when the user cancels without giving one.
const USER_CANCEL_REASON: &str = "cancelled by the user";

#[derive(Debug, Clone)]
pub struct CoreConfig {
    /// Database (`yhtye.sqlite3`) and task worktrees (`worktrees/`).
    pub data_dir: PathBuf,
    /// Registered harnesses (`core-design.md` §15.2). Ignored when `detection`
    /// is set: the installed ones are registered instead.
    pub harnesses: Vec<HarnessPreset>,
    /// Harness × model of every role that has no settings. Ignored when
    /// `detection` is set.
    pub default_agent: AgentChoice,
    /// Finds the installed harnesses at start and on `GetHarnesses` /
    /// `DetectHarnesses` / `SetHarnessPath`; `None` keeps `harnesses` as they are.
    pub detection: Option<DetectionConfig>,
    /// Where each project's MCP server listens (`127.0.0.1:0`).
    pub mcp_bind: SocketAddr,
    pub domain: DomainConfig,
    /// Harness asked for subscription usage (`ApiCommand::GetUsage`); `None`
    /// answers `unavailable`.
    pub usage: Option<HarnessConfig>,
    /// Where the values of the secret environment variables live (the OS
    /// credential store; tests use an in-memory one).
    pub secret_backend: Arc<dyn SecretBackend>,
    /// This PC on Tailscale, for remote access (the `tailscale` command; tests
    /// use a fake).
    pub tailscale: Arc<dyn Tailscale>,
}

impl CoreConfig {
    /// Claude Code registered, every role defaulting to it with `model`
    /// (tests and development use Haiku).
    #[must_use]
    pub fn claude_code(data_dir: impl Into<PathBuf>, model: &str) -> Self {
        let preset = HarnessPreset::claude_code(model);
        Self {
            data_dir: data_dir.into(),
            default_agent: AgentChoice::new(preset.id.clone(), Some(model)),
            harnesses: vec![preset],
            detection: None,
            mcp_bind: SocketAddr::from(([127, 0, 0, 1], 0)),
            domain: DomainConfig::default(),
            usage: Some(HarnessConfig::claude_code_usage_probe()),
            secret_backend: Arc::new(KeyringBackend::new()),
            tailscale: Arc::new(TailscaleCli),
        }
    }

    /// The app's configuration (Tauri and the dev bridge): the harnesses that
    /// are installed on this machine are registered (found again when the
    /// settings are opened), Claude Code with `model`. A role without settings
    /// runs the first of them ([`crate::agents::default_choice`]).
    #[must_use]
    pub fn installed(data_dir: impl Into<PathBuf>, model: &str) -> Self {
        let mut cfg = Self::claude_code(data_dir, model);
        cfg.detection = Some(DetectionConfig::from_process(model));
        cfg
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
    /// Serializes the detections of the installed harnesses.
    detecting: tokio::sync::Mutex<()>,
    /// Serializes the changes of the agent settings and the secrets, each from
    /// its validation to its last step (several clients may change them at once).
    settings_lock: tokio::sync::Mutex<()>,
    /// [`crate::api::AgentSettingsView::revision`]: grows with every change.
    settings_revision: AtomicU64,
    events: broadcast::Sender<ApiEvent>,
    usage: UsageService,
    agents: Arc<AgentCatalog>,
    models: ModelService,
    secrets: Arc<Secrets>,
    remote: RemoteService,
    /// Set by `shutdown`: no project may be opened any more.
    closed: AtomicBool,
}

impl Core {
    /// Opens the database. Projects are started by [`ApiCommand::OpenProject`].
    pub async fn start(cfg: CoreConfig) -> Result<Self, ApiError> {
        let store = Store::open(&db_path(&cfg.data_dir)).await?;
        // Before any agent of this run exists: kill those a previous run left behind.
        crate::acp::start_process_registry(&cfg.data_dir).await;
        let (events, _) = broadcast::channel(EVENT_BUFFER);
        let secrets = Arc::new(Secrets::new(
            cfg.secret_backend.clone(),
            store.secret_names().await?,
        ));
        let (presets, builtin, usage_probe) = match &cfg.detection {
            Some(detection) => {
                let found = detection.detect(&store.harness_paths().await?);
                found.log();
                let probe = cfg.usage.clone().map(|u| found.with_found_npx(u));
                (found.presets, found.builtin, probe)
            }
            None => (
                cfg.harnesses.clone(),
                cfg.default_agent.clone(),
                cfg.usage.clone(),
            ),
        };
        let usage = UsageService::new(usage_probe, cfg.data_dir.join(USAGE_PROBE_DIR))
            .with_secrets(secrets.clone());
        let agents = Arc::new(AgentCatalog::new(presets, builtin).with_secrets(secrets.clone()));
        for row in store.agent_settings().await? {
            agents.set(row.project.as_deref(), row.role, Some(row.settings));
        }
        let models = ModelService::new(cfg.data_dir.join(MODEL_PROBE_DIR), secrets.clone());
        let remote =
            RemoteService::new(store.clone(), cfg.tailscale.clone(), events.clone()).await?;
        Ok(Self {
            inner: Arc::new(Inner {
                cfg,
                store,
                open: Mutex::new(HashMap::new()),
                opening: tokio::sync::Mutex::new(()),
                detecting: tokio::sync::Mutex::new(()),
                settings_lock: tokio::sync::Mutex::new(()),
                settings_revision: AtomicU64::new(settings::initial_revision()),
                events,
                usage,
                agents,
                models,
                secrets,
                remote,
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
            ApiCommand::Ping => Ok(ApiResponse::Pong { host: host_name() }),
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
            ApiCommand::SendUserMessage {
                project,
                chat,
                text,
            } => {
                if text.trim().is_empty() {
                    return Err(ApiError::invalid_argument("the message is empty"));
                }
                self.orchestration(&project)?
                    .send_user_message(chat, text)
                    .await
                    .map_err(user_action_error)?;
                Ok(ApiResponse::Accepted)
            }
            ApiCommand::OpenChat { project, chat } => {
                let starting = self
                    .orchestration(&project)?
                    .open_chat(chat)
                    .await
                    .map_err(user_action_error)?;
                Ok(ApiResponse::ChatOpened { starting })
            }
            ApiCommand::CancelOrchestratorTurn { project, chat } => {
                self.orchestration(&project)?
                    .cancel_orchestrator_turn(chat)
                    .await
                    .map_err(user_action_error)?;
                Ok(ApiResponse::Accepted)
            }
            ApiCommand::CompactChat { project, chat } => {
                self.orchestration(&project)?
                    .compact_chat(chat)
                    .await
                    .map_err(user_action_error)?;
                Ok(ApiResponse::Accepted)
            }
            ApiCommand::ListChats { project } => {
                self.known_project(&project).await?;
                Ok(ApiResponse::Chats {
                    chats: self.inner.store.chats(&project).await?,
                })
            }
            ApiCommand::CreateChat {
                project,
                branch,
                worktree,
            } => {
                let target = chat_target(branch, worktree)?;
                let chat = self
                    .open_orchestration(&project)
                    .await?
                    .create_chat_for(target)
                    .await
                    .map_err(user_action_error)?;
                Ok(ApiResponse::Chat { chat })
            }
            ApiCommand::RenameChat {
                project,
                chat,
                title,
            } => {
                let chat = self
                    .orchestration(&project)?
                    .rename_chat(chat, title)
                    .await
                    .map_err(user_action_error)?;
                Ok(ApiResponse::Chat { chat })
            }
            ApiCommand::DeleteChat { project, chat } => {
                self.orchestration(&project)?
                    .delete_chat(chat)
                    .await
                    .map_err(user_action_error)?;
                Ok(ApiResponse::Accepted)
            }
            ApiCommand::CreateBranch {
                project,
                name,
                from,
            } => {
                let chat = self
                    .open_orchestration(&project)
                    .await?
                    .create_branch(name, from)
                    .await
                    .map_err(user_action_error)?;
                Ok(ApiResponse::Chat { chat })
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
            ApiCommand::GetAgentSettings { project } => Ok(ApiResponse::AgentSettings {
                settings: Box::new(self.agent_settings(project).await?),
            }),
            ApiCommand::SetAgentSettings {
                project,
                role,
                settings,
                expected_revision,
            } => {
                self.set_agent_settings(project.as_deref(), role, settings, expected_revision)
                    .await?;
                Ok(ApiResponse::AgentSettings {
                    settings: Box::new(self.agent_settings(project).await?),
                })
            }
            ApiCommand::ListHarnessModels { harness, refresh } => {
                let preset = self
                    .inner
                    .agents
                    .preset(&harness)
                    .ok_or_else(|| ApiError::not_found(format!("unknown harness {harness}")))?;
                let models = self
                    .inner
                    .models
                    .get(&preset, refresh.unwrap_or(false))
                    .await
                    .map_err(|e| ApiError::unavailable(format!("models of {harness}: {e}")))?;
                Ok(ApiResponse::HarnessModels { models })
            }
            ApiCommand::ListModelEfforts { harness, model } => {
                let preset = self
                    .inner
                    .agents
                    .preset(&harness)
                    .ok_or_else(|| ApiError::not_found(format!("unknown harness {harness}")))?;
                let efforts = self
                    .inner
                    .models
                    .get_efforts(&preset, &model)
                    .await
                    .map_err(|e| {
                        ApiError::unavailable(format!("efforts of {harness}/{model}: {e}"))
                    })?;
                Ok(ApiResponse::ModelEfforts {
                    efforts: ModelEfforts {
                        harness,
                        model,
                        efforts,
                    },
                })
            }
            ApiCommand::GetHarnesses => self.get_harnesses().await,
            ApiCommand::DetectHarnesses => self.detect_harnesses().await,
            ApiCommand::SetHarnessPath { harness, path } => {
                self.set_harness_path(harness, path).await
            }
            ApiCommand::ListSecretEnv => Ok(ApiResponse::SecretEnv {
                names: self.inner.secrets.names(),
            }),
            ApiCommand::SetSecretEnv { name, value } => {
                self.set_secret(&name, value).await?;
                Ok(ApiResponse::SecretEnv {
                    names: self.inner.secrets.names(),
                })
            }
            ApiCommand::DeleteSecretEnv { name } => {
                self.delete_secret(&name).await?;
                Ok(ApiResponse::SecretEnv {
                    names: self.inner.secrets.names(),
                })
            }
            ApiCommand::GetRemoteStatus => Ok(ApiResponse::RemoteStatus {
                status: self.inner.remote.status().await,
            }),
            ApiCommand::SetRemoteSettings { enabled, port } => {
                if enabled && self.inner.closed.load(Ordering::SeqCst) {
                    return Err(ApiError::unavailable("Yhtye is shutting down"));
                }
                Ok(ApiResponse::RemoteStatus {
                    status: self.inner.remote.set_settings(self, enabled, port).await?,
                })
            }
            ApiCommand::CreatePairingCode => Ok(ApiResponse::PairingCode {
                pairing: self.inner.remote.create_pairing_code().await?,
            }),
            ApiCommand::ListRemoteDevices => Ok(ApiResponse::RemoteDevices {
                devices: self.inner.remote.devices().await?,
            }),
            ApiCommand::RevokeRemoteDevice { device } => Ok(ApiResponse::RemoteDevices {
                devices: self.inner.remote.revoke(&device).await?,
            }),
            ApiCommand::ListClients => Ok(ApiResponse::Clients {
                clients: self.inner.remote.clients().await?,
            }),
        }
    }

    /// Starts remote access if its stored settings enable it (the app calls it
    /// once after starting; `core-design.md` §18). A failure to start is kept
    /// in the status (`GetRemoteStatus`), not returned.
    pub async fn start_remote(&self) {
        if !self.inner.closed.load(Ordering::SeqCst) {
            self.inner.remote.start_if_enabled(self).await;
        }
    }

    async fn known_project(&self, project: &str) -> Result<ProjectRecord, ApiError> {
        self.inner
            .store
            .projects()
            .await?
            .into_iter()
            .find(|r| r.id == project)
            .ok_or_else(|| ApiError::not_found(format!("unknown project {project}")))
    }

    /// Opens every known project that has unfinished work — an active or
    /// finishing group, or undelivered inbox entries — so its interrupted tasks
    /// resume right away instead of waiting for the UI to open it (Stage 5
    /// decision, `core-design.md` §2). A chat's orchestrator is not unfinished
    /// work: it comes back when its chat is opened or gets something (T-48).
    /// Projects are opened one after another; the result of each is returned (and
    /// failures logged).
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
            if !self.has_unfinished_work(&r.id).await {
                continue;
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

    /// Whether `project` has work that must continue after a start (see
    /// [`Core::resume_unfinished`]); a read error counts as none (and is logged).
    async fn has_unfinished_work(&self, project: &str) -> bool {
        let store = &self.inner.store;
        let state = match store.load_state(project).await {
            Ok(Some(state)) => state,
            Ok(None) => return false,
            Err(e) => {
                tracing::error!("could not read the state of {project}: {e}");
                return false;
            }
        };
        state.has_open_group() || !state.inbox.is_empty()
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
        // First: remote clients must not start work while the projects stop.
        self.inner.remote.shutdown().await;
        self.inner.usage.close().await;
        self.inner.models.close().await;
        let _opening = self.inner.opening.lock().await;
        let open: Vec<Arc<Orchestration>> = self.lock_open().drain().map(|(_, o)| o).collect();
        for orch in open {
            orch.shutdown().await;
        }
    }

    async fn list_projects(&self) -> Result<Vec<ProjectInfo>, ApiError> {
        let records = self.inner.store.projects().await?;
        let open: HashMap<String, Arc<Orchestration>> = self.lock_open().clone();
        Ok(project_infos(&records, |r| {
            open.get(&r.id).map(|orch| is_running(&r.id, orch))
        })
        .await)
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
        let already = self.lock_open().get(&record.id).cloned();
        if let Some(orch) = already {
            let running = is_running(&record.id, &orch).await;
            return Ok(project_info(&record, true, running));
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
        let orch = Arc::new(orch);
        self.lock_open()
            .insert(record.id.clone(), Arc::clone(&orch));
        // Registered, or open now: every client's project list changed.
        let _ = self
            .inner
            .events
            .send(ApiEvent::app_wide(ApiEventBody::ProjectsChanged {
                project: record.id.clone(),
            }));
        // Work that was interrupted may already be resuming.
        let running = is_running(&record.id, &orch).await;
        Ok(project_info(&record, true, running))
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

    /// Like [`Core::orchestration`], but a known project that is not open is
    /// `unavailable` (creating chats and branches needs the running project).
    async fn open_orchestration(&self, project: &str) -> Result<Arc<Orchestration>, ApiError> {
        self.known_project(project).await?;
        self.lock_open()
            .get(project)
            .cloned()
            .ok_or_else(|| ApiError::unavailable(format!("project {project} is not open")))
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
            agents: self.inner.agents.clone(),
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

fn project_info(r: &ProjectRecord, open: bool, running: bool) -> ProjectInfo {
    ProjectInfo {
        id: r.id.clone(),
        name: dir_name(&r.path),
        path: r.path.display().to_string(),
        open,
        running,
    }
}

/// Whether work is on in an open project. A project busy handling a request
/// for longer than `RUNNING_QUERY_TIMEOUT` (a merge, a restart) counts as
/// running, so a listing never waits for it; one that cannot tell (it is
/// shutting down, or its store failed) counts as idle, and the failure is logged.
async fn is_running(project: &str, orch: &Orchestration) -> bool {
    match tokio::time::timeout(RUNNING_QUERY_TIMEOUT, orch.is_running()).await {
        Ok(Ok(running)) => running,
        Ok(Err(e)) => {
            tracing::warn!("could not tell whether {project} is running: {e}");
            false
        }
        Err(_) => true,
    }
}

/// The listing of `records`, in order. `running` asks whether work is on in a
/// project that is open (`None`: it is not open). The projects are asked all at
/// once: a busy one costs up to `RUNNING_QUERY_TIMEOUT`, and N busy ones must
/// not cost N times that (the UI waits for the listing before it reopens the
/// last project).
async fn project_infos<'a, Q: Future<Output = bool>>(
    records: &'a [ProjectRecord],
    running: impl Fn(&'a ProjectRecord) -> Option<Q>,
) -> Vec<ProjectInfo> {
    join_all(records.iter().map(|r| {
        let query = running(r);
        async move {
            match query {
                Some(query) => project_info(r, true, query.await),
                None => project_info(r, false, false),
            }
        }
    }))
    .await
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

/// `create_chat`'s target: exactly one of a branch or a worktree.
fn chat_target(branch: Option<String>, worktree: Option<String>) -> Result<ChatTarget, ApiError> {
    match (branch, worktree) {
        (Some(b), None) => Ok(ChatTarget::Branch(b)),
        (None, Some(w)) => Ok(ChatTarget::Worktree(PathBuf::from(w))),
        _ => Err(ApiError::invalid_argument(
            "give exactly one of branch or worktree",
        )),
    }
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

    #[tokio::test(start_paused = true)]
    async fn busy_projects_cost_one_timeout_together_not_one_each() {
        let records: Vec<ProjectRecord> = (0..5).map(|i| record(&format!("p{i}"))).collect();
        let started = tokio::time::Instant::now();
        // Every open project is busy until the timeout (which then counts it as running).
        let infos = project_infos(&records, |_| {
            Some(async {
                tokio::time::sleep(RUNNING_QUERY_TIMEOUT).await;
                true
            })
        })
        .await;
        assert_eq!(started.elapsed(), RUNNING_QUERY_TIMEOUT);
        assert!(infos.iter().all(|p| p.open && p.running));
    }

    #[tokio::test(start_paused = true)]
    async fn the_listing_keeps_the_order_and_tells_open_from_running() {
        let records = [record("a"), record("b"), record("c")];
        let infos = project_infos(&records, |r| match r.id.as_str() {
            // `a` answers late, `b` at once: the order is still the records'.
            "a" => Some(Box::pin(async {
                tokio::time::sleep(RUNNING_QUERY_TIMEOUT).await;
                true
            })
                as std::pin::Pin<Box<dyn Future<Output = bool>>>),
            "b" => Some(Box::pin(async { false })),
            _ => None,
        })
        .await;
        let flags: Vec<_> = infos
            .iter()
            .map(|p| (p.id.as_str(), p.open, p.running))
            .collect();
        assert_eq!(
            flags,
            [("a", true, true), ("b", true, false), ("c", false, false)]
        );
    }

    #[test]
    fn project_ids_come_from_the_directory_name() {
        assert_eq!(new_project_id(Path::new("/a/My App"), &[]), "my-app");
        assert_eq!(new_project_id(Path::new("/a/日本"), &[]), "project");
        let known = [record("app"), record("app-2")];
        assert_eq!(new_project_id(Path::new("/b/app"), &known), "app-3");
    }

    #[test]
    fn a_chat_is_created_for_exactly_one_target() {
        assert_eq!(
            chat_target(Some("main".into()), None).ok(),
            Some(ChatTarget::Branch("main".into()))
        );
        assert_eq!(
            chat_target(None, Some("/w".into())).ok(),
            Some(ChatTarget::Worktree("/w".into()))
        );
        assert!(chat_target(None, None).is_err());
        assert!(chat_target(Some("main".into()), Some("/w".into())).is_err());
    }

    #[test]
    fn empty_cancel_reasons_get_the_default() {
        assert_eq!(reason_or_default(None), USER_CANCEL_REASON);
        assert_eq!(reason_or_default(Some("  ".into())), USER_CANCEL_REASON);
        assert_eq!(reason_or_default(Some("why".into())), "why");
    }
}
