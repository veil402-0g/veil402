import { useCallback, useEffect, useRef, useState } from 'react'
import { PROTOCOL_SCENES, SCENE_HOP, SCENE_REACH, pillColours } from '../data/protocol'
import { useFitScale } from './useFitScale'
import { useInView } from './useInView'

const SCENE_COUNT = PROTOCOL_SCENES.length
const DWELL_MS = 3600
/** One frame of "settled at the start position" before the packet travels. */
const SETTLE_MS = 60

/** The diagram is authored at this size and scaled down to fit. */
const DIAGRAM_W = 1240
const DIAGRAM_H = 430
/** Below this container width the diagram stops shrinking and scrolls instead. */
const MIN_FIT_W = 700

const NODE_COUNT = 6
const TRAVEL = 'left 1.5s cubic-bezier(.65,0,.25,1), width 1.5s cubic-bezier(.65,0,.25,1), opacity .3s ease'

type NodeIdx = 0 | 1 | 2 | 3 | 4 | 5
type SceneIdx = 0 | 1 | 2 | 3 | 4 | 5 | 6

type NodeStyles = Record<
  | `nb${NodeIdx}` | `nbg${NodeIdx}` | `nic${NodeIdx}`
  | `capF${NodeIdx}` | `nan${NodeIdx}` | `nsh${NodeIdx}`,
  string
> &
  Record<`cap${NodeIdx}`, number>
type HopStyles = Record<`hb${NodeIdx}` | `hbg${NodeIdx}`, string>
type ChipStyles = Record<
  `pcf${SceneIdx}` | `pct${SceneIdx}` | `pgp${SceneIdx}` | `pgt${SceneIdx}` | `pcc${SceneIdx}`,
  string
>
type ChipHandlers = Record<`pc${SceneIdx}`, () => void>

const fitDiagram = (w: number) =>
  w >= MIN_FIT_W ? Math.min(1, w / DIAGRAM_W) : MIN_FIT_W / DIAGRAM_W

/** Seven scenes on a dwell timer; pausing freezes the timer, not the current scene. */
export function useProtocol() {
  // The headline comes in as the section crosses the viewport edge; the diagram
  // only starts once the section is properly on screen.
  const [headlineRef, headlineIn] = useInView(1)
  const [diagramRef, diagramIn] = useInView(0.8)

  const [scene, setScene] = useState(0)
  /** 0 while the packet sits at the scene's start, 1 once it travels. */
  const [settled, setSettled] = useState(1)
  const [paused, setPaused] = useState(false)

  const settleTimer = useRef<number | undefined>(undefined)
  const diagramScale = useFitScale('[data-pstage]', fitDiagram)

  const goTo = useCallback((n: number) => {
    setScene(n)
    setSettled(0)
    window.clearTimeout(settleTimer.current)
    settleTimer.current = window.setTimeout(() => setSettled(1), SETTLE_MS)
  }, [])

  const reduced =
    typeof window !== 'undefined' && window.matchMedia('(prefers-reduced-motion: reduce)').matches

  useEffect(() => {
    // Land on the final, fully-drawn frame instead of animating through.
    if (reduced) {
      setScene(SCENE_COUNT - 1)
      setSettled(1)
      setPaused(true)
      return
    }
    if (diagramIn) goTo(0)
  }, [diagramIn, reduced, goTo])

  // The dwell timer free-runs and skips while paused, so un-pausing resumes
  // mid-slice rather than granting a fresh one.
  const sceneRef = useRef(scene)
  sceneRef.current = scene
  const pausedRef = useRef(paused)
  pausedRef.current = paused

  useEffect(() => {
    if (!diagramIn || reduced) return
    const id = window.setInterval(() => {
      if (pausedRef.current) return
      goTo((sceneRef.current + 1) % SCENE_COUNT)
    }, DWELL_MS)
    return () => window.clearInterval(id)
  }, [diagramIn, reduced, goTo])

  useEffect(() => () => window.clearTimeout(settleTimer.current), [])

  const sc = PROTOCOL_SCENES[scene]!
  const last = scene === SCENE_COUNT - 1
  const x = settled ? sc.to : sc.from
  const running = settled === 1 && !paused
  const revealed = headlineIn || reduced

  const nodes = {} as NodeStyles
  const reach = SCENE_REACH[scene]!
  for (let n = 0; n < NODE_COUNT; n++) {
    const i = n as NodeIdx
    const active = sc.node === i || (i === 5 && last)
    // The shielded pool (index 5) only appears once a session is open.
    const reached = i < 5 ? i <= reach : scene >= 2
    nodes[`nb${i}`] = active || reached ? '#1230d6' : '#d0d2d8'
    nodes[`nbg${i}`] = active ? '#1230d6' : '#fff'
    nodes[`nic${i}`] = active ? '#fff' : reached ? '#1230d6' : '#a3a6ae'
    nodes[`cap${i}`] = active ? 1 : 0.28
    nodes[`capF${i}`] = active ? 'none' : 'grayscale(1)'
    nodes[`nan${i}`] = active ? 'vnpulse 2s ease-out infinite' : 'none'
    nodes[`nsh${i}`] = active
      ? '0 0 0 6px rgba(18,48,214,.12), 0 12px 24px -8px rgba(18,48,214,.5)'
      : 'none'
  }

  const hops = {} as HopStyles
  const hop = SCENE_HOP[scene]!
  for (let n = 0; n < NODE_COUNT; n++) {
    const i = n as NodeIdx
    const on = hop === i || last
    hops[`hb${i}`] = on ? (i === 4 ? '#ff4d1a' : '#1230d6') : '#e3e5ea'
    hops[`hbg${i}`] = hop === i ? '#f4f6fd' : '#fff'
  }

  const chips = { ...({} as ChipStyles), ...({} as ChipHandlers) }
  for (let n = 0; n < SCENE_COUNT; n++) {
    const i = n as SceneIdx
    const on = scene === i
    const chipRunning = on && running
    // Picking a scene leaves the paused state alone, so PAUSE stays honoured.
    chips[`pc${i}`] = () => goTo(i)
    chips[`pcf${i}`] = on
      ? 'inset(0 0 0 0)'
      : i < scene
        ? 'inset(0 0 calc(100% - 2px) 0)'
        : 'inset(0 100% 0 0)'
    chips[`pct${i}`] = on ? 'clip-path .55s cubic-bezier(.65,0,.25,1)' : 'clip-path .35s ease'
    chips[`pgp${i}`] = chipRunning ? 'scaleX(1)' : 'scaleX(0)'
    chips[`pgt${i}`] = chipRunning ? 'transform 3.4s linear .2s' : 'none'
    chips[`pcc${i}`] = chipRunning ? '#fff' : '#0d0e12'
  }

  // One stable callback: a fresh closure here would detach and re-attach the
  // observers on every render.
  const protoRef = useCallback(
    (el: Element | null) => {
      headlineRef(el)
      diagramRef(el)
    },
    [headlineRef, diagramRef],
  )

  const [pillText, pillBg, pillDot] = pillColours(sc.pill)
  const k = diagramScale ?? 1

  return {
    protoRef,

    // Travelling packet and the rail it leaves behind.
    pkL: `${x - 7}px`,
    pkO: last ? 0 : 1,
    pkTr: settled ? TRAVEL : 'none',
    pkT: sc.packet,
    pgW: `${last ? 810 : x - 70}px`,
    pgTr: settled ? TRAVEL : 'none',

    // The shielded note drops down while the pool scene is up.
    ntT: `${scene === 2 && settled ? 338 : 163}px`,
    ntO: scene === 2 ? 1 : 0,
    ntTr: settled ? 'top 1.5s cubic-bezier(.65,0,.25,1), opacity .3s ease' : 'none',
    brS: scene >= 2 ? 'scaleY(1)' : 'scaleY(0)',

    // The reply sweeps all the way back on the final scene.
    rpL: `${(last && settled ? 70 : 880) - 7}px`,
    rpO: last ? 1 : 0,
    rpTr: last && settled ? 'left 2.4s cubic-bezier(.65,0,.25,1), opacity .3s ease' : 'none',
    rLit: last ? 1 : 0,
    rLabC: last ? '#1230d6' : '#8b8e97',
    stC: scene === 2 || last ? '#1230d6' : '#8b8e97',
    teeO: scene >= 5 ? 1 : 0.35,

    ...nodes,
    ...hops,
    ...chips,

    pN: `0${scene + 1}`,
    pTitle: sc.title,
    pBody: sc.body,
    pPill: sc.pill,
    ppC: pillText,
    ppB: pillBg,
    ppD: pillDot,

    pK: `scale(${k})`,
    pW: `${Math.round(DIAGRAM_W * k)}px`,
    pH: `${Math.round(DIAGRAM_H * k)}px`,
    pLabel: paused ? 'PLAY' : 'PAUSE',
    pToggle: () => setPaused((p) => !p),

    // Section headline / copy reveals.
    vxLine: revealed ? 'scaleX(1)' : 'scaleX(0)',
    vxWord: revealed ? 'translateY(0)' : 'translateY(110%)',
    vxOp: revealed ? 1 : 0,
    vxUp: revealed ? 'translateY(0)' : 'translateY(18px)',
    vxRise: revealed ? 'translateY(0)' : 'translateY(40px)',
    vxHl: revealed ? '100% 100%' : '0% 100%',
    vxHc: revealed ? '#1230d6' : '#4d505a',
    vxClip: revealed ? 'inset(0 0 0 0)' : 'inset(0 50% 0 50%)',
  }
}
