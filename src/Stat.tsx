/// One labelled number in a `.stat-row`. Two screens show these, and a second copy of six lines
/// is how two screens start rendering the same figure in two different sizes.
export function Stat({ label, value, tone }: { label: string; value: string; tone?: string }) {
  return (
    <div className="panel stat-cell">
      <div className="text-muted">{label}</div>
      <div className={`stat-value${tone ? ` ${tone}` : ''}`}>{value}</div>
    </div>
  )
}
