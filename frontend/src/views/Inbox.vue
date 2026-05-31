<script setup>
import { ref, onMounted } from 'vue'
import { MiuixButton, MiuixCard, MiuixDialog } from 'miuix-vue'
import { api } from '../api'

const emails = ref([])
const loading = ref(true)
const selectedEmail = ref(null)
const showDialog = ref(false)

async function loadEmails() {
  loading.value = true
  try {
    const data = await api.getEmails('INBOX')
    emails.value = data.emails || []
  } catch (e) {
    console.error(e)
  } finally {
    loading.value = false
  }
}

async function openEmail(email) {
  try {
    const data = await api.getEmail(email.id)
    selectedEmail.value = data.email
    showDialog.value = true
    // Mark as read
    if (!email.is_read) {
      await api.markRead(email.id)
      email.is_read = true
    }
  } catch (e) {
    console.error(e)
  }
}

async function deleteEmail(id) {
  if (confirm('确定删除这封邮件？')) {
    try {
      await api.deleteEmail(id)
      emails.value = emails.value.filter((e) => e.id !== id)
    } catch (e) {
      console.error(e)
    }
  }
}

function formatDate(dateStr) {
  if (!dateStr) return ''
  const d = new Date(dateStr)
  const now = new Date()
  const diff = now - d
  if (diff < 86400000) {
    return d.toLocaleTimeString('zh-CN', { hour: '2-digit', minute: '2-digit' })
  }
  return d.toLocaleDateString('zh-CN', { month: 'short', day: 'numeric' })
}

onMounted(loadEmails)
</script>

<template>
  <div class="inbox">
    <div class="header">
      <h1>收件箱</h1>
      <MiuixButton @click="loadEmails">刷新</MiuixButton>
    </div>

    <div v-if="loading" class="loading">加载中...</div>

    <div v-else-if="emails.length === 0" class="empty">
      <div class="empty-icon">📭</div>
      <p>暂无邮件</p>
    </div>

    <div v-else class="email-list">
      <MiuixCard
        v-for="email in emails"
        :key="email.id"
        :class="{ unread: !email.is_read }"
        @click="openEmail(email)"
      >
        <div class="card-inner email-item">
          <div class="email-avatar">
            {{ email.sender?.charAt(0)?.toUpperCase() || '?' }}
          </div>
          <div class="email-content">
            <div class="email-header">
              <span class="email-sender">{{ email.sender }}</span>
              <span class="email-date">{{ formatDate(email.created_at) }}</span>
            </div>
            <div class="email-subject">{{ email.subject || '(无主题)' }}</div>
            <div class="email-preview">
              {{ (email.body_text || '').substring(0, 100) }}
            </div>
          </div>
          <MiuixButton
            class="delete-btn"
            @click.stop="deleteEmail(email.id)"
          >
            🗑️
          </MiuixButton>
        </div>
      </MiuixCard>
    </div>

    <!-- Email Detail Dialog -->
    <MiuixDialog v-model="showDialog" :title="selectedEmail?.subject || '(无主题)'">
      <div v-if="selectedEmail" class="email-detail">
        <div class="detail-header">
          <div><strong>发件人：</strong>{{ selectedEmail.sender }}</div>
          <div><strong>时间：</strong>{{ selectedEmail.created_at }}</div>
        </div>
        <div class="detail-body">
          {{ selectedEmail.body_text || '(无内容)' }}
        </div>
      </div>
      <template #footer="{ close }">
        <MiuixButton @click="close">关闭</MiuixButton>
      </template>
    </MiuixDialog>
  </div>
</template>

<style scoped>
.inbox h1 {
  font-size: 24px;
  font-weight: 600;
  color: var(--m-color-text);
}

.header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 24px;
}

.loading,
.empty {
  text-align: center;
  padding: 80px 20px;
  color: var(--m-color-text-secondary);
}

.empty-icon {
  font-size: 56px;
  margin-bottom: 20px;
}

.card-inner {
  padding: 20px;
}

.email-list {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.email-item {
  display: flex;
  align-items: center;
  gap: 16px;
  cursor: pointer;
}

.unread {
  border-left: 3px solid var(--m-color-primary);
}

.unread .email-sender,
.unread .email-subject {
  font-weight: 600;
}

.email-avatar {
  width: 44px;
  height: 44px;
  border-radius: 50%;
  background: var(--m-color-primary);
  color: white;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 18px;
  font-weight: 600;
  flex-shrink: 0;
}

.email-content {
  flex: 1;
  min-width: 0;
}

.email-header {
  display: flex;
  justify-content: space-between;
  margin-bottom: 4px;
}

.email-sender {
  font-size: 14px;
  color: var(--m-color-text);
}

.email-date {
  font-size: 12px;
  color: var(--m-color-text-secondary);
  flex-shrink: 0;
}

.email-subject {
  font-size: 15px;
  color: var(--m-color-text);
  margin-bottom: 4px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.email-preview {
  font-size: 13px;
  color: var(--m-color-text-secondary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.delete-btn {
  opacity: 0;
  transition: opacity 0.2s;
  flex-shrink: 0;
}

.email-item:hover .delete-btn {
  opacity: 1;
}

.email-detail {
  padding: 8px 0;
}

.detail-header {
  margin-bottom: 16px;
  font-size: 14px;
  color: var(--m-color-text-secondary);
}

.detail-header div {
  margin-bottom: 4px;
}

.detail-body {
  white-space: pre-wrap;
  line-height: 1.6;
  color: var(--m-color-text);
}
</style>
