/**
 * The daemon's answers, as the ledger the console already knows how to draw.
 *
 * Everything the console computes — treemaps, funnels, projections — reads
 * one `Ledger`. So this is the whole integration: the view model never
 * learns whether it is looking at a simulation or at a daemon.
 */
import type {
  Bootstrap,
  Call,
  Client,
  Envelope402,
  Event,
  Mandate,
  Pool,
} from '../api/client'
import { shortHash } from './format'
import type { Agent, AgentStatus, Ledger, Session, Tx, TxKind } from './model'

/** How often each answer is worth asking for again. */
export const CADENCE = {
  activity: 2_000,
  pool: 3_000,
  envelope: 3_000,
  agents: 5_000,
} as const

/** Pool units to whole tokens. */
function scaled(units: number, decimals: number): number {
  return units / 10 ** decimals
}

/** Seconds on the wire; the console works in milliseconds. */
function millis(seconds: number): number {
  return seconds * 1000
}

function status(mandate: Mandate): AgentStatus {
  if (mandate.revoked) return 'revoked'
  if (mandate.paused) return 'paused'
  return 'active'
}

function agent(mandate: Mandate, decimals: number): Agent {
  return {
    id: mandate.id,
    name: mandate.label,
    purpose: mandate.models
      ? `${mandate.models.length} endpoint${mandate.models.length === 1 ? '' : 's'} allowed`
      : 'any endpoint',
    budget: mandate.budget === null ? null : scaled(mandate.budget, decimals),
    spent: scaled(mandate.spent, decimals),
    status: status(mandate),
    last: millis(mandate.used ?? mandate.created),
    txs: mandate.calls,
  }
}

/**
 * The open envelope, as one row per agent drawing on it.
 *
 * There is only ever one envelope: the pool holds a single lock against a
 * single note. The daemon splits its ceiling so the shares add up to the
 * figure the chain holds, which is what lets the console meter each agent
 * without inventing escrow that does not exist.
 */
function sessions(envelope: Envelope402 | null, decimals: number): Session[] {
  if (!envelope) return []
  return envelope.agents.map((drawn) => ({
    id: `ses_${envelope.nullifier.slice(0, 6)}_${drawn.id.slice(0, 4)}`,
    agentId: drawn.id,
    vendor: drawn.model,
    escrow: scaled(drawn.escrow, decimals),
    metered: scaled(drawn.metered, decimals),
    tokens: drawn.tokens,
    started: millis(envelope.opened),
  }))
}

function event(entry: Event, decimals: number): Tx {
  return {
    id: `tx_${entry.id.slice(0, 6)}`,
    commit: entry.commitment ?? '',
    nullifier: entry.nullifier ?? '',
    t: millis(entry.at),
    agent: entry.label ?? (entry.kind === 'deposit' ? 'Treasury' : '—'),
    vendor: entry.vendor,
    amount: scaled(entry.amount, decimals),
    kind: entry.kind as TxKind,
    tokens: entry.tokens,
  }
}

/** What one poll of the daemon returned. */
export interface Snapshot {
  pool: Pool
  agents: Mandate[]
  envelope: Envelope402 | null
  activity: Event[]
}

export async function snapshot(api: Client): Promise<Snapshot> {
  const [pool, agents, envelope, activity] = await Promise.all([
    api.pool(),
    api.agents(),
    api.envelope(),
    api.activity(40),
  ])
  return { pool, agents, envelope, activity }
}

/**
 * Folds a snapshot into the ledger, keeping what the daemon does not know
 * about: which payment the inspector is open on, and the proofs this
 * session has generated.
 */
export function fold(previous: Ledger, taken: Snapshot, config: Bootstrap): Ledger {
  const { decimals } = config
  const feed = taken.activity.map((entry) => event(entry, decimals))
  const agents = taken.agents.map((mandate) => agent(mandate, decimals))

  // The inspector stays where the operator put it, unless what it was open
  // on has fallen off the end of the feed.
  const selected =
    previous.selectedTx && feed.some((row) => row.id === previous.selectedTx)
      ? previous.selectedTx
      : (feed.find((row) => row.kind === 'settled')?.id ?? null)

  return {
    lastDeposit: taken.pool.last_deposit
      ? {
          amount: scaled(taken.pool.last_deposit.amount, decimals),
          t: millis(taken.pool.last_deposit.at),
        }
      : null,
    agents,
    sessions: sessions(taken.envelope, decimals),
    feed,
    balance: scaled(taken.pool.balance, decimals),
    selectedTx: selected,
    proofs: previous.proofs,
    notes: taken.pool.notes,
    leaves: taken.pool.leaves,
    depositable: scaled(taken.pool.depositable ?? 0, decimals),
    auditAgent:
      previous.auditAgent && agents.some((row) => row.id === previous.auditAgent)
        ? previous.auditAgent
        : (agents[0]?.id ?? null),
    root: taken.pool.root ? shortHash(taken.pool.root) : null,
  }
}

/** One audit row as the report shows it. */
export function reported(call: Call, decimals: number): Tx {
  return {
    id: `tx_${call.id.slice(0, 6)}`,
    commit: call.proof.attestation,
    nullifier: call.proof.nullifier,
    t: millis(call.at),
    agent: call.label,
    vendor: call.model,
    amount: scaled(call.cost, decimals),
    kind: 'settled',
    tokens: call.tokens ?? 0,
  }
}
