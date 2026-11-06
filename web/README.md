# veil402 web

The marketing site and console UI: React 19, Vite 7, TypeScript 5.9, `react-router`,
CSS Modules. No UI, animation or state library — the design is CSS transitions plus
`requestAnimationFrame`, and that is what this ships.

```bash
npm install
npm run dev        # vite, honours $PORT
npm run build      # tsc -b && vite build
npm run typecheck
npm run lint
```

## Layout

Feature folders, after `mercury/apps/web`: a domain owns its components, hooks,
data and helpers together rather than being split across `hooks/`, `lib/` and
`data/` buckets.

```
src/
  App.tsx          routes; the console is lazy, the landing is the cold-start paint
  main.tsx
  tokens.css       palette, base reset, global keyframes -- the only global sheet
  app/             both screens, their generated section views and stylesheets
  hero/            the wordmark: canvas artwork, entrance, pointer tilt, typed command
  showcase/        the five-stage browser mockup and its fixture data
  protocol/        the seven-scene diagram and its scenes
  usecases/        the inverting card grid
  ledger/          the console's simulated ledger and its formatters
  transition/      the smoke that covers a route change
  motion/          named easing curves, reduced-motion check
  ui/              primitives with no domain: in-view, fit-scale, scroll, arrows
```

## Where the markup comes from

The *markup* in `app/Hero.tsx`, `ConsoleShowcase.tsx`, `Protocol.tsx`,
`UseCases.tsx`, `Footer.tsx` and `ConsoleView.tsx`, and all of
`app/landing.module.css` / `app/console.module.css`, is generated from the
Claude Design sources in `design/` by `tools/dcgen.mjs`:

```bash
npm run gen        # dcgen + postgen
```

`tools/make-icons.mjs` generates the favicons from the same brand mark — a disc
with a square at its centre — so the PNGs are reproducible rather than
hand-placed binaries.

The design files are ~2 500 inline-styled elements. `dcgen` hoists every `style`
attribute into a deduped CSS Module class, keeps only the interpolated declarations
dynamic (as CSS custom properties set from `style={{ '--d0': … }}`), and rewrites the
design runtime's constructs — `sc-for`, `sc-if`, `style-hover`, `style-focus`,
`{{ expr }}` — into JSX. `tools/postgen.mjs` moves the result into `src/app`, re-applies
the hand-written page shell, and fails the run if a component names a class or a
keyframe its stylesheet does not declare.

Regenerating overwrites those six components and two stylesheets. Everything else
is hand-written: edit the design source and re-run, or stop regenerating and own
the output.

## Where the behaviour comes from

The design's logic class was ported by hand into focused hooks, each returning the
binding names its markup reads:

| Hook | Drives |
| --- | --- |
| `useHeroIntro` | hero entrance, rolling paragraph highlight |
| `useTypedCommand` | the typed `curl` line and its copy button |
| `useHalftone` + `hero/halftone.ts` | the hero's generative canvas: stripe halftone, pointer ripple, scanline warp |
| `usePointerTilt` | `--mx` / `--my` for the wordmark's 3D parallax |
| `useScrollEffects` | hero parallax and the scroll-progress bar |
| `useShowcase` | the five-stage browser mockup, budget columns, history grid |
| `useProtocol` | the seven-scene protocol diagram |
| `useUseCases` | the inverting card grid |
| `useFitScale` | fits the fixed-size mockups into fluid containers |
| `usePinnedListHeight` | holds the showcase list at its tallest row, so cycling stages does not resize the page |

`src/app/useLandingBindings.ts` composes them into the single flat object the
generated markup expects.

## Console data

`src/ledger/model.ts` is a self-running ledger simulation — mandates, metered
sessions, a shielded payment feed that advances once a second — carried over from the
design so the console is useful before a backend exists. Swapping it for a real API
means replacing that module and the state wiring in `app/useConsoleBindings.ts`;
the view reads only the derived bindings.

## Routing

`/` and `/console`. The design linked the two screens by `location.hash`; here they
are routes. `src/transition/` covers the swap with smoke that wells up from the
point you clicked, and the console route is `lazy()`, so the landing does not
download the simulated ledger and the console's stylesheet to render.
