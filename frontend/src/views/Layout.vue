<script setup>
import { ref, computed, onMounted, onUnmounted, watch } from 'vue'
import { useRouter, useRoute, RouterView } from 'vue-router'
import { MiuixButton, MiuixSwitch, setThemeMode } from 'miuix-vue'
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
  { path: '/inbox', name: '收件箱', icon: '📥' },
  { path: '/compose', name: '写邮件', icon: '✏️' },
  { path: '/domains', name: '域名', icon: '🌐' },
  { path: '/users', name: '用户', icon: '👥' },
  { path: '/settings', name: '设置', icon: '⚙️' },
]

const mailboxList = [
  { id: 'INBOX', name: '收件箱', icon: '📥' },
  { id: 'Sent', name: '已发送', icon: '📤' },
  { id: 'Drafts', name: '草稿', icon: '📝' },
  { id: 'Trash', name: '垃圾箱', icon: '🗑️' },
  { id: 'Spam', name: '垃圾邮件', icon: '⚠️' },
]

const mailboxCounts = ref({})
const isDark = ref(false)
const themeMode = ref('system')
const sidebarOpen = ref(false)

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

function goToMailbox(id) {
  router.push({ path: '/inbox', query: { mailbox: id } })
}
</script>

<template>
  <div class="layout">
    <!-- Mobile header -->
    <div class="mobile-header">
      <MiuixButton class="menu-btn" @click="sidebarOpen = !sidebarOpen">☰</MiuixButton>
      <span class="mobile-title">Kuria Mail</span>
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

      <!-- Mailbox folders -->
      <div class="mailboxes">
        <div class="mailbox-title">邮箱文件夹</div>
        <div
          v-for="mb in mailboxList"
          :key="mb.id"
          class="mailbox-item"
          :class="{ active: route.query.mailbox === mb.id || (!route.query.mailbox && mb.id === 'INBOX' && route.path === '/inbox') }"
          @click="goToMailbox(mb.id)"
        >
          <span class="mailbox-icon">{{ mb.icon }}</span>
          <span class="mailbox-name">{{ mb.name }}</span>
          <span v-if="mailboxCounts[mb.id]?.unread" class="mailbox-badge">
            {{ mailboxCounts[mb.id].unread }}
          </span>
        </div>
      </div>

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

.mobile-header {
  display: none;
}

.sidebar-overlay {
  display: none;
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
  overflow-y: auto;
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

.mailboxes {
  padding: 8px;
  border-top: 1px solid var(--m-color-border);
}

.mailbox-title {
  font-size: 11px;
  font-weight: 600;
  color: var(--m-color-text-secondary);
  text-transform: uppercase;
  letter-spacing: 0.5px;
  padding: 8px 16px 4px;
}

.mailbox-item {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 8px 16px;
  border-radius: 8px;
  cursor: pointer;
  font-size: 13px;
  color: var(--m-color-text-secondary);
  transition: all 0.2s;
}

.mailbox-item:hover {
  background: var(--m-color-hover);
  color: var(--m-color-text);
}

.mailbox-item.active {
  background: var(--m-color-primary);
  color: white;
}

.mailbox-icon {
  font-size: 16px;
  width: 20px;
  text-align: center;
}

.mailbox-name {
  flex: 1;
}

.mailbox-badge {
  font-size: 11px;
  font-weight: 600;
  background: #e74c3c;
  color: white;
  padding: 1px 6px;
  border-radius: 10px;
  min-width: 18px;
  text-align: center;
}

.mailbox-item.active .mailbox-badge {
  background: rgba(255, 255, 255, 0.3);
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
</style>
