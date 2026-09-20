import { expect, test } from 'vitest'
import { money, pct, signed } from './format'

test('money is grouped and carries its currency', () => {
  expect(money(1234567.5, 'NOK')).toMatch(/1.234.568/)
  expect(money(1234567.5, 'NOK')).toContain('NOK')
})

test('percentages keep one decimal', () => {
  expect(pct(8.04)).toBe('8.0%')
  expect(pct(-2.349)).toBe('-2.3%')
})

test('a signed figure always shows its sign, and zero is not negative', () => {
  expect(signed(4.2)).toBe('+4.2')
  expect(signed(-4.2)).toBe('-4.2')
  expect(signed(0)).toBe('+0.0')
  expect(signed(-0)).toBe('+0.0')
})
