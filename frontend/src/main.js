import { createApp } from 'vue'
import { createRouter, createWebHistory } from 'vue-router'
import App from './App.vue'
import { setThemeMode } from 'miuix-vue'
import 'miuix-vue/style.css'

// Router
const router = createRouter({
  history: createWebHistory(),
  routes: [
    { path: '/setup', name: 'setup', component: () => import('./views/Setup.vue') },
    { path: '/login', name: 'login', component: () => import('./views/Login.vue') },
    {
      path: '/',
      name: 'home',
      component: () => import('./views/Layout.vue'),
      children: [
        { path: '', name: 'dashboard', component: () => import('./views/Dashboard.vue') },
        { path: 'inbox', name: 'inbox', component: () => import('./views/Inbox.vue') },
        { path: 'compose', name: 'compose', component: () => import('./views/Compose.vue') },
        { path: 'domains', name: 'domains', component: () => import('./views/Domains.vue') },
        { path: 'users', name: 'users', component: () => import('./views/Users.vue') },
      ],
    },
  ],
})

// Check if system is initialized
let initialized = null

async function checkInitialized() {
  if (initialized !== null) return initialized
  try {
    const res = await fetch('/api/setup/status')
    const data = await res.json()
    initialized = data.initialized
    return initialized
  } catch {
    return true // Assume initialized on error
  }
}

// Auth guard with setup check
router.beforeEach(async (to) => {
  const isInit = await checkInitialized()

  // If already initialized, block access to setup page
  if (to.name === 'setup') {
    if (isInit) {
      const token = localStorage.getItem('token')
      return { name: token ? 'dashboard' : 'login' }
    }
    return
  }

  // If not initialized, redirect to setup
  if (!isInit) {
    return { name: 'setup' }
  }

  // Check auth for protected routes
  const token = localStorage.getItem('token')
  if (!token && to.name !== 'login') {
    return { name: 'login' }
  }
  if (token && to.name === 'login') {
    return { name: 'dashboard' }
  }
})

// Restore theme from localStorage
const savedTheme = localStorage.getItem('theme')
if (savedTheme === 'dark') {
  setThemeMode('dark')
} else if (savedTheme === 'light') {
  setThemeMode('light')
} else {
  setThemeMode('system')
}

const app = createApp(App)
app.use(router)
app.mount('#app')
