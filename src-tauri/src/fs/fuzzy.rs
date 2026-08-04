//! Fuzzy matching, modelled on fzy (the algorithm fzf's default mode is
//! built on).
//!
//! This is the single place where "does this name match what the user
//! typed, and how well" is decided, so the filter box and the recursive
//! finder always rank things the same way.
//!
//! A match is a subsequence, but not all subsequences are equal: a run
//! of consecutive characters, or a character that starts a word, is what
//! the user almost always meant. The optimal alignment is found with a
//! small dynamic program rather than by taking the leftmost match, so
//! `mkdir` scores higher against `make_dir` than against `my_kind_of_dir`.
//!
//! A term containing `*` or `?` escapes fuzzy matching entirely and is
//! treated as a shell-style wildcard anchored to the whole candidate, so
//! `*.txt` means what everyone expects it to mean.

/// Unreachable score used as "no match here"; halved so adding a gap
/// penalty cannot overflow.
const SCORE_MIN: i32 = i32::MIN / 2;

const GAP_LEADING: i32 = -5;
const GAP_TRAILING: i32 = -5;
const GAP_INNER: i32 = -10;
/// A consecutive run is the strongest signal there is, so it has to
/// outrank every boundary bonus below; the ratios are fzy's, scaled up
/// to keep the whole table in integers.
const MATCH_CONSECUTIVE: i32 = 1000;
const BONUS_SLASH: i32 = 900;
const BONUS_WORD: i32 = 800;
const BONUS_CAPITAL: i32 = 700;
const BONUS_DOT: i32 = 600;

/// Beyond these sizes the dynamic program costs more than the ranking is
/// worth; such candidates fall back to a plain subsequence scan.
const MAX_HAYSTACK: usize = 1024;
const MAX_NEEDLE: usize = 64;

/// One whitespace-separated term. As in fzf, every term must match, which
/// lets `src rs` mean "somewhere under src, ending in rs".
struct Term {
    chars: Vec<char>,
    /// Smart case, as in fd and fzf: a lowercase term ignores case, a term
    /// with any uppercase is taken literally.
    case_sensitive: bool,
    /// A term containing `*` or `?` is a wildcard pattern anchored to the
    /// whole candidate rather than a fuzzy subsequence, because someone
    /// typing `*.txt` means the shell glob, not "these six characters,
    /// scattered". Without this, `*` matches nothing at all: no filename
    /// contains one.
    glob: bool,
    /// The term's non-wildcard characters, used as a cheap necessary
    /// condition before running the matcher.
    literals: Vec<char>,
}

impl Term {
    fn new(text: &str) -> Self {
        let chars: Vec<char> = text.chars().collect();
        let glob = chars.iter().any(|&c| c == '*' || c == '?');
        Term {
            case_sensitive: chars.iter().any(|c| c.is_uppercase()),
            literals: if glob {
                chars.iter().copied().filter(|&c| c != '*' && c != '?').collect()
            } else {
                Vec::new()
            },
            glob,
            chars,
        }
    }

    /// Characters that must appear, in order, for the term to have any
    /// chance of matching.
    fn required(&self) -> &[char] {
        if self.glob {
            &self.literals
        } else {
            &self.chars
        }
    }
}

pub struct Query {
    terms: Vec<Term>,
}

impl Query {
    pub fn new(input: &str) -> Self {
        let terms = input
            .split_whitespace()
            .map(Term::new)
            .filter(|t| !t.chars.is_empty())
            .collect();
        Query { terms }
    }

    pub fn is_empty(&self) -> bool {
        self.terms.is_empty()
    }

    /// Cheap subsequence test used to reject the vast majority of
    /// candidates before paying for the dynamic program.
    pub fn is_candidate(&self, haystack: &str) -> bool {
        let chars: Vec<char> = haystack.chars().collect();
        self.terms.iter().all(|t| is_subsequence(t.required(), t.case_sensitive, &chars))
    }

    /// Score `haystack`, or `None` when it does not match every term.
    /// Higher is better. Positions are char indices, for highlighting.
    pub fn score(&self, haystack: &str) -> Option<(i32, Vec<usize>)> {
        if self.terms.is_empty() {
            return Some((0, Vec::new()));
        }
        let chars: Vec<char> = haystack.chars().collect();
        let bonus = bonus_table(&chars);

        let mut total = 0i32;
        let mut positions = Vec::new();
        for term in &self.terms {
            let (score, pos) = if term.glob {
                score_glob(term, &chars, &bonus)?
            } else {
                score_term(term, &chars, &bonus)?
            };
            total = total.saturating_add(score);
            positions.extend(pos);
        }
        positions.sort_unstable();
        positions.dedup();
        Some((total, positions))
    }
}

fn eq(a: char, b: char, case_sensitive: bool) -> bool {
    if case_sensitive {
        a == b
    } else {
        a.eq_ignore_ascii_case(&b)
            || (!a.is_ascii() && a.to_lowercase().eq(b.to_lowercase()))
    }
}

fn is_subsequence(needle: &[char], case_sensitive: bool, haystack: &[char]) -> bool {
    let mut it = haystack.iter();
    needle
        .iter()
        .all(|&n| it.any(|&h| eq(n, h, case_sensitive)))
}

/// Match a wildcard term against the whole candidate, `*` standing for any
/// run of characters (path separators included, so `*.txt` finds a nested
/// file in a recursive search) and `?` for exactly one.
///
/// Iterative with a single backtrack point, so a pattern like `*a*b*` costs
/// no more than the greedy scan it looks like; the returned positions are
/// the characters the pattern pinned down, for highlighting.
fn glob_match(term: &Term, haystack: &[char]) -> Option<Vec<usize>> {
    let pattern = &term.chars;
    let (mut p, mut h) = (0usize, 0usize);
    // Where to resume if the current `*` turns out to have consumed too
    // little, together with how much of the match was recorded by then.
    let mut star: Option<(usize, usize, usize)> = None;
    let mut positions: Vec<usize> = Vec::with_capacity(term.literals.len());

    while h < haystack.len() {
        match pattern.get(p) {
            Some(&'*') => {
                star = Some((p, h, positions.len()));
                p += 1;
            }
            Some(&'?') => {
                positions.push(h);
                p += 1;
                h += 1;
            }
            Some(&c) if eq(c, haystack[h], term.case_sensitive) => {
                positions.push(h);
                p += 1;
                h += 1;
            }
            _ => {
                let (star_p, star_h, kept) = star?;
                positions.truncate(kept);
                p = star_p + 1;
                h = star_h + 1;
                star = Some((star_p, h, kept));
            }
        }
    }

    // Only trailing stars may still be unconsumed.
    while pattern.get(p) == Some(&'*') {
        p += 1;
    }
    (p == pattern.len()).then_some(positions)
}

/// Rank a wildcard match by where it landed, not by how much it skipped:
/// the gaps a `*` covers are what the user asked for, so they cost
/// nothing, while a match on a word or extension boundary still wins.
/// Equally placed matches tie and are broken by path length upstream.
fn score_glob(term: &Term, haystack: &[char], bonus: &[i32]) -> Option<(i32, Vec<usize>)> {
    let positions = glob_match(term, haystack)?;
    let mut score = 0i32;
    let mut prev: Option<usize> = None;
    for &p in &positions {
        score = score.saturating_add(bonus[p]);
        if prev.is_some_and(|q| q + 1 == p) {
            score = score.saturating_add(MATCH_CONSECUTIVE);
        }
        prev = Some(p);
    }
    Some((score, positions))
}

/// Per-position bonus, derived from the *preceding* character: the start
/// of a path segment or a word is where the eye expects a match to begin.
fn bonus_table(haystack: &[char]) -> Vec<i32> {
    let mut table = Vec::with_capacity(haystack.len());
    let mut prev = '/';
    for &c in haystack {
        table.push(if c.is_alphanumeric() {
            match prev {
                '/' | '\\' => BONUS_SLASH,
                '-' | '_' | ' ' => BONUS_WORD,
                '.' => BONUS_DOT,
                p if p.is_lowercase() && c.is_uppercase() => BONUS_CAPITAL,
                _ => 0,
            }
        } else {
            0
        });
        prev = c;
    }
    table
}

/// Fallback for pathological inputs: keep the match, drop the ranking.
fn leftmost_match(term: &Term, haystack: &[char]) -> Option<(i32, Vec<usize>)> {
    let mut positions = Vec::with_capacity(term.chars.len());
    let mut j = 0usize;
    for &n in &term.chars {
        loop {
            let &h = haystack.get(j)?;
            j += 1;
            if eq(n, h, term.case_sensitive) {
                positions.push(j - 1);
                break;
            }
        }
    }
    Some((0, positions))
}

fn score_term(term: &Term, haystack: &[char], bonus: &[i32]) -> Option<(i32, Vec<usize>)> {
    let n = term.chars.len();
    let m = haystack.len();
    if n == 0 || m == 0 || n > m {
        return None;
    }
    if !is_subsequence(&term.chars, term.case_sensitive, haystack) {
        return None;
    }
    // An exact-length match is the best possible; skip the DP.
    if n == m {
        return Some((i32::MAX / 4, (0..m).collect()));
    }
    if m > MAX_HAYSTACK || n > MAX_NEEDLE {
        return leftmost_match(term, haystack);
    }

    // `best[i][j]`: best score for the first i+1 needle chars over the
    // first j+1 haystack chars. `end[i][j]`: same, but forced to end on a
    // match at j, which is what makes a consecutive run detectable.
    let mut best = vec![SCORE_MIN; n * m];
    let mut end = vec![SCORE_MIN; n * m];

    for i in 0..n {
        let gap = if i == n - 1 { GAP_TRAILING } else { GAP_INNER };
        let mut running = SCORE_MIN;
        for j in 0..m {
            let idx = i * m + j;
            if eq(term.chars[i], haystack[j], term.case_sensitive) {
                let score = if i == 0 {
                    // Leading gaps are cheap: matching late in a long name
                    // is normal, so penalise it only gently.
                    (j as i32) * GAP_LEADING + bonus[j]
                } else if j > 0 {
                    let prev = (i - 1) * m + (j - 1);
                    (best[prev] + bonus[j]).max(end[prev] + MATCH_CONSECUTIVE)
                } else {
                    SCORE_MIN
                };
                end[idx] = score;
                running = score.max(running.saturating_add(gap));
            } else {
                running = running.saturating_add(gap);
            }
            best[idx] = running;
        }
    }

    let total = best[(n - 1) * m + (m - 1)];
    Some((total, backtrack(&best, &end, n, m)))
}

/// Walk the table backwards to recover which characters were matched.
fn backtrack(best: &[i32], end: &[i32], n: usize, m: usize) -> Vec<usize> {
    let mut positions = vec![0usize; n];
    let mut i = n;
    let mut consecutive = false;

    for j in (0..m).rev() {
        if i == 0 {
            break;
        }
        let idx = (i - 1) * m + j;
        if end[idx] == SCORE_MIN {
            continue;
        }
        // Either this is where the best path ends, or the character to the
        // right was only reachable as part of a consecutive run.
        if !consecutive && end[idx] != best[idx] {
            continue;
        }
        consecutive = i > 1
            && j > 0
            && end[idx] == end[(i - 2) * m + (j - 1)] + MATCH_CONSECUTIVE;
        i -= 1;
        positions[i] = j;
    }
    positions
}

#[cfg(test)]
mod tests {
    use super::*;

    fn score(query: &str, text: &str) -> Option<i32> {
        Query::new(query).score(text).map(|(s, _)| s)
    }

    fn positions(query: &str, text: &str) -> Vec<usize> {
        Query::new(query).score(text).unwrap().1
    }

    #[test]
    fn a_non_subsequence_does_not_match() {
        assert!(score("xyz", "readme.txt").is_none());
        assert!(score("zyx", "xyz.txt").is_none());
    }

    #[test]
    fn an_empty_query_matches_everything() {
        assert_eq!(score("", "anything"), Some(0));
        assert!(Query::new("   ").is_empty());
    }

    #[test]
    fn consecutive_characters_beat_a_scattered_match() {
        let tight = score("cargo", "cargo.toml").unwrap();
        let loose = score("cargo", "c_a_r_g_o.toml").unwrap();
        assert!(tight > loose, "{} should beat {}", tight, loose);
    }

    #[test]
    fn a_word_boundary_beats_a_mid_word_match() {
        let boundary = score("fb", "file_browser.vue").unwrap();
        let inner = score("fb", "affable.vue").unwrap();
        assert!(boundary > inner, "{} should beat {}", boundary, inner);
    }

    #[test]
    fn matching_the_start_of_a_path_segment_is_favoured() {
        let seg = score("se", "src\\search.rs").unwrap();
        let mid = score("se", "src\\unused.rs").unwrap();
        assert!(seg > mid, "{} should beat {}", seg, mid);
    }

    #[test]
    fn lowercase_queries_ignore_case_but_uppercase_ones_do_not() {
        assert!(score("readme", "README.md").is_some());
        assert!(score("README", "README.md").is_some());
        assert!(score("README", "readme.md").is_none());
    }

    #[test]
    fn every_space_separated_term_must_match() {
        assert!(score("src rs", "src\\fs\\search.rs").is_some());
        assert!(score("src zz", "src\\fs\\search.rs").is_none());
        assert!(score("docs rs", "src\\fs\\search.rs").is_none());
    }

    #[test]
    fn a_term_matches_as_a_subsequence_not_a_substring() {
        // "rs" is spread across "sea(r)ch.vue" — fuzzy, so it still matches.
        assert!(score("rs", "search.vue").is_none());
        assert!(score("sr", "search.vue").is_some());
    }

    #[test]
    fn positions_point_at_the_matched_characters() {
        // c-a-r-g-o: 'c' at 0, 'g' at 3, 'o' at 4.
        assert_eq!(positions("cgo", "cargo"), vec![0, 3, 4]);
        assert_eq!(positions("car", "cargo"), vec![0, 1, 2]);
    }

    #[test]
    fn positions_prefer_the_consecutive_run() {
        // Both "ab" runs are reachable; the adjacent one is the better match.
        assert_eq!(positions("ab", "a-ab"), vec![2, 3]);
    }

    #[test]
    fn positions_from_several_terms_are_merged_and_sorted() {
        let pos = positions("ab yz", "abcxyz");
        assert_eq!(pos, vec![0, 1, 4, 5]);
    }

    #[test]
    fn an_exact_name_outranks_a_longer_one() {
        let exact = score("main.rs", "main.rs").unwrap();
        let longer = score("main.rs", "domain.rs.bak").unwrap();
        assert!(exact > longer);
    }

    #[test]
    fn non_ascii_names_match_case_insensitively() {
        assert!(score("ä", "Äpfel.txt").is_some());
        assert!(score("文件", "测试文件.txt").is_some());
    }

    #[test]
    fn the_candidate_filter_agrees_with_the_scorer() {
        let q = Query::new("cgo");
        assert!(q.is_candidate("cargo.toml"));
        assert!(!q.is_candidate("readme.md"));
    }

    #[test]
    fn a_very_long_name_still_matches_without_the_dynamic_program() {
        let long = "a".repeat(MAX_HAYSTACK + 10) + "zz";
        assert!(score("zz", &long).is_some());
    }

    #[test]
    fn a_star_matches_any_run_of_characters() {
        assert!(score("*.txt", "readme.txt").is_some());
        assert!(score("*.txt", "readme.md").is_none());
        assert!(score("*.txt", ".txt").is_some());
        assert!(score("report*", "report-2024.pdf").is_some());
        assert!(score("report*", "annual-report.pdf").is_none());
        assert!(score("*report*", "annual-report.pdf").is_some());
    }

    #[test]
    fn a_wildcard_is_anchored_where_a_fuzzy_term_is_not() {
        // Without wildcards the term is still a plain fuzzy subsequence.
        assert!(score("txt", "a-t-x-t.md").is_some());
        // With one, the pattern must describe the whole name.
        assert!(score("*.txt", "notes.txt.bak").is_none());
    }

    #[test]
    fn a_question_mark_matches_exactly_one_character() {
        assert!(score("log?.txt", "log1.txt").is_some());
        assert!(score("log?.txt", "log.txt").is_none());
        assert!(score("log??.txt", "log12.txt").is_some());
    }

    #[test]
    fn a_lone_star_matches_everything() {
        assert!(score("*", "anything.txt").is_some());
        assert!(score("*", "").is_some());
    }

    #[test]
    fn several_stars_backtrack_correctly() {
        assert!(score("*a*b*c*", "xxaxxbxxcxx").is_some());
        assert!(score("*a*b*c*", "xxaxxcxxbxx").is_none());
        // The greedy first attempt has to give ground for this to match.
        assert!(score("*ab*", "aaab").is_some());
    }

    #[test]
    fn a_wildcard_crosses_path_separators() {
        assert!(score("*.rs", "src\\fs\\search.rs").is_some());
        assert!(score("src\\*.rs", "src\\fs\\search.rs").is_some());
    }

    #[test]
    fn wildcards_honour_smart_case_like_every_other_term() {
        assert!(score("*.txt", "README.TXT").is_some());
        assert!(score("*.TXT", "readme.txt").is_none());
        assert!(score("*.TXT", "README.TXT").is_some());
    }

    #[test]
    fn a_wildcard_can_be_combined_with_a_fuzzy_term() {
        assert!(score("src *.rs", "src\\fs\\search.rs").is_some());
        assert!(score("docs *.rs", "src\\fs\\search.rs").is_none());
    }

    #[test]
    fn wildcard_positions_cover_the_literal_characters_only() {
        // "*.txt" pins the last four characters of "a.txt".
        assert_eq!(positions("*.txt", "a.txt"), vec![1, 2, 3, 4]);
        assert_eq!(positions("log?.txt", "log1.txt"), vec![0, 1, 2, 3, 4, 5, 6, 7]);
    }

    #[test]
    fn a_wildcard_match_on_a_boundary_outranks_one_in_the_middle() {
        let boundary = score("*report*", "annual_report.pdf").unwrap();
        let inner = score("*report*", "xxreportxx.pdf").unwrap();
        assert!(boundary > inner, "{} should beat {}", boundary, inner);
    }

    #[test]
    fn the_candidate_filter_never_rejects_a_wildcard_match() {
        let q = Query::new("*.txt");
        assert!(q.is_candidate("readme.txt"));
        assert!(q.is_candidate("a.txt"));
        assert!(!q.is_candidate("readme.md"));
    }
}
