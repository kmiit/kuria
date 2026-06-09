<script setup>
import { computed, onMounted, ref } from 'vue'
import { MiuixButton, MiuixCard, MiuixInput } from 'miuix-vue'
import { api } from '../api'
import PasswordInput from '../components/PasswordInput.vue'

const settings = ref(null)
const plugins = ref(null)
const loading = ref(true)
const savingSettings = ref(false)
const error = ref('')
const result = ref('')
const pluginsError = ref('')

const hostnameDraft = ref('')
const oldPassword = ref('')
const newPassword = ref('')
const confirmPassword = ref('')
const changingPassword = ref(false)

const domainPattern = /^[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?(?:\.[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?)+$/

const passwordStrength = computed(() => {
  const value = newPassword.value
  let score = 0
  if (value.length >= 8) score += 1
  if (/[A-Z]/.test(value) && /[a-z]/.test(value)) score += 1
  if (/\d/.test(value)) score += 1
  if (/[^A-Za-z0-9]/.test(value)) score += 1
  if (!value) return { label: '未填写', className: '' }
  if (score <= 1) return { label: '偏弱', className: 'weak' }
  if (score <= 3) return { label: '可用', className: 'medium' }
  return { label: '较强', className: 'strong' }
})

async function loadSettings() {
  loading.value = true
  error.value = ''
  result.value = ''
  pluginsError.value = ''
  try {
    const settingsData = await api.getSettings()
    settings.value = settingsData
    hostnameDraft.value = settingsData.hostname || ''
    plugins.value = settingsData.plugins || null
    if (!settingsData.plugins) {
      pluginsError.value = '后端没有返回插件状态，请重启后端后再试。'
    }
  } catch (err) {
    error.value = err.message || '加载设置失败'
    plugins.value = null
  } finally {
    loading.value = false
  }
}

function normalizeDomain(value) {
  return String(value || '')
    .trim()
    .toLowerCase()
    .replace(/^https?:\/\//, '')
    .replace(/\/.*$/, '')
    .replace(/\.$/, '')
}

async function saveServerSettings() {
  const hostname = normalizeDomain(hostnameDraft.value)
  result.value = ''
  error.value = ''

  if (!domainPattern.test(hostname)) {
    error.value = '主机名格式不正确，例如 mail.example.com'
    return
  }

  savingSettings.value = true
  try {
    const data = await api.updateSettings({ hostname })
    settings.value = { ...settings.value, hostname: data.hostname || hostname }
    hostnameDraft.value = settings.value.hostname
    result.value = '服务器设置已保存'
  } catch (err) {
    error.value = err.message || '保存设置失败'
  } finally {
    savingSettings.value = false
  }
}

async function handleChangePassword() {
  result.value = ''
  error.value = ''

  if (!oldPassword.value || !newPassword.value || !confirmPassword.value) {
    error.value = '请填写所有密码字段'
    return
  }
  if (newPassword.value.length < 6) {
    error.value = '新密码至少需要 6 个字符'
    return
  }
  if (newPassword.value !== confirmPassword.value) {
    error.value = '两次输入的新密码不一致'
    return
  }

  changingPassword.value = true
  try {
    await api.changePassword(oldPassword.value, newPassword.value)
    result.value = '密码已修改'
    oldPassword.value = ''
    newPassword.value = ''
    confirmPassword.value = ''
  } catch {
    error.value = '修改失败，请检查当前密码是否正确'
  } finally {
    changingPassword.value = false
  }
}

function copyValue(value) {
  navigator.clipboard.writeText(String(value || '')).then(() => {
    result.value = '已复制到剪贴板'
  })
}

onMounted(loadSettings)
</script>

<template>
  <div class="settings">
    <div class="page-header">
      <div>
        <h1>设置</h1>
        <p class="subtitle">维护服务器标识、查看运行配置，并管理当前账号密码。</p>
      </div>
      <MiuixButton @click="loadSettings">刷新</MiuixButton>
    </div>

    <div v-if="error" class="notice error">{{ error }}</div>
    <div v-if="result" class="notice success">{{ result }}</div>
    <div v-if="loading" class="loading">正在加载设置...</div>

    <template v-else>
      <div class="settings-stack">
        <MiuixCard v-if="settings">
          <div class="card-inner">
            <h2 class="section-title">服务器设置</h2>
            <div class="server-form">
              <label class="form-group">
                <span>主机名</span>
                <div class="input-action-row">
                  <MiuixInput
                    v-model="hostnameDraft"
                    placeholder="mail.example.com"
                    @keyup.enter="saveServerSettings"
                  />
                  <MiuixButton
                    type="primary"
                    class="save-hostname-button"
                    :disabled="savingSettings"
                    @click="saveServerSettings"
                  >
                    {{ savingSettings ? '保存中...' : '保存主机名' }}
                  </MiuixButton>
                </div>
                <small>用于 SMTP 问候语、邮件 Message-ID 和本地域名判断。修改后新请求立即生效。</small>
              </label>
            </div>

            <div class="info-grid">
              <button class="info-item" type="button" @click="copyValue(settings.hostname)">
                <span class="info-label">当前主机名</span>
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
              <label class="form-group">
                <span>当前密码</span>
                <PasswordInput v-model="oldPassword" placeholder="输入当前密码" />
              </label>
              <label class="form-group">
                <span>新密码</span>
                <PasswordInput
                  v-model="newPassword"
                  placeholder="至少 6 个字符"
                  autocomplete="new-password"
                />
                <small class="strength" :class="passwordStrength.className">
                  强度：{{ passwordStrength.label }}
                </small>
              </label>
              <label class="form-group">
                <span>确认新密码</span>
                <PasswordInput
                  v-model="confirmPassword"
                  placeholder="再次输入新密码"
                  autocomplete="new-password"
                  @keyup-enter="handleChangePassword"
                />
              </label>

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

.notice.success {
  color: var(--app-success);
  border: 1px solid color-mix(in srgb, var(--app-success) 32%, transparent);
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

.server-form {
  margin-bottom: 18px;
}

.input-action-row {
  display: grid;
  grid-template-columns: minmax(0, 1fr) auto;
  gap: 12px;
  align-items: center;
}

.save-hostname-button {
  min-width: 108px;
  min-height: 40px;
  align-self: center;
}

.form-group {
  display: flex;
  flex-direction: column;
  gap: 7px;
}

.form-group span {
  font-size: 14px;
  font-weight: 650;
  color: var(--m-color-text);
}

.form-group small {
  color: var(--m-color-text-secondary);
  font-size: 12px;
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
  .page-header,
  .input-action-row {
    align-items: stretch;
    grid-template-columns: 1fr;
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
