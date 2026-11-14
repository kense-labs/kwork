//! Harness-compatible matching for user-facing list filters.

/// One prepared name or other readable identifier.
#[derive(Clone, Debug)]
struct SearchName {
    stripped: String,
    folded: String,
}

/// Match and rank one name using the current Harness search tiers.
///
/// Every whitespace-separated term must match. A fuzzy subsequence must begin
/// at a word boundary, unlike a substring. Ties retain the caller's order.
pub(crate) fn name_rank(query: &str, label: &str) -> Option<(bool, i64)> {
    let label = SearchName {
        stripped: strip(label),
        folded: fold(label),
    };
    let mut prefix = true;
    let mut total = 0_i64;
    for term in query.split_whitespace() {
        let term = fold(term);
        if term.is_empty() {
            continue;
        }
        if label.folded.starts_with(&term) {
            total += 40_000;
        } else if let Some(position) = label.folded.find(&term) {
            prefix = false;
            total += 30_000 - position.min(10_000) as i64;
        } else {
            prefix = false;
            total += fuzzy_term(&label, &term)?;
        }
    }
    Some((prefix, total))
}

fn fold(value: &str) -> String {
    strip(value).to_lowercase()
}

/// NFKD removes combining accents before the final width normalization. The
/// explicit stroke mappings mirror TextSearchIndex for letters NFKD keeps whole.
fn strip(value: &str) -> String {
    use unicode_normalization::{UnicodeNormalization, char::is_combining_mark};
    value
        .nfkd()
        .filter(|character| !is_combining_mark(*character))
        .map(|character| match character {
            'ø' => 'o',
            'Ø' => 'O',
            'ł' => 'l',
            'Ł' => 'L',
            'đ' | 'ð' => 'd',
            'Đ' | 'Ð' => 'D',
            'ı' => 'i',
            'ħ' => 'h',
            'Ħ' => 'H',
            'ŧ' => 't',
            'Ŧ' => 'T',
            other => other,
        })
        .collect::<String>()
        .nfkc()
        .collect()
}

/// Dynamic programming over Unicode scalar values. Word starts include
/// separators, camel-case humps, and every character of unspaced scripts.
fn fuzzy_term(label: &SearchName, term: &str) -> Option<i64> {
    if !is_subsequence(&label.folded, term) {
        return None;
    }
    let mut best = None;
    for (start, character) in label.stripped.char_indices() {
        if !word_start(&label.stripped, start) {
            continue;
        }
        if !character
            .to_lowercase()
            .eq(term.chars().next()?.to_lowercase())
        {
            continue;
        }
        if let Some(score) = fuzzy_alignment(&label.stripped[start..], term) {
            best = Some(best.map_or(score, |best: i64| best.max(score)));
        }
    }
    best.map(|score| 20_000 + score)
}

fn fuzzy_alignment(label: &str, term: &str) -> Option<i64> {
    let name = label.chars().collect::<Vec<_>>();
    let query = term.chars().collect::<Vec<_>>();
    let boundary = |index: usize| {
        if index == 0 {
            return 8;
        }
        let character = name[index];
        let previous = name[index - 1];
        if !previous.is_alphanumeric()
            || is_unspaced(character)
            || (character.is_uppercase() && previous.is_lowercase())
        {
            8
        } else {
            0
        }
    };
    let missing = i64::MIN / 2;
    let first = *query.first()?;
    let mut scores = name
        .iter()
        .enumerate()
        .map(|(index, character)| {
            if character.to_lowercase().eq(first.to_lowercase()) {
                1 + boundary(index) - index as i64
            } else {
                missing
            }
        })
        .collect::<Vec<_>>();
    let mut next = vec![missing; name.len()];
    for unit in query.iter().skip(1) {
        let mut adjacent = missing;
        let mut two_back = missing;
        let mut gapped = missing;
        for (index, previous) in scores.iter().copied().enumerate() {
            if two_back != missing {
                gapped = gapped.max(two_back + index as i64 - 2);
            }
            next[index] = if name[index].to_lowercase().eq(unit.to_lowercase()) {
                let bonus = 1 + boundary(index);
                let joined = if adjacent == missing {
                    missing
                } else {
                    adjacent + bonus + 4
                };
                let skipped = if gapped == missing {
                    missing
                } else {
                    gapped + bonus + 1 - index as i64
                };
                joined.max(skipped)
            } else {
                missing
            };
            two_back = adjacent;
            adjacent = previous;
        }
        std::mem::swap(&mut scores, &mut next);
    }
    let score = scores.into_iter().max()?;
    (score != missing).then_some(20_000 + score)
}

fn word_start(text: &str, index: usize) -> bool {
    if index == 0 {
        return true;
    }
    let character = text[index..].chars().next().unwrap();
    let previous = text[..index].chars().next_back().unwrap();
    !previous.is_alphanumeric()
        || is_unspaced(character)
        || (character.is_uppercase() && previous.is_lowercase())
}

fn is_subsequence(text: &str, term: &str) -> bool {
    let mut characters = text.chars();
    term.chars()
        .all(|wanted| characters.by_ref().any(|character| character == wanted))
}

fn is_unspaced(character: char) -> bool {
    matches!(
        character as u32,
        0x3400..=0x4dbf
            | 0x4e00..=0x9fff
            | 0xf900..=0xfaff
            | 0x20000..=0x323af
            | 0x3040..=0x30ff
            | 0x31f0..=0x31ff
            | 0xac00..=0xd7af
            | 0x1100..=0x11ff
            | 0xa960..=0xa97f
    )
}

#[cfg(test)]
mod tests {
    use super::name_rank;

    #[test]
    fn every_term_matches_and_word_start_fuzzy_is_preserved() {
        assert!(name_rank("cpt", "compact").is_some());
        assert!(name_rank("com pact", "compact").is_some());
        assert!(name_rank("fab", "Refactor build").is_none());
        assert!(name_rank("ref build", "Refactor build").is_some());
    }

    #[test]
    fn width_accents_and_camel_case_are_normalized() {
        assert!(name_rank("resume", "Re\u{301}sume\u{301}").is_some());
        assert!(name_rank("ｃｏｍｐａｃｔ", "compact").is_some());
        assert!(name_rank("ostrom", "Østrøm").is_some());
        assert!(name_rank("lks", "Łukasz").is_some());
        assert!(name_rank("mh", "ModelHub").is_some());
    }

    #[test]
    fn prefix_and_substring_outrank_fuzzy_matches() {
        let mut rows = ["recompile", "compact", "recompute", "compile"];
        rows.sort_by_key(|label| std::cmp::Reverse(name_rank("comp", label)));
        assert_eq!(rows, ["compact", "compile", "recompile", "recompute"]);
    }
}
