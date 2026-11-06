import {
  createElement,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ChangeEvent,
  type MouseEvent,
} from 'react'
import { useNavigate } from 'react-router'
import { clock, compact, money, randomHex, relative, shortHash } from '../ledger/format'
import {
  AMOUNT_SIGN,
  KIND_LABEL,
  avatar,
  seedEmpty,
  totals,
  type Ledger,
  type Totals,
} from '../ledger/model'
import { useViewTransition } from '../transition/context'
import { ApiError, client, type Bootstrap, type Progress, type Stage } from '../api/client'
import { CADENCE, fold, snapshot } from '../ledger/live'
import { forget, keyFor, masked, remember } from '../api/keys'

/** Treemap grid for the pool-composition dot field. */
const DOT_ROWS = 6
const DOT_COLS = 72
const DOT_N = DOT_ROWS * DOT_COLS

/** Mini timeline: 45 buckets of 20s. */
const TL_COLS = 45
const TL_ROWS = 6
const TL_BUCKET_MS = 20_000

/** Range chart ceiling: 125% of budget. */
const RANGE_CEILING = 1.25
const RANGE_STEPS = 20
const RANGE_STEP = 1 / 16
const RANGE_OVER_CELL = 16
/** Each agent occupies this many plot columns; the 5th carries its cells. */
const PLOT_GROUP = 9

const PROOF_MS = 1400
const TOAST_MS = 3400

type Scope = 'tx' | 'agent' | 'range'

interface Toast {
  title: string
  sub: string
  color: string
}

interface Form {
  fundAmount: string
  funding: boolean
  mName: string
  mBudget: string
  mEndpoints: string[]
  mError: string
}

/** Turns a failure into something an operator can act on. */
function explain(error: unknown): string {
  return error instanceof ApiError
    ? error.message
    : 'the daemon is not answering on this address'
}


/** The steps a deposit goes through, in the order they happen. */
const STEPS: { key: Stage; label: string; note: string }[] = [
  { key: 'approving', label: 'Allowance', note: 'letting the pool move the tokens' },
  { key: 'shielding', label: 'Shielding', note: 'the transfer itself' },
  { key: 'placing', label: 'Committing', note: 'writing the note down' },
]

/** Fractions of what there is, which is more use than a figure typed here. */
function shares(of: number): { label: string; amount: string }[] {
  const cut = (part: number) => Math.floor(of * part * 1e6) / 1e6
  return [
    { label: '10%', amount: String(cut(0.1)) },
    { label: '50%', amount: String(cut(0.5)) },
    { label: 'MAX', amount: String(cut(1)) },
  ]
}

/** Parses "5,000" and other loosely-typed money input. */
function amountOf(v: string): number {
  return parseFloat(String(v).replace(/[^0-9.]/g, '')) || 0
}

/** Where the Playground button points; the compose stack publishes Langflow here. */
const PLAYGROUND_URL = import.meta.env.VITE_PLAYGROUND_URL ?? 'http://localhost:7860'

export interface ConsoleOptions {
  /** `Range` shows the projection chart, `Columns` the stacked budget chart. */
  agentChart?: 'Range' | 'Columns'
}

export function useConsoleBindings({ agentChart = 'Range' }: ConsoleOptions = {}) {
  const navigate = useNavigate()
  const transition = useViewTransition()

  // Empty until the daemon answers: everything here is its ledger.
  const [ledger, setLedger] = useState<Ledger>(seedEmpty)
  const [now, setNow] = useState(() => Date.now())
  const [config, setConfig] = useState<Bootstrap | null>(null)
  /** How the deposit in flight is getting on. */
  const [depositing, setDepositing] = useState<Progress | null>(null)
  /** The key a mandate was just issued with. The one moment it exists. */
  const [issued, setIssued] = useState<{ id: string; name: string; key: string } | null>(null)
  /** Bumped when the keyring changes, so the rows re-read it. */
  const [keyring, setKeyring] = useState(0)
  /** What stopped the daemon answering, in words the operator can act on. */
  const [offline, setOffline] = useState('')

  const [scope, setScope] = useState<Scope>('tx')
  const [proving, setProving] = useState(false)
  const [toast, setToast] = useState<Toast | null>(null)
  const [fundOpen, setFundOpen] = useState(false)
  const [mandateOpen, setMandateOpen] = useState(false)
  const [form, setForm] = useState<Form>({
    fundAmount: '5,000',
    funding: false,
    mName: '',
    mBudget: '2,000',
    mEndpoints: ['llama-3.3-70b', 'deepseek-r1'],
    mError: '',
  })

  // Hover latches: null means "nothing hovered", which dims nothing.
  const [hoverStat, setHoverStat] = useState<number | null>(null)
  const [hoverDot, setHoverDot] = useState<number | null>(null)
  const [hoverStage, setHoverStage] = useState<number | null>(null)
  const [hoverRange, setHoverRange] = useState<number | null>(null)
  const [hoverBalance, setHoverBalance] = useState(false)
  /** Gates the range chart's entrance animation. */
  const [rangeIn, setRangeIn] = useState(false)

  const toastTimer = useRef<number | undefined>(undefined)

  const notify = useCallback((title: string, sub: string, color = '#1230d6') => {
    window.clearTimeout(toastTimer.current)
    setToast({ title, sub, color })
    toastTimer.current = window.setTimeout(() => setToast(null), TOAST_MS)
  }, [])

  useEffect(() => () => window.clearTimeout(toastTimer.current), [])

  const api = useMemo(() => client(), [])

  // Only to keep relative times moving; the ledger itself advances when the
  // daemon says it has.
  useEffect(() => {
    const id = window.setInterval(() => setNow(Date.now()), 1000)
    return () => window.clearInterval(id)
  }, [])

  // The daemon's own addresses, limits and catalogue: read once, because a
  // console built against one deployment and pointed at another should say
  // so rather than quietly draw the wrong chain.
  // Both at once: waiting for the settings before asking for the ledger
  // would leave the page on zeroes for a round trip it does not need.
  useEffect(() => {
    let dropped = false
    Promise.all([api.config(), snapshot(api)])
      .then(([read, taken]) => {
        if (dropped) return
        setConfig(read)
        setLedger((l) => fold(l, taken, read))
        setOffline('')
      })
      .catch((error: unknown) => {
        if (!dropped) setOffline(explain(error))
      })
    return () => {
      dropped = true
    }
  }, [api])

  const refresh = useCallback(async () => {
    if (!config) return
    try {
      const taken = await snapshot(api)
      setLedger((l) => fold(l, taken, config))
      setOffline('')
    } catch (error: unknown) {
      setOffline(explain(error))
    }
  }, [api, config])

  useEffect(() => {
    void refresh()
    const id = window.setInterval(() => void refresh(), CADENCE.activity)
    return () => window.clearInterval(id)
  }, [refresh])

  useEffect(() => {
    const running = depositing && depositing.stage !== 'done' && depositing.stage !== 'failed'
    if (!running) return
    const id = window.setInterval(() => {
      api
        .depositProgress()
        .then((at) => {
          if (!at) return
          setDepositing(at)
          if (at.stage === 'done' || at.stage === 'failed') {
            setForm((f) => ({ ...f, funding: false }))
            void refresh()
          }
        })
        .catch(() => undefined)
    }, 1200)
    return () => window.clearInterval(id)
  }, [api, depositing, refresh])

  useEffect(() => {
    const id = window.setTimeout(() => setRangeIn(true), 350)
    return () => window.clearTimeout(id)
  }, [])

  const T: Totals = totals(ledger)
  const selected = ledger.feed.find((f) => f.id === ledger.selectedTx)
  const auditAgent = ledger.agents.find((a) => a.id === ledger.auditAgent) ?? ledger.agents[0]
  const isRevoked = (name: string) =>
    ledger.agents.find((a) => a.name === name)?.status === 'revoked'

  // --- pool composition treemap -------------------------------------------

  const categories: [string, number, string, number][] = [
    ['Spent', T.spent, '#0b1f9c', 1],
    ['In escrow', T.escrow, '#2c63e6', 1],
    ['Mandated', T.mandated, '#8fb0f2', 0.82],
    ['Unallocated', T.free, '#dfe4f3', 0.45],
  ]
  const bounds: [number, number][] = []
  let acc = 0
  for (const c of categories) {
    const n = T.total > 0 ? Math.round((c[1] / T.total) * DOT_N) : 0
    bounds.push([acc, acc + n])
    acc += n
  }
  if (T.total > 0) bounds[3]![1] = DOT_N

  const dots: { bg: string; op: number; tf: string; tr: string; enter: () => void }[] = []
  for (let r = 0; r < DOT_ROWS; r++) {
    for (let c = 0; c < DOT_COLS; c++) {
      const i = c * DOT_ROWS + r
      let k = 3
      if (T.total > 0) {
        for (let j = 0; j < 4; j++) {
          if (i >= bounds[j]![0] && i < bounds[j]![1]) {
            k = j
            break
          }
        }
      }
      const cat = T.total > 0 ? categories[k]! : ['', 0, '#eef0f4', 0.4] as const
      const lit = T.total > 0 && hoverDot === k
      const anyHover = hoverDot != null && T.total > 0
      const span = Math.max(bounds[k]![1] - bounds[k]![0], 1)
      const within = (i - bounds[k]![0]) / span
      const enter = () => {
        if (T.total > 0) setHoverDot(k)
      }

      if (!anyHover) {
        dots.push({
          bg: cat[2],
          op: 1,
          tf: `scale(${cat[3]})`,
          tr: `background .45s ease ${c * 4}ms,transform .5s cubic-bezier(.2,.8,.2,1) ${c * 4}ms,opacity .35s ease`,
          enter,
        })
      } else if (lit) {
        const delay = Math.round(within * 260 + r * 18)
        dots.push({
          bg: ['#1230d6', '#2c63e6', '#4c86ee', '#8fb0f2'][k]!,
          op: 1,
          tf: 'scale(1.12)',
          tr: `background .35s ease ${delay}ms,transform .55s cubic-bezier(.3,1.6,.5,1) ${delay}ms,opacity .3s ease`,
          enter,
        })
      } else {
        const delay = Math.round(Math.abs(c - DOT_COLS / 2) * 3)
        dots.push({
          bg: cat[2],
          op: 0.22,
          tf: `scale(${(cat[3] * 0.7).toFixed(2)})`,
          tr: `background .3s ease,transform .45s cubic-bezier(.65,0,.35,1) ${delay}ms,opacity .4s ease ${delay}ms`,
          enter,
        })
      }
    }
  }

  const segs = categories.map((c, j) => {
    const w = T.total > 0 ? (bounds[j]![1] - bounds[j]![0]) / DOT_N : 0
    const left = T.total > 0 ? Math.min((bounds[j]![0] / DOT_N) * 100, 88) : j * 25
    const anyHover = hoverDot != null && T.total > 0
    const on = anyHover && hoverDot === j
    return {
      enter: () => {
        if (T.total > 0) setHoverDot(j)
      },
      op: anyHover && !on ? 0.35 : 1,
      sq: ['#0b1f9c', '#2c63e6', '#8fb0f2', '#c9d2ef'][j]!,
      pc: on ? '#1230d6' : '#6b6e78',
      label: c[0],
      pct: T.total > 0 ? `${Math.round(w * 100)}%` : '0%',
      left: `${left}%`,
      display: T.total === 0 || w > 0.035 ? 'flex' : 'none',
    }
  })

  const ticks = Array.from({ length: 11 }, (_, i) => ({
    label: i === 10 ? '100%' : String(i * 10),
    c: i === 0 || i === 10 ? 'transparent' : '#cfd2d9',
  }))

  // --- headline stats ------------------------------------------------------

  const liveMandates = ledger.agents.filter((a) => a.status !== 'revoked').length
  const settlements = ledger.agents.reduce((t, a) => t + a.txs, 0)

  const stats = [
    { label: 'Mandated', value: money(T.mandated), sub: `${liveMandates} live mandates`, color: '#8b8e97' },
    {
      label: 'In escrow',
      value: money(T.escrow),
      sub: `${ledger.sessions.length} metered ${ledger.sessions.length === 1 ? 'session' : 'sessions'}`,
      color: '#1230d6',
    },
    { label: 'Spent', value: money(T.spent), sub: `${settlements.toLocaleString()} settlements`, color: '#0d0e12' },
    { label: 'Unallocated', value: money(T.free), sub: 'available to mandate', color: '#d9dae0' },
  ].map((x, k) => {
    const on = hoverStat === k
    return {
      ...x,
      border: '0',
      enter: () => setHoverStat(k),
      clip: on ? 'inset(0 0 0 0)' : 'inset(100% 0 0 0)',
      bar: on ? 'scaleX(1)' : 'scaleX(0)',
      labC: on ? '#fff' : '#6b6e78',
      valC: on ? '#fff' : '#0d0e12',
      subC: on ? 'rgba(255,255,255,.85)' : '#8b8e97',
      arO: on ? 1 : 0,
      arT: on ? 'translate(0,0)' : 'translate(-4px,4px)',
      sw: on ? '0 0 0 1.5px #fff' : 'none',
    }
  })

  // --- agents --------------------------------------------------------------

  const heldFor = (agentId: string) =>
    ledger.sessions.filter((x) => x.agentId === agentId).reduce((q, x) => q + x.escrow, 0)

  const agentRows = ledger.agents.map((a) => {
    void keyring
    const known = keyFor(a.id)
    const held = heldFor(a.id)
    const pct = a.budget ? a.spent / a.budget : 0
    const revoked = a.status === 'revoked'
    const hot = pct >= 0.95 && !revoked

    let statusLabel = 'Active'
    let statusColor = '#0d0e12'
    if (revoked) {
      statusLabel = 'Revoked'
      statusColor = '#ff4d1a'
    } else if (a.status === 'paused') {
      statusLabel = 'Paused'
      statusColor = '#8b8e97'
    } else if (pct >= 0.95) {
      statusLabel = 'Near limit'
      statusColor = '#ff4d1a'
    } else if (held > 0 || now - a.last < 60e3) {
      statusLabel = 'Metering'
      statusColor = '#1230d6'
    }

    const TICKS = 50
    const spentTicks = Math.round(Math.min(pct, 1) * TICKS)
    const escrowTicks =
      revoked || a.budget === null
        ? 0
        : Math.max(held > 0 ? 1 : 0, Math.round(Math.min(held / a.budget, 1) * TICKS))

    const runway = () => {
      if (revoked) return '—'
      if (a.status === 'paused') return 'paused'
      if (a.budget === null) return 'uncapped'
      const rate = a.spent / Math.max(1, a.txs / 12)
      const days = Math.max(a.budget - a.spent - held, 0) / Math.max(rate, 1)
      return days < 1 ? '< 1 day runway' : `≈ ${Math.round(days)}d runway`
    }

    return {
      id: a.id,
      name: a.name,
      purpose: a.purpose,
      budgetF: a.budget === null ? 'uncapped' : money(a.budget),
      spentF: money(a.spent),
      pctF: `${Math.round(pct * 100)}%`,
      spentW: `${Math.min(pct * 100, 100)}%`,
      escW: `${
        revoked || a.budget === null
          ? 0
          : Math.min((held / a.budget) * 100, 100 - Math.min(pct * 100, 100))
      }%`,
      barColor: revoked ? '#c4c8d4' : hot ? '#ff4d1a' : '#1230d6',
      statusLabel,
      statusColor,
      lastF: relative(now - a.last),
      opacity: revoked ? 0.5 : 1,
      canRevoke: !revoked,
      isRevoked: revoked,
      // Only a key this browser issued can be handed back; nothing else
      // holds one.
      keyKnown: !revoked && known !== null,
      keyMask: known ? masked(known) : '',
      copyKey: () => {
        if (!known) return
        void navigator.clipboard?.writeText(known)
        notify(`Key for ${a.name} copied`, a.id)
      },
      canPause: !revoked && config?.features.pause === true,
      pauseLabel: a.status === 'paused' ? 'Resume' : 'Pause',
      togglePause: () => {
        const paused = a.status !== 'paused'
        api
          .pause(a.id, paused)
          .then(() => {
            void refresh()
            notify(`Mandate ${a.id} ${paused ? 'paused' : 'resumed'}`, a.name)
          })
          .catch((error: unknown) => notify('Refused', explain(error), '#ff4d1a'))
      },
      revoke: () => {
        api
          .revoke(a.id)
          .then(() => {
            forget(a.id)
            setKeyring((n) => n + 1)
            void refresh()
            notify(`Mandate ${a.id} revoked · ${a.name}`, 'the key no longer opens', '#ff4d1a')
          })
          .catch((error: unknown) => notify('Revoke refused', explain(error), '#ff4d1a'))
      },
      ticks: Array.from({ length: TICKS }, (_, i) => {
        const major = i % 10 === 0
        if (i < spentTicks) {
          return { bg: revoked ? '#c4c6cc' : hot ? '#ff4d1a' : '#1230d6', h: `${major ? 100 : 78}%` }
        }
        if (i < spentTicks + escrowTicks) return { bg: '#1230d6', h: '100%' }
        return { bg: '#d0d2d8', h: `${major ? 46 : 24}%` }
      }),
      leftF: revoked
        ? 'mandate nullified'
        : a.budget === null
          ? 'no ceiling'
          : `${money(Math.max(a.budget - a.spent - held, 0))} left`,
      runway: runway(),
      runColor: pct >= 0.95 && !revoked ? '#ff4d1a' : '#8b8e97',
      ...avatar(a.name, revoked),
    }
  })

  const nameOf = (id: string) => ledger.agents.find((a) => a.id === id)?.name ?? '?'

  const sessionRows = ledger.sessions.map((x) => ({
    id: x.id,
    agent: nameOf(x.agentId),
    vendor: x.vendor,
    meteredF: money(x.metered),
    escrowF: money(x.escrow),
    tokens: compact(x.tokens),
    age: relative(now - x.started).replace(' ago', ''),
    pctF: `${Math.round((x.metered / x.escrow) * 100)}%`,
    fillW: `${Math.min(100, (x.metered / x.escrow) * 100).toFixed(1)}%`,
    refundF: money(Math.max(x.escrow - x.metered, 0)),
    // The refund figure fades out as the escrow is consumed.
    refundOp: x.metered / x.escrow > 0.78 ? 0 : 1,
    rate: compact(Math.round(x.tokens / Math.max(5, (now - x.started) / 1000))),
    ...avatar(nameOf(x.agentId), isRevoked(nameOf(x.agentId))),
  }))

  const feedRows = ledger.feed.slice(0, 7).map((f) => ({
    clock: clock(f.t),
    agent: f.agent,
    vendor: f.vendor,
    kindLabel: KIND_LABEL[f.kind],
    rel: relative(now - f.t),
    amountF: AMOUNT_SIGN[f.kind] + money(f.amount),
    amtColor:
      f.kind === 'deposit'
        ? '#1230d6'
        : f.kind === 'escrow'
          ? '#8b8e97'
          : f.kind === 'revoke'
            ? '#ff4d1a'
            : '#0d0e12',
    bg: f.id === ledger.selectedTx ? '#eef1ff' : 'transparent',
    select: () => setLedger((l) => ({ ...l, selectedTx: f.id })),
    ...avatar(f.agent, isRevoked(f.agent)),
  }))

  // --- selective disclosure ------------------------------------------------

  const otherTx = ledger.agents.reduce((t, a) => t + a.txs, 0)
  let scopeIn = '—'
  let scopeOut = '—'
  if (scope === 'tx') {
    scopeIn = selected
      ? `${selected.id} — ${selected.agent} → ${selected.vendor}, ${money(selected.amount)} USDC`
      : 'Select a payment in the feed.'
    scopeOut = `${otherTx.toLocaleString()} other settlements, all balances, all other agents.`
  } else if (scope === 'agent') {
    scopeIn = auditAgent
      ? `${auditAgent.name} — ${auditAgent.txs} settlements under ${auditAgent.id}`
      : 'No agents yet.'
    scopeOut = `${Math.max(ledger.agents.length - 1, 0)} other agents, pool balance, unallocated funds.`
  } else {
    scopeIn = 'All settlements, 1 Oct – 28 Oct 2026, aggregated per agent.'
    scopeOut = 'Everything outside the period; note openings and pool balance.'
  }

  const canProve =
    !proving &&
    ((scope === 'tx' && !!selected) ||
      (scope === 'agent' && !!auditAgent) ||
      (scope === 'range' && ledger.feed.length > 0))

  const genProof = () => {
    if (!canProve) return
    if (!config?.features.disclose) {
      notify('Viewing proofs are not served yet', 'the daemon reports no disclosure endpoint', '#8b8e97')
      return
    }
    setProving(true)
    const label =
      scope === 'tx'
        ? `${selected!.id} · single payment`
        : scope === 'agent'
          ? `${auditAgent!.name} · full mandate history`
          : 'Oct 2026 · period report'
    window.setTimeout(() => {
      setProving(false)
      setLedger((l) => ({
        ...l,
        proofs: [{ id: `vp_${randomHex(8)}`, label, t: Date.now() }, ...l.proofs],
      }))
      notify('Viewing proof ready', 'scoped key + zk proof · verifier link copied')
    }, PROOF_MS)
  }

  // --- funnel + mini timeline ---------------------------------------------

  const openedN = ledger.feed.filter((f) => f.kind === 'escrow').length + ledger.sessions.length
  const settledN = ledger.feed.filter((f) => f.kind === 'settled').length
  const tokensAll = ledger.sessions.reduce((t, x) => t + x.tokens, 0)

  const stages = [
    { n: '01', value: String(openedN), label: '402 quoted', h: '176px', bg: '#f4f6fd', fg: '#0d0e12', border: '0', arrow: 'block' },
    { n: '02', value: money(T.escrow), label: 'escrow locked', h: '200px', bg: '#0d0e12', fg: '#fff', border: '0', arrow: 'block' },
    { n: '03', value: compact(tokensAll), label: 'tokens metered', h: '164px', bg: '#1230d6', fg: '#fff', border: '0', arrow: 'block' },
    { n: '04', value: String(settledN), label: 'settled once', h: '188px', bg: '#fff', fg: '#0d0e12', border: '1px solid #0d0e12', arrow: 'none' },
  ].map((x, k) => {
    const on = hoverStage === k
    return {
      ...x,
      enter: () => setHoverStage(k),
      clip: on ? 'inset(0 0 0 0)' : 'inset(100% 0 0 0)',
      prog: `${((k + 1) / 4) * 100}%`,
      nc: on ? '#fff' : '#8b8e97',
      vc: on ? '#fff' : '#0d0e12',
      lc: on ? 'rgba(255,255,255,.85)' : '#6b6e78',
      arO: k < 3 ? 1 : 0,
    }
  })

  const tlCols = Array.from({ length: TL_COLS }, (_, c) => {
    const t1 = now - (TL_COLS - 1 - c) * TL_BUCKET_MS
    const t0 = t1 - TL_BUCKET_MS
    const inBucket = ledger.feed.filter((f) => f.t > t0 && f.t <= t1)
    const sum = inBucket
      .filter((f) => f.kind === 'settled' || f.kind === 'refund')
      .reduce((q, f) => q + f.amount, 0)
    const n = sum > 0 ? Math.min(TL_ROWS, Math.ceil(sum / 8)) : 0
    const sel = inBucket.some((f) => f.id === ledger.selectedTx)
    const rv = inBucket.some((f) => f.kind === 'revoke')
    const esc = inBucket.some((f) => f.kind === 'escrow')
    return {
      sum,
      h: '0%',
      bg: '',
      cap: '',
      mkBg: rv ? '#ff4d1a' : '#fff',
      mkBd: rv ? 'none' : 'inset 0 0 0 1.5px #1230d6',
      mkO: esc || rv ? 1 : 0,
      dots: Array.from({ length: TL_ROWS }, (_, r) => {
        const fromBottom = TL_ROWS - 1 - r
        if (fromBottom < n) return { bg: sel ? '#1230d6' : '#4c86ee', s: 1 }
        if (fromBottom === n && rv) return { bg: '#ff4d1a', s: 1 }
        if (fromBottom === n && esc && n === 0) return { bg: '#1230d6', s: 0.6 }
        return { bg: '#e3e5ea', s: 0.4 }
      }),
    }
  })

  // The bars show the running total, so the series only ever climbs.
  let cumulative = 0
  const running = tlCols.map((x) => (cumulative += x.sum))
  const peak = Math.max(1, cumulative)
  tlCols.forEach((x, i) => {
    const v = running[i]! / peak
    x.h = `${(running[i]! > 0 ? Math.max(3, v * 92) : 0).toFixed(1)}%`
    x.cap = running[i]! > 0 ? (x.sum > 0 ? '#1230d6' : '#8fb0f2') : 'transparent'
    x.bg =
      x.sum > 0
        ? 'linear-gradient(to top,rgba(44,99,230,.5),rgba(76,134,238,.2))'
        : 'linear-gradient(to top,rgba(44,99,230,.32),rgba(143,176,242,.1))'
  })

  // --- stacked budget columns ---------------------------------------------

  const agentCols = ledger.agents.map((a) => {
    const held = heldFor(a.id)
    const pct = a.budget ? a.spent / a.budget : 0
    const revoked = a.status === 'revoked'
    const hot = pct >= 0.95 && !revoked
    const sp = Math.min(pct, 1) * 100
    const es = revoked || a.budget === null ? 0 : Math.min((held / a.budget) * 100, 100 - sp)

    let sBg = '#0d0e12'
    let sBorder = '0'
    let sIcon = '✓'
    if (revoked) sBg = '#ff4d1a'
    else if (a.status === 'paused') {
      sBg = '#fff'
      sBorder = '1px solid #8b8e97'
    } else if (hot) {
      sBg = '#ff4d1a'
      sIcon = '!'
    } else if (held > 0) {
      sBg = '#1230d6'
      sIcon = '↻'
    }

    return {
      name: a.name,
      pctF: `${Math.round(pct * 100)}%`,
      spentH: `${sp}%`,
      escH:
        es > 0 || (!revoked && held > 0)
          ? `min(calc(${es}% + 18px), max(0px, calc(${100 - sp}% - 10px)))`
          : '0px',
      grad: revoked ? '#d0d2d8' : hot ? '#ff4d1a' : '#0d0e12',
      head: revoked ? '#fff' : hot ? '#ff4d1a' : '#0d0e12',
      headBorder: revoked ? '2px solid #c9ccd3' : '0',
      op: revoked ? 0.55 : 1,
      sBg,
      sFg: '#fff',
      sBorder,
      sIcon,
      ghosts: [25, 50, 75, 100].map((b) => ({ b: `${b}%` })),
    }
  })

  // --- range projection chart ---------------------------------------------

  const pctOfCeiling = (v: number) => `${((Math.min(v, RANGE_CEILING) / RANGE_CEILING) * 100).toFixed(2)}%`

  const rangeCols = ledger.agents.map((a, i) => {
    const held = heldFor(a.id)
    const revoked = a.status === 'revoked'
    const paused = a.status === 'paused'
    const dead = revoked || paused
    const pct = a.budget ? a.spent / a.budget : 0

    const daily = a.spent / 20
    const projected =
      dead || a.budget === null
        ? pct
        : Math.min(RANGE_CEILING, (a.spent + held + daily * 8) / a.budget)
    const lo = Math.min(pct, RANGE_CEILING)
    const hi = Math.max(projected, lo + 0.02)

    const todayCell = Math.min(RANGE_STEPS - 1, Math.floor(lo / RANGE_STEP + 1e-9))
    const projCell = dead
      ? todayCell
      : Math.min(RANGE_STEPS - 1, Math.max(todayCell, Math.ceil(hi / RANGE_STEP - 1e-9) - 1))
    const over = projCell >= RANGE_OVER_CELL && !dead

    const cells = Array.from({ length: RANGE_STEPS }, (_, k) => {
      const idx = RANGE_STEPS - 1 - k
      if (idx < todayCell) return { s: '6px', bg: dead ? '#e3e5ea' : '#b9c6f7', bd: '0' }
      if (idx === todayCell) {
        const ring = revoked ? '#c4c8d4' : paused ? '#8b8e97' : idx >= RANGE_OVER_CELL ? '#ff4d1a' : '#1230d6'
        return { s: '12px', bg: '#fff', bd: `2px solid ${ring}` }
      }
      if (idx <= projCell) return { s: '12px', bg: idx >= RANGE_OVER_CELL ? '#ff4d1a' : '#1230d6', bd: '0' }
      return { s: '0px', bg: 'transparent', bd: '0' }
    })

    const span = Math.max(hi, lo, 0.04)
    const barFrac = Math.min(span, RANGE_CEILING) / RANGE_CEILING
    const barPx = `${Math.round(barFrac * 250)}px`
    const overCeiling = !dead && hi > 1
    const barBg = dead
      ? 'linear-gradient(to top,#d6d9e2,rgba(214,217,226,0))'
      : overCeiling
        ? `linear-gradient(to top,#2c63e6 0%,#4c86ee ${Math.round(100 / span)}%,rgba(255,122,77,.55) ${Math.round(100 / span)}%,rgba(255,122,77,0) 100%)`
        : 'linear-gradient(to top,#2c63e6 0%,#4c86ee 45%,rgba(143,176,242,.35) 85%,rgba(143,176,242,0) 100%)'

    const projTop = dead ? lo : Math.min(hi, 1)
    const on = hoverRange === i
    const dim = hoverRange != null && !on
    const total = ledger.agents.length || 1

    return {
      tagF: revoked ? 'REVOKED' : paused ? 'PAUSED' : `TODAY ${Math.round(pct * 100)}%`,
      todayW: `${Math.min(pct * 100, 100).toFixed(1)}%`,
      barH: `${(barFrac * 100).toFixed(2)}%`,
      barPx,
      todayB: `${(span > 0 ? Math.min(lo / span, 1) * 100 : 0).toFixed(1)}%`,
      barBg,
      todayF: `${Math.round(pct * 100)}%`,
      spentF: money(a.spent),
      budF: a.budget === null ? 'uncapped' : money(a.budget),

      spH: pctOfCeiling(lo),
      spC: dead ? '#e3e5ea' : '#b9c6f7',
      pjB: pctOfCeiling(lo),
      pjH: dead || projTop <= lo ? '0%' : `calc(${pctOfCeiling(projTop)} - ${pctOfCeiling(lo)})`,
      ovB: pctOfCeiling(Math.max(lo, 1)),
      ovH:
        !dead && hi > 1
          ? `calc(${pctOfCeiling(hi)} - ${pctOfCeiling(Math.max(lo, 1))})`
          : '0%',
      mk: dead ? '14px' : '16px',
      mkC: revoked ? '#c4c8d4' : paused ? '#8b8e97' : lo >= 1 ? '#ff4d1a' : '#1230d6',
      labB: pctOfCeiling(Math.max(hi, lo)),
      name: a.name,
      cells,
      labTop: `${(RANGE_STEPS - 1 - projCell) * 14 - 1}px`,
      projF: `${Math.round(projected * 100)}%`,
      lc: dead ? '#8b8e97' : over ? '#ff4d1a' : '#1230d6',

      cx: `${((i * PLOT_GROUP + 4.5) / (ledger.agents.length * PLOT_GROUP || 1)) * 100}%`,
      grow: rangeIn ? 'scaleY(1)' : 'scaleY(0)',
      dl: `${i * 90}ms`,
      dl2: `${i * 90 + 450}ms`,
      dl3: `${i * 70 + 520}ms`,
      mkS: rangeIn ? (on ? 1.2 : 1) : 0,
      labO: rangeIn ? 1 : 0,
      labBg: 'transparent',
      conH: `max(0px, calc(318px - ${barPx}))`,
      cardL: `${(i / total) * 100}%`,
      cardW: `${100 / total}%`,
      cardB: `calc(${barPx} + 6px)`,
      cFill: on ? '100% 100%' : '0% 100%',
      cTx: on ? '#fff' : '#0d0e12',
      cPc: on ? '#fff' : dead ? '#8b8e97' : over ? '#ff4d1a' : '#1230d6',
      cO: rangeIn ? (dim ? 0.5 : 1) : 0,
      cT: rangeIn ? 'translateY(0)' : 'translateY(10px)',
      cSh: on
        ? '0 18px 40px -16px rgba(18,48,214,.45),inset 0 0 0 1px rgba(18,48,214,.25)'
        : '0 1px 0 rgba(13,14,18,.02),inset 0 0 0 1px #e3e5ea',
      num: String(i + 1).padStart(2, '0'),
      barO: dim ? 0.45 : 1,
      tO: rangeIn ? 1 : 0,
      enter: () => setHoverRange(i),
      left: `${((i * PLOT_GROUP) / (ledger.agents.length * PLOT_GROUP || 1)) * 100}%`,
      w: `${(PLOT_GROUP / (ledger.agents.length * PLOT_GROUP || 1)) * 100}%`,
      bandO: on ? 1 : 0,
      bandS: on ? 'scaleY(1)' : 'scaleY(.6)',
      op: dim ? 0.35 : revoked ? 0.5 : 1,
      nc: on ? '#1230d6' : '#0d0e12',
      tk: on ? '#1230d6' : '#c9ccd3',
      ...avatar(a.name, revoked),
    }
  })

  // --- modals --------------------------------------------------------------

  const confirmFund = () => {
    const amount = amountOf(form.fundAmount)
    if (!amount || form.funding || !config) return
    setForm((f) => ({ ...f, funding: true }))
    const units = Math.round(amount * 10 ** config.decimals)
    api
      .deposit(units)
      .then(() => {
        setDepositing({
          stage: 'approving',
          amount: units,
          started: Math.floor(Date.now() / 1000),
          commitment: null,
          error: null,
        })
      })
      .catch((error: unknown) => {
        setForm((f) => ({ ...f, funding: false }))
        notify('Deposit refused', explain(error), '#ff4d1a')
      })
  }

  const confirmMandate = () => {
    const budget = amountOf(form.mBudget)
    const name = form.mName.trim() || `agent-${randomHex(2)}`
    if (!budget) {
      setForm((f) => ({ ...f, mError: 'Enter a budget.' }))
      return
    }
    if (config) {
      // Pool units, not whole tokens: the daemon counts in the token's own
      // smallest unit and the form collects dollars.
      api
        .issue({
          label: name,
          budget: Math.round(budget * 10 ** config.decimals),
          models: form.mEndpoints.length ? form.mEndpoints : undefined,
        })
        .then((granted) => {
          remember(granted.id, granted.key)
          setKeyring((n) => n + 1)
          setIssued({ id: granted.id, name, key: granted.key })
          void refresh()
          setForm((f) => ({ ...f, mName: '', mError: '' }))
        })
        .catch((error: unknown) => setForm((f) => ({ ...f, mError: explain(error) })))
      return
    }
    const id = `mnd_${randomHex(6)}`
    setMandateOpen(false)
    setForm((f) => ({ ...f, mName: '', mError: '' }))
    setLedger((l) => ({
      ...l,
      auditAgent: l.auditAgent ?? id,
      agents: [
        ...l.agents,
        {
          id,
          name,
          purpose: `${form.mEndpoints.length} endpoints allowed`,
          budget,
          spent: 0,
          status: 'active' as const,
          last: Date.now(),
          txs: 0,
        },
      ],
    }))
    notify(
      `Mandate ${id} issued to ${name}`,
      `spending key derived · budget ${money(budget)} USDC · hidden on-chain`,
    )
  }

  // --- assembled bindings --------------------------------------------------

  const [balInt, balDec] = money(ledger.balance).split('.') as [string, string]
  const sessionSpend = ledger.feed
    .filter((f) => f.kind === 'settled' || f.kind === 'refund')
    .reduce((t, f) => t + f.amount, 0)
  const [settledInt, settledDec] = money(sessionSpend).split('.') as [string, string]
  const [escInt, escDec] = money(T.escrow).split('.') as [string, string]
  const budgetAll = ledger.agents.reduce((t, a) => t + (a.budget ?? 0), 0)

  return {
    goHome: (e: MouseEvent) => {
      e.preventDefault()
      transition.go('/', 'VEIL402.XYZ', { x: e.clientX, y: e.clientY })
    },
    navigate,
    playgroundUrl: PLAYGROUND_URL,

    balInt,
    balDec,
    balG: hoverBalance ? '100% 100%' : '0% 100%',
    balOn: () => setHoverBalance(true),
    balOff: () => setHoverBalance(false),
    noteCount: ledger.notes,
    anonSet: ledger.leaves.toLocaleString(),
    lastDeposit: ledger.lastDeposit
      ? `↑ ${money(ledger.lastDeposit.amount)} · ${relative(now - ledger.lastDeposit.t)}`
      : 'no deposits yet',
    root: ledger.root ?? shortHash(randomHex(8) + String(ledger.notes * 7919) + 'a1'),
    totalFunded: money(T.total),
    dotUnit: T.total > 0 ? `${money(T.total / DOT_N)} USDC` : 'one unit',

    dots,
    dotOff: () => setHoverDot(null),
    segs,
    ticks,
    stats,
    statOff: () => setHoverStat(null),

    agentRows,
    agentN: ledger.agents.length,
    agentA: ledger.agents.filter((a) => a.status === 'active').length,
    hasAgents: ledger.agents.length > 0,
    noAgents: ledger.agents.length === 0,
    agentCols,
    spentPctAll: budgetAll ? Math.round((T.spent / budgetAll) * 100) : 0,

    rangeCols,
    rangeOff: () => setHoverRange(null),
    chartRange: agentChart === 'Range',
    chartColumns: agentChart !== 'Range',

    stages,
    stOff: () => setHoverStage(null),
    tlCols,

    hasSessions: ledger.sessions.length > 0,
    noSessions: ledger.sessions.length === 0,
    sessionRows,
    sessionCount: ledger.sessions.length,
    sessionWord: ledger.sessions.length === 1 ? 'session' : 'sessions',
    settledInt,
    settledDec,
    escInt,
    escDec,

    hasFeed: ledger.feed.length > 0,
    noFeed: ledger.feed.length === 0,
    feedRows,
    liveLabel: offline ? 'OFFLINE' : 'LIVE',
    livePulse: createElement('span', {
      style: {
        width: 7,
        height: 7,
        borderRadius: '50%',
        background: '#1230d6',
        display: 'inline-block',
        animation: offline ? 'none' : 'vpulse 1.6s infinite',
      },
    }),
    cursor: createElement(
      'span',
      { style: { animation: 'vblink 1s steps(1) infinite', color: '#ff4d1a' } },
      '_',
    ),

    hasSel: !!selected,
    chainMini: selected
      ? [
          { k: 'Commitment', v: shortHash(selected.commit) },
          { k: 'Nullifier', v: shortHash(selected.nullifier) },
          { k: 'Amount', v: selected.kind === 'deposit' ? money(selected.amount) : 'hidden' },
        ]
      : [
          { k: 'Commitment', v: '—' },
          { k: 'Nullifier', v: '—' },
          { k: 'Amount', v: 'hidden' },
        ],
    selV: selected
      ? {
          agent: selected.agent,
          vendor: selected.vendor,
          amountF: money(selected.amount),
          meta: `${selected.id} · ${KIND_LABEL[selected.kind]}`,
        }
      : { agent: 'No payment yet', vendor: '—', amountF: '0.00', meta: 'select a payment in the feed' },

    scopeCards: (
      [
        ['tx', '01', 'Single payment', 'One settlement, verifiable on its own.'],
        ['agent', '02', 'One agent', 'Every spend under one mandate.'],
        ['range', '03', 'One period', 'All settlements in a date range.'],
      ] as const
    ).map(([key, num, label, desc]) => ({
      num,
      label,
      desc,
      op: scope === key ? 1 : 0.6,
      bar: scope === key ? '#fff' : 'rgba(255,255,255,.35)',
      pick: () => setScope(key),
    })),
    scopeAgent: scope === 'agent' && ledger.agents.length > 0,
    scopeIn,
    scopeOut,
    auditAgent: auditAgent?.id ?? '',
    agentOptions: ledger.agents.map((a) => ({ id: a.id, name: a.name })),
    pickAuditAgent: (e: ChangeEvent<HTMLSelectElement>) =>
      setLedger((l) => ({ ...l, auditAgent: e.target.value })),

    proofBtnLabel: proving
      ? 'Generating viewing proof…'
      : config?.features.disclose
        ? 'Generate viewing proof'
        : 'Viewing proof — not served yet',
    proofBtnOpacity: canProve || proving ? 1 : 0.5,
    genProof,
    hasProofs: ledger.proofs.length > 0,
    proofRows: ledger.proofs.slice(0, 3).map((p) => ({
      id: p.id,
      label: p.label,
      rel: relative(now - p.t),
    })),

    step1Label: ledger.balance > 0 ? 'Funded ✓  Add more' : 'Fund pool',
    step2Opacity: ledger.balance > 0 ? 1 : 0.45,

    fundOpen,
    mandateOpen,
    openFund: () => {
      setForm((f) => ({ ...f, fundAmount: String(ledger.depositable) }))
      setFundOpen(true)
    },
    openMandate: () => {
      setMandateOpen(true)
      setForm((f) => ({ ...f, mError: '' }))
    },
    closeModals: () => {
      setFundOpen(false)
      setMandateOpen(false)
    },
    stop: (e: MouseEvent) => e.stopPropagation(),

    fundAmount: form.fundAmount,
    depositableF: `${money(ledger.depositable)} USDC`,
    walletF: `${money(ledger.depositable)} unshielded in wallet`,
    setFundAmount: (e: ChangeEvent<HTMLInputElement>) =>
      setForm((f) => ({ ...f, fundAmount: e.target.value })),
    fundPresets: shares(ledger.depositable).map(({ label, amount }) => ({
      label,
      pick: () => setForm((f) => ({ ...f, fundAmount: amount })),
    })),
    budgetPresets: shares(T.free > 0 ? T.free : ledger.balance).map(({ label, amount }) => ({
      label,
      pick: () => setForm((f) => ({ ...f, mBudget: amount })),
    })),
    depositing: !!depositing,
    depositDone: depositing?.stage === 'done',
    depositFailed: depositing?.stage === 'failed',
    depositError: depositing?.error ?? '',
    depositAmountF: depositing ? money(depositing.amount / 10 ** (config?.decimals ?? 6)) : '',
    depositSteps: STEPS.map((step, i) => {
      const at = depositing ? STEPS.findIndex((x) => x.key === depositing.stage) : -1
      const finished = depositing?.stage === 'done' || (at >= 0 && i < at)
      const active = at === i && depositing?.stage !== 'failed'
      const broke = depositing?.stage === 'failed' && at === i
      return {
        label: step.label,
        note: step.note,
        mark: finished ? '✓' : broke ? '!' : String(i + 1).padStart(2, '0'),
        bg: finished ? '#1230d6' : active ? '#0d0e12' : broke ? '#ff4d1a' : '#fff',
        fg: finished || active || broke ? '#fff' : '#8b8e97',
        ring: finished ? '#1230d6' : broke ? '#ff4d1a' : '#e3e5ea',
        barW: finished ? '100%' : active ? '55%' : '0%',
        op: finished || active || broke ? 1 : 0.5,
      }
    }),
    closeDeposit: () => {
      setDepositing(null)
      setFundOpen(false)
      setForm((f) => ({ ...f, funding: false }))
    },
    fundBtnLabel: form.funding ? 'Shielding…' : `Shield ${form.fundAmount} USDC`,
    confirmFund,

    mName: form.mName,
    setMName: (e: ChangeEvent<HTMLInputElement>) => setForm((f) => ({ ...f, mName: e.target.value })),
    mBudget: form.mBudget,
    setMBudget: (e: ChangeEvent<HTMLInputElement>) =>
      setForm((f) => ({ ...f, mBudget: e.target.value })),
    mError: form.mError,
    issuedKey: issued?.key ?? '',
    issuedFor: issued ? `${issued.name} · ${issued.id}` : '',
    hasIssued: !!issued,
    copyIssued: () => {
      if (!issued) return
      void navigator.clipboard?.writeText(issued.key)
      notify('Key copied', issued.id)
    },
    doneIssued: () => {
      setIssued(null)
      setMandateOpen(false)
    },
    freeF:
      T.free > 0
        ? `${money(T.free)} USDC`
        // What is still promised, against what is still there. Spent money
        // has already left the pool and commits nothing.
        : `0.00 USDC · over-committed by ${money(T.mandated + T.escrow - ledger.balance)}`,
    endpointChips: (config?.models ?? []).map((ep) => {
      const on = form.mEndpoints.includes(ep)
      return {
        label: ep,
        bg: on ? '#0d0e12' : '#fff',
        fg: on ? '#fff' : '#0d0e12',
        border: on ? '#0d0e12' : '#e3e5ea',
        toggle: () =>
          setForm((f) => ({
            ...f,
            mEndpoints: on ? f.mEndpoints.filter((x) => x !== ep) : [...f.mEndpoints, ep],
          })),
      }
    }),
    confirmMandate,

    offline,

    toast,
  }
}

export type ConsoleBindings = ReturnType<typeof useConsoleBindings>
