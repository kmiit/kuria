<script setup>
import { ref, onMounted, watch, computed } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { MiuixButton, MiuixInput, MiuixCard, MiuixSwitch } from 'miuix-vue'
import { api } from '../api'
import { notifyMailboxCountsChanged } from '../mailboxEvents'

const route = useRoute()
const router = useRouter()
const draftKey = 'kuria-compose-draft'

const to = ref('')
const cc = ref('')
const bcc = ref('')
const subject = ref('')
const body = ref('')
const sending = ref(false)
const savingDraft = ref(false)
const result = ref('')
const showCc = ref(false)
const showBcc = ref(false)
const sendAsHtml = ref(false)
const restoredDraft = ref(false)
const serverDraftLoaded = ref(false)
const draftId = ref(route.query.draft ? Number(route.query.draft) : null)
const fileInput = ref(null)
const selectedAttachments = ref([])

const maxAttachments = 10
const maxAttachmentBytes = 10 * 1024 * 1024
const maxTotalAttachmentBytes = 25 * 1024 * 1024

const user = computed(() => {
  try {
    return JSON.parse(localStorage.getItem('user') || '{}')
  } catch {
    return {}
  }
})

const bodyCount = computed(() => body.value.length)
const pageTitle = computed(() => (draftId.value ? '编辑草稿' : '写邮件'))
const resultIsSuccess = computed(
  () => ['邮件已发送', '草稿已保存', '草稿已清除'].includes(result.value)
    || result.value.startsWith('已加入原邮件')
    || result.value.startsWith('已恢复草稿附件'),
)
const attachmentsTotalSize = computed(() =>
  selectedAttachments.value.reduce((total, item) => total + item.file.size, 0),
)
const attachmentsSummary = computed(() => {
  if (!selectedAttachments.value.length) return ''
  return `${selectedAttachments.value.length} 个附件，${formatBytes(attachmentsTotalSize.value)}`
})

function splitRecipients(value) {
  return value
    .split(/[,\n;]/)
    .map((s) => extractEmailAddress(s))
    .filter(Boolean)
}

function extractEmailAddress(value) {
  const trimmed = String(value || '').trim()
  const match = trimmed.match(/<([^<>]+)>/)
  if (match) return match[1].trim()
  return trimmed.replace(/^"|"$/g, '').trim()
}

function validateEmails(list) {
  return list.filter((email) => !/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(email))
}

function formatBytes(bytes) {
  if (!bytes) return '0 B'
  const units = ['B', 'KB', 'MB', 'GB']
  const index = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1)
  const value = bytes / 1024 ** index
  return `${value >= 10 || index === 0 ? value.toFixed(0) : value.toFixed(1)} ${units[index]}`
}

function openAttachmentPicker() {
  fileInput.value?.click()
}

function makeAttachmentItem(file, source = 'manual') {
  return {
    id: `${file.name}-${file.size}-${file.lastModified}-${crypto.randomUUID?.() || Date.now()}`,
    file,
    source,
  }
}

function addAttachmentFiles(files, source = 'manual') {
  if (!files.length) return true
  if (selectedAttachments.value.length + files.length > maxAttachments) {
    result.value = `最多添加 ${maxAttachments} 个附件`
    return false
  }

  const oversized = files.find((file) => file.size > maxAttachmentBytes)
  if (oversized) {
    result.value = `附件 ${oversized.name} 超过 ${formatBytes(maxAttachmentBytes)}`
    return false
  }

  const nextTotal = files.reduce((total, file) => total + file.size, attachmentsTotalSize.value)
  if (nextTotal > maxTotalAttachmentBytes) {
    result.value = `附件总大小不能超过 ${formatBytes(maxTotalAttachmentBytes)}`
    return false
  }

  const items = files.map((file) => makeAttachmentItem(file, source))
  selectedAttachments.value = [...selectedAttachments.value, ...items]
  return true
}

function handleAttachmentChange(event) {
  result.value = ''
  const files = Array.from(event.target.files || [])
  event.target.value = ''
  addAttachmentFiles(files)
}

function removeAttachment(id) {
  selectedAttachments.value = selectedAttachments.value.filter((item) => item.id !== id)
}

function fileToPayload(item) {
  return new Promise((resolve, reject) => {
    const reader = new FileReader()
    reader.onerror = () => reject(new Error(`读取附件失败：${item.file.name}`))
    reader.onload = () => {
      const dataUrl = String(reader.result || '')
      resolve({
        filename: item.file.name || 'attachment',
        content_type: item.file.type || 'application/octet-stream',
        data_base64: dataUrl.split(',')[1] || '',
      })
    }
    reader.readAsDataURL(item.file)
  })
}

async function attachmentPayloads() {
  if (!selectedAttachments.value.length) return undefined
  return Promise.all(selectedAttachments.value.map(fileToPayload))
}

async function loadRemoteAttachments(attachments, source, successMessage) {
  if (!attachments.length) return

  try {
    const files = await Promise.all(
      attachments.map(async (attachment) => {
        const blob = await api.downloadAttachment(attachment.id)
        return new File([blob], attachment.filename || 'attachment', {
          type: attachment.content_type || blob.type || 'application/octet-stream',
          lastModified: Date.now(),
        })
      }),
    )
    if (addAttachmentFiles(files, source)) {
      result.value = successMessage(files.length)
    }
  } catch (e) {
    result.value = '加载附件失败：' + (e.message || '未知错误')
  }
}

async function loadForwardedAttachments(attachments) {
  await loadRemoteAttachments(
    attachments,
    'forwarded',
    (count) => `已加入原邮件的 ${count} 个附件`,
  )
}

async function loadDraftAttachments(attachments) {
  selectedAttachments.value = []
  await loadRemoteAttachments(
    attachments,
    'draft',
    (count) => `已恢复草稿附件 ${count} 个`,
  )
}

function draftPayload() {
  return {
    to: to.value,
    cc: cc.value,
    bcc: bcc.value,
    subject: subject.value,
    body: body.value,
    showCc: showCc.value,
    showBcc: showBcc.value,
    sendAsHtml: sendAsHtml.value,
  }
}

function sendPayload() {
  const recipients = splitRecipients(to.value)
  const ccList = splitRecipients(cc.value)
  const bccList = splitRecipients(bcc.value)
  return {
    recipients,
    ccList,
    bccList,
    invalid: [
      ...validateEmails(recipients),
      ...validateEmails(ccList),
      ...validateEmails(bccList),
    ],
  }
}

function serverDraftPayload() {
  const payload = sendPayload()
  return {
    id: draftId.value || undefined,
    to: payload.recipients,
    cc: payload.ccList.length ? payload.ccList : undefined,
    bcc: payload.bccList.length ? payload.bccList : undefined,
    subject: subject.value,
    body_text: sendAsHtml.value ? undefined : body.value,
    body_html: sendAsHtml.value ? body.value : undefined,
    send_as_html: sendAsHtml.value,
  }
}

function hasDraftContent(payload = draftPayload()) {
  return Boolean(
    payload.to
      || payload.cc
      || payload.bcc
      || payload.subject
      || payload.body
      || selectedAttachments.value.length,
  )
}

function saveLocalDraft() {
  if (draftId.value) return
  const payload = draftPayload()
  if (hasDraftContent(payload)) {
    localStorage.setItem(draftKey, JSON.stringify(payload))
  } else {
    localStorage.removeItem(draftKey)
  }
}

function restoreLocalDraft() {
  const raw = localStorage.getItem(draftKey)
  if (!raw) return
  try {
    const draft = JSON.parse(raw)
    to.value = draft.to || ''
    cc.value = draft.cc || ''
    bcc.value = draft.bcc || ''
    subject.value = draft.subject || ''
    body.value = draft.body || ''
    showCc.value = Boolean(draft.showCc || draft.cc)
    showBcc.value = Boolean(draft.showBcc || draft.bcc)
    sendAsHtml.value = Boolean(draft.sendAsHtml)
    restoredDraft.value = hasDraftContent(draft)
    serverDraftLoaded.value = false
  } catch {
    localStorage.removeItem(draftKey)
  }
}

function clearForm() {
  to.value = ''
  cc.value = ''
  bcc.value = ''
  subject.value = ''
  body.value = ''
  showCc.value = false
  showBcc.value = false
  sendAsHtml.value = false
  restoredDraft.value = false
  serverDraftLoaded.value = false
  draftId.value = null
  selectedAttachments.value = []
  if (fileInput.value) fileInput.value.value = ''
  localStorage.removeItem(draftKey)
}

async function clearDraft() {
  const currentDraftId = draftId.value
  if (currentDraftId) {
    try {
      await api.deleteDraft(currentDraftId)
    } catch (e) {
      result.value = '清除草稿失败：' + (e.message || '未知错误')
      return
    }
  }

  clearForm()
  await router.replace('/compose')
  result.value = '草稿已清除'
  notifyMailboxCountsChanged()
}

function applyServerDraft(draft) {
  draftId.value = draft.id
  to.value = (draft.to || []).join(', ')
  cc.value = (draft.cc || []).join(', ')
  bcc.value = (draft.bcc || []).join(', ')
  subject.value = draft.subject || ''
  sendAsHtml.value = Boolean(draft.send_as_html)
  body.value = sendAsHtml.value ? draft.body_html || '' : draft.body_text || ''
  showCc.value = Boolean(cc.value)
  showBcc.value = Boolean(bcc.value)
  restoredDraft.value = false
  serverDraftLoaded.value = true
  selectedAttachments.value = []
  localStorage.removeItem(draftKey)
}

async function loadServerDraft(id) {
  result.value = ''
  const numericId = Number(id)
  if (!Number.isFinite(numericId) || numericId <= 0) {
    result.value = '草稿不存在'
    return
  }

  try {
    const data = await api.getDraft(numericId)
    applyServerDraft(data.draft)
    await loadDraftAttachments(data.attachments || [])
  } catch (e) {
    result.value = '加载草稿失败：' + (e.message || '未知错误')
  }
}

async function saveServerDraft() {
  result.value = ''

  if (!hasDraftContent()) {
    result.value = '没有可保存的草稿内容'
    return
  }

  savingDraft.value = true

  try {
    const payload = serverDraftPayload()
    const attachments = await attachmentPayloads()
    if (attachments) payload.attachments = attachments
    const data = await api.saveDraft(payload)
    applyServerDraft(data.draft)
    await loadDraftAttachments(data.attachments || [])
    await router.replace({ path: '/compose', query: { draft: data.draft.id } })
    result.value = '草稿已保存'
    notifyMailboxCountsChanged()
  } catch (e) {
    result.value = '保存草稿失败：' + (e.message || '未知错误')
  } finally {
    savingDraft.value = false
  }
}

async function handleSend() {
  result.value = ''

  const { recipients, ccList, bccList, invalid } = sendPayload()

  if (!recipients.length && !ccList.length && !bccList.length) {
    result.value = '请填写收件人、抄送或密送'
    return
  }
  if (invalid.length) {
    result.value = `邮箱格式不正确：${invalid.join(', ')}`
    return
  }
  if (!subject.value.trim()) {
    result.value = '请填写主题'
    return
  }

  sending.value = true

  try {
    const attachments = selectedAttachments.value.length
      ? await attachmentPayloads()
      : draftId.value
        ? []
        : undefined
    await api.sendEmail({
      to: recipients,
      cc: ccList.length ? ccList : undefined,
      bcc: bccList.length ? bccList : undefined,
      subject: subject.value.trim(),
      body_text: sendAsHtml.value ? undefined : body.value,
      body_html: sendAsHtml.value ? body.value : undefined,
      attachments,
      draft_id: draftId.value || undefined,
    })

    clearForm()
    await router.replace('/compose')
    result.value = '邮件已发送'
    notifyMailboxCountsChanged()
  } catch (e) {
    result.value = '发送失败：' + (e.message || '未知错误')
  } finally {
    sending.value = false
  }
}

async function loadReplyOrForward() {
  if (route.query.draft) {
    await loadServerDraft(route.query.draft)
    return
  }

  const sourceId = route.query.reply || route.query.forward
  if (!sourceId) {
    restoreLocalDraft()
    return
  }

  try {
    const data = await api.getEmail(sourceId)
    const email = data.email
    const date = email.created_at ? new Date(email.created_at).toLocaleString('zh-CN') : ''
    const prefix = route.query.reply ? 'Re:' : 'Fwd:'
    subject.value = email.subject?.startsWith(prefix) ? email.subject : `${prefix} ${email.subject || ''}`

    if (route.query.reply) {
      to.value = extractEmailAddress(email.sender)
      body.value = `\n\n--- 原始邮件 ---\n发件人: ${email.sender}\n时间: ${date}\n主题: ${email.subject || '(无主题)'}\n\n${email.body_text || ''}`
    } else {
      body.value = `\n\n--- 转发邮件 ---\n发件人: ${email.sender}\n收件人: ${email.recipients}\n时间: ${date}\n主题: ${email.subject || '(无主题)'}\n\n${email.body_text || ''}`
      await loadForwardedAttachments(data.attachments || [])
    }
  } catch (e) {
    result.value = '加载原始邮件失败：' + (e.message || '未知错误')
  }
}

watch([to, cc, bcc, subject, body, showCc, showBcc, sendAsHtml], saveLocalDraft)

watch(() => route.query.draft, async (id) => {
  if (id && Number(id) !== draftId.value) {
    await loadServerDraft(id)
  }
})

onMounted(loadReplyOrForward)
</script>

<template>
  <div class="compose">
    <div class="page-header">
      <div>
        <h1>{{ pageTitle }}</h1>
        <p class="subtitle">从 {{ user.email || '当前账号' }} 发送</p>
      </div>
      <div class="header-actions">
        <MiuixButton @click="router.push('/inbox')">返回收件箱</MiuixButton>
        <MiuixButton v-if="hasDraftContent()" @click="clearDraft">清除草稿</MiuixButton>
      </div>
    </div>

    <p v-if="restoredDraft" class="notice">已恢复上次未发送的草稿。</p>
    <p v-if="serverDraftLoaded" class="notice">已加载草稿箱中的草稿。</p>

    <MiuixCard>
      <div class="card-inner compose-card">
        <div class="form-row">
          <label>收件人</label>
          <MiuixInput v-model="to" placeholder="多个收件人用逗号、分号或换行分隔" />
        </div>

        <div class="cc-bcc-toggle">
          <button v-if="!showCc" type="button" class="toggle-link" @click="showCc = true">添加抄送</button>
          <button v-if="!showBcc" type="button" class="toggle-link" @click="showBcc = true">添加密送</button>
        </div>

        <div v-if="showCc" class="form-row">
          <label>抄送</label>
          <MiuixInput v-model="cc" placeholder="多个抄送人用逗号、分号或换行分隔" />
        </div>

        <div v-if="showBcc" class="form-row">
          <label>密送</label>
          <MiuixInput v-model="bcc" placeholder="多个密送人用逗号、分号或换行分隔" />
        </div>

        <div class="form-row">
          <label>主题</label>
          <MiuixInput v-model="subject" placeholder="邮件主题" />
        </div>

        <div class="editor-head">
          <label>内容</label>
          <div class="editor-options">
            <span>{{ bodyCount }} 字</span>
            <label class="html-switch">
              <span>HTML</span>
              <MiuixSwitch v-model="sendAsHtml" />
            </label>
          </div>
        </div>
        <textarea
          v-model="body"
          :placeholder="sendAsHtml ? '<p>输入 HTML 内容...</p>' : '输入邮件内容...'"
          rows="16"
          class="body-textarea"
        ></textarea>

        <div class="form-row attachment-row">
          <label>附件</label>
          <div class="attachment-panel">
            <div class="attachment-toolbar">
              <input
                ref="fileInput"
                type="file"
                multiple
                class="file-input"
                @change="handleAttachmentChange"
              />
              <MiuixButton :disabled="sending || savingDraft" @click="openAttachmentPicker">
                添加附件
              </MiuixButton>
              <span v-if="attachmentsSummary" class="attachment-summary">{{ attachmentsSummary }}</span>
            </div>
            <ul v-if="selectedAttachments.length" class="attachment-list">
              <li v-for="item in selectedAttachments" :key="item.id" class="attachment-item">
                <span class="attachment-name" :title="item.file.name">{{ item.file.name }}</span>
                <span class="attachment-meta">
                  <span v-if="item.source === 'forwarded'" class="attachment-source">转发</span>
                  <span class="attachment-size">{{ formatBytes(item.file.size) }}</span>
                </span>
                <button
                  type="button"
                  class="attachment-remove"
                  :title="`移除 ${item.file.name}`"
                  :disabled="sending || savingDraft"
                  @click="removeAttachment(item.id)"
                >
                  移除
                </button>
              </li>
            </ul>
          </div>
        </div>

        <div class="actions">
          <p v-if="result" class="result" :class="{ success: resultIsSuccess }">
            {{ result }}
          </p>
          <div class="send-actions">
            <MiuixButton :disabled="sending || savingDraft" @click="saveServerDraft">
              {{ savingDraft ? '保存中...' : '保存草稿' }}
            </MiuixButton>
            <MiuixButton type="primary" :disabled="sending || savingDraft" @click="handleSend">
              {{ sending ? '发送中...' : '发送' }}
            </MiuixButton>
          </div>
        </div>
      </div>
    </MiuixCard>
  </div>
</template>

<style scoped>
.compose {
  max-width: 860px;
}

.page-header {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 16px;
  margin-bottom: 22px;
}

.compose h1 {
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
.send-actions {
  display: flex;
  gap: 8px;
  flex-shrink: 0;
}

.notice {
  margin-bottom: 14px;
  padding: 12px 14px;
  border-radius: var(--app-radius);
  background: color-mix(in srgb, var(--app-info) 12%, transparent);
  color: var(--app-info);
}

.card-inner {
  padding: 28px;
}

.compose-card {
  display: flex;
  flex-direction: column;
  gap: 18px;
}

.form-row {
  display: grid;
  grid-template-columns: 88px minmax(0, 1fr);
  gap: 12px;
  align-items: center;
}

.form-row label,
.editor-head > label {
  font-size: 14px;
  font-weight: 650;
  color: var(--m-color-text);
}

.cc-bcc-toggle {
  display: flex;
  gap: 14px;
  margin-left: 100px;
}

.toggle-link {
  border: 0;
  background: transparent;
  color: var(--m-color-primary);
  cursor: pointer;
  padding: 0;
  min-height: auto;
  font-size: 13px;
}

.toggle-link:hover {
  text-decoration: underline;
}

.editor-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}

.editor-options,
.html-switch {
  display: flex;
  align-items: center;
  gap: 10px;
  color: var(--m-color-text-secondary);
  font-size: 13px;
}

.html-switch {
  gap: 6px;
}

.body-textarea {
  width: 100%;
  padding: 14px;
  font-size: 14px;
  font-family: inherit;
  resize: vertical;
  background: var(--m-color-card);
  color: var(--m-color-text);
  transition: border-color 0.2s, box-shadow 0.2s;
}

.body-textarea:focus {
  box-shadow: 0 0 0 3px var(--app-focus);
}

.attachment-row {
  align-items: flex-start;
}

.attachment-panel {
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.attachment-toolbar {
  display: flex;
  align-items: center;
  gap: 12px;
  flex-wrap: wrap;
}

.file-input {
  display: none;
}

.attachment-summary {
  color: var(--m-color-text-secondary);
  font-size: 13px;
}

.attachment-list {
  list-style: none;
  display: flex;
  flex-direction: column;
  gap: 8px;
  margin: 0;
  padding: 0;
}

.attachment-item {
  min-height: 38px;
  display: grid;
  grid-template-columns: minmax(0, 1fr) auto auto;
  align-items: center;
  gap: 10px;
  padding: 8px 10px;
  border: 1px solid var(--m-color-outline-variant);
  border-radius: var(--app-radius);
  background: color-mix(in srgb, var(--m-color-secondary-container) 60%, transparent);
}

.attachment-name {
  min-width: 0;
  overflow: hidden;
  color: var(--m-color-text);
  font-size: 14px;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.attachment-size {
  color: var(--m-color-text-secondary);
  font-size: 13px;
  white-space: nowrap;
}

.attachment-meta {
  display: inline-flex;
  align-items: center;
  justify-content: flex-end;
  gap: 8px;
  min-width: 0;
}

.attachment-source {
  border-radius: 999px;
  padding: 2px 7px;
  background: color-mix(in srgb, var(--m-color-primary) 12%, transparent);
  color: var(--m-color-primary);
  font-size: 12px;
  font-weight: 650;
  white-space: nowrap;
}

.attachment-remove {
  min-height: 28px;
  padding: 0 8px;
  border: 0;
  border-radius: var(--app-radius);
  background: transparent;
  color: var(--app-danger);
  cursor: pointer;
  font-size: 13px;
}

.attachment-remove:hover:not(:disabled) {
  background: color-mix(in srgb, var(--app-danger) 10%, transparent);
}

.attachment-remove:disabled {
  cursor: not-allowed;
  opacity: 0.55;
}

.actions {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
}

.result {
  font-size: 14px;
  color: var(--app-danger);
}

.result.success {
  color: var(--app-success);
}

@media (max-width: 680px) {
  .page-header,
  .actions {
    flex-direction: column;
    align-items: stretch;
  }

  .header-actions,
  .send-actions {
    width: 100%;
  }

  .header-actions > *,
  .send-actions > * {
    flex: 1;
  }

  .form-row {
    grid-template-columns: 1fr;
    gap: 8px;
  }

  .cc-bcc-toggle {
    margin-left: 0;
  }

  .attachment-item {
    grid-template-columns: minmax(0, 1fr) auto;
  }

  .attachment-remove {
    grid-column: 1 / -1;
    justify-self: start;
  }
}
</style>
