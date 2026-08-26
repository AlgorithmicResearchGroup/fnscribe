use serde::{Deserialize, Serialize};
use std::cmp::Reverse;
use std::ops::Range;

pub const MAX_DICTIONARY_ENTRIES: usize = 500;
pub const MAX_DICTIONARY_PHRASE_CHARS: usize = 80;
const MAX_VOCABULARY_PROMPT_BYTES: usize = 900;
const MAX_VOCABULARY_PROMPT_TERMS: usize = 64;
const MAX_REWRITE_PASSES: usize = 128;

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct DictionaryEntry {
    pub written_form: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spoken_form: Option<String>,
}

pub fn prepare_dictionary_entry(
    written_form: &str,
    spoken_form: Option<&str>,
) -> Result<DictionaryEntry, String> {
    let written_form = normalize_phrase(written_form);
    validate_phrase(&written_form, "Write as")?;

    let spoken_form = spoken_form
        .map(normalize_phrase)
        .filter(|value| !value.is_empty());
    if let Some(spoken_form) = &spoken_form {
        validate_phrase(spoken_form, "Common mishearing")?;
    }
    let spoken_form =
        spoken_form.filter(|value| !value.to_lowercase().eq(&written_form.to_lowercase()));

    Ok(DictionaryEntry {
        written_form,
        spoken_form,
    })
}

pub fn validate_dictionary(entries: &[DictionaryEntry]) -> Result<(), String> {
    if entries.len() > MAX_DICTIONARY_ENTRIES {
        return Err(format!(
            "The personal dictionary supports up to {MAX_DICTIONARY_ENTRIES} entries."
        ));
    }

    let mut triggers = Vec::with_capacity(entries.len() * 2);
    for entry in entries {
        let prepared = prepare_dictionary_entry(&entry.written_form, entry.spoken_form.as_deref())?;
        for trigger in [
            Some(prepared.written_form.as_str()),
            prepared.spoken_form.as_deref(),
        ]
        .into_iter()
        .flatten()
        {
            let normalized = trigger.to_lowercase();
            if triggers
                .iter()
                .any(|existing: &String| existing == &normalized)
            {
                return Err(format!(
                    "“{trigger}” is already used by another dictionary entry."
                ));
            }
            triggers.push(normalized);
        }
    }
    Ok(())
}

pub fn vocabulary_prompt(entries: &[DictionaryEntry]) -> Option<String> {
    const PREFIX: &str = "Vocabulary: ";
    let mut selected = Vec::new();
    let mut selected_bytes = PREFIX.len() + 1;

    // Recent entries are the most likely additions the user is actively
    // testing, so select them first when the local model's prompt is full.
    // They are emitted last because whisper.cpp preserves the tail if it must
    // trim prompt context.
    for entry in entries.iter().rev() {
        if selected.len() >= MAX_VOCABULARY_PROMPT_TERMS {
            break;
        }
        let term = normalize_phrase(&entry.written_form);
        if term.is_empty() || term.chars().any(char::is_control) {
            continue;
        }
        let separator_bytes = usize::from(!selected.is_empty()) * 2;
        if selected_bytes + separator_bytes + term.len() > MAX_VOCABULARY_PROMPT_BYTES {
            break;
        }
        selected_bytes += separator_bytes + term.len();
        selected.push(term);
    }

    if selected.is_empty() {
        return None;
    }
    selected.reverse();
    Some(format!("{PREFIX}{}.", selected.join(", ")))
}

pub fn process_transcript(
    transcript: &str,
    dictionary: &[DictionaryEntry],
    smart_cleanup: bool,
) -> String {
    let cleaned = if smart_cleanup {
        let without_backtracks = apply_backtracks(transcript);
        let without_fillers = remove_fillers(&without_backtracks);
        let with_lists = format_spoken_lists(&without_fillers);
        let with_commands = replace_spoken_commands(&with_lists);
        normalize_whitespace(&with_commands)
    } else {
        transcript.trim().to_string()
    };

    replace_dictionary_terms(&cleaned, dictionary)
        .trim()
        .to_string()
}

fn normalize_phrase(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn validate_phrase(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty() {
        return Err(format!("{label} cannot be empty."));
    }
    if value.chars().count() > MAX_DICTIONARY_PHRASE_CHARS {
        return Err(format!(
            "{label} must be {MAX_DICTIONARY_PHRASE_CHARS} characters or fewer."
        ));
    }
    if value.chars().any(char::is_control) {
        return Err(format!("{label} cannot contain control characters."));
    }
    Ok(())
}

struct ReplacementCandidate<'a> {
    trigger: &'a str,
    normalized: String,
    replacement: &'a str,
    character_count: usize,
}

fn replace_dictionary_terms(text: &str, entries: &[DictionaryEntry]) -> String {
    let mut candidates = entries
        .iter()
        .flat_map(|entry| {
            [
                Some(entry.written_form.as_str()),
                entry.spoken_form.as_deref(),
            ]
            .into_iter()
            .flatten()
            .map(move |trigger| ReplacementCandidate {
                trigger,
                normalized: trigger.to_lowercase(),
                replacement: &entry.written_form,
                character_count: trigger.chars().count(),
            })
        })
        .collect::<Vec<_>>();
    candidates.sort_by_key(|candidate| Reverse(candidate.character_count));

    if candidates.is_empty() {
        return text.to_string();
    }

    let mut output = String::with_capacity(text.len());
    let mut cursor = 0;
    while cursor < text.len() {
        let matched = candidates.iter().find_map(|candidate| {
            match_phrase_at(text, cursor, candidate.trigger, &candidate.normalized)
                .map(|end| (end, candidate.replacement))
        });
        if let Some((end, replacement)) = matched {
            output.push_str(replacement);
            cursor = end;
        } else {
            let character = text[cursor..]
                .chars()
                .next()
                .expect("cursor remains on a character boundary");
            output.push(character);
            cursor += character.len_utf8();
        }
    }
    output
}

fn apply_backtracks(text: &str) -> String {
    let scratched = apply_scratch_that(text);
    apply_actual_corrections(&scratched)
}

fn apply_scratch_that(text: &str) -> String {
    let mut current = text.to_string();
    for _ in 0..MAX_REWRITE_PASSES {
        let Some(command) = find_phrase_from(&current, "scratch that", 0) else {
            break;
        };
        let clause_start = current[..command.start]
            .char_indices()
            .rev()
            .find(|(_, character)| is_hard_boundary(*character))
            .map_or(0, |(index, character)| index + character.len_utf8());
        let prefix = current[..clause_start].trim_end();
        let suffix = current[command.end..].trim_start_matches(|character: char| {
            character.is_whitespace() || matches!(character, ',' | ';' | ':' | '—' | '–')
        });
        let removed_clause = current[clause_start..command.start].trim_start();
        let suffix = if starts_with_uppercase(removed_clause) {
            capitalize_first(suffix)
        } else {
            suffix.to_string()
        };
        current = join_fragments(prefix, &suffix);
    }
    current
}

fn apply_actual_corrections(text: &str) -> String {
    let mut current = text.to_string();
    let mut search_from = 0;
    for _ in 0..MAX_REWRITE_PASSES {
        let Some(command) = find_phrase_from(&current, "actually", search_from) else {
            break;
        };
        let before = current[..command.start].trim_end();
        let Some(before_without_marker) = strip_soft_correction_marker(before) else {
            search_from = command.end;
            continue;
        };
        let mut suffix = current[command.end..].trim_start_matches(|character: char| {
            character.is_whitespace() || matches!(character, ',' | ';' | ':' | '—' | '–')
        });
        let explicit = find_phrase_from(suffix, "make that", 0)
            .filter(|matched| matched.start == 0)
            .is_some_and(|matched| {
                suffix = suffix[matched.end..].trim_start_matches(|character: char| {
                    character.is_whitespace() || matches!(character, ',' | ';' | ':')
                });
                true
            });
        let correction_end = suffix
            .char_indices()
            .find(|(_, character)| is_hard_boundary(*character))
            .map_or(suffix.len(), |(index, _)| index);
        let correction_words = word_ranges(&suffix[..correction_end]);
        if correction_words.is_empty() || correction_words.len() > 4 {
            search_from = command.end;
            continue;
        }

        let previous_clause_start = before_without_marker
            .char_indices()
            .rev()
            .find(|(_, character)| is_hard_boundary(*character))
            .map_or(0, |(index, character)| index + character.len_utf8());
        let previous_words = word_ranges(&before_without_marker[previous_clause_start..]);
        if previous_words.len() < correction_words.len() {
            search_from = command.end;
            continue;
        }
        let previous_last_range = previous_words.last().expect("non-empty words");
        let previous_last = &before_without_marker[previous_clause_start + previous_last_range.start
            ..previous_clause_start + previous_last_range.end];
        let correction_first = &suffix[correction_words[0].clone()];
        if !explicit
            && !is_correction_value(previous_last)
            && !is_correction_value(correction_first)
        {
            search_from = command.end;
            continue;
        }

        let remove_word = previous_words.len() - correction_words.len();
        let remove_from = previous_clause_start + previous_words[remove_word].start;
        let prefix = before_without_marker[..remove_from].trim_end();
        let next_search_from = prefix.len();
        current = join_fragments(prefix, suffix);
        search_from = next_search_from;
    }
    current
}

fn strip_soft_correction_marker(value: &str) -> Option<&str> {
    let value = value.trim_end();
    if let Some(stripped) = value.strip_suffix("...") {
        return Some(stripped.trim_end());
    }
    value
        .strip_suffix([',', ';', '—', '–', '…'])
        .map(str::trim_end)
}

fn is_correction_value(value: &str) -> bool {
    let normalized = value
        .trim_matches(|character: char| !is_word_character(character))
        .to_lowercase();
    normalized
        .chars()
        .any(|character| character.is_ascii_digit())
        || matches!(
            normalized.as_str(),
            "zero"
                | "one"
                | "two"
                | "three"
                | "four"
                | "five"
                | "six"
                | "seven"
                | "eight"
                | "nine"
                | "ten"
                | "eleven"
                | "twelve"
                | "thirteen"
                | "fourteen"
                | "fifteen"
                | "sixteen"
                | "seventeen"
                | "eighteen"
                | "nineteen"
                | "twenty"
                | "monday"
                | "tuesday"
                | "wednesday"
                | "thursday"
                | "friday"
                | "saturday"
                | "sunday"
                | "january"
                | "february"
                | "march"
                | "april"
                | "may"
                | "june"
                | "july"
                | "august"
                | "september"
                | "october"
                | "november"
                | "december"
                | "today"
                | "tomorrow"
                | "yesterday"
                | "am"
                | "pm"
                | "yes"
                | "no"
        )
}

fn remove_fillers(text: &str) -> String {
    const FILLERS: &[&str] = &["umm", "uhh", "erm", "um", "uh"];

    let mut current = text.to_string();
    for _ in 0..MAX_REWRITE_PASSES {
        let Some(filler) = find_earliest_phrase(&current, FILLERS, 0) else {
            break;
        };
        let mut remove_start = filler.start;
        let mut remove_end = filler.end;
        let previous = previous_non_whitespace(&current, remove_start);
        let next = next_non_whitespace(&current, remove_end);

        if previous.is_some_and(|(_, character)| character == ',')
            && next.is_some_and(|(_, character)| character == ',')
        {
            remove_start = previous.expect("checked above").0;
            let (index, character) = next.expect("checked above");
            remove_end = index + character.len_utf8();
        } else if next.is_some_and(|(_, character)| character == ',') {
            let (index, character) = next.expect("checked above");
            remove_end = index + character.len_utf8();
        }

        while remove_start > 0 {
            let Some((index, character)) = current[..remove_start].char_indices().next_back()
            else {
                break;
            };
            if !character.is_whitespace() || character == '\n' {
                break;
            }
            remove_start = index;
        }
        while remove_end < current.len() {
            let Some(character) = current[remove_end..].chars().next() else {
                break;
            };
            if !character.is_whitespace() || character == '\n' {
                break;
            }
            remove_end += character.len_utf8();
        }

        let prefix = current[..remove_start].trim_end();
        let suffix = current[remove_end..].trim_start();
        current = join_fragments(prefix, suffix);
    }
    current
}

#[derive(Clone)]
struct NumberMarker {
    range: Range<usize>,
    number: u8,
}

fn format_spoken_lists(text: &str) -> String {
    if let Some(formatted) = format_numbered_list(text) {
        return formatted;
    }
    format_bullet_list(text).unwrap_or_else(|| text.to_string())
}

fn format_numbered_list(text: &str) -> Option<String> {
    const MARKERS: &[(&str, u8)] = &[
        ("number one", 1),
        ("number 1", 1),
        ("number two", 2),
        ("number 2", 2),
        ("number three", 3),
        ("number 3", 3),
        ("number four", 4),
        ("number 4", 4),
        ("number five", 5),
        ("number 5", 5),
        ("number six", 6),
        ("number 6", 6),
        ("number seven", 7),
        ("number 7", 7),
        ("number eight", 8),
        ("number 8", 8),
        ("number nine", 9),
        ("number 9", 9),
        ("number ten", 10),
        ("number 10", 10),
    ];

    let mut markers = Vec::new();
    let mut cursor = 0;
    while cursor < text.len() {
        let matched = MARKERS
            .iter()
            .filter_map(|(phrase, number)| {
                match_phrase_at(text, cursor, phrase, phrase).map(|end| NumberMarker {
                    range: cursor..end,
                    number: *number,
                })
            })
            .max_by_key(|marker| marker.range.end - marker.range.start);
        if let Some(marker) = matched {
            cursor = marker.range.end;
            markers.push(marker);
        } else {
            let character = text[cursor..].chars().next()?;
            cursor += character.len_utf8();
        }
    }

    for start in 0..markers.len() {
        if markers[start].number != 1 {
            continue;
        }
        let mut end = start + 1;
        while end < markers.len()
            && markers[end].number == markers[end - 1].number.saturating_add(1)
        {
            end += 1;
        }
        if end - start >= 2 {
            let run = &markers[start..end];
            return format_list(text, run, |marker| format!("{}. ", marker.number));
        }
    }
    None
}

fn format_bullet_list(text: &str) -> Option<String> {
    const MARKERS: &[&str] = &["bullet point", "bullet"];
    let mut ranges = Vec::new();
    let mut cursor = 0;
    while cursor < text.len() {
        let matched = MARKERS
            .iter()
            .filter_map(|phrase| {
                match_phrase_at(text, cursor, phrase, phrase).map(|end| cursor..end)
            })
            .max_by_key(|range| range.end - range.start);
        if let Some(range) = matched {
            cursor = range.end;
            ranges.push(range);
        } else {
            let character = text[cursor..].chars().next()?;
            cursor += character.len_utf8();
        }
    }
    if ranges.len() < 2 {
        return None;
    }
    let markers = ranges
        .into_iter()
        .map(|range| NumberMarker { range, number: 0 })
        .collect::<Vec<_>>();
    format_list(text, &markers, |_| "• ".to_string())
}

fn format_list(
    text: &str,
    markers: &[NumberMarker],
    marker_text: impl Fn(&NumberMarker) -> String,
) -> Option<String> {
    let mut items = Vec::with_capacity(markers.len());
    for (index, marker) in markers.iter().enumerate() {
        let item_end = markers
            .get(index + 1)
            .map_or(text.len(), |next| next.range.start);
        let item = text[marker.range.end..item_end].trim_matches(|character: char| {
            character.is_whitespace() || matches!(character, ',' | ':' | ';' | '-')
        });
        let item = item
            .strip_prefix('.')
            .filter(|after_period| after_period.chars().next().is_some_and(char::is_whitespace))
            .unwrap_or(item)
            .trim_start();
        if item.is_empty() {
            return None;
        }
        items.push((marker, item.to_string()));
    }

    let prefix = text[..markers[0].range.start].trim_end();
    let mut output = String::new();
    if !prefix.is_empty() {
        output.push_str(prefix);
        output.push('\n');
    }
    for (index, (marker, item)) in items.into_iter().enumerate() {
        if index > 0 {
            output.push('\n');
        }
        output.push_str(&marker_text(marker));
        output.push_str(&item);
    }
    Some(output)
}

#[derive(Clone, Copy)]
enum CommandSpacing {
    Punctuation,
    Newline,
    Open,
    Close,
    Tight,
    EmDash,
}

#[derive(Clone, Copy)]
struct SpokenCommand {
    phrase: &'static str,
    replacement: &'static str,
    spacing: CommandSpacing,
}

fn replace_spoken_commands(text: &str) -> String {
    const COMMANDS: &[SpokenCommand] = &[
        SpokenCommand {
            phrase: "new paragraph",
            replacement: "\n\n",
            spacing: CommandSpacing::Newline,
        },
        SpokenCommand {
            phrase: "exclamation point",
            replacement: "!",
            spacing: CommandSpacing::Punctuation,
        },
        SpokenCommand {
            phrase: "exclamation mark",
            replacement: "!",
            spacing: CommandSpacing::Punctuation,
        },
        SpokenCommand {
            phrase: "open parenthesis",
            replacement: "(",
            spacing: CommandSpacing::Open,
        },
        SpokenCommand {
            phrase: "close parenthesis",
            replacement: ")",
            spacing: CommandSpacing::Close,
        },
        SpokenCommand {
            phrase: "question mark",
            replacement: "?",
            spacing: CommandSpacing::Punctuation,
        },
        SpokenCommand {
            phrase: "forward slash",
            replacement: "/",
            spacing: CommandSpacing::Tight,
        },
        SpokenCommand {
            phrase: "equals sign",
            replacement: "=",
            spacing: CommandSpacing::Tight,
        },
        SpokenCommand {
            phrase: "open bracket",
            replacement: "[",
            spacing: CommandSpacing::Open,
        },
        SpokenCommand {
            phrase: "close bracket",
            replacement: "]",
            spacing: CommandSpacing::Close,
        },
        SpokenCommand {
            phrase: "new line",
            replacement: "\n",
            spacing: CommandSpacing::Newline,
        },
        SpokenCommand {
            phrase: "full stop",
            replacement: ".",
            spacing: CommandSpacing::Punctuation,
        },
        SpokenCommand {
            phrase: "em dash",
            replacement: "—",
            spacing: CommandSpacing::EmDash,
        },
        SpokenCommand {
            phrase: "semicolon",
            replacement: ";",
            spacing: CommandSpacing::Punctuation,
        },
        SpokenCommand {
            phrase: "ellipsis",
            replacement: "…",
            spacing: CommandSpacing::Punctuation,
        },
        SpokenCommand {
            phrase: "underscore",
            replacement: "_",
            spacing: CommandSpacing::Tight,
        },
        SpokenCommand {
            phrase: "hyphen",
            replacement: "-",
            spacing: CommandSpacing::Tight,
        },
        SpokenCommand {
            phrase: "comma",
            replacement: ",",
            spacing: CommandSpacing::Punctuation,
        },
        SpokenCommand {
            phrase: "period",
            replacement: ".",
            spacing: CommandSpacing::Punctuation,
        },
        SpokenCommand {
            phrase: "colon",
            replacement: ":",
            spacing: CommandSpacing::Punctuation,
        },
        SpokenCommand {
            phrase: "dash",
            replacement: "-",
            spacing: CommandSpacing::Tight,
        },
        SpokenCommand {
            phrase: "dot",
            replacement: ".",
            spacing: CommandSpacing::Tight,
        },
    ];

    let mut output = String::with_capacity(text.len());
    let mut cursor = 0;
    while cursor < text.len() {
        let matched = COMMANDS.iter().find_map(|command| {
            match_phrase_at(text, cursor, command.phrase, command.phrase).map(|end| (end, *command))
        });
        if let Some((end, command)) = matched {
            match command.spacing {
                CommandSpacing::Punctuation | CommandSpacing::Close => {
                    trim_horizontal_end(&mut output);
                    output.push_str(command.replacement);
                }
                CommandSpacing::Newline => {
                    trim_horizontal_end(&mut output);
                    while output.ends_with('\n') {
                        output.pop();
                    }
                    output.push_str(command.replacement);
                }
                CommandSpacing::Open => {
                    output.push_str(command.replacement);
                }
                CommandSpacing::Tight => {
                    trim_horizontal_end(&mut output);
                    output.push_str(command.replacement);
                }
                CommandSpacing::EmDash => {
                    trim_horizontal_end(&mut output);
                    if !output.is_empty() {
                        output.push(' ');
                    }
                    output.push_str(command.replacement);
                    output.push(' ');
                }
            }
            cursor = end;
            if matches!(
                command.spacing,
                CommandSpacing::Newline | CommandSpacing::Open | CommandSpacing::Tight
            ) {
                cursor = skip_horizontal_whitespace(text, cursor);
            }
        } else {
            let character = text[cursor..]
                .chars()
                .next()
                .expect("cursor remains on a character boundary");
            output.push(character);
            cursor += character.len_utf8();
        }
    }
    output
}

fn normalize_whitespace(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut pending_space = false;
    let mut newline_count = 0;

    for character in text.chars() {
        if character == '\n' {
            trim_horizontal_end(&mut output);
            pending_space = false;
            newline_count = (newline_count + 1).min(2);
            continue;
        }
        if character.is_whitespace() {
            pending_space = true;
            continue;
        }
        if newline_count > 0 {
            while output.ends_with(' ') {
                output.pop();
            }
            for _ in 0..newline_count {
                output.push('\n');
            }
            newline_count = 0;
            pending_space = false;
        } else if pending_space && !output.is_empty() {
            output.push(' ');
            pending_space = false;
        }
        output.push(character);
    }
    output.trim().to_string()
}

fn match_phrase_at(
    text: &str,
    start: usize,
    phrase: &str,
    normalized_phrase: &str,
) -> Option<usize> {
    if start > text.len() || !text.is_char_boundary(start) {
        return None;
    }
    let first = phrase.chars().next()?;
    let candidate_first = text[start..].chars().next()?;
    if !candidate_first.to_lowercase().eq(first.to_lowercase()) {
        return None;
    }
    if is_word_character(first)
        && text[..start]
            .chars()
            .next_back()
            .is_some_and(is_word_character)
    {
        return None;
    }

    let mut end = start;
    for _ in phrase.chars() {
        let character = text[end..].chars().next()?;
        end += character.len_utf8();
    }
    let candidate = &text[start..end];
    if !candidate
        .chars()
        .flat_map(char::to_lowercase)
        .eq(normalized_phrase.chars())
    {
        return None;
    }
    let last = phrase.chars().next_back()?;
    if is_word_character(last) && text[end..].chars().next().is_some_and(is_word_character) {
        return None;
    }
    Some(end)
}

fn find_phrase_from(text: &str, phrase: &str, from: usize) -> Option<Range<usize>> {
    let normalized = phrase.to_lowercase();
    text[from..].char_indices().find_map(|(offset, _)| {
        let start = from + offset;
        match_phrase_at(text, start, phrase, &normalized).map(|end| start..end)
    })
}

fn find_earliest_phrase(text: &str, phrases: &[&str], from: usize) -> Option<Range<usize>> {
    phrases
        .iter()
        .filter_map(|phrase| find_phrase_from(text, phrase, from))
        .min_by_key(|range| (range.start, Reverse(range.end - range.start)))
}

fn word_ranges(text: &str) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    let mut start = None;
    for (index, character) in text.char_indices() {
        if is_word_character(character) || matches!(character, '\'' | '’') && start.is_some() {
            start.get_or_insert(index);
        } else if let Some(start) = start.take() {
            ranges.push(start..index);
        }
    }
    if let Some(start) = start {
        ranges.push(start..text.len());
    }
    ranges
}

fn is_word_character(character: char) -> bool {
    character.is_alphanumeric() || character == '_'
}

fn is_hard_boundary(character: char) -> bool {
    matches!(character, '.' | '!' | '?' | '\n')
}

fn previous_non_whitespace(text: &str, before: usize) -> Option<(usize, char)> {
    text[..before]
        .char_indices()
        .rev()
        .find(|(_, character)| !character.is_whitespace())
}

fn next_non_whitespace(text: &str, after: usize) -> Option<(usize, char)> {
    text[after..]
        .char_indices()
        .find(|(_, character)| !character.is_whitespace())
        .map(|(index, character)| (after + index, character))
}

fn join_fragments(prefix: &str, suffix: &str) -> String {
    if prefix.is_empty() {
        return suffix.to_string();
    }
    if suffix.is_empty() {
        return prefix.to_string();
    }
    let needs_space = !prefix.ends_with(['\n', ' ', '(', '[', '{', '/', '-', '_'])
        && !suffix.starts_with([
            '\n', ',', '.', ';', ':', '!', '?', ')', ']', '}', '/', '-', '_',
        ]);
    format!("{prefix}{}{suffix}", if needs_space { " " } else { "" })
}

fn trim_horizontal_end(value: &mut String) {
    while value.ends_with([' ', '\t', '\r']) {
        value.pop();
    }
}

fn skip_horizontal_whitespace(text: &str, mut cursor: usize) -> usize {
    while cursor < text.len() {
        let character = text[cursor..]
            .chars()
            .next()
            .expect("cursor remains on a character boundary");
        if !matches!(character, ' ' | '\t' | '\r') {
            break;
        }
        cursor += character.len_utf8();
    }
    cursor
}

fn capitalize_first(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut capitalized = false;
    for character in value.chars() {
        if !capitalized && character.is_alphabetic() {
            output.extend(character.to_uppercase());
            capitalized = true;
        } else {
            output.push(character);
        }
    }
    output
}

fn starts_with_uppercase(value: &str) -> bool {
    value
        .chars()
        .find(|character| character.is_alphabetic())
        .is_some_and(char::is_uppercase)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(written: &str, spoken: Option<&str>) -> DictionaryEntry {
        prepare_dictionary_entry(written, spoken).unwrap()
    }

    #[test]
    fn validates_and_normalizes_dictionary_entries() {
        assert_eq!(
            prepare_dictionary_entry("  Wispr   Flow ", Some("whisper flow")).unwrap(),
            DictionaryEntry {
                written_form: "Wispr Flow".to_string(),
                spoken_form: Some("whisper flow".to_string()),
            }
        );
        assert_eq!(
            prepare_dictionary_entry("OpenAI", Some("openai"))
                .unwrap()
                .spoken_form,
            None
        );
        assert!(prepare_dictionary_entry("", None).is_err());
        assert!(prepare_dictionary_entry(&"x".repeat(81), None).is_err());
    }

    #[test]
    fn rejects_ambiguous_dictionary_triggers() {
        let entries = vec![
            entry("Wispr Flow", Some("whisper flow")),
            entry("Whisper Flow", None),
        ];
        assert!(validate_dictionary(&entries).is_err());
    }

    #[test]
    fn dictionary_replacement_is_longest_whole_phrase_and_non_cascading() {
        let entries = vec![
            entry("OpenAI", None),
            entry("Wispr Flow", Some("whisper flow")),
            entry("Flow", None),
            entry("B", Some("A")),
            entry("C", Some("B")),
        ];
        assert_eq!(
            process_transcript("openai and whisper flow in workflow A", &entries, false),
            "OpenAI and Wispr Flow in workflow B"
        );
    }

    #[test]
    fn vocabulary_prompt_is_local_bounded_and_recent_first() {
        let entries = vec![entry("FirstTerm", None), entry("NewestTerm", None)];
        let prompt = vocabulary_prompt(&entries).unwrap();
        assert_eq!(prompt, "Vocabulary: FirstTerm, NewestTerm.");
        assert!(prompt.len() <= MAX_VOCABULARY_PROMPT_BYTES);
    }

    #[test]
    fn removes_conservative_fillers() {
        assert_eq!(
            process_transcript("Um, this uh works.", &[], true),
            "this works."
        );
        assert_eq!(
            process_transcript("I, um, think so.", &[], true),
            "I think so."
        );
        assert_eq!(
            process_transcript("The umbrella stays.", &[], true),
            "The umbrella stays."
        );
    }

    #[test]
    fn expands_spoken_punctuation_and_layout() {
        assert_eq!(
            process_transcript(
                "Hello comma world period new paragraph Next line colon new line done exclamation point",
                &[],
                true,
            ),
            "Hello, world.\n\nNext line:\ndone!"
        );
        assert_eq!(
            process_transcript("example dot com forward slash docs", &[], true),
            "example.com/docs"
        );
    }

    #[test]
    fn formats_explicit_numbered_and_bullet_lists() {
        assert_eq!(
            process_transcript(
                "Groceries colon number one apples number two bananas number three milk",
                &[],
                true,
            ),
            "Groceries:\n1. apples\n2. bananas\n3. milk"
        );
        assert_eq!(
            process_transcript("Tasks bullet call Sam bullet send notes", &[], true),
            "Tasks\n• call Sam\n• send notes"
        );
        assert_eq!(
            process_transcript("This is the number one priority.", &[], true),
            "This is the number one priority."
        );
        assert_eq!(
            process_transcript("Files number one .env number two Cargo.toml", &[], true,),
            "Files\n1. .env\n2. Cargo.toml"
        );
    }

    #[test]
    fn scratch_that_discards_only_the_current_clause() {
        assert_eq!(
            process_transcript(
                "Email Bob. Tell Alice Tuesday, scratch that, tell Alice Friday.",
                &[],
                true,
            ),
            "Email Bob. Tell Alice Friday."
        );
        assert_eq!(
            process_transcript("git status scratch that cargo test", &[], true),
            "cargo test"
        );
    }

    #[test]
    fn actually_corrects_values_but_preserves_normal_usage() {
        assert_eq!(
            process_transcript("Let's meet at two, actually three.", &[], true),
            "Let's meet at three."
        );
        assert_eq!(
            process_transcript("This is good, actually very good.", &[], true),
            "This is good, actually very good."
        );
        assert_eq!(
            process_transcript(
                "Use the green button, actually make that the blue button.",
                &[],
                true,
            ),
            "Use the blue button."
        );
    }

    #[test]
    fn cleanup_can_be_disabled_without_disabling_dictionary() {
        let dictionary = vec![entry("FnScribe", Some("fn scribe"))];
        assert_eq!(
            process_transcript("um fn scribe comma", &dictionary, false),
            "um FnScribe comma"
        );
    }
}
