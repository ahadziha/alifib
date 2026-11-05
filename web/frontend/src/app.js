import { EditorView, keymap, lineNumbers, drawSelection, highlightActiveLine, Decoration } from '@codemirror/view';
import { EditorState, Compartment, StateField, StateEffect } from '@codemirror/state';
import { defaultKeymap, insertTab, indentLess, history as cmHistory, historyKeymap } from '@codemirror/commands';
import { indentUnit, bracketMatching } from '@codemirror/language';
import { closeBrackets, closeBracketsKeymap } from '@codemirror/autocomplete';
import { search, searchKeymap, findNext } from '@codemirror/search';
import { aliExtensions } from './ali-lang.js';
import { run, Failure, canvas, selectionView, viewKey } from './sync.js';
import {
  rewriteOf, zeroOf, controls, sessionKey, menuKey, moduleLabel, itemOf,
  generatorKeys, thinTags as moduleThinTags, scopeThinOverlay, formatStartCmd,
} from './model.js';

// ── State ─────────────────────────────────────────────────────────────────────

let repl = null;
let loop = null;
let shown = null;          // The shown cut (sync.js), null until the first answer.
let editorLink = null;
let thinOverlay = null;
// The page's local choices (sync.js `canvas`); the DOM controls render them.
let choice = { selection: null, preview: null, proof: false, targetPressed: false };
let highlight = null;      // null | {of:'rewrite', index} | {of:'bunch'}: canvas styling only.
let bunch = [];            // Menu indices bunched for one parallel apply.
const replHistory = [];
let histIdx = -1;
let currentLayout = null;
let selectedEl = null;
let dragState = null;
let panState = null;
let zoomLevel = 1;
let splitterDrag = null;
let generatorKeyOf = new Map(); // Tag → canonical generator key (model.js).
let thinTags = new Set();
let fullyThinTags = new Set();
const tagFaces = new Map();
let proofBoundaryMap = null;

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

function applyThin(module) {
  thinTags = moduleThinTags(module, thinOverlay.values);
  fullyThinTags = new Set([...thinTags].filter(tag => tagFaces.get(tag)?.every(face => thinTags.has(face))));
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
const btnTheme    = document.getElementById('btn-theme');

// ── Editor state & tabs ──────────────────────────────────────────────────────

const editorTabs = {
  tabs: [],
  activeTabId: null,
};

let tabIdCounter = 0;
let untitledCounter = 0;
let activeTabRename = null;

function activeTab() {
  return editorTabs.tabs.find(t => t.id === editorTabs.activeTabId) || null;
}

function tabNameAvailable(name, excludeTabId = null) {
  const conflict = editorTabs.tabs.find(t => t.id !== excludeTabId && t.name === name);
  if (!conflict) return true;
  appendReplMsg(`Tab "${conflict.name}" is already open`, 'repl-result err');
  return false;
}

function nextUntitledName() {
  let name;
  do { name = `Untitled${++untitledCounter}`; }
  while (editorTabs.tabs.some(tab => tab.name === name));
  return name;
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
        { key: 'Mod-Enter', run: () => { evaluateSource(); return true; } },
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
  if (!tabNameAvailable(name)) return null;
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
  settleTabRename(activeTabRename);
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

// Retire ownership and both callbacks before a renderer can remove the input.
// Only direct Enter/blur attempts keep a duplicate candidate open for correction.
function settleTabRename(rename, { apply = true, keepDuplicate = false } = {}) {
  if (!rename || activeTabRename !== rename) return false;
  const tab = editorTabs.tabs.find(tab => tab.id === rename.tabId);
  const name = stemFromFilename(rename.input.value.trim());
  if (apply && tab && name) {
    if (tabNameAvailable(name, tab.id)) tab.name = name;
    else if (keepDuplicate) {
      rename.input.classList.add('tab-rename--error');
      rename.input.title = `"${name}" is already open`;
      return false;
    }
  }
  rename.input.removeEventListener('keydown', rename.onKeydown);
  rename.input.removeEventListener('blur', rename.onBlur);
  activeTabRename = null;
  return true;
}

function startTabRename(tabId, nameSpan) {
  const tab = editorTabs.tabs.find(t => t.id === tabId);
  if (!tab || activeTabRename) return;
  const input = document.createElement('input');
  input.type = 'text';
  input.className = 'tab-rename';
  input.value = tab.name || '';
  input.placeholder = 'module name';
  const rename = { tabId, input, onKeydown, onBlur };
  activeTabRename = rename;
  input.addEventListener('keydown', onKeydown);
  input.addEventListener('blur', onBlur);
  nameSpan.replaceWith(input);
  input.focus();
  input.select();

  function onKeydown(event) {
    if (activeTabRename !== rename) return;
    if (event.key === 'Enter') {
      event.preventDefault();
      onBlur();
    } else if (event.key === 'Escape') {
      event.preventDefault();
      settleTabRename(rename, { apply: false });
      renderTabBar();
    }
  }

  function onBlur() {
    if (settleTabRename(rename, { keepDuplicate: true })) renderTabBar();
  }
}

// ── Boot ──────────────────────────────────────────────────────────────────────

// A decoded call answers JSON; anything else breaks the wire.
function parseAnswer(text) {
  try {
    return JSON.parse(text);
  } catch (error) {
    throw new Failure('protocol', `malformed answer: ${error.message}`);
  }
}

class WasmBackend {
  constructor(kernel) {
    this.kernel = kernel;
    this.label = 'WASM';
  }

  async call(request) {
    let text;
    try {
      text = this.kernel.call(JSON.stringify(request));
    } catch (error) {
      throw new Failure('backend', error.message, { cause: error });
    }
    return parseAnswer(text);
  }
}

class HttpBackend {
  constructor(baseUrl = '') {
    this.baseUrl = baseUrl;
    this.label = 'HTTP';
  }

  async call(request) {
    let response, text;
    try {
      response = await fetch(this.baseUrl + '/api', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(request),
      });
      text = await response.text();
    } catch (error) {
      throw new Failure('transport', error.message, { cause: error });
    }
    if (!response.ok) throw new Failure('http', `HTTP ${response.status}: ${text}`);
    return parseAnswer(text);
  }
}

const FAILURE_LABEL = {
  transport: 'backend unreachable',
  http: 'backend error',
  backend: 'backend failed',
  protocol: 'protocol error',
  paint: 'display failed',
};

function call(method, args) {
  return repl.call({ method, args });
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
  if (config.mode === 'http') return new HttpBackend(config.apiBase);
  const wasm = await import('../pkg/alifib_wasm.js');
  await wasm.default();
  return new WasmBackend(new wasm.WasmKernel());
}

function captureExpectation() {
  return loop && shown && { instance: shown.record.instance, revision: shown.record.revision };
}

function submit(line) {
  const expect = captureExpectation();
  if (!expect) return;
  if (line === 'clear') {
    replOutput.innerHTML = '';
    return;
  }
  loop.enqueue(page => call('eval', { line, expect, page }));
}

function tabText(tabId) {
  return editorTabs.tabs.find(tab => tab.id === tabId)?.cmState.doc.toString() ?? null;
}

// Apply a settled editor write in place, never switching tabs.
function writeTab({ tabId, from, to }) {
  const tab = editorTabs.tabs.find(tab => tab.id === tabId);
  if (tabId === editorTabs.activeTabId) replaceEditorSource(to, from);
  else {
    tab.cmState = tab.cmState.update({ changes: { from: 0, to: tab.cmState.doc.length, insert: to } }).state;
    markDirty(tab);
  }
}

function commit(result, p) {
  for (const reply of result.transcript) renderReply(reply);
  for (const note of result.notes) appendReplMsg(note);
  if (result.write) writeTab(result.write);
  for (const note of result.editorNotes) appendReplMsg(note, 'repl-result err');
  const previous = shown;
  shown = result.shown;
  editorLink = result.editorLink;
  thinOverlay = scopeThinOverlay(thinOverlay, shown.record.instance, shown.generation);
  const hasState = Object.hasOwn(p, 'state');
  const menuChanged = hasState && endGestures(previous);
  if (Object.hasOwn(p, 'module')) installModule(p.module);
  if (hasState || Object.hasOwn(p, 'module')) renderSession(previous, menuChanged);
  if (!previous) {
    btnEval.disabled = false;
    btnEval.textContent = 'Evaluate';
    replInput.disabled = false;
    submit('help');
  }
}

// A new state ends the target press, whose button it replaces. A new session
// (load or activity) ends all session UI; a new menu ends what indexes it.
// Returns whether the menu changed.
function endGestures(previous) {
  choice.targetPressed = false;
  if (sessionKey(previous) !== sessionKey(shown)) {
    deselectRow();
    choice = { selection: null, preview: null, proof: false, targetPressed: false };
  } else if (menuKey(previous.state) === menuKey(shown.state)) return false;
  choice.preview = null;
  highlight = null;
  bunch = [];
  return true;
}

// Catch up once each time the page becomes active (visible and focused):
// visibilitychange and focus usually both announce one return.
function watchActivity() {
  const isActive = () => document.visibilityState === 'visible' && document.hasFocus();
  let active = isActive();
  const update = () => {
    const now = isActive();
    if (now && !active) loop?.focus();
    active = now;
  };
  window.addEventListener('focus', update);
  window.addEventListener('blur', update);
  document.addEventListener('visibilitychange', update);
}

async function boot() {
  btnEval.textContent = 'Loading…';
  try {
    repl = await createBackend();
    appendReplMsg('\u{266B} No nit not, nit no not, nit nit folly bololey...', 'repl-dim');
    appendReplMsg(' ', 'repl-dim');
    appendReplMsg(`${repl.label} engine ready`, 'repl-dim');
    loop = run({
      poll: page => call('poll', { page }),
      getState: () => ({ shown, editorLink }),
      choices: () => choice,
      textOf: tabText,
      commit, paint,
      onError: error => appendReplMsg(`${FAILURE_LABEL[error.kind] ?? 'action failed'}: ${error.message}`, 'repl-result err'),
    });
    void loop.failed.catch(error => {
      appendReplMsg(`Page loop failed: ${error.message}`, 'repl-result err');
      btnEval.disabled = true;
      replInput.disabled = true;
      loop = null;
    });
    if (repl instanceof HttpBackend) watchActivity();
    void populateExamples();
  } catch (error) {
    btnEval.textContent = 'Error';
    appendReplMsg('Failed to load backend: ' + error, 'repl-result err');
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

btnEval.addEventListener('click', evaluateSource);

// Capture the tabs when the user acts and start fetching the examples at once,
// so example I/O never holds up the call loop.
function evaluateSource() {
  const expect = captureExpectation();
  if (!expect) return;
  const tab = activeTab();
  const source = tab.cmState.doc.toString();
  const tabs = Object.fromEntries(editorTabs.tabs.map(open => [`${open.name}.ali`, open.cmState.doc.toString()]));
  const examples = fetchExamples(tabs);
  loop.enqueue(async page => call('load_source', {
    source, modules: { ...await examples, ...tabs }, name: `${tab.name}.ali`, expect, page,
  }), { tabId: tab.id, text: source });
}

function installModule(module) {
  tagFaces.clear();
  for (const type of module.types) {
    for (const generator of type.generators) {
      if (generator.tag != null) tagFaces.set(generator.tag, generator.face_tags);
    }
  }
  generatorKeyOf = generatorKeys(module);
  applyThin(module);
  // A selection survives a module change while its row does.
  const selection = choice.selection;
  const item = selection && itemOf(module, selection);
  if (!item) choice.selection = null;
  else if (selection.generator != null && !item.generators.some(generator => generator.name === selection.generator)) {
    choice.selection = { ...selection, generator: null, boundary: null };
  }
  selectedEl = null;
  fileOutput.innerHTML = '';
  buildModuleAccordion(module, fileOutput);
  for (let parent = selectedEl?.parentElement; parent && parent !== fileOutput; parent = parent.parentElement) {
    if (parent.tagName === 'DETAILS') parent.open = true;
  }
  const previousType = selType.value;
  selType.innerHTML = '<option value="">— select type —</option>';
  const rootTypes = module.types.filter(type => type.module === module.root);
  for (const type of rootTypes) {
    const option = document.createElement('option');
    option.value = option.textContent = type.name;
    selType.appendChild(option);
  }
  if (rootTypes.some(type => type.name === previousType)) selType.value = previousType;
}

// ── Holes ──────────────────────────────────────────────────────────────────

const ofMap = ({ module, type_name, name }) => row =>
  row.module === module && row.type_name === type_name && row.map_name === name;

function holesOf(selection) {
  return shown.module.holes.filter(ofMap(selection));
}

// The constraint equations a map's pending assignments impose, flattened across
// its assignments — shown after the holes, in the same style.
function constraintsOf(selection) {
  return shown.module.constraints.filter(ofMap(selection)).flatMap(constraint => constraint.equations);
}

// ── Accordion builders ───────────────────────────────────────────────────────

function buildModuleAccordion(module, container) {
  const groups = [...Map.groupBy(module.types, type => type.module)];
  groups.forEach(([path, types], i) => {
    const details = document.createElement('details');
    details.className = 'acc-module';
    details.open = i === groups.length - 1;
    const summary = document.createElement('summary');
    summary.innerHTML = esc(moduleLabel(path, module.base)) + ` <span class="acc-count">${types.length}</span>`;
    details.appendChild(summary);
    for (const type of types) details.appendChild(buildTypeAccordion(type));
    container.appendChild(details);
  });
}

function buildTypeAccordion(type) {
  const details = document.createElement('details');
  details.className = 'acc-type';
  const summary = document.createElement('summary');
  summary.textContent = type.name;
  details.appendChild(summary);

  const body = document.createElement('div');
  body.className = 'acc-type-body';
  const selection = (kind, name) => ({ module: type.module, type_name: type.name, kind, name, generator: null, boundary: null });
  if (type.generators.length) body.appendChild(buildSection('Generators', type.generators,
    g => buildGeneratorRow(g, selection('generator', g.name))));
  if (type.diagrams.length) body.appendChild(buildSection('Diagrams', type.diagrams,
    d => buildClickableRow(hi(d.name), selection('diagram', d.name))));
  if (type.maps.length) body.appendChild(buildSection('Maps', type.maps,
    m => buildClickableRow(hi(m.name), selection('map', m.name))));

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
  for (const item of items) list.appendChild(buildItem(item));
  details.appendChild(list);
  return details;
}

function buildClickableRow(innerHTML, selection) {
  const div = document.createElement('div');
  div.className = 'acc-leaf';
  div.innerHTML = innerHTML;
  bindSelectableRow(div, selection);
  return div;
}

function buildGeneratorRow(g, selection) {
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
    const thin = !thinTags.has(g.tag);
    thinOverlay.values.set(generatorKeyOf.get(g.tag), thin);
    applyThin(shown.module);
    document.querySelectorAll(`.acc-leaf-gen[data-tag="${g.tag}"]`).forEach(row => {
      row.classList.toggle('acc-leaf--thin', thin);
      row.querySelector('.thin-toggle').classList.toggle('thin-toggle--active', thin);
    });
    resizeAndRender();
  });
  div.appendChild(toggle);
  div.dataset.tag = g.tag ?? '';
  div.dataset.gen = g.name;
  bindSelectableRow(div, selection);
  return div;
}

// A row selects its item; clicking the selected row returns to the session.
function bindSelectableRow(row, selection) {
  const current = choice.selection;
  if (current && ['module', 'type_name', 'kind', 'name'].every(field => current[field] === selection[field])) {
    selectedEl = row;
    row.classList.add('acc-leaf--selected');
  }
  row.addEventListener('click', () => {
    const reselect = selectedEl !== row;
    deselectRow();
    if (reselect) {
      selectedEl = row;
      row.classList.add('acc-leaf--selected');
    }
    select(reselect ? selection : null);
  });
}

function deselectRow() {
  selectedEl?.classList.remove('acc-leaf--selected');
  selectedEl = null;
}

// The selection owns the canvas and the infobox text.
function select(selection) {
  choice.selection = selection;
  renderInfobox();
  loop?.choicesChanged();
}

// ── Session setup ─────────────────────────────────────────────────────────────

btnStart.addEventListener('click', startSession);
inpSource.addEventListener('keydown', event => { if (event.key === 'Enter' && !event.ctrlKey && !event.metaKey) { event.preventDefault(); startSession(); } });
inpTarget.addEventListener('keydown', event => { if (event.key === 'Enter' && !event.ctrlKey && !event.metaKey) { event.preventDefault(); startSession(); } });
btnStop.addEventListener('click', () => submit('stop'));
chkBackward.addEventListener('change', () => {
  const desired = chkBackward.checked;
  chkBackward.checked = Boolean(shown.state.backward);
  submit(`backward ${desired ? 'on' : 'off'}`);
});

function startSession() {
  const typeName = selType.value;
  const source = inpSource.value.trim();
  if (!typeName) { appendReplMsg('Select a type first', 'repl-result err'); return; }
  if (!source) { appendReplMsg('Enter an initial diagram', 'repl-result err'); return; }
  submit(formatStartCmd(typeName, source, inpTarget.value));
}

// The panels a state decides, then the infobox and, when it changed, the menu.
function renderSession(previous, menuChanged) {
  const state = shown.state;
  const ui = controls(state, canvas(choice).owner);
  fileOutput.hidden = !ui.accordion;
  sessionSetup.hidden = !ui.setup;
  sessionStop.hidden = !ui.stop;
  chkBackward.checked = ui.backward;
  renderInfobox();
  if (menuChanged) renderMenu();
  const reached = Boolean(rewriteOf(state)?.target_reached);
  const sameSession = sessionKey(previous) === sessionKey(shown);
  if (reached && !(sameSession && rewriteOf(previous.state)?.target_reached)) {
    visContainer.classList.remove('target-reached-flash');
    void visContainer.offsetWidth;
    visContainer.classList.add('target-reached-flash');
    visContainer.addEventListener('animationend', () => visContainer.classList.remove('target-reached-flash'), { once: true });
  }
  syncAnalysisLayout();
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
  if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) return;
  if (e.key === 'Enter') {
    e.preventDefault();
    const cmd = replInput.value.trim();
    if (!cmd || !captureExpectation()) return;
    replHistory.unshift(cmd);
    histIdx = -1;
    replInput.value = '';
    submit(cmd);
  } else if (e.key === 'ArrowUp') {
    e.preventDefault();
    if (histIdx + 1 < replHistory.length) { histIdx++; replInput.value = replHistory[histIdx]; }
  } else if (e.key === 'ArrowDown') {
    e.preventDefault();
    if (histIdx > 0) { histIdx--; replInput.value = replHistory[histIdx]; }
    else { histIdx = -1; replInput.value = ''; }
  }
});

// ── Rendering ─────────────────────────────────────────────────────────────────

// Style a shared RichText (lines of role-tagged segments, produced by the Rust
// `richtext::render` and carried in the reply's `rendered` field) into
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
  return rendered.lines.map(line =>
    line.map(seg => {
      const cls = ROLE_CLASS[seg.role];
      return cls ? `<span class="${cls}">${esc(seg.text)}</span>` : esc(seg.text);
    }).join('')
  ).join('\n');
}

// ── Formatting helpers ────────────────────────────────────────────────────────

// Escape raw strings before embedding in HTML.
const ESCAPES = { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' };
function esc(s) {
  return String(s).replace(/[&<>"]/g, c => ESCAPES[c]);
}

// UI outside the transcript uses the same value style as RichText.
function hi(s)  { return `<span class="repl-hi">${esc(s)}</span>`; }

// A successful reply carries a message or rendered text, never both.
function renderReply(reply) {
  if (reply.status === 'error') appendReplEntry(reply.input, formatError(reply));
  else appendReplEntry(reply.input, reply.rendered
    ? renderSegments(reply.rendered)
    : { cls: 'repl-result meta', text: reply.message });
}

// Error replies carry a message and optional structured diagnostics.
// When `diagnostics` is present we render a structured block with line:col
// and a source snippet; otherwise we fall back to the reply's message.
function formatError({ message, diagnostics }) {
  if (!diagnostics?.length) return { cls: 'repl-result err', text: message };
  const html = `<div>${esc(message)}</div>` + diagnostics.map(renderDiagnostic).join('');
  return { cls: 'repl-result err', html };
}

const utf8 = new TextEncoder();
const fromUtf8 = new TextDecoder();

function renderDiagnostic(d) {
  const kindLabel = `${d.kind} error`;
  const path = d.path ? ` in ${d.path}` : '';
  const loc = `line ${d.start.line}:${d.start.col}`;
  const header =
    `<div class="err-header">` +
      `<span class="err-kind">${esc(kindLabel)}</span>` +
      `<span class="err-loc">${esc(path)} at ${esc(loc)}</span>` +
      `<span class="err-msg"> — ${esc(d.message)}</span>` +
    `</div>`;
  // Offsets are bytes into the excerpt, whose first line holds the start.
  const bytes = utf8.encode(d.excerpt.text);
  const span = (from, to) => Array.from(fromUtf8.decode(bytes.subarray(from - d.excerpt.byte, to - d.excerpt.byte)));
  const before = span(d.excerpt.byte, d.start.byte);
  const marked = span(d.start.byte, d.end.byte);
  const line = d.excerpt.text.split(/\r\n|[\r\n\v\f\u0085\u2028\u2029]/u)[0];
  // The caret's indent keeps the line's tabs, so it aligns however tabs render.
  const indent = before.map(character => character === '\t' ? '\t' : ' ').join('');
  const carets = '^'.repeat(Math.max(1, Math.min(marked.length, Array.from(line).length - before.length)));
  const snippet = `<pre class="err-snippet">${esc(line)}\n${indent}${carets}</pre>`;
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

  // Each committed reply carries its own input part; Evaluate has none.
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
      // plain-text message or error
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

// ── Examples, load/save ──────────────────────────────────────────────────────

const EXAMPLES_BASE = 'examples';

// Picker index, refreshed at boot and by every Evaluate: names are module
// paths; values are exact file names (one .ali per tab).
let EXAMPLES_INDEX = null;

// Why a fetch failed: the server explains in JSON {error}; other hosts send a page.
async function failure(response) {
  let error;
  try {
    ({ error } = JSON.parse(await response.text()));
  } catch {
    // Not the server's JSON.
  }
  return error ?? `HTTP ${response.status}`;
}

async function fetchExamplesIndex() {
  const response = await fetch(`${EXAMPLES_BASE}/index.json`, { cache: 'no-store' });
  if (!response.ok) throw new Error(await failure(response));
  const { modules } = await response.json();
  if (!modules || typeof modules !== 'object' || Array.isArray(modules)) throw new Error('invalid examples index');
  return modules;
}

function renderExamplePicker(index) {
  EXAMPLES_INDEX = index;
  selExamples.innerHTML = '<option value="">Examples…</option>';
  for (const name of Object.keys(index).sort()) {
    const option = document.createElement('option');
    option.value = option.textContent = name;
    selExamples.appendChild(option);
  }
}

async function populateExamples() {
  try {
    renderExamplePicker(await fetchExamplesIndex());
  } catch (error) {
    appendReplMsg(`Examples unavailable: ${error.message}`, 'repl-result err');
  }
}

async function fetchExampleFile(path) {
  const urlPath = path.split('/').map(encodeURIComponent).join('/');
  const response = await fetch(`${EXAMPLES_BASE}/${urlPath}`, { cache: 'no-store' });
  if (!response.ok) throw new Error(`${path}: ${await failure(response)}`);
  return response.text();
}

async function fetchExampleByKey(key) {
  const path = EXAMPLES_INDEX?.[key];
  if (!path) throw new Error(`unknown example: ${key}`);
  return fetchExampleFile(path);
}

// The indexed files the tabs do not shadow, fetched concurrently. Never
// rejects: what cannot be fetched is skipped with a note, and the kernel
// reports any include that therefore fails.
async function fetchExamples(tabs) {
  let index;
  try {
    index = await fetchExamplesIndex();
  } catch (error) {
    appendReplMsg(`examples not sent: ${error.message}`, 'repl-result err');
    return {};
  }
  renderExamplePicker(index);
  const paths = Object.values(index).filter(path => !Object.hasOwn(tabs, path));
  const files = await Promise.allSettled(paths.map(fetchExampleFile));
  const skipped = files.flatMap(file => file.status === 'rejected' ? [file.reason.message] : []);
  if (skipped.length) appendReplMsg(`examples skipped: ${skipped.join('; ')}`, 'repl-result err');
  return Object.fromEntries(files.flatMap((file, i) => file.status === 'fulfilled' ? [[paths[i], file.value]] : []));
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
  try {
    const content = await fetchExampleByKey(name);
    // A tab may have been opened or renamed while the fetch was in flight.
    const opened = editorTabs.tabs.find(tab => tab.name === name);
    if (opened) { switchTab(opened.id); return; }
    const tab = activeTab();
    if (tab && !tab.dirty && tab.savedSnapshot === '') {
      if (!tabNameAvailable(name, tab.id)) return;
      tab.name = name;
      tab.fileHandle = null;
      setEditorValue(content);
    } else createTab(name, content, null);
  } catch (error) {
    appendReplMsg(`Failed to fetch example: ${error.message}`, 'repl-result err');
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
      }
      const name = stemFromFilename(handle.name);
      if (!tabNameAvailable(name, tab.id)) return;
      const writable = await handle.createWritable();
      await writable.write(content);
      await writable.close();
      if (!tabNameAvailable(name, tab.id)) return;
      tab.fileHandle = handle;
      tab.name = name;
      markClean(tab, content);
    } catch (e) {
      if (e?.name === 'AbortError') return;
      appendReplMsg('Failed to save file: ' + (e?.message || e), 'repl-result err');
    }
  } else {
    const defaultName = (tab.name || 'untitled') + '.ali';
    const name = window.prompt('Save as:', defaultName);
    if (!name) return;
    const filename = name.endsWith('.ali') ? name : name + '.ali';
    const tabName = stemFromFilename(filename);
    if (!tabNameAvailable(tabName, tab.id)) return;
    const blob = new Blob([content], { type: 'text/plain;charset=utf-8' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = filename;
    document.body.appendChild(a);
    a.click();
    document.body.removeChild(a);
    setTimeout(() => URL.revokeObjectURL(url), 0);
    tab.name = tabName;
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
  if (!mod || e.defaultPrevented) return;
  switch (e.key.toLowerCase()) {
    case 'enter': e.preventDefault(); evaluateSource(); break;
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

// ── Session display (DOM only; the canvas belongs to paint) ───────────────────

// The infobox header: the owner's text and the actions the state admits. It is
// rebuilt only when the state, selection or owner change, never by paint, so a
// pressed button keeps its element; paint fills only its drawn-info slot.
function renderInfobox() {
  const state = shown?.state;
  const active = state?.mode === 'rewrite' || state?.mode === 'fill';
  infobox.hidden = !choice.selection && !active;
  if (!infobox.hidden) {
    infoboxText.innerHTML = actionsMarkup(controls(state, canvas(choice).owner))
      + (choice.selection ? itemMarkup(choice.selection) : sessionMarkup(state))
      + '<div class="infobox-drawn"></div>';
    bindInfobox();
  }
  syncAnalysisLayout();
}

function actionsMarkup(ui) {
  if (!ui.stop) return '';
  let buttons = '';
  if (ui.target) buttons += '<button id="btn-target-vis" class="btn-target-vis btn-secondary" title="Show target diagram">Target</button>';
  buttons += `<button id="btn-undo-vis" class="btn-undo-vis btn-secondary" title="Undo"${ui.canUndo ? '' : ' disabled'}>&#x21A9;</button>`;
  buttons += `<button id="btn-redo-vis" class="btn-redo-vis btn-secondary" title="Redo"${ui.canRedo ? '' : ' disabled'}>&#x21AA;</button>`;
  if (ui.proof) {
    const label = choice.proof ? 'Step view' : 'Proof view';
    buttons += `<button id="btn-view-toggle" class="btn-view-toggle" title="${label}">${label}</button>`;
  }
  if (ui.done) buttons += `<button id="btn-done-fill" class="btn-done-fill btn-secondary"${ui.canDone ? '' : ' disabled'}>Done</button>`;
  return `<div class="infobox-actions">${buttons}</div>`;
}

const TARGET_REACHED = '<div class="target-reached-banner">&#x2714; target reached</div>';

function sessionMarkup(state) {
  const zero = zeroOf(state);
  if (zero) {
    return `<span class="infobox-qual">Fill ${hi(state.hole.source_name)}</span>` + (zero.chosen != null
      ? `<div class="infobox-name">${hi(zero.chosen)}</div>${TARGET_REACHED}`
      : '<div class="infobox-name">Choose a 0-cell</div>');
  }
  const rewrite = rewriteOf(state);
  let html = '<span class="infobox-qual">Current diagram</span>'
    + `<div class="infobox-name">${hi(rewrite.current.label || '—')} <span class="acc-dim">dim ${rewrite.current.dim}, step ${rewrite.step_count}</span></div>`;
  if (rewrite.target) {
    html += `<div class="infobox-boundary">target: ${esc(rewrite.target.label)}</div>`;
    if (rewrite.target_reached) html += TARGET_REACHED;
  }
  return html;
}

const QUALIFIER = { generator: 'Generator of', diagram: 'Diagram at', map: 'Map at' };

function itemMarkup(selection) {
  const { type_name, kind, name, boundary } = selection;
  const item = itemOf(shown.module, selection);
  const title = boundary && kind !== 'map'
    ? `${boundary.side === 'output' ? 'Output' : 'Input'} ${boundary.dim}-boundary of ${name}` : name;
  let html = `<span class="infobox-qual">${QUALIFIER[kind]} ${hi(type_name)}</span><div class="infobox-name">${hi(title)}`;
  if (!boundary && kind === 'generator') html += ` <span class="acc-dim">dim ${item.dim}</span>`;
  html += '</div>';
  return kind === 'map' ? html + mapMarkup(selection, item) : html;
}

function mapMarkup(selection, map) {
  let html = `<div class="infobox-boundary">:: ${esc(map.domain)}</div>`;
  const holes = holesOf(selection);
  if (holes.length) {
    html += '<div class="infobox-holes"><div class="infobox-holes-title">Open holes</div>';
    for (const hole of holes) html += `<div class="infobox-hole"><span class="infobox-hole-bd">${hi(hole.boundary)}</span><button class="btn-fill-hole btn-secondary" data-hole-index="${hole.index}">Fill</button></div>`;
    html += '</div>';
  }
  const equations = constraintsOf(selection);
  if (equations.length) {
    html += '<div class="infobox-holes"><div class="infobox-holes-title">Constraints</div>';
    for (const equation of equations) html += `<div class="infobox-hole"><span class="infobox-hole-bd">${hi(equation)}</span></div>`;
    html += '</div>';
  }
  if (map.generators.length) {
    html += '<div class="setup-row infobox-map-gen"><label for="sel-map-gen">Image of</label><select id="sel-map-gen"><option value="">— select generator —</option>';
    for (const generator of map.generators) html += `<option value="${esc(generator.name)}"${selection.generator === generator.name ? ' selected' : ''}>${esc(generator.name)} (dim ${generator.dim})</option>`;
    html += '</select></div>';
  }
  return html;
}

function bindInfobox() {
  const element = id => document.getElementById(id);
  element('btn-undo-vis')?.addEventListener('click', () => submit('undo'));
  element('btn-redo-vis')?.addEventListener('click', () => submit('redo'));
  element('btn-done-fill')?.addEventListener('click', () => submit('done'));
  const target = element('btn-target-vis');
  target?.addEventListener('mousedown', event => {
    event.preventDefault();
    choice.targetPressed = true;
    loop?.choicesChanged();
  });
  const releaseTarget = () => {
    if (!choice.targetPressed) return;
    choice.targetPressed = false;
    loop?.choicesChanged();
  };
  target?.addEventListener('mouseup', releaseTarget);
  target?.addEventListener('mouseleave', releaseTarget);
  element('btn-view-toggle')?.addEventListener('click', () => {
    choice.proof = !choice.proof;
    renderInfobox();
    renderMenu();
    loop?.choicesChanged();
  });
  for (const button of infoboxText.querySelectorAll('.btn-fill-hole')) {
    button.addEventListener('click', () => {
      deselectRow();
      select(null);
      submit(`fill ${button.dataset.holeIndex}`);
    });
  }
  element('sel-map-gen')?.addEventListener('change', event => {
    select({ ...choice.selection, generator: event.target.value || null, boundary: null });
  });
}

// What the drawn view adds to the infobox: an item's label and boundary, or a
// view's error (a session's only while it has an engine to draw from).
function renderDrawnInfo(result) {
  const slot = infoboxText.querySelector('.infobox-drawn');
  if (!slot) return;
  const engine = shown && rewriteOf(shown.state);
  if (result.kind === 'error' && (result.owner === 'selection' || engine)) {
    slot.innerHTML = `<div class="infobox-boundary infobox-error">${esc(result.message)}</div>`;
  } else if (result.kind === 'ok' && result.owner === 'selection') {
    const { label, input, output } = result.data;
    slot.innerHTML = (label ? `<div class="infobox-label">${esc(label)}</div>` : '')
      + (input || output ? `<div class="infobox-boundary">${esc(input)} → ${esc(output)}</div>` : '');
  } else slot.innerHTML = '';
}

// The menu: rewrite rows, or a 0-cell fill's choices. Rebuilt only when it,
// the bunch or the proof view changed, so a hovered row keeps its element.
function renderMenu() {
  const state = shown.state;
  const zero = zeroOf(state);
  const rewrite = rewriteOf(state);
  rewriteList.innerHTML = '';
  rewriteList.hidden = !(zero?.choices ?? rewrite?.rewrites ?? []).length;
  if (zero) {
    for (const option of zero.choices) {
      const row = document.createElement('div');
      row.className = 'rewrite-row';
      row.innerHTML = `<span class="rw-content"><span class="rw-name">${esc(option.name)}</span></span>`;
      row.addEventListener('click', () => submit(`apply ${option.index}`));
      rewriteList.appendChild(row);
    }
  } else if (rewrite) {
    // A bunch hides its members and every rewrite overlapping them.
    const positions = bunchPositions(rewrite.rewrites);
    if (bunch.length) rewriteList.appendChild(bunchCard(rewrite.rewrites));
    for (const r of rewrite.rewrites) {
      if (bunch.length && (bunch.includes(r.index) || r.match_positions.some(p => positions.has(p)))) continue;
      rewriteList.appendChild(rewriteRow(r, rewrite.parallel));
    }
  }
  syncAnalysisLayout();
}

function bunchPositions(rewrites) {
  return new Set(bunch.flatMap(index => rewrites.find(r => r.index === index)?.match_positions ?? []));
}

function setBunch(indices) {
  bunch = indices;
  highlight = null;
  renderMenu();
  loop?.repaint();
}

// Hovering highlights a redex, or a bunch's union, on the drawn diagram: a
// styling choice that asks nothing. A held preview freezes the highlight.
function hover(next) {
  if (choice.preview != null) return;
  highlight = next;
  loop?.repaint();
}

// Pressing Preview draws that rewrite's result until released.
function holdPreview(index) {
  choice.preview = index;
  loop?.choicesChanged();
}

function releasePreview() {
  if (choice.preview == null) return;
  choice.preview = null;
  loop?.choicesChanged();
}

function bunchCard(rewrites) {
  const card = document.createElement('div');
  card.className = 'rewrite-row rewrite-row--bunch';
  const names = bunch.map(index => esc(rewrites.find(r => r.index === index)?.rule_name ?? index));
  card.innerHTML = `<span class="rw-content"><span class="rw-bunch-label">parallel</span><span class="rw-name">${names.join(', ')}</span></span>`
    + '<span class="rewrite-actions rewrite-actions--bunch"><button class="rw-btn-unbunch">Unbunch</button></span>';
  card.querySelector('.rw-btn-unbunch').addEventListener('click', event => {
    event.stopPropagation();
    setBunch([]);
  });
  card.addEventListener('click', () => submit(`apply ${bunch.join(' ')}`));
  card.addEventListener('mouseenter', () => hover({ of: 'bunch' }));
  card.addEventListener('mouseleave', () => hover(null));
  return card;
}

function rewriteRow(r, parallel) {
  const bunching = bunch.length > 0;
  const row = document.createElement('div');
  row.className = 'rewrite-row';
  const family = r.family?.length ? ` <span class="rw-parallel-badge">parallel ×${r.family.length}</span>` : '';
  row.innerHTML = `<span class="rw-content"><span class="rw-index">${r.index}</span><span class="rw-name">${esc(r.rule_name)}</span>${family}</span>`;

  // Action buttons are always present, shown on hover via CSS.
  const actions = document.createElement('span');
  actions.className = 'rewrite-actions';
  if (parallel && !bunching) {
    const btnBunch = document.createElement('button');
    btnBunch.className = 'rw-btn-bunch';
    btnBunch.textContent = 'Bunch';
    btnBunch.addEventListener('click', event => {
      event.stopPropagation();
      setBunch([r.index]);
    });
    actions.appendChild(btnBunch);
  }
  if (!choice.proof) {
    const btnPreview = document.createElement('button');
    btnPreview.className = 'rw-btn-preview';
    btnPreview.textContent = 'Preview';
    btnPreview.addEventListener('mousedown', event => {
      event.stopPropagation();
      event.preventDefault();
      holdPreview(r.index);
    });
    btnPreview.addEventListener('mouseup', releasePreview);
    btnPreview.addEventListener('mouseleave', releasePreview);
    btnPreview.addEventListener('click', event => event.stopPropagation());
    actions.appendChild(btnPreview);
  }
  row.appendChild(actions);

  // Click: in bunch mode, add to the bunch; otherwise apply.
  row.addEventListener('click', bunching ? () => setBunch([...bunch, r.index]) : () => submit(`apply ${r.index}`));
  row.addEventListener('mouseenter', () => hover({ of: 'rewrite', index: r.index }));
  row.addEventListener('mouseleave', () => hover(null));
  return row;
}

// ── String diagram visualisation ─────────────────────────────────────────────

// The dimension the boundary options offer: the main view's, once held, else
// the module row's (for a map, its chosen generator's).
function selectionDim(selection) {
  const main = selectionView({ ...selection, boundary: null });
  const held = main && shown.views.get(viewKey(main));
  if (held?.status === 'ok') return held.data.dim;
  const item = itemOf(shown.module, selection);
  return selection.kind !== 'map' ? item.dim
    : item.generators.find(generator => generator.name === selection.generator)?.dim ?? 0;
}

const signRadios = document.querySelectorAll('input[name="bd-sign"]');
let boundaryDim = null; // The dimension the boundary options were built for.

// Render the selection's boundary choice; the controls never hold it.
function renderBoundaryControls() {
  const selection = choice.selection;
  const dim = selection ? selectionDim(selection) : 0;
  boundaryControls.hidden = dim < 1;
  if (dim !== boundaryDim) {
    boundaryDim = dim;
    selBoundary.innerHTML = '<option value="main">Main</option>';
    for (let boundary = dim - 1; boundary >= 0; boundary--) {
      const option = document.createElement('option');
      option.value = String(boundary);
      option.textContent = `${boundary}-boundary`;
      selBoundary.appendChild(option);
    }
  }
  const boundary = selection?.boundary;
  selBoundary.value = boundary ? String(boundary.dim) : 'main';
  for (const radio of signRadios) {
    radio.disabled = !boundary;
    if (boundary) radio.checked = radio.value === boundary.side;
  }
  signControls.style.opacity = boundary ? '1' : '0.35';
}

function setBoundary(boundary) {
  select({ ...choice.selection, boundary });
}

selBoundary.addEventListener('change', () => {
  const side = [...signRadios].find(radio => radio.checked).value;
  setBoundary(selBoundary.value === 'main' ? null : { dim: Number(selBoundary.value), side });
});
for (const radio of signRadios) {
  radio.addEventListener('change', () => setBoundary({ ...choice.selection.boundary, side: radio.value }));
}

// Layouts by view key. An entry lives while its payload is unchanged, so drags,
// zoom and scroll survive view switches and an equal payload re-sent in a new
// cut; entries of views no longer held are dropped.
const layouts = new Map();
let painted = null; // The last painted {result, entry}.

function layoutEntry(view, data) {
  const key = viewKey(view);
  const orientation = selOrientation.value;
  const old = layouts.get(key);
  const payload = old?.data === data ? old.payload : JSON.stringify(data.strdiag);
  if (old?.payload === payload) {
    old.data = data;
    if (old.orientation !== orientation) Object.assign(old, { orientation, layout: layoutStrDiag(data.strdiag, orientation) });
    return old;
  }
  const entry = {
    data, payload, orientation, layout: layoutStrDiag(data.strdiag, orientation),
    zoom: 1, scrollLeft: 0, scrollTop: 0,
  };
  layouts.set(key, entry);
  return entry;
}

// The only owner of canvas view and layout replacement.
function paint(result) {
  const entry = result.kind === 'ok' ? layoutEntry(result.view, result.data) : null;
  for (const key of layouts.keys()) if (!shown?.views.has(key)) layouts.delete(key);
  const switched = entry !== painted?.entry;
  if (switched) {
    if (painted?.entry) {
      Object.assign(painted.entry, { zoom: zoomLevel, scrollLeft: visContainer.scrollLeft, scrollTop: visContainer.scrollTop });
    }
    dragState = null;
    panState = null;
  }
  painted = { result, entry };
  currentLayout = entry?.layout ?? null;
  proofBoundaryMap = result.kind === 'ok' && result.view.of === 'proof' ? result.data.output_boundary_map : null;
  renderBoundaryControls();
  renderDrawnInfo(result);
  const visible = Boolean(currentLayout);
  visContainer.hidden = !visible;
  visControls.hidden = !visible;
  if (!visible) canvasCtx.clearRect(0, 0, visCanvas.width, visCanvas.height);
  updateRewriteHighlight();
  syncAnalysisLayout();
  if (!switched) resizeAndRender();
  else {
    setZoom(entry?.zoom ?? 1);
    visContainer.scrollLeft = entry?.scrollLeft ?? 0;
    visContainer.scrollTop = entry?.scrollTop ?? 0;
  }
}

function updateRewriteHighlight() {
  if (!currentLayout) return;
  const rewrites = rewriteOf(shown.state)?.rewrites ?? [];
  const positions = highlight?.of === 'bunch' ? [...bunchPositions(rewrites)]
    : highlight ? rewrites.find(rewrite => rewrite.index === highlight.index)?.match_positions ?? [] : [];
  const drawnView = painted.result.view.of;
  currentLayout._highlightPositions = drawnView === 'current' ? positions : null;
  currentLayout._highlightWires = drawnView === 'proof'
    ? positions.map(position => proofBoundaryMap?.[position]).filter(wire => wire != null) : null;
  currentLayout._highlightWireInputHalf = Boolean(shown.state.backward);
}

selOrientation.addEventListener('change', () => loop?.repaint());

appearanceBtn.addEventListener('click', (e) => {
  e.stopPropagation();
  appearanceMenu.hidden = !appearanceMenu.hidden;
});
appearanceMenu.addEventListener('click', (e) => e.stopPropagation());
document.addEventListener('click', () => { appearanceMenu.hidden = true; });

chkNodeLabels.addEventListener('change', () => resizeAndRender());
chkWireLabels.addEventListener('change', () => resizeAndRender());

zoomSlider.addEventListener('input', () => setZoom(parseFloat(zoomSlider.value)));
btnZoomReset.addEventListener('click', () => {
  setZoom(1);
  visContainer.scrollLeft = 0;
  visContainer.scrollTop = 0;
});

function setZoom(level) {
  zoomLevel = level;
  zoomSlider.value = String(level);
  zoomLabel.textContent = level.toFixed(1) + 'x';
  resizeAndRender();
}

// ── Layout ───────────────────────────────────────────────────────────────────

const NODE_R = 6;
const WIRE_R = 3;
const PAD = 0;

function layoutStrDiag(data, orientation = 'bt') {
  const n = data.vertices.length;
  if (n === 0) return { verts: [], pos: [], orientation, hAdj: [], wAdj: [], dAdj: [], hPred: [], wPred: [], hTopo: [], wTopo: [] };

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

  return { verts: data.vertices, pos, orientation, hAdj, hPred, wAdj, wPred, dAdj,
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

  setZoom(newZoom);
  visContainer.scrollLeft = fracX * rect.width * newZoom - (e.clientX - rect.left);
  visContainer.scrollTop = fracY * rect.height * newZoom - (e.clientY - rect.top);
}, { passive: false });

// ── Init ──────────────────────────────────────────────────────────────────────

syncWorkspaceLayout();
syncAnalysisLayout();
boot();
