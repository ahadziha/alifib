/** Pure derivations from wire data for the browser UI (no DOM, no calls). */

/** The engine state behind a rewrite or a fill by rewrite, else null. */
export function rewriteOf(state) {
  return state.mode === 'rewrite' ? state.rewrite
    : state.mode === 'fill' ? state.by.rewrite ?? null : null;
}

/** The choice state of a boundaryless 0-cell fill, else null. */
export function zeroOf(state) {
  return state.mode === 'fill' ? state.by.zero_cell ?? null : null;
}

/** What the state admits. Target and the proof toggle concern the session's
 * canvas, so they are offered only while the session owns it.
 */
export function controls(state, owner) {
  const rewrite = rewriteOf(state);
  const engine = zeroOf(state) ?? rewrite;
  const active = state.mode === 'rewrite' || state.mode === 'fill';
  const session = owner === 'session';
  return {
    accordion: state.mode !== 'unloaded',
    setup: state.mode === 'idle',
    stop: active,
    done: state.mode === 'fill',
    canDone: state.mode === 'fill' && engine.target_reached,
    canUndo: Boolean(engine?.can_undo),
    canRedo: Boolean(engine?.can_redo),
    target: session && Boolean(rewrite?.target),
    proof: session && rewrite !== null,
    backward: Boolean(state.backward),
  };
}

/** One session of the page: a load, and within it the rewrite, resume or fill
 * that began. Session UI (selection, proof view, bunch, hover) lives this long.
 */
export function sessionKey(cut) {
  return cut && JSON.stringify([cut.record.instance, cut.generation, cut.state.activity ?? null]);
}

/** The menu that bunch and preview indices refer to. */
export function menuKey(state) {
  const rewrite = rewriteOf(state);
  const zero = zeroOf(state);
  return JSON.stringify(rewrite ? [rewrite.parallel, rewrite.rewrites] : zero ? zero.choices : null);
}

/** Display a full store path without changing its wire identity. */
export function moduleLabel(path, base) {
  const prefix = base ? base.replace(/\/+$/, '') + '/' : null;
  const relative = prefix && path.startsWith(prefix) ? path.slice(prefix.length) : path;
  return relative.replace(/\.ali$/, '');
}

const ROWS = { generator: 'generators', diagram: 'diagrams', map: 'maps' };

/** The module row a selection addresses, or null when the module lacks it. */
export function itemOf(module, selection) {
  const type = module.types.find(type => type.module === selection.module && type.name === selection.type_name);
  return type?.[ROWS[selection.kind]].find(item => item.name === selection.name) ?? null;
}

/** Each tag's canonical identity: its FIRST generator row in module order,
 * never a diagram row, a display label, or the alias through which it was
 * toggled. Names survive store and done even if global tag ids are reassigned.
 */
export function generatorKeys(module) {
  const keys = new Map();
  for (const type of module.types) {
    for (const generator of type.generators) {
      if (generator.tag != null && !keys.has(generator.tag)) {
        keys.set(generator.tag, JSON.stringify([type.module, type.name, generator.name]));
      }
    }
  }
  return keys;
}

/** The module's thin tags, overridden by the overlay Map<generatorKey,boolean>. */
export function thinTags(module, overlay) {
  const tags = new Set(module.types.flatMap(type => type.thin_tags));
  for (const [tag, key] of generatorKeys(module)) {
    if (!overlay.has(key)) continue;
    if (overlay.get(key)) tags.add(tag);
    else tags.delete(tag);
  }
  return tags;
}

/** ModuleInfo has no counters: scope the local overlay to each answer's load.
 * Initial overlay is null; callers toggle entries of the returned values Map.
 */
export function scopeThinOverlay(overlay, instance, generation) {
  return overlay && overlay.instance === instance && overlay.generation === generation
    ? overlay
    : { instance, generation, values: new Map() };
}

/** The type is a name; diagrams are expressions, not command string literals. */
export function formatStartCmd(typeName, source, target = '') {
  const diagram = text => /^[\p{Alphabetic}\p{N}_.]+$/u.test(text) ? text : `(${text})`;
  const fields = ['start', typeName, diagram(source.trim())];
  if (target.trim()) fields.push(diagram(target.trim()));
  return fields.join(' ');
}
