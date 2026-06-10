<script setup>
import { computed, onMounted, ref, watch } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { MiuixButton, MiuixCard } from 'miuix-vue'
import { api } from '../api'

const route = useRoute()
const router = useRouter()

const plugins = ref(null)
const loading = ref(true)
const error = ref('')

const pluginName = computed(() => String(route.params.plugin || ''))
const plugin = computed(() =>
  (plugins.value?.loaded || []).find(
    (item) => item.name?.toLowerCase() === pluginName.value.toLowerCase(),
  ),
)
const frameSrc = computed(() => {
  if (!plugin.value?.admin_path) return ''
  const url = new URL(plugin.value.admin_path, window.location.origin)
  url.searchParams.set('host', 'kuria')
  return url.pathname + url.search
})

async function loadPlugins() {
  loading.value = true
  error.value = ''
  try {
    plugins.value = await api.getPlugins()
  } catch (err) {
    error.value = err.message || '加载插件信息失败'
    plugins.value = null
  } finally {
    loading.value = false
  }
}

onMounted(loadPlugins)
watch(() => route.params.plugin, loadPlugins)
</script>

<template>
  <div class="plugin-config">
    <div class="page-header">
      <div>
        <button class="back-button" type="button" @click="router.push('/settings')">设置</button>
        <h1>{{ plugin?.name || pluginName }}</h1>
        <p class="subtitle">{{ plugin?.description || '插件配置' }}</p>
      </div>
      <MiuixButton class="app-secondary-button" :disabled="loading" @click="loadPlugins">
        {{ loading ? '刷新中...' : '刷新' }}
      </MiuixButton>
    </div>

    <div v-if="error" class="notice error">{{ error }}</div>
    <div v-if="loading" class="loading">正在加载插件配置...</div>

    <MiuixCard v-else-if="!plugin">
      <div class="empty-state">插件未加载。</div>
    </MiuixCard>

    <MiuixCard v-else-if="!frameSrc">
      <div class="empty-state">该插件没有注册配置页面。</div>
    </MiuixCard>

    <iframe
      v-else
      class="plugin-frame"
      :src="frameSrc"
      :title="`${plugin.name} 配置`"
    ></iframe>
  </div>
</template>

<style scoped>
.plugin-config {
  max-width: 980px;
}

.page-header {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 16px;
  margin-bottom: 22px;
}

.back-button {
  padding: 0;
  border: 0;
  background: transparent;
  color: var(--m-color-primary);
  cursor: pointer;
  font: inherit;
  font-size: 13px;
  font-weight: 700;
}

.plugin-config h1 {
  margin-top: 4px;
  font-size: 26px;
  font-weight: 700;
  color: var(--m-color-text);
}

.subtitle {
  margin-top: 4px;
  color: var(--m-color-text-secondary);
  font-size: 14px;
}

.notice,
.loading,
.empty-state {
  padding: 14px;
  border-radius: var(--app-radius);
  background: var(--m-color-card);
}

.notice.error {
  color: var(--app-danger);
  border: 1px solid color-mix(in srgb, var(--app-danger) 32%, transparent);
  margin-bottom: 14px;
}

.empty-state {
  color: var(--m-color-text-secondary);
}

.plugin-frame {
  display: block;
  width: 100%;
  min-height: min(780px, calc(100vh - 130px));
  border: 0;
  border-radius: var(--app-radius);
  background: var(--m-color-card);
}

@media (max-width: 620px) {
  .page-header {
    align-items: stretch;
    flex-direction: column;
  }

  .plugin-frame {
    min-height: calc(100vh - 150px);
  }
}
</style>
