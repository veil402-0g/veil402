import { Fragment } from 'react'
import type { CSSVars } from '../ui/css-vars'
import type { ConsoleBindings } from './useConsoleBindings'
import s from './console.module.css'


/** A value the daemon has not answered for yet. */
function Skel({ w, h, r = 3 }: { w: number | string; h: number; r?: number }) {
  return (
    <span
      className={s.skel}
      style={{ '--d0': typeof w === 'number' ? `${w}px` : w, '--d1': `${h}px`, '--d2': `${r}px` } as CSSVars}
    />
  )
}

/** A list the daemon has not answered for yet. */
function SkelRows({ n, h }: { n: number; h: number }) {
  return (
    <div className={s.skelRows}>
      {Array.from({ length: n }, (_, i) => (
        <Skel key={i} w="100%" h={h} r={4} />
      ))}
    </div>
  )
}

export function ConsoleView({ v }: { v: ConsoleBindings }) {
  return (
    <>
    <div
      className={s.s001}
    >
      <header
        className={s.s002}
      >
        <a
          className={s.s003}
          href="/"
          onClick={v.goHome}
          aria-label="Back to Veil402 home"
        >
          <span
            className={s.s004}
            aria-hidden="true"
          >
            <span
              className={s.s005}
            />
          </span>
          <span
            className={s.s006}
          >
            Veil402
          </span>
          <span
            className={s.s007}
          >
            CONSOLE
          </span>
        </a>
        <div
          className={s.s008}
        >
          <a
            className={s.s009}
            href={v.playgroundUrl}
            target="_blank"
            rel="noopener"
          >
            Playground
          </a>
          <button
            className={s.s010}
          >
            <span
              className={s.s011}
            >
              HW
            </span>
            <span
              className={s.s012}
            >
              <span
                className={s.s013}
              >
                Hashira Works
              </span>
              <span
                className={s.s014}
              >
                TREASURY ADMIN
              </span>
            </span>
          </button>
        </div>
      </header>
      <div
        className={s.s015}
      >
        <main
          className={s.s016}
        >
          <section
            className={s.s023}
          >
            <div
              className={s.s024}
            >
              <div
                className={s.s025}
              >
                <div
                  className={s.s026}
                >
                  <h1
                    className={s.s027}
                  >
                    Shielded pool
                  </h1>
                </div>
                <div
                  className={s.s028}
                >
                  <span
                    className={s.s029}
                  >
                    <span
                      className={s.s030}
                    >
                      {v.loading ? <Skel w={26} h={16} /> : v.noteCount}
                    </span>
                    <span
                      className={s.s031}
                    >
                      NOTES
                    </span>
                  </span>
                  <span
                    className={s.s029}
                  >
                    <span
                      className={s.s032}
                    >
                      {v.loading ? <Skel w={42} h={16} /> : v.anonSet}
                    </span>
                    <span
                      className={s.s031}
                    >
                      ANONYMITY SET
                    </span>
                  </span>
                </div>
              </div>
              <div
                className={s.s033}
              >
                <div
                  className={s.s034}
                  onMouseEnter={v.balOn}
                  onMouseLeave={v.balOff}
                >
                  <span
                    className={s.s035}
                    style={{ '--d0': `${v.balG},100% 100%` } as CSSVars}
                  >
                    {v.loading ? <Skel w={236} h={88} r={6} /> : v.balInt}
                  </span>
                  <span
                    className={s.s036}
                    style={{ '--d0': `${v.balG},100% 100%` } as CSSVars}
                  >
                    {v.loading ? <Skel w={74} h={40} r={5} /> : <>.{v.balDec}</>}
                  </span>
                </div>
                <div
                  className={s.s037}
                >
                  <span
                    className={s.s038}
                  >
                    USDC
                  </span>
                  <span
                    className={s.s039}
                  >
                    {v.loading ? <Skel w={128} h={11} /> : v.lastDeposit}
                  </span>
                  <span
                    className={s.s039}
                  >
                    {v.loading ? <Skel w={172} h={11} /> : v.walletF}
                  </span>
                </div>
              </div>
              <p
                className={s.s040}
              >
                Balance is held as encrypted notes on Hedera. Only this organisation's viewing key can sum them — the chain sees commitments, not amounts.
              </p>
              <div
                className={s.s041}
              >
                <button
                  className={s.s042}
                  onClick={v.openFund}
                >
                  Fund pool{' '}
                  <span
                    className={s.s043}
                  >
                    +
                  </span>
                </button>
                <button
                  className={s.s044}
                  onClick={v.openMandate}
                >
                  Issue mandate
                </button>
              </div>
            </div>
            <div
              className={s.s045}
            >
              {v.stats.map((st, i) => (
                <Fragment key={i}>
                <div
                  className={s.s046}
                  onMouseEnter={st.enter}
                  onMouseLeave={v.statOff}
                >
                  <span
                    className={s.s047}
                    aria-hidden="true"
                    style={{ '--d0': st.clip } as CSSVars}
                  />
                  <span
                    className={s.s048}
                    aria-hidden="true"
                    style={{ '--d0': st.bar } as CSSVars}
                  />
                  <div
                    className={s.s049}
                    style={{ '--d0': st.labC } as CSSVars}
                  >
                    <span>
                      {st.label}
                    </span>
                    <span
                      className={s.s050}
                      style={{ '--d0': st.arO, '--d1': st.arT } as CSSVars}
                    >
                      ↗
                    </span>
                  </div>
                  <div
                    className={s.s051}
                    style={{ '--d0': st.valC } as CSSVars}
                  >
                    {v.loading ? <Skel w={86} h={24} r={4} /> : st.value}
                  </div>
                  <div
                    className={s.s052}
                    style={{ '--d0': st.subC } as CSSVars}
                  >
                    {v.loading ? <Skel w={104} h={10} /> : st.sub}
                  </div>
                </div>
                </Fragment>
              ))}
            </div>
          </section>
          <section
            className={s.s053}
          >
            <div
              className={s.s054}
            >
              <span
                className={s.s055}
              >
                Each dot is {v.loading ? <Skel w={62} h={10} /> : v.dotUnit} of funded capital — where every unit currently sits.
              </span>
              <span
                className={s.s056}
              >
                total funded {v.loading ? <Skel w={52} h={10} /> : v.totalFunded}
              </span>
            </div>
            <div
              className={s.s057}
            >
              {v.segs.map((sg, i) => (
                <Fragment key={i}>
                <div
                  className={s.s058}
                  onMouseEnter={sg.enter}
                  onMouseLeave={v.dotOff}
                  style={{ '--d0': sg.left, '--d1': sg.display, '--d2': sg.op } as CSSVars}
                >
                  <span
                    className={s.s059}
                    style={{ '--d0': sg.sq } as CSSVars}
                  />
                  {sg.label}{' '}
                  <span
                    className={s.s060}
                    style={{ '--d0': sg.pc } as CSSVars}
                  >
                    {v.loading ? <Skel w={24} h={10} /> : sg.pct}
                  </span>
                </div>
                </Fragment>
              ))}
            </div>
            <div
              className={s.s061}
            >
              {v.loading ? <Skel w="100%" h={93} r={6} /> : null}
              <div
                className={s.s062}
                onMouseLeave={v.dotOff}
                hidden={v.loading}
              >
                {v.dots.map((d, i) => (
                  <Fragment key={i}>
                  <span
                    className={s.s063}
                    onMouseEnter={d.enter}
                    style={{ '--d0': d.bg, '--d1': d.op, '--d2': d.tf, '--d3': d.tr } as CSSVars}
                  />
                  </Fragment>
                ))}
              </div>
            </div>
            <div
              className={s.s064}
            />
            <div
              className={s.s065}
            >
              {v.ticks.map((tk, i) => (
                <Fragment key={i}>
                <span>
                  {tk.label}
                </span>
                </Fragment>
              ))}
            </div>
          </section>
          <section
            className={s.s066}
          >
            <div
              className={s.s067}
            >
              <div
                className={s.s068}
              >
                <div
                  className={s.s069}
                >
                  <h2
                    className={s.s027}
                  >
                    Agents
                  </h2>
                  <div
                    className={s.s028}
                  >
                    <span
                      className={s.s029}
                    >
                      <span
                        className={s.s030}
                      >
                        {v.loading ? <Skel w={20} h={16} /> : v.agentN}
                      </span>
                      <span
                        className={s.s031}
                      >
                        MANDATES
                      </span>
                    </span>
                    <span
                      className={s.s029}
                    >
                      <span
                        className={s.s032}
                      >
                        {v.loading ? <Skel w={20} h={16} /> : v.agentA}
                      </span>
                      <span
                        className={s.s031}
                      >
                        ACTIVE
                      </span>
                    </span>
                  </div>
                </div>
                <span
                  className={s.s070}
                >
                  Each agent spends from a private mandate.
                </span>
              </div>
              <div
                className={s.s071}
              >
                {v.chartColumns ? (
                  <>
                  <div
                    className={s.s072}
                  >
                    <span
                      className={s.s073}
                    >
                      <span
                        className={s.s074}
                      />
                      Spent
                    </span>
                    <span
                      className={s.s073}
                    >
                      <span
                        className={s.s074}
                      />
                      In escrow
                    </span>
                    <span
                      className={s.s073}
                    >
                      <span
                        className={s.s075}
                      />
                      Remaining
                    </span>
                  </div>
                  </>
                ) : null}
                <label
                  className={s.agentSearch}
                >
                  <input
                    className={s.agentSearchInput}
                    value={v.agentQuery}
                    onChange={v.setAgentQuery}
                    placeholder="Search mandates"
                    aria-label="Search mandates"
                  />
                  {v.hasAgentQuery ? (
                    <button
                      className={s.agentSearchClear}
                      onClick={v.clearAgentQuery}
                      aria-label="Clear search"
                    >
                      ×
                    </button>
                  ) : null}
                  <span
                    className={s.agentSearchGlass}
                    aria-hidden="true"
                  />
                </label>
                <button
                  className={s.s044}
                  onClick={v.openMandate}
                >
                  + Issue mandate
                </button>
              </div>
            </div>
            {v.hasAgents ? (
              <>
              {v.chartRange ? (
                <>
                <div
                  className={s.s076}
                >
                  <div
                    className={s.s077}
                  >
                    <span>
                      Each column is one mandate. The white line marks spend today; the column rises to projected spend at period end — 8 days left.
                    </span>
                    <div
                      className={s.s078}
                    >
                      <span
                        className={s.s079}
                      >
                        <span
                          className={s.s080}
                        />
                        SPENT TODAY
                      </span>
                      <span
                        className={s.s079}
                      >
                        <span
                          className={s.s081}
                        />
                        PROJECTED
                      </span>
                      <span
                        className={s.s079}
                      >
                        <span
                          className={s.s082}
                        />
                        OVER LIMIT
                      </span>
                    </div>
                  </div>
                  <div
                    className={s.s083}
                    onMouseLeave={v.rangeOff}
                  >
                    <span
                      className={s.s084}
                      aria-hidden="true"
                    />
                    <span
                      className={s.s085}
                    >
                      <span>
                        LIMIT
                      </span>
                      <span
                        className={s.s086}
                      >
                        100%
                      </span>
                    </span>
                    <span
                      className={s.s087}
                    >
                      125%
                    </span>
                    <span
                      className={s.s088}
                    >
                      0
                    </span>
                    <div
                      className={s.s089}
                    >
                      <div
                        className={s.s090}
                      >
                        {v.rangeCols.map((r, i) => (
                          <Fragment key={i}>
                          <div
                            className={s.s091}
                            onMouseEnter={r.enter}
                          >
                            <div
                              className={s.s092}
                              style={{ '--d0': r.barH, '--d1': r.grow, '--d2': `transform 1s cubic-bezier(.2,.8,.2,1) ${r.dl}` } as CSSVars}
                            >
                              <span
                                className={s.s093}
                                aria-hidden="true"
                                style={{ '--d0': r.barBg, '--d1': r.barO } as CSSVars}
                              />
                              <span
                                className={s.s094}
                                aria-hidden="true"
                                style={{ '--d0': r.todayB } as CSSVars}
                              />
                            </div>
                          </div>
                          </Fragment>
                        ))}
                      </div>
                      <div
                        className={s.s095}
                      >
                        {v.rangeCols.map((r, i) => (
                          <Fragment key={i}>
                          <div
                            className={s.s096}
                            style={{ '--d0': r.cO, '--d1': r.cT, '--d2': `opacity .6s ease ${r.dl2},transform .7s cubic-bezier(.2,.8,.2,1) ${r.dl2}` } as CSSVars}
                          >
                            <div
                              className={s.s097}
                              onMouseEnter={r.enter}
                              style={{ '--d0': r.cFill } as CSSVars}
                            >
                              <span
                                className={s.s098}
                                aria-hidden="true"
                                style={{ '--d0': r.avBg, '--d1': r.avPos } as CSSVars}
                              >
                                {r.ini}
                              </span>
                              <span
                                className={s.s099}
                              >
                                <span
                                  className={s.s100}
                                  style={{ '--d0': r.cTx } as CSSVars}
                                  title={r.name}
                                >
                                  {r.name}
                                </span>
                                <span
                                  className={s.s101}
                                  style={{ '--d0': r.cPc } as CSSVars}
                                >
                                  {r.projF}
                                </span>
                              </span>
                            </div>
                            <span
                              className={s.s102}
                              aria-hidden="true"
                              style={{ '--d0': r.conH } as CSSVars}
                            >
                              <span
                                className={s.s103}
                              />
                            </span>
                          </div>
                          </Fragment>
                        ))}
                      </div>
                    </div>
                  </div>
                </div>
                </>
              ) : null}
              {v.chartColumns ? (
                <>
                <div
                  className={s.s104}
                >
                  <div
                    className={s.s105}
                  >
                    {v.agentCols.map((c, i) => (
                      <Fragment key={i}>
                      <div
                        className={s.s106}
                        style={{ '--d0': c.op } as CSSVars}
                      >
                        <div
                          className={s.s107}
                        >
                          <span
                            className={s.s108}
                          />
                          {c.ghosts.map((g, i) => (
                            <Fragment key={i}>
                            <span
                              className={s.s109}
                              style={{ '--d0': g.b } as CSSVars}
                            />
                            </Fragment>
                          ))}
                          <span
                            className={s.s110}
                            style={{ '--d0': c.spentH, '--d1': c.grad } as CSSVars}
                          />
                          <span
                            className={s.s111}
                            style={{ '--d0': `calc(${c.spentH} + 10px)`, '--d1': c.escH } as CSSVars}
                          />
                          <span
                            className={s.s112}
                            style={{ '--d0': c.spentH, '--d1': c.head, '--d2': c.headBorder } as CSSVars}
                          />
                        </div>
                        <span
                          className={s.s113}
                          style={{ '--d0': c.sBg, '--d1': c.sBorder } as CSSVars}
                        />
                        <div
                          className={s.s114}
                        >
                          <span
                            className={s.s115}
                          >
                            {c.name}
                          </span>
                          <span
                            className={s.s116}
                          >
                            {c.pctF}
                          </span>
                        </div>
                      </div>
                      </Fragment>
                    ))}
                  </div>
                  <div
                    className={s.s117}
                  >
                    <div
                      className={s.s118}
                    >
                      <span
                        className={s.s119}
                      >
                        100%
                      </span>
                      <span
                        className={s.s120}
                      >
                        75%
                      </span>
                      <span
                        className={s.s121}
                      >
                        50%
                      </span>
                      <span
                        className={s.s122}
                      >
                        25%
                      </span>
                    </div>
                    <span
                      className={s.s123}
                    >
                      {v.spentPctAll}
                      <span
                        className={s.s124}
                      >
                        %
                      </span>
                    </span>
                    <span
                      className={s.s125}
                    >
                      budget used
                    </span>
                  </div>
                </div>
                </>
              ) : null}
              <div
                className={s.s126}
              >
                <span>
                  Agent · mandate
                </span>
                <span
                  className={s.s127}
                >
                  Spent / budget
                </span>
                <span>
                  Runway
                </span>
                <span>
                  Status
                </span>
                <span>
                  Last active
                </span>
                <span />
              </div>
              {v.loading ? <SkelRows n={5} h={54} /> : null}
              {v.agentRows.map((a, i) => (
                <Fragment key={i}>
                <div
                  className={s.s128}
                  style={{ '--d0': a.opacity } as CSSVars}
                >
                  <div
                    className={s.s129}
                  >
                    <span
                      className={s.s130}
                      aria-hidden="true"
                      style={{ '--d0': a.avBg, '--d1': a.avPos, '--d2': a.avO } as CSSVars}
                    >
                      {a.ini}
                    </span>
                    <div
                      className={s.s131}
                    >
                      <span
                        className={s.s132}
                      >
                        {a.name}
                      </span>
                      <span
                        className={s.s133}
                      >
                        {a.id} · {a.purpose}
                      </span>
                    </div>
                  </div>
                  <div
                    className={s.s134}
                  >
                    <span
                      className={s.s135}
                    >
                      {a.spentF}
                    </span>
                    <span
                      className={s.s116}
                    >
                      / {a.budgetF}
                    </span>
                  </div>
                  <div
                    className={s.s136}
                  >
                    <span
                      className={s.s137}
                      style={{ '--d0': a.runColor } as CSSVars}
                    >
                      {a.runway}
                    </span>
                    <span
                      className={s.s138}
                    >
                      {a.leftF}
                    </span>
                  </div>
                  <span
                    className={a.statusLive ? s.statusPillLive : s.statusPill}
                    style={{ '--d0': a.statusInk } as CSSVars}
                  >
                    <span
                      className={s.statusDot}
                      style={{ '--d0': a.statusOrb } as CSSVars}
                    />
                    {a.statusLabel}
                  </span>
                  <span
                    className={s.s141}
                  >
                    {a.lastF}
                  </span>
                  <div className={s.rowActions}>
                    {a.keyKnown ? (
                      <button
                        className={s.s142}
                        onClick={a.copyKey}
                        title={`Copy API key ${a.keyMask}`}
                      >
                        Copy key
                      </button>
                    ) : null}
                    {a.canPause ? (
                      <button
                        className={s.s142}
                        onClick={a.togglePause}
                      >
                        {a.pauseLabel}
                      </button>
                    ) : null}
                    {a.canRevoke ? (
                      <button
                        className={s.s142}
                        onClick={a.revoke}
                      >
                        Revoke
                      </button>
                    ) : null}
                    {a.isRevoked ? (
                      <span
                        className={s.s143}
                      >
                        nullified
                      </span>
                    ) : null}
                  </div>
                </div>
                </Fragment>
              ))}
              {v.agentFillers.map((i) => (
                <div
                  key={`filler-${i}`}
                  className={s.rowFiller}
                  aria-hidden="true"
                >
                  <div
                    className={s.s129}
                  >
                    <span
                      className={s.s130}
                    />
                    <div
                      className={s.s131}
                    >
                      <span
                        className={s.s132}
                      >
                        &nbsp;
                      </span>
                      <span
                        className={s.s133}
                      >
                        &nbsp;
                      </span>
                    </div>
                  </div>
                  <div
                    className={s.s134}
                  >
                    <span
                      className={s.s135}
                    >
                      &nbsp;
                    </span>
                    <span
                      className={s.s116}
                    >
                      &nbsp;
                    </span>
                  </div>
                </div>
              ))}
              {v.agentNoMatch ? (
                <div
                  className={s.agentEmpty}
                >
                  No mandate matches that search.
                </div>
              ) : null}
              {v.agentPager ? (
                <div
                  className={s.pager}
                >
                  <button
                    className={s.pagerArrow}
                    onClick={v.agentPrev}
                    disabled={!v.canAgentPrev}
                    aria-label="Previous page"
                  >
                    ‹
                  </button>
                  <div
                    className={s.pagerPages}
                  >
                    {v.agentPageItems.map((p, i) => (
                      <Fragment key={i}>
                      {p.gap ? (
                        <span
                          className={s.pagerGap}
                        >
                          …
                        </span>
                      ) : (
                        <button
                          className={p.current ? s.pagerPageOn : s.pagerPage}
                          onClick={p.go}
                          aria-current={p.current}
                        >
                          {p.label}
                        </button>
                      )}
                      </Fragment>
                    ))}
                  </div>
                  <button
                    className={s.pagerArrow}
                    onClick={v.agentNext}
                    disabled={!v.canAgentNext}
                    aria-label="Next page"
                  >
                    ›
                  </button>
                  <span
                    className={s.pagerCount}
                  >
                    {v.agentRange}
                  </span>
                </div>
              ) : null}
              </>
            ) : null}
            {v.noAgents ? (
              <>
              <div
                className={s.s144}
              >
                <div
                  className={s.s145}
                >
                  <span
                    className={s.s146}
                  >
                    01
                  </span>
                  <span
                    className={s.s147}
                  >
                    Fund the pool
                  </span>
                  <span
                    className={s.s148}
                  >
                    Deposit USDC from your Hedera treasury. It becomes a shielded note only your organisation can see.
                  </span>
                  <button
                    className={s.s149}
                    onClick={v.openFund}
                  >
                    {v.step1Label}
                  </button>
                </div>
                <div
                  className={s.s150}
                  style={{ '--d0': v.step2Opacity } as CSSVars}
                >
                  <span
                    className={s.s151}
                  >
                    02
                  </span>
                  <span
                    className={s.s147}
                  >
                    Issue a mandate
                  </span>
                  <span
                    className={s.s148}
                  >
                    Give an agent a private, revocable allowance — budget, allowed endpoints, expiry.
                  </span>
                  <button
                    className={s.s152}
                    onClick={v.openMandate}
                  >
                    Issue mandate
                  </button>
                </div>
                <div
                  className={s.s153}
                >
                  <span
                    className={s.s151}
                  >
                    03
                  </span>
                  <span
                    className={s.s147}
                  >
                    Point the agent at x402
                  </span>
                  <div
                    className={s.s154}
                  >
                    VEIL_MANDATE=mnd_••••••
                    <br />
                    X402_ENDPOINT=inference.0g.ai
                  </div>
                </div>
              </div>
              </>
            ) : null}
          </section>
          <section
            className={s.s155}
          >
            <div
              className={s.s156}
            >
              <div
                className={s.s157}
              >
                <h2
                  className={s.s158}
                >
                  Live spend
                </h2>
                <span
                  className={s.s159}
                >
                  {v.livePulse}{v.liveLabel}
                </span>
              </div>
              <div
                className={s.s160}
              >
                <div
                  className={s.s161}
                >
                  <span
                    className={s.s162}
                  >
                    {v.loading ? <Skel w={132} h={62} r={5} /> : v.settledInt}
                  </span>
                  <span
                    className={s.s163}
                  >
                    {v.loading ? <Skel w={46} h={28} r={4} /> : <>.{v.settledDec}</>}
                  </span>
                </div>
                <div
                  className={s.s164}
                >
                  <span
                    className={s.s038}
                  >
                    USDC
                  </span>
                  <span
                    className={s.s056}
                  >
                    settled this session
                  </span>
                </div>
              </div>
              <div
                className={s.s165}
              >
                <div
                  className={s.s166}
                >
                  {v.tlCols.map((col, i) => (
                    <Fragment key={i}>
                    <div
                      className={s.s167}
                    >
                      {col.dots.map((d, i) => (
                        <Fragment key={i}>
                        <span
                          className={s.s168}
                          style={{ '--d0': d.bg, '--d1': `scale(${d.s})` } as CSSVars}
                        />
                        </Fragment>
                      ))}
                    </div>
                    </Fragment>
                  ))}
                </div>
                <div
                  className={s.s064}
                />
                <div
                  className={s.s169}
                >
                  <span>
                    −15m
                  </span>
                  <span>
                    −10m
                  </span>
                  <span>
                    −5m
                  </span>
                  <span>
                    now
                  </span>
                </div>
              </div>
              {v.hasFeed ? (
                <>
                <div
                  className={s.s170}
                >
                  {v.loading ? <SkelRows n={3} h={46} /> : null}
                  {v.feedRows.map((f, i) => (
                    <Fragment key={i}>
                    <div
                      className={s.s171}
                      onClick={f.select}
                      style={{ '--d0': f.bg } as CSSVars}
                    >
                      <span
                        className={s.s172}
                      >
                        {f.clock}
                      </span>
                      <div
                        className={s.s129}
                      >
                        <span
                          className={s.s173}
                          aria-hidden="true"
                          style={{ '--d0': f.avBg, '--d1': f.avPos, '--d2': f.avO } as CSSVars}
                        >
                          {f.ini}
                        </span>
                        <div
                          className={s.s136}
                        >
                          <span
                            className={s.s174}
                          >
                            {f.agent}
                          </span>
                          <span
                            className={s.s175}
                          >
                            → {f.vendor}
                          </span>
                        </div>
                      </div>
                      <div
                        className={s.s134}
                      >
                        <span
                          className={s.s176}
                          style={{ '--d0': f.amtColor } as CSSVars}
                        >
                          {f.amountF}
                        </span>
                        <span
                          className={s.s177}
                        >
                          <span
                            className={s.s178}
                            style={{ '--d0': f.amtColor } as CSSVars}
                          />
                          {f.kindLabel}
                        </span>
                      </div>
                    </div>
                    </Fragment>
                  ))}
                </div>
                </>
              ) : null}
              {v.noFeed ? (
                <>
                <div
                  className={s.s179}
                >
                  <span
                    className={s.s180}
                  >
                    Waiting for first payment{v.cursor}
                  </span>
                  <span
                    className={s.s181}
                  >
                    Spends appear the moment an agent settles an x402 session.
                  </span>
                </div>
                </>
              ) : null}
              {v.feedPager ? (
                <div
                  className={s.pager}
                >
                  <button
                    className={s.pagerArrow}
                    onClick={v.feedPrev}
                    disabled={!v.canFeedPrev}
                    aria-label="Previous page"
                  >
                    ‹
                  </button>
                  <div
                    className={s.pagerPages}
                  >
                    {v.feedPageItems.map((p, i) => (
                      <Fragment key={i}>
                      {p.gap ? (
                        <span
                          className={s.pagerGap}
                        >
                          …
                        </span>
                      ) : (
                        <button
                          className={p.current ? s.pagerPageOn : s.pagerPage}
                          onClick={p.go}
                          aria-current={p.current}
                        >
                          {p.label}
                        </button>
                      )}
                      </Fragment>
                    ))}
                  </div>
                  <button
                    className={s.pagerArrow}
                    onClick={v.feedNext}
                    disabled={!v.canFeedNext}
                    aria-label="Next page"
                  >
                    ›
                  </button>
                  <span
                    className={s.pagerCount}
                  >
                    {v.feedRange}
                  </span>
                </div>
              ) : null}
            </div>
            <div
              className={s.s182}
            >
              <div
                className={s.s183}
              >
                <h2
                  className={s.s027}
                >
                  Escrowed sessions
                </h2>
                <div
                  className={s.s028}
                >
                  <span
                    className={s.s029}
                  >
                    <span
                      className={s.s030}
                    >
                      x402
                    </span>
                    <span
                      className={s.s031}
                    >
                      PROTOCOL
                    </span>
                  </span>
                  <span
                    className={s.s029}
                  >
                    <span
                      className={s.s032}
                    >
                      TEE
                    </span>
                    <span
                      className={s.s031}
                    >
                      METERED IN
                    </span>
                  </span>
                </div>
              </div>
              <div
                className={s.s160}
              >
                <div
                  className={s.s161}
                >
                  <span
                    className={s.s162}
                  >
                    {v.loading ? <Skel w={132} h={62} r={5} /> : v.escInt}
                  </span>
                  <span
                    className={s.s163}
                  >
                    {v.loading ? <Skel w={46} h={28} r={4} /> : <>.{v.escDec}</>}
                  </span>
                </div>
                <div
                  className={s.s164}
                >
                  <span
                    className={s.s038}
                  >
                    USDC
                  </span>
                  <span
                    className={s.s184}
                  >
                    locked in {v.loading ? <Skel w={86} h={10} /> : `${v.sessionCount} ${v.sessionWord}`}
                  </span>
                </div>
              </div>
              <div
                className={s.s185}
              >
                {v.stages.map((st, i) => (
                  <Fragment key={i}>
                  <div
                    className={s.s186}
                    onMouseEnter={st.enter}
                    onMouseLeave={v.stOff}
                  >
                    <span
                      className={s.s187}
                      aria-hidden="true"
                      style={{ '--d0': st.clip } as CSSVars}
                    />
                    <span
                      className={s.s188}
                      aria-hidden="true"
                      style={{ '--d0': st.prog } as CSSVars}
                    />
                    <span
                      className={s.s189}
                      style={{ '--d0': st.nc } as CSSVars}
                    >
                      <span>
                        {st.n}
                      </span>
                      <span
                        className={s.s190}
                        style={{ '--d0': st.arO } as CSSVars}
                      >
                        →
                      </span>
                    </span>
                    <div
                      className={s.s191}
                    >
                      <span
                        className={s.s192}
                        style={{ '--d0': st.vc } as CSSVars}
                      >
                        {v.loading ? <Skel w={54} h={22} r={4} /> : st.value}
                      </span>
                      <span
                        className={s.s193}
                        style={{ '--d0': st.lc } as CSSVars}
                      >
                        {st.label}
                      </span>
                    </div>
                  </div>
                  </Fragment>
                ))}
              </div>
              {v.hasSessions ? (
                <>
                <div
                  className={s.s170}
                >
                  {v.loading ? <SkelRows n={3} h={46} /> : null}
                  {v.sessionRows.map((se, i) => (
                    <Fragment key={i}>
                    <div
                      className={s.s194}
                    >
                      <div
                        className={s.s195}
                      >
                        <div
                          className={s.s129}
                        >
                          <span
                            className={s.s196}
                            aria-hidden="true"
                            style={{ '--d0': se.avBg, '--d1': se.avPos, '--d2': se.avO } as CSSVars}
                          >
                            {se.ini}
                          </span>
                          <div
                            className={s.s136}
                          >
                            <span
                              className={s.s197}
                            >
                              {se.agent}
                            </span>
                            <span
                              className={s.s175}
                            >
                              → {se.vendor}
                            </span>
                          </div>
                        </div>
                        <span
                          className={s.s198}
                        >
                          {se.meteredF}{' '}
                          <span
                            className={s.s199}
                          >
                            / {se.escrowF}
                          </span>
                        </span>
                      </div>
                    </div>
                    </Fragment>
                  ))}
                </div>
                </>
              ) : null}
              {v.noSessions ? (
                <>
                <div
                  className={s.s206}
                >
                  <div
                    className={s.s207}
                  />
                  <span
                    className={s.s181}
                  >
                    No open sessions.
                  </span>
                </div>
                </>
              ) : null}
              {v.sessionPager ? (
                <div
                  className={s.pager}
                >
                  <button
                    className={s.pagerArrow}
                    onClick={v.sessionPrev}
                    disabled={!v.canSessionPrev}
                    aria-label="Previous page"
                  >
                    ‹
                  </button>
                  <div
                    className={s.pagerPages}
                  >
                    {v.sessionPageItems.map((p, i) => (
                      <Fragment key={i}>
                      {p.gap ? (
                        <span
                          className={s.pagerGap}
                        >
                          …
                        </span>
                      ) : (
                        <button
                          className={p.current ? s.pagerPageOn : s.pagerPage}
                          onClick={p.go}
                          aria-current={p.current}
                        >
                          {p.label}
                        </button>
                      )}
                      </Fragment>
                    ))}
                  </div>
                  <button
                    className={s.pagerArrow}
                    onClick={v.sessionNext}
                    disabled={!v.canSessionNext}
                    aria-label="Next page"
                  >
                    ›
                  </button>
                  <span
                    className={s.pagerCount}
                  >
                    {v.sessionRange}
                  </span>
                </div>
              ) : null}
            </div>
          </section>
          <section
            className={s.s208}
          >
            <div
              className={s.s209}
              aria-hidden="true"
            />
            <div
              className={s.s210}
              aria-hidden="true"
            />
            <div
              className={s.s211}
            >
              <div
                className={s.s212}
              >
                <span
                  className={s.s213}
                >
                  AUDIT · SELECTIVE DISCLOSURE
                </span>
                <h2
                  className={s.s214}
                >
                  Prove exactly what's asked. Nothing more.
                </h2>
                <span
                  className={s.s215}
                >
                  Hand an auditor a scoped viewing proof — one payment, one agent's history, or one period. They verify it against Hedera; the rest of the ledger stays sealed.
                </span>
                <div
                  className={s.s216}
                >
                  <span>
                    POOL CONTRACT 0.0.4821930 · GROTH16
                  </span>
                  <span>
                    MERKLE ROOT {v.root}
                  </span>
                </div>
              </div>
              <div
                className={s.s217}
              >
                <div
                  className={s.s218}
                >
                  <span
                    className={s.s213}
                  >
                    TWO VIEWS OF ONE PAYMENT
                  </span>
                  <span
                    className={s.s213}
                  >
                    SELECT IN FEED
                  </span>
                </div>
                <div
                  className={s.s219}
                >
                  <div
                    className={s.s220}
                  >
                    <span
                      className={s.s213}
                    >
                      HEDERA SEES
                    </span>
                    {v.chainMini.map((r, i) => (
                      <Fragment key={i}>
                      <div
                        className={s.s221}
                      >
                        <span
                          className={s.s222}
                        >
                          {r.k}
                        </span>
                        <span
                          className={s.s223}
                        >
                          {v.loading ? <Skel w={92} h={10} /> : r.v}
                        </span>
                      </div>
                      </Fragment>
                    ))}
                  </div>
                  <div
                    className={s.s224}
                  >
                    <span
                      className={s.s225}
                    >
                      YOU SEE
                    </span>
                    <span
                      className={s.s226}
                    >
                      {v.selV.agent}
                    </span>
                    <span
                      className={s.s175}
                    >
                      → {v.loading ? <Skel w={88} h={10} /> : v.selV.vendor}
                    </span>
                    <span
                      className={s.s227}
                    >
                      {v.loading ? <Skel w={70} h={20} r={4} /> : v.selV.amountF}
                    </span>
                    <span
                      className={s.s228}
                    >
                      {v.selV.meta}
                    </span>
                  </div>
                  {v.hasSel ? (
                    <>
                    <span
                      className={s.s229}
                    >
                      PROOF ✓
                    </span>
                    </>
                  ) : null}
                </div>
              </div>
            </div>
            <div
              className={s.s230}
            >
              <div
                className={s.s231}
              >
                {v.scopeCards.map((sc, i) => (
                  <Fragment key={i}>
                  <button
                    className={s.s232}
                    onClick={sc.pick}
                    style={{ '--d0': sc.op } as CSSVars}
                  >
                    <span
                      className={s.s233}
                    >
                      {sc.num}
                    </span>
                    <span
                      className={s.s234}
                      style={{ '--d0': sc.bar } as CSSVars}
                    />
                    <span
                      className={s.s147}
                    >
                      {sc.label}
                    </span>
                    <span
                      className={s.s235}
                    >
                      {sc.desc}
                    </span>
                  </button>
                  </Fragment>
                ))}
              </div>
              <div
                className={s.s236}
              >
                {v.scopeAgent ? (
                  <>
                  <select
                    className={s.s237}
                    value={v.auditAgent}
                    onChange={v.pickAuditAgent}
                  >
                    {v.agentOptions.map((o, i) => (
                      <Fragment key={i}>
                      <option
                        className={s.s238}
                        value={o.id}
                      >
                        {o.name} · {o.id}
                      </option>
                      </Fragment>
                    ))}
                  </select>
                  </>
                ) : null}
                <div
                  className={s.s026}
                >
                  <span
                    className={s.s213}
                  >
                    IN SCOPE
                  </span>
                  <span
                    className={s.s239}
                  >
                    {v.loading ? <Skel w={200} h={11} /> : v.scopeIn}
                  </span>
                  <span
                    className={s.s240}
                  >
                    <span
                      className={s.s213}
                    >
                      STAYS SEALED
                    </span>
                  </span>
                  <span
                    className={s.s239}
                  >
                    {v.loading ? <Skel w={220} h={11} /> : v.scopeOut}
                  </span>
                </div>
                <button
                  className={s.s241}
                  onClick={v.genProof}
                  style={{ '--d0': v.proofBtnOpacity } as CSSVars}
                >
                  <span>
                    {v.proofBtnLabel}
                  </span>
                  <span>
                    ↗
                  </span>
                </button>
                {v.hasProofs ? (
                  <>
                  <div
                    className={s.s242}
                  >
                    {v.loading ? <SkelRows n={2} h={58} /> : null}
                    {v.proofRows.map((p, i) => (
                      <Fragment key={i}>
                      <div
                        className={s.s243}
                      >
                        <div
                          className={s.s136}
                        >
                          <span
                            className={s.s244}
                          >
                            {p.label}
                          </span>
                          <span
                            className={s.s245}
                          >
                            {p.id} · {p.rel}
                          </span>
                        </div>
                        <span
                          className={s.s246}
                        >
                          COPY LINK
                        </span>
                      </div>
                      </Fragment>
                    ))}
                  </div>
                  </>
                ) : null}
              </div>
            </div>
          </section>
        </main>
      </div>
      {v.fundOpen ? (
        <>
        <div
          className={s.s247}
          onClick={v.closeModals}
        >
          <div
            className={s.s248}
            onClick={v.stop}
          >
            <div
              className={s.s157}
            >
              <span
                className={s.s249}
              >
                Fund shielded pool
              </span>
              <button
                className={s.s250}
                onClick={v.closeModals}
              >
                ×
              </button>
            </div>
            <div
              className={s.s251}
            >
              <input
                className={s.s252}
                value={v.fundAmount}
                onChange={v.setFundAmount}
                inputMode="decimal"
              />
              <span
                className={s.s197}
              >
                USDC
              </span>
            </div>
            <div
              className={s.s253}
            >
              {v.fundPresets.map((fp, i) => (
                <Fragment key={i}>
                <button
                  className={s.s254}
                  onClick={fp.pick}
                >
                  {fp.label}
                </button>
                </Fragment>
              ))}
            </div>
            <div
              className={s.s255}
            >
              <div
                className={s.s256}
              >
                <span
                  className={s.s257}
                >
                  From
                </span>
                <span
                  className={s.s258}
                >
                  Treasury 0.0.1029384
                </span>
              </div>
              <div
                className={s.s256}
              >
                <span
                  className={s.s257}
                >
                  Creates
                </span>
                <span>
                  1 shielded note
                </span>
              </div>
              <div
                className={s.s256}
              >
                <span
                  className={s.s257}
                >
                  Visible on-chain
                </span>
                <span>
                  deposit only — later spends unlinkable
                </span>
              </div>
            </div>
            {v.depositing ? (
              <div className={s.steps}>
                <div className={s.track} style={{ '--d0': v.depositFill } as CSSVars}>
                  <span className={s.trackFill} />
                  {v.depositSteps.map((st, i) => (
                    <span
                      key={i}
                      className={s.trackDot}
                      data-run={st.running ? 'true' : undefined}
                      style={
                        {
                          '--d0': st.left,
                          '--d1': st.dotBg,
                          '--d2': st.dotFg,
                          '--d3': st.ring,
                          '--d4': st.op,
                        } as CSSVars
                      }
                    >
                      {st.running ? (
                        <span className={s.trackSpin} />
                      ) : st.done ? (
                        <svg className={s.trackTick} viewBox="0 0 16 16" aria-hidden="true">
                          <path
                            d="M3.6 8.5 6.6 11.5 12.4 5.2"
                            fill="none"
                            stroke="currentColor"
                            strokeWidth="2.1"
                            strokeLinecap="round"
                            strokeLinejoin="round"
                          />
                        </svg>
                      ) : (
                        st.mark
                      )}
                    </span>
                  ))}
                </div>
                <div className={s.trackLabels}>
                  {v.depositSteps.map((st, i) => (
                    <span key={i} className={s.trackLabel} style={{ '--d0': st.op } as CSSVars}>
                      <span className={s.trackNum} style={{ '--d1': st.numFg } as CSSVars}>
                        {st.num}
                      </span>
                      <span className={s.trackName} style={{ '--d1': st.labelFg } as CSSVars}>
                        {st.label}
                      </span>
                    </span>
                  ))}
                </div>
                {v.depositFailed ? (
                  <span className={s.stepFailed}>{v.depositError}</span>
                ) : null}
                {v.depositDone || v.depositFailed ? (
                  <button className={s.s259} onClick={v.closeDeposit}>
                    {v.doneBtnLabel}
                  </button>
                ) : null}
              </div>
            ) : (
              <button
                className={s.s259}
                onClick={v.confirmFund}
              >
                {v.fundBtnLabel}
              </button>
            )}
          </div>
        </div>
        </>
      ) : null}
      {v.mandateOpen ? (
        <>
        <div
          className={s.s247}
          onClick={v.closeModals}
        >
          <div
            className={s.s260}
            onClick={v.stop}
          >
            <div
              className={s.s157}
            >
              <span
                className={s.s249}
              >
                Issue mandate
              </span>
              <button
                className={s.s250}
                onClick={v.closeModals}
              >
                ×
              </button>
            </div>
            {!v.hasIssued ? (<>
            <label
              className={s.s261}
            >
              Agent name
              <input
                className={s.s262}
                value={v.mName}
                onChange={v.setMName}
              />
            </label>
            <label
              className={s.s261}
            >
              Budget (USDC)
              <input
                className={s.s262}
                value={v.mBudget}
                onChange={v.setMBudget}
                inputMode="decimal"
              />
            </label>
            <div className={s.s253}>
              {v.budgetPresets.map((bp, i) => (
                <button key={i} className={s.s254} onClick={bp.pick}>
                  {bp.label}
                </button>
              ))}
            </div>
            <div
              className={s.s263}
            >
              Allowed endpoints
              <div
                className={s.s264}
              >
                {v.endpointChips.map((ep, i) => (
                  <Fragment key={i}>
                  <button
                    className={s.s265}
                    onClick={ep.toggle}
                    style={{ '--d0': `1px solid ${ep.border}`, '--d1': ep.bg, '--d2': ep.fg } as CSSVars}
                  >
                    {ep.label}
                  </button>
                  </Fragment>
                ))}
              </div>
            </div>
            <div
              className={s.s266}
            >
              <span
                className={s.s257}
              >
                Unallocated in pool
              </span>
              <span
                className={s.s043}
              >
                {v.freeF}
              </span>
            </div>
            </>) : null}
            {v.mError ? (
              <>
              <span
                className={s.s267}
              >
                {v.mError}
              </span>
              </>
            ) : null}
            {v.hasIssued ? (
              <div className={s.keyPanel}>
                <div className={s.keyPanelMark}>
                  <span className={s.keyPanelDot}>
                    <svg className={s.trackTick} viewBox="0 0 16 16" aria-hidden="true">
                      <path
                        d="M3.6 8.5 6.6 11.5 12.4 5.2"
                        fill="none"
                        stroke="currentColor"
                        strokeWidth="2.1"
                        strokeLinecap="round"
                        strokeLinejoin="round"
                      />
                    </svg>
                  </span>
                  <span className={s.keyPanelText}>
                    <span className={s.keyPanelWho}>{v.issuedFor}</span>
                    <span className={s.keyPanelId}>{v.issuedId}</span>
                  </span>
                </div>
                <div className={s.keyReveal}>
                  <code className={s.keyRevealValue}>
                    {v.keyShown ? v.issuedKey : v.keyMasked}
                  </code>
                  <button type="button" className={s.keyToggle} onClick={v.toggleKey}>
                    {v.showBtnLabel}
                  </button>
                </div>
                <span className={s.keyPanelNote}>
                  Kept as a hash. Nothing can show it again.
                </span>
                <div className={s.keyPanelRow}>
                  <button className={s.s259} onClick={v.copyIssued}>
                    {v.copyBtnLabel}
                  </button>
                  <button className={s.keyPanelDone} onClick={v.doneIssued}>
                    Done
                  </button>
                </div>
              </div>
            ) : (
              <button
                className={s.s259}
                onClick={v.confirmMandate}
              >
                Sign & issue mandate
              </button>
            )}
          </div>
        </div>
        </>
      ) : null}
      {v.toast ? (
        <>
        <div
          className={s.s268}
        >
          <span
            className={s.s269}
            style={{ '--d0': v.toast.color } as CSSVars}
          />
          <div
            className={s.s270}
          >
            <span className={s.toastTitle}>{v.toast.title}</span>
            <span className={s.toastSub}>{v.toast.sub}</span>
          </div>
        </div>
        </>
      ) : null}
    </div>
    </>
  )
}
