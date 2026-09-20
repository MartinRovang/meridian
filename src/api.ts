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
    // Name the host that failed: with a remote server this is the difference between a useful
    // error and a spinner that never resolves.
    throw new Error(`cannot reach ${base}`)
  }
  const text = await res.text()
  const parsed = text ? (JSON.parse(text) as Record<string, unknown>) : {}
  if (!res.ok) throw new Error((parsed.error as string) || `${res.status}`)
  return parsed as T
}

export const get = <T,>(path: string) => call<T>('GET', path)
export const post = <T,>(path: string, body?: unknown) => call<T>('POST', path, body ?? {})
export const patch = <T,>(path: string, body: unknown) => call<T>('PATCH', path, body)
export const del = <T,>(path: string) => call<T>('DELETE', path)
