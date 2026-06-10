<script setup>
import { ref, computed, onMounted } from 'vue'
import { useRouter } from 'vue-router'
import { MiuixButton, MiuixCard } from 'miuix-vue'
import { api } from '../api'

const router = useRouter()

const stats = ref({
  totalEmails: 0,
  unreadEmails: 0,
  domains: 0,
  users: 0,
})

const settings = ref(null)
const mailboxCounts = ref({})
const loading = ref(true)
const error = ref('')

const user = computed(() => {
  try {
    return JSON.parse(localStorage.getItem('user') || '{}')
  } catch {
    return {}
  }
})

const mailboxSummary = computed(() =>
  Object.entries(mailboxCounts.value).map(([name, value]) => ({
    name,
    total: value?.total || 0,
    unread: value?.unread || 0,
  })),
)

const serviceCards = computed(() => [
  { name: 'SMTP 服务', detail: `端口 ${settings.value?.smtp_port || 25}`, status: '运行中' },
  { name: 'IMAP 服务', detail: `端口 ${settings.value?.imap_port || 143}`, status: '运行中' },
  { name: 'POP3 服务', detail: `端口 ${settings.value?.pop3_port || 110}`, status: '运行中' },
  { name: 'Web 服务', detail: `端口 ${settings.value?.web_port || 8080}`, status: '运行中' },
])

async function loadDashboard() {
  loading.value = true
  error.value = ''
  try {
    const counts = await api.getMailboxCounts()
    mailboxCounts.value = counts.mailboxes || {}

    const totalEmails = Object.values(mailboxCounts.value)
      .reduce((sum, mb) => sum + (mb?.total || 0), 0)
    const unreadEmails = Object.values(mailboxCounts.value)
      .reduce((sum, mb) => sum + (mb?.unread || 0), 0)

    if (user.value.is_admin) {
      const [domains, users, s] = await Promise.all([
        api.getDomains(),
        api.getUsers(),
        api.getSettings(),
      ])
      stats.value = {
        totalEmails,
        unreadEmails,
        domains: domains.domains?.length || 0,
        users: users.users?.length || 0,
      }
      settings.value = s
    } else {
      stats.value = {
        totalEmails,
        unreadEmails,
        domains: 0,
        users: 0,
      }
    }
  } catch (e) {
    error.value = e.message || '加载仪表盘失败'
  } finally {
    loading.value = false
  }
}

onMounted(loadDashboard)
</script>

<template>
  <div class="dashboard">
    <div class="page-header">
      <div>
        <h1>仪表盘</h1>
        <p class="subtitle">查看邮箱状态、服务信息和常用操作。</p>
      </div>
      <div class="header-actions">
        <MiuixButton @click="loadDashboard">刷新</MiuixButton>
        <MiuixButton type="primary" @click="router.push('/compose')">写邮件</MiuixButton>
      </div>
    </div>

    <div v-if="error" class="notice error">{{ error }}</div>
    <div v-if="loading" class="loading">正在加载仪表盘...</div>

    <template v-else>
      <div class="stats-grid">
        <MiuixCard>
          <div class="card-inner stat-card">
            <div class="stat-icon">📬</div>
            <div class="stat-value">{{ stats.totalEmails }}</div>
            <div class="stat-label">全部邮件</div>
          </div>
        </MiuixCard>

        <MiuixCard>
          <div class="card-inner stat-card">
            <div class="stat-icon">📨</div>
            <div class="stat-value">{{ stats.unreadEmails }}</div>
            <div class="stat-label">未读邮件</div>
          </div>
        </MiuixCard>

        <MiuixCard>
          <div class="card-inner stat-card">
            <div class="stat-icon">🌐</div>
            <div class="stat-value">{{ user.is_admin ? stats.domains : '-' }}</div>
            <div class="stat-label">域名数量</div>
          </div>
        </MiuixCard>

        <MiuixCard>
          <div class="card-inner stat-card">
            <div class="stat-icon">👥</div>
            <div class="stat-value">{{ user.is_admin ? stats.users : '-' }}</div>
            <div class="stat-label">用户数量</div>
          </div>
        </MiuixCard>
      </div>

      <div class="content-grid">
        <MiuixCard>
          <div class="card-inner">
            <div class="section-head">
              <h2>邮箱概览</h2>
              <MiuixButton @click="router.push('/inbox')">打开收件箱</MiuixButton>
            </div>
            <div v-if="mailboxSummary.length" class="mailbox-summary">
              <div v-for="mb in mailboxSummary" :key="mb.name" class="mailbox-row">
                <span class="mailbox-name">{{ mb.name }}</span>
                <span class="mailbox-meta">{{ mb.total }} 封</span>
                <span v-if="mb.unread" class="unread-badge">{{ mb.unread }} 未读</span>
              </div>
            </div>
            <div v-else class="empty-compact">暂无邮箱数据</div>
          </div>
        </MiuixCard>

        <MiuixCard v-if="settings">
          <div class="card-inner settings-card">
            <h2>服务器信息</h2>
            <div class="settings-grid">
              <div class="setting-item">
                <span class="setting-label">主机名</span>
                <span class="setting-value">{{ settings.hostname }}</span>
              </div>
              <div class="setting-item">
                <span class="setting-label">SMTP 端口</span>
                <span class="setting-value">{{ settings.smtp_port }}</span>
              </div>
              <div class="setting-item">
                <span class="setting-label">IMAP 端口</span>
                <span class="setting-value">{{ settings.imap_port }}</span>
              </div>
              <div class="setting-item">
                <span class="setting-label">Web 端口</span>
                <span class="setting-value">{{ settings.web_port }}</span>
              </div>
              <div class="setting-item">
                <span class="setting-label">DKIM 选择器</span>
                <span class="setting-value">{{ settings.dkim_selector }}</span>
              </div>
            </div>
          </div>
        </MiuixCard>
      </div>

      <div v-if="settings" class="services-status">
        <MiuixCard v-for="service in serviceCards" :key="service.name">
          <div class="card-inner service-card">
            <div class="service-dot"></div>
            <div class="service-info">
              <div class="service-name">{{ service.name }}</div>
              <div class="service-detail">{{ service.detail }} - {{ service.status }}</div>
            </div>
          </div>
        </MiuixCard>
      </div>
    </template>
  </div>
</template>

<style scoped>
.dashboard {
  max-width: 1180px;
}

.page-header {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 16px;
  margin-bottom: 24px;
}

.dashboard h1 {
  font-size: 26px;
  font-weight: 700;
  color: var(--m-color-text);
}

.subtitle {
  margin-top: 4px;
  color: var(--m-color-text-secondary);
  font-size: 14px;
}

.header-actions {
  display: flex;
  gap: 8px;
  flex-shrink: 0;
}

.card-inner {
  padding: 22px;
}

.loading,
.notice {
  padding: 16px;
  border-radius: var(--app-radius);
  margin-bottom: 16px;
  background: var(--m-color-card);
  color: var(--m-color-text-secondary);
}

.notice.error {
  color: var(--app-danger);
  border: 1px solid color-mix(in srgb, var(--app-danger) 32%, transparent);
}

.stats-grid {
  display: grid;
  grid-template-columns: repeat(4, minmax(0, 1fr));
  gap: 14px;
  margin-bottom: 20px;
}

.stat-card {
  text-align: left;
}

.stat-icon {
  font-size: 24px;
  margin-bottom: 12px;
}

.stat-value {
  font-size: 34px;
  font-weight: 750;
  color: var(--m-color-primary);
  line-height: 1;
  margin-bottom: 8px;
}

.stat-label {
  font-size: 13px;
  color: var(--m-color-text-secondary);
}

.content-grid {
  display: grid;
  grid-template-columns: minmax(0, 0.9fr) minmax(0, 1.1fr);
  gap: 16px;
}

.section-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  margin-bottom: 16px;
}

.section-head h2,
.settings-card h2 {
  font-size: 17px;
  font-weight: 650;
  color: var(--m-color-text);
}

.mailbox-summary {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.mailbox-row {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 10px 0;
  border-bottom: 1px solid var(--m-color-border);
}

.mailbox-row:last-child {
  border-bottom: 0;
}

.mailbox-name {
  flex: 1;
  font-weight: 600;
  color: var(--m-color-text);
}

.mailbox-meta {
  color: var(--m-color-text-secondary);
  font-size: 13px;
}

.unread-badge {
  font-size: 12px;
  font-weight: 700;
  color: white;
  background: var(--app-danger);
  border-radius: 999px;
  padding: 3px 8px;
}

.empty-compact {
  color: var(--m-color-text-secondary);
  font-size: 14px;
}

.settings-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(150px, 1fr));
  gap: 16px;
  margin-top: 18px;
}

.setting-item {
  display: flex;
  flex-direction: column;
  gap: 5px;
}

.setting-label {
  font-size: 12px;
  color: var(--m-color-text-secondary);
}

.setting-value {
  font-size: 14px;
  font-weight: 600;
  color: var(--m-color-text);
  word-break: break-word;
}

.services-status {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 14px;
  margin-top: 20px;
}

.service-card {
  display: flex;
  align-items: center;
  gap: 14px;
}

.service-dot {
  width: 10px;
  height: 10px;
  border-radius: 50%;
  background: var(--app-success);
  box-shadow: 0 0 0 5px color-mix(in srgb, var(--app-success) 18%, transparent);
  flex-shrink: 0;
}

.service-name {
  font-weight: 650;
  font-size: 15px;
  color: var(--m-color-text);
}

.service-detail {
  font-size: 13px;
  color: var(--m-color-text-secondary);
  margin-top: 3px;
}

@media (max-width: 980px) {
  .stats-grid,
  .content-grid,
  .services-status {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }
}

@media (max-width: 620px) {
  .page-header {
    flex-direction: column;
  }

  .header-actions {
    width: 100%;
  }

  .header-actions > * {
    flex: 1;
  }

  .stats-grid,
  .content-grid,
  .services-status {
    grid-template-columns: 1fr;
  }
}
</style>
