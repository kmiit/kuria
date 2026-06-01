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
const showHtml = ref(false)
const moveTarget = ref('')

const mailboxes = ['INBOX', 'Sent', 'Drafts', 'Trash', 'Spam']

const displayRecipients = computed(() => {
  if (!email.value) return ''
  try {
    const recips = JSON.parse(email.value.recipients)
    return Array.isArray(recips) ? recips.join(', ') : email.value.recipients
  } catch {
    return email.value.recipients
  }
})

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
  try {
    const data = await api.getEmail(route.params.id)
    email.value = data.email
    attachments.value = data.attachments || []
    // Default to HTML view if available
    showHtml.value = !!data.email.body_html
  } catch (e) {
    console.error(e)
  } finally {
    loading.value = false
  }
}

async function handleDelete() {
  if (!confirm('确定删除这封邮件？')) return
  try {
    await api.deleteEmail(email.value.id)
    router.push('/inbox')
  } catch (e) {
    console.error(e)
  }
}

async function handleMove(mailbox) {
  try {
    await api.moveEmail(email.value.id, mailbox)
    router.push('/inbox')
  } catch (e) {
    console.error(e)
  }
}

function downloadAttachment(att) {
  const token = localStorage.getItem('token')
  const url = api.getAttachmentUrl(att.id)
  // Create a temporary link with auth header workaround
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
      <MiuixButton @click="router.push('/inbox')">← 返回</MiuixButton>
      <div class="toolbar-actions">
        <select v-model="moveTarget" @change="handleMove(moveTarget)" class="move-select">
          <option value="" disabled>移动到...</option>
          <option v-for="m in mailboxes" :key="m" :value="m">{{ m }}</option>
        </select>
        <MiuixButton @click="router.push({ path: '/compose', query: { reply: email?.id } })">↩ 回复</MiuixButton>
        <MiuixButton @click="handleDelete">🗑️ 删除</MiuixButton>
      </div>
    </div>

    <div v-if="loading" class="loading">加载中...</div>

    <template v-else-if="email">
      <MiuixCard>
        <div class="card-inner">
          <h1 class="subject">{{ email.subject || '(无主题)' }}</h1>

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
              <span class="meta-value">{{ displayRecipients }}</span>
            </div>
            <div class="meta-row">
              <span class="meta-label">时间</span>
              <span class="meta-value">{{ formatDate(email.created_at) }}</span>
            </div>
            <div v-if="email.spf_result" class="meta-row">
              <span class="meta-label">认证</span>
              <span class="meta-value">
                <span class="auth-badge" :class="{ pass: email.spf_result === 'pass' }">
                  SPF: {{ email.spf_result }}
                </span>
              </span>
            </div>
          </div>
        </div>
      </MiuixCard>

      <!-- Attachments -->
      <MiuixCard v-if="attachments.length > 0">
        <div class="card-inner">
          <h3 class="section-title">📎 附件 ({{ attachments.length }})</h3>
          <div class="attachment-list">
            <div
              v-for="att in attachments"
              :key="att.id"
              class="attachment-item"
              @click="downloadAttachment(att)"
            >
              <span class="att-icon">📄</span>
              <div class="att-info">
                <span class="att-name">{{ att.filename || '未命名附件' }}</span>
                <span class="att-size">{{ formatSize(att.size) }}</span>
              </div>
              <span class="att-download">⬇</span>
            </div>
          </div>
        </div>
      </MiuixCard>

      <!-- Email Body -->
      <MiuixCard>
        <div class="card-inner">
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

          <div v-if="showHtml && email.body_html" class="html-body">
            <iframe
              :srcdoc="email.body_html"
              sandbox="allow-same-origin"
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
  max-width: 900px;
}

.toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 20px;
}

.toolbar-actions {
  display: flex;
  gap: 8px;
  align-items: center;
}

.move-select {
  padding: 8px 12px;
  border: 1px solid var(--m-color-border, #ddd);
  border-radius: 8px;
  font-size: 13px;
  background: var(--m-color-card);
  color: var(--m-color-text);
  cursor: pointer;
}

.loading {
  text-align: center;
  padding: 60px 20px;
  color: var(--m-color-text-secondary);
}

.card-inner {
  padding: 24px;
}

.subject {
  font-size: 22px;
  font-weight: 600;
  color: var(--m-color-text);
  margin-bottom: 20px;
  line-height: 1.3;
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
  padding-top: 2px;
}

.meta-value {
  font-size: 14px;
  color: var(--m-color-text);
  display: flex;
  align-items: center;
  gap: 8px;
}

.sender-avatar {
  width: 28px;
  height: 28px;
  border-radius: 50%;
  background: var(--m-color-primary);
  color: white;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  font-size: 13px;
  font-weight: 600;
  flex-shrink: 0;
}

.auth-badge {
  font-size: 12px;
  padding: 2px 8px;
  border-radius: 4px;
  background: #fee;
  color: #c33;
}

.auth-badge.pass {
  background: #efe;
  color: #363;
}

.section-title {
  font-size: 15px;
  font-weight: 600;
  color: var(--m-color-text);
  margin-bottom: 16px;
}

.attachment-list {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.attachment-item {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 12px;
  background: var(--m-color-bg);
  border-radius: 8px;
  cursor: pointer;
  transition: background 0.2s;
}

.attachment-item:hover {
  background: var(--m-color-hover);
}

.att-icon {
  font-size: 24px;
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
  font-size: 12px;
  color: var(--m-color-text-secondary);
}

.att-download {
  font-size: 18px;
  flex-shrink: 0;
}

.body-toggle {
  display: flex;
  gap: 4px;
  margin-bottom: 16px;
  padding: 4px;
  background: var(--m-color-bg);
  border-radius: 8px;
  width: fit-content;
}

.body-toggle .active {
  background: var(--m-color-primary);
  color: white;
}

.html-body {
  border: 1px solid var(--m-color-border);
  border-radius: 8px;
  overflow: hidden;
}

.html-frame {
  width: 100%;
  min-height: 400px;
  border: none;
  background: white;
}

.text-body {
  white-space: pre-wrap;
  line-height: 1.7;
  color: var(--m-color-text);
  font-size: 14px;
}
</style>
