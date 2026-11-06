import { Fragment } from 'react'
import type { CSSVars } from '../../styles/css-vars'
import type { ConsoleBindings } from './useConsoleBindings'
import s from './console.module.css'


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
          <button
            className={s.s009}
          >
            <span
              className={s.s010}
            >
              HW
            </span>
            <span
              className={s.s011}
            >
              <span
                className={s.s012}
              >
                Hashira Works
              </span>
              <span
                className={s.s013}
              >
                TREASURY ADMIN
              </span>
            </span>
          </button>
        </div>
      </header>
      <div
        className={s.s014}
      >
        <main
          className={s.s015}
        >
          <div
            className={s.s016}
          >
            <div
              className={s.s017}
            >
              <span
                className={s.s018}
              >
                PREVIEW
              </span>
              <div
                className={s.s019}
              >
                <button
                  className={s.s020}
                  onClick={v.showDemo}
                  style={{ '--d0': v.demoBg, '--d1': v.demoFg } as CSSVars}
                >
                  Operating
                </button>
                <button
                  className={s.s021}
                  onClick={v.showFresh}
                  style={{ '--d0': v.freshBg, '--d1': v.freshFg } as CSSVars}
                >
                  Fresh install
                </button>
              </div>
            </div>
          </div>
          <section
            className={s.s022}
          >
            <div
              className={s.s023}
            >
              <div
                className={s.s024}
              >
                <div
                  className={s.s025}
                >
                  <h1
                    className={s.s026}
                  >
                    Shielded pool
                  </h1>
                </div>
                <div
                  className={s.s027}
                >
                  <span
                    className={s.s028}
                  >
                    <span
                      className={s.s029}
                    >
                      {v.noteCount}
                    </span>
                    <span
                      className={s.s030}
                    >
                      NOTES
                    </span>
                  </span>
                  <span
                    className={s.s028}
                  >
                    <span
                      className={s.s031}
                    >
                      {v.anonSet}
                    </span>
                    <span
                      className={s.s030}
                    >
                      ANONYMITY SET
                    </span>
                  </span>
                </div>
              </div>
              <div
                className={s.s032}
              >
                <div
                  className={s.s033}
                  onMouseEnter={v.balOn}
                  onMouseLeave={v.balOff}
                >
                  <span
                    className={s.s034}
                    style={{ '--d0': `${v.balG},100% 100%` } as CSSVars}
                  >
                    {v.balInt}
                  </span>
                  <span
                    className={s.s035}
                    style={{ '--d0': `${v.balG},100% 100%` } as CSSVars}
                  >
                    .{v.balDec}
                  </span>
                </div>
                <div
                  className={s.s036}
                >
                  <span
                    className={s.s037}
                  >
                    USDC
                  </span>
                  <span
                    className={s.s038}
                  >
                    {v.lastDeposit}
                  </span>
                </div>
              </div>
              <p
                className={s.s039}
              >
                Balance is held as encrypted notes on Hedera. Only this organisation's viewing key can sum them — the chain sees commitments, not amounts.
              </p>
              <div
                className={s.s040}
              >
                <button
                  className={s.s041}
                  onClick={v.openFund}
                >
                  Fund pool{' '}
                  <span
                    className={s.s042}
                  >
                    +
                  </span>
                </button>
                <button
                  className={s.s043}
                  onClick={v.openMandate}
                >
                  Issue mandate
                </button>
                <button
                  className={s.s043}
                >
                  Withdraw
                </button>
              </div>
            </div>
            <div
              className={s.s044}
            >
              {v.stats.map((st, i) => (
                <Fragment key={i}>
                <div
                  className={s.s045}
                  onMouseEnter={st.enter}
                  onMouseLeave={v.statOff}
                >
                  <span
                    className={s.s046}
                    aria-hidden="true"
                    style={{ '--d0': st.clip } as CSSVars}
                  />
                  <span
                    className={s.s047}
                    aria-hidden="true"
                    style={{ '--d0': st.bar } as CSSVars}
                  />
                  <div
                    className={s.s048}
                    style={{ '--d0': st.labC } as CSSVars}
                  >
                    <span>
                      {st.label}
                    </span>
                    <span
                      className={s.s049}
                      style={{ '--d0': st.arO, '--d1': st.arT } as CSSVars}
                    >
                      ↗
                    </span>
                  </div>
                  <div
                    className={s.s050}
                    style={{ '--d0': st.valC } as CSSVars}
                  >
                    {st.value}
                  </div>
                  <div
                    className={s.s051}
                    style={{ '--d0': st.subC } as CSSVars}
                  >
                    {st.sub}
                  </div>
                </div>
                </Fragment>
              ))}
            </div>
          </section>
          <section
            className={s.s052}
          >
            <div
              className={s.s053}
            >
              <span
                className={s.s054}
              >
                Each dot is {v.dotUnit} of funded capital — where every unit currently sits.
              </span>
              <span
                className={s.s055}
              >
                total funded {v.totalFunded}
              </span>
            </div>
            <div
              className={s.s056}
            >
              {v.segs.map((sg, i) => (
                <Fragment key={i}>
                <div
                  className={s.s057}
                  onMouseEnter={sg.enter}
                  onMouseLeave={v.dotOff}
                  style={{ '--d0': sg.left, '--d1': sg.display, '--d2': sg.op } as CSSVars}
                >
                  <span
                    className={s.s058}
                    style={{ '--d0': sg.sq } as CSSVars}
                  />
                  {sg.label}{' '}
                  <span
                    className={s.s059}
                    style={{ '--d0': sg.pc } as CSSVars}
                  >
                    {sg.pct}
                  </span>
                </div>
                </Fragment>
              ))}
            </div>
            <div
              className={s.s060}
            >
              <div
                className={s.s061}
                onMouseLeave={v.dotOff}
              >
                {v.dots.map((d, i) => (
                  <Fragment key={i}>
                  <span
                    className={s.s062}
                    onMouseEnter={d.enter}
                    style={{ '--d0': d.bg, '--d1': d.op, '--d2': d.tf, '--d3': d.tr } as CSSVars}
                  />
                  </Fragment>
                ))}
              </div>
            </div>
            <div
              className={s.s063}
            />
            <div
              className={s.s064}
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
            className={s.s065}
          >
            <div
              className={s.s066}
            >
              <div
                className={s.s067}
              >
                <div
                  className={s.s068}
                >
                  <h2
                    className={s.s026}
                  >
                    Agents
                  </h2>
                  <div
                    className={s.s027}
                  >
                    <span
                      className={s.s028}
                    >
                      <span
                        className={s.s029}
                      >
                        {v.agentN}
                      </span>
                      <span
                        className={s.s030}
                      >
                        MANDATES
                      </span>
                    </span>
                    <span
                      className={s.s028}
                    >
                      <span
                        className={s.s031}
                      >
                        {v.agentA}
                      </span>
                      <span
                        className={s.s030}
                      >
                        ACTIVE
                      </span>
                    </span>
                  </div>
                </div>
                <span
                  className={s.s069}
                >
                  Each agent spends from a private mandate.
                </span>
              </div>
              <div
                className={s.s070}
              >
                {v.chartColumns ? (
                  <>
                  <div
                    className={s.s071}
                  >
                    <span
                      className={s.s072}
                    >
                      <span
                        className={s.s073}
                      />
                      Spent
                    </span>
                    <span
                      className={s.s072}
                    >
                      <span
                        className={s.s073}
                      />
                      In escrow
                    </span>
                    <span
                      className={s.s072}
                    >
                      <span
                        className={s.s074}
                      />
                      Remaining
                    </span>
                  </div>
                  </>
                ) : null}
                <button
                  className={s.s043}
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
                  className={s.s075}
                >
                  <div
                    className={s.s076}
                  >
                    <span>
                      Each column is one mandate. The white line marks spend today; the column rises to projected spend at period end — 8 days left.
                    </span>
                    <div
                      className={s.s077}
                    >
                      <span
                        className={s.s078}
                      >
                        <span
                          className={s.s079}
                        />
                        SPENT TODAY
                      </span>
                      <span
                        className={s.s078}
                      >
                        <span
                          className={s.s080}
                        />
                        PROJECTED
                      </span>
                      <span
                        className={s.s078}
                      >
                        <span
                          className={s.s081}
                        />
                        OVER LIMIT
                      </span>
                    </div>
                  </div>
                  <div
                    className={s.s082}
                    onMouseLeave={v.rangeOff}
                  >
                    <span
                      className={s.s083}
                      aria-hidden="true"
                    />
                    <span
                      className={s.s084}
                    >
                      <span>
                        LIMIT
                      </span>
                      <span
                        className={s.s085}
                      >
                        100%
                      </span>
                    </span>
                    <span
                      className={s.s086}
                    >
                      125%
                    </span>
                    <span
                      className={s.s087}
                    >
                      0
                    </span>
                    <div
                      className={s.s088}
                    >
                      <div
                        className={s.s089}
                      >
                        {v.rangeCols.map((r, i) => (
                          <Fragment key={i}>
                          <div
                            className={s.s090}
                            onMouseEnter={r.enter}
                          >
                            <div
                              className={s.s091}
                              style={{ '--d0': r.barH, '--d1': r.grow, '--d2': `transform 1s cubic-bezier(.2,.8,.2,1) ${r.dl}` } as CSSVars}
                            >
                              <span
                                className={s.s092}
                                aria-hidden="true"
                                style={{ '--d0': r.barBg, '--d1': r.barO } as CSSVars}
                              />
                              <span
                                className={s.s093}
                                aria-hidden="true"
                                style={{ '--d0': r.todayB } as CSSVars}
                              />
                            </div>
                          </div>
                          </Fragment>
                        ))}
                      </div>
                      <div
                        className={s.s094}
                      >
                        {v.rangeCols.map((r, i) => (
                          <Fragment key={i}>
                          <div
                            className={s.s095}
                            style={{ '--d0': r.cO, '--d1': r.cT, '--d2': `opacity .6s ease ${r.dl2},transform .7s cubic-bezier(.2,.8,.2,1) ${r.dl2}` } as CSSVars}
                          >
                            <div
                              className={s.s096}
                              onMouseEnter={r.enter}
                              style={{ '--d0': r.cFill } as CSSVars}
                            >
                              <span
                                className={s.s097}
                                aria-hidden="true"
                                style={{ '--d0': r.avBg, '--d1': r.avPos } as CSSVars}
                              >
                                {r.ini}
                              </span>
                              <span
                                className={s.s098}
                              >
                                <span
                                  className={s.s099}
                                  style={{ '--d0': r.cTx } as CSSVars}
                                >
                                  {r.name}
                                </span>
                                <span
                                  className={s.s100}
                                  style={{ '--d0': r.cPc } as CSSVars}
                                >
                                  {r.projF}
                                </span>
                              </span>
                            </div>
                            <span
                              className={s.s101}
                              aria-hidden="true"
                              style={{ '--d0': r.conH } as CSSVars}
                            >
                              <span
                                className={s.s102}
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
                  className={s.s103}
                >
                  <div
                    className={s.s104}
                  >
                    {v.agentCols.map((c, i) => (
                      <Fragment key={i}>
                      <div
                        className={s.s105}
                        style={{ '--d0': c.op } as CSSVars}
                      >
                        <div
                          className={s.s106}
                        >
                          <span
                            className={s.s107}
                          />
                          {c.ghosts.map((g, i) => (
                            <Fragment key={i}>
                            <span
                              className={s.s108}
                              style={{ '--d0': g.b } as CSSVars}
                            />
                            </Fragment>
                          ))}
                          <span
                            className={s.s109}
                            style={{ '--d0': c.spentH, '--d1': c.grad } as CSSVars}
                          />
                          <span
                            className={s.s110}
                            style={{ '--d0': `calc(${c.spentH} + 10px)`, '--d1': c.escH } as CSSVars}
                          />
                          <span
                            className={s.s111}
                            style={{ '--d0': c.spentH, '--d1': c.head, '--d2': c.headBorder } as CSSVars}
                          />
                        </div>
                        <span
                          className={s.s112}
                          style={{ '--d0': c.sBg, '--d1': c.sBorder } as CSSVars}
                        />
                        <div
                          className={s.s113}
                        >
                          <span
                            className={s.s114}
                          >
                            {c.name}
                          </span>
                          <span
                            className={s.s115}
                          >
                            {c.pctF}
                          </span>
                        </div>
                      </div>
                      </Fragment>
                    ))}
                  </div>
                  <div
                    className={s.s116}
                  >
                    <div
                      className={s.s117}
                    >
                      <span
                        className={s.s118}
                      >
                        100%
                      </span>
                      <span
                        className={s.s119}
                      >
                        75%
                      </span>
                      <span
                        className={s.s120}
                      >
                        50%
                      </span>
                      <span
                        className={s.s121}
                      >
                        25%
                      </span>
                    </div>
                    <span
                      className={s.s122}
                    >
                      {v.spentPctAll}
                      <span
                        className={s.s123}
                      >
                        %
                      </span>
                    </span>
                    <span
                      className={s.s124}
                    >
                      budget used
                    </span>
                  </div>
                </div>
                </>
              ) : null}
              <div
                className={s.s125}
              >
                <span>
                  Agent · mandate
                </span>
                <span
                  className={s.s126}
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
              {v.agentRows.map((a, i) => (
                <Fragment key={i}>
                <div
                  className={s.s127}
                  style={{ '--d0': a.opacity } as CSSVars}
                >
                  <div
                    className={s.s128}
                  >
                    <span
                      className={s.s129}
                      aria-hidden="true"
                      style={{ '--d0': a.avBg, '--d1': a.avPos, '--d2': a.avO } as CSSVars}
                    >
                      {a.ini}
                    </span>
                    <div
                      className={s.s130}
                    >
                      <span
                        className={s.s131}
                      >
                        {a.name}
                      </span>
                      <span
                        className={s.s132}
                      >
                        {a.id} · {a.purpose}
                      </span>
                    </div>
                  </div>
                  <div
                    className={s.s133}
                  >
                    <span
                      className={s.s134}
                    >
                      {a.spentF}
                    </span>
                    <span
                      className={s.s115}
                    >
                      / {a.budgetF}
                    </span>
                  </div>
                  <div
                    className={s.s135}
                  >
                    <span
                      className={s.s136}
                      style={{ '--d0': a.runColor } as CSSVars}
                    >
                      {a.runway}
                    </span>
                    <span
                      className={s.s137}
                    >
                      {a.leftF}
                    </span>
                  </div>
                  <span
                    className={s.s138}
                    style={{ '--d0': a.statusColor } as CSSVars}
                  >
                    <span
                      className={s.s139}
                      style={{ '--d0': a.statusColor } as CSSVars}
                    />
                    {a.statusLabel}
                  </span>
                  <span
                    className={s.s140}
                  >
                    {a.lastF}
                  </span>
                  {a.canRevoke ? (
                    <>
                    <button
                      className={s.s141}
                      onClick={a.revoke}
                    >
                      Revoke
                    </button>
                    </>
                  ) : null}
                  {a.isRevoked ? (
                    <>
                    <span
                      className={s.s142}
                    >
                      nullified
                    </span>
                    </>
                  ) : null}
                </div>
                </Fragment>
              ))}
              </>
            ) : null}
            {v.noAgents ? (
              <>
              <div
                className={s.s143}
              >
                <div
                  className={s.s144}
                >
                  <span
                    className={s.s145}
                  >
                    01
                  </span>
                  <span
                    className={s.s146}
                  >
                    Fund the pool
                  </span>
                  <span
                    className={s.s147}
                  >
                    Deposit USDC from your Hedera treasury. It becomes a shielded note only your organisation can see.
                  </span>
                  <button
                    className={s.s148}
                    onClick={v.openFund}
                  >
                    {v.step1Label}
                  </button>
                </div>
                <div
                  className={s.s149}
                  style={{ '--d0': v.step2Opacity } as CSSVars}
                >
                  <span
                    className={s.s150}
                  >
                    02
                  </span>
                  <span
                    className={s.s146}
                  >
                    Issue a mandate
                  </span>
                  <span
                    className={s.s147}
                  >
                    Give an agent a private, revocable allowance — budget, allowed endpoints, expiry.
                  </span>
                  <button
                    className={s.s151}
                    onClick={v.openMandate}
                  >
                    Issue mandate
                  </button>
                </div>
                <div
                  className={s.s152}
                >
                  <span
                    className={s.s150}
                  >
                    03
                  </span>
                  <span
                    className={s.s146}
                  >
                    Point the agent at x402
                  </span>
                  <div
                    className={s.s153}
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
            className={s.s154}
          >
            <div
              className={s.s155}
            >
              <div
                className={s.s156}
              >
                <h2
                  className={s.s157}
                >
                  Live spend
                </h2>
                <span
                  className={s.s158}
                >
                  {v.livePulse}{v.liveLabel}
                </span>
              </div>
              <div
                className={s.s159}
              >
                <div
                  className={s.s160}
                >
                  <span
                    className={s.s161}
                  >
                    {v.settledInt}
                  </span>
                  <span
                    className={s.s162}
                  >
                    .{v.settledDec}
                  </span>
                </div>
                <div
                  className={s.s163}
                >
                  <span
                    className={s.s037}
                  >
                    USDC
                  </span>
                  <span
                    className={s.s055}
                  >
                    settled this session
                  </span>
                </div>
              </div>
              <div
                className={s.s164}
              >
                <div
                  className={s.s165}
                >
                  {v.tlCols.map((col, i) => (
                    <Fragment key={i}>
                    <div
                      className={s.s166}
                    >
                      {col.dots.map((d, i) => (
                        <Fragment key={i}>
                        <span
                          className={s.s167}
                          style={{ '--d0': d.bg, '--d1': `scale(${d.s})` } as CSSVars}
                        />
                        </Fragment>
                      ))}
                    </div>
                    </Fragment>
                  ))}
                </div>
                <div
                  className={s.s063}
                />
                <div
                  className={s.s168}
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
                  className={s.s169}
                >
                  {v.feedRows.map((f, i) => (
                    <Fragment key={i}>
                    <div
                      className={s.s170}
                      onClick={f.select}
                      style={{ '--d0': f.bg } as CSSVars}
                    >
                      <span
                        className={s.s171}
                      >
                        {f.clock}
                      </span>
                      <div
                        className={s.s128}
                      >
                        <span
                          className={s.s172}
                          aria-hidden="true"
                          style={{ '--d0': f.avBg, '--d1': f.avPos, '--d2': f.avO } as CSSVars}
                        >
                          {f.ini}
                        </span>
                        <div
                          className={s.s135}
                        >
                          <span
                            className={s.s173}
                          >
                            {f.agent}
                          </span>
                          <span
                            className={s.s174}
                          >
                            → {f.vendor}
                          </span>
                        </div>
                      </div>
                      <div
                        className={s.s133}
                      >
                        <span
                          className={s.s175}
                          style={{ '--d0': f.amtColor } as CSSVars}
                        >
                          {f.amountF}
                        </span>
                        <span
                          className={s.s176}
                        >
                          <span
                            className={s.s177}
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
                  className={s.s178}
                >
                  <span
                    className={s.s179}
                  >
                    Waiting for first payment{v.cursor}
                  </span>
                  <span
                    className={s.s180}
                  >
                    Spends appear the moment an agent settles an x402 session.
                  </span>
                </div>
                </>
              ) : null}
            </div>
            <div
              className={s.s181}
            >
              <div
                className={s.s182}
              >
                <h2
                  className={s.s026}
                >
                  Escrowed sessions
                </h2>
                <div
                  className={s.s027}
                >
                  <span
                    className={s.s028}
                  >
                    <span
                      className={s.s029}
                    >
                      x402
                    </span>
                    <span
                      className={s.s030}
                    >
                      PROTOCOL
                    </span>
                  </span>
                  <span
                    className={s.s028}
                  >
                    <span
                      className={s.s031}
                    >
                      TEE
                    </span>
                    <span
                      className={s.s030}
                    >
                      METERED IN
                    </span>
                  </span>
                </div>
              </div>
              <div
                className={s.s159}
              >
                <div
                  className={s.s160}
                >
                  <span
                    className={s.s161}
                  >
                    {v.escInt}
                  </span>
                  <span
                    className={s.s162}
                  >
                    .{v.escDec}
                  </span>
                </div>
                <div
                  className={s.s163}
                >
                  <span
                    className={s.s037}
                  >
                    USDC
                  </span>
                  <span
                    className={s.s183}
                  >
                    locked in {v.sessionCount} {v.sessionWord}
                  </span>
                </div>
              </div>
              <div
                className={s.s184}
              >
                {v.stages.map((st, i) => (
                  <Fragment key={i}>
                  <div
                    className={s.s185}
                    onMouseEnter={st.enter}
                    onMouseLeave={v.stOff}
                  >
                    <span
                      className={s.s186}
                      aria-hidden="true"
                      style={{ '--d0': st.clip } as CSSVars}
                    />
                    <span
                      className={s.s187}
                      aria-hidden="true"
                      style={{ '--d0': st.prog } as CSSVars}
                    />
                    <span
                      className={s.s188}
                      style={{ '--d0': st.nc } as CSSVars}
                    >
                      <span>
                        {st.n}
                      </span>
                      <span
                        className={s.s189}
                        style={{ '--d0': st.arO } as CSSVars}
                      >
                        →
                      </span>
                    </span>
                    <div
                      className={s.s190}
                    >
                      <span
                        className={s.s191}
                        style={{ '--d0': st.vc } as CSSVars}
                      >
                        {st.value}
                      </span>
                      <span
                        className={s.s192}
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
                  className={s.s169}
                >
                  {v.sessionRows.map((se, i) => (
                    <Fragment key={i}>
                    <div
                      className={s.s193}
                    >
                      <div
                        className={s.s194}
                      >
                        <div
                          className={s.s128}
                        >
                          <span
                            className={s.s195}
                            aria-hidden="true"
                            style={{ '--d0': se.avBg, '--d1': se.avPos, '--d2': se.avO } as CSSVars}
                          >
                            {se.ini}
                          </span>
                          <div
                            className={s.s135}
                          >
                            <span
                              className={s.s196}
                            >
                              {se.agent}
                            </span>
                            <span
                              className={s.s174}
                            >
                              → {se.vendor}
                            </span>
                          </div>
                        </div>
                        <span
                          className={s.s197}
                        >
                          {se.meteredF}{' '}
                          <span
                            className={s.s198}
                          >
                            / {se.escrowF}
                          </span>
                        </span>
                      </div>
                      <div
                        className={s.s199}
                      >
                        <div
                          className={s.s200}
                          style={{ '--d0': se.fillW } as CSSVars}
                        />
                        <span
                          className={s.s201}
                          style={{ '--d0': se.fillW } as CSSVars}
                        />
                      </div>
                      <div
                        className={s.s202}
                      >
                        <span
                          className={s.s203}
                        >
                          {se.pctF} · {se.rate} tok/s · refund {se.refundF}
                        </span>
                        <button
                          className={s.s204}
                          onClick={se.settle}
                        >
                          CLOSE & SETTLE ↗
                        </button>
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
                  className={s.s205}
                >
                  <div
                    className={s.s206}
                  />
                  <span
                    className={s.s180}
                  >
                    No open sessions.
                  </span>
                </div>
                </>
              ) : null}
            </div>
          </section>
          <section
            className={s.s207}
          >
            <div
              className={s.s208}
              aria-hidden="true"
            />
            <div
              className={s.s209}
              aria-hidden="true"
            />
            <div
              className={s.s210}
            >
              <div
                className={s.s211}
              >
                <span
                  className={s.s212}
                >
                  AUDIT · SELECTIVE DISCLOSURE
                </span>
                <h2
                  className={s.s213}
                >
                  Prove exactly what's asked. Nothing more.
                </h2>
                <span
                  className={s.s214}
                >
                  Hand an auditor a scoped viewing proof — one payment, one agent's history, or one period. They verify it against Hedera; the rest of the ledger stays sealed.
                </span>
                <div
                  className={s.s215}
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
                className={s.s216}
              >
                <div
                  className={s.s217}
                >
                  <span
                    className={s.s212}
                  >
                    TWO VIEWS OF ONE PAYMENT
                  </span>
                  <span
                    className={s.s212}
                  >
                    SELECT IN FEED
                  </span>
                </div>
                <div
                  className={s.s218}
                >
                  <div
                    className={s.s219}
                  >
                    <span
                      className={s.s212}
                    >
                      HEDERA SEES
                    </span>
                    {v.chainMini.map((r, i) => (
                      <Fragment key={i}>
                      <div
                        className={s.s220}
                      >
                        <span
                          className={s.s221}
                        >
                          {r.k}
                        </span>
                        <span
                          className={s.s222}
                        >
                          {r.v}
                        </span>
                      </div>
                      </Fragment>
                    ))}
                  </div>
                  <div
                    className={s.s223}
                  >
                    <span
                      className={s.s224}
                    >
                      YOU SEE
                    </span>
                    <span
                      className={s.s225}
                    >
                      {v.selV.agent}
                    </span>
                    <span
                      className={s.s174}
                    >
                      → {v.selV.vendor}
                    </span>
                    <span
                      className={s.s226}
                    >
                      {v.selV.amountF}
                    </span>
                    <span
                      className={s.s227}
                    >
                      {v.selV.meta}
                    </span>
                  </div>
                  {v.hasSel ? (
                    <>
                    <span
                      className={s.s228}
                    >
                      PROOF ✓
                    </span>
                    </>
                  ) : null}
                </div>
              </div>
            </div>
            <div
              className={s.s229}
            >
              <div
                className={s.s230}
              >
                {v.scopeCards.map((sc, i) => (
                  <Fragment key={i}>
                  <button
                    className={s.s231}
                    onClick={sc.pick}
                    style={{ '--d0': sc.op } as CSSVars}
                  >
                    <span
                      className={s.s232}
                    >
                      {sc.num}
                    </span>
                    <span
                      className={s.s233}
                      style={{ '--d0': sc.bar } as CSSVars}
                    />
                    <span
                      className={s.s146}
                    >
                      {sc.label}
                    </span>
                    <span
                      className={s.s234}
                    >
                      {sc.desc}
                    </span>
                  </button>
                  </Fragment>
                ))}
              </div>
              <div
                className={s.s235}
              >
                {v.scopeAgent ? (
                  <>
                  <select
                    className={s.s236}
                    value={v.auditAgent}
                    onChange={v.pickAuditAgent}
                  >
                    {v.agentOptions.map((o, i) => (
                      <Fragment key={i}>
                      <option
                        className={s.s237}
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
                  className={s.s025}
                >
                  <span
                    className={s.s212}
                  >
                    IN SCOPE
                  </span>
                  <span
                    className={s.s238}
                  >
                    {v.scopeIn}
                  </span>
                  <span
                    className={s.s239}
                  >
                    <span
                      className={s.s212}
                    >
                      STAYS SEALED
                    </span>
                  </span>
                  <span
                    className={s.s238}
                  >
                    {v.scopeOut}
                  </span>
                </div>
                <button
                  className={s.s240}
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
                    className={s.s241}
                  >
                    {v.proofRows.map((p, i) => (
                      <Fragment key={i}>
                      <div
                        className={s.s242}
                      >
                        <div
                          className={s.s135}
                        >
                          <span
                            className={s.s243}
                          >
                            {p.label}
                          </span>
                          <span
                            className={s.s244}
                          >
                            {p.id} · {p.rel}
                          </span>
                        </div>
                        <span
                          className={s.s245}
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
          className={s.s246}
          onClick={v.closeModals}
        >
          <div
            className={s.s247}
            onClick={v.stop}
          >
            <div
              className={s.s156}
            >
              <span
                className={s.s248}
              >
                Fund shielded pool
              </span>
              <button
                className={s.s249}
                onClick={v.closeModals}
              >
                ×
              </button>
            </div>
            <div
              className={s.s250}
            >
              <input
                className={s.s251}
                value={v.fundAmount}
                onChange={v.setFundAmount}
                inputMode="decimal"
              />
              <span
                className={s.s196}
              >
                USDC
              </span>
            </div>
            <div
              className={s.s252}
            >
              {v.fundPresets.map((fp, i) => (
                <Fragment key={i}>
                <button
                  className={s.s253}
                  onClick={fp.pick}
                >
                  {fp.label}
                </button>
                </Fragment>
              ))}
            </div>
            <div
              className={s.s254}
            >
              <div
                className={s.s255}
              >
                <span
                  className={s.s256}
                >
                  From
                </span>
                <span
                  className={s.s257}
                >
                  Treasury 0.0.1029384
                </span>
              </div>
              <div
                className={s.s255}
              >
                <span
                  className={s.s256}
                >
                  Creates
                </span>
                <span>
                  1 shielded note
                </span>
              </div>
              <div
                className={s.s255}
              >
                <span
                  className={s.s256}
                >
                  Visible on-chain
                </span>
                <span>
                  deposit only — later spends unlinkable
                </span>
              </div>
            </div>
            <button
              className={s.s258}
              onClick={v.confirmFund}
            >
              {v.fundBtnLabel}
            </button>
          </div>
        </div>
        </>
      ) : null}
      {v.mandateOpen ? (
        <>
        <div
          className={s.s246}
          onClick={v.closeModals}
        >
          <div
            className={s.s259}
            onClick={v.stop}
          >
            <div
              className={s.s156}
            >
              <span
                className={s.s248}
              >
                Issue mandate
              </span>
              <button
                className={s.s249}
                onClick={v.closeModals}
              >
                ×
              </button>
            </div>
            <label
              className={s.s260}
            >
              Agent name
              <input
                className={s.s261}
                value={v.mName}
                onChange={v.setMName}
              />
            </label>
            <label
              className={s.s260}
            >
              Budget (USDC)
              <input
                className={s.s261}
                value={v.mBudget}
                onChange={v.setMBudget}
                inputMode="decimal"
              />
            </label>
            <div
              className={s.s262}
            >
              Allowed endpoints
              <div
                className={s.s263}
              >
                {v.endpointChips.map((ep, i) => (
                  <Fragment key={i}>
                  <button
                    className={s.s264}
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
              className={s.s265}
            >
              <span
                className={s.s256}
              >
                Unallocated in pool
              </span>
              <span
                className={s.s042}
              >
                {v.freeF}
              </span>
            </div>
            {v.mError ? (
              <>
              <span
                className={s.s266}
              >
                {v.mError}
              </span>
              </>
            ) : null}
            <button
              className={s.s258}
              onClick={v.confirmMandate}
            >
              Sign & issue mandate
            </button>
          </div>
        </div>
        </>
      ) : null}
      {v.toast ? (
        <>
        <div
          className={s.s267}
        >
          <span
            className={s.s268}
            style={{ '--d0': v.toast.color } as CSSVars}
          />
          <div
            className={s.s269}
          >
            <span
              className={s.s243}
            >
              {v.toast.title}
            </span>
            <span
              className={s.s171}
            >
              {v.toast.sub}
            </span>
          </div>
        </div>
        </>
      ) : null}
    </div>
    </>
  )
}
