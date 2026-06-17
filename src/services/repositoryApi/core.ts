import { Channel, invoke } from "@tauri-apps/api/core";
import { openPath, openUrl, revealItemInDir } from "@tauri-apps/plugin-opener";

type ExternalFileDragEvent = {
  result: "Dropped" | "Cancel";
  cursorPos: {
    x: number;
    y: number;
  };
};

const fallbackFileDragIcon =
  "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAEAAAABACAYAAACqaXHeAAAA1klEQVR4nO2aMQ6DMAxFf6n//7JzSbqVKAWyJE6RMXBpZRmCcL7ugPfr7YkNQKkAqQBYAGABBkKr+S2q681+RjvfbWAA4FO9VxOAQ3VFcvLaKQwwqgbgtI2kQNdEAZpWAJalYBWIVgD62AhgGwhqgVAFYqCx+WK4QClyHb1ZAMKoHDCu5TgBsK4IuM0EnJbx0B5oEGh96F/Nh78qfm83pkQ+ZpA6lAyoo9CRPz39QLLm9YkA8C1yNEioOl4H8NZuTkAmFK5e4Z4A1UkaIBUAC4AFwALg3AE5mFG5Q1UzmgAAAABJRU5ErkJggg==";

export function invokeCommand<T>(command: string, args?: Record<string, unknown>) {
  return invoke<T>(command, args);
}

export function createProgressChannel<T>(onEvent: (event: T) => void) {
  const progress = new Channel<T>();
  progress.onmessage = onEvent;
  return progress;
}

export function invokeWithProgress<TResult, TEvent>(
  command: string,
  args: Record<string, unknown>,
  onEvent: (event: TEvent) => void,
) {
  return invokeCommand<TResult>(command, {
    ...args,
    progress: createProgressChannel(onEvent),
  });
}

export function startExternalFileDrag(paths: string[], icon = fallbackFileDragIcon) {
  return invokeCommand<void>("plugin:drag|start_drag", {
    item: paths,
    image: icon,
    options: { mode: "copy" },
    onEvent: createProgressChannel<ExternalFileDragEvent>(() => undefined),
  });
}

export function openRepositoryPath(path: string) {
  return openPath(path);
}

export function openExternalUrl(url: string) {
  return openUrl(url);
}

export function revealRepositoryPath(path: string) {
  return revealItemInDir(path);
}
