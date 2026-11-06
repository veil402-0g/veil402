// A self-running ledger simulation, as the design shipped it: no network, so the
// console is useful before a backend exists.
import { randomHex } from '../../lib/format'

export const VENDORS = [
  '0G · llama-3.3-70b',
  '0G · deepseek-r1',
  '0G · qwen2.5-coder',
  '0G · whisper-v3',
  '0G · sdxl-turbo',
] as const

export const ENDPOINTS = [
  'llama-3.3-70b',
  'deepseek-r1',
  'qwen2.5-coder',
  'whisper-v3',
  'sdxl-turbo',
] as const

export type TxKind = 'settled' | 'escrow' | 'deposit' | 'revoke' | 'refund'

export const KIND_LABEL: Record<TxKind, string> = {
  settled: 'settled',
  escrow: 'escrow opened',
  deposit: 'deposit',
  revoke: 'mandate revoked',
  refund: 'escrow refunded',
}

export const AMOUNT_SIGN: Record<TxKind, string> = {
  settled: '−',
  escrow: '',
  deposit: '+',
  revoke: '',
  refund: '−',
}

export type AgentStatus = 'active' | 'paused' | 'revoked'

export interface Agent {
  id: string
  name: string
  purpose: string
  budget: number
  spent: number
  status: AgentStatus
  /** Last activity, epoch ms. */
  last: number
  txs: number
}

export interface Session {
  id: string
  agentId: string
  vendor: string
  escrow: number
  metered: number
  tokens: number
  started: number
}

export interface Tx {
  id: string
  commit: string
  nullifier: string
  t: number
  agent: string
  vendor: string
  amount: number
  kind: TxKind
  tokens: number
}

export interface Proof {
  id: string
  label: string
  t: number
}

export interface Ledger {
  lastDeposit: { amount: number; t: number } | null
  agents: Agent[]
  sessions: Session[]
  feed: Tx[]
  balance: number
  selectedTx: string | null
  proofs: Proof[]
  notes: number
  auditAgent: string | null
  root: string | null
}

/** The feed is a rolling window; older entries fall off the end. */
const FEED_CAP = 40

export function makeTx(o: Partial<Tx> & Pick<Tx, 'agent' | 'vendor' | 'amount' | 'kind'>): Tx {
  return {
    id: `tx_${randomHex(6)}`,
    commit: randomHex(64),
    nullifier: randomHex(64),
    t: Date.now(),
    tokens: 0,
    ...o,
  }
}

/** Pads a hash so a fixture can pin both ends and randomise the middle. */
function pinnedHash(head: string, tail: string): string {
  return head + randomHex(64 - head.length - tail.length) + tail
}

export function seedDemo(): Ledger {
  const now = Date.now()

  const agents: Agent[] = [
    { id: 'mnd_7Q2aK1', name: 'research-03', purpose: 'Literature review', budget: 6000, spent: 4287.85, status: 'active', last: now - 16e3, txs: 214 },
    { id: 'mnd_3fLx90', name: 'support-triage', purpose: 'Ticket routing', budget: 2500, spent: 2043.92, status: 'active', last: now - 2e3, txs: 1190 },
    { id: 'mnd_Zk81pe', name: 'codegen-ci', purpose: 'PR review & fixes', budget: 10000, spent: 3204.61, status: 'active', last: now - 12e3, txs: 404 },
    { id: 'mnd_c0R4mm', name: 'pricing-scout', purpose: 'Market scan', budget: 1200, spent: 1192.31, status: 'active', last: now - 3 * 60e3, txs: 88 },
    { id: 'mnd_Wq55tn', name: 'legal-summarizer', purpose: 'Contract digests', budget: 3000, spent: 412.0, status: 'paused', last: now - 5 * 3600e3, txs: 31 },
    { id: 'mnd_Hh02vb', name: 'growth-outreach', purpose: 'Lead enrichment', budget: 1500, spent: 1500, status: 'revoked', last: now - 2 * 86400e3, txs: 640 },
  ]

  const sessions: Session[] = [
    { id: 'ses_1c77d0', agentId: 'mnd_7Q2aK1', vendor: VENDORS[0], escrow: 25, metered: 0.86, tokens: 35100, started: now - 5e3 },
  ]

  const recent: [string, number, number, TxKind, number][] = [
    ['research-03', 0, 25, 'escrow', 5],
    ['support-triage', 1, 4.3, 'settled', 12],
    ['codegen-ci', 3, 12.8, 'settled', 184],
    ['research-03', 1, 26.31, 'settled', 188],
    ['pricing-scout', 3, 4.48, 'settled', 200],
  ]
  const feed: Tx[] = recent.map(([agent, v, amount, kind, age]) =>
    makeTx({
      agent,
      vendor: VENDORS[v]!,
      amount,
      kind,
      t: now - age * 1000,
      tokens: Math.round(amount * 41000),
    }),
  )

  // The inspector opens on this one, so its hashes are pinned.
  feed.push(
    makeTx({
      id: 'tx_9302c9',
      agent: 'research-03',
      vendor: VENDORS[1],
      amount: 12.84,
      kind: 'settled',
      t: now - 260e3,
      tokens: 526000,
      commit: pinnedHash('f4cd5f', '0443'),
      nullifier: pinnedHash('f91c44', '0ff1'),
    }),
  )

  const names = ['research-03', 'support-triage', 'codegen-ci', 'pricing-scout']
  const older = [18.4, 9.95, 44.17, 2.06, 21.3, 7.42, 15.6, 3.21, 11.08, 26.9, 5.75, 19.33, 8.6, 31.14, 6.27, 13.62, 4.99, 7.5]
  older.forEach((amount, k) => {
    feed.push(
      makeTx({
        agent: names[k % 4]!,
        vendor: VENDORS[k % 5]!,
        amount,
        kind: 'settled',
        t: now - (320 + k * 55) * 1000,
        tokens: Math.round(amount * 41000),
      }),
    )
  })

  // Nudge the oldest settlement so the 24h total lands on a round headline figure.
  const settledSum = feed.filter((f) => f.kind === 'settled').reduce((t, f) => t + f.amount, 0)
  const tail = feed[feed.length - 1]!
  tail.amount = Number((tail.amount + (350.62 - settledSum)).toFixed(2))

  for (let q = 0; q < 15; q++) {
    feed.push(
      makeTx({
        agent: names[q % 4]!,
        vendor: VENDORS[(q + 2) % 5]!,
        amount: [25, 10, 50][q % 3]!,
        kind: 'escrow',
        t: now - (330 + q * 61) * 1000,
      }),
    )
  }

  feed.sort((a, b) => b.t - a.t)
  feed.push(
    makeTx({ agent: 'Treasury', vendor: 'Shielded pool', amount: 5000, kind: 'deposit', t: now - 2 * 86400e3 }),
  )

  return {
    lastDeposit: { amount: 5000, t: now - 2 * 86400e3 },
    agents,
    sessions,
    feed,
    balance: 48163.68,
    selectedTx: 'tx_9302c9',
    proofs: [{ id: 'vp_077be697', label: 'research-03 · Q3 mandate history', t: now - 3 * 86400e3 }],
    notes: 39,
    auditAgent: 'mnd_7Q2aK1',
    root: '0x464fc5…98a1',
  }
}

export function seedEmpty(): Ledger {
  return {
    lastDeposit: null,
    agents: [],
    sessions: [],
    feed: [],
    balance: 0,
    selectedTx: null,
    proofs: [],
    notes: 0,
    auditAgent: null,
    root: null,
  }
}

/** Closes a session: the agent is charged the metered amount, escrow released. */
export function settle(l: Ledger, sessionId: string, kind: TxKind = 'settled'): Ledger {
  const session = l.sessions.find((x) => x.id === sessionId)
  if (!session) return l
  const agent = l.agents.find((a) => a.id === session.agentId)
  const amount = Math.round(session.metered * 100) / 100

  return {
    ...l,
    agents: l.agents.map((a) =>
      a.id === session.agentId
        ? { ...a, spent: a.spent + amount, last: Date.now(), txs: a.txs + 1 }
        : a,
    ),
    sessions: l.sessions.filter((x) => x.id !== sessionId),
    balance: l.balance - amount,
    feed: [
      makeTx({ agent: agent?.name ?? '?', vendor: session.vendor, amount, kind, tokens: session.tokens }),
      ...l.feed,
    ].slice(0, FEED_CAP),
    notes: l.notes + 1,
  }
}

/** One second of simulated traffic: meters open sessions, opens and closes some. */
export function advance(l: Ledger, tick: number): Ledger {
  let next: Ledger = {
    ...l,
    sessions: l.sessions.map((x) => {
      const delta = x.escrow * (0.004 + Math.random() * 0.018)
      return {
        ...x,
        metered: Math.min(x.escrow, x.metered + delta),
        tokens: x.tokens + Math.round(delta * 41000),
      }
    }),
  }

  const exhausted = next.sessions.find((x) => x.metered >= x.escrow * 0.985)
  if (exhausted) next = settle(next, exhausted.id)

  if (tick % 4 !== 0) return next

  const idle = next.agents.filter(
    (a) => a.status === 'active' && !next.sessions.some((x) => x.agentId === a.id),
  )
  if (next.sessions.length < 4 && idle.length && Math.random() < 0.6) {
    const a = pickAgent(idle)
    const escrow = [10, 25, 50][(Math.random() * 3) | 0]!
    if (a.budget - a.spent >= escrow) {
      const session: Session = {
        id: `ses_${randomHex(6)}`,
        agentId: a.id,
        vendor: VENDORS[(Math.random() * VENDORS.length) | 0]!,
        escrow,
        metered: 0,
        tokens: 0,
        started: Date.now(),
      }
      return {
        ...next,
        sessions: [...next.sessions, session],
        agents: next.agents.map((x) => (x.id === a.id ? { ...x, last: Date.now() } : x)),
        feed: [
          makeTx({ agent: a.name, vendor: session.vendor, amount: escrow, kind: 'escrow' }),
          ...next.feed,
        ].slice(0, FEED_CAP),
      }
    }
    return next
  }

  if (!next.sessions.length) return next
  const fullest = [...next.sessions].sort((a, b) => b.metered / b.escrow - a.metered / a.escrow)[0]!
  return fullest.metered / fullest.escrow > 0.4 ? settle(next, fullest.id) : next
}

function pickAgent(agents: Agent[]): Agent {
  return agents[(Math.random() * agents.length) | 0]!
}

/** Revoking settles open escrow as a refund, then nullifies the mandate. */
export function revoke(l: Ledger, id: string): Ledger {
  const agent = l.agents.find((x) => x.id === id)
  if (!agent) return l

  let next = l
  for (const session of l.sessions.filter((x) => x.agentId === id)) {
    next = settle(next, session.id, 'refund')
  }

  return {
    ...next,
    agents: next.agents.map((x) => (x.id === id ? { ...x, status: 'revoked', last: Date.now() } : x)),
    feed: [
      makeTx({
        agent: agent.name,
        vendor: `Mandate ${agent.id}`,
        amount: Math.max(agent.budget - agent.spent, 0),
        kind: 'revoke',
      }),
      ...next.feed,
    ].slice(0, FEED_CAP),
  }
}

export interface Totals {
  spent: number
  escrow: number
  mandated: number
  free: number
  total: number
}

export function totals(l: Ledger): Totals {
  const spent = l.agents.reduce((t, a) => t + a.spent, 0)
  const escrow = l.sessions.reduce((t, x) => t + x.escrow, 0)
  const mandated = l.agents
    .filter((a) => a.status !== 'revoked')
    .reduce((t, a) => {
      const held = l.sessions.filter((x) => x.agentId === a.id).reduce((q, x) => q + x.escrow, 0)
      return t + Math.max(a.budget - a.spent - held, 0)
    }, 0)
  const free = Math.max(l.balance - escrow - mandated, 0)
  return { spent, escrow, mandated, free, total: spent + escrow + mandated + free }
}

const AVATAR_POSITIONS = ['0% 0%', '100% 20%', '30% 100%', '80% 80%', '50% 40%', '10% 60%', '90% 0%']

export const AVATAR_MESH =
  'radial-gradient(60% 50% at 30% 30%,#2c63e6 0%,transparent 70%),' +
  'radial-gradient(40% 30% at 88% 34%,#d8b9a6 0%,transparent 70%),' +
  'radial-gradient(70% 45% at 40% 92%,#c3d3f6 0%,transparent 70%),' +
  'linear-gradient(170deg,#3d74ea 0%,#4c86ee 45%,#8fb0f2 80%,#b6c9f4 100%)'

export interface Avatar {
  ini: string
  avBg: string
  avPos: string
  avO: number
}

/** Initials plus a stable mesh offset, so one agent always looks the same. */
export function avatar(name: string | undefined, dead: boolean): Avatar {
  const n = String(name || '?')
  const words = n.split(/[-_\s]+/).filter((p) => /[a-z]/i.test(p))
  const first = words[0] ?? n
  const ini = (first[0]! + (words[1]?.[0] ?? first[1] ?? '')).toUpperCase()
  let h = 0
  for (const ch of n) h = (h * 31 + ch.charCodeAt(0)) % 997
  return {
    ini,
    avBg: dead ? 'linear-gradient(170deg,#8b8e97,#b4b7c0)' : AVATAR_MESH,
    avPos: AVATAR_POSITIONS[h % AVATAR_POSITIONS.length]!,
    avO: 1,
  }
}
