/**
 * Name matching for the one place Rust cannot help: listings served from
 * inside an archive, which the recursive finder cannot walk.
 *
 * It deliberately mirrors the backend's rules rather than inventing its
 * own, so a query typed in an archive behaves like the same query typed
 * in a folder: every whitespace-separated term must match, a term
 * containing `*` or `?` is an anchored wildcard, and case is ignored
 * unless the term contains an uppercase letter.
 */

const WILDCARD = /[*?]/

/** Escape everything a regex treats specially, then re-expand the globs. */
function toRegExp(term: string): RegExp {
  const body = term.replace(/[.+^${}()|[\]\\]/g, '\\$&').replace(/[*?]/g, (c) => (c === '*' ? '.*' : '.'))
  return new RegExp(`^${body}$`, /[A-Z]/.test(term) ? '' : 'i')
}

function matchesTerm(name: string, term: string): boolean {
  if (WILDCARD.test(term)) return toRegExp(term).test(name)
  // A plain term stays a substring match: that is what a filter box is for.
  return /[A-Z]/.test(term) ? name.includes(term) : name.toLowerCase().includes(term.toLowerCase())
}

/** Whether `name` satisfies every term in `query`; an empty query matches. */
export function matchesName(name: string, query: string): boolean {
  const terms = query.split(/\s+/).filter(Boolean)
  return terms.every((term) => matchesTerm(name, term))
}
