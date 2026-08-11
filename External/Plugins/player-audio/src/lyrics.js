/** LRC 歌词路径、解析与时间定位工具。 */

export function siblingLrcPath(path) {
  const extensionIndex = path.lastIndexOf(".");
  return extensionIndex >= 0 ? `${path.slice(0, extensionIndex)}.lrc` : `${path}.lrc`;
}

export function decodeTextBytes(bytes) {
  if (bytes.length >= 3 && bytes[0] === 0xef && bytes[1] === 0xbb && bytes[2] === 0xbf) {
    return new TextDecoder("utf-8").decode(bytes.slice(3));
  }
  return new TextDecoder("utf-8").decode(bytes);
}

export async function readLocalTextFile(sourceUrl) {
  const response = await fetch(sourceUrl);
  if (!response.ok) throw new Error(`failed to read local text file: ${response.status}`);
  return response.text();
}

/** 将多时间标签 LRC 展开为有序歌词行。 */
export function parseLrcLyrics(text) {
  const rawLines = text.replace(/\r\n?/g, "\n")
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean);
  const parsed = [];
  for (const rawLine of rawLines) {
    const timeTags = [...rawLine.matchAll(/\[(\d{1,2}):(\d{1,2})(?:[.:](\d{1,3}))?\]/g)];
    const plainText = rawLine.replace(/\[(\d{1,2}):(\d{1,2})(?:[.:](\d{1,3}))?\]/g, "").trim();
    if (timeTags.length) {
      for (const [index, tag] of timeTags.entries()) {
        parsed.push({
          id: `${tag[0]}-${parsed.length}-${index}`,
          text: plainText || "…",
          timeMs: timestampToMs(tag[1], tag[2], tag[3]),
        });
      }
    } else if (plainText) {
      parsed.push({ id: `plain-${parsed.length}`, text: plainText, timeMs: null });
    }
  }
  return parsed.sort((left, right) => {
    if (left.timeMs == null && right.timeMs == null) return 0;
    if (left.timeMs == null) return 1;
    if (right.timeMs == null) return -1;
    return left.timeMs - right.timeMs;
  });
}

export function findActiveLyricIndex(lines, playbackMs) {
  let index = -1;
  for (let cursor = 0; cursor < lines.length; cursor += 1) {
    if (lines[cursor].timeMs != null && lines[cursor].timeMs <= playbackMs) index = cursor;
  }
  return index;
}

function timestampToMs(minutes, seconds, fraction) {
  const minuteValue = Number.parseInt(minutes, 10);
  const secondValue = Number.parseInt(seconds, 10);
  const fractionValue = fraction ? Number.parseInt(fraction.padEnd(3, "0").slice(0, 3), 10) : 0;
  return (minuteValue * 60000) + (secondValue * 1000) + fractionValue;
}
