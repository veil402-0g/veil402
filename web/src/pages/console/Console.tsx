import { useEffect } from 'react'
import { useDocumentTitle } from '../../hooks/useDocumentTitle'
import { ConsoleView } from './ConsoleView'
import { useConsoleBindings } from './useConsoleBindings'

export function Console() {
  const v = useConsoleBindings()
  useDocumentTitle('Veil402 Console')

  // The console sits on the blue page ground; the landing stays white.
  useEffect(() => {
    document.body.dataset.page = 'console'
    return () => {
      delete document.body.dataset.page
    }
  }, [])

  return <ConsoleView v={v} />
}
