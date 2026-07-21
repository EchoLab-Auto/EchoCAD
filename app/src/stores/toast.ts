import { defineStore } from "pinia";
import { ref } from "vue";

export type ToastType = "info" | "success" | "error";

export interface Toast {
  id: number;
  type: ToastType;
  message: string;
}

/**
 * Global toast-notification store. Any component or handler can push a
 * toast via `useToastStore().success(...)` etc. without prop drilling.
 */
export const useToastStore = defineStore("toast", () => {
  const toasts = ref<Toast[]>([]);
  let nextId = 0;

  function show(type: ToastType, message: string, duration = 3000): void {
    const id = ++nextId;
    toasts.value.push({ id, type, message });
    setTimeout(() => {
      toasts.value = toasts.value.filter(t => t.id !== id);
    }, duration);
  }

  function info(message: string, duration?: number): void {
    show("info", message, duration);
  }

  function success(message: string, duration?: number): void {
    show("success", message, duration);
  }

  function error(message: string, duration?: number): void {
    show("error", message, duration);
  }

  return { toasts, show, info, success, error };
});
