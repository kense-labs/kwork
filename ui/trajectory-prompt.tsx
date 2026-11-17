/** Extracted from upstream TrajectoryTable.tsx at 39163dd; host types/i18n adapted. */
import { structuredPatch } from 'diff';
import { JsonTree, type JsonTreeProps, type JsonTreeLabels } from './vendor/harness-primitives/JsonTree';
import { IconChevronRightOutlineRegular } from './vendor/harness-primitives/icons';
import css from './trajectory-prompt.module.css';
interface ConversationPromptSnapshot {
  system: string;
  tools: readonly { name: string; description: string; parameters: unknown }[];
}
function ToolGlyph() {
  return (
    <svg
      className={css.toolCatalogIcon}
      width="12"
      height="12"
      viewBox="0 0 24 24"
      fill="none"
      aria-hidden="true"
    >
      <path
        d="M14.7 6.3a1 1 0 0 0 0 1.4l1.6 1.6a1 1 0 0 0 1.4 0l3.77-3.77a6 6 0 0 1-7.94 7.94l-6.91 6.91a2.12 2.12 0 0 1-3-3l6.91-6.91a6 6 0 0 1 7.94-7.94z"
        stroke="currentColor"
        strokeWidth="1.8"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  )
}

export function ToolCatalog({
  tools,
  stringWrapping,
  labels,
}: {
  tools: ConversationPromptSnapshot['tools']
  stringWrapping: JsonTreeProps['stringWrapping']
  labels: JsonTreeLabels
}) {
  if (tools.length === 0) return <p className={css.noPayload}>本次请求没有工具</p>
  return (
    <div className={css.toolCatalog}>
      {tools.map((tool, index) => (
        <details className={css.toolCatalogItem} key={`${tool.name}:${index}`}>
          <summary className={css.toolCatalogSummary}>
            <IconChevronRightOutlineRegular className={css.toolCatalogChevron} size={12} />
            <ToolGlyph />
            <span className={css.toolCatalogName}>{tool.name}</span>
            <span className={css.toolCatalogDescription}>{tool.description}</span>
          </summary>
          <div className={css.toolCatalogDefinition}>
            {tool.description !== '' && (
              <p className={css.toolCatalogFullDescription}>{tool.description}</p>
            )}
            <JsonTree
              data={tool.parameters}
              stringWrapping={stringWrapping}
              label={`${tool.name} 参数 JSON`}
              labels={labels}
              className={css.toolCatalogTree}
            />
          </div>
        </details>
      ))}
    </div>
  )
}

interface PromptDiffLine {
  kind: 'meta' | 'context' | 'added' | 'removed'
  text: string
}

function promptDiffLines(before: string, after: string): readonly PromptDiffLine[] {
  const patch = structuredPatch('', '', before, after, undefined, undefined, { context: 3 })
  return patch.hunks.flatMap((hunk, hunkIndex) => [
    ...(hunkIndex === 0 ? [] : [{ kind: 'meta' as const, text: '' }]),
    {
      kind: 'meta' as const,
      text: `@@ -${hunk.oldStart},${hunk.oldLines} +${hunk.newStart},${hunk.newLines} @@`,
    },
    ...hunk.lines.flatMap((line): PromptDiffLine[] => {
      if (line.startsWith('\\')) return []
      if (line.startsWith('+')) return [{ kind: 'added', text: line }]
      if (line.startsWith('-')) return [{ kind: 'removed', text: line }]
      return [{ kind: 'context', text: line }]
    }),
  ])
}

function PromptDiffSection({
  title,
  before,
  after,
}: {
  title: string
  before: string
  after: string
}) {
  const lines = promptDiffLines(before, after)
  if (lines.length === 0) return null
  return (
    <section className={css.promptDiffSection}>
      <h3 className={css.promptDiffTitle}>{title}</h3>
      <pre className={css.promptDiff}>
        {lines.map((line, index) => (
          <span className={css[`promptDiffLine${line.kind}`]} key={index}>
            {line.text || ' '}
            {'\n'}
          </span>
        ))}
      </pre>
    </section>
  )
}

export function SystemPromptDiff({
  before,
  after,
}: {
  before: ConversationPromptSnapshot
  after: ConversationPromptSnapshot
}) {
  const toolsBefore = JSON.stringify(before.tools, null, 2)
  const toolsAfter = JSON.stringify(after.tools, null, 2)
  return (
    <div className={css.promptDiffSections}>
      {before.system !== after.system && (
        <PromptDiffSection
          title="系统提示词"
          before={before.system}
          after={after.system}
        />
      )}
      {toolsBefore !== toolsAfter && (
        <PromptDiffSection
          title="工具"
          before={toolsBefore}
          after={toolsAfter}
        />
      )}
    </div>
  )
}
