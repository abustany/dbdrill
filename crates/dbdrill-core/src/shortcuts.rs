use std::collections::HashSet;

fn is_consonnant(c: char) -> bool {
    !matches!(c, 'a' | 'e' | 'i' | 'o' | 'u')
}

/// Assigns a unique shortcut character to each of the given strings.
///
/// Returns, for each input string, the index and value of the character to use
/// as a shortcut, or `None` if no unused character could be found.
pub fn assign_shortcuts<'a>(strs: impl IntoIterator<Item = &'a str>) -> Vec<Option<(usize, char)>> {
    let mut assigned: HashSet<char> = HashSet::new();
    let mut res: Vec<Option<(usize, char)>> = Vec::new();

    'outer: for s in strs {
        let mut is_prev_alphabetic = false;
        let word_starts = s.chars().enumerate().filter(|(_, c)| {
            let is_alphabetic = c.is_alphabetic();
            let is_word_start = is_alphabetic && !is_prev_alphabetic;
            is_prev_alphabetic = is_alphabetic;
            is_word_start
        });
        let consonnants = s
            .chars()
            .enumerate()
            .filter(|(_, c)| c.is_alphabetic() && is_consonnant(*c));
        let all_alphas = s.chars().enumerate().filter(|(_, c)| c.is_alphabetic());

        for (idx, c) in word_starts.chain(consonnants).chain(all_alphas) {
            let c = c.to_lowercase().next().expect("error lowercasing");
            if assigned.contains(&c) {
                continue;
            }

            assigned.insert(c);
            res.push(Some((idx, c)));
            continue 'outer;
        }

        res.push(None);
    }

    res
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_assign_shortcuts() {
        let items: Vec<&str> = vec![
            "Case",
            "Case list",
            "Case list item",
            "Presentation",
            "Slide",
            "Space",
            "User",
        ];
        assert_eq!(
            assign_shortcuts(items),
            vec![
                Some((0, 'c')),
                Some((5, 'l')),
                Some((10, 'i')),
                Some((0, 'p')),
                Some((0, 's')),
                Some((2, 'a')),
                Some((0, 'u')),
            ]
        );
    }
}
