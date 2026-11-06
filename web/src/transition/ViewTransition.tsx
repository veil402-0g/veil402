import { useCallback, useRef, useState, type ReactNode } from 'react'
import { useNavigate } from 'react-router'
import s from './ViewTransition.module.css'
import { ViewTransitionContext } from './context'
import type { CSSVars } from '../styles/css-vars'

/** 0 = idle, 1 = smoke rolling in, 2 = smoke dispersing. */
type Phase = 0 | 1 | 2

const COVER_MS = 680
const REVEAL_MS = 760

/** Billow offsets and sizes, so the puffs never read as a symmetric burst. */
const PUFFS = [
  { dx: '-20vw', dy: '-12vh', s: 2.5, c: '#3d74ea' },
  { dx: '24vw', dy: '-16vh', s: 2.2, c: '#2c63e6' },
  { dx: '-26vw', dy: '16vh', s: 2.6, c: '#4c86ee' },
  { dx: '20vw', dy: '20vh', s: 2.3, c: '#8fb0f2' },
  { dx: '-4vw', dy: '-26vh', s: 2.0, c: '#d8b9a6' },
] as const

/** Two frames, so the resting styles are committed before the smoke animates. */
function nextPaint(fn: () => void) {
  requestAnimationFrame(() => requestAnimationFrame(fn))
}

export function ViewTransitionProvider({ children }: { children: ReactNode }) {
  const navigate = useNavigate()
  const [phase, setPhase] = useState<Phase>(0)
  const [label, setLabel] = useState('VEIL402.XYZ/CONSOLE')
  /** Where the smoke wells up from — the click point, when there was one. */
  const [origin, setOrigin] = useState({ x: '50%', y: '50%' })
  const busy = useRef(false)

  const go = useCallback(
    (to: string, nextLabel: string, from?: { x: number; y: number }) => {
      if (busy.current || window.location.pathname === to) return
      busy.current = true
      setLabel(nextLabel)
      setOrigin(from ? { x: `${from.x}px`, y: `${from.y}px` } : { x: '50%', y: '50%' })
      setPhase(0)
      nextPaint(() => setPhase(1))

      window.setTimeout(() => {
        void navigate(to)
        window.scrollTo(0, 0)
        nextPaint(() => setPhase(2))
        window.setTimeout(() => {
          setPhase(0)
          busy.current = false
        }, REVEAL_MS)
      }, COVER_MS)
    },
    [navigate],
  )

  const covering = phase === 1

  return (
    <ViewTransitionContext
      value={{
        go,
        pageScale: covering ? 'scale(.97)' : 'none',
        pageOpacity: covering ? 0.4 : 1,
      }}
    >
      {children}
      <div
        aria-hidden="true"
        className={s.veil}
        data-phase={phase}
        style={{ '--ox': origin.x, '--oy': origin.y } as CSSVars}
      >
        {phase !== 0 && (
          <>
            <div className={s.ground} />
            <div className={s.puffs}>
              {PUFFS.map((p, i) => (
                <span
                  key={i}
                  className={s.puff}
                  style={
                    { '--dx': p.dx, '--dy': p.dy, '--s': p.s, '--c': p.c, '--i': i } as CSSVars
                  }
                />
              ))}
            </div>
            <div className={s.grain} />
          </>
        )}
        <div
          className={s.mark}
          style={
            {
              '--opacity': covering ? 1 : 0,
              '--transform': covering ? 'translateY(0)' : 'translateY(10px)',
            } as CSSVars
          }
        >
          <span className={s.dot}>
            <span className={s.dotCore} />
          </span>
          <span className={s.label}>{label}</span>
        </div>
      </div>
    </ViewTransitionContext>
  )
}
