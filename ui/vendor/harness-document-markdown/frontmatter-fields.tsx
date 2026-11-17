/**
 * Field list for a Markdown document's leading YAML frontmatter. The Markdown
 * body loads this module lazily so the YAML parser stays out of the startup
 * chunk.
 */
import { useMemo, type ReactNode } from 'react'
import { parseFields } from './frontmatter-field-values.ts'
import css from './frontmatter-fields.module.css'

/** Owner inputs for the lazily loaded field list. */
export interface FrontmatterFieldsProps {
  /** YAML between the frontmatter delimiters. */
  source: string
  /** Verbatim block shown for invalid YAML and non-mapping documents. */
  fallback: ReactNode
}

/**
 * Render frontmatter fields, or the owner's verbatim block when the YAML is not a mapping.
 * @param props - frontmatter source and its verbatim fallback.
 * @returns the field list, the fallback, or nothing for a document without content.
 */
export function FrontmatterFields({ source, fallback }: FrontmatterFieldsProps): ReactNode {
  const fields = useMemo(() => parseFields(source), [source])
  if (fields === undefined) return fallback
  if (fields.length === 0) return null
  return (
    <dl className={css.fields}>
      {fields.map((field, index) => (
        <div key={index} className={css.field}>
          <dt className={css.key}>{field.key}</dt>
          <dd className={field.block ? css.block : css.value}>{field.value}</dd>
        </div>
      ))}
    </dl>
  )
}
