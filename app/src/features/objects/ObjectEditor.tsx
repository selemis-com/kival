import type { FormEvent } from "react";
import { useEffect, useMemo, useRef, useState } from "react";
import { kival } from "../../shared/api";
import { styles } from "../../shared/styles/index";
import type {
  CreateObjectRequest,
  CurrentObjectResponse,
  FlatMetadata,
  JsonObject,
  JsonValue,
  UpdateObjectRequest,
} from "../../shared/types";
import { MarkdownEditor } from "./components/MarkdownEditor";

type CreateProps = {
  mode: "create";
  loading: boolean;
  onCancel: () => void;
  onSubmit: (input: CreateObjectRequest) => Promise<void>;
  workspaceId: string;
  onOpenObject: (objectId: string) => void;
  onDirtyChange: (dirty: boolean) => void;
};

type EditProps = {
  mode: "edit";
  value: CurrentObjectResponse;
  loading: boolean;
  onCancel: () => void;
  onSubmit: (input: UpdateObjectRequest) => Promise<void>;
  workspaceId: string;
  onOpenObject: (objectId: string) => void;
  onDirtyChange: (dirty: boolean) => void;
};

type Props = CreateProps | EditProps;

type MetadataProperty = {
  id: number;
  key: string;
  value: string;
  originalValue?: unknown;
  preserveOriginal?: boolean;
};

function formatMetadataValue(value: unknown) {
  if (typeof value === "string") {
    return value;
  }

  const serialized = JSON.stringify(value);
  return serialized ?? String(value);
}

function metadataToProperties(metadata: Record<string, unknown>): MetadataProperty[] {
  return Object.entries(metadata).map(([key, value], index) => ({
    id: index,
    key,
    value: formatMetadataValue(value),
    originalValue: value,
    preserveOriginal: true,
  }));
}

function parseMetadataValue(value: string): JsonValue {
  const trimmed = value.trim();

  if (!trimmed) {
    return "";
  }

  try {
    return JSON.parse(trimmed) as JsonValue;
  } catch {
    return value;
  }
}

function propertiesToMetadata(properties: MetadataProperty[]) {
  const metadata: JsonObject = {};

  for (const property of properties) {
    const key = property.key.trim();

    if (!key) {
      continue;
    }

    metadata[key] =
      property.preserveOriginal && property.value === formatMetadataValue(property.originalValue)
        ? (property.originalValue as JsonValue)
        : parseMetadataValue(property.value);
  }

  return metadata;
}

function isFlatMetadata(metadata: JsonObject): metadata is FlatMetadata {
  return Object.values(metadata).every((value) => {
    if (Array.isArray(value)) {
      return value.every(
        (item) =>
          item === null ||
          typeof item === "boolean" ||
          typeof item === "number" ||
          typeof item === "string",
      );
    }

    return (
      value === null ||
      typeof value === "boolean" ||
      typeof value === "number" ||
      typeof value === "string"
    );
  });
}

export function ObjectEditor(props: Props) {
  const isEditing = props.mode === "edit";
  const [title, setTitle] = useState(isEditing ? props.value.current_version.title : "");
  const [body, setBody] = useState(isEditing ? props.value.current_version.body : "");
  const [metadataProperties, setMetadataProperties] = useState<MetadataProperty[]>(() =>
    metadataToProperties(isEditing ? props.value.current_version.metadata : {}),
  );
  const [nextMetadataPropertyId, setNextMetadataPropertyId] = useState(metadataProperties.length);
  const [error, setError] = useState<string | null>(null);
  const formRef = useRef<HTMLFormElement>(null);
  const metadataObject = useMemo(
    () => propertiesToMetadata(metadataProperties),
    [metadataProperties],
  );
  const dirty = isEditing
    ? title.trim() !== props.value.current_version.title ||
      body !== props.value.current_version.body ||
      JSON.stringify(metadataObject) !== JSON.stringify(props.value.current_version.metadata)
    : title.trim().length > 0 || body.length > 0 || metadataProperties.length > 0;

  useEffect(() => {
    props.onDirtyChange(dirty);

    return () => {
      props.onDirtyChange(false);
    };
  }, [dirty, props.onDirtyChange]);

  useEffect(() => {
    if (!dirty || props.loading) {
      return;
    }

    function handleBeforeUnload(event: BeforeUnloadEvent) {
      event.preventDefault();
    }

    window.addEventListener("beforeunload", handleBeforeUnload);
    return () => window.removeEventListener("beforeunload", handleBeforeUnload);
  }, [dirty, props.loading]);

  useEffect(() => {
    function handleSaveShortcut(event: KeyboardEvent) {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "s") {
        event.preventDefault();
        formRef.current?.requestSubmit();
      }
    }

    document.addEventListener("keydown", handleSaveShortcut);
    return () => document.removeEventListener("keydown", handleSaveShortcut);
  }, []);

  async function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setError(null);

    const normalizedTitle = title.trim();

    const metadataKeys = metadataProperties.map((property) => property.key.trim()).filter(Boolean);

    if (new Set(metadataKeys).size !== metadataKeys.length) {
      setError("Metadata keys must be unique.");
      return;
    }

    if (!isFlatMetadata(metadataObject)) {
      setError("Metadata values must be JSON scalars or one-dimensional lists of JSON scalars.");
      return;
    }

    if (!normalizedTitle) {
      setError("Title is required.");
      return;
    }

    try {
      if (props.mode === "create") {
        await props.onSubmit({
          title: normalizedTitle,
          body,
          metadata: metadataObject,
        });
      } else {
        const input: UpdateObjectRequest = {
          expected_current_version_id: props.value.current_version.id,
        };

        if (normalizedTitle !== props.value.current_version.title) {
          input.title = normalizedTitle;
        }

        if (body !== props.value.current_version.body) {
          input.body = body;
        }

        if (
          JSON.stringify(metadataObject) !== JSON.stringify(props.value.current_version.metadata)
        ) {
          input.metadata = metadataObject;
        }

        if (Object.keys(input).length === 1) {
          setError("No changes to save.");
          return;
        }

        await props.onSubmit(input);
      }
    } catch (error) {
      setError(error instanceof Error ? error.message : String(error));
    }
  }

  return (
    <form ref={formRef} style={styles.editorForm} onSubmit={handleSubmit}>
      <div style={styles.editorHeader}>
        <div>
          <p style={styles.eyebrow}>Object</p>
          <h1 style={styles.pageTitle}>{isEditing ? "Edit object" : "Create object"}</h1>
        </div>

        <div style={styles.editorActions}>
          <button type="button" style={styles.secondaryButton} onClick={props.onCancel}>
            Cancel
          </button>
          <button type="submit" style={styles.primaryButtonCompact} disabled={props.loading}>
            {props.loading ? "Saving…" : isEditing ? "Save changes" : "Create object"}
          </button>
        </div>
      </div>

      {error && (
        <div style={styles.errorBox}>
          <strong>Could not save object</strong>
          <span>{error}</span>
        </div>
      )}

      <label style={styles.field}>
        <span>Title</span>
        <input
          data-1p-ignore="true"
          value={title}
          onChange={(event) => setTitle(event.target.value)}
          style={styles.input}
          autoComplete="off"
        />
      </label>

      <div style={styles.field}>
        <span>Metadata</span>

        <div style={styles.metadataEditor}>
          {metadataProperties.map((property) => (
            <div key={property.id} style={styles.metadataEditorRow}>
              <input
                data-1p-ignore="true"
                value={property.key}
                onChange={(event) => {
                  const key = event.target.value;

                  setMetadataProperties((properties) =>
                    properties.map((candidate) =>
                      candidate.id === property.id ? { ...candidate, key } : candidate,
                    ),
                  );
                }}
                style={styles.metadataEditorKey}
                placeholder="Key"
                autoComplete="off"
              />

              <input
                data-1p-ignore="true"
                value={property.value}
                onChange={(event) => {
                  const value = event.target.value;

                  setMetadataProperties((properties) =>
                    properties.map((candidate) =>
                      candidate.id === property.id ? { ...candidate, value } : candidate,
                    ),
                  );
                }}
                style={styles.metadataEditorValue}
                placeholder="Value"
                autoComplete="off"
              />

              <button
                type="button"
                style={styles.metadataEditorRemove}
                aria-label={`Remove ${property.key || "metadata"} metadata entry`}
                onClick={() =>
                  setMetadataProperties((properties) =>
                    properties.filter((candidate) => candidate.id !== property.id),
                  )
                }
              >
                ×
              </button>
            </div>
          ))}

          <button
            type="button"
            style={styles.metadataEditorAdd}
            onClick={() => {
              const id = nextMetadataPropertyId;

              setNextMetadataPropertyId(id + 1);
              setMetadataProperties((properties) => [...properties, { id, key: "", value: "" }]);
            }}
          >
            + Add metadata
          </button>
        </div>
      </div>

      <div style={styles.field}>
        <span>Body</span>
        <MarkdownEditor
          value={body}
          onChange={setBody}
          workspaceId={props.workspaceId}
          objectId={props.mode === "edit" ? props.value.object.id : undefined}
          onOpenObject={props.onOpenObject}
          onUploadAttachment={
            props.mode === "edit"
              ? async (file) =>
                  await kival.uploadObjectAttachment({
                    workspaceId: props.workspaceId,
                    objectId: props.value.object.id,
                    params: {
                      name: file.name,
                      media_type: file.type || undefined,
                    },
                    body: file,
                  })
              : undefined
          }
        />
      </div>
    </form>
  );
}
