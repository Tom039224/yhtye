//! The control socket of `yhtye --headless` (`core-design.md` §9.4): a Unix
//! domain socket on which a running headless instance answers the commands of
//! `yhtye remote …`. The pairing code lives in the process's memory and
//! revoking a device closes its connections, so something has to ask the
//! process that runs.
//!
//! One JSON line per request: an [`ApiCommand`], or [`SHUTDOWN_REQUEST`] (stop
//! the instance); one per reply: `{"ok": ApiResponse}` (`{"ok": {"type":
//! "stopping"}}` for the shutdown) or `{"err": ApiError}` ([`Reply`]). The
//! socket's permissions (0600) and a check of the peer's user are the
//! authentication, so every command is allowed, the local-only ones too
//! ([`ApiCommand::is_local_only`]: the same as `SessionPolicy::LOCAL`).

use std::io::{self, BufRead, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::os::unix::net::UnixStream as StdUnixStream;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;
use yhtye_core::api::{ApiCommand, ApiError, ApiResponse};
use yhtye_core::remote::{PairingCode, RemoteDevice, RemoteStatus};
use yhtye_core::runtime::Core;

/// Name of the socket in the data directory.
pub const SOCKET_FILE: &str = "yhtye.sock";

/// The longest socket path used as it is. `sun_path` holds 108 bytes on Linux
/// and 104 on macOS (with the NUL); a data directory deeper than this gets its
/// socket elsewhere ([`socket_path_with`]).
const MAX_SOCKET_PATH_BYTES: usize = 100;

/// The longest request line taken (a command with a secret value is the
/// largest; this is far above it).
const MAX_LINE_BYTES: usize = 1 << 20;

/// A connection that sends nothing for this long is closed.
const IDLE_TIMEOUT: Duration = Duration::from_secs(300);

/// How long the client waits for an answer.
const CLIENT_TIMEOUT: Duration = Duration::from_secs(30);

/// The socket of the headless instance on `data_dir`: `<data dir>/yhtye.sock`,
/// else (the path is too long) `$XDG_RUNTIME_DIR/yhtye-<hash>.sock`. The server
/// and the clients both call this, so they agree.
#[must_use]
pub fn socket_path(data_dir: &Path) -> PathBuf {
    let runtime_dir = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from);
    socket_path_with(
        data_dir,
        runtime_dir.as_deref(),
        &std::env::temp_dir(),
        rustix::process::geteuid().as_raw(),
    )
}

/// [`socket_path`] with its inputs given.
#[must_use]
pub fn socket_path_with(
    data_dir: &Path,
    runtime_dir: Option<&Path>,
    temp_dir: &Path,
    uid: u32,
) -> PathBuf {
    let fits = |p: &Path| p.as_os_str().len() <= MAX_SOCKET_PATH_BYTES;
    let direct = data_dir.join(SOCKET_FILE);
    if fits(&direct) {
        return direct;
    }
    let hash = short_hash(data_dir);
    if let Some(dir) = runtime_dir.filter(|d| d.is_absolute()) {
        let path = dir.join(format!("yhtye-{hash}.sock"));
        if fits(&path) {
            return path;
        }
    }
    // A shared directory: the user id keeps users' sockets apart.
    temp_dir.join(format!("yhtye-{uid}-{hash}.sock"))
}

/// The first 8 bytes of the SHA-256 of the path, in hex.
fn short_hash(path: &Path) -> String {
    let digest = Sha256::digest(path.as_os_str().as_bytes());
    digest[..8].iter().map(|b| format!("{b:02x}")).collect()
}

/// The request line that stops the instance: not an [`ApiCommand`] (the core has
/// no such command; it is the process that stops).
pub const SHUTDOWN_REQUEST: &str = r#"{"type":"shutdown"}"#;

/// What a request line asks for.
enum Request {
    Shutdown,
    Command(ApiCommand),
}

fn parse_request(line: &str) -> Result<Request, serde_json::Error> {
    // Exactly the shutdown request, nothing else in it.
    let shutdown = serde_json::json!({"type": "shutdown"});
    if serde_json::from_str::<serde_json::Value>(line).is_ok_and(|v| v == shutdown) {
        return Ok(Request::Shutdown);
    }
    serde_json::from_str(line).map(Request::Command)
}

/// What a request is answered with.
#[allow(clippy::large_enum_variant)] // like `ApiResponse`: short-lived, written at once
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum Answer {
    Api(ApiResponse),
    /// The instance is stopping (it answers before it has stopped).
    Stopping {
        #[serde(rename = "type")]
        kind: &'static str,
    },
}

/// The reply line: `{"ok": …}` or `{"err": …}`.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reply<T> {
    Ok(T),
    Err(ApiError),
}

// ---------------------------------------------------------------- the server

/// Starts listening on `path`: a socket file left by an earlier run (its
/// process is gone: the caller holds the data directory's lock) is removed
/// first, and the new one is made readable and writable by this user only.
pub fn bind(path: &Path) -> io::Result<UnixListener> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_socket() => std::fs::remove_file(path)?,
        Ok(_) => {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("{} exists and is not a socket", path.display()),
            ));
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    let listener = UnixListener::bind(path)?;
    if let Err(e) = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)) {
        let _ = std::fs::remove_file(path);
        return Err(e);
    }
    Ok(listener)
}

/// Answers connections until `cancel` fires. A [`SHUTDOWN_REQUEST`] notifies `stop`.
pub async fn serve(
    listener: UnixListener,
    core: Core,
    stop: Arc<Notify>,
    cancel: CancellationToken,
) {
    loop {
        let stream = tokio::select! {
            () = cancel.cancelled() => return,
            accepted = listener.accept() => match accepted {
                Ok((stream, _)) => stream,
                Err(e) => {
                    tracing::warn!("control socket: accept failed: {e}");
                    // Not a busy loop when the error stays (no descriptors left).
                    tokio::time::sleep(Duration::from_millis(200)).await;
                    continue;
                }
            },
        };
        let core = core.clone();
        let stop = stop.clone();
        let cancel = cancel.child_token();
        tokio::spawn(async move {
            if let Err(e) = handle_connection(stream, &core, &stop, &cancel).await {
                tracing::debug!("control socket: connection ended: {e}");
            }
        });
    }
}

/// Only this user (and root) may talk to the socket, whatever its mode.
fn peer_is_allowed(stream: &UnixStream) -> io::Result<bool> {
    let uid = stream.peer_cred()?.uid();
    Ok(uid == 0 || uid == rustix::process::geteuid().as_raw())
}

async fn handle_connection(
    stream: UnixStream,
    core: &Core,
    stop: &Notify,
    cancel: &CancellationToken,
) -> io::Result<()> {
    if !peer_is_allowed(&stream)? {
        tracing::warn!("control socket: refused a connection of another user");
        return Ok(());
    }
    let (read, mut write) = stream.into_split();
    let mut read = BufReader::new(read);
    loop {
        let line = tokio::select! {
            () = cancel.cancelled() => return Ok(()),
            line = tokio::time::timeout(IDLE_TIMEOUT, read_line(&mut read, MAX_LINE_BYTES)) => line,
        };
        let Some(line) = line.map_err(|_| io::ErrorKind::TimedOut)?? else {
            return Ok(());
        };
        let mut reply = serde_json::to_string(&respond(core, stop, &line).await)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        reply.push('\n');
        write.write_all(reply.as_bytes()).await?;
    }
}

/// Runs one request line on the core (or asks the instance to stop).
pub async fn respond(core: &Core, stop: &Notify, line: &str) -> Reply<Answer> {
    // The line is not logged: the command may carry a secret value.
    let cmd = match parse_request(line) {
        Ok(Request::Command(cmd)) => cmd,
        Ok(Request::Shutdown) => {
            stop.notify_one();
            return Reply::Ok(Answer::Stopping { kind: "stopping" });
        }
        Err(e) => return Reply::Err(ApiError::invalid_argument(format!("bad command: {e}"))),
    };
    match core.command(cmd).await {
        Ok(ok) => Reply::Ok(Answer::Api(ok)),
        Err(err) => Reply::Err(err),
    }
}

/// The next line (without its newline), `None` at the end of the stream; a
/// line longer than `max` bytes is an error (the memory is bounded).
async fn read_line<R: AsyncBufRead + Unpin>(
    reader: &mut R,
    max: usize,
) -> io::Result<Option<String>> {
    let mut line = Vec::new();
    loop {
        let chunk = reader.fill_buf().await?;
        if chunk.is_empty() {
            if line.is_empty() {
                return Ok(None);
            }
            break;
        }
        let end = chunk.iter().position(|b| *b == b'\n');
        let take = end.unwrap_or(chunk.len());
        if line.len() + take > max {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "request line too long",
            ));
        }
        line.extend_from_slice(&chunk[..take]);
        reader.consume(end.map_or(take, |i| i + 1));
        if end.is_some() {
            break;
        }
    }
    String::from_utf8(line)
        .map(Some)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

// ---------------------------------------------------------------- the client

/// What a thin client reads of an [`ApiResponse`]: the answers it asks for.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Response {
    RemoteStatus {
        status: RemoteStatus,
    },
    PairingCode {
        pairing: PairingCode,
    },
    RemoteDevices {
        devices: Vec<RemoteDevice>,
    },
    /// The answer to [`SHUTDOWN_REQUEST`].
    Stopping,
    #[serde(other)]
    Other,
}

#[derive(Debug)]
pub enum ClientError {
    /// Nothing listens on the socket: no headless instance on this data directory.
    NotRunning(PathBuf),
    /// The instance answered with an error.
    Api(ApiError),
    Io(io::Error),
    /// The answer was not what the protocol says.
    Protocol(String),
}

impl std::fmt::Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotRunning(path) => write!(
                f,
                "no headless Yhtye answers on {} (start one with `yhtye --headless`; \
                 a desktop Yhtye has no control socket, use its settings panel; \
                 pass --data-dir if it runs on another data directory)",
                path.display()
            ),
            Self::Api(e) => f.write_str(&e.message),
            Self::Io(e) => write!(f, "control socket: {e}"),
            Self::Protocol(e) => write!(f, "control socket: unexpected answer: {e}"),
        }
    }
}

impl std::error::Error for ClientError {}

impl From<io::Error> for ClientError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

/// A connection to a running headless instance (blocking: the clients have no
/// reason to start a runtime).
pub struct Client {
    reader: io::BufReader<StdUnixStream>,
    writer: StdUnixStream,
}

impl Client {
    pub fn connect(path: &Path) -> Result<Self, ClientError> {
        let writer = StdUnixStream::connect(path).map_err(|e| match e.kind() {
            // Absent, or left over by a process that is gone.
            io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused => {
                ClientError::NotRunning(path.to_path_buf())
            }
            _ => ClientError::Io(e),
        })?;
        writer.set_read_timeout(Some(CLIENT_TIMEOUT))?;
        writer.set_write_timeout(Some(CLIENT_TIMEOUT))?;
        let reader = io::BufReader::new(writer.try_clone()?);
        Ok(Self { reader, writer })
    }

    /// The process id of the instance that listens on the socket (the kernel
    /// tells it: `SO_PEERCRED`); `None` where the system cannot say.
    #[must_use]
    pub fn server_pid(&self) -> Option<u32> {
        #[cfg(any(target_os = "linux", target_os = "android"))]
        {
            let cred = rustix::net::sockopt::socket_peercred(&self.writer).ok()?;
            u32::try_from(cred.pid.as_raw_nonzero().get()).ok()
        }
        #[cfg(not(any(target_os = "linux", target_os = "android")))]
        {
            None
        }
    }

    /// Runs `cmd` and returns its answer.
    pub fn call(&mut self, cmd: &ApiCommand) -> Result<Response, ClientError> {
        let line = serde_json::to_string(cmd).map_err(|e| ClientError::Protocol(e.to_string()))?;
        self.send_line(&line)
    }

    /// Asks the instance to stop. It answers at once, before the agents have
    /// stopped (watch [`Client::server_pid`] for that).
    pub fn shutdown(&mut self) -> Result<(), ClientError> {
        match self.send_line(SHUTDOWN_REQUEST)? {
            Response::Stopping => Ok(()),
            _ => Err(ClientError::Protocol("not the answer of shutdown".into())),
        }
    }

    fn send_line(&mut self, request: &str) -> Result<Response, ClientError> {
        let mut line = request.to_string();
        line.push('\n');
        self.writer.write_all(line.as_bytes())?;
        let mut reply = String::new();
        if self.reader.read_line(&mut reply)? == 0 {
            return Err(ClientError::Protocol(
                "the connection was closed (Yhtye is stopping?)".into(),
            ));
        }
        match serde_json::from_str::<Reply<Response>>(&reply) {
            Ok(Reply::Ok(ok)) => Ok(ok),
            Ok(Reply::Err(err)) => Err(ClientError::Api(err)),
            Err(e) => Err(ClientError::Protocol(e.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::json;
    use yhtye_core::api::ApiErrorCode;
    use yhtye_core::runtime::CoreConfig;
    use yhtye_core::secrets::MemoryBackend;

    use super::*;

    #[test]
    fn the_socket_lies_in_the_data_directory() {
        let path = socket_path_with(Path::new("/data/yhtye"), None, Path::new("/tmp"), 1000);
        assert_eq!(path, Path::new("/data/yhtye/yhtye.sock"));
    }

    #[test]
    fn a_data_directory_too_deep_for_a_socket_path_gets_one_under_the_runtime_directory() {
        let deep = PathBuf::from(format!("/data/{}", "d".repeat(120)));
        let runtime = Path::new("/run/user/1000");
        let path = socket_path_with(&deep, Some(runtime), Path::new("/tmp"), 1000);
        assert_eq!(path.parent(), Some(runtime));
        let name = path.file_name().and_then(|n| n.to_str()).expect("name");
        assert!(
            name.starts_with("yhtye-") && name.ends_with(".sock"),
            "{name}"
        );
        assert!(path.as_os_str().len() <= MAX_SOCKET_PATH_BYTES);
        // The same directory gives the same socket; another one, another socket.
        assert_eq!(
            path,
            socket_path_with(&deep, Some(runtime), Path::new("/tmp"), 1000)
        );
        let other = PathBuf::from(format!("/data/{}", "e".repeat(120)));
        assert_ne!(
            path,
            socket_path_with(&other, Some(runtime), Path::new("/tmp"), 1000)
        );
    }

    #[test]
    fn without_a_usable_runtime_directory_the_socket_goes_to_the_temp_directory_by_user() {
        let deep = PathBuf::from(format!("/data/{}", "d".repeat(120)));
        for runtime in [None, Some(Path::new("relative"))] {
            let path = socket_path_with(&deep, runtime, Path::new("/tmp"), 1000);
            assert_eq!(path.parent(), Some(Path::new("/tmp")));
            let name = path.file_name().and_then(|n| n.to_str()).expect("name");
            assert!(name.starts_with("yhtye-1000-"), "{name}");
        }
    }

    async fn lines(input: &[u8], max: usize) -> Vec<io::Result<Option<String>>> {
        let mut reader = BufReader::new(input);
        let mut out = Vec::new();
        loop {
            let line = read_line(&mut reader, max).await;
            let stop = !matches!(line, Ok(Some(_)));
            out.push(line);
            if stop {
                return out;
            }
        }
    }

    #[tokio::test]
    async fn lines_are_split_at_newlines_and_the_last_may_lack_one() {
        let got: Vec<_> = lines(b"one\ntwo\n\nthree", 100)
            .await
            .into_iter()
            .map(|l| l.expect("line"))
            .collect();
        let text = |s: &str| Some(s.to_string());
        assert_eq!(
            got,
            [text("one"), text("two"), text(""), text("three"), None]
        );
    }

    #[tokio::test]
    async fn a_line_over_the_limit_is_an_error_not_a_buffer() {
        let got = lines(b"0123456789\n", 5).await;
        assert!(got[0].is_err());
        // At the limit it still passes.
        let got = lines(b"01234\n", 5).await;
        assert_eq!(got[0].as_ref().expect("line").as_deref(), Some("01234"));
    }

    #[tokio::test]
    async fn bytes_that_are_not_text_are_an_error() {
        let got = lines(b"\xff\xfe\n", 100).await;
        assert!(got[0].is_err());
    }

    async fn test_core(dir: &Path) -> Core {
        let mut cfg = CoreConfig::claude_code(dir, "haiku");
        cfg.usage = None;
        cfg.headless = true;
        cfg.secret_backend = Arc::new(MemoryBackend::default());
        Core::start(cfg).await.expect("core")
    }

    struct Served {
        core: Core,
        stop: Arc<Notify>,
        cancel: CancellationToken,
        socket: PathBuf,
        _tmp: tempfile::TempDir,
    }

    async fn served() -> Served {
        let tmp = tempfile::tempdir().expect("tempdir");
        let core = test_core(tmp.path()).await;
        let socket = socket_path(tmp.path());
        let listener = bind(&socket).expect("bind");
        let cancel = CancellationToken::new();
        let stop = Arc::new(Notify::new());
        tokio::spawn(serve(listener, core.clone(), stop.clone(), cancel.clone()));
        Served {
            core,
            stop,
            cancel,
            socket,
            _tmp: tmp,
        }
    }

    impl Served {
        async fn finish(self) {
            self.cancel.cancel();
            self.core.shutdown().await;
        }
    }

    /// Sends raw lines on a connection and returns the lines it answers.
    async fn exchange(socket: PathBuf, requests: Vec<String>) -> Vec<String> {
        tokio::task::spawn_blocking(move || {
            let mut stream = StdUnixStream::connect(socket).expect("connect");
            stream
                .set_read_timeout(Some(Duration::from_secs(20)))
                .expect("timeout");
            let mut reader = io::BufReader::new(stream.try_clone().expect("clone"));
            let mut answers = Vec::new();
            for request in requests {
                stream
                    .write_all(format!("{request}\n").as_bytes())
                    .expect("write");
                let mut answer = String::new();
                if reader.read_line(&mut answer).expect("read") == 0 {
                    break;
                }
                answers.push(answer.trim_end().to_string());
            }
            answers
        })
        .await
        .expect("join")
    }

    #[tokio::test]
    async fn a_command_runs_on_the_core_and_its_answer_comes_back_as_one_line() {
        let s = served().await;
        let answers = exchange(s.socket.clone(), vec![r#"{"type":"ping"}"#.into()]).await;
        let reply: serde_json::Value = serde_json::from_str(&answers[0]).expect("json");
        assert_eq!(reply["ok"]["type"], "pong");
        assert_eq!(reply["ok"]["headless"], true, "{reply}");
        s.finish().await;
    }

    #[tokio::test]
    async fn local_only_commands_are_allowed() {
        let s = served().await;
        let answers = exchange(
            s.socket.clone(),
            vec![
                r#"{"type":"get_remote_status"}"#.into(),
                r#"{"type":"list_remote_devices"}"#.into(),
            ],
        )
        .await;
        let status: serde_json::Value = serde_json::from_str(&answers[0]).expect("json");
        assert_eq!(status["ok"]["type"], "remote_status");
        assert_eq!(status["ok"]["status"]["running"], false);
        let devices: serde_json::Value = serde_json::from_str(&answers[1]).expect("json");
        assert_eq!(
            devices["ok"],
            json!({"type": "remote_devices", "devices": []})
        );
        s.finish().await;
    }

    #[tokio::test]
    async fn a_bad_line_is_answered_with_an_error_and_the_connection_goes_on() {
        let s = served().await;
        let answers = exchange(
            s.socket.clone(),
            vec![
                "not json".into(),
                r#"{"type":"no_such_command"}"#.into(),
                r#"{"type":"revoke_remote_device","device":"nobody"}"#.into(),
                r#"{"type":"ping"}"#.into(),
            ],
        )
        .await;
        assert_eq!(answers.len(), 4);
        for answer in &answers[..2] {
            let reply: Reply<serde_json::Value> = serde_json::from_str(answer).expect("reply");
            let Reply::Err(err) = reply else {
                panic!("an error: {answer}");
            };
            assert_eq!(err.code, ApiErrorCode::InvalidArgument);
        }
        let reply: Reply<serde_json::Value> = serde_json::from_str(&answers[2]).expect("reply");
        let Reply::Err(err) = reply else {
            panic!("an error: {}", answers[2]);
        };
        assert_eq!(err.code, ApiErrorCode::NotFound);
        assert!(answers[3].contains("pong"), "{}", answers[3]);
        s.finish().await;
    }

    #[tokio::test]
    async fn the_socket_is_for_its_user_only() {
        let s = served().await;
        let mode = std::fs::metadata(&s.socket)
            .expect("meta")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
        s.finish().await;
    }

    #[tokio::test]
    async fn binding_replaces_a_stale_socket_but_not_another_kind_of_file() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join(SOCKET_FILE);
        // A socket left behind by a process that is gone.
        drop(bind(&path).expect("first bind"));
        assert!(path.exists());
        drop(bind(&path).expect("the stale socket is replaced"));
        // A regular file is somebody's data: refuse to remove it.
        std::fs::remove_file(&path).expect("remove");
        std::fs::write(&path, b"data").expect("write");
        let err = bind(&path).expect_err("a regular file");
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(std::fs::read(&path).expect("kept"), b"data");
    }

    #[tokio::test]
    async fn the_client_reads_what_the_server_answers() {
        let s = served().await;
        let socket = s.socket.clone();
        let (status, err) = tokio::task::spawn_blocking(move || {
            let mut client = Client::connect(&socket).expect("connect");
            let status = client.call(&ApiCommand::GetRemoteStatus).expect("status");
            // `create_pairing_code` fails while remote access is not running.
            let err = client
                .call(&ApiCommand::CreatePairingCode)
                .expect_err("not running");
            (status, err)
        })
        .await
        .expect("join");
        let Response::RemoteStatus { status } = status else {
            panic!("a status");
        };
        assert!(!status.running);
        let ClientError::Api(err) = err else {
            panic!("an api error: {err}");
        };
        assert_eq!(err.code, ApiErrorCode::InvalidState);
        s.finish().await;
    }

    #[tokio::test]
    async fn a_shutdown_request_is_answered_and_asks_the_instance_to_stop() {
        let s = served().await;
        let answers = exchange(
            s.socket.clone(),
            vec![SHUTDOWN_REQUEST.into(), r#"{"type":"ping"}"#.into()],
        )
        .await;
        let reply: serde_json::Value = serde_json::from_str(&answers[0]).expect("json");
        assert_eq!(reply, json!({"ok": {"type": "stopping"}}));
        tokio::time::timeout(Duration::from_secs(10), s.stop.notified())
            .await
            .expect("the instance was asked to stop");
        // Nothing else stops it: the connection is still served until `cancel`.
        assert!(answers[1].contains("pong"), "{}", answers[1]);
        s.finish().await;
    }

    #[tokio::test]
    async fn the_client_stops_the_instance_and_knows_its_pid() {
        let s = served().await;
        let socket = s.socket.clone();
        let pid = tokio::task::spawn_blocking(move || {
            let mut client = Client::connect(&socket).expect("connect");
            client.shutdown().expect("shutdown");
            client.server_pid()
        })
        .await
        .expect("join");
        tokio::time::timeout(Duration::from_secs(10), s.stop.notified())
            .await
            .expect("the instance was asked to stop");
        // The kernel names the listener: this very process (Linux).
        if cfg!(any(target_os = "linux", target_os = "android")) {
            assert_eq!(pid, Some(std::process::id()));
        }
        s.finish().await;
    }

    #[tokio::test]
    async fn a_request_that_only_looks_like_a_shutdown_is_an_error() {
        let s = served().await;
        let answers = exchange(
            s.socket.clone(),
            vec![
                r#"{"type":"shutdown","force":true}"#.into(),
                r#"{"type":"stop"}"#.into(),
            ],
        )
        .await;
        for answer in &answers {
            let reply: Reply<serde_json::Value> = serde_json::from_str(answer).expect("reply");
            assert!(matches!(reply, Reply::Err(_)), "{answer}");
        }
        assert!(
            tokio::time::timeout(Duration::from_millis(100), s.stop.notified())
                .await
                .is_err(),
            "nothing asked it to stop"
        );
        s.finish().await;
    }

    #[test]
    fn the_client_says_so_when_no_headless_instance_is_there() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join(SOCKET_FILE);
        let Err(err) = Client::connect(&path) else {
            panic!("no socket");
        };
        assert!(matches!(err, ClientError::NotRunning(_)));
        assert!(err.to_string().contains("yhtye --headless"));
        // A socket file nobody listens on (the process was killed).
        drop(std::os::unix::net::UnixListener::bind(&path).expect("bind"));
        let Err(err) = Client::connect(&path) else {
            panic!("nobody listens");
        };
        assert!(matches!(err, ClientError::NotRunning(_)));
    }
}
