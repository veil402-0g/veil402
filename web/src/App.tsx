import { BrowserRouter, Route, Routes } from 'react-router'
import { ViewTransitionProvider } from './transition/ViewTransition'
import { Landing } from './pages/landing/Landing'
import { Console } from './pages/console/Console'

export function App() {
  return (
    <BrowserRouter>
      <ViewTransitionProvider>
        <Routes>
          <Route path="/" element={<Landing />} />
          <Route path="/console" element={<Console />} />
        </Routes>
      </ViewTransitionProvider>
    </BrowserRouter>
  )
}
