<script setup>
import { ref, onMounted } from 'vue'
import { MiuixButton, MiuixInput, MiuixCard } from 'miuix-vue'
import { api } from '../api'

const settings = ref(null)
const loading = ref(true)

// Password change
const oldPassword = ref('')
const newPassword = ref('')
const confirmPassword = ref('')
const changingPassword = ref(false)
const passwordResult = ref('')

async function loadSettings() {
  loading.value = true
  try {
    settings.value = await api.getSettings()
  } catch (e) {
    console.error(e)
  } finally {
    loading.value = false
  }
}

async function handleChangePassword() {
  passwordResult.value = ''

  if (!oldPassword.value || !newPassword.value) {
    passwordResult.value = '请填写所有字段'
    return
  }
  if (newPassword.value.length < 6) {
    passwordResult.value = '新密码至少需要 6 个字符'
    return
  }
  if (newPassword.value !== confirmPassword.value) {
    passwordResult.value = '两次输入的密码不一致'
    return
  }

  changingPassword.value = true
  try {
    await api.changePassword(oldPassword.value, newPassword.value)
    passwordResult.value = '✅ 密码已修改成功'
    oldPassword.value = ''
    newPassword.value = ''
    confirmPassword.value = ''
  } catch (e) {
    passwordResult.value = '❌ 修改失败：请检查旧密码是否正确'
  } finally {
    changingPassword.value = false
  }
}

onMounted(loadSettings)
</script>

<template>
  <div class="settings">
    <h1>设置</h1>

    <!-- Server Info -->
    <MiuixCard v-if="settings">
      <div class="card-inner">
        <h2 class="section-title">🖥️ 服务器信息</h2>
        <div class="info-grid">
          <div class="info-item">
            <span class="info-label">主机名</span>
            <span class="info-value">{{ settings.hostname }}</span>
          </div>
          <div class="info-item">
            <span class="info-label">SMTP 端口</span>
            <span class="info-value">{{ settings.smtp_port }}</span>
          </div>
          <div class="info-item">
            <span class="info-label">IMAP 端口</span>
            <span class="info-value">{{ settings.imap_port }}</span>
          </div>
          <div class="info-item">
            <span class="info-label">Web 端口</span>
            <span class="info-value">{{ settings.web_port }}</span>
          </div>
          <div class="info-item">
            <span class="info-label">DKIM 选择器</span>
            <span class="info-value">{{ settings.dkim_selector }}</span>
          </div>
        </div>
      </div>
    </MiuixCard>

    <!-- Change Password -->
    <MiuixCard>
      <div class="card-inner">
        <h2 class="section-title">🔒 修改密码</h2>
        <div class="password-form">
          <div class="form-group">
            <label>当前密码</label>
            <MiuixInput
              v-model="oldPassword"
              type="password"
              placeholder="输入当前密码"
            />
          </div>
          <div class="form-group">
            <label>新密码</label>
            <MiuixInput
              v-model="newPassword"
              type="password"
              placeholder="至少 6 个字符"
            />
          </div>
          <div class="form-group">
            <label>确认新密码</label>
            <MiuixInput
              v-model="confirmPassword"
              type="password"
              placeholder="再次输入新密码"
            />
          </div>

          <p
            v-if="passwordResult"
            class="result"
            :class="{ success: passwordResult.startsWith('✅') }"
          >
            {{ passwordResult }}
          </p>

          <MiuixButton
            type="primary"
            :disabled="changingPassword"
            @click="handleChangePassword"
          >
            {{ changingPassword ? '修改中...' : '修改密码' }}
          </MiuixButton>
        </div>
      </div>
    </MiuixCard>

    <!-- About -->
    <MiuixCard>
      <div class="card-inner">
        <h2 class="section-title">ℹ️ 关于</h2>
        <div class="about">
          <p><strong>Kuria Mail</strong> — 轻量级自托管邮件服务器</p>
          <p class="about-desc">支持 SMTP/IMAP 协议，提供 Web 管理界面</p>
        </div>
      </div>
    </MiuixCard>
  </div>
</template>

<style scoped>
.settings {
  max-width: 700px;
}

.settings h1 {
  font-size: 24px;
  font-weight: 600;
  color: var(--m-color-text);
  margin-bottom: 24px;
}

.card-inner {
  padding: 24px;
}

.section-title {
  font-size: 16px;
  font-weight: 600;
  color: var(--m-color-text);
  margin-bottom: 20px;
}

.info-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(180px, 1fr));
  gap: 16px;
}

.info-item {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.info-label {
  font-size: 12px;
  color: var(--m-color-text-secondary);
}

.info-value {
  font-size: 14px;
  font-weight: 500;
  color: var(--m-color-text);
}

.password-form {
  display: flex;
  flex-direction: column;
  gap: 16px;
  max-width: 400px;
}

.form-group {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.form-group label {
  font-size: 14px;
  font-weight: 500;
  color: var(--m-color-text);
}

.result {
  font-size: 14px;
  color: #e74c3c;
}

.result.success {
  color: #27ae60;
}

.about p {
  color: var(--m-color-text);
  font-size: 14px;
  margin-bottom: 4px;
}

.about-desc {
  color: var(--m-color-text-secondary);
  font-size: 13px;
}
</style>
