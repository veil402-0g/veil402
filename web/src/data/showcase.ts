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

export const MESH_GRADIENT =
  'radial-gradient(60% 50% at 30% 30%,#2c63e6 0%,transparent 70%),' +
  'radial-gradient(40% 30% at 88% 34%,#d8b9a6 0%,transparent 70%),' +
  'radial-gradient(70% 45% at 40% 92%,#c3d3f6 0%,transparent 70%),' +
  'linear-gradient(170deg,#3d74ea 0%,#4c86ee 45%,#8fb0f2 80%,#b6c9f4 100%)'

export const MUTED_GRADIENT = 'linear-gradient(170deg,#8b8e97,#b4b7c0)'

/** `research-03` -> `RE`, `support-triage` -> `ST`. */
export function initials(name: string): string {
  const words = name.split('-').filter((p) => /[a-z]/.test(p))
  const first = words[0]!
  return (first[0]! + (words[1]?.[0] ?? first[1]!)).toUpperCase()
}
