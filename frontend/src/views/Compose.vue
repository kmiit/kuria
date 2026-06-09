<script setup>
import { ref, onMounted, watch, computed } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { MiuixButton, MiuixInput, MiuixCard, MiuixSwitch } from 'miuix-vue'
import { api } from '../api'

const route = useRoute()
const router = useRouter()
const draftKey = 'kuria-compose-draft'

const to = ref('')
const cc = ref('')
const bcc = ref('')
const subject = ref('')
const body = ref('')
const sending = ref(false)
const result = ref('')
const showCc = ref(false)
const showBcc = ref(false)
const sendAsHtml = ref(false)
const restoredDraft = ref(false)

const user = computed(() => {
  try {
    return JSON.parse(localStorage.getItem('user') || '{}')
  } catch {
    return {}
  }
})

const bodyCount = computed(() => body.value.length)

function splitRecipients(value) {
  return value
    .split(/[,\n;]/)
    .map((s) => s.trim())
    .filter(Boolean)
}

function validateEmails(list) {
  return list.filter((email) => !/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(email))
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

function hasDraftContent(payload = draftPayload()) {
  return Boolean(payload.to || payload.cc || payload.bcc || payload.subject || payload.body)
}

function saveDraft() {
  const payload = draftPayload()
  if (hasDraftContent(payload)) {
    localStorage.setItem(draftKey, JSON.stringify(payload))
  } else {
    localStorage.removeItem(draftKey)
  }
}

function restoreDraft() {
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
  localStorage.removeItem(draftKey)
}

function clearDraft() {
  clearForm()
  result.value = '草稿已清除'
}

async function handleSend() {
  result.value = ''

  const recipients = splitRecipients(to.value)
  const ccList = splitRecipients(cc.value)
  const bccList = splitRecipients(bcc.value)
  const invalid = [
    ...validateEmails(recipients),
    ...validateEmails(ccList),
    ...validateEmails(bccList),
  ]

  if (!recipients.length) {
    result.value = '请填写收件人'
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
    await api.sendEmail({
      to: recipients,
      cc: ccList.length ? ccList : undefined,
      bcc: bccList.length ? bccList : undefined,
      subject: subject.value.trim(),
      body_text: sendAsHtml.value ? undefined : body.value,
      body_html: sendAsHtml.value ? body.value : undefined,
    })

    clearForm()
    result.value = '邮件已发送'
  } catch (e) {
    result.value = '发送失败：' + (e.message || '未知错误')
  } finally {
    sending.value = false
  }
}

async function loadReplyOrForward() {
  const sourceId = route.query.reply || route.query.forward
  if (!sourceId) {
    restoreDraft()
    return
  }

  try {
    const data = await api.getEmail(sourceId)
    const email = data.email
    const date = email.created_at ? new Date(email.created_at).toLocaleString('zh-CN') : ''
    const prefix = route.query.reply ? 'Re:' : 'Fwd:'
    subject.value = email.subject?.startsWith(prefix) ? email.subject : `${prefix} ${email.subject || ''}`

    if (route.query.reply) {
      to.value = email.sender
      body.value = `\n\n--- 原始邮件 ---\n发件人: ${email.sender}\n时间: ${date}\n主题: ${email.subject || '(无主题)'}\n\n${email.body_text || ''}`
    } else {
      body.value = `\n\n--- 转发邮件 ---\n发件人: ${email.sender}\n收件人: ${email.recipients}\n时间: ${date}\n主题: ${email.subject || '(无主题)'}\n\n${email.body_text || ''}`
    }
  } catch (e) {
    result.value = '加载原始邮件失败：' + (e.message || '未知错误')
  }
}

watch([to, cc, bcc, subject, body, showCc, showBcc, sendAsHtml], saveDraft)

onMounted(loadReplyOrForward)
</script>

<template>
  <div class="compose">
    <div class="page-header">
      <div>
        <h1>写邮件</h1>
        <p class="subtitle">从 {{ user.email || '当前账号' }} 发送</p>
      </div>
      <div class="header-actions">
        <MiuixButton @click="router.push('/inbox')">返回收件箱</MiuixButton>
        <MiuixButton v-if="hasDraftContent()" @click="clearDraft">清除草稿</MiuixButton>
      </div>
    </div>

    <p v-if="restoredDraft" class="notice">已恢复上次未发送的草稿。</p>

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

        <div class="actions">
          <p v-if="result" class="result" :class="{ success: result === '邮件已发送' }">
            {{ result }}
          </p>
          <div class="send-actions">
            <MiuixButton :disabled="sending" @click="saveDraft">保存草稿</MiuixButton>
            <MiuixButton type="primary" :disabled="sending" @click="handleSend">
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
}
</style>
