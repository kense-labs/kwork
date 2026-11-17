import { TextSearchIndex } from './vendor/harness-text-search.ts';

interface Metadata {
  id: string;
  title: string;
  workspace: string;
}

interface IndexedMetadata extends Metadata {
  position: number;
}

const keys = ({ title, workspace, id }: IndexedMetadata) => ({
  fuzzy: [title.trim(), workspace],
  substring: [id],
});

export class SessionMetadataIndex {
  #index = new TextSearchIndex<IndexedMetadata>([], keys);

  update(entries: readonly Metadata[], newestFirst: Uint32Array) {
    // Preserve the positions in the catalog while using the Host's recency
    // order to break ties in TextSearchIndex.
    const ordered = Array.from(newestFirst, position => ({ ...entries[position]!, position }));
    this.#index = this.#index.update(ordered, keys);
  }

  search(query: string): Uint32Array {
    return Uint32Array.from(this.#index.search(query), entry => entry.position);
  }
}
