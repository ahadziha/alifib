import { EditorView, keymap, lineNumbers, drawSelection, highlightActiveLine, Decoration } from '@codemirror/view';
import { EditorState, Compartment, StateField, StateEffect } from '@codemirror/state';
import { defaultKeymap, insertTab, indentLess, history as cmHistory, historyKeymap } from '@codemirror/commands';
import { indentUnit, bracketMatching } from '@codemirror/language';
import { closeBrackets, closeBracketsKeymap } from '@codemirror/autocomplete';
import { search, searchKeymap, findNext } from '@codemirror/search';
import { aliExtensions } from './ali-lang.js';

// ── State ─────────────────────────────────────────────────────────────────────

let repl = null;
let sessionActive = false;
const replHistory = [];
let histIdx = -1;
let currentLayout = null;
let selectedEl = null;
let dragState = null;
let panState = null;
let zoomLevel = 1;
let splitterDrag = null;
const thinTags = new Set();
const tagFaces = new Map();
const fullyThinTags = new Set();
let proofView = false;
let proofLayout = null;
let proofBoundaryMap = null;
let openHoles = [];        // open holes of the module, from the `holes` command
let fillSession = null;    // { index, dim } when a fill is active

// ── Theme ────────────────────────────────────────────────────────────────────

const themeComp = new Compartment();

// ── Fill-clause flash highlight ───────────────────────────────────────────────
// A transient mark over the clause `done` inserts, cleared after a moment.

const setFlash = StateEffect.define();
const clearFlash = StateEffect.define();
const flashMark = Decoration.mark({ class: 'cm-fill-flash' });
const flashField = StateField.define({
  create() { return Decoration.none; },
  update(deco, tr) {
    deco = deco.map(tr.changes);
    for (const e of tr.effects) {
      if (e.is(setFlash)) deco = Decoration.set([flashMark.range(e.value.from, e.value.to)]);
      else if (e.is(clearFlash)) deco = Decoration.none;
    }
    return deco;
  },
  provide: f => EditorView.decorations.from(f),
});

function isDark() {
  return document.documentElement.dataset.theme !== 'light';
}

function themeExtensions() {
  const dark = isDark();
  return [
    ...aliExtensions(dark),
    EditorView.theme({
      '&': { height: '100%', backgroundColor: 'var(--bg)' },
      '.cm-scroller': { overflow: 'auto' },
    }, { dark }),
  ];
}

function canvasColors() {
  if (isDark()) {
    return {
      wire: '#d4d4d8', thin: '#505058',
      nodeFill: '#7c6af2', nodeStroke: '#ffffff',
      hlFill: '#ffffff', hlShadow: '#ffffff',
      labelNode: '#f4f4f5', labelWire: '#a1a1aa',
    };
  }
  return {
    wire: '#4a4740', thin: '#c5c0b5',
    nodeFill: '#4a7c7c', nodeStroke: '#2a2824',
    hlFill: '#fff8e0', hlShadow: '#f0a000',
    labelNode: '#2a2824', labelWire: '#6a6560',
  };
}

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
const editorWrap  = document.getElementById('editor-wrap');
const selExamples = document.getElementById('sel-examples');
const btnLoad     = document.getElementById('btn-load');
const btnSave     = document.getElementById('btn-save');
const fileInput   = document.getElementById('file-input');
const btnEval     = document.getElementById('btn-evaluate');
const fileOutput  = document.getElementById('file-output');
const selType     = document.getElementById('sel-type');
const inpSource   = document.getElementById('inp-source');
const inpTarget   = document.getElementById('inp-target');
const chkBackward = document.getElementById('chk-backward');
const btnStart    = document.getElementById('btn-start');
const sessionSetup = document.getElementById('session-setup');
const btnStop     = document.getElementById('btn-stop');
const sessionStop = document.getElementById('session-stop');
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
const appearanceBtn  = document.getElementById('appearance-btn');
const appearanceMenu = document.getElementById('appearance-menu');
const chkNodeLabels  = document.getElementById('chk-node-labels');
const chkWireLabels  = document.getElementById('chk-wire-labels');
const zoomSlider     = document.getElementById('zoom-slider');
const zoomLabel      = document.getElementById('zoom-label');
const btnZoomReset   = document.getElementById('btn-zoom-reset');
const rewriteResizer = document.getElementById('rewrite-resizer');
const rewriteList = document.getElementById('rewrite-list');
const canvasCtx   = visCanvas.getContext('2d');
const tabBar      = document.getElementById('tab-bar');
const btnNewTab   = document.getElementById('btn-new-tab');
const fileLabel   = document.getElementById('file-label');
const btnTheme    = document.getElementById('btn-theme');

// ── Editor state & tabs ──────────────────────────────────────────────────────

const editorTabs = {
  tabs: [],
  activeTabId: null,
};

let tabIdCounter = 0;
let untitledCounter = 0;

function activeTab() {
  return editorTabs.tabs.find(t => t.id === editorTabs.activeTabId) || null;
}

function tabNameExists(name, excludeTabId) {
  return editorTabs.tabs.some(t => t.id !== excludeTabId && t.name === name);
}

function nextUntitledName() {
  untitledCounter++;
  return `Untitled${untitledCounter}`;
}

function getEditorText() {
  return view.state.doc.toString();
}

function markDirty(tab) {
  const wasDirty = tab.dirty;
  tab.dirty = tab.cmState.doc.toString() !== tab.savedSnapshot;
  if (wasDirty !== tab.dirty) renderTabBar();
}

function markClean(tab, snapshot) {
  tab.savedSnapshot = snapshot;
  tab.dirty = false;
  renderTabBar();
}

function anyDirty() {
  return editorTabs.tabs.some(t => t.dirty);
}

window.addEventListener('beforeunload', (e) => {
  if (anyDirty()) e.preventDefault();
});

function makeEditorState(doc) {
  return EditorState.create({
    doc,
    extensions: [
      lineNumbers(),
      cmHistory(),
      drawSelection(),
      highlightActiveLine(),
      indentUnit.of('    '),
      bracketMatching(),
      closeBrackets(),
      search({ caseSensitive: true }),
      flashField,
      themeComp.of(themeExtensions()),
      keymap.of([
        ...closeBracketsKeymap,
        ...searchKeymap,
        ...defaultKeymap,
        ...historyKeymap,
        { key: 'Tab', run: insertTab },
        { key: 'Shift-Tab', run: indentLess },
        { key: 'Mod-Enter', run: () => { void evaluateSource(); return true; } },
      ]),
      EditorState.languageData.of(() => [{ commentTokens: { block: { open: '(*', close: '*)' } } }]),
      EditorView.updateListener.of(update => {
        if (update.docChanged) {
          const tab = activeTab();
          if (tab) {
            tab.cmState = update.state;
            markDirty(tab);
          }
        }
        if (update.transactions.some(tr => tr.isUserEvent('input.replace'))) {
          findNext(update.view);
        }
        for (const tf of update.view.dom.querySelectorAll('.cm-search .cm-textfield:not([autocomplete])')) {
          tf.setAttribute('autocomplete', 'off');
        }
      }),
    ],
  });
}

let view = null;

function createTab(name, content, fileHandle) {
  const id = String(++tabIdCounter);
  if (!name) name = nextUntitledName();
  const cmState = makeEditorState(content);
  const tab = { id, name, cmState, savedSnapshot: content, dirty: false, fileHandle };
  editorTabs.tabs.push(tab);
  if (!view) {
    view = new EditorView({ state: cmState, parent: editorWrap });
    editorTabs.activeTabId = id;
    renderTabBar();
  } else {
    switchTab(id);
  }
  return tab;
}

function switchTab(tabId) {
  if (editorTabs.activeTabId === tabId) return;
  const departing = activeTab();
  if (departing && view) {
    departing.cmState = view.state;
    const scroller = view.scrollDOM;
    departing.scrollTop = scroller.scrollTop;
    departing.scrollLeft = scroller.scrollLeft;
  }
  editorTabs.activeTabId = tabId;
  const arriving = activeTab();
  if (arriving && view) {
    view.setState(arriving.cmState);
    view.dispatch({ effects: themeComp.reconfigure(themeExtensions()) });
    if (arriving.scrollTop != null) {
      const top = arriving.scrollTop, left = arriving.scrollLeft;
      requestAnimationFrame(() => {
        view.scrollDOM.scrollTop = top;
        view.scrollDOM.scrollLeft = left;
      });
    }
  }
  renderTabBar();
}

function closeTab(tabId) {
  const tab = editorTabs.tabs.find(t => t.id === tabId);
  if (!tab) return;
  if (tab.dirty) {
    if (!window.confirm('Close tab? Unsaved changes will be lost.')) return;
  }
  const idx = editorTabs.tabs.indexOf(tab);
  editorTabs.tabs.splice(idx, 1);
  if (editorTabs.tabs.length === 0) {
    createTab(null, '', null);
    return;
  }
  if (editorTabs.activeTabId === tabId) {
    const nextIdx = Math.min(idx, editorTabs.tabs.length - 1);
    switchTab(editorTabs.tabs[nextIdx].id);
  } else {
    renderTabBar();
  }
}

function renderTabBar() {
  if (!tabBar) return;
  tabBar.querySelectorAll('.tab-item').forEach(el => el.remove());
  for (const tab of editorTabs.tabs) {
    const el = document.createElement('button');
    el.className = 'tab-item'
      + (tab.id === editorTabs.activeTabId ? ' tab-item--active' : '')
      + (tab.dirty ? ' tab-item--dirty' : '');
    el.dataset.tabId = tab.id;

    const nameSpan = document.createElement('span');
    nameSpan.className = 'tab-name';
    nameSpan.textContent = tab.name || 'untitled';
    el.appendChild(nameSpan);

    const closeBtn = document.createElement('span');
    closeBtn.className = 'tab-close';
    closeBtn.textContent = '×';
    closeBtn.addEventListener('click', e => { e.stopPropagation(); closeTab(tab.id); });
    el.appendChild(closeBtn);

    el.addEventListener('click', () => switchTab(tab.id));
    nameSpan.addEventListener('click', e => {
      if (tab.id === editorTabs.activeTabId) {
        e.stopPropagation();
        startTabRename(tab.id, nameSpan);
      }
    });
    tabBar.insertBefore(el, btnNewTab);
  }
}

function startTabRename(tabId, nameSpan) {
  const tab = editorTabs.tabs.find(t => t.id === tabId);
  if (!tab) return;
  const input = document.createElement('input');
  input.type = 'text';
  input.className = 'tab-rename';
  input.value = tab.name || '';
  input.placeholder = 'module name';
  nameSpan.replaceWith(input);
  input.focus();
  input.select();

  function commit() {
    const raw = input.value.trim();
    if (!raw) { renderTabBar(); return; }
    const name = stemFromFilename(raw);
    if (name && tabNameExists(name, tab.id)) {
      input.classList.add('tab-rename--error');
      input.title = `"${name}" is already open`;
      return;
    }
    tab.name = name || nextUntitledName();
    renderTabBar();
  }
  input.addEventListener('keydown', e => {
    if (e.key === 'Enter') { e.preventDefault(); commit(); }
    if (e.key === 'Escape') { e.preventDefault(); renderTabBar(); }
  });
  input.addEventListener('blur', commit);
}

// ── Boot ──────────────────────────────────────────────────────────────────────

class WasmBackend {
  constructor(inner) {
    this.inner = inner;
    this.label = 'WASM';
  }

  async reset() {
    this.inner.reset();
  }

  async stop_session() {
    this.inner.stop_session();
  }

  async load_source(source, modules, sourceName) {
    const modulesJson = modules ? JSON.stringify(modules) : null;
    return this.inner.load_source(source, modulesJson, sourceName || null);
  }

  async start_session(typeName, initial, target, backward = false) {
    return this.inner.start_session(typeName, initial, target ?? null, backward);
  }

  async resume_session(typeName, proof, target, backward = false) {
    return this.inner.resume_session(typeName, proof, target ?? null, backward);
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

  async get_target_strdiag() {
    return this.inner.get_target_strdiag();
  }

  async get_rewrite_preview_strdiag(choice) {
    return this.inner.get_rewrite_preview_strdiag(choice);
  }

  async get_map_image_strdiag(typeName, mapName, genName, boundaryDim, boundarySign) {
    return this.inner.get_map_image_strdiag(typeName, mapName, genName, boundaryDim, boundarySign);
  }

  async set_proof_view(on) {
    return this.inner.set_proof_view(on);
  }

  async get_proof_strdiag() {
    return this.inner.get_proof_strdiag();
  }
}

class HttpBackend {
  constructor(baseUrl = '') {
    this.baseUrl = baseUrl;
    this.label = 'HTTP';
  }

  async reset() {}

  async stop_session() {
    return this.post('/api/stop_session', {});
  }

  async load_source(source, modules, sourceName) {
    const body = { source, modules: modules || {} };
    if (sourceName) body.source_name = sourceName;
    return this.post('/api/load_source', body);
  }

  async start_session(typeName, initial, target, backward = false) {
    const body = { type_name: typeName, initial };
    if (target) body.target = target;
    if (backward) body.backward = true;
    return this.post('/api/start_session', body);
  }

  async resume_session(typeName, proof, target, backward = false) {
    const body = { type_name: typeName, proof };
    if (target) body.target = target;
    if (backward) body.backward = true;
    return this.post('/api/resume_session', body);
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

  async get_target_strdiag() {
    return this.post('/api/get_target_strdiag', {});
  }

  async get_rewrite_preview_strdiag(choice) {
    return this.post('/api/get_rewrite_preview_strdiag', { choice });
  }

  async get_map_image_strdiag(typeName, mapName, genName, boundaryDim, boundarySign) {
    return this.post('/api/get_map_image_strdiag', {
      type_name: typeName,
      map_name: mapName,
      gen_name: genName,
      boundary_dim: boundaryDim,
      boundary_sign: boundarySign,
    });
  }

  async set_proof_view(on) {
    return this.post('/api/set_proof_view', { on });
  }

  async get_proof_strdiag() {
    return this.post('/api/get_proof_strdiag', {});
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

  const wasm = await import('../pkg/alifib_wasm.js');
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
    appendReplMsg('\u{266B} No nit not, nit no not, nit nit folly bololey...', 'repl-dim');
    appendReplMsg(' ', 'repl-dim');
    appendReplMsg(`${repl.label} engine ready. Evaluate a file to begin.`, 'repl-dim');
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

// ── Theme toggle ─────────────────────────────────────────────────────────────

function applyTheme(dark) {
  if (dark) {
    delete document.documentElement.dataset.theme;
  } else {
    document.documentElement.dataset.theme = 'light';
  }
  btnTheme.textContent = dark ? 'Light' : 'Dark';
  localStorage.setItem('alifib-theme', dark ? 'dark' : 'light');
  if (view) {
    view.dispatch({ effects: themeComp.reconfigure(themeExtensions()) });
  }
  resizeAndRender();
}

btnTheme.addEventListener('click', () => applyTheme(!isDark()));

{
  const saved = localStorage.getItem('alifib-theme');
  if (saved === 'light') applyTheme(false);
}

// ── Evaluate ──────────────────────────────────────────────────────────────────

btnEval.addEventListener('click', () => { void evaluateSource(); });

async function evaluateSource(silent = false) {
  if (!repl) return;
  const src = getEditorText();
  if (!src.trim()) return;

  const previousType = selType.value;

  await repl.reset();
  const tab = activeTab();
  const tabName = tab ? tab.name : null;
  const modules = await collectIncludeModules(src, tabName);
  const result = await parseReplResponse(repl.load_source(src, modules, tabName));

  if (result.status === 'error') {
    fileOutput.innerHTML = '';
    fileOutput.hidden = true;
    appendReplEntry('', formatError(result));
    resetSession();
    sessionSetup.hidden = true;
    replInput.disabled = true;
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
    for (const tag of (t.thin_tags || [])) {
      if (tag != null) thinTags.add(tag);
    }
  }
  recomputeFullyThin();

  // Build accordion in file output area, grouped by module
  fileOutput.innerHTML = '';
  fileOutput.hidden = types.length === 0;
  buildModuleAccordion(types, fileOutput);

  await fetchHoles();

  selType.innerHTML = '<option value="">— select type —</option>';
  const sourceMod = tabName || 'source';
  const sourceTypes = types.filter(t => (t.module || 'source') === sourceMod);
  sourceTypes.forEach(t => {
    const opt = document.createElement('option');
    opt.value = opt.textContent = t.name;
    selType.appendChild(opt);
  });

  if (previousType && sourceTypes.some(t => t.name === previousType)) {
    selType.value = previousType;
  }

  resetSession();
  sessionSetup.hidden = false;
  replInput.disabled = false;
  syncAnalysisLayout();
  // Suppress the success notice for an implicit re-evaluation (e.g. after
  // `done`), so finalising a fill feels seamless; errors are always shown.
  if (!silent) {
    appendReplEntry('', formatOk(
      types.length
        ? `Loaded ${types.length} type${types.length !== 1 ? 's' : ''}.`
        : 'Loaded (no named types found).'
    ));
  }
}

// ── Holes ──────────────────────────────────────────────────────────────────

// Refresh the module's open holes from the backend.  Holes are shown in a map's
// infobox, not the accordion, so this just refreshes the cache.
async function fetchHoles() {
  openHoles = [];
  if (!repl) return null;
  const result = await parseReplResponse(repl.run_command('{"command":"holes"}'));
  if (result.status === 'ok' && result.data && Array.isArray(result.data.holes)) {
    openHoles = result.data.holes;
  }
  return result;
}

function holesForMap(typeName, mapName) {
  return openHoles.filter(h => h.type_name === typeName && h.map_name === mapName);
}

// ── Accordion builders ───────────────────────────────────────────────────────

function displayModuleName(mod) {
  return mod;
}

function buildModuleAccordion(types, container) {
  const groups = [];
  const indexByModule = new Map();
  for (const t of types) {
    const mod = t.module || 'source';
    let idx = indexByModule.get(mod);
    if (idx === undefined) {
      idx = groups.length;
      indexByModule.set(mod, idx);
      groups.push({ module: mod, types: [] });
    }
    groups[idx].types.push(t);
  }
  for (let i = 0; i < groups.length; i++) {
    const g = groups[i];
    const details = document.createElement('details');
    details.className = 'acc-module';
    details.open = i === groups.length - 1;
    const summary = document.createElement('summary');
    summary.innerHTML = esc(displayModuleName(g.module)) + ` <span class="acc-count">${g.types.length}</span>`;
    details.appendChild(summary);
    g.types.forEach(t => details.appendChild(buildTypeAccordion(t)));
    container.appendChild(details);
  }
}

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
      () => selectItem(t.name, { kind: 'diagram', name: d.name, input: d.input, output: d.output }))));
  if (t.maps.length) body.appendChild(buildSection('Maps', t.maps,
    m => buildClickableRow(hi(m.name),
      () => selectItem(t.name, { kind: 'map', name: m.name, domain: m.domain, generators: m.generators || [] }))));

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
    void selectItem(typeName, { kind: 'generator', name: g.name, dim: g.dim, input: g.input, output: g.output });
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
btnStop.addEventListener('click', () => {
  if (!sessionActive || !repl) return;
  void repl.stop_session();
  resetSession();
  appendReplEntry('stop', '<span class="meta">Session stopped</span>');
});

async function startSession() {
  if (!repl) return;
  const typeName = selType.value;
  const src = inpSource.value.trim();
  const tgt = inpTarget.value.trim() || undefined;
  const backward = chkBackward.checked;
  if (!typeName) { appendReplMsg('Select a type first.', 'repl-result err'); return; }
  if (!src)      { appendReplMsg('Enter an initial diagram.', 'repl-result err'); return; }
  await startSessionFromRepl(typeName, src, tgt, backward, formatStartCmd(typeName, src, tgt));
}

function resetSession() {
  const wasActive = sessionActive;
  sessionActive = false;
  fillSession = null;
  sessionStop.hidden = true;
  if (wasActive) sessionSetup.hidden = false;
  if (selectedEl) {
    selectedEl.classList.remove('acc-item--selected');
    selectedEl.classList.remove('acc-leaf--selected');
    selectedEl = null;
  }
  currentItem = null;
  currentLayout = null;
  resetZoom();
  sessionStrdiag = null;
  selectedRewrite = null;
  previewActive = false;
  proofView = false;
  proofLayout = null;
  proofBoundaryMap = null;
  infoboxText.innerHTML = '';
  rewriteList.innerHTML = '';
  infobox.hidden = true;
  rewriteList.hidden = true;
  visContainer.hidden = true;
  visContainer.classList.remove('target-reached-flash');
  visControls.hidden = true;
  boundaryControls.hidden = true;
  syncAnalysisLayout();
}

async function startSessionFromRepl(typeName, src, tgt, backward, rawCmd) {
  await enterSession(repl.start_session(typeName, src, tgt, backward), rawCmd);
}

async function startResumeFromRepl(typeName, proof, target, backward, rawCmd) {
  await enterSession(repl.resume_session(typeName, proof, target, backward), rawCmd);
}

async function enterSession(responsePromise, rawCmd) {
  const result = await parseReplResponse(responsePromise);
  if (result.status === 'error') {
    appendReplEntry(rawCmd, formatError(result));
    return;
  }
  sessionActive = true;
  sessionSetup.hidden = true;
  sessionStop.hidden = false;
  replInput.focus();
  if (selectedEl) {
    selectedEl.classList.remove('acc-item--selected');
    selectedEl.classList.remove('acc-leaf--selected');
    selectedEl = null;
  }
  currentItem = null;
  appendReplEntry(rawCmd, renderSegments(result.rendered));
  await showSessionDiagram(result.data);
}

// ── Hole-filling ──────────────────────────────────────────────────────────────

// Start a fill session for the hole with the given (0-based) index.  A fill is
// just a rewrite session in the backend, so it reuses the session machinery; a
// 0-cell hole has no rewrites and is filled by choosing one of the type's
// 0-cells.
async function startFill(index) {
  if (!repl) return;
  const hole = openHoles.find(h => h.index === index);
  const result = await parseReplResponse(repl.run_command(JSON.stringify({ command: 'fill', index, backward: chkBackward.checked })));
  if (result.status === 'error') {
    appendReplEntry(`fill ${index}`, formatError(result));
    return;
  }
  sessionActive = true;
  fillSession = {
    index,
    dim: result.data.fill ? result.data.fill.dim : (hole ? hole.dim : 1),
  };
  sessionSetup.hidden = true;
  sessionStop.hidden = false;
  if (selectedEl) {
    selectedEl.classList.remove('acc-item--selected');
    selectedEl.classList.remove('acc-leaf--selected');
    selectedEl = null;
  }
  currentItem = null;
  // Print the session state immediately, exactly as starting a normal session.
  const isZero = !!result.data.zero_cell;
  appendReplEntry(`fill ${index}`, renderSegments(result.rendered));
  if (isZero) {
    await showZeroCellFill(result.data);
  } else {
    await showSessionDiagram(result.data);
  }
}

// Render a boundaryless 0-cell fill so it feels like a normal session: an
// Undo/Redo/Done action row, the candidate 0-cells as a clickable list while
// unchosen, the chosen cell + target-reached once picked.  Never a string diagram.
async function showZeroCellFill(data) {
  selectedRewrite = null;
  previewActive = false;
  infobox.hidden = false;
  boundaryControls.hidden = true;
  const zc = data.zero_cell || { choices: [], chosen: null, target_reached: false, can_undo: false, can_redo: false };

  let buttons = '';
  buttons += `<button id="btn-undo-vis" class="btn-undo-vis btn-secondary" title="Undo"${zc.can_undo ? '' : ' disabled'}>&#x21A9;</button>`;
  buttons += `<button id="btn-redo-vis" class="btn-redo-vis btn-secondary" title="Redo"${zc.can_redo ? '' : ' disabled'}>&#x21AA;</button>`;
  buttons += `<button id="btn-done-fill" class="btn-done-fill btn-secondary" title="Finalise this fill"${zc.target_reached ? '' : ' disabled'}>Done</button>`;
  // A 0-cell has no current/target diagram; choosing one completes the proof.
  let html = `<div class="infobox-actions">${buttons}</div>`;
  if (zc.chosen) {
    html += `<span class="infobox-qual">Chosen 0-cell</span>`;
    html += `<div class="infobox-name">${hi(zc.chosen)}</div>`;
    html += `<div class="target-reached-banner">&#x2714; target reached</div>`;
  } else {
    html += `<span class="infobox-qual">Choose a 0-cell</span>`;
  }
  infoboxText.innerHTML = html;
  document.getElementById('btn-undo-vis')?.addEventListener('click', () => { void zeroCellCmd('undo'); });
  document.getElementById('btn-redo-vis')?.addEventListener('click', () => { void zeroCellCmd('redo'); });
  document.getElementById('btn-done-fill')?.addEventListener('click', () => { void finishFill(); });

  // A 0-cell fill never draws a string diagram.
  resetZoom();
  currentLayout = null;
  visContainer.hidden = true;
  visControls.hidden = true;

  buildZeroCellChoices(zc.choices);
  syncAnalysisLayout();
}

// Clickable list of candidate 0-cells, in the rewrite-list slot.  Empty once a
// cell is chosen (the backend offers candidates only while unchosen).
function buildZeroCellChoices(choices) {
  rewriteList.innerHTML = '';
  if (!choices.length) { rewriteList.hidden = true; return; }
  rewriteList.hidden = false;
  for (const c of choices) {
    const row = document.createElement('div');
    row.className = 'rewrite-row';
    row.innerHTML = `<span class="rw-content"><span class="rw-name">${esc(c.name)}</span></span>`;
    row.addEventListener('click', () => { void chooseZeroCell(c.index); });
    rewriteList.appendChild(row);
  }
}

async function chooseZeroCell(choice) {
  if (!repl) return;
  const result = await parseReplResponse(repl.run_command(JSON.stringify({ command: 'step', choice })));
  if (result.status === 'error') { appendReplEntry(`apply ${choice}`, formatError(result)); return; }
  // Log the choice to the transcript, exactly as clicking a rewrite does.
  appendReplEntry(`apply ${choice}`, renderSegments(result.rendered));
  await showZeroCellFill(result.data);
}

// Undo/redo within a 0-cell fill.
async function zeroCellCmd(command) {
  if (!repl) return;
  const result = await parseReplResponse(repl.run_command(JSON.stringify({ command })));
  if (result.status === 'error') { appendReplEntry(command, formatError(result)); return; }
  appendReplEntry(command, renderSegments(result.rendered));
  await showZeroCellFill(result.data);
}

// Route a typed session command while a 0-cell fill is active, mirroring the CLI:
// updates both the analysis pane and the transcript.
async function runZeroCellText(raw, cmd, arg) {
  if (cmd === 'stop') {
    void repl.stop_session();
    resetSession();
    appendReplEntry(raw, formatOk('Session stopped'));
    return;
  }
  let req;
  switch (cmd) {
    case 'apply': case 'a': {
      const n = parseInt(arg.split(/\s+/)[0], 10);
      if (Number.isNaN(n)) { appendReplEntry(raw, formatError('usage: apply <n>')); return; }
      req = { command: 'step', choice: n };
      break;
    }
    case 'undo': case 'u': case 'restart': req = { command: 'undo' }; break;
    case 'redo':                            req = { command: 'redo' }; break;
    case 'show': case 'status': case 'rules': case 'r': req = { command: 'show' }; break;
    default:
      appendReplEntry(raw, formatError("in a 0-cell fill use 'apply <n>', 'undo', 'redo', or 'done'"));
      return;
  }
  const result = await parseReplResponse(repl.run_command(JSON.stringify(req)));
  if (result.status === 'error') { appendReplEntry(raw, formatError(result)); return; }
  await showZeroCellFill(result.data);
  appendReplEntry(raw, renderSegments(result.rendered));
}

// Finalise the active fill: the backend appends the clause and returns the
// updated source, which we drop into the editor (flashing the new clause) and
// re-evaluate.
async function finishFill() {
  if (!repl) return;
  const before = getEditorText();
  const result = await parseReplResponse(repl.run_command('{"command":"done"}'));
  if (result.status === 'error') {
    appendReplEntry('done', formatError(result));
    return;
  }
  const message = result.data.message || 'Filled';
  const newSource = result.data.source || before;
  appendReplEntry('done', formatOk(message));
  fillSession = null;
  resetSession();
  replaceEditorSource(newSource, before);
  await evaluateSource(true);
  // The silent re-evaluation reflows the layout after the last scroll, so
  // re-pin the transcript to the bottom (where the `done` message is).
  replOutput.scrollTop = replOutput.scrollHeight;
}

// Replace the editor contents with `next` and briefly flash the inserted span
// (located by diffing against `prev`).
function replaceEditorSource(next, prev) {
  if (!view) return;
  // Common prefix / suffix → the inserted middle.
  let p = 0;
  const maxP = Math.min(prev.length, next.length);
  while (p < maxP && prev[p] === next[p]) p++;
  let s = 0;
  while (s < (maxP - p) && prev[prev.length - 1 - s] === next[next.length - 1 - s]) s++;
  const from = p;
  const to = next.length - s;

  view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: next } });
  if (to > from && to <= view.state.doc.length) {
    view.dispatch({
      effects: [setFlash.of({ from, to }), EditorView.scrollIntoView(from, { y: 'center' })],
    });
    setTimeout(() => { if (view) view.dispatch({ effects: clearFlash.of(null) }); }, 1600);
  }
}

// ── REPL input ────────────────────────────────────────────────────────────────

replInput.addEventListener('keydown', e => {
  if (e.key === 'Enter') {
    e.preventDefault();
    const cmd = replInput.value.trim();
    if (!cmd) return;
    replHistory.unshift(cmd);
    histIdx = -1;
    replInput.value = '';
    void handleCommand(cmd);
  } else if (e.key === 'ArrowUp') {
    e.preventDefault();
    if (histIdx + 1 < replHistory.length) { histIdx++; replInput.value = replHistory[histIdx]; }
  } else if (e.key === 'ArrowDown') {
    e.preventDefault();
    if (histIdx > 0) { histIdx--; replInput.value = replHistory[histIdx]; }
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

  // Hole-filling commands drive the same flows as the GUI buttons.
  if (cmd === 'holes') {
    const result = await fetchHoles();
    appendReplEntry(raw, result ? renderSegments(result.rendered) : '');
    return;
  }
  if (cmd === 'fill') {
    const n = parseInt(arg, 10);
    if (Number.isNaN(n)) { appendReplEntry(raw, formatError({ message: 'usage: fill <n>' })); return; }
    await startFill(n);
    return;
  }
  if (cmd === 'done') {
    await finishFill();
    return;
  }

  // In a 0-cell fill the text REPL acts like a rewrite session: `show`/`rules`
  // list the candidate 0-cells, `apply <n>` chooses, `undo`/`redo` reverse it.
  if (fillSession && fillSession.dim === 0) {
    await runZeroCellText(raw, cmd, arg);
    return;
  }

  const json = buildCommand(cmd, arg, raw);
  if (!json) return;

  const result = await parseReplResponse(repl.run_command(json));

  if (result.status === 'error') {
    appendReplEntry(raw, formatError(result));
  } else {
    // The backend renders the transcript (shared with the CLI) as `rendered`;
    // `parallel` is the lone transcript command without one — show its mode line.
    appendReplEntry(raw, result.rendered
      ? renderSegments(result.rendered)
      : dim('parallel mode: ' + (result.data && result.data.parallel ? 'on' : 'off')));
    // Only update the session diagram display for state-changing commands.
    const stateCommands = ['apply', 'a', 'auto', 'random', 'undo', 'u', 'redo', 'restart', 'show', 'status', 'store', 'parallel'];
    if (sessionActive && stateCommands.includes(cmd)) {
      await updateVisInfo(result.data);
    }
    // Append definition to editor and refresh accordion when store succeeds.
    if (cmd === 'store' && result.data && result.data.stored) {
      const s = result.data.stored;
      const code = `\n\n@${s.type_name}\nlet ${s.def_name} = ${s.expr}`;
      const doc = view.state.doc;
      const trimmed = doc.toString().trimEnd();
      const newText = trimmed + code + '\n';
      view.dispatch({
        changes: { from: 0, to: doc.length, insert: newText },
        selection: { anchor: newText.length },
        scrollIntoView: true,
      });
      await refreshAccordion();
    }
  }
}

function quoteArg(s) { return /\s/.test(s) ? `'${s}'` : s; }

function formatStartCmd(type, src, tgt) {
  return `start ${quoteArg(type)} ${quoteArg(src)}${tgt ? ' ' + quoteArg(tgt) : ''}`;
}

function splitQuotedArgs(s) {
  const args = [];
  let i = 0;
  while (i < s.length) {
    while (i < s.length && s[i] === ' ') i++;
    if (i >= s.length) break;
    let tok = '';
    const q = (s[i] === "'" || s[i] === '"') ? s[i++] : null;
    while (i < s.length) {
      if (q && s[i] === q) { i++; break; }
      if (!q && /\s/.test(s[i])) break;
      tok += s[i++];
    }
    if (tok) args.push(tok);
  }
  return args;
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
    case 'redo':
      if (arg) return JSON.stringify({ command: 'redo_to', step: parseInt(arg, 10) });
      return '{"command":"redo"}';
    case 'apply':
    case 'a': {
      const nums = arg.trim().split(/\s+/).map(s => parseInt(s, 10));
      if (nums.length === 0 || nums.some(isNaN)) {
        appendReplEntry(raw, formatError('usage: apply <n> [<n2> ...]'));
        return null;
      }
      if (nums.length === 1) return JSON.stringify({ command: 'step', choice: nums[0] });
      return JSON.stringify({ command: 'step_multi', choices: nums });
    }
    case 'auto': {
      const n = parseInt(arg, 10);
      if (isNaN(n) || n < 0) { appendReplEntry(raw, formatError('usage: auto <n>')); return null; }
      return JSON.stringify({ command: 'auto', max_steps: n });
    }
    case 'random':
      const n = parseInt(arg, 10);
      if (isNaN(n) || n < 0) { appendReplEntry(raw, formatError('usage: random <n>')); return null; }
      return JSON.stringify({ command: 'random', max_steps: n });
    case 'restart':  return JSON.stringify({ command: 'undo_to', step: 0 });
    case 'rules':
    case 'r':        return '{"command":"list_rules"}';
    case 'history':
    case 'h':        return '{"command":"history"}';
    case 'proof':
    case 'p':        return '{"command":"proof"}';
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
      if (!arg)          return JSON.stringify({ command: 'show' });
      appendReplEntry(raw, formatError('usage: parallel [on|off]'));
      return null;
    case 'backward':
      if (sessionActive) {
        appendReplEntry(raw, `<span class="meta">Backward mode ${chkBackward.checked ? 'on' : 'off'}</span>`);
      } else {
        if (arg === 'on')       chkBackward.checked = true;
        else if (arg === 'off') chkBackward.checked = false;
        else if (arg) { appendReplEntry(raw, formatError('usage: backward [on|off]')); return null; }
        appendReplEntry(raw, `<span class="meta">Backward mode ${chkBackward.checked ? 'on' : 'off'}</span>`);
      }
      return null;
    case 'stop': {
      if (!sessionActive) {
        appendReplEntry(raw, formatError('no active session'));
        return null;
      }
      void repl.stop_session();
      resetSession();
      appendReplEntry(raw, '<span class="meta">Session stopped</span>');
      return null;
    }
    case 'clear':
      replOutput.innerHTML = '';
      return null;
    case 'start': {
      if (sessionActive) {
        appendReplEntry(raw, formatError('session already active — use stop first'));
        return null;
      }
      const parts = splitQuotedArgs(arg);
      if (parts.length < 2 || parts.length > 3) {
        appendReplEntry(raw, formatError('usage: start <type> <initial> [<target>]'));
        return null;
      }
      const [typeName, src, tgt] = parts;
      void startSessionFromRepl(typeName, src, tgt, chkBackward.checked, raw);
      return null;
    }
    case 'resume': {
      if (sessionActive) {
        appendReplEntry(raw, formatError('session already active — use stop first'));
        return null;
      }
      const parts = splitQuotedArgs(arg);
      if (parts.length < 2 || parts.length > 3) {
        appendReplEntry(raw, formatError('usage: resume <type> <proof> [<target>]'));
        return null;
      }
      const [typeName, proof, target] = parts;
      void startResumeFromRepl(typeName, proof, target, chkBackward.checked, raw);
      return null;
    }
    default:
      appendReplEntry(raw, formatError(`unknown command '${cmd}' — type help for commands`));
      return null;
  }
}

// ── Rendering ─────────────────────────────────────────────────────────────────

// Style a shared RichText (lines of role-tagged segments, produced by the Rust
// `render_response` and carried in the response's `rendered` field) into
// transcript HTML.  This is the web half of the single shared renderer — the
// CLI styles the same RichText to ANSI; here each role maps to a `repl-*` span,
// the same classes the rest of the REPL uses.  Layout lives in Rust, not here.
const ROLE_CLASS = {
  label: 'repl-dim',
  value: 'repl-hi',
  src: 'repl-src',
  tgt: 'repl-tgt',
  section: 'repl-section-title',
  ok: 'repl-ok',
  redex: 'repl-match-hi',
};
function renderSegments(rendered) {
  if (!rendered || !Array.isArray(rendered.lines)) return '';
  return rendered.lines.map(line =>
    line.map(seg => {
      const cls = ROLE_CLASS[seg.role];
      return cls ? `<span class="${cls}">${esc(seg.text)}</span>` : esc(seg.text);
    }).join('')
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

// Span helpers for UI outside the REPL transcript (the accordion, the `parallel`
// mode line).  Transcript rendering goes through `renderSegments` instead, which
// maps RichText roles to the same `repl-*` classes.
function hi(s)  { return `<span class="repl-hi">${esc(s)}</span>`; }
function dim(s) { return `<span class="repl-dim">${esc(s)}</span>`; }

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

  // Omit the `> cmd` line for entries that aren't real commands (e.g. the
  // implicit re-evaluation after editing or `done`).
  if (cmdText) {
    const cmdEl = document.createElement('div');
    cmdEl.className = 'repl-cmd';
    cmdEl.textContent = cmdText;
    entry.appendChild(cmdEl);
  }

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

const HELP_TEXT = `Always available:
  types               list all types in the file
  type <name>         inspect a type
  homology <name>     compute cellular homology of a type
  start <t> <i>       start a rewrite session (target optional)
  resume <t> <p>      resume a session from a diagram (target optional)
  holes               list open holes of maps in this module
  fill <n>            start a hole-filling session for hole <n>
  backward [on|off]   show or toggle backward rewrite mode (default: off)
  stop                end the active session
  clear               clear the REPL output
  help / ?            show this message

Session commands (require active session):
  apply <n> [<n2>..]  apply rewrite(s) at given indices (alias: a)
  auto <n>            apply up to n rewrites automatically
  parallel [on|off]   show or toggle parallel rewrite mode (default: on)
  random <n>          apply up to n randomly selected rewrites
  undo [<n>]          undo last step, or back to step n (alias: u)
  redo [<n>]          redo last undone step, or forward to step n
  undo all / restart  reset to the source diagram
  show / status       show current state
  rules (r)           list all rewrite rules
  history (h)         show move history
  proof (p)           show the running proof diagram
  store <name>        store the current proof as a named diagram
  done                finalise the active fill, extending the map

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

async function fetchExampleByKey(key) {
  if (EXAMPLE_CONTENTS.has(key)) return EXAMPLE_CONTENTS.get(key);
  const relPath = EXAMPLES_INDEX && EXAMPLES_INDEX[key];
  if (!relPath) return null;
  const resp = await fetch(`${EXAMPLES_BASE}/${relPath}`, { cache: 'no-store' });
  if (!resp.ok) return null;
  const text = await resp.text();
  EXAMPLE_CONTENTS.set(key, text);
  return text;
}

function resolveIncludeKey(stem, parentKey) {
  if (!EXAMPLES_INDEX) return null;
  const parts = parentKey ? parentKey.split('/') : [];
  const parentDir = parts.slice(0, -1).join('/');
  const parentStem = parts[parts.length - 1] || '';
  const candidates = [];
  candidates.push(parentDir ? `${parentDir}/${stem}` : stem);
  if (parentStem) {
    candidates.push(parentDir ? `${parentDir}/${parentStem}/${stem}` : `${parentStem}/${stem}`);
  }
  if (parentDir) candidates.push(stem);
  for (const c of candidates) {
    if (EXAMPLES_INDEX[c]) return c;
  }
  return null;
}

async function collectIncludeModules(source, parentKey) {
  const map = {};
  const pending = [{ src: source, parent: parentKey || null }];
  const seen = new Set();
  while (pending.length) {
    const { src, parent } = pending.pop();
    for (const stem of collectDirectIncludes(src)) {
      const tab = editorTabs.tabs.find(t =>
        t.name === stem || t.name.split('/').pop() === stem
      );
      if (tab) {
        const filePath = `${stem}.ali`;
        if (seen.has(filePath)) continue;
        seen.add(filePath);
        const content = tab.cmState.doc.toString();
        map[filePath] = content;
        pending.push({ src: content, parent: tab.name });
        continue;
      }
      const key = resolveIncludeKey(stem, parent);
      if (!key) continue;
      const relPath = EXAMPLES_INDEX[key];
      if (seen.has(relPath)) continue;
      seen.add(relPath);
      const content = await fetchExampleByKey(key);
      if (content === null) continue;
      map[relPath] = content;
      pending.push({ src: content, parent: key });
    }
  }
  return map;
}

function collectDirectIncludes(source) {
  const re = /(^|[,\s])include\s+([A-Za-z_][A-Za-z0-9_]*)\b/g;
  const names = [];
  let m;
  while ((m = re.exec(source)) !== null) names.push(m[2]);
  return names;
}

selExamples.addEventListener('change', async () => {
  const name = selExamples.value;
  selExamples.value = '';
  selExamples.blur();
  if (!name) return;
  const existing = editorTabs.tabs.find(t => t.name === name);
  if (existing) {
    switchTab(existing.id);
    return;
  }
  const content = await fetchExampleByKey(name);
  if (content === null) {
    appendReplMsg(`Failed to fetch example: ${name}`, 'repl-result err');
    return;
  }
  const tab = activeTab();
  if (tab && !tab.dirty && tab.savedSnapshot === '') {
    tab.name = name;
    setEditorValue(content);
  } else {
    createTab(name, content, null);
  }
});

const hasFsAccess = typeof window.showOpenFilePicker === 'function';
const aliPickerTypes = [{
  description: 'alifib source',
  accept: { 'text/plain': ['.ali'] },
}];

function stemFromFilename(name) {
  if (!name) return null;
  return name.endsWith('.ali') ? name.slice(0, -4) : name;
}

btnLoad.addEventListener('click', async () => {
  if (hasFsAccess) {
    try {
      const [handle] = await window.showOpenFilePicker({ types: aliPickerTypes });
      const file = await handle.getFile();
      const text = await file.text();
      createTab(stemFromFilename(file.name), text, handle);
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
    createTab(stemFromFilename(f.name), String(reader.result || ''), null);
  };
  reader.onerror = () => { appendReplMsg('Failed to read file: ' + reader.error, 'repl-result err'); };
  reader.readAsText(f);
});

btnSave.addEventListener('click', async () => {
  const tab = activeTab();
  if (!tab) return;
  const content = getEditorText();
  if (hasFsAccess) {
    try {
      let handle = tab.fileHandle;
      if (!handle) {
        const suggestedName = (tab.name || 'untitled') + '.ali';
        handle = await window.showSaveFilePicker({
          suggestedName,
          types: aliPickerTypes,
        });
        tab.fileHandle = handle;
      }
      const writable = await handle.createWritable();
      await writable.write(content);
      await writable.close();
      tab.name = stemFromFilename(handle.name);
      markClean(tab, content);
    } catch (e) {
      if (e?.name === 'AbortError') return;
      appendReplMsg('Failed to save file: ' + (e?.message || e), 'repl-result err');
    }
  } else {
    const defaultName = (tab.name || 'untitled') + '.ali';
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
    tab.name = stemFromFilename(a.download);
    markClean(tab, content);
  }
});

function setEditorValue(text) {
  const tab = activeTab();
  view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: text } });
  if (tab) markClean(tab, text);
}

// ── Default example & editor init ────────────────────────────────────────────

createTab(null, '', null);
btnNewTab.addEventListener('click', () => createTab(null, '', null));

window.addEventListener('keydown', e => {
  const mod = e.ctrlKey || e.metaKey;
  if (!mod) return;
  switch (e.key.toLowerCase()) {
    case 's': e.preventDefault(); btnSave.click(); break;
    case 'o': e.preventDefault(); btnLoad.click(); break;
    case 't': e.preventDefault(); createTab(null, '', null); break;
    case 'w': e.preventDefault(); closeTab(editorTabs.activeTabId); break;
    case '1': case '2': case '3': case '4': case '5':
    case '6': case '7': case '8': case '9': {
      e.preventDefault();
      const tabs = editorTabs.tabs;
      const idx = e.key === '9' ? tabs.length - 1 : Number(e.key) - 1;
      if (tabs[idx]) switchTab(tabs[idx].id);
      break;
    }
  }
});

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
  buildModuleAccordion(types, fileOutput);
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

  if (proofView) {
    try {
      const proofResult = await parseReplResponse(repl.get_proof_strdiag());
      if (proofResult.status === 'ok') {
        proofLayout = layoutStrDiag(proofResult.data.strdiag, selOrientation.value);
        proofBoundaryMap = proofResult.data.output_boundary_map;
      }
    } catch (e) {
      console.error('proof strdiag fetch failed:', e);
    }
  }

  // Show infobox with session info.
  infobox.hidden = false;
  boundaryControls.hidden = true;

  let html = '';
  let buttons = '';
  if (data.target) buttons += `<button id="btn-target-vis" class="btn-target-vis btn-secondary" title="Show target diagram">Target</button>`;
  buttons += `<button id="btn-undo-vis" class="btn-undo-vis btn-secondary" title="Undo"${data.step_count === 0 ? ' disabled' : ''}>&#x21A9;</button>`;
  buttons += `<button id="btn-redo-vis" class="btn-redo-vis btn-secondary" title="Redo"${!data.can_redo ? ' disabled' : ''}>&#x21AA;</button>`;
  const viewLabel = proofView ? 'Step view' : 'Proof view';
  buttons += `<button id="btn-view-toggle" class="btn-view-toggle" title="${viewLabel}">${viewLabel}</button>`;
  if (fillSession) buttons += `<button id="btn-done-fill" class="btn-done-fill btn-secondary" title="Finalise this fill"${data.target_reached ? '' : ' disabled'}>Done</button>`;
  html += `<div class="infobox-actions">${buttons}</div>`;
  html += `<span class="infobox-qual">Current diagram</span>`;
  html += `<div class="infobox-name">${hi(data.current.label || '—')} <span class="acc-dim">dim ${data.current.dim}, step ${data.step_count}</span></div>`;
  if (data.target) {
    html += `<div class="infobox-boundary">target: ${esc(data.target.label)}</div>`;
    if (data.target_reached) {
      html += `<div class="target-reached-banner">&#x2714; target reached</div>`;
    }
  }
  infoboxText.innerHTML = html;
  const btnUndo = document.getElementById('btn-undo-vis');
  if (btnUndo) btnUndo.addEventListener('click', () => { void performUndo(); });
  const btnRedo = document.getElementById('btn-redo-vis');
  if (btnRedo) btnRedo.addEventListener('click', () => { void performRedo(); });
  const btnTarget = document.getElementById('btn-target-vis');
  if (btnTarget) {
    btnTarget.addEventListener('mousedown', (e) => {
      e.preventDefault();
      previewActive = true;
      void showTargetPreview();
    });
    btnTarget.addEventListener('mouseup', () => {
      previewActive = false;
      endRewritePreview();
    });
    btnTarget.addEventListener('mouseleave', () => {
      if (previewActive) { previewActive = false; endRewritePreview(); }
    });
  }
  const btnViewToggle = document.getElementById('btn-view-toggle');
  if (btnViewToggle) btnViewToggle.addEventListener('click', () => { void toggleProofView(); });
  const btnDoneFill = document.getElementById('btn-done-fill');
  if (btnDoneFill) btnDoneFill.addEventListener('click', () => { void finishFill(); });

  // Render the string diagram.
  resetZoom();
  currentLayout = proofView && proofLayout
    ? proofLayout
    : layoutStrDiag(sessionStrdiag, selOrientation.value);
  visContainer.hidden = false;
  visControls.hidden = false;
  if (data.target_reached) {
    visContainer.classList.remove('target-reached-flash');
    void visContainer.offsetWidth;
    visContainer.classList.add('target-reached-flash');
    visContainer.addEventListener('animationend', () => {
      visContainer.classList.remove('target-reached-flash');
    }, { once: true });
  }
  resizeAndRender();

  // Build rewrite list.
  bunchedIndices = [];
  bunchPositions = new Set();
  lastParallelMode = !!data.parallel;
  buildRewriteList(data.rewrites || []);
  syncAnalysisLayout();
}

async function toggleProofView() {
  if (!repl) return;
  proofView = !proofView;
  try {
    if (proofView) {
      await parseReplResponse(repl.set_proof_view(true));
    } else {
      await parseReplResponse(repl.set_proof_view(false));
      proofLayout = null;
      proofBoundaryMap = null;
    }
  } catch (e) {
    console.error('set_proof_view failed:', e);
  }
  const result = await parseReplResponse(repl.run_command('{"command":"show"}'));
  if (result.status === 'ok' && result.data) {
    await showSessionDiagram(result.data);
  }
}

function buildRewriteList(rewrites) {
  rewriteList.innerHTML = '';
  lastRewriteData = rewrites;
  if (!rewrites.length) {
    rewriteList.hidden = true;
    return;
  }
  rewriteList.hidden = false;

  const inBunchMode = bunchedIndices.length > 0;

  // Bunch card at top when bunching is active.
  if (inBunchMode) {
    const bunchCard = document.createElement('div');
    bunchCard.className = 'rewrite-row rewrite-row--bunch';

    const content = document.createElement('span');
    content.className = 'rw-content';
    const names = bunchedIndices.map(idx => {
      const r = rewrites.find(rw => rw.index === idx);
      return r ? esc(r.rule_name) : String(idx);
    });
    content.innerHTML = `<span class="rw-bunch-label">parallel</span>`
      + `<span class="rw-name">${names.join(', ')}</span>`;
    bunchCard.appendChild(content);

    const actions = document.createElement('span');
    actions.className = 'rewrite-actions rewrite-actions--bunch';
    const btnUnbunch = document.createElement('button');
    btnUnbunch.className = 'rw-btn-unbunch';
    btnUnbunch.textContent = 'Unbunch';
    btnUnbunch.addEventListener('click', (e) => {
      e.stopPropagation();
      bunchedIndices = [];
      bunchPositions = new Set();
      buildRewriteList(lastRewriteData);
    });
    actions.appendChild(btnUnbunch);
    bunchCard.appendChild(actions);

    bunchCard.addEventListener('click', () => { void applyBunch(); });

    // Hover: highlight union of all bunched positions.
    bunchCard.addEventListener('mouseenter', () => {
      if (previewActive) return;
      selectedRewrite = -1;
      if (currentLayout) {
        const positions = [...bunchPositions];
        if (proofView && proofBoundaryMap) {
          currentLayout._highlightPositions = null;
          currentLayout._highlightWires = positions.map(p => proofBoundaryMap[p]).filter(w => w != null);
          currentLayout._highlightWireInputHalf = chkBackward.checked;
        } else {
          currentLayout._highlightPositions = positions;
          currentLayout._highlightWires = null;
          currentLayout._highlightWireInputHalf = false;
        }
        resizeAndRender();
      }
    });
    bunchCard.addEventListener('mouseleave', () => {
      if (previewActive) return;
      selectedRewrite = null;
      if (currentLayout) {
        currentLayout._highlightPositions = null;
        currentLayout._highlightWires = null;
        currentLayout._highlightWireInputHalf = false;
        resizeAndRender();
      }
    });

    rewriteList.appendChild(bunchCard);
  }

  rewrites.forEach((r, i) => {
    // In bunch mode, hide overlapping and already-bunched rewrites.
    if (inBunchMode) {
      if (bunchedIndices.includes(r.index)) return;
      if (r.match_positions && r.match_positions.some(p => bunchPositions.has(p))) return;
    }

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

    // Bunch button (only when parallel mode is on and not already in bunch mode
    // where clicking adds automatically).
    if (lastParallelMode && !inBunchMode) {
      const btnBunch = document.createElement('button');
      btnBunch.className = 'rw-btn-bunch';
      btnBunch.textContent = 'Bunch';
      btnBunch.addEventListener('click', (e) => {
        e.stopPropagation();
        bunchedIndices = [r.index];
        bunchPositions = new Set(r.match_positions || []);
        buildRewriteList(lastRewriteData);
      });
      actions.appendChild(btnBunch);
    }

    if (!proofView) {
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
    }
    row.appendChild(actions);

    // Click: in bunch mode, add to bunch. Otherwise apply.
    if (inBunchMode) {
      row.addEventListener('click', () => {
        bunchedIndices.push(r.index);
        for (const p of (r.match_positions || [])) bunchPositions.add(p);
        buildRewriteList(lastRewriteData);
      });
    } else {
      row.addEventListener('click', () => { void applyRewrite(i); });
    }

    // Hover: highlight match positions.
    row.addEventListener('mouseenter', () => {
      if (previewActive) return;
      selectedRewrite = i;
      if (currentLayout) {
        if (proofView && proofBoundaryMap) {
          currentLayout._highlightPositions = null;
          currentLayout._highlightWires = (r.match_positions || []).map(p => proofBoundaryMap[p]).filter(w => w != null);
          currentLayout._highlightWireInputHalf = chkBackward.checked;
        } else {
          currentLayout._highlightPositions = r.match_positions;
          currentLayout._highlightWires = null;
          currentLayout._highlightWireInputHalf = false;
        }
        resizeAndRender();
      }
    });
    row.addEventListener('mouseleave', () => {
      if (previewActive) return;
      selectedRewrite = null;
      if (currentLayout) {
        currentLayout._highlightPositions = null;
        currentLayout._highlightWires = null;
        currentLayout._highlightWireInputHalf = false;
        resizeAndRender();
      }
    });

    rewriteList.appendChild(row);
  });
}

let savedLayoutBeforePreview = null;

async function showRewritePreview(choice) {
  if (!repl) return;
  savedLayoutBeforePreview = currentLayout;
  const result = await parseReplResponse(repl.get_rewrite_preview_strdiag(choice));
  if (result.status !== 'ok') return;
  currentLayout = layoutStrDiag(result.data, selOrientation.value);
  currentLayout._highlightPositions = null;
  resizeAndRender();
}

async function showTargetPreview() {
  if (!repl) return;
  savedLayoutBeforePreview = currentLayout;
  const result = await parseReplResponse(repl.get_target_strdiag());
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
      const positions = lastRewriteData[selectedRewrite].match_positions;
      if (proofView && proofBoundaryMap) {
        currentLayout._highlightPositions = null;
        currentLayout._highlightWires = (positions || []).map(p => proofBoundaryMap[p]).filter(w => w != null);
        currentLayout._highlightWireInputHalf = chkBackward.checked;
      } else {
        currentLayout._highlightPositions = positions;
        currentLayout._highlightWires = null;
        currentLayout._highlightWireInputHalf = false;
      }
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
  appendReplEntry('undo', renderSegments(result.rendered));
  selectedRewrite = null;
  previewActive = false;
  await showSessionDiagram(result.data);
}

async function performRedo() {
  if (!repl) return;
  const result = await parseReplResponse(repl.run_command('{"command":"redo"}'));
  if (result.status === 'error') {
    appendReplMsg('Redo error: ' + result.message, 'repl-result err');
    return;
  }
  appendReplEntry('redo', renderSegments(result.rendered));
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
  appendReplEntry(`apply ${choice}`, renderSegments(result.rendered));
  selectedRewrite = null;
  previewActive = false;
  // Update the session display.
  await showSessionDiagram(result.data);
}

async function applyBunch() {
  if (!bunchedIndices.length) return;
  const json = JSON.stringify({ command: 'step_multi', choices: bunchedIndices });
  const result = await parseReplResponse(repl.run_command(json));
  if (result.status === 'error') {
    appendReplMsg('Parallel apply error: ' + result.message, 'repl-result err');
    return;
  }
  appendReplEntry(`apply ${bunchedIndices.join(' ')}`, renderSegments(result.rendered));
  bunchedIndices = [];
  bunchPositions = new Set();
  selectedRewrite = null;
  previewActive = false;
  await showSessionDiagram(result.data);
}

// Store last rewrite data for re-highlighting after preview ends.
let lastRewriteData = null;
let lastParallelMode = false;

// Bunch state: indices into the rewrites list that have been bunched.
let bunchedIndices = [];
let bunchPositions = new Set();

// ── String diagram visualisation ─────────────────────────────────────────────

let currentItem = null;   // { typeName, item }
let currentItemDim = null; // dimension of the main diagram
let currentMapGen = null;  // selected generator name for map infobox
let sessionStrdiag = null; // strdiag data for current session diagram
let selectedRewrite = null; // index of selected rewrite
let previewActive = false;

async function selectItem(typeName, item) {
  currentItem = { typeName, item };
  currentMapGen = null;
  infobox.hidden = false;
  rewriteList.hidden = true; // hide session rewrite list when inspecting an item

  if (item.kind === 'map') {
    currentItemDim = 0;
    boundaryControls.hidden = true;
    await refreshInfobox();
    return;
  }

  // For generators and diagrams: fetch dimension from the main strdiag response
  if (repl) {
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
  if (currentItemDim >= 1) {
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
  const mapHasGen = item.kind === 'map' && currentMapGen;
  const isBoundary = bdVal !== 'main' && (item.kind !== 'map' || mapHasGen);
  const bdDim = isBoundary ? parseInt(bdVal, 10) : null;
  const bdSign = isBoundary ? document.querySelector('input[name="bd-sign"]:checked').value : null;

  // Build infobox text
  const qualPrefix = item.kind === 'generator' ? 'Generator of'
                   : item.kind === 'diagram'   ? 'Diagram at'
                   : 'Map at';
  let displayName;
  if (isBoundary && !mapHasGen) {
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
    const mapHoles = holesForMap(typeName, item.name);
    if (mapHoles.length) {
      html += `<div class="infobox-holes">`;
      html += `<div class="infobox-holes-title">Open holes</div>`;
      for (const h of mapHoles) {
        html += `<div class="infobox-hole">`
              + `<span class="infobox-hole-bd">${hi(h.boundary)}</span>`
              + `<button class="btn-fill-hole btn-secondary" data-hole-index="${h.index}">Fill</button>`
              + `</div>`;
      }
      html += `</div>`;
    }
    if (item.generators && item.generators.length) {
      html += `<div class="setup-row infobox-map-gen"><label for="sel-map-gen">Image of</label><select id="sel-map-gen">`;
      html += `<option value="">— select generator —</option>`;
      for (const g of item.generators) {
        const sel = currentMapGen === g.name ? ' selected' : '';
        html += `<option value="${esc(g.name)}"${sel}>${esc(g.name)} (dim ${g.dim})</option>`;
      }
      html += `</select></div>`;
    }

    currentLayout = null;
    resetZoom();
    if (currentMapGen && repl) {
      const result = await parseReplResponse(
        repl.get_map_image_strdiag(typeName, item.name, currentMapGen, bdDim ?? undefined, bdSign ?? undefined)
      );
      if (result.status === 'error') {
        html += `<div class="infobox-boundary" style="color:var(--err)">${esc(result.message)}</div>`;
      } else {
        const data = result.data;
        if (data.label) html += `<div class="infobox-label">${esc(data.label)}</div>`;
        if (data.input || data.output) html += `<div class="infobox-boundary">${esc(data.input)} → ${esc(data.output)}</div>`;
        currentLayout = layoutStrDiag(data.strdiag, selOrientation.value);
        const imageDim = data.dim ?? 0;
        if (bdDim == null && imageDim !== currentItemDim) {
          currentItemDim = imageDim;
          if (currentItemDim >= 1) {
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
        }
      }
    }

    infoboxText.innerHTML = html;

    infoboxText.querySelectorAll('.btn-fill-hole').forEach(btn => {
      btn.addEventListener('click', (e) => {
        e.stopPropagation();
        void startFill(parseInt(btn.dataset.holeIndex, 10));
      });
    });

    const selMapGen = document.getElementById('sel-map-gen');
    if (selMapGen) {
      selMapGen.addEventListener('change', async () => {
        currentMapGen = selMapGen.value || null;
        currentItemDim = 0;
        boundaryControls.hidden = true;
        await refreshInfobox();
      });
    }

    if (currentLayout) {
      visContainer.hidden = false;
      visControls.hidden = false;
      resizeAndRender();
    } else {
      visContainer.hidden = true;
      visControls.hidden = true;
    }
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
    resetZoom();
    syncAnalysisLayout();
    return;
  }

  const data = result.data;
  if (data.label) {
    html += `<div class="infobox-label">${esc(data.label)}</div>`;
  }
  if (data.input || data.output) {
    html += `<div class="infobox-boundary">${esc(data.input)} → ${esc(data.output)}</div>`;
  }
  infoboxText.innerHTML = html;

  resetZoom();
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

appearanceBtn.addEventListener('click', (e) => {
  e.stopPropagation();
  appearanceMenu.hidden = !appearanceMenu.hidden;
});
appearanceMenu.addEventListener('click', (e) => e.stopPropagation());
document.addEventListener('click', () => { appearanceMenu.hidden = true; });

chkNodeLabels.addEventListener('change', () => resizeAndRender());
chkWireLabels.addEventListener('change', () => resizeAndRender());

zoomSlider.addEventListener('input', () => {
  zoomLevel = parseFloat(zoomSlider.value);
  zoomLabel.textContent = zoomLevel.toFixed(1) + 'x';
  resizeAndRender();
});
btnZoomReset.addEventListener('click', () => resetZoom());

function resetZoom() {
  zoomLevel = 1;
  zoomSlider.value = '1';
  zoomLabel.textContent = '1.0x';
  visContainer.scrollLeft = 0;
  visContainer.scrollTop = 0;
  resizeAndRender();
}

// ── Layout ───────────────────────────────────────────────────────────────────

const NODE_R = 6;
const WIRE_R = 3;
const PAD = 0;

function layoutStrDiag(data, orientation = 'bt') {
  const n = data.vertices.length;
  if (n === 0) return { _raw: data, verts: [], pos: [], orientation, hAdj: [], wAdj: [], dAdj: [], hPred: [], wPred: [], hTopo: [], wTopo: [] };

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
           hTopo: hDist.topo, wTopo: wDist.topo,
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

  return { bw, fw, topo: fwdOrder };
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
  const w = rect.width * zoomLevel, h = rect.height * zoomLevel;
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

  const C = canvasColors();
  const wireColor = C.wire;
  const thinColor = C.thin;
  const BORDER_W = 6;
  const WIRE_W = 2;

  function wirePathParts(wi) {
    const wp = px[wi];
    const sources = L.hPred[wi].length > 0
      ? L.hPred[wi].map(pi => px[pi])
      : [entryPoint(wp, o, 'input', cw, ch)];
    const targets = L.hAdj[wi].length > 0
      ? L.hAdj[wi].map(si => px[si])
      : [entryPoint(wp, o, 'output', cw, ch)];
    return { wp, sources, targets };
  }

  function strokeWirePaths(wi) {
    const { wp, sources, targets } = wirePathParts(wi);
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

  function strokeWireOutputHalf(wi) {
    const { wp, targets } = wirePathParts(wi);
    for (const tgt of targets) {
      const q1 = isVert ? { x: wp.x, y: tgt.y } : { x: tgt.x, y: wp.y };
      ctx.beginPath();
      ctx.moveTo(wp.x, wp.y);
      ctx.quadraticCurveTo(q1.x, q1.y, tgt.x, tgt.y);
      ctx.stroke();
    }
  }

  function strokeWireInputHalf(wi) {
    const { wp, sources } = wirePathParts(wi);
    for (const src of sources) {
      const q0 = isVert ? { x: wp.x, y: src.y } : { x: src.x, y: wp.y };
      ctx.beginPath();
      ctx.moveTo(src.x, src.y);
      ctx.quadraticCurveTo(q0.x, q0.y, wp.x, wp.y);
      ctx.stroke();
    }
  }

  const hlWires = L._highlightWires ? new Set(L._highlightWires) : null;

  function drawWire(wi) {
    const wireThin = L.verts[wi].tag != null && thinTags.has(L.verts[wi].tag);
    ctx.strokeStyle = wireThin ? thinColor : wireColor;
    ctx.lineWidth = WIRE_W;
    ctx.lineCap = 'round';
    strokeWirePaths(wi);
  }

  function drawWireHighlight(wi) {
    ctx.save();
    ctx.shadowColor = C.hlShadow;
    ctx.shadowBlur = 10;
    ctx.strokeStyle = C.hlFill;
    ctx.lineWidth = WIRE_W + 2;
    ctx.lineCap = 'round';
    (L._highlightWireInputHalf ? strokeWireInputHalf : strokeWireOutputHalf)(wi);
    ctx.restore();
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
      if (hlWires) {
        for (const wi of levels[lv]) { if (hlWires.has(wi)) drawWireHighlight(wi); }
      }
    }
  } else {
    for (let wi = 0; wi < L.numWires; wi++) drawWire(wi);
    if (hlWires) {
      for (let wi = 0; wi < L.numWires; wi++) { if (hlWires.has(wi)) drawWireHighlight(wi); }
    }
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
      ctx.shadowColor = C.hlShadow;
      ctx.shadowBlur = 10;
      ctx.beginPath();
      ctx.arc(np.x, np.y, WIRE_R + 1.5, 0, Math.PI * 2);
      ctx.fillStyle = C.hlFill;
      ctx.fill();
      ctx.strokeStyle = C.hlShadow;
      ctx.lineWidth = 1;
      ctx.stroke();
      ctx.restore();
    } else if (nodeThin) {
      ctx.beginPath();
      ctx.arc(np.x, np.y, WIRE_R, 0, Math.PI * 2);
      ctx.fillStyle = nodeFullyThin ? thinColor : wireColor;
      ctx.fill();
    } else if (highlighted) {
      ctx.save();
      ctx.shadowColor = C.hlShadow;
      ctx.shadowBlur = 14;
      ctx.beginPath();
      ctx.arc(np.x, np.y, NODE_R, 0, Math.PI * 2);
      ctx.fillStyle = C.hlFill;
      ctx.fill();
      ctx.strokeStyle = C.hlShadow;
      ctx.lineWidth = 1.5;
      ctx.stroke();
      ctx.restore();
    } else {
      ctx.beginPath();
      ctx.arc(np.x, np.y, NODE_R, 0, Math.PI * 2);
      ctx.fillStyle = C.nodeFill;
      ctx.fill();
      ctx.strokeStyle = C.nodeStroke;
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
    if (isNode && !chkNodeLabels.checked) continue;
    if (!isNode && !chkWireLabels.checked) continue;
    const labelThin = L.verts[i].tag != null && thinTags.has(L.verts[i].tag);
    ctx.fillStyle = labelThin ? thinColor : (isNode ? C.labelNode : C.labelWire);
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
  } else if (zoomLevel > 1) {
    panState = {
      startX: e.clientX, startY: e.clientY,
      scrollLeft: visContainer.scrollLeft, scrollTop: visContainer.scrollTop,
    };
    e.preventDefault();
  }
});

visCanvas.addEventListener('mousemove', (e) => {
  if (panState) {
    visContainer.scrollLeft = panState.scrollLeft - (e.clientX - panState.startX);
    visContainer.scrollTop = panState.scrollTop - (e.clientY - panState.startY);
    return;
  }
  if (!dragState || !currentLayout) return;
  const rect = visCanvas.getBoundingClientRect();
  const mx = e.clientX - rect.left, my = e.clientY - rect.top;
  const cw = rect.width, ch = rect.height;

  const sx = (mx - PAD) / (cw - 2 * PAD);
  const sy = (my - PAD) / (ch - 2 * PAD);
  const mouseAbs = fromScreen(sx, sy, currentLayout.orientation);

  const L = currentLayout;
  const i = dragState.idx;
  const n = L.verts.length;
  const init = dragState.initPos;
  const inf = dragState.influence;

  // Compute the delta of the dragged vertex (clamped by its own constraints).
  let dragW = mouseAbs.w;
  let dragH = mouseAbs.h;
  for (const s of L.hAdj[i])  dragH = Math.min(dragH, init[s].h);
  for (const p of L.hPred[i]) dragH = Math.max(dragH, init[p].h);
  for (const s of (L.wAdj[i] || []))  dragW = Math.min(dragW, init[s].w);
  for (const p of (L.wPred[i] || [])) dragW = Math.max(dragW, init[p].w);

  const dw = dragW - init[i].w;
  const dh = dragH - init[i].h;

  // Compute unclamped positions (influence-weighted delta applied to initial).
  const newH = new Float64Array(n);
  const newW = new Float64Array(n);
  for (let v = 0; v < n; v++) {
    newH[v] = init[v].h + dh * inf[v];
    newW[v] = init[v].w + dw * inf[v];
  }

  // Two-pass constraint propagation in topological order ensures every
  // vertex respects ALL transitive constraints as hard barriers.
  // Backward pass (reverse topo): clamp against successors' final positions.
  for (let k = L.hTopo.length - 1; k >= 0; k--) {
    const v = L.hTopo[k];
    for (const s of L.hAdj[v]) newH[v] = Math.min(newH[v], newH[s]);
  }
  for (let k = L.wTopo.length - 1; k >= 0; k--) {
    const v = L.wTopo[k];
    for (const s of (L.wAdj[v] || [])) newW[v] = Math.min(newW[v], newW[s]);
  }
  // Forward pass (topo order): clamp against predecessors' final positions.
  for (const v of L.hTopo) {
    for (const p of L.hPred[v]) newH[v] = Math.max(newH[v], newH[p]);
  }
  for (const v of L.wTopo) {
    for (const p of (L.wPred[v] || [])) newW[v] = Math.max(newW[v], newW[p]);
  }

  for (let v = 0; v < n; v++) {
    if (inf[v] === 0) continue;
    L.pos[v] = { w: newW[v], h: newH[v] };
  }

  resizeAndRender();
});

visCanvas.addEventListener('mouseup', () => {
  dragState = null;
  panState = null;
});
visCanvas.addEventListener('mouseleave', () => {
  dragState = null;
  panState = null;
});

// ── Zoom ─────────────────────────────────────────────────────────────────────

visCanvas.addEventListener('wheel', (e) => {
  if (!e.ctrlKey && !e.metaKey) return;
  e.preventDefault();

  const MIN_ZOOM = 1, MAX_ZOOM = 5, ZOOM_STEP = 0.1;
  const oldZoom = zoomLevel;
  const newZoom = Math.max(MIN_ZOOM, Math.min(MAX_ZOOM,
    zoomLevel - Math.sign(e.deltaY) * ZOOM_STEP));
  if (newZoom === oldZoom) return;

  const rect = visContainer.getBoundingClientRect();
  const cursorX = e.clientX - rect.left + visContainer.scrollLeft;
  const cursorY = e.clientY - rect.top + visContainer.scrollTop;
  const canvasW = rect.width * oldZoom;
  const canvasH = rect.height * oldZoom;
  const fracX = cursorX / canvasW;
  const fracY = cursorY / canvasH;

  zoomLevel = newZoom;
  resizeAndRender();

  visContainer.scrollLeft = fracX * rect.width * newZoom - (e.clientX - rect.left);
  visContainer.scrollTop = fracY * rect.height * newZoom - (e.clientY - rect.top);

  zoomSlider.value = zoomLevel;
  zoomLabel.textContent = zoomLevel.toFixed(1) + 'x';
}, { passive: false });

// ── Init ──────────────────────────────────────────────────────────────────────

syncWorkspaceLayout();
syncAnalysisLayout();
boot();
