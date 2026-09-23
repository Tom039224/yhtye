//! Current-state tables: writing the rows of a [`State`] that changed in a
//! transition, and reading a whole [`State`] back.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use sqlx::{FromRow, Sqlite, SqliteConnection, Transaction};

use super::StoreError;
use super::codec::{from_json, int, json, name, parse, parse_opt, uint};
use crate::domain::{Group, Help, InboxEntry, InboxItem, State, Step, Task};

type Tx = Transaction<'static, Sqlite>;

pub(super) async fn insert_project(
    tx: &mut Tx,
    state: &State,
    path: &Path,
) -> Result<(), StoreError> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    sqlx::query(
        "INSERT INTO projects (id, path, config, counters, created_ms) VALUES (?, ?, ?, ?, ?) \
         ON CONFLICT (id) DO NOTHING",
    )
    .bind(&state.project)
    .bind(path.to_string_lossy().into_owned())
    .bind(json(&state.config)?)
    .bind(json(&state.counters)?)
    .bind(int(now)?)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Writes every row of `after` that differs from `before`.
pub(super) async fn write(tx: &mut Tx, before: &State, after: &State) -> Result<(), StoreError> {
    let project = after.project.as_str();
    if before.counters != after.counters || before.config != after.config {
        sqlx::query("UPDATE projects SET config = ?, counters = ? WHERE id = ?")
            .bind(json(&after.config)?)
            .bind(json(&after.counters)?)
            .bind(project)
            .execute(&mut **tx)
            .await?;
    }
    for (ord, g) in after.groups.iter().enumerate() {
        if before.group(&g.id) != Some(g) {
            upsert_group(tx, project, ord, g).await?;
        }
    }
    for (ord, t) in after.tasks.iter().enumerate() {
        if before.task(&t.id) != Some(t) {
            upsert_task(tx, project, ord, t).await?;
        }
    }
    for (ord, h) in after.helps.iter().enumerate() {
        if before.help(&h.id) != Some(h) {
            upsert_help(tx, project, ord, h).await?;
        }
    }
    write_inbox(tx, project, &before.inbox, &after.inbox).await
}

async fn upsert_group(tx: &mut Tx, project: &str, ord: usize, g: &Group) -> Result<(), StoreError> {
    sqlx::query(
        "INSERT OR REPLACE INTO task_groups (project_id, id, ord, title, summary, base_branch, \
         group_branch, status, finish_summary, detail, finish_nudges) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(project)
    .bind(&g.id)
    .bind(int(ord)?)
    .bind(&g.title)
    .bind(&g.summary)
    .bind(&g.base_branch)
    .bind(&g.group_branch)
    .bind(name(&g.status)?)
    .bind(&g.finish_summary)
    .bind(&g.detail)
    .bind(i64::from(g.finish_nudges))
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn upsert_task(tx: &mut Tx, project: &str, ord: usize, t: &Task) -> Result<(), StoreError> {
    sqlx::query(
        "INSERT OR REPLACE INTO tasks (project_id, id, ord, group_id, title, kind, instruction, \
         current_step, status, review_rounds, workdir, cancel_reason) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(project)
    .bind(&t.id)
    .bind(int(ord)?)
    .bind(&t.group)
    .bind(&t.title)
    .bind(name(&t.kind)?)
    .bind(&t.instruction)
    .bind(int(t.current)?)
    .bind(name(&t.status)?)
    .bind(i64::from(t.review_rounds))
    .bind(t.workdir.as_ref().map(|p| p.to_string_lossy().into_owned()))
    .bind(&t.cancel_reason)
    .execute(&mut **tx)
    .await?;
    for sql in [
        "DELETE FROM steps WHERE project_id = ? AND task_id = ?",
        "DELETE FROM task_deps WHERE project_id = ? AND task_id = ?",
    ] {
        sqlx::query(sql)
            .bind(project)
            .bind(&t.id)
            .execute(&mut **tx)
            .await?;
    }
    for (idx, s) in t.steps.iter().enumerate() {
        insert_step(tx, project, &t.id, idx, s).await?;
    }
    for (ord, dep) in t.depends_on.iter().enumerate() {
        sqlx::query(
            "INSERT INTO task_deps (project_id, task_id, ord, depends_on) VALUES (?, ?, ?, ?)",
        )
        .bind(project)
        .bind(&t.id)
        .bind(int(ord)?)
        .bind(dep)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

async fn insert_step(
    tx: &mut Tx,
    project: &str,
    task: &str,
    idx: usize,
    s: &Step,
) -> Result<(), StoreError> {
    sqlx::query(
        "INSERT INTO steps (project_id, task_id, idx, kind, instruction, status, result, verdict, \
         note, nudges) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(project)
    .bind(task)
    .bind(int(idx)?)
    .bind(name(&s.kind)?)
    .bind(&s.instruction)
    .bind(name(&s.status)?)
    .bind(&s.result)
    .bind(s.verdict.as_ref().map(name).transpose()?)
    .bind(&s.note)
    .bind(i64::from(s.nudges))
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn upsert_help(tx: &mut Tx, project: &str, ord: usize, h: &Help) -> Result<(), StoreError> {
    sqlx::query(
        "INSERT OR REPLACE INTO helps (project_id, id, ord, task_id, step, kind, message, source, \
         state, agent_lost, reply) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(project)
    .bind(&h.id)
    .bind(int(ord)?)
    .bind(&h.task)
    .bind(int(h.step)?)
    .bind(name(&h.kind)?)
    .bind(&h.message)
    .bind(json(&h.source)?)
    .bind(name(&h.state)?)
    .bind(i64::from(h.agent_lost))
    .bind(&h.reply)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn write_inbox(
    tx: &mut Tx,
    project: &str,
    before: &[InboxEntry],
    after: &[InboxEntry],
) -> Result<(), StoreError> {
    let kept: HashSet<u64> = after.iter().map(|e| e.id).collect();
    let old: HashSet<u64> = before.iter().map(|e| e.id).collect();
    for gone in before.iter().filter(|e| !kept.contains(&e.id)) {
        sqlx::query("DELETE FROM inbox WHERE project_id = ? AND id = ?")
            .bind(project)
            .bind(int(gone.id)?)
            .execute(&mut **tx)
            .await?;
    }
    for new in after.iter().filter(|e| !old.contains(&e.id)) {
        sqlx::query("INSERT INTO inbox (project_id, id, kind, attrs, body) VALUES (?, ?, ?, ?, ?)")
            .bind(project)
            .bind(int(new.id)?)
            .bind(name(&new.item.kind)?)
            .bind(json(&new.item.attrs)?)
            .bind(&new.item.body)
            .execute(&mut **tx)
            .await?;
    }
    Ok(())
}

#[derive(FromRow)]
struct ProjectRow {
    config: String,
    counters: String,
}

#[derive(FromRow)]
struct GroupRow {
    id: String,
    title: String,
    summary: Option<String>,
    base_branch: String,
    group_branch: String,
    status: String,
    finish_summary: Option<String>,
    detail: Option<String>,
    finish_nudges: i64,
}

#[derive(FromRow)]
struct TaskRow {
    id: String,
    group_id: String,
    title: String,
    kind: String,
    instruction: Option<String>,
    current_step: i64,
    status: String,
    review_rounds: i64,
    workdir: Option<String>,
    cancel_reason: Option<String>,
}

#[derive(FromRow)]
struct StepRow {
    task_id: String,
    kind: String,
    instruction: Option<String>,
    status: String,
    result: Option<String>,
    verdict: Option<String>,
    note: Option<String>,
    nudges: i64,
}

#[derive(FromRow)]
struct HelpRow {
    id: String,
    task_id: String,
    step: i64,
    kind: String,
    message: String,
    source: String,
    state: String,
    agent_lost: i64,
    reply: Option<String>,
}

#[derive(FromRow)]
struct InboxRow {
    id: i64,
    kind: String,
    attrs: String,
    body: String,
}

/// Reads the whole state of `project`.
pub(super) async fn load(
    conn: &mut SqliteConnection,
    project: &str,
) -> Result<Option<State>, StoreError> {
    let Some(p) =
        sqlx::query_as::<_, ProjectRow>("SELECT config, counters FROM projects WHERE id = ?")
            .bind(project)
            .fetch_optional(&mut *conn)
            .await?
    else {
        return Ok(None);
    };
    let mut state = State::new(project, from_json(&p.config)?);
    state.counters = from_json(&p.counters)?;
    state.groups = load_groups(conn, project).await?;
    state.tasks = load_tasks(conn, project).await?;
    state.helps = load_helps(conn, project).await?;
    state.inbox = load_inbox(conn, project).await?;
    Ok(Some(state))
}

async fn load_groups(conn: &mut SqliteConnection, project: &str) -> Result<Vec<Group>, StoreError> {
    let rows = sqlx::query_as::<_, GroupRow>(
        "SELECT id, title, summary, base_branch, group_branch, status, finish_summary, detail, \
         finish_nudges FROM task_groups WHERE project_id = ? ORDER BY ord",
    )
    .bind(project)
    .fetch_all(&mut *conn)
    .await?;
    rows.into_iter()
        .map(|r| {
            Ok(Group {
                id: r.id,
                title: r.title,
                summary: r.summary,
                base_branch: r.base_branch,
                group_branch: r.group_branch,
                status: parse(&r.status)?,
                finish_summary: r.finish_summary,
                detail: r.detail,
                finish_nudges: uint(r.finish_nudges)?,
            })
        })
        .collect()
}

async fn load_tasks(conn: &mut SqliteConnection, project: &str) -> Result<Vec<Task>, StoreError> {
    let mut steps = load_steps(conn, project).await?;
    let mut deps = load_deps(conn, project).await?;
    let rows = sqlx::query_as::<_, TaskRow>(
        "SELECT id, group_id, title, kind, instruction, current_step, status, review_rounds, \
         workdir, cancel_reason FROM tasks WHERE project_id = ? ORDER BY ord",
    )
    .bind(project)
    .fetch_all(&mut *conn)
    .await?;
    rows.into_iter()
        .map(|r| {
            Ok(Task {
                steps: steps.remove(&r.id).unwrap_or_default(),
                depends_on: deps.remove(&r.id).unwrap_or_default(),
                id: r.id,
                group: r.group_id,
                title: r.title,
                kind: parse(&r.kind)?,
                instruction: r.instruction,
                current: uint(r.current_step)?,
                status: parse(&r.status)?,
                review_rounds: uint(r.review_rounds)?,
                workdir: r.workdir.map(PathBuf::from),
                cancel_reason: r.cancel_reason,
            })
        })
        .collect()
}

async fn load_steps(
    conn: &mut SqliteConnection,
    project: &str,
) -> Result<HashMap<String, Vec<Step>>, StoreError> {
    let rows = sqlx::query_as::<_, StepRow>(
        "SELECT task_id, kind, instruction, status, result, verdict, note, nudges \
         FROM steps WHERE project_id = ? ORDER BY task_id, idx",
    )
    .bind(project)
    .fetch_all(&mut *conn)
    .await?;
    let mut by_task: HashMap<String, Vec<Step>> = HashMap::new();
    for r in rows {
        let step = Step {
            kind: parse(&r.kind)?,
            instruction: r.instruction,
            status: parse(&r.status)?,
            result: r.result,
            verdict: parse_opt(r.verdict.as_deref())?,
            note: r.note,
            nudges: uint(r.nudges)?,
        };
        by_task.entry(r.task_id).or_default().push(step);
    }
    Ok(by_task)
}

async fn load_deps(
    conn: &mut SqliteConnection,
    project: &str,
) -> Result<HashMap<String, Vec<String>>, StoreError> {
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT task_id, depends_on FROM task_deps WHERE project_id = ? ORDER BY task_id, ord",
    )
    .bind(project)
    .fetch_all(&mut *conn)
    .await?;
    let mut by_task: HashMap<String, Vec<String>> = HashMap::new();
    for (task, dep) in rows {
        by_task.entry(task).or_default().push(dep);
    }
    Ok(by_task)
}

async fn load_helps(conn: &mut SqliteConnection, project: &str) -> Result<Vec<Help>, StoreError> {
    let rows = sqlx::query_as::<_, HelpRow>(
        "SELECT id, task_id, step, kind, message, source, state, agent_lost, reply \
         FROM helps WHERE project_id = ? ORDER BY ord",
    )
    .bind(project)
    .fetch_all(&mut *conn)
    .await?;
    rows.into_iter()
        .map(|r| {
            Ok(Help {
                id: r.id,
                task: r.task_id,
                step: uint(r.step)?,
                kind: parse(&r.kind)?,
                message: r.message,
                source: from_json(&r.source)?,
                state: parse(&r.state)?,
                agent_lost: r.agent_lost != 0,
                reply: r.reply,
            })
        })
        .collect()
}

async fn load_inbox(
    conn: &mut SqliteConnection,
    project: &str,
) -> Result<Vec<InboxEntry>, StoreError> {
    let rows = sqlx::query_as::<_, InboxRow>(
        "SELECT id, kind, attrs, body FROM inbox WHERE project_id = ? ORDER BY id",
    )
    .bind(project)
    .fetch_all(&mut *conn)
    .await?;
    rows.into_iter()
        .map(|r| {
            Ok(InboxEntry {
                id: uint(r.id)?,
                item: InboxItem {
                    kind: parse(&r.kind)?,
                    attrs: from_json(&r.attrs)?,
                    body: r.body,
                },
            })
        })
        .collect()
}
