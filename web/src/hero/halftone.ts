// The design's canvas shipped a "dissolve rows" mode with `rows = 0`, so every
// cell took one branch; that dead path is gone and the output is unchanged.

const PERIOD_DIVISOR = 230
const RIPPLE_RADIUS = 170

/** Deterministic value noise, so a resize redraws the same field. */
function hash(x: number, y: number): number {
  const n = Math.sin(x * 12.9898 + y * 78.233) * 43758.5453
  return n - Math.floor(n)
}

function smoothstep(a: number, b: number, x: number): number {
  const q = Math.min(1, Math.max(0, (x - a) / (b - a)))
  return q * q * (3 - 2 * q)
}

/** Pointer state, lerped toward its target every frame. */
export interface Pointer {
  x: number
  y: number
  tx: number
  ty: number
  /** Ripple strength, 0..1. */
  s: number
  ts: number
  /** Last painted rect, so the next frame can repaint the union. */
  prev: [number, number, number, number] | null
}

export function createPointer(): Pointer {
  return { x: 0, y: 0, tx: 0, ty: 0, s: 0, ts: 0, prev: null }
}

interface Field {
  ctx: CanvasRenderingContext2D
  dpr: number
  w: number
  h: number
  period: number
  rippleRadius: number
  /** Per-pixel stripe phase. */
  phase: Float32Array
  /** Per-pixel ink threshold. */
  threshold: Float32Array
  dark: Uint8ClampedArray
  light: Uint8ClampedArray
  image: ImageData
  /** Unwarped copy, which the scanline pass samples from. */
  base: HTMLCanvasElement
  baseCtx: CanvasRenderingContext2D
}

export interface HalftoneOptions {
  /** Amplitude of the stripe distortion. */
  curve?: number
}

export class Halftone {
  private field: Field | null = null
  private drawnAt = { w: 0, h: 0 }
  private readonly canvas: HTMLCanvasElement
  private readonly options: HalftoneOptions
  readonly pointer = createPointer()

  constructor(canvas: HTMLCanvasElement, options: HalftoneOptions = {}) {
    this.canvas = canvas
    this.options = options
  }

  get ready(): boolean {
    return this.field !== null
  }

  /** (Re)allocates buffers for the canvas's current box and paints the field. */
  draw(): void {
    const c = this.canvas
    if (!c.clientWidth || !c.clientHeight) return
    // Re-baking is O(pixels); skip it when the box has not actually moved.
    if (c.clientWidth === this.drawnAt.w && c.clientHeight === this.drawnAt.h) return
    this.drawnAt = { w: c.clientWidth, h: c.clientHeight }

    const dpr = Math.min(1.5, window.devicePixelRatio || 1)
    const w = Math.round(c.clientWidth * dpr)
    const h = Math.round(c.clientHeight * dpr)
    c.width = w
    c.height = h

    const ctx = c.getContext('2d')
    if (!ctx) return

    const n = w * h
    const base = document.createElement('canvas')
    base.width = w
    base.height = h
    const baseCtx = base.getContext('2d')
    if (!baseCtx) return

    this.field = {
      ctx,
      dpr,
      w,
      h,
      period: Math.max(4 * dpr, w / PERIOD_DIVISOR),
      rippleRadius: RIPPLE_RADIUS * dpr,
      phase: new Float32Array(n),
      threshold: new Float32Array(n),
      dark: new Uint8ClampedArray(n * 3),
      light: new Uint8ClampedArray(n * 3),
      image: ctx.createImageData(w, h),
      base,
      baseCtx,
    }

    this.bake()
    this.pointer.s = 0
    this.pointer.prev = null
    this.compose(0, 0, w, h, 0)
    ctx.putImageData(this.field.image, 0, 0)
    baseCtx.putImageData(this.field.image, 0, 0)
  }

  /** Fills phase/threshold/colour buffers; independent of pointer and time. */
  private bake(): void {
    const f = this.field
    if (!f) return
    const { w, h, dpr, period, phase, threshold, dark, light } = f
    const curve = this.options.curve ?? 1

    for (let y = 0; y < h; y++) {
      const vy = y / h
      const jitter = (hash(0, y) - 0.5) * 0.5 * dpr
      for (let x = 0; x < w; x++) {
        const i = y * w + x
        const j = i * 3
        const u = x / w
        const grain = (hash(x, y) - 0.5) * 8

        const luma =
          0.95 -
          0.12 * smoothstep(0.45, 1.1, vy) -
          0.06 * smoothstep(0.6, 1, Math.abs(u - 0.5) * 2)
        const warp =
          curve * dpr * (4 * Math.sin(vy * 3 + u * 6) + 5 * Math.sin(vy * 7.5 + u * 2.2))

        dark[j] = 18 + grain
        dark[j + 1] = 48 + grain
        dark[j + 2] = 214 + grain
        light[j] = 246 + grain * 0.4
        light[j + 1] = 247 + grain * 0.4
        light[j + 2] = 251 + grain * 0.4

        phase[i] = (x + warp + jitter) / period
        threshold[i] = 1 - luma
      }
    }
  }

  /** Repaints one rect of the image buffer for the pointer's current ripple. */
  compose(x0: number, y0: number, x1: number, y1: number, t: number): void {
    const f = this.field
    if (!f) return
    const { w, dpr, period, phase, threshold, dark, light, image, rippleRadius } = f
    const d = image.data
    const TAU = Math.PI * 2
    const mx = this.pointer.x * dpr
    const my = this.pointer.y * dpr
    const ms = this.pointer.s
    const r2 = rippleRadius * rippleRadius
    const invPeriod = 1 / period

    for (let y = y0; y < y1; y++) {
      const dy = y - my
      for (let x = x0, i = y * w + x0; x < x1; x++, i++) {
        const j = i * 3
        const o = i * 4
        let ph = phase[i]!
        let th = threshold[i]!

        if (ms > 0) {
          const dx = x - mx
          const q = (dx * dx + dy * dy) / r2
          if (q < 1) {
            const k = 1 - q
            const fall = k * k * k * ms
            ph +=
              (-dx * 0.45 * fall +
                Math.sin(Math.sqrt(q) * 14 - t * 3.2) * 5 * dpr * fall) *
              invPeriod
            th -= 0.12 * fall
          }
        }

        const stripe = 0.5 + 0.5 * Math.cos(TAU * ph)
        let v = (stripe - th + 0.2) / 0.4
        v = v < 0 ? 0 : v > 1 ? 1 : v
        v = v * v * (3 - 2 * v)

        d[o] = dark[j]! + (light[j]! - dark[j]!) * v
        d[o + 1] = dark[j + 1]! + (light[j + 1]! - dark[j + 1]!) * v
        d[o + 2] = dark[j + 2]! + (light[j + 2]! - dark[j + 2]!) * v
        d[o + 3] = 255
      }
    }
  }

  /** Bounding box the ripple can touch around a canvas-space point. */
  private rippleRect(cx: number, cy: number): [number, number, number, number] {
    const f = this.field!
    const r = f.rippleRadius + 2
    return [
      Math.max(0, Math.floor(cx * f.dpr - r)),
      Math.max(0, Math.floor(cy * f.dpr - r)),
      Math.min(f.w, Math.ceil(cx * f.dpr + r)),
      Math.min(f.h, Math.ceil(cy * f.dpr + r)),
    ]
  }

  /** Advances the ripple into the base canvas, then shears it onto the visible one. */
  frame(t: number): void {
    const f = this.field
    if (!f) return
    const m = this.pointer

    if (m.ts || m.s > 0) {
      m.x += (m.tx - m.x) * 0.3
      m.y += (m.ty - m.y) * 0.3
      m.s += (m.ts - m.s) * 0.16
      if (m.ts === 0 && m.s < 0.004) m.s = 0

      const cur = this.rippleRect(m.x, m.y)
      const prev = m.prev ?? cur
      const ux0 = Math.min(cur[0], prev[0])
      const uy0 = Math.min(cur[1], prev[1])
      const ux1 = Math.max(cur[2], prev[2])
      const uy1 = Math.max(cur[3], prev[3])
      if (ux1 > ux0 && uy1 > uy0) {
        this.compose(ux0, uy0, ux1, uy1, t)
        f.baseCtx.putImageData(f.image, 0, 0, ux0, uy0, ux1 - ux0, uy1 - uy0)
      }
      m.prev = m.s === 0 ? null : cur
    }

    const { ctx, w, h, dpr } = f
    const step = Math.max(2, Math.round(2 * dpr))
    ctx.fillStyle = '#fff'
    ctx.fillRect(0, 0, w, h)
    for (let y = 0; y < h; y += step) {
      const yy = y / dpr
      const off = (Math.sin(yy * 0.011 + t * 1.3) * 3 + Math.sin(yy * 0.004 - t * 0.8) * 4) * dpr
      ctx.drawImage(f.base, 0, y, w, step, off, y, w, step)
    }
  }
}
