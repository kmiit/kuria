<script setup>
import { ref, onMounted, computed } from 'vue'
import { MiuixButton, MiuixInput, MiuixCard, MiuixDialog, MiuixSwitch } from 'miuix-vue'
import { api } from '../api'
import PasswordInput from '../components/PasswordInput.vue'

const users = ref([])
const domains = ref([])
const loading = ref(true)
const saving = ref(false)
const showAddDialog = ref(false)
const search = ref('')
const message = ref('')
const error = ref('')

const newUser = ref({
  email: '',
  password: '',
  domain_id: 0,
  is_admin: false,
})

const currentUser = computed(() => {
  try {
    return JSON.parse(localStorage.getItem('user') || '{}')
  } catch {
    return {}
  }
})

const filteredUsers = computed(() => {
  const q = search.value.trim().toLowerCase()
  if (!q) return users.value
  return users.value.filter((user) =>
    user.email.toLowerCase().includes(q) ||
    getDomainName(user.domain_id).toLowerCase().includes(q),
  )
})

async function loadData() {
  loading.value = true
  error.value = ''
  try {
    const [u, d] = await Promise.all([api.getUsers(), api.getDomains()])
    users.value = u.users || []
    domains.value = d.domains || []
    if (domains.value.length > 0 && !newUser.value.domain_id) {
      newUser.value.domain_id = domains.value[0].id
    }
  } catch (e) {
    error.value = e.message || '加载用户失败'
  } finally {
    loading.value = false
  }
}

function resetUserForm() {
  newUser.value = {
    email: '',
    password: '',
    domain_id: domains.value[0]?.id || 0,
    is_admin: false,
  }
}

function generatePassword() {
  const chars = 'ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz23456789!@#$%+='
  newUser.value.password = Array.from({ length: 14 }, () =>
    chars[Math.floor(Math.random() * chars.length)],
  ).join('')
}

function isValidEmail(value) {
  return /^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(value)
}

async function addUser() {
  message.value = ''
  error.value = ''
  if (!domains.value.length) {
    error.value = '请先添加域名'
    return
  }
  if (!isValidEmail(newUser.value.email)) {
    error.value = '请输入有效邮箱地址'
    return
  }
  if (newUser.value.password.length < 6) {
    error.value = '密码至少需要 6 个字符'
    return
  }
  saving.value = true
  try {
    await api.createUser(newUser.value)
    showAddDialog.value = false
    resetUserForm()
    message.value = '用户已创建'
    await loadData()
  } catch (e) {
    if (e.status === 400) {
      error.value = '创建失败：邮箱、密码或所属域名不符合要求'
    } else {
      error.value = '创建失败：' + (e.message || '未知错误')
    }
  } finally {
    saving.value = false
  }
}

async function deleteUser(id, email) {
  message.value = ''
  error.value = ''
  if (id === currentUser.value.id) {
    error.value = '不能删除当前登录账号'
    return
  }
  if (!confirm(`确定删除用户 ${email}？`)) return
  try {
    await api.deleteUser(id)
    users.value = users.value.filter((u) => u.id !== id)
    message.value = '用户已删除'
  } catch (e) {
    if (e.status === 404) {
      error.value = '用户不存在或已被删除'
      await loadData()
    } else if (e.status === 400) {
      error.value = '不能删除当前登录账号'
    } else {
      error.value = e.message || '删除失败'
    }
  }
}

function getDomainName(domainId) {
  const d = domains.value.find((d) => d.id === domainId)
  return d ? d.domain_name : `ID: ${domainId}`
}

function formatDate(dateStr) {
  if (!dateStr) return '未知时间'
  return new Date(dateStr).toLocaleDateString('zh-CN')
}

onMounted(loadData)
</script>

<template>
  <div class="users">
    <div class="page-header">
      <div>
        <h1>用户管理</h1>
        <p class="subtitle">创建邮箱账号、分配域名与管理员权限。</p>
      </div>
      <div class="header-actions">
        <MiuixButton @click="loadData">刷新</MiuixButton>
        <MiuixButton type="primary" :disabled="!domains.length" @click="showAddDialog = true">添加用户</MiuixButton>
      </div>
    </div>

    <p v-if="message" class="notice success">{{ message }}</p>
    <p v-if="error" class="notice error">{{ error }}</p>

    <div class="toolbar">
      <MiuixInput v-model="search" placeholder="搜索邮箱或域名" />
      <span class="count">{{ filteredUsers.length }} / {{ users.length }} 个用户</span>
    </div>

    <div v-if="loading" class="loading">加载中...</div>

    <div v-else-if="!domains.length" class="empty">
      <div class="empty-icon">🌐</div>
      <p>创建用户前需要先添加域名。</p>
    </div>

    <div v-else-if="users.length === 0" class="empty">
      <div class="empty-icon">👥</div>
      <p>暂无用户</p>
      <MiuixButton type="primary" @click="showAddDialog = true">创建第一个用户</MiuixButton>
    </div>

    <div v-else class="user-list">
      <MiuixCard v-for="user in filteredUsers" :key="user.id">
        <div class="card-inner user-card">
          <div class="user-info">
            <div class="user-avatar">
              {{ user.is_admin ? '👑' : '👤' }}
            </div>
            <div class="user-copy">
              <div class="user-email">{{ user.email }}</div>
              <div class="user-meta">
                <span class="domain-tag">{{ getDomainName(user.domain_id) }}</span>
                <span v-if="user.is_admin" class="admin-tag">管理员</span>
                <span v-if="user.id === currentUser.id" class="self-tag">当前账号</span>
                <span class="date-tag">{{ formatDate(user.created_at) }}</span>
              </div>
            </div>
          </div>
          <div class="user-actions">
            <MiuixButton :disabled="user.id === currentUser.id" @click="deleteUser(user.id, user.email)">删除</MiuixButton>
          </div>
        </div>
      </MiuixCard>

      <div v-if="filteredUsers.length === 0" class="empty compact">
        <p>没有匹配的用户</p>
      </div>
    </div>

    <MiuixDialog v-model="showAddDialog" title="添加用户">
      <div class="add-form">
        <div class="form-group">
          <label>邮箱</label>
          <MiuixInput v-model="newUser.email" placeholder="user@example.com" />
        </div>
        <div class="form-group">
          <label>密码</label>
          <div class="password-row">
            <PasswordInput
              v-model="newUser.password"
              placeholder="至少 6 个字符"
              autocomplete="new-password"
            />
            <MiuixButton @click="generatePassword">生成</MiuixButton>
          </div>
        </div>
        <div class="form-group">
          <label>域名</label>
          <select v-model="newUser.domain_id" class="domain-select">
            <option v-for="d in domains" :key="d.id" :value="d.id">
              {{ d.domain_name }}
            </option>
          </select>
        </div>
        <label class="admin-switch">
          <span>管理员</span>
          <MiuixSwitch v-model="newUser.is_admin" />
        </label>
      </div>
      <template #footer="{ close }">
        <MiuixButton @click="close">取消</MiuixButton>
        <MiuixButton type="primary" :disabled="saving" @click="addUser">
          {{ saving ? '创建中...' : '创建' }}
        </MiuixButton>
      </template>
    </MiuixDialog>
  </div>
</template>

<style scoped>
.users {
  max-width: 980px;
}

.page-header {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 16px;
  margin-bottom: 22px;
}

.users h1 {
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
.user-actions {
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

.toolbar {
  display: grid;
  grid-template-columns: minmax(0, 1fr) auto;
  gap: 12px;
  align-items: center;
  margin-bottom: 16px;
}

.count {
  color: var(--m-color-text-secondary);
  font-size: 13px;
  white-space: nowrap;
}

.loading,
.empty {
  text-align: center;
  padding: 80px 20px;
  color: var(--m-color-text-secondary);
}

.empty.compact {
  padding: 24px;
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

.user-list {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.user-card {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
}

.user-info {
  display: flex;
  align-items: center;
  gap: 16px;
  min-width: 0;
}

.user-avatar {
  width: 42px;
  height: 42px;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: var(--app-radius);
  background: var(--m-color-bg);
  font-size: 22px;
  flex-shrink: 0;
}

.user-copy {
  min-width: 0;
}

.user-email {
  font-size: 16px;
  font-weight: 700;
  color: var(--m-color-text);
  overflow-wrap: anywhere;
}

.user-meta {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  margin-top: 8px;
}

.domain-tag,
.admin-tag,
.self-tag,
.date-tag {
  font-size: 12px;
  padding: 3px 8px;
  border-radius: 999px;
}

.domain-tag,
.date-tag {
  background: var(--m-color-bg);
  color: var(--m-color-text-secondary);
}

.admin-tag {
  background: var(--m-color-primary);
  color: white;
}

.self-tag {
  color: var(--app-info);
  background: color-mix(in srgb, var(--app-info) 12%, transparent);
}

.add-form {
  padding: 8px 0;
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.form-group {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.form-group label,
.admin-switch {
  font-size: 14px;
  font-weight: 650;
  color: var(--m-color-text);
}

.password-row {
  display: grid;
  grid-template-columns: minmax(0, 1fr) auto;
  gap: 8px;
}

.domain-select {
  min-height: 38px;
  padding: 0 12px;
  background: var(--m-color-card);
  color: var(--m-color-text);
}

.admin-switch {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}

@media (max-width: 680px) {
  .page-header,
  .user-card {
    align-items: stretch;
    flex-direction: column;
  }

  .header-actions,
  .user-actions {
    width: 100%;
  }

  .header-actions > *,
  .user-actions > * {
    flex: 1;
  }

  .toolbar,
  .password-row {
    grid-template-columns: 1fr;
  }
}
</style>
