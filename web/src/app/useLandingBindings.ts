import type { MouseEvent } from 'react'
import { useArrowHover } from '../ui/useArrowHover'
import { useHalftone } from '../hero/useHalftone'
import { useHeroIntro } from '../hero/useHeroIntro'
import { usePointerTilt } from '../hero/usePointerTilt'
import { usePosters } from '../posters/usePosters'
import { useProtocol } from '../protocol/useProtocol'
import { useScrollEffects } from '../ui/useScrollEffects'
import { useShowcase } from '../showcase/useShowcase'
import { useTypedCommand } from '../hero/useTypedCommand'
import { useUseCases } from '../usecases/useUseCases'
import { useViewTransition } from '../transition/context'

/** Display faces the design offers for the hero wordmark. */
export type DisplayFont = 'Bagel Fat One' | 'Shrikhand' | 'Climate Crisis' | 'Rampart One'

/** Each face needs its own optical tracking at hero size. */
const TRACKING: Record<DisplayFont, string> = {
  'Bagel Fat One': '-0.03em',
  'Rampart One': '-0.03em',
  'Climate Crisis': '-0.02em',
  Shrikhand: '-0.01em',
}

export interface LandingOptions {
  displayFont?: DisplayFont
}

/** One flat bag, as the design's markup expects; each concern lives in its own hook. */
export function useLandingBindings({ displayFont = 'Bagel Fat One' }: LandingOptions = {}) {
  const hero = useHeroIntro()
  const command = useTypedCommand()
  const showcase = useShowcase()
  const posters = usePosters()
  const protocol = useProtocol()
  const useCases = useUseCases()
  const transition = useViewTransition()

  const heroArt = useHalftone()
  const showcaseArt = useHalftone()
  usePointerTilt(true)

  const consoleArrow = useArrowHover()
  const sourceArrow = useArrowHover()
  const installArrow = useArrowHover()
  const footerArrowA = useArrowHover()
  const footerArrowB = useArrowHover()

  useScrollEffects(true)

  return {
    ...hero,
    ...command,
    ...showcase,
    ...posters,
    ...protocol,
    ...useCases,

    dispFont: `'${displayFont}',cursive`,
    dispTrack: TRACKING[displayFont],

    canvasRef: heroArt.ref,
    onEnter: heroArt.onMouseEnter,
    onMove: heroArt.onMouseMove,
    onLeave: heroArt.onMouseLeave,
    canvasRef2: showcaseArt.ref,
    onEnter2: showcaseArt.onMouseEnter,
    onMove2: showcaseArt.onMouseMove,
    onLeave2: showcaseArt.onMouseLeave,

    openConsole: (e: MouseEvent) => {
      e.preventDefault()
      transition.go('/console', 'VEIL402.XYZ/CONSOLE', { x: e.clientX, y: e.clientY })
    },
    pageScale: transition.pageScale,
    pageOpacity: transition.pageOpacity,

    a1: consoleArrow.shift,
    h1on: consoleArrow.on,
    h1off: consoleArrow.off,
    a2: sourceArrow.shift,
    h2on: sourceArrow.on,
    h2off: sourceArrow.off,
    a3: installArrow.shift,
    h3on: installArrow.on,
    h3off: installArrow.off,
    fa1: footerArrowA.shift,
    f1on: footerArrowA.on,
    f1off: footerArrowA.off,
    fa2: footerArrowB.shift,
    f2on: footerArrowB.on,
    f2off: footerArrowB.off,
  }
}

export type LandingBindings = ReturnType<typeof useLandingBindings>
