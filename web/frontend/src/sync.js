/** The browser's single-cut adapter (no DOM): one call loop, the reduction of
 * each answer into the shown cut, the editor link, and what the one canvas draws.
 *
 * shown is null before the first answer, then a Cut:
 *   {record:{instance,revision,module_revision}, generation, state,
 *    module:null|ModuleInfo, views:Map<viewKey, wire {status,data|message}>}.
 * module is null exactly while nothing is loaded.
 */

/** Why a call or a paint failed: 'transport', 'http', 'backend' (the app's
 * backends), 'protocol' (a decoded answer breaks the wire), or 'paint'.
 */
export class Failure extends Error {
  constructor(kind, message, options) {
    super(message, options);
    this.kind = kind;
  }
}

const NONE = new Map();
const CURRENT = Object.freeze({ of: 'current' });
const TARGET = Object.freeze({ of: 'target' });

/** Install answer p, asked with draw, over the shown cut. Notes are this
 * page's view of the change; the transcript is p's own replies.
 */
export function reduce(shown, draw, p) {
  const notes = [];
  if (shown && shown.record.instance !== p.instance) notes.push('server restarted');
  // Polls have no replies. A lost answer may have been this page's own
  // mutation, so this note deliberately attributes the change to nobody.
  if (shown && !Object.hasOwn(p, 'replies') && Object.hasOwn(p, 'state')) {
    notes.push('the session changed since this page last showed it');
  }
  const views = shown && !Object.hasOwn(p, 'state') ? new Map(shown.views) : new Map();
  p.views?.forEach((answer, i) => { if (answer !== null) views.set(viewKey(draw[i].view), answer); });
  const sameModule = shown?.record.instance === p.instance && shown.record.module_revision === p.module_revision;
  return {
    shown: {
      record: { instance: p.instance, revision: p.revision, module_revision: p.module_revision },
      generation: p.generation,
      state: Object.hasOwn(p, 'state') ? p.state : shown.state,
      module: Object.hasOwn(p, 'module') ? p.module : sameModule ? shown.module : null,
      views,
    },
    transcript: p.replies ?? [],
    notes,
  };
}

// Whether the editor link names the kernel's source at (instance, generation).
function linkStatus(link, instance, generation) {
  return !link ? 'none'
    : link.instance !== instance ? 'restarted'
    : link.generation !== generation ? 'reloaded' : 'linked';
}

const UNLINKED = {
  none: 'the source was not loaded by Evaluate on this page',
  restarted: 'the server restarted after this page\'s Evaluate',
  reloaded: 'the session was reloaded after this page\'s Evaluate',
};

/** The editor link's only transition. link is null or
 * {instance, generation, tabId, text}: the tab this page's last successful
 * Evaluate loaded, and the text it last sent or received. evaluate is the
 * call's local {tabId, text}, or null. textOf(tabId) gives that tab's text
 * now, or null when it is closed; it is asked only when a write is pending.
 * The app applies `write` in place (never switching tabs) and installs
 * `editorLink`; editorNotes say why an edit did not reach the editor.
 */
export function settle(link, p, evaluate, textOf) {
  const replies = p.replies ?? [];
  if (evaluate && replies.length === 1 && replies[0].status === 'ok') {
    link = { instance: p.instance, generation: replies[0].generation, tabId: evaluate.tabId, text: evaluate.text };
  }
  const editorNotes = [];
  const note = reason => editorNotes.push(`not written to the editor: ${reason}`);
  for (const reply of replies.filter(reply => reply.edited)) {
    const status = linkStatus(link, p.instance, reply.generation);
    if (status !== 'linked') note(UNLINKED[status]);
    else if (reply.generation !== p.generation) note('a later load in the same line replaced it');
  }
  const unchanged = { editorLink: link, write: null, editorNotes };
  if (!Object.hasOwn(p, 'module') || linkStatus(link, p.instance, p.generation) !== 'linked' ||
      p.module.source === link.text) return unchanged;
  const text = textOf(link.tabId);
  if (text === null) note('the tab is closed');
  else if (text !== link.text) note('the tab was edited since');
  else {
    const to = p.module.source;
    return { editorLink: { ...link, text: to }, write: { tabId: link.tabId, from: link.text, to }, editorNotes };
  }
  return unchanged;
}

/** The view an item or map selection asks to draw; a map with no generator
 * chosen asks for none. module and type_name address the exact store type.
 */
export function selectionView(selection) {
  const { module, type_name, name, boundary } = selection;
  if (selection.kind !== 'map') return { of: 'item', module, type_name, item: name, boundary };
  return selection.generator == null ? null
    : { of: 'map_image', module, type_name, map: name, generator: selection.generator, boundary };
}

/** The one canvas's owner and the view it asks to draw (null: none).
 * choices = {selection:null|{module,type_name,kind,name,generator,boundary},
 *            preview:null|number, proof:boolean, targetPressed:boolean}.
 * The selection owns the canvas; otherwise the session draws its target while
 * pressed, a held preview, the proof view, or else its current diagram.
 */
export function canvas(choices) {
  if (choices.selection) return { owner: 'selection', view: selectionView(choices.selection) };
  const view = choices.targetPressed ? TARGET
    : choices.preview != null ? { of: 'preview', choice: choices.preview }
    : choices.proof ? { of: 'proof' } : CURRENT;
  return { owner: 'session', view };
}

/** What a page asks to draw: current and target always (the session's
 * fallback and press), plus the owner's view. Nothing the owner will not draw.
 */
export function views(choices) {
  const { view } = canvas(choices);
  return view && view !== CURRENT && view !== TARGET ? [CURRENT, TARGET, view] : [CURRENT, TARGET];
}

/** What the canvas shows, tagged: {owner, kind:'none'} when the owner asks
 * for nothing; else {owner, kind, view} with kind 'pending' (not yet held),
 * 'ok' (and data) or 'error' (and message). A session view not yet held draws
 * the current diagram instead; a selection never falls back.
 */
export function drawn(held, choices) {
  const { owner, view: asked } = canvas(choices);
  if (!asked) return { owner, kind: 'none' };
  const view = owner === 'session' && !holds(held, asked) ? CURRENT : asked;
  const answer = held.get(viewKey(view));
  return !answer ? { owner, kind: 'pending', view }
    : answer.status === 'ok' ? { owner, kind: 'ok', view, data: answer.data }
    : { owner, kind: 'error', view, message: answer.message };
}

/** A view's identity. Fixed wire fields, not object insertion order, define
 * equality; boundary is part of every item key.
 */
export function viewKey(view) {
  switch (view.of) {
    case 'item':
      return JSON.stringify([view.of, view.module, view.type_name, view.item, view.boundary?.dim ?? null, view.boundary?.side ?? null]);
    case 'map_image':
      return JSON.stringify([view.of, view.module, view.type_name, view.map, view.generator, view.boundary?.dim ?? null, view.boundary?.side ?? null]);
    case 'preview': return JSON.stringify([view.of, view.choice]);
    default: return JSON.stringify([view.of]);
  }
}

export function holds(held, view) {
  return held.has(viewKey(view));
}

// A decoded answer that breaks the wire is a protocol failure, not a cut.
function check(p, draw, shown) {
  if (p.status === 'error') throw new Failure('protocol', p.message);
  if (p.views && p.views.length !== draw.length) {
    throw new Failure('protocol', `${p.views.length} view places answer ${draw.length} draw entries`);
  }
  if (!shown && !Object.hasOwn(p, 'state')) throw new Failure('protocol', 'the first answer carries no state');
}

/** Start the page's only call loop; returns {enqueue, choicesChanged, repaint,
 * focus, failed}. enqueue(step, evaluate = null) queues a step(page) ->
 * Promise<answer>; the app captures line/files/expect when the user acts,
 * never in this loop. evaluate marks a step as Evaluate: {tabId, text}.
 * poll(page) is the same kind of step. page = {seen, draw}: seen is null
 * before the first answer, then the shown record; draw is asked at dispatch.
 *
 * getState() returns the app's sole {shown, editorLink}. After each answer the
 * loop reduces it and settles the editor link (textOf(tabId) gives a tab's
 * text now, or null), then calls commit(result, p) synchronously with
 * {shown, transcript, notes, editorLink, write, editorNotes}: the app applies
 * write and installs shown and editorLink. It may change its choices too.
 *
 * Demand is by missing views: a poll is due while some view the choices want
 * is neither held nor quietened by a failed request. The loop re-checks at
 * every decision, including after each commit, so choices changed in flight
 * or by a commit itself are asked for; nothing else need signal them. A
 * failed call quietens its unheld views until the next event (a choice, focus,
 * or command); there are no automatic retries. focus() (HTTP only) asks for a
 * fresh cut; any answer arriving after it is that cut.
 *
 * Outside commit, update choices then call choicesChanged(): it registers
 * demand, then paints at once, even during a call. repaint() repaints alone.
 * Call failures go to onError with their Failure kind; so do paint failures,
 * which never stop the loop. A failing reduce, settle or commit is a broken
 * invariant: it rejects `failed` and stops the loop. Nothing cancels I/O.
 */
export function run({ poll, getState, choices, textOf, commit, paint, onError }) {
  const queue = [];
  let focusDemand = true; // Page load, then focus events not yet answered.
  let quiet = null; // Keys of views whose request failed, until the next event.
  let wake = null;
  const held = () => getState().shown?.views ?? NONE;
  const missing = () => views(choices()).some(view => !holds(held(), view) && !quiet?.has(viewKey(view)));
  const signal = () => wake?.();
  const repaint = () => {
    try {
      paint(drawn(held(), choices()));
    } catch (error) {
      onError(new Failure('paint', error.message, { cause: error }));
    }
  };

  const failed = (async () => {
    for (;;) {
      if (!queue.length && !focusDemand && !missing()) {
        await new Promise(resolve => { wake = resolve; });
        wake = null;
        continue;
      }
      const { step, evaluate } = queue.shift() ?? { step: poll, evaluate: null };
      const { shown } = getState();
      const draw = views(choices()).map(view => ({ view, held: holds(held(), view) }));
      focusDemand = false;
      let p;
      try {
        p = await step({ seen: shown?.record ?? null, draw });
        check(p, draw, shown);
      } catch (error) {
        quiet = new Set(draw.filter(entry => !entry.held).map(entry => viewKey(entry.view)));
        onError(error);
        continue;
      }
      quiet = null; // Every draw entry is now held.
      focusDemand = false; // This answer is the fresh cut a focus in flight asked for.
      const state = getState();
      commit({ ...reduce(state.shown, draw, p), ...settle(state.editorLink, p, evaluate, textOf) }, p);
      repaint();
    }
  })();

  return {
    enqueue(step, evaluate = null) { queue.push({ step, evaluate }); signal(); },
    choicesChanged() {
      quiet = null;
      if (missing()) signal();
      repaint();
    },
    repaint,
    focus() { focusDemand = true; signal(); },
    failed,
  };
}
