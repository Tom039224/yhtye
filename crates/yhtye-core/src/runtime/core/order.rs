//! The recently-used order of the projects and of each project's branches
//! (`order.rs`) as [`Core`] keeps it: stored in `app_settings` as JSON lists
//! (`order.projects`, `order.branches.<project>`), changed under one lock (several
//! clients may use or drag at once), and announced to every client. Using an
//! item moves it to the front; a drag stores the order the user made.
//!
//! An order is a preference: a failure to read or write it is logged and never
//! fails the command that used the item.

use super::{Core, ProjectInfo, now_ms};
use crate::api::{ApiError, ApiEvent, ApiEventBody};
use crate::order::{decode, encode, reordered, touched};
use crate::store::{ChatInfo, ProjectRecord};

const PROJECTS_KEY: &str = "order.projects";

fn branches_key(project: &str) -> String {
    format!("order.branches.{project}")
}

impl Core {
    async fn stored_order(&self, key: &str) -> Result<Vec<String>, ApiError> {
        Ok(self
            .inner
            .store
            .app_setting(key)
            .await?
            .map(|text| decode(&text))
            .unwrap_or_default())
    }

    async fn store_order(&self, key: &str, list: &[String]) -> Result<(), ApiError> {
        self.inner
            .store
            .set_app_setting(key, &encode(list), now_ms())
            .await?;
        Ok(())
    }

    fn announce(&self, body: ApiEventBody) {
        // Nobody listening is fine.
        let _ = self.inner.events.send(ApiEvent::app_wide(body));
    }

    /// The registered projects in the project order.
    pub(super) async fn ordered_projects(&self) -> Result<Vec<ProjectRecord>, ApiError> {
        let records = self.inner.store.projects().await?;
        let order = self.stored_order(PROJECTS_KEY).await.unwrap_or_else(|e| {
            tracing::warn!("could not read the project order: {e}");
            Vec::new()
        });
        Ok(crate::order::sorted_by_order(records, &order, |r| &r.id))
    }

    /// The user used `project`: it moves to the front. Announced with
    /// `projects_changed` when `announce` and the order changed (a caller about
    /// to announce something itself passes `false`).
    pub(super) async fn touch_project(&self, project: &str, announce: bool) {
        let _guard = self.inner.order_lock.lock().await;
        let result: Result<bool, ApiError> = async {
            let list = self.stored_order(PROJECTS_KEY).await?;
            let next = touched(&list, project);
            if next == list {
                return Ok(false);
            }
            self.store_order(PROJECTS_KEY, &next).await?;
            Ok(true)
        }
        .await;
        match result {
            Ok(true) if announce => self.announce(ApiEventBody::ProjectsChanged {
                project: project.to_string(),
            }),
            Ok(_) => {}
            Err(e) => tracing::warn!("could not move {project} to the front: {e}"),
        }
    }

    /// Stores the order the user made; answers with the listing.
    pub(super) async fn reorder_projects(
        &self,
        ids: Vec<String>,
    ) -> Result<Vec<ProjectInfo>, ApiError> {
        {
            let _guard = self.inner.order_lock.lock().await;
            let known: Vec<String> = self
                .inner
                .store
                .projects()
                .await?
                .into_iter()
                .map(|r| r.id)
                .collect();
            let ids: Vec<String> = ids.into_iter().filter(|id| known.contains(id)).collect();
            let list = self.stored_order(PROJECTS_KEY).await?;
            let next = reordered(&list, &ids);
            if next != list {
                self.store_order(PROJECTS_KEY, &next).await?;
                self.announce(ApiEventBody::ProjectsChanged {
                    project: String::new(),
                });
            }
        }
        self.list_projects().await
    }

    /// The order of `project`'s branches.
    pub(super) async fn branch_order(&self, project: &str) -> Result<Vec<String>, ApiError> {
        self.known_project(project).await?;
        self.stored_order(&branches_key(project)).await
    }

    /// Stores the order the user made; answers with the whole order.
    pub(super) async fn reorder_branches(
        &self,
        project: &str,
        keys: Vec<String>,
    ) -> Result<Vec<String>, ApiError> {
        self.known_project(project).await?;
        let _guard = self.inner.order_lock.lock().await;
        let key = branches_key(project);
        let list = self.stored_order(&key).await?;
        let next = reordered(&list, &keys);
        if next != list {
            self.store_order(&key, &next).await?;
            self.announce(ApiEventBody::BranchOrderChanged {
                project: project.to_string(),
                keys: next.clone(),
            });
        }
        Ok(next)
    }

    /// A chat of the worktree at `worktree` was used (opened, written to,
    /// created): that worktree's branch moves to the front.
    pub(super) async fn touch_worktree(&self, project: &str, worktree: &str) {
        let _guard = self.inner.order_lock.lock().await;
        let key = branches_key(project);
        let result: Result<(), ApiError> = async {
            let list = self.stored_order(&key).await?;
            let next = touched(&list, worktree);
            if next == list {
                return Ok(());
            }
            self.store_order(&key, &next).await?;
            self.announce(ApiEventBody::BranchOrderChanged {
                project: project.to_string(),
                keys: next,
            });
            Ok(())
        }
        .await;
        if let Err(e) = result {
            tracing::warn!("could not move a branch of {project} to the front: {e}");
        }
    }

    /// `chat` of `project` was used: see [`Core::touch_worktree`].
    pub(super) async fn touch_chat(&self, project: &str, chat: &str) {
        match self.inner.store.chats(project).await {
            Ok(chats) => {
                if let Some(c) = chats.iter().find(|c| c.id == chat) {
                    self.touch_worktree(project, &c.worktree).await;
                }
            }
            Err(e) => tracing::warn!("could not read the chats of {project}: {e}"),
        }
    }

    /// A chat was just made: its branch is in use.
    pub(super) async fn touch_new_chat(&self, project: &str, chat: &ChatInfo) {
        self.touch_worktree(project, &chat.worktree).await;
    }
}
