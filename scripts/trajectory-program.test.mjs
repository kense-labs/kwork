import assert from 'node:assert/strict';
import { test } from 'bun:test';
import { codeProgram } from '../ui/vendor/harness-trajectory/code-program.ts';

const cell = schema => ({ kind: 'tool', toolName: 'run_code',
  inputDetail: '{ "code": "\\nconst x = 1;\\n", "description": "  " }', schemaDetail: JSON.stringify(schema) });

test('program language comes exclusively from the recorded schema', () => {
  assert.equal(codeProgram(cell({ description: 'Execute a program in `TypeScript`' })).language, 'typescript');
  assert.equal(codeProgram(cell({ parameters: { properties: { code: { description: 'Python source' } } } })).language, 'python');
  assert.equal(codeProgram(cell({ description: 'Execute a program in `ruby`', parameters: { properties: { code: { description: 'TypeScript source' } } } })).language, undefined);
  assert.equal(codeProgram(cell({ description: 'Execute a program in `TypeScript`', parameters: { properties: { code: { description: 'Python source' } } } })).language, undefined);
  assert.equal(codeProgram(cell({})).language, undefined, 'Recognizable source is never a language hint');
});

test('source and original JSON are preserved while summaries use the official fallback', () => {
  const input = cell({});
  const program = codeProgram(input);
  assert.equal(program.rawInput, input.inputDetail);
  assert.equal(program.source, '\nconst x = 1;\n');
  assert.equal(program.description, 'const x = 1;');
  assert.equal(codeProgram({ ...input, inputDetail: '{"code":"ok","description":null}' }), undefined);
  assert.equal(codeProgram({ ...input, toolName: 'bash' }), undefined);
  assert.equal(codeProgram({ ...input, kind: 'message' }), undefined);
});
