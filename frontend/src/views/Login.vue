<script setup>
import { ref } from 'vue'
import { useRouter } from 'vue-router'
import { MiuixButton, MiuixInput, MiuixCard } from 'miuix-vue'
import { api } from '../api'

const router = useRouter()
const email = ref('')
const password = ref('')
const error = ref('')
const loading = ref(false)

async function handleLogin() {
  error.value = ''
  loading.value = true
  try {
    const data = await api.login(email.value, password.value)
    localStorage.setItem('token', data.token)
    localStorage.setItem('user', JSON.stringify(data.user))
    router.push('/')
  } catch (e) {
    error.value = '登录失败：邮箱或密码错误'
  } finally {
    loading.value = false
  }
}
</script>

<template>
  <div class="login-page">
    <div class="login-card">
      <div class="logo">📧</div>
      <h1>Kuria Mail</h1>
      <p class="subtitle">轻量级自托管邮件服务器</p>

      <div class="form">
        <MiuixInput v-model="email" placeholder="邮箱地址" />
        <MiuixInput v-model="password" type="password" placeholder="密码" />

        <p v-if="error" class="error">{{ error }}</p>

        <MiuixButton
          type="primary"
          :disabled="loading"
          style="width: 100%; margin-top: 16px"
          @click="handleLogin"
        >
          {{ loading ? '登录中...' : '登录' }}
        </MiuixButton>
      </div>
    </div>
  </div>
</template>

<style scoped>
.login-page {
  min-height: 100vh;
  display: flex;
  align-items: center;
  justify-content: center;
  background: linear-gradient(135deg, #667eea 0%, #764ba2 100%);
}

.login-card {
  background: var(--m-color-card, #fff);
  border-radius: 16px;
  padding: 40px;
  width: 100%;
  max-width: 400px;
  box-shadow: 0 20px 60px rgba(0, 0, 0, 0.15);
}

.logo {
  font-size: 48px;
  text-align: center;
  margin-bottom: 16px;
}

h1 {
  text-align: center;
  color: var(--m-color-text, #1a1a1a);
  font-size: 24px;
  margin-bottom: 8px;
}

.subtitle {
  text-align: center;
  color: var(--m-color-text-secondary, #666);
  margin-bottom: 32px;
}

.form {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.error {
  color: #e74c3c;
  font-size: 14px;
  text-align: center;
}
</style>
