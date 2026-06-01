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
}

body {
  font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, 'Helvetica Neue', Arial, sans-serif;
  background: var(--m-color-bg);
  color: var(--m-color-text);
  min-height: 100vh;
}
</style>
