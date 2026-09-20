// The one place a network request is made. The base URL and token arrive at boot and are the
// only difference between talking to an in-process server and talking to a hosted one.

let base = ''
let token = ''

export function configure(url: string, apiToken: string) {
  base = url.replace(/\/+$/, '')
  token = apiToken
}

export function apiBase() {
  return base
}

/// The status always leads: "the server said no" and "the server is not there" are different
/// problems, and a bare message makes them look the same.
function fail(res: Response, parsed: Record<string, unknown>): string {
  return `${res.status}: ${(parsed.error as string) || res.statusText || 'request failed'}`
}

async function call<T>(method: string, path: string, body?: unknown): Promise<T> {
  const url = `${base}${path}`
  let res: Response
  try {
    res = await fetch(url, {
      method,
      headers: { 'X-Meridian-Token': token, 'Content-Type': 'application/json' },
      body: body === undefined ? undefined : JSON.stringify(body),
    })
  } catch {
    // No status exists here: fetch rejected before any reply. Printing the address would put a
    // token-bearing URL on the splash, and it tells a user nothing they can act on.
    throw new Error('cannot reach the server')
  }
  const text = await res.text()
  const parsed = text ? (JSON.parse(text) as Record<string, unknown>) : {}
  if (!res.ok) throw new Error(fail(res, parsed))
  return parsed as T
}

/// Send a file exactly as it sits on disk.
///
/// Not JSON: broker exports are commonly UTF-16, and a JSON string would destroy the encoding the
/// parser on the other end exists to cope with.
export async function postFile<T>(path: string, file: ArrayBuffer): Promise<T> {
  let res: Response
  try {
    res = await fetch(`${base}${path}`, {
      method: 'POST',
      headers: { 'X-Meridian-Token': token, 'Content-Type': 'application/octet-stream' },
      body: file,
    })
  } catch {
    // No status exists here: fetch rejected before any reply. Printing the address would put a
    // token-bearing URL on the splash, and it tells a user nothing they can act on.
    throw new Error('cannot reach the server')
  }
  const text = await res.text()
  const parsed = text ? (JSON.parse(text) as Record<string, unknown>) : {}
  if (!res.ok) throw new Error(fail(res, parsed))
  return parsed as T
}

export const get = <T,>(path: string) => call<T>('GET', path)
export const post = <T,>(path: string, body?: unknown) => call<T>('POST', path, body ?? {})
export const patch = <T,>(path: string, body: unknown) => call<T>('PATCH', path, body)
export const del = <T,>(path: string) => call<T>('DELETE', path)
