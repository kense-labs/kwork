import { isMap, isNode, isScalar, parseDocument } from 'yaml'

/** One top-level entry; `block` marks the authored YAML of a collection or alias value. */
interface FrontmatterField {
  key: string
  value: string
  block: boolean
}

/**
 * Resolve top-level mapping fields from the YAML syntax tree.
 * @param source - YAML between the frontmatter delimiters.
 * @returns the fields, `[]` for a document without content, or `undefined` for invalid YAML and non-mapping documents.
 */
export function parseFields(source: string): readonly FrontmatterField[] | undefined {
  const parsed = parseDocument(source)
  if (parsed.errors.length > 0) return undefined
  if (parsed.contents === null) return []
  if (!isMap(parsed.contents)) return undefined
  return parsed.contents.items.map(({ key, value }) => ({
    key: nodeText(source, key),
    value: nodeText(source, value),
    block: isNode(value) && !isScalar(value),
  }))
}

// Strings show their resolved value, so folded and multiline text reads as prose; other
// nodes keep their authored text, so `1.0` stays `1.0` and aliases stay unexpanded.
function nodeText(source: string, node: unknown): string {
  if (isScalar(node) && typeof node.value === 'string') return node.value.trimEnd()
  if (!isNode(node)) return ''
  /* v8 ignore next -- parsed nodes always carry their source range. */
  const [start, end] = node.range ?? [0, 0]
  const indent = start - source.lastIndexOf('\n', start - 1) - 1
  return source.slice(start, end).replace(new RegExp(`\\n {0,${indent}}`, 'gu'), '\n').trimEnd()
}
