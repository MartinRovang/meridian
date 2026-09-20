/// One labelled number in a `.stat-row`. Two screens show these, and a second copy of six lines
/// is how two screens start rendering the same figure in two different sizes.

import type { ReactNode } from 'react'

export function Stat({
  label,
  value,
  tone,
}: {
  label: string
  /// A node rather than a string, so a figure with two ends can colour them separately.
  value: ReactNode
  tone?: string
}) {
  return (
    <div className="panel stat-cell">
      <div className="text-muted">{label}</div>
      <div className={`stat-value${tone ? ` ${tone}` : ''}`}>{value}</div>
    </div>
  )
}
