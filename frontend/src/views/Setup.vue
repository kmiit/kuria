<script setup>
import { computed, ref, watch } from 'vue'
import { useRouter } from 'vue-router'
import { MiuixButton, MiuixCard, MiuixInput } from 'miuix-vue'
import { api } from '../api'
import { setInitialized } from '../setupState'
import PasswordInput from '../components/PasswordInput.vue'

const router = useRouter()

const steps = [
  { title: '基础信息', caption: '确认邮件域名和主机名' },
  { title: '管理员', caption: '创建第一个邮箱账号' },
  { title: '确认', caption: '检查配置并初始化' },
  { title: '完成', caption: '保存 DNS 记录' },
]

const step = ref(1)
const loading = ref(false)
const error = ref('')
const copiedKey = ref('')
const setupResult = ref(null)
const manualServerIps = ref({
  ipv4: '',
  ipv6: '',
})
const savingPublicIps = ref(false)

const form = ref({
  hostname: '',
  domain: '',
  adminEmail: '',
  adminPassword: '',
  confirmPassword: '',
})

const progress = computed(() => ((step.value - 1) / (steps.length - 1)) * 100)
const domainPattern = /^[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?(?:\.[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?)+$/
const emailPattern = /^[^\s@]+@[^\s@]+\.[^\s@]+$/

const passwordStrength = computed(() => {
  const value = form.value.adminPassword
  let score = 0
  if (value.length >= 8) score += 1
  if (/[a-z]/.test(value) && /[A-Z]/.test(value)) score += 1
  if (/\d/.test(value)) score += 1
  if (/[^A-Za-z0-9]/.test(value)) score += 1

  if (!value) return { label: '未填写', className: '' }
  if (score <= 1) return { label: '偏弱', className: 'weak' }
  if (score <= 3) return { label: '可用', className: 'medium' }
  return { label: '较强', className: 'strong' }
})

const dnsRows = computed(() => {
  if (!setupResult.value) return []
  const domain = setupDomain()
  const mxHost = setupHostname()
  const addressRecords = addressRows(domain, mxHost)
  return [
    ...addressRecords,
    {
      key: 'mx',
      label: 'MX',
      purpose: '接收发往该域名的邮件',
      value: zoneLine(domain, domain, 'MX', `10 ${absoluteHost(mxHost)}`),
    },
    {
      key: 'spf',
      label: 'TXT / SPF',
      purpose: '声明允许这台服务器发信',
      value: zoneLine(domain, domain, 'TXT', quoteTxt(`v=spf1 mx:${domain} -all`)),
    },
    {
      key: 'dkim',
      label: 'TXT / DKIM',
      purpose: '初始化后生成 DKIM 公钥',
      value: '; TODO: enter Domains, generate DKIM, then import the DKIM TXT record.',
    },
    {
      key: 'dmarc',
      label: 'TXT / DMARC',
      purpose: '定义域名认证失败策略',
      value: zoneLine(
        domain,
        `_dmarc.${domain}`,
        'TXT',
        quoteTxt(`v=DMARC1; p=quarantine; rua=mailto:postmaster@${domain}`),
      ),
    },
  ]
})

const cloudflareZoneFile = computed(() => {
  if (!setupResult.value) return ''
  const domain = setupDomain()
  const hostname = setupHostname()
  const hostRelative = relativeHost(hostname, domain)
  const hostInThisZone = !hostRelative.endsWith('.')
  const ipv4 = detectedIp('ipv4')
  const ipv6 = detectedIp('ipv6')
  const lines = [
    `$ORIGIN ${absoluteHost(domain)}`,
    '$TTL 3600',
    '; Cloudflare zone file for Kuria Mail',
  ]

  if (!hostInThisZone) {
    lines.push(`; TODO: make sure ${absoluteHost(hostname)} has A/AAAA records in its own DNS zone.`)
  } else if (!effectiveServerIp('ipv4') && !effectiveServerIp('ipv6')) {
    lines.push('; TODO before import: no public server IP was detected automatically.')
    if (ipv4?.address) lines.push(`; Detected IPv4 ${ipv4.address} is not public, so it was not imported.`)
    if (ipv6?.address) lines.push(`; Detected IPv6 ${ipv6.address} is not public, so it was not imported.`)
    lines.push(`; ${hostRelative} 3600 IN A <server-ipv4>`)
    lines.push(`; ${hostRelative} 3600 IN AAAA <server-ipv6>`)
  }

  dnsRows.value.forEach((row) => {
    if (row.value) lines.push(row.value)
  })

  return `${lines.join('\n')}\n`
})

const normalizedPreview = computed(() => ({
  hostname: normalizeDomain(form.value.hostname) || 'mail.example.com',
  domain: normalizeDomain(form.value.domain) || 'example.com',
  adminEmail: normalizeEmail(form.value.adminEmail) || 'admin@example.com',
}))

watch(
  () => form.value.domain,
  (domain) => {
    const normalized = normalizeDomain(domain)
    if (!form.value.adminEmail || /^admin@/.test(form.value.adminEmail)) {
      form.value.adminEmail = normalized ? `admin@${normalized}` : ''
    }
  },
)

function normalizeDomain(value) {
  return String(value || '')
    .trim()
    .toLowerCase()
    .replace(/^https?:\/\//, '')
    .replace(/\/.*$/, '')
    .replace(/\.$/, '')
}

function normalizeEmail(value) {
  return String(value || '').trim().toLowerCase()
}

function trimTrailingDot(value) {
  return String(value || '').replace(/\.$/, '')
}

function absoluteHost(value) {
  const host = trimTrailingDot(value)
  return host ? `${host}.` : ''
}

function relativeHost(host, domainName) {
  const name = trimTrailingDot(host).toLowerCase()
  const zone = trimTrailingDot(domainName).toLowerCase()
  if (name === zone) return '@'
  if (name.endsWith(`.${zone}`)) return name.slice(0, -(zone.length + 1))
  return absoluteHost(name)
}

function escapeTxt(value) {
  return String(value).replace(/\\/g, '\\\\').replace(/"/g, '\\"')
}

function quoteTxt(value) {
  const chunkSize = 240
  const chunks = []
  for (let index = 0; index < value.length; index += chunkSize) {
    chunks.push(value.slice(index, index + chunkSize))
  }
  return chunks.map((chunk) => `"${escapeTxt(chunk)}"`).join(' ')
}

function zoneLine(domain, host, type, value) {
  return `${relativeHost(host, domain)} 3600 IN ${type} ${value}`
}

function setupDomain() {
  return normalizeDomain(setupResult.value?.domain?.domain_name || form.value.domain)
}

function setupHostname() {
  return normalizeDomain(setupResult.value?.hostname || form.value.hostname)
}

function detectedIp(version) {
  const item = setupResult.value?.detected_ips?.[version]
  return item?.address ? item : null
}

function applyManualPublicIps(data) {
  manualServerIps.value = {
    ipv4: data?.manual_public_ips?.ipv4 || manualServerIps.value.ipv4,
    ipv6: data?.manual_public_ips?.ipv6 || manualServerIps.value.ipv6,
  }
}

function publicDetectedIp(version) {
  const item = detectedIp(version)
  return item?.public ? item.address : ''
}

function normalizeIp(value) {
  return String(value || '').trim()
}

function isValidIpv4(value) {
  const parts = normalizeIp(value).split('.')
  return parts.length === 4 && parts.every((part) => {
    if (!/^\d{1,3}$/.test(part)) return false
    const number = Number(part)
    return number >= 0 && number <= 255 && String(number) === String(Number(part))
  })
}

function isValidIpv6(value) {
  const ip = normalizeIp(value)
  if (!ip.includes(':') || /[^0-9a-f:]/i.test(ip)) return false
  if ((ip.match(/::/g) || []).length > 1) return false
  const parts = ip.split(':').filter(Boolean)
  return parts.length <= 8 && parts.every((part) => part.length <= 4)
}

function manualServerIp(version) {
  const value = normalizeIp(manualServerIps.value[version])
  if (!value) return ''
  if (version === 'ipv4') return isValidIpv4(value) ? value : ''
  return isValidIpv6(value) ? value : ''
}

function manualIpInvalid(version) {
  const value = normalizeIp(manualServerIps.value[version])
  if (!value) return false
  return version === 'ipv4' ? !isValidIpv4(value) : !isValidIpv6(value)
}

function effectiveServerIp(version) {
  return manualServerIp(version) || publicDetectedIp(version)
}

function addressRows(domain, hostname) {
  const publicIpv4 = effectiveServerIp('ipv4')
  const publicIpv6 = effectiveServerIp('ipv6')
  const rows = []

  if (publicIpv4) {
    rows.push({
      key: 'a',
      label: 'A',
      purpose: '邮件主机 IPv4',
      value: zoneLine(domain, hostname, 'A', publicIpv4),
    })
  }

  if (publicIpv6) {
    rows.push({
      key: 'aaaa',
      label: 'AAAA',
      purpose: '邮件主机 IPv6',
      value: zoneLine(domain, hostname, 'AAAA', publicIpv6),
    })
  }

  if (!rows.length) {
    rows.push({
      key: 'address-todo',
      label: 'A / AAAA',
      purpose: '邮件主机地址',
      value: '; TODO: add A/AAAA for the mail hostname after confirming the server public IP.',
    })
  }

  return rows
}

function clearFeedback() {
  error.value = ''
  copiedKey.value = ''
}

function validateDomains() {
  form.value.hostname = normalizeDomain(form.value.hostname)
  form.value.domain = normalizeDomain(form.value.domain)

  if (!form.value.hostname) return '请填写服务器主机名'
  if (!form.value.domain) return '请填写邮件域名'
  if (!domainPattern.test(form.value.hostname)) return '服务器主机名格式不正确，例如 mail.example.com'
  if (!domainPattern.test(form.value.domain)) return '邮件域名格式不正确，例如 example.com'
  return ''
}

function validateAdmin() {
  form.value.adminEmail = normalizeEmail(form.value.adminEmail)

  if (!emailPattern.test(form.value.adminEmail)) return '请填写有效的管理员邮箱'
  if (!form.value.adminEmail.endsWith(`@${form.value.domain}`)) {
    return `管理员邮箱建议使用 ${form.value.domain} 域名`
  }
  if (form.value.adminPassword.length < 6) return '密码至少需要 6 个字符'
  if (form.value.adminPassword !== form.value.confirmPassword) return '两次输入的密码不一致'
  return ''
}

function nextStep() {
  clearFeedback()

  if (step.value === 1) {
    error.value = validateDomains()
    if (error.value) return
    step.value = 2
    return
  }

  if (step.value === 2) {
    error.value = validateAdmin()
    if (error.value) return
    step.value = 3
    return
  }

  if (step.value === 3) {
    runSetup()
    return
  }

  router.replace({ name: 'dashboard' })
}

function prevStep() {
  if (step.value <= 1 || step.value >= 4) return
  clearFeedback()
  step.value -= 1
}

async function runSetup() {
  loading.value = true
  clearFeedback()

  try {
    const data = await api.runSetup({
      hostname: form.value.hostname,
      domain: form.value.domain,
      admin_email: form.value.adminEmail,
      admin_password: form.value.adminPassword,
    })

    setupResult.value = data
    applyManualPublicIps(data)
    if (data.token) {
      localStorage.setItem('token', data.token)
      localStorage.setItem('user', JSON.stringify(data.user))
    }
    setInitialized(true)
    step.value = 4
  } catch (err) {
    error.value = setupErrorMessage(err)
  } finally {
    loading.value = false
  }
}

async function saveManualPublicIps() {
  error.value = ''

  if (manualIpInvalid('ipv4') || manualIpInvalid('ipv6')) {
    error.value = '公网 IP 格式不正确'
    return
  }

  savingPublicIps.value = true
  try {
    const data = await api.updateSettings({
      public_ipv4: normalizeIp(manualServerIps.value.ipv4),
      public_ipv6: normalizeIp(manualServerIps.value.ipv6),
    })
    setupResult.value = {
      ...setupResult.value,
      manual_public_ips: data.manual_public_ips,
    }
    applyManualPublicIps(data)
    copiedKey.value = 'public-ips'
  } catch (err) {
    error.value = err.message || '保存公网 IP 失败'
  } finally {
    savingPublicIps.value = false
  }
}

function setupErrorMessage(err) {
  const message = err?.message || ''
  if (message.includes('409') || message.includes('CONFLICT')) return '系统已经初始化，请直接登录'
  if (message.includes('400') || message.includes('BAD_REQUEST')) return '表单信息不完整，请检查后重试'
  return message ? `初始化失败：${message}` : '初始化失败，请稍后重试'
}

async function copyRecord(row) {
  try {
    await navigator.clipboard.writeText(row.value)
    copiedKey.value = row.key
  } catch {
    error.value = '复制失败，请手动选择记录内容'
  }
}

function copyAllRecords() {
  navigator.clipboard.writeText(cloudflareZoneFile.value).then(() => {
    copiedKey.value = 'all'
  }).catch(() => {
    error.value = '复制失败，请手动选择记录内容'
  })
}
</script>

<template>
  <main class="setup-page">
    <section class="setup-shell">
      <aside class="setup-rail">
        <div class="brand-mark">K</div>
        <div>
          <h1>Kuria Mail</h1>
          <p>初始化邮件服务、管理员账号和域名记录。</p>
        </div>

        <div class="progress-track">
          <div class="progress-fill" :style="{ height: progress + '%' }"></div>
        </div>

        <ol class="step-list">
          <li
            v-for="(item, index) in steps"
            :key="item.title"
            :class="{ active: step === index + 1, done: step > index + 1 }"
          >
            <span>{{ index + 1 }}</span>
            <div>
              <strong>{{ item.title }}</strong>
              <small>{{ item.caption }}</small>
            </div>
          </li>
        </ol>
      </aside>

      <MiuixCard class="setup-card">
        <div v-if="step === 1" class="panel">
          <div class="panel-head">
            <p class="eyebrow">第一步</p>
            <h2>配置域名</h2>
            <p>主机名用于 MX 记录，邮件域名用于创建第一个域和管理员邮箱。</p>
          </div>

          <div class="form-grid">
            <label class="field">
              <span>服务器主机名</span>
              <MiuixInput v-model="form.hostname" placeholder="mail.example.com" />
              <small>需要有 A/AAAA 记录指向这台服务器。</small>
            </label>

            <label class="field">
              <span>邮件域名</span>
              <MiuixInput v-model="form.domain" placeholder="example.com" />
              <small>用户邮箱会使用这个域名，例如 user@example.com。</small>
            </label>
          </div>

          <div class="preview-box">
            <span>记录预览</span>
            <code>{{ normalizedPreview.domain }} MX 10 {{ normalizedPreview.hostname }}</code>
          </div>
        </div>

        <div v-else-if="step === 2" class="panel">
          <div class="panel-head">
            <p class="eyebrow">第二步</p>
            <h2>创建管理员</h2>
            <p>这个账号会同时作为 Web 管理员和第一个邮箱用户。</p>
          </div>

          <div class="form-grid">
            <label class="field">
              <span>管理员邮箱</span>
              <MiuixInput v-model="form.adminEmail" placeholder="admin@example.com" />
              <small>建议使用当前邮件域名，便于后续收发测试。</small>
            </label>

            <label class="field">
              <span>密码</span>
              <PasswordInput
                v-model="form.adminPassword"
                placeholder="至少 6 个字符"
                autocomplete="new-password"
              />
              <small class="strength" :class="passwordStrength.className">
                强度：{{ passwordStrength.label }}
              </small>
            </label>

            <label class="field">
              <span>确认密码</span>
              <PasswordInput
                v-model="form.confirmPassword"
                placeholder="再次输入密码"
                autocomplete="new-password"
                @keyup-enter="nextStep"
              />
            </label>
          </div>
        </div>

        <div v-else-if="step === 3" class="panel">
          <div class="panel-head">
            <p class="eyebrow">第三步</p>
            <h2>确认初始化</h2>
            <p>提交后会创建域名和管理员用户，并自动登录到管理后台。</p>
          </div>

          <div class="review-grid">
            <div>
              <span>服务器主机名</span>
              <strong>{{ form.hostname }}</strong>
            </div>
            <div>
              <span>邮件域名</span>
              <strong>{{ form.domain }}</strong>
            </div>
            <div>
              <span>管理员邮箱</span>
              <strong>{{ form.adminEmail }}</strong>
            </div>
          </div>

          <div class="notice">
            初始化完成后会生成 Cloudflare 可导入的 DNS 记录。DKIM 需要进入域名管理生成密钥后再补充。
          </div>
        </div>

        <div v-else class="panel complete-panel">
          <div class="panel-head">
            <p class="eyebrow">完成</p>
            <h2>初始化完成</h2>
            <p>请把下面的记录添加到 DNS 服务商处。记录生效后再测试收发邮件。</p>
          </div>

          <div class="completion-grid">
            <div class="account-summary">
              <span>管理员账号</span>
              <strong>{{ form.adminEmail }}</strong>
              <small>已自动登录</small>
            </div>
            <div class="account-summary">
              <span>Web 管理地址</span>
              <strong>{{ form.hostname }}</strong>
              <small>如果使用 Nginx 反代，请访问你的 HTTPS 域名。</small>
            </div>
          </div>

          <div v-if="dnsRows.length" class="dns-section">
            <div class="dns-head">
              <div>
                <h3>Cloudflare DNS 导入记录</h3>
                <p>复制全部后，可在 Cloudflare DNS 的导入 zone file 功能中粘贴。</p>
              </div>
              <MiuixButton class="app-secondary-button" @click="copyAllRecords">
                {{ copiedKey === 'all' ? '已复制' : '复制全部' }}
              </MiuixButton>
            </div>

            <div class="manual-ip-panel">
              <div>
                <h4>服务器公网 IP</h4>
                <p>自动探测失败时手动填写；Cloudflare 导入文本会优先使用这里的值。</p>
              </div>
              <div class="manual-ip-grid">
                <label class="manual-ip-field">
                  <span>IPv4</span>
                  <MiuixInput
                    v-model="manualServerIps.ipv4"
                    placeholder="公网 IPv4，可留空"
                    @keyup.enter="saveManualPublicIps"
                  />
                  <small v-if="manualIpInvalid('ipv4')" class="field-error">IPv4 格式不正确</small>
                  <small v-else-if="publicDetectedIp('ipv4')">已探测：{{ publicDetectedIp('ipv4') }}</small>
                  <small v-else>{{ savingPublicIps ? '正在保存...' : '未探测到公网 IPv4' }}</small>
                </label>
                <label class="manual-ip-field">
                  <span>IPv6</span>
                  <MiuixInput
                    v-model="manualServerIps.ipv6"
                    placeholder="公网 IPv6，可留空"
                    @keyup.enter="saveManualPublicIps"
                  />
                  <small v-if="manualIpInvalid('ipv6')" class="field-error">IPv6 格式不正确</small>
                  <small v-else-if="publicDetectedIp('ipv6')">已探测：{{ publicDetectedIp('ipv6') }}</small>
                  <small v-else>{{ savingPublicIps ? '正在保存...' : '未探测到公网 IPv6' }}</small>
                </label>
              </div>
              <div class="manual-ip-actions">
                <MiuixButton type="primary" :disabled="savingPublicIps" @click="saveManualPublicIps">
                  {{ savingPublicIps ? '保存中...' : '保存' }}
                </MiuixButton>
              </div>
            </div>

            <pre class="zone-preview"><code>{{ cloudflareZoneFile }}</code></pre>

            <div class="dns-list">
              <div v-for="row in dnsRows" :key="row.key" class="dns-row">
                <div class="dns-meta">
                  <span>{{ row.label }}</span>
                  <small>{{ row.purpose }}</small>
                </div>
                <code>{{ row.value }}</code>
                <MiuixButton class="app-secondary-button" @click="copyRecord(row)">
                  {{ copiedKey === row.key ? '已复制' : '复制' }}
                </MiuixButton>
              </div>
            </div>
          </div>
        </div>

        <p v-if="error" class="error">{{ error }}</p>

        <div class="actions">
          <MiuixButton v-if="step > 1 && step < 4" class="app-secondary-button" :disabled="loading" @click="prevStep">
            上一步
          </MiuixButton>
          <span v-else></span>

          <MiuixButton type="primary" :disabled="loading" @click="nextStep">
            <template v-if="loading">正在初始化...</template>
            <template v-else-if="step === 3">开始初始化</template>
            <template v-else-if="step === 4">进入管理后台</template>
            <template v-else>下一步</template>
          </MiuixButton>
        </div>
      </MiuixCard>
    </section>
  </main>
</template>

<style scoped>
.setup-page {
  min-height: 100vh;
  display: grid;
  place-items: center;
  padding: 28px;
  background:
    linear-gradient(135deg, rgba(15, 118, 110, 0.92), rgba(54, 75, 61, 0.91) 56%, rgba(183, 121, 31, 0.88)),
    var(--m-color-bg);
}

.setup-shell {
  width: min(1080px, 100%);
  display: grid;
  grid-template-columns: 320px minmax(0, 1fr);
  gap: 20px;
  align-items: stretch;
}

.setup-rail {
  position: relative;
  color: white;
  padding: 32px;
  border-radius: var(--app-radius);
  background: rgba(255, 255, 255, 0.14);
  border: 1px solid rgba(255, 255, 255, 0.18);
  overflow: hidden;
}

.brand-mark {
  width: 52px;
  height: 52px;
  display: grid;
  place-items: center;
  margin-bottom: 18px;
  border-radius: var(--app-radius);
  background: rgba(255, 255, 255, 0.18);
  font-size: 26px;
  font-weight: 800;
}

.setup-rail h1 {
  font-size: 30px;
  line-height: 1.1;
  margin-bottom: 8px;
}

.setup-rail p {
  color: rgba(255, 255, 255, 0.82);
  font-size: 14px;
}

.progress-track {
  position: absolute;
  left: 46px;
  top: 188px;
  bottom: 42px;
  width: 2px;
  background: rgba(255, 255, 255, 0.2);
}

.progress-fill {
  width: 100%;
  background: white;
  transition: height 0.25s ease;
}

.step-list {
  position: relative;
  z-index: 1;
  display: flex;
  flex-direction: column;
  gap: 24px;
  margin-top: 48px;
  list-style: none;
}

.step-list li {
  display: grid;
  grid-template-columns: 28px minmax(0, 1fr);
  gap: 14px;
  align-items: start;
  color: rgba(255, 255, 255, 0.62);
}

.step-list li > span {
  width: 28px;
  height: 28px;
  display: grid;
  place-items: center;
  border-radius: 50%;
  background: rgba(255, 255, 255, 0.2);
  color: inherit;
  font-size: 13px;
  font-weight: 700;
}

.step-list li.active,
.step-list li.done {
  color: white;
}

.step-list li.done > span,
.step-list li.active > span {
  color: #0f766e;
  background: white;
}

.step-list strong {
  display: block;
  font-size: 14px;
}

.step-list small {
  display: block;
  margin-top: 2px;
  font-size: 12px;
  color: rgba(255, 255, 255, 0.68);
}

.setup-card {
  min-height: 620px;
  border-radius: var(--app-radius);
  background: var(--m-color-card);
  box-shadow: var(--app-shadow);
}

.panel {
  padding: 38px;
}

.panel-head {
  max-width: 620px;
  margin-bottom: 30px;
}

.eyebrow {
  margin-bottom: 8px;
  color: var(--app-info);
  font-size: 12px;
  font-weight: 800;
}

.panel h2 {
  color: var(--m-color-text);
  font-size: 28px;
  line-height: 1.15;
  margin-bottom: 8px;
}

.panel-head p:not(.eyebrow) {
  color: var(--m-color-text-secondary);
  font-size: 14px;
}

.form-grid {
  display: grid;
  gap: 18px;
  max-width: 560px;
}

.field {
  display: grid;
  gap: 8px;
}

.field span {
  color: var(--m-color-text);
  font-size: 14px;
  font-weight: 650;
}

.field small,
.account-summary small {
  color: var(--m-color-text-secondary);
  font-size: 12px;
}

.strength.weak {
  color: var(--app-danger);
}

.strength.medium {
  color: var(--app-warning);
}

.strength.strong {
  color: var(--app-success);
}

.preview-box,
.notice {
  margin-top: 24px;
  max-width: 640px;
  padding: 16px;
  border-radius: var(--app-radius);
  background: var(--m-color-bg);
  color: var(--m-color-text-secondary);
}

.preview-box {
  display: grid;
  gap: 8px;
}

.preview-box span {
  font-size: 12px;
  font-weight: 700;
  color: var(--m-color-text-secondary);
}

code {
  font-family: Consolas, Monaco, monospace;
  font-size: 12px;
  color: var(--m-color-text);
  overflow-wrap: anywhere;
}

.review-grid,
.completion-grid {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 12px;
}

.review-grid > div,
.account-summary {
  display: grid;
  gap: 6px;
  padding: 16px;
  border-radius: var(--app-radius);
  background: var(--m-color-bg);
}

.review-grid span,
.account-summary span {
  color: var(--m-color-text-secondary);
  font-size: 12px;
}

.review-grid strong,
.account-summary strong {
  color: var(--m-color-text);
  font-size: 15px;
  overflow-wrap: anywhere;
}

.completion-grid {
  grid-template-columns: repeat(2, minmax(0, 1fr));
  margin-bottom: 22px;
}

.dns-section {
  margin-top: 18px;
}

.dns-head,
.dns-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}

.dns-head {
  margin-bottom: 12px;
}

.dns-head h3 {
  color: var(--m-color-text);
  font-size: 17px;
}

.dns-head p {
  margin-top: 4px;
  color: var(--m-color-text-secondary);
  font-size: 12px;
}

.manual-ip-panel {
  display: grid;
  grid-template-columns: minmax(180px, 0.8fr) minmax(0, 1.4fr);
  gap: 16px;
  margin-bottom: 12px;
  padding: 14px;
  border: 1px solid var(--m-color-border);
  border-radius: var(--app-radius);
  background: var(--m-color-bg);
}

.manual-ip-panel h4 {
  color: var(--m-color-text);
  font-size: 14px;
  font-weight: 750;
}

.manual-ip-panel p,
.manual-ip-field small {
  color: var(--m-color-text-secondary);
  font-size: 12px;
  line-height: 1.45;
}

.manual-ip-panel p {
  margin-top: 4px;
}

.manual-ip-grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 10px;
}

.manual-ip-field {
  display: grid;
  gap: 6px;
  min-width: 0;
}

.manual-ip-field span {
  color: var(--m-color-text);
  font-size: 12px;
  font-weight: 750;
}

.manual-ip-field .field-error {
  color: var(--app-danger);
}

.manual-ip-actions {
  display: flex;
  justify-content: flex-end;
  grid-column: 2;
}

.zone-preview {
  max-height: 210px;
  margin-bottom: 12px;
  padding: 14px;
  overflow: auto;
  border: 1px solid var(--m-color-border);
  border-radius: var(--app-radius);
  background: var(--m-color-bg);
}

.zone-preview code {
  white-space: pre;
}

.dns-list {
  display: grid;
  gap: 10px;
}

.dns-row {
  padding: 14px;
  border: 1px solid var(--m-color-border);
  border-radius: var(--app-radius);
  background: var(--m-color-bg);
}

.dns-meta {
  width: 118px;
  flex-shrink: 0;
}

.dns-meta span {
  display: block;
  color: var(--app-info);
  font-size: 12px;
  font-weight: 800;
}

.dns-meta small {
  display: block;
  margin-top: 3px;
  color: var(--m-color-text-secondary);
  font-size: 12px;
}

.dns-row code {
  flex: 1;
  padding: 8px 10px;
  border-radius: var(--app-radius);
  background: var(--m-color-card);
}

.error {
  margin: 0 38px;
  padding: 12px 14px;
  border-radius: var(--app-radius);
  color: var(--app-danger);
  background: color-mix(in srgb, var(--app-danger) 10%, transparent);
  font-size: 14px;
}

.actions {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding: 0 38px 38px;
}

@media (max-width: 880px) {
  .setup-page {
    place-items: start center;
    padding: 18px;
  }

  .setup-shell {
    grid-template-columns: 1fr;
  }

  .setup-rail {
    padding: 24px;
  }

  .progress-track,
  .step-list small {
    display: none;
  }

  .step-list {
    flex-direction: row;
    gap: 10px;
    margin-top: 24px;
  }

  .step-list li {
    grid-template-columns: 28px;
  }

  .step-list div {
    display: none;
  }

  .setup-card {
    min-height: auto;
  }
}

@media (max-width: 640px) {
  .panel {
    padding: 24px;
  }

  .panel h2 {
    font-size: 24px;
  }

  .review-grid,
  .completion-grid,
  .manual-ip-panel,
  .manual-ip-grid {
    grid-template-columns: 1fr;
  }

  .manual-ip-actions {
    grid-column: auto;
  }

  .dns-row {
    align-items: stretch;
    flex-direction: column;
  }

  .dns-meta {
    width: auto;
  }

  .actions {
    align-items: stretch;
    flex-direction: column;
    padding: 0 24px 24px;
  }

  .actions > * {
    width: 100%;
  }

  .error {
    margin: 0 24px;
  }
}
</style>
