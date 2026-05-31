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
      <MiuixCard>
        <div class="card-inner stat-card">
          <div class="stat-icon">📬</div>
          <div class="stat-value">{{ stats.totalEmails }}</div>
          <div class="stat-label">总邮件数</div>
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
          <div class="stat-value">{{ stats.domains }}</div>
          <div class="stat-label">域名数量</div>
        </div>
      </MiuixCard>

      <MiuixCard>
        <div class="card-inner stat-card">
          <div class="stat-icon">👥</div>
          <div class="stat-value">{{ stats.users }}</div>
          <div class="stat-label">用户数量</div>
        </div>
      </MiuixCard>
    </div>

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

    <div class="services-status">
      <MiuixCard>
        <div class="card-inner service-card">
          <div class="service-icon" style="color: #27ae60">●</div>
          <div class="service-info">
            <div class="service-name">SMTP 服务</div>
            <div class="service-detail">端口 25 - 运行中</div>
          </div>
        </div>
      </MiuixCard>

      <MiuixCard>
        <div class="card-inner service-card">
          <div class="service-icon" style="color: #27ae60">●</div>
          <div class="service-info">
            <div class="service-name">IMAP 服务</div>
            <div class="service-detail">端口 143 - 运行中</div>
          </div>
        </div>
      </MiuixCard>

      <MiuixCard>
        <div class="card-inner service-card">
          <div class="service-icon" style="color: #27ae60">●</div>
          <div class="service-info">
            <div class="service-name">Web 服务</div>
            <div class="service-detail">端口 8080 - 运行中</div>
          </div>
        </div>
      </MiuixCard>
    </div>
  </div>
</template>

<style scoped>
.dashboard h1 {
  margin-bottom: 24px;
  font-size: 24px;
  font-weight: 600;
  color: var(--m-color-text);
}

.card-inner {
  padding: 24px;
}

.stats-grid {
  display: grid;
  grid-template-columns: repeat(4, 1fr);
  gap: 16px;
  margin-bottom: 24px;
}

.stat-card {
  text-align: center;
}

.stat-icon {
  font-size: 32px;
  margin-bottom: 12px;
}

.stat-value {
  font-size: 36px;
  font-weight: 700;
  color: var(--m-color-primary);
  margin-bottom: 6px;
}

.stat-label {
  font-size: 14px;
  color: var(--m-color-text-secondary);
}

.settings-card h2 {
  font-size: 18px;
  font-weight: 600;
  margin-bottom: 20px;
  color: var(--m-color-text);
}

.settings-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
  gap: 20px;
}

.setting-item {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.setting-label {
  font-size: 12px;
  color: var(--m-color-text-secondary);
}

.setting-value {
  font-size: 15px;
  font-weight: 500;
  color: var(--m-color-text);
}

.services-status {
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  gap: 16px;
  margin-top: 24px;
}

.service-card {
  display: flex;
  align-items: center;
  gap: 16px;
}

.service-icon {
  font-size: 12px;
}

.service-name {
  font-weight: 600;
  font-size: 15px;
  color: var(--m-color-text);
}

.service-detail {
  font-size: 13px;
  color: var(--m-color-text-secondary);
  margin-top: 3px;
}
</style>
