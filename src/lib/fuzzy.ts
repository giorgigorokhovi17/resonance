const WORD_SEP = /[\s\-_/:.([\]]/;

/**
 * Simple subsequence fuzzy match with bonuses for word starts, consecutive runs,
 * and prefix/substring hits. Returns null when `query` is not a subsequence of `text`.
 */
export function fuzzyScore(query: string, text: string): number | null {
  const q = query.trim().toLowerCase();
  if (!q) return 0;
  const t = text.toLowerCase();

  let qi = 0;
  let score = 0;
  let prev = -2;

  for (let ti = 0; ti < t.length && qi < q.length; ti++) {
    if (t[ti] !== q[qi]) continue;
    score += 1;
    if (ti === 0 || WORD_SEP.test(t[ti - 1])) score += 10;
    if (ti === prev + 1) score += 6;
    prev = ti;
    qi++;
  }

  if (qi < q.length) return null;
  if (t.startsWith(q)) score += 40;
  else if (t.includes(q)) score += 20;
  return score - t.length * 0.05;
}
