/**
 * Global notification event bus.
 *
 * Uses a simple pub/sub pattern to avoid Svelte 5 cross-module reactivity
 * issues with $state.  The root layout component owns the $state for
 * toasts/loading/status and subscribes via the functions below.
 */

/** @type {Array<(event: any) => void>} */
const toastListeners = [];
/** @type {Array<(event: any) => void>} */
const loadingListeners = [];
/** @type {Array<(message: string) => void>} */
const statusListeners = [];

/**
 * @typedef {{ id: number, message: string, type: 'info' | 'success' | 'error' | 'warning' }} Toast
 * @typedef {{ id: number, toast: Toast } | { id: number, dismiss: true }} ToastEvent
 * @typedef {{ busy: boolean, operation: string }} LoadingEvent
 */

/**
 * @param {(event: ToastEvent) => void} fn
 * @returns {() => void} unsubscribe
 */
export function subscribeToasts(fn) {
  toastListeners.push(fn);
  return () => {
    const i = toastListeners.indexOf(fn);
    if (i >= 0) toastListeners.splice(i, 1);
  };
}

/**
 * @param {(event: LoadingEvent) => void} fn
 * @returns {() => void} unsubscribe
 */
export function subscribeLoading(fn) {
  loadingListeners.push(fn);
  return () => {
    const i = loadingListeners.indexOf(fn);
    if (i >= 0) loadingListeners.splice(i, 1);
  };
}

/**
 * @param {(message: string) => void} fn
 * @returns {() => void} unsubscribe
 */
export function subscribeStatus(fn) {
  statusListeners.push(fn);
  return () => {
    const i = statusListeners.indexOf(fn);
    if (i >= 0) statusListeners.splice(i, 1);
  };
}

let toastId = 0;

/**
 * @param {string} message
 * @param {'info' | 'success' | 'error' | 'warning'} [type]
 * @param {number} [duration] - ms before auto-dismiss, 0 = sticky
 * @returns {number} toast id (can be used with dismissToast)
 */
export function notify(message, type = 'info', duration = 4000) {
  const id = ++toastId;
  const toast = { id, message, type };
  toastListeners.forEach(fn => fn({ id, toast }));
  if (duration > 0) {
    setTimeout(() => dismissToast(id), duration);
  }
  return id;
}

/** @param {number} id */
export function dismissToast(id) {
  toastListeners.forEach(fn => fn({ id, dismiss: true }));
}

/** @type {LoadingEvent} */
let currentLoading = { busy: false, operation: '' };

/**
 * @param {boolean} busy
 * @param {string} [operation]
 */
export function setLoading(busy, operation = '') {
  currentLoading = { busy, operation };
  loadingListeners.forEach(fn => fn(currentLoading));
}

/** @param {string} message */
export function setStatus(message) {
  statusListeners.forEach(fn => fn(message));
}
