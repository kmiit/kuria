<script setup>
import { ref, onMounted } from 'vue'
import { MiuixCard } from 'miuix-vue'
import { api } from '../api'

const stats = ref({
  totalEmails: 0,
  unreadEmails: 0,
  domains: 0,
  users: 0,
})

const settings = ref(null)

onMounted(async () => {
  try {
    const [emails, domains, users, s] = await Promise.all([
      api.getEmails('INBOX'),
      api.getDomains(),
      api.getUsers(),
      api.getSettings(),
    ])
    stats.value = {
      totalEmails: emails.total || 0,
      unreadEmails: emails.emails?.filter((e) => !e.is_read).length || 0,
      domains: domains.domains?.length || 0,
      users: users.users?.length || 0,
    }
    settings.value = s
  } catch (e) {
    console.error(e)
  }
})
</script>

<template>
  <div class="dashboard">
    <h1>仪表盘</h1>

    <div class="stats-grid">
      <MiuixCard class="stat-card">
        <div class="stat-icon">📬</div>
        <div class="stat-value">{{ stats.totalEmails }}</div>
        <div class="stat-label">总邮件数</div>
      </MiuixCard>

      <MiuixCard class="stat-card">
        <div class="stat-icon">📨</div>
        <div class="stat-value">{{ stats.unreadEmails }}</div>
        <div class="stat-label">未读邮件</div>
      </MiuixCard>

      <MiuixCard class="stat-card">
        <div class="stat-icon">🌐</div>
        <div class="stat-value">{{ stats.domains }}</div>
        <div class="stat-label">域名数量</div>
      </MiuixCard>

      <MiuixCard class="stat-card">
        <div class="stat-icon">👥</div>
        <div class="stat-value">{{ stats.users }}</div>
        <div class="stat-label">用户数量</div>
      </MiuixCard>
    </div>

    <MiuixCard v-if="settings" class="settings-card">
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
    </MiuixCard>

    <div class="services-status">
      <MiuixCard class="service-card">
        <div class="service-icon" style="color: #27ae60">●</div>
        <div class="service-info">
          <div class="service-name">SMTP 服务</div>
          <div class="service-detail">端口 25 - 运行中</div>
        </div>
      </MiuixCard>

      <MiuixCard class="service-card">
        <div class="service-icon" style="color: #27ae60">●</div>
        <div class="service-info">
          <div class="service-name">IMAP 服务</div>
          <div class="service-detail">端口 143 - 运行中</div>
        </div>
      </MiuixCard>

      <MiuixCard class="service-card">
        <div class="service-icon" style="color: #27ae60">●</div>
        <div class="service-info">
          <div class="service-name">Web 服务</div>
          <div class="service-detail">端口 8080 - 运行中</div>
        </div>
      </MiuixCard>
    </div>
  </div>
</template>

<style scoped>
.dashboard h1 {
  margin-bottom: 24px;
  font-size: 24px;
  color: var(--m-color-text, #1a1a1a);
}

.stats-grid {
  display: grid;
  grid-template-columns: repeat(4, 1fr);
  gap: 16px;
  margin-bottom: 24px;
}

.stat-card {
  padding: 24px;
  text-align: center;
}

.stat-icon {
  font-size: 32px;
  margin-bottom: 12px;
}

.stat-value {
  font-size: 32px;
  font-weight: 700;
  color: var(--m-color-primary, #4a90d9);
  margin-bottom: 4px;
}

.stat-label {
  font-size: 14px;
  color: var(--m-color-text-secondary, #666);
}

.settings-card {
  padding: 24px;
  margin-bottom: 24px;
}

.settings-card h2 {
  font-size: 18px;
  margin-bottom: 16px;
  color: var(--m-color-text, #1a1a1a);
}

.settings-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
  gap: 16px;
}

.setting-item {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.setting-label {
  font-size: 12px;
  color: var(--m-color-text-secondary, #666);
  text-transform: uppercase;
  letter-spacing: 0.5px;
}

.setting-value {
  font-size: 15px;
  font-weight: 500;
  color: var(--m-color-text, #1a1a1a);
}

.services-status {
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  gap: 16px;
}

.service-card {
  padding: 20px;
  display: flex;
  align-items: center;
  gap: 16px;
}

.service-icon {
  font-size: 12px;
}

.service-name {
  font-weight: 600;
  color: var(--m-color-text, #1a1a1a);
}

.service-detail {
  font-size: 13px;
  color: var(--m-color-text-secondary, #666);
}
</style>
