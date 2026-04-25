// ── State ─────────────────────────────────────────────────────────────────────

let repl = null;
let sessionActive = false;
const history = [];
let histIdx = -1;
let currentLayout = null;
let selectedEl = null;
let dragState = null;
let splitterDrag = null;
const thinTags = new Set();
const tagFaces = new Map();
const fullyThinTags = new Set();

function recomputeFullyThin() {
  fullyThinTags.clear();
  for (const tag of thinTags) {
    const faces = tagFaces.get(tag);
    if (faces && faces.every(f => thinTags.has(f))) {
      fullyThinTags.add(tag);
    }
  }
}

const MIN_WORKSPACE_WIDTHS = [240, 260, 280];
const MIN_ANALYSIS_HEIGHTS = [60, 180];
const MIN_INFOBOX_HEADER_HEIGHT = 56;
const MIN_INFOBOX_VIS_HEIGHT = 120;
const MIN_REWRITE_HEIGHT = 72;
const layoutState = {
  workspaceRatios: [1 / 3, 1 / 3, 1 / 3],
  analysisRatio: 0.2,
  infoboxHeaderRatio: 0.18,
  rewriteRatio: 0.22,
};

// ── DOM refs ──────────────────────────────────────────────────────────────────

const workspace   = document.getElementById('workspace');
const paneFile    = document.getElementById('pane-file');
const paneRepl    = document.getElementById('pane-repl');
const paneAnalysis = document.getElementById('pane-analysis');
const resizerFileRepl = document.getElementById('resizer-file-repl');
const resizerReplAnalysis = document.getElementById('resizer-repl-analysis');
const analysisBody = document.getElementById('analysis-body');
const analysisResizer = document.getElementById('analysis-resizer');
const editor      = document.getElementById('editor');
const editorHighlight = document.getElementById('editor-highlight');
const selExamples = document.getElementById('sel-examples');
const btnLoad     = document.getElementById('btn-load');
const btnSave     = document.getElementById('btn-save');
const fileInput   = document.getElementById('file-input');
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
const visContainer = document.getElementById('vis-container');
const infobox     = document.getElementById('infobox');
const infoboxHeader = document.getElementById('infobox-header');
const infoboxResizer = document.getElementById('infobox-resizer');
const infoboxText = document.getElementById('infobox-text');
const boundaryControls = document.getElementById('boundary-controls');
const selBoundary = document.getElementById('sel-boundary');
const signControls = document.getElementById('sign-controls');
const visCanvas   = document.getElementById('vis-canvas');
const visControls = document.getElementById('vis-controls');
const selOrientation = document.getElementById('sel-orientation');
const rewriteResizer = document.getElementById('rewrite-resizer');
const rewriteList = document.getElementById('rewrite-list');
const canvasCtx   = visCanvas.getContext('2d');

// ── Boot ──────────────────────────────────────────────────────────────────────

class WasmBackend {
  constructor(inner) {
    this.inner = inner;
    this.label = 'WASM';
  }

  async reset() {
    this.inner.reset();
  }

  async load_source(source, modules) {
    const modulesJson = modules ? JSON.stringify(modules) : null;
    return this.inner.load_source(source, modulesJson);
  }

  async init_session(typeName, sourceDiagram, targetDiagram) {
    return this.inner.init_session(typeName, sourceDiagram, targetDiagram);
  }

  async run_command(commandJson) {
    return this.inner.run_command(commandJson);
  }

  async get_types() {
    return this.inner.get_types();
  }

  async get_strdiag(typeName, itemName, boundaryDim, boundarySign) {
    return this.inner.get_strdiag(typeName, itemName, boundaryDim, boundarySign);
  }

  async get_session_strdiag() {
    return this.inner.get_session_strdiag();
  }

  async get_rewrite_preview_strdiag(choice) {
    return this.inner.get_rewrite_preview_strdiag(choice);
  }
}

class HttpBackend {
  constructor(baseUrl = '') {
    this.baseUrl = baseUrl;
    this.label = 'HTTP';
  }

  async reset() {}

  async load_source(source, modules) {
    return this.post('/api/load_source', { source, modules: modules || {} });
  }

  async init_session(typeName, sourceDiagram, targetDiagram) {
    return this.post('/api/init_session', {
      type_name: typeName,
      source_diagram: sourceDiagram,
      target_diagram: targetDiagram,
    });
  }

  async run_command(commandJson) {
    return this.post('/api/run_command', { command_json: commandJson });
  }

  async get_types() {
    return this.post('/api/get_types', {});
  }

  async get_strdiag(typeName, itemName, boundaryDim, boundarySign) {
    return this.post('/api/get_strdiag', {
      type_name: typeName,
      item_name: itemName,
      boundary_dim: boundaryDim,
      boundary_sign: boundarySign,
    });
  }

  async get_session_strdiag() {
    return this.post('/api/get_session_strdiag', {});
  }

  async get_rewrite_preview_strdiag(choice) {
    return this.post('/api/get_rewrite_preview_strdiag', { choice });
  }

  async post(path, body) {
    try {
      const response = await fetch(this.baseUrl + path, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body),
      });
      const text = await response.text();
      if (text) return text;
      return JSON.stringify({
        status: 'error',
        message: `empty response from ${path}`,
      });
    } catch (error) {
      return JSON.stringify({
        status: 'error',
        message: `request failed: ${error}`,
      });
    }
  }
}

function backendConfig() {
  const query = new URLSearchParams(window.location.search);
  const config = globalThis.ALIFIB_CONFIG || {};
  return {
    mode: config.backend || query.get('backend') || 'wasm',
    apiBase: config.apiBase || '',
  };
}

async function createBackend() {
  const config = backendConfig();
  if (config.mode === 'http') {
    return new HttpBackend(config.apiBase);
  }

  const wasm = await import('./pkg/alifib_wasm.js');
  await wasm.default();
  return new WasmBackend(new wasm.WasmRepl());
}

async function parseReplResponse(promise) {
  return JSON.parse(await promise);
}

async function boot() {
  btnEval.disabled = true;
  btnEval.textContent = 'Loading…';
  try {
    repl = await createBackend();
    btnEval.disabled = false;
    btnEval.textContent = 'Evaluate';
    appendReplMsg(`${repl.label} engine ready. Evaluate a file to begin.`, 'repl-dim');
    appendReplMsg('', 'repl-dim');
    const helpEl = document.createElement('div');
    helpEl.className = 'repl-result';
    helpEl.textContent = HELP_TEXT;
    replOutput.appendChild(helpEl);
    void populateExamples();
  } catch (e) {
    btnEval.textContent = 'Error';
    appendReplMsg('Failed to load backend: ' + e, 'repl-result err');
  }
}

// ── Pane layout ──────────────────────────────────────────────────────────────

function cssPx(name, fallback) {
  const value = parseFloat(getComputedStyle(document.documentElement).getPropertyValue(name));
  return Number.isFinite(value) ? value : fallback;
}

function clamp(value, min, max) {
  return Math.min(max, Math.max(min, value));
}

function scaledMins(mins, total) {
  const sum = mins.reduce((acc, min) => acc + min, 0);
  if (sum <= total || total <= 0) return mins.slice();
  const scale = total / sum;
  return mins.map(min => min * scale);
}

function distributeSizes(total, ratios, mins) {
  if (total <= 0) return mins.slice();

  const widths = ratios.map(r => Math.max(0, r) * total);
  const baseTotal = widths.reduce((acc, width) => acc + width, 0);
  if (baseTotal > 0) {
    for (let i = 0; i < widths.length; i++) {
      widths[i] = widths[i] / baseTotal * total;
    }
  } else {
    widths.fill(total / widths.length);
  }

  const locked = new Array(widths.length).fill(false);
  while (true) {
    let fixedTotal = 0;
    let flexTotal = 0;
    const flexIdx = [];

    for (let i = 0; i < widths.length; i++) {
      if (locked[i]) {
        fixedTotal += widths[i];
      } else {
        flexIdx.push(i);
        flexTotal += widths[i];
      }
    }

    if (!flexIdx.length) break;

    const remaining = total - fixedTotal;
    for (const idx of flexIdx) {
      widths[idx] = flexTotal > 0 ? widths[idx] / flexTotal * remaining : remaining / flexIdx.length;
    }

    let changed = false;
    for (const idx of flexIdx) {
      if (widths[idx] < mins[idx]) {
        widths[idx] = mins[idx];
        locked[idx] = true;
        changed = true;
      }
    }
    if (!changed) break;
  }

  widths[widths.length - 1] += total - widths.reduce((acc, width) => acc + width, 0);
  return widths;
}

function setSplitterActive(resizer, cursor) {
  resizer.classList.add('is-active');
  document.body.classList.add('is-resizing');
  document.body.style.setProperty('--resize-cursor', cursor);
}

function clearSplitterActive() {
  document.body.classList.remove('is-resizing');
  document.body.style.removeProperty('--resize-cursor');
  if (splitterDrag?.resizer) splitterDrag.resizer.classList.remove('is-active');
}

function getSplitterLineSize() {
  return cssPx('--splitter-line', 1);
}

function applyWorkspaceWidths(widths) {
  const splitterSize = getSplitterLineSize();
  const total = widths.reduce((acc, width) => acc + width, 0) || 1;
  workspace.style.gridTemplateColumns = `${widths[0]}px ${splitterSize}px ${widths[1]}px ${splitterSize}px ${widths[2]}px`;
  layoutState.workspaceRatios = widths.map(width => width / total);
}

function syncWorkspaceLayout() {
  const splitterSize = getSplitterLineSize();
  const available = workspace.clientWidth - splitterSize * 2;
  if (available <= 0) return;

  const mins = scaledMins(MIN_WORKSPACE_WIDTHS, available);
  const widths = distributeSizes(available, layoutState.workspaceRatios, mins);
  applyWorkspaceWidths(widths);
}

function applyAnalysisHeights(top, bottom) {
  const splitterSize = getSplitterLineSize();
  analysisBody.style.gridTemplateRows = `${top}px ${splitterSize}px ${bottom}px`;
  fileOutput.style.gridRow = '1';
  analysisResizer.style.gridRow = '2';
  infobox.style.gridRow = '3';
  analysisResizer.hidden = false;
  layoutState.analysisRatio = top / Math.max(1, top + bottom);
}

function getInfoboxSectionRatios() {
  const header = Math.max(0.01, layoutState.infoboxHeaderRatio);
  const rewrite = Math.max(0, layoutState.rewriteRatio);
  const vis = Math.max(0.01, 1 - header - rewrite);
  const total = header + vis + rewrite || 1;
  return [header / total, vis / total, rewrite / total];
}

function syncInfoboxRatios(headerHeight, visHeight, rewriteHeight) {
  const total = headerHeight + visHeight + rewriteHeight || 1;
  layoutState.infoboxHeaderRatio = headerHeight / total;
  layoutState.rewriteRatio = rewriteHeight / total;
}

function syncInfoboxHeaderRatio(headerHeight, visHeight) {
  const rewriteRatio = Math.max(0, layoutState.rewriteRatio);
  const activeShare = Math.max(0.01, 1 - rewriteRatio);
  const total = headerHeight + visHeight || 1;
  layoutState.infoboxHeaderRatio = activeShare * (headerHeight / total);
}

function applyInfoboxHeights(headerHeight, visHeight, rewriteHeight = null) {
  const splitterSize = getSplitterLineSize();
  infoboxHeader.style.gridRow = '1';

  if (rewriteHeight === null) {
    infobox.style.gridTemplateRows = `${headerHeight}px ${splitterSize}px ${visHeight}px`;
    infoboxResizer.style.gridRow = '2';
    visContainer.style.gridRow = '3';
    infoboxResizer.hidden = false;
    rewriteResizer.hidden = true;
    syncInfoboxHeaderRatio(headerHeight, visHeight);
    return;
  }

  infobox.style.gridTemplateRows = `${headerHeight}px ${splitterSize}px ${visHeight}px ${splitterSize}px ${rewriteHeight}px`;
  infoboxResizer.style.gridRow = '2';
  visContainer.style.gridRow = '3';
  rewriteResizer.style.gridRow = '4';
  rewriteList.style.gridRow = '5';
  infoboxResizer.hidden = false;
  rewriteResizer.hidden = false;
  syncInfoboxRatios(headerHeight, visHeight, rewriteHeight);
}

function syncInfoboxLayout() {
  if (infobox.hidden) return;

  const visVisible = !visContainer.hidden;
  const rewriteVisible = !rewriteList.hidden;
  const splitterSize = getSplitterLineSize();

  if (visVisible && rewriteVisible) {
    const available = infobox.clientHeight - splitterSize * 2;
    if (available <= 0) return;

    const heights = distributeSizes(
      available,
      getInfoboxSectionRatios(),
      [MIN_INFOBOX_HEADER_HEIGHT, MIN_INFOBOX_VIS_HEIGHT, MIN_REWRITE_HEIGHT],
    );
    applyInfoboxHeights(heights[0], heights[1], heights[2]);
    return;
  }

  if (visVisible) {
    const [headerRatio, visRatio] = getInfoboxSectionRatios();
    const available = infobox.clientHeight - splitterSize;
    if (available <= 0) return;

    const heights = distributeSizes(
      available,
      [headerRatio, visRatio],
      [MIN_INFOBOX_HEADER_HEIGHT, MIN_INFOBOX_VIS_HEIGHT],
    );
    applyInfoboxHeights(heights[0], heights[1]);
    return;
  }

  infoboxResizer.hidden = true;
  rewriteResizer.hidden = true;
  if (rewriteVisible) {
    infobox.style.gridTemplateRows = 'auto minmax(0, 1fr)';
    infoboxHeader.style.gridRow = '1';
    rewriteList.style.gridRow = '2';
    return;
  }

  infobox.style.gridTemplateRows = 'minmax(0, 1fr)';
  infoboxHeader.style.gridRow = '1';
}

function syncAnalysisLayout() {
  const topVisible = !fileOutput.hidden;
  const bottomVisible = !infobox.hidden;

  if (topVisible && bottomVisible) {
    const splitterSize = getSplitterLineSize();
    const available = analysisBody.clientHeight - splitterSize;
    if (available <= 0) return;

    const mins = scaledMins(MIN_ANALYSIS_HEIGHTS, available);
    const top = clamp(layoutState.analysisRatio * available, mins[0], available - mins[1]);
    applyAnalysisHeights(top, available - top);
    syncInfoboxLayout();
    return;
  }

  analysisResizer.hidden = true;
  analysisBody.style.gridTemplateRows = 'minmax(0, 1fr)';
  if (topVisible) fileOutput.style.gridRow = '1';
  if (bottomVisible) infobox.style.gridRow = '1';
  syncInfoboxLayout();
}

function startWorkspaceDrag(which, event) {
  const widths = [paneFile, paneRepl, paneAnalysis].map(pane => pane.getBoundingClientRect().width);
  splitterDrag = {
    kind: 'workspace',
    which,
    startX: event.clientX,
    widths,
    resizer: which === 0 ? resizerFileRepl : resizerReplAnalysis,
  };
  setSplitterActive(splitterDrag.resizer, 'ew-resize');
  event.preventDefault();
}

function updateWorkspaceDrag(clientX) {
  if (!splitterDrag || splitterDrag.kind !== 'workspace') return;

  const dx = clientX - splitterDrag.startX;
  const widths = splitterDrag.widths.slice();
  const mins = scaledMins(MIN_WORKSPACE_WIDTHS, widths.reduce((acc, width) => acc + width, 0));

  if (splitterDrag.which === 0) {
    const pair = widths[0] + widths[1];
    widths[0] = clamp(widths[0] + dx, mins[0], pair - mins[1]);
    widths[1] = pair - widths[0];
  } else {
    const pair = widths[1] + widths[2];
    widths[1] = clamp(widths[1] + dx, mins[1], pair - mins[2]);
    widths[2] = pair - widths[1];
  }

  applyWorkspaceWidths(widths);
}

function startAnalysisDrag(event) {
  if (fileOutput.hidden || infobox.hidden) return;

  splitterDrag = {
    kind: 'analysis',
    startY: event.clientY,
    heights: [
      fileOutput.getBoundingClientRect().height,
      infobox.getBoundingClientRect().height,
    ],
    resizer: analysisResizer,
  };
  setSplitterActive(analysisResizer, 'ns-resize');
  event.preventDefault();
}

function startInfoboxDrag(event) {
  if (visContainer.hidden || infobox.hidden) return;

  splitterDrag = {
    kind: 'infobox',
    startY: event.clientY,
    heights: [
      infoboxHeader.getBoundingClientRect().height,
      visContainer.getBoundingClientRect().height,
    ],
    rewriteHeight: rewriteList.hidden ? null : rewriteList.getBoundingClientRect().height,
    resizer: infoboxResizer,
  };
  setSplitterActive(infoboxResizer, 'ns-resize');
  event.preventDefault();
}

function startRewriteDrag(event) {
  if (visContainer.hidden || rewriteList.hidden || infobox.hidden) return;

  splitterDrag = {
    kind: 'rewrite',
    startY: event.clientY,
    headerHeight: infoboxHeader.getBoundingClientRect().height,
    heights: [
      visContainer.getBoundingClientRect().height,
      rewriteList.getBoundingClientRect().height,
    ],
    resizer: rewriteResizer,
  };
  setSplitterActive(rewriteResizer, 'ns-resize');
  event.preventDefault();
}

function updateAnalysisDrag(clientY) {
  if (!splitterDrag || splitterDrag.kind !== 'analysis') return;

  const dy = clientY - splitterDrag.startY;
  const [startTop, startBottom] = splitterDrag.heights;
  const total = startTop + startBottom;
  const mins = scaledMins(MIN_ANALYSIS_HEIGHTS, total);
  const top = clamp(startTop + dy, mins[0], total - mins[1]);
  applyAnalysisHeights(top, total - top);
  syncInfoboxLayout();
}

function updateInfoboxDrag(clientY) {
  if (!splitterDrag || splitterDrag.kind !== 'infobox') return;

  const dy = clientY - splitterDrag.startY;
  const [startHeader, startVis] = splitterDrag.heights;
  const total = startHeader + startVis;
  const mins = scaledMins([MIN_INFOBOX_HEADER_HEIGHT, MIN_INFOBOX_VIS_HEIGHT], total);
  const headerHeight = clamp(startHeader + dy, mins[0], total - mins[1]);
  const visHeight = total - headerHeight;

  if (splitterDrag.rewriteHeight === null) {
    applyInfoboxHeights(headerHeight, visHeight);
    return;
  }

  applyInfoboxHeights(headerHeight, visHeight, splitterDrag.rewriteHeight);
}

function updateRewriteDrag(clientY) {
  if (!splitterDrag || splitterDrag.kind !== 'rewrite') return;

  const dy = clientY - splitterDrag.startY;
  const [startVis, startRewrite] = splitterDrag.heights;
  const total = startVis + startRewrite;
  const mins = scaledMins([MIN_INFOBOX_VIS_HEIGHT, MIN_REWRITE_HEIGHT], total);
  const visHeight = clamp(startVis + dy, mins[0], total - mins[1]);
  applyInfoboxHeights(splitterDrag.headerHeight, visHeight, total - visHeight);
}

function endSplitterDrag() {
  if (!splitterDrag) return;
  clearSplitterActive();
  splitterDrag = null;
}

resizerFileRepl.addEventListener('mousedown', (event) => startWorkspaceDrag(0, event));
resizerReplAnalysis.addEventListener('mousedown', (event) => startWorkspaceDrag(1, event));
analysisResizer.addEventListener('mousedown', startAnalysisDrag);
infoboxResizer.addEventListener('mousedown', startInfoboxDrag);
rewriteResizer.addEventListener('mousedown', startRewriteDrag);

document.addEventListener('mousemove', (event) => {
  if (!splitterDrag) return;
  if (splitterDrag.kind === 'workspace') updateWorkspaceDrag(event.clientX);
  if (splitterDrag.kind === 'analysis') updateAnalysisDrag(event.clientY);
  if (splitterDrag.kind === 'infobox') updateInfoboxDrag(event.clientY);
  if (splitterDrag.kind === 'rewrite') updateRewriteDrag(event.clientY);
});
document.addEventListener('mouseup', endSplitterDrag);
window.addEventListener('blur', endSplitterDrag);

const workspaceResizeObs = new ResizeObserver(() => syncWorkspaceLayout());
workspaceResizeObs.observe(workspace);

const analysisResizeObs = new ResizeObserver(() => syncAnalysisLayout());
analysisResizeObs.observe(analysisBody);

// ── Evaluate ──────────────────────────────────────────────────────────────────

btnEval.addEventListener('click', () => { void evaluateSource(); });
editor.addEventListener('keydown', e => {
  if ((e.ctrlKey || e.metaKey) && e.key === 'Enter') { e.preventDefault(); void evaluateSource(); }
});

async function evaluateSource() {
  if (!repl) return;
  const src = editor.value;
  if (!src.trim()) return;

  const previousType = selType.value;

  await repl.reset();
  const modules = await collectIncludeModules(src);
  const result = await parseReplResponse(repl.load_source(src, modules));

  if (result.status === 'error') {
    fileOutput.innerHTML = '';
    fileOutput.hidden = true;
    appendReplEntry('(evaluate)', formatError(result));
    sessionSetup.hidden = true;
    resetSession();
    syncAnalysisLayout();
    return;
  }

  const types = result.types || [];

  thinTags.clear();
  tagFaces.clear();
  fullyThinTags.clear();
  for (const t of types) {
    for (const g of t.generators) {
      if (g.tag != null) tagFaces.set(g.tag, (g.face_tags || []).filter(ft => ft != null));
    }
  }

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

  if (previousType && types.some(t => t.name === previousType)) {
    selType.value = previousType;
  }

  sessionSetup.hidden = false;
  resetSession();
  syncAnalysisLayout();
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

  if (t.generators.length) body.appendChild(buildSection('Generators', t.generators,
    g => buildGeneratorRow(g, t.name)));
  if (t.diagrams.length) body.appendChild(buildSection('Diagrams', t.diagrams,
    d => buildClickableRow(hi(d.name),
      () => selectItem(t.name, { kind: 'diagram', name: d.name, src: d.src, tgt: d.tgt }))));
  if (t.maps.length) body.appendChild(buildSection('Maps', t.maps,
    m => buildClickableRow(hi(m.name),
      () => selectItem(t.name, { kind: 'map', name: m.name, domain: m.domain }))));

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

function buildClickableRow(innerHTML, onClick) {
  const div = document.createElement('div');
  div.className = 'acc-leaf';
  div.innerHTML = innerHTML;
  div.addEventListener('click', () => {
    if (selectedEl === div) {
      // Deselect: return to session view.
      div.classList.remove('acc-leaf--selected');
      selectedEl = null;
      currentItem = null;
      void returnToSessionView();
      return;
    }
    if (selectedEl) selectedEl.classList.remove('acc-leaf--selected');
    selectedEl = div;
    div.classList.add('acc-leaf--selected');
    void onClick();
  });
  return div;
}

function buildGeneratorRow(g, typeName) {
  const div = document.createElement('div');
  div.className = 'acc-leaf acc-leaf-gen';
  if (g.tag != null && thinTags.has(g.tag)) div.classList.add('acc-leaf--thin');

  const text = document.createElement('span');
  text.className = 'acc-leaf-text';
  text.innerHTML = `${hi(g.name)} <span class="acc-dim">dim ${g.dim}</span>`;
  div.appendChild(text);

  const toggle = document.createElement('span');
  toggle.className = 'thin-toggle' + (g.tag != null && thinTags.has(g.tag) ? ' thin-toggle--active' : '');
  toggle.title = 'Toggle thin';
  toggle.addEventListener('click', (e) => {
    e.stopPropagation();
    if (g.tag == null) return;
    const wasThin = thinTags.has(g.tag);
    if (wasThin) thinTags.delete(g.tag);
    else thinTags.add(g.tag);
    recomputeFullyThin();
    toggle.classList.toggle('thin-toggle--active', !wasThin);
    div.classList.toggle('acc-leaf--thin', !wasThin);
    document.querySelectorAll(`.acc-leaf-gen[data-tag="${g.tag}"]`).forEach(row => {
      row.classList.toggle('acc-leaf--thin', !wasThin);
      row.querySelector('.thin-toggle')?.classList.toggle('thin-toggle--active', !wasThin);
    });
    resizeAndRender();
  });
  div.appendChild(toggle);
  div.dataset.tag = g.tag != null ? g.tag : '';
  div.dataset.gen = g.name;

  div.addEventListener('click', () => {
    if (selectedEl === div) {
      div.classList.remove('acc-leaf--selected');
      selectedEl = null;
      currentItem = null;
      void returnToSessionView();
      return;
    }
    if (selectedEl) selectedEl.classList.remove('acc-leaf--selected');
    selectedEl = div;
    div.classList.add('acc-leaf--selected');
    void selectItem(typeName, { kind: 'generator', name: g.name, dim: g.dim, src: g.src, tgt: g.tgt });
  });

  return div;
}

async function returnToSessionView() {
  if (!sessionActive || !repl) {
    infobox.hidden = true;
    rewriteList.hidden = true;
    syncAnalysisLayout();
    return;
  }
  // Re-fetch session state and show diagram.
  const result = await parseReplResponse(repl.run_command('{"command":"show"}'));
  if (result.status === 'ok' && result.data) {
    await showSessionDiagram(result.data);
  }
}

// ── Session setup ─────────────────────────────────────────────────────────────

btnStart.addEventListener('click', () => { void startSession(); });
inpSource.addEventListener('keydown', e => { if (e.key === 'Enter') { e.preventDefault(); void startSession(); } });
inpTarget.addEventListener('keydown', e => { if (e.key === 'Enter') { e.preventDefault(); void startSession(); } });

async function startSession() {
  if (!repl) return;
  const typeName = selType.value;
  const src = inpSource.value.trim();
  const tgt = inpTarget.value.trim() || undefined;
  if (!typeName) { appendReplMsg('Select a type first.', 'repl-result err'); return; }
  if (!src)      { appendReplMsg('Enter a source diagram.', 'repl-result err'); return; }

  const result = await parseReplResponse(repl.init_session(typeName, src, tgt));
  if (result.status === 'error') {
    appendReplEntry('(start session)', formatError(result));
    return;
  }
  sessionActive = true;
  replInput.disabled = false;
  replInput.focus();
  // Clear any accordion selection so session display takes over.
  if (selectedEl) {
    selectedEl.classList.remove('acc-item--selected');
    selectedEl.classList.remove('acc-leaf--selected');
    selectedEl = null;
  }
  currentItem = null;
  appendReplEntry(`start ${typeName} ${src}${tgt ? ' → ' + tgt : ''}`, renderState(result.data));
  await showSessionDiagram(result.data);
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
    void handleCommand(cmd);
  } else if (e.key === 'ArrowUp') {
    e.preventDefault();
    if (histIdx + 1 < history.length) { histIdx++; replInput.value = history[histIdx]; }
  } else if (e.key === 'ArrowDown') {
    e.preventDefault();
    if (histIdx > 0) { histIdx--; replInput.value = history[histIdx]; }
    else { histIdx = -1; replInput.value = ''; }
  }
});

async function handleCommand(raw) {
  const [cmd, ...rest] = raw.trim().split(/\s+/);
  const arg = rest.join(' ');

  if (cmd === 'help' || cmd === '?') {
    appendReplEntry(raw, { cls: 'repl-result', text: HELP_TEXT });
    return;
  }

  const json = buildCommand(cmd, arg, raw);
  if (!json) return;

  const result = await parseReplResponse(repl.run_command(json));

  if (result.status === 'error') {
    appendReplEntry(raw, formatError(result));
  } else {
    const rendered = renderCommandResult(cmd, result.data);
    appendReplEntry(raw, rendered);
    // Only update the session diagram display for state-changing commands.
    const stateCommands = ['apply', 'a', 'auto', 'undo', 'u', 'restart', 'show', 'status', 'store', 'parallel'];
    if (stateCommands.includes(cmd)) {
      await updateVisInfo(result.data);
    }
    // Append definition to editor and refresh accordion when store succeeds.
    if (cmd === 'store' && result.data && result.data.stored) {
      const s = result.data.stored;
      const code = `\n\n@${s.type_name}\nlet ${s.def_name} = ${s.expr}`;
      editor.value = editor.value.trimEnd() + code + '\n';
      updateHighlight();
      await refreshAccordion();
    }
  }
}

function buildCommand(cmd, arg, raw) {
  switch (cmd) {
    case 'show':
    case 'status':   return '{"command":"show"}';
    case 'undo':
    case 'u':
      if (arg === 'all') return JSON.stringify({ command: 'undo_to', step: 0 });
      if (arg) return JSON.stringify({ command: 'undo_to', step: parseInt(arg, 10) });
      return '{"command":"undo"}';
    case 'apply':
    case 'a': {
      const n = parseInt(arg, 10);
      if (isNaN(n)) { appendReplEntry(raw, formatError('usage: apply <n>')); return null; }
      return JSON.stringify({ command: 'step', choice: n });
    }
    case 'auto': {
      const n = parseInt(arg, 10);
      if (isNaN(n) || n < 0) { appendReplEntry(raw, formatError('usage: auto <n>')); return null; }
      return JSON.stringify({ command: 'auto', max_steps: n });
    }
    case 'restart':  return JSON.stringify({ command: 'undo_to', step: 0 });
    case 'rules':
    case 'r':        return '{"command":"list_rules"}';
    case 'history':
    case 'h':        return '{"command":"history"}';
    case 'types':    return '{"command":"types"}';
    case 'type':
      if (!arg) { appendReplEntry(raw, formatError('usage: type <name>')); return null; }
      return JSON.stringify({ command: 'type', name: arg });
    case 'homology':
      if (!arg) { appendReplEntry(raw, formatError('usage: homology <name>')); return null; }
      return JSON.stringify({ command: 'homology', name: arg });
    case 'store':
      if (!arg) { appendReplEntry(raw, formatError('usage: store <name>')); return null; }
      return JSON.stringify({ command: 'store', name: arg });
    case 'parallel':
      if (arg === 'on')  return JSON.stringify({ command: 'parallel', on: true });
      if (arg === 'off') return JSON.stringify({ command: 'parallel', on: false });
      appendReplEntry(raw, formatError('usage: parallel on|off'));
      return null;
    default:
      appendReplEntry(raw, formatError(`unknown command '${cmd}' — type help for commands`));
      return null;
  }
}

// ── Rendering ─────────────────────────────────────────────────────────────────

function renderCommandResult(cmd, data) {
  if (!data) return formatError('(no data)');

  switch (cmd) {
    case 'types':          return renderTypes(data);
    case 'type':           return renderTypeDetail(data.type_detail);
    case 'rules': case 'r':     return renderRules(data.rules);
    case 'history': case 'h':   return renderHistory(data.history);
    case 'store':          return renderStore(data);
    case 'homology':       return renderHomology(data);
    case 'auto':           return renderAuto(data);
    default:               return renderState(data);
  }
}

function renderAuto(data) {
  const info = data && data.auto;
  const applied = info ? info.applied : 0;
  const reason = info && info.stop_reason ? ` (${info.stop_reason})` : '';
  const summary = dim(`applied ${applied} step${applied === 1 ? '' : 's'}${reason}`);
  const state = renderState(data);
  return state ? `${summary}\n${state}` : summary;
}

function renderState(data) {
  if (!data) return '';
  let out = [];

  out.push(dim('step:') + ' ' + hi(data.step_count));

  const cur = data.current;
  if (cur) out.push(dim('current:') + ' ' + hi(cur.label || '—'));

  if (data.target) {
    const reached = data.target_reached;
    out.push(dim('target:') + ' ' + hi(data.target.label) +
      (reached ? ' ' + ok('✓ reached') : ''));
  }

  if (data.parallel) out.push(dim('parallel mode: on'));

  if (data.rewrites && data.rewrites.length > 0) {
    out.push('');
    out.push(sec('available rewrites:'));
    data.rewrites.forEach(r => {
      const isFamily = r.family && r.family.length > 0;
      const label = isFamily
        ? `${hi(r.rule_name)}  (parallel ×${r.family.length})`
        : `${hi(r.rule_name)}  ${src(r.source.label)} → ${tgt(r.target.label)}`;
      out.push(`  [${hi(r.index)}] ${label}`);
      if (r.match_display) {
        const highlighted = esc(r.match_display).replace(/\[([^\]]*)\]/g,
          '<span class="repl-src">$1</span>');
        out.push(`      match: ${highlighted}`);
      }
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
  if (d.generators && d.generators.length) {
    out.push(dim('generators:'));
    d.generators.forEach(g => {
      const bounds = g.source ? `  ${dim(g.source.label)} → ${dim(g.target.label)}` : '';
      out.push(`  ${hi(g.name)} ${dim(`(dim ${g.dim})`)}${bounds}`);
    });
  }
  if (d.diagrams && d.diagrams.length) {
    out.push(dim('diagrams:'));
    d.diagrams.forEach(g => {
      const bounds = g.source ? `${hi(g.name)} : ${dim(g.source.label)} → ${dim(g.target.label)}` : hi(g.name);
      out.push(`  ${bounds}  = ${dim(g.expr)}`);
    });
  }
  if (d.maps && d.maps.length) {
    out.push(dim('maps:'));
    d.maps.forEach(m => {
      out.push(`  ${hi(m.name)} :: ${dim(m.domain)}`);
    });
  }
  return out.join('\n');
}



function renderRules(rules) {
  if (!rules || !rules.length) return dim('(no rules)');
  return rules.map(r =>
    `  ${hi(r.name)}  ${dim(r.source.label)} → ${dim(r.target.label)}`
  ).join('\n');
}

function renderStore(data) {
  if (!data || !data.stored) return formatError('store failed');
  const s = data.stored;
  let out = [ok(`Stored '${esc(s.def_name)}'.`)];
  out.push(`  let ${hi(s.def_name)} = ${dim(s.expr)}`);
  return out.join('\n');
}

function renderHomology(data) {
  if (!data || !data.homology) return dim('(no data)');
  if (data.homology.length === 0) return dim('(no generators)');
  const lines = data.homology.map(h =>
    `  ${dim('H')}${dim('_' + h.dim)} = ${hi(h.display)}`
  );
  lines.push(`  ${dim('χ')} = ${hi(String(data.euler_characteristic))}`);
  return lines.join('\n');
}

function renderHistory(hist) {
  if (!hist || !hist.length) return dim('(no moves yet)');
  return hist.map(h =>
    `  ${dim(h.step + '.')} ${hi(h.rule_name)} ${dim('[choice ' + h.choice + ']')}`
  ).join('\n');
}

async function updateVisInfo(data) {
  // When a session is active and no accordion item is selected,
  // show the current diagram in the analysis pane.
  if (!sessionActive || !data || !data.current) return;
  if (currentItem) return; // accordion item takes priority

  await showSessionDiagram(data);
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
function src(s) { return `<span class="repl-src">${esc(s)}</span>`; }
function tgt(s) { return `<span class="repl-tgt">${esc(s)}</span>`; }

// Plain-text messages (errors, status) — no HTML, use textContent.
function formatOk(msg)    { return { cls: 'repl-result ok',  text: msg }; }

// Errors come in two shapes:
//   - a plain string (call-site emitted message, e.g. "usage: apply <n>")
//   - a parsed REPL response `{message, diagnostics?}` from the engine
// When `diagnostics` is present we render a structured block with line:col
// and a source snippet; otherwise we fall back to the flat text path.
function formatError(messageOrResult) {
  if (typeof messageOrResult === 'string') {
    return { cls: 'repl-result err', text: messageOrResult };
  }
  const { message, diagnostics } = messageOrResult || {};
  if (!Array.isArray(diagnostics) || diagnostics.length === 0) {
    return { cls: 'repl-result err', text: message || 'unknown error' };
  }
  const html = diagnostics.map(renderDiagnostic).join('');
  return { cls: 'repl-result err', html };
}

function renderDiagnostic(d) {
  const kindLabel = `${d.kind} error`;
  const path = d.path ? ` in ${esc(d.path)}` : '';
  const loc = `line ${d.start.line}:${d.start.col}`;
  const header =
    `<div class="err-header">` +
      `<span class="err-kind">${esc(kindLabel)}</span>` +
      `<span class="err-loc">${esc(path)} at ${esc(loc)}</span>` +
      `<span class="err-msg"> — ${esc(d.message)}</span>` +
    `</div>`;
  const snippet = d.snippet
    ? `<pre class="err-snippet">${esc(d.snippet)}</pre>`
    : '';
  const notes = (d.notes || [])
    .map(n => `<div class="err-note">note: ${esc(n)}</div>`)
    .join('');
  return `<div class="err-block">${header}${snippet}${notes}</div>`;
}

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
    } else if (typeof result.html === 'string') {
      // structured-error object — pre-rendered HTML (escaped at build time)
      resEl.className = result.cls;
      resEl.innerHTML = result.html;
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
  apply <n> (a)    apply rewrite choice n
  auto <n>         apply up to n rewrites, always picking choice 0
  undo (u)         undo last step
  undo <n>         undo back to step n
  undo all         undo all steps
  restart          same as undo all
  show / status    show current state
  rules (r)        list all rewrite rules
  history (h)      show move history
  types            list all types in the file
  type <name>      inspect a type
  homology <name>  compute cellular homology of a type
  store <name>     store the current proof as a named diagram
  parallel on/off  toggle parallel rewrite matching
  help / ?         show this message

Keyboard: ↑/↓ navigate history · Ctrl+Enter evaluate file`;

// ── Examples, load/save, syntax highlighting ─────────────────────────────────
//
// Examples are served as plain HTTP files alongside the frontend:
//   examples/index.json   —  { "Theory": "Theory.ali",
//                              "YangBaxter": "topics/braided/YangBaxter.ali",
//                              ... }  or  { "error": "<message>" }
//   examples/<relpath>    —  file contents
//
// A file's **stem** (e.g. "Theory") is its language-level identity — that's
// what `include <name>` sees.  Subdirectories are purely organisational; the
// stem is globally unique across the tree, enforced by the server and the
// deploy workflow (duplicate stems → loud error, not silent shadowing).
//
// Under `alifib web [<dir>]`, the Rust server generates the manifest
// dynamically.  Under a static WASM deployment (GitHub Pages etc.), the
// manifest is a committed artefact produced by the deploy workflow from the
// same recursive scan.  Either way the frontend code is identical.

const EXAMPLES_BASE = 'examples';

// Index: stem → relative path (e.g. "YangBaxter" → "topics/braided/YangBaxter.ali").
// Populated once at boot by populateExamples().
let EXAMPLES_INDEX = null;

// Cache of stem → contents, filled lazily so `include <Name>` in the editor
// can be forwarded to the backend without a round-trip per include.
const EXAMPLE_CONTENTS = new Map();

async function populateExamples() {
  try {
    const resp = await fetch(`${EXAMPLES_BASE}/index.json`, { cache: 'no-store' });
    if (!resp.ok) return;
    const data = await resp.json();
    if (data && typeof data === 'object' && typeof data.error === 'string') {
      appendReplMsg('Examples unavailable: ' + data.error, 'repl-result err');
      return;
    }
    if (!data || typeof data !== 'object' || Array.isArray(data)) return;
    EXAMPLES_INDEX = data;
    const names = Object.keys(data).sort();
    selExamples.innerHTML = '<option value="">Examples…</option>';
    for (const name of names) {
      const opt = document.createElement('option');
      opt.value = name;
      opt.textContent = name;
      selExamples.appendChild(opt);
    }
  } catch (e) {
    appendReplMsg('Failed to load examples: ' + e, 'repl-result err');
  }
}

async function fetchExample(name) {
  if (EXAMPLE_CONTENTS.has(name)) return EXAMPLE_CONTENTS.get(name);
  const relPath = EXAMPLES_INDEX && EXAMPLES_INDEX[name];
  if (!relPath) return null;
  // Each path segment is already identifier-only (server validates, deploy
  // enforces), so no URL escaping is needed beyond `/` staying as-is.
  const resp = await fetch(`${EXAMPLES_BASE}/${relPath}`, { cache: 'no-store' });
  if (!resp.ok) return null;
  const text = await resp.text();
  EXAMPLE_CONTENTS.set(name, text);
  return text;
}

// Collects `include <Name>` references in the given source, fetches the
// matching examples (and their transitive includes), and returns a
// `<Name>.ali → content` map ready for load_source_with_modules.
async function collectIncludeModules(source) {
  const map = {};
  const pending = collectDirectIncludes(source);
  const seen = new Set();
  while (pending.length) {
    const name = pending.pop();
    if (seen.has(name)) continue;
    seen.add(name);
    const content = await fetchExample(name);
    if (content === null) continue;
    map[`${name}.ali`] = content;
    for (const next of collectDirectIncludes(content)) {
      if (!seen.has(next)) pending.push(next);
    }
  }
  return map;
}

const INCLUDE_RE = /(^|[,\s])include\s+([A-Za-z_][A-Za-z0-9_]*)\b/g;
function collectDirectIncludes(source) {
  const names = [];
  let m;
  while ((m = INCLUDE_RE.exec(source)) !== null) names.push(m[2]);
  return names;
}

selExamples.addEventListener('change', async () => {
  const name = selExamples.value;
  selExamples.value = '';
  if (!name) return;
  if (!confirmReplaceIfDirty()) return;
  const content = await fetchExample(name);
  if (content === null) {
    appendReplMsg(`Failed to fetch example: ${name}`, 'repl-result err');
    return;
  }
  currentFileHandle = null;
  currentFile = name;
  setEditorValue(content);
});

const hasFsAccess = typeof window.showOpenFilePicker === 'function';
const aliPickerTypes = [{
  description: 'alifib source',
  accept: { 'text/plain': ['.ali'] },
}];
let currentFileHandle = null;

btnLoad.addEventListener('click', async () => {
  if (!confirmReplaceIfDirty()) return;
  if (hasFsAccess) {
    try {
      const [handle] = await window.showOpenFilePicker({ types: aliPickerTypes });
      const file = await handle.getFile();
      const text = await file.text();
      currentFileHandle = handle;
      currentFile = file.name;
      setEditorValue(text);
    } catch (e) {
      if (e?.name === 'AbortError') return;
      appendReplMsg('Failed to open file: ' + (e?.message || e), 'repl-result err');
    }
  } else {
    fileInput.click();
  }
});
fileInput.addEventListener('change', () => {
  const f = fileInput.files && fileInput.files[0];
  fileInput.value = '';
  if (!f) return;
  const reader = new FileReader();
  reader.onload = () => {
    currentFileHandle = null;
    currentFile = f.name;
    setEditorValue(String(reader.result || ''));
  };
  reader.onerror = () => { appendReplMsg('Failed to read file: ' + reader.error, 'repl-result err'); };
  reader.readAsText(f);
});

btnSave.addEventListener('click', async () => {
  const content = editor.value;
  if (hasFsAccess) {
    try {
      let handle = currentFileHandle;
      if (!handle) {
        handle = await window.showSaveFilePicker({
          suggestedName: currentFileName() || 'untitled.ali',
          types: aliPickerTypes,
        });
        currentFileHandle = handle;
      }
      const writable = await handle.createWritable();
      await writable.write(content);
      await writable.close();
      currentFile = handle.name;
      savedSnapshot = content;
    } catch (e) {
      if (e?.name === 'AbortError') return;
      appendReplMsg('Failed to save file: ' + (e?.message || e), 'repl-result err');
    }
  } else {
    const defaultName = currentFileName() || 'untitled.ali';
    const name = window.prompt('Save as:', defaultName);
    if (!name) return;
    const blob = new Blob([content], { type: 'text/plain;charset=utf-8' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = name.endsWith('.ali') ? name : name + '.ali';
    document.body.appendChild(a);
    a.click();
    document.body.removeChild(a);
    setTimeout(() => URL.revokeObjectURL(url), 0);
    savedSnapshot = content;
    currentFile = a.download;
  }
});

let currentFile = null;
let savedSnapshot = '';

function currentFileName() { return currentFile; }

function confirmReplaceIfDirty() {
  if (editor.value === savedSnapshot) return true;
  return window.confirm('Replace the editor contents? Unsaved changes will be lost.');
}

function setEditorValue(text) {
  editor.value = text;
  savedSnapshot = text;
  updateHighlight();
}

// ── Syntax highlighting (overlay renderer) ───────────────────────────────────
//
// A hidden <pre> behind the textarea is repainted on every input.  Both
// layers share every metric that affects glyph advance — font, padding,
// line-height, tab-size, wrapping — so the rendered colours line up with the
// caret.  Scroll is mirrored from the textarea (the source of truth).

const ALI_KEYWORDS_CONTROL = new Set(['attach', 'along', 'include', 'assert']);
const ALI_KEYWORDS_OTHER   = new Set(['let', 'def', 'as', 'total', 'map']);
const ALI_KEYWORDS_BOUND   = new Set(['in', 'out']);

function escapeHtmlChar(ch) {
  switch (ch) {
    case '&': return '&amp;';
    case '<': return '&lt;';
    case '>': return '&gt;';
    default:  return ch;
  }
}

function escapeHtml(s) {
  let out = '';
  for (const ch of s) out += escapeHtmlChar(ch);
  return out;
}

// Returns an HTML string with <span class="tok-…"> wrappers.  The token set
// mirrors the VSCode TextMate grammar at editors/vscode/syntaxes/ali.tmLanguage.json.
function highlightAli(src) {
  let out = '';
  let i = 0;
  const n = src.length;

  const isIdentStart = c => /[A-Za-z_]/.test(c);
  const isIdentPart  = c => /[A-Za-z0-9_]/.test(c);
  const isDigit      = c => /[0-9]/.test(c);

  while (i < n) {
    const c = src[i];

    // Block comment (* ... *), with balanced nesting.
    if (c === '(' && src[i + 1] === '*') {
      let depth = 1;
      let j = i + 2;
      while (j < n && depth > 0) {
        if (src[j] === '(' && src[j + 1] === '*') { depth++; j += 2; }
        else if (src[j] === '*' && src[j + 1] === ')') { depth--; j += 2; }
        else { j++; }
      }
      out += `<span class="tok-comment">${escapeHtml(src.slice(i, j))}</span>`;
      i = j;
      continue;
    }

    // Decorators: @Type, @Name, @Name.Sub
    if (c === '@') {
      let j = i + 1;
      while (j < n && (isIdentPart(src[j]) || src[j] === '.')) j++;
      const rest = src.slice(i + 1, j);
      if (rest === 'Type') {
        out += `<span class="tok-deco">${escapeHtml(src.slice(i, j))}</span>`;
      } else {
        out += `<span class="tok-deco">@</span><span class="tok-decoId">${escapeHtml(rest)}</span>`;
      }
      i = j;
      continue;
    }

    // Type-definition head:   Name <<=
    if (isIdentStart(c)) {
      let j = i + 1;
      while (j < n && isIdentPart(src[j])) j++;
      const word = src.slice(i, j);

      // Lookahead, skipping horizontal whitespace, for `<<=`.
      let k = j;
      while (k < n && (src[k] === ' ' || src[k] === '\t')) k++;
      const isTypeHead = src[k] === '<' && src[k + 1] === '<' && src[k + 2] === '=';

      let cls = null;
      if (ALI_KEYWORDS_CONTROL.has(word))      cls = 'tok-keyword';
      else if (ALI_KEYWORDS_OTHER.has(word))   cls = 'tok-keyword';
      else if (ALI_KEYWORDS_BOUND.has(word))   cls = 'tok-bound';
      else if (isTypeHead)                     cls = 'tok-typehead';

      out += cls ? `<span class="${cls}">${escapeHtml(word)}</span>` : escapeHtml(word);
      i = j;
      continue;
    }

    // Numbers
    if (isDigit(c)) {
      let j = i + 1;
      while (j < n && isDigit(src[j])) j++;
      out += `<span class="tok-num">${escapeHtml(src.slice(i, j))}</span>`;
      i = j;
      continue;
    }

    // Multi-char operators
    if (c === '<' && src[i + 1] === '<' && src[i + 2] === '=') {
      out += `<span class="tok-arrow">&lt;&lt;=</span>`;
      i += 3;
      continue;
    }
    if (c === '-' && src[i + 1] === '>') {
      out += `<span class="tok-arrow">-&gt;</span>`;
      i += 2;
      continue;
    }
    if (c === '=' && src[i + 1] === '>') {
      out += `<span class="tok-arrow">=&gt;</span>`;
      i += 2;
      continue;
    }
    if (c === ':' && src[i + 1] === ':') {
      out += `<span class="tok-op">::</span>`;
      i += 2;
      continue;
    }

    // Single-char tokens
    if (c === '?') { out += `<span class="tok-hole">?</span>`; i++; continue; }
    if (c === '#') { out += `<span class="tok-arrow">#</span>`; i++; continue; }
    if (c === '=') { out += `<span class="tok-arrow">=</span>`; i++; continue; }
    if (c === '.' || c === ',' || c === ':' || c === ';') {
      out += `<span class="tok-punct">${escapeHtmlChar(c)}</span>`;
      i++;
      continue;
    }
    if (c === '(' || c === ')' || c === '[' || c === ']' || c === '{' || c === '}') {
      out += `<span class="tok-punct">${c}</span>`;
      i++;
      continue;
    }

    // Whitespace and everything else passes through.
    out += escapeHtmlChar(c);
    i++;
  }

  return out;
}

function updateHighlight() {
  // Trailing newline keeps the highlight layer's last line visible so the
  // textarea's caret at EOF still sits on a rendered glyph row.
  const text = editor.value;
  editorHighlight.innerHTML = highlightAli(text) + '\n';
  syncScroll();
}

function syncScroll() {
  editorHighlight.scrollTop = editor.scrollTop;
  editorHighlight.scrollLeft = editor.scrollLeft;
}

editor.addEventListener('input', updateHighlight);
editor.addEventListener('scroll', syncScroll);

// ── Default example ───────────────────────────────────────────────────────────

editor.value = `(*
    A minimal setup for the topological Eckmann-Hilton argument.
    In type TwoCells, construct a diagram with source A.cell B.cell
    and target B.cell A.cell.
*)

@Type
Equation <<= {
    s0,
    t0,

    s1: s0 -> t0,
    t1: s0 -> t0,

    lhs: s1 -> t1,
    rhs: s1 -> t1,

    dir: lhs -> rhs,
    inv: rhs -> lhs
},

Unit <<= {
    pt,

    ob: pt -> pt,

    id: ob -> ob,

    attach Id_id :: Equation along [
        lhs => id id,
        rhs => id
    ],

    merge: ob ob -> ob,
    split: ob -> ob ob,

    attach Split_merge :: Equation along [
        lhs => split merge,
        rhs => id
    ]
},

Cell <<= Unit {
    cell: ob -> ob,

    attach Cell_id :: Equation along [
        lhs => cell id,
        rhs => cell
    ],
    attach Id_cell :: Equation along [
        lhs => id cell,
        rhs => cell
    ],

    attach Left_split :: Equation along [
        lhs => cell split,
        rhs => split (cell ob)
    ],
    attach Right_split :: Equation along [
        lhs => cell split,
        rhs => split (ob cell)
    ],
    attach Left_merge :: Equation along [
        lhs => merge cell,
        rhs => (cell ob) merge
    ],
    attach Right_merge :: Equation along [
        lhs => merge cell,
        rhs => (ob cell) merge
    ]
},

TwoCells <<= Unit {
    attach A :: Cell along [ Unit => Unit ],
    attach B :: Cell along [ Unit => Unit ]
}
`;
savedSnapshot = editor.value;
updateHighlight();

async function refreshAccordion() {
  if (!repl) return;
  const result = await parseReplResponse(repl.get_types());
  if (result.status !== 'ok') return;
  const types = result.data.types || [];
  tagFaces.clear();
  for (const t of types) {
    for (const g of t.generators) {
      if (g.tag != null) tagFaces.set(g.tag, (g.face_tags || []).filter(ft => ft != null));
    }
  }
  recomputeFullyThin();
  fileOutput.innerHTML = '';
  fileOutput.hidden = types.length === 0;
  types.forEach(t => fileOutput.appendChild(buildTypeAccordion(t)));
  syncAnalysisLayout();
}

// ── Session diagram display ──────────────────────────────────────────────────

async function showSessionDiagram(data) {
  selectedRewrite = null;
  previewActive = false;

  // Fetch strdiag for current diagram.
  if (!repl) return;
  const strResult = await parseReplResponse(repl.get_session_strdiag());
  if (strResult.status !== 'ok') return;

  sessionStrdiag = strResult.data;

  // Show infobox with session info.
  infobox.hidden = false;
  boundaryControls.hidden = true;

  let html = '';
  if (data.step_count > 0) html += `<button id="btn-undo-vis" class="btn-undo-vis btn-secondary" title="Undo">&#x21A9;</button>`;
  html += `<span class="infobox-qual">Current diagram</span>`;
  html += `<div class="infobox-name">${hi(data.current.label || '—')} <span class="acc-dim">dim ${data.current.dim}, step ${data.step_count}</span></div>`;
  if (data.target) {
    const reached = data.target_reached ? ` <span class="repl-ok">reached</span>` : '';
    html += `<div class="infobox-boundary">target: ${esc(data.target.label)}${reached}</div>`;
  }
  infoboxText.innerHTML = html;
  const btnUndo = document.getElementById('btn-undo-vis');
  if (btnUndo) btnUndo.addEventListener('click', () => { void performUndo(); });

  // Render the string diagram.
  currentLayout = layoutStrDiag(sessionStrdiag, selOrientation.value);
  visContainer.hidden = false;
  visControls.hidden = false;
  resizeAndRender();

  // Build rewrite list.
  buildRewriteList(data.rewrites || []);
  syncAnalysisLayout();
}

function buildRewriteList(rewrites) {
  rewriteList.innerHTML = '';
  lastRewriteData = rewrites;
  if (!rewrites.length) {
    rewriteList.hidden = true;
    return;
  }
  rewriteList.hidden = false;

  rewrites.forEach((r, i) => {
    const row = document.createElement('div');
    row.className = 'rewrite-row';

    const isFamily = r.family && r.family.length > 0;
    const content = document.createElement('span');
    content.className = 'rw-content';
    content.innerHTML = `<span class="rw-index">${r.index}</span>`
      + `<span class="rw-name">${esc(r.rule_name)}</span>`
      + (isFamily ? ` <span class="rw-parallel-badge">parallel ×${r.family.length}</span>` : '');
    row.appendChild(content);

    // Build action buttons (always present, shown on hover via CSS).
    const actions = document.createElement('span');
    actions.className = 'rewrite-actions';

    const btnPreview = document.createElement('button');
    btnPreview.className = 'rw-btn-preview';
    btnPreview.textContent = 'Preview';
    btnPreview.addEventListener('mousedown', (e) => {
      e.stopPropagation();
      previewActive = true;
      void showRewritePreview(i);
    });
    btnPreview.addEventListener('mouseup', () => {
      previewActive = false;
      endRewritePreview();
    });
    btnPreview.addEventListener('mouseleave', () => {
      if (previewActive) { previewActive = false; endRewritePreview(); }
    });
    btnPreview.addEventListener('click', (e) => e.stopPropagation());

    actions.appendChild(btnPreview);
    row.appendChild(actions);

    // Click anywhere on row (except Preview) applies the rewrite.
    row.addEventListener('click', () => { void applyRewrite(i); });

    // Hover: highlight match positions.
    row.addEventListener('mouseenter', () => {
      if (previewActive) return;
      selectedRewrite = i;
      if (currentLayout) {
        currentLayout._highlightPositions = r.match_positions;
        resizeAndRender();
      }
    });
    row.addEventListener('mouseleave', () => {
      if (previewActive) return;
      selectedRewrite = null;
      if (currentLayout) {
        currentLayout._highlightPositions = null;
        resizeAndRender();
      }
    });

    rewriteList.appendChild(row);
  });
}

let savedLayoutBeforePreview = null;

async function showRewritePreview(choice) {
  if (!repl) return;
  // Save the current layout (including any drag modifications) before switching.
  savedLayoutBeforePreview = currentLayout;
  const result = await parseReplResponse(repl.get_rewrite_preview_strdiag(choice));
  if (result.status !== 'ok') return;
  currentLayout = layoutStrDiag(result.data, selOrientation.value);
  currentLayout._highlightPositions = null;
  resizeAndRender();
}

function endRewritePreview() {
  // Restore the saved layout (with any drag modifications intact).
  if (savedLayoutBeforePreview) {
    currentLayout = savedLayoutBeforePreview;
    savedLayoutBeforePreview = null;
    if (selectedRewrite !== null && lastRewriteData && lastRewriteData[selectedRewrite]) {
      currentLayout._highlightPositions = lastRewriteData[selectedRewrite].match_positions;
    }
    resizeAndRender();
  }
}

async function performUndo() {
  if (!repl) return;
  const result = await parseReplResponse(repl.run_command('{"command":"undo"}'));
  if (result.status === 'error') {
    appendReplMsg('Undo error: ' + result.message, 'repl-result err');
    return;
  }
  appendReplEntry('undo', renderState(result.data));
  selectedRewrite = null;
  previewActive = false;
  await showSessionDiagram(result.data);
}

async function applyRewrite(choice) {
  // Send apply command through the REPL.
  const json = JSON.stringify({ command: 'step', choice });
  const result = await parseReplResponse(repl.run_command(json));
  if (result.status === 'error') {
    appendReplMsg('Apply error: ' + result.message, 'repl-result err');
    return;
  }
  appendReplEntry(`apply ${choice}`, renderState(result.data));
  selectedRewrite = null;
  previewActive = false;
  // Update the session display.
  await showSessionDiagram(result.data);
}

// Store last rewrite data for re-highlighting after preview ends.
let lastRewriteData = null;

// ── String diagram visualisation ─────────────────────────────────────────────

let currentItem = null;   // { typeName, item }
let currentItemDim = null; // dimension of the main diagram
let sessionStrdiag = null; // strdiag data for current session diagram
let selectedRewrite = null; // index of selected rewrite
let previewActive = false;

async function selectItem(typeName, item) {
  currentItem = { typeName, item };
  infobox.hidden = false;
  rewriteList.hidden = true; // hide session rewrite list when inspecting an item

  // For generators and diagrams: fetch dimension from the main strdiag response
  if (item.kind !== 'map' && repl) {
    const mainResult = await parseReplResponse(
      repl.get_strdiag(typeName, item.name, undefined, undefined)
    );
    if (mainResult.status === 'ok') {
      currentItemDim = mainResult.data.dim;
    } else {
      currentItemDim = item.dim || 0;
    }
  } else {
    currentItemDim = item.dim || 0;
  }

  // Populate boundary selector
  if (item.kind !== 'map' && currentItemDim >= 1) {
    selBoundary.innerHTML = '<option value="main">Main</option>';
    for (let k = currentItemDim - 1; k >= 0; k--) {
      const opt = document.createElement('option');
      opt.value = String(k);
      opt.textContent = `${k}-boundary`;
      selBoundary.appendChild(opt);
    }
    boundaryControls.hidden = false;
    selBoundary.value = 'main';
    setSignControlsEnabled(false);
  } else {
    boundaryControls.hidden = true;
  }

  await refreshInfobox();
}

async function refreshInfobox() {
  if (!currentItem) return;
  const { typeName, item } = currentItem;
  const bdVal = selBoundary.value;
  const isBoundary = bdVal !== 'main' && item.kind !== 'map';
  const bdDim = isBoundary ? parseInt(bdVal, 10) : null;
  const bdSign = isBoundary ? document.querySelector('input[name="bd-sign"]:checked').value : null;

  // Build infobox text
  const qualPrefix = item.kind === 'generator' ? 'Generator of'
                   : item.kind === 'diagram'   ? 'Diagram at'
                   : 'Map at';
  let displayName;
  if (isBoundary) {
    const signLabel = bdSign === 'output' ? 'Output' : 'Input';
    displayName = `${signLabel} ${bdDim}-boundary of ${item.name}`;
  } else {
    displayName = item.name;
  }

  let html = `<span class="infobox-qual">${esc(qualPrefix)} ${hi(typeName)}</span>`;
  html += `<div class="infobox-name">${hi(displayName)}`;
  if (!isBoundary && item.kind === 'generator') html += ` <span class="acc-dim">dim ${item.dim}</span>`;
  html += `</div>`;

  if (item.kind === 'map') {
    html += `<div class="infobox-boundary">:: ${esc(item.domain)}</div>`;
    infoboxText.innerHTML = html;
    visContainer.hidden = true;
    visControls.hidden = true;
    currentLayout = null;
    syncAnalysisLayout();
    return;
  }

  // Fetch strdiag (with optional boundary)
  if (!repl) { infoboxText.innerHTML = html; return; }
  const result = await parseReplResponse(
    repl.get_strdiag(typeName, item.name, bdDim ?? undefined, bdSign ?? undefined)
  );
  if (result.status === 'error') {
    html += `<div class="infobox-boundary" style="color:var(--err)">${esc(result.message)}</div>`;
    infoboxText.innerHTML = html;
    visContainer.hidden = true;
    visControls.hidden = true;
    currentLayout = null;
    syncAnalysisLayout();
    return;
  }

  const data = result.data;
  if (data.label) {
    html += `<div class="infobox-label">${esc(data.label)}</div>`;
  }
  if (data.src || data.tgt) {
    html += `<div class="infobox-boundary">${esc(data.src)} → ${esc(data.tgt)}</div>`;
  }
  infoboxText.innerHTML = html;

  currentLayout = layoutStrDiag(data.strdiag, selOrientation.value);
  visContainer.hidden = false;
  visControls.hidden = false;
  resizeAndRender();
  syncAnalysisLayout();
}

function setSignControlsEnabled(enabled) {
  signControls.style.opacity = enabled ? '1' : '0.35';
  document.querySelectorAll('input[name="bd-sign"]').forEach(r => r.disabled = !enabled);
}

selBoundary.addEventListener('change', () => {
  const isBd = selBoundary.value !== 'main';
  setSignControlsEnabled(isBd);
  void refreshInfobox();
});
document.querySelectorAll('input[name="bd-sign"]').forEach(r =>
  r.addEventListener('change', () => { void refreshInfobox(); }));

selOrientation.addEventListener('change', () => {
  if (currentLayout) {
    currentLayout = layoutStrDiag(currentLayout._raw, selOrientation.value);
    resizeAndRender();
  }
});

// ── Layout ───────────────────────────────────────────────────────────────────

const NODE_R = 6;
const WIRE_R = 3;
const PAD = 0;

function layoutStrDiag(data, orientation = 'bt') {
  const n = data.vertices.length;
  if (n === 0) return { _raw: data, verts: [], pos: [], orientation, hAdj: [], wAdj: [], dAdj: [], hPred: [], wPred: [] };

  const hAdj = buildAdj(n, data.height.edges);
  const hPred = buildPred(n, data.height.edges);
  const wAdj = buildAdj(n, data.width.edges);
  const wPred = buildPred(n, data.width.edges);
  const dAdj = buildAdj(n, data.depth.edges);

  const hDist = longestPathDistances(n, hAdj, hPred);
  const wDist = longestPathDistances(n, wAdj, wPred);

  // Store positions in abstract (w, h) space. Each vertex is centered within
  // its band: pos = (backward + 1) / (backward + forward + 2).
  const pos = data.vertices.map((v, i) => ({
    w: (wDist.bw[i] + 1) / (wDist.bw[i] + wDist.fw[i] + 2),
    h: (hDist.bw[i] + 1) / (hDist.bw[i] + hDist.fw[i] + 2),
  }));

  separateOverlaps(pos, n);

  return { _raw: data, verts: data.vertices, pos, orientation, hAdj, hPred, wAdj, wPred, dAdj,
           numWires: data.num_wires, numNodes: data.num_nodes, depthEdges: data.depth.edges };
}

// Convert abstract (w, h) to screen normalised (x, y) in [0,1]^2.
function toScreen(p, o) {
  switch (o) {
    case 'bt': return { x: p.w, y: 1 - p.h };
    case 'tb': return { x: p.w, y: p.h };
    case 'lr': return { x: p.h, y: 1 - p.w };
    case 'rl': return { x: 1 - p.h, y: 1 - p.w };
    default:   return { x: p.w, y: 1 - p.h };
  }
}

// Convert screen normalised (x, y) back to abstract (w, h).
function fromScreen(sx, sy, o) {
  switch (o) {
    case 'bt': return { w: sx, h: 1 - sy };
    case 'tb': return { w: sx, h: sy };
    case 'lr': return { w: 1 - sy, h: sx };
    case 'rl': return { w: 1 - sy, h: 1 - sx };
    default:   return { w: sx, h: 1 - sy };
  }
}

function buildAdj(n, edges) {
  const a = Array.from({length: n}, () => []);
  for (const [u, v] of edges) a[u].push(v);
  return a;
}
function buildPred(n, edges) {
  const a = Array.from({length: n}, () => []);
  for (const [u, v] of edges) a[v].push(u);
  return a;
}

/// Compute both forward (from sources) and backward (from sinks) longest-path
/// distances for a DAG. Returns { bw, fw } where bw[i] is the longest path from
/// any source to i, and fw[i] is the longest path from i to any sink.
function longestPathDistances(n, succ, pred) {
  // Forward: longest path from any source to each vertex.
  const bw = new Array(n).fill(0);
  const indeg = new Array(n).fill(0);
  for (let u = 0; u < n; u++) for (const v of succ[u]) indeg[v]++;
  const q = [];
  for (let i = 0; i < n; i++) if (indeg[i] === 0) q.push(i);
  const fwdOrder = [];
  while (q.length) {
    const u = q.shift();
    fwdOrder.push(u);
    for (const v of succ[u]) { if (--indeg[v] === 0) q.push(v); }
  }
  for (const u of fwdOrder) {
    for (const v of succ[u]) {
      bw[v] = Math.max(bw[v], bw[u] + 1);
    }
  }

  // Backward: longest path from each vertex to any sink (reverse topo order).
  const fw = new Array(n).fill(0);
  for (let k = fwdOrder.length - 1; k >= 0; k--) {
    const u = fwdOrder[k];
    for (const v of succ[u]) {
      fw[u] = Math.max(fw[u], fw[v] + 1);
    }
  }

  return { bw, fw };
}

/// Nudge vertices that are exactly (or nearly exactly) coincident.
/// Only acts on vertices closer than EPSILON; does not enforce a global minimum distance.
function separateOverlaps(pos, n) {
  const EPSILON = 0.001;  // threshold for nearly-coincident vertices
  const SPREAD = 0.08;    // how far apart to push overlapping vertices

  // Group coincident vertices.
  const visited = new Array(n).fill(false);
  for (let i = 0; i < n; i++) {
    if (visited[i]) continue;
    const group = [i];
    for (let j = i + 1; j < n; j++) {
      if (visited[j]) continue;
      const dist = Math.hypot(pos[j].w - pos[i].w, pos[j].h - pos[i].h);
      if (dist < EPSILON) {
        group.push(j);
        visited[j] = true;
      }
    }
    if (group.length <= 1) continue;
    // Spread the group on a small circle around their centroid.
    const cw = group.reduce((s, k) => s + pos[k].w, 0) / group.length;
    const ch = group.reduce((s, k) => s + pos[k].h, 0) / group.length;
    const r = SPREAD * (group.length - 1) / (2 * Math.PI);
    for (let gi = 0; gi < group.length; gi++) {
      const angle = (2 * Math.PI * gi) / group.length;
      pos[group[gi]].w = cw + r * Math.cos(angle);
      pos[group[gi]].h = ch + r * Math.sin(angle);
    }
  }
}

// ── Rendering ────────────────────────────────────────────────────────────────

function resizeAndRender() {
  if (!currentLayout) return;
  const rect = visContainer.getBoundingClientRect();
  const dpr = window.devicePixelRatio || 1;
  const w = rect.width, h = rect.height;
  if (w < 1 || h < 1) return;
  visCanvas.width = w * dpr;
  visCanvas.height = h * dpr;
  visCanvas.style.width = w + 'px';
  visCanvas.style.height = h + 'px';
  canvasCtx.setTransform(dpr, 0, 0, dpr, 0, 0);
  renderStrDiag(canvasCtx, currentLayout, w, h);
}

const resizeObs = new ResizeObserver(() => resizeAndRender());
resizeObs.observe(document.getElementById('vis-container'));

function renderStrDiag(ctx, L, cw, ch) {
  ctx.clearRect(0, 0, cw, ch);
  if (!L || !L.verts.length) return;

  const o = L.orientation;
  const isVert = (o === 'bt' || o === 'tb');

  // Map abstract positions to canvas pixels via screen normalised coords.
  const px = L.pos.map(p => {
    const s = toScreen(p, o);
    return { x: PAD + s.x * (cw - 2 * PAD), y: PAD + s.y * (ch - 2 * PAD) };
  });

  const wireColor = '#d4d4d8';
  const thinColor = '#505058';
  const BORDER_W = 6;
  const WIRE_W = 2;

  function strokeWirePaths(wi) {
    const wp = px[wi];
    const sources = L.hPred[wi].length > 0
      ? L.hPred[wi].map(pi => px[pi])
      : [entryPoint(wp, o, 'input', cw, ch)];
    const targets = L.hAdj[wi].length > 0
      ? L.hAdj[wi].map(si => px[si])
      : [entryPoint(wp, o, 'output', cw, ch)];
    for (const src of sources) {
      for (const tgt of targets) {
        const q0 = isVert ? { x: wp.x, y: src.y } : { x: src.x, y: wp.y };
        const q1 = isVert ? { x: wp.x, y: tgt.y } : { x: tgt.x, y: wp.y };
        ctx.beginPath();
        ctx.moveTo(src.x, src.y);
        ctx.quadraticCurveTo(q0.x, q0.y, wp.x, wp.y);
        ctx.quadraticCurveTo(q1.x, q1.y, tgt.x, tgt.y);
        ctx.stroke();
      }
    }
  }

  function drawWire(wi) {
    const wireThin = L.verts[wi].tag != null && thinTags.has(L.verts[wi].tag);
    ctx.strokeStyle = wireThin ? thinColor : wireColor;
    ctx.lineWidth = WIRE_W;
    ctx.lineCap = 'round';
    strokeWirePaths(wi);
    if (!wireThin) {
      ctx.beginPath();
      ctx.arc(px[wi].x, px[wi].y, WIRE_R, 0, Math.PI * 2);
      ctx.fillStyle = wireColor;
      ctx.fill();
    }
  }

  if (L.depthEdges.length > 0) {
    // Layer-based rendering: crossing gaps only between depth-related wires.
    const depthLevel = new Array(L.numWires).fill(0);
    const dTopoOrder = topoSort(L.numWires, L.dAdj);
    for (const u of dTopoOrder) {
      for (const v of (L.dAdj[u] || [])) {
        if (v < L.numWires) depthLevel[v] = Math.max(depthLevel[v], depthLevel[u] + 1);
      }
    }
    const maxLevel = Math.max(0, ...depthLevel);
    const levels = Array.from({length: maxLevel + 1}, () => []);
    for (let i = 0; i < L.numWires; i++) levels[depthLevel[i]].push(i);

    for (let lv = 0; lv <= maxLevel; lv++) {
      if (lv > 0) {
        ctx.save();
        ctx.globalCompositeOperation = 'destination-out';
        ctx.strokeStyle = 'rgba(255,255,255,1)';
        ctx.lineWidth = WIRE_W + BORDER_W;
        ctx.lineCap = 'butt';
        for (const wi of levels[lv]) strokeWirePaths(wi);
        ctx.restore();
      }
      for (const wi of levels[lv]) drawWire(wi);
    }
  } else {
    for (let wi = 0; wi < L.numWires; wi++) drawWire(wi);
  }

  // Draw nodes
  const hlPositions = L._highlightPositions ? new Set(L._highlightPositions) : null;
  for (let i = L.numWires; i < L.verts.length; i++) {
    const np = px[i];
    const nodePos = i - L.numWires;
    const highlighted = hlPositions && hlPositions.has(nodePos);
    const nodeThin = L.verts[i].tag != null && thinTags.has(L.verts[i].tag);
    const nodeFullyThin = L.verts[i].tag != null && fullyThinTags.has(L.verts[i].tag);
    if (nodeThin && highlighted) {
      ctx.save();
      ctx.shadowColor = '#ffffff';
      ctx.shadowBlur = 14;
      ctx.beginPath();
      ctx.arc(np.x, np.y, WIRE_R, 0, Math.PI * 2);
      ctx.fillStyle = '#ffffff';
      ctx.fill();
      ctx.restore();
    } else if (nodeThin) {
      ctx.beginPath();
      ctx.arc(np.x, np.y, WIRE_R, 0, Math.PI * 2);
      ctx.fillStyle = nodeFullyThin ? thinColor : wireColor;
      ctx.fill();
    } else if (highlighted) {
      ctx.save();
      ctx.shadowColor = '#ffffff';
      ctx.shadowBlur = 14;
      ctx.beginPath();
      ctx.arc(np.x, np.y, NODE_R, 0, Math.PI * 2);
      ctx.fillStyle = '#ffffff';
      ctx.fill();
      ctx.restore();
    } else {
      ctx.beginPath();
      ctx.arc(np.x, np.y, NODE_R, 0, Math.PI * 2);
      ctx.fillStyle = '#7c6af2';
      ctx.fill();
      ctx.strokeStyle = '#ffffff';
      ctx.lineWidth = 1.5;
      ctx.stroke();
    }
  }

  // Draw labels
  ctx.font = '11px system-ui, sans-serif';
  for (let i = 0; i < L.verts.length; i++) {
    const p = px[i];
    const label = L.verts[i].label;
    if (!label) continue;
    const isNode = L.verts[i].kind === 'node';
    const labelThin = L.verts[i].tag != null && thinTags.has(L.verts[i].tag);
    ctx.fillStyle = labelThin ? thinColor : (isNode ? '#f4f4f5' : '#a1a1aa');
    const r = (isNode && !labelThin) ? NODE_R : WIRE_R;
    if (isNode) {
      if (isVert) {
        ctx.textAlign = 'left';
        ctx.textBaseline = 'bottom';
        ctx.fillText(label, p.x + r + 3, p.y - 2);
      } else {
        ctx.textAlign = 'left';
        ctx.textBaseline = 'bottom';
        ctx.fillText(label, p.x + 2, p.y - r - 3);
      }
    } else {
      if (isVert) {
        ctx.textAlign = 'left';
        ctx.textBaseline = 'middle';
        ctx.fillText(label, p.x + r + 4, p.y);
      } else {
        ctx.textAlign = 'center';
        ctx.textBaseline = 'bottom';
        ctx.fillText(label, p.x, p.y - r - 3);
      }
    }
  }
}

function entryPoint(wp, orientation, side, cw, ch) {
  const isInput = side === 'input';
  switch (orientation) {
    case 'bt': return { x: wp.x, y: isInput ? ch : 0 };
    case 'tb': return { x: wp.x, y: isInput ? 0 : ch };
    case 'lr': return { x: isInput ? 0 : cw, y: wp.y };
    case 'rl': return { x: isInput ? cw : 0, y: wp.y };
    default:   return { x: wp.x, y: isInput ? ch : 0 };
  }
}

function topoSort(numWires, dAdj) {
  const indeg = new Array(numWires).fill(0);
  for (let u = 0; u < numWires; u++) {
    for (const v of (dAdj[u] || [])) {
      if (v < numWires) indeg[v]++;
    }
  }
  const q = [];
  for (let i = 0; i < numWires; i++) if (indeg[i] === 0) q.push(i);
  const order = [];
  const visited = new Set();
  while (q.length) {
    const u = q.shift();
    order.push(u);
    visited.add(u);
    for (const v of (dAdj[u] || [])) {
      if (v < numWires && --indeg[v] === 0) q.push(v);
    }
  }
  for (let i = 0; i < numWires; i++) if (!visited.has(i)) order.push(i);
  return order;
}

// ── Drag interaction ─────────────────────────────────────────────────────────

visCanvas.addEventListener('mousedown', (e) => {
  if (!currentLayout) return;
  const rect = visCanvas.getBoundingClientRect();
  const mx = e.clientX - rect.left, my = e.clientY - rect.top;
  const cw = rect.width, ch = rect.height;
  const L = currentLayout;
  const pxArr = L.pos.map(p => {
    const s = toScreen(p, L.orientation);
    return { x: PAD + s.x * (cw - 2 * PAD), y: PAD + s.y * (ch - 2 * PAD) };
  });
  let best = -1, bestD = 25;
  for (let i = 0; i < L.verts.length; i++) {
    const d = Math.hypot(mx - pxArr[i].x, my - pxArr[i].y);
    if (d < bestD) { bestD = d; best = i; }
  }
  if (best >= 0) {
    // BFS from dragged vertex through height graph to compute influence weights.
    const n = L.verts.length;
    const influence = new Array(n).fill(0);
    influence[best] = 1;
    const DECAY = 0.5;
    const visited = new Set([best]);
    let frontier = [best];
    let weight = DECAY;
    while (frontier.length > 0 && weight > 0.01) {
      const next = [];
      for (const v of frontier) {
        // Height graph neighbours (both directions).
        for (const nb of [...(L.hAdj[v] || []), ...(L.hPred[v] || [])]) {
          if (!visited.has(nb)) {
            visited.add(nb);
            influence[nb] = weight;
            next.push(nb);
          }
        }
      }
      frontier = next;
      weight *= DECAY;
    }
    // Record initial positions for all influenced vertices.
    const initPos = L.pos.map(p => ({ w: p.w, h: p.h }));
    dragState = { idx: best, influence, initPos };
    e.preventDefault();
  }
});

visCanvas.addEventListener('mousemove', (e) => {
  if (!dragState || !currentLayout) return;
  const rect = visCanvas.getBoundingClientRect();
  const mx = e.clientX - rect.left, my = e.clientY - rect.top;
  const cw = rect.width, ch = rect.height;

  const sx = (mx - PAD) / (cw - 2 * PAD);
  const sy = (my - PAD) / (ch - 2 * PAD);
  const mouseAbs = fromScreen(sx, sy, currentLayout.orientation);

  const L = currentLayout;
  const i = dragState.idx;
  function clamp(val, limit, mustBeLess) {
    if (mustBeLess ? val > limit : val < limit) return limit;
    return val;
  }

  // Compute the delta of the dragged vertex (clamped by its own constraints).
  let dragW = mouseAbs.w;
  let dragH = mouseAbs.h;
  for (const s of L.hAdj[i])  dragH = clamp(dragH, dragState.initPos[s].h, true);
  for (const p of L.hPred[i]) dragH = clamp(dragH, dragState.initPos[p].h, false);
  for (const s of (L.wAdj[i] || []))  dragW = clamp(dragW, dragState.initPos[s].w, true);
  for (const p of (L.wPred[i] || [])) dragW = clamp(dragW, dragState.initPos[p].w, false);

  const dw = dragW - dragState.initPos[i].w;
  const dh = dragH - dragState.initPos[i].h;

  // Apply influence-weighted delta to all vertices, then clamp constraints.
  for (let v = 0; v < L.verts.length; v++) {
    const inf = dragState.influence[v];
    if (inf === 0) continue;
    let newW = dragState.initPos[v].w + dw * inf;
    let newH = dragState.initPos[v].h + dh * inf;
    // Clamp to this vertex's own constraints (using projected positions of
    // neighbours shifted by their own influence).
    for (const s of L.hAdj[v]) {
      const sH = dragState.initPos[s].h + dh * dragState.influence[s];
      newH = clamp(newH, sH, true);
    }
    for (const p of L.hPred[v]) {
      const pH = dragState.initPos[p].h + dh * dragState.influence[p];
      newH = clamp(newH, pH, false);
    }
    for (const s of (L.wAdj[v] || [])) {
      const sW = dragState.initPos[s].w + dw * dragState.influence[s];
      newW = clamp(newW, sW, true);
    }
    for (const p of (L.wPred[v] || [])) {
      const pW = dragState.initPos[p].w + dw * dragState.influence[p];
      newW = clamp(newW, pW, false);
    }
    L.pos[v] = { w: newW, h: newH };
  }

  resizeAndRender();
});

visCanvas.addEventListener('mouseup', () => { dragState = null; });
visCanvas.addEventListener('mouseleave', () => { dragState = null; });

// ── Init ──────────────────────────────────────────────────────────────────────

syncWorkspaceLayout();
syncAnalysisLayout();
boot();
