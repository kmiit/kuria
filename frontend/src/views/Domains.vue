<script setup>
import { ref, onMounted } from 'vue'
import { MiuixButton, MiuixInput, MiuixCard, MiuixDialog } from 'miuix-vue'
import { api } from '../api'

const domains = ref([])
const loading = ref(true)
const newDomain = ref('')
const showAddDialog = ref(false)

async function loadDomains() {
  loading.value = true
  try {
    const data = await api.getDomains()
    domains.value = data.domains || []
  } catch (e) {
    console.error(e)
  } finally {
    loading.value = false
  }
}

async function addDomain() {
  if (!newDomain.value) return
  try {
    await api.createDomain(newDomain.value)
    newDomain.value = ''
    showAddDialog.value = false
    await loadDomains()
  } catch (e) {
    alert('添加失败：' + (e.message || '未知错误'))
  }
}

async function deleteDomain(id, name) {
  if (confirm(`确定删除域名 ${name}？`)) {
    try {
      await api.deleteDomain(id)
      domains.value = domains.value.filter((d) => d.id !== id)
    } catch (e) {
      alert('删除失败')
    }
  }
}

onMounted(loadDomains)
</script>

<template>
  <div class="domains">
    <div class="header">
      <h1>域名管理</h1>
      <MiuixButton type="primary" @click="showAddDialog = true">+ 添加域名</MiuixButton>
    </div>

    <div v-if="loading" class="loading">加载中...</div>

    <div v-else-if="domains.length === 0" class="empty">
      <div class="empty-icon">🌐</div>
      <p>暂无域名</p>
      <MiuixButton type="primary" @click="showAddDialog = true">添加第一个域名</MiuixButton>
    </div>

    <div v-else class="domain-list">
      <MiuixCard v-for="domain in domains" :key="domain.id">
        <div class="card-inner domain-card">
          <div class="domain-info">
            <div class="domain-icon">🌐</div>
            <div>
              <div class="domain-name">{{ domain.domain_name }}</div>
              <div class="domain-detail">
                DKIM 选择器：{{ domain.dkim_selector || '未配置' }}
              </div>
            </div>
          </div>
          <div class="domain-actions">
            <MiuixButton @click="deleteDomain(domain.id, domain.domain_name)">删除</MiuixButton>
          </div>
        </div>
      </MiuixCard>
    </div>

    <!-- Add Domain Dialog -->
    <MiuixDialog v-model="showAddDialog" title="添加域名">
      <div class="add-form">
        <MiuixInput v-model="newDomain" placeholder="例如：example.com" />
      </div>
      <template #footer="{ close }">
        <MiuixButton @click="close">取消</MiuixButton>
        <MiuixButton type="primary" @click="addDomain">添加</MiuixButton>
      </template>
    </MiuixDialog>
  </div>
</template>

<style scoped>
.domains h1 {
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

.domain-list {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.domain-card {
  display: flex;
  align-items: center;
  justify-content: space-between;
}

.domain-info {
  display: flex;
  align-items: center;
  gap: 16px;
}

.domain-icon {
  font-size: 32px;
}

.domain-name {
  font-size: 16px;
  font-weight: 600;
  color: var(--m-color-text);
}

.domain-detail {
  font-size: 13px;
  color: var(--m-color-text-secondary);
  margin-top: 4px;
}

.domain-actions {
  display: flex;
  gap: 8px;
}

.add-form {
  padding: 8px 0;
}
</style>
