import test from 'node:test';
import assert from 'node:assert/strict';
import { reduce, settle, canvas, views, drawn, holds, run, Failure } from '../src/sync.js';

const INSTANCE = '1780000000000000001';
const RESTART = '1780000000000000002';
const current = { of: 'current' };
const target = { of: 'target' };
const proof = { of: 'proof' };
const preview = choice => ({ of: 'preview', choice });
const ok = data => ({ status: 'ok', data });
const error = { status: 'error', message: 'no session active' };
const idle = { mode: 'idle', backward: false, module: 'Main.ali' };
const moduleInfo = (source = 'source') => ({
  base: null, root: 'Main.ali', source, types: [], holes: [], constraints: [],
});
const rewriting = { mode: 'rewrite', activity: 1, backward: false, rewrite: {
  step_count: 0, can_undo: false, can_redo: false, current: { dim: 1 }, initial: { dim: 1 }, target: null,
  target_reached: false, parallel: true, rewrites: [],
} };
const reply = (fields = {}) => ({
  input: 'show', generation: 1, status: 'ok', message: null,
  rendered: { lines: [[{ role: 'plain', text: 'own reply' }]] }, edited: false, ...fields,
});
const link = () => ({ instance: INSTANCE, generation: 1, tabId: '7', text: 'source' });
const choices = (fields = {}) => ({
  selection: null, preview: null, proof: false, targetPressed: false, ...fields,
});
const selection = (name, fields = {}) => ({
  module: 'Main.ali', type_name: 'T', kind: 'generator', name, generator: null, boundary: null, ...fields,
});
const item = (name, fields = {}) => ({ of: 'item', module: 'Main.ali', type_name: 'T', item: name, boundary: null, ...fields });
const draw = (...want) => want.map(view => ({ view, held: false }));
const noText = () => assert.fail('textOf is asked only when a write is pending');
const textOf = texts => tabId => texts[tabId] ?? null;

// Every answer crosses the actual JSON boundary, as the backends' do.
function answer(fields = {}) {
  return JSON.parse(JSON.stringify({
    status: 'ok', instance: INSTANCE, generation: 1, revision: 1, module_revision: 1, ...fields,
  }));
}
function loaded() {
  return reduce(null, [], answer({ state: idle, module: moduleInfo() })).shown;
}
function holding(entries) {
  return reduce(null, draw(...entries.map(([view]) => view)), answer({
    state: { mode: 'unloaded' }, views: entries.map(([, result]) => result),
  })).shown.views;
}

// Reducer: exact cuts.
test('reduce first answer installs the exact cut, state, module, and positional views', () => {
  const p = answer({
    revision: 17, module_revision: 13, generation: 9,
    state: { ...idle, backward: true }, module: moduleInfo(), views: [ok({ vertices: [] }), error],
  });
  const result = reduce(null, draw(current, target), p);
  assert.deepEqual(result.shown.record, { instance: INSTANCE, revision: 17, module_revision: 13 });
  assert.equal(result.shown.generation, 9);
  assert.equal(result.shown.state, p.state);
  assert.equal(result.shown.module, p.module);
  assert.deepEqual([...result.shown.views.values()], p.views);
  assert.equal(holds(result.shown.views, current), true);
  assert.equal(holds(result.shown.views, target), true);
  assert.deepEqual(result.notes, []);
  assert.deepEqual(result.transcript, []);
});

test('reduce notes a restart, and a change only a poll reveals, without guessing a client', () => {
  const restarted = reduce(loaded(), [], answer({ instance: RESTART, state: idle, module: moduleInfo('foreign'), replies: [] }));
  assert.deepEqual(restarted.notes, ['server restarted']);
  assert.equal(restarted.shown.record.instance, RESTART);
  const first = reduce(null, [], answer({ state: { mode: 'unloaded' } }));
  assert.deepEqual(first.notes, []);
  assert.deepEqual(reduce(first.shown, [], answer({ revision: 2, state: { mode: 'unloaded' } })).notes,
    ['the session changed since this page last showed it']);
  assert.deepEqual(reduce(first.shown, [], answer({ revision: 2 })).notes, []);
  assert.deepEqual(reduce(first.shown, [], answer({ revision: 2, state: { mode: 'unloaded' }, replies: [] })).notes, []);
});

test('reduce keeps the shown state and module unless sent; a moved module stamp with none sent is unloaded', () => {
  const before = loaded();
  const query = reduce(before, [], answer({ revision: 4, replies: [reply()] }));
  assert.equal(query.shown.state, before.state);
  assert.equal(query.shown.module, before.module);
  assert.deepEqual(query.shown.record, { instance: INSTANCE, revision: 4, module_revision: 1 });
  assert.deepEqual(query.transcript, [reply()]);
  const unloaded = reduce(query.shown, draw(current, target), answer({
    generation: 5, revision: 5, module_revision: 5,
    state: { mode: 'unloaded' }, views: [error, error], replies: [reply({ status: 'error', generation: 5 })],
  }));
  assert.equal(unloaded.shown.module, null);
  assert.equal(unloaded.shown.state.mode, 'unloaded');
  const restartedEmpty = reduce(query.shown, [], answer({ instance: RESTART, state: { mode: 'unloaded' } }));
  assert.equal(restartedEmpty.shown.module, null);
  const moduleOnly = reduce(query.shown, [], answer({ module: moduleInfo('changed'), replies: [] }));
  assert.equal(moduleOnly.shown.state, before.state);
  assert.equal(moduleOnly.shown.module.source, 'changed');
});

test('reduce replaces views on state, otherwise joins by draw position, skipping null and holding errors', () => {
  const old = holding([[current, ok('current')], [target, error]]);
  const shown = { ...loaded(), views: old };
  const joined = reduce(shown, [
    { view: target, held: true }, { view: preview(0), held: false }, { view: current, held: true },
  ], answer({ views: [null, error, null], replies: [] }));
  assert.equal(joined.shown.views.size, 3);
  assert.equal(holds(joined.shown.views, preview(0)), true);
  assert.equal(holds(joined.shown.views, target), true);
  assert.equal(joined.shown.views.get(JSON.stringify(['current'])), old.get(JSON.stringify(['current'])));
  assert.equal(old.size, 2);
  const replaced = reduce(joined.shown, draw(target, current), answer({
    state: { mode: 'unloaded' }, views: [error, null], replies: [],
  }));
  assert.equal(replaced.shown.views.size, 1);
  assert.equal(holds(replaced.shown.views, current), false);
  assert.equal(holds(replaced.shown.views, target), true);
  assert.equal(holds(replaced.shown.views, preview(0)), false);
});

test('holds keys views by their specific fields, independent of property order and including boundaries', () => {
  const a = item('x', { boundary: { dim: 0, side: 'input' } });
  const held = holding([[a, error]]);
  assert.equal(holds(held, { boundary: { side: 'input', dim: 0 }, item: 'x', type_name: 'T', module: 'Main.ali', of: 'item' }), true);
  for (const other of [
    { ...a, boundary: { dim: 0, side: 'output' } }, { ...a, boundary: { dim: 1, side: 'input' } },
    { ...a, boundary: null }, { ...a, item: 'y' }, { ...a, type_name: 'U' },
    { of: 'map_image', module: 'Main.ali', type_name: 'T', map: 'x', generator: 'g', boundary: a.boundary },
  ]) assert.equal(holds(held, other), false);
});

// The editor link: settle is its only transition.
test('settle links a successful Evaluate before comparing its module', () => {
  const result = settle(link(), answer({
    instance: RESTART, generation: 8, revision: 8, module_revision: 8,
    replies: [reply({ generation: 8 })], module: moduleInfo('new source'), state: idle,
  }), { tabId: '12', text: 'new source' }, noText);
  assert.deepEqual(result, {
    editorLink: { instance: RESTART, generation: 8, tabId: '12', text: 'new source' }, write: null, editorNotes: [],
  });
});

for (const failure of ['failed', 'refused']) {
  test(`settle leaves the link untouched after a ${failure} Evaluate`, () => {
    const oldLink = link();
    const generation = failure === 'failed' ? 2 : 1;
    const result = settle(oldLink, answer({
      generation, state: { mode: failure === 'failed' ? 'unloaded' : 'idle', backward: false },
      replies: [reply({ status: 'error', generation,
        message: failure === 'failed' ? 'parse error' : 'not run: the session changed since this page last showed it' })],
    }), { tabId: '12', text: 'bad source' }, noText);
    assert.deepEqual(result, { editorLink: oldLink, write: null, editorNotes: [] });
  });
}

test('settle writes only a changed module source of the linked instance and generation', () => {
  const written = settle(link(), answer({ module: moduleInfo('changed') }), null, textOf({ 7: 'source' }));
  assert.deepEqual(written, {
    editorLink: { ...link(), text: 'changed' }, write: { tabId: '7', from: 'source', to: 'changed' }, editorNotes: [],
  });
  for (const fields of [
    { revision: 4, module_revision: 4, module: moduleInfo('source'), state: idle },
    { generation: 2, module: moduleInfo('foreign') },
    { instance: RESTART, module: moduleInfo('foreign') },
    { replies: [reply({ rendered: { lines: [[{ role: 'plain', text: 'source from print' }]] } })] },
  ]) {
    const oldLink = link();
    assert.deepEqual(settle(oldLink, answer(fields), null, noText), { editorLink: oldLink, write: null, editorNotes: [] });
  }
  assert.deepEqual(settle(null, answer({ module: moduleInfo('foreign') }), null, noText),
    { editorLink: null, write: null, editorNotes: [] });
});

test('settle names why an own edit did not reach the editor', () => {
  const store = generation => reply({ input: 'store p', generation, edited: true });
  for (const [editorLink, fields, reason] of [
    [null, {}, 'the source was not loaded by Evaluate on this page'],
    [link(), { instance: RESTART }, 'the server restarted after this page\'s Evaluate'],
    [link(), { generation: 2, revision: 2, module_revision: 2 }, 'the session was reloaded after this page\'s Evaluate'],
  ]) {
    const result = settle(editorLink, answer({ replies: [reply({ input: 'print' }), store(fields.generation ?? 1)],
      module: moduleInfo('foreign'), ...fields }), null, noText);
    assert.deepEqual(result.editorNotes, [`not written to the editor: ${reason}`]);
    assert.equal(result.write, null);
  }
});

test('settle notes an own edit replaced by a later successful or failed load in the same line', () => {
  for (const failed of [false, true]) {
    const result = settle(link(), answer({
      generation: 3, revision: 3, module_revision: 3,
      replies: [reply({ input: 'store p', edited: true }),
        reply({ input: 'load B.ali', generation: 3, status: failed ? 'error' : 'ok' })],
      state: { mode: failed ? 'unloaded' : 'idle', backward: false },
      ...(failed ? {} : { module: moduleInfo('B source') }),
    }), null, noText);
    assert.deepEqual(result, {
      editorLink: link(), write: null,
      editorNotes: ['not written to the editor: a later load in the same line replaced it'],
    });
  }
});

// Commit-time settlement reads the linked tab's CURRENT text, never send-time tabs.
for (const [kind, text, reason] of [
  ['unchanged', 'source', null],
  ['edited', 'user edit', 'the tab was edited since'],
  ['closed', undefined, 'the tab is closed'],
]) {
  test(`settle against a ${kind} tab: only an accepted write advances the link text`, () => {
    const asked = [];
    const result = settle(link(), answer({ module: moduleInfo('changed'), replies: [] }), null, tabId => {
      asked.push(tabId);
      return textOf({ 7: text })(tabId);
    });
    assert.deepEqual(asked, ['7']);
    assert.deepEqual(result, reason
      ? { editorLink: link(), write: null, editorNotes: [`not written to the editor: ${reason}`] }
      : { editorLink: { ...link(), text: 'changed' }, write: { tabId: '7', from: 'source', to: 'changed' }, editorNotes: [] });
  });
}

// Choices and the one canvas.
test('views asks current and target, plus only the view the canvas owner draws', () => {
  const selections = [selection('a'), selection('d', { kind: 'diagram' }), selection('F', { kind: 'map' }),
    selection('F', { kind: 'map', generator: 'g' })];
  for (const previewed of [null, 0, 19]) {
    for (const proofed of [false, true]) {
      for (const targetPressed of [false, true]) {
        const session = choices({ preview: previewed, proof: proofed, targetPressed });
        const owned = targetPressed ? [] : previewed !== null ? [preview(previewed)] : proofed ? [proof] : [];
        assert.deepEqual(views(session), [current, target, ...owned]);
        for (const selected of selections) {
          for (const boundary of [null, { dim: 0, side: 'input' }, { dim: 2, side: 'output' }]) {
            const s = { ...selected, boundary };
            const view = s.kind !== 'map' ? [item(s.name, { boundary })]
              : s.generator ? [{ of: 'map_image', module: 'Main.ali', type_name: 'T', map: 'F', generator: 'g', boundary }] : [];
            assert.deepEqual(views({ ...session, selection: s }), [current, target, ...view]);
          }
        }
      }
    }
  }
});

test('canvas gives the selection the canvas, otherwise target press, preview, proof, then current', () => {
  const c = choices({ selection: selection('a'), targetPressed: true, preview: 0, proof: true });
  assert.deepEqual(canvas(c), { owner: 'selection', view: item('a') });
  c.selection = null;
  assert.deepEqual(canvas(c), { owner: 'session', view: target });
  c.targetPressed = false;
  assert.deepEqual(canvas(c), { owner: 'session', view: preview(0) });
  c.preview = null;
  assert.deepEqual(canvas(c), { owner: 'session', view: proof });
  c.proof = false;
  assert.deepEqual(canvas(c), { owner: 'session', view: current });
  assert.deepEqual(canvas(choices({ selection: selection('F', { kind: 'map' }) })), { owner: 'selection', view: null });
});

const canvasCases = [
  ['current', choices(), current],
  ['target press', choices({ targetPressed: true }), target],
  ['preview', choices({ preview: 0 }), preview(0)],
  ['proof', choices({ proof: true }), proof],
  ['item boundary', choices({ selection: selection('a', { boundary: { dim: 0, side: 'output' } }) }),
    item('a', { boundary: { dim: 0, side: 'output' } })],
  ['map image boundary', choices({ selection: selection('F', { kind: 'map', generator: 'g', boundary: { dim: 1, side: 'input' } }) }),
    { of: 'map_image', module: 'Main.ali', type_name: 'T', map: 'F', generator: 'g', boundary: { dim: 1, side: 'input' } }],
];
for (const [name, choice, view] of canvasCases) {
  test(`drawn ${name}: held payload, permitted fallback, pending, or the held error's message`, () => {
    const owner = choice.selection ? 'selection' : 'session';
    const data = { strdiag: name };
    assert.deepEqual(drawn(holding([[view, ok(data)]]), choice), { owner, kind: 'ok', view, data });
    const missing = holding(view.of === 'current' ? [] : [[current, ok('fallback')]]);
    assert.deepEqual(drawn(missing, choice), owner === 'selection' || view.of === 'current'
      ? { owner, kind: 'pending', view } : { owner, kind: 'ok', view: current, data: 'fallback' });
    const failed = holding(view.of === 'current' ? [[view, error]] : [[current, ok('fallback')], [view, error]]);
    assert.deepEqual(drawn(failed, choice), { owner, kind: 'error', view, message: error.message });
  });
}

test('drawn map without a chosen generator draws nothing, not the session or a previous item', () => {
  const held = holding([[current, ok('session')], [item('old'), ok('old')]]);
  assert.deepEqual(drawn(held, choices({ selection: selection('F', { kind: 'map' }), proof: true })),
    { owner: 'selection', kind: 'none' });
});

test('drawn never draws A once B is selected, whether A arrives before or after B', () => {
  const choiceB = choices({ selection: selection('B') });
  for (const aFirst of [true, false]) {
    let shown = loaded();
    for (const view of aFirst ? [item('A'), item('B')] : [item('B'), item('A')]) {
      shown = reduce(shown, draw(view), answer({ views: [ok(view.item)], replies: [] })).shown;
      assert.deepEqual(drawn(shown.views, choiceB), holds(shown.views, item('B'))
        ? { owner: 'selection', kind: 'ok', view: item('B'), data: 'B' }
        : { owner: 'selection', kind: 'pending', view: item('B') });
    }
  }
});

test('drawn never draws a left boundary, map generator, preview or proof choice', () => {
  const oldMap = { of: 'map_image', module: 'Main.ali', type_name: 'T', map: 'F', generator: 'g', boundary: null };
  const held = holding([[current, ok('current')], [item('a', { boundary: { dim: 0, side: 'input' } }), ok('old boundary')],
    [oldMap, ok('old generator')], [preview(0), ok('old preview')], [proof, ok('old proof')]]);
  const output = item('a', { boundary: { dim: 0, side: 'output' } });
  assert.deepEqual(drawn(held, choices({ selection: selection('a', { boundary: output.boundary }) })),
    { owner: 'selection', kind: 'pending', view: output });
  assert.deepEqual(drawn(held, choices({ selection: selection('F', { kind: 'map', generator: 'h' }) })),
    { owner: 'selection', kind: 'pending', view: { ...oldMap, generator: 'h' } });
  assert.deepEqual(drawn(held, choices({ preview: 1 })), { owner: 'session', kind: 'ok', view: current, data: 'current' });
  assert.deepEqual(drawn(held, choices()), { owner: 'session', kind: 'ok', view: current, data: 'current' });
  assert.deepEqual(drawn(new Map(), choices({ proof: true })), { owner: 'session', kind: 'pending', view: current });
});

// The loop. Tests synchronize on calls, commits and failures; quiescent()
// drains every microtask, after which a loop with no call outstanding waits.
// No timers, sleeps, or assumptions about a number of microtask turns.
function deferred() {
  let resolve, reject;
  const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
function mailbox() {
  const pending = [], readers = [];
  return {
    put(value) { if (readers.length) readers.shift()(value); else pending.push(value); },
    next() { return pending.length ? Promise.resolve(pending.shift()) : new Promise(resolve => readers.push(resolve)); },
  };
}
const quiescent = () => new Promise(resolve => setImmediate(resolve));

function runner(options = {}) {
  const calls = mailbox(), commits = mailbox(), failures = mailbox();
  const sent = [], committed = [], failed = [], painted = [];
  const tabs = new Map([['7', 'source']]);
  let state = { shown: null, editorLink: null };
  let localChoices = choices();
  let inFlight = 0, maxInFlight = 0;
  const step = (name, captured = null) => page => {
    assert.equal(inFlight, 0, 'dispatch never overlaps another call');
    inFlight++;
    maxInFlight = Math.max(maxInFlight, inFlight);
    const completion = deferred();
    const call = { name, captured, page, ...completion };
    sent.push(call);
    calls.put(call);
    return completion.promise.finally(() => { inFlight--; });
  };
  const loop = run({
    poll: step('poll'), getState: () => state, choices: () => localChoices,
    textOf: tabId => tabs.get(tabId) ?? null,
    // The app's whole commit obligation: apply the write and install the
    // settled cut and link. A new state ends the held preview, as a new menu does.
    commit(result, p) {
      options.beforeCommit?.(result, p);
      if (result.write) tabs.set(result.write.tabId, result.write.to);
      state = { shown: result.shown, editorLink: result.editorLink };
      if (Object.hasOwn(p, 'state')) localChoices = { ...localChoices, preview: null, ...options.stateChoices };
      const entry = { result, p, state };
      committed.push(entry);
      commits.put(entry);
    },
    paint(result) { options.beforePaint?.(result); painted.push(result); },
    onError(error) { failed.push(error); failures.put(error); },
  });
  return {
    loop, calls, commits, failures, sent, committed, failed, painted, step, tabs,
    get state() { return state; }, get localChoices() { return localChoices; },
    get maxInFlight() { return maxInFlight; },
    choose(fields) { localChoices = { ...localChoices, ...fields }; loop.choicesChanged(); },
  };
}
async function boot(h, fields = {}) {
  const first = await h.calls.next();
  assert.equal(first.name, 'poll');
  assert.equal(first.page.seen, null);
  assert.deepEqual(first.page.draw, draw(current, target));
  first.resolve(answer({ state: idle, module: moduleInfo(), views: [error, error], ...fields }));
  await h.commits.next();
}
const evaluating = h => [h.step('Evaluate'), { tabId: '7', text: 'source' }];

test('run serializes FIFO commands, committing each answer before the next dispatch', async () => {
  const h = runner();
  await boot(h);
  const expect = { instance: h.state.shown.record.instance, revision: h.state.shown.record.revision };
  h.loop.enqueue(h.step('first', { line: 'apply 0', expect }));
  const first = await h.calls.next();
  h.loop.focus();
  h.loop.enqueue(h.step('second', { line: 'done', expect }));
  h.loop.enqueue(h.step('third', { line: 'help', expect }));
  assert.equal(h.sent.length, 2);
  first.resolve(answer({ revision: 2, replies: [reply({ input: 'apply 0' })] }));
  const second = await h.calls.next();
  assert.equal(second.name, 'second');
  assert.equal(h.committed.at(-1).p.revision, 2);
  assert.equal(second.page.seen.revision, 2);
  assert.equal(second.captured.expect.revision, 1);
  second.resolve(answer({ revision: 3, replies: [reply({ input: 'done' })] }));
  const third = await h.calls.next();
  assert.equal(third.name, 'third');
  assert.equal(h.committed.at(-1).p.revision, 3);
  assert.equal(third.page.seen.revision, 3);
  third.resolve(answer({ replies: [reply({ input: 'help' })], revision: 3 }));
  await quiescent();
  // The focus raised in flight was answered by the first command's cut.
  assert.deepEqual(h.sent.map(c => c.name), ['poll', 'first', 'second', 'third']);
  assert.equal(h.maxInFlight, 1);
  assert.equal(h.committed.length, 4);
});

test('run polls only for a missing view or a focus; a focus in flight is answered by that call', async () => {
  const h = runner();
  await boot(h);
  h.choose({ proof: false }); // Nothing missing.
  h.choose({ targetPressed: true }); // The target is held: pressing it asks nothing.
  h.choose({ targetPressed: false });
  await quiescent();
  assert.equal(h.sent.length, 1);
  h.loop.focus();
  const focus = await h.calls.next();
  assert.equal(focus.name, 'poll');
  assert.deepEqual(focus.page.seen, h.state.shown.record);
  assert.deepEqual(focus.page.draw, [{ view: current, held: true }, { view: target, held: true }]);
  h.loop.focus(); // A second announcement of the same return.
  focus.resolve(answer());
  await quiescent();
  assert.equal(h.sent.length, 2);
  h.choose({ proof: true });
  const missing = await h.calls.next();
  assert.deepEqual(missing.page.draw, [{ view: current, held: true }, { view: target, held: true }, { view: proof, held: false }]);
});

test('run keeps a focus raised during a call that fails', async () => {
  const h = runner();
  await boot(h);
  h.loop.focus();
  const lost = await h.calls.next();
  h.loop.focus();
  lost.reject(new Failure('transport', 'offline'));
  await h.failures.next();
  const retry = await h.calls.next();
  assert.equal(retry.name, 'poll');
  retry.resolve(answer());
  await quiescent();
  assert.equal(h.sent.length, 3);
});

test('run idle current/target errors are held without missing-view retries', async () => {
  const h = runner();
  await boot(h);
  assert.equal(holds(h.state.shown.views, current), true);
  assert.equal(holds(h.state.shown.views, target), true);
  assert.deepEqual(h.painted.at(-1), { owner: 'session', kind: 'error', view: current, message: error.message });
  await quiescent();
  assert.equal(h.sent.length, 1);
});

test('run choice changed in flight paints the fallback at once and asks for its view after the answer', async () => {
  const h = runner();
  await boot(h, { state: rewriting, views: [ok('current'), error] });
  h.choose({ preview: 0 });
  const a = await h.calls.next();
  h.choose({ preview: 1 });
  assert.deepEqual(h.painted.at(-1), { owner: 'session', kind: 'ok', view: current, data: 'current' });
  assert.equal(h.sent.length, 2);
  a.resolve(answer({ views: [null, null, ok('preview A')] }));
  const b = await h.calls.next();
  assert.equal(holds(h.state.shown.views, preview(0)), true);
  assert.deepEqual(h.painted.at(-1), { owner: 'session', kind: 'ok', view: current, data: 'current' });
  assert.deepEqual(b.page.draw, [
    { view: current, held: true }, { view: target, held: true }, { view: preview(1), held: false },
  ]);
  b.resolve(answer({ views: [null, null, ok('preview B')] }));
  await quiescent();
  assert.deepEqual(h.painted.at(-1), { owner: 'session', kind: 'ok', view: preview(1), data: 'preview B' });
  assert.equal(h.committed.length, 3);
});

test('run choices returned to the sent ones in flight spend no extra poll', async () => {
  const h = runner();
  await boot(h);
  h.loop.focus();
  const pending = await h.calls.next();
  h.choose({ proof: true });
  h.choose({ proof: false });
  pending.resolve(answer());
  await quiescent();
  assert.equal(h.sent.length, 2);
});

test('run asks for a view the commit itself adds', async () => {
  const h = runner({ stateChoices: { proof: true } });
  await boot(h);
  const proofPoll = await h.calls.next();
  assert.deepEqual(proofPoll.page.draw.at(-1), { view: proof, held: false });
});

for (const event of ['command', 'choice', 'focus']) {
  test(`run failed poll quietens its missing view until a new ${event} event`, async () => {
    const h = runner();
    await boot(h);
    const before = h.state;
    h.choose({ proof: true });
    const poll = await h.calls.next();
    const failure = new Failure('transport', 'backend unreachable');
    poll.reject(failure);
    assert.equal(await h.failures.next(), failure);
    await quiescent();
    assert.equal(h.state, before);
    assert.equal(holds(h.state.shown.views, proof), false);
    assert.equal(h.sent.length, 2);
    if (event === 'command') h.loop.enqueue(h.step('help', { line: 'help' }));
    else if (event === 'choice') h.choose({ proof: true });
    else h.loop.focus();
    const next = await h.calls.next();
    assert.equal(next.name, event === 'command' ? 'help' : 'poll');
    assert.deepEqual(next.page.draw.at(-1), { view: proof, held: false });
    next.resolve(answer({ views: next.page.draw.map(d => d.held ? null : error),
      ...(event === 'command' ? { replies: [reply({ input: 'help' })] } : {}) }));
    await quiescent();
    assert.equal(h.committed.length, 2);
    assert.equal(h.sent.length, 3);
    assert.equal(holds(h.state.shown.views, proof), true);
  });
}

test('run failed initial poll waits with no cut until a focus', async () => {
  const h = runner();
  const initial = await h.calls.next();
  initial.reject(new Failure('transport', 'offline'));
  await h.failures.next();
  await quiescent();
  assert.equal(h.state.shown, null);
  assert.equal(h.sent.length, 1);
  h.loop.focus();
  const recovery = await h.calls.next();
  assert.equal(recovery.page.seen, null);
  recovery.resolve(answer({ state: { mode: 'unloaded' }, views: [error, error] }));
  await quiescent();
  assert.equal(h.committed.length, 1);
});

test('run outer RPC error at boot is a protocol failure that commits and paints nothing', async () => {
  const h = runner();
  const initial = await h.calls.next();
  initial.resolve(answer({ status: 'error', message: 'invalid poll request' }));
  const failure = await h.failures.next();
  await quiescent();
  assert.ok(failure instanceof Failure);
  assert.equal(failure.kind, 'protocol');
  assert.equal(failure.message, 'invalid poll request');
  assert.deepEqual(h.state, { shown: null, editorLink: null });
  assert.equal(h.committed.length, 0);
  assert.equal(h.painted.length, 0);
  assert.equal(h.sent.length, 1);
});

for (const [name, views, message] of [
  ['too many', [error, error, error], '3 view places answer 2 draw entries'],
  ['too few', [error], '1 view places answer 2 draw entries'],
]) {
  test(`run answer with ${name} view places is a protocol failure`, async () => {
    const h = runner();
    const initial = await h.calls.next();
    initial.resolve(answer({ state: idle, views }));
    const failure = await h.failures.next();
    assert.deepEqual([failure.kind, failure.message], ['protocol', message]);
    await quiescent();
    assert.equal(h.committed.length, 0);
    assert.equal(h.sent.length, 1);
  });
}

test('run outer RPC error preserves the shown cut and editor link until an explicit event recovers', async () => {
  const h = runner();
  await boot(h);
  h.loop.enqueue(...evaluating(h));
  const load = await h.calls.next();
  load.resolve(answer({ module: moduleInfo(), replies: [reply()] }));
  await h.commits.next();
  const before = h.state;
  assert.deepEqual(before.editorLink, link());
  h.choose({ proof: true });
  const poll = await h.calls.next();
  const paintCount = h.painted.length;
  poll.resolve(answer({ status: 'error', message: 'invalid draw request' }));
  const failure = await h.failures.next();
  await quiescent();
  assert.deepEqual([failure.kind, failure.message], ['protocol', 'invalid draw request']);
  assert.equal(h.state, before);
  assert.equal(h.tabs.get('7'), 'source');
  assert.equal(h.committed.length, 2);
  assert.equal(h.painted.length, paintCount);
  assert.equal(h.sent.length, 3);
  assert.equal(holds(h.state.shown.views, proof), false);

  h.loop.focus();
  const recovery = await h.calls.next();
  assert.equal(recovery.page.seen, before.shown.record);
  assert.deepEqual(recovery.page.draw.at(-1), { view: proof, held: false });
  recovery.resolve(answer({ revision: 2, module_revision: 2, module: moduleInfo('recovered source'),
    views: [null, null, error] }));
  await quiescent();
  assert.equal(h.state.shown.record.revision, 2);
  assert.equal(h.state.editorLink.text, 'recovered source');
  assert.equal(h.tabs.get('7'), 'recovered source');
  assert.equal(h.committed.length, 3);
  assert.equal(h.failed.length, 1);
  assert.equal(h.sent.length, 4);
});

test('run outer ok with inner reply and view errors still commits the cut', async () => {
  const h = runner();
  await boot(h);
  h.loop.enqueue(h.step('failed load', { line: 'load Broken.ali' }));
  const call = await h.calls.next();
  const p = answer({ generation: 2, revision: 2, module_revision: 2,
    state: { mode: 'unloaded' }, views: [error, error],
    replies: [reply({ input: 'load Broken.ali', generation: 2, status: 'error', message: 'parse error' })] });
  call.resolve(p);
  await quiescent();
  assert.equal(h.state.shown.record.revision, 2);
  assert.equal(h.state.shown.state.mode, 'unloaded');
  assert.equal(h.state.shown.module, null);
  assert.deepEqual(h.committed.at(-1).result.transcript, p.replies);
  assert.deepEqual(h.painted.at(-1), { owner: 'session', kind: 'error', view: current, message: error.message });
  assert.equal(h.committed.length, 2);
  assert.equal(h.failed.length, 0);
  assert.equal(h.sent.length, 2);
});

test('run Evaluate and line retain action-time expect/files while page seen and draw come from dispatch', async () => {
  const h = runner();
  await boot(h);
  h.loop.enqueue(h.step('earlier command'));
  const earlier = await h.calls.next();
  const expect = { instance: h.state.shown.record.instance, revision: h.state.shown.record.revision };
  let text = 'source';
  const modules = { 'Main.ali': text, 'Aux.ali': 'captured auxiliary' };
  // This is the app's action-time closure, not work done by run at dispatch.
  const captured = { expect: { ...expect }, text, modules: { ...modules }, tabId: '7', name: 'Main.ali' };
  h.loop.enqueue(h.step('Evaluate', captured), { tabId: captured.tabId, text: captured.text });
  h.loop.enqueue(h.step('line', { expect: { ...expect }, line: 'store captured' }));
  text = 'new typing';
  modules['Aux.ali'] = 'new disk source';
  h.choose({ proof: true });
  earlier.resolve(answer({ revision: 2, replies: [reply({ input: 'earlier' })] }));
  const load = await h.calls.next();
  assert.equal(load.page.seen.revision, 2);
  assert.equal(load.captured.expect.revision, 1);
  assert.equal(load.captured.text, 'source');
  assert.equal(load.captured.modules['Aux.ali'], 'captured auxiliary');
  assert.deepEqual(load.page.draw.at(-1), { view: proof, held: false });
  load.resolve(answer({ generation: 3, revision: 3, module_revision: 3,
    state: idle, module: moduleInfo('source'), replies: [reply({ generation: 3 })], views: [error, error, error] }));
  const line = await h.calls.next();
  assert.equal(line.page.seen.revision, 3);
  assert.equal(line.captured.expect.revision, 1);
  assert.equal(line.captured.line, 'store captured');
  assert.deepEqual(h.state.editorLink, { instance: INSTANCE, generation: 3, tabId: '7', text: 'source' });
  line.resolve(answer({ generation: 3, revision: 3, module_revision: 3, replies: [reply()] }));
  await quiescent();
  assert.equal(h.sent.length, 4); // Evaluate answered the proof view asked in flight.
});

test('run commit ends a held preview on state before painting; the next dispatch reads current choices', async () => {
  const h = runner();
  await boot(h, { views: [ok('current'), error] });
  h.choose({ preview: 0 });
  const pending = await h.calls.next();
  h.loop.enqueue(h.step('after state'));
  pending.resolve(answer({ revision: 2, state: rewriting, views: [ok('new current'), error, ok('old menu preview')] }));
  const next = await h.calls.next();
  assert.equal(h.localChoices.preview, null);
  assert.deepEqual(h.painted.at(-1), { owner: 'session', kind: 'ok', view: current, data: 'new current' });
  assert.deepEqual(next.page.draw, [{ view: current, held: true }, { view: target, held: true }]);
  next.resolve(answer({ revision: 2, replies: [reply()] }));
  await quiescent();
  assert.equal(h.sent.length, 3);
});

test('run settles each answer against the installed link, so successive stores each reach the tab', async () => {
  const h = runner();
  await boot(h);
  h.loop.enqueue(...evaluating(h));
  const load = await h.calls.next();
  load.resolve(answer({ module: moduleInfo(), replies: [reply()] }));
  await h.commits.next();
  for (const [revision, from, to] of [[2, 'source', 'stored once'], [3, 'stored once', 'stored twice']]) {
    h.loop.enqueue(h.step('store'));
    const store = await h.calls.next();
    store.resolve(answer({ revision, module_revision: revision, module: moduleInfo(to),
      replies: [reply({ input: 'store p', edited: true })] }));
    const { result } = await h.commits.next();
    assert.deepEqual(result.write, { tabId: '7', from, to });
    assert.equal(h.state.editorLink.text, to);
    assert.equal(h.tabs.get('7'), to);
  }
  assert.equal(h.failed.length, 0);
});

test('run rejected step leaves cut/link untouched and later queued answers commit normally', async () => {
  const h = runner();
  await boot(h);
  h.loop.enqueue(...evaluating(h));
  const load = await h.calls.next();
  load.resolve(answer({ module: moduleInfo(), replies: [reply()] }));
  await h.commits.next();
  const before = h.state;
  const fetch = deferred(), fetchStarted = deferred();
  h.loop.enqueue(async () => { fetchStarted.resolve(); await fetch.promise; assert.fail('a failed step ends'); });
  await fetchStarted.promise;
  h.loop.enqueue(h.step('later line'));
  const failure = new Error('step failed');
  fetch.reject(failure);
  assert.equal(await h.failures.next(), failure);
  const later = await h.calls.next();
  assert.equal(h.state, before);
  assert.equal(later.page.seen, before.shown.record);
  later.resolve(answer({ revision: 2, module_revision: 2, module: moduleInfo('later source'),
    replies: [reply({ edited: true })] }));
  await quiescent();
  assert.equal(h.state.editorLink.text, 'later source');
  assert.equal(h.committed.length, 3);
  assert.equal(h.failed.length, 1);
});

test('run failed backend command spends demand; an already queued later command commits normally', async () => {
  const h = runner();
  await boot(h);
  const before = h.state;
  h.loop.enqueue(h.step('lost answer'));
  const lost = await h.calls.next();
  h.loop.enqueue(h.step('later'));
  lost.reject(new Failure('transport', 'answer lost'));
  await h.failures.next();
  const later = await h.calls.next();
  assert.equal(h.state, before);
  later.resolve(answer({ revision: 4, state: { mode: 'unloaded' }, views: [error, error], replies: [reply()] }));
  await quiescent();
  assert.equal(h.state.shown.record.revision, 4);
  assert.equal(h.committed.length, 2);
  assert.deepEqual(h.sent.map(c => c.name), ['poll', 'lost answer', 'later']);
});

test('run commit exceptions reject failed and dispatch no further call', async () => {
  const fatal = new Error('commit invariant failed');
  const h = runner({ beforeCommit: () => { throw fatal; } });
  const first = await h.calls.next();
  h.loop.enqueue(h.step('must not run'));
  const rejected = assert.rejects(h.loop.failed, error => error === fatal);
  first.resolve(answer({ state: { mode: 'unloaded' }, views: [error, error] }));
  await rejected;
  assert.equal(h.sent.length, 1);
  assert.equal(h.failed.length, 0);
  assert.equal(h.committed.length, 0);
});

test('run reports a paint failure and keeps going', async () => {
  let broken = true;
  const h = runner({ beforePaint: () => { if (broken) throw new Error('layout failed'); } });
  await boot(h);
  const failure = await h.failures.next();
  assert.deepEqual([failure.kind, failure.message], ['paint', 'layout failed']);
  broken = false;
  h.loop.enqueue(h.step('help'));
  const help = await h.calls.next();
  help.resolve(answer({ replies: [reply({ input: 'help' })] }));
  await quiescent();
  assert.equal(h.committed.length, 2);
  assert.equal(h.painted.length, 1);
});
