// What the UI says of the other Yhtye hosts this app is a client of (T-98): how a
// host's connection reads in words, and the words for what can go wrong when
// pairing with one. The shell and the core answer in English; the wording of the
// screen is picked here. `src-tauri/src/remote_hosts/tests.rs` pins the English
// starts of the messages that {@link describePairError} tells apart.

import type { HostCompatibility, HostToken, RemoteHostInfo } from "../api/remoteHosts";
import type { CommandError } from "../api/transport";
import type { HostEntry } from "../store/hosts";

/** A remote host's connection in words; `down`: it cannot be used now (greyed out). */
export interface RemoteState {
  text: string;
  down: boolean;
}

/** The state of a remote host: its connection as the shell reports it, and whether its store hears it. */
export function remoteState(entry: Pick<HostEntry, "info" | "connection">): RemoteState {
  const connection = entry.info?.status.connection;
  switch (connection?.state) {
    case "connected":
      if (entry.connection === "open") return { text: "接続中", down: false };
      return entry.connection === "connecting" ? { text: "接続を確認中", down: true } : { text: "切断", down: true };
    case "connecting":
      return { text: "接続しています…", down: true };
    case "waiting_to_reconnect":
      return { text: "切断 (再接続待ち)", down: true };
    case "revoked":
      return { text: REVOKED_TEXT, down: true };
    case "rejected":
      return { text: "接続先に受け付けられていません", down: true };
    case "idle":
    case undefined:
      return { text: "切断", down: true };
  }
}

/** What a host that removed this device is said to have done. */
export const REVOKED_TEXT = "このホストから外されました";

/** `host name · state`: the tooltip of a remote host's mark. */
export function remoteTitle(entry: Pick<HostEntry, "name" | "key" | "info" | "connection">): string {
  return `${entry.name ?? entry.key} · ${remoteState(entry).text}`;
}

/** A remote host's address as it is shown: the first host and the port. */
export function remoteAddress(info: Pick<RemoteHostInfo, "hosts" | "port">): string {
  return `${info.hosts[0] ?? "?"}:${info.port}`;
}

/** What to tell of the device token of a host, if anything is amiss. */
export function tokenNote(token: HostToken): string | null {
  switch (token.place) {
    case "keyring":
      return null;
    case "memory_only":
      return "キーリングが使えないため、この端末のトークンを保存できませんでした。Yhtye を再起動すると、このホストにはペアリングし直す必要があります";
    case "missing":
      return "この端末のトークンが失われています (キーリングが使えず、再起動したため)。ペアリングし直してください";
  }
}

/** A warning about a host whose version does not fit this app's, if there is one. */
export function compatibilityNote(compatibility: HostCompatibility | null): string | null {
  if (compatibility?.type !== "incompatible") return null;
  return `バージョンが合いません (接続先 ${compatibility.host} / この PC のアプリ ${compatibility.app})。どちらかを更新してください`;
}

/** Why pairing (or reading what a person pasted or typed) failed. */
export interface PairProblem {
  /** What it is and what to do, in the screen's words. */
  text: string;
  /** The English reason as the shell gave it, when it adds something to `text` (shown small). */
  detail?: string;
}

const NOT_TAILSCALE = /^(.*) is not a Tailscale address$/s;

/**
 * The words for an error of pairing, by what the core and the shell say
 * (`PairingFailure`, `InvalidPairingInput`, `HostsError`: the code and the
 * start of the English message). An error not told apart shows as it is.
 */
export function describePairError(e: CommandError): PairProblem {
  const message = e.message;
  const starts = (text: string) => message.startsWith(text);
  switch (e.code) {
    case "forbidden":
      if (starts("the pairing code was refused")) {
        return {
          text: "コードが違うか、有効期限が切れたか、すでに使われています。接続先で新しいコードを作ってください (コードは 5 分間・1 回だけ使えます)",
          detail: noteOf(message),
        };
      }
      if (starts("the host does not take devices of other Tailscale users")) {
        return {
          text: "接続先がこの PC を受け付けませんでした。同じ Tailscale ユーザーの端末でない (別のアカウントでログインしている)、またはタグ付きノードの可能性があります",
          detail: noteOf(message),
        };
      }
      return { text: "接続先に拒否されました", detail: message };
    case "conflict":
      if (starts("another Yhtye answered")) {
        return {
          text: "リンクの接続先とは別の Yhtye が答えました (アドレスが違うようです)。その Yhtye がこの PC を登録してしまった場合は、そちらの端末一覧から外してください",
          detail: message,
        };
      }
      return { text: message };
    case "unavailable":
      if (starts("could not reach")) {
        return {
          text: "接続先に届きません。この PC で Tailscale が動いているか、接続先の Yhtye が動いていて「リモート接続」がオンか、アドレスとポートが合っているか確認してください",
          detail: message,
        };
      }
      if (starts("the host answered with an error")) {
        return { text: "接続先がエラーを返しました。接続先の Yhtye の状態を確認してください", detail: message };
      }
      return { text: message };
    case "invalid_argument":
      return invalidInput(message);
    default:
      return { text: message };
  }
}

/** The host's own note after a `: ` in an English message (`the pairing code was refused: expired`). */
function noteOf(message: string): string | undefined {
  const at = message.indexOf(": ");
  return at < 0 ? undefined : message.slice(at + 2);
}

function invalidInput(message: string): PairProblem {
  const notTailscale = NOT_TAILSCALE.exec(message);
  if (notTailscale) {
    return {
      text: `${notTailscale[1]} は Tailscale のアドレスではありません。Tailscale の IP アドレス (100.x.y.z など) か、MagicDNS の名前 (…ts.net) を指定してください`,
    };
  }
  if (message.startsWith("this is not a Yhtye pairing code")) {
    return { text: "Yhtye のペアリングリンクではありません (yhtye://pair?… の形のもの)" };
  }
  if (message.startsWith("this pairing code is of a newer format")) {
    return { text: "新しい形式のペアリングコードです。この PC の Yhtye を更新してください", detail: message };
  }
  if (message.startsWith("no host was given")) return { text: "ホストを入力してください" };
  if (message.startsWith("the port must be a number")) return { text: "ポートは 1〜65535 の数字で入力してください" };
  if (message.startsWith("no pairing code was given")) return { text: "コードを入力してください" };
  if (message.startsWith("the pairing code must be")) {
    return { text: "コードは英数字 8 文字です (0・O・1・I・L は使われません)。ハイフンや空白は入れても構いません" };
  }
  if (message.startsWith("the host did not accept the request")) {
    return { text: "接続先がこの要求を受け付けませんでした (端末名が長すぎるなど)", detail: noteOf(message) };
  }
  return { text: message };
}
