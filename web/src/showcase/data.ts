/** Fixture data behind the console showcase inside the landing page. */

export interface BudgetColumn {
  name: string
  /** Cells already spent today. */
  spent: number
  /** Cells the projection reaches. */
  projected: number
  label: string
  /** Paused or revoked mandates render in grey and never show as over-limit. */
  muted?: boolean
}

export const BUDGET_COLUMNS: BudgetColumn[] = [
  { name: 'research-03', spent: 11, projected: 15, label: '99%' },
  { name: 'support-triage', spent: 12, projected: 17, label: '112%' },
  { name: 'codegen-ci', spent: 4, projected: 6, label: '44%' },
  { name: 'pricing-scout', spent: 15, projected: 19, label: '125%' },
  { name: 'legal-summarizer', spent: 2, projected: 2, label: 'paused', muted: true },
  { name: 'growth-outreach', spent: 15, projected: 15, label: 'revoked', muted: true },
]

/** Calls per day over the trailing 48 days, 0..6 dots high. */
export const HISTORY_COLUMNS = [
  0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 6, 0, 0, 0, 0, 0, 0, 0, 0, 0, 3, 0,
  0, 0, 0, 0, 0, 1, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 1, 1, 0, 6, 4, 3, 2, 1,
]

/** Columns 39..41 are the run the "recent spike" copy refers to. */
export const HISTORY_ACCENT_RANGE = [39, 41] as const

/** Background-position per avatar, so neighbouring chips sample different mesh. */
export const AVATAR_MESH_POSITIONS = ['0% 0%', '100% 20%', '30% 100%', '80% 80%', '50% 40%', '10% 60%']

/** `research-03` -> `RE`, `support-triage` -> `ST`. */
export function initials(name: string): string {
  const words = name.split('-').filter((p) => /[a-z]/.test(p))
  const first = words[0]!
  return (first[0]! + (words[1]?.[0] ?? first[1]!)).toUpperCase()
}
