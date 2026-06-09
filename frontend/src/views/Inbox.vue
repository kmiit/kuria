<script setup>
import { ref, onMounted, watch, computed } from 'vue'
import { useRouter, useRoute } from 'vue-router'
import { MiuixButton, MiuixCard, MiuixInput } from 'miuix-vue'
import { api } from '../api'

const router = useRouter()
const route = useRoute()

const emails = ref([])
const loading = ref(true)
const error = ref('')
const searchQuery = ref('')
const searchInput = ref('')
const currentMailbox = ref('INBOX')
const page = ref(1)
const total = ref(0)
const selectedIds = ref([])
const bulkMoveTarget = ref('')
const perPage = 50

const mailboxTabs = [
  { id: 'INBOX', name: '收件箱', icon: '📥' },
  { id: 'Sent', name: '已发送', icon: '📤' },
  { id: 'Drafts', name: '草稿', icon: '📝' },
  { id: 'Trash', name: '垃圾箱', icon: '🗑️' },
  { id: 'Spam', name: '垃圾邮件', icon: '⚠️' },
]

const totalPages = computed(() => Math.ceil(total.value / perPage) || 1)
const selectedCount = computed(() => selectedIds.value.length)
const allSelected = computed(() =>
  emails.value.length > 0 && selectedIds.value.length === emails.value.length,
)
const pageStart = computed(() => (total.value === 0 ? 0 : (page.value - 1) * perPage + 1))
const pageEnd = computed(() => Math.min(page.value * perPage, total.value))
const currentMailboxLabel = computed(() =>
  mailboxTabs.find((m) => m.id === currentMailbox.value)?.name || '收件箱',
)

async function loadEmails() {
  loading.value = true
  error.value = ''
  selectedIds.value = []
  bulkMoveTarget.value = ''
  try {
    let data
    if (searchQuery.value) {
      data = await api.searchEmails(searchQuery.value, page.value, perPage)
    } else {
      data = await api.getEmails(currentMailbox.value, page.value, perPage)
    }
    emails.value = data.emails || []
    total.value = data.total || 0
  } catch (e) {
    emails.value = []
    total.value = 0
    error.value = e.message || '加载邮件失败'
  } finally {
    loading.value = false
  }
}

function openEmail(email) {
  router.push(`/email/${email.id}`)
}

async function deleteEmail(id) {
  if (!confirm('确定删除这封邮件？')) return
  try {
    await api.deleteEmail(id)
    emails.value = emails.value.filter((e) => e.id !== id)
    selectedIds.value = selectedIds.value.filter((selected) => selected !== id)
    total.value = Math.max(0, total.value - 1)
  } catch (e) {
    error.value = e.message || '删除失败'
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
  page.value = Math.min(Math.max(1, p), totalPages.value)
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

function primaryLine(email) {
  if (currentMailbox.value === 'Sent') return `发给 ${parseRecipients(email.recipients)}`
  return email.sender
}

function toggleSelection(id) {
  if (selectedIds.value.includes(id)) {
    selectedIds.value = selectedIds.value.filter((selected) => selected !== id)
  } else {
    selectedIds.value = [...selectedIds.value, id]
  }
}

function toggleSelectAll() {
  selectedIds.value = allSelected.value ? [] : emails.value.map((email) => email.id)
}

async function bulkMarkRead() {
  if (!selectedCount.value) return
  try {
    await Promise.all(selectedIds.value.map((id) => api.markRead(id)))
    emails.value = emails.value.map((email) =>
      selectedIds.value.includes(email.id) ? { ...email, is_read: true } : email,
    )
    selectedIds.value = []
  } catch (e) {
    error.value = e.message || '标记已读失败'
  }
}

async function bulkDelete() {
  if (!selectedCount.value) return
  if (!confirm(`确定删除选中的 ${selectedCount.value} 封邮件？`)) return
  const ids = [...selectedIds.value]
  try {
    await Promise.all(ids.map((id) => api.deleteEmail(id)))
    emails.value = emails.value.filter((email) => !ids.includes(email.id))
    total.value = Math.max(0, total.value - ids.length)
    selectedIds.value = []
  } catch (e) {
    error.value = e.message || '批量删除失败'
  }
}

async function bulkMove() {
  if (!selectedCount.value || !bulkMoveTarget.value) return
  const ids = [...selectedIds.value]
  try {
    await Promise.all(ids.map((id) => api.moveEmail(id, bulkMoveTarget.value)))
    if (!searchQuery.value && bulkMoveTarget.value !== currentMailbox.value) {
      emails.value = emails.value.filter((email) => !ids.includes(email.id))
      total.value = Math.max(0, total.value - ids.length)
    }
    selectedIds.value = []
    bulkMoveTarget.value = ''
  } catch (e) {
    error.value = e.message || '移动邮件失败'
  }
}

onMounted(() => {
  if (route.query.mailbox) {
    currentMailbox.value = route.query.mailbox
  }
  loadEmails()
})

watch(() => route.query.mailbox, (mb) => {
  const nextMailbox = mb || 'INBOX'
  if (nextMailbox === currentMailbox.value) return
  currentMailbox.value = nextMailbox
  searchInput.value = ''
  searchQuery.value = ''
  page.value = 1
  loadEmails()
})
</script>

<template>
  <div class="inbox">
    <div class="page-header">
      <div>
        <h1>{{ searchQuery ? '搜索结果' : currentMailboxLabel }}</h1>
        <p class="subtitle">
          <template v-if="searchQuery">正在搜索“{{ searchQuery }}”</template>
          <template v-else>{{ total }} 封邮件</template>
        </p>
      </div>
      <div class="header-actions">
        <MiuixButton @click="loadEmails">刷新</MiuixButton>
        <MiuixButton type="primary" @click="router.push('/compose')">写邮件</MiuixButton>
      </div>
    </div>

    <div v-if="error" class="notice error">{{ error }}</div>

    <div class="search-bar">
      <MiuixInput
        v-model="searchInput"
        placeholder="搜索主题、发件人或内容"
        @keyup.enter="doSearch"
      />
      <MiuixButton @click="doSearch">搜索</MiuixButton>
      <MiuixButton v-if="searchQuery" @click="clearSearch">清除</MiuixButton>
    </div>

    <div v-if="!searchQuery" class="mailbox-tabs">
      <button
        v-for="mb in mailboxTabs"
        :key="mb.id"
        class="tab"
        :class="{ active: currentMailbox === mb.id }"
        type="button"
        @click="switchMailbox(mb.id)"
      >
        <span class="tab-icon">{{ mb.icon }}</span>
        <span class="tab-name">{{ mb.name }}</span>
      </button>
    </div>

    <div v-if="selectedCount" class="bulk-bar">
      <label class="select-all">
        <input type="checkbox" :checked="allSelected" @change="toggleSelectAll" />
        已选 {{ selectedCount }} 封
      </label>
      <div class="bulk-actions">
        <MiuixButton @click="bulkMarkRead">标记已读</MiuixButton>
        <select v-model="bulkMoveTarget" class="bulk-select" @change="bulkMove">
          <option value="">移动到...</option>
          <option
            v-for="mb in mailboxTabs"
            :key="mb.id"
            :value="mb.id"
            :disabled="mb.id === currentMailbox"
          >
            {{ mb.name }}
          </option>
        </select>
        <MiuixButton @click="bulkDelete">删除</MiuixButton>
      </div>
    </div>
    <label v-else-if="emails.length" class="select-all idle">
      <input type="checkbox" :checked="allSelected" @change="toggleSelectAll" />
      选择本页
    </label>

    <div v-if="loading" class="loading">加载中...</div>

    <div v-else-if="emails.length === 0" class="empty">
      <div class="empty-icon">📭</div>
      <p>{{ searchQuery ? '没有找到匹配的邮件' : '暂无邮件' }}</p>
      <MiuixButton v-if="searchQuery" @click="clearSearch">返回 {{ currentMailboxLabel }}</MiuixButton>
    </div>

    <div v-else>
      <div class="email-list">
        <MiuixCard
          v-for="email in emails"
          :key="email.id"
          :class="{ unread: !email.is_read, selected: selectedIds.includes(email.id) }"
        >
          <div class="card-inner email-item" @click="openEmail(email)">
            <input
              class="email-check"
              type="checkbox"
              :checked="selectedIds.includes(email.id)"
              @click.stop
              @change="toggleSelection(email.id)"
            />
            <div class="email-avatar">
              {{ primaryLine(email)?.charAt(0)?.toUpperCase() || '?' }}
            </div>
            <div class="email-content">
              <div class="email-header">
                <span class="email-sender">{{ primaryLine(email) }}</span>
                <span class="email-date">{{ formatDate(email.created_at) }}</span>
              </div>
              <div class="email-subject">{{ email.subject || '(无主题)' }}</div>
              <div class="email-preview">
                {{ (email.body_text || '').substring(0, 140) || '无正文预览' }}
              </div>
            </div>
            <MiuixButton
              class="delete-btn"
              title="删除"
              @click.stop="deleteEmail(email.id)"
            >
              删除
            </MiuixButton>
          </div>
        </MiuixCard>
      </div>

      <div class="pagination">
        <span class="page-range">显示 {{ pageStart }}-{{ pageEnd }} / {{ total }}</span>
        <div class="page-actions">
          <MiuixButton :disabled="page <= 1" @click="goPage(page - 1)">上一页</MiuixButton>
          <span class="page-info">{{ page }} / {{ totalPages }}</span>
          <MiuixButton :disabled="page >= totalPages" @click="goPage(page + 1)">下一页</MiuixButton>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.inbox {
  max-width: 1120px;
}

.page-header {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 16px;
  margin-bottom: 18px;
}

.inbox h1 {
  font-size: 26px;
  font-weight: 700;
  color: var(--m-color-text);
}

.subtitle {
  margin-top: 4px;
  font-size: 14px;
  color: var(--m-color-text-secondary);
}

.header-actions {
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

.notice.error {
  color: var(--app-danger);
  border: 1px solid color-mix(in srgb, var(--app-danger) 32%, transparent);
}

.search-bar {
  display: grid;
  grid-template-columns: minmax(0, 1fr) auto auto;
  gap: 8px;
  margin-bottom: 14px;
  align-items: center;
}

.mailbox-tabs {
  display: flex;
  gap: 4px;
  margin-bottom: 14px;
  padding: 4px;
  background: var(--m-color-card);
  border: 1px solid var(--m-color-border);
  border-radius: var(--app-radius);
  overflow-x: auto;
}

.tab {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 8px 14px;
  border: 0;
  border-radius: 6px;
  cursor: pointer;
  font-size: 13px;
  color: var(--m-color-text-secondary);
  background: transparent;
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

.bulk-bar,
.select-all.idle {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  margin-bottom: 12px;
  padding: 10px 12px;
  border-radius: var(--app-radius);
  background: var(--m-color-card);
  border: 1px solid var(--m-color-border);
}

.select-all {
  display: flex;
  align-items: center;
  gap: 8px;
  color: var(--m-color-text-secondary);
  font-size: 13px;
}

.bulk-actions {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}

.bulk-select {
  min-height: 36px;
  padding: 0 10px;
  background: var(--m-color-card);
  color: var(--m-color-text);
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
  padding: 16px 18px;
}

.email-list {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.email-item {
  display: flex;
  align-items: center;
  gap: 14px;
  cursor: pointer;
}

.selected {
  outline: 2px solid var(--app-focus);
}

.unread {
  border-left: 3px solid var(--m-color-primary);
}

.unread .email-sender,
.unread .email-subject {
  font-weight: 700;
}

.email-check {
  width: 16px;
  height: 16px;
  flex-shrink: 0;
}

.email-avatar {
  width: 42px;
  height: 42px;
  border-radius: 50%;
  background: var(--m-color-primary);
  color: white;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 17px;
  font-weight: 700;
  flex-shrink: 0;
}

.email-content {
  flex: 1;
  min-width: 0;
}

.email-header {
  display: flex;
  justify-content: space-between;
  gap: 12px;
  margin-bottom: 3px;
}

.email-sender {
  font-size: 14px;
  color: var(--m-color-text);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.email-date {
  font-size: 12px;
  color: var(--m-color-text-secondary);
  flex-shrink: 0;
}

.email-subject {
  font-size: 15px;
  color: var(--m-color-text);
  margin-bottom: 3px;
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

.email-item:hover .delete-btn,
.email-item:focus-within .delete-btn {
  opacity: 1;
}

.pagination {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  margin-top: 20px;
  padding: 16px 0;
}

.page-actions {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 12px;
}

.page-range,
.page-info {
  font-size: 13px;
  color: var(--m-color-text-secondary);
  white-space: nowrap;
}

@media (max-width: 720px) {
  .page-header {
    flex-direction: column;
  }

  .header-actions,
  .search-bar,
  .bulk-bar,
  .pagination {
    width: 100%;
  }

  .header-actions {
    display: grid;
    grid-template-columns: 1fr 1fr;
  }

  .search-bar {
    grid-template-columns: 1fr;
  }

  .bulk-bar,
  .pagination {
    align-items: stretch;
    flex-direction: column;
  }

  .bulk-actions,
  .page-actions {
    justify-content: space-between;
  }

  .email-avatar {
    display: none;
  }

  .delete-btn {
    opacity: 1;
  }
}
</style>
