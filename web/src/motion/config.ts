/** The design's easing curves, named by what they do. */
export const EASE = {
  /** Long sweeps: the protocol packet, clip-path reveals, grid rows opening. */
  sweep: 'cubic-bezier(.65,0,.25,1)',
  /** Settling into place: entrances, parallax, the mark on the veil. */
  settle: 'cubic-bezier(.2,.8,.2,1)',
  /** A wipe that leaves hard and lands soft. */
  wipe: 'cubic-bezier(.7,0,.2,1)',
  /** The page transition's snap. */
  snap: 'cubic-bezier(.76,0,.24,1)',
  /** Display type arriving from a long way off. */
  drift: 'cubic-bezier(.16,1,.3,1)',
  /** A small overshoot, for marks and dots. */
  pop: 'cubic-bezier(.3,1.5,.5,1)',
} as const

export function prefersReducedMotion(): boolean {
  return typeof window !== 'undefined' && window.matchMedia('(prefers-reduced-motion: reduce)').matches
}
