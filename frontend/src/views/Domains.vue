<script setup>
import { computed, onMounted, ref } from 'vue'
import { MiuixButton, MiuixCard, MiuixDialog, MiuixInput } from 'miuix-vue'
import { api } from '../api'

const domains = ref([])
const settings = ref(null)
const loading = ref(true)
const saving = ref(false)
const generatingId = ref(null)
const newDomain = ref('')
const showAddDialog = ref(false)
const message = ref('')
const error = ref('')
const expandedDomains = ref({})
const manualServerIps = ref({
  ipv4: '',
  ipv6: '',
})
const savingPublicIps = ref(false)

const normalizedDomain = computed(() => normalizeDomain(newDomain.value))

function normalizeDomain(value) {
  return String(value || '')
    .trim()
    .toLowerCase()
    .replace(/^https?:\/\//, '')
    .replace(/\/.*$/, '')
    .replace(/\.$/, '')
}

function isValidDomain(value) {
  return /^[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?(?:\.[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?)+$/.test(value)
}

function trimTrailingDot(value) {
  return String(value || '').replace(/\.$/, '')
}

function absoluteHost(value) {
  const host = trimTrailingDot(value)
  return host ? `${host}.` : ''
}

function mailHost(domain) {
  const hostname = normalizeDomain(settings.value?.hostname)
  return isValidDomain(hostname) ? hostname : `mail.${domain.domain_name}`
}

function detectedIp(version) {
  const item = settings.value?.detected_ips?.[version]
  return item?.address ? item : null
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

function applyManualPublicIps(data) {
  manualServerIps.value = {
    ipv4: data?.manual_public_ips?.ipv4 || '',
    ipv6: data?.manual_public_ips?.ipv6 || '',
  }
}

function effectiveServerIp(version) {
  return manualServerIp(version) || publicDetectedIp(version)
}

function addressRecordLines(domain) {
  const mxHost = mailHost(domain)
  return [
    effectiveServerIp('ipv4') ? { type: 'A', value: zoneLine(domain, mxHost, 'A', effectiveServerIp('ipv4')) } : null,
    effectiveServerIp('ipv6') ? { type: 'AAAA', value: zoneLine(domain, mxHost, 'AAAA', effectiveServerIp('ipv6')) } : null,
  ].filter(Boolean)
}

function rowActionsLabel(record, domain) {
  if (record.type === 'A/AAAA') {
    return savingPublicIps.value ? '保存中...' : '保存'
  }
  if (record.type === 'DKIM') {
    return generatingId.value === domain.id
      ? '生成中...'
      : domain.dkim_public_key
        ? '重新生成'
        : '生成 DKIM'
  }
  return ''
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
  return `${relativeHost(host, domain.domain_name)} 3600 IN ${type} ${value}`
}

function spfValue(domain) {
  return domain.spf_record || `v=spf1 mx:${domain.domain_name} -all`
}

function dkimSelector(domain) {
  return domain.dkim_selector || 'kuria'
}

function dkimHost(domain) {
  return `${dkimSelector(domain)}._domainkey.${domain.domain_name}`
}

function dkimValue(domain) {
  return domain.dkim_public_key ? `v=DKIM1; k=rsa; p=${domain.dkim_public_key}` : ''
}

function dmarcHost(domain) {
  return `_dmarc.${domain.domain_name}`
}

function dmarcValue(domain) {
  return `v=DMARC1; p=quarantine; rua=mailto:postmaster@${domain.domain_name}`
}

function publicKeyFingerprint(value) {
  if (!value) return ''
  let hash = 2166136261
  for (let index = 0; index < value.length; index += 1) {
    hash = Math.imul(hash ^ value.charCodeAt(index), 16777619)
  }
  return (hash >>> 0).toString(16).toUpperCase().padStart(8, '0')
}

function dnsRecords(domain) {
  const spf = spfValue(domain)
  const dkim = dkimValue(domain)
  const dmarc = dmarcValue(domain)
  const mxHost = mailHost(domain)
  const publicIpv4 = effectiveServerIp('ipv4')
  const publicIpv6 = effectiveServerIp('ipv6')
  const detectedIpv4 = detectedIp('ipv4')?.address || ''
  const detectedIpv6 = detectedIp('ipv6')?.address || ''
  const mailHostInThisZone = trimTrailingDot(mxHost).endsWith(`.${domain.domain_name}`)
  const fingerprint = publicKeyFingerprint(domain.dkim_public_key)
  const addressLines = [
    publicIpv4 ? zoneLine(domain, mxHost, 'A', publicIpv4) : '',
    publicIpv6 ? zoneLine(domain, mxHost, 'AAAA', publicIpv6) : '',
  ].filter(Boolean)
  const detectedIpText = [detectedIpv4, detectedIpv6].filter(Boolean).join(' / ')

  return [
    {
      type: 'A/AAAA',
      title: '邮件主机地址',
      dnsType: 'A 或 AAAA',
      host: mxHost,
      value: addressLines.length
        ? addressLines.join('\n')
        : detectedIpText || '未探测到可用于 DNS 的公网 IP',
      line: addressLines.join('\n'),
      ready: Boolean(addressLines.length),
      note: `${mxHost} 必须能解析到这台邮件服务器。`,
      detail: addressLines.length
        ? '已根据公网 IP 生成，可直接导入。'
        : mailHostInThisZone
          ? '未探测到公网 IP，请手动添加 A/AAAA。'
          : '邮件主机不在当前域名下，请到对应 DNS 区域配置 A/AAAA。',
      copyLabel: '邮件主机地址记录',
    },
    {
      type: 'MX',
      title: '收信入口',
      dnsType: 'MX',
      host: domain.domain_name,
      value: `10 ${absoluteHost(mxHost)}`,
      line: zoneLine(domain, domain.domain_name, 'MX', `10 ${absoluteHost(mxHost)}`),
      ready: true,
      note: `把 ${domain.domain_name} 的收件服务器指向 ${mxHost}。`,
      detail: '接收外部邮件必需。',
      copyLabel: 'MX 记录',
    },
    {
      type: 'SPF',
      title: 'SPF 发信授权',
      dnsType: 'TXT',
      host: domain.domain_name,
      value: spf,
      line: zoneLine(domain, domain.domain_name, 'TXT', quoteTxt(spf)),
      ready: Boolean(spf),
      note: '告诉收件方哪些服务器可以代表这个域名发信。',
      detail: spf ? '已生成，添加到 DNS 的 TXT 记录即可。' : '缺少 SPF 记录。',
      copyLabel: 'SPF 记录',
    },
    {
      type: 'DKIM',
      title: 'DKIM 签名公钥',
      dnsType: 'TXT',
      host: dkimHost(domain),
      value: dkim,
      line: dkim ? zoneLine(domain, dkimHost(domain), 'TXT', `"${dkim}"`) : '',
      ready: Boolean(dkim),
      note: dkim
        ? `selector ${dkimSelector(domain)}，key hash ${fingerprint}`
        : '生成密钥后，把 TXT 记录添加到 DNS，用于验证邮件签名。',
      detail: dkim ? '重新生成会得到新的 selector 和公钥。' : '尚未生成。',
      copyLabel: 'DKIM 记录',
    },
    {
      type: 'DMARC',
      title: 'DMARC 策略',
      dnsType: 'TXT',
      host: dmarcHost(domain),
      value: dmarc,
      line: zoneLine(domain, dmarcHost(domain), 'TXT', quoteTxt(dmarc)),
      ready: true,
      note: '告诉收件方 SPF/DKIM 失败时如何处理。',
      detail: '建议先用 quarantine，确认投递稳定后再收紧。',
      copyLabel: 'DMARC 记录',
    },
  ]
}

function recordStatusCounts(domain) {
  const records = dnsRecords(domain)
  return {
    ready: records.filter((record) => record.ready).length,
    total: records.length,
  }
}

function cloudflareZoneFile(domain) {
  const records = dnsRecords(domain)
  const mxHost = mailHost(domain)
  const mailHostName = trimTrailingDot(mxHost)
  const mailHostRelative = relativeHost(mailHostName, domain.domain_name)
  const mailHostInThisZone = !mailHostRelative.endsWith('.')
  const ipv4 = detectedIp('ipv4')
  const ipv6 = detectedIp('ipv6')
  const importedIpv4 = effectiveServerIp('ipv4')
  const importedIpv6 = effectiveServerIp('ipv6')
  const lines = [
    `$ORIGIN ${absoluteHost(domain.domain_name)}`,
    '$TTL 3600',
    '; Cloudflare zone file for Kuria Mail',
  ]

  if (!mailHostInThisZone) {
    lines.push(`; TODO: make sure ${absoluteHost(mailHostName)} has A/AAAA records in its own DNS zone.`)
  } else if (!importedIpv4 && !importedIpv6) {
    lines.push('; TODO before import: no public server IP was detected automatically.')
    if (ipv4?.address) lines.push(`; Detected IPv4 ${ipv4.address} is not public, so it was not imported.`)
    if (ipv6?.address) lines.push(`; Detected IPv6 ${ipv6.address} is not public, so it was not imported.`)
    lines.push(`; ${mailHostRelative} 3600 IN A <server-ipv4>`)
    lines.push(`; ${mailHostRelative} 3600 IN AAAA <server-ipv6>`)
  }

  records.forEach((record) => {
    if (record.line) lines.push(record.line)
    if (record.type === 'DKIM' && !record.ready) {
      lines.push('; TODO: generate DKIM in Kuria, then import the DKIM TXT record.')
    }
  })

  return `${lines.join('\n')}\n`
}

function isDomainExpanded(id) {
  return Boolean(expandedDomains.value[id])
}

function setDomainExpanded(id, expanded) {
  expandedDomains.value = {
    ...expandedDomains.value,
    [id]: expanded,
  }
}

function toggleDomainRecords(id) {
  setDomainExpanded(id, !isDomainExpanded(id))
}

async function loadDomains() {
  loading.value = true
  error.value = ''
  try {
    const [domainsData, settingsData] = await Promise.all([api.getDomains(), api.getSettings()])
    domains.value = domainsData.domains || []
    settings.value = settingsData
    applyManualPublicIps(settingsData)
  } catch (err) {
    error.value = err.message || '加载域名失败'
  } finally {
    loading.value = false
  }
}

async function saveManualPublicIps() {
  message.value = ''
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
    settings.value = { ...settings.value, ...data }
    applyManualPublicIps(data)
    message.value = '公网 IP 已保存'
  } catch (err) {
    error.value = err.message || '保存公网 IP 失败'
  } finally {
    savingPublicIps.value = false
  }
}

async function addDomain() {
  message.value = ''
  error.value = ''
  const domain = normalizedDomain.value
  if (!isValidDomain(domain)) {
    error.value = '请输入有效域名，例如 example.com'
    return
  }

  saving.value = true
  try {
    await api.createDomain(domain)
    newDomain.value = ''
    showAddDialog.value = false
    message.value = '域名已添加'
    await loadDomains()
  } catch (err) {
    if (err.status === 400) {
      error.value = '域名格式不正确，请输入类似 example.com 的域名'
    } else {
      error.value = err.message || '添加域名失败'
    }
  } finally {
    saving.value = false
  }
}

async function deleteDomain(id, name) {
  message.value = ''
  error.value = ''
  if (!confirm(`确定删除域名 ${name}？相关用户可能无法继续收发邮件。`)) return

  try {
    await api.deleteDomain(id)
    domains.value = domains.value.filter((domain) => domain.id !== id)
    message.value = '域名已删除'
  } catch (err) {
    if (err.status === 409) {
      error.value = '这个域名下还有用户，请先删除或迁移相关用户后再删除域名'
    } else if (err.status === 404) {
      error.value = '域名不存在或已被删除'
      await loadDomains()
    } else {
      error.value = err.message || '删除失败'
    }
  }
}

async function generateDkim(domain) {
  message.value = ''
  error.value = ''
  generatingId.value = domain.id
  try {
    const data = await api.generateDkim(domain.id)
    await loadDomains()

    const updatedDomain =
      data.domain || domains.value.find((item) => item.id === domain.id) || domain
    const selector = data.selector || dkimSelector(updatedDomain)
    const fingerprint = publicKeyFingerprint(updatedDomain.dkim_public_key)
    message.value = fingerprint
      ? `DKIM 已重新生成：selector ${selector}，key hash ${fingerprint}`
      : 'DKIM 密钥已生成'
    setDomainExpanded(domain.id, true)
  } catch (err) {
    error.value = err.message || '生成 DKIM 记录失败'
  } finally {
    generatingId.value = null
  }
}

async function copyToClipboard(text, label = '记录') {
  if (!text) return
  try {
    if (navigator.clipboard?.writeText) {
      await navigator.clipboard.writeText(text)
      message.value = `${label}已复制`
    } else {
      fallbackCopy(text, label)
    }
  } catch {
    fallbackCopy(text, label)
  }
}

function fallbackCopy(text, label) {
  const textarea = document.createElement('textarea')
  textarea.value = text
  textarea.style.position = 'fixed'
  textarea.style.opacity = '0'
  document.body.appendChild(textarea)
  textarea.select()
  try {
    document.execCommand('copy')
    message.value = `${label}已复制`
  } catch {
    error.value = '复制失败，请手动选中记录复制'
  } finally {
    document.body.removeChild(textarea)
  }
}

function copyAllRecords(domain) {
  copyToClipboard(cloudflareZoneFile(domain), 'Cloudflare 导入记录')
}

function formatDate(dateStr) {
  if (!dateStr) return '未知时间'
  return new Date(dateStr).toLocaleDateString('zh-CN')
}

onMounted(loadDomains)
</script>

<template>
  <div class="domains">
    <div class="page-header">
      <div>
        <h1>域名管理</h1>
        <p class="subtitle">每个域名的 SPF 和 DKIM 记录放在同一张卡片里，方便复制到 DNS。</p>
      </div>
      <div class="header-actions">
        <MiuixButton class="app-secondary-button" @click="loadDomains">刷新</MiuixButton>
        <MiuixButton type="primary" @click="showAddDialog = true">添加域名</MiuixButton>
      </div>
    </div>

    <p v-if="message" class="notice success">{{ message }}</p>
    <p v-if="error" class="notice error">{{ error }}</p>

    <div v-if="loading" class="loading">正在加载域名...</div>

    <div v-else-if="domains.length === 0" class="empty">
      <p>还没有配置域名。</p>
      <MiuixButton type="primary" @click="showAddDialog = true">添加第一个域名</MiuixButton>
    </div>

    <div v-else class="domain-list">
      <MiuixCard v-for="domain in domains" :key="domain.id">
        <div class="card-inner">
          <div class="domain-head">
            <div class="domain-title">
              <div class="domain-name-row">
                <div class="domain-name">{{ domain.domain_name }}</div>
                <span class="compact-status">
                  DNS {{ recordStatusCounts(domain).ready }}/{{ recordStatusCounts(domain).total }}
                </span>
              </div>
              <div class="domain-meta">创建于 {{ formatDate(domain.created_at) }}</div>
            </div>
            <div class="domain-actions">
              <MiuixButton class="app-secondary-button" @click="copyAllRecords(domain)">复制</MiuixButton>
              <MiuixButton class="app-secondary-button" @click="toggleDomainRecords(domain.id)">
                {{ isDomainExpanded(domain.id) ? '收起' : '展开' }}
              </MiuixButton>
              <MiuixButton class="app-danger-button" @click="deleteDomain(domain.id, domain.domain_name)">删除</MiuixButton>
            </div>
          </div>

          <div v-if="isDomainExpanded(domain.id)" class="dns-list">
            <div class="dns-toolbar">
            <div class="dns-statuses">
              <span
                v-for="record in dnsRecords(domain)"
                :key="record.type"
                class="record-chip"
                :class="{ ready: record.ready, missing: !record.ready }"
              >
                <strong>{{ record.type }}</strong>
                {{ record.ready ? '可用' : '待处理' }}
              </span>
            </div>
            </div>

            <div
              v-for="record in dnsRecords(domain)"
              :key="record.type"
              class="dns-row"
              :class="{ missing: !record.ready }"
            >
              <div class="record-summary">
                <span class="record-type">{{ record.type }}</span>
                <div>
                  <h2>{{ record.title }}</h2>
                  <p>{{ record.note }}</p>
                </div>
              </div>

              <div class="record-values">
                <div class="record-field">
                  <span>主机名</span>
                  <code>{{ record.host }}</code>
                </div>
                <div class="record-field">
                  <span>{{ record.dnsType }} 值</span>
                  <div v-if="record.type === 'A/AAAA'" class="address-record-box">
                    <div class="manual-ip-grid">
                      <label class="manual-ip-field">
                        <span>公网 IPv4</span>
                        <MiuixInput
                          v-model="manualServerIps.ipv4"
                          placeholder="可留空"
                          @keyup.enter="saveManualPublicIps"
                        />
                        <small v-if="manualIpInvalid('ipv4')" class="field-error">IPv4 格式不正确</small>
                        <small v-else-if="publicDetectedIp('ipv4')">已探测：{{ publicDetectedIp('ipv4') }}</small>
                        <small v-else>{{ savingPublicIps ? '正在保存...' : '未探测到公网 IPv4' }}</small>
                      </label>
                      <label class="manual-ip-field">
                        <span>公网 IPv6</span>
                        <MiuixInput
                          v-model="manualServerIps.ipv6"
                          placeholder="可留空"
                          @keyup.enter="saveManualPublicIps"
                        />
                        <small v-if="manualIpInvalid('ipv6')" class="field-error">IPv6 格式不正确</small>
                        <small v-else-if="publicDetectedIp('ipv6')">已探测：{{ publicDetectedIp('ipv6') }}</small>
                        <small v-else>{{ savingPublicIps ? '正在保存...' : '未探测到公网 IPv6' }}</small>
                      </label>
                    </div>
                    <div v-if="addressRecordLines(domain).length" class="address-lines">
                      <div
                        v-for="line in addressRecordLines(domain)"
                        :key="line.type"
                        class="address-line"
                      >
                        <span>{{ line.type }}</span>
                        <code>{{ line.value }}</code>
                      </div>
                    </div>
                    <em v-else>未生成 A/AAAA 记录，请填写公网 IP</em>
                  </div>
                  <code v-else-if="record.value">{{ record.value }}</code>
                  <em v-else>未生成</em>
                </div>
              </div>

              <div class="record-actions">
                <span class="status" :class="{ ready: record.ready }">
                  {{ record.ready ? '可用' : '待处理' }}
                </span>
                <small>{{ record.detail }}</small>
                <div class="record-button-row">
                  <MiuixButton
                    v-if="record.type === 'A/AAAA'"
                    type="primary"
                    :disabled="savingPublicIps"
                    @click="saveManualPublicIps"
                  >
                    {{ rowActionsLabel(record, domain) }}
                  </MiuixButton>
                  <MiuixButton
                    v-if="record.type === 'DKIM'"
                    type="primary"
                    :disabled="generatingId === domain.id"
                    @click="generateDkim(domain)"
                  >
                    {{ rowActionsLabel(record, domain) }}
                  </MiuixButton>
                  <MiuixButton
                    class="app-secondary-button dns-copy-button"
                    :disabled="!record.line"
                    @click="copyToClipboard(record.line, record.copyLabel)"
                  >
                    复制
                  </MiuixButton>
                </div>
              </div>
            </div>
          </div>
        </div>
      </MiuixCard>
    </div>

    <MiuixDialog v-model="showAddDialog" title="添加域名">
      <div class="add-form">
        <label>域名</label>
        <MiuixInput v-model="newDomain" placeholder="example.com" @keyup.enter="addDomain" />
        <p class="hint">系统会自动移除协议和路径，只保留域名。</p>
      </div>
      <template #footer="{ close }">
        <MiuixButton class="app-secondary-button" @click="close">取消</MiuixButton>
        <MiuixButton type="primary" :disabled="saving" @click="addDomain">
          {{ saving ? '添加中...' : '添加' }}
        </MiuixButton>
      </template>
    </MiuixDialog>
  </div>
</template>

<style scoped>
.domains {
  max-width: 1080px;
}

.page-header,
.domain-head,
.header-actions,
.domain-actions {
  display: flex;
  gap: 12px;
}

.page-header,
.domain-head {
  align-items: flex-start;
  justify-content: space-between;
}

.page-header {
  margin-bottom: 22px;
}

.domains h1 {
  color: var(--m-color-text);
  font-size: 26px;
  font-weight: 700;
}

.subtitle,
.domain-meta {
  color: var(--m-color-text-secondary);
  font-size: 14px;
}

.subtitle {
  margin-top: 4px;
}

.header-actions,
.domain-actions {
  flex-shrink: 0;
}

.notice,
.loading,
.empty {
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

.loading,
.empty {
  text-align: center;
  color: var(--m-color-text-secondary);
  padding: 56px 20px;
}

.empty p {
  margin-bottom: 16px;
}

.domain-list {
  display: flex;
  flex-direction: column;
  gap: 14px;
}

.card-inner {
  padding: 14px 16px;
}

.domain-title {
  min-width: 0;
}

.domain-name-row {
  display: flex;
  align-items: baseline;
  gap: 10px;
  min-width: 0;
}

.domain-name {
  color: var(--m-color-text);
  font-size: 18px;
  font-weight: 750;
  line-height: 28px;
  overflow-wrap: anywhere;
}

.compact-status {
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  height: 22px;
  padding: 0 8px;
  border-radius: 999px;
  color: var(--app-success);
  background: color-mix(in srgb, var(--app-success) 11%, transparent);
  font-size: 12px;
  font-weight: 750;
  line-height: 1;
  transform: translateY(-1px);
}

.domain-meta {
  margin-top: 3px;
}

.dns-list {
  display: flex;
  flex-direction: column;
  gap: 12px;
  margin-top: 12px;
}

.manual-ip-field small {
  color: var(--m-color-text-secondary);
  font-size: 12px;
  line-height: 1.45;
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

.address-record-box {
  display: grid;
  gap: 10px;
  min-height: 42px;
  padding: 10px;
  background: var(--m-color-card);
  border: 1px solid color-mix(in srgb, var(--m-color-border) 70%, transparent);
  border-radius: var(--app-radius);
}

.address-lines {
  display: grid;
  gap: 8px;
}

.address-line {
  display: grid;
  grid-template-columns: 54px minmax(0, 1fr);
  gap: 8px;
  align-items: start;
}

.dns-toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding: 12px 14px;
  border: 1px solid var(--m-color-border);
  border-radius: var(--app-radius);
  background: var(--m-color-bg);
}

.dns-statuses {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  min-width: 0;
}

.record-chip {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  min-height: 30px;
  padding: 4px 10px;
  border: 1px solid color-mix(in srgb, var(--m-color-border) 78%, transparent);
  border-radius: 999px;
  color: var(--m-color-text-secondary);
  background: var(--m-color-card);
  font-size: 12px;
  font-weight: 650;
}

.record-chip strong {
  color: var(--m-color-text);
  font-weight: 800;
}

.record-chip.ready {
  color: var(--app-success);
  background: color-mix(in srgb, var(--app-success) 10%, transparent);
  border-color: color-mix(in srgb, var(--app-success) 26%, transparent);
}

.dns-row {
  display: grid;
  grid-template-columns: minmax(170px, 0.65fr) minmax(0, 1.7fr) 178px;
  gap: 16px;
  align-items: stretch;
  padding: 14px;
  border: 1px solid var(--m-color-border);
  border-radius: var(--app-radius);
  background: var(--m-color-bg);
}

.dns-row.missing {
  border-style: dashed;
}

.record-summary {
  display: flex;
  gap: 10px;
  min-width: 0;
}

.record-type {
  flex-shrink: 0;
  width: 48px;
  height: 28px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border-radius: 999px;
  color: var(--m-color-primary);
  background: color-mix(in srgb, var(--m-color-primary) 12%, transparent);
  font-size: 12px;
  font-weight: 800;
}

.record-summary h2 {
  color: var(--m-color-text);
  font-size: 14px;
  font-weight: 750;
  line-height: 1.35;
}

.record-summary p,
.record-actions small {
  color: var(--m-color-text-secondary);
  font-size: 12px;
  line-height: 1.45;
}

.record-summary p {
  margin-top: 4px;
}

.record-values {
  display: grid;
  grid-template-columns: minmax(120px, 0.68fr) minmax(0, 1.32fr);
  gap: 10px;
  min-width: 0;
}

.record-field {
  min-width: 0;
}

.record-field span {
  display: block;
  margin-bottom: 5px;
  color: var(--m-color-text-secondary);
  font-size: 12px;
  font-weight: 650;
}

.record-field code,
.record-field em {
  display: block;
  min-height: 42px;
  padding: 10px;
  color: var(--m-color-text);
  background: var(--m-color-card);
  border: 1px solid color-mix(in srgb, var(--m-color-border) 70%, transparent);
  border-radius: var(--app-radius);
  font-family: Consolas, Monaco, monospace;
  font-size: 12px;
  font-style: normal;
  line-height: 1.45;
  overflow-wrap: anywhere;
  word-break: break-word;
}

.record-field em {
  color: var(--m-color-text-secondary);
  font-family: inherit;
}

.record-field .address-line > span {
  display: grid;
  place-items: center;
  height: 26px;
  margin: 0;
  padding: 0 8px;
  border-radius: 999px;
  color: var(--m-color-primary);
  background: color-mix(in srgb, var(--m-color-primary) 11%, transparent);
  font-size: 11px;
  font-weight: 800;
  line-height: 1;
}

.record-field .address-line code {
  min-height: 26px;
  padding: 5px 0;
  background: transparent;
  border: 0;
}

.record-actions {
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  justify-content: space-between;
  gap: 10px;
}

.record-button-row {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}

.record-button-row :deep(.m-button) {
  min-height: 32px;
  padding: 9px 12px;
  border-radius: var(--app-radius);
  font-size: 14px;
}

.record-button-row :deep(.m-button.dns-copy-button) {
  color: var(--m-color-text);
  background: var(--m-color-card);
  border: 1px solid var(--m-color-border);
}

.record-button-row :deep(.m-button.dns-copy-button:hover:not(:disabled)) {
  border-color: var(--m-color-primary);
  background: color-mix(in srgb, var(--m-color-primary) 8%, var(--m-color-card));
}

.record-button-row :deep(.m-button.dns-copy-button:disabled) {
  opacity: 0.52;
}

.status {
  width: fit-content;
  padding: 4px 9px;
  border-radius: 999px;
  color: var(--m-color-text-secondary);
  background: var(--m-color-card);
  font-size: 12px;
  font-weight: 750;
}

.status.ready {
  color: var(--app-success);
  background: color-mix(in srgb, var(--app-success) 13%, transparent);
}

.add-form {
  padding: 8px 0;
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.add-form label {
  color: var(--m-color-text);
  font-size: 14px;
  font-weight: 650;
}

.hint {
  color: var(--m-color-text-secondary);
  font-size: 12px;
}

@media (max-width: 940px) {
  .dns-row {
    grid-template-columns: 1fr;
  }

  .record-actions {
    align-items: center;
    flex-direction: row;
  }
}

@media (max-width: 680px) {
  .page-header,
  .domain-head {
    align-items: stretch;
    flex-direction: column;
  }

  .domain-name-row {
    align-items: flex-start;
    flex-direction: column;
    gap: 5px;
  }

  .header-actions,
  .domain-actions,
  .dns-toolbar,
  .record-actions {
    flex-wrap: wrap;
  }

  .header-actions > *,
  .domain-actions > *,
  .record-actions > button {
    flex: 1;
  }

  .record-values {
    grid-template-columns: 1fr;
  }

  .manual-ip-grid {
    grid-template-columns: 1fr;
  }

  .address-line {
    grid-template-columns: 1fr;
  }
}
</style>
