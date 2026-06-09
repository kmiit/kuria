<script setup>
import { ref, onMounted, computed } from 'vue'
import { MiuixButton, MiuixInput, MiuixCard, MiuixDialog } from 'miuix-vue'
import { api } from '../api'

const domains = ref([])
const loading = ref(true)
const saving = ref(false)
const newDomain = ref('')
const showAddDialog = ref(false)
const message = ref('')
const error = ref('')
const dkimRecord = ref('')

const normalizedDomain = computed(() =>
  newDomain.value.trim().toLowerCase().replace(/^https?:\/\//, '').replace(/\/.*$/, ''),
)

function isValidDomain(value) {
  return /^[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?(?:\.[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?)+$/.test(value)
}

async function loadDomains() {
  loading.value = true
  error.value = ''
  try {
    const data = await api.getDomains()
    domains.value = data.domains || []
  } catch (e) {
    error.value = e.message || '加载域名失败'
  } finally {
    loading.value = false
  }
}

async function addDomain() {
  message.value = ''
  error.value = ''
  if (!isValidDomain(normalizedDomain.value)) {
    error.value = '请输入有效域名，例如 example.com'
    return
  }
  saving.value = true
  try {
    await api.createDomain(normalizedDomain.value)
    newDomain.value = ''
    showAddDialog.value = false
    message.value = '域名已添加'
    await loadDomains()
  } catch (e) {
    error.value = '添加失败：' + (e.message || '未知错误')
  } finally {
    saving.value = false
  }
}

async function deleteDomain(id, name) {
  message.value = ''
  error.value = ''
  if (!confirm(`确定删除域名 ${name}？相关用户可能无法继续收发邮件。`)) return
  try {
    await api.deleteDomain(id)
    domains.value = domains.value.filter((d) => d.id !== id)
    message.value = '域名已删除'
  } catch (e) {
    error.value = e.message || '删除失败'
  }
}

async function generateDkim(domain) {
  message.value = ''
  error.value = ''
  dkimRecord.value = ''
  try {
    const data = await api.generateDkim(domain.id)
    dkimRecord.value = data.dns_record || ''
    message.value = data.message || 'DKIM 记录已生成'
  } catch (e) {
    error.value = e.message || '生成 DKIM 记录失败'
  }
}

function copyToClipboard(text) {
  navigator.clipboard.writeText(text).then(() => {
    message.value = '已复制到剪贴板'
  })
}

function formatDate(dateStr) {
  if (!dateStr) return '未知时间'
  return new Date(dateStr).toLocaleDateString('zh-CN')
}

onMounted(loadDomains)
</script>

<template>
  <div class="domains">
    <div class="page-header">
      <div>
        <h1>域名管理</h1>
        <p class="subtitle">管理收发邮件使用的域名与认证记录。</p>
      </div>
      <div class="header-actions">
        <MiuixButton @click="loadDomains">刷新</MiuixButton>
        <MiuixButton type="primary" @click="showAddDialog = true">添加域名</MiuixButton>
      </div>
    </div>

    <p v-if="message" class="notice success">{{ message }}</p>
    <p v-if="error" class="notice error">{{ error }}</p>

    <MiuixCard v-if="dkimRecord">
      <div class="card-inner dns-card">
        <div class="dns-head">
          <div>
            <h2>DKIM DNS 记录</h2>
            <p>将以下 TXT 记录添加到域名 DNS 管理中。</p>
          </div>
          <MiuixButton @click="copyToClipboard(dkimRecord)">复制</MiuixButton>
        </div>
        <code>{{ dkimRecord }}</code>
      </div>
    </MiuixCard>

    <div v-if="loading" class="loading">加载中...</div>

    <div v-else-if="domains.length === 0" class="empty">
      <div class="empty-icon">🌐</div>
      <p>还没有配置域名。</p>
      <MiuixButton type="primary" @click="showAddDialog = true">添加第一个域名</MiuixButton>
    </div>

    <div v-else class="domain-list">
      <MiuixCard v-for="domain in domains" :key="domain.id">
        <div class="card-inner domain-card">
          <div class="domain-info">
            <div class="domain-icon">🌐</div>
            <div class="domain-copy">
              <div class="domain-name">{{ domain.domain_name }}</div>
              <div class="domain-detail">
                DKIM 选择器：{{ domain.dkim_selector || '未配置' }} · 创建于 {{ formatDate(domain.created_at) }}
              </div>
              <div class="record-tags">
                <span :class="{ active: domain.spf_record }">SPF</span>
                <span :class="{ active: domain.dkim_public_key }">DKIM</span>
              </div>
            </div>
          </div>
          <div class="domain-actions">
            <MiuixButton @click="generateDkim(domain)">DKIM</MiuixButton>
            <MiuixButton @click="deleteDomain(domain.id, domain.domain_name)">删除</MiuixButton>
          </div>
        </div>
      </MiuixCard>
    </div>

    <MiuixDialog v-model="showAddDialog" title="添加域名">
      <div class="add-form">
        <label>域名</label>
        <MiuixInput v-model="newDomain" placeholder="example.com" @keyup.enter="addDomain" />
        <p class="hint">系统会自动移除协议和路径，只保留域名。</p>
      </div>
      <template #footer="{ close }">
        <MiuixButton @click="close">取消</MiuixButton>
        <MiuixButton type="primary" :disabled="saving" @click="addDomain">
          {{ saving ? '添加中...' : '添加' }}
        </MiuixButton>
      </template>
    </MiuixDialog>
  </div>
</template>

<style scoped>
.domains {
  max-width: 980px;
}

.page-header {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 16px;
  margin-bottom: 22px;
}

.domains h1 {
  font-size: 26px;
  font-weight: 700;
  color: var(--m-color-text);
}

.subtitle {
  margin-top: 4px;
  color: var(--m-color-text-secondary);
  font-size: 14px;
}

.header-actions,
.domain-actions {
  display: flex;
  gap: 8px;
  flex-shrink: 0;
}

.notice {
  padding: 12px 14px;
  border-radius: var(--app-radius);
  margin-bottom: 14px;
  background: var(--m-color-card);
}

.notice.success {
  color: var(--app-success);
  border: 1px solid color-mix(in srgb, var(--app-success) 28%, transparent);
}

.notice.error {
  color: var(--app-danger);
  border: 1px solid color-mix(in srgb, var(--app-danger) 32%, transparent);
}

.loading,
.empty {
  text-align: center;
  padding: 80px 20px;
  color: var(--m-color-text-secondary);
}

.empty-icon {
  font-size: 52px;
  margin-bottom: 16px;
}

.empty p {
  margin-bottom: 16px;
}

.card-inner {
  padding: 22px;
}

.dns-card {
  margin-bottom: 16px;
}

.dns-head {
  display: flex;
  justify-content: space-between;
  gap: 16px;
  margin-bottom: 14px;
}

.dns-head h2 {
  font-size: 16px;
  color: var(--m-color-text);
}

.dns-head p {
  color: var(--m-color-text-secondary);
  font-size: 13px;
  margin-top: 4px;
}

.dns-card code {
  display: block;
  padding: 12px;
  background: var(--m-color-bg);
  border-radius: var(--app-radius);
  color: var(--m-color-text);
  overflow-wrap: anywhere;
  font-family: Consolas, Monaco, monospace;
  font-size: 12px;
}

.domain-list {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.domain-card {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
}

.domain-info {
  display: flex;
  align-items: center;
  gap: 16px;
  min-width: 0;
}

.domain-icon {
  width: 42px;
  height: 42px;
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--m-color-bg);
  border-radius: var(--app-radius);
  font-size: 24px;
  flex-shrink: 0;
}

.domain-copy {
  min-width: 0;
}

.domain-name {
  font-size: 16px;
  font-weight: 700;
  color: var(--m-color-text);
  word-break: break-word;
}

.domain-detail {
  font-size: 13px;
  color: var(--m-color-text-secondary);
  margin-top: 3px;
}

.record-tags {
  display: flex;
  gap: 6px;
  margin-top: 10px;
}

.record-tags span {
  font-size: 11px;
  font-weight: 700;
  padding: 3px 7px;
  border-radius: 999px;
  color: var(--m-color-text-secondary);
  background: var(--m-color-bg);
}

.record-tags span.active {
  color: var(--app-success);
  background: color-mix(in srgb, var(--app-success) 12%, transparent);
}

.add-form {
  padding: 8px 0;
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.add-form label {
  font-size: 14px;
  font-weight: 650;
  color: var(--m-color-text);
}

.hint {
  color: var(--m-color-text-secondary);
  font-size: 12px;
}

@media (max-width: 680px) {
  .page-header,
  .domain-card,
  .dns-head {
    align-items: stretch;
    flex-direction: column;
  }

  .header-actions,
  .domain-actions {
    width: 100%;
  }

  .header-actions > *,
  .domain-actions > * {
    flex: 1;
  }
}
</style>
