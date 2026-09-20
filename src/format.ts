export function money(n: number, ccy: string, dp = 0): string {
  return new Intl.NumberFormat('nb-NO', {
    style: 'currency',
    currency: ccy,
    // nb-NO localises NOK to "kr" but leaves SEK and DKK as codes. In a table that mixes
    // currencies the inconsistency reads as a bug, so every currency shows its code.
    currencyDisplay: 'code',
    minimumFractionDigits: dp,
    maximumFractionDigits: dp,
  }).format(n)
}

/// A share price, unlike a total, is meaningless rounded to whole units: a 2.35 krone stock
/// would read as 2, and a move from 418.50 to 419.00 would not show at all.
export const price = (n: number, ccy: string) => money(n, ccy, 2)

export function pct(n: number): string {
  return `${n.toFixed(1)}%`
}

export function signed(n: number): string {
  // Object.is catches -0, which would otherwise render as "-0.0".
  const v = Object.is(n, -0) ? 0 : n
  return `${v >= 0 ? '+' : ''}${v.toFixed(1)}`
}
