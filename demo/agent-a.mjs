#!/usr/bin/env node
// Your build agent. You ask it for a smart contract in plain words; it finds
// the ContractShop (another agent), gets an x402 "402 Payment Required", pays
// for it PRIVATELY through your Veil wallet, waits for the build, and hands you
// the finished contract. Everything after your one prompt happens on its own.
//
//   node agent-a.mjs                      → chat: ask it, it does the rest
//   node agent-a.mjs "an escrow contract" → one-shot

import { writeFileSync } from 'node:fs'
import { createInterface } from 'node:readline'

const SHOP = process.env.AGENT_B ?? 'http://127.0.0.1:8500'
const DAEMON_A = process.env.DAEMON_A ?? 'http://127.0.0.1:8402/api/v1'

const usd = (units) => `$${(units / 1e6).toFixed(2)}`
const sleep = (ms) => new Promise((r) => setTimeout(r, ms))

async function post(url, body) {
  const res = await fetch(url, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  })
  return { status: res.status, body: await res.json().catch(() => ({})) }
}

/** One commission, start to finish, from a single plain-language request. */
async function commission(request) {
  console.log(`\n🤖 on it — commissioning: "${request}"`)

  // 1. Ask the ContractShop. It answers with a 402 and a price.
  const first = await post(`${SHOP}/task`, { request })
  if (first.status !== 402) throw new Error(`the shop did not ask for payment (got ${first.status})`)
  const { invoiceId, price, payTo } = first.body
  console.log(`   → the ContractShop asked for ${usd(price)} to build it  (x402: 402 Payment Required)`)

  // 2. Pay privately through the pool. Keep the blinding — it is the proof.
  console.log(`   → paying ${usd(price)} privately through the shielded pool…`)
  const pay = await post(`${DAEMON_A}/pay`, { address: payTo, amount: price })
  if (pay.body.status !== 'ok') throw new Error(pay.body.error || 'payment failed')
  const { recipient_commitment: commitment, blinding } = pay.body.result
  console.log(`   ✓ paid — on chain this is one opaque commitment ${commitment.slice(0, 12)}…  (no sender, recipient, or amount)`)

  // 3. Present the proof and wait for the build. The fetch blocks while the
  //    shop computes; the loop only re-tries while payment isn't yet confirmed.
  console.log(`   → proving the payment and waiting for the build…`)
  for (let i = 0; i < 200; i++) {
    const retry = await post(`${SHOP}/task`, { invoiceId, commitment, blinding })
    if (retry.status === 200) {
      const r = retry.body
      console.log(`   ✓ delivered: ${r.contract || 'a contract'}${r.address ? ' @ ' + r.address : ''}`)
      if (r.network) console.log(`     network:  ${r.network}`)
      if (r.explorerUrl) console.log(`     explorer: ${r.explorerUrl}`)
      if (r.source) {
        const out = `./${r.contract || 'Contract'}.sol`
        writeFileSync(out, r.source)
        console.log(`     saved ${r.source.split('\n').length} lines of Solidity to ${out}`)
      }
      console.log('   done.')
      return r
    }
    await sleep(2000)
  }
  throw new Error('the shop never confirmed the payment')
}

async function main() {
  const arg = process.argv.slice(2).join(' ')
  if (arg) {
    await commission(arg)
    return
  }
  const rl = createInterface({ input: process.stdin, output: process.stdout })
  console.log("🤖 I'm your build agent. Tell me what smart contract you need and I'll")
  console.log('   commission it from the ContractShop and pay for it privately. (Ctrl-C to quit)')
  const loop = () =>
    rl.question('\nyou> ', async (line) => {
      const req = line.trim()
      if (req === 'exit' || req === 'quit') return rl.close()
      if (req) {
        try {
          await commission(req)
        } catch (error) {
          console.error('   ✗ failed:', error.message)
        }
      }
      loop()
    })
  loop()
}

main().catch((error) => {
  console.error('failed:', error.message)
  process.exit(1)
})
