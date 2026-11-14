import assert from 'node:assert/strict';
import test from 'node:test';
import { renderMath } from '../ui/document-math.js';

test('typesets inline and display TeX as standalone vector paths', () => {
  for (const [source, display] of [['E = mc^2', false], ['\\frac{a+b}{2}', true], ['\\sqrt{x^2 + y^2}', true]]) {
    const result = renderMath(source, display);
    assert.ok(result, source);
    assert.match(result.svg, /^<svg[^>]*xmlns="http:\/\/www.w3.org\/2000\/svg"/);
    assert.match(result.svg, /<path/);
    assert.match(result.svg, /currentColor/);
    assert.doesNotMatch(result.svg, /<script|<foreignObject|(?:href|src)="https?:/);
    assert.ok(result.width > 0 && result.height > 0);
    assert.ok(Number.isFinite(result.baselineOffset));
  }
  const inline = renderMath('E = mc^2', false, 14);
  const fraction = renderMath('\\frac{a+b}{2}', true, 14);
  assert.equal(inline.width, 8.699 * 7);
  assert.equal(inline.height, 2.072 * 7);
  assert.equal(fraction.height, 4.652 * 7);
  assert.equal(inline.baselineOffset, 0.186 * 7);
});

test('supports ordinary TeX packages and leaves invalid or empty input inert', () => {
  const result = renderMath('\\color{red}{x} \\in \\mathbb{R}', false);
  assert.ok(result);
  assert.equal(renderMath('\\frac{1', false), null);
  assert.equal(renderMath('  ', false), null);
});
