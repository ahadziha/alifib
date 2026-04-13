import init, { WasmRepl } from './pkg/alifib_wasm.js';

// ── State ─────────────────────────────────────────────────────────────────────

let repl = null;
let sessionActive = false;
const history = [];
let histIdx = -1;

// ── DOM refs ──────────────────────────────────────────────────────────────────

const editor      = document.getElementById('editor');
const btnEval     = document.getElementById('btn-evaluate');
const fileOutput  = document.getElementById('file-output');
const selType     = document.getElementById('sel-type');
const inpSource   = document.getElementById('inp-source');
const inpTarget   = document.getElementById('inp-target');
const btnStart    = document.getElementById('btn-start');
const sessionSetup = document.getElementById('session-setup');
const replOutput  = document.getElementById('repl-output');
const replInput   = document.getElementById('repl-input');
const btnClear    = document.getElementById('btn-clear-repl');
const visInfo     = document.getElementById('vis-info');

// ── Boot ──────────────────────────────────────────────────────────────────────

async function boot() {
  btnEval.disabled = true;
  btnEval.textContent = 'Loading…';
  try {
    await init();
    repl = new WasmRepl();
    btnEval.disabled = false;
    btnEval.textContent = 'Evaluate';
    appendReplMsg('WASM engine ready. Evaluate a file to begin.', 'repl-dim');
  } catch (e) {
    btnEval.textContent = 'Error';
    appendReplMsg('Failed to load WASM: ' + e, 'repl-result err');
  }
}

// ── Evaluate ──────────────────────────────────────────────────────────────────

btnEval.addEventListener('click', evaluateSource);
editor.addEventListener('keydown', e => {
  if ((e.ctrlKey || e.metaKey) && e.key === 'Enter') { e.preventDefault(); evaluateSource(); }
});

function evaluateSource() {
  if (!repl) return;
  const src = editor.value.trim();
  if (!src) return;

  const result = JSON.parse(repl.load_source(src));

  if (result.status === 'error') {
    fileOutput.innerHTML = '';
    fileOutput.hidden = true;
    appendReplEntry('(evaluate)', formatError(result.message));
    sessionSetup.hidden = true;
    resetSession();
    return;
  }

  const types = result.types || [];

  // Build accordion in file output area
  fileOutput.innerHTML = '';
  fileOutput.hidden = types.length === 0;
  types.forEach(t => fileOutput.appendChild(buildTypeAccordion(t)));

  // Populate type selector
  selType.innerHTML = '<option value="">— select type —</option>';
  types.forEach(t => {
    const opt = document.createElement('option');
    opt.value = opt.textContent = t.name;
    selType.appendChild(opt);
  });

  sessionSetup.hidden = false;
  resetSession();
  appendReplEntry('(evaluate)', formatOk(
    types.length
      ? `Loaded ${types.length} type${types.length !== 1 ? 's' : ''}.`
      : 'Loaded (no named types found).'
  ));
}

// ── Accordion builders ───────────────────────────────────────────────────────

function buildTypeAccordion(t) {
  const details = document.createElement('details');
  details.className = 'acc-type';
  const summary = document.createElement('summary');
  summary.textContent = t.name;
  details.appendChild(summary);

  const body = document.createElement('div');
  body.className = 'acc-type-body';

  if (t.generators.length) body.appendChild(buildSection('Generators', t.generators, buildGeneratorItem));
  if (t.diagrams.length)   body.appendChild(buildSection('Diagrams', t.diagrams, buildDiagramItem));
  if (t.maps.length)       body.appendChild(buildSection('Maps', t.maps, buildMapItem));

  details.appendChild(body);
  return details;
}

function buildSection(title, items, buildItem) {
  const details = document.createElement('details');
  details.className = 'acc-section';
  const summary = document.createElement('summary');
  summary.innerHTML = esc(title) + ` <span class="acc-count">${items.length}</span>`;
  details.appendChild(summary);

  const list = document.createElement('div');
  list.className = 'acc-section-body';
  items.forEach(item => list.appendChild(buildItem(item)));
  details.appendChild(list);
  return details;
}

function buildGeneratorItem(g) {
  if (!g.src && !g.tgt) {
    // 0-cell: no boundary to expand
    const div = document.createElement('div');
    div.className = 'acc-leaf';
    div.innerHTML = `${hi(g.name)} <span class="acc-dim">dim ${g.dim}</span>`;
    return div;
  }
  const details = document.createElement('details');
  details.className = 'acc-item';
  const summary = document.createElement('summary');
  summary.innerHTML = `${hi(g.name)} <span class="acc-dim">dim ${g.dim}</span>`;
  details.appendChild(summary);

  const body = document.createElement('div');
  body.className = 'acc-item-body';
  body.innerHTML = `${esc(g.src)} → ${esc(g.tgt)}`;
  details.appendChild(body);
  return details;
}

function buildDiagramItem(d) {
  if (!d.src && !d.tgt) {
    const div = document.createElement('div');
    div.className = 'acc-leaf';
    div.innerHTML = hi(d.name);
    return div;
  }
  const details = document.createElement('details');
  details.className = 'acc-item';
  const summary = document.createElement('summary');
  summary.innerHTML = hi(d.name);
  details.appendChild(summary);

  const body = document.createElement('div');
  body.className = 'acc-item-body';
  body.innerHTML = `${esc(d.src)} → ${esc(d.tgt)}`;
  details.appendChild(body);
  return details;
}

function buildMapItem(m) {
  const details = document.createElement('details');
  details.className = 'acc-item';
  const summary = document.createElement('summary');
  summary.innerHTML = hi(m.name);
  details.appendChild(summary);

  const body = document.createElement('div');
  body.className = 'acc-item-body';
  body.innerHTML = `:: ${esc(m.domain)}`;
  details.appendChild(body);
  return details;
}

// ── Session setup ─────────────────────────────────────────────────────────────

btnStart.addEventListener('click', startSession);

function startSession() {
  if (!repl) return;
  const typeName = selType.value;
  const src = inpSource.value.trim();
  const tgt = inpTarget.value.trim() || undefined;
  if (!typeName) { appendReplMsg('Select a type first.', 'repl-result err'); return; }
  if (!src)      { appendReplMsg('Enter a source diagram.', 'repl-result err'); return; }

  const result = JSON.parse(repl.init_session(typeName, src, tgt));
  if (result.status === 'error') {
    appendReplEntry('(start session)', formatError(result.message));
    return;
  }
  sessionActive = true;
  replInput.disabled = false;
  replInput.focus();
  appendReplEntry(`start ${typeName} ${src}${tgt ? ' → ' + tgt : ''}`, renderState(result.data));
  updateVisInfo(result.data);
}

function resetSession() {
  sessionActive = false;
  replInput.disabled = true;
}

// ── REPL input ────────────────────────────────────────────────────────────────

replInput.addEventListener('keydown', e => {
  if (e.key === 'Enter') {
    e.preventDefault();
    const cmd = replInput.value.trim();
    if (!cmd) return;
    history.unshift(cmd);
    histIdx = -1;
    replInput.value = '';
    handleCommand(cmd);
  } else if (e.key === 'ArrowUp') {
    e.preventDefault();
    if (histIdx + 1 < history.length) { histIdx++; replInput.value = history[histIdx]; }
  } else if (e.key === 'ArrowDown') {
    e.preventDefault();
    if (histIdx > 0) { histIdx--; replInput.value = history[histIdx]; }
    else { histIdx = -1; replInput.value = ''; }
  }
});

function handleCommand(raw) {
  const [cmd, ...rest] = raw.trim().split(/\s+/);
  const arg = rest.join(' ');

  if (cmd === 'help' || cmd === '?') {
    appendReplEntry(raw, HELP_TEXT);
    return;
  }

  const json = buildCommand(cmd, arg, raw);
  if (!json) return;

  const result = JSON.parse(repl.run_command(json));

  if (result.status === 'error') {
    appendReplEntry(raw, formatError(result.message));
  } else {
    const rendered = renderCommandResult(cmd, result.data);
    appendReplEntry(raw, rendered);
    updateVisInfo(result.data);
  }
}

function buildCommand(cmd, arg, raw) {
  switch (cmd) {
    case 'show':     return '{"command":"show"}';
    case 'undo':
      if (arg) return JSON.stringify({ command: 'undo_to', step: parseInt(arg, 10) });
      return '{"command":"undo"}';
    case 'step': {
      const n = parseInt(arg, 10);
      if (isNaN(n)) { appendReplEntry(raw, formatError('usage: step <n>')); return null; }
      return JSON.stringify({ command: 'step', choice: n });
    }
    case 'rules':    return '{"command":"list_rules"}';
    case 'history':  return '{"command":"history"}';
    case 'types':    return '{"command":"types"}';
    case 'type':
      if (!arg) { appendReplEntry(raw, formatError('usage: type <name>')); return null; }
      return JSON.stringify({ command: 'type', name: arg });
    case 'cell':
      if (!arg) { appendReplEntry(raw, formatError('usage: cell <name>')); return null; }
      return JSON.stringify({ command: 'cell', name: arg });
    case 'store':
      if (!arg) { appendReplEntry(raw, formatError('usage: store <name>')); return null; }
      return JSON.stringify({ command: 'store', name: arg });
    default:
      appendReplEntry(raw, formatError(`unknown command '${cmd}' — type help for commands`));
      return null;
  }
}

// ── Rendering ─────────────────────────────────────────────────────────────────

function renderCommandResult(cmd, data) {
  if (!data) return formatError('(no data)');

  switch (cmd) {
    case 'types':   return renderTypes(data);
    case 'type':    return renderTypeDetail(data.type_detail);
    case 'cell':    return renderCellDetail(data.cell_detail);
    case 'rules':   return renderRules(data.rules);
    case 'history': return renderHistory(data.history);
    default:        return renderState(data);
  }
}

function renderState(data) {
  if (!data) return '';
  let out = [];

  out.push(dim('step:') + ' ' + hi(data.step_count));

  const cur = data.current;
  if (cur) out.push(dim('current:') + ' ' + hi(cur.label || '—') + dim(` (dim ${cur.dim}, ${cur.cell_count} cell${cur.cell_count !== 1 ? 's' : ''})`));

  if (data.target) {
    const reached = data.target_reached;
    out.push(dim('target:') + ' ' + hi(data.target.label) +
      (reached ? ' ' + ok('✓ reached') : ''));
  }

  if (data.rewrites && data.rewrites.length > 0) {
    out.push('');
    out.push(sec('available rewrites:'));
    data.rewrites.forEach(r => {
      out.push(`  [${hi(r.index)}] ${hi(r.rule_name)}  ${dim(r.source.label)} → ${dim(r.target.label)}`);
      if (r.match_display) out.push(`      match: ${esc(r.match_display)}`);
    });
  } else if (data.step_count > 0) {
    out.push(dim('no rewrites available'));
  }

  if (data.target_reached) out.push('');

  return out.join('\n');
}

function renderTypes(data) {
  if (!data.types || !data.types.length) return dim('(no types)');
  return data.types.map(t =>
    hi(t.name) + dim(` — ${t.generator_count} gen, ${t.diagram_count} diag, dim ${t.max_dim ?? '?'}`)
  ).join('\n');
}

function renderTypeDetail(d) {
  if (!d) return dim('(no type detail)');
  let out = [sec(d.name)];
  if (d.generators.length) {
    out.push(dim('generators:'));
    d.generators.forEach(g => {
      const bounds = g.source ? `  ${dim(g.source.label)} → ${dim(g.target.label)}` : '';
      out.push(`  ${hi(g.name)} ${dim(`(dim ${g.dim})`)}${bounds}`);
    });
  }
  if (d.diagrams.length) {
    out.push(dim('diagrams:'));
    d.diagrams.forEach(g => {
      out.push(`  let ${hi(g.name)} = ${dim(g.expr)}`);
    });
  }
  return out.join('\n');
}

function renderCellDetail(d) {
  if (!d) return dim('(not found)');
  let out = [`${hi(d.name)} ${dim(`[${d.kind}, dim ${d.dim}]`)}`];
  if (d.source) out.push(`  src: ${esc(d.source.label)}`);
  if (d.target) out.push(`  tgt: ${esc(d.target.label)}`);
  if (d.expr)   out.push(`  = ${esc(d.expr)}`);
  return out.join('\n');
}

function renderRules(rules) {
  if (!rules || !rules.length) return dim('(no rules)');
  return rules.map(r =>
    `  ${hi(r.name)}  ${dim(r.source.label)} → ${dim(r.target.label)}`
  ).join('\n');
}

function renderHistory(hist) {
  if (!hist || !hist.length) return dim('(no moves yet)');
  return hist.map(h =>
    `  ${dim(h.step + '.')} ${hi(h.rule_name)} ${dim('[choice ' + h.choice + ']')}`
  ).join('\n');
}

function updateVisInfo(data) {
  if (!data || !data.current) { visInfo.hidden = true; return; }
  const cur = data.current;
  let lines = [`dim: ${cur.dim}`, `cells: ${cur.cell_count}`];
  if (data.step_count) lines.push(`steps: ${data.step_count}`);
  if (data.target_reached) lines.push('target reached ✓');
  visInfo.textContent = lines.join('\n');
  visInfo.hidden = false;
}

// ── Formatting helpers ────────────────────────────────────────────────────────

// Escape raw strings before embedding in HTML.
function esc(s) {
  return String(s)
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;');
}

// These return HTML strings; render functions compose them and set innerHTML.
function hi(s)  { return `<span class="repl-hi">${esc(s)}</span>`; }
function dim(s) { return `<span class="repl-dim">${esc(s)}</span>`; }
function sec(s) { return `<span class="repl-section-title">${esc(s)}</span>`; }
function ok(s)  { return `<span class="repl-ok">${esc(s)}</span>`; }

// Plain-text messages (errors, status) — no HTML, use textContent.
function formatOk(msg)    { return { cls: 'repl-result ok',  text: msg }; }
function formatError(msg) { return { cls: 'repl-result err', text: msg }; }

function appendReplMsg(text, cls = 'repl-dim') {
  const el = document.createElement('div');
  el.className = cls;
  el.textContent = text;
  replOutput.appendChild(el);
  replOutput.scrollTop = replOutput.scrollHeight;
}

function appendReplEntry(cmdText, result) {
  const entry = document.createElement('div');
  entry.className = 'repl-entry';

  const cmdEl = document.createElement('div');
  cmdEl.className = 'repl-cmd';
  cmdEl.textContent = cmdText;
  entry.appendChild(cmdEl);

  if (result) {
    const resEl = document.createElement('div');
    if (typeof result === 'string') {
      // result is an HTML string from the render functions (hi/dim/sec/ok spans)
      resEl.className = 'repl-result';
      resEl.innerHTML = result;
    } else {
      // plain-text object from formatOk/formatError
      resEl.className = result.cls;
      resEl.textContent = result.text;
    }
    entry.appendChild(resEl);
  }

  replOutput.appendChild(entry);
  replOutput.scrollTop = replOutput.scrollHeight;
}

// ── Misc ──────────────────────────────────────────────────────────────────────

btnClear.addEventListener('click', () => {
  replOutput.innerHTML = '';
});

const HELP_TEXT = `Commands:
  step <n>      apply rewrite choice n
  undo          undo last step
  undo <n>      undo back to step n (0 = reset)
  show          show current state
  rules         list all rewrite rules
  history       show move history
  types         list all types in the file
  type <name>   inspect a type
  cell <name>   inspect a generator or let-binding
  store <name>  register current proof as a generator
  help / ?      show this message

Keyboard: ↑/↓ navigate history · Ctrl+Enter evaluate file`;

// ── Default example ───────────────────────────────────────────────────────────

editor.value = `@Type
(* A simple example: equation between two composable morphisms *)
Ob <<= {
  pt,
  ob : pt -> pt
},

Cat <<= {
  attach Ob :: Ob,
  let o = Ob.ob,
  f : o -> o,
  g : o -> o,
  h : o -> o,
  assoc_l : (f g) h -> f (g h),
  assoc_r : f (g h) -> (f g) h
}
`;

// ── Init ──────────────────────────────────────────────────────────────────────

boot();
