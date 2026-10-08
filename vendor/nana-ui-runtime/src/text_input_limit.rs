use crate::TextInputState;

pub(crate) fn accepts_value(previous: &str, next: &str, limit: Option<usize>) -> bool {
    limit.is_none_or(|limit| {
        next.encode_utf16().count() <= limit.max(previous.encode_utf16().count())
    })
}

pub(crate) fn replace(
    state: &mut TextInputState,
    text: &str,
    limit: Option<usize>,
    primary_only: bool,
) -> bool {
    let Some(limit) = limit else {
        return if primary_only {
            state.replace_primary_selection(text)
        } else {
            state.replace_selection(text)
        };
    };
    let mut next = state.clone();
    next.normalize_selections();
    let selections = if primary_only {
        vec![next.selection]
    } else {
        next.selections().into_owned()
    };
    if selections
        .iter()
        .any(|selection| !selection.is_valid_for(&next.value))
    {
        return false;
    }
    let current = next.value.encode_utf16().count();
    let removed: usize = selections
        .iter()
        .map(|selection| next.value[selection.ordered()].encode_utf16().count())
        .sum();
    let budget = limit
        .max(current)
        .saturating_sub(current.saturating_sub(removed))
        / selections.len().max(1);
    let mut units = 0;
    let end = text
        .char_indices()
        .take_while(|(_, ch)| {
            units += ch.len_utf16();
            units <= budget
        })
        .map(|(index, ch)| index + ch.len_utf8())
        .last()
        .unwrap_or(0);
    if !text.is_empty() && end == 0 {
        return false;
    }
    let changed = if primary_only {
        next.replace_primary_selection(&text[..end])
    } else {
        next.replace_selection(&text[..end])
    };
    if !changed || next == *state {
        return false;
    }
    *state = next;
    true
}
