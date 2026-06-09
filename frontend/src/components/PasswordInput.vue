<script setup>
import { computed, ref } from 'vue'
import { MiuixIcon, MiuixIconButton } from 'miuix-vue'
import { Hide, Show } from 'miuix-vue/icons'

const props = defineProps({
  modelValue: {
    type: String,
    default: '',
  },
  placeholder: {
    type: String,
    default: '',
  },
  disabled: {
    type: Boolean,
    default: false,
  },
  readonly: {
    type: Boolean,
    default: false,
  },
  autocomplete: {
    type: String,
    default: 'current-password',
  },
})

const emit = defineEmits(['update:modelValue', 'keyupEnter'])
const visible = ref(false)

const inputType = computed(() => (visible.value ? 'text' : 'password'))
const maskedValue = computed(() => '*'.repeat(props.modelValue.length))
const shouldMask = computed(() => !visible.value && props.modelValue.length > 0)
const toggleLabel = computed(() => (visible.value ? '隐藏密码' : '显示密码'))
const toggleIcon = computed(() => (visible.value ? Hide : Show))

function handleInput(event) {
  emit('update:modelValue', event.target.value)
}

function toggleVisible() {
  visible.value = !visible.value
}
</script>

<template>
  <div class="password-input" :class="{ 'password-input--disabled': disabled }">
    <span class="password-input__content">
      <input
        class="password-input__field"
        :class="{ 'password-input__field--masked': shouldMask }"
        :type="inputType"
        :value="modelValue"
        :placeholder="placeholder"
        :disabled="disabled"
        :readonly="readonly"
        :autocomplete="autocomplete"
        autocapitalize="none"
        spellcheck="false"
        @input="handleInput"
        @change="handleInput"
        @keyup.enter="emit('keyupEnter')"
      />
      <span v-if="shouldMask" class="password-input__mask">{{ maskedValue }}</span>
    </span>
    <span class="password-input__trailing">
      <MiuixIconButton
        class="password-input__toggle"
        :aria-label="toggleLabel"
        :aria-pressed="visible"
        :title="toggleLabel"
        :disabled="disabled"
        @click.stop="toggleVisible"
      >
        <MiuixIcon :icon="toggleIcon" :size="20" />
      </MiuixIconButton>
    </span>
  </div>
</template>

<style scoped>
.password-input {
  width: 100%;
  min-height: 50px;
  display: flex;
  align-items: stretch;
  border-radius: var(--m-radius-md);
  background: var(--m-color-secondary-container);
  cursor: text;
  position: relative;
}

.password-input:focus-within {
  box-shadow: inset 0 0 0 2px var(--m-color-primary);
}

.password-input--disabled {
  cursor: not-allowed;
}

.password-input__content {
  flex: 1;
  min-width: 0;
  display: flex;
  position: relative;
}

.password-input__field {
  box-sizing: border-box;
  width: 100%;
  min-width: 0;
  color: var(--m-color-on-background);
  caret-color: var(--m-color-primary);
  background: transparent;
  border: 0;
  outline: 0;
  margin: 0;
  padding: 14px 0 14px 16px;
  font-family: inherit;
  font-size: var(--m-text-main-size);
  line-height: 1.2;
}

.password-input__field::placeholder {
  color: var(--m-color-on-secondary-container);
  opacity: 1;
}

.password-input__field:disabled {
  cursor: not-allowed;
}

.password-input__field--masked {
  color: transparent;
  -webkit-text-fill-color: transparent;
}

.password-input__mask {
  pointer-events: none;
  position: absolute;
  left: 16px;
  right: 0;
  top: 50%;
  transform: translateY(-50%);
  overflow: hidden;
  color: var(--m-color-on-background);
  font-family: Consolas, Monaco, monospace;
  font-size: var(--m-text-main-size);
  line-height: 1.2;
  white-space: nowrap;
}

.password-input__trailing {
  flex: none;
  display: flex;
  align-items: center;
  padding-right: 6px;
}

.password-input__toggle.m-icon-button {
  --m-icon-button-min-width: 38px;
  --m-icon-button-min-height: 38px;
  --m-icon-button-radius: var(--app-radius);
  color: var(--m-color-on-secondary-variant);
}
</style>
