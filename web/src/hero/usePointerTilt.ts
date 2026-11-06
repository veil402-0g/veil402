import { useEffect } from 'react'
import { prefersReducedMotion } from '../motion/config'

/** After this long without pointer movement, the tilt drifts on its own. */
const IDLE_MS = 2500
const EASE = 0.06

/** Publishes the pointer as `--mx`/`--my`, which the stacked wordmark layers offset against. */
export function usePointerTilt(active: boolean): void {
  useEffect(() => {
    if (!active) return
    if (prefersReducedMotion()) return

    const layers = [...document.querySelectorAll<HTMLElement>('[data-h3d]')]
    if (!layers.length) return

    let tx = 0
    let ty = 0
    let cx = 0
    let cy = 0
    let lastMove = 0

    const onMove = (e: PointerEvent) => {
      tx = (e.clientX / window.innerWidth - 0.5) * 2
      ty = (e.clientY / window.innerHeight - 0.5) * 2
      lastMove = performance.now()
    }
    window.addEventListener('pointermove', onMove, { passive: true })

    let raf = 0
    const tick = (t: number) => {
      if (t - lastMove > IDLE_MS) {
        tx = Math.sin(t / 2600) * 0.45
        ty = Math.cos(t / 3400) * 0.35
      }
      cx += (tx - cx) * EASE
      cy += (ty - cy) * EASE
      for (const el of layers) {
        el.style.setProperty('--mx', cx.toFixed(4))
        el.style.setProperty('--my', cy.toFixed(4))
      }
      raf = requestAnimationFrame(tick)
    }
    raf = requestAnimationFrame(tick)

    return () => {
      cancelAnimationFrame(raf)
      window.removeEventListener('pointermove', onMove)
    }
  }, [active])
}
