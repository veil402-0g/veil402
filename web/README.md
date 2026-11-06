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

## Where the markup comes from

`src/pages/*/` holds the two screens. The *markup* in `Hero.tsx`, `ConsoleShowcase.tsx`,
`Protocol.tsx`, `UseCases.tsx`, `Footer.tsx` and `ConsoleView.tsx`, and all of
`landing.module.css` / `console.module.css`, is generated from the Claude Design
sources in `design/` by `tools/dcgen.mjs`:

```bash
npm run gen        # dcgen + postgen
```

The design files are ~2 500 inline-styled elements. `dcgen` hoists every `style`
attribute into a deduped CSS Module class, keeps only the interpolated declarations
dynamic (as CSS custom properties set from `style={{ '--d0': … }}`), and rewrites the
design runtime's constructs — `sc-for`, `sc-if`, `style-hover`, `style-focus`,
`{{ expr }}` — into JSX. `tools/postgen.mjs` moves the result into `src/pages` and
re-applies the hand-written page shell.

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
| `useHalftone` + `src/art/halftone.ts` | the hero's generative canvas: stripe halftone, pointer ripple, scanline warp |
| `usePointerTilt` | `--mx` / `--my` for the wordmark's 3D parallax |
| `useScrollEffects` | hero parallax and the scroll-progress bar |
| `useShowcase` | the five-stage browser mockup, budget columns, history grid |
| `useProtocol` | the seven-scene protocol diagram |
| `useUseCases` | the inverting card grid |
| `useFitScale` | fits the fixed-size mockups into fluid containers |

`src/pages/landing/useLandingBindings.ts` composes them into the single flat object
the generated markup expects.

## Console data

`src/pages/console/model.ts` is a self-running ledger simulation — mandates, metered
sessions, a shielded payment feed that advances once a second — carried over from the
design so the console is useful before a backend exists. Swapping it for a real API
means replacing that module and the state wiring in `useConsoleBindings.ts`; the view
reads only the derived bindings.

## Routing

`/` and `/console`. The design linked the two screens by `location.hash`; here they
are routes, and `src/transition/` keeps the full-bleed clip-path veil between them.
