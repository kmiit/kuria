<script setup>
import { ref, onMounted, computed } from 'vue'
import { MiuixButton, MiuixInput, MiuixCard } from 'miuix-vue'
import { api } from '../api'

const settings = ref(null)
const plugins = ref(null)
const loading = ref(true)
const error = ref('')
const pluginsError = ref('')

const oldPassword = ref('')
const newPassword = ref('')
const confirmPassword = ref('')
const changingPassword = ref(false)
const passwordResult = ref('')

const passwordStrength = computed(() => {
  const value = newPassword.value
  let score = 0
  if (value.length >= 8) score++
  if (/[A-Z]/.test(value) && /[a-z]/.test(value)) score++
  if (/\d/.test(value)) score++
  if (/[^A-Za-z0-9]/.test(value)) score++
  if (!value) return { label: '未填写', className: '' }
  if (score <= 1) return { label: '较弱', className: 'weak' }
  if (score <= 3) return { label: '中等', className: 'medium' }
  return { label: '较强', className: 'strong' }
})

async function loadSettings() {
  loading.value = true
  error.value = ''
  pluginsError.value = ''
  try {
    const settingsData = await api.getSettings()
    settings.value = settingsData
    plugins.value = settingsData.plugins || null
    if (!settingsData.plugins) {
      pluginsError.value = '当前后端未返回插件状态，请重启后端以启用插件管理。'
    }
  } catch (e) {
    error.value = e.message || '加载设置失败'
    plugins.value = null
  } finally {
    loading.value = false
  }
}

async function handleChangePassword() {
  passwordResult.value = ''

  if (!oldPassword.value || !newPassword.value || !confirmPassword.value) {
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
    passwordResult.value = '密码已修改成功'
    oldPassword.value = ''
    newPassword.value = ''
    confirmPassword.value = ''
  } catch (e) {
    passwordResult.value = '修改失败：请检查旧密码是否正确'
  } finally {
    changingPassword.value = false
  }
}

function copyValue(value) {
  navigator.clipboard.writeText(String(value || '')).then(() => {
    passwordResult.value = '已复制到剪贴板'
  })
}

onMounted(loadSettings)
</script>

<template>
  <div class="settings">
    <div class="page-header">
      <div>
        <h1>设置</h1>
        <p class="subtitle">查看服务配置，维护当前账号密码。</p>
      </div>
      <MiuixButton @click="loadSettings">刷新</MiuixButton>
    </div>

    <div v-if="error" class="notice error">{{ error }}</div>
    <div v-if="loading" class="loading">加载中...</div>

    <template v-else>
      <div class="settings-stack">
        <MiuixCard v-if="settings">
          <div class="card-inner">
            <h2 class="section-title">服务器信息</h2>
            <div class="info-grid">
              <button class="info-item" type="button" @click="copyValue(settings.hostname)">
                <span class="info-label">主机名</span>
                <span class="info-value">{{ settings.hostname }}</span>
              </button>
              <button class="info-item" type="button" @click="copyValue(settings.smtp_port)">
                <span class="info-label">SMTP 端口</span>
                <span class="info-value">{{ settings.smtp_port }}</span>
              </button>
              <button class="info-item" type="button" @click="copyValue(settings.imap_port)">
                <span class="info-label">IMAP 端口</span>
                <span class="info-value">{{ settings.imap_port }}</span>
              </button>
              <button class="info-item" type="button" @click="copyValue(settings.web_port)">
                <span class="info-label">Web 端口</span>
                <span class="info-value">{{ settings.web_port }}</span>
              </button>
              <button class="info-item" type="button" @click="copyValue(settings.dkim_selector)">
                <span class="info-label">DKIM 选择器</span>
                <span class="info-value">{{ settings.dkim_selector }}</span>
              </button>
            </div>
          </div>
        </MiuixCard>

        <MiuixCard>
          <div class="card-inner">
            <div class="section-row">
              <h2 class="section-title">插件管理</h2>
              <span v-if="plugins" class="status-pill" :class="{ active: plugins.enabled }">
                {{ plugins.enabled ? '已启用' : '未启用' }}
              </span>
            </div>

            <div v-if="pluginsError" class="plugin-empty error">
              {{ pluginsError }}
            </div>

            <div v-else-if="plugins" class="plugin-summary">
              <div class="summary-item">
                <span class="summary-value">{{ plugins.loaded_count }}</span>
                <span class="summary-label">已加载</span>
              </div>
              <div class="summary-item">
                <span class="summary-value">{{ plugins.configured_count }}</span>
                <span class="summary-label">已配置</span>
              </div>
              <div class="summary-item">
                <span class="summary-value">{{ plugins.abi_version }}</span>
                <span class="summary-label">ABI</span>
              </div>
            </div>

            <div v-if="plugins?.loaded?.length" class="plugin-list">
              <div v-for="plugin in plugins.loaded" :key="plugin.path" class="plugin-item">
                <div class="plugin-main">
                  <div class="plugin-name">{{ plugin.name }}</div>
                  <div class="plugin-desc">{{ plugin.description || '无描述' }}</div>
                  <code>{{ plugin.path }}</code>
                </div>
                <span v-if="plugin.version" class="version-tag">v{{ plugin.version }}</span>
              </div>
            </div>

            <div v-else-if="plugins" class="plugin-empty">
              {{ plugins.enabled ? '插件系统已启用，但当前没有成功加载的插件。' : '插件系统未启用。' }}
            </div>

            <div v-if="plugins?.configured_paths?.length" class="path-list">
              <h3>配置路径</h3>
              <code v-for="path in plugins.configured_paths" :key="path">{{ path }}</code>
            </div>

            <div v-if="plugins?.load_errors?.length" class="plugin-errors">
              <h3>加载失败</h3>
              <div v-for="item in plugins.load_errors" :key="item.path" class="plugin-error">
                <code>{{ item.path }}</code>
                <span>{{ item.error }}</span>
              </div>
            </div>
          </div>
        </MiuixCard>

        <MiuixCard>
          <div class="card-inner">
            <h2 class="section-title">修改密码</h2>
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
                <div class="strength" :class="passwordStrength.className">
                  强度：{{ passwordStrength.label }}
                </div>
              </div>
              <div class="form-group">
                <label>确认新密码</label>
                <MiuixInput
                  v-model="confirmPassword"
                  type="password"
                  placeholder="再次输入新密码"
                  @keyup.enter="handleChangePassword"
                />
              </div>

              <p
                v-if="passwordResult"
                class="result"
                :class="{ success: passwordResult.includes('成功') || passwordResult.includes('复制') }"
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

        <MiuixCard>
          <div class="card-inner about-card">
            <h2 class="section-title">关于</h2>
            <p><strong>Kuria Mail</strong> 是轻量级自托管邮件服务器。</p>
            <p class="about-desc">支持 SMTP/IMAP 协议，并提供 Web 管理界面。</p>
          </div>
        </MiuixCard>
      </div>
    </template>
  </div>
</template>

<style scoped>
.settings {
  max-width: 780px;
}

.page-header {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 16px;
  margin-bottom: 22px;
}

.settings h1 {
  font-size: 26px;
  font-weight: 700;
  color: var(--m-color-text);
}

.subtitle {
  margin-top: 4px;
  color: var(--m-color-text-secondary);
  font-size: 14px;
}

.notice,
.loading {
  padding: 12px 14px;
  border-radius: var(--app-radius);
  margin-bottom: 14px;
  background: var(--m-color-card);
}

.notice.error {
  color: var(--app-danger);
  border: 1px solid color-mix(in srgb, var(--app-danger) 32%, transparent);
}

.settings-stack {
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.card-inner {
  padding: 24px;
}

.section-title {
  font-size: 17px;
  font-weight: 700;
  color: var(--m-color-text);
  margin-bottom: 18px;
}

.section-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  margin-bottom: 18px;
}

.section-row .section-title {
  margin-bottom: 0;
}

.status-pill {
  flex-shrink: 0;
  padding: 4px 9px;
  border-radius: 999px;
  font-size: 12px;
  font-weight: 700;
  color: var(--m-color-text-secondary);
  background: var(--m-color-bg);
}

.status-pill.active {
  color: var(--app-success);
  background: color-mix(in srgb, var(--app-success) 14%, transparent);
}

.info-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(160px, 1fr));
  gap: 12px;
}

.info-item {
  display: flex;
  flex-direction: column;
  gap: 5px;
  padding: 14px;
  text-align: left;
  color: inherit;
  background: var(--m-color-bg);
  border: 1px solid transparent;
  border-radius: var(--app-radius);
  cursor: pointer;
}

.info-item:hover {
  border-color: var(--m-color-border);
}

.info-label {
  font-size: 12px;
  color: var(--m-color-text-secondary);
}

.info-value {
  font-size: 14px;
  font-weight: 700;
  color: var(--m-color-text);
  overflow-wrap: anywhere;
}

.plugin-summary {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 10px;
  margin-bottom: 16px;
}

.summary-item {
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: 12px;
  background: var(--m-color-bg);
  border-radius: var(--app-radius);
}

.summary-value {
  font-size: 22px;
  line-height: 1;
  font-weight: 750;
  color: var(--m-color-primary);
}

.summary-label {
  font-size: 12px;
  color: var(--m-color-text-secondary);
}

.plugin-list {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.plugin-item {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 12px;
  padding: 14px;
  background: var(--m-color-bg);
  border-radius: var(--app-radius);
}

.plugin-main {
  min-width: 0;
}

.plugin-name {
  font-size: 15px;
  font-weight: 700;
  color: var(--m-color-text);
}

.plugin-desc {
  margin-top: 3px;
  font-size: 13px;
  color: var(--m-color-text-secondary);
}

.plugin-item code,
.path-list code,
.plugin-error code {
  display: block;
  margin-top: 8px;
  font-family: Consolas, Monaco, monospace;
  font-size: 12px;
  color: var(--m-color-text);
  overflow-wrap: anywhere;
}

.version-tag {
  flex-shrink: 0;
  padding: 3px 8px;
  border-radius: 999px;
  font-size: 12px;
  color: var(--app-info);
  background: color-mix(in srgb, var(--app-info) 12%, transparent);
}

.plugin-empty {
  padding: 14px;
  color: var(--m-color-text-secondary);
  background: var(--m-color-bg);
  border-radius: var(--app-radius);
  font-size: 14px;
}

.plugin-empty.error {
  color: var(--app-danger);
  background: color-mix(in srgb, var(--app-danger) 10%, transparent);
}

.path-list,
.plugin-errors {
  margin-top: 16px;
}

.path-list h3,
.plugin-errors h3 {
  font-size: 14px;
  color: var(--m-color-text);
  margin-bottom: 8px;
}

.path-list code {
  padding: 10px;
  background: var(--m-color-bg);
  border-radius: var(--app-radius);
}

.path-list code + code {
  margin-top: 8px;
}

.plugin-error {
  padding: 12px;
  border-radius: var(--app-radius);
  background: color-mix(in srgb, var(--app-danger) 10%, transparent);
  color: var(--app-danger);
}

.plugin-error + .plugin-error {
  margin-top: 8px;
}

.plugin-error span {
  display: block;
  margin-top: 6px;
  font-size: 13px;
}

.password-form {
  display: flex;
  flex-direction: column;
  gap: 16px;
  max-width: 430px;
}

.form-group {
  display: flex;
  flex-direction: column;
  gap: 7px;
}

.form-group label {
  font-size: 14px;
  font-weight: 650;
  color: var(--m-color-text);
}

.strength {
  font-size: 12px;
  color: var(--m-color-text-secondary);
}

.strength.weak {
  color: var(--app-danger);
}

.strength.medium {
  color: var(--app-warning);
}

.strength.strong {
  color: var(--app-success);
}

.result {
  font-size: 14px;
  color: var(--app-danger);
}

.result.success {
  color: var(--app-success);
}

.about-card p {
  color: var(--m-color-text);
  font-size: 14px;
  margin-bottom: 4px;
}

.about-desc {
  color: var(--m-color-text-secondary);
  font-size: 13px;
}

@media (max-width: 620px) {
  .page-header {
    align-items: stretch;
    flex-direction: column;
  }

  .plugin-summary {
    grid-template-columns: 1fr;
  }

  .plugin-item,
  .section-row {
    align-items: stretch;
    flex-direction: column;
  }
}
</style>
