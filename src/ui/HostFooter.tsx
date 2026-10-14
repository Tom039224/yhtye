import { useState } from "react";

import type { ClientDevice } from "../api/generated";
import { useAppState } from "../store/useStore";
import { CLIENTS_SHOWN, clientKindIcon, clientKindLabel, clientState, formatPingAge, hostTone, hostToneLabel, orderClients, pingAgeSeconds } from "./hostStatus";
import { Icon } from "./Icon";
import { useNow } from "./useNow";

/**
 * The bottom of the sidebar (design §3.3): the machine the core runs on, then
 * the paired devices. The icons tell the server from the devices.
 */
export function HostFooter() {
  return (
    <div className="side-footer host-footer">
      <ServerRow />
      <ClientRows />
    </div>
  );
}

/**
 * The core's host (its host name, read by the core), a dot for the connection
 * to it, and how long ago the core last answered a ping ("now" up to 5 s).
 */
function ServerRow() {
  const connection = useAppState((s) => s.connection.state);
  const host = useAppState((s) => s.host);
  const now = useNow(1000);
  const age = pingAgeSeconds(now, host.lastPingAt);
  const tone = hostTone(connection, age);
  const state = hostToneLabel(tone, connection);
  const name = host.name ?? "—";
  return (
    <div className="host-row" data-testid="host" title={`${name} · ${state}`}>
      <Icon name="server" size={12} className="client-kind" />
      <span className={`dot dot-${tone}`} role="img" aria-label={state} />
      <span className="host">{name}</span>
      <span className="spacer" />
      <span className="host-sync" title={age === null ? "まだ応答がありません" : "最後の ping からの経過"}>
        <Icon name="clock" size={11} />
        {age === null ? "—" : formatPingAge(age)}
      </span>
    </div>
  );
}

/**
 * The paired devices, connected ones first; offline ones are greyed out. Past
 * {@link CLIENTS_SHOWN} the rest fold into "他 N 台" (unfolded, the list
 * scrolls in a box of its own so the sidebar keeps its room). Nothing without devices.
 */
function ClientRows() {
  const devices = useAppState((s) => s.clients);
  const [expanded, setExpanded] = useState(false);
  if (devices.length === 0) return null;
  const ordered = orderClients(devices);
  const hidden = ordered.length - CLIENTS_SHOWN;
  const shown = expanded || hidden <= 0 ? ordered : ordered.slice(0, CLIENTS_SHOWN);
  return (
    <>
      <ul className={`client-list${expanded ? " expanded" : ""}`} aria-label="クライアント">
        {shown.map((d) => (
          <ClientRow key={d.id} device={d} />
        ))}
      </ul>
      {hidden > 0 && (
        <button type="button" className="client-more" aria-expanded={expanded} onClick={() => setExpanded(!expanded)}>
          <Icon name="chevron-down" size={11} className={expanded ? "caret open" : "caret"} />
          {expanded ? "折りたたむ" : `他 ${hidden} 台`}
        </button>
      )}
    </>
  );
}

function ClientRow({ device }: { device: ClientDevice }) {
  const state = clientState(device);
  return (
    <li
      className={`client-row${device.connected ? "" : " offline"}`}
      aria-label={`${device.name} · ${state}`}
      title={`${device.name} · ${clientKindLabel(device.kind)} · ${state}`}
    >
      <Icon name={clientKindIcon(device.kind)} size={12} className="client-kind" />
      <span className={`dot${device.connected ? " dot-ok" : ""}`} aria-hidden="true" />
      <span className="client-name">{device.name}</span>
    </li>
  );
}
