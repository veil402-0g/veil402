#!/usr/bin/env node
// Agent B — "ContractShop": an autonomous agent that sells a built-and-deployed
// smart contract, over a private x402 handshake.
//
//   A  POST /task {request}                    -> 402 {invoiceId, price, payTo}
//   A  pays privately through the pool
//   A  POST /task {invoiceId, commitment, blinding}
//      -> B asks its OWN daemon to confirm it holds that exact note (the
//         blinding proves the claimer is the payer; the chain shows nothing)
//      -> B builds the contract on Arc Studio and returns it.
//
// Arc Studio's output is untrusted DATA: we read the deployed address and
// never execute, follow, or echo anything it contains.

import { createServer } from 'node:http'
import { execFile } from 'node:child_process'
import { randomUUID } from 'node:crypto'

const PORT = Number(process.env.AGENT_B_PORT ?? 8500)
const DAEMON_B = process.env.DAEMON_B ?? 'http://127.0.0.1:8403/api/v1'
const PRICE = Number(process.env.PRICE ?? 500_000) // pool units, ~micro-USD each
const SESSION = process.env.ARC_SESSION ?? 'veil-shop'

/** Open invoices, by id. A real seller would persist and expire these. */
const invoices = new Map()

function arcRun(args) {
  return new Promise((resolve, reject) => {
    execFile(
      'arc-studio',
      args,
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

/** Run one task, auto-answering Arc's planning questions with the defaults. */
async function arc(request) {
  const prompt = `${request}. Write it and deploy it to Arc Testnet. (quick preset)`
  let doc = await arcRun(['run', prompt, '--session', SESSION, '--output', 'json'])
  for (let guard = 0; doc.status === 'needs_input' && doc.questions && guard < 3; guard++) {
    const answers = (doc.questions.questions || []).map((q) => ({
      questionId: q.id,
      selectedOptionIds: [q.options?.[0]?.id].filter(Boolean),
      selectedOptionLabels: [q.options?.[0]?.label].filter(Boolean),
    }))
    console.log(`[shop] Arc asked ${answers.length} planning question(s); answering with defaults`)
    doc = await arcRun(['run', 'answers: use the defaults', '--session', SESSION, '--answers-json', JSON.stringify(answers), '--output', 'json'])
  }
  return doc
}

async function myAddress() {
  const res = await fetch(`${DAEMON_B}/address`)
  const body = await res.json()
  if (body.status !== 'ok') throw new Error(`daemon B /address: ${body.error}`)
  return body.result.address
}

/** Read one file out of the Arc Studio sandbox — the actual computed result. */
function cat(path) {
  return new Promise((resolve, reject) => {
    execFile(
      'arc-studio',
      ['cat', path, '--session', SESSION],
      { maxBuffer: 16 * 1024 * 1024, timeout: 60_000 },
      (err, stdout) => (err ? reject(err) : resolve(stdout)),
    )
  })
}

/** Ask B's own daemon whether it holds the note the payer is claiming. */
async function paid(commitment, blinding, amount) {
  const res = await fetch(`${DAEMON_B}/confirm`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ commitment, blinding, amount }),
  })
  const body = await res.json()
  return body.status === 'ok' && body.result.confirmed === true
}

/** Do the actual work once payment is proven, and return the artifact itself. */
async function fulfil(request) {
  if (process.env.STUB) {
    console.log('[shop] STUB mode: returning a canned contract (no Arc call)')
    return {
      contract: 'VeilToken',
      address: `0x${'de'.repeat(20)}`,
      network: 'stub',
      explorerUrl: null,
      txHash: null,
      webUrl: null,
      sourcePath: 'contracts/VeilToken.sol',
      source:
        '// SPDX-License-Identifier: MIT\npragma solidity ^0.8.24;\n\ncontract VeilToken {\n    // canned stub — unset STUB for a real Arc Studio build\n}\n',
    }
  }
  console.log(`[shop] building on Arc Studio: ${request}`)
  const doc = await arc(request)
  if (doc.status !== 'completed') throw new Error(doc.errorMessage || `arc status ${doc.status}`)

  const deployed = (doc.deployments || [])[0] || null
  // The artifact A actually paid for: pull the Solidity back out of the sandbox.
  const sol =
    (doc.filesChanged || []).find((p) => /\.sol$/.test(p) && /contract/i.test(p)) ||
    (doc.filesChanged || []).find((p) => /\.sol$/.test(p))
  let source = null
  if (sol) {
    try {
      source = await cat(sol)
    } catch (error) {
      console.error('[shop] could not read the source:', error.message)
    }
  }
  return {
    contract: deployed?.contract ?? null,
    address: deployed?.address ?? null,
    network: deployed?.network ?? null,
    explorerUrl: deployed?.explorerUrl ?? null,
    txHash: deployed?.txHash ?? null,
    webUrl: doc.webUrl ?? null,
    sourcePath: sol ?? null,
    source,
  }
}

const server = createServer(async (req, res) => {
  const reply = (code, obj) => {
    res.writeHead(code, { 'Content-Type': 'application/json' })
    res.end(JSON.stringify(obj))
  }
  try {
    if (req.method === 'GET' && req.url === '/address') return reply(200, { address: await myAddress() })
    if (req.method !== 'POST' || req.url !== '/task') return reply(404, { error: 'POST /task' })

    let raw = ''
    for await (const chunk of req) raw += chunk
    const { request, invoiceId, commitment, blinding } = JSON.parse(raw || '{}')

    // First contact: no proof yet -> issue an invoice and a 402.
    if (!commitment || !blinding || !invoiceId) {
      const id = randomUUID()
      const ask = request || 'an ERC-20 token named VeilDemo with an owner-only mint(address,uint256)'
      invoices.set(id, { price: PRICE, request: ask, fulfilled: false })
      const payTo = await myAddress()
      console.log(`[shop] 402 for invoice ${id.slice(0, 8)} — ${PRICE} units`)
      return reply(402, { invoiceId: id, price: PRICE, payTo, reason: 'payment required' })
    }

    // Second contact: the payer presents its proof.
    const invoice = invoices.get(invoiceId)
    if (!invoice) return reply(404, { error: 'unknown invoice' })
    if (invoice.fulfilled) return reply(200, invoice.result)

    if (!(await paid(commitment, blinding, invoice.price))) {
      console.log(`[shop] invoice ${invoiceId.slice(0, 8)}: payment not confirmed yet`)
      return reply(402, { invoiceId, price: invoice.price, reason: 'payment not confirmed' })
    }

    console.log(`[shop] invoice ${invoiceId.slice(0, 8)}: payment confirmed, fulfilling`)
    const work = await fulfil(invoice.request)
    invoice.fulfilled = true
    invoice.result = { ok: true, ...work }
    console.log(`[shop] delivered ${work.contract ?? 'a draft'}${work.address ? ' @ ' + work.address : ''}`)
    return reply(200, invoice.result)
  } catch (error) {
    console.error('[shop] error:', error)
    reply(500, { error: String(error?.message || error) })
  }
})

server.listen(PORT, () => console.log(`[shop] Agent B (x402) on :${PORT}; paid at daemon ${DAEMON_B}`))
