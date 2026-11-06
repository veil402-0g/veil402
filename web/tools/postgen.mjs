/**
 * Moves one `dcgen` run into the app tree: rewrites the generated imports to
 * the hand-written hooks and stylesheets, and re-applies the page-shell
 * classes that replace the generated shell in the landing stylesheet.
 *
 * Run after `node tools/dcgen.mjs`; `npm run gen` does both.
 */
import { copyFileSync, readFileSync, rmSync, writeFileSync } from 'node:fs'

const ROOT = new URL('..', import.meta.url).pathname
const read = (p) => readFileSync(ROOT + p, 'utf8')
const write = (p, s) => writeFileSync(ROOT + p, s)

const LANDING_SECTIONS = {
  Hero: 'Hero',
  Console: 'ConsoleShowcase',
  Protocol: 'Protocol',
  UseCases: 'UseCases',
  Footer: 'Footer',
}

for (const [from, to] of Object.entries(LANDING_SECTIONS)) {
  const src = read(`src/generated/landing/${from}.tsx`)
    .replace(/'\.\/bindings'/g, "'./useLandingBindings'")
    .replace(/'\.\/styles\.module\.css'/g, "'./landing.module.css'")
    .replace(/'\.\.\/\.\.\/styles\/css-vars'/g, "'../ui/css-vars'")
    .replace(`export function ${from}(`, `export function ${to}(`)
  write(`src/app/${to}.tsx`, src)
}

write(
  'src/app/ConsoleView.tsx',
  read('src/generated/console/Root.tsx')
    .replace(/'\.\/bindings'/g, "'./useConsoleBindings'")
    .replace(/'\.\/styles\.module\.css'/g, "'./console.module.css'")
    .replace(/'\.\.\/\.\.\/styles\/css-vars'/g, "'../ui/css-vars'")
    .replace('export function Root(', 'export function ConsoleView('),
)
copyFileSync(`${ROOT}src/generated/console/styles.module.css`, `${ROOT}src/app/console.module.css`)

// The app owns the page shell (progress bar + page treatment) and the veil in
// src/transition, so those classes are hand-written. Generated classes are never
// removed: dcgen dedupes by style string, so a shell class can be shared by
// unrelated elements -- the logo's inner square is one such.
const SHELL = `/* --- page shell ------------------------------------------------------- */

.scrollProgress {
  position: fixed;
  left: 0;
  top: 0;
  width: 100%;
  height: 2px;
  z-index: 200;
  pointer-events: none;
  background: var(--blue);
  transform-origin: left;
  transform: scaleX(0);
}

.page {
  transform: var(--page-scale);
  opacity: var(--page-opacity);
  transform-origin: 50% 30vh;
  transition:
    transform 0.5s cubic-bezier(0.76, 0, 0.24, 1),
    opacity 0.5s ease;
}

`
write('src/app/landing.module.css', SHELL + read('src/generated/landing/styles.module.css'))

copyFileSync(`${ROOT}src/generated/landing/bindings.ts`, `${ROOT}src/app/landing.bindings.generated.ts`)
copyFileSync(`${ROOT}src/generated/console/bindings.ts`, `${ROOT}src/app/console.bindings.generated.ts`)

rmSync(`${ROOT}src/generated`, { recursive: true, force: true })

// Guard: every class a component names must exist in its stylesheet. Deleting a
// generated class by hand once silently removed the logo's inner square, which
// shared a style string with the page shell.
const PAIRS = [
  ['src/app/landing.module.css', Object.values(LANDING_SECTIONS).map((n) => `src/app/${n}.tsx`)],
  ['src/app/console.module.css', ['src/app/ConsoleView.tsx']],
]
let missing = 0
for (const [sheet, components] of PAIRS) {
  const defined = new Set([...read(sheet).matchAll(/^\.([\w-]+)/gm)].map((m) => m[1]))
  for (const component of components) {
    for (const m of read(component).matchAll(/\bs\.([\w-]+)/g)) {
      if (defined.has(m[1])) continue
      console.error(`postgen: ${component} uses .${m[1]}, missing from ${sheet}`)
      missing++
    }
  }
}

// Guard: a CSS Module rewrites `animation-name`, so any keyframes it names by
// literal must be declared in the same file -- a global copy will not resolve.
for (const [sheet] of PAIRS) {
  const css = read(sheet)
  const declared = new Set([...css.matchAll(/@keyframes\s+([\w-]+)/g)].map((m) => m[1]))
  for (const m of css.matchAll(/animation(?:-name)?:\s*([a-zA-Z][\w-]*)/g)) {
    if (m[1] === 'none' || m[1] === 'var' || declared.has(m[1])) continue
    console.error(`postgen: ${sheet} animates ${m[1]}, not declared in that file`)
    missing++
  }
}

if (missing) process.exit(1)

console.log('postgen: generated output moved into src/app')
