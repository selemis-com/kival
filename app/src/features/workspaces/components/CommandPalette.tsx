import { useEffect, useMemo, useRef, useState } from "react";
import { styles } from "../../../shared/styles/index";
import type { ObjectSummary, Workspace } from "../../../shared/types";

type Command = {
  id: string;
  label: string;
  hint?: string;
  keywords: string;
  onSelect: () => void;
};

type Props = {
  open: boolean;
  workspace: Workspace;
  workspaces: Workspace[];
  objects: ObjectSummary[];
  canManageWorkspace: boolean;
  onClose: () => void;
  onNavigate: (path: string) => void;
  onOpenObject: (objectId: string) => void;
  onSwitchWorkspace: (workspaceId: string) => void;
};

export function CommandPalette({
  open,
  workspace,
  workspaces,
  objects,
  canManageWorkspace,
  onClose,
  onNavigate,
  onOpenObject,
  onSwitchWorkspace,
}: Props) {
  const [query, setQuery] = useState("");
  const [activeIndex, setActiveIndex] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const basePath = `/w/${workspace.id}`;

  const commands = useMemo<Command[]>(() => {
    const navigationCommands: Command[] = [
      {
        id: "home",
        label: "Go to Home",
        hint: "Workspace",
        keywords: "home objects",
        onSelect: () => onNavigate(basePath),
      },
      {
        id: "recent",
        label: "Go to Recent",
        hint: "Workspace",
        keywords: "recent updated",
        onSelect: () => onNavigate(`${basePath}/recent`),
      },
      {
        id: "graph",
        label: "Open Graph",
        hint: "Workspace",
        keywords: "graph network",
        onSelect: () => onNavigate(`${basePath}/graph`),
      },
      {
        id: "archived",
        label: "Go to Archived",
        hint: "Workspace",
        keywords: "archived deleted",
        onSelect: () => onNavigate(`${basePath}/archived`),
      },
      {
        id: "members",
        label: "Go to Members",
        hint: "Workspace",
        keywords: "members people",
        onSelect: () => onNavigate(`${basePath}/members`),
      },
      {
        id: "new",
        label: "Create new object",
        hint: "Action",
        keywords: "new create object",
        onSelect: () => onNavigate(`${basePath}/new`),
      },
    ];

    if (canManageWorkspace) {
      navigationCommands.splice(5, 0, {
        id: "settings",
        label: "Go to Settings",
        hint: "Workspace",
        keywords: "settings configure",
        onSelect: () => onNavigate(`${basePath}/settings`),
      });
    }

    const objectCommands = objects.map<Command>((object) => ({
      id: `object-${object.id}`,
      label: object.title,
      hint: "Object",
      keywords: object.title,
      onSelect: () => onOpenObject(object.id),
    }));

    const workspaceCommands = workspaces
      .filter((candidate) => candidate.id !== workspace.id)
      .map<Command>((candidate) => ({
        id: `workspace-${candidate.id}`,
        label: `Switch to ${candidate.name}`,
        hint: "Workspace",
        keywords: `switch workspace ${candidate.name}`,
        onSelect: () => onSwitchWorkspace(candidate.id),
      }));

    return [...navigationCommands, ...objectCommands, ...workspaceCommands];
  }, [
    basePath,
    canManageWorkspace,
    objects,
    onNavigate,
    onOpenObject,
    onSwitchWorkspace,
    workspace.id,
    workspaces,
  ]);

  const normalizedQuery = query.trim().toLowerCase();
  const filteredCommands = normalizedQuery
    ? commands.filter((command) =>
        `${command.label} ${command.keywords}`.toLowerCase().includes(normalizedQuery),
      )
    : commands;

  useEffect(() => {
    if (!open) {
      setQuery("");
      setActiveIndex(0);
      return;
    }

    requestAnimationFrame(() => inputRef.current?.focus());
  }, [open]);

  if (!open) {
    return null;
  }

  function choose(command: Command | undefined) {
    if (!command) {
      return;
    }

    onClose();
    command.onSelect();
  }

  return (
    <div
      style={styles.commandPaletteBackdrop}
      role="presentation"
      onPointerDown={(event) => {
        if (event.target === event.currentTarget) {
          onClose();
        }
      }}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-label="Command palette"
        style={styles.commandPalette}
      >
        <div style={styles.commandPaletteSearchRow}>
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" aria-hidden="true">
            <circle cx="11" cy="11" r="7" stroke="currentColor" strokeWidth="2" />
            <path d="m20 20-4-4" stroke="currentColor" strokeWidth="2" strokeLinecap="round" />
          </svg>
          <input
            data-1p-ignore="true"
            autoComplete="off"
            ref={inputRef}
            value={query}
            onChange={(event) => {
              setQuery(event.target.value);
              setActiveIndex(0);
            }}
            onKeyDown={(event) => {
              if (event.key === "Escape") {
                event.preventDefault();
                onClose();
              } else if (event.key === "ArrowDown") {
                event.preventDefault();
                setActiveIndex((index) =>
                  Math.min(index + 1, Math.max(0, filteredCommands.length - 1)),
                );
              } else if (event.key === "ArrowUp") {
                event.preventDefault();
                setActiveIndex((index) => Math.max(0, index - 1));
              } else if (event.key === "Enter") {
                event.preventDefault();
                choose(filteredCommands[activeIndex]);
              }
            }}
            placeholder="Search objects or run a command…"
            aria-label="Search commands"
            style={styles.commandPaletteInput}
          />
          <kbd style={styles.commandPaletteKey}>Esc</kbd>
        </div>

        <div role="listbox" aria-label="Commands" style={styles.commandPaletteResults}>
          {filteredCommands.slice(0, 12).map((command, index) => (
            <button
              key={command.id}
              type="button"
              role="option"
              aria-selected={index === activeIndex}
              style={
                index === activeIndex ? styles.commandPaletteItemActive : styles.commandPaletteItem
              }
              onPointerMove={() => setActiveIndex(index)}
              onClick={() => choose(command)}
            >
              <span>{command.label}</span>
              {command.hint && <span style={styles.commandPaletteHint}>{command.hint}</span>}
            </button>
          ))}

          {filteredCommands.length === 0 && (
            <div style={styles.commandPaletteEmpty}>No matching objects or commands.</div>
          )}
        </div>
      </div>
    </div>
  );
}
