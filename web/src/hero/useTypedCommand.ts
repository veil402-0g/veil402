import { Fragment, createElement, useCallback, useEffect, useRef, useState, type ReactNode } from 'react'

/** Split so each run can be tinted separately in the hero. */
export const INSTALL_COMMAND_PARTS = [
  'curl',
  ' -fsSL',
  ' https://veil402.xyz/install.sh',
  ' |',
  ' bash',
] as const

export const INSTALL_COMMAND = INSTALL_COMMAND_PARTS.join('')

const START_DELAY_MS = 1000
const CHAR_MS = 16
const COPIED_FOR_MS = 1600

export interface TypedCommandBindings {
  t1: string
  t2: string
  t3: string
  t4: string
  t5: string
  tcursor: ReactNode
  copied: boolean
  notCopied: boolean
  copy: () => void
}

/** A word-joiner keeps the caret glued to the last typed character. */
const CARET = createElement(
  Fragment,
  null,
  '\u2060',
  createElement('span', {
    style: {
      display: 'inline-block',
      width: '0.6em',
      height: '1.1em',
      background: '#fff',
      verticalAlign: '-0.2em',
      marginLeft: '1px',
      marginRight: 'calc(-0.6em - 1px)',
      animation: 'vblink 1s steps(1) infinite',
    },
  }),
)

/** Types the install command out one character at a time, then handles copy. */
export function useTypedCommand(): TypedCommandBindings {
  const [typed, setTyped] = useState(0)
  const [copied, setCopied] = useState(false)
  const copiedTimer = useRef<number | undefined>(undefined)

  useEffect(() => {
    const total = INSTALL_COMMAND.length
    let interval: number | undefined
    const start = window.setTimeout(() => {
      let n = 0
      interval = window.setInterval(() => {
        n += 1
        setTyped(n)
        if (n >= total) window.clearInterval(interval)
      }, CHAR_MS)
    }, START_DELAY_MS)
    return () => {
      window.clearTimeout(start)
      window.clearInterval(interval)
    }
  }, [])

  useEffect(() => () => window.clearTimeout(copiedTimer.current), [])

  const copy = useCallback(() => {
    void navigator.clipboard?.writeText(INSTALL_COMMAND)
    setCopied(true)
    window.clearTimeout(copiedTimer.current)
    copiedTimer.current = window.setTimeout(() => setCopied(false), COPIED_FOR_MS)
  }, [])

  let remaining = typed
  const parts = INSTALL_COMMAND_PARTS.map((part) => {
    const slice = part.slice(0, Math.max(0, remaining))
    remaining -= part.length
    return slice
  })

  return {
    t1: parts[0]!,
    t2: parts[1]!,
    t3: parts[2]!,
    t4: parts[3]!,
    t5: parts[4]!,
    tcursor: CARET,
    copied,
    notCopied: !copied,
    copy,
  }
}
