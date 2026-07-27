import { writable, derived } from "svelte/store";

type Dict = Record<string, string>;

const STORAGE_KEY = "devpanel-locale";
const FALLBACK_LOCALE = "en";

function loadLocale(): string {
  try {
    return localStorage.getItem(STORAGE_KEY) || FALLBACK_LOCALE;
  } catch {
    return FALLBACK_LOCALE;
  }
}

function saveLocale(locale: string): void {
  try {
    localStorage.setItem(STORAGE_KEY, locale);
  } catch { /* noop */ }
}

export const locale = writable<string>(loadLocale());

const dictionaries = new Map<string, Dict>();

export const t = derived(locale, ($locale) => {
  return (key: string, params?: Record<string, string | number>): string => {
    const dict = dictionaries.get($locale) || dictionaries.get(FALLBACK_LOCALE) || {};
    let msg = dict[key] || key;
    if (params) {
      for (const [k, v] of Object.entries(params)) {
        msg = msg.replace(`{${k}}`, String(v));
      }
    }
    return msg;
  };
});

export async function loadDictionary(localeCode: string): Promise<void> {
  if (dictionaries.has(localeCode)) return;
  try {
    const mod = await import(`./locales/${localeCode}.json`);
    dictionaries.set(localeCode, mod.default as Dict);
  } catch {
    if (localeCode !== FALLBACK_LOCALE) {
      await loadDictionary(FALLBACK_LOCALE);
    }
  }
}

export function setLocale(next: string): void {
  locale.set(next);
  saveLocale(next);
}

export const supportedLocales = [
  { code: "en", label: "English" },
  { code: "es", label: "Español" },
];
