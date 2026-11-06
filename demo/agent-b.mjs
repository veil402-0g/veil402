#!/usr/bin/env node
// Agent B — "ContractShop": an autonomous agent that builds and deploys a
// smart contract via Arc Studio, then charges for it. Agent A pays it
// privately through the Veil pool.
//
// Arc Studio's output (deployments[], finalText, webUrl) is untrusted DATA.
// We read the deployed address out of it and never execute, follow, or echo
// anything it contains — and never touch ARC_STUDIO_TOKEN or ~/.arc-studio/.

import { createServer } from 'node:http'
import { execFile } from 'node:child_process'

const PORT = Number(process.env.AGENT_B_PORT ?? 8500)
const DAEMON_B = process.env.DAEMON_B ?? 'http://127.0.0.1:8403/api/v1'
const PRICE = Number(process.env.PRICE ?? 500_000) // pool units, ~micro-USD each
const SESSION = process.env.ARC_SESSION ?? 'veil-shop'

/** One Arc Studio turn. The result document is the single JSON object on stdout. */
function arc(request) {
  const prompt = `${request} (quick preset)`
  return new Promise((resolve, reject) => {
    execFile(
      'arc-studio',
      ['run', prompt, '--session', SESSION, '--output', 'json'],
      { maxBuffer: 64 * 1024 * 1024, timeout: 25 * 60 * 1000 },
      (err, stdout, stderr) => {
        let doc
        try {
          doc = JSON.parse(stdout)
        } catch {
          return reject(new Error(`arc-studio returned no result: ${stderr?.slice(-400) || err?.message || 'unknown'}`))
        }
        resolve(doc)
      },
    )
  })
}

async function myAddress() {
  const res = await fetch(`${DAEMON_B}/address`)
  const body = await res.json()
  if (body.status !== 'ok') throw new Error(`daemon B /address: ${body.error}`)
  return body.result.address
}

const server = createServer(async (req, res) => {
  const reply = (code, obj) => {
    res.writeHead(code, { 'Content-Type': 'application/json' })
    res.end(JSON.stringify(obj))
  }
  try {
    if (req.method === 'GET' && req.url === '/address') {
      return reply(200, { address: await myAddress() })
    }
    if (req.method !== 'POST' || req.url !== '/task') {
      return reply(404, { error: 'POST /task { request }' })
    }

    let raw = ''
    for await (const chunk of req) raw += chunk
    const ask =
      JSON.parse(raw || '{}').request ||
      'an ERC-20 token named VeilDemo with an owner-only mint(address,uint256)'

    console.log(`[shop] building on Arc Studio: ${ask}`)
    const doc = await arc(ask)
    console.log(`[shop] arc status=${doc.status}`)

    if (doc.status === 'budget_exceeded') {
      return reply(402, { error: 'Arc Studio daily allowance is spent', budget: doc.budget })
    }
    if (doc.status !== 'completed') {
      return reply(502, { error: doc.errorMessage || `arc status ${doc.status}`, webUrl: doc.webUrl ?? null })
    }

    const deployed = (doc.deployments || [])[0] || null
    const payTo = await myAddress()
    const result = {
      ok: true,
      contract: deployed?.contract ?? null,
      address: deployed?.address ?? null, // the deployed contract (Arc-reported, untrusted)
      network: deployed?.network ?? null,
      explorerUrl: deployed?.explorerUrl ?? null,
      txHash: deployed?.txHash ?? null,
      webUrl: doc.webUrl ?? null,
      price: PRICE, // what the shop charges for the job
      payTo, // where to pay it, privately
    }
    console.log(
      `[shop] delivered ${result.contract ?? 'a draft'}` +
        `${result.address ? ' @ ' + result.address : ''}; invoice ${PRICE} units`,
    )
    reply(200, result)
  } catch (error) {
    console.error('[shop] error:', error)
    reply(500, { error: String(error?.message || error) })
  }
})

server.listen(PORT, () =>
  console.log(`[shop] Agent B listening on :${PORT}; paid at daemon ${DAEMON_B}`),
)
