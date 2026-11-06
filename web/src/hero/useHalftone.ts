import { useCallback, useEffect, useRef, useState } from 'react'
import { Halftone } from './halftone'

/** Throttles the scanline pass to ~30fps, as the design does. */
const FRAME_MS = 33
/** The field settles flat before the scanline shear starts moving. */
const WARP_DELAY_MS = 800

export interface HalftoneBinding {
  ref: (el: HTMLCanvasElement | null) => void
  onMouseEnter: (e: React.MouseEvent) => void
  onMouseMove: (e: React.MouseEvent) => void
  onMouseLeave: () => void
}

/** Drives one artwork canvas; animates only while on screen and the tab is visible. */
export function useHalftone(): HalftoneBinding {
  const artRef = useRef<Halftone | null>(null)
  const [canvas, setCanvas] = useState<HTMLCanvasElement | null>(null)

  const ref = useCallback((el: HTMLCanvasElement | null) => setCanvas(el), [])

  useEffect(() => {
    if (!canvas) return

    const art = new Halftone(canvas)
    artRef.current = art
    art.draw()

    // Redraw on the next frame rather than after a delay: a debounce leaves the
    // old bitmap stretched across the new box, which reads as pixelation.
    let redrawFrame = 0
    const resize = new ResizeObserver(() => {
      cancelAnimationFrame(redrawFrame)
      redrawFrame = requestAnimationFrame(() => art.draw())
    })
    resize.observe(canvas)

    let onScreen = true
    const visibility = new IntersectionObserver(([entry]) => {
      onScreen = entry?.isIntersecting ?? false
    })
    visibility.observe(canvas)

    let raf = 0
    let last = 0
    const loop = (now: number) => {
      raf = requestAnimationFrame(loop)
      if (document.hidden || !onScreen || now - last < FRAME_MS) return
      last = now
      art.frame(now / 1000)
    }
    const warpStart = window.setTimeout(() => {
      raf = requestAnimationFrame(loop)
    }, WARP_DELAY_MS)

    void document.fonts?.ready.then(() => art.draw())

    return () => {
      window.clearTimeout(warpStart)
      cancelAnimationFrame(raf)
      cancelAnimationFrame(redrawFrame)
      resize.disconnect()
      visibility.disconnect()
      artRef.current = null
    }
  }, [canvas])

  const aim = useCallback(
    (e: React.MouseEvent) => {
      const art = artRef.current
      if (!art || !canvas) return null
      const box = canvas.getBoundingClientRect()
      return { art, x: e.clientX - box.left, y: e.clientY - box.top }
    },
    [canvas],
  )

  return {
    ref,
    onMouseEnter: useCallback(
      (e: React.MouseEvent) => {
        const hit = aim(e)
        if (!hit) return
        const { art, x, y } = hit
        // Jump the ripple to the entry point rather than sweeping in from 0,0.
        if (art.pointer.s < 0.01) {
          art.pointer.x = x
          art.pointer.y = y
        }
        art.pointer.tx = x
        art.pointer.ty = y
        art.pointer.ts = 1
      },
      [aim],
    ),
    onMouseMove: useCallback(
      (e: React.MouseEvent) => {
        const hit = aim(e)
        if (!hit) return
        hit.art.pointer.tx = hit.x
        hit.art.pointer.ty = hit.y
        hit.art.pointer.ts = 1
      },
      [aim],
    ),
    onMouseLeave: useCallback(() => {
      if (artRef.current) artRef.current.pointer.ts = 0
    }, []),
  }
}
