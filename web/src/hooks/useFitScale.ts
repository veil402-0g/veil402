import { useEffect, useState } from 'react'

/** Fits a fixed-size design box into a fluid container; the design polled this at 500ms. */
export function useFitScale(
  selector: string,
  fit: (width: number, height: number) => number,
  epsilon = 0.004,
): number | null {
  const [scale, setScale] = useState<number | null>(null)

  useEffect(() => {
    const el = document.querySelector(selector)
    if (!el) return

    const measure = () => {
      const { clientWidth, clientHeight } = el
      if (!clientWidth) return
      const next = Number(fit(clientWidth, clientHeight).toFixed(3))
      setScale((prev) => (prev === null || Math.abs(next - prev) > epsilon ? next : prev))
    }

    const ro = new ResizeObserver(measure)
    ro.observe(el)
    measure()
    return () => ro.disconnect()
  }, [selector, fit, epsilon])

  return scale
}
