import { createApp } from 'vue'
import { createRouter, createWebHistory } from 'vue-router'
import App from './App.vue'
import { setThemeMode } from 'miuix-vue'
import 'miuix-vue/style.css'
import { checkInitialized } from './setupState'

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
        { path: 'email/:id', name: 'email-detail', component: () => import('./views/EmailDetail.vue') },
        { path: 'compose', name: 'compose', component: () => import('./views/Compose.vue') },
        { path: 'domains', name: 'domains', component: () => import('./views/Domains.vue'), meta: { admin: true } },
        { path: 'users', name: 'users', component: () => import('./views/Users.vue'), meta: { admin: true } },
        { path: 'settings', name: 'settings', component: () => import('./views/Settings.vue'), meta: { admin: true } },
      ],
    },
  ],
})

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
  if (token && to.meta.admin) {
    try {
      const user = JSON.parse(localStorage.getItem('user') || '{}')
      if (!user.is_admin) return { name: 'dashboard' }
    } catch {
      return { name: 'dashboard' }
    }
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
