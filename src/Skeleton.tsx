// ponytail: one primitive and three shapes. A skeleton is a grey box that pulses, which is nine
// lines of CSS; a library for it would be tens of kilobytes to avoid writing them.
//
// A skeleton stands in for data that is NOT THERE YET. It is never shown over numbers that are
// already on screen: replacing a real total with a grey bar during a refresh tells the user their
// money vanished for a second. Refreshes keep the stale figure and say so in the banner.

const px = (v: number | string) => (typeof v === 'number' ? `${v}px` : v)

export function Skel({ w = '100%', h = 13 }: { w?: number | string; h?: number }) {
  return <span className="skel" style={{ width: px(w), height: px(h) }} />
}

/// Shaped like `.trade` so the panel does not jump when real rows replace it.
export function SkelTrades({ n = 3 }: { n?: number }) {
  return (
    <div className="legend" aria-busy="true" aria-label="Working out trades">
      {Array.from({ length: n }, (_, i) => (
        <div key={i} className="trade">
          <Skel w={34} />
          <Skel w={54} />
          <Skel w={`${60 - i * 8}%`} />
          <Skel w={88} />
        </div>
      ))}
    </div>
  )
}

/// Shaped like `.hit`. Three rows: enough to read as a list, few enough that a one-hit answer
/// does not collapse the dropdown dramatically.
export function SkelHits({ n = 3 }: { n?: number }) {
  return (
    <div aria-busy="true" aria-label="Searching">
      {Array.from({ length: n }, (_, i) => (
        <div key={i} className="hit">
          <Skel w={72} />
          <Skel w={`${70 - i * 15}%`} />
          <Skel w={40} />
        </div>
      ))}
    </div>
  )
}

/// The whole work area, for the window between mount and the first state landing. The splash
/// covers this on a cold boot; this covers a reload that empties the store mid-session.
export function SkelScreen() {
  return (
    <div className="screen" aria-busy="true" aria-label="Loading">
      <div className="stat-band">
        <div className="stat-head">
          <Skel w={220} h={30} />
          <Skel w={110} />
          <Skel w={140} />
        </div>
      </div>
      <section className="panel">
        <div className="panel-head">
          <Skel w={130} />
        </div>
        <div className="legend">
          {Array.from({ length: 5 }, (_, i) => (
            <div key={i} className="legend-row">
              <Skel w={9} h={9} />
              <Skel w={`${72 - i * 9}%`} />
              <Skel w={52} />
              <Skel w={96} />
            </div>
          ))}
        </div>
      </section>
    </div>
  )
}
