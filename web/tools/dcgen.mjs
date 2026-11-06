/**
 * One-shot codemod: Claude Design `.dc.html` -> React TSX + CSS Modules.
 *
 * The design files are ~100% inline styles. This hoists every `style` attribute
 * into a deduped CSS Module class, keeps only the `{{ }}`-interpolated
 * declarations dynamic (as CSS custom properties set from `style={{...}}`), and
 * rewrites the design runtime's template constructs into JSX:
 *
 *   style-hover="..."              -> `.cls:hover { ... }`
 *   <sc-for list="{{ xs }}" as="x"> -> {xs.map((x, i) => <Fragment key={i}>…)}
 *   <sc-if value="{{ c }}">        -> {c ? <>…</> : null}
 *   {{ expr }}                     -> {v.expr}   (loop vars stay bare)
 *   ref= / onClick= / onMouseEnter= -> real React props
 *
 * Output is a starting point, not the deliverable: generated class names are
 * positional and the markup is still flat. Curation happens by hand afterwards.
 *
 * Usage: node tools/dcgen.mjs
 */
import { readFileSync, writeFileSync, mkdirSync } from 'node:fs'
import { parseFragment } from 'parse5'

const ROOT = new URL('..', import.meta.url).pathname

/** The brand mesh, as every gradient-wipe button in the design writes it. */
const MESH =
  'radial-gradient(60% 50% at 30% 30%,#2c63e6 0%,transparent 70%),' +
  'radial-gradient(40% 30% at 88% 34%,#d8b9a6 0%,transparent 70%),' +
  'radial-gradient(70% 45% at 40% 92%,#c3d3f6 0%,transparent 70%),' +
  'linear-gradient(170deg,#3d74ea 0%,#4c86ee 45%,#8fb0f2 80%,#b6c9f4 100%)'

/** The Posters heading: one mask span per word, each wiping up on its own beat. */
const pzWord = (delay, word) =>
  '<span style="display:inline-block;overflow:hidden;vertical-align:top;padding-bottom:.08em;margin-bottom:-.08em">' +
  `<span style="display:inline-block;transform:{{ pzW }};transition:transform 1s cubic-bezier(.7,0,.2,1) ${delay}s">${word}</span></span>`

/** The design ruled a word at rest and filled it on hover. */
const PZ_RULED =
  '<span style="position:relative;display:inline-block;cursor:default' +
  ';background-image:linear-gradient(#1230d6,#1230d6);background-repeat:no-repeat' +
  ';background-position:0 100%;background-size:100% .07em' +
  ';transition:background-size .45s cubic-bezier(.7,0,.2,1),color .3s"' +
  ' style-hover="background-size:100% 100%;color:#fff">'

/** The console heading's version: bare at rest, with a rule that wipes in after. */
const PZ_PLAIN = PZ_RULED.replace('background-size:100% .07em', 'background-size:100% 0%')
const PZ_RULE =
  '<span style="position:absolute;left:0;right:.1em;bottom:-.02em;height:.07em;background:#1230d6' +
  ';transform:{{ pzLine }};transform-origin:left' +
  ';transition:transform .9s cubic-bezier(.7,0,.2,1) 1.3s"></span>'

/** Words the heading rules at rest, and the beat each one arrives on. */
const PZ_WORDS = [
  ['0.25', 'atomic,'],
  ['0.30', 'private,'],
  ['0.35', 'and'],
  ['0.40', 'auditable'],
  ['0.45', 'AI'],
]

/** Vendor marks, checked in as monochrome SVGs beside the one that is remote. */
const LOGOS = [
  ['https://github.com/openclaw.png?size=128', 'openclaw', 'OpenClaw'],
  ['assets/hermes.png', 'hermes', 'Hermes'],
  ['https://github.com/All-Hands-AI.png?size=128', 'openhands', 'OpenHands'],
]

/** Poster 03's ground: the same mesh pushed deep, with one warm corner. */
const POSTER_MESH =
  'radial-gradient(45% 40% at 85% 10%,#0b1f9c 0%,transparent 70%),' +
  'radial-gradient(50% 45% at 15% 60%,#1230d6 0%,transparent 70%),' +
  'radial-gradient(40% 35% at 80% 80%,#8fb0f2 0%,transparent 70%),' +
  'radial-gradient(35% 30% at 25% 15%,#d8b9a6 0%,transparent 70%),' +
  'linear-gradient(150deg,#2c63e6,#1230d6 45%,#4c86ee 80%,#c3d3f6)'

/** The card the posters float on that ground. */
const CARD = '#f4f6fd'

/** The posters' dot grid, as an overlay the ground can carry. */
const POSTER_DOTS =
  '<div aria-hidden="true" style="position:absolute;inset:0;pointer-events:none' +
  ';background-image:radial-gradient(rgba(255,255,255,.4) .8px,transparent 1.2px)' +
  ';background-size:14px 14px;animation:vdots 6s linear infinite"></div>'

/** A state badge: stadium, mono, as `Attested` and `Ciphertext only` write it. */
const STATE_BADGE = (fg, bg) =>
  "display:inline-flex;align-items:center;gap:7px;font-family:'JetBrains Mono',monospace" +
  `;font-size:13px;letter-spacing:.06em;color:${fg};background:${bg}` +
  ';padding:7px 13px;border-radius:999px;white-space:nowrap'

/** A budget tag: a stadium badge with the brand tint falling across it. */
const BADGE = (radius, pad) =>
  `;height:${radius}px;padding:${pad};display:flex;align-items:center;gap:8px;border-radius:999px`

/** Elements split out into their own component file, keyed by a source marker. */
const TARGETS = [
  {
    src: 'design/Veil 402 Landing.dc.html',
    out: 'src/generated/landing',
    name: 'Landing',
    // The hero is the only major section the design left unlabelled.
    markers: [
      ['<div style="height:100vh;min-height:700px', 'data-dc-split="Hero" '],
    ],
    edits: [
      // A budget tag broke mid-word and left one-line cards sitting higher
      // than two-line ones. Keyed on style strings: class names renumber
      // whenever the design gains an element.
      [
        `<span style="font-family:'JetBrains Mono',monospace;font-size:11.5px;line-height:1.15;color:{{ col.nc }};overflow-wrap:anywhere">`,
        `<span title="{{ col.name }}" style="font-family:'JetBrains Mono',monospace;font-size:11px;line-height:1.2;color:{{ col.nc }};white-space:nowrap;overflow:hidden;text-overflow:ellipsis">`,
      ],
      // ...and the tag itself reads as a badge rather than a box. Its height
      // is mirrored by CHART_CHROME in useShowcase: the connector below it is
      // measured from the chart, so the two have to agree.
      [
        '<div style="box-sizing:border-box;width:100%;height:58px;padding:8px 10px;display:flex;align-items:center;gap:8px;background:rgba(255,255,255,.8);box-shadow:inset 0 0 0 1px rgba(18,48,214,.16)">',
        '<div style="box-sizing:border-box;width:100%' +
          BADGE(46, '6px 12px 6px 6px') +
          ';background-color:rgba(255,255,255,.86)' +
          ';background-image:linear-gradient(115deg,rgba(255,255,255,0) 35%,rgba(199,214,248,.55) 100%)' +
          ';box-shadow:inset 0 0 0 1px rgba(18,48,214,.16)">',
      ],
      // The Posters heading ruled every clause at rest, which read as six
      // separate links. The console heading above it rules only its last word
      // and wipes that rule in; this is the same heading, so it does that too.
      ...PZ_WORDS.map(([d, w]) => [PZ_RULED + pzWord(d, w) + '</span>', pzWord(d, w)]),
      [
        PZ_RULED + pzWord('0.50', 'commerce.') + '</span>',
        PZ_PLAIN + pzWord('0.50', 'commerce.') + PZ_RULE + '</span>',
      ],
      // The showcase CTA was the one button on the page that started filled
      // and emptied on hover.
      [
        `style="width:min(100%,320px);display:flex;align-items:center;justify-content:space-between;gap:22px;color:#fff;font:500 12px 'JetBrains Mono',monospace;letter-spacing:.08em;text-transform:uppercase;white-space:nowrap;padding:12px 14px;box-sizing:border-box;background-image:linear-gradient(#1230d6,#1230d6);background-repeat:no-repeat;background-position:0 100%;background-size:100% 100%;transition:background-size .4s cubic-bezier(.7,0,.2,1),color .12s linear .12s" style-hover="background-size:100% 1px;color:#1230d6;transition:background-size .4s cubic-bezier(.7,0,.2,1),color .12s linear .2s"`,
        `style="width:fit-content;display:flex;align-items:center;gap:22px;color:#1230d6;font:500 12px 'JetBrains Mono',monospace;letter-spacing:.08em;text-transform:uppercase;white-space:nowrap;padding:12px 14px;box-sizing:border-box;background-image:linear-gradient(#1230d6,#1230d6);background-repeat:no-repeat;background-position:0 100%;background-size:100% 1px;transition:background-size .4s cubic-bezier(.7,0,.2,1),color .12s linear .2s" style-hover="background-size:100% 100%;color:#fff;transition:background-size .4s cubic-bezier(.7,0,.2,1),color .12s linear .12s"`,
      ],
      // The diagram panel sat at 88% white over its own mesh and grain, which
      // flattened it into a wireframe. Let the ground through, and carry the
      // posters' dot grid across it.
      [
        'border:1px solid rgba(13,14,18,.08);background:rgba(255,255,255,.88)' +
          ';backdrop-filter:blur(18px) saturate(1.5);-webkit-backdrop-filter:blur(18px) saturate(1.5)' +
          ';box-shadow:0 30px 70px -30px rgba(18,48,214,.45),0 2px 8px rgba(13,14,18,.06)',
        'border:0;background:' +
          CARD +
          ';box-shadow:0 44px 90px -36px rgba(6,14,64,.55),0 4px 14px rgba(6,14,64,.16)',
      ],
      // ...and the ground it sits on is the poster's, not the pale site mesh.
      [
        'background:' + MESH + ';background-size:180% 180%;animation:vbgshift 22s',
        'background:' + POSTER_MESH + ';background-size:180% 180%;animation:vbgshift 22s',
      ],
      // A taller frame above and below; the card keeps the page content
      // width, so its edges line up with the heading above it.
      [
        'padding:clamp(28px,4vw,64px) max(clamp(20px,4vw,56px),calc(50vw - 664px))',
        'padding:clamp(48px,6vw,112px) max(clamp(20px,4vw,56px),calc(50vw - 664px))',
      ],
      // The drifting blobs were lit for a pale ground and wash this one out.
      ...[
        ['closest-side,rgba(205,220,250,.95),transparent', 'closest-side,rgba(205,220,250,.3),transparent'],
        ['closest-side,#5f93f3,transparent', 'closest-side,rgba(95,147,243,.4),transparent'],
        ['closest-side,#1f55e0,transparent', 'closest-side,rgba(31,85,224,.75),transparent'],
      ].map(([a, b]) => [a, b]),
      // The rails were grey hairlines from when the panel was white; on the
      // mesh ground they read as a different drawing to everything on it.
      ...[
        ['<div style="position:absolute;left:70px;top:169.25px;width:810px;height:1.5px;background:#d9dae0"></div>', 'rgba(18,48,214,.2)'],
        ['<div style="position:absolute;left:279.25px;top:200px;width:1.5px;height:115px;background:#d9dae0"></div>', 'rgba(18,48,214,.2)'],
      ].map(([html, c]) => [html, html.replace('#d9dae0', c)]),
      ...[
        'left:70px;top:250px;width:810px;border-top:1.5px dashed #c4c8d4',
        'left:880px;top:200px;height:50px;border-left:1.5px dashed #c4c8d4',
        'left:70px;top:200px;height:50px;border-left:1.5px dashed #c4c8d4',
      ].map((d) => [d, d.replace('#c4c8d4', 'rgba(18,48,214,.26)')]),
      // ✓ and ✕ are text glyphs here, and Plus Jakarta draws a thin, slanted
      // check that sits off-centre in its disc. Both become drawn marks.
      [
        'justify-content:center;font-size:10px">\u2713</span>',
        'justify-content:center;flex-shrink:0">' +
          '<svg width="9" height="9" viewBox="0 0 16 16" fill="none" stroke="#fff"' +
          ' stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round">' +
          '<path d="M3 8.5L6.5 12L13 4.5"></path></svg></span>',
      ],
      [
        'justify-content:center;font-size:9px">\u2715</span>',
        'justify-content:center;flex-shrink:0">' +
          '<svg width="8" height="8" viewBox="0 0 16 16" fill="none" stroke="#8b8e97"' +
          ' stroke-width="2.6" stroke-linecap="round">' +
          '<path d="M4 4l8 8M12 4l-8 8"></path></svg></span>',
      ],
      // A full blue outline made every node a boxed icon. The step tabs carry
      // their state on the top edge over a soft hairline; nodes do the same.
      ...[0, 1, 2, 3, 4, 5].map((i) => [
        `border:1.5px solid {{ nb${i} }};background:`,
        `border:0;border-top:2px solid {{ nb${i} }};background:`,
      ]),
      // The mesh is authored large; at 60px a node samples only its palest
      // corner, so scale it up the way the avatar chips do.
      ...[0, 1, 2, 3, 4, 5].map((i) => [
        `background:{{ nbg${i} }};box-shadow:`,
        `background:{{ nbg${i} }};background-size:210% 210%;background-position:${['0% 0%', '100% 20%', '30% 100%', '80% 80%', '50% 40%', '10% 60%'][i]};box-shadow:`,
      ]),
      // The step tabs read as flat boxes against it.
      [
        'min-width:84px;border:0;background:rgba(255,255,255,.6);box-shadow:inset 0 0 0 1px rgba(18,48,214,.1)',
        'min-width:84px;border:0;background:linear-gradient(180deg,rgba(255,255,255,.92),rgba(255,255,255,.6))' +
          ';box-shadow:inset 0 0 0 1px rgba(18,48,214,.1),0 8px 16px -14px rgba(18,48,214,.6)',
      ],
      // Four state badges were stadium and three were square boxes. Buttons
      // stay square — the split is state vs. action, not slide by slide.
      [
        `font-size:12px;letter-spacing:.1em;color:#1230d6;padding:7px 11px;background:rgba(255,255,255,.75);box-shadow:inset 0 0 0 1px rgba(18,48,214,.16);white-space:nowrap"`,
        `font-size:12px;letter-spacing:.1em;color:#1230d6;padding:7px 13px;border-radius:999px;background:rgba(255,255,255,.75);box-shadow:inset 0 0 0 1px rgba(18,48,214,.16);white-space:nowrap"`,
      ],
      [
        'gap:7px;font-size:14.5px;border:1px solid #d0d2d8;background:#fff;padding:7px 12px"',
        "gap:7px;font-family:'JetBrains Mono',monospace;font-size:13px;letter-spacing:.06em;border:1px solid #d0d2d8;background:#fff;padding:7px 13px;border-radius:999px\"",
      ],
      [
        '<span style="font-size:17px;display:flex;align-items:center;gap:8px"><span style="width:9px;height:9px;border-radius:50%;background:#1230d6;display:block"></span>Active</span>',
        `<span style="${STATE_BADGE('#1230d6', '#e3e8ff')};justify-self:start"><span style="width:7px;height:7px;border-radius:50%;background:#1230d6;display:block"></span>Active</span>`,
      ],
      [
        '<span style="font-size:17px;display:flex;align-items:center;gap:8px"><span style="width:9px;height:9px;border-radius:50%;background:#c4c6cc;display:block"></span>Revoked</span>',
        `<span style="${STATE_BADGE('#8b8e97', 'rgba(255,255,255,.8)')};justify-self:start"><span style="width:7px;height:7px;border-radius:50%;background:#c4c6cc;display:block"></span>Revoked</span>`,
      ],
      // These three are logos, not avatars, so they sit inside the tile
      // instead of covering it. Aider publishes an SVG and stays remote.
      ...LOGOS.map(([src, file, alt]) => [
        `<img src="${src}" alt="${alt}" title="${alt}" style="width:100%;height:100%;object-fit:cover;display:block;filter:grayscale(1) contrast(1.15);mix-blend-mode:multiply">`,
        `<img src="/logos/${file}.svg" alt="${alt}" title="${alt}" style="width:68%;height:68%;object-fit:contain;display:block;opacity:.78">`,
      ]),
    ],
    inject: [["animation:vbgshift 22s ease-in-out infinite alternate;\">", POSTER_DOTS]],
    // The design linked screens by hash and by file; the app uses routes.
    urls: { '#console': '/console' },
  },
  {
    src: 'design/Console.dc.html',
    out: 'src/generated/console',
    name: 'Console',
    markers: [],
    urls: { 'Veil%20402%20Landing.dc.html': '/' },
    edits: [
      [
        ';width:100%;height:54px;padding:7px 10px;display:flex;align-items:center;gap:8px;cursor:default',
        ';width:fit-content;max-width:100%' + BADGE(46, '6px 12px 6px 6px') + ';cursor:default',
      ],
    ],
    // Markup the app adds to the design's header, kept here so `npm run gen`
    // does not drop it.
    inject: [
      [
        '<div style="display:flex;align-items:stretch;height:40px;flex-shrink:0">',
        '<a href="{{ playgroundUrl }}" target="_blank" rel="noopener"' +
          ' style="display:flex;align-items:center;align-self:center;background-image:' +
          MESH +
          ';background-repeat:no-repeat;background-position:0 0;background-size:0% 100%' +
          ';background-color:rgba(255,255,255,.75);box-shadow:inset 0 0 0 1px rgba(18,48,214,.16)' +
          ";color:#0d0e12;font-family:'JetBrains Mono',monospace;font-size:11.5px;font-weight:500" +
          ';letter-spacing:.08em;text-transform:uppercase;white-space:nowrap;text-decoration:none' +
          ';padding:9px 14px;margin-right:14px' +
          ';transition:background-size .5s cubic-bezier(.65,0,.25,1),color .2s ease .12s,box-shadow .25s ease"' +
          ' style-hover="background-size:100% 100%;color:#fff;box-shadow:inset 0 0 0 1px rgba(18,48,214,0)"' +
          '>Playground</a>',
      ],
    ],
  },
]

// ---------------------------------------------------------------- css helpers

/** Split a `a:b;c:d` style string on top-level `;`, respecting quotes/parens. */
function splitDecls(style) {
  const out = []
  let depth = 0
  let quote = null
  let start = 0
  for (let i = 0; i < style.length; i++) {
    const ch = style[i]
    if (quote) {
      if (ch === quote && style[i - 1] !== '\\') quote = null
      continue
    }
    if (ch === '"' || ch === "'") quote = ch
    else if (ch === '(') depth++
    else if (ch === ')') depth--
    else if (ch === ';' && depth === 0) {
      out.push(style.slice(start, i))
      start = i + 1
    }
  }
  out.push(style.slice(start))
  return out.map((d) => d.trim()).filter(Boolean)
}

function splitDecl(decl) {
  const i = decl.indexOf(':')
  return i === -1 ? null : [decl.slice(0, i).trim(), decl.slice(i + 1).trim()]
}

// ------------------------------------------------------------ expr rewriting

const LITERAL = /^(true|false|null|-?\d+(\.\d+)?|'[^']*'|"[^"]*")$/

/** Collects every non-loop root identifier a template reads, so the hook can be typed. */
let collected = new Set()

/** `{{ col.barH }}` -> `col.barH` under a loop, else `v.hOp`. */
function rewriteExpr(expr, scope) {
  const e = expr.trim()
  if (LITERAL.test(e)) return e
  const root = /^[A-Za-z_$][\w$]*/.exec(e)
  if (!root) return e
  if (scope.has(root[0])) return e
  collected.add(root[0])
  return `v.${e}`
}

const HAS_BINDING = /\{\{([^}]*)\}\}/

/** Turn a mixed `a {{ b }} c` attribute value into a JS template expression. */
function valueToExpr(value, scope) {
  if (!HAS_BINDING.test(value)) return JSON.stringify(value)
  const only = /^\s*\{\{([^}]*)\}\}\s*$/.exec(value)
  if (only) return rewriteExpr(only[1], scope)
  const tpl = value.replace(/\{\{([^}]*)\}\}/g, (_, e) => '${' + rewriteExpr(e, scope) + '}')
  return '`' + tpl.replace(/`/g, '\\`') + '`'
}

// ------------------------------------------------------------- attribute map

const SVG_CAMEL = new Set([
  'stroke-width', 'stroke-linecap', 'stroke-linejoin', 'stroke-dasharray',
  'stroke-dashoffset', 'stroke-opacity', 'stroke-miterlimit', 'fill-opacity',
  'fill-rule', 'clip-rule', 'clip-path', 'text-anchor', 'font-size',
  'font-family', 'font-weight', 'stop-color', 'stop-opacity', 'gradientUnits',
  'gradientTransform', 'patternUnits', 'markerWidth', 'markerHeight',
  'refX', 'refY', 'viewBox', 'preserveAspectRatio', 'baseFrequency',
  'numOctaves', 'stitchTiles', 'colorInterpolationFilters', 'dominant-baseline',
  'letter-spacing', 'paint-order', 'vector-effect', 'shape-rendering',
])

const RENAME = { class: 'className', for: 'htmlFor', inputmode: 'inputMode', tabindex: 'tabIndex', colspan: 'colSpan', rowspan: 'rowSpan', maxlength: 'maxLength', autocomplete: 'autoComplete', readonly: 'readOnly', contenteditable: 'contentEditable', spellcheck: 'spellCheck', srcset: 'srcSet', crossorigin: 'crossOrigin' }

const camel = (s) => s.replace(/-([a-z])/g, (_, c) => c.toUpperCase())

/** parse5 lowercases attribute names, so React's camelCase has to be restored. */
const EVENTS = [
  'Click', 'DoubleClick', 'Change', 'Input', 'Submit', 'Focus', 'Blur', 'KeyDown', 'KeyUp',
  'KeyPress', 'MouseDown', 'MouseUp', 'MouseEnter', 'MouseLeave', 'MouseMove', 'MouseOver',
  'MouseOut', 'PointerDown', 'PointerUp', 'PointerMove', 'PointerEnter', 'PointerLeave',
  'TouchStart', 'TouchEnd', 'TouchMove', 'Scroll', 'Wheel', 'Drag', 'DragStart', 'DragEnd',
  'DragOver', 'Drop', 'Copy', 'Paste', 'ContextMenu', 'AnimationEnd', 'TransitionEnd',
]
const EVENT_BY_LOWER = new Map(EVENTS.map((e) => [`on${e.toLowerCase()}`, `on${e}`]))

function reactAttrName(name) {
  if (RENAME[name]) return RENAME[name]
  if (name.startsWith('data-') || name.startsWith('aria-')) return name
  if (SVG_CAMEL.has(name)) return camel(name)
  if (EVENT_BY_LOWER.has(name)) return EVENT_BY_LOWER.get(name)
  return name
}

// ----------------------------------------------------------------- keyframes

/** Pulls `@keyframes name{...}` blocks out of the design's <helmet> stylesheet. */
function extractKeyframes(css) {
  const out = []
  const re = /@keyframes\s+([\w-]+)\s*\{/g
  let m
  while ((m = re.exec(css))) {
    let depth = 1
    let i = re.lastIndex
    while (i < css.length && depth > 0) {
      if (css[i] === '{') depth++
      else if (css[i] === '}') depth--
      i++
    }
    out.push({ name: m[1], text: css.slice(m.index, i) })
    re.lastIndex = i
  }
  return out
}

// ------------------------------------------------------------- block markers

/**
 * `sc-for` / `sc-if` become comments before parsing.
 *
 * As elements they are subject to HTML tree construction: inside `<select>` or
 * `<table>` an unknown element is dropped and its children foster-parented,
 * which silently loses the loop. Comments survive in every insertion mode.
 */
function markBlocks(html) {
  return html
    .replace(/<sc-for\b([^>]*)>/g, (_, attrs) => {
      const list = /list="\{\{([^}]*)\}\}"/.exec(attrs)[1].trim()
      const as = /\bas="([^"]*)"/.exec(attrs)[1]
      return `<!--dc:for ${list}|${as}-->`
    })
    .replace(/<\/sc-for>/g, '<!--dc:end-->')
    .replace(/<sc-if\b([^>]*)>/g, (_, attrs) => {
      const value = /value="\{\{([^}]*)\}\}"/.exec(attrs)[1].trim()
      return `<!--dc:if ${value}-->`
    })
    .replace(/<\/sc-if>/g, '<!--dc:end-->')
}

/** Rebuilds the nesting the marker comments encode, as a flat sibling scan. */
function groupBlocks(nodes) {
  const out = []
  const stack = [out]
  for (const node of nodes) {
    const top = stack[stack.length - 1]
    const marker = node.nodeName === '#comment' ? /^dc:(for|if|end)\s*(.*)$/.exec(node.data) : null
    if (!marker) {
      top.push(node)
      continue
    }
    const [, kind, rest] = marker
    if (kind === 'end') {
      if (stack.length > 1) stack.pop()
      continue
    }
    const block =
      kind === 'for'
        ? { block: 'for', list: rest.split('|')[0], as: rest.split('|')[1], childNodes: [] }
        : { block: 'if', value: rest, childNodes: [] }
    top.push(block)
    stack.push(block.childNodes)
  }
  return out
}

// ------------------------------------------------------------------ the pass

class Generator {
  constructor(name, urls = {}) {
    this.name = name
    this.urls = urls
    /** @type {Map<string, {cls: string, body: string[], hover: string[], vars: string[]}>} */
    this.classes = new Map()
    this.bindings = new Set()
    /** @type {Array<{name: string, jsx: string}>} */
    this.components = []
  }

  /** Hoist one element's style and its :hover / :focus overrides into a shared class. */
  styleClass(style, hover, focus) {
    const key = `${style}@@${hover}@@${focus}`
    const found = this.classes.get(key)
    if (found) return found

    const body = []
    const vars = []
    for (const decl of splitDecls(style)) {
      const kv = splitDecl(decl)
      if (!kv) continue
      const [prop, value] = kv
      if (HAS_BINDING.test(value)) {
        const i = vars.length
        vars.push(value)
        body.push(`${prop}: var(--d${i});`)
      } else {
        body.push(`${prop}: ${value};`)
      }
    }
    const states = {
      ':hover': hover ? splitDecls(hover).map((d) => `${d};`) : [],
      ':focus-visible': focus ? splitDecls(focus).map((d) => `${d};`) : [],
    }

    const entry = { cls: `s${String(this.classes.size + 1).padStart(3, '0')}`, body, states, vars }
    this.classes.set(key, entry)
    return entry
  }

  css(keyframes) {
    const out = []
    // CSS Modules rewrites `animation-name` inside a module even when the
    // keyframes are global, so the module has to carry its own copy.
    for (const k of keyframes) out.push(k.text)
    for (const e of this.classes.values()) {
      out.push(`.${e.cls} {\n  ${e.body.join('\n  ')}\n}`)
      for (const [state, decls] of Object.entries(e.states)) {
        if (decls.length) out.push(`.${e.cls}${state} {\n  ${decls.join('\n  ')}\n}`)
      }
    }
    // Grain is a texture, not an animation. The design jitters it five times a
    // second, which reads as noise crawling across the large flat panels, so it
    // is held still everywhere while the gradients and blobs keep drifting.
    const grainy = [...this.classes.values()]
      .filter((e) => e.body.some((d) => /^animation:.*\bvgrain\b/.test(d)))
      .map((e) => `.${e.cls}`)
    if (grainy.length) {
      out.push(`${grainy.join(',\n')} {\n  animation: none;\n}`)
    }

    // Replaces the design's `[style*="vbgshift"]` rule, which could only match
    // while the animation lived in an inline style attribute.
    const drifting = [...this.classes.values()]
      .filter((e) => e.body.some((d) => /^animation:.*\bvbgshift\b/.test(d)))
      .map((e) => `.${e.cls}`)
    if (drifting.length) {
      out.push(
        `@media (prefers-reduced-motion: reduce) {\n  ${drifting.join(',\n  ')} {\n    animation: none;\n  }\n}`,
      )
    }
    return out.join('\n\n') + '\n'
  }

  // -- node walking ---------------------------------------------------------

  text(node) {
    const t = node.value
    // Whitespace that spans lines is the source file's indentation; whitespace
    // within a line is a real word break between inline elements.
    if (!t.trim()) return t.includes('\n') ? '' : "{' '}"
    if (HAS_BINDING.test(t)) {
      const parts = []
      let last = 0
      for (const m of t.matchAll(/\{\{([^}]*)\}\}/g)) {
        if (m.index > last) parts.push(this.literal(t.slice(last, m.index)))
        parts.push(`{${rewriteExpr(m[1], this.scope)}}`)
        last = m.index + m[0].length
      }
      if (last < t.length) parts.push(this.literal(t.slice(last)))
      return parts.join('')
    }
    return this.literal(t)
  }

  literal(t) {
    return /[{}<>]/.test(t) ? `{${JSON.stringify(t)}}` : t
  }

  children(nodes, indent) {
    return groupBlocks(nodes).map((n) => this.node(n, indent)).filter(Boolean).join('')
  }

  edgeSpace(raw) {
    const lead = /^\s+/.exec(raw)?.[0] ?? ''
    const trail = /\s+$/.exec(raw)?.[0] ?? ''
    return [
      lead && !lead.includes('\n') ? "{' '}" : '',
      trail && !trail.includes('\n') ? "{' '}" : '',
    ]
  }

  node(node, indent) {
    if (node.block === 'for') {
      const list = rewriteExpr(node.list, this.scope)
      this.scope.add(node.as)
      const inner = this.children(node.childNodes, indent + '  ')
      this.scope.delete(node.as)
      return `\n${indent}{${list}.map((${node.as}, i) => (\n${indent}  <Fragment key={i}>${inner}\n${indent}  </Fragment>\n${indent}))}`
    }
    if (node.block === 'if') {
      const cond = rewriteExpr(node.value, this.scope)
      const inner = this.children(node.childNodes, indent + '  ')
      return `\n${indent}{${cond} ? (\n${indent}  <>${inner}\n${indent}  </>\n${indent}) : null}`
    }
    if (node.nodeName === '#text') {
      const t = this.text(node)
      if (!t.trim()) return t
      const [lead, trail] = this.edgeSpace(node.value)
      return `\n${indent}${lead}${t.trim()}${trail}`
    }
    if (node.nodeName === '#comment') return ''

    const tag = node.tagName
    const attrs = Object.fromEntries(node.attrs.map((a) => [a.name, a.value]))

    if (tag === 'dc-import') {
      const props = Object.entries(attrs)
        .filter(([k]) => k.startsWith('on-'))
        .map(([k, val]) => ` ${camel(k.slice(3).replace(/^/, 'on-'))}={${rewriteExpr(/\{\{([^}]*)\}\}/.exec(val)[1], this.scope)}}`)
        .join('')
      return `\n${indent}<${attrs.name}Import${props} />`
    }
    if (tag === 'helmet') {
      this.helmet = node
      return ''
    }

    // -- split point: emit this subtree as its own component
    const splitName = attrs['data-dc-split'] || attrs['data-screen-label']
    if (splitName && !this.splitting) {
      const compName = splitName.replace(/(^|\s)(\w)/g, (_, _s, c) => c.toUpperCase()).replace(/\s/g, '')
      this.splitting = true
      const jsx = this.element(node, tag, attrs, '  ')
      this.splitting = false
      this.components.push({ name: compName, jsx })
      return `\n${indent}<${compName} v={v} />`
    }

    return this.element(node, tag, attrs, indent)
  }

  element(node, tag, attrs, indent) {
    const props = []
    const styleEntries = []

    if (attrs.style !== undefined) {
      const e = this.styleClass(attrs.style, attrs['style-hover'] || '', attrs['style-focus'] || '')
      props.push(`className={s.${e.cls}}`)
      e.vars.forEach((value, i) => {
        styleEntries.push(`'--d${i}': ${valueToExpr(value, this.scope)}`)
      })
    } else if (attrs['class']) {
      props.push(`className=${JSON.stringify(attrs['class'])}`)
    }

    for (const [name, value] of Object.entries(attrs)) {
      if (name === 'style' || name.startsWith('style-') || name === 'class') continue
      if (name.startsWith('hint-') || name === 'data-dc-split') continue
      const prop = reactAttrName(name)
      if ((name === 'href' || name === 'src') && this.urls[value]) {
        props.push(`${prop}=${JSON.stringify(this.urls[value])}`)
        continue
      }
      if (HAS_BINDING.test(value)) {
        const expr = valueToExpr(value, this.scope)
        props.push(`${prop}={${expr}}`)
      } else {
        props.push(`${prop}=${JSON.stringify(value)}`)
      }
    }

    if (styleEntries.length) {
      props.push(`style={{ ${styleEntries.join(', ')} } as CSSVars}`)
    }

    const open = props.length ? `<${tag}\n${indent}  ${props.join(`\n${indent}  `)}\n${indent}>` : `<${tag}>`
    const inner = this.children(node.childNodes, indent + '  ')
    if (!inner.trim()) {
      const selfClose = props.length
        ? `<${tag}\n${indent}  ${props.join(`\n${indent}  `)}\n${indent}/>`
        : `<${tag} />`
      return `\n${indent}${selfClose}`
    }
    return `\n${indent}${open}${inner}\n${indent}</${tag}>`
  }

  run(templateHtml) {
    this.scope = new Set()
    collected = this.bindings
    const frag = parseFragment(markBlocks(templateHtml))
    const jsx = this.children(frag.childNodes, '    ')
    return jsx
  }
}

// ---------------------------------------------------------------- file emit

function componentFile(name, jsx, bindingsType) {
  const needsFragment = /<Fragment key/.test(jsx)
  const imports = [`import type { CSSVars } from '../../styles/css-vars'`]
  if (needsFragment) imports.unshift(`import { Fragment } from 'react'`)
  imports.push(`import type { ${bindingsType} } from './bindings'`)
  imports.push(`import s from './styles.module.css'`)
  return `${imports.join('\n')}\n\nexport function ${name}({ v }: { v: ${bindingsType} }) {\n  return (\n    <>${jsx}\n    </>\n  )\n}\n`
}

for (const t of TARGETS) {
  let src = readFileSync(`${ROOT}${t.src}`, 'utf8')
  for (const [find, attrs] of t.markers) {
    if (!src.includes(find)) throw new Error(`marker not found in ${t.src}: ${find}`)
    src = src.replace(find, find.replace('<div ', `<div ${attrs}`))
  }
  for (const [find, replace] of t.edits ?? []) {
    if (!src.includes(find)) throw new Error(`edit target not found in ${t.src}: ${find.slice(0, 60)}…`)
    src = src.split(find).join(replace)
  }
  for (const [after, html] of t.inject ?? []) {
    if (!src.includes(after)) throw new Error(`inject anchor not found in ${t.src}: ${after}`)
    src = src.replace(after, after + html)
  }

  const open = /<x-dc(?:\s[^>]*)?>/.exec(src)
  const close = src.lastIndexOf('</x-dc>')
  const template = src.slice(open.index + open[0].length, close)

  const g = new Generator(t.name, t.urls ?? {})
  const rootJsx = g.run(template)

  const helmetCss = g.helmet?.childNodes.find((n) => n.tagName === 'style')?.childNodes[0].value ?? ''
  const keyframes = extractKeyframes(helmetCss)

  const outDir = `${ROOT}${t.out}`
  mkdirSync(outDir, { recursive: true })

  const bindingsType = `${t.name}Bindings`
  for (const c of g.components) {
    writeFileSync(`${outDir}/${c.name}.tsx`, componentFile(c.name, c.jsx, bindingsType))
  }
  const rootImports = g.components.map((c) => `import { ${c.name} } from './${c.name}'`)
  writeFileSync(
    `${outDir}/Root.tsx`,
    componentFile('Root', rootJsx, bindingsType).replace(
      `import s from './styles.module.css'`,
      `import s from './styles.module.css'\n${rootImports.join('\n')}`,
    ),
  )
  writeFileSync(`${outDir}/styles.module.css`, g.css(keyframes))

  // Every `{{ name }}` the template reads, so the hand-written hook can be typed against it.
  const names = [...g.bindings].sort()
  writeFileSync(
    `${outDir}/bindings.ts`,
    `/* Generated: every binding the ${t.name} template reads. */\nexport interface ${bindingsType} {\n${names
      .map((n) => `  ${/^[A-Za-z_$][\w$]*$/.test(n) ? n : JSON.stringify(n)}: unknown`)
      .join('\n')}\n}\n`,
  )

  console.log(
    `${t.name}: ${g.classes.size} classes, ${keyframes.length} keyframes, ` +
      `${names.length} bindings, ${g.components.length + 1} components -> ${t.out}`,
  )
}
