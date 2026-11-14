// TeX typesetting for GPUI. The SVG has no HTML, script, external font or
// network dependency: GPUI paints its paths with the surrounding text color.
import { mathjax } from 'mathjax-full/js/mathjax.js';
import { TeX } from 'mathjax-full/js/input/tex.js';
import { SVG } from 'mathjax-full/js/output/svg.js';
import { liteAdaptor } from 'mathjax-full/js/adaptors/liteAdaptor.js';
import { RegisterHTMLHandler } from 'mathjax-full/js/handlers/html.js';
import { AllPackages } from 'mathjax-full/js/input/tex/AllPackages.js';

const adaptor = liteAdaptor({ fontSize: 16 });
RegisterHTMLHandler(adaptor);
const tex = new TeX({ packages: AllPackages });
const svg = new SVG({ fontCache: 'local' });
const document = mathjax.document('', { InputJax: tex, OutputJax: svg });

/** Return a standalone, currentColor SVG and its layout at a given font size. */
export function renderMath(source, displayMode, fontSize = 14) {
  if (typeof source !== 'string' || !source.trim()) return null;
  try {
    const container = document.convert(source, {
      display: displayMode,
      em: fontSize,
      ex: fontSize / 2,
      containerWidth: 1200,
      lineWidth: 1200,
    });
    const paths = adaptor.tags(container, 'svg')[0];
    if (!paths) return null;
    const markup = adaptor.outerHTML(paths);
    if (markup.includes('data-mjx-error=')) return null;
    // MathJax emits ex units, while GPUI accepts absolute pixel dimensions.
    const units = value => Number.parseFloat(value) * fontSize / 2;
    const width = units(adaptor.getAttribute(paths, 'width'));
    const height = units(adaptor.getAttribute(paths, 'height'));
    const style = adaptor.getAttribute(paths, 'style') || '';
    const depth = Number.parseFloat(style.match(/vertical-align:\s*([-\d.]+)ex/)?.[1] || 0);
    if (!(width > 0 && height > 0) || !markup.includes('<svg')) return null;
    return {
      svg: markup,
      width,
      height,
      baselineOffset: -depth * fontSize / 2,
    };
  } catch {
    // Incomplete TeX is ordinary Markdown text until it can be typeset.
    return null;
  }
}
