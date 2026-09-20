export function money(n: number, ccy: string): string {
  return new Intl.NumberFormat('nb-NO', {
    style: 'currency',
    currency: ccy,
    // nb-NO localises NOK to "kr" but leaves SEK and DKK as codes. In a table that mixes
    // currencies the inconsistency reads as a bug, so every currency shows its code.
    currencyDisplay: 'code',
    maximumFractionDigits: 0,
  }).format(n)
}

export function pct(n: number): string {
  return `${n.toFixed(1)}%`
}

export function signed(n: number): string {
  // Object.is catches -0, which would otherwise render as "-0.0".
  const v = Object.is(n, -0) ? 0 : n
  return `${v >= 0 ? '+' : ''}${v.toFixed(1)}`
}
