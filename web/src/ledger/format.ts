/** Formatting helpers shared by the console's view model. */

export function money(n: number): string {
  return n.toLocaleString('en-US', { minimumFractionDigits: 2, maximumFractionDigits: 2 })
}

/** 1_250_000 -> "1.25M", 3_400 -> "3.4K". */
export function compact(n: number): string {
  if (n >= 1e6) return `${(n / 1e6).toFixed(2)}M`
  if (n >= 1e3) return `${(n / 1e3).toFixed(1)}K`
  return String(Math.round(n))
}

export function shortHash(h: string): string {
  return `0x${h.slice(0, 6)}…${h.slice(-4)}`
}

export function relative(ms: number): string {
  const s = Math.max(0, Math.round(ms / 1000))
  if (s < 5) return 'just now'
  if (s < 60) return `${s}s ago`
  if (s < 3600) return `${Math.floor(s / 60)}m ago`
  if (s < 86400) return `${Math.floor(s / 3600)}h ago`
  return `${Math.floor(s / 86400)}d ago`
}

export function clock(t: number): string {
  return new Date(t).toTimeString().slice(0, 8)
}

export function randomHex(n: number): string {
  let s = ''
  for (let i = 0; i < n; i++) s += '0123456789abcdef'[(Math.random() * 16) | 0]
  return s
}

export function pick<T>(a: readonly T[]): T {
  return a[(Math.random() * a.length) | 0]!
}
