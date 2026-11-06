#!/usr/bin/env node
// Agent A — the client. It needs a smart contract, asks the ContractShop
// (Agent B) to build one, then pays Agent B PRIVATELY through its own Veil
// daemon. On chain the payment is only commitments and nullifiers: no sender,
// no recipient, no amount.

const SHOP = process.env.AGENT_B ?? 'http://127.0.0.1:8500'
const DAEMON_A = process.env.DAEMON_A ?? 'http://127.0.0.1:8402/api/v1'
const request =
  process.argv.slice(2).join(' ') ||
  'an ERC-20 token named VeilDemo with an owner-only mint(address,uint256)'

async function call(url, init) {
  const res = await fetch(url, init)
  const body = await res.json().catch(() => ({}))
  if (!res.ok || body.status === 'error') throw new Error(body.error || `${url} -> ${res.status}`)
  return body
}

async function main() {
  console.log(`[client] I need: ${request}`)
  console.log('[client] asking the ContractShop (this builds + deploys on Arc Studio; it can take a few minutes)…')
  const job = await call(`${SHOP}/task`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ request }),
  })

  console.log(`[client] delivered: ${job.contract ?? 'a draft'}${job.address ? ' @ ' + job.address : ''}`)
  if (job.network) console.log(`[client] network: ${job.network}`)
  if (job.explorerUrl) console.log(`[client] explorer: ${job.explorerUrl}`)
  console.log(`[client] invoice: ${job.price} pool units, pay to ${String(job.payTo).slice(0, 30)}…`)

  console.log('[client] paying privately through the pool…')
  const paid = await call(`${DAEMON_A}/pay`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ address: job.payTo, amount: job.price }),
  })

  console.log(
    `[client] paid. output commitment ${String(paid.result.recipient_commitment).slice(0, 18)}… — ` +
      'the chain shows no sender, no recipient, no amount.',
  )
  console.log("[client] Agent B's console shows the received payment within a sync interval (~5s).")
}

main().catch((error) => {
  console.error('[client] failed:', error.message)
  process.exit(1)
})
