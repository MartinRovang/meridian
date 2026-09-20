/// A correlation matrix as a grid of shaded cells. No chart library: this is a table with a
/// background colour per cell, and the colour is one expression.

// Blue for moving apart, red for moving together, and the accent's own hue for neither. A
// diverging scale, because 0 is a meaningful middle here and not just a small number.
function shade(v: number): string {
  const x = Math.max(-1, Math.min(1, v))
  const hue = x >= 0 ? 4 : 205
  return `hsl(${hue} 60% 50% / ${(Math.abs(x) * 0.55).toFixed(2)})`
}

// The ticker without its exchange, because the column heads are narrow.
const short = (s: string) => s.replace(/\.[A-Z]+$/, '')

export function Corr({ symbols, matrix }: { symbols: string[]; matrix: number[][] }) {
  if (symbols.length < 2 || matrix.length !== symbols.length) return null
  return (
    <div className="corr-wrap">
      <table className="corr">
        <thead>
          <tr>
            <th />
            {symbols.map((s) => (
              <th key={s} title={s}>
                {short(s)}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {matrix.map((row, i) => (
            <tr key={symbols[i]}>
              <th title={symbols[i]}>{short(symbols[i])}</th>
              {row.map((v, j) => (
                <td
                  key={symbols[j]}
                  style={{ background: i === j ? 'transparent' : shade(v) }}
                  title={`${symbols[i]} vs ${symbols[j]}`}
                >
                  {i === j ? '' : v.toFixed(2)}
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  )
}
