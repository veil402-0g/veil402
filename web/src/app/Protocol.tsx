import type { CSSVars } from '../ui/css-vars'
import type { LandingBindings } from './useLandingBindings'
import s from './landing.module.css'

export function Protocol({ v }: { v: LandingBindings }) {
  return (
    <>
  <section
    className={s.s425}
    data-screen-label="Protocol"
    ref={v.protoRef}
  >
    <div
      className={s.s426}
    >
      <div
        className={s.s427}
      >
        <span
          className={s.s428}
          style={{ '--d0': v.vxOp } as CSSVars}
        >
          ONE GATEWAY FOR EVERY AGENT YOU RUN
        </span>
        <h2
          className={s.s429}
        >
          <span
            className={s.s065}
          >
            <span
              className={s.s430}
              style={{ '--d0': v.vxWord } as CSSVars}
            >
              A
            </span>
          </span>
          <span
            className={s.s074}
          >
            <span
              className={s.s065}
            >
              <span
                className={s.s431}
                style={{ '--d0': v.vxWord } as CSSVars}
              >
                VPN
              </span>
            </span>
            <span
              className={s.s432}
              style={{ '--d0': v.vxLine } as CSSVars}
            />
          </span>
          <span
            className={s.s065}
          >
            <span
              className={s.s433}
              style={{ '--d0': v.vxWord } as CSSVars}
            >
              for
            </span>
          </span>
          <span
            className={s.s065}
          >
            <span
              className={s.s434}
              style={{ '--d0': v.vxWord } as CSSVars}
            >
              AI
            </span>
          </span>
        </h2>
      </div>
      <p
        className={s.s435}
        style={{ '--d0': v.vxOp, '--d1': v.vxUp } as CSSVars}
      >
        A VPN hides your identity. Veil402{' '}
        <span
          className={s.s436}
          style={{ '--d0': v.vxHc, '--d1': v.vxHl } as CSSVars}
        >
          hides your payments
        </span>
        {' '}from providers while keeping every agent, call, and cent{' '}
        <span
          className={s.s437}
          style={{ '--d0': v.vxHc, '--d1': v.vxHl } as CSSVars}
        >
          auditable
        </span>
        .
      </p>
    </div>
    <div
      className={s.s438}
      style={{ '--d0': v.vxClip } as CSSVars}
    >
      <div
        className={s.s439}
        aria-hidden="true"
      >
        <div
          className={s.s440}
          aria-hidden="true"
        />
        <div
          className={s.s086}
          aria-hidden="true"
        />
        <div
          className={s.s087}
          aria-hidden="true"
        />
        <div
          className={s.s088}
          aria-hidden="true"
        />
        <div
          className={s.s089}
          aria-hidden="true"
        />
        <div
          className={s.s090}
          aria-hidden="true"
        />
        <div
          className={s.s091}
          aria-hidden="true"
        />
      </div>
      <div
        className={s.s441}
        style={{ '--d0': v.vxOp, '--d1': v.vxRise } as CSSVars}
      >
        <div
          className={s.s442}
        >
          <span />
          <button
            className={s.s443}
            onClick={v.pToggle}
            aria-label="Play or pause"
          >
            {v.pLabel}
          </button>
        </div>
        <div
          className={s.s444}
          data-pstage="1"
        >
          <div
            className={s.s445}
            style={{ '--d0': v.pW, '--d1': v.pH } as CSSVars}
          >
            <div
              className={s.s446}
              style={{ '--d0': v.pK } as CSSVars}
            >
              <div
                className={s.s447}
              >
                <div
                  className={s.s448}
                  style={{ '--d0': v.cap0, '--d1': v.capF0 } as CSSVars}
                >
                  <span
                    className={s.s449}
                  >
                    Your agent
                  </span>
                  <span
                    className={s.s450}
                  />
                  <span
                    className={s.s451}
                  >
                    Any OpenAI-compatible tool or SDK. Secure access to private compute.
                  </span>
                </div>
                <div
                  className={s.s452}
                  style={{ '--d0': v.cap1, '--d1': v.capF1 } as CSSVars}
                >
                  <span
                    className={s.s449}
                  >
                    Veil402 daemon
                  </span>
                  <span
                    className={s.s453}
                  >
                    YOURS
                  </span>
                  <span
                    className={s.s451}
                  >
                    private ledger · shields payments · encrypts comms · proves accounts
                  </span>
                </div>
                <div
                  className={s.s454}
                  style={{ '--d0': v.cap2, '--d1': v.capF2 } as CSSVars}
                >
                  <span
                    className={s.s449}
                  >
                    Enclave
                  </span>
                  <span
                    className={s.s453}
                  >
                    THE SELLER
                  </span>
                  <span
                    className={s.s451}
                  >
                    authorises inference · gates escrow · meters usage · signs settlement
                  </span>
                </div>
                <div
                  className={s.s455}
                  style={{ '--d0': v.cap3, '--d1': v.capF3 } as CSSVars}
                >
                  <span
                    className={s.s449}
                  >
                    0G Private Computer
                  </span>
                  <span
                    className={s.s453}
                  >
                    BLIND TO THE PAYER
                  </span>
                  <span
                    className={s.s451}
                  >
                    routes inference · selects TEE providers · forwards encrypted requests · handles billing
                  </span>
                </div>
                <div
                  className={s.s456}
                  style={{ '--d0': v.cap4, '--d1': v.capF4 } as CSSVars}
                >
                  <span
                    className={s.s449}
                  >
                    TEE provider
                  </span>
                  <span
                    className={s.s450}
                  />
                  <span
                    className={s.s451}
                  >
                    Intel TDX · weights inside the enclave
                  </span>
                </div>
                <div
                  className={s.s457}
                />
                <div
                  className={s.s458}
                  style={{ '--d0': v.pgW, '--d1': v.pgTr } as CSSVars}
                />
                <div
                  className={s.s459}
                />
                <div
                  className={s.s460}
                  style={{ '--d0': v.rLit } as CSSVars}
                />
                <div
                  className={s.s461}
                />
                <div
                  className={s.s462}
                />
                <span
                  className={s.s463}
                  style={{ '--d0': v.rLabC } as CSSVars}
                >
                  answer, streamed
                </span>
                <div
                  className={s.s464}
                />
                <div
                  className={s.s465}
                  style={{ '--d0': v.brS } as CSSVars}
                />
                <span
                  className={s.s466}
                  style={{ '--d0': v.stC } as CSSVars}
                >
                  settlement, proved on Hedera
                </span>
                <div
                  className={s.s467}
                  style={{ '--d0': `2px solid ${v.nb0}`, '--d1': v.nbg0, '--d2': v.nsh0, '--d3': v.nan0 } as CSSVars}
                >
                  <svg
                    className={s.s468}
                    width="26"
                    height="26"
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke={v.nic0}
                    strokeWidth="1.6"
                  >
                    <rect
                      x="3"
                      y="5"
                      width="18"
                      height="14"
                    />
                    <path
                      d="M7 10l3 2-3 2M12.5 14.5h4"
                    />
                  </svg>
                </div>
                <div
                  className={s.s469}
                  style={{ '--d0': `2px solid ${v.nb1}`, '--d1': v.nbg1, '--d2': v.nsh1, '--d3': v.nan1 } as CSSVars}
                >
                  <svg
                    className={s.s468}
                    width="26"
                    height="26"
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke={v.nic1}
                    strokeWidth="1.6"
                  >
                    <rect
                      x="4"
                      y="4"
                      width="16"
                      height="7"
                    />
                    <rect
                      x="4"
                      y="13"
                      width="16"
                      height="7"
                    />
                    <path
                      d="M7.5 7.5h3M7.5 16.5h3"
                    />
                  </svg>
                </div>
                <div
                  className={s.s470}
                  style={{ '--d0': `2px solid ${v.nb2}`, '--d1': v.nbg2, '--d2': v.nsh2, '--d3': v.nan2 } as CSSVars}
                >
                  <svg
                    className={s.s468}
                    width="26"
                    height="26"
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke={v.nic2}
                    strokeWidth="1.6"
                  >
                    <rect
                      x="5"
                      y="10.5"
                      width="14"
                      height="10"
                    />
                    <path
                      d="M8 10.5V7.5a4 4 0 018 0v3M12 14.5v2.5"
                    />
                  </svg>
                </div>
                <div
                  className={s.s471}
                  style={{ '--d0': `2px solid ${v.nb3}`, '--d1': v.nbg3, '--d2': v.nsh3, '--d3': v.nan3 } as CSSVars}
                >
                  <svg
                    className={s.s468}
                    width="26"
                    height="26"
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke={v.nic3}
                    strokeWidth="1.6"
                  >
                    <path
                      d="M4 8h14M14.5 4.5L18 8l-3.5 3.5M20 16H6M9.5 12.5L6 16l3.5 3.5"
                    />
                  </svg>
                </div>
                <div
                  className={s.s472}
                  style={{ '--d0': `2px solid ${v.nb4}`, '--d1': v.nbg4, '--d2': v.nsh4, '--d3': v.nan4 } as CSSVars}
                >
                  <svg
                    className={s.s468}
                    width="26"
                    height="26"
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke={v.nic4}
                    strokeWidth="1.6"
                  >
                    <rect
                      x="6.5"
                      y="6.5"
                      width="11"
                      height="11"
                    />
                    <rect
                      x="10"
                      y="10"
                      width="4"
                      height="4"
                    />
                    <path
                      d="M9.5 3v3.5M14.5 3v3.5M9.5 17.5V21M14.5 17.5V21M3 9.5h3.5M3 14.5h3.5M17.5 9.5H21M17.5 14.5H21"
                    />
                  </svg>
                </div>
                <div
                  className={s.s473}
                  style={{ '--d0': `2px solid ${v.nb5}`, '--d1': v.nbg5, '--d2': v.nsh5, '--d3': v.nan5 } as CSSVars}
                >
                  <svg
                    className={s.s468}
                    width="26"
                    height="26"
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke={v.nic5}
                    strokeWidth="1.6"
                  >
                    <path
                      d="M12 3l8 4.5v9L12 21l-8-4.5v-9z"
                    />
                    <circle
                      cx="12"
                      cy="12"
                      r="3"
                    />
                  </svg>
                </div>
                <div
                  className={s.s474}
                  style={{ '--d0': v.cap5, '--d1': v.capF5 } as CSSVars}
                >
                  <span
                    className={s.s150}
                  >
                    <span
                      className={s.s449}
                    >
                      Shielded pool
                    </span>
                    <span
                      className={s.s475}
                    >
                      HEDERA
                    </span>
                  </span>
                  <span
                    className={s.s476}
                  >
                    shielded note · time-locked escrow · ZK settlement
                  </span>
                </div>
                <svg
                  className={s.s477}
                  width="1200"
                  height="430"
                  fill="none"
                >
                  <g
                    stroke="#d9dae0"
                    strokeWidth="1.5"
                  >
                    <path
                      d="M910 170 H960 V110 H1000"
                    />
                    <path
                      d="M960 170 V230 H1000"
                      strokeDasharray="4 4"
                    />
                  </g>
                  <g
                    className={s.s478}
                    stroke="#1230d6"
                    strokeWidth="1.5"
                    style={{ '--d0': v.teeO } as CSSVars}
                  >
                    <path
                      d="M910 170 H960 V110 H1000"
                    />
                  </g>
                  <path
                    d="M1170 96 V244"
                    stroke="#c4c8d4"
                    strokeWidth="1"
                    strokeDasharray="2 4"
                  />
                </svg>
                <div
                  className={s.s479}
                  style={{ '--d0': v.teeO } as CSSVars}
                >
                  <span
                    className={s.s095}
                  >
                    <span
                      className={s.s480}
                    >
                      <svg
                        width="9"
                        height="9"
                        viewBox="0 0 16 16"
                        fill="none"
                        stroke="#fff"
                        strokeWidth="2.6"
                        strokeLinecap="round"
                        strokeLinejoin="round"
                      >
                        <path
                          d="M3 8.5L6.5 12L13 4.5"
                        />
                      </svg>
                    </span>
                    <span
                      className={s.s481}
                    >
                      TeeML
                    </span>
                    <span
                      className={s.s482}
                    >
                      WE SELL THIS
                    </span>
                  </span>
                  <span
                    className={s.s483}
                  >
                    Weights run inside the enclave; the provider cannot read the prompt.
                  </span>
                </div>
                <div
                  className={s.s484}
                >
                  <span
                    className={s.s095}
                  >
                    <span
                      className={s.s485}
                    >
                      <svg
                        width="8"
                        height="8"
                        viewBox="0 0 16 16"
                        fill="none"
                        stroke="#8b8e97"
                        strokeWidth="2.6"
                        strokeLinecap="round"
                      >
                        <path
                          d="M4 4l8 8M12 4l-8 8"
                        />
                      </svg>
                    </span>
                    <span
                      className={s.s486}
                    >
                      TeeTLS
                    </span>
                    <span
                      className={s.s487}
                    >
                      EXCLUDED
                    </span>
                  </span>
                  <span
                    className={s.s488}
                  >
                    TLS ends in an enclave, then a vendor reads the prompt.
                  </span>
                </div>
                <span
                  className={s.s489}
                >
                  none
                </span>
                <span
                  className={s.s490}
                >
                  read
                </span>
                <span
                  className={s.s491}
                >
                  PROMPT EXPOSURE
                </span>
                <div
                  className={s.s492}
                  style={{ '--d0': v.pkL, '--d1': v.pkO, '--d2': v.pkTr } as CSSVars}
                >
                  <span
                    className={s.s493}
                  >
                    {v.pkT}
                  </span>
                </div>
                <div
                  className={s.s494}
                  style={{ '--d0': v.ntT, '--d1': v.ntO, '--d2': v.ntTr } as CSSVars}
                />
                <div
                  className={s.s495}
                  style={{ '--d0': v.rpL, '--d1': v.rpO, '--d2': v.rpTr } as CSSVars}
                />
              </div>
            </div>
          </div>
        </div>
        <div
          className={s.s496}
        >
          <div
            className={s.s497}
          >
            <span
              className={s.s498}
            >
              {v.pN} / 07
            </span>
            <div
              className={s.s499}
            >
              <span
                className={s.s500}
              >
                {v.pTitle}
              </span>
              <span
                className={s.s501}
              >
                {v.pBody}
              </span>
            </div>
          </div>
          <span
            className={s.s502}
            style={{ '--d0': v.ppC, '--d1': v.ppB } as CSSVars}
          >
            <span
              className={s.s503}
              style={{ '--d0': v.ppD } as CSSVars}
            />
            {v.pPill}
          </span>
        </div>
        <div
          className={s.s504}
        >
          <button
            className={s.s505}
            onClick={v.pc0}
          >
            <span
              className={s.s506}
            >
              <span
                className={s.s507}
              >
                01
              </span>
              CALL
            </span>
            <span
              className={s.s508}
              aria-hidden="true"
              style={{ '--d0': v.pcf0, '--d1': v.pct0 } as CSSVars}
            >
              <span
                className={s.s509}
                style={{ '--d0': v.pgp0, '--d1': v.pgt0 } as CSSVars}
              />
              <span
                className={s.s510}
              >
                <span
                  className={s.s507}
                >
                  01
                </span>
                CALL
              </span>
            </span>
          </button>
          <button
            className={s.s505}
            onClick={v.pc1}
          >
            <span
              className={s.s506}
            >
              <span
                className={s.s507}
              >
                02
              </span>
              RESERVE
            </span>
            <span
              className={s.s511}
              aria-hidden="true"
              style={{ '--d0': v.pcf1, '--d1': v.pct1 } as CSSVars}
            >
              <span
                className={s.s512}
                style={{ '--d0': v.pgp1, '--d1': v.pgt1 } as CSSVars}
              />
              <span
                className={s.s510}
              >
                <span
                  className={s.s507}
                >
                  02
                </span>
                RESERVE
              </span>
            </span>
          </button>
          <button
            className={s.s505}
            onClick={v.pc2}
          >
            <span
              className={s.s506}
            >
              <span
                className={s.s507}
              >
                03
              </span>
              ESCROW
            </span>
            <span
              className={s.s513}
              aria-hidden="true"
              style={{ '--d0': v.pcf2, '--d1': v.pct2 } as CSSVars}
            >
              <span
                className={s.s514}
                style={{ '--d0': v.pgp2, '--d1': v.pgt2 } as CSSVars}
              />
              <span
                className={s.s510}
              >
                <span
                  className={s.s507}
                >
                  03
                </span>
                ESCROW
              </span>
            </span>
          </button>
          <button
            className={s.s505}
            onClick={v.pc3}
          >
            <span
              className={s.s506}
            >
              <span
                className={s.s507}
              >
                04
              </span>
              METER
            </span>
            <span
              className={s.s515}
              aria-hidden="true"
              style={{ '--d0': v.pcf3, '--d1': v.pct3 } as CSSVars}
            >
              <span
                className={s.s516}
                style={{ '--d0': v.pgp3, '--d1': v.pgt3 } as CSSVars}
              />
              <span
                className={s.s510}
              >
                <span
                  className={s.s507}
                >
                  04
                </span>
                METER
              </span>
            </span>
          </button>
          <button
            className={s.s505}
            onClick={v.pc4}
          >
            <span
              className={s.s506}
            >
              <span
                className={s.s507}
              >
                05
              </span>
              ROUTE
            </span>
            <span
              className={s.s517}
              aria-hidden="true"
              style={{ '--d0': v.pcf4, '--d1': v.pct4 } as CSSVars}
            >
              <span
                className={s.s518}
                style={{ '--d0': v.pgp4, '--d1': v.pgt4 } as CSSVars}
              />
              <span
                className={s.s510}
              >
                <span
                  className={s.s507}
                >
                  05
                </span>
                ROUTE
              </span>
            </span>
          </button>
          <button
            className={s.s505}
            onClick={v.pc5}
          >
            <span
              className={s.s506}
            >
              <span
                className={s.s507}
              >
                06
              </span>
              INFER
            </span>
            <span
              className={s.s519}
              aria-hidden="true"
              style={{ '--d0': v.pcf5, '--d1': v.pct5 } as CSSVars}
            >
              <span
                className={s.s520}
                style={{ '--d0': v.pgp5, '--d1': v.pgt5 } as CSSVars}
              />
              <span
                className={s.s510}
              >
                <span
                  className={s.s507}
                >
                  06
                </span>
                INFER
              </span>
            </span>
          </button>
          <button
            className={s.s505}
            onClick={v.pc6}
          >
            <span
              className={s.s506}
            >
              <span
                className={s.s507}
              >
                07
              </span>
              SETTLE
            </span>
            <span
              className={s.s521}
              aria-hidden="true"
              style={{ '--d0': v.pcf6, '--d1': v.pct6 } as CSSVars}
            >
              <span
                className={s.s522}
                style={{ '--d0': v.pgp6, '--d1': v.pgt6 } as CSSVars}
              />
              <span
                className={s.s510}
              >
                <span
                  className={s.s507}
                >
                  07
                </span>
                SETTLE
              </span>
            </span>
          </button>
        </div>
      </div>
    </div>
  </section>
    </>
  )
}
