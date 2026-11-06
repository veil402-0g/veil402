import type { CSSVars } from '../ui/css-vars'
import type { LandingBindings } from './useLandingBindings'
import s from './landing.module.css'

export function Protocol({ v }: { v: LandingBindings }) {
  return (
    <>
  <section
    className={s.s373}
    data-screen-label="Protocol"
    ref={v.protoRef}
  >
    <div
      className={s.s374}
    >
      <div
        className={s.s375}
      >
        <span
          className={s.s376}
          style={{ '--d0': v.vxOp } as CSSVars}
        >
          ONE GATEWAY FOR EVERY AGENT YOU RUN
        </span>
        <h2
          className={s.s377}
        >
          <span
            className={s.s065}
          >
            <span
              className={s.s378}
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
                className={s.s379}
                style={{ '--d0': v.vxWord } as CSSVars}
              >
                VPN
              </span>
            </span>
            <span
              className={s.s380}
              style={{ '--d0': v.vxLine } as CSSVars}
            />
          </span>
          <span
            className={s.s065}
          >
            <span
              className={s.s381}
              style={{ '--d0': v.vxWord } as CSSVars}
            >
              for
            </span>
          </span>
          <span
            className={s.s065}
          >
            <span
              className={s.s382}
              style={{ '--d0': v.vxWord } as CSSVars}
            >
              AI
            </span>
          </span>
        </h2>
      </div>
      <p
        className={s.s383}
        style={{ '--d0': v.vxOp, '--d1': v.vxUp } as CSSVars}
      >
        A VPN hides your identity. Veil402{' '}
        <span
          className={s.s384}
          style={{ '--d0': v.vxHc, '--d1': v.vxHl } as CSSVars}
        >
          hides your payments
        </span>
        {' '}from providers while keeping every agent, call, and cent{' '}
        <span
          className={s.s385}
          style={{ '--d0': v.vxHc, '--d1': v.vxHl } as CSSVars}
        >
          auditable
        </span>
        .
      </p>
    </div>
    <div
      className={s.s386}
      style={{ '--d0': v.vxClip } as CSSVars}
    >
      <div
        className={s.s387}
        aria-hidden="true"
      >
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
        className={s.s388}
        style={{ '--d0': v.vxOp, '--d1': v.vxRise } as CSSVars}
      >
        <div
          className={s.s389}
        >
          <span />
          <button
            className={s.s390}
            onClick={v.pToggle}
            aria-label="Play or pause"
          >
            {v.pLabel}
          </button>
        </div>
        <div
          className={s.s391}
          data-pstage="1"
        >
          <div
            className={s.s392}
            style={{ '--d0': v.pW, '--d1': v.pH } as CSSVars}
          >
            <div
              className={s.s393}
              style={{ '--d0': v.pK } as CSSVars}
            >
              <div
                className={s.s394}
              >
                <div
                  className={s.s395}
                  style={{ '--d0': v.cap0, '--d1': v.capF0 } as CSSVars}
                >
                  <span
                    className={s.s396}
                  >
                    Your agent
                  </span>
                  <span
                    className={s.s397}
                  />
                  <span
                    className={s.s398}
                  >
                    Any OpenAI-compatible tool or SDK. Secure access to private compute.
                  </span>
                </div>
                <div
                  className={s.s399}
                  style={{ '--d0': v.cap1, '--d1': v.capF1 } as CSSVars}
                >
                  <span
                    className={s.s396}
                  >
                    Veil402 daemon
                  </span>
                  <span
                    className={s.s400}
                  >
                    YOURS
                  </span>
                  <span
                    className={s.s398}
                  >
                    private ledger · shields payments · encrypts comms · proves accounts
                  </span>
                </div>
                <div
                  className={s.s401}
                  style={{ '--d0': v.cap2, '--d1': v.capF2 } as CSSVars}
                >
                  <span
                    className={s.s396}
                  >
                    Enclave
                  </span>
                  <span
                    className={s.s400}
                  >
                    THE SELLER
                  </span>
                  <span
                    className={s.s398}
                  >
                    authorises inference · gates escrow · meters usage · signs settlement
                  </span>
                </div>
                <div
                  className={s.s402}
                  style={{ '--d0': v.cap3, '--d1': v.capF3 } as CSSVars}
                >
                  <span
                    className={s.s396}
                  >
                    0G Private Computer
                  </span>
                  <span
                    className={s.s400}
                  >
                    BLIND TO THE PAYER
                  </span>
                  <span
                    className={s.s398}
                  >
                    routes inference · selects TEE providers · forwards encrypted requests · handles billing
                  </span>
                </div>
                <div
                  className={s.s403}
                  style={{ '--d0': v.cap4, '--d1': v.capF4 } as CSSVars}
                >
                  <span
                    className={s.s396}
                  >
                    TEE provider
                  </span>
                  <span
                    className={s.s397}
                  />
                  <span
                    className={s.s398}
                  >
                    Intel TDX · weights inside the enclave
                  </span>
                </div>
                <div
                  className={s.s404}
                />
                <div
                  className={s.s405}
                  style={{ '--d0': v.pgW, '--d1': v.pgTr } as CSSVars}
                />
                <div
                  className={s.s406}
                />
                <div
                  className={s.s407}
                  style={{ '--d0': v.rLit } as CSSVars}
                />
                <div
                  className={s.s408}
                />
                <div
                  className={s.s409}
                />
                <span
                  className={s.s410}
                  style={{ '--d0': v.rLabC } as CSSVars}
                >
                  answer, streamed
                </span>
                <div
                  className={s.s411}
                />
                <div
                  className={s.s412}
                  style={{ '--d0': v.brS } as CSSVars}
                />
                <span
                  className={s.s413}
                  style={{ '--d0': v.stC } as CSSVars}
                >
                  settlement, proved on Hedera
                </span>
                <div
                  className={s.s414}
                  style={{ '--d0': `1.5px solid ${v.nb0}`, '--d1': v.nbg0, '--d2': v.nsh0, '--d3': v.nan0 } as CSSVars}
                >
                  <svg
                    className={s.s415}
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
                  className={s.s416}
                  style={{ '--d0': `1.5px solid ${v.nb1}`, '--d1': v.nbg1, '--d2': v.nsh1, '--d3': v.nan1 } as CSSVars}
                >
                  <svg
                    className={s.s415}
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
                  className={s.s417}
                  style={{ '--d0': `1.5px solid ${v.nb2}`, '--d1': v.nbg2, '--d2': v.nsh2, '--d3': v.nan2 } as CSSVars}
                >
                  <svg
                    className={s.s415}
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
                  className={s.s418}
                  style={{ '--d0': `1.5px solid ${v.nb3}`, '--d1': v.nbg3, '--d2': v.nsh3, '--d3': v.nan3 } as CSSVars}
                >
                  <svg
                    className={s.s415}
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
                  className={s.s419}
                  style={{ '--d0': `1.5px solid ${v.nb4}`, '--d1': v.nbg4, '--d2': v.nsh4, '--d3': v.nan4 } as CSSVars}
                >
                  <svg
                    className={s.s415}
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
                  className={s.s420}
                  style={{ '--d0': `1.5px solid ${v.nb5}`, '--d1': v.nbg5, '--d2': v.nsh5, '--d3': v.nan5 } as CSSVars}
                >
                  <svg
                    className={s.s415}
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
                  className={s.s421}
                  style={{ '--d0': v.cap5, '--d1': v.capF5 } as CSSVars}
                >
                  <span
                    className={s.s149}
                  >
                    <span
                      className={s.s396}
                    >
                      Shielded pool
                    </span>
                    <span
                      className={s.s422}
                    >
                      HEDERA
                    </span>
                  </span>
                  <span
                    className={s.s423}
                  >
                    shielded note · time-locked escrow · ZK settlement
                  </span>
                </div>
                <svg
                  className={s.s424}
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
                    className={s.s425}
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
                  className={s.s426}
                  style={{ '--d0': v.teeO } as CSSVars}
                >
                  <span
                    className={s.s095}
                  >
                    <span
                      className={s.s427}
                    >
                      ✓
                    </span>
                    <span
                      className={s.s428}
                    >
                      TeeML
                    </span>
                    <span
                      className={s.s429}
                    >
                      WE SELL THIS
                    </span>
                  </span>
                  <span
                    className={s.s430}
                  >
                    Weights run inside the enclave; the provider cannot read the prompt.
                  </span>
                </div>
                <div
                  className={s.s431}
                >
                  <span
                    className={s.s095}
                  >
                    <span
                      className={s.s432}
                    >
                      ✕
                    </span>
                    <span
                      className={s.s433}
                    >
                      TeeTLS
                    </span>
                    <span
                      className={s.s434}
                    >
                      EXCLUDED
                    </span>
                  </span>
                  <span
                    className={s.s435}
                  >
                    TLS ends in an enclave, then a vendor reads the prompt.
                  </span>
                </div>
                <span
                  className={s.s436}
                >
                  none
                </span>
                <span
                  className={s.s437}
                >
                  read
                </span>
                <span
                  className={s.s438}
                >
                  PROMPT EXPOSURE
                </span>
                <div
                  className={s.s439}
                  style={{ '--d0': v.pkL, '--d1': v.pkO, '--d2': v.pkTr } as CSSVars}
                >
                  <span
                    className={s.s440}
                  >
                    {v.pkT}
                  </span>
                </div>
                <div
                  className={s.s441}
                  style={{ '--d0': v.ntT, '--d1': v.ntO, '--d2': v.ntTr } as CSSVars}
                />
                <div
                  className={s.s442}
                  style={{ '--d0': v.rpL, '--d1': v.rpO, '--d2': v.rpTr } as CSSVars}
                />
              </div>
            </div>
          </div>
        </div>
        <div
          className={s.s443}
        >
          <div
            className={s.s444}
          >
            <span
              className={s.s445}
            >
              {v.pN} / 07
            </span>
            <div
              className={s.s446}
            >
              <span
                className={s.s447}
              >
                {v.pTitle}
              </span>
              <span
                className={s.s448}
              >
                {v.pBody}
              </span>
            </div>
          </div>
          <span
            className={s.s449}
            style={{ '--d0': v.ppC, '--d1': v.ppB } as CSSVars}
          >
            <span
              className={s.s450}
              style={{ '--d0': v.ppD } as CSSVars}
            />
            {v.pPill}
          </span>
        </div>
        <div
          className={s.s451}
        >
          <button
            className={s.s452}
            onClick={v.pc0}
          >
            <span
              className={s.s453}
            >
              <span
                className={s.s454}
              >
                01
              </span>
              CALL
            </span>
            <span
              className={s.s455}
              aria-hidden="true"
              style={{ '--d0': v.pcf0, '--d1': v.pct0 } as CSSVars}
            >
              <span
                className={s.s456}
                style={{ '--d0': v.pgp0, '--d1': v.pgt0 } as CSSVars}
              />
              <span
                className={s.s457}
              >
                <span
                  className={s.s454}
                >
                  01
                </span>
                CALL
              </span>
            </span>
          </button>
          <button
            className={s.s452}
            onClick={v.pc1}
          >
            <span
              className={s.s453}
            >
              <span
                className={s.s454}
              >
                02
              </span>
              RESERVE
            </span>
            <span
              className={s.s458}
              aria-hidden="true"
              style={{ '--d0': v.pcf1, '--d1': v.pct1 } as CSSVars}
            >
              <span
                className={s.s459}
                style={{ '--d0': v.pgp1, '--d1': v.pgt1 } as CSSVars}
              />
              <span
                className={s.s457}
              >
                <span
                  className={s.s454}
                >
                  02
                </span>
                RESERVE
              </span>
            </span>
          </button>
          <button
            className={s.s452}
            onClick={v.pc2}
          >
            <span
              className={s.s453}
            >
              <span
                className={s.s454}
              >
                03
              </span>
              ESCROW
            </span>
            <span
              className={s.s460}
              aria-hidden="true"
              style={{ '--d0': v.pcf2, '--d1': v.pct2 } as CSSVars}
            >
              <span
                className={s.s461}
                style={{ '--d0': v.pgp2, '--d1': v.pgt2 } as CSSVars}
              />
              <span
                className={s.s457}
              >
                <span
                  className={s.s454}
                >
                  03
                </span>
                ESCROW
              </span>
            </span>
          </button>
          <button
            className={s.s452}
            onClick={v.pc3}
          >
            <span
              className={s.s453}
            >
              <span
                className={s.s454}
              >
                04
              </span>
              METER
            </span>
            <span
              className={s.s462}
              aria-hidden="true"
              style={{ '--d0': v.pcf3, '--d1': v.pct3 } as CSSVars}
            >
              <span
                className={s.s463}
                style={{ '--d0': v.pgp3, '--d1': v.pgt3 } as CSSVars}
              />
              <span
                className={s.s457}
              >
                <span
                  className={s.s454}
                >
                  04
                </span>
                METER
              </span>
            </span>
          </button>
          <button
            className={s.s452}
            onClick={v.pc4}
          >
            <span
              className={s.s453}
            >
              <span
                className={s.s454}
              >
                05
              </span>
              ROUTE
            </span>
            <span
              className={s.s464}
              aria-hidden="true"
              style={{ '--d0': v.pcf4, '--d1': v.pct4 } as CSSVars}
            >
              <span
                className={s.s465}
                style={{ '--d0': v.pgp4, '--d1': v.pgt4 } as CSSVars}
              />
              <span
                className={s.s457}
              >
                <span
                  className={s.s454}
                >
                  05
                </span>
                ROUTE
              </span>
            </span>
          </button>
          <button
            className={s.s452}
            onClick={v.pc5}
          >
            <span
              className={s.s453}
            >
              <span
                className={s.s454}
              >
                06
              </span>
              INFER
            </span>
            <span
              className={s.s466}
              aria-hidden="true"
              style={{ '--d0': v.pcf5, '--d1': v.pct5 } as CSSVars}
            >
              <span
                className={s.s467}
                style={{ '--d0': v.pgp5, '--d1': v.pgt5 } as CSSVars}
              />
              <span
                className={s.s457}
              >
                <span
                  className={s.s454}
                >
                  06
                </span>
                INFER
              </span>
            </span>
          </button>
          <button
            className={s.s452}
            onClick={v.pc6}
          >
            <span
              className={s.s453}
            >
              <span
                className={s.s454}
              >
                07
              </span>
              SETTLE
            </span>
            <span
              className={s.s468}
              aria-hidden="true"
              style={{ '--d0': v.pcf6, '--d1': v.pct6 } as CSSVars}
            >
              <span
                className={s.s469}
                style={{ '--d0': v.pgp6, '--d1': v.pgt6 } as CSSVars}
              />
              <span
                className={s.s457}
              >
                <span
                  className={s.s454}
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
