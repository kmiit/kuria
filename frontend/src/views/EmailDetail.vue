<script setup>
import { ref, onMounted, computed } from 'vue'
import { useRouter, useRoute } from 'vue-router'
import { MiuixButton, MiuixCard } from 'miuix-vue'
import { api } from '../api'

const router = useRouter()
const route = useRoute()

const email = ref(null)
const attachments = ref([])
const loading = ref(true)
const error = ref('')
const actionMessage = ref('')
const showHtml = ref(false)
const moveTarget = ref('')

const mailboxes = [
  { id: 'INBOX', name: '收件箱' },
  { id: 'Sent', name: '已发送' },
  { id: 'Drafts', name: '草稿' },
  { id: 'Trash', name: '垃圾箱' },
  { id: 'Spam', name: '垃圾邮件' },
]

const displayRecipients = computed(() => {
  if (!email.value) return ''
  try {
    const recips = JSON.parse(email.value.recipients)
    return Array.isArray(recips) ? recips.join(', ') : email.value.recipients
  } catch {
    return email.value.recipients
  }
})

const authResults = computed(() => {
  if (!email.value) return []
  return [
    { label: 'SPF', value: email.value.spf_result },
    { label: 'DKIM', value: email.value.dkim_signature ? 'pass' : '' },
    { label: 'DMARC', value: email.value.dmarc_result },
  ].filter((item) => item.value)
})

function mailboxName(id) {
  return mailboxes.find((m) => m.id === id)?.name || id || '收件箱'
}

function backToMailbox(mailbox = email.value?.mailbox || route.query.mailbox || 'INBOX') {
  router.push({ path: '/inbox', query: { mailbox } })
}

function formatDate(dateStr) {
  if (!dateStr) return ''
  const d = new Date(dateStr)
  return d.toLocaleString('zh-CN', {
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
  })
}

function formatSize(bytes) {
  if (!bytes) return ''
  if (bytes < 1024) return bytes + ' B'
  if (bytes < 1024 * 1024) return (bytes / 1024).toFixed(1) + ' KB'
  return (bytes / (1024 * 1024)).toFixed(1) + ' MB'
}

async function loadEmail() {
  loading.value = true
  error.value = ''
  actionMessage.value = ''
  try {
    const data = await api.getEmail(route.params.id)
    email.value = data.email
    attachments.value = data.attachments || []
    showHtml.value = !!data.email.body_html
  } catch (e) {
    error.value = e.message || '加载邮件失败'
  } finally {
    loading.value = false
  }
}

async function handleDelete() {
  if (!confirm('确定删除这封邮件？')) return
  try {
    await api.deleteEmail(email.value.id)
    backToMailbox()
  } catch (e) {
    actionMessage.value = e.message || '删除失败'
  }
}

async function handleMove(mailbox) {
  if (!mailbox || !email.value) return
  try {
    await api.moveEmail(email.value.id, mailbox)
    actionMessage.value = `已移动到${mailboxName(mailbox)}`
    backToMailbox(mailbox)
  } catch (e) {
    actionMessage.value = e.message || '移动失败'
  }
}

function replyEmail() {
  router.push({ path: '/compose', query: { reply: email.value?.id } })
}

function forwardEmail() {
  router.push({ path: '/compose', query: { forward: email.value?.id } })
}

function downloadAttachment(att) {
  const token = localStorage.getItem('token')
  const url = api.getAttachmentUrl(att.id)
  const a = document.createElement('a')
  a.href = url + '?token=' + token
  a.download = att.filename || 'attachment'
  a.target = '_blank'
  document.body.appendChild(a)
  a.click()
  document.body.removeChild(a)
}

onMounted(loadEmail)
</script>

<template>
  <div class="email-detail">
    <div class="toolbar">
      <MiuixButton @click="backToMailbox()">← 返回</MiuixButton>
      <div class="toolbar-actions">
        <select v-model="moveTarget" class="move-select" @change="handleMove(moveTarget)">
          <option value="" disabled>移动到...</option>
          <option
            v-for="m in mailboxes"
            :key="m.id"
            :value="m.id"
            :disabled="m.id === email?.mailbox"
          >
            {{ m.name }}
          </option>
        </select>
        <MiuixButton :disabled="!email" @click="replyEmail">回复</MiuixButton>
        <MiuixButton :disabled="!email" @click="forwardEmail">转发</MiuixButton>
        <MiuixButton :disabled="!email" @click="handleDelete">删除</MiuixButton>
      </div>
    </div>

    <div v-if="error" class="notice error">{{ error }}</div>
    <div v-if="actionMessage" class="notice">{{ actionMessage }}</div>
    <div v-if="loading" class="loading">加载中...</div>

    <template v-else-if="email">
      <MiuixCard>
        <div class="card-inner">
          <div class="subject-row">
            <div>
              <h1 class="subject">{{ email.subject || '(无主题)' }}</h1>
              <p class="mailbox-line">{{ mailboxName(email.mailbox) }} · {{ formatDate(email.created_at) }}</p>
            </div>
            <span v-if="!email.is_read" class="unread-pill">未读</span>
          </div>

          <div class="meta">
            <div class="meta-row">
              <span class="meta-label">发件人</span>
              <span class="meta-value">
                <span class="sender-avatar">{{ email.sender?.charAt(0)?.toUpperCase() || '?' }}</span>
                {{ email.sender }}
              </span>
            </div>
            <div class="meta-row">
              <span class="meta-label">收件人</span>
              <span class="meta-value recipients">{{ displayRecipients }}</span>
            </div>
            <div v-if="authResults.length" class="meta-row">
              <span class="meta-label">认证</span>
              <span class="meta-value auth-list">
                <span
                  v-for="item in authResults"
                  :key="item.label"
                  class="auth-badge"
                  :class="{ pass: item.value === 'pass' }"
                >
                  {{ item.label }}: {{ item.value }}
                </span>
              </span>
            </div>
          </div>
        </div>
      </MiuixCard>

      <MiuixCard v-if="attachments.length > 0">
        <div class="card-inner">
          <h3 class="section-title">附件 ({{ attachments.length }})</h3>
          <div class="attachment-list">
            <button
              v-for="att in attachments"
              :key="att.id"
              class="attachment-item"
              type="button"
              @click="downloadAttachment(att)"
            >
              <span class="att-icon">📄</span>
              <span class="att-info">
                <span class="att-name">{{ att.filename || '未命名附件' }}</span>
                <span class="att-size">{{ formatSize(att.size) }}</span>
              </span>
              <span class="att-download">下载</span>
            </button>
          </div>
        </div>
      </MiuixCard>

      <MiuixCard>
        <div class="card-inner">
          <div class="body-header">
            <h3 class="section-title">正文</h3>
            <div class="body-toggle" v-if="email.body_html && email.body_text">
              <MiuixButton
                :class="{ active: !showHtml }"
                @click="showHtml = false"
              >纯文本</MiuixButton>
              <MiuixButton
                :class="{ active: showHtml }"
                @click="showHtml = true"
              >HTML</MiuixButton>
            </div>
          </div>

          <div v-if="showHtml && email.body_html" class="html-body">
            <iframe
              :srcdoc="email.body_html"
              sandbox=""
              class="html-frame"
            ></iframe>
          </div>
          <div v-else class="text-body">
            {{ email.body_text || '(无内容)' }}
          </div>
        </div>
      </MiuixCard>
    </template>
  </div>
</template>

<style scoped>
.email-detail {
  max-width: 920px;
}

.toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  margin-bottom: 18px;
}

.toolbar-actions {
  display: flex;
  gap: 8px;
  align-items: center;
  flex-wrap: wrap;
  justify-content: flex-end;
}

.move-select {
  min-height: 36px;
  padding: 0 12px;
  background: var(--m-color-card);
  color: var(--m-color-text);
  cursor: pointer;
}

.notice,
.loading {
  padding: 14px;
  border-radius: var(--app-radius);
  margin-bottom: 14px;
  background: var(--m-color-card);
  color: var(--m-color-text-secondary);
}

.notice.error {
  color: var(--app-danger);
  border: 1px solid color-mix(in srgb, var(--app-danger) 32%, transparent);
}

.card-inner {
  padding: 24px;
}

.subject-row {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 16px;
  margin-bottom: 20px;
}

.subject {
  font-size: 24px;
  font-weight: 700;
  color: var(--m-color-text);
  line-height: 1.3;
  overflow-wrap: anywhere;
}

.mailbox-line {
  margin-top: 6px;
  color: var(--m-color-text-secondary);
  font-size: 13px;
}

.unread-pill {
  flex-shrink: 0;
  color: white;
  background: var(--m-color-primary);
  border-radius: 999px;
  padding: 4px 10px;
  font-size: 12px;
  font-weight: 700;
}

.meta {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.meta-row {
  display: flex;
  align-items: flex-start;
  gap: 12px;
}

.meta-label {
  font-size: 13px;
  color: var(--m-color-text-secondary);
  min-width: 60px;
  flex-shrink: 0;
  padding-top: 5px;
}

.meta-value {
  font-size: 14px;
  color: var(--m-color-text);
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
}

.recipients {
  overflow-wrap: anywhere;
}

.sender-avatar {
  width: 30px;
  height: 30px;
  border-radius: 50%;
  background: var(--m-color-primary);
  color: white;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  font-size: 13px;
  font-weight: 700;
  flex-shrink: 0;
}

.auth-list {
  flex-wrap: wrap;
}

.auth-badge {
  font-size: 12px;
  padding: 3px 8px;
  border-radius: 999px;
  background: color-mix(in srgb, var(--app-warning) 18%, transparent);
  color: var(--app-warning);
}

.auth-badge.pass {
  background: color-mix(in srgb, var(--app-success) 16%, transparent);
  color: var(--app-success);
}

.section-title {
  font-size: 16px;
  font-weight: 650;
  color: var(--m-color-text);
}

.attachment-list {
  display: flex;
  flex-direction: column;
  gap: 8px;
  margin-top: 14px;
}

.attachment-item {
  display: flex;
  align-items: center;
  gap: 12px;
  width: 100%;
  padding: 12px;
  background: var(--m-color-bg);
  border: 1px solid transparent;
  border-radius: var(--app-radius);
  cursor: pointer;
  color: inherit;
  text-align: left;
  transition: background 0.2s, border-color 0.2s;
}

.attachment-item:hover {
  background: var(--m-color-hover);
  border-color: var(--m-color-border);
}

.att-icon {
  font-size: 22px;
  flex-shrink: 0;
}

.att-info {
  flex: 1;
  min-width: 0;
}

.att-name {
  display: block;
  font-size: 14px;
  color: var(--m-color-text);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.att-size {
  display: block;
  font-size: 12px;
  color: var(--m-color-text-secondary);
}

.att-download {
  font-size: 13px;
  color: var(--m-color-primary);
  font-weight: 650;
  flex-shrink: 0;
}

.body-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  margin-bottom: 16px;
}

.body-toggle {
  display: flex;
  gap: 4px;
  padding: 4px;
  background: var(--m-color-bg);
  border-radius: var(--app-radius);
}

.body-toggle .active {
  background: var(--m-color-primary);
  color: white;
}

.html-body {
  border: 1px solid var(--m-color-border);
  border-radius: var(--app-radius);
  overflow: hidden;
}

.html-frame {
  width: 100%;
  min-height: 480px;
  border: none;
  background: white;
}

.text-body {
  white-space: pre-wrap;
  line-height: 1.75;
  color: var(--m-color-text);
  font-size: 14px;
  overflow-wrap: anywhere;
}

@media (max-width: 680px) {
  .toolbar,
  .subject-row,
  .body-header {
    align-items: stretch;
    flex-direction: column;
  }

  .toolbar-actions {
    justify-content: flex-start;
  }

  .move-select {
    width: 100%;
  }

  .meta-row {
    flex-direction: column;
    gap: 6px;
  }

  .meta-label {
    padding-top: 0;
  }
}
</style>
