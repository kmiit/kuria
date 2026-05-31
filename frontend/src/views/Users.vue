<script setup>
import { ref, onMounted } from 'vue'
import { MiuixButton, MiuixInput, MiuixCard, MiuixDialog, MiuixSwitch } from 'miuix-vue'
import { api } from '../api'

const users = ref([])
const domains = ref([])
const loading = ref(true)
const showAddDialog = ref(false)

const newUser = ref({
  email: '',
  password: '',
  domain_id: 0,
  is_admin: false,
})

async function loadData() {
  loading.value = true
  try {
    const [u, d] = await Promise.all([api.getUsers(), api.getDomains()])
    users.value = u.users || []
    domains.value = d.domains || []
    if (domains.value.length > 0 && !newUser.value.domain_id) {
      newUser.value.domain_id = domains.value[0].id
    }
  } catch (e) {
    console.error(e)
  } finally {
    loading.value = false
  }
}

async function addUser() {
  if (!newUser.value.email || !newUser.value.password) {
    alert('请填写邮箱和密码')
    return
  }
  try {
    await api.createUser(newUser.value)
    showAddDialog.value = false
    newUser.value = { email: '', password: '', domain_id: domains.value[0]?.id || 0, is_admin: false }
    await loadData()
  } catch (e) {
    alert('创建失败：' + (e.message || '未知错误'))
  }
}

async function deleteUser(id, email) {
  if (confirm(`确定删除用户 ${email}？`)) {
    try {
      await api.deleteUser(id)
      users.value = users.value.filter((u) => u.id !== id)
    } catch (e) {
      alert('删除失败')
    }
  }
}

function getDomainName(domainId) {
  const d = domains.value.find((d) => d.id === domainId)
  return d ? d.domain_name : `ID: ${domainId}`
}

onMounted(loadData)
</script>

<template>
  <div class="users">
    <div class="header">
      <h1>用户管理</h1>
      <MiuixButton type="primary" @click="showAddDialog = true">+ 添加用户</MiuixButton>
    </div>

    <div v-if="loading" class="loading">加载中...</div>

    <div v-else-if="users.length === 0" class="empty">
      <div class="empty-icon">👥</div>
      <p>暂无用户</p>
      <MiuixButton type="primary" @click="showAddDialog = true">创建第一个用户</MiuixButton>
    </div>

    <div v-else class="user-list">
      <MiuixCard v-for="user in users" :key="user.id">
        <div class="card-inner user-card">
          <div class="user-info">
            <div class="user-avatar">
              {{ user.is_admin ? '👑' : '👤' }}
            </div>
            <div>
              <div class="user-email">{{ user.email }}</div>
              <div class="user-meta">
                <span class="domain-tag">{{ getDomainName(user.domain_id) }}</span>
                <span v-if="user.is_admin" class="admin-tag">管理员</span>
              </div>
            </div>
          </div>
          <div class="user-actions">
            <MiuixButton @click="deleteUser(user.id, user.email)">删除</MiuixButton>
          </div>
        </div>
      </MiuixCard>
    </div>

    <!-- Add User Dialog -->
    <MiuixDialog v-model="showAddDialog" title="添加用户">
      <div class="add-form">
        <div class="form-group">
          <label>邮箱</label>
          <MiuixInput v-model="newUser.email" placeholder="user@example.com" />
        </div>
        <div class="form-group">
          <label>密码</label>
          <MiuixInput v-model="newUser.password" type="password" placeholder="输入密码" />
        </div>
        <div class="form-group">
          <label>域名</label>
          <select v-model="newUser.domain_id" class="domain-select">
            <option v-for="d in domains" :key="d.id" :value="d.id">
              {{ d.domain_name }}
            </option>
          </select>
        </div>
        <div class="form-group">
          <label>管理员</label>
          <MiuixSwitch v-model="newUser.is_admin" />
        </div>
      </div>
      <template #footer="{ close }">
        <MiuixButton @click="close">取消</MiuixButton>
        <MiuixButton type="primary" @click="addUser">创建</MiuixButton>
      </template>
    </MiuixDialog>
  </div>
</template>

<style scoped>
.users h1 {
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
  padding: 24px;
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
}

.user-info {
  display: flex;
  align-items: center;
  gap: 16px;
}

.user-avatar {
  font-size: 32px;
}

.user-email {
  font-size: 16px;
  font-weight: 600;
  color: var(--m-color-text);
}

.user-meta {
  display: flex;
  gap: 8px;
  margin-top: 4px;
}

.domain-tag {
  font-size: 12px;
  padding: 2px 8px;
  background: var(--m-color-bg, #f0f0f0);
  border-radius: 4px;
  color: var(--m-color-text-secondary);
}

.admin-tag {
  font-size: 12px;
  padding: 2px 8px;
  background: var(--m-color-primary);
  color: white;
  border-radius: 4px;
}

.user-actions {
  display: flex;
  gap: 8px;
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

.form-group label {
  font-size: 14px;
  font-weight: 500;
  color: var(--m-color-text);
}

.domain-select {
  padding: 10px 12px;
  border: 1px solid var(--m-color-border, #ddd);
  border-radius: 8px;
  font-size: 14px;
  background: var(--m-color-card);
  color: var(--m-color-text);
}
</style>
