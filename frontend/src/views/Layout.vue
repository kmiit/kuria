<script setup>
import { ref, computed, onMounted, onUnmounted } from 'vue'
import { useRouter, useRoute, RouterView } from 'vue-router'
import { MiuixButton, MiuixSwitch, setThemeMode } from 'miuix-vue'

const router = useRouter()
const route = useRoute()

const user = computed(() => {
  try {
    return JSON.parse(localStorage.getItem('user') || '{}')
  } catch {
    return {}
  }
})

const navItems = [
  { path: '/', name: '仪表盘', icon: '📊' },
  { path: '/inbox', name: '收件箱', icon: '📥' },
  { path: '/compose', name: '写邮件', icon: '✏️' },
  { path: '/domains', name: '域名', icon: '🌐' },
  { path: '/users', name: '用户', icon: '👥' },
]

const isDark = ref(false)
const themeMode = ref('system') // 'system' | 'light' | 'dark'

let mediaQuery = null
let mediaHandler = null

function applyTheme(mode) {
  themeMode.value = mode
  if (mode === 'system') {
    const prefersDark = window.matchMedia('(prefers-color-scheme: dark)').matches
    isDark.value = prefersDark
    setThemeMode('system')
  } else {
    isDark.value = mode === 'dark'
    setThemeMode(mode)
  }
}

onMounted(() => {
  const saved = localStorage.getItem('theme')
  if (saved === 'dark' || saved === 'light') {
    applyTheme(saved)
  } else {
    // Default: follow system
    applyTheme('system')
  }

  // Listen for system theme changes
  mediaQuery = window.matchMedia('(prefers-color-scheme: dark)')
  mediaHandler = (e) => {
    if (themeMode.value === 'system') {
      isDark.value = e.matches
    }
  }
  mediaQuery.addEventListener('change', mediaHandler)
})

onUnmounted(() => {
  if (mediaQuery && mediaHandler) {
    mediaQuery.removeEventListener('change', mediaHandler)
  }
})

function onThemeChange(val) {
  isDark.value = val
  const mode = val ? 'dark' : 'light'
  themeMode.value = mode
  setThemeMode(mode)
  localStorage.setItem('theme', mode)
}

function resetTheme() {
  localStorage.removeItem('theme')
  applyTheme('system')
}

function logout() {
  localStorage.removeItem('token')
  localStorage.removeItem('user')
  router.push('/login')
}
</script>

<template>
  <div class="layout">
    <!-- Sidebar -->
    <aside class="sidebar">
      <div class="sidebar-header">
        <span class="logo-icon">📧</span>
        <span class="logo-text">Kuria Mail</span>
      </div>

      <nav class="nav">
        <router-link
          v-for="item in navItems"
          :key="item.path"
          :to="item.path"
          class="nav-item"
          :class="{ active: route.path === item.path }"
        >
          <span class="nav-icon">{{ item.icon }}</span>
          <span class="nav-label">{{ item.name }}</span>
        </router-link>
      </nav>

      <div class="sidebar-footer">
        <div class="user-info">
          <span class="user-avatar">👤</span>
          <span class="user-email">{{ user.email }}</span>
        </div>

        <div class="theme-toggle">
          <div class="theme-left" @click="resetTheme" title="跟随系统">
            <span class="theme-icon">{{ isDark ? '🌙' : '☀️' }}</span>
            <span class="theme-label">{{ themeMode === 'system' ? '跟随系统' : (isDark ? '暗色' : '亮色') }}</span>
          </div>
          <MiuixSwitch :modelValue="isDark" @update:modelValue="onThemeChange" />
        </div>

        <MiuixButton class="logout-btn" @click="logout">退出登录</MiuixButton>
      </div>
    </aside>

    <!-- Main Content -->
    <main class="main">
      <RouterView />
    </main>
  </div>
</template>

<style scoped>
.layout {
  display: flex;
  min-height: 100vh;
  background: var(--m-color-bg);
}

.sidebar {
  width: 240px;
  background: var(--m-color-card);
  border-right: 1px solid var(--m-color-border);
  display: flex;
  flex-direction: column;
  position: fixed;
  top: 0;
  left: 0;
  bottom: 0;
  z-index: 10;
}

.sidebar-header {
  padding: 20px;
  display: flex;
  align-items: center;
  gap: 12px;
  border-bottom: 1px solid var(--m-color-border);
}

.logo-icon {
  font-size: 28px;
}

.logo-text {
  font-size: 18px;
  font-weight: 600;
  color: var(--m-color-text);
}

.nav {
  flex: 1;
  padding: 12px 8px;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.nav-item {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 10px 16px;
  border-radius: 10px;
  text-decoration: none;
  color: var(--m-color-text-secondary);
  font-size: 14px;
  transition: all 0.2s;
}

.nav-item:hover {
  background: var(--m-color-hover);
  color: var(--m-color-text);
}

.nav-item.active {
  background: var(--m-color-primary);
  color: white;
}

.nav-icon {
  font-size: 18px;
  width: 24px;
  text-align: center;
}

.sidebar-footer {
  padding: 12px;
  border-top: 1px solid var(--m-color-border);
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.user-info {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 10px 12px;
  background: var(--m-color-bg);
  border-radius: 10px;
}

.user-avatar {
  font-size: 18px;
}

.user-email {
  font-size: 13px;
  color: var(--m-color-text);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  flex: 1;
}

.theme-toggle {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 8px 12px;
  background: var(--m-color-bg);
  border-radius: 10px;
}

.theme-left {
  display: flex;
  align-items: center;
  gap: 6px;
  cursor: pointer;
}

.theme-icon {
  font-size: 16px;
}

.theme-label {
  font-size: 13px;
  color: var(--m-color-text);
}

.logout-btn {
  width: 100%;
}

.main {
  flex: 1;
  margin-left: 240px;
  padding: 24px 32px;
  min-height: 100vh;
}
</style>
