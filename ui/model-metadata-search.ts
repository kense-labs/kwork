import { TextSearchIndex } from './vendor/harness-text-search.ts';

type ModelKeys = readonly [name: string, id: string | undefined, description: string | undefined];

export function rank_model_choices(models: readonly ModelKeys[], query: string): Uint32Array {
  const rows = models.map(([name, id, description], position) => ({ name, id, description, position }));
  const index = new TextSearchIndex(rows, row => ({
    fuzzy: [row.name, row.id],
    substring: [row.description],
  }));
  return Uint32Array.from(index.search(query), row => row.position);
}
