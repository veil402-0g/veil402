import { useCallback, useEffect, useRef, useState } from 'react'

/** One-shot in-view latch; `rootMargin` reproduces the design's 85%-of-viewport trigger. */
export function useInView(enterRatio = 0.85): [(el: Element | null) => void, boolean] {
  const [seen, setSeen] = useState(false)
  const seenRef = useRef(false)
  const [node, setNode] = useState<Element | null>(null)

  const ref = useCallback((el: Element | null) => setNode(el), [])

  useEffect(() => {
    if (!node || seenRef.current) return
    const bottom = Math.round((1 - enterRatio) * 100)
    const io = new IntersectionObserver(
      (entries) => {
        if (!entries.some((e) => e.isIntersecting)) return
        seenRef.current = true
        setSeen(true)
        io.disconnect()
      },
      { threshold: 0.01, rootMargin: `0px 0px ${bottom ? `-${bottom}%` : '0px'} 0px` },
    )
    io.observe(node)
    return () => io.disconnect()
  }, [node, enterRatio])

  return [ref, seen]
}
