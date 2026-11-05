import test from 'node:test';
import assert from 'node:assert/strict';
import {
  rewriteOf, zeroOf, controls, sessionKey, menuKey, moduleLabel, itemOf,
  generatorKeys, thinTags, scopeThinOverlay, formatStartCmd,
} from '../src/model.js';

const INSTANCE = '1780000000000000001';
const RESTART = '1780000000000000002';
const rewriteState = (fields = {}) => ({
  step_count: 0, can_undo: false, can_redo: false, current: { dim: 1 }, initial: { dim: 1 }, target: null,
  target_reached: false, parallel: true, rewrites: [], ...fields,
});
const moduleInfo = (types = []) => ({ base: null, root: 'Main.ali', source: 'source', types, holes: [], constraints: [] });

// Controls depend only on each answered state and the canvas owner.
for (const mode of ['unloaded', 'idle', 'rewrite']) {
  test(`controls ${mode}: setup/stop/backward from state alone`, () => {
    const state = { mode, ...(mode === 'unloaded' ? {} : { backward: true }),
      ...(mode === 'rewrite' ? { activity: 1, rewrite: rewriteState({ step_count: 2, can_undo: true, can_redo: true, target: { dim: 1 } }) } : {}) };
    assert.deepEqual(controls(state, 'session'), {
      accordion: mode !== 'unloaded', setup: mode === 'idle', stop: mode === 'rewrite', done: false, canDone: false,
      canUndo: mode === 'rewrite', canRedo: mode === 'rewrite', target: mode === 'rewrite', proof: mode === 'rewrite',
      backward: mode !== 'unloaded',
    });
  });
}
for (const dim of [1, 2]) {
  for (const reached of [false, true]) {
    test(`controls ${dim}-cell fill by rewrite, target reached=${reached}`, () => {
      const state = { mode: 'fill', activity: 1, backward: false, hole: { dim }, by: {
        rewrite: rewriteState({ step_count: reached ? 1 : 0, can_undo: reached, can_redo: !reached,
          current: { dim: dim - 1 }, target: { dim: dim - 1 }, target_reached: reached }),
      } };
      assert.deepEqual(controls(state, 'session'), {
        accordion: true, setup: false, stop: true, done: true, canDone: reached,
        canUndo: reached, canRedo: !reached, target: true, proof: true, backward: false,
      });
    });
  }
}
for (const chosen of [false, true]) {
  for (const reached of [false, true]) {
    test(`controls 0-cell fill chosen=${chosen}, target reached=${reached}`, () => {
      const state = { mode: 'fill', activity: 1, backward: true, hole: { dim: 0 }, by: { zero_cell: {
        choices: chosen ? [] : [{ index: 0, name: 'x' }], ...(chosen ? { chosen: 'x' } : {}),
        target_reached: reached, can_undo: chosen, can_redo: !chosen,
      } } };
      assert.deepEqual(controls(state, 'session'), {
        accordion: true, setup: false, stop: true, done: true, canDone: reached,
        canUndo: chosen, canRedo: !chosen, target: false, proof: false, backward: true,
      });
    });
  }
}

test('controls offer target and proof only while the session owns the canvas; undo/redo/done always', () => {
  const state = { mode: 'fill', activity: 1, backward: false, hole: { dim: 1 }, by: {
    rewrite: rewriteState({ step_count: 1, can_undo: true, target: { dim: 0 }, target_reached: true }),
  } };
  const session = controls(state, 'session');
  const selection = controls(state, 'selection');
  assert.deepEqual([session.target, session.proof], [true, true]);
  assert.deepEqual([selection.target, selection.proof], [false, false]);
  assert.deepEqual({ ...selection, target: true, proof: true }, session);
});

test('rewriteOf and zeroOf read the engine behind each mode', () => {
  const rewrite = rewriteState();
  const zero = { choices: [], target_reached: false, can_undo: false, can_redo: false };
  assert.equal(rewriteOf({ mode: 'rewrite', rewrite }), rewrite);
  assert.equal(rewriteOf({ mode: 'fill', by: { rewrite } }), rewrite);
  assert.equal(rewriteOf({ mode: 'fill', by: { zero_cell: zero } }), null);
  assert.equal(zeroOf({ mode: 'fill', by: { zero_cell: zero } }), zero);
  for (const state of [{ mode: 'idle' }, { mode: 'unloaded' }, { mode: 'rewrite', rewrite }]) {
    assert.equal(zeroOf(state), null);
  }
  assert.equal(rewriteOf({ mode: 'idle' }), null);
});

// Session and menu identities decide what session UI survives an answer.
test('sessionKey changes with the instance, the load, or the activity, and only then', () => {
  const cut = (instance, generation, state) => ({ record: { instance }, generation, state });
  const rewrite = activity => ({ mode: 'rewrite', activity, rewrite: rewriteState() });
  const base = sessionKey(cut(INSTANCE, 3, rewrite(1)));
  assert.equal(sessionKey(cut(INSTANCE, 3, { ...rewrite(1), rewrite: rewriteState({ step_count: 4 }) })), base);
  for (const other of [cut(RESTART, 3, rewrite(1)), cut(INSTANCE, 4, rewrite(1)), cut(INSTANCE, 3, rewrite(2)),
    cut(INSTANCE, 3, { mode: 'idle', backward: false })]) {
    assert.notEqual(sessionKey(other), base);
  }
  assert.equal(sessionKey(null), null);
  assert.equal(sessionKey(cut(INSTANCE, 3, { mode: 'idle' })), sessionKey(cut(INSTANCE, 3, { mode: 'idle', backward: true })));
});

test('menuKey changes exactly with the menu its indices refer to', () => {
  const rewrites = [{ index: 0, rule_name: 'r', match_positions: [0] }];
  const state = fields => ({ mode: 'rewrite', activity: 1, rewrite: rewriteState({ rewrites, ...fields }) });
  const base = menuKey(state());
  assert.equal(menuKey(state({ step_count: 3, can_undo: true })), base);
  assert.notEqual(menuKey(state({ rewrites: [] })), base);
  assert.notEqual(menuKey(state({ parallel: false })), base);
  const zero = choices => ({ mode: 'fill', by: { zero_cell: { choices } } });
  assert.notEqual(menuKey(zero([{ index: 0, name: 'x' }])), menuKey(zero([])));
  assert.equal(menuKey({ mode: 'idle' }), menuKey({ mode: 'unloaded' }));
});

// Module labels never replace wire lookup addresses.
test('moduleLabel strips the exact served base prefix and one .ali suffix only', () => {
  for (const [path, base, expected] of [
    ['/served/Sub/Main.ali', '/served', 'Sub/Main'],
    ['/served/Sub/Main/Aux.ali', '/served/', 'Sub/Main/Aux'],
    ['Sub/Main.ali', null, 'Sub/Main'], ['Sub/Main', null, 'Sub/Main'],
    ['/else/Sub/Main.ali', '/served', '/else/Sub/Main'],
    ['/served-other/Main.ali', '/served', '/served-other/Main'],
    ['/served/Sub/Main.ali', null, '/served/Sub/Main'],
    ['/Sub/Main.ali', '/', 'Sub/Main'], ['A.ali.ali', '', 'A.ali'],
    ['Main.alien', null, 'Main.alien'],
  ]) assert.equal(moduleLabel(path, base), expected);
});

test('itemOf finds the exact module, type and row kind a selection addresses', () => {
  const generator = { name: 'F', tag: 1, dim: 1 };
  const diagram = { name: 'F', dim: 2 };
  const map = { name: 'F', domain: 'D', generators: [] };
  const module = moduleInfo([
    { module: 'A.ali', name: 'T', generators: [generator], diagrams: [diagram], maps: [map], thin_tags: [] },
    { module: 'B.ali', name: 'T', generators: [], diagrams: [], maps: [], thin_tags: [] },
  ]);
  const at = (kind, fields = {}) => itemOf(module, { module: 'A.ali', type_name: 'T', kind, name: 'F', ...fields });
  assert.equal(at('generator'), generator);
  assert.equal(at('diagram'), diagram);
  assert.equal(at('map'), map);
  assert.equal(at('generator', { module: 'B.ali' }), null);
  assert.equal(at('generator', { type_name: 'U' }), null);
  assert.equal(at('diagram', { name: 'G' }), null);
});

function thinModule(onTag = 10, offTag = 20) {
  return moduleInfo([
    { module: 'Included.ali', name: 'T', thin_tags: [offTag],
      generators: [{ name: 'on', tag: onTag }, { name: 'off', tag: offTag }, { name: 'local', tag: null }],
      diagrams: [{ name: 'not-a-generator', tag: onTag }] },
    { module: 'Main.ali', name: 'Alias', thin_tags: [],
      generators: [{ name: 'aliasOn', tag: onTag }, { name: 'aliasOff', tag: offTag }], diagrams: [] },
  ]);
}

test('thinTags on/off overlays survive store and done id reassignment by canonical names', () => {
  const module = thinModule();
  const keys = generatorKeys(module);
  const overlay = new Map([[keys.get(10), true], [keys.get(20), false]]);
  assert.deepEqual(thinTags(module, overlay), new Set([10]));
  const stored = thinModule();
  stored.source = 'source with stored proof';
  stored.types[0].diagrams.push({ name: 'p', tag: 30 });
  assert.deepEqual(thinTags(stored, overlay), new Set([10]));
  assert.deepEqual(thinTags(thinModule(110, 120), overlay), new Set([110]));
  assert.deepEqual(thinTags(moduleInfo(), overlay), new Set());
  assert.deepEqual(thinTags(module, new Map()), new Set([20]));
  assert.equal(overlay.size, 2);
});

test('generatorKeys uses the first GENERATOR row of each tag, ignoring diagram rows and null tags', () => {
  const module = thinModule();
  module.types.unshift({ module: 'Earlier.ali', name: 'U', generators: [], diagrams: [{ name: 'd', tag: 10 }], thin_tags: [] });
  const keys = generatorKeys(module);
  assert.deepEqual([...keys], [
    [10, JSON.stringify(['Included.ali', 'T', 'on'])],
    [20, JSON.stringify(['Included.ali', 'T', 'off'])],
  ]);
});

test('thinTags alternating aliases share one key with the last toggle standing in either row order', () => {
  for (const reversed of [false, true]) {
    const module = thinModule();
    if (reversed) module.types.reverse();
    const keys = generatorKeys(module);
    const overlay = new Map();
    for (const [row, value] of [[0, true], [1, false], [0, true], [1, false]]) {
      overlay.set(keys.get(module.types[row].generators[0].tag), value);
      assert.equal(overlay.size, 1);
      for (const type of module.types) assert.equal(thinTags(module, overlay).has(type.generators[0].tag), value);
    }
  }
});

test('scopeThinOverlay retains within a load and drops on another load or a restart', () => {
  const initial = scopeThinOverlay(null, INSTANCE, 1);
  initial.values.set('key', true);
  assert.equal(scopeThinOverlay(initial, INSTANCE, 1), initial);
  for (const [instance, generation] of [[INSTANCE, 2], [RESTART, 1]]) {
    const next = scopeThinOverlay(initial, instance, generation);
    assert.deepEqual(next, { instance, generation, values: new Map() });
  }
  assert.equal(initial.values.size, 1);
});

test('formatStartCmd keeps digit-first type names and name/dotted diagrams verbatim; groups expressions without quoting', () => {
  for (const [type, source, target, expected] of [
    ['0Simplex', 'a', '', 'start 0Simplex a'],
    ['T', 'A.0cell', 'β_2', 'start T A.0cell β_2'],
    ['T', '  a #0 b  ', 'c', 'start T (a #0 b) c'],
    ['Assoc', '"aaa"', "'bbb'", 'start Assoc ("aaa") (\'bbb\')'],
    ['T', '(a #0 b)', '(c #0 d)', 'start T ((a #0 b)) ((c #0 d))'],
    ['T', 'a (* comment *)', '', 'start T (a (* comment *))'],
  ]) assert.equal(formatStartCmd(type, source, target), expected);
});
