<script setup>
import { ref, computed, onMounted, onUnmounted, watch } from 'vue'
import { useRouter, useRoute, RouterView } from 'vue-router'
import { MiuixButton, setThemeMode } from 'miuix-vue'
import { api } from '../api'

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
  { path: '/inbox', name: '邮箱', icon: '📬' },
  { path: '/compose', name: '写邮件', icon: '✏️' },
  { path: '/domains', name: '域名', icon: '🌐', admin: true },
  { path: '/users', name: '用户', icon: '👥', admin: true },
  { path: '/settings', name: '设置', icon: '⚙️', admin: true },
]

const themeOptions = [
  { id: 'system', name: '跟随', icon: '🖥️', title: '跟随系统' },
  { id: 'light', name: '亮色', icon: '☀️', title: '亮色模式' },
  { id: 'dark', name: '暗色', icon: '🌙', title: '暗色模式' },
]

const mailboxCounts = ref({})
const isDark = ref(false)
const themeMode = ref('system')
const sidebarOpen = ref(false)

const visibleNavItems = computed(() =>
  navItems.filter((item) => !item.admin || user.value.is_admin),
)

const totalUnread = computed(() =>
  Object.values(mailboxCounts.value).reduce((sum, mb) => sum + (mb?.unread || 0), 0),
)

const currentTitle = computed(() => {
  if (route.path.startsWith('/email/')) return '邮件详情'
  const item = navItems.find((nav) => isNavActive(nav))
  return item?.name || 'Kuria Mail'
})

let mediaQuery = null
let mediaHandler = null

async function loadMailboxCounts() {
  try {
    const data = await api.getMailboxCounts()
    mailboxCounts.value = data.mailboxes || {}
  } catch (e) {
    console.error(e)
  }
}

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
    applyTheme('system')
  }

  mediaQuery = window.matchMedia('(prefers-color-scheme: dark)')
  mediaHandler = (e) => {
    if (themeMode.value === 'system') {
      isDark.value = e.matches
    }
  }
  mediaQuery.addEventListener('change', mediaHandler)

  loadMailboxCounts()
})

onUnmounted(() => {
  if (mediaQuery && mediaHandler) {
    mediaQuery.removeEventListener('change', mediaHandler)
  }
})

// Reload counts when navigating to inbox
watch(() => route.path, (path) => {
  if (path === '/inbox' || path.startsWith('/email/')) {
    loadMailboxCounts()
  }
  sidebarOpen.value = false
})

function setThemePreference(mode) {
  if (mode === 'system') {
    localStorage.removeItem('theme')
  } else {
    localStorage.setItem('theme', mode)
  }
  applyTheme(mode)
}

function logout() {
  localStorage.removeItem('token')
  localStorage.removeItem('user')
  router.push('/login')
}

function isNavActive(item) {
  if (item.path === '/') return route.path === '/'
  if (item.path === '/inbox') return route.path === '/inbox' || route.path.startsWith('/email/')
  return route.path.startsWith(item.path)
}
</script>

<template>
  <div class="layout">
    <!-- Mobile header -->
    <div class="mobile-header">
      <MiuixButton class="menu-btn" title="打开导航" @click="sidebarOpen = !sidebarOpen">☰</MiuixButton>
      <span class="mobile-title">{{ currentTitle }}</span>
      <span v-if="totalUnread" class="mobile-badge">{{ totalUnread }}</span>
    </div>

    <!-- Overlay for mobile -->
    <div
      v-if="sidebarOpen"
      class="sidebar-overlay"
      @click="sidebarOpen = false"
    ></div>

    <!-- Sidebar -->
    <aside class="sidebar" :class="{ open: sidebarOpen }">
      <div class="sidebar-header">
        <div class="logo-icon">📧</div>
        <div>
          <span class="logo-text">Kuria Mail</span>
          <span class="logo-subtitle">Mail server console</span>
        </div>
      </div>

      <nav class="nav">
        <router-link
          v-for="item in visibleNavItems"
          :key="item.path"
          :to="item.path"
          class="nav-item"
          :class="{ active: isNavActive(item) }"
        >
          <span class="nav-icon">{{ item.icon }}</span>
          <span class="nav-label">{{ item.name }}</span>
          <span v-if="item.path === '/inbox' && totalUnread" class="nav-badge">{{ totalUnread }}</span>
        </router-link>
      </nav>

      <div class="sidebar-footer">
        <div class="user-info">
          <span class="user-avatar">👤</span>
          <span class="user-email">{{ user.email || '未登录' }}</span>
          <span v-if="user.is_admin" class="role-badge">Admin</span>
        </div>

        <div class="theme-toggle" aria-label="主题模式">
          <button
            v-for="option in themeOptions"
            :key="option.id"
            type="button"
            class="theme-option"
            :class="{ active: themeMode === option.id }"
            :title="option.title"
            :aria-pressed="themeMode === option.id"
            @click="setThemePreference(option.id)"
          >
            <span class="theme-icon">{{ option.icon }}</span>
            <span>{{ option.name }}</span>
          </button>
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

.mobile-header {
  display: none;
}

.sidebar-overlay {
  display: none;
}

.sidebar {
  width: 264px;
  background: var(--m-color-card);
  border-right: 1px solid var(--m-color-border);
  display: flex;
  flex-direction: column;
  position: fixed;
  top: 0;
  left: 0;
  bottom: 0;
  z-index: 10;
  overflow-y: auto;
}

.sidebar-header {
  padding: 22px 18px;
  display: flex;
  align-items: center;
  gap: 12px;
  border-bottom: 1px solid var(--m-color-border);
}

.logo-icon {
  width: 40px;
  height: 40px;
  border-radius: var(--app-radius);
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--m-color-bg);
  font-size: 22px;
  flex-shrink: 0;
}

.logo-text {
  display: block;
  font-size: 18px;
  font-weight: 600;
  color: var(--m-color-text);
}

.logo-subtitle {
  display: block;
  font-size: 11px;
  color: var(--m-color-text-secondary);
  margin-top: 2px;
}

.nav {
  padding: 12px 8px;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.nav-item {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 10px 12px;
  border-radius: var(--app-radius);
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

.nav-label {
  flex: 1;
}

.nav-badge,
.mobile-badge,
.role-badge {
  font-size: 11px;
  font-weight: 700;
  line-height: 1;
  border-radius: 999px;
}

.nav-badge,
.mobile-badge {
  background: var(--app-danger);
  color: white;
  padding: 4px 7px;
}

.sidebar-footer {
  margin-top: auto;
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
  border-radius: var(--app-radius);
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

.role-badge {
  background: color-mix(in srgb, var(--m-color-primary) 18%, transparent);
  color: var(--m-color-primary);
  padding: 4px 6px;
}

.theme-toggle {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 4px;
  padding: 4px;
  background: var(--m-color-bg);
  border-radius: var(--app-radius);
}

.theme-option {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 4px;
  min-width: 0;
  min-height: 34px;
  padding: 6px 4px;
  border: 0;
  border-radius: 6px;
  background: transparent;
  color: var(--m-color-text-secondary);
  cursor: pointer;
  font-size: 12px;
  transition: background 0.2s, color 0.2s;
}

.theme-option:hover {
  background: var(--m-color-hover);
  color: var(--m-color-text);
}

.theme-option.active {
  background: var(--m-color-primary);
  color: white;
}

.theme-icon {
  font-size: 14px;
  line-height: 1;
}

.logout-btn {
  width: 100%;
}

.main {
  flex: 1;
  margin-left: 264px;
  padding: 28px 36px;
  min-height: 100vh;
  min-width: 0;
}

/* Mobile responsive */
@media (max-width: 768px) {
  .mobile-header {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 12px 16px;
    background: var(--m-color-card);
    border-bottom: 1px solid var(--m-color-border);
    position: fixed;
    top: 0;
    left: 0;
    right: 0;
    z-index: 20;
  }

  .menu-btn {
    padding: 6px 10px;
    font-size: 20px;
  }

  .mobile-title {
    font-size: 16px;
    font-weight: 600;
    color: var(--m-color-text);
    flex: 1;
  }

  .sidebar-overlay {
    display: block;
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.5);
    z-index: 29;
  }

  .sidebar {
    transform: translateX(-100%);
    transition: transform 0.3s ease;
    z-index: 30;
  }

  .sidebar.open {
    transform: translateX(0);
  }

  .main {
    margin-left: 0;
    padding: 72px 16px 24px;
  }
}

@media (max-width: 480px) {
  .sidebar {
    width: min(88vw, 320px);
  }
}
</style>
