import katex from "katex";
import "katex/dist/katex.min.css";
import type { ReactNode } from "react";
import { getObjectAttachmentContentUrl } from "../../../shared/api";
import { styles } from "../../../shared/styles/index";
import type { ObjectVersionWikilink } from "../../../shared/types";
import { readDisplayMath, readInlineMath } from "./markdownMath";

type Props = {
  body: string;
  workspaceId: string;
  objectId?: string;
  wikilinks?: ObjectVersionWikilink[];
  onOpenObject: (objectId: string) => void;
};

type InlineToken =
  | { kind: "text"; value: string }
  | { kind: "strong"; children: InlineToken[] }
  | { kind: "emphasis"; children: InlineToken[] }
  | { kind: "strikethrough"; children: InlineToken[] }
  | { kind: "code"; value: string }
  | { kind: "math"; value: string }
  | { kind: "link"; label: InlineToken[]; href: string }
  | { kind: "wikilink"; target: string; label: string }
  | { kind: "image"; alt: string; attachmentId: string }
  | { kind: "break" };

type TableAlignment = "left" | "center" | "right" | null;

type ListItem = {
  blocks: Block[];
  task: boolean;
  checked: boolean;
};

type Block =
  | { kind: "paragraph"; lines: string[] }
  | { kind: "heading"; level: 1 | 2 | 3 | 4 | 5 | 6; text: string }
  | { kind: "list"; ordered: boolean; start: number; items: ListItem[] }
  | { kind: "blockquote"; blocks: Block[] }
  | { kind: "code"; language: string | null; value: string }
  | { kind: "math"; value: string }
  | { kind: "table"; headers: string[]; alignments: TableAlignment[]; rows: string[][] }
  | { kind: "rule" };

const KIVAL_OBJECT_URL =
  /^kival:\/\/objects\/([0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})$/i;
const KIVAL_ATTACHMENT_URL =
  /^kival:\/\/attachments\/([0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})$/i;
const ABSOLUTE_URL = /^(?:https?:\/\/|mailto:)/i;
const BARE_URL = /^https?:\/\/[^\s<]+/i;
const EMAIL = /^[A-Z0-9._%+-]+@[A-Z0-9.-]+\.[A-Z]{2,}/i;
function containsUnsafeUrlCharacters(value: string) {
  for (const character of value) {
    const codePoint = character.codePointAt(0);

    if (codePoint !== undefined && (codePoint <= 0x1f || codePoint === 0x7f)) {
      return true;
    }
  }

  return false;
}

type SafeDestination =
  | { kind: "kival-object"; objectId: string }
  | { kind: "kival-attachment"; attachmentId: string }
  | { kind: "external"; href: string; newTab: boolean };

function getSafeLinkDestination(rawHref: string): SafeDestination | null {
  const href = rawHref.trim();

  if (!href || containsUnsafeUrlCharacters(href)) {
    return null;
  }

  const objectId = getObjectId(href);

  if (objectId) {
    return { kind: "kival-object", objectId };
  }

  const attachmentId = getAttachmentId(href);

  if (attachmentId) {
    return { kind: "kival-attachment", attachmentId };
  }

  if (/^https?:\/\//i.test(href)) {
    try {
      const url = new URL(href);

      if (url.protocol !== "http:" && url.protocol !== "https:") {
        return null;
      }

      return { kind: "external", href: url.href, newTab: true };
    } catch {
      return null;
    }
  }

  if (/^mailto:/i.test(href)) {
    const address = href.slice("mailto:".length);

    if (!address || /[\s<>]/.test(address)) {
      return null;
    }

    return { kind: "external", href, newTab: false };
  }

  return null;
}

function getSafeImageDestination(rawSrc: string): SafeDestination | null {
  const src = rawSrc.trim();

  if (!src || containsUnsafeUrlCharacters(src)) {
    return null;
  }

  const attachmentId = getAttachmentId(src);
  return attachmentId ? { kind: "kival-attachment", attachmentId } : null;
}

function getObjectId(href: string) {
  return KIVAL_OBJECT_URL.exec(href)?.[1] ?? null;
}

function getAttachmentId(href: string) {
  return KIVAL_ATTACHMENT_URL.exec(href)?.[1] ?? null;
}

function findClosingDelimiter(source: string, delimiter: string, from: number) {
  let index = from;

  while (index < source.length) {
    const match = source.indexOf(delimiter, index);

    if (match === -1) {
      return -1;
    }

    let escapes = 0;
    let escapeIndex = match - 1;

    while (escapeIndex >= 0 && source[escapeIndex] === "\\") {
      escapes += 1;
      escapeIndex -= 1;
    }

    if (escapes % 2 === 0) {
      return match;
    }

    index = match + delimiter.length;
  }

  return -1;
}

function trimAutolinkPunctuation(value: string) {
  let end = value.length;

  while (end > 0 && /[.,!?;:]$/.test(value.slice(0, end))) {
    end -= 1;
  }

  while (end > 0 && value[end - 1] === ")") {
    const candidate = value.slice(0, end);
    const opens = (candidate.match(/\(/g) ?? []).length;
    const closes = (candidate.match(/\)/g) ?? []).length;

    if (closes <= opens) {
      break;
    }

    end -= 1;
  }

  return [value.slice(0, end), value.slice(end)] as const;
}

function parseInline(source: string): InlineToken[] {
  const tokens: InlineToken[] = [];
  let text = "";
  let index = 0;

  const flushText = () => {
    if (text) {
      tokens.push({ kind: "text", value: text });
      text = "";
    }
  };

  while (index < source.length) {
    if (source[index] === "\\" && index + 1 < source.length) {
      text += source[index + 1];
      index += 2;
      continue;
    }

    if (source[index] === "`") {
      let runLength = 1;

      while (source[index + runLength] === "`") {
        runLength += 1;
      }

      const delimiter = "`".repeat(runLength);
      const end = findClosingDelimiter(source, delimiter, index + runLength);

      if (end !== -1) {
        flushText();
        let value = source.slice(index + runLength, end).replace(/\n/g, " ");

        if (value.startsWith(" ") && value.endsWith(" ") && value.trim()) {
          value = value.slice(1, -1);
        }

        tokens.push({ kind: "code", value });
        index = end + runLength;
        continue;
      }
    }

    const math = readInlineMath(source, index);

    if (math) {
      flushText();
      tokens.push({ kind: "math", value: math.value });
      index = math.end;
      continue;
    }

    if (source.startsWith("[[", index)) {
      const end = source.indexOf("]]", index + 2);

      if (end !== -1) {
        const content = source.slice(index + 2, end);

        if (!content.includes("[[")) {
          const separator = content.indexOf("|");
          const target = (separator === -1 ? content : content.slice(0, separator)).trim();
          const display = separator === -1 ? "" : content.slice(separator + 1).trim();

          if (target) {
            flushText();
            tokens.push({ kind: "wikilink", target, label: display || target });
            index = end + 2;
            continue;
          }
        }
      }
    }

    if (source.startsWith("![", index)) {
      const labelEnd = source.indexOf("](", index + 2);

      if (labelEnd !== -1) {
        const hrefEnd = source.indexOf(")", labelEnd + 2);

        if (hrefEnd !== -1) {
          const raw = source.slice(index, hrefEnd + 1);
          const alt = source.slice(index + 2, labelEnd);
          const destination = getSafeImageDestination(source.slice(labelEnd + 2, hrefEnd));

          if (destination?.kind === "kival-attachment") {
            flushText();
            tokens.push({ kind: "image", alt, attachmentId: destination.attachmentId });
          } else {
            text += raw;
          }

          index = hrefEnd + 1;
          continue;
        }
      }
    }

    if (source[index] === "[") {
      const labelEnd = source.indexOf("](", index + 1);

      if (labelEnd !== -1) {
        const hrefEnd = source.indexOf(")", labelEnd + 2);

        if (hrefEnd !== -1) {
          const raw = source.slice(index, hrefEnd + 1);
          const href = source.slice(labelEnd + 2, hrefEnd).trim();

          if (getSafeLinkDestination(href)) {
            flushText();
            tokens.push({
              kind: "link",
              label: parseInline(source.slice(index + 1, labelEnd)),
              href,
            });
          } else {
            text += raw;
          }

          index = hrefEnd + 1;
          continue;
        }
      }
    }

    if (source[index] === "<") {
      const end = source.indexOf(">", index + 1);

      if (end !== -1) {
        const candidate = source.slice(index + 1, end);
        const href = ABSOLUTE_URL.test(candidate)
          ? candidate
          : EMAIL.test(candidate) && EMAIL.exec(candidate)?.[0] === candidate
            ? `mailto:${candidate}`
            : null;

        if (href) {
          flushText();
          tokens.push({ kind: "link", label: [{ kind: "text", value: candidate }], href });
          index = end + 1;
          continue;
        }
      }
    }

    if (source.startsWith("~~", index)) {
      const end = findClosingDelimiter(source, "~~", index + 2);

      if (end !== -1 && end > index + 2) {
        flushText();
        tokens.push({
          kind: "strikethrough",
          children: parseInline(source.slice(index + 2, end)),
        });
        index = end + 2;
        continue;
      }
    }

    const strongDelimiter = source.startsWith("**", index)
      ? "**"
      : source.startsWith("__", index)
        ? "__"
        : null;

    if (strongDelimiter) {
      const end = findClosingDelimiter(source, strongDelimiter, index + 2);

      if (end !== -1 && end > index + 2) {
        flushText();
        tokens.push({
          kind: "strong",
          children: parseInline(source.slice(index + 2, end)),
        });
        index = end + 2;
        continue;
      }
    }

    const emphasisDelimiter = source[index] === "*" || source[index] === "_" ? source[index] : null;

    if (emphasisDelimiter) {
      const previous = index > 0 ? source[index - 1] : "";
      const next = source[index + 1] ?? "";
      const isIntrawordUnderscore =
        emphasisDelimiter === "_" && /[A-Za-z0-9]/.test(previous) && /[A-Za-z0-9]/.test(next);

      if (!isIntrawordUnderscore) {
        const end = findClosingDelimiter(source, emphasisDelimiter, index + 1);

        if (end !== -1 && end > index + 1) {
          flushText();
          tokens.push({
            kind: "emphasis",
            children: parseInline(source.slice(index + 1, end)),
          });
          index = end + 1;
          continue;
        }
      }
    }

    const remainder = source.slice(index);
    const urlMatch = BARE_URL.exec(remainder);

    if (urlMatch) {
      const [href, trailing] = trimAutolinkPunctuation(urlMatch[0]);
      flushText();
      tokens.push({ kind: "link", label: [{ kind: "text", value: href }], href });
      text += trailing;
      index += urlMatch[0].length;
      continue;
    }

    const emailMatch = EMAIL.exec(remainder);

    if (emailMatch && (index === 0 || /[\s(]/.test(source[index - 1]))) {
      flushText();
      const email = emailMatch[0];
      tokens.push({
        kind: "link",
        label: [{ kind: "text", value: email }],
        href: `mailto:${email}`,
      });
      index += email.length;
      continue;
    }

    text += source[index];
    index += 1;
  }

  flushText();
  return tokens;
}

function splitTableRow(line: string) {
  const trimmed = line.trim().replace(/^\|/, "").replace(/\|$/, "");
  const cells: string[] = [];
  let current = "";
  let escaped = false;

  for (const character of trimmed) {
    if (escaped) {
      current += character;
      escaped = false;
      continue;
    }

    if (character === "\\") {
      current += character;
      escaped = true;
      continue;
    }

    if (character === "|") {
      cells.push(current.trim());
      current = "";
      continue;
    }

    current += character;
  }

  cells.push(current.trim());
  return cells;
}

function parseTableDelimiter(line: string): TableAlignment[] | null {
  const cells = splitTableRow(line);

  if (cells.length === 0 || cells.some((cell) => !/^:?-{3,}:?$/.test(cell.replace(/\s/g, "")))) {
    return null;
  }

  return cells.map((cell) => {
    const value = cell.replace(/\s/g, "");
    const left = value.startsWith(":");
    const right = value.endsWith(":");

    if (left && right) return "center";
    if (right) return "right";
    if (left) return "left";
    return null;
  });
}

function leadingIndent(line: string) {
  const match = /^(\s*)/.exec(line)?.[1] ?? "";
  return match.replace(/\t/g, "    ").length;
}

function isListMarker(line: string) {
  return /^(\s*)([-+*]|\d+[.)])\s+/.exec(line);
}

function parseList(lines: string[], startIndex: number): { block: Block; nextIndex: number } {
  const first = isListMarker(lines[startIndex]);

  if (!first) {
    throw new Error("parseList called without a list marker");
  }

  const baseIndent = first[1].replace(/\t/g, "    ").length;
  const ordered = /^\d/.test(first[2]);
  const start = ordered ? Number.parseInt(first[2], 10) || 1 : 1;
  const items: ListItem[] = [];
  let index = startIndex;

  while (index < lines.length) {
    const marker = isListMarker(lines[index]);

    if (!marker) {
      break;
    }

    const indent = marker[1].replace(/\t/g, "    ").length;
    const markerOrdered = /^\d/.test(marker[2]);

    if (indent !== baseIndent || markerOrdered !== ordered) {
      break;
    }

    const contentStart = marker[0].length;
    const itemLines = [lines[index].slice(contentStart)];
    index += 1;

    while (index < lines.length) {
      const nextLine = lines[index];
      const nextMarker = isListMarker(nextLine);
      const nextIndent = leadingIndent(nextLine);

      if (nextMarker) {
        const markerIndent = nextMarker[1].replace(/\t/g, "    ").length;
        const nextOrdered = /^\d/.test(nextMarker[2]);

        if (markerIndent === baseIndent && nextOrdered === ordered) {
          break;
        }

        if (markerIndent < baseIndent) {
          break;
        }
      } else if (nextLine.trim() && nextIndent <= baseIndent) {
        break;
      }

      const stripCount = Math.min(nextLine.length, baseIndent + 2);
      itemLines.push(nextLine.trim() ? nextLine.slice(stripCount) : "");
      index += 1;
    }

    const taskMatch = /^\[([ xX])\]\s+/.exec(itemLines[0]);

    if (taskMatch) {
      itemLines[0] = itemLines[0].slice(taskMatch[0].length);
    }

    items.push({
      blocks: parseBlocks(itemLines),
      task: Boolean(taskMatch),
      checked: taskMatch?.[1].toLowerCase() === "x",
    });
  }

  return { block: { kind: "list", ordered, start, items }, nextIndex: index };
}

function startsBlock(line: string, nextLine?: string) {
  return (
    /^\s*(```+|~~~+)/.test(line) ||
    /^\s*\$\$\s*$/.test(line) ||
    /^\s*\$\$.+\$\$\s*$/.test(line) ||
    /^ {4}\S/.test(line) ||
    /^(#{1,6})\s+/.test(line) ||
    /^\s*([-*_])(?:\s*\1){2,}\s*$/.test(line) ||
    Boolean(isListMarker(line)) ||
    /^\s*>\s?/.test(line) ||
    Boolean(nextLine && line.includes("|") && parseTableDelimiter(nextLine))
  );
}

function parseBlocks(source: string | string[]): Block[] {
  const lines = Array.isArray(source) ? source : source.replace(/\r\n?/g, "\n").split("\n");
  const blocks: Block[] = [];
  let index = 0;

  while (index < lines.length) {
    const line = lines[index];

    if (!line.trim()) {
      index += 1;
      continue;
    }

    const displayMath = readDisplayMath(lines, index);

    if (displayMath) {
      blocks.push({ kind: "math", value: displayMath.value });
      index = displayMath.nextIndex;
      continue;
    }

    const fence = /^\s*(`{3,}|~{3,})(.*)$/.exec(line);

    if (fence) {
      const delimiter = fence[1][0];
      const minimumLength = fence[1].length;
      const language = fence[2].trim().split(/\s+/)[0] || null;
      const code: string[] = [];
      index += 1;

      while (
        index < lines.length &&
        !new RegExp(`^\\s*${delimiter}{${minimumLength},}\\s*$`).test(lines[index])
      ) {
        code.push(lines[index]);
        index += 1;
      }

      if (index < lines.length) {
        index += 1;
      }

      blocks.push({ kind: "code", language, value: code.join("\n") });
      continue;
    }

    if (/^ {4}\S/.test(line)) {
      const code: string[] = [];

      while (index < lines.length && (/^ {4}/.test(lines[index]) || !lines[index].trim())) {
        code.push(lines[index].startsWith("    ") ? lines[index].slice(4) : "");
        index += 1;
      }

      blocks.push({ kind: "code", language: null, value: code.join("\n").replace(/\n+$/, "") });
      continue;
    }

    const heading = /^(#{1,6})\s+(.+?)\s*#*\s*$/.exec(line);

    if (heading) {
      blocks.push({
        kind: "heading",
        level: heading[1].length as 1 | 2 | 3 | 4 | 5 | 6,
        text: heading[2],
      });
      index += 1;
      continue;
    }

    if (/^\s*([-*_])(?:\s*\1){2,}\s*$/.test(line)) {
      blocks.push({ kind: "rule" });
      index += 1;
      continue;
    }

    if (isListMarker(line)) {
      const parsed = parseList(lines, index);
      blocks.push(parsed.block);
      index = parsed.nextIndex;
      continue;
    }

    if (/^\s*>\s?/.test(line)) {
      const quote: string[] = [];

      while (index < lines.length && (/^\s*>\s?/.test(lines[index]) || !lines[index].trim())) {
        quote.push(lines[index].replace(/^\s*>\s?/, ""));
        index += 1;
      }

      blocks.push({ kind: "blockquote", blocks: parseBlocks(quote) });
      continue;
    }

    if (index + 1 < lines.length && line.includes("|")) {
      const alignments = parseTableDelimiter(lines[index + 1]);

      if (alignments) {
        const headers = splitTableRow(line);
        const rows: string[][] = [];
        index += 2;

        while (index < lines.length && lines[index].trim() && lines[index].includes("|")) {
          rows.push(splitTableRow(lines[index]));
          index += 1;
        }

        const columnCount = Math.max(headers.length, alignments.length);
        blocks.push({
          kind: "table",
          headers: Array.from({ length: columnCount }, (_, column) => headers[column] ?? ""),
          alignments: Array.from(
            { length: columnCount },
            (_, column) => alignments[column] ?? null,
          ),
          rows: rows.map((row) =>
            Array.from({ length: columnCount }, (_, column) => row[column] ?? ""),
          ),
        });
        continue;
      }
    }

    const paragraph: string[] = [];

    while (index < lines.length && lines[index].trim()) {
      const current = lines[index];

      if (paragraph.length > 0 && startsBlock(current, lines[index + 1])) {
        break;
      }

      paragraph.push(current);
      index += 1;
    }

    if (paragraph.length > 0) {
      blocks.push({ kind: "paragraph", lines: paragraph });
      continue;
    }

    index += 1;
  }

  return blocks;
}

function stableValueKey(value: unknown) {
  const serialized = JSON.stringify(value);
  let hash = 2166136261;

  for (let index = 0; index < serialized.length; index += 1) {
    hash ^= serialized.charCodeAt(index);
    hash = Math.imul(hash, 16777619);
  }

  return (hash >>> 0).toString(36);
}

function withStableKeys<T>(items: T[]) {
  const occurrences = new Map<string, number>();

  return items.map((item) => {
    const baseKey = stableValueKey(item);
    const occurrence = occurrences.get(baseKey) ?? 0;
    occurrences.set(baseKey, occurrence + 1);

    return { item, key: occurrence === 0 ? baseKey : `${baseKey}-${occurrence}` };
  });
}

function AttachmentReference({
  workspaceId,
  objectId,
  attachmentId,
  label,
  image = false,
}: {
  workspaceId: string;
  objectId?: string;
  attachmentId: string;
  label: ReactNode;
  image?: boolean;
}) {
  const title = `kival://attachments/${attachmentId}`;

  if (!objectId) {
    return (
      <span
        style={styles.markdownAttachmentReference}
        title={title}
        data-attachment-id={attachmentId}
      >
        <span aria-hidden="true">{image ? "▧" : "↳"}</span>
        <span>{label}</span>
      </span>
    );
  }

  const href = getObjectAttachmentContentUrl(workspaceId, objectId, attachmentId);

  if (image) {
    return (
      <img
        src={href}
        alt={typeof label === "string" ? label : ""}
        title={title}
        data-attachment-id={attachmentId}
        loading="lazy"
        decoding="async"
        style={styles.markdownAttachmentImage}
      />
    );
  }

  return (
    <a
      href={href}
      style={styles.markdownAttachmentReference}
      title={title}
      data-attachment-id={attachmentId}
    >
      <span aria-hidden="true">↳</span>
      <span>{label}</span>
    </a>
  );
}

function MathExpression({ value, display }: { value: string; display: boolean }) {
  try {
    const html = katex.renderToString(value, {
      displayMode: display,
      maxExpand: 1000,
      maxSize: 20,
      output: "htmlAndMathml",
      strict: "error",
      throwOnError: true,
      trust: false,
    });

    // biome-ignore-start lint/security/noDangerouslySetInnerHtml: KaTeX escapes input; trust is disabled.
    const expression = (
      <span
        data-math-display={display || undefined}
        style={display ? styles.markdownMathDisplay : styles.markdownMathInline}
        dangerouslySetInnerHTML={{ __html: html }}
      />
    );
    // biome-ignore-end lint/security/noDangerouslySetInnerHtml: KaTeX escapes input; trust is disabled.

    return expression;
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    const source = display ? `$$\n${value}\n$$` : `$${value}$`;

    return (
      <code
        data-math-error
        title={message}
        style={display ? styles.markdownMathErrorDisplay : styles.markdownMathErrorInline}
      >
        {source}
      </code>
    );
  }
}

function renderInline(
  tokens: InlineToken[],
  workspaceId: string,
  objectId: string | undefined,
  wikilinks: ObjectVersionWikilink[],
  onOpenObject: (objectId: string) => void,
): ReactNode[] {
  return withStableKeys(tokens).map(({ item: token, key }) => {
    switch (token.kind) {
      case "text":
        return token.value;
      case "strong":
        return (
          <strong key={key}>
            {renderInline(token.children, workspaceId, objectId, wikilinks, onOpenObject)}
          </strong>
        );
      case "emphasis":
        return (
          <em key={key}>
            {renderInline(token.children, workspaceId, objectId, wikilinks, onOpenObject)}
          </em>
        );
      case "strikethrough":
        return (
          <del key={key}>
            {renderInline(token.children, workspaceId, objectId, wikilinks, onOpenObject)}
          </del>
        );
      case "code":
        return (
          <code key={key} style={styles.markdownCode}>
            {token.value}
          </code>
        );
      case "math":
        return <MathExpression key={key} value={token.value} display={false} />;
      case "break":
        return <br key={key} />;
      case "image":
        return (
          <AttachmentReference
            key={key}
            workspaceId={workspaceId}
            objectId={objectId}
            attachmentId={token.attachmentId}
            label={token.alt || "Image attachment"}
            image
          />
        );
      case "wikilink": {
        const targetObjectId = wikilinks.find(
          (reference) => reference.raw_target === token.target,
        )?.target_object_id;

        if (!targetObjectId) {
          return (
            <span key={key} data-wikilink-target={token.target}>
              {token.label}
            </span>
          );
        }

        return (
          <a
            key={key}
            href={`/w/${workspaceId}/objects/${targetObjectId}`}
            style={styles.markdownLink}
            data-wikilink-target={token.target}
            onClick={(event) => {
              event.preventDefault();
              onOpenObject(targetObjectId);
            }}
          >
            {token.label}
          </a>
        );
      }
      case "link": {
        const destination = getSafeLinkDestination(token.href);

        if (!destination) {
          return renderInline(token.label, workspaceId, objectId, wikilinks, onOpenObject);
        }

        if (destination.kind === "kival-object") {
          return (
            <a
              key={key}
              href={`/w/${workspaceId}/objects/${destination.objectId}`}
              style={styles.markdownLink}
              onClick={(event) => {
                event.preventDefault();
                onOpenObject(destination.objectId);
              }}
            >
              {renderInline(token.label, workspaceId, objectId, wikilinks, onOpenObject)}
            </a>
          );
        }

        if (destination.kind === "kival-attachment") {
          return (
            <AttachmentReference
              key={key}
              workspaceId={workspaceId}
              objectId={objectId}
              attachmentId={destination.attachmentId}
              label={renderInline(token.label, workspaceId, objectId, wikilinks, onOpenObject)}
            />
          );
        }

        return (
          <a
            key={key}
            href={destination.href}
            style={styles.markdownLink}
            target={destination.newTab ? "_blank" : undefined}
            rel={destination.newTab ? "noopener noreferrer" : undefined}
          >
            {renderInline(token.label, workspaceId, objectId, wikilinks, onOpenObject)}
          </a>
        );
      }
      default: {
        const exhaustive: never = token;
        return exhaustive;
      }
    }
  });
}

function renderParagraphLines(
  lines: string[],
  workspaceId: string,
  objectId: string | undefined,
  wikilinks: ObjectVersionWikilink[],
  onOpenObject: (objectId: string) => void,
) {
  const tokens: InlineToken[] = [];

  lines.forEach((line, index) => {
    const hardBreak = /(?: {2,}|\\)$/.test(line);
    const content = hardBreak ? line.replace(/(?: {2,}|\\)$/, "") : line;

    if (index > 0) {
      const previousLine = lines[index - 1];
      const previousHardBreak = /(?: {2,}|\\)$/.test(previousLine);
      tokens.push(previousHardBreak ? { kind: "break" } : { kind: "text", value: " " });
    }

    tokens.push(...parseInline(content));
  });

  return renderInline(tokens, workspaceId, objectId, wikilinks, onOpenObject);
}

function renderBlocks(
  blocks: Block[],
  workspaceId: string,
  objectId: string | undefined,
  wikilinks: ObjectVersionWikilink[],
  onOpenObject: (objectId: string) => void,
): ReactNode[] {
  return withStableKeys(blocks).map(({ item: block, key }) => {
    switch (block.kind) {
      case "heading": {
        const children = renderInline(
          parseInline(block.text),
          workspaceId,
          objectId,
          wikilinks,
          onOpenObject,
        );

        switch (block.level) {
          case 1:
            return (
              <h1 key={key} style={styles.markdownH1}>
                {children}
              </h1>
            );
          case 2:
            return (
              <h2 key={key} style={styles.markdownH2}>
                {children}
              </h2>
            );
          case 3:
            return (
              <h3 key={key} style={styles.markdownH3}>
                {children}
              </h3>
            );
          case 4:
            return (
              <h4 key={key} style={styles.markdownH4}>
                {children}
              </h4>
            );
          case 5:
            return (
              <h5 key={key} style={styles.markdownH5}>
                {children}
              </h5>
            );
          case 6:
            return (
              <h6 key={key} style={styles.markdownH6}>
                {children}
              </h6>
            );
          default: {
            const exhaustive: never = block.level;
            return exhaustive;
          }
        }
      }
      case "paragraph":
        return (
          <p key={key} style={styles.markdownParagraph}>
            {renderParagraphLines(block.lines, workspaceId, objectId, wikilinks, onOpenObject)}
          </p>
        );
      case "list": {
        const Tag = block.ordered ? "ol" : "ul";

        return (
          <Tag
            key={key}
            start={block.ordered && block.start !== 1 ? block.start : undefined}
            style={block.ordered ? styles.markdownOrderedList : styles.markdownList}
          >
            {withStableKeys(block.items).map(({ item, key: itemKey }) => (
              <li
                key={itemKey}
                style={
                  item.task
                    ? { ...styles.markdownListItem, ...styles.markdownTaskListItem }
                    : styles.markdownListItem
                }
              >
                {item.task && (
                  <input
                    type="checkbox"
                    checked={item.checked}
                    readOnly
                    tabIndex={-1}
                    aria-label={item.checked ? "Completed task" : "Incomplete task"}
                    style={styles.markdownTaskCheckbox}
                  />
                )}
                <div style={item.task ? styles.markdownTaskContent : undefined}>
                  {renderBlocks(item.blocks, workspaceId, objectId, wikilinks, onOpenObject)}
                </div>
              </li>
            ))}
          </Tag>
        );
      }
      case "blockquote":
        return (
          <blockquote key={key} style={styles.markdownBlockquote}>
            {renderBlocks(block.blocks, workspaceId, objectId, wikilinks, onOpenObject)}
          </blockquote>
        );
      case "code":
        return (
          <pre key={key} style={styles.markdownPre}>
            <code data-language={block.language ?? undefined}>{block.value}</code>
          </pre>
        );
      case "math":
        return (
          <div key={key} style={styles.markdownMathBlock}>
            <MathExpression value={block.value} display />
          </div>
        );
      case "table":
        return (
          <div key={key} style={styles.markdownTableWrap}>
            <table style={styles.markdownTable}>
              <thead>
                <tr>
                  {withStableKeys(block.headers).map(({ item: header, key: headerKey }, column) => (
                    <th
                      key={headerKey}
                      style={{
                        ...styles.markdownTableHeader,
                        textAlign: block.alignments[column] ?? "left",
                      }}
                    >
                      {renderInline(
                        parseInline(header),
                        workspaceId,
                        objectId,
                        wikilinks,
                        onOpenObject,
                      )}
                    </th>
                  ))}
                </tr>
              </thead>
              <tbody>
                {withStableKeys(block.rows).map(({ item: row, key: rowKey }) => (
                  <tr key={rowKey}>
                    {withStableKeys(row).map(({ item: cell, key: cellKey }, column) => (
                      <td
                        key={cellKey}
                        style={{
                          ...styles.markdownTableCell,
                          textAlign: block.alignments[column] ?? "left",
                        }}
                      >
                        {renderInline(
                          parseInline(cell),
                          workspaceId,
                          objectId,
                          wikilinks,
                          onOpenObject,
                        )}
                      </td>
                    ))}
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        );
      case "rule":
        return <hr key={key} style={styles.markdownRule} />;
      default: {
        const exhaustive: never = block;
        return exhaustive;
      }
    }
  });
}

export function MarkdownBody({ body, workspaceId, objectId, wikilinks = [], onOpenObject }: Props) {
  return (
    <div style={styles.markdownBody}>
      {renderBlocks(parseBlocks(body), workspaceId, objectId, wikilinks, onOpenObject)}
    </div>
  );
}
