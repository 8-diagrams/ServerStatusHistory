export interface ServerDetailRoute {
  name: string
  gid?: string
}

export function serverDetailHref(name: string, gid?: string) {
  const params = new URLSearchParams()
  if (gid !== undefined)
    params.set('gid', gid)
  const query = params.toString()
  return `#/server/${encodeURIComponent(name)}${query ? `?${query}` : ''}`
}

export function parseServerDetailRoute(hash: string): ServerDetailRoute | undefined {
  const route = hash.replace(/^#\/?/, '')
  const [path, query = ''] = route.split('?', 2)
  if (!path.startsWith('server/'))
    return undefined
  const encodedName = path.slice('server/'.length)
  if (!encodedName || encodedName.includes('/'))
    return undefined
  try {
    const name = decodeURIComponent(encodedName)
    if (!name)
      return undefined
    const params = new URLSearchParams(query)
    const gid = params.has('gid') ? params.get('gid') || '' : undefined
    return { name, gid }
  }
  catch {
    return undefined
  }
}
