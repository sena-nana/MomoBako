import {
  getRegisteredToolPage,
  listRegisteredToolPages,
  type RegisteredToolPage,
} from "./sdk";

export function listToolPages() {
  return listRegisteredToolPages();
}

export function getToolPage(pageId: string | null | undefined) {
  return getRegisteredToolPage(pageId);
}

export type { RegisteredToolPage };
