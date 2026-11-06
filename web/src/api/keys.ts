/**
 * The keys this browser has seen.
 *
 * A mandate is an API key, and the daemon keeps only a hash of it — so the
 * one moment the key exists in full is the response that issued it. Nothing
 * can ask for it again. Kept here so the console can hand it back, which
 * means a key issued from another browser, or from the command line, has
 * nothing to copy and the console says so rather than guessing.
 */

const STORE = 'veil402.keys'

type Keyring = Record<string, string>

function read(): Keyring {
  try {
    return JSON.parse(window.localStorage.getItem(STORE) ?? '{}') as Keyring
  } catch {
    return {}
  }
}

function write(keyring: Keyring): void {
  try {
    window.localStorage.setItem(STORE, JSON.stringify(keyring))
  } catch {
    // A console in a private window still issues keys; it just forgets them.
  }
}

export function remember(id: string, key: string): void {
  write({ ...read(), [id]: key })
}

export function keyFor(id: string): string | null {
  return read()[id] ?? null
}

/** A revoked mandate's key opens nothing, so keeping it helps no one. */
export function forget(id: string): void {
  const keyring = read()
  delete keyring[id]
  write(keyring)
}

/** Enough to recognise a key without putting the whole of it on screen. */
export function masked(key: string): string {
  return key.length > 18 ? `${key.slice(0, 12)}…${key.slice(-4)}` : key
}
