import { useEffect, useState } from 'react'

/** Phrases in the hero paragraph that take the highlight in turn. */
const HIGHLIGHT_COUNT = 4
const HIGHLIGHT_START_MS = 1700
const HIGHLIGHT_EVERY_MS = 2200

export interface HeroIntroBindings {
  hLet: string
  hDot: string
  hLine: string
  hOp: number
  hUp: string
  hSq: string
  hHl1: string
  hHl2: string
  hHl3: string
  hHl4: string
  hHp1: string
  hHp2: string
  hHp3: string
  hHp4: string
}

/** The hero entrance, plus the highlight that rolls across the paragraph's four claims. */
export function useHeroIntro(): HeroIntroBindings {
  const [entered, setEntered] = useState(false)
  const [highlight, setHighlight] = useState(-1)

  useEffect(() => {
    // Two frames so the initial (pre-transition) styles are committed first.
    const raf = requestAnimationFrame(() => requestAnimationFrame(() => setEntered(true)))
    const fallback = window.setTimeout(() => setEntered(true), 120)
    return () => {
      cancelAnimationFrame(raf)
      window.clearTimeout(fallback)
    }
  }, [])

  useEffect(() => {
    let interval: number | undefined
    const start = window.setTimeout(() => {
      setHighlight(0)
      interval = window.setInterval(
        () => setHighlight((k) => (k + 1) % HIGHLIGHT_COUNT),
        HIGHLIGHT_EVERY_MS,
      )
    }, HIGHLIGHT_START_MS)
    return () => {
      window.clearTimeout(start)
      window.clearInterval(interval)
    }
  }, [])

  const lit = (n: number) => highlight === n - 1

  return {
    hLet: entered ? 'translateY(0) rotate(0)' : 'translateY(110%) rotate(6deg)',
    hDot: entered ? 'scale(1)' : 'scale(0)',
    hLine: entered ? 'translateY(0)' : 'translateY(105%)',
    hOp: entered ? 1 : 0,
    hUp: entered ? 'translateY(0)' : 'translateY(18px)',
    hSq: entered ? 'rotate(0) scale(1)' : 'rotate(90deg) scale(0)',
    hHl1: lit(1) ? '100%' : '0%',
    hHl2: lit(2) ? '100%' : '0%',
    hHl3: lit(3) ? '100%' : '0%',
    hHl4: lit(4) ? '100%' : '0%',
    // Sweeps in from the left while lit, retreats to the right when not.
    hHp1: lit(1) ? '0 50%' : '100% 50%',
    hHp2: lit(2) ? '0 50%' : '100% 50%',
    hHp3: lit(3) ? '0 50%' : '100% 50%',
    hHp4: lit(4) ? '0 50%' : '100% 50%',
  }
}
