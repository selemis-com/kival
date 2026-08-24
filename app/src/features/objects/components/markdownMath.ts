export type InlineMathMatch = {
  value: string;
  end: number;
};

export type DisplayMathMatch = {
  value: string;
  nextIndex: number;
};

function hasUnescapedDollar(value: string) {
  for (let index = 0; index < value.length; index += 1) {
    if (value[index] !== "$") {
      continue;
    }

    let escapes = 0;
    let escapeIndex = index - 1;

    while (escapeIndex >= 0 && value[escapeIndex] === "\\") {
      escapes += 1;
      escapeIndex -= 1;
    }

    if (escapes % 2 === 0) {
      return true;
    }
  }

  return false;
}

function isLikelyMath(value: string) {
  if (!value || hasUnescapedDollar(value)) {
    return false;
  }

  if (/^\d/.test(value) && /\s/.test(value) && !/[=+\-*/^_{}\\<>]/.test(value)) {
    return false;
  }

  return true;
}

export function readInlineMath(source: string, start: number): InlineMathMatch | null {
  if (
    source[start] !== "$" ||
    source[start - 1] === "$" ||
    source[start + 1] === "$" ||
    /\s/.test(source[start + 1] ?? "")
  ) {
    return null;
  }

  let index = start + 1;

  while (index < source.length) {
    const end = source.indexOf("$", index);

    if (end === -1) {
      return null;
    }

    let escapes = 0;
    let escapeIndex = end - 1;

    while (escapeIndex >= 0 && source[escapeIndex] === "\\") {
      escapes += 1;
      escapeIndex -= 1;
    }

    const previous = source[end - 1] ?? "";
    const next = source[end + 1] ?? "";

    if (escapes % 2 === 0 && !/\s/.test(previous) && next !== "$" && !/\d/.test(next)) {
      const value = source.slice(start + 1, end);
      return isLikelyMath(value) ? { value, end: end + 1 } : null;
    }

    index = end + 1;
  }

  return null;
}

export function readDisplayMath(lines: string[], start: number): DisplayMathMatch | null {
  const singleLine = /^\s*\$\$(.+?)\$\$\s*$/.exec(lines[start] ?? "");

  if (singleLine) {
    const value = singleLine[1].trim();
    return value ? { value, nextIndex: start + 1 } : null;
  }

  if (!/^\s*\$\$\s*$/.test(lines[start] ?? "")) {
    return null;
  }

  const math: string[] = [];
  let closingIndex = start + 1;

  while (closingIndex < lines.length && !/^\s*\$\$\s*$/.test(lines[closingIndex])) {
    math.push(lines[closingIndex]);
    closingIndex += 1;
  }

  if (closingIndex >= lines.length || !math.some((line) => line.trim())) {
    return null;
  }

  return { value: math.join("\n"), nextIndex: closingIndex + 1 };
}
