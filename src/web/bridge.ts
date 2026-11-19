// Web build only (`npm run build:web`): answers Tauri's IPC from the coa-web backend over HTTP, so `src/lib/api.ts` and
// every screen run unchanged in a browser. Commands go to `POST /api/invoke/<command>`, events arrive from
// `GET /api/events` (Server-Sent Events). Same hook as `src/dev/mock.ts`: `window.__TAURI_INTERNALS__`.
export {};

type Callback = (value: unknown) => void;

const callbacks = new Map<number, Callback>();
const listeners = new Map<string, Set<number>>();
let nextId = 1;
let events: EventSource | null = null;
let loggingIn: Promise<void> | null = null;

function connectEvents() {
  if (events) return;
  events = new EventSource("/api/events", { withCredentials: true });
  events.onmessage = (m) => {
    let message: { event: string; payload: unknown };
    try { message = JSON.parse(m.data); } catch { return; }
    for (const id of listeners.get(message.event) ?? []) {
      callbacks.get(id)?.({ event: message.event, id, payload: message.payload });
    }
  };
  // EventSource reconnects by itself; after a 401 it gives up, so start over once logged in again.
  events.onerror = () => {
    if (events?.readyState === EventSource.CLOSED) { events = null; setTimeout(connectEvents, 3000); }
  };
}

async function login(): Promise<void> {
  loggingIn ??= (async () => {
    for (;;) {
      const token = window.prompt("CoA Server Manager: access token");
      if (token === null) throw { human: { title: "Not logged in", message: "The access token is needed.", actions: [] }, technical: "login cancelled", fields: [] };
      const r = await fetch("/api/login", {
        method: "POST",
        credentials: "same-origin",
        headers: { "content-type": "application/json", "x-coa-request": "1" },
        body: JSON.stringify({ token }),
      });
      if (r.ok) return;
      window.alert("Wrong token.");
    }
  })().finally(() => { loggingIn = null; });
  return loggingIn;
}

async function call(command: string, args: unknown): Promise<unknown> {
  const r = await fetch(`/api/invoke/${encodeURIComponent(command)}`, {
    method: "POST",
    credentials: "same-origin",
    headers: { "content-type": "application/json", "x-coa-request": "1" },
    body: JSON.stringify(args ?? {}),
  });
  if (r.status === 401) { await login(); return call(command, args); }
  const text = await r.text();
  let body: unknown = null;
  if (text) { try { body = JSON.parse(text); } catch { body = text; } }
  // The desktop app rejects with the command's error value (a UiError); do the same.
  if (!r.ok) throw body;
  return body;
}

let version: string | null = null;

async function plugin(command: string, args: any): Promise<unknown> {
  switch (command) {
    case "plugin:event|listen": {
      const set = listeners.get(args.event) ?? new Set<number>();
      set.add(args.handler);
      listeners.set(args.event, set);
      connectEvents();
      return args.handler;
    }
    case "plugin:event|unlisten":
      listeners.get(args.event)?.delete(args.eventId);
      return null;
    case "plugin:app|version":
      if (!version) version = (await (await fetch("/api/info", { credentials: "same-origin" })).json()).version;
      return version;
    case "plugin:app|name":
      return "CoA Server Manager";
    case "plugin:dialog|open": {
      // There is no file picker for the server's disk in a browser: ask for the path on the server.
      const o = args?.options ?? {};
      const path = window.prompt(o.title ?? (o.directory ? "Folder on the server" : "File on the server"), o.defaultPath ?? "");
      return path ? (o.multiple ? [path] : path) : null;
    }
    case "plugin:dialog|message":
      window.alert(args?.message ?? "");
      return null;
    case "plugin:dialog|ask":
    case "plugin:dialog|confirm":
      return window.confirm(args?.message ?? "");
    case "plugin:updater|check":
      return null; // the web service is updated with its host, not by the interface
    case "plugin:process|restart":
    case "plugin:process|exit":
      location.reload();
      return null;
    case "plugin:opener|open_url":
      window.open(args?.url, "_blank", "noopener");
      return null;
    default:
      // Window controls (size, title, tray) have no meaning in a browser tab.
      return null;
  }
}

(window as any).__TAURI_EVENT_PLUGIN_INTERNALS__ = {
  unregisterListener(event: string, id: number) {
    listeners.get(event)?.delete(id);
    callbacks.delete(id);
  },
};

(window as any).__TAURI_INTERNALS__ = {
  metadata: { currentWindow: { label: "main" }, currentWebview: { windowLabel: "main", label: "main" } },
  transformCallback(callback: Callback, once = false) {
    const id = nextId++;
    callbacks.set(id, once ? (v) => { callbacks.delete(id); callback(v); } : callback);
    return id;
  },
  unregisterCallback(id: number) { callbacks.delete(id); },
  convertFileSrc: (path: string) => path,
  invoke: (command: string, args?: unknown) =>
    command.startsWith("plugin:") ? plugin(command, args) : call(command, args),
};
