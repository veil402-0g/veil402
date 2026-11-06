import { useCallback, useEffect, useRef, useState } from 'react'
import {
  AVATAR_MESH_POSITIONS,
  BUDGET_COLUMNS,
  HISTORY_ACCENT_RANGE,
  HISTORY_COLUMNS,
  MESH_GRADIENT,
  MUTED_GRADIENT,
  initials,
} from './data'
import { useFitScale } from '../ui/useFitScale'
import { useInView } from '../ui/useInView'
import { EASE } from '../motion/config'
import { usePinnedListHeight } from './usePinnedListHeight'

const STAGE_COUNT = 5
const DWELL_MS = 3500

/** The mockup is authored at this size and scaled to fit its container. */
const STAGE_W = 960
const STAGE_H = 680
const STAGE_EPSILON = 0.005

/** Budget chart geometry: 20 cells tall, full column is 125% of the limit. */
const CELL_PCT = 6.25
const OVER_LIMIT_CELL = 16
const CHART_H = 400
const CHART_CHROME = 58 + 4
const BAR_MAX_PX = 270

export interface BudgetColumnView {
  name: string
  lbl: string
  /** Column height as a share of the 125% ceiling. */
  barH: string
  /** Where today's spend sits inside the column. */
  todayB: string
  barBg: string
  grow: string
  dl: string
  dl2: string
  tO: number
  conH: string
  ini: string
  avBg: string
  avPos: string
  pc: string
  nc: string
}

export interface HistoryColumnView {
  d: { sz: string; bg: string; tf: string; dl: string }[]
}

function buildBudgetColumns(animate: boolean): BudgetColumnView[] {
  return BUDGET_COLUMNS.map((c, ci) => {
    const over = c.projected >= OVER_LIMIT_CELL && !c.muted
    const top = ((c.muted ? c.spent : c.projected) + 1) * CELL_PCT
    const today = c.spent * CELL_PCT
    const height = Math.min(top, 125) / 125
    const limitStop = Math.round((100 / top) * 100)

    const barBg = c.muted
      ? 'linear-gradient(to top,#d6d9e2,rgba(214,217,226,.1))'
      : top > 100
        ? `linear-gradient(to top,#2c63e6 0%,#4c86ee ${limitStop}%,rgba(255,122,77,.6) ${limitStop}%,rgba(255,122,77,.05) 100%)`
        : 'linear-gradient(to top,#2c63e6 0%,#4c86ee 45%,rgba(143,176,242,.35) 85%,rgba(143,176,242,.05) 100%)'

    return {
      name: c.name,
      lbl: c.label,
      barH: `${(height * 100).toFixed(1)}%`,
      todayB: `${((today / top) * 100).toFixed(1)}%`,
      barBg,
      grow: animate ? 'scaleY(1)' : 'scaleY(0)',
      dl: `${ci * 0.09}s`,
      dl2: `${0.4 + ci * 0.09}s`,
      tO: animate ? 1 : 0,
      conH: `max(0px, calc(${CHART_H - CHART_CHROME}px - ${Math.round(height * BAR_MAX_PX)}px))`,
      ini: initials(c.name),
      avBg: c.muted ? MUTED_GRADIENT : MESH_GRADIENT,
      avPos: AVATAR_MESH_POSITIONS[ci % AVATAR_MESH_POSITIONS.length]!,
      pc: over ? '#ff4d1a' : c.muted ? '#8b8e97' : '#1230d6',
      nc: c.muted ? '#8b8e97' : '#0d0e12',
    }
  })
}

function buildHistory(animate: boolean): HistoryColumnView[] {
  const [accentFrom, accentTo] = HISTORY_ACCENT_RANGE
  return HISTORY_COLUMNS.map((calls, ci) => ({
    d: Array.from({ length: 6 }, (_, r) => {
      const lit = r < calls
      const accent = lit && ci >= accentFrom && ci <= accentTo
      return {
        sz: lit ? '9px' : '4px',
        bg: lit ? (accent ? '#1230d6' : '#8fb0f2') : '#e3e5ea',
        tf: animate || !lit ? 'scale(1)' : 'scale(0)',
        dl: `${(0.3 + ci * 0.012 + r * 0.04).toFixed(3)}s`,
      }
    }),
  }))
}

const fitStage = (w: number, h: number) => Math.min(w / STAGE_W, h / STAGE_H)

type Stage = 1 | 2 | 3 | 4 | 5

/** The `sO1`/`a1Y`/`rC1`… names the stage markup reads, one set per stage. */
type StageStyles = Record<
  | `sO${Stage}` | `sT${Stage}` | `sF${Stage}`
  | `a${Stage}O` | `a${Stage}Y`
  | `rC${Stage}` | `rX${Stage}` | `rHc${Stage}` | `rNc${Stage}` | `rY${Stage}` | `rYd${Stage}`
  | `rTr${Stage}` | `rPg${Stage}` | `rPt${Stage}` | `rT${Stage}` | `rO${Stage}` | `rA${Stage}`
  | `cWipe${Stage}` | `cTxt${Stage}`,
  string | number
>
type StageHandlers = Record<`rOn${Stage}`, () => void>

/** Five stages on a dwell timer, restarted whenever a stage is picked by hand. */
export function useShowcase() {
  const [sectionRef, visible] = useInView()
  // 0 until the section is reached, so stage 01's reveal plays on arrival.
  const [act, setAct] = useState(0)
  const timer = useRef<number | undefined>(undefined)

  const stageScale = useFitScale('[data-stage]', fitStage, STAGE_EPSILON)
  usePinnedListHeight('[data-clist]')

  const startCycle = useCallback(() => {
    window.clearInterval(timer.current)
    timer.current = window.setInterval(() => setAct((n) => (n % STAGE_COUNT) + 1), DWELL_MS)
  }, [])

  useEffect(() => {
    if (!visible) return
    setAct((n) => n || 1)
    startCycle()
    return () => window.clearInterval(timer.current)
  }, [visible, startCycle])

  const pick = useCallback(
    (n: number) => () => {
      setAct(n)
      startCycle()
    },
    [startCycle],
  )

  const stages = Array.from({ length: STAGE_COUNT }, (_, i) => act === i + 1)
  const [, on2, on3, on4, on5] = stages as [boolean, boolean, boolean, boolean, boolean]

  const perStage = {} as StageStyles & StageHandlers
  stages.forEach((on, idx) => {
    const n = (idx + 1) as Stage
    perStage[`sO${n}`] = on ? 1 : 0
    perStage[`sT${n}`] = on ? 'translateY(0)' : 'translateY(14px)'
    perStage[`sF${n}`] = on ? 'blur(0px)' : 'blur(6px)'
    perStage[`a${n}O`] = on ? 1 : 0
    perStage[`a${n}Y`] = on ? 'translateY(0)' : 'translateY(10px)'
    perStage[`rC${n}`] = on ? 'inset(0 0 0 0)' : 'inset(0 100% 0 0)'
    perStage[`rX${n}`] = on ? '1fr' : '0fr'
    perStage[`rHc${n}`] = on ? '#0d0e12' : '#a3a6ae'
    perStage[`rNc${n}`] = on ? '#1230d6' : '#a3a6ae'
    perStage[`rY${n}`] = on ? 1 : 0
    perStage[`rYd${n}`] = on ? '.2s' : '0s'
    perStage[`rTr${n}`] = `clip-path ${on ? '.6s' : '.45s'} ${EASE.sweep}`
    perStage[`rPg${n}`] = on ? 'scaleX(1)' : 'scaleX(0)'
    perStage[`rPt${n}`] = on ? 'transform 3.5s linear .3s' : 'none'
    perStage[`rT${n}`] = on ? 'translateX(8px)' : 'translateX(0)'
    perStage[`rO${n}`] = on ? 1 : 0
    perStage[`rA${n}`] = on ? 'translateX(0)' : 'translateX(-10px)'
    perStage[`rOn${n}`] = pick(n)
    // The wipe/typewriter reveals ship disabled in this revision of the design.
    perStage[`cWipe${n}`] = 'none'
    perStage[`cTxt${n}`] = 'none'
  })

  return {
    consoleRef: sectionRef,

    actN: `0${act || 1}`,
    sk: `scale(${stageScale ?? 0.6})`,
    gT: visible ? 'translateY(0) scale(1)' : 'translateY(28px) scale(.96)',

    // Section headline / copy reveals.
    cWord: visible ? 'translateY(0)' : 'translateY(110%)',
    cHl: visible ? '100% 100%' : '0% 100%',
    cHc: visible ? '#1230d6' : '#5c5f69',
    cOp: visible ? 1 : 0,
    cUp: visible ? 'translateY(0)' : 'translateY(18px)',
    cLine: visible ? 'scaleX(1)' : 'scaleX(0)',
    cClip: visible ? 'inset(0 0 0 0)' : 'inset(100% 0 0 0)',
    cZoom: visible ? 'scale(1)' : 'scale(1.12)',

    ...perStage,

    // Stage 3 (proof) slides its two halves together.
    c3L: on3 ? 'translateX(0)' : 'translateX(60px)',
    c3R: on3 ? 'translateX(0)' : 'translateX(-60px)',
    c3P: on3 ? 'scale(1)' : 'scale(0)',
    // Stage 4 (settlement) pops its node and grows the connector.
    p4: on4 ? 'scale(1)' : 'scale(.6)',
    l4: on4 ? 'scaleY(1)' : 'scaleY(0)',
    // Stage 5 (history) pops the heatmap tiles.
    tile5: on5 ? 'translateY(0) scale(1)' : 'translateY(12px) scale(.85)',

    bCols: buildBudgetColumns(on2),
    hist: buildHistory(on5),
  }
}
