import { useEffect } from 'react'

/** The row's expanding detail is the grid whose rows the active stage animates. */
function detailOf(row: HTMLElement): HTMLElement | null {
  for (const span of row.querySelectorAll('span')) {
    if (getComputedStyle(span).display !== 'grid') continue
    const first = span.firstElementChild
    return first instanceof HTMLElement ? first : null
  }
  return null
}

/**
 * Pins a list to the height its tallest expanded row needs, so cycling the open
 * row does not resize the list and shove the rest of the page around.
 */
export function usePinnedListHeight(selector: string): void {
  useEffect(() => {
    const list = document.querySelector<HTMLElement>(selector)
    if (!list) return

    const fit = () => {
      let collapsed = 0
      let tallestDetail = 0
      for (const row of list.children) {
        if (!(row instanceof HTMLElement) || row.tagName !== 'DIV') continue
        const detail = detailOf(row)
        collapsed += row.offsetHeight - (detail?.offsetHeight ?? 0)
        if (detail) tallestDetail = Math.max(tallestDetail, detail.scrollHeight)
      }
      if (collapsed + tallestDetail > 0) {
        list.style.minHeight = `${Math.ceil(collapsed + tallestDetail)}px`
      }
    }

    // Measure against the real faces, not the fallback metrics.
    void document.fonts?.ready.then(fit)
    const onResize = () => fit()
    window.addEventListener('resize', onResize)
    fit()

    return () => {
      window.removeEventListener('resize', onResize)
      list.style.removeProperty('min-height')
    }
  }, [selector])
}
