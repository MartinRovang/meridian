/// The two questions the Stress test screen asks: how bad has it been, and how bad could it get.
/// Both are arithmetic over numbers the app already has, so neither invents a risk model.

import type { Point } from './stats'

export type Window = { pct: number; from: string; to: string }

/// The worst move across any `span` consecutive points, with the days it ran between.
///
/// Not the worst single day inside the window: a slide that takes three days to lose 15% never
/// shows up in a daily figure, and it is the slide that empties an account.
export function worstWindow(points: Point[], span: number): Window | null {
  if (points.length <= span) return null
  let worst: Window | null = null
  for (let i = 0; i + span < points.length; i++) {
    const pct = (points[i + span].index / points[i].index - 1) * 100
    if (!worst || pct < worst.pct) worst = { pct, from: points[i].day, to: points[i + span].day }
  }
  return worst
}

/// A holding reduced to what a shock needs: what it is worth, and whether that worth is foreign.
export type Exposure = { name: string; value: number; currency: string; priced: boolean }

export type Shock = {
  before: number
  after: number
  loss: number
  /// Value held in something other than the base currency, which is what the FX shock moves.
  foreign: number
  /// Named, not counted: a holding with no quote is missing from both figures.
  unpriced: string[]
}

/// Apply a uniform market fall and a fall in every foreign currency against the base one.
///
/// ponytail: no betas. Estimating each holding's sensitivity to "the market" from five years of
/// prices would produce a number to two decimal places that is mostly noise, and it would hide
/// the honest part of this screen, which is that the user chose the shock.
export function shock(
  holdings: Exposure[],
  base: string,
  marketPct: number,
  fxPct: number,
): Shock {
  const m = 1 - marketPct / 100
  const f = 1 - fxPct / 100
  let before = 0
  let after = 0
  let foreign = 0
  const unpriced: string[] = []
  for (const h of holdings) {
    if (!h.priced) {
      unpriced.push(h.name)
      continue
    }
    before += h.value
    const abroad = h.currency !== base
    if (abroad) foreign += h.value
    after += h.value * m * (abroad ? f : 1)
  }
  return { before, after, loss: after - before, foreign, unpriced }
}
