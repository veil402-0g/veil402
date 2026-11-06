import { Suspense, lazy } from 'react'
import { BrowserRouter, Route, Routes } from 'react-router'
import { ViewTransitionProvider } from './transition/ViewTransition'

/**
 * The landing ships eagerly: it is the first paint for anyone arriving cold,
 * and it carries the generative canvas. The console is a whole simulated
 * ledger and its own stylesheet, so it only downloads when asked for.
 */
import { Landing } from './app/Landing'

const Console = lazy(() => import('./app/Console').then((m) => ({ default: m.Console })))

/** Holds the page ground while the console chunk arrives, so nothing flashes. */
const RouteFallback = () => <div className="route-fallback" />

export function App() {
  return (
    <BrowserRouter>
      <ViewTransitionProvider>
        <Suspense fallback={<RouteFallback />}>
          <Routes>
            <Route path="/" element={<Landing />} />
            <Route path="/console" element={<Console />} />
          </Routes>
        </Suspense>
      </ViewTransitionProvider>
    </BrowserRouter>
  )
}
