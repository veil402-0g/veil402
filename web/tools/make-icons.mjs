/**
 * Generates the favicons from the brand mark: a disc with a square at its
 * centre, the same shape the hero, footer and console header draw in CSS.
 *
 * The PNGs are encoded here rather than checked in by hand so the mark lives in
 * one place and the binaries are reproducible.
 *
 * Usage: node tools/make-icons.mjs
 */
import { writeFileSync } from 'node:fs'
import { deflateSync, crc32 } from 'node:zlib'

const BLUE = [0x12, 0x30, 0xd6]
const WHITE = [0xff, 0xff, 0xff]
/** The square is 30% of the disc, as in the markup. */
const SQUARE = 0.3
/** Supersampling per axis, so the disc edge is not jagged at 32px. */
const SS = 4

const ROOT = new URL('..', import.meta.url).pathname

function chunk(type, data) {
  const out = Buffer.alloc(8 + data.length + 4)
  out.writeUInt32BE(data.length, 0)
  out.write(type, 4, 'ascii')
  data.copy(out, 8)
  out.writeUInt32BE(crc32(Buffer.concat([Buffer.from(type, 'ascii'), data])), 8 + data.length)
  return out
}

/** @param {(x: number, y: number) => [number, number, number, number]} shade */
function png(size, shade) {
  const rows = Buffer.alloc(size * (size * 4 + 1))
  for (let y = 0; y < size; y++) {
    const row = y * (size * 4 + 1)
    rows[row] = 0 // filter: none
    for (let x = 0; x < size; x++) {
      let r = 0
      let g = 0
      let b = 0
      let a = 0
      for (let sy = 0; sy < SS; sy++) {
        for (let sx = 0; sx < SS; sx++) {
          const [pr, pg, pb, pa] = shade((x + (sx + 0.5) / SS) / size, (y + (sy + 0.5) / SS) / size)
          r += pr * pa
          g += pg * pa
          b += pb * pa
          a += pa
        }
      }
      const n = SS * SS
      const o = row + 1 + x * 4
      // Un-premultiply, so a soft edge keeps its colour instead of going grey.
      rows[o] = a ? Math.round(r / a) : 0
      rows[o + 1] = a ? Math.round(g / a) : 0
      rows[o + 2] = a ? Math.round(b / a) : 0
      rows[o + 3] = Math.round((a / n) * 255)
    }
  }

  const ihdr = Buffer.alloc(13)
  ihdr.writeUInt32BE(size, 0)
  ihdr.writeUInt32BE(size, 4)
  ihdr[8] = 8 // bit depth
  ihdr[9] = 6 // RGBA
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk('IHDR', ihdr),
    chunk('IDAT', deflateSync(rows, { level: 9 })),
    chunk('IEND', Buffer.alloc(0)),
  ])
}

const inSquare = (u, v) => Math.abs(u - 0.5) <= SQUARE / 2 && Math.abs(v - 0.5) <= SQUARE / 2

/** Browser tab: a blue disc on transparency, white square at its centre. */
const disc = (u, v) => {
  if (inSquare(u, v)) return [...WHITE, 1]
  const d = Math.hypot(u - 0.5, v - 0.5)
  return d <= 0.5 ? [...BLUE, 1] : [0, 0, 0, 0]
}

/** Home screen: iOS masks its own corners, so this is full-bleed. */
const tile = (u, v) => (inSquare(u, v) ? [...WHITE, 1] : [...BLUE, 1])

writeFileSync(`${ROOT}public/favicon-32.png`, png(32, disc))
writeFileSync(`${ROOT}public/favicon-192.png`, png(192, disc))
writeFileSync(`${ROOT}public/apple-touch-icon.png`, png(180, tile))

const pct = (n) => +(n * 100).toFixed(1)
writeFileSync(
  `${ROOT}public/favicon.svg`,
  `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100">
  <circle cx="50" cy="50" r="50" fill="#1230d6"/>
  <rect x="${pct((1 - SQUARE) / 2)}" y="${pct((1 - SQUARE) / 2)}" width="${pct(SQUARE)}" height="${pct(SQUARE)}" fill="#fff"/>
</svg>
`,
)

console.log('icons: favicon.svg, favicon-32.png, favicon-192.png, apple-touch-icon.png')
