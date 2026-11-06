import { useCallback, useState } from 'react'
import { useInView } from '../ui/useInView'

const POSTER_COUNT = 3

type Poster = 1 | 2 | 3

type PosterStyles = Record<`pzLift${Poster}` | `pzSh${Poster}`, string>
type PosterHandlers = Record<`pzOn${Poster}`, () => void>

/** Three poster cards; hovering one lifts it and animates the art inside it. */
export function usePosters() {
  const [sectionRef, visible] = useInView()
  // 0 means none hovered, so every poster sits at rest.
  const [hovered, setHovered] = useState(0)

  const leave = useCallback(() => setHovered(0), [])

  const cards = {} as PosterStyles & PosterHandlers
  for (let k = 1; k <= POSTER_COUNT; k++) {
    const n = k as Poster
    const on = hovered === n
    cards[`pzOn${n}`] = () => setHovered(n)
    cards[`pzLift${n}`] = on ? 'translateY(-8px)' : 'none'
    cards[`pzSh${n}`] = on
      ? '0 40px 70px -34px rgba(18,48,214,.55)'
      : '0 0 0 0 rgba(18,48,214,0)'
  }

  return {
    pzRef: sectionRef,
    pzW: visible ? 'translateY(0)' : 'translateY(110%)',
    pzLine: visible ? 'scaleX(1)' : 'scaleX(0)',
    pzOff: leave,
    ...cards,

    // Poster 1: the arc rises and its chip lifts off the ground.
    pzCirc: hovered === 1 ? '-50%' : '-62%',
    pzChip: hovered === 1 ? '24%' : '16%',
    pzChipSh: hovered === 1 ? '0 14px 30px -12px rgba(18,48,214,.5)' : '0 0 0 0 rgba(18,48,214,0)',

    // Poster 2: the frame tightens and the mark turns.
    pzX: hovered === 2 ? '8%' : '15%',
    pzY: hovered === 2 ? '12%' : '20%',
    pzC2: hovered === 2 ? 'translateY(-4px)' : 'none',
    pzSqR: hovered === 2 ? 'rotate(90deg)' : 'none',

    // Poster 3: the card grows and the ground drifts behind it.
    pzSq: hovered === 3 ? 'translateY(-50%) scale(1.04)' : 'translateY(-50%)',
    pzSqSh: hovered === 3 ? '0 30px 60px -24px rgba(13,30,120,.6)' : '0 0 0 0 rgba(13,30,120,0)',
    pzZoom: hovered === 3 ? 'scale(1.15) rotate(-4deg)' : 'none',
  }
}
