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
    // The design linked screens by hash and by file; the app uses routes.
    hrefs: { '#console': '/console' },
  },
  {
    src: 'design/Console.dc.html',
    out: 'src/generated/console',
    name: 'Console',
    markers: [],
    hrefs: { 'Veil%20402%20Landing.dc.html': '/' },
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
  constructor(name, hrefs = {}) {
    this.name = name
    this.hrefs = hrefs
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
      ':focus': focus ? splitDecls(focus).map((d) => `${d};`) : [],
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
      if (name === 'href' && this.hrefs[value]) {
        props.push(`href=${JSON.stringify(this.hrefs[value])}`)
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
  for (const [find, inject] of t.markers) {
    if (!src.includes(find)) throw new Error(`marker not found in ${t.src}: ${find}`)
    src = src.replace(find, find.replace('<div ', `<div ${inject}`))
  }

  const open = /<x-dc(?:\s[^>]*)?>/.exec(src)
  const close = src.lastIndexOf('</x-dc>')
  const template = src.slice(open.index + open[0].length, close)

  const g = new Generator(t.name, t.hrefs ?? {})
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
