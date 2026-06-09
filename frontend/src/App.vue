<script setup>
import { RouterView } from 'vue-router'
</script>

<template>
  <RouterView />
</template>

<style>
* {
  margin: 0;
  padding: 0;
  box-sizing: border-box;
}

/* Alias custom variable names to miuix-vue's actual theme variables */
:root {
  --m-color-bg: var(--m-color-secondary-container);
  --m-color-card: var(--m-color-surface-container);
  --m-color-text: var(--m-color-on-surface);
  --m-color-text-secondary: var(--m-color-on-secondary-container);
  --m-color-border: var(--m-color-outline);
  --m-color-hover: var(--m-color-background);
  --app-radius: 8px;
  --app-shadow: 0 10px 32px rgba(18, 32, 46, 0.08);
  --app-danger: #c2410c;
  --app-success: #15803d;
  --app-warning: #b7791f;
  --app-info: #0f766e;
  --app-focus: rgba(15, 118, 110, 0.24);
}

html {
  min-height: 100%;
}

body {
  font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, 'Helvetica Neue', Arial, sans-serif;
  background: var(--m-color-bg);
  color: var(--m-color-text);
  min-height: 100vh;
  line-height: 1.5;
  text-rendering: optimizeLegibility;
  -webkit-font-smoothing: antialiased;
}

button,
input,
textarea,
select {
  font: inherit;
}

button {
  min-height: 36px;
}

a {
  color: inherit;
}

select,
textarea {
  border: 1px solid var(--m-color-border, rgba(0, 0, 0, 0.16));
  border-radius: var(--app-radius);
}

select:focus,
textarea:focus {
  outline: 3px solid var(--app-focus);
  border-color: var(--m-color-primary);
}

::selection {
  background: var(--app-focus);
}

::-webkit-scrollbar {
  width: 10px;
  height: 10px;
}

::-webkit-scrollbar-thumb {
  background: color-mix(in srgb, var(--m-color-text-secondary) 34%, transparent);
  border: 3px solid transparent;
  border-radius: 999px;
  background-clip: padding-box;
}

#app {
  min-height: 100vh;
}
</style>
