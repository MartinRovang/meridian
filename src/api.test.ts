import { beforeEach, expect, test, vi } from 'vitest'
import { configure, get } from './api'

beforeEach(() => {
  configure('http://127.0.0.1:9999', 'secret')
})

test('every request is absolute, so the api can live on another host', async () => {
  const fetchMock = vi.fn().mockResolvedValue(
    new Response('{"ok":true}', { status: 200, headers: { 'content-type': 'application/json' } }),
  )
  vi.stubGlobal('fetch', fetchMock)
  await get('/api/state')
  expect(fetchMock.mock.calls[0][0]).toBe('http://127.0.0.1:9999/api/state')
})

test('the token rides in a header, never in the url', async () => {
  const fetchMock = vi.fn().mockResolvedValue(
    new Response('{}', { status: 200, headers: { 'content-type': 'application/json' } }),
  )
  vi.stubGlobal('fetch', fetchMock)
  await get('/api/state')
  const init = fetchMock.mock.calls[0][1]
  expect(init.headers['X-Meridian-Token']).toBe('secret')
  expect(fetchMock.mock.calls[0][0]).not.toContain('secret')
})

test('a trailing slash on the base does not produce a double slash', async () => {
  configure('http://127.0.0.1:9999/', 'secret')
  const fetchMock = vi.fn().mockResolvedValue(
    new Response('{}', { status: 200, headers: { 'content-type': 'application/json' } }),
  )
  vi.stubGlobal('fetch', fetchMock)
  await get('/api/state')
  expect(fetchMock.mock.calls[0][0]).toBe('http://127.0.0.1:9999/api/state')
})

test('an error status surfaces the server message rather than a blank failure', async () => {
  vi.stubGlobal('fetch', vi.fn().mockResolvedValue(
    new Response('{"error":"bad token"}', { status: 401, headers: { 'content-type': 'application/json' } }),
  ))
  await expect(get('/api/state')).rejects.toThrow('bad token')
})

test('a network failure names the base that was tried', async () => {
  vi.stubGlobal('fetch', vi.fn().mockRejectedValue(new TypeError('failed to fetch')))
  await expect(get('/api/state')).rejects.toThrow('http://127.0.0.1:9999')
})
