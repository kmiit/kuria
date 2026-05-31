<script setup>
import { ref, computed } from 'vue'
import { useRouter, useRoute, RouterView } from 'vue-router'
import { MiuixButton, setThemeMode } from 'miuix-vue'

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

function toggleTheme() {
  isDark.value = !isDark.value
  setThemeMode(isDark.value ? 'dark' : 'light')
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
        <div class="footer-actions">
          <MiuixButton @click="toggleTheme" style="flex: 1">
            {{ isDark ? '☀️' : '🌙' }}
          </MiuixButton>
          <MiuixButton @click="logout" style="flex: 1">退出</MiuixButton>
        </div>
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
}

.sidebar {
  width: 240px;
  background: var(--m-color-card, #fff);
  border-right: 1px solid var(--m-color-border, #e0e0e0);
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
  border-bottom: 1px solid var(--m-color-border, #e0e0e0);
}

.logo-icon {
  font-size: 28px;
}

.logo-text {
  font-size: 18px;
  font-weight: 600;
  color: var(--m-color-text, #1a1a1a);
}

.nav {
  flex: 1;
  padding: 12px 8px;
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.nav-item {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 12px 16px;
  border-radius: 10px;
  text-decoration: none;
  color: var(--m-color-text-secondary, #666);
  font-size: 14px;
  transition: all 0.2s;
}

.nav-item:hover {
  background: var(--m-color-hover, #f5f5f5);
  color: var(--m-color-text, #1a1a1a);
}

.nav-item.active {
  background: var(--m-color-primary, #4a90d9);
  color: white;
}

.nav-icon {
  font-size: 18px;
}

.sidebar-footer {
  padding: 16px;
  border-top: 1px solid var(--m-color-border, #e0e0e0);
}

.user-info {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 12px;
  padding: 8px;
  background: var(--m-color-bg, #f5f5f5);
  border-radius: 8px;
}

.user-avatar {
  font-size: 20px;
}

.user-email {
  font-size: 13px;
  color: var(--m-color-text, #1a1a1a);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.footer-actions {
  display: flex;
  gap: 8px;
}

.main {
  flex: 1;
  margin-left: 240px;
  padding: 24px;
  min-height: 100vh;
}
</style>
