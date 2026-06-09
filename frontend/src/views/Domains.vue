<script setup>
import { computed, onMounted, ref } from 'vue'
import { MiuixButton, MiuixCard, MiuixDialog, MiuixInput } from 'miuix-vue'
import { api } from '../api'

const domains = ref([])
const loading = ref(true)
const saving = ref(false)
const generatingId = ref(null)
const newDomain = ref('')
const showAddDialog = ref(false)
const message = ref('')
const error = ref('')
const expandedDomains = ref({})

const normalizedDomain = computed(() => normalizeDomain(newDomain.value))

function normalizeDomain(value) {
  return String(value || '')
    .trim()
    .toLowerCase()
    .replace(/^https?:\/\//, '')
    .replace(/\/.*$/, '')
    .replace(/\.$/, '')
}

function isValidDomain(value) {
  return /^[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?(?:\.[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?)+$/.test(value)
}

function dnsLine(host, value) {
  return value ? `${host} IN TXT "${value}"` : ''
}

function spfValue(domain) {
  return domain.spf_record || ''
}

function dkimSelector(domain) {
  return domain.dkim_selector || 'kuria'
}

function dkimHost(domain) {
  return `${dkimSelector(domain)}._domainkey.${domain.domain_name}`
}

function dkimValue(domain) {
  return domain.dkim_public_key ? `v=DKIM1; k=rsa; p=${domain.dkim_public_key}` : ''
}

function publicKeyFingerprint(value) {
  if (!value) return ''
  let hash = 2166136261
  for (let index = 0; index < value.length; index += 1) {
    hash = Math.imul(hash ^ value.charCodeAt(index), 16777619)
  }
  return (hash >>> 0).toString(16).toUpperCase().padStart(8, '0')
}

function dnsRecords(domain) {
  const spf = spfValue(domain)
  const dkim = dkimValue(domain)
  const fingerprint = publicKeyFingerprint(domain.dkim_public_key)

  return [
    {
      type: 'SPF',
      title: 'SPF 发信授权',
      host: domain.domain_name,
      value: spf,
      line: dnsLine(domain.domain_name, spf),
      ready: Boolean(spf),
      note: '告诉收件方哪些服务器可以代表这个域名发信。',
      detail: spf ? '已生成，添加到 DNS 的 TXT 记录即可。' : '缺少 SPF 记录。',
      copyLabel: 'SPF 记录',
    },
    {
      type: 'DKIM',
      title: 'DKIM 签名公钥',
      host: dkimHost(domain),
      value: dkim,
      line: dnsLine(dkimHost(domain), dkim),
      ready: Boolean(dkim),
      note: dkim
        ? `selector ${dkimSelector(domain)}，key hash ${fingerprint}`
        : '生成密钥后，把 TXT 记录添加到 DNS，用于验证邮件签名。',
      detail: dkim ? '重新生成会得到新的 selector 和公钥。' : '尚未生成。',
      copyLabel: 'DKIM 记录',
    },
  ]
}

function isDomainExpanded(id) {
  return Boolean(expandedDomains.value[id])
}

function setDomainExpanded(id, expanded) {
  expandedDomains.value = {
    ...expandedDomains.value,
    [id]: expanded,
  }
}

function toggleDomainRecords(id) {
  setDomainExpanded(id, !isDomainExpanded(id))
}

async function loadDomains() {
  loading.value = true
  error.value = ''
  try {
    const data = await api.getDomains()
    domains.value = data.domains || []
  } catch (err) {
    error.value = err.message || '加载域名失败'
  } finally {
    loading.value = false
  }
}

async function addDomain() {
  message.value = ''
  error.value = ''
  const domain = normalizedDomain.value
  if (!isValidDomain(domain)) {
    error.value = '请输入有效域名，例如 example.com'
    return
  }

  saving.value = true
  try {
    await api.createDomain(domain)
    newDomain.value = ''
    showAddDialog.value = false
    message.value = '域名已添加'
    await loadDomains()
  } catch (err) {
    error.value = err.message || '添加域名失败'
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
    domains.value = domains.value.filter((domain) => domain.id !== id)
    message.value = '域名已删除'
  } catch (err) {
    error.value = err.message || '删除失败'
  }
}

async function generateDkim(domain) {
  message.value = ''
  error.value = ''
  generatingId.value = domain.id
  try {
    const data = await api.generateDkim(domain.id)
    await loadDomains()

    const updatedDomain =
      data.domain || domains.value.find((item) => item.id === domain.id) || domain
    const selector = data.selector || dkimSelector(updatedDomain)
    const fingerprint = publicKeyFingerprint(updatedDomain.dkim_public_key)
    message.value = fingerprint
      ? `DKIM 已重新生成：selector ${selector}，key hash ${fingerprint}`
      : 'DKIM 密钥已生成'
    setDomainExpanded(domain.id, true)
  } catch (err) {
    error.value = err.message || '生成 DKIM 记录失败'
  } finally {
    generatingId.value = null
  }
}

async function copyToClipboard(text, label = '记录') {
  if (!text) return
  try {
    await navigator.clipboard.writeText(text)
    message.value = `${label}已复制`
  } catch {
    error.value = '复制失败，请手动选中记录复制'
  }
}

function copyAllRecords(domain) {
  const records = dnsRecords(domain)
    .map((record) => record.line)
    .filter(Boolean)
  copyToClipboard(records.join('\n'), 'DNS 记录')
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
        <p class="subtitle">每个域名的 SPF 和 DKIM 记录放在同一张卡片里，方便复制到 DNS。</p>
      </div>
      <div class="header-actions">
        <MiuixButton @click="loadDomains">刷新</MiuixButton>
        <MiuixButton type="primary" @click="showAddDialog = true">添加域名</MiuixButton>
      </div>
    </div>

    <p v-if="message" class="notice success">{{ message }}</p>
    <p v-if="error" class="notice error">{{ error }}</p>

    <div v-if="loading" class="loading">正在加载域名...</div>

    <div v-else-if="domains.length === 0" class="empty">
      <p>还没有配置域名。</p>
      <MiuixButton type="primary" @click="showAddDialog = true">添加第一个域名</MiuixButton>
    </div>

    <div v-else class="domain-list">
      <MiuixCard v-for="domain in domains" :key="domain.id">
        <div class="card-inner">
          <div class="domain-head">
            <div class="domain-title">
              <div class="domain-name">{{ domain.domain_name }}</div>
              <div class="domain-meta">创建于 {{ formatDate(domain.created_at) }}</div>
            </div>
            <div class="domain-actions">
              <MiuixButton @click="copyAllRecords(domain)">复制全部</MiuixButton>
              <MiuixButton :disabled="generatingId === domain.id" @click="generateDkim(domain)">
                {{ generatingId === domain.id ? '生成中...' : domain.dkim_public_key ? '重新生成 DKIM' : '生成 DKIM' }}
              </MiuixButton>
              <MiuixButton @click="deleteDomain(domain.id, domain.domain_name)">删除</MiuixButton>
            </div>
          </div>

          <div class="dns-toolbar">
            <div class="dns-statuses">
              <span
                v-for="record in dnsRecords(domain)"
                :key="record.type"
                class="record-chip"
                :class="{ ready: record.ready, missing: !record.ready }"
              >
                <strong>{{ record.type }}</strong>
                {{ record.ready ? '可用' : '待处理' }}
              </span>
            </div>
            <MiuixButton @click="toggleDomainRecords(domain.id)">
              {{ isDomainExpanded(domain.id) ? '收起记录' : '展开记录' }}
            </MiuixButton>
          </div>

          <div v-if="isDomainExpanded(domain.id)" class="dns-list">
            <div
              v-for="record in dnsRecords(domain)"
              :key="record.type"
              class="dns-row"
              :class="{ missing: !record.ready }"
            >
              <div class="record-summary">
                <span class="record-type">{{ record.type }}</span>
                <div>
                  <h2>{{ record.title }}</h2>
                  <p>{{ record.note }}</p>
                </div>
              </div>

              <div class="record-values">
                <div class="record-field">
                  <span>主机名</span>
                  <code>{{ record.host }}</code>
                </div>
                <div class="record-field">
                  <span>TXT 值</span>
                  <code v-if="record.value">{{ record.value }}</code>
                  <em v-else>未生成</em>
                </div>
              </div>

              <div class="record-actions">
                <span class="status" :class="{ ready: record.ready }">
                  {{ record.ready ? '可用' : '待处理' }}
                </span>
                <small>{{ record.detail }}</small>
                <MiuixButton
                  :disabled="!record.line"
                  @click="copyToClipboard(record.line, record.copyLabel)"
                >
                  复制
                </MiuixButton>
              </div>
            </div>
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
  max-width: 1080px;
}

.page-header,
.domain-head,
.header-actions,
.domain-actions {
  display: flex;
  gap: 12px;
}

.page-header,
.domain-head {
  align-items: flex-start;
  justify-content: space-between;
}

.page-header {
  margin-bottom: 22px;
}

.domains h1 {
  color: var(--m-color-text);
  font-size: 26px;
  font-weight: 700;
}

.subtitle,
.domain-meta {
  color: var(--m-color-text-secondary);
  font-size: 14px;
}

.subtitle {
  margin-top: 4px;
}

.header-actions,
.domain-actions {
  flex-shrink: 0;
}

.notice,
.loading,
.empty {
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
  color: var(--m-color-text-secondary);
  padding: 56px 20px;
}

.empty p {
  margin-bottom: 16px;
}

.domain-list {
  display: flex;
  flex-direction: column;
  gap: 14px;
}

.card-inner {
  padding: 22px;
}

.domain-title {
  min-width: 0;
}

.domain-name {
  color: var(--m-color-text);
  font-size: 18px;
  font-weight: 750;
  overflow-wrap: anywhere;
}

.domain-meta {
  margin-top: 3px;
}

.dns-list {
  display: flex;
  flex-direction: column;
  gap: 12px;
  margin-top: 12px;
}

.dns-toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  margin-top: 18px;
  padding: 12px 14px;
  border: 1px solid var(--m-color-border);
  border-radius: var(--app-radius);
  background: var(--m-color-bg);
}

.dns-statuses {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  min-width: 0;
}

.record-chip {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  min-height: 30px;
  padding: 4px 10px;
  border: 1px solid color-mix(in srgb, var(--m-color-border) 78%, transparent);
  border-radius: 999px;
  color: var(--m-color-text-secondary);
  background: var(--m-color-card);
  font-size: 12px;
  font-weight: 650;
}

.record-chip strong {
  color: var(--m-color-text);
  font-weight: 800;
}

.record-chip.ready {
  color: var(--app-success);
  background: color-mix(in srgb, var(--app-success) 10%, transparent);
  border-color: color-mix(in srgb, var(--app-success) 26%, transparent);
}

.dns-row {
  display: grid;
  grid-template-columns: minmax(170px, 0.65fr) minmax(0, 1.7fr) 132px;
  gap: 16px;
  align-items: stretch;
  padding: 14px;
  border: 1px solid var(--m-color-border);
  border-radius: var(--app-radius);
  background: var(--m-color-bg);
}

.dns-row.missing {
  border-style: dashed;
}

.record-summary {
  display: flex;
  gap: 10px;
  min-width: 0;
}

.record-type {
  flex-shrink: 0;
  width: 48px;
  height: 28px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border-radius: 999px;
  color: var(--m-color-primary);
  background: color-mix(in srgb, var(--m-color-primary) 12%, transparent);
  font-size: 12px;
  font-weight: 800;
}

.record-summary h2 {
  color: var(--m-color-text);
  font-size: 14px;
  font-weight: 750;
  line-height: 1.35;
}

.record-summary p,
.record-actions small {
  color: var(--m-color-text-secondary);
  font-size: 12px;
  line-height: 1.45;
}

.record-summary p {
  margin-top: 4px;
}

.record-values {
  display: grid;
  grid-template-columns: minmax(120px, 0.68fr) minmax(0, 1.32fr);
  gap: 10px;
  min-width: 0;
}

.record-field {
  min-width: 0;
}

.record-field span {
  display: block;
  margin-bottom: 5px;
  color: var(--m-color-text-secondary);
  font-size: 12px;
  font-weight: 650;
}

.record-field code,
.record-field em {
  display: block;
  min-height: 42px;
  padding: 10px;
  color: var(--m-color-text);
  background: var(--m-color-card);
  border: 1px solid color-mix(in srgb, var(--m-color-border) 70%, transparent);
  border-radius: var(--app-radius);
  font-family: Consolas, Monaco, monospace;
  font-size: 12px;
  font-style: normal;
  line-height: 1.45;
  overflow-wrap: anywhere;
  word-break: break-word;
}

.record-field em {
  color: var(--m-color-text-secondary);
  font-family: inherit;
}

.record-actions {
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  justify-content: space-between;
  gap: 10px;
}

.status {
  width: fit-content;
  padding: 4px 9px;
  border-radius: 999px;
  color: var(--m-color-text-secondary);
  background: var(--m-color-card);
  font-size: 12px;
  font-weight: 750;
}

.status.ready {
  color: var(--app-success);
  background: color-mix(in srgb, var(--app-success) 13%, transparent);
}

.add-form {
  padding: 8px 0;
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.add-form label {
  color: var(--m-color-text);
  font-size: 14px;
  font-weight: 650;
}

.hint {
  color: var(--m-color-text-secondary);
  font-size: 12px;
}

@media (max-width: 940px) {
  .dns-row {
    grid-template-columns: 1fr;
  }

  .record-actions {
    align-items: center;
    flex-direction: row;
  }
}

@media (max-width: 680px) {
  .page-header,
  .domain-head {
    align-items: stretch;
    flex-direction: column;
  }

  .header-actions,
  .domain-actions,
  .dns-toolbar,
  .record-actions {
    flex-wrap: wrap;
  }

  .header-actions > *,
  .domain-actions > *,
  .dns-toolbar > button,
  .record-actions > button {
    flex: 1;
  }

  .record-values {
    grid-template-columns: 1fr;
  }
}
</style>
