//! Finding tiles: what a tile offers to a search and how a query matches it (SPEC "Find", D-264).
//!
//! A query is split into words; a tile matches when every word appears somewhere in its
//! text, in any order, ignoring case and accents ("cafe" finds "Café"). The match
//! reports each line of the tile's text that holds one of the words, so the results can
//! show why a tile was found.

use crate::model::Tile;
use serde::Serialize;

/// Where a line of a tile's searchable text comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Field {
    /// The tile's title.
    Title,
    /// The name of the file it was made from (`Tile::source`), without its extension.
    File,
    /// A line of its notes.
    Notes,
    /// A line of the clip's subtitles (`Tile::subtitles`, D-260).
    Subtitles,
}

impl Field {
    /// The word the results list shows before the matched text.
    pub fn label(self) -> &'static str {
        match self {
            Field::Title => "Title",
            Field::File => "File",
            Field::Notes => "Notes",
            Field::Subtitles => "Subtitles",
        }
    }
}

/// Everything a tile offers to a search, one entry per line, the title first. This is
/// the one place a new source of text is added. A clip's subtitles give one entry per
/// line, as [`Field::Subtitles`].
pub fn searchable(t: &Tile) -> Vec<(Field, &str)> {
    let mut out = vec![(Field::Title, t.title.as_str())];
    // The file's name without its extension (".wav" on every tile would match any "a"),
    // unless it's the title over again.
    let stem = t.source.as_deref().map(|s| s.rsplit_once('.').map_or(s, |(stem, _)| stem));
    if let Some(s) = stem.filter(|s| !s.is_empty() && fold(s) != fold(&t.title)) {
        out.push((Field::File, s));
    }
    out.extend(t.notes.lines().map(str::trim).filter(|l| !l.is_empty()).map(|l| (Field::Notes, l)));
    out.extend(t.subtitles.iter().flat_map(|c| c.text.lines()).map(str::trim).filter(|l| !l.is_empty()).map(|l| (Field::Subtitles, l)));
    out
}

/// One line that matched, as the results list and `tile.search` show it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Matched {
    pub field: Field,
    pub text: String,
}

/// A query, folded once.
#[derive(Clone, Debug, Default)]
pub struct Query {
    words: Vec<String>,
}

impl Query {
    pub fn new(text: &str) -> Query {
        Query { words: fold(text).split_whitespace().map(str::to_owned).collect() }
    }

    pub fn is_empty(&self) -> bool {
        self.words.is_empty()
    }

    /// Whether every word is somewhere in the tile's text. An empty query matches nothing.
    pub fn matches(&self, t: &Tile) -> bool {
        self.find(t).is_some()
    }

    /// The tile's lines that hold any of the words, when every word is found; `None`
    /// when it doesn't match.
    pub fn find(&self, t: &Tile) -> Option<Vec<Matched>> {
        if self.is_empty() {
            return None;
        }
        let lines: Vec<(Field, &str, String)> = searchable(t).into_iter().map(|(f, s)| (f, s, fold(s))).collect();
        if !self.words.iter().all(|w| lines.iter().any(|(_, _, l)| l.contains(w.as_str()))) {
            return None;
        }
        Some(
            lines
                .into_iter()
                .filter(|(_, _, l)| self.words.iter().any(|w| l.contains(w.as_str())))
                .map(|(field, text, _)| Matched { field, text: text.to_owned() })
                .collect(),
        )
    }
}

/// Lower case without accents, for comparing: "Ça Fait Été" → "ca fait ete". Covers
/// Latin-1 and Latin Extended-A, and drops combining marks (decomposed text).
pub fn fold(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars().flat_map(char::to_lowercase) {
        match c {
            '\u{300}'..='\u{36f}' => {}
            'ß' => out.push_str("ss"),
            'æ' => out.push_str("ae"),
            'œ' => out.push_str("oe"),
            'þ' => out.push_str("th"),
            '\u{2018}' | '\u{2019}' | '\u{2032}' => out.push('\''),
            '\u{201c}' | '\u{201d}' => out.push('"'),
            c => out.push(base_letter(c)),
        }
    }
    out
}

fn base_letter(c: char) -> char {
    match c {
        'à'..='å' | 'ā' | 'ă' | 'ą' => 'a',
        'ç' | 'ć' | 'ĉ' | 'ċ' | 'č' => 'c',
        'ď' | 'đ' | 'ð' => 'd',
        'è'..='ë' | 'ē' | 'ĕ' | 'ė' | 'ę' | 'ě' => 'e',
        'ĝ' | 'ğ' | 'ġ' | 'ģ' => 'g',
        'ĥ' | 'ħ' => 'h',
        'ì'..='ï' | 'ĩ' | 'ī' | 'ĭ' | 'į' | 'ı' => 'i',
        'ĵ' => 'j',
        'ķ' => 'k',
        'ĺ' | 'ļ' | 'ľ' | 'ŀ' | 'ł' => 'l',
        'ñ' | 'ń' | 'ņ' | 'ň' => 'n',
        'ò'..='ö' | 'ø' | 'ō' | 'ŏ' | 'ő' => 'o',
        'ŕ' | 'ŗ' | 'ř' => 'r',
        'ś' | 'ŝ' | 'ş' | 'š' => 's',
        'ţ' | 'ť' | 'ŧ' => 't',
        'ù'..='ü' | 'ũ' | 'ū' | 'ŭ' | 'ů' | 'ű' | 'ų' => 'u',
        'ŵ' => 'w',
        'ý' | 'ÿ' | 'ŷ' => 'y',
        'ź' | 'ż' | 'ž' => 'z',
        c => c,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tile(title: &str, source: Option<&str>, notes: &str) -> Tile {
        Tile { title: title.into(), source: source.map(Into::into), notes: notes.into(), ..Default::default() }
    }

    #[test]
    fn subtitles_are_searched_line_by_line() {
        let mut t = tile("Clip 12", None, "");
        for text in ["Welcome back.", "Lights down!\nCurtain up."] {
            t.subtitles.push(crate::subtitles::SubtitleLine { start: 0.0, end: 1.0, text: text.into(), words: Vec::new(), aligned: false, source_timing: None, added: false, machine: None });
        }
        assert!(Query::new("curtain").matches(&t));
        assert!(Query::new("clip lights").matches(&t), "a word in the title and one in a subtitle");
        assert!(!Query::new("applause").matches(&t));
        assert!(searchable(&t).contains(&(Field::Subtitles, "Curtain up.")), "one entry per line of text");
        assert_eq!(serde_json::to_value(Field::Subtitles).unwrap(), "subtitles");
    }

    #[test]
    fn folds_case_and_accents() {
        assert_eq!(fold("Ça Fait Été"), "ca fait ete");
        assert_eq!(fold("STRAßE Œuvre"), "strasse oeuvre");
        assert_eq!(fold("Zoë Łódź"), "zoe lodz");
        // Decomposed: e + combining acute.
        assert_eq!(fold("Cafe\u{301}"), "cafe");
        assert_eq!(fold("Don\u{2019}t"), "don't");
    }

    #[test]
    fn every_word_must_match_in_any_order_and_any_field() {
        let t = tile("Big Applause", Some("crowd-cheer_02.wav"), "after the toast\nlouder in Act 2");
        assert!(Query::new("applause").matches(&t));
        assert!(Query::new("APPLAUSE big").matches(&t), "order and case don't matter");
        assert!(Query::new("applause cheer").matches(&t), "words may match different fields");
        assert!(Query::new("toast act").matches(&t));
        assert!(!Query::new("applause rimshot").matches(&t), "every word must match");
        assert!(!Query::new("   ").matches(&t), "an empty query matches nothing");
        assert!(Query::new("ÁPPLÄUSE").matches(&t), "accents in the query are ignored too");
    }

    #[test]
    fn reports_the_lines_that_matched() {
        let t = tile("Door Slam", Some("door.wav"), "first line\nslam on the cue\n");
        let m = Query::new("slam").find(&t).unwrap();
        assert_eq!(
            m,
            vec![Matched { field: Field::Title, text: "Door Slam".into() }, Matched { field: Field::Notes, text: "slam on the cue".into() }]
        );
        let t = tile("Slam", Some("door-03.wav"), "");
        let m = Query::new("door").find(&t).unwrap();
        assert_eq!(m, vec![Matched { field: Field::File, text: "door-03".into() }]);
        assert!(!Query::new("wav").matches(&t), "the extension isn't searched");
        assert_eq!(Query::new("zzz").find(&t), None);
    }

    #[test]
    fn a_tile_without_a_source_or_notes_offers_its_title() {
        let t = tile("Rimshot", None, "");
        assert_eq!(searchable(&t), vec![(Field::Title, "Rimshot")]);
        let t = tile("Rimshot", Some("rimshot.WAV"), "");
        assert_eq!(searchable(&t), vec![(Field::Title, "Rimshot")], "a file name that is the title isn't offered twice");
    }
}
