// A native form over the measured content area of the GPUI settings panel.
let dispatch;
let controls;
let state;
let bounds;

export function listen_model_provider_form(callback) {
  dispatch = callback;
}

export function model_provider_logo(bytes) {
  return URL.createObjectURL(new Blob([bytes], { type: 'image/png' }));
}

export function provider_endpoint_origin(value, protocol) {
  try {
    const url = new URL(value);
    if (!/^https?:\/\//i.test(value) || /\s/.test(value) || url.username || url.password || /[?#]/.test(url.href)) return;
    const path = url.pathname.replace(/\/+$/, '');
    if (/\/(?:chat\/completions|responses|models)$/.test(path) ||
        protocol === 'anthropic-messages' && /\/v1\/messages$/.test(path) ||
        protocol === 'google-generative-ai' && /\/models\/[^/]+:(?:streamGenerateContent|generateContent)$/.test(path)) return;
    return url.origin;
  } catch {
    return undefined;
  }
}

export function place_model_provider_form(left, top, width, height) {
  if (bounds?.left === left && bounds.top === top && bounds.width === width && bounds.height === height) return;
  bounds = { left, top, width, height };
  if (controls) place();
}

function place() {
  if (!bounds || !state) return;
  for (const [property, value] of Object.entries(bounds)) controls.root.style[property] = `${value}px`;
}

function setRevealed(revealed) {
  const { key, reveal } = controls;
  controls.revealedDraft = revealed ? key.value : undefined;
  key.type = revealed ? 'text' : 'password';
  reveal.textContent = revealed ? '隐藏' : '显示';
  reveal.setAttribute('aria-label', revealed ? '隐藏服务商密钥' : '显示服务商密钥');
  reveal.setAttribute('aria-pressed', String(revealed));
}

function readField(input) {
  if (input.composing || input.value === input.sent) return;
  input.sent = input.value;
  if (input === controls.key && input.type === 'text') setRevealed(false);
  if (input.dataset.field === 'picker-search') dispatch?.('provider-picker-search', input.value);
  else dispatch?.('provider-field', JSON.stringify([input.dataset.field, input.value]));
}

function readFields() {
  if (!state || state.busy || state.confirmRemove) return;
  if (state.picker) {
    readField(controls.fields['picker-search']);
    return;
  }
  for (const input of Object.values(controls.fields)) {
    if (input.dataset.field !== 'picker-search') readField(input);
  }
  const active = document.activeElement;
  if (active?.matches('input[data-model-field]')) readModelField(active);
  if (active?.matches('input[data-reasoning-wire]')) readReasoningWire(active);
  const { key, reveal } = controls;
  if (key.type === 'text' && controls.revealedDraft !== key.value) setRevealed(false);
  const hide = key.value === '';
  if (reveal.hidden !== hide) reveal.hidden = hide;
}

function send(action, value = '') {
  readFields();
  dispatch?.(action, value);
}

function setField(field, value) {
  const input = controls.fields[field];
  if (input.backend === value) return;
  input.backend = value;
  if (!input.composing && input.value !== value) input.value = value;
  input.sent = value;
}

function catalogAuthHint(catalog, keyConfigured) {
  if (keyConfigured) return null;
  if (catalog.oauthConfigured) return '已检测到 Host 的服务商登录凭据。';
  if (catalog.signedIn) return '此服务商已登录。';
  if (catalog.oauth && !catalog.apiKeyAllowed) return '此服务商使用 Host 的原生登录认证。';
  if (!catalog.authEnv.length) return null;
  const names = catalog.authEnv.join(' / ');
  return catalog.authDetected ? `已检测到环境认证 ${names}。` : `可通过 ${names} 提供环境认证。`;
}

function showProviderDirectory(open) {
  controls.actions['toggle-directory'].setAttribute('aria-expanded', String(open));
  controls.lists.directory.hidden = !open;
}

function rowButtons(container, items, action) {
  const stamp = JSON.stringify(items);
  if (container.stamp === stamp) return;
  container.stamp = stamp;
  const focused = container.contains(document.activeElement) ? document.activeElement.dataset.choice : undefined;
  container.replaceChildren(...items.map(item => {
    const button = document.createElement('button');
    button.type = 'button';
    button.dataset.choice = item.id;
    button.setAttribute('aria-pressed', String(item.selected));
    if (item.logo) {
      const mark = document.createElement('img');
      mark.src = item.logo;
      mark.alt = '';
      button.append(mark);
    }
    button.append(item.name);
    button.onclick = () => send(action, item.id);
    return button;
  }));
  if (focused !== undefined) {
    const same = [...container.children].find(button => button.dataset.choice === focused);
    (same ?? container.firstElementChild ?? controls.heading).focus({ preventScroll: true });
  }
}

function readModelField(input) {
  if (!state || state.busy || state.confirmRemove || input.composing || input.value === input.sent) return;
  input.sent = input.value;
  const key = Number(input.closest('[data-model-key]').dataset.modelKey);
  dispatch?.('provider-model-field', JSON.stringify([key, input.dataset.modelField, input.value]));
}

function readReasoningWire(input) {
  if (!state || state.busy || state.confirmRemove || state.picker || input.disabled ||
      input.composing || input.value === input.sent) return;
  input.sent = input.value;
  input.sendValue(input.value);
}

function createReasoningEditor(entry, key) {
  const editor = entry.querySelector('.provider-reasoning');
  const toggle = editor.querySelector('[role="switch"]');
  const advanced = editor.querySelector('[data-reasoning-levels]');
  const list = editor.querySelector('[data-reasoning-list]');
  const add = editor.querySelector('[data-reasoning-add]');
  const help = editor.querySelector('[data-reasoning-help]');
  const explanation = editor.querySelector('[data-reasoning-explanation]');
  explanation.id = `provider-reasoning-help-${key}`;
  help.setAttribute('aria-controls', explanation.id);
  help.onclick = () => {
    explanation.hidden = !explanation.hidden;
    help.setAttribute('aria-expanded', String(!explanation.hidden));
  };
  const edit = action => send('provider-model-reasoning', JSON.stringify([key, action]));
  toggle.onchange = () => edit({ action: 'toggle', enabled: toggle.checked });
  add.onclick = () => edit({ action: 'add' });
  return { editor, toggle, advanced, list, add, edit, explanation, modelKey: key, rows: new Map() };
}

function createReasoningRow(editor, key) {
  const entry = document.createElement('div');
  entry.className = 'provider-reasoning-row';
  entry.innerHTML = `<select></select><input type="text" data-reasoning-wire spellcheck="false">
    <button type="button" data-remove>移除</button>`;
  const select = entry.querySelector('select');
  const input = entry.querySelector('input');
  const remove = entry.querySelector('button');
  const row = { entry, select, input, remove };
  select.onchange = () => editor.edit({ action: 'level', key, level: select.value });
  input.sendValue = value => dispatch?.('provider-model-reasoning', JSON.stringify([
    editor.modelKey, { action: 'wire', key, value },
  ]));
  input.addEventListener('compositionstart', () => { input.composing = true; });
  input.addEventListener('compositionend', () => { input.composing = false; readReasoningWire(input); });
  input.addEventListener('input', () => readReasoningWire(input));
  input.addEventListener('blur', () => {
    if (input.disabled || input.composing || !row.level) return;
    readReasoningWire(input);
    if (input.value !== input.backend) input.value = input.sent = input.value.trim() || row.level;
  });
  remove.onclick = () => editor.edit({ action: 'remove', key });
  return row;
}

function updateReasoning(editor, model, position, catalog, fixedValues, busy, issue) {
  const active = document.activeElement;
  const { toggle, advanced, rows, add } = editor;
  const label = `推理等级 ${position}`;
  editor.editor.setAttribute('aria-label', label);
  toggle.setAttribute('aria-label', label);
  const explanation = fixedValues
    ? 'Gemini API 支持 minimal、low、medium 和 high，API 参数值与档位名相同。'
    : 'API 参数值默认与档位名相同，可按服务商文档修改。Off 表示关闭思考。配置中已有的 Off 参数值会保留。';
  if (editor.explanation.textContent !== explanation) editor.explanation.textContent = explanation;
  if (model.issue?.field === 'reasoning') toggle.setAttribute('aria-describedby', issue.id);
  else toggle.removeAttribute('aria-describedby');
  toggle.checked = Boolean(model.reasoning);
  toggle.disabled = busy;
  advanced.hidden = !model.reasoning;
  const levels = model.reasoning ?? [];
  const used = new Set(levels.map(row => row.level));
  add.disabled = busy || catalog.every(level => used.has(level));
  const inList = editor.list.contains(active);
  const present = new Set(levels.map(row => row.key));
  for (const [key, row] of rows) {
    if (present.has(key)) continue;
    row.entry.remove();
    rows.delete(key);
  }
  let before = editor.list.firstElementChild;
  for (const [index, { key, level, wire }] of levels.entries()) {
    let row = rows.get(key);
    if (!row) {
      row = createReasoningRow(editor, key);
      rows.set(key, row);
    }
    const { entry, select, input, remove } = row;
    if (entry !== before) editor.list.insertBefore(entry, before);
    before = entry.nextElementSibling;
    const choices = catalog.filter(choice => choice === level || !used.has(choice));
    if (!catalog.includes(level)) choices.unshift(level);
    const stamp = choices.join(',');
    if (select.stamp !== stamp) {
      select.stamp = stamp;
      select.replaceChildren(...choices.map(choice => new Option(choice, choice)));
    }
    if (select.value !== level) select.value = level;
    select.disabled = remove.disabled = busy;
    select.setAttribute('aria-label', `档位 ${index + 1}`);
    remove.setAttribute('aria-label', `删除档位 ${level}`);
    input.setAttribute('aria-label', `${level} API 参数值`);
    input.placeholder = level === 'off' ? '不发送参数' : level;
    input.disabled = busy || level === 'off' || fixedValues;
    const invalid = fixedValues ? !catalog.includes(level) || wire !== level
      : wire == null ? level !== 'off' : wire.length === 0;
    input.setAttribute('aria-invalid', String(invalid));
    if (input.backend !== (wire ?? '') || row.level !== level) {
      input.backend = wire ?? '';
      if (row.level !== level || active !== input && !input.composing) input.value = input.sent = input.backend;
    }
    row.level = level;
  }
  if (inList && !active.isConnected && !busy) (model.reasoning && !add.disabled ? add : toggle).focus({ preventScroll: true });
}

function createModelRow(key) {
  const entry = document.createElement('div');
  entry.className = 'provider-model-entry';
  entry.dataset.modelKey = key;
  entry.innerHTML = `
    <div class="provider-model-row">
      <button type="button" data-order aria-keyshortcuts="ArrowUp ArrowDown" title="拖动或按上下方向键排序">↕</button>
      <input type="text" data-model-field="id" placeholder="模型 ID" spellcheck="false">
      <button type="button" data-advanced aria-expanded="false">高级</button>
      <button type="button" data-remove>移除</button>
    </div>
    <div class="provider-model-advanced" hidden>
      <label>显示名称<input type="text" data-model-field="name" placeholder="继承模型名称"></label>
      <div class="provider-model-capacities">
        <label>上下文窗口<input type="text" data-model-field="contextWindow" placeholder="1M" spellcheck="false"></label>
        <label>最大输出令牌数<input type="text" data-model-field="maxTokens" placeholder="128K" spellcheck="false"></label>
      </div>
      <fieldset>
        <legend>输入类型</legend>
        <label><input type="checkbox" value="text">文字</label>
        <label><input type="checkbox" value="image">图片</label>
      </fieldset>
      <div class="provider-reasoning" role="group">
        <div class="provider-reasoning-head">
          <label><input type="checkbox" role="switch">推理等级</label>
          <button type="button" data-reasoning-help aria-label="推理等级说明" aria-expanded="false">?</button>
        </div>
        <p data-reasoning-explanation hidden></p>
        <div data-reasoning-levels hidden>
          <div class="provider-reasoning-columns" aria-hidden="true"><span>档位</span><span>API 参数值</span></div>
          <div data-reasoning-list></div>
          <button type="button" data-reasoning-add>添加档位</button>
        </div>
      </div>
    </div>
    <p role="alert" hidden></p>`;
  const fields = Object.fromEntries([...entry.querySelectorAll('[data-model-field]')]
    .map(input => [input.dataset.modelField, input]));
  const order = entry.querySelector('[data-order]');
  const advanced = entry.querySelector('[data-advanced]');
  const details = entry.querySelector('.provider-model-advanced');
  const remove = entry.querySelector('[data-remove]');
  const issue = entry.querySelector('[role="alert"]');
  const modalities = [...entry.querySelectorAll('fieldset [type="checkbox"]')];
  const reasoning = createReasoningEditor(entry, key);
  details.id = `provider-model-advanced-${key}`;
  issue.id = `provider-model-error-${key}`;
  advanced.setAttribute('aria-controls', details.id);
  advanced.onclick = () => {
    details.hidden = !details.hidden;
    advanced.setAttribute('aria-expanded', String(!details.hidden));
  };
  remove.onclick = () => send('provider-remove-model', String(key));
  for (const input of Object.values(fields)) {
    input.addEventListener('compositionstart', () => { input.composing = true; });
    input.addEventListener('compositionend', () => { input.composing = false; readModelField(input); });
    input.addEventListener('input', () => readModelField(input));
    input.addEventListener('blur', () => readModelField(input));
  }
  for (const input of modalities) {
    input.onchange = () => send('provider-model-field', JSON.stringify([key, input.value, String(input.checked)]));
  }
  const move = (source, target, after) => send('provider-model-order', JSON.stringify([source, target, after]));
  order.onkeydown = event => {
    if (event.key !== 'ArrowDown' && event.key !== 'ArrowUp') return;
    event.preventDefault();
    event.stopPropagation();
    const after = event.key === 'ArrowDown';
    const sibling = after ? entry.nextElementSibling : entry.previousElementSibling;
    if (sibling) move(key, Number(sibling.dataset.modelKey), after);
  };
  order.ondragstart = event => {
    controls.dragModel = key;
    event.dataTransfer.effectAllowed = 'move';
    event.dataTransfer.setData('application/x-kwork-model-order', String(key));
  };
  order.ondragend = () => {
    controls.dragModel = undefined;
    controls.lists.models.querySelector('[data-drop]')?.removeAttribute('data-drop');
  };
  const half = event => {
    const bounds = entry.getBoundingClientRect();
    return event.clientY >= bounds.top + bounds.height / 2;
  };
  entry.ondragover = event => {
    if (controls.dragModel === undefined || controls.dragModel === key) return;
    event.preventDefault();
    event.dataTransfer.dropEffect = 'move';
    entry.dataset.drop = half(event) ? 'after' : 'before';
  };
  entry.ondragleave = event => {
    if (!(event.relatedTarget instanceof Node) || !entry.contains(event.relatedTarget)) delete entry.dataset.drop;
  };
  entry.ondrop = event => {
    if (controls.dragModel === undefined || controls.dragModel === key) return;
    event.preventDefault();
    delete entry.dataset.drop;
    move(controls.dragModel, key, half(event));
  };
  return { entry, fields, order, advanced, remove, issue, modalities, reasoning };
}

function updateModelRows(models, reasoningLevels, fixedReasoningValues, busy, readOnly, catalog) {
  const { modelRows, lists, actions } = controls;
  const locked = busy || readOnly;
  const active = document.activeElement;
  const activePosition = [...lists.models.children].findIndex(entry => entry.contains(active));
  const present = new Set(models.map(model => model.key));
  for (const [key, row] of modelRows) {
    if (present.has(key)) continue;
    row.entry.remove();
    modelRows.delete(key);
  }

  let before = lists.models.firstElementChild;
  for (const [index, model] of models.entries()) {
    let row = modelRows.get(model.key);
    if (!row) {
      row = createModelRow(model.key);
      modelRows.set(model.key, row);
    }
    const { entry, fields, order, advanced, remove, issue, modalities } = row;
    if (entry !== before) lists.models.insertBefore(entry, before);
    before = entry.nextElementSibling;
    const reference = model.id.trim() || `${index + 1}`;
    order.setAttribute('aria-label', `排序模型 ${reference}`);
    advanced.setAttribute('aria-label', `模型高级参数 ${reference}`);
    remove.setAttribute('aria-label', `移除 ${model.name.trim() || reference}`);
    for (const [field, label] of Object.entries({ id: '模型 ID', name: '显示名称', contextWindow: '上下文窗口', maxTokens: '最大输出令牌数' })) {
      const input = fields[field];
      input.setAttribute('aria-label', `${label} ${index + 1}`);
      input.disabled = locked;
      if (input.backend !== model[field]) {
        input.backend = model[field];
        if (!input.composing && input.value !== model[field]) input.value = model[field];
        input.sent = model[field];
      }
      input.setAttribute('aria-invalid', String(model.issue?.field === field));
      if (model.issue?.field === field) input.setAttribute('aria-describedby', issue.id);
      else input.removeAttribute('aria-describedby');
    }
    showText(issue, model.issue?.message);
    order.draggable = !locked;
    order.disabled = remove.disabled = locked;
    advanced.disabled = busy;
    for (const input of modalities) {
      input.checked = model[input.value];
      input.disabled = locked || input.checked && !(model.text && model.image);
    }
    row.reasoning.editor.hidden = catalog;
    if (!catalog) updateReasoning(row.reasoning, model, index + 1, reasoningLevels, fixedReasoningValues, locked, issue);
  }
  if (!busy && activePosition >= 0 && !lists.models.contains(document.activeElement)) {
    const next = active.isConnected ? active : lists.models.children[activePosition]?.querySelector('input') ?? actions['add-model'];
    next.focus({ preventScroll: true });
  }
}

function updatePicker(picker) {
  const { lists, candidateRows, actions, parts } = controls;
  const candidates = picker?.candidates ?? [];
  const visible = new Set(candidates.map(candidate => candidate.id));
  for (const [id, row] of candidateRows) {
    if (visible.has(id)) continue;
    row.item.remove();
    candidateRows.delete(id);
  }
  if (!picker) {
    const search = controls.fields['picker-search'];
    search.composing = false;
    if (search.value !== '') search.value = '';
    search.backend = search.sent = '';
    return;
  }
  setField('picker-search', picker.query);
  let before = lists.candidates.firstElementChild;
  for (const candidate of candidates) {
    let row = candidateRows.get(candidate.id);
    if (!row) {
      const item = document.createElement('li');
      const label = document.createElement('label');
      const checkbox = document.createElement('input');
      checkbox.type = 'checkbox';
      checkbox.onchange = () => send('provider-picker-toggle', candidate.id);
      const id = document.createElement('span');
      id.textContent = candidate.id;
      id.title = candidate.name;
      label.append(checkbox, id);
      item.append(label);
      row = { item, checkbox };
      candidateRows.set(candidate.id, row);
    }
    if (row.item !== before) lists.candidates.insertBefore(row.item, before);
    before = row.item.nextElementSibling;
    row.checkbox.checked = candidate.selected;
  }
  lists.candidates.hidden = !candidates.length;
  parts['picker-empty'].hidden = Boolean(candidates.length);
  actions['picker-bulk'].textContent = picker.allVisibleSelected ? '取消全选' : '全选';
  actions['picker-bulk'].disabled = !candidates.length;
  actions['picker-apply'].disabled = Boolean(picker.issue);
  showText(parts['picker-issue'], picker.issue);
}

const retryCodeLabels = new Map([
  ['EMPTY_RESPONSE', '空响应'], ['MALFORMED_RESPONSE', '响应格式错误'], ['QUOTA', '配额已用尽'],
  ['RATE_LIMIT', '请求限流'], ['SERVER', '服务器错误'], ['STREAM_CLOSED', '流提前结束'],
  ['TIMEOUT', '超时'], ['TRANSPORT', '网络错误'],
]);

function addRetryCode() {
  const input = controls.fields['retry-custom-code'];
  if (!state || state.busy || state.picker || state.confirmRemove ||
      state.retry?.mode !== 'normal' || input.composing || !input.value.trim()) return;
  send('provider-retry-add-code');
  input.value = input.sent = input.backend = '';
}

function updateRetry(retry, reset, busy) {
  const { retryModes, parts, actions, fields, lists, retryCodes } = controls;
  parts.retry.hidden = !retry;
  if (!retry) return;
  const mode = retry.mode ?? '';
  const normal = mode === 'normal';
  const modeLabels = { normal: '仅临时错误', always: '所有错误' };
  retryModes.options[0].textContent = `默认（${modeLabels[retry.inheritedMode]}）`;
  if (retryModes.value !== mode) retryModes.value = mode;
  parts['retry-normal'].hidden = !normal;
  parts['retry-always'].hidden = mode !== 'always';
  const backoff = parts['retry-backoff'];
  backoff.hidden = !mode;
  if (reset) backoff.open = retry.hasCustomBackoff;
  for (const [key, number] of Object.entries({
    'retry-max-retries': retry.maxRetries, 'retry-initial-delay': retry.initialDelayMs,
    'retry-max-delay': retry.maxDelayMs, 'retry-jitter': retry.jitterRatio,
  })) {
    const input = fields[key];
    if (reset) input.backend = undefined;
    setField(key, number.text);
    input.placeholder = number.placeholder;
    const invalid = retry.issue?.field === key;
    input.setAttribute('aria-invalid', String(invalid));
    if (invalid) input.setAttribute('aria-describedby', 'provider-retry-issue');
    else input.removeAttribute('aria-describedby');
  }
  showText(parts['retry-issue'], retry.issue?.message);
  if (reset) fields['retry-custom-code'].backend = undefined;
  setField('retry-custom-code', retry.customCode);
  actions['retry-add-code'].disabled = busy || !retry.customCode.trim();
  const live = new Set(retry.codes.map(item => item.code));
  for (const [code, row] of retryCodes) {
    if (live.has(code)) continue;
    row.label.remove();
    retryCodes.delete(code);
  }
  const selected = retry.codes.filter(item => item.selected).length;
  let before = lists['retry-codes'].firstElementChild;
  for (const { code, selected: checked } of retry.codes) {
    let row = retryCodes.get(code);
    if (!row) {
      const label = document.createElement('label');
      const input = document.createElement('input');
      input.type = 'checkbox';
      input.onchange = () => send('provider-retry-code', JSON.stringify([code, input.checked]));
      label.title = code;
      label.append(input, retryCodeLabels.get(code) ?? code);
      row = { label, input };
      retryCodes.set(code, row);
    }
    if (row.label !== before) lists['retry-codes'].insertBefore(row.label, before);
    before = row.label.nextElementSibling;
    row.input.checked = checked;
    row.input.disabled = busy || checked && selected === 1;
  }
}

function mount() {
  const root = document.createElement('section');
  root.id = 'model-provider-form';
  root.setAttribute('aria-labelledby', 'model-provider-heading');
  root.innerHTML = `
    <div class="provider-main">
      <header class="provider-title">
        <h2 id="model-provider-heading" tabindex="-1">模型服务</h2>
        <button type="button" data-action="close" aria-label="关闭设置">×</button>
      </header>
      <div class="provider-panes">
        <aside aria-label="服务商">
          <input type="search" aria-label="搜索服务商" data-field="provider-search" placeholder="搜索服务商">
          <div class="provider-list">
            <div data-group="saved" role="group" aria-label="已配置的服务商">
              <h3>已配置</h3><div data-list="saved"></div>
            </div>
            <div data-group="presets" role="group" aria-label="常用服务商">
              <h3>常用服务商</h3><div data-list="presets"></div>
            </div>
            <div data-group="directory" class="provider-directory" hidden>
              <button type="button" data-action="toggle-directory" aria-controls="provider-directory-list" aria-expanded="false">第三方模型服务商</button>
              <div id="provider-directory-list" data-list="directory" role="group" aria-label="可添加的目录服务商" hidden></div>
            </div>
            <p data-part="no-providers">没有匹配的服务商</p>
          </div>
          <button type="button" data-action="new">添加服务商</button>
          <button type="button" data-action="reload">重新加载</button>
        </aside>
        <form class="provider-details" novalidate>
          <div class="provider-fields">
            <h3 data-part="editor-heading"></h3>
            <p data-part="deployment-note" hidden>此服务商由启动参数或全局配置管理。</p>
            <p data-part="catalog-diagnostic" role="alert" hidden></p>
            <label data-part="provider-name">服务商名称<input type="text" data-field="name" placeholder="例如 我的模型网关"></label>
            <label>API 地址<input type="url" data-field="url" placeholder="https://example.com/v1" aria-describedby="provider-url-error provider-endpoint-hint"></label>
            <p id="provider-url-error" role="alert" hidden>请输入有效的 HTTP 或 HTTPS 基础地址，保留版本路径，省略请求方法和查询参数。</p>
            <p id="provider-endpoint-hint" data-part="endpoint-hint" hidden></p>
            <label data-part="protocol-field">接口协议
              <select aria-label="接口协议"></select>
            </label>
            <div class="provider-key" data-part="key-field">
              <label><span data-part="key-label">API Key</span>
                <input type="password" data-field="key" autocomplete="new-password" autocapitalize="none" spellcheck="false" placeholder="输入 API Key">
              </label>
              <button type="button" data-action="reveal" hidden></button>
              <button type="button" data-action="clear-key" hidden>清除已保存密钥</button>
            </div>
            <p id="provider-key-error" role="alert" data-part="key-error" hidden>请输入当前 Gemini API 地址的密钥。</p>
            <p data-part="catalog-auth" id="provider-catalog-auth" hidden></p>
            <div class="provider-login-actions" data-part="login-actions" hidden>
              <button type="button" data-action="auth-open">登录服务商</button>
              <button type="button" data-action="auth-request-logout">退出服务商登录</button>
            </div>
            <section class="provider-retry" data-part="retry" aria-label="重试策略" hidden>
              <label>重试策略<select data-retry-mode>
                <option value="">默认（仅临时错误）</option>
                <option value="normal">仅临时错误</option>
                <option value="always">所有错误</option>
              </select></label>
              <p data-part="retry-always" hidden>失败的请求会持续重试，直到成功或本轮被停止。</p>
              <div data-part="retry-normal" hidden>
                <label>最大重试次数<input type="text" inputmode="numeric" data-field="retry-max-retries"></label>
                <fieldset>
                  <legend>错误码</legend>
                  <div data-list="retry-codes"></div>
                  <div class="provider-retry-custom">
                    <input type="text" data-field="retry-custom-code" aria-label="自定义错误码" placeholder="自定义错误码" spellcheck="false">
                    <button type="button" data-action="retry-add-code">添加错误码</button>
                  </div>
                </fieldset>
              </div>
              <details data-part="retry-backoff" hidden>
                <summary>退避</summary>
                <div>
                  <label>初始延迟 (ms)<input type="text" inputmode="decimal" data-field="retry-initial-delay"></label>
                  <label>最大延迟 (ms)<input type="text" inputmode="decimal" data-field="retry-max-delay"></label>
                  <label>抖动比例<input type="text" inputmode="decimal" data-field="retry-jitter"></label>
                </div>
              </details>
              <p id="provider-retry-issue" role="alert" data-part="retry-issue" hidden></p>
            </section>
            <section class="provider-models" aria-label="模型">
              <div class="provider-title">
                <h3>模型</h3>
                <button type="button" data-action="discover">同步模型</button>
                <button type="button" data-action="add-model">手动添加</button>
                <button type="button" data-action="reset-models" hidden>恢复默认模型</button>
              </div>
              <div data-group="models" role="group" aria-label="已添加的模型"><div data-list="models"></div></div>
              <p data-part="no-models"></p>
            </section>
          </div>
          <p role="status" data-part="feedback" hidden></p>
          <footer>
            <button type="button" data-action="ask-remove" hidden>删除服务商</button>
            <button type="submit" data-action="save" disabled>保存服务商</button>
          </footer>
        </form>
      </div>
    </div>
    <div class="provider-dialog-layer" data-layer="remove" hidden>
      <section role="alertdialog" aria-modal="true" aria-labelledby="provider-remove-title" aria-describedby="provider-remove-description">
        <h3 id="provider-remove-title"></h3>
        <p id="provider-remove-description">该服务商将从模型列表移除。</p>
        <p role="alert" data-part="remove-feedback" hidden></p>
        <footer>
          <button type="button" data-action="cancel-remove">取消</button>
          <button type="button" data-action="remove">确认删除</button>
        </footer>
      </section>
    </div>
    <div class="provider-dialog-layer" data-layer="picker" hidden>
      <section class="provider-picker-dialog" role="dialog" aria-modal="true" aria-labelledby="provider-picker-title" aria-describedby="provider-picker-description">
        <header class="provider-title">
          <h3 id="provider-picker-title">选择模型</h3>
          <button type="button" data-action="picker-close" aria-label="关闭模型选择">×</button>
        </header>
        <p id="provider-picker-description">勾选同步结果并更新模型列表。服务商未列出的现有模型会保留。</p>
        <div class="provider-picker-toolbar">
          <input type="search" data-field="picker-search" aria-label="搜索模型" placeholder="搜索模型">
          <button type="button" data-action="picker-bulk" title="仅更改当前筛选结果">全选</button>
        </div>
        <div class="provider-picker-results">
          <ul data-list="candidates" aria-label="同步结果"></ul>
          <p role="status" data-part="picker-empty" hidden>没有匹配的模型。</p>
        </div>
        <p role="alert" data-part="picker-issue" hidden></p>
        <footer>
          <button type="button" data-action="picker-cancel">取消</button>
          <button type="button" data-action="picker-apply">更新列表</button>
        </footer>
      </section>
    </div>
    <div class="provider-dialog-layer" data-layer="login" hidden>
      <section class="provider-login-dialog" role="dialog" aria-modal="true" aria-labelledby="provider-login-heading">
        <h3 id="provider-login-heading" data-part="login-title" tabindex="-1"></h3>
        <form data-part="login-form">
          <label data-part="login-method-label">登录方式<select data-part="login-method"></select></label>
          <p role="status" data-part="login-status" hidden></p>
          <p data-part="login-instruction" hidden></p>
          <div data-part="login-browser" hidden>
            <button type="button" data-action="auth-open-link">在浏览器中打开</button>
          </div>
          <div class="provider-login-actions" data-part="login-device" hidden>
            <span data-part="login-code"></span>
            <button type="button" data-action="auth-copy-code">复制设备码</button>
          </div>
          <div class="provider-login-answer" data-part="login-answer-label" hidden>
            <span id="provider-login-prompt" data-part="login-prompt"></span>
            <input data-part="login-answer" aria-labelledby="provider-login-prompt" type="text" autocomplete="off" spellcheck="false" autocapitalize="none">
            <select data-part="login-choice" aria-labelledby="provider-login-prompt" hidden></select>
          </div>
          <p role="alert" data-part="login-error" hidden></p>
          <footer>
            <button type="button" data-action="auth-close">取消</button>
            <button type="button" data-action="auth-retry" hidden>重试</button>
            <button type="button" data-action="auth-refresh" hidden>刷新状态</button>
            <button type="submit" data-action="auth-submit">开始登录</button>
          </footer>
        </form>
      </section>
    </div>`;

  const parts = Object.fromEntries([...root.querySelectorAll('[data-part]')].map(el => [el.dataset.part, el]));
  const actions = Object.fromEntries([...root.querySelectorAll('[data-action]')].map(el => [el.dataset.action, el]));
  const fields = Object.fromEntries([...root.querySelectorAll('[data-field]')].map(el => [el.dataset.field, el]));
  const lists = Object.fromEntries([...root.querySelectorAll('[data-list]')].map(el => [el.dataset.list, el]));
  const groups = Object.fromEntries([...root.querySelectorAll('[data-group]')].map(el => [el.dataset.group, el]));
  const heading = root.querySelector('h2');
  const main = root.querySelector('.provider-main');
  const remove = root.querySelector('[data-layer="remove"]');
  const picker = root.querySelector('[data-layer="picker"]');
  const login = root.querySelector('[data-layer="login"]');
  const form = root.querySelector('form');
  const protocol = root.querySelector('select');
  const retryModes = root.querySelector('[data-retry-mode]');
  controls = { root, main, remove, picker, login, heading, parts, actions, fields, lists, groups, protocol,
    retryModes, retryCodes: new Map(), modelRows: new Map(), candidateRows: new Map(), key: fields.key, reveal: actions.reveal };

  for (const [action, button] of Object.entries(actions)) {
    if (action === 'reveal' || action === 'save' || action === 'auth-submit') continue;
    button.onclick = () => send(`provider-${action}`);
  }
  actions['add-model'].onclick = () => {
    controls.focusAdded = true;
    send('provider-add-model');
  };
  actions['picker-close'].onclick = () => send('provider-picker-cancel');
  actions['toggle-directory'].onclick = () => showProviderDirectory(lists.directory.hidden);
  actions['retry-add-code'].onclick = addRetryCode;
  for (const input of Object.values(fields)) {
    input.sent = input.value;
    input.addEventListener('compositionstart', () => { input.composing = true; });
    input.addEventListener('compositionend', () => { input.composing = false; readField(input); });
    input.addEventListener('input', readFields);
  }
  actions.reveal.onclick = () => setRevealed(fields.key.type === 'password');
  protocol.onchange = () => send('provider-protocol', protocol.value);
  retryModes.onchange = () => send('provider-retry-mode', retryModes.value);
  form.onsubmit = event => {
    event.preventDefault();
    if (!state?.busy && !state?.confirmRemove && !state?.picker) send('provider-save');
  };
  parts['login-form'].onsubmit = event => {
    event.preventDefault();
    const dialog = state?.login;
    if (!dialog || dialog.busy) return;
    if (!dialog.attempt) {
      send(dialog.logout ? 'provider-auth-logout' : 'provider-auth-start', parts['login-method'].value);
      return;
    }
    const prompt = dialog.attempt.prompt;
    if (!prompt || dialog.attempt.status !== 'running') return;
    const input = parts[prompt.kind === 'select' ? 'login-choice' : 'login-answer'];
    if (!input.value.trim()) return;
    const response = JSON.stringify([prompt.id, input.value]);
    if (prompt.kind === 'secret') input.value = '';
    send('provider-auth-answer', response);
  };
  root.addEventListener('keydown', event => {
    if (event.isComposing) return;
    if (event.key === 'Escape') {
      event.preventDefault();
      if (!event.repeat) send(state.login ? 'provider-auth-close' : state.picker ? 'provider-picker-cancel'
        : state.confirmRemove ? 'provider-cancel-remove' : 'provider-close');
    } else if (event.key === 'Enter' && event.target === fields['retry-custom-code']) {
      event.preventDefault();
      if (!event.repeat) addRetryCode();
    } else if (event.key === 'Enter' && event.target.matches('input[data-model-field], input[data-reasoning-wire], input[data-field^="retry-"]')) {
      event.preventDefault();
    } else if (event.key === 'Tab') {
      const surface = state.login ? login : state.picker ? picker : state.confirmRemove ? remove : main;
      const items = [...surface.querySelectorAll('button, input, select, summary')]
        .filter(el => !el.disabled && el.getClientRects().length > 0);
      const first = items[0];
      const last = items.at(-1);
      if (items.length > 0 && (event.shiftKey ? document.activeElement === first : document.activeElement === last)) {
        event.preventDefault();
        (event.shiftKey ? last : first).focus();
      }
    }
  });
  remove.addEventListener('click', event => {
    if (event.target === remove) send('provider-cancel-remove');
  });
  picker.addEventListener('click', event => {
    if (event.target === picker) send('provider-picker-cancel');
  });
  setRevealed(false);
  document.body.append(root);
}

function showText(node, message) {
  const text = message ?? '';
  if (node.textContent !== text) node.textContent = text;
  node.hidden = !message;
}

const loginStatuses = {
  authorized: '登录成功。', cancelled: '登录已取消。', failed: '登录失败，请重试。',
  expired: '登录请求已失效，请重新开始。', 'signed-out': '已退出登录。',
};

function updateProviderLogin(dialog, previous) {
  const { login, parts, actions, heading } = controls;
  const answer = parts['login-answer'];
  login.hidden = !dialog;
  if (!dialog) {
    answer.value = '';
    if (previous) {
      const trigger = actions[previous.logout ? 'auth-request-logout' : 'auth-open'];
      (trigger.hidden ? heading : trigger).focus({ preventScroll: true });
    }
    return;
  }
  const attempt = dialog.attempt;
  const prompt = attempt?.prompt;
  const running = attempt?.status === 'running';
  const finished = Boolean(attempt && !running);
  const choosing = !dialog.logout && !attempt;
  const selecting = prompt?.kind === 'select';
  const chooseMethod = parts['login-method'];
  const pickAnswer = parts['login-choice'];
  parts['login-title'].textContent = dialog.logout ? `退出登录 ${dialog.name}` : `登录服务商 ${dialog.name}`;
  parts['login-method-label'].hidden = !choosing;
  if (JSON.stringify(dialog.methods) !== chooseMethod.stamp) {
    chooseMethod.stamp = JSON.stringify(dialog.methods);
    chooseMethod.replaceChildren(...dialog.methods.map(method => new Option(method.label, method.id)));
  }
  const status = attempt ? loginStatuses[attempt.status] ?? attempt.message ?? '等待授权…'
    : dialog.logout ? '退出会移除此服务商在当前 Host 保存的登录凭据。' : dialog.busy ? '正在启动登录…' : null;
  showText(parts['login-status'], status);
  showText(parts['login-instruction'], attempt?.instruction);
  showText(parts['login-code'], attempt?.code ? `设备码 ${attempt.code}` : null);
  showText(parts['login-error'], dialog.error);
  parts['login-browser'].hidden = !attempt?.url;
  parts['login-device'].hidden = !attempt?.code;
  actions['auth-copy-code'].textContent = dialog.copied ? '已复制设备码' : '复制设备码';

  const priorPrompt = previous?.attempt?.prompt;
  parts['login-answer-label'].hidden = !prompt;
  if (prompt?.id !== priorPrompt?.id) {
    answer.value = '';
    pickAnswer.replaceChildren(...(prompt?.options ?? []).map(option => new Option(
      option.description ? `${option.label} · ${option.description}` : option.label, option.id)));
  }
  showText(parts['login-prompt'], prompt?.message);
  answer.hidden = !prompt || selecting;
  answer.required = Boolean(prompt && !selecting);
  answer.type = prompt?.kind === 'secret' ? 'password' : 'text';
  answer.autocomplete = prompt?.kind === 'secret' ? 'new-password' : 'off';
  answer.placeholder = prompt?.placeholder ?? '';
  pickAnswer.hidden = !selecting;
  chooseMethod.disabled = answer.disabled = pickAnswer.disabled = dialog.busy;
  const submit = actions['auth-submit'];
  const close = actions['auth-close'];
  close.textContent = finished ? '完成' : running ? '取消登录' : '取消';
  close.disabled = dialog.busy;
  actions['auth-retry'].hidden = !finished || !['failed', 'expired'].includes(attempt.status);
  actions['auth-refresh'].hidden = !running || dialog.polling;
  actions['auth-refresh'].disabled = dialog.busy;
  submit.hidden = finished || running && !prompt;
  submit.textContent = dialog.busy ? '处理中…' : prompt ? '提交' : dialog.logout ? '确认退出' : '开始登录';
  submit.disabled = dialog.busy;
  if (!previous || (!attempt && previous.attempt)) (dialog.logout ? close : chooseMethod).focus();
  else if (prompt && !dialog.busy && (prompt.id !== priorPrompt?.id || previous.busy)) (selecting ? pickAnswer : answer).focus();
  else if ((finished && previous.attempt?.status !== attempt.status) || (!prompt && priorPrompt)) close.focus();
  else if (dialog.busy && !previous?.busy) parts['login-title'].focus();
}

export function update_model_provider_form(next) {
  const previous = state;
  state = next;
  if (!next) {
    if (controls) {
      controls.root.hidden = true;
      controls.parts['login-answer'].value = '';
      if (controls.key.type !== 'password') setRevealed(false);
      clearInterval(controls.poll);
      bounds = undefined;
    }
    return;
  }
  if (!controls) mount();
  if (!previous) controls.poll = setInterval(readFields, 150);
  const { root, main, remove, picker, heading, parts, actions, fields, lists, groups, protocol, key } = controls;
  root.hidden = false;
  root.dataset.appearance = next.dark ? 'dark' : 'light';
  place();
  main.inert = next.confirmRemove || Boolean(next.picker || next.login);
  remove.hidden = !next.confirmRemove;
  picker.hidden = !next.picker;
  if (controls.keyRevision !== next.keyRevision) {
    controls.keyRevision = next.keyRevision;
    key.value = '';
    key.sent = '';
    setRevealed(false);
  }
  for (const [field, value] of Object.entries({
    'provider-search': next.providerSearch,
    name: next.name, url: next.url,
  })) setField(field, value);
  const protocolLabels = {
    'openai-completions': 'Chat Completions', 'openai-responses': 'Responses',
    'anthropic-messages': 'Anthropic Messages', 'google-generative-ai': 'Gemini API',
  };
  const offered = next.protocols.join(',');
  if (protocol.offered !== offered) {
    protocol.offered = offered;
    protocol.replaceChildren(...next.protocols.map(value => new Option(protocolLabels[value] ?? value, value)));
  }
  if (protocol.value !== next.protocol) protocol.value = next.protocol;
  const catalog = next.catalog;
  const gemini = !catalog && next.protocol === 'google-generative-ai';
  parts['editor-heading'].textContent = next.custom ? '添加模型服务商' : next.name;
  parts['deployment-note'].hidden = !next.readOnly;
  showText(parts['catalog-diagnostic'], catalog?.diagnostic);
  parts['provider-name'].hidden = Boolean(catalog);
  parts['protocol-field'].hidden = Boolean(catalog);
  fields.url.placeholder = catalog ? catalog.defaultBaseUrl || '使用服务商的默认端点' : 'https://example.com/v1';
  showText(parts['endpoint-hint'], catalog && !catalog.defaultBaseUrl && catalog.endpointEnv.length
    ? `默认端点由 ${catalog.endpointEnv.join(' / ')} 配置。` : null);
  parts['key-field'].hidden = catalog?.apiKeyAllowed === false;
  const authHint = catalog ? catalogAuthHint(catalog, next.keyConfigured) : null;
  showText(parts['catalog-auth'], authHint);
  actions['auth-open'].hidden = !catalog?.loginAvailable;
  actions['auth-open'].textContent = catalog?.signedIn ? '重新登录服务商' : '登录服务商';
  actions['auth-request-logout'].hidden = !catalog?.signedIn;
  parts['login-actions'].hidden = actions['auth-open'].hidden && actions['auth-request-logout'].hidden;
  parts['key-label'].textContent = next.clearKey ? 'API Key（保存时清除）'
    : next.keyConfigured ? 'API Key（已保存）' : 'API Key';
  key.setAttribute('aria-required', String(gemini));
  key.setAttribute('aria-invalid', String(next.missingApiKey));
  if (next.missingApiKey) key.setAttribute('aria-describedby', 'provider-key-error');
  else if (authHint) key.setAttribute('aria-describedby', 'provider-catalog-auth');
  else key.removeAttribute('aria-describedby');
  parts['key-error'].hidden = !next.missingApiKey;
  actions['clear-key'].hidden = !next.canClearKey;
  actions['clear-key'].textContent = next.clearKey ? '保留已保存密钥' : '清除已保存密钥';
  actions.reload.hidden = next.loaded && !next.refreshable;
  actions.reload.textContent = next.loaded ? '刷新列表' : '重新加载';
  actions['ask-remove'].hidden = !next.removable;
  actions.discover.textContent = catalog
    ? next.busy === 'discover' ? '读取中…' : '选择目录模型'
    : next.busy === 'discover' ? '同步中…' : '同步模型';
  actions.discover.title = next.clearKey ? '请先保存密钥更改，再同步模型。' : '';
  actions.save.textContent = next.busy === 'save' ? '保存中…' : '保存服务商';
  actions.new.setAttribute('aria-pressed', String(next.custom));
  root.querySelector('#provider-url-error').hidden = !next.invalidUrl;
  fields.url.setAttribute('aria-invalid', String(next.invalidUrl));
  showText(parts.feedback, next.confirmRemove ? null : next.feedback);
  showText(parts['remove-feedback'], next.confirmRemove ? next.feedback : null);
  root.querySelector('#provider-remove-title').textContent = `删除服务商 ${next.name}`;
  const busy = Boolean(next.busy);
  if (busy && !previous?.busy) {
    controls.resumeFocus = root.contains(document.activeElement) ? document.activeElement : null;
  }
  actions.reveal.hidden = !key.value || busy;
  if (busy && key.type === 'text') setRevealed(false);

  rowButtons(lists.saved, next.saved, 'provider-select');
  rowButtons(lists.presets, next.presets, 'provider-preset');
  rowButtons(lists.directory, next.directory, 'provider-select');
  if (next.directory.length && next.providerSearch.trim() && next.providerSearch !== previous?.providerSearch) {
    showProviderDirectory(true);
  }
  for (const [group, node] of Object.entries(groups)) node.hidden = next[group].length === 0;
  parts['no-providers'].hidden = next.saved.length > 0 || next.presets.length > 0 || next.directory.length > 0;
  showText(parts['no-models'], next.models.length ? null : catalog ? '使用完整的内置模型目录。' : '同步服务端列表，或手动输入模型 ID。');
  actions['reset-models'].hidden = !catalog?.canResetModels;
  for (const control of main.querySelectorAll('button, input, select')) {
    if (!control.closest('[data-model-key]')) control.disabled = busy || next.readOnly && Boolean(control.closest('form'));
  }
  updateRetry(next.retry, next.keyRevision !== previous?.keyRevision || next.retry?.mode !== previous?.retry?.mode, busy || next.readOnly);
  updateModelRows(next.models, next.reasoningLevels, gemini, busy, next.readOnly, Boolean(catalog));
  actions.close.disabled = false;
  actions['add-model'].disabled = busy || !next.canAddModel;
  actions.save.disabled = busy || !next.canSave;
  actions.discover.disabled = busy || !next.canDiscover;
  actions['auth-open'].disabled = actions['auth-request-logout'].disabled = busy || Boolean(catalog?.loginRunning);
  actions.remove.disabled = actions['cancel-remove'].disabled = busy;
  updatePicker(next.picker);
  updateProviderLogin(next.login, previous?.login);
  if (next.login || previous?.login) return;
  if (next.picker && !previous?.picker) fields['picker-search'].focus();
  else if (!next.picker && previous?.picker) actions.discover.focus({ preventScroll: true });
  else if (next.confirmRemove && !previous?.confirmRemove) actions['cancel-remove'].focus();
  else if (previous?.confirmRemove && !next.confirmRemove) {
    (actions['ask-remove'].hidden ? heading : actions['ask-remove']).focus();
  } else if (controls.focusAdded && next.models.length > (previous?.models.length ?? 0)) {
    controls.focusAdded = false;
    controls.modelRows.get(next.models.at(-1).key).fields.id.focus();
  }
  else if (previous?.busy && !busy && controls.resumeFocus?.isConnected &&
    !controls.resumeFocus.disabled && !controls.resumeFocus.closest('[hidden]')) {
    controls.resumeFocus.focus({ preventScroll: true });
  }
  else if (!previous) requestAnimationFrame(() => {
    if (state && !root.contains(document.activeElement)) heading.focus();
  });
}
