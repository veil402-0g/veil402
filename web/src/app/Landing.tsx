import type { CSSVars } from '../ui/css-vars'
import s from './landing.module.css'
import { useLandingBindings } from './useLandingBindings'
import { Hero } from './Hero'
import { ConsoleShowcase } from './ConsoleShowcase'
import { Protocol } from './Protocol'
import { UseCases } from './UseCases'
import { Footer } from './Footer'

export function Landing() {
  const v = useLandingBindings()

  return (
    <>
      <div aria-hidden="true" className={s.scrollProgress} data-sp="1" />
      <div
        className={s.page}
        style={
          { '--page-scale': v.pageScale, '--page-opacity': v.pageOpacity } as CSSVars
        }
      >
        <Hero v={v} />
        <ConsoleShowcase v={v} />
        <Protocol v={v} />
        <UseCases v={v} />
        <Footer v={v} />
      </div>
    </>
  )
}
