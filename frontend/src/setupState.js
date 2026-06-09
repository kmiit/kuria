let initializedCache = null

export function setInitialized(value) {
  initializedCache = Boolean(value)
}

export function resetInitialized() {
  initializedCache = null
}

export async function checkInitialized() {
  if (initializedCache !== null) return initializedCache

  try {
    const res = await fetch('/api/setup/status')
    if (!res.ok) throw new Error(`HTTP ${res.status}`)
    const data = await res.json()
    initializedCache = Boolean(data.initialized)
    return initializedCache
  } catch {
    return true
  }
}
