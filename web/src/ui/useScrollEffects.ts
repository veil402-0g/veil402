import { useEffect } from 'react'

/** The parallax only runs over the first viewport, where the hero is. */
const LAYERS = {
  a: { rate: 0.32, fade: 900, scale: 0.00012 },
  b: { rate: 0.14, fade: 1100, scale: 0 },
} as const

/** Attribute-driven because the layers and the progress bar sit in different subtrees. */
export function useScrollEffects(active: boolean): void {
  useEffect(() => {
    if (!active) return

    let layers: HTMLElement[] = []
    let progress: HTMLElement | null = null

    const apply = () => {
      if (!layers.length || !layers[0]?.isConnected) {
        layers = [...document.querySelectorAll<HTMLElement>('[data-par]')]
        progress = document.querySelector<HTMLElement>('[data-sp]')
      }
      const y = Math.min(window.scrollY, window.innerHeight)
      for (const el of layers) {
        const cfg = el.dataset.par === 'a' ? LAYERS.a : LAYERS.b
        const scale = cfg.scale ? ` scale(${(1 - y * cfg.scale).toFixed(4)})` : ''
        el.style.transform = `translate3d(0,${(y * cfg.rate).toFixed(1)}px,0)${scale}`
        el.style.opacity = (1 - y / cfg.fade).toFixed(3)
      }
      if (progress) {
        const max = Math.max(1, document.documentElement.scrollHeight - window.innerHeight)
        progress.style.transform = `scaleX(${(window.scrollY / max).toFixed(4)})`
      }
    }

    let queued = 0
    const onScroll = () => {
      if (queued) return
      queued = requestAnimationFrame(() => {
        queued = 0
        apply()
      })
    }

    window.addEventListener('scroll', onScroll, { passive: true })
    apply()

    return () => {
      cancelAnimationFrame(queued)
      window.removeEventListener('scroll', onScroll)
    }
  }, [active])
}
