<script setup>
import { ref, onMounted, watch, computed } from 'vue'
import { useRouter, useRoute } from 'vue-router'
import { MiuixButton, MiuixCard, MiuixInput } from 'miuix-vue'
import { api } from '../api'

const router = useRouter()
const route = useRoute()

const emails = ref([])
const loading = ref(true)
const searchQuery = ref('')
const searchInput = ref('')
const currentMailbox = ref('INBOX')
const page = ref(1)
const total = ref(0)
const perPage = 50

const mailboxTabs = [
  { id: 'INBOX', name: '收件箱', icon: '📥' },
  { id: 'Sent', name: '已发送', icon: '📤' },
  { id: 'Drafts', name: '草稿', icon: '📝' },
  { id: 'Trash', name: '垃圾箱', icon: '🗑️' },
  { id: 'Spam', name: '垃圾邮件', icon: '⚠️' },
]

const totalPages = computed(() => Math.ceil(total.value / perPage) || 1)

async function loadEmails() {
  loading.value = true
  try {
    let data
    if (searchQuery.value) {
      data = await api.searchEmails(searchQuery.value, page.value)
    } else {
      data = await api.getEmails(currentMailbox.value, page.value)
    }
    emails.value = data.emails || []
    total.value = data.total || 0
  } catch (e) {
    console.error(e)
  } finally {
    loading.value = false
  }
}

function openEmail(email) {
  router.push(`/email/${email.id}`)
}

async function deleteEmail(id) {
  if (confirm('确定删除这封邮件？')) {
    try {
      await api.deleteEmail(id)
      emails.value = emails.value.filter((e) => e.id !== id)
      total.value--
    } catch (e) {
      console.error(e)
    }
  }
}

function doSearch() {
  searchQuery.value = searchInput.value.trim()
  page.value = 1
  loadEmails()
}

function clearSearch() {
  searchInput.value = ''
  searchQuery.value = ''
  page.value = 1
  loadEmails()
}

function switchMailbox(id) {
  currentMailbox.value = id
  searchInput.value = ''
  searchQuery.value = ''
  page.value = 1
  router.replace({ query: { mailbox: id } })
  loadEmails()
}

function goPage(p) {
  page.value = p
  loadEmails()
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

function parseRecipients(recipients) {
  try {
    const arr = JSON.parse(recipients)
    return Array.isArray(arr) ? arr.join(', ') : recipients
  } catch {
    return recipients
  }
}

onMounted(() => {
  if (route.query.mailbox) {
    currentMailbox.value = route.query.mailbox
  }
  loadEmails()
})

watch(() => route.query.mailbox, (mb) => {
  if (mb && mb !== currentMailbox.value) {
    currentMailbox.value = mb
    searchInput.value = ''
    searchQuery.value = ''
    page.value = 1
    loadEmails()
  }
})
</script>

<template>
  <div class="inbox">
    <div class="header">
      <h1>{{ searchQuery ? '搜索结果' : (mailboxTabs.find(m => m.id === currentMailbox)?.name || '收件箱') }}</h1>
      <MiuixButton @click="loadEmails">刷新</MiuixButton>
    </div>

    <!-- Search bar -->
    <div class="search-bar">
      <MiuixInput
        v-model="searchInput"
        placeholder="搜索邮件主题、发件人、内容..."
        @keyup.enter="doSearch"
      />
      <MiuixButton @click="doSearch">搜索</MiuixButton>
      <MiuixButton v-if="searchQuery" @click="clearSearch">清除</MiuixButton>
    </div>

    <!-- Mailbox tabs -->
    <div v-if="!searchQuery" class="mailbox-tabs">
      <div
        v-for="mb in mailboxTabs"
        :key="mb.id"
        class="tab"
        :class="{ active: currentMailbox === mb.id }"
        @click="switchMailbox(mb.id)"
      >
        <span class="tab-icon">{{ mb.icon }}</span>
        <span class="tab-name">{{ mb.name }}</span>
      </div>
    </div>

    <div v-if="loading" class="loading">加载中...</div>

    <div v-else-if="emails.length === 0" class="empty">
      <div class="empty-icon">📭</div>
      <p>{{ searchQuery ? '没有找到匹配的邮件' : '暂无邮件' }}</p>
    </div>

    <div v-else>
      <div class="email-list">
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
                {{ (email.body_text || '').substring(0, 120) }}
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

      <!-- Pagination -->
      <div v-if="totalPages > 1" class="pagination">
        <MiuixButton :disabled="page <= 1" @click="goPage(page - 1)">上一页</MiuixButton>
        <span class="page-info">{{ page }} / {{ totalPages }}</span>
        <MiuixButton :disabled="page >= totalPages" @click="goPage(page + 1)">下一页</MiuixButton>
      </div>
    </div>
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
  margin-bottom: 16px;
}

.search-bar {
  display: flex;
  gap: 8px;
  margin-bottom: 16px;
  align-items: center;
}

.search-bar > :first-child {
  flex: 1;
}

.mailbox-tabs {
  display: flex;
  gap: 4px;
  margin-bottom: 20px;
  padding: 4px;
  background: var(--m-color-card);
  border-radius: 12px;
  overflow-x: auto;
}

.tab {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 8px 16px;
  border-radius: 8px;
  cursor: pointer;
  font-size: 13px;
  color: var(--m-color-text-secondary);
  transition: all 0.2s;
  white-space: nowrap;
}

.tab:hover {
  background: var(--m-color-hover);
  color: var(--m-color-text);
}

.tab.active {
  background: var(--m-color-primary);
  color: white;
}

.tab-icon {
  font-size: 16px;
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

.pagination {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 16px;
  margin-top: 24px;
  padding: 16px 0;
}

.page-info {
  font-size: 14px;
  color: var(--m-color-text-secondary);
}
</style>
