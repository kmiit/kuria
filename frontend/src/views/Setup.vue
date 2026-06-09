<script setup>
import { ref, computed } from 'vue'
import { useRouter } from 'vue-router'
import { MiuixButton, MiuixInput, MiuixCard } from 'miuix-vue'

const router = useRouter()

const step = ref(1)
const totalSteps = 4
const loading = ref(false)
const error = ref('')
const copyMessage = ref('')

// Form data
const hostname = ref('')
const domain = ref('')
const adminEmail = ref('')
const adminPassword = ref('')
const confirmPassword = ref('')

// Setup result
const setupResult = ref(null)

const progress = computed(() => (step.value / totalSteps) * 100)

function nextStep() {
  error.value = ''
  copyMessage.value = ''

  if (step.value === 1) {
    // Welcome - just proceed
    step.value = 2
  } else if (step.value === 2) {
    // Validate domain settings
    hostname.value = hostname.value.trim().toLowerCase().replace(/^https?:\/\//, '').replace(/\/.*$/, '')
    domain.value = domain.value.trim().toLowerCase().replace(/^https?:\/\//, '').replace(/\/.*$/, '')
    if (!hostname.value) {
      error.value = '请输入服务器主机名'
      return
    }
    if (!domain.value) {
      error.value = '请输入域名'
      return
    }
    if (!/^[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?(?:\.[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?)+$/.test(hostname.value)) {
      error.value = '请输入有效的服务器主机名'
      return
    }
    if (!/^[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?(?:\.[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?)+$/.test(domain.value)) {
      error.value = '请输入有效的邮件域名'
      return
    }
    // Auto-fill email
    if (!adminEmail.value) {
      adminEmail.value = `admin@${domain.value}`
    }
    step.value = 3
  } else if (step.value === 3) {
    // Validate admin account
    adminEmail.value = adminEmail.value.trim().toLowerCase()
    if (!/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(adminEmail.value)) {
      error.value = '请输入有效的邮箱地址'
      return
    }
    if (adminPassword.value.length < 6) {
      error.value = '密码至少需要 6 个字符'
      return
    }
    if (adminPassword.value !== confirmPassword.value) {
      error.value = '两次输入的密码不一致'
      return
    }
    runSetup()
  } else if (step.value === 4) {
    // Complete - go to dashboard
    router.push('/')
  }
}

function prevStep() {
  if (step.value > 1) {
    error.value = ''
    step.value--
  }
}

async function runSetup() {
  loading.value = true
  error.value = ''

  try {
    const res = await fetch('/api/setup', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        hostname: hostname.value,
        domain: domain.value,
        admin_email: adminEmail.value,
        admin_password: adminPassword.value,
      }),
    })

    if (!res.ok) {
      const text = await res.text()
      throw new Error(text || 'Setup failed')
    }

    const data = await res.json()
    setupResult.value = data

    // Save token
    if (data.token) {
      localStorage.setItem('token', data.token)
      localStorage.setItem('user', JSON.stringify(data.user))
    }

    step.value = 4
  } catch (e) {
    error.value = '设置失败：' + (e.message || '请重试')
  } finally {
    loading.value = false
  }
}

function copyToClipboard(text) {
  navigator.clipboard.writeText(text).then(() => {
    copyMessage.value = '已复制到剪贴板'
  })
}
</script>

<template>
  <div class="setup-page">
    <div class="setup-container">
      <!-- Progress bar -->
      <div class="progress-bar">
        <div class="progress-fill" :style="{ width: progress + '%' }"></div>
      </div>

      <!-- Step indicators -->
      <div class="steps">
        <div
          v-for="i in totalSteps"
          :key="i"
          class="step-dot"
          :class="{ active: i === step, completed: i < step }"
        >
          <span v-if="i < step">✓</span>
          <span v-else>{{ i }}</span>
        </div>
      </div>

      <MiuixCard class="setup-card">
        <!-- Step 1: Welcome -->
        <div v-if="step === 1" class="step-content">
          <div class="welcome-icon">📧</div>
          <h1>欢迎使用 Kuria Mail</h1>
          <p class="subtitle">让我们一起设置您的邮件服务器</p>
          <div class="features">
            <div class="feature">
              <span class="feature-icon">📬</span>
              <div>
                <div class="feature-title">SMTP 邮件收发</div>
                <div class="feature-desc">支持标准 SMTP 协议，兼容所有邮件客户端</div>
              </div>
            </div>
            <div class="feature">
              <span class="feature-icon">📨</span>
              <div>
                <div class="feature-title">IMAP 邮箱访问</div>
                <div class="feature-desc">支持 IMAP 协议，随时随地访问邮件</div>
              </div>
            </div>
            <div class="feature">
              <span class="feature-icon">🌐</span>
              <div>
                <div class="feature-title">Web 管理界面</div>
                <div class="feature-desc">直观的管理界面，轻松管理域名和用户</div>
              </div>
            </div>
            <div class="feature">
              <span class="feature-icon">🔒</span>
              <div>
                <div class="feature-title">安全认证</div>
                <div class="feature-desc">支持 DKIM、SPF、DMARC 邮件认证</div>
              </div>
            </div>
          </div>
        </div>

        <!-- Step 2: Domain Settings -->
        <div v-if="step === 2" class="step-content">
          <h1>🌐 域名设置</h1>
          <p class="subtitle">配置您的邮件服务器域名</p>

          <div class="form-group">
            <label>服务器主机名</label>
            <MiuixInput v-model="hostname" placeholder="例如：mail.example.com" />
            <p class="hint">这是您邮件服务器的完整域名，需要有 A 记录指向服务器 IP</p>
          </div>

          <div class="form-group">
            <label>邮件域名</label>
            <MiuixInput v-model="domain" placeholder="例如：example.com" />
            <p class="hint">用户邮箱将使用此域名，如 user@example.com</p>
          </div>
        </div>

        <!-- Step 3: Admin Account -->
        <div v-if="step === 3" class="step-content">
          <h1>👤 管理员账号</h1>
          <p class="subtitle">创建您的管理员邮箱账号</p>

          <div class="form-group">
            <label>管理员邮箱</label>
            <MiuixInput v-model="adminEmail" placeholder="admin@example.com" />
            <p class="hint">这将是您的登录账号和第一个邮箱</p>
          </div>

          <div class="form-group">
            <label>密码</label>
            <MiuixInput v-model="adminPassword" type="password" placeholder="至少 6 个字符" />
          </div>

          <div class="form-group">
            <label>确认密码</label>
            <MiuixInput v-model="confirmPassword" type="password" placeholder="再次输入密码" />
          </div>
        </div>

        <!-- Step 4: Complete -->
        <div v-if="step === 4" class="step-content">
          <div class="success-icon">🎉</div>
          <h1>设置完成！</h1>
          <p class="subtitle">您的邮件服务器已准备就绪</p>

          <div v-if="setupResult" class="dns-info">
            <h3>📋 DNS 记录配置</h3>
            <p class="dns-hint">请在您的域名 DNS 管理中添加以下记录：</p>
            <p v-if="copyMessage" class="copy-message">{{ copyMessage }}</p>

            <div class="dns-records">
              <div class="dns-record">
                <div class="record-header">
                  <span class="record-type">MX</span>
                  <MiuixButton @click="copyToClipboard(setupResult.dns_records.mx)">复制</MiuixButton>
                </div>
                <code>{{ setupResult.dns_records.mx }}</code>
              </div>

              <div class="dns-record">
                <div class="record-header">
                  <span class="record-type">TXT (SPF)</span>
                  <MiuixButton @click="copyToClipboard(setupResult.dns_records.spf)">复制</MiuixButton>
                </div>
                <code>{{ setupResult.dns_records.spf }}</code>
              </div>

              <div class="dns-record">
                <div class="record-header">
                  <span class="record-type">TXT (DKIM)</span>
                  <MiuixButton @click="copyToClipboard(setupResult.dns_records.dkim)">复制</MiuixButton>
                </div>
                <code>{{ setupResult.dns_records.dkim }}</code>
              </div>

              <div class="dns-record">
                <div class="record-header">
                  <span class="record-type">TXT (DMARC)</span>
                  <MiuixButton @click="copyToClipboard(setupResult.dns_records.dmarc)">复制</MiuixButton>
                </div>
                <code>{{ setupResult.dns_records.dmarc }}</code>
              </div>
            </div>
          </div>

          <div class="account-info">
            <h3>🔐 您的账号信息</h3>
            <p><strong>邮箱：</strong>{{ adminEmail }}</p>
            <p><strong>Web 界面：</strong>http://{{ hostname }}:8080</p>
          </div>
        </div>

        <!-- Error message -->
        <p v-if="error" class="error">{{ error }}</p>

        <!-- Navigation buttons -->
        <div class="actions">
          <MiuixButton v-if="step > 1 && step < 4" @click="prevStep">
            上一步
          </MiuixButton>
          <div v-else></div>

          <MiuixButton
            type="primary"
            :disabled="loading"
            @click="nextStep"
          >
            <template v-if="loading">设置中...</template>
            <template v-else-if="step === 1">开始设置</template>
            <template v-else-if="step === 3">完成设置</template>
            <template v-else-if="step === 4">进入管理界面</template>
            <template v-else>下一步</template>
          </MiuixButton>
        </div>
      </MiuixCard>
    </div>
  </div>
</template>

<style scoped>
.setup-page {
  min-height: 100vh;
  display: flex;
  align-items: center;
  justify-content: center;
  background:
    linear-gradient(135deg, rgba(15, 118, 110, 0.92), rgba(64, 81, 59, 0.9) 52%, rgba(183, 121, 31, 0.88)),
    var(--m-color-bg);
  padding: 20px;
}

.setup-container {
  width: 100%;
  max-width: 600px;
}

.progress-bar {
  height: 4px;
  background: rgba(255, 255, 255, 0.3);
  border-radius: 2px;
  margin-bottom: 24px;
  overflow: hidden;
}

.progress-fill {
  height: 100%;
  background: white;
  border-radius: 2px;
  transition: width 0.3s ease;
}

.steps {
  display: flex;
  justify-content: center;
  gap: 24px;
  margin-bottom: 24px;
}

.step-dot {
  width: 36px;
  height: 36px;
  border-radius: 50%;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 14px;
  font-weight: 600;
  background: rgba(255, 255, 255, 0.2);
  color: rgba(255, 255, 255, 0.6);
  transition: all 0.3s ease;
}

.step-dot.active {
  background: white;
  color: #667eea;
  transform: scale(1.1);
}

.step-dot.completed {
  background: rgba(255, 255, 255, 0.8);
  color: #27ae60;
}

.setup-card {
  padding: 40px;
  border-radius: var(--app-radius);
  background: var(--m-color-card);
  box-shadow: var(--app-shadow);
}

.step-content {
  text-align: center;
}

.welcome-icon,
.success-icon {
  font-size: 64px;
  margin-bottom: 16px;
}

h1 {
  font-size: 24px;
  color: var(--m-color-text);
  margin-bottom: 8px;
}

.subtitle {
  color: var(--m-color-text-secondary);
  margin-bottom: 32px;
}

.features {
  text-align: left;
  display: flex;
  flex-direction: column;
  gap: 16px;
  margin-bottom: 32px;
}

.feature {
  display: flex;
  align-items: flex-start;
  gap: 12px;
  padding: 12px;
  background: var(--m-color-bg);
  border-radius: var(--app-radius);
}

.feature-icon {
  font-size: 24px;
  flex-shrink: 0;
}

.feature-title {
  font-weight: 600;
  color: var(--m-color-text);
  margin-bottom: 2px;
}

.feature-desc {
  font-size: 13px;
  color: var(--m-color-text-secondary);
}

.form-group {
  text-align: left;
  margin-bottom: 20px;
}

.form-group label {
  display: block;
  font-size: 14px;
  font-weight: 500;
  color: var(--m-color-text);
  margin-bottom: 8px;
}

.hint {
  font-size: 12px;
  color: var(--m-color-text-secondary);
  margin-top: 6px;
}

.error {
  color: var(--app-danger);
  font-size: 14px;
  margin-top: 16px;
  text-align: center;
}

.actions {
  display: flex;
  justify-content: space-between;
  margin-top: 32px;
}

.dns-info {
  text-align: left;
  background: var(--m-color-bg);
  border-radius: var(--app-radius);
  padding: 20px;
  margin-top: 24px;
}

.dns-info h3 {
  margin-bottom: 8px;
  color: var(--m-color-text);
}

.dns-hint {
  font-size: 13px;
  color: var(--m-color-text-secondary);
  margin-bottom: 16px;
}

.copy-message {
  color: var(--app-success);
  font-size: 13px;
  margin-bottom: 12px;
}

.dns-records {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.dns-record {
  background: var(--m-color-card);
  border-radius: var(--app-radius);
  padding: 12px;
  border: 1px solid var(--m-color-border);
}

.record-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 8px;
}

.record-type {
  font-size: 12px;
  font-weight: 600;
  color: var(--app-info);
  background: color-mix(in srgb, var(--app-info) 12%, transparent);
  padding: 2px 8px;
  border-radius: 999px;
}

.dns-record code {
  display: block;
  font-size: 12px;
  color: var(--m-color-text);
  background: var(--m-color-bg);
  padding: 8px;
  border-radius: var(--app-radius);
  word-break: break-all;
  font-family: 'Monaco', 'Consolas', monospace;
}

.account-info {
  text-align: left;
  background: var(--m-color-bg);
  border-radius: var(--app-radius);
  padding: 20px;
  margin-top: 16px;
}

.account-info h3 {
  margin-bottom: 12px;
  color: var(--m-color-text);
}

.account-info p {
  margin-bottom: 8px;
  color: var(--m-color-text);
  font-size: 14px;
}

@media (max-width: 620px) {
  .setup-page {
    align-items: flex-start;
  }

  .setup-card {
    padding: 24px;
  }

  .steps {
    gap: 12px;
  }

  .record-header,
  .actions {
    align-items: stretch;
    flex-direction: column;
    gap: 10px;
  }
}
</style>
