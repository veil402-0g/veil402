import { useCallback, useState } from 'react'
import { useInView } from './useInView'

const CARD_COUNT = 6

type Card = 1 | 2 | 3 | 4 | 5 | 6
type CardStyles = Record<
  | `uzA${Card}` | `uzB${Card}` | `uzI${Card}` | `uzK${Card}` | `uzX${Card}` | `uzH${Card}`
  | `uzD${Card}` | `uzR${Card}` | `uzT${Card}` | `uzL${Card}` | `uzC${Card}` | `uzP${Card}`
  | `uzZ${Card}`,
  string
>
type CardHandlers = Record<`uzOn${Card}`, () => void>

/** Hovering a card inverts it to the blue fill — a dozen colours, not one class. */
export function useUseCases() {
  const [sectionRef, visible] = useInView()
  const [hovered, setHovered] = useState(0)

  const leave = useCallback(() => setHovered(0), [])

  const cards = {} as CardStyles & CardHandlers
  for (let k = 1; k <= CARD_COUNT; k++) {
    const n = k as Card
    const on = hovered === n
    cards[`uzOn${n}`] = () => setHovered(n)
    cards[`uzA${n}`] = on ? '#fff' : 'transparent'
    cards[`uzB${n}`] = on ? '#fff' : '#d0d2d8'
    cards[`uzI${n}`] = on ? '#1230d6' : '#0d0e12'
    cards[`uzK${n}`] = on ? 'inset(0 0 0 0)' : 'inset(100% 0 0 0)'
    cards[`uzX${n}`] = on ? '#c9d2ff' : '#6b6e78'
    cards[`uzH${n}`] = on ? '#fff' : '#0d0e12'
    cards[`uzD${n}`] = on ? '#e3e8ff' : '#4d505a'
    cards[`uzR${n}`] = on ? 'rgba(255,255,255,.3)' : '#e3e5ea'
    cards[`uzT${n}`] = on ? 'translate(2px,-2px)' : 'none'
    cards[`uzL${n}`] = on ? 'scaleX(1)' : 'scaleX(0)'
    cards[`uzC${n}`] = on ? '#fff' : '#0d0e12'
    cards[`uzP${n}`] = on ? 'running' : 'paused'
    cards[`uzZ${n}`] = on ? 'scale(1.06)' : 'scale(1)'
  }

  return {
    ucRef: sectionRef,
    uzOff: leave,
    uzOp: visible ? 1 : 0,
    uzUp: visible ? 'translateY(0)' : 'translateY(28px)',
    uzW: visible ? 'translateY(0)' : 'translateY(110%)',
    ...cards,
  }
}
