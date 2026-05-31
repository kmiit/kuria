<script setup>
import { ref } from 'vue'
import { MiuixButton, MiuixInput, MiuixCard } from 'miuix-vue'
import { api } from '../api'

const to = ref('')
const subject = ref('')
const body = ref('')
const sending = ref(false)
const result = ref('')

async function handleSend() {
  if (!to.value || !subject.value) {
    result.value = '请填写收件人和主题'
    return
  }

  sending.value = true
  result.value = ''

  try {
    const recipients = to.value
      .split(',')
      .map((s) => s.trim())
      .filter(Boolean)

    await api.sendEmail({
      to: recipients,
      subject: subject.value,
      body_text: body.value,
    })

    result.value = '✅ 邮件已发送'
    to.value = ''
    subject.value = ''
    body.value = ''
  } catch (e) {
    result.value = '❌ 发送失败：' + (e.message || '未知错误')
  } finally {
    sending.value = false
  }
}
</script>

<template>
  <div class="compose">
    <h1>写邮件</h1>

    <MiuixCard class="compose-card">
      <div class="form-group">
        <label>收件人</label>
        <MiuixInput v-model="to" placeholder="多个收件人用逗号分隔" />
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
          rows="12"
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
    </MiuixCard>
  </div>
</template>

<style scoped>
.compose h1 {
  font-size: 24px;
  color: var(--m-color-text, #1a1a1a);
  margin-bottom: 24px;
}

.compose-card {
  padding: 24px;
  max-width: 700px;
}

.form-group {
  margin-bottom: 20px;
}

.form-group label {
  display: block;
  font-size: 14px;
  font-weight: 500;
  color: var(--m-color-text, #1a1a1a);
  margin-bottom: 8px;
}

.body-textarea {
  width: 100%;
  padding: 12px;
  border: 1px solid var(--m-color-border, #ddd);
  border-radius: 8px;
  font-size: 14px;
  font-family: inherit;
  resize: vertical;
  background: var(--m-color-card, #fff);
  color: var(--m-color-text, #1a1a1a);
  transition: border-color 0.2s;
}

.body-textarea:focus {
  outline: none;
  border-color: var(--m-color-primary, #4a90d9);
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
