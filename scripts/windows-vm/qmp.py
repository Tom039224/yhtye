#!/usr/bin/env python3
"""Minimal QMP client for the Windows 11 test VM (stdlib only).

Usage:
  qmp.py SOCKET COMMAND [JSON_ARGUMENTS]   run one QMP command, print the return value as JSON
  qmp.py SOCKET hmp 'sendkey ctrl-alt-delete'   run a human monitor command
  qmp.py SOCKET spam-key KEY SECONDS        press KEY every 0.5 s for SECONDS (e.g. the
                                            "Press any key to boot from CD" prompt)
"""

import json
import socket
import sys
import time


class Qmp:
    def __init__(self, path):
        self.sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.sock.connect(path)
        self.reader = self.sock.makefile("r", encoding="utf-8")
        self._read_reply()  # greeting
        self.execute("qmp_capabilities")

    def _read_reply(self):
        while True:
            line = self.reader.readline()
            if not line:
                raise ConnectionError("QMP socket closed")
            msg = json.loads(line)
            if "event" not in msg:
                return msg

    def execute(self, command, arguments=None):
        req = {"execute": command}
        if arguments:
            req["arguments"] = arguments
        self.sock.sendall((json.dumps(req) + "\n").encode())
        reply = self._read_reply()
        if "error" in reply:
            raise RuntimeError(f"{command}: {reply['error'].get('desc', reply['error'])}")
        return reply.get("return")


def main(argv):
    if len(argv) < 3:
        print(__doc__, file=sys.stderr)
        return 2
    qmp = Qmp(argv[1])
    command = argv[2]
    if command == "hmp":
        out = qmp.execute("human-monitor-command", {"command-line": " ".join(argv[3:])})
        if out:
            print(out, end="")
        return 0
    if command == "spam-key":
        key, seconds = argv[3], float(argv[4])
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            qmp.execute("send-key", {"keys": [{"type": "qcode", "data": key}]})
            time.sleep(0.5)
        return 0
    arguments = json.loads(argv[3]) if len(argv) > 3 else None
    result = qmp.execute(command, arguments)
    if result not in (None, {}):
        print(json.dumps(result, indent=2, ensure_ascii=False))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
