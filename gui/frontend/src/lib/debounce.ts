/**
 * 极简防抖器：`delayMs` 内的多次 `schedule` 只触发**最后一次** `fn`。
 *
 * 渲染页的实时预览用它落 300ms 防抖（见 `stores/appStore.ts` 的 `scheduleRender`）。
 */
export interface Debouncer {
  /** 重新计时；到点后执行 `fn`。 */
  schedule(fn: () => void): void;
  /** 取消未触发的调度（`renderNow` 立即渲染前会调用）。 */
  cancel(): void;
  /** 是否有待触发的调度。 */
  readonly pending: boolean;
}

export function createDebouncer(delayMs: number): Debouncer {
  let timer: ReturnType<typeof setTimeout> | null = null;
  return {
    schedule(fn: () => void) {
      if (timer !== null) clearTimeout(timer);
      timer = setTimeout(() => {
        timer = null;
        fn();
      }, delayMs);
    },
    cancel() {
      if (timer !== null) {
        clearTimeout(timer);
        timer = null;
      }
    },
    get pending() {
      return timer !== null;
    },
  };
}
