import { computed, ref, type Ref } from "vue";
import type { RegisteredLibraryExtension } from "../../plugins/sdk";
import type { FileBrowserEntry } from "../../types/repository";

type SelectionMode = "replace" | "toggle" | "range";
type BoxSelectionMode = "replace" | "append";
type EntryDragIntent = {
  entryPath: string;
  pointerId: number;
  startX: number;
  startY: number;
};
type BoxSelectionState = {
  pointerId: number;
  startX: number;
  startY: number;
  currentX: number;
  currentY: number;
  additive: boolean;
  didDrag: boolean;
};

type FileBrowserPanelViewModelOptions = {
  filesListRef: Ref<HTMLElement | null>;
  props: {
    canDragEntries: boolean;
    dropTargetPath: string | null;
    libraryExtensions: RegisteredLibraryExtension[];
    selectedEntries: FileBrowserEntry[];
    selectedFilePaths: string[];
  };
  emit: {
    (event: "entryDragEnd", value: PointerEvent | null): void;
    (event: "entryDragMove", value: PointerEvent): void;
    (event: "entryDragStart", entry: FileBrowserEntry, value: PointerEvent): void;
    (event: "openDirectory", path: string): void;
    (event: "previewFile", entry: FileBrowserEntry): void;
    (event: "selectEntries", paths: string[], mode: BoxSelectionMode): void;
    (event: "selectEntry", entry: FileBrowserEntry, mode: SelectionMode): void;
  };
};

const dragStartThreshold = 7;

export function useFileBrowserPanelViewModel(options: FileBrowserPanelViewModelOptions) {
  let entryDragIntent: EntryDragIntent | null = null;
  let activeDragPointerId: number | null = null;
  let suppressClickPath: string | null = null;
  const boxSelection = ref<BoxSelectionState | null>(null);

  const selectedPathSet = computed(() => new Set(options.props.selectedFilePaths));
  const dropTargetPathSet = computed(() => (
    options.props.dropTargetPath ? new Set([options.props.dropTargetPath]) : new Set<string>()
  ));
  const multiSelectionSummary = computed(() => {
    const directoryCount = options.props.selectedEntries.filter((entry) => entry.kind === "directory").length;
    const fileCount = options.props.selectedEntries.length - directoryCount;
    return [
      directoryCount ? `${directoryCount} 个文件夹` : "",
      fileCount ? `${fileCount} 个文件` : "",
    ].filter(Boolean).join(" · ");
  });
  const selectionBoxStyle = computed(() => {
    const selection = boxSelection.value;
    const container = options.filesListRef.value;
    if (!selection || !selection.didDrag || !container) return null;

    const rect = container.getBoundingClientRect();
    const left = Math.max(0, Math.min(selection.startX, selection.currentX) - rect.left + container.scrollLeft);
    const top = Math.max(0, Math.min(selection.startY, selection.currentY) - rect.top + container.scrollTop);
    const width = Math.abs(selection.currentX - selection.startX);
    const height = Math.abs(selection.currentY - selection.startY);
    return {
      left: `${left}px`,
      top: `${top}px`,
      width: `${width}px`,
      height: `${height}px`,
    };
  });

  function librarySummary(entry: FileBrowserEntry) {
    for (const extension of options.props.libraryExtensions) {
      const summary = extension.fileSummary?.(entry);
      if (summary?.inline || summary?.rows?.length) return summary;
    }
    return null;
  }

  function selectionModeFromEvent(event: MouseEvent): SelectionMode {
    if (event.shiftKey) return "range";
    if (event.ctrlKey || event.metaKey) return "toggle";
    return "replace";
  }

  function handleEntryClick(entry: FileBrowserEntry, event: MouseEvent) {
    if (suppressClickPath === entry.path) {
      suppressClickPath = null;
      return;
    }
    options.emit("selectEntry", entry, selectionModeFromEvent(event));
  }

  function handleEntryDoubleClick(entry: FileBrowserEntry) {
    if (entry.kind === "directory") {
      options.emit("openDirectory", entry.path);
      return;
    }
    options.emit("previewFile", entry);
  }

  function handleListPointerDown(event: PointerEvent) {
    if (event.button !== 0) return;
    if ((event.target as HTMLElement | null)?.closest(".files-list__item")) return;

    boxSelection.value = {
      pointerId: event.pointerId,
      startX: event.clientX,
      startY: event.clientY,
      currentX: event.clientX,
      currentY: event.clientY,
      additive: event.ctrlKey || event.metaKey,
      didDrag: false,
    };
    options.filesListRef.value?.setPointerCapture?.(event.pointerId);
  }

  function collectBoxSelectionPaths(selection: BoxSelectionState) {
    const container = options.filesListRef.value;
    if (!container) return [];

    const left = Math.min(selection.startX, selection.currentX);
    const right = Math.max(selection.startX, selection.currentX);
    const top = Math.min(selection.startY, selection.currentY);
    const bottom = Math.max(selection.startY, selection.currentY);

    return Array.from(container.querySelectorAll<HTMLElement>(".files-list__item[data-entry-path]"))
      .filter((item) => {
        const rect = item.getBoundingClientRect();
        return rect.right >= left && rect.left <= right && rect.bottom >= top && rect.top <= bottom;
      })
      .map((item) => item.dataset.entryPath ?? "")
      .filter(Boolean);
  }

  function updateBoxSelection(event: PointerEvent) {
    const selection = boxSelection.value;
    if (!selection || selection.pointerId !== event.pointerId) return;

    selection.currentX = event.clientX;
    selection.currentY = event.clientY;
    selection.didDrag ||= Math.abs(selection.currentX - selection.startX) > 3 || Math.abs(selection.currentY - selection.startY) > 3;
    boxSelection.value = { ...selection };

    if (!selection.didDrag) return;
    const paths = collectBoxSelectionPaths(selection);
    options.emit("selectEntries", paths, selection.additive ? "append" : "replace");
  }

  function clearBoxSelection(event: PointerEvent) {
    const selection = boxSelection.value;
    if (!selection || selection.pointerId !== event.pointerId) return;

    if (!selection.didDrag && !selection.additive) {
      options.emit("selectEntries", [], "replace");
    }
    options.filesListRef.value?.releasePointerCapture?.(event.pointerId);
    boxSelection.value = null;
  }

  function releaseEntryPointer(event: PointerEvent) {
    const target = event.currentTarget as HTMLElement | null;
    if (target?.hasPointerCapture?.(event.pointerId)) {
      target.releasePointerCapture(event.pointerId);
    }
  }

  function handleEntryPointerDown(entry: FileBrowserEntry, event: PointerEvent) {
    if (!options.props.canDragEntries || event.button !== 0) return;
    entryDragIntent = {
      entryPath: entry.path,
      pointerId: event.pointerId,
      startX: event.clientX,
      startY: event.clientY,
    };
    activeDragPointerId = null;
    (event.currentTarget as HTMLElement | null)?.setPointerCapture?.(event.pointerId);
  }

  function handleEntryPointerMove(entry: FileBrowserEntry, event: PointerEvent) {
    if (activeDragPointerId === event.pointerId) {
      options.emit("entryDragMove", event);
      return;
    }

    const intent = entryDragIntent;
    if (!intent || intent.pointerId !== event.pointerId || intent.entryPath !== entry.path) return;
    if ((event.buttons & 1) !== 1) {
      entryDragIntent = null;
      return;
    }

    const distance = Math.hypot(event.clientX - intent.startX, event.clientY - intent.startY);
    if (distance < dragStartThreshold) return;

    suppressClickPath = entry.path;
    activeDragPointerId = event.pointerId;
    entryDragIntent = null;
    options.emit("entryDragStart", entry, event);
    options.emit("entryDragMove", event);
  }

  function clearEntryDragIntent(event: PointerEvent) {
    const shouldEmitDragEnd = activeDragPointerId === event.pointerId;
    if (entryDragIntent?.pointerId === event.pointerId) {
      entryDragIntent = null;
    }
    if (shouldEmitDragEnd) {
      activeDragPointerId = null;
      options.emit("entryDragEnd", event);
    }
    releaseEntryPointer(event);
  }

  function cancelEntryDragIntent(event: PointerEvent) {
    const shouldEmitDragEnd = activeDragPointerId === event.pointerId;
    if (entryDragIntent?.pointerId === event.pointerId) {
      entryDragIntent = null;
    }
    if (shouldEmitDragEnd) {
      activeDragPointerId = null;
      options.emit("entryDragEnd", null);
    }
    releaseEntryPointer(event);
  }

  return {
    boxSelection,
    dropTargetPathSet,
    multiSelectionSummary,
    selectedPathSet,
    selectionBoxStyle,
    cancelEntryDragIntent,
    clearBoxSelection,
    clearEntryDragIntent,
    handleEntryClick,
    handleEntryDoubleClick,
    handleEntryPointerDown,
    handleEntryPointerMove,
    handleListPointerDown,
    librarySummary,
    updateBoxSelection,
  };
}

export type { BoxSelectionMode, SelectionMode };
