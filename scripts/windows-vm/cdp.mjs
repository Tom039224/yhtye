#!/usr/bin/env node
// Evaluate JavaScript in the Yhtye window inside the Windows VM through the WebView2 DevTools protocol.
// The app must run with WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222 and the port
// forwarded to the host (`vm.sh ssh -N -L 9222:127.0.0.1:9222`; see README.md).
//   node cdp.mjs 'document.title'            print the value (promises are awaited) as JSON
//   node cdp.mjs --port 9223 'location.href'
// Needs Node 22+ (global WebSocket).

const args = process.argv.slice(2);
let port = 9222;
if (args[0] === "--port") {
  port = Number(args[1]);
  args.splice(0, 2);
}
const expression = args.join(" ");
if (!expression) {
  console.error("usage: cdp.mjs [--port N] EXPRESSION");
  process.exit(2);
}

const targets = await (await fetch(`http://127.0.0.1:${port}/json`)).json();
const page = targets.find((t) => t.type === "page");
if (!page) {
  console.error("no page target", targets);
  process.exit(1);
}

const ws = new WebSocket(page.webSocketDebuggerUrl);
await new Promise((resolve, reject) => {
  ws.onopen = resolve;
  ws.onerror = reject;
});
ws.onmessage = (event) => {
  const msg = JSON.parse(event.data);
  if (msg.id !== 1) return;
  ws.close();
  if (msg.error || msg.result.exceptionDetails) {
    console.error(JSON.stringify(msg.error ?? msg.result.exceptionDetails, null, 2));
    process.exit(1);
  }
  const value = msg.result.result.value;
  console.log(typeof value === "string" ? value : JSON.stringify(value, null, 2));
  process.exit(0);
};
ws.send(
  JSON.stringify({
    id: 1,
    method: "Runtime.evaluate",
    params: { expression, awaitPromise: true, returnByValue: true },
  }),
);
