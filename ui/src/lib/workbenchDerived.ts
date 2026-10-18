// Pure sidebar derivations; App memoises them by catalog and filter inputs.
export interface CatalogItemLike {
  id: string;
  title: string;
  description?: string;
  tags: string[];
}

// Case-insensitive match over the title, description, id and tags, combined with
// an exact tag filter. The query is lower-cased once per call rather than once
// per item; results and order are otherwise identical to the inline filter.
export function filterCatalogItems<T extends CatalogItemLike>(
  items: readonly T[],
  query: string,
  tag: string,
): T[] {
  const needle = query.toLowerCase();
  return items.filter(
    (item) =>
      (!query ||
        [item.title, item.description ?? "", item.id, ...item.tags]
          .join(" ")
          .toLowerCase()
          .includes(needle)) &&
      (!tag || item.tags.includes(tag)),
  );
}

// Unique tags in first-seen order, for the sidebar's tag selector.
export function catalogTags(items: readonly CatalogItemLike[]): string[] {
  return [...new Set(items.flatMap((item) => item.tags))];
}
