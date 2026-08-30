//! Cursor pagination helpers.

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use kival_sdk::{EventListParams, ListParams, ListResponse};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::api::error::{ApiError, ApiResult};

/// Maximum accepted encoded pagination cursor length.
const MAX_CURSOR_LENGTH: usize = 1024;

/// Decoded cursor for created-at ordered list pages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CreatedAtCursor {
    /// Creation timestamp at the page boundary.
    pub(crate) created_at: DateTime<Utc>,
    /// Stable ID tie-breaker at the page boundary.
    pub(crate) id: Uuid,
}

/// Decoded cursor for updated-at ordered list pages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct UpdatedAtCursor {
    /// Update timestamp at the page boundary.
    pub(crate) updated_at: DateTime<Utc>,
    /// Stable ID tie-breaker at the page boundary.
    pub(crate) id: Uuid,
}

/// Decoded cursor for descending sequence-number pagination.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SequenceCursor {
    /// Sequence number at the page boundary.
    pub(crate) sequence_number: i64,
}

/// Decoded cursor for version-number ordered list pages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct VersionCursor {
    /// Version number at the page boundary.
    pub(crate) version_number: i64,
}

/// Decoded cursor for ranked workspace search pagination.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct SearchCursor {
    /// Search rank at the page boundary.
    pub(crate) rank: f32,
    /// Object identifier at the page boundary.
    pub(crate) object_id: Uuid,
    /// Version number at the page boundary.
    pub(crate) version_number: i64,
    /// Version identifier at the page boundary.
    pub(crate) version_id: Uuid,
}

/// Serialized pagination cursor variants.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Cursor {
    /// Cursor for resources ordered by creation time and ID.
    CreatedAt {
        /// Resource kind this cursor is valid for.
        kind: String,
        /// Optional parent scope this cursor is valid for.
        scope: Option<Uuid>,
        /// Creation timestamp at the page boundary.
        created_at: DateTime<Utc>,
        /// Stable ID tie-breaker at the page boundary.
        id: Uuid,
    },
    /// Cursor for resources ordered by update time and ID.
    UpdatedAt {
        /// Resource kind this cursor is valid for.
        kind: String,
        /// Optional parent scope this cursor is valid for.
        scope: Option<Uuid>,
        /// Update timestamp at the page boundary.
        updated_at: DateTime<Utc>,
        /// Stable ID tie-breaker at the page boundary.
        id: Uuid,
    },
    /// Cursor for resources ordered by descending sequence number.
    Sequence {
        /// Resource kind this cursor is valid for.
        kind: String,
        /// Optional parent scope this cursor is valid for.
        scope: Option<Uuid>,
        /// Sequence number at the page boundary.
        sequence_number: i64,
    },
    /// Cursor for resources ordered by version number.
    Version {
        /// Resource kind this cursor is valid for.
        kind: String,
        /// Parent scope this cursor is valid for.
        scope: Uuid,
        /// Version number at the page boundary.
        version_number: i64,
    },
    /// Cursor for ranked search results.
    Search {
        /// Search kind including the normalized filter fingerprint.
        kind: String,
        /// Workspace scope this cursor is valid for.
        scope: Uuid,
        /// IEEE-754 bits for the rank at the page boundary.
        rank_bits: u32,
        /// Object identifier at the page boundary.
        object_id: Uuid,
        /// Version number at the page boundary.
        version_number: i64,
        /// Version identifier at the page boundary.
        version_id: Uuid,
    },
}

/// Validates optional event sequence boundaries.
///
/// Event sequence numbers are non-negative.
///
/// # Errors
///
/// Returns an API error if either boundary is negative.
pub(crate) fn validated_event_bounds(
    params: &EventListParams,
) -> ApiResult<(Option<i64>, Option<i64>)> {
    for (field, value) in
        [("after_sequence", params.after_sequence), ("before_sequence", params.before_sequence)]
    {
        if value.is_some_and(|value| value < 0) {
            return Err(ApiError::bad_request(format!("{field} must be at least 0")));
        }
    }

    Ok((params.after_sequence, params.before_sequence))
}

/// Decodes and validates a created-at pagination cursor from a list params.
pub(crate) fn decode_created_at(
    params: &ListParams,
    kind: &str,
    scope: Option<Uuid>,
) -> ApiResult<Option<CreatedAtCursor>> {
    let Some(cursor) = decode(params.cursor.as_deref())? else {
        return Ok(None);
    };

    match cursor {
        Cursor::CreatedAt { kind: cursor_kind, scope: cursor_scope, created_at, id }
            if cursor_kind == kind && cursor_scope == scope =>
        {
            Ok(Some(CreatedAtCursor { created_at, id }))
        }
        _ => Err(ApiError::bad_request("invalid pagination cursor")),
    }
}

/// Decodes and validates an updated-at pagination cursor from a list params.
///
/// # Errors
///
/// Returns an API error if the cursor is malformed or belongs to a different result set.
pub(crate) fn decode_updated_at(
    params: &ListParams,
    kind: &str,
    scope: Option<Uuid>,
) -> ApiResult<Option<UpdatedAtCursor>> {
    let Some(cursor) = decode(params.cursor.as_deref())? else {
        return Ok(None);
    };

    match cursor {
        Cursor::UpdatedAt { kind: cursor_kind, scope: cursor_scope, updated_at, id }
            if cursor_kind == kind && cursor_scope == scope =>
        {
            Ok(Some(UpdatedAtCursor { updated_at, id }))
        }
        _ => Err(ApiError::bad_request("invalid pagination cursor")),
    }
}

/// Decodes and validates a descending sequence pagination cursor from a list params.
pub(crate) fn decode_sequence(
    params: &ListParams,
    kind: &str,
    scope: Option<Uuid>,
) -> ApiResult<Option<SequenceCursor>> {
    let Some(cursor) = decode(params.cursor.as_deref())? else {
        return Ok(None);
    };

    match cursor {
        Cursor::Sequence { kind: cursor_kind, scope: cursor_scope, sequence_number }
            if cursor_kind == kind && cursor_scope == scope =>
        {
            Ok(Some(SequenceCursor { sequence_number }))
        }
        _ => Err(ApiError::bad_request("invalid pagination cursor")),
    }
}

/// Decodes and validates a version pagination cursor from a list params.
pub(crate) fn decode_version(
    params: &ListParams,
    kind: &'static str,
    scope: Uuid,
) -> ApiResult<Option<VersionCursor>> {
    let Some(cursor) = decode(params.cursor.as_deref())? else {
        return Ok(None);
    };

    match cursor {
        Cursor::Version { kind: cursor_kind, scope: cursor_scope, version_number }
            if cursor_kind == kind && cursor_scope == scope =>
        {
            Ok(Some(VersionCursor { version_number }))
        }
        _ => Err(ApiError::bad_request("invalid pagination cursor")),
    }
}

/// Decodes and validates a ranked search pagination cursor.
pub(crate) fn decode_search(
    cursor: Option<&str>,
    kind: &str,
    scope: Uuid,
) -> ApiResult<Option<SearchCursor>> {
    let Some(cursor) = decode(cursor)? else {
        return Ok(None);
    };

    match cursor {
        Cursor::Search {
            kind: cursor_kind,
            scope: cursor_scope,
            rank_bits,
            object_id,
            version_number,
            version_id,
        } if cursor_kind == kind && cursor_scope == scope => {
            let rank = f32::from_bits(rank_bits);
            if !rank.is_finite() {
                return Err(ApiError::bad_request("invalid pagination cursor"));
            }
            Ok(Some(SearchCursor { rank, object_id, version_number, version_id }))
        }
        _ => Err(ApiError::bad_request("invalid pagination cursor")),
    }
}

/// Builds a paginated response for ranked workspace search results.
pub(crate) fn search_page<T>(
    mut items: Vec<T>,
    limit: i64,
    kind: &str,
    scope: Uuid,
    cursor_of: impl Fn(&T) -> (f32, Uuid, i64, Uuid),
) -> ApiResult<ListResponse<T>> {
    let has_next = items.len() > limit as usize;
    if has_next {
        items.truncate(limit as usize);
    }

    let next_cursor = if has_next {
        let (rank, object_id, version_number, version_id) =
            cursor_of(items.last().ok_or_else(|| ApiError::internal("empty pagination page"))?);
        Some(encode(&Cursor::Search {
            kind: kind.to_owned(),
            scope,
            rank_bits: rank.to_bits(),
            object_id,
            version_number,
            version_id,
        })?)
    } else {
        None
    };

    Ok(ListResponse { items, next_cursor })
}

/// Builds a paginated response for created-at ordered items.
pub(crate) fn created_at_page<T>(
    mut items: Vec<T>,
    limit: i64,
    kind: &str,
    scope: Option<Uuid>,
    cursor_of: impl Fn(&T) -> (DateTime<Utc>, Uuid),
) -> ApiResult<ListResponse<T>> {
    let has_next = items.len() > limit as usize;
    if has_next {
        items.truncate(limit as usize);
    }

    let next_cursor = if has_next {
        let (created_at, id) =
            cursor_of(items.last().ok_or_else(|| ApiError::internal("empty pagination page"))?);
        Some(encode(&Cursor::CreatedAt { kind: kind.to_owned(), scope, created_at, id })?)
    } else {
        None
    };

    Ok(ListResponse { items, next_cursor })
}

/// Builds a paginated response for updated-at ordered items.
///
/// # Errors
///
/// Returns an API error if a continuation cursor cannot be encoded.
pub(crate) fn updated_at_page<T>(
    mut items: Vec<T>,
    limit: i64,
    kind: &str,
    scope: Option<Uuid>,
    cursor_of: impl Fn(&T) -> (DateTime<Utc>, Uuid),
) -> ApiResult<ListResponse<T>> {
    let has_next = items.len() > limit as usize;
    if has_next {
        items.truncate(limit as usize);
    }

    let next_cursor = if has_next {
        let (updated_at, id) =
            cursor_of(items.last().ok_or_else(|| ApiError::internal("empty pagination page"))?);
        Some(encode(&Cursor::UpdatedAt { kind: kind.to_owned(), scope, updated_at, id })?)
    } else {
        None
    };

    Ok(ListResponse { items, next_cursor })
}

/// Builds a paginated response for descending sequence-ordered items.
pub(crate) fn sequence_page<T>(
    mut items: Vec<T>,
    limit: i64,
    kind: &str,
    scope: Option<Uuid>,
    cursor_of: impl Fn(&T) -> i64,
) -> ApiResult<ListResponse<T>> {
    let has_next = items.len() > limit as usize;
    if has_next {
        items.truncate(limit as usize);
    }

    let next_cursor = if has_next {
        let sequence_number =
            cursor_of(items.last().ok_or_else(|| ApiError::internal("empty pagination page"))?);
        Some(encode(&Cursor::Sequence { kind: kind.to_owned(), scope, sequence_number })?)
    } else {
        None
    };

    Ok(ListResponse { items, next_cursor })
}

/// Builds a paginated response for version ordered items.
pub(crate) fn version_page<T>(
    mut items: Vec<T>,
    limit: i64,
    kind: &'static str,
    scope: Uuid,
    cursor_of: impl Fn(&T) -> i64,
) -> ApiResult<ListResponse<T>> {
    let has_next = items.len() > limit as usize;
    if has_next {
        items.truncate(limit as usize);
    }

    let next_cursor = if has_next {
        let version_number =
            cursor_of(items.last().ok_or_else(|| ApiError::internal("empty pagination page"))?);
        Some(encode(&Cursor::Version { kind: kind.to_owned(), scope, version_number })?)
    } else {
        None
    };

    Ok(ListResponse { items, next_cursor })
}

/// Builds a cursor kind bound to the normalized filters that define a result set.
///
/// # Errors
///
/// Returns an internal API error if the filter discriminator cannot be serialized.
pub(crate) fn filtered_kind<T: Serialize>(base: &str, discriminator: &T) -> ApiResult<String> {
    let discriminator = serde_json::to_vec(discriminator)
        .map_err(|_| ApiError::internal("pagination cursor filter encoding failed"))?;
    Ok(format!("{base}:{}", hex::encode(Sha256::digest(&discriminator))))
}

/// Decodes a cursor string from URL-safe base64 JSON.
fn decode(cursor: Option<&str>) -> ApiResult<Option<Cursor>> {
    let Some(cursor) = cursor else {
        return Ok(None);
    };
    if cursor.len() > MAX_CURSOR_LENGTH {
        return Err(ApiError::bad_request("invalid pagination cursor"));
    }

    let bytes = URL_SAFE_NO_PAD
        .decode(cursor)
        .map_err(|_| ApiError::bad_request("invalid pagination cursor"))?;
    let cursor = serde_json::from_slice(&bytes)
        .map_err(|_| ApiError::bad_request("invalid pagination cursor"))?;

    Ok(Some(cursor))
}

/// Encodes a cursor as URL-safe base64 JSON.
fn encode(cursor: &Cursor) -> ApiResult<String> {
    let bytes = serde_json::to_vec(cursor)
        .map_err(|_| ApiError::internal("pagination cursor encoding failed"))?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filtered_kind_uses_a_fixed_size_filter_fingerprint() {
        let short = filtered_kind("users", &("active", "a")).expect("filter should encode");
        let long_filter = "a".repeat(16 * 1024);
        let long = filtered_kind("users", &("active", long_filter.as_str()))
            .expect("long filter should encode");
        let changed = filtered_kind("users", &("active", "b")).expect("filter should encode");

        assert_eq!(short.len(), "users:".len() + 64);
        assert_eq!(long.len(), short.len());
        assert_ne!(short, changed);
    }

    #[test]
    fn decode_rejects_oversized_cursors_before_decoding() {
        let oversized = "A".repeat(MAX_CURSOR_LENGTH + 1);

        assert!(decode(Some(&oversized)).is_err());
    }

    #[test]
    fn sequence_pages_preserve_scope_and_boundary() {
        let scope = Uuid::now_v7();
        let page = sequence_page(vec![9_i64, 8, 7], 2, "inbox", Some(scope), |value| *value)
            .expect("sequence page should encode");
        assert_eq!(page.items, vec![9, 8]);

        let params = ListParams { limit: None, cursor: page.next_cursor };
        let cursor = decode_sequence(&params, "inbox", Some(scope))
            .expect("cursor should decode")
            .expect("cursor should be present");
        assert_eq!(cursor.sequence_number, 8);
        assert!(decode_sequence(&params, "inbox", None).is_err());
    }
}
