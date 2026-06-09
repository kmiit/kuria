<script setup>
import { ref } from 'vue'
import { useRouter } from 'vue-router'
import { MiuixButton, MiuixInput } from 'miuix-vue'
import { api } from '../api'
import PasswordInput from '../components/PasswordInput.vue'

const router = useRouter()
const email = ref('')
const password = ref('')
const error = ref('')
const loading = ref(false)

async function handleLogin() {
  error.value = ''
  if (!email.value || !password.value) {
    error.value = '请输入邮箱和密码'
    return
  }
  loading.value = true
  try {
    const data = await api.login(email.value, password.value)
    localStorage.setItem('token', data.token)
    localStorage.setItem('user', JSON.stringify(data.user))
    router.push('/')
  } catch {
    error.value = '登录失败：邮箱或密码错误'
  } finally {
    loading.value = false
  }
}
</script>

<template>
  <div class="login-page">
    <div class="brand-panel">
      <div class="logo">📧</div>
      <div>
        <h1>Kuria Mail</h1>
        <p>轻量级自托管邮件服务器</p>
      </div>
    </div>

    <div class="login-card">
      <div class="form-head">
        <h2>登录</h2>
        <p>进入邮件管理界面</p>
      </div>

      <div class="form">
        <MiuixInput v-model="email" placeholder="邮箱地址" @keyup.enter="handleLogin" />
        <PasswordInput v-model="password" placeholder="密码" @keyup-enter="handleLogin" />

        <p v-if="error" class="error">{{ error }}</p>

        <MiuixButton
          type="primary"
          :disabled="loading"
          class="login-button"
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
  display: grid;
  grid-template-columns: minmax(280px, 0.75fr) minmax(320px, 1fr);
  background:
    linear-gradient(135deg, rgba(15, 118, 110, 0.92), rgba(64, 81, 59, 0.9) 52%, rgba(183, 121, 31, 0.88)),
    var(--m-color-bg);
}

.brand-panel {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 16px;
  padding: 40px;
  color: white;
}

.logo {
  width: 58px;
  height: 58px;
  border-radius: var(--app-radius);
  background: rgba(255, 255, 255, 0.18);
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 32px;
  flex-shrink: 0;
}

h1 {
  font-size: 30px;
  line-height: 1.1;
  margin-bottom: 8px;
}

.brand-panel p {
  color: rgba(255, 255, 255, 0.82);
}

.login-card {
  align-self: center;
  justify-self: center;
  background: var(--m-color-card, #fff);
  border-radius: var(--app-radius);
  padding: 36px;
  width: min(400px, calc(100vw - 32px));
  box-shadow: var(--app-shadow);
}

.form-head {
  margin-bottom: 28px;
}

.form-head h2 {
  color: var(--m-color-text, #1a1a1a);
  font-size: 24px;
  margin-bottom: 6px;
}

.form-head p {
  color: var(--m-color-text-secondary, #666);
  font-size: 14px;
}

.form {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.error {
  color: var(--app-danger);
  font-size: 14px;
}

.login-button {
  width: 100%;
  margin-top: 8px;
}

@media (max-width: 760px) {
  .login-page {
    grid-template-columns: 1fr;
    align-content: center;
    gap: 24px;
    padding: 24px 16px;
  }

  .brand-panel {
    padding: 0;
    justify-content: flex-start;
  }

  .login-card {
    width: 100%;
  }
}
</style>
