/**
 * The daemon's operator API.
 *
 * Hand-written rather than generated: both ends of this live in one repo and
 * land in one commit, so there is no boundary for codegen to guard yet.
 */

/** Dev proxies `/api` to the daemon; a built console is told where it lives. */
const BASE: string = import.meta.env.VITE_API_BASE ?? '/api/v1'

/** The daemon answered, and said no. */
export class ApiError extends Error {
  readonly status: number

  constructor(status: number, message: string) {
    super(message)
    this.name = 'ApiError'
    this.status = status
  }

}

/** Nothing answered: the daemon is not running, or not reachable.  */
export class NetworkError extends Error {
  constructor(message: string, options?: { cause?: unknown }) {
    super(message, options)
    this.name = 'NetworkError'
  }
}

/** Every console response wears this. */
interface Envelope<T> {
  status: 'ok' | 'error'
  result?: T
  error?: string
}

interface Page<T> {
  items: T[]
  next_cursor?: string
}

export interface Features {
  deposit: boolean
  disclose: boolean
  pause: boolean
  pay: boolean
}

export interface Bootstrap {
  chain_id: number
  pool: string
  token: string
  verifier: string
  seller: string
  /** Pool units per whole token, as a power of ten. */
  decimals: number
  session: number
  window: number
  features: Features
  models: string[]
}

export interface Pool {
  balance: number
  notes: number
  /** Every commitment the pool holds, which is the set a spend hides in. */
  leaves: number
  root: string
  synced: boolean
  /** What the funding key still holds in the clear, in pool units. */
  depositable: number | null
  last_deposit: { amount: number; at: number } | null
}

export type Stage = 'approving' | 'shielding' | 'placing' | 'done' | 'failed'

/** A deposit in flight, or the last one there was. */
export interface Progress {
  stage: Stage
  amount: number
  started: number
  commitment: string | null
  error: string | null
}

export interface Mandate {
  id: string
  label: string
  /** `null` is uncapped: only the envelope's ceiling stops it. */
  budget: number | null
  spent: number
  calls: number
  created: number
  used: number | null
  revoked: boolean
  paused: boolean
  models: string[] | null
}

/** One agent's draw on the shared envelope. */
export interface Drawn {
  id: string
  label: string
  metered: number
  escrow: number
  calls: number
  tokens: number
  model: string
}

export interface Envelope402 {
  nullifier: string
  seller: string
  ceiling: number
  spent: number
  remaining: number
  calls: number
  tokens: number
  opened: number
  deadline: number
  agents: Drawn[]
}

export type EventKind = 'deposit' | 'escrow' | 'settled' | 'paid' | 'received'

/** A private payment this wallet sent. */
export interface Sent {
  recipient_commitment: string
  change: string | null
}

export interface Event {
  id: string
  kind: EventKind
  at: number
  agent: string | null
  label: string | null
  vendor: string
  amount: number
  tokens: number
  commitment: string | null
  nullifier: string | null
}

export interface Witness {
  reference: string
  prompt: number
  completion: number
  provider: string | null
}

export interface Call {
  id: string
  agent: string
  label: string
  model: string
  cost: number
  at: number
  tokens: number | null
  proof: {
    nullifier: string
    attestation: string
    delivered: string | null
    sequence: number
    witness: Witness | null
  }
}

export interface Granted extends Mandate {
  /** Shown once. Nothing can recover it after this. */
  key: string
}

export interface AuditScope {
  agent?: string
  from?: number
  to?: number
  tx?: string
  limit?: number
  cursor?: string
}

export interface Client {
  config(): Promise<Bootstrap>
  pool(): Promise<Pool>
  agents(): Promise<Mandate[]>
  envelope(): Promise<Envelope402 | null>
  activity(limit?: number): Promise<Event[]>
  audit(scope?: AuditScope): Promise<Page<Call>>
  issue(mandate: { label: string; budget?: number | undefined; models?: string[] | undefined }): Promise<Granted>
  /** Starts a deposit. It outlives the request; watch `depositProgress`. */
  deposit(amount: number): Promise<void>
  depositProgress(): Promise<Progress | null>
  revoke(id: string): Promise<void>
  pause(id: string, paused: boolean): Promise<void>
  /** This wallet's payment address, for another agent to pay it at. */
  address(): Promise<string>
  /** Pays `address` privately; resolves once the transfer has landed. */
  pay(address: string, amount: number): Promise<Sent>
}

export function client(base: string = BASE): Client {
  const send = async <T>(path: string, init?: RequestInit): Promise<T> => {
    let response: Response
    try {
      response = await fetch(`${base}${path}`, {
        ...init,
        headers: {
          ...(init?.body ? { 'Content-Type': 'application/json' } : {}),
          ...init?.headers,
        },
      })
    } catch (cause) {
      throw new NetworkError('the daemon did not answer', { cause })
    }

    // A refusal carries the same envelope as a success, so the message is
    // read the same way either way.
    const body = (await response.json().catch(() => null)) as Envelope<T> | null
    if (!response.ok || body?.status === 'error') {
      throw new ApiError(response.status, body?.error ?? response.statusText)
    }
    return body?.result as T
  }

  const query = (scope: Record<string, string | number | undefined>): string => {
    const search = new URLSearchParams()
    for (const [key, value] of Object.entries(scope)) {
      if (value !== undefined && value !== null) search.set(key, String(value))
    }
    const encoded = search.toString()
    return encoded ? `?${encoded}` : ''
  }

  return {
    config: () => send<Bootstrap>('/config'),
    pool: () => send<Pool>('/pool'),
    agents: () => send<Page<Mandate>>('/agents').then((page) => page.items),
    envelope: () => send<Envelope402 | null>('/envelope'),
    activity: (limit) => send<Page<Event>>(`/activity${query({ limit })}`).then((p) => p.items),
    audit: (scope = {}) => send<Page<Call>>(`/audit${query({ ...scope })}`),
    issue: (mandate) =>
      send<Granted>('/agents', { method: 'POST', body: JSON.stringify(mandate) }),
    deposit: (amount) =>
      send<void>('/deposit', { method: 'POST', body: JSON.stringify({ amount }) }),
    depositProgress: () => send<Progress | null>('/deposit'),
    revoke: (id) => send<void>(`/agents/${id}`, { method: 'DELETE' }),
    pause: (id, paused) =>
      send<void>(`/agents/${id}/${paused ? 'pause' : 'resume'}`, { method: 'POST' }),
    address: () => send<{ address: string }>('/address').then((a) => a.address),
    pay: (address, amount) =>
      send<Sent>('/pay', { method: 'POST', body: JSON.stringify({ address, amount }) }),
  }
}
