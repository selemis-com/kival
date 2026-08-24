type Pinnable = {
  pinned?: boolean;
  pinned_at?: string | null;
};

/** Orders pinned resources by when they were pinned, oldest first. */
export function comparePinOrder(left: Pinnable, right: Pinnable) {
  if (!left.pinned_at) {
    return right.pinned_at ? 1 : 0;
  }
  if (!right.pinned_at) {
    return -1;
  }
  return Date.parse(left.pinned_at) - Date.parse(right.pinned_at);
}

/** Places pinned resources first while preserving their pin order. */
export function comparePinnedFirst(left: Pinnable, right: Pinnable) {
  const pinStateOrder = Number(Boolean(right.pinned)) - Number(Boolean(left.pinned));
  return pinStateOrder || (left.pinned && right.pinned ? comparePinOrder(left, right) : 0);
}
