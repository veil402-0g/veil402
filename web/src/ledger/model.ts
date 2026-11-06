// The shape every panel in the console reads. What fills it comes from the
// daemon; see `live.ts`.

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
  /** `null` is uncapped: only the envelope's ceiling stops it. */
  budget: number | null
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
  /** Commitments in the whole pool, which is the set a spend hides in. */
  leaves: number
  /** What is left to shield, which is not the same as what is shielded. */
  depositable: number
  auditAgent: string | null
  root: string | null
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
    leaves: 0,
    depositable: 0,
    auditAgent: null,
    root: null,
  }
}

export interface Totals {
  spent: number
  escrow: number
  /** What the unlocked balance has been promised to: the chart's share. */
  mandated: number
  /** What every live mandate adds up to, which may exceed the pool. */
  promised: number
  free: number
  total: number
}

/**
 * Where the pool's balance sits right now.
 *
 * The lock and the mandates overlap — an agent's calls are paid out of the
 * very note the envelope has locked — so the three shares are carved out of
 * the balance in order rather than added up. They total the balance exactly,
 * which is the figure the headline shows.
 */
export function totals(l: Ledger): Totals {
  const spent = l.agents.reduce((t, a) => t + a.spent, 0)
  const escrow = Math.min(
    l.sessions.reduce((t, x) => t + x.escrow, 0),
    l.balance,
  )
  // An uncapped mandate reserves nothing: there is no figure to set aside,
  // and treating it as infinite would swallow the whole pool.
  const promised = l.agents
    .filter((a) => a.status !== 'revoked' && a.budget !== null)
    .reduce((t, a) => t + Math.max((a.budget ?? 0) - a.spent, 0), 0)
  const mandated = Math.min(promised, l.balance - escrow)
  const free = l.balance - escrow - mandated
  return { spent, escrow, mandated, promised, free, total: l.balance }
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
