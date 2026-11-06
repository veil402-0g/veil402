import type { CSSVars } from '../ui/css-vars'
import type { LandingBindings } from './useLandingBindings'
import s from './landing.module.css'

export function Hero({ v }: { v: LandingBindings }) {
  return (
    <>
  <div
    className={s.s010}
  >
    <div
      className={s.s011}
      onMouseMove={v.onMove}
      onMouseEnter={v.onEnter}
      onMouseLeave={v.onLeave}
    >
      <div
        className={s.s012}
        aria-hidden="true"
      />
      <div
        className={s.s013}
        aria-hidden="true"
      />
      <div
        className={s.s014}
        aria-hidden="true"
      />
      <div
        className={s.s015}
        aria-hidden="true"
      />
      <canvas
        className={s.s016}
        ref={v.canvasRef}
        aria-hidden="true"
      />
      <div
        className={s.s017}
        aria-hidden="true"
      />
      <div
        className={s.s018}
        aria-hidden="true"
      />
      <div
        className={s.s019}
        aria-hidden="true"
      />
      <div
        className={s.s020}
        data-par="a"
      >
        <span
          className={s.s021}
          aria-hidden="true"
          style={{ '--d0': v.hDot } as CSSVars}
        >
          <span
            className={s.s005}
          />
        </span>
        <div
          className={s.s022}
          data-h3d="1"
        >
          <span
            className={s.s023}
            aria-hidden="true"
            style={{ '--d0': v.dispFont, '--d1': v.dispTrack, '--d2': v.hOp } as CSSVars}
          >
            Veil402
          </span>
          <span
            className={s.s024}
            aria-hidden="true"
            style={{ '--d0': v.dispFont, '--d1': v.dispTrack, '--d2': v.hOp } as CSSVars}
          >
            Veil402
          </span>
          <span
            className={s.s025}
            aria-hidden="true"
            style={{ '--d0': v.dispFont, '--d1': v.dispTrack, '--d2': v.hOp } as CSSVars}
          >
            Veil402
          </span>
          <span
            className={s.s026}
            aria-hidden="true"
            style={{ '--d0': v.dispFont, '--d1': v.dispTrack, '--d2': v.hOp } as CSSVars}
          >
            Veil402
          </span>
          <h1
            className={s.s027}
            style={{ '--d0': v.dispFont, '--d1': v.dispTrack } as CSSVars}
          >
            <span
              className={s.s028}
            >
              <span
                className={s.s029}
                style={{ '--d0': v.hLet, '--d1': v.hOp } as CSSVars}
              >
                V
              </span>
            </span>
            <span
              className={s.s028}
            >
              <span
                className={s.s030}
                style={{ '--d0': v.hLet, '--d1': v.hOp } as CSSVars}
              >
                e
              </span>
            </span>
            <span
              className={s.s028}
            >
              <span
                className={s.s031}
                style={{ '--d0': v.hLet, '--d1': v.hOp } as CSSVars}
              >
                i
              </span>
            </span>
            <span
              className={s.s028}
            >
              <span
                className={s.s032}
                style={{ '--d0': v.hLet, '--d1': v.hOp } as CSSVars}
              >
                l
              </span>
            </span>
            <span
              className={s.s028}
            >
              <span
                className={s.s033}
                style={{ '--d0': v.hLet, '--d1': v.hOp } as CSSVars}
              >
                4
              </span>
            </span>
            <span
              className={s.s028}
            >
              <span
                className={s.s034}
                style={{ '--d0': v.hLet, '--d1': v.hOp } as CSSVars}
              >
                0
              </span>
            </span>
            <span
              className={s.s028}
            >
              <span
                className={s.s035}
                style={{ '--d0': v.hLet, '--d1': v.hOp } as CSSVars}
              >
                2
              </span>
            </span>
          </h1>
        </div>
      </div>
      <div
        className={s.s036}
        data-par="b"
      >
        <div
          className={s.s037}
        >
          <span
            className={s.s038}
            style={{ '--d0': v.hOp } as CSSVars}
          >
            <span
              className={s.s039}
              style={{ '--d0': v.hSq } as CSSVars}
            />
            PRIVATE AI INFERENCE
          </span>
          <h2
            className={s.s040}
          >
            <span
              className={s.s041}
            >
              <span
                className={s.s042}
                style={{ '--d0': v.hLine } as CSSVars}
              >
                Private payments–
              </span>
            </span>
            <span
              className={s.s041}
            >
              <span
                className={s.s043}
                style={{ '--d0': v.hLine } as CSSVars}
              >
                Verifiable execution
              </span>
            </span>
          </h2>
        </div>
        <div
          className={s.s044}
        >
          <p
            className={s.s045}
            style={{ '--d0': v.hOp, '--d1': v.hUp } as CSSVars}
          >
            402-powered AI infrastructure that keeps your data{' '}
            <span
              className={s.s046}
              style={{ '--d0': `${v.hHp1},0 100%`, '--d1': `${v.hHl1} 100%,${v.hHl1} 1.5px` } as CSSVars}
            >
              private
            </span>
            <span>
              , makes execution{' '}
            </span>
            <span
              className={s.s047}
              style={{ '--d0': `${v.hHp2},0 100%`, '--d1': `${v.hHl2} 100%,${v.hHl2} 1.5px` } as CSSVars}
            >
              verifiable
            </span>
            <span>
              , and enables{' '}
            </span>
            <span
              className={s.s048}
              style={{ '--d0': `${v.hHp3},0 100%`, '--d1': `${v.hHl3} 100%,${v.hHl3} 1.5px` } as CSSVars}
            >
              shielded, escrow-backed payments
            </span>
            <span>
              {' '}with{' '}
            </span>
            <span
              className={s.s049}
              style={{ '--d0': `${v.hHp4},0 100%`, '--d1': `${v.hHl4} 100%,${v.hHl4} 1.5px` } as CSSVars}
            >
              selective auditability
            </span>
            <span>
              .
            </span>
          </p>
          <div
            className={s.s050}
            style={{ '--d0': v.hOp, '--d1': v.hUp } as CSSVars}
          >
            <span
              className={s.s051}
            >
              $
            </span>
            <code
              className={s.s052}
            >
              <span
                className={s.s053}
                aria-hidden="true"
              >
                curl -fsSL https://veil402.xyz/install.sh | bash
              </span>
              <span
                className={s.s054}
              >
                <span>
                  {v.t1}
                </span>
                <span
                  className={s.s051}
                >
                  {v.t2}
                </span>
                <span>
                  {v.t3}
                </span>
                <span
                  className={s.s051}
                >
                  {v.t4}
                </span>
                <span>
                  {v.t5}
                </span>
                {v.tcursor}
              </span>
            </code>
            <button
              className={s.s055}
              onClick={v.copy}
              aria-label="Copy command"
              title="Copy"
            >
              {v.notCopied ? (
                <>
                <svg
                  width="15"
                  height="15"
                  viewBox="0 0 16 16"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="1.4"
                >
                  <rect
                    x="5.5"
                    y="5.5"
                    width="8"
                    height="8"
                  />
                  <path
                    d="M10.5 5.5V2.5H2.5V10.5H5.5"
                  />
                </svg>
                </>
              ) : null}
              {v.copied ? (
                <>
                <svg
                  width="15"
                  height="15"
                  viewBox="0 0 16 16"
                  fill="none"
                  stroke="#fff"
                  strokeWidth="1.6"
                >
                  <path
                    d="M3 8.5L6.5 12L13 4.5"
                  />
                </svg>
                </>
              ) : null}
            </button>
          </div>
          <div
            className={s.s056}
            style={{ '--d0': v.hOp, '--d1': v.hUp } as CSSVars}
          >
            <a
              className={s.s057}
              href="/console"
              onClick={v.openConsole}
              onMouseEnter={v.h1on}
              onMouseLeave={v.h1off}
            >
              Open the console
              <span
                className={s.s058}
              >
                <span
                  className={s.s059}
                  style={{ '--d0': v.a1 } as CSSVars}
                >
                  <span
                    className={s.s060}
                  >
                    ↗
                  </span>
                  <span
                    className={s.s060}
                  >
                    ↗
                  </span>
                </span>
              </span>
            </a>
            <a
              className={s.s057}
              href="https://github.com/veil402-0g/veil402"
              target="_blank"
              rel="noopener"
              onMouseEnter={v.h2on}
              onMouseLeave={v.h2off}
            >
              View source
              <span
                className={s.s058}
              >
                <span
                  className={s.s061}
                  style={{ '--d0': v.a2 } as CSSVars}
                >
                  <span
                    className={s.s060}
                  >
                    ↗
                  </span>
                  <span
                    className={s.s060}
                  >
                    ↗
                  </span>
                </span>
              </span>
            </a>
          </div>
        </div>
      </div>
    </div>
  </div>
    </>
  )
}
