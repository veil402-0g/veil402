import { useMemo, useState } from 'react'

export interface ArrowHover {
  /** Slides a two-glyph strip so the trailing arrow replaces the leading one. */
  shift: string
  on: () => void
  off: () => void
}

/** The ↗ swap every link and button in the design performs on hover. */
export function useArrowHover(): ArrowHover {
  const [hovered, setHovered] = useState(false)
  return useMemo(
    () => ({
      shift: hovered ? 'translateX(0)' : 'translateX(-50%)',
      on: () => setHovered(true),
      off: () => setHovered(false),
    }),
    [hovered],
  )
}
