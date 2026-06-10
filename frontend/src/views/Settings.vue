<script setup>
import { computed, onMounted, ref } from 'vue'
import { MiuixButton, MiuixCard, MiuixInput } from 'miuix-vue'
import { api } from '../api'
import PasswordInput from '../components/PasswordInput.vue'

const settings = ref(null)
const plugins = ref(null)
const queueItems = ref([])
const queueStatus = ref('queued')
const loading = ref(true)
const queueLoading = ref(false)
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
const queueStatusOptions = [
  { value: 'queued', label: '待发送' },
  { value: 'failed', label: '失败' },
  { value: 'sent', label: '已发送' },
  { value: '', label: '全部' },
]

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

async function loadQueue() {
  queueLoading.value = true
  try {
    const data = await api.getQueue(queueStatus.value, 50)
    queueItems.value = data.items || []
  } catch (err) {
    error.value = err.message || '加载外发队列失败'
    queueItems.value = []
  } finally {
    queueLoading.value = false
  }
}

async function retryQueueItem(item) {
  result.value = ''
  error.value = ''
  try {
    await api.retryQueueItem(item.id)
    result.value = '已重新加入发送队列'
    await loadQueue()
  } catch (err) {
    error.value = err.message || '重试失败'
  }
}

async function deleteQueueItem(item) {
  if (!confirm(`删除队列项 #${item.id}？`)) return
  result.value = ''
  error.value = ''
  try {
    await api.deleteQueueItem(item.id)
    result.value = '队列项已删除'
    await loadQueue()
  } catch (err) {
    error.value = err.message || '删除失败'
  }
}

function queueStatusLabel(status) {
  return queueStatusOptions.find((item) => item.value === status)?.label || status
}

function pluginConfigRoute(plugin) {
  if (!plugin?.admin_path) return null
  return { name: 'plugin-config', params: { plugin: plugin.name } }
}

function formatRecipients(recipients) {
  return Array.isArray(recipients) ? recipients.join(', ') : ''
}

onMounted(async () => {
  await loadSettings()
  await loadQueue()
})
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
              <button class="info-item" type="button" @click="copyValue(settings.pop3_port)">
                <span class="info-label">POP3 端口</span>
                <span class="info-value">{{ settings.pop3_port }}</span>
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
              <div v-for="plugin in plugins.loaded" :key="plugin.name" class="plugin-item">
                <div class="plugin-main">
                  <div class="plugin-title-row">
                    <div class="plugin-name">{{ plugin.name }}</div>
                    <span v-if="plugin.version" class="version-tag">v{{ plugin.version }}</span>
                  </div>
                  <div class="plugin-desc">{{ plugin.description || '无描述' }}</div>
                  <div class="plugin-meta">
                    <span class="plugin-status-dot active"></span>
                    <span>已加载</span>
                    <span v-if="plugin.admin_path">可配置</span>
                  </div>
                </div>
                <div class="plugin-actions">
                  <router-link
                    v-if="pluginConfigRoute(plugin)"
                    class="plugin-config-link"
                    :to="pluginConfigRoute(plugin)"
                  >
                    配置
                  </router-link>
                </div>
              </div>
            </div>

            <div v-else-if="plugins" class="plugin-empty">
              {{ plugins.enabled ? '插件系统已启用，但当前没有成功加载的插件。' : '插件系统未启用。' }}
            </div>

            <div v-if="plugins?.load_errors?.length" class="plugin-errors">
              <h3>加载失败</h3>
              <div v-for="item in plugins.load_errors" :key="item.path" class="plugin-error">
                <span>{{ item.error }}</span>
              </div>
            </div>
          </div>
        </MiuixCard>

        <MiuixCard>
          <div class="card-inner">
            <div class="section-row">
              <h2 class="section-title">外发队列</h2>
              <MiuixButton class="app-secondary-button" :disabled="queueLoading" @click="loadQueue">
                {{ queueLoading ? '刷新中...' : '刷新' }}
              </MiuixButton>
            </div>

            <div class="queue-tabs">
              <button
                v-for="option in queueStatusOptions"
                :key="option.value"
                type="button"
                class="queue-tab"
                :class="{ active: queueStatus === option.value }"
                @click="queueStatus = option.value; loadQueue()"
              >
                {{ option.label }}
              </button>
            </div>

            <div v-if="queueLoading" class="plugin-empty">正在加载外发队列...</div>
            <div v-else-if="!queueItems.length" class="plugin-empty">当前没有队列项。</div>
            <div v-else class="queue-list">
              <div v-for="item in queueItems" :key="item.id" class="queue-item">
                <div class="queue-main">
                  <div class="queue-title">
                    <span>#{{ item.id }}</span>
                    <span class="status-pill" :class="{ active: item.status === 'queued' }">
                      {{ queueStatusLabel(item.status) }}
                    </span>
                  </div>
                  <div class="queue-meta">
                    <span>发件人：{{ item.envelope_sender }}</span>
                    <span>收件人：{{ formatRecipients(item.recipients) }}</span>
                    <span>尝试：{{ item.attempts }} / {{ item.max_attempts }}</span>
                    <span>大小：{{ item.raw_size }} B</span>
                    <span v-if="item.next_attempt_at">下次：{{ item.next_attempt_at }}</span>
                  </div>
                  <div v-if="item.last_error" class="queue-error">{{ item.last_error }}</div>
                </div>
                <div class="queue-actions">
                  <MiuixButton
                    v-if="item.status === 'failed'"
                    class="app-secondary-button"
                    @click="retryQueueItem(item)"
                  >
                    重试
                  </MiuixButton>
                  <MiuixButton class="app-danger-button" @click="deleteQueueItem(item)">
                    删除
                  </MiuixButton>
                </div>
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

.plugin-title-row {
  display: flex;
  align-items: center;
  gap: 8px;
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

.plugin-meta {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 7px;
  margin-top: 9px;
  color: var(--m-color-text-secondary);
  font-size: 12px;
}

.plugin-meta span:not(.plugin-status-dot) {
  padding: 3px 7px;
  border-radius: 999px;
  background: color-mix(in srgb, var(--m-color-text-secondary) 10%, transparent);
}

.plugin-status-dot {
  width: 7px;
  height: 7px;
  border-radius: 999px;
  background: var(--m-color-text-secondary);
}

.plugin-status-dot.active {
  background: var(--app-success);
  box-shadow: 0 0 0 4px color-mix(in srgb, var(--app-success) 14%, transparent);
}

.plugin-actions {
  display: flex;
  flex-shrink: 0;
  align-items: center;
  gap: 8px;
}

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

.plugin-config-link {
  flex-shrink: 0;
  min-height: 28px;
  padding: 5px 10px;
  border-radius: var(--app-radius);
  color: var(--m-color-primary);
  background: color-mix(in srgb, var(--m-color-primary) 10%, transparent);
  font-size: 13px;
  font-weight: 700;
  line-height: 18px;
  text-decoration: none;
}

.plugin-config-link:hover {
  background: color-mix(in srgb, var(--m-color-primary) 16%, transparent);
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

.queue-tabs {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  margin-bottom: 14px;
}

.queue-tab {
  padding: 7px 12px;
  color: var(--m-color-text-secondary);
  background: var(--m-color-bg);
  border: 1px solid transparent;
  border-radius: var(--app-radius);
  cursor: pointer;
  font: inherit;
  font-size: 13px;
}

.queue-tab.active {
  color: var(--m-color-primary);
  border-color: var(--m-color-primary);
  background: color-mix(in srgb, var(--m-color-primary) 10%, transparent);
}

.queue-list {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.queue-item {
  display: grid;
  grid-template-columns: minmax(0, 1fr) auto;
  gap: 14px;
  padding: 14px;
  background: var(--m-color-bg);
  border-radius: var(--app-radius);
}

.queue-title {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 8px;
  font-weight: 700;
  color: var(--m-color-text);
}

.queue-meta {
  display: flex;
  flex-wrap: wrap;
  gap: 6px 12px;
  color: var(--m-color-text-secondary);
  font-size: 13px;
}

.queue-meta span {
  overflow-wrap: anywhere;
}

.queue-error {
  margin-top: 9px;
  padding: 9px;
  color: var(--app-danger);
  background: color-mix(in srgb, var(--app-danger) 10%, transparent);
  border-radius: var(--app-radius);
  font-size: 12px;
  overflow-wrap: anywhere;
}

.queue-actions {
  display: flex;
  align-items: flex-start;
  gap: 8px;
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
  .queue-item,
  .section-row {
    align-items: stretch;
    grid-template-columns: 1fr;
    flex-direction: column;
  }

  .plugin-actions {
    align-items: center;
    justify-content: flex-start;
  }

  .queue-actions {
    justify-content: flex-start;
  }
}
</style>
