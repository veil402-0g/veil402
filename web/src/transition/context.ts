import { createContext, use } from 'react'

export interface ViewTransitionValue {
  /** Navigates under the smoke. `from` is the point it wells up from. */
  go: (to: string, label: string, from?: { x: number; y: number }) => void
  /** Page-level treatment while the smoke covers: scale back and dim. */
  pageScale: string
  pageOpacity: number
}

export const ViewTransitionContext = createContext<ViewTransitionValue | null>(null)

export function useViewTransition(): ViewTransitionValue {
  const value = use(ViewTransitionContext)
  if (!value) throw new Error('useViewTransition must be used inside <ViewTransitionProvider>')
  return value
}
