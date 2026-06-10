import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'
import { readFileSync } from 'node:fs'

function readBackendTarget() {
  if (process.env.KURIA_API_TARGET) return process.env.KURIA_API_TARGET
  if (process.env.VITE_API_TARGET) return process.env.VITE_API_TARGET

  try {
    const config = readFileSync(new URL('../config.toml', import.meta.url), 'utf8')
    const listenAddr = readTomlValue(config, 'web', 'listen_addr')
    if (listenAddr) return listenAddrToTarget(listenAddr)
  } catch {
    // Fall back to the default backend port below.
  }

  return 'http://localhost:8080'
}

function readTomlValue(content, section, key) {
  let currentSection = ''
  for (const rawLine of content.split(/\r?\n/)) {
    const line = rawLine.trim()
    if (!line || line.startsWith('#')) continue

    const sectionMatch = line.match(/^\[([^\]]+)\]$/)
    if (sectionMatch) {
      currentSection = sectionMatch[1]
      continue
    }

    if (currentSection !== section) continue

    const valueMatch = line.match(new RegExp(`^${key}\\s*=\\s*"([^"]+)"`))
    if (valueMatch) return valueMatch[1]
  }
  return ''
}

function listenAddrToTarget(listenAddr) {
  const normalized = listenAddr.trim()
  if (/^https?:\/\//.test(normalized)) return normalized

  const bracketedIpv6 = normalized.match(/^\[([^\]]+)\]:(\d+)$/)
  if (bracketedIpv6) {
    const host = bracketedIpv6[1] === '::' ? 'localhost' : `[${bracketedIpv6[1]}]`
    return `http://${host}:${bracketedIpv6[2]}`
  }

  const port = normalized.match(/:(\d+)$/)?.[1]
  if (!port) return 'http://localhost:8080'

  const host = normalized.slice(0, -(port.length + 1))
  const proxyHost = host === '0.0.0.0' || host === '::' || host === '' ? 'localhost' : host
  return `http://${proxyHost}:${port}`
}

const apiTarget = readBackendTarget()
console.info(`[kuria] proxy /api and /plugin-assets -> ${apiTarget}`)

function isKnownDependencyWarning(level, log) {
  if (level !== 'warn' || log?.code !== 'INVALID_ANNOTATION') return false

  const source = [log.id, log.loc?.file, log.message, log.frame]
    .filter(Boolean)
    .join('\n')
    .replaceAll('\\', '/')

  return source.includes('node_modules/@vueuse/core/dist/index.js')
}

export default defineConfig({
  plugins: [vue()],
  server: {
    host: '127.0.0.1',
    port: 3000,
    proxy: {
      '/api': {
        target: apiTarget,
        changeOrigin: true,
      },
      '/plugin-assets': {
        target: apiTarget,
        changeOrigin: true,
      },
    },
  },
  build: {
    outDir: '../static/dist',
    emptyOutDir: true,
    rollupOptions: {
      onLog(level, log, handler) {
        if (isKnownDependencyWarning(level, log)) return
        handler(level, log)
      },
    },
  },
})
