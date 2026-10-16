const entries = [];

export function logError(what, e) {
  console.error(what, e);
  entries.push({ at: new Date().toISOString(), what, e });
}

export const hasLog = () => entries.length > 0;

const describe = (e) => {
  if (!(e instanceof Error) && !(globalThis.DOMException && e instanceof DOMException)) return String(e);
  const head = `${e.name}: ${e.message}`;
  const stack = typeof e.stack === 'string' ? e.stack.split('\n').filter((l) => l.trim() && l !== head) : [];
  return [head, ...stack].join('\n');
};

export const logText = (agent) =>
  [agent, ...entries.map(({ at, what, e }) => `${at} ${what}: ${describe(e)}`)].join('\n');
