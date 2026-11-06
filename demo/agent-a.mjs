#!/usr/bin/env node
// Agent A — the client. It needs a smart contract. It asks the ContractShop
// (Agent B), gets back an x402 "402 Payment Required", pays B PRIVATELY through
// its own Veil daemon, then presents the proof and collects the contract.
//
// On chain the payment is only commitments and nullifiers. The proof A hands
// B is the note's blinding — a secret only the payer holds — so B can confirm
// it was paid without the payment ever being public.

import { writeFileSync } from 'node:fs'

const SHOP = process.env.AGENT_B ?? 'http://127.0.0.1:8500'
const DAEMON_A = process.env.DAEMON_A ?? 'http://127.0.0.1:8402/api/v1'
const request =
  process.argv.slice(2).join(' ') ||
  'an ERC-20 token named VeilDemo with an owner-only mint(address,uint256)'

const sleep = (ms) => new Promise((r) => setTimeout(r, ms))

async function post(url, body) {
  const res = await fetch(url, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  })
  return { status: res.status, body: await res.json().catch(() => ({})) }
}

async function main() {
  console.log(`[client] I need: ${request}`)

  // 1. Ask. Expect a 402 with the price and where to pay it.
  console.log('[client] → POST /task')
  const first = await post(`${SHOP}/task`, { request })
  if (first.status !== 402) throw new Error(`expected 402, got ${first.status}`)
  const { invoiceId, price, payTo } = first.body
  console.log(`[client] ← 402 Payment Required: ${price} units → ${String(payTo).slice(0, 30)}…  (invoice ${String(invoiceId).slice(0, 8)})`)

  // 2. Pay privately through the pool. Keep the blinding — it is the proof.
  console.log('[client] paying privately through the pool…')
  const pay = await post(`${DAEMON_A}/pay`, { address: payTo, amount: price })
  if (pay.body.status !== 'ok') throw new Error(pay.body.error || 'payment failed')
  const { recipient_commitment: commitment, blinding } = pay.body.result
  console.log(`[client] paid. commitment ${String(commitment).slice(0, 16)}… (the chain shows no sender, recipient, or amount)`)

  // 3. Present the proof. B confirms the note arrived in its own wallet; retry
  //    while B's daemon catches up (discovery + a block).
  console.log('[client] → POST /task with proof, waiting for B to confirm…')
  for (let i = 0; i < 15; i++) {
    const retry = await post(`${SHOP}/task`, { invoiceId, commitment, blinding })
    if (retry.status === 200) {
      const r = retry.body
      console.log(`[client] ← 200 OK. delivered: ${r.contract ?? 'a draft'}${r.address ? ' @ ' + r.address : ''}`)
      if (r.network) console.log(`[client]   network: ${r.network}`)
      if (r.explorerUrl) console.log(`[client]   explorer: ${r.explorerUrl}`)
      if (r.source) {
        const out = `./${r.contract || 'Contract'}.sol`
        writeFileSync(out, r.source)
        const lines = r.source.split('\n')
        console.log(`[client]   received the computed contract (${lines.length} lines), saved to ${out}:`)
        console.log('    ' + lines.slice(0, 22).join('\n    '))
        if (lines.length > 22) console.log('    … (full file on disk)')
      }
      console.log('[client] done — requested, paid privately over x402, and received the computed result.')
      return
    }
    await sleep(2000)
  }
  throw new Error('B never confirmed the payment')
}

main().catch((error) => {
  console.error('[client] failed:', error.message)
  process.exit(1)
})
