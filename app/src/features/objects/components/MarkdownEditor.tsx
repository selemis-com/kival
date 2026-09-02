import { type ReactNode, useEffect, useRef, useState } from "react";
import { styles } from "../../../shared/styles/index";
import type { ObjectAttachment } from "../../../shared/types";
import { MarkdownBody } from "./MarkdownBody";

type Mode = "edit" | "split" | "preview";

type Props = {
  value: string;
  onChange: (value: string) => void;
  workspaceId: string;
  objectId?: string;
  onOpenObject: (objectId: string) => void;
  onUploadAttachment?: (file: File) => Promise<ObjectAttachment>;
};

type SelectionTransform = (selected: string) => {
  text: string;
  selectionStart?: number;
  selectionEnd?: number;
};

function ToolbarIcon({ children }: { children: ReactNode }) {
  return (
    <svg
      aria-hidden="true"
      width="20"
      height="20"
      style={{ display: "block", flex: "0 0 20px" }}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.8"
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      {children}
    </svg>
  );
}

function BoldIcon() {
  return (
    <ToolbarIcon>
      <path d="M7 5h6a3.5 3.5 0 0 1 0 7H7z" />
      <path d="M7 12h6.5a3.5 3.5 0 0 1 0 7H7z" />
    </ToolbarIcon>
  );
}

function ItalicIcon() {
  return (
    <ToolbarIcon>
      <path d="M10 5h7" />
      <path d="M7 19h7" />
      <path d="m14 5-4 14" />
    </ToolbarIcon>
  );
}

function HeadingIcon({ level }: { level: 1 | 2 | 3 | 4 | 5 | 6 }) {
  const levelPath = {
    1: "M16 11l2.5-2v9",
    2: "M15.5 11a2.5 2.5 0 1 1 4.5 1.5L15.5 18h5",
    3: "M15.5 10a2.5 2.5 0 1 1 2 4.2H16m1.5 0a2.5 2.5 0 1 1-2 3.8",
    4: "M20 18V9l-5 6h6",
    5: "M20 9h-4l-.5 4h2a2.5 2.5 0 1 1-2.2 3.8",
    6: "M20 9h-1a4 4 0 0 0-4 4v2a3 3 0 1 0 3-3h-3",
  }[level];

  return (
    <ToolbarIcon>
      <path d="M4 6v12M11 6v12M4 12h7" />
      <path d={levelPath} />
    </ToolbarIcon>
  );
}

function StrikethroughIcon() {
  return (
    <ToolbarIcon>
      <path d="M17 6c-1.2-1.3-3-2-5.1-2C8.9 4 7 5.4 7 7.5c0 1.6.9 2.5 2.7 3.2l2.3.8" />
      <path d="m12 12.5 2.1.7c2 .7 2.9 1.7 2.9 3.2 0 2.3-2.1 3.8-5.1 3.8-2.3 0-4.3-.8-5.7-2.3" />
      <path d="M4 12h16" strokeWidth="2" />
    </ToolbarIcon>
  );
}

function LinkIcon() {
  return (
    <ToolbarIcon>
      <path d="M9 17H7a5 5 0 0 1 0-10h2" />
      <path d="M15 7h2a5 5 0 0 1 0 10h-2" />
      <path d="M8 12h8" />
    </ToolbarIcon>
  );
}

function PaperclipIcon() {
  return (
    <ToolbarIcon>
      <path d="m21.4 11.6-8.9 8.9a6 6 0 0 1-8.5-8.5l9.6-9.6a4 4 0 0 1 5.7 5.7l-9.6 9.6a2 2 0 0 1-2.8-2.8l8.9-8.9" />
    </ToolbarIcon>
  );
}

function UploadingIcon() {
  return (
    <ToolbarIcon>
      <circle cx="6" cy="12" r="1" fill="currentColor" stroke="none" />
      <circle cx="12" cy="12" r="1" fill="currentColor" stroke="none" />
      <circle cx="18" cy="12" r="1" fill="currentColor" stroke="none" />
    </ToolbarIcon>
  );
}

function LatexIcon() {
  return (
    <ToolbarIcon>
      <path d="m7 5 10 14M17 5 7 19" />
    </ToolbarIcon>
  );
}

function BulletedListIcon() {
  return (
    <ToolbarIcon>
      <circle cx="5" cy="7" r="1" fill="currentColor" stroke="none" />
      <circle cx="5" cy="12" r="1" fill="currentColor" stroke="none" />
      <circle cx="5" cy="17" r="1" fill="currentColor" stroke="none" />
      <path d="M9 7h10" />
      <path d="M9 12h10" />
      <path d="M9 17h10" />
    </ToolbarIcon>
  );
}

function NumberedListIcon() {
  return (
    <ToolbarIcon>
      <path d="M4 6h1.5v4" />
      <path d="M4 10h3" />
      <path d="M4 14.5a1.5 1.5 0 1 1 3 0c0 1-3 2-3 3.5h3" />
      <path d="M10 7h10" />
      <path d="M10 12h10" />
      <path d="M10 17h10" />
    </ToolbarIcon>
  );
}

function TaskListIcon() {
  return (
    <ToolbarIcon>
      <rect x="3.5" y="4.5" width="6" height="6" rx="1" />
      <path d="m5 7.5 1.3 1.3L8.5 6" />
      <path d="M13 7.5h7" />
      <rect x="3.5" y="14" width="6" height="6" rx="1" />
      <path d="M13 17h7" />
    </ToolbarIcon>
  );
}

function BlockquoteIcon() {
  return (
    <ToolbarIcon>
      <path d="M9 5C6 6.2 4.5 8.7 4.5 12v6H10v-6H5" />
      <path d="M19 5c-3 1.2-4.5 3.7-4.5 7v6H20v-6h-5" />
    </ToolbarIcon>
  );
}

function CodeBlockIcon() {
  return (
    <ToolbarIcon>
      <path d="m8 8-4 4 4 4" />
      <path d="m16 8 4 4-4 4" />
      <path d="m14.5 5-5 14" />
    </ToolbarIcon>
  );
}

function FullscreenIcon({ expanded }: { expanded: boolean }) {
  return (
    <ToolbarIcon>
      {expanded ? (
        <>
          <path d="M9 4v5H4" />
          <path d="m4 9 5-5" />
          <path d="M15 20v-5h5" />
          <path d="m20 15-5 5" />
        </>
      ) : (
        <>
          <path d="M9 4H4v5" />
          <path d="m4 4 5 5" />
          <path d="M15 20h5v-5" />
          <path d="m20 20-5-5" />
        </>
      )}
    </ToolbarIcon>
  );
}

function ToolbarButton({
  label,
  title,
  active = false,
  disabled = false,
  onClick,
}: {
  label: ReactNode;
  title: string;
  active?: boolean;
  disabled?: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      className="kival-markdown-tool"
      data-tooltip={title}
      aria-label={title}
      aria-pressed={active || undefined}
      disabled={disabled}
      style={
        active
          ? { ...styles.markdownToolbarButton, ...styles.markdownToolbarButtonActive }
          : styles.markdownToolbarButton
      }
      onMouseDown={(event) => event.preventDefault()}
      onClick={onClick}
    >
      {label}
    </button>
  );
}

export function MarkdownEditor({
  value,
  onChange,
  workspaceId,
  objectId,
  onOpenObject,
  onUploadAttachment,
}: Props) {
  const textareaRef = useRef<HTMLTextAreaElement | null>(null);
  const attachmentInputRef = useRef<HTMLInputElement | null>(null);
  const headingMenuRef = useRef<HTMLDivElement | null>(null);
  const undoStackRef = useRef<string[]>([]);
  const redoStackRef = useRef<string[]>([]);
  const lastValueRef = useRef(value);
  const lastTypingAtRef = useRef(0);
  const lastSelectionRef = useRef({ start: 0, end: 0 });
  const [mode, setMode] = useState<Mode>("split");
  const [isFullscreen, setIsFullscreen] = useState(false);
  const [headingMenuOpen, setHeadingMenuOpen] = useState(false);
  const [attachmentUploading, setAttachmentUploading] = useState(false);
  const [attachmentError, setAttachmentError] = useState<string | null>(null);

  useEffect(() => {
    if (value !== lastValueRef.current) {
      undoStackRef.current = [];
      redoStackRef.current = [];
      lastValueRef.current = value;
      lastTypingAtRef.current = 0;
    }
  }, [value]);

  useEffect(() => {
    if (!headingMenuOpen) {
      return;
    }

    const closeOnOutsidePointer = (event: PointerEvent) => {
      if (event.target instanceof Node && !headingMenuRef.current?.contains(event.target)) {
        setHeadingMenuOpen(false);
      }
    };
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setHeadingMenuOpen(false);
      }
    };

    document.addEventListener("pointerdown", closeOnOutsidePointer);
    document.addEventListener("keydown", closeOnEscape);
    return () => {
      document.removeEventListener("pointerdown", closeOnOutsidePointer);
      document.removeEventListener("keydown", closeOnEscape);
    };
  }, [headingMenuOpen]);

  useEffect(() => {
    if (!isFullscreen) {
      return;
    }

    const previousBodyOverflow = document.body.style.overflow;
    const exitOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setIsFullscreen(false);
      }
    };

    document.body.style.overflow = "hidden";
    document.addEventListener("keydown", exitOnEscape);

    return () => {
      document.body.style.overflow = previousBodyOverflow;
      document.removeEventListener("keydown", exitOnEscape);
    };
  }, [isFullscreen]);

  function commitValue(nextValue: string, recordUndo = true) {
    const currentValue = lastValueRef.current;

    if (nextValue === currentValue) {
      return;
    }

    if (recordUndo) {
      undoStackRef.current.push(currentValue);
      redoStackRef.current = [];
    }

    lastValueRef.current = nextValue;
    onChange(nextValue);
  }

  function commitInputValue(
    nextValue: string,
    inputType: string,
    selectionStart: number,
    selectionEnd: number,
  ) {
    const now = Date.now();
    const isTypingEdit =
      inputType === "insertText" ||
      inputType === "insertCompositionText" ||
      inputType === "deleteContentBackward" ||
      inputType === "deleteContentForward";
    const startsNewUndoStep = !isTypingEdit || now - lastTypingAtRef.current > 500;

    commitValue(nextValue, startsNewUndoStep);
    lastTypingAtRef.current = isTypingEdit ? now : 0;
    lastSelectionRef.current = { start: selectionStart, end: selectionEnd };
  }

  function breakTypingGroup() {
    lastTypingAtRef.current = 0;
  }

  function undo() {
    lastTypingAtRef.current = 0;
    const previousValue = undoStackRef.current.pop();

    if (previousValue === undefined) {
      return;
    }

    redoStackRef.current.push(lastValueRef.current);
    commitValue(previousValue, false);
  }

  function redo() {
    lastTypingAtRef.current = 0;
    const nextValue = redoStackRef.current.pop();

    if (nextValue === undefined) {
      return;
    }

    undoStackRef.current.push(lastValueRef.current);
    commitValue(nextValue, false);
  }

  function updateSelection(transform: SelectionTransform) {
    lastTypingAtRef.current = 0;
    const textarea = textareaRef.current;

    if (!textarea) {
      return;
    }

    const start = textarea.selectionStart;
    const end = textarea.selectionEnd;
    const selected = value.slice(start, end);
    const result = transform(selected);
    const nextValue = `${value.slice(0, start)}${result.text}${value.slice(end)}`;
    const nextStart = start + (result.selectionStart ?? 0);
    const nextEnd = start + (result.selectionEnd ?? result.text.length);

    commitValue(nextValue);

    requestAnimationFrame(() => {
      textarea.focus();
      textarea.setSelectionRange(nextStart, nextEnd);
    });
  }

  function updateSelectedLines(transform: (lines: string[]) => string[]) {
    lastTypingAtRef.current = 0;
    const textarea = textareaRef.current;

    if (!textarea) {
      return;
    }

    const selection =
      document.activeElement === textarea
        ? { start: textarea.selectionStart, end: textarea.selectionEnd }
        : lastSelectionRef.current;
    const selectionStart = selection.start;
    const selectionEnd = selection.end;
    const lineStart = value.lastIndexOf("\n", Math.max(0, selectionStart - 1)) + 1;
    const nextLineBreak = value.indexOf("\n", selectionEnd);
    const lineEnd = nextLineBreak === -1 ? value.length : nextLineBreak;
    const selectedBlock = value.slice(lineStart, lineEnd);
    const transformed = transform(selectedBlock.split("\n")).join("\n");
    const nextValue = `${value.slice(0, lineStart)}${transformed}${value.slice(lineEnd)}`;

    commitValue(nextValue);

    requestAnimationFrame(() => {
      textarea.focus();
      textarea.setSelectionRange(lineStart, lineStart + transformed.length);
    });
  }

  function toggleWrap(prefix: string, suffix = prefix, placeholder = "text") {
    lastTypingAtRef.current = 0;
    const textarea = textareaRef.current;

    if (!textarea) {
      return;
    }

    const start = textarea.selectionStart;
    const end = textarea.selectionEnd;
    const selected = value.slice(start, end);

    if (
      selected.startsWith(prefix) &&
      selected.endsWith(suffix) &&
      selected.length >= prefix.length + suffix.length
    ) {
      const content = selected.slice(prefix.length, selected.length - suffix.length);
      const nextValue = `${value.slice(0, start)}${content}${value.slice(end)}`;

      commitValue(nextValue);
      requestAnimationFrame(() => {
        textarea.focus();
        textarea.setSelectionRange(start, start + content.length);
      });
      return;
    }

    const hasSurroundingMarkers =
      start >= prefix.length &&
      value.slice(start - prefix.length, start) === prefix &&
      value.slice(end, end + suffix.length) === suffix;

    if (hasSurroundingMarkers) {
      const nextValue = `${value.slice(0, start - prefix.length)}${selected}${value.slice(end + suffix.length)}`;
      const nextStart = start - prefix.length;

      commitValue(nextValue);
      requestAnimationFrame(() => {
        textarea.focus();
        textarea.setSelectionRange(nextStart, nextStart + selected.length);
      });
      return;
    }

    updateSelection((currentSelection) => {
      const content = currentSelection || placeholder;

      return {
        text: `${prefix}${content}${suffix}`,
        selectionStart: prefix.length,
        selectionEnd: prefix.length + content.length,
      };
    });
  }

  function setHeadingLevel(level: 1 | 2 | 3 | 4 | 5 | 6) {
    updateSelectedLines((lines) => {
      const headingPattern = /^#{1,6}\s+/;
      const targetPrefix = `${"#".repeat(level)} `;

      return lines.map((line) => `${targetPrefix}${line.replace(headingPattern, "")}`);
    });
  }

  function toggleLinePrefix(prefix: string, placeholder: string) {
    const textarea = textareaRef.current;

    if (!textarea) {
      return;
    }

    if (!value.slice(textarea.selectionStart, textarea.selectionEnd) && !value.trim()) {
      updateSelection(() => ({
        text: `${prefix}${placeholder}`,
        selectionStart: prefix.length,
        selectionEnd: prefix.length + placeholder.length,
      }));
      return;
    }

    updateSelectedLines((lines) => {
      const allPrefixed = lines.every((line) => line.startsWith(prefix));

      return lines.map((line) => (allPrefixed ? line.slice(prefix.length) : `${prefix}${line}`));
    });
  }

  function toggleNumberedList() {
    updateSelectedLines((lines) => {
      const numberedPattern = /^\d+\.\s+/;
      const allNumbered = lines.every((line) => numberedPattern.test(line));

      if (allNumbered) {
        return lines.map((line) => line.replace(numberedPattern, ""));
      }

      return lines.map((line, index) => `${index + 1}. ${line.replace(numberedPattern, "")}`);
    });
  }

  function toggleTaskList() {
    const textarea = textareaRef.current;

    if (!textarea) {
      return;
    }

    if (!value.slice(textarea.selectionStart, textarea.selectionEnd) && !value.trim()) {
      updateSelection(() => ({
        text: "- [ ] Task",
        selectionStart: 6,
        selectionEnd: 10,
      }));
      return;
    }

    updateSelectedLines((lines) => {
      const taskPattern = /^(\s*)[-*+]\s+\[[ xX]\]\s+/;
      const allTasks = lines.every((line) => taskPattern.test(line));

      if (allTasks) {
        return lines.map((line) => line.replace(taskPattern, "$1"));
      }

      return lines.map((line) => {
        const indentation = line.match(/^\s*/)?.[0] ?? "";
        const content = line
          .slice(indentation.length)
          .replace(/^[-*+]\s+\[[ xX]\]\s+/, "")
          .replace(/^(?:[-*+]\s+|\d+\.\s+)/, "");
        return `${indentation}- [ ] ${content}`;
      });
    });
  }

  function toggleLink() {
    updateSelection((selected) => {
      const fullLinkMatch = selected.match(/^\[([^\]]+)\]\(([^)]+)\)$/);

      if (fullLinkMatch) {
        return {
          text: fullLinkMatch[1],
          selectionStart: 0,
          selectionEnd: fullLinkMatch[1].length,
        };
      }

      const label = selected || "link text";
      const text = `[${label}](https://)`;
      const hrefStart = label.length + 3;

      return {
        text,
        selectionStart: hrefStart,
        selectionEnd: hrefStart + "https://".length,
      };
    });
  }

  function insertAttachmentReference(attachment: ObjectAttachment, file: File) {
    const { start, end } = lastSelectionRef.current;
    const currentValue = lastValueRef.current;
    const label = (attachment.name || file.name || "attachment").replace(/[[\]]/g, "");
    const reference = `kival://attachments/${attachment.id}`;
    const markdown = (attachment.media_type || file.type).startsWith("image/")
      ? `![${label}](${reference})`
      : `[${label}](${reference})`;
    const nextValue = `${currentValue.slice(0, start)}${markdown}${currentValue.slice(end)}`;
    const nextCursor = start + markdown.length;

    commitValue(nextValue);
    lastSelectionRef.current = { start: nextCursor, end: nextCursor };

    requestAnimationFrame(() => {
      const textarea = textareaRef.current;
      textarea?.focus();
      textarea?.setSelectionRange(nextCursor, nextCursor);
    });
  }

  async function handleAttachmentSelected(file: File | undefined) {
    if (!file || !onUploadAttachment) {
      return;
    }

    setAttachmentUploading(true);
    setAttachmentError(null);

    try {
      const attachment = await onUploadAttachment(file);
      insertAttachmentReference(attachment, file);
    } catch (error) {
      setAttachmentError(error instanceof Error ? error.message : String(error));
    } finally {
      setAttachmentUploading(false);
    }
  }

  function toggleDisplayMath() {
    updateSelection((selected) => {
      const displayMathMatch = selected.match(/^\$\$\n([\s\S]*?)\n\$\$$/);

      if (displayMathMatch) {
        return {
          text: displayMathMatch[1],
          selectionStart: 0,
          selectionEnd: displayMathMatch[1].length,
        };
      }

      const content = selected || "equation";
      const text = `$$\n${content}\n$$`;

      return {
        text,
        selectionStart: 3,
        selectionEnd: 3 + content.length,
      };
    });
  }

  function toggleCodeBlock() {
    updateSelection((selected) => {
      const fencedMatch = selected.match(/^```[^\n]*\n([\s\S]*?)\n```$/);

      if (fencedMatch) {
        return {
          text: fencedMatch[1],
          selectionStart: 0,
          selectionEnd: fencedMatch[1].length,
        };
      }

      const content = selected || "code";
      const text = `\`\`\`\n${content}\n\`\`\``;

      return {
        text,
        selectionStart: 4,
        selectionEnd: 4 + content.length,
      };
    });
  }

  return (
    <div
      style={
        isFullscreen
          ? { ...styles.markdownEditor, ...styles.markdownEditorFullscreen }
          : styles.markdownEditor
      }
    >
      <div style={styles.markdownEditorTopBar}>
        <div style={styles.markdownToolbar} role="toolbar" aria-label="Markdown formatting">
          <ToolbarButton
            label={<BoldIcon />}
            title="Bold (Ctrl/Cmd+B)"
            onClick={() => toggleWrap("**")}
          />
          <ToolbarButton
            label={<ItalicIcon />}
            title="Italic (Ctrl/Cmd+I)"
            onClick={() => toggleWrap("*")}
          />
          <ToolbarButton
            label={<StrikethroughIcon />}
            title="Strikethrough"
            onClick={() => toggleWrap("~~")}
          />
          <div ref={headingMenuRef} style={styles.markdownHeadingControl}>
            <button
              type="button"
              style={styles.markdownHeadingTrigger}
              aria-label="Heading level"
              aria-haspopup="menu"
              aria-expanded={headingMenuOpen}
              onMouseDown={(event) => event.preventDefault()}
              onClick={() => setHeadingMenuOpen((open) => !open)}
            >
              <HeadingIcon level={1} />
              <svg aria-hidden="true" width="8" height="5" viewBox="0 0 8 5">
                <path
                  d="m1 1 3 3 3-3"
                  fill="none"
                  stroke="currentColor"
                  strokeWidth="1.25"
                  strokeLinecap="round"
                  strokeLinejoin="round"
                />
              </svg>
            </button>

            {headingMenuOpen ? (
              <div role="menu" aria-label="Heading level" style={styles.markdownHeadingMenu}>
                {([1, 2, 3, 4, 5, 6] as const).map((level) => (
                  <button
                    key={level}
                    type="button"
                    role="menuitem"
                    className="kival-markdown-tool"
                    data-tooltip={`Heading ${level}`}
                    aria-label={`Heading ${level}`}
                    style={styles.markdownHeadingOption}
                    onMouseDown={(event) => event.preventDefault()}
                    onClick={() => {
                      setHeadingMenuOpen(false);
                      setHeadingLevel(level);
                    }}
                  >
                    <HeadingIcon level={level} />
                  </button>
                ))}
              </div>
            ) : null}
          </div>
          <ToolbarButton label={<LinkIcon />} title="Link (Ctrl/Cmd+K)" onClick={toggleLink} />
          {onUploadAttachment && (
            <>
              <ToolbarButton
                label={attachmentUploading ? <UploadingIcon /> : <PaperclipIcon />}
                title="Upload attachment"
                disabled={attachmentUploading}
                onClick={() => attachmentInputRef.current?.click()}
              />
              <input
                ref={attachmentInputRef}
                type="file"
                hidden
                onChange={(event) => {
                  const file = event.currentTarget.files?.[0];
                  event.currentTarget.value = "";
                  void handleAttachmentSelected(file);
                }}
              />
            </>
          )}
          <ToolbarButton
            label={<LatexIcon />}
            title="LaTeX equation block"
            onClick={toggleDisplayMath}
          />
          <ToolbarButton
            label={<BulletedListIcon />}
            title="Bulleted list"
            onClick={() => toggleLinePrefix("- ", "List item")}
          />
          <ToolbarButton
            label={<NumberedListIcon />}
            title="Numbered list"
            onClick={toggleNumberedList}
          />
          <ToolbarButton
            label={<TaskListIcon />}
            title="Task list with checkboxes"
            onClick={toggleTaskList}
          />
          <ToolbarButton
            label={<BlockquoteIcon />}
            title="Quote"
            onClick={() => toggleLinePrefix("> ", "Quote")}
          />
          <ToolbarButton label={<CodeBlockIcon />} title="Code block" onClick={toggleCodeBlock} />
        </div>

        <div style={styles.markdownEditorViewControls}>
          <fieldset style={{ ...styles.markdownModeSwitch, border: 0, margin: 0, padding: 0 }}>
            <legend
              style={{
                position: "absolute",
                width: 1,
                height: 1,
                padding: 0,
                margin: -1,
                overflow: "hidden",
                clip: "rect(0, 0, 0, 0)",
                whiteSpace: "nowrap",
                border: 0,
              }}
            >
              Markdown editor mode
            </legend>
            {(["edit", "split", "preview"] as const).map((candidate) => (
              <button
                key={candidate}
                type="button"
                style={
                  mode === candidate
                    ? { ...styles.markdownModeButton, ...styles.markdownModeButtonActive }
                    : styles.markdownModeButton
                }
                onClick={() => setMode(candidate)}
              >
                {candidate[0].toUpperCase() + candidate.slice(1)}
              </button>
            ))}
          </fieldset>
          <ToolbarButton
            label={<FullscreenIcon expanded={isFullscreen} />}
            title={isFullscreen ? "Exit fullscreen (Esc)" : "Fullscreen editor"}
            active={isFullscreen}
            onClick={() => setIsFullscreen((fullscreen) => !fullscreen)}
          />
        </div>
      </div>

      {attachmentError && <div style={styles.markdownAttachmentError}>{attachmentError}</div>}

      <div
        style={{
          ...styles.markdownEditorSurface,
          ...(mode === "split" ? styles.markdownEditorSurfaceSplit : {}),
          ...(isFullscreen ? styles.markdownEditorSurfaceFullscreen : {}),
        }}
      >
        {mode !== "preview" && (
          <textarea
            ref={textareaRef}
            name="markdown_body"
            inputMode="text"
            autoComplete="off"
            data-1p-ignore="true"
            className="kival-markdown-textarea"
            value={value}
            onChange={(event) => {
              const nativeEvent = event.nativeEvent as InputEvent;

              commitInputValue(
                event.target.value,
                nativeEvent.inputType ?? "",
                event.target.selectionStart,
                event.target.selectionEnd,
              );
            }}
            onSelect={(event) => {
              const nextSelection = {
                start: event.currentTarget.selectionStart,
                end: event.currentTarget.selectionEnd,
              };
              const previousSelection = lastSelectionRef.current;

              if (
                nextSelection.start !== previousSelection.start ||
                nextSelection.end !== previousSelection.end
              ) {
                breakTypingGroup();
                lastSelectionRef.current = nextSelection;
              }
            }}
            onPaste={breakTypingGroup}
            onCut={breakTypingGroup}
            onPointerDown={breakTypingGroup}
            onKeyDown={(event) => {
              const modifier = event.metaKey || event.ctrlKey;

              if (modifier && event.key.toLowerCase() === "z") {
                event.preventDefault();

                if (event.shiftKey) {
                  redo();
                } else {
                  undo();
                }
              } else if (modifier && event.key.toLowerCase() === "y") {
                event.preventDefault();
                redo();
              } else if (modifier && event.key.toLowerCase() === "b") {
                event.preventDefault();
                toggleWrap("**");
              } else if (modifier && event.key.toLowerCase() === "i") {
                event.preventDefault();
                toggleWrap("*");
              } else if (modifier && event.key.toLowerCase() === "k") {
                event.preventDefault();
                toggleLink();
              }
            }}
            style={{
              ...styles.markdownTextarea,
              ...(mode === "split" ? styles.markdownTextareaSplit : {}),
              ...(isFullscreen ? styles.markdownTextareaFullscreen : {}),
            }}
            placeholder="Start writing in Markdown…"
            spellCheck="true"
          />
        )}

        {mode !== "edit" && (
          <div
            style={{
              ...styles.markdownPreview,
              ...(mode === "split" ? styles.markdownPreviewSplit : {}),
              ...(isFullscreen ? styles.markdownPreviewFullscreen : {}),
            }}
          >
            {value.trim() ? (
              <MarkdownBody
                body={value}
                workspaceId={workspaceId}
                objectId={objectId}
                onOpenObject={onOpenObject}
              />
            ) : (
              <p style={styles.markdownPreviewEmpty}>Nothing to preview yet.</p>
            )}
          </div>
        )}
      </div>
    </div>
  );
}
