import type { CSSProperties } from 'react'

/** React forwards `--*` keys verbatim, but `CSSProperties` does not model them. */
export type CSSVars = CSSProperties & Record<`--${string}`, string | number>
