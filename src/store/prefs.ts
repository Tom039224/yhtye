// Small per-device preferences (the project opened last). Browser storage can
// be missing or throw (private windows, blocked site data), so every access is
// guarded and a failure only means the preference is not remembered.

export interface Prefs {
  get(key: string): string | null;
  set(key: string, value: string): void;
}

export function memoryPrefs(initial: Record<string, string> = {}): Prefs {
  const values = new Map(Object.entries(initial));
  return {
    get: (key) => values.get(key) ?? null,
    set: (key, value) => {
      values.set(key, value);
    },
  };
}

/** `localStorage`, falling back to nothing when it is unavailable. */
export function browserPrefs(): Prefs {
  return {
    get: (key) => {
      try {
        return window.localStorage.getItem(key);
      } catch {
        return null;
      }
    },
    set: (key, value) => {
      try {
        window.localStorage.setItem(key, value);
      } catch {
        // Not remembered; the user picks the project again after a reload.
      }
    },
  };
}
