import { setLoading, setStatus, notify } from "./notifications.svelte.js";

/**
 * Wraps an async invoke with standardised try/catch/finally and global
 * notification hooks (loading bar, status bar, error toasts).
 *
 * The caller provides a plain reactive object (created in the .svelte
 * component) with `busy`, `error`, and `operation` properties.  This
 * avoids the Svelte 5 cross-module reactivity boundary issue of
 * non-writable getter returns from `.svelte.js`.
 *
 * Usage (inside a .svelte <script>):
 *   let state = $state({ busy: false, error: "", operation: "" });
 *   await invokeWith(state, async () => {
 *     await invoke("some_command");
 *   }, "optional label");
 *
 *   {#if state.busy} {state.operation}… {/if}
 *   {#if state.error} <div>{state.error}</div> {/if}
 *
 * @template T
 * @param {{ busy: boolean, error: string, operation: string }} state
 * @param {() => Promise<T>} fn
 * @param {string} [label] - shown in loading bar / status bar
 * @param {{ toastError?: boolean, toastSuccess?: string }} [opts]
 * @returns {Promise<T | null>}
 */
export async function invokeWith(state, fn, label, opts = {}) {
  if (state.busy) return null;
  const { toastError = true, toastSuccess } = opts;
  state.busy = true;
  state.operation = label ?? "";
  state.error = "";
  if (label) {
    setLoading(true, label);
    setStatus(label + "…");
  }
  try {
    const result = await fn();
    if (toastSuccess) notify(toastSuccess, "success");
    return result;
  } catch (/** @type {any} */ e) {
    const msg = typeof e === "string" ? e : e?.message ?? String(e);
    state.error = msg;
    if (toastError) notify(msg, "error", 6000);
    return null;
  } finally {
    state.busy = false;
    state.operation = "";
    if (label) {
      setLoading(false);
      setStatus("");
    }
  }
}
