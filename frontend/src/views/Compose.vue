<script setup>
import { ref, onMounted } from 'vue'
import { useRoute } from 'vue-router'
import { MiuixButton, MiuixInput, MiuixCard } from 'miuix-vue'
import { api } from '../api'

const route = useRoute()

const to = ref('')
const cc = ref('')
const bcc = ref('')
const subject = ref('')
const body = ref('')
const sending = ref(false)
const result = ref('')
const showCc = ref(false)
const showBcc = ref(false)

async function handleSend() {
  if (!to.value || !subject.value) {
    result.value = '请填写收件人和主题'
    return
  }

  sending.value = true
  result.value = ''

  try {
    const recipients = to.value.split(',').map((s) => s.trim()).filter(Boolean)
    const ccList = cc.value ? cc.value.split(',').map((s) => s.trim()).filter(Boolean) : undefined
    const bccList = bcc.value ? bcc.value.split(',').map((s) => s.trim()).filter(Boolean) : undefined

    await api.sendEmail({
      to: recipients,
      cc: ccList,
      bcc: bccList,
      subject: subject.value,
      body_text: body.value,
    })

    result.value = '✅ 邮件已发送'
    to.value = ''
    cc.value = ''
    bcc.value = ''
    subject.value = ''
    body.value = ''
  } catch (e) {
    result.value = '❌ 发送失败：' + (e.message || '未知错误')
  } finally {
    sending.value = false
  }
}

onMounted(async () => {
  // Handle reply
  if (route.query.reply) {
    try {
      const data = await api.getEmail(route.query.reply)
      const email = data.email
      to.value = email.sender
      subject.value = email.subject?.startsWith('Re:') ? email.subject : `Re: ${email.subject || ''}`
      const date = email.created_at ? new Date(email.created_at).toLocaleString('zh-CN') : ''
      body.value = `\n\n--- 原始邮件 ---\n发件人: ${email.sender}\n时间: ${date}\n主题: ${email.subject}\n\n${email.body_text || ''}`
    } catch (e) {
      console.error(e)
    }
  }
})
</script>

<template>
  <div class="compose">
    <h1>写邮件</h1>

    <MiuixCard>
      <div class="card-inner compose-card">
        <div class="form-group">
          <label>收件人</label>
          <MiuixInput v-model="to" placeholder="多个收件人用逗号分隔" />
        </div>

        <div class="cc-bcc-toggle">
          <span v-if="!showCc" class="toggle-link" @click="showCc = true">添加抄送</span>
          <span v-if="!showBcc" class="toggle-link" @click="showBcc = true">添加密送</span>
        </div>

        <div v-if="showCc" class="form-group">
          <label>抄送 (CC)</label>
          <MiuixInput v-model="cc" placeholder="多个抄送人用逗号分隔" />
        </div>

        <div v-if="showBcc" class="form-group">
          <label>密送 (BCC)</label>
          <MiuixInput v-model="bcc" placeholder="多个密送人用逗号分隔" />
        </div>

        <div class="form-group">
          <label>主题</label>
          <MiuixInput v-model="subject" placeholder="邮件主题" />
        </div>

        <div class="form-group">
          <label>内容</label>
          <textarea
            v-model="body"
            placeholder="输入邮件内容..."
            rows="14"
            class="body-textarea"
          ></textarea>
        </div>

        <div class="actions">
          <p v-if="result" class="result" :class="{ success: result.startsWith('✅') }">
            {{ result }}
          </p>
          <MiuixButton type="primary" :disabled="sending" @click="handleSend">
            {{ sending ? '发送中...' : '📤 发送' }}
          </MiuixButton>
        </div>
      </div>
    </MiuixCard>
  </div>
</template>

<style scoped>
.compose h1 {
  font-size: 24px;
  font-weight: 600;
  color: var(--m-color-text);
  margin-bottom: 24px;
}

.card-inner {
  padding: 28px;
}

.compose-card {
  max-width: 700px;
}

.form-group {
  margin-bottom: 20px;
}

.form-group label {
  display: block;
  font-size: 14px;
  font-weight: 500;
  color: var(--m-color-text);
  margin-bottom: 8px;
}

.cc-bcc-toggle {
  display: flex;
  gap: 16px;
  margin-bottom: 16px;
  margin-top: -8px;
}

.toggle-link {
  font-size: 13px;
  color: var(--m-color-primary);
  cursor: pointer;
  transition: opacity 0.2s;
}

.toggle-link:hover {
  opacity: 0.8;
}

.body-textarea {
  width: 100%;
  padding: 12px;
  border: 1px solid var(--m-color-border, #ddd);
  border-radius: 8px;
  font-size: 14px;
  font-family: inherit;
  resize: vertical;
  background: var(--m-color-card);
  color: var(--m-color-text);
  transition: border-color 0.2s;
}

.body-textarea:focus {
  outline: none;
  border-color: var(--m-color-primary);
  border-width: 2px;
}

.actions {
  display: flex;
  align-items: center;
  justify-content: space-between;
}

.result {
  font-size: 14px;
  color: #e74c3c;
}

.result.success {
  color: #27ae60;
}
</style>
