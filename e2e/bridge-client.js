// e2e bridge-client — injected into the real React app (running in Chromium under
// Playwright) BEFORE any app code, via `page.addInitScript`. It installs the
// `window.__TAURI_INTERNALS__` the `@tauri-apps/api` expects, so `isTauri()` is true and
// `invoke(cmd, args)` is forwarded — over plain HTTP — to the windowless Rust e2e_server
// (which runs the REAL commands against a SimDevice offline, or the real unit online).
// `invoke.ts` / `liveEvents.ts` are untouched; all the shim lives here.
//
// Events and Channels are bridged too. The server captures every `tmp://` event and every
// Channel send in-process (`e2e_ui_bridge.rs`) into a bounded ring served at
// `GET /ui/events?after=<seq>`. While any `listen` handler or Channel is open, this client
// polls it every ~50 ms and dispatches each entry: an event to its listeners as
// `{event, id, payload}`, a Channel message to the Channel's callback as
// `{message, index}` (the shape `@tauri-apps/api`'s Channel reorders by). An invoke that
// carried a Channel polls once more before it resolves, so every row the command streamed
// reaches the page before its return value does. Other `plugin:*` invokes (log) resolve
// locally.
(() => {
  // `__E2E_PORT__` is injected by fixtures/test.ts from TMP_E2E_PORT (per-worktree bridge
  // port); falls back to 7600, matching the Playwright configs' + e2e_server's default.
  const PORT = window.__E2E_PORT__ || 7600;
  const BASE = `http://127.0.0.1:${PORT}`;
  const SERIALIZE_KEY = "__TAURI_TO_IPC_KEY__"; // @tauri-apps/api SERIALIZE_TO_IPC_FN
  const CHANNEL_PREFIX = "__CHANNEL__:";
  const POLL_MS = 50;

  let nextId = 0;
  const callbacks = new Map();
  // event name → Map(eventId → handler callback id)
  const listeners = new Map();
  // Channel ids (as strings, the ring's `name`) still owned by an in-flight invoke.
  const openChannels = new Set();

  // Walk args, replacing any IPC-serializable value (e.g. a Channel) with its wire form,
  // and note every Channel id the invoke carries.
  const serialize = (v, channels) => {
    if (v && typeof v === "object") {
      const fn = v[SERIALIZE_KEY];
      if (typeof fn === "function") {
        const wire = fn.call(v);
        if (typeof wire === "string" && wire.startsWith(CHANNEL_PREFIX)) {
          channels.push(wire.slice(CHANNEL_PREFIX.length));
        }
        return wire;
      }
      if (Array.isArray(v)) return v.map((x) => serialize(x, channels));
      const out = {};
      for (const k of Object.keys(v)) out[k] = serialize(v[k], channels);
      return out;
    }
    return v;
  };

  const runCallback = (id, data) => {
    const cb = callbacks.get(id);
    if (cb) cb(data);
  };

  const dispatch = (entry) => {
    if (entry.kind === "event") {
      const byId = listeners.get(entry.name);
      if (!byId) return;
      for (const [eventId, handler] of byId) {
        runCallback(handler, {
          event: entry.name,
          id: eventId,
          payload: entry.payload,
        });
      }
    } else if (entry.kind === "channel") {
      runCallback(Number(entry.name), {
        message: entry.payload,
        index: entry.index,
      });
    }
  };

  // The ring cursor. Start at the head so a page never replays entries from before it
  // loaded (online, `/sim/reset` clears nothing).
  let cursor = null;
  const fetchSince = async (after) => {
    const res = await fetch(`${BASE}/ui/events?after=${String(after)}`);
    if (!res.ok) throw new Error(`/ui/events answered ${String(res.status)}`);
    return res.json();
  };
  // Retry the head read rather than fall back to 0, which would replay pre-load entries.
  const ready = (async () => {
    for (;;) {
      try {
        cursor = (await fetchSince(Number.MAX_SAFE_INTEGER)).head;
        return;
      } catch {
        await new Promise((resolve) => setTimeout(resolve, POLL_MS));
      }
    }
  })();

  // One poll at a time, in order.
  let chain = Promise.resolve();
  const pollOnce = () => {
    chain = chain.then(async () => {
      await ready;
      try {
        const r = await fetchSince(cursor);
        for (const entry of r.entries) {
          cursor = Math.max(cursor, entry.seq);
          dispatch(entry);
        }
      } catch {
        // a dead server surfaces on the next invoke, not here
      }
    });
    return chain;
  };

  let timer = null;
  const active = () => listeners.size > 0 || openChannels.size > 0;
  const schedule = () => {
    if (timer !== null || !active()) return;
    timer = setTimeout(() => {
      timer = null;
      void pollOnce().then(schedule);
    }, POLL_MS);
  };

  const removeListener = (event, eventId) => {
    const byId = listeners.get(event);
    if (!byId) return;
    byId.delete(eventId);
    if (byId.size === 0) listeners.delete(event);
  };

  const eventPlugin = (cmd, args) => {
    if (cmd === "plugin:event|listen") {
      const eventId = ++nextId;
      if (!listeners.has(args.event)) listeners.set(args.event, new Map());
      listeners.get(args.event).set(eventId, args.handler);
      schedule();
      return eventId;
    }
    if (cmd === "plugin:event|unlisten") {
      removeListener(args.event, args.eventId);
    }
    return null;
  };

  window.__TAURI_INTERNALS__ = {
    transformCallback(cb, _once) {
      const id = ++nextId;
      callbacks.set(id, cb);
      return id;
    },
    unregisterCallback(id) {
      callbacks.delete(id);
    },
    runCallback,
    async invoke(cmd, args) {
      if (cmd.startsWith("plugin:event|")) return eventPlugin(cmd, args ?? {});
      if (cmd.startsWith("plugin:")) return null;
      const channels = [];
      const body = JSON.stringify({
        cmd,
        args: serialize(args ?? {}, channels),
      });
      for (const c of channels) openChannels.add(c);
      schedule();
      try {
        const res = await fetch(`${BASE}/invoke`, {
          method: "POST",
          headers: { "content-type": "application/json" },
          body,
        });
        const env = await res.json();
        // The command has returned, so every row it streamed is already in the ring.
        if (channels.length > 0) await pollOnce();
        if (env.ok) return env.data;
        throw env.error;
      } finally {
        for (const c of channels) openChannels.delete(c);
      }
    },
  };

  // `@tauri-apps/api/event`'s unlisten goes through this separate internal object first.
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = {
    unregisterListener(event, eventId) {
      removeListener(event, eventId);
    },
  };
})();
