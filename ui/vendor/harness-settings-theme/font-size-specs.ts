/** Font-size contract from upstream ui-theme/theme-settings.ts. */
export type FontRole = 'text' | 'code' | 'terminal';
export const FONT_SIZE_SPECS = {
  text: { field: 'fontSize', variable: '--dsh-content-font-size', min: 10, max: 22, default: 14 },
  code: { field: 'codeFontSize', variable: '--dsh-code-font-size', min: 10, max: 16, default: 11 },
  terminal: { field: 'terminalFontSize', variable: '--dsh-terminal-font-size', min: 10, max: 20, default: 13 },
} as const;

/** CSS Fonts generic and system family keywords, which must stay unquoted. */
const GENERIC_FAMILIES = new Set([
  'serif', 'sans-serif', 'monospace', 'cursive', 'fantasy', 'system-ui', 'ui-serif', 'ui-sans-serif',
  'ui-monospace', 'ui-rounded', 'emoji', 'math', 'fangsong', '-apple-system', 'blinkmacsystemfont',
])

/**
 * Normalize user input into a CSS font-family list. Names are comma separated;
 * quotes, backslashes, angle brackets, and control characters are dropped, and
 * every non-generic name is double-quoted, so the result is always a valid list
 * safe to embed in inline CSS and boot scripts. Normalizing a result returns it unchanged.
 * @param value - user-entered or stored font list.
 * @returns the normalized list, or `''` when no family name remains.
 */
export function normalizeFontFamily(value: string): string {
  return value.split(',')
    .map(name => name.replace(/["'\\<>\u0000-\u001f\u007f]/gu, '').replace(/\s+/gu, ' ').trim())
    .filter(name => name !== '')
    .map(name => GENERIC_FAMILIES.has(name.toLowerCase()) ? name : `"${name}"`)
    .join(', ')
}
