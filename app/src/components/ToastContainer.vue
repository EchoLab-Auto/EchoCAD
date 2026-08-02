<template>
  <div class="toast-container">
    <div v-for="toast in toastStore.toasts" :key="toast.id" :class="['toast', 'toast-' + toast.type]">
      {{ toast.message }}
    </div>
  </div>
</template>

<script setup lang="ts">
import { useToastStore } from "@/stores/toast";

const toastStore = useToastStore();
</script>

<style scoped>
.toast-container {
  position: fixed;
  bottom: calc(var(--statusbar-height) + 12px);
  right: 16px;
  z-index: 400;
  display: flex;
  flex-direction: column;
  gap: 6px;
  pointer-events: none;
}
.toast {
  padding: 8px 16px;
  border-radius: var(--radius);
  font-size: 13px;
  color: #fff;
  box-shadow: 0 2px 12px rgba(0, 0, 0, 0.4);
  animation: toast-in 0.25s ease-out;
}
.toast-info {
  background: var(--accent);
}
.toast-success {
  background: var(--success);
}
.toast-error {
  background: var(--danger);
}
@keyframes toast-in {
  from {
    opacity: 0;
    transform: translateY(10px);
  }
  to {
    opacity: 1;
    transform: translateY(0);
  }
}
</style>
