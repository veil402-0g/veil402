/** The seven scenes of the protocol walkthrough. */
export interface ProtocolScene {
  title: string
  body: string
  /** Badge copy; also selects the badge's colour triplet. */
  pill: string
  /** Index of the node that lights up, 0..5. */
  node: number
  /** Packet x-position at the start and end of the scene, in design px. */
  from: number
  to: number
  /** Label that rides along with the packet. */
  packet: string
}

export const PROTOCOL_SCENES: ProtocolScene[] = [
  {
    title: 'Your agent',
    body: 'Connect any existing AI agent, tool, or SDK to confidential compute through one secure, OpenAI-compatible endpoint.',
    pill: 'Enforced',
    node: 0,
    from: 70,
    to: 280,
    packet: 'POST /v1/chat',
  },
  {
    title: 'The Gateway to private compute',
    body: 'Runs on your infrastructure. You own the agent keys, budgets, and private accounting ledger, while encrypted communication and ZK proofs protect and make spending selectively auditable.',
    pill: 'Yours',
    node: 1,
    from: 280,
    to: 280,
    packet: 'reserve 161 µUSD',
  },
  {
    title: 'Shielded pool',
    body: 'One shielded note locks the session. Calls are metered privately, then one ZK proof settles payment and returns the change.',
    pill: 'Unlinkable',
    node: 5,
    from: 280,
    to: 280,
    packet: 'session open',
  },
  {
    title: 'Enclave · the seller',
    body: 'The enclave sets prices, authorises escrow, processes confidential requests, meters usage, and signs receipts — without learning the agent’s identity.',
    pill: 'Attested',
    node: 2,
    from: 280,
    to: 490,
    packet: 'metered',
  },
  {
    title: '0G Private Computer',
    body: 'Routes encrypted requests to TEE providers, verifies execution, and handles billing without learning the payer’s identity.',
    pill: 'Blind',
    node: 3,
    from: 490,
    to: 690,
    packet: 'routed',
  },
  {
    title: 'The model runs in a TEE',
    body: 'TeeML only: weights run inside Intel TDX. Attestation is provider-level, not yet bound to each response.',
    pill: 'Provider-level',
    node: 4,
    from: 690,
    to: 880,
    packet: 'inference',
  },
  {
    title: 'Answer streamed, spend proved',
    body: 'The answer streams back to your agent. The spend settles on Hedera with a Groth16 proof.',
    pill: 'Groth16 · settled',
    node: 0,
    from: 880,
    to: 70,
    packet: '',
  },
]

/** How far along the node chain each scene has progressed. */
export const SCENE_REACH = [1, 1, 1, 2, 3, 4, 4]

/** Which hop in the strip highlights per scene; -1 means none. */
export const SCENE_HOP = [0, 1, 2, 3, 4, 5, -1]

/** Badge colours keyed by pill copy: [text, background, dot]. */
export function pillColours(pill: string): [string, string, string] {
  if (pill === 'Yours') return ['#fff', '#1230d6', '#fff']
  if (pill === 'Provider-level') return ['#4d505a', '#f1f2f5', '#8b8e97']
  return ['#1230d6', '#eef1ff', '#1230d6']
}
