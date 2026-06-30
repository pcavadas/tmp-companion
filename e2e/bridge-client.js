// e2e bridge-client — injected into the real React app (running in Chromium under
// Playwright) BEFORE any app code, via `page.addInitScript`. It installs the
// `window.__TAURI_INTERNALS__` the `@tauri-apps/api` expects, so `isTauri()` is true and
// `invoke(cmd, args)` is forwarded — over plain HTTP — to the windowless Rust e2e_server
// (which runs the REAL commands against a SimDevice). `invoke.ts` / `liveEvents.ts` are
// untouched; all the shim lives here.
//
// Request/response only: the V1 Copy/Level journeys complete on the command's return
// value. `Channel` args serialize to "__CHANNEL__:<id>" (the device ignores the no-op
// callback), and `plugin:*` invokes (event/log) resolve locally so the app never errors
// on a feature this offline tier doesn't bridge.
(() => {
  const PORT = 7600; // matches playwright.config.ts webServer + TMP_E2E_PORT
  const BASE = `http://127.0.0.1:${PORT}`;
  const SERIALIZE_KEY = "__TAURI_TO_IPC_KEY__"; // @tauri-apps/api SERIALIZE_TO_IPC_FN

  // Walk args, replacing any IPC-serializable value (e.g. a Channel) with its wire form.
  const serialize = (v) => {
    if (v && typeof v === "object") {
      const fn = v[SERIALIZE_KEY];
      if (typeof fn === "function") return fn.call(v);
      if (Array.isArray(v)) return v.map(serialize);
      const out = {};
      for (const k of Object.keys(v)) out[k] = serialize(v[k]);
      return out;
    }
    return v;
  };

  let nextId = 0;
  const callbacks = new Map();

  window.__TAURI_INTERNALS__ = {
    transformCallback(cb, _once) {
      const id = ++nextId;
      callbacks.set(id, cb);
      return id;
    },
    unregisterCallback(id) {
      callbacks.delete(id);
    },
    async invoke(cmd, args) {
      // The event/log plugins aren't bridged in this offline tier — resolve locally so
      // a `listen(...)` or a log call never rejects and crashes a mount effect.
      if (cmd.startsWith("plugin:event|")) return ++nextId; // a unique listener id
      if (cmd.startsWith("plugin:")) return null;
      const res = await fetch(`${BASE}/invoke`, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ cmd, args: serialize(args ?? {}) }),
      });
      const env = await res.json();
      if (env.ok) return env.data;
      throw env.error;
    },
  };

  // `@tauri-apps/api/event`'s unlisten goes through this separate internal object.
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = {
    unregisterListener() {},
  };
})();
