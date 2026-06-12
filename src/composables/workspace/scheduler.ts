type IdleWindow = Window & {
  requestIdleCallback?: (callback: IdleRequestCallback, options?: IdleRequestOptions) => number;
  cancelIdleCallback?: (handle: number) => void;
};

export function yieldToUi() {
  return new Promise<void>((resolve) => {
    window.setTimeout(resolve, 0);
  });
}

export function scheduleIdleTask(callback: () => void, timeout = 500) {
  const currentWindow = window as IdleWindow;
  if (currentWindow.requestIdleCallback) {
    const id = currentWindow.requestIdleCallback(() => callback(), { timeout });
    return () => currentWindow.cancelIdleCallback?.(id);
  }

  const id = window.setTimeout(callback, Math.min(timeout, 16));
  return () => window.clearTimeout(id);
}

export async function yieldEvery(index: number, batchSize = 500) {
  if (index > 0 && index % batchSize === 0) {
    await yieldToUi();
  }
}
