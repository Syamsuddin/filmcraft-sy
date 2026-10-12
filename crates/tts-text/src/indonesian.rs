//! Indonesian (Bahasa Indonesia) front end: a script in, Kokoro phoneme chunks out, for the
//! Kokoro voices offered as Indonesian narrators.
//!
//! Kokoro-82M has no Indonesian voices, but it speaks whatever phonemes it is given. Indonesian
//! spelling is close to phonemic, so letter rules get most words right without a dictionary:
//!
//! - **Reading** (`read_*`): numbers in Indonesian words (`1.250` → "seribu dua ratus lima puluh",
//!   `2,5` → "dua koma lima", `07` and phone numbers digit by digit), `%` → "persen",
//!   `Rp 15.000` → "lima belas ribu rupiah", `ke-2` → "kedua" (`ke-1` → "pertama"), a few
//!   common abbreviations (`dll`, `yg`, `dgn`, …), acronyms of up to four capitals spelled out with
//!   Indonesian letter names (`KTP` → "ka te pe"; longer ones such as `ASEAN` are read as words),
//!   and reduplication hyphens (`anak-anak`) as a space.
//! - **Sounds** ([`word_phonemes`]): `ng` → ŋ, `ny` → n j, `sy` → ʃ, `kh` → h, `c` → ʧ, `j` → ʤ,
//!   `y` → j, `q` → k, `x` → k s; `ai`, `au`, `oi` at the end of a word are diphthongs, elsewhere two
//!   syllables (`main` = ma-in). The letter `e` is the neutral vowel ə unless it is written `é`/`è`,
//!   ends the word, or comes before a final `h` or `k` (`boleh`, `jelek`); a short list of common
//!   words is respelled with é where it is said (`mereka` → meréka, `persen` → persén, `meja`, …). Users fix the rest with the
//!   pronunciation list ([`Overrides`]): an Indonesian respelling (`"telepon": "télépon"`) or raw
//!   phonemes between slashes.
//! - **Stress** on the second-to-last syllable, or on the last one when the second-to-last is ə;
//!   words of one syllable get none. The mark goes right before the vowel, as in the English path.
//!
//! The output uses only [`SYMBOLS`](crate::SYMBOLS) (the symbols the English path already sends
//! Kokoro); sounds Kokoro's American voices lack are approximated (ɲ as n j, the trilled r as ɹ,
//! final k as k). Written from scratch from the EYD spelling rules (clean-room); never panics:
//! input over 1 MiB is refused, numbers too long to read are read digit by digit.

use std::collections::BTreeMap;

use crate::normalize::{NormalizeError, Token, parse_pause};
use crate::phonemize::{Chunk, KEPT_PUNCT, Overrides, bounded_overrides, raw_override, split_long_chunk};

/// Maximum accepted input size (1 MiB), as for English.
const MAX_INPUT: usize = 1_048_576;
/// Longest digit run read as a number; longer runs are read digit by digit.
const MAX_NUMBER_DIGITS: usize = 15;
/// Longest word phonemized; longer "words" (pasted junk) are skipped.
const MAX_WORD_CHARS: usize = 64;
/// Digit names, 0–9.
const DIGITS: [&str; 10] = ["nol", "satu", "dua", "tiga", "empat", "lima", "enam", "tujuh", "delapan", "sembilan"];

/// Common words whose `e` the position rules get wrong, respelled with é where it is said
/// (stems; affixed forms are not covered). Written from general knowledge of Indonesian.
const E_RESPELLED: &[(&str, &str)] = &[
    ("bebas", "bébas"),
    ("bebek", "bébék"),
    ("becak", "bécak"),
    ("beda", "béda"),
    ("desa", "désa"),
    ("dewa", "déwa"),
    ("dewan", "déwan"),
    ("efek", "éfék"),
    ("efektif", "éféktif"),
    ("ekonomi", "ékonomi"),
    ("ekor", "ékor"),
    ("elektronik", "éléktronik"),
    ("ember", "émber"),
    ("enak", "énak"),
    ("energi", "énergi"),
    ("eropa", "éropa"),
    ("internet", "internét"),
    ("kereta", "keréta"),
    ("lebar", "lébar"),
    ("lele", "lélé"),
    ("meja", "méja"),
    ("merah", "mérah"),
    ("mereka", "meréka"),
    ("metode", "métode"),
    ("modern", "modérn"),
    ("nenek", "nénék"),
    ("pena", "péna"),
    ("persen", "persén"),
    ("peta", "péta"),
    ("presiden", "présidén"),
    ("problem", "problém"),
    ("rekan", "rékan"),
    ("sehat", "séhat"),
    ("sistem", "sistém"),
    ("teknologi", "téknologi"),
    ("telepon", "télépon"),
    ("televisi", "télévisi"),
    ("tema", "téma"),
    ("teori", "téori"),
];

/// Common shortened forms read in full.
fn abbreviation(lower: &str) -> Option<&'static str> {
    Some(match lower {
        "dll" => "dan lain lain",
        "dsb" => "dan sebagainya",
        "dst" => "dan seterusnya",
        "yg" => "yang",
        "dgn" => "dengan",
        "tdk" => "tidak",
        "utk" => "untuk",
        "sdh" => "sudah",
        "blm" => "belum",
        "kg" => "kilogram",
        "km" => "kilometer",
        "cm" => "sentimeter",
        "mm" => "milimeter",
        "jl" => "jalan",
        "no" => "nomor",
        "tsb" => "tersebut",
        _ => return None,
    })
}

/// Indonesian letter names, for spelling acronyms.
fn letter_name(c: char) -> Option<&'static str> {
    Some(match c.to_ascii_lowercase() {
        'a' => "a",
        'b' => "be",
        'c' => "ce",
        'd' => "de",
        'e' => "é",
        'f' => "ef",
        'g' => "ge",
        'h' => "ha",
        'i' => "i",
        'j' => "je",
        'k' => "ka",
        'l' => "el",
        'm' => "em",
        'n' => "en",
        'o' => "o",
        'p' => "pe",
        'q' => "ki",
        'r' => "er",
        's' => "es",
        't' => "te",
        'u' => "u",
        'v' => "fe",
        'w' => "we",
        'x' => "éks",
        'y' => "ye",
        'z' => "zet",
        _ => return None,
    })
}

// ---------------------------------------------------------------- numbers

/// 0–999 in words; `None` above.
fn below_thousand(n: u64) -> Option<Vec<&'static str>> {
    if n >= 1000 {
        return None;
    }
    let mut out = Vec::new();
    let hundreds = n / 100;
    let rest = n % 100;
    match hundreds {
        0 => {}
        1 => out.push("seratus"),
        h => {
            out.push(DIGITS.get(usize::try_from(h).ok()?)?);
            out.push("ratus");
        }
    }
    match rest {
        0 => {}
        10 => out.push("sepuluh"),
        11 => out.push("sebelas"),
        12..=19 => {
            out.push(DIGITS.get(usize::try_from(rest - 10).ok()?)?);
            out.push("belas");
        }
        20..=99 => {
            out.push(DIGITS.get(usize::try_from(rest / 10).ok()?)?);
            out.push("puluh");
            if !rest.is_multiple_of(10) {
                out.push(DIGITS.get(usize::try_from(rest % 10).ok()?)?);
            }
        }
        d => out.push(DIGITS.get(usize::try_from(d).ok()?)?),
    }
    Some(out)
}

/// A whole number in Indonesian words (`0` → "nol"); `None` from 10^15 up.
pub fn cardinal(n: u64) -> Option<Vec<&'static str>> {
    if n == 0 {
        return Some(vec!["nol"]);
    }
    if n >= 1_000_000_000_000_000 {
        return None;
    }
    let mut out = Vec::new();
    for (scale, word) in [(1_000_000_000_000u64, "triliun"), (1_000_000_000, "miliar"), (1_000_000, "juta"), (1_000, "ribu")] {
        let part = (n / scale) % 1000;
        if part == 0 {
            continue;
        }
        if part == 1 && scale == 1_000 {
            out.push("seribu");
        } else {
            out.extend(below_thousand(part)?);
            out.push(word);
        }
    }
    out.extend(below_thousand(n % 1000)?);
    Some(out)
}

/// `ke-n`: "pertama" for 1, else "ke" + the number as one word ("kedua", "kesepuluh").
fn ordinal(n: u64) -> Option<Vec<String>> {
    if n == 1 {
        return Some(vec!["pertama".into()]);
    }
    let mut words: Vec<String> = cardinal(n)?.into_iter().map(String::from).collect();
    let first = words.first_mut()?;
    *first = format!("ke{first}");
    Some(words)
}

fn digit_words(digits: &str) -> Vec<String> {
    digits.chars().filter_map(|c| c.to_digit(10).and_then(|d| DIGITS.get(d as usize)).map(|w| w.to_string())).collect()
}

/// Read a number starting at `s` (which starts with a digit). Returns the words and bytes used.
/// `1.250.000` (dot thousands) and `2,5` / `2.5` (decimal comma or point) are understood; a
/// leading zero or a run too long to read goes digit by digit; a following `%` is "persen".
/// For `money`, a zero decimal part (`,00`) and the `,-` of a rupiah amount are not read.
fn read_number(s: &str, money: bool) -> (Vec<String>, usize) {
    let int_len = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
    let mut digits = s.get(..int_len).unwrap_or("").to_string();
    let mut used = int_len;
    // thousands groups: `.` followed by exactly three digits (and no fourth)
    loop {
        let rest = s.get(used..).unwrap_or("");
        let group = rest.strip_prefix('.').map(|r| r.find(|c: char| !c.is_ascii_digit()).unwrap_or(r.len()));
        match group {
            Some(3) if int_len <= 3 && !(int_len > 1 && digits.starts_with('0')) => {
                digits.push_str(rest.get(1..4).unwrap_or(""));
                used += 4;
            }
            _ => break,
        }
    }
    let mut words = if digits.len() > 1 && digits.starts_with('0') || digits.len() > MAX_NUMBER_DIGITS {
        digit_words(&digits)
    } else {
        match digits.parse::<u64>().ok().and_then(cardinal) {
            Some(w) => w.into_iter().map(String::from).collect(),
            None => digit_words(&digits),
        }
    };
    // a decimal part: `,` or `.` then digits, read digit by digit
    let rest = s.get(used..).unwrap_or("");
    if let Some(after) = rest.strip_prefix(',').or_else(|| rest.strip_prefix('.')) {
        let n = after.find(|c: char| !c.is_ascii_digit()).unwrap_or(after.len());
        let decimals = after.get(..n).unwrap_or("");
        if money && n > 0 && decimals.chars().all(|c| c == '0') {
            used += 1 + n;
        } else if money && n == 0 && after.starts_with('-') {
            used += 2;
        } else if n > 0 && n <= MAX_NUMBER_DIGITS {
            words.push("koma".into());
            words.extend(digit_words(decimals));
            used += 1 + n;
        }
    }
    if s.get(used..).is_some_and(|r| r.starts_with('%')) {
        words.push("persen".into());
        used += 1;
    }
    (words, used)
}

// ---------------------------------------------------------------- reading the script

fn push_words(out: &mut Vec<Token>, words: impl IntoIterator<Item = String>) {
    out.extend(words.into_iter().map(Token::Word));
}

/// Script → tokens (words in plain Indonesian, kept punctuation, sentence ends, pauses).
fn read(text: &str) -> Result<Vec<Token>, NormalizeError> {
    if text.len() > MAX_INPUT {
        return Err(NormalizeError::TooLong);
    }
    let mut out: Vec<Token> = Vec::new();
    let mut pos = 0usize;
    while let Some(rest) = text.get(pos..) {
        let Some(c) = rest.chars().next() else { break };
        if c == '['
            && let Some((millis, used)) = parse_pause(rest)
        {
            out.push(Token::Pause { millis });
            pos += used;
            continue;
        }
        if c.is_ascii_digit() {
            let (words, used) = read_number(rest, false);
            push_words(&mut out, words);
            pos += used.max(1);
            continue;
        }
        if c.is_alphabetic() {
            let len = rest.find(|c: char| !(c.is_alphabetic() || c == '\'')).unwrap_or(rest.len());
            let word = rest.get(..len).unwrap_or("").trim_end_matches('\'');
            pos += len.max(c.len_utf8());
            let lower = word.to_lowercase();
            let after = text.get(pos..).unwrap_or("");
            // Rp 15.000 / Rp15.000 / Rp. 15.000 → "lima belas ribu rupiah"
            if lower == "rp" {
                let skip = after.len() - after.trim_start_matches(['.', ' ']).len();
                let num = after.get(skip..).unwrap_or("");
                if num.starts_with(|c: char| c.is_ascii_digit()) {
                    let (words, used) = read_number(num, true);
                    push_words(&mut out, words);
                    out.push(Token::Word("rupiah".into()));
                    pos += skip + used;
                    continue;
                }
            }
            // ke-2 → "kedua"
            if lower == "ke"
                && let Some(num) = after.strip_prefix('-')
                && num.starts_with(|c: char| c.is_ascii_digit())
            {
                let n = num.find(|c: char| !c.is_ascii_digit()).unwrap_or(num.len());
                if let Some(words) = num.get(..n).and_then(|d| d.parse::<u64>().ok()).and_then(ordinal) {
                    push_words(&mut out, words);
                    pos += 1 + n;
                    continue;
                }
            }
            if let Some(full) = abbreviation(&lower) {
                push_words(&mut out, full.split(' ').map(String::from));
                // `dll.` ends no sentence
                if after.starts_with('.') && !after.get(1..).unwrap_or("").trim_start().is_empty() {
                    pos += 1;
                }
                continue;
            }
            let chars = word.chars().count();
            if (2..=4).contains(&chars) && word.chars().all(|c| c.is_ascii_uppercase()) {
                push_words(&mut out, word.chars().filter_map(letter_name).map(String::from));
                continue;
            }
            out.push(Token::Word(lower));
            continue;
        }
        pos += c.len_utf8();
        match c {
            '.' | '!' | '?' => {
                // "..." and "?!" end one sentence
                if !matches!(out.last(), Some(Token::SentenceEnd | Token::SentenceEndWith(_)) | None) {
                    out.push(if c == '.' { Token::SentenceEnd } else { Token::SentenceEndWith(c) });
                }
            }
            '%' => out.push(Token::Word("persen".into())),
            _ if KEPT_PUNCT.contains(&c) => out.push(Token::Punct(c)),
            // hyphens (anak-anak), apostrophes, symbols: a word break
            _ => {}
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------- letters to sounds

/// One sound: its Kokoro symbol(s) and whether it is a syllable nucleus.
#[derive(Clone, Copy)]
struct Sound {
    sym: &'static str,
    vowel: bool,
}

const fn v(sym: &'static str) -> Sound {
    Sound { sym, vowel: true }
}
const fn k(sym: &'static str) -> Sound {
    Sound { sym, vowel: false }
}

/// Fold the accented letters Indonesian writing uses; `é`/`è` stay marked as é (`E`), `ê` is ə.
fn fold(c: char) -> Option<char> {
    Some(match c {
        'a'..='z' => c,
        'é' | 'è' | 'ë' => 'E',
        'ê' => 'e',
        'á' | 'à' | 'â' | 'ä' => 'a',
        'í' | 'ì' | 'î' | 'ï' => 'i',
        'ó' | 'ò' | 'ô' | 'ö' => 'o',
        'ú' | 'ù' | 'û' | 'ü' => 'u',
        _ => return None,
    })
}

/// Whether the `e` at `i` of `w` (lowercase, folded) is é by the position rules.
fn e_is_taik(w: &[char], i: usize) -> bool {
    let next = w.get(i + 1).copied();
    let after = w.get(i + 2).copied();
    match (next, after) {
        // word-final: sate, kafe, tempe
        (None, _) => true,
        // before a final h or k: boleh, oleh, jelek, pendek
        (Some('h' | 'k'), None) => true,
        _ => false,
    }
}

/// The sounds of one word (lowercase letters, `é` allowed). Unknown characters are skipped.
fn sounds(word: &str) -> Vec<Sound> {
    let spelled = E_RESPELLED.iter().find(|(w, _)| *w == word).map_or(word, |(_, r)| r);
    let w: Vec<char> = spelled.chars().filter_map(fold).collect();
    let mut out = Vec::with_capacity(w.len());
    let mut i = 0usize;
    while let Some(&c) = w.get(i) {
        let next = w.get(i + 1).copied();
        let last = i + 2 >= w.len();
        let (sound, n): (&[Sound], usize) = match (c, next) {
            ('n', Some('g')) => (&[k("ŋ")], 2),
            ('n', Some('y')) => (&[k("n"), k("j")], 2),
            ('s', Some('y')) => (&[k("ʃ")], 2),
            ('k', Some('h')) => (&[k("h")], 2),
            ('a', Some('i')) if last => (&[v("I")], 2),
            ('a', Some('u')) if last => (&[v("W")], 2),
            ('o', Some('i')) if last => (&[v("Y")], 2),
            ('a', _) => (&[v("ɑ")], 1),
            ('i', _) => (&[v("i")], 1),
            ('u', _) => (&[v("u")], 1),
            ('o', _) => (&[v("O")], 1),
            ('E', _) => (&[v("ɛ")], 1),
            ('e', _) if e_is_taik(&w, i) => (&[v("ɛ")], 1),
            ('e', _) => (&[v("ə")], 1),
            ('c', _) => (&[k("ʧ")], 1),
            ('j', _) => (&[k("ʤ")], 1),
            ('y', _) => (&[k("j")], 1),
            ('q', _) => (&[k("k")], 1),
            ('x', _) => (&[k("k"), k("s")], 1),
            ('r', _) => (&[k("ɹ")], 1),
            ('g', _) => (&[k("ɡ")], 1),
            ('b', _) => (&[k("b")], 1),
            ('d', _) => (&[k("d")], 1),
            ('f', _) => (&[k("f")], 1),
            ('h', _) => (&[k("h")], 1),
            ('k', _) => (&[k("k")], 1),
            ('l', _) => (&[k("l")], 1),
            ('m', _) => (&[k("m")], 1),
            ('n', _) => (&[k("n")], 1),
            ('p', _) => (&[k("p")], 1),
            ('s', _) => (&[k("s")], 1),
            ('t', _) => (&[k("t")], 1),
            ('v', _) => (&[k("f")], 1),
            ('w', _) => (&[k("w")], 1),
            ('z', _) => (&[k("z")], 1),
            _ => (&[], 1),
        };
        // a doubled consonant (Allah, kalla) is one sound
        for s in sound {
            let same = out.last().is_some_and(|p: &Sound| !p.vowel && !s.vowel && p.sym == s.sym);
            if !same {
                out.push(*s);
            }
        }
        i += n;
    }
    out
}

/// Kokoro phonemes of one Indonesian word with its stress mark, or `None` if it has no sounds.
pub fn word_phonemes(word: &str) -> Option<String> {
    let lower = word.to_lowercase();
    if lower.chars().count() > MAX_WORD_CHARS {
        return None;
    }
    let s = sounds(&lower);
    let nuclei: Vec<usize> = s.iter().enumerate().filter(|(_, x)| x.vowel).map(|(i, _)| i).collect();
    let stressed = match nuclei.as_slice() {
        [] | [_] => None,
        [.., pen, last] => Some(if s.get(*pen).is_some_and(|x| x.sym == "ə") { *last } else { *pen }),
    };
    let mut out = String::new();
    for (i, x) in s.iter().enumerate() {
        if Some(i) == stressed {
            out.push('ˈ');
        }
        out.push_str(x.sym);
    }
    (!out.is_empty()).then_some(out)
}

fn word_with_overrides(word: &str, overrides: &BTreeMap<String, String>) -> Option<String> {
    if let Some(value) = overrides.get(word) {
        if value.starts_with('/') {
            if let Some(raw) = raw_override(value) {
                return Some(raw);
            }
        } else {
            // an Indonesian respelling, read without overrides (no loops)
            let tokens = read(value).ok()?;
            let parts: Vec<String> = tokens.iter().filter_map(|t| if let Token::Word(w) = t { word_phonemes(w) } else { None }).collect();
            if !parts.is_empty() {
                return Some(parts.join(" "));
            }
        }
    }
    word_phonemes(word)
}

fn flush(sentence: &mut Vec<Token>, chunks: &mut Vec<Chunk>, overrides: &BTreeMap<String, String>) {
    let mut out = String::new();
    for token in std::mem::take(sentence) {
        match token {
            Token::Word(w) => {
                let Some(p) = word_with_overrides(&w, overrides) else { continue };
                if !out.is_empty() {
                    out.push(' ');
                }
                out.push_str(&p);
            }
            Token::Punct(c) => out.push(c),
            Token::Pause { .. } | Token::SentenceEnd | Token::SentenceEndWith(_) => {}
        }
    }
    for piece in split_long_chunk(out) {
        if piece.chars().any(|c| c.is_alphabetic()) {
            chunks.push(Chunk::Phonemes(piece));
        }
    }
}

/// Indonesian script → Kokoro chunks: one per sentence (split when over
/// [`MAX_CHUNK_CHARS`](crate::MAX_CHUNK_CHARS)), pauses between them.
pub fn phonemize(text: &str, overrides: &Overrides) -> Result<Vec<Chunk>, NormalizeError> {
    let overrides: BTreeMap<String, String> = bounded_overrides(overrides);
    let mut chunks = Vec::new();
    let mut sentence = Vec::new();
    for token in read(text)? {
        match token {
            Token::SentenceEnd => {
                sentence.push(Token::Punct('.'));
                flush(&mut sentence, &mut chunks, &overrides);
            }
            Token::SentenceEndWith(c) => {
                sentence.push(Token::Punct(c));
                flush(&mut sentence, &mut chunks, &overrides);
            }
            Token::Pause { millis } => {
                flush(&mut sentence, &mut chunks, &overrides);
                chunks.push(Chunk::Pause { millis });
            }
            other => sentence.push(other),
        }
    }
    flush(&mut sentence, &mut chunks, &overrides);
    Ok(chunks)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MAX_CHUNK_CHARS, SYMBOLS};

    fn said(text: &str) -> String {
        read(text)
            .unwrap()
            .into_iter()
            .map(|t| match t {
                Token::Word(w) => w,
                Token::Pause { millis } => format!("<{millis}>"),
                Token::SentenceEnd => "|".into(),
                Token::SentenceEndWith(c) => format!("{c}|"),
                Token::Punct(c) => c.to_string(),
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn ph(word: &str) -> String {
        word_phonemes(word).unwrap()
    }

    #[test]
    fn numbers_are_read_in_indonesian() {
        for (n, words) in [
            (0, "nol"),
            (7, "tujuh"),
            (10, "sepuluh"),
            (11, "sebelas"),
            (15, "lima belas"),
            (20, "dua puluh"),
            (25, "dua puluh lima"),
            (100, "seratus"),
            (101, "seratus satu"),
            (250, "dua ratus lima puluh"),
            (1000, "seribu"),
            (1001, "seribu satu"),
            (2024, "dua ribu dua puluh empat"),
            (11_000, "sebelas ribu"),
            (100_000, "seratus ribu"),
            (1_000_000, "satu juta"),
            (2_500_000, "dua juta lima ratus ribu"),
            (1_000_000_000, "satu miliar"),
            (3_000_000_000_000, "tiga triliun"),
        ] {
            assert_eq!(cardinal(n).unwrap().join(" "), words, "{n}");
        }
        assert_eq!(cardinal(1_000_000_000_000_000), None);
        assert_eq!(cardinal(u64::MAX), None);
    }

    #[test]
    fn number_formats_money_percent_and_ordinals() {
        assert_eq!(said("1.250"), "seribu dua ratus lima puluh");
        assert_eq!(said("1.000.000"), "satu juta");
        assert_eq!(said("2,5"), "dua koma lima");
        assert_eq!(said("3.14"), "tiga koma satu empat");
        assert_eq!(said("75%"), "tujuh puluh lima persen");
        assert_eq!(said("2,5 %"), "dua koma lima persen");
        assert_eq!(said("Rp 15.000"), "lima belas ribu rupiah");
        assert_eq!(said("Rp15.000,-"), "lima belas ribu rupiah");
        assert_eq!(said("Rp. 2.500"), "dua ribu lima ratus rupiah");
        assert_eq!(said("ke-1"), "pertama");
        assert_eq!(said("ke-2"), "kedua");
        assert_eq!(said("ke-10"), "kesepuluh");
        assert_eq!(said("ke-21"), "kedua puluh satu");
        assert_eq!(said("0812"), "nol delapan satu dua");
        assert_eq!(
            said("1234567890123456789"),
            "satu dua tiga empat lima enam tujuh delapan sembilan nol satu dua tiga empat lima enam tujuh delapan sembilan"
        );
    }

    #[test]
    fn words_abbreviations_acronyms_and_sentences() {
        assert_eq!(said("Anak-anak bermain."), "anak anak bermain |");
        assert_eq!(said("Apa kabar? Baik!"), "apa kabar ?| baik !|");
        assert_eq!(said("buah, sayur, dll. Semua segar."), "buah , sayur , dan lain lain semua segar |");
        assert_eq!(said("Bawa KTP dan SIM"), "bawa ka te pe dan es i em");
        assert_eq!(said("Negara ASEAN"), "negara asean");
        assert_eq!(said("Tunggu [pause 1s] ya"), "tunggu <1000> ya");
        assert_eq!(said("Hmm... oke"), "hmm | oke");
    }

    #[test]
    fn letters_become_sounds() {
        assert_eq!(ph("bangun"), "bˈɑŋun");
        assert_eq!(ph("nyanyi"), "njˈɑnji");
        assert_eq!(ph("syukur"), "ʃˈukuɹ");
        assert_eq!(ph("cinta"), "ʧˈintɑ");
        assert_eq!(ph("jalan"), "ʤˈɑlɑn");
        assert_eq!(ph("khusus"), "hˈusus");
        assert_eq!(ph("pantai"), "pˈɑntI");
        assert_eq!(ph("pulau"), "pˈulW");
        assert_eq!(ph("main"), "mˈɑin");
        assert_eq!(ph("tangga"), "tˈɑŋɡɑ");
        assert_eq!(ph("Allah"), "ˈɑlɑh");
    }

    #[test]
    fn the_letter_e() {
        // ə by default
        assert_eq!(ph("besar"), "bəsˈɑɹ");
        // é: written, word-final, before a final h or k, or a listed word
        assert_eq!(ph("bébas"), "bˈɛbɑs");
        assert_eq!(ph("sate"), "sˈɑtɛ");
        assert_eq!(ph("boleh"), "bˈOlɛh");
        assert_eq!(ph("jelek"), "ʤəlˈɛk");
        assert_eq!(ph("enak"), "ˈɛnɑk");
        assert_eq!(ph("meja"), "mˈɛʤɑ");
        // a listed word marks only the é it has
        assert_eq!(ph("mereka"), "məɹˈɛkɑ");
        assert_eq!(ph("persen"), "pəɹsˈɛn");
        assert_eq!(ph("nenek"), "nˈɛnɛk");
    }

    #[test]
    fn stress_is_on_the_second_to_last_syllable_unless_it_is_schwa() {
        assert_eq!(ph("rumah"), "ɹˈumɑh");
        assert_eq!(ph("pemerintah"), "pəməɹˈintɑh");
        // the second-to-last vowel is ə: stress the last
        assert_eq!(ph("kecil"), "kəʧˈil");
        assert_eq!(ph("tenang"), "tənˈɑŋ");
        // one syllable: none
        assert_eq!(ph("dan"), "dɑn");
        assert_eq!(ph("yang"), "jɑŋ");
    }

    #[test]
    fn overrides_respell_or_give_raw_phonemes() {
        let mut o = Overrides::default();
        o.0.insert("telepon".into(), "télépon".into());
        o.0.insert("wifi".into(), "/wˈIfI/".into());
        o.0.insert("bad".into(), "/not kokoro!/".into());
        let c = phonemize("Telepon wifi bad", &o).unwrap();
        assert_eq!(c, vec![Chunk::Phonemes("tɛlˈɛpOn wˈIfI bɑd".into())]);
    }

    #[test]
    fn output_is_kokoro_symbols_in_bounded_chunks() {
        let script = "Selamat pagi, Indonesia! Hari ini kita belajar tentang air putih: 75% tubuh kita adalah air. \
            [pause 500ms] Minumlah 8 gelas sehari, ya? Harga sebotol Rp 3.500. Ke-3 tips berikutnya ada di bawah… \
            ÉKSPOR, Jl. Sudirman No. 12, Jakarta. 中文 😀 #@$ ";
        let long = format!("{script} {}", "kata yang sangat panjang sekali ".repeat(60));
        for text in [script, long.as_str()] {
            let chunks = phonemize(text, &Overrides::default()).unwrap();
            assert!(!chunks.is_empty());
            for c in &chunks {
                if let Chunk::Phonemes(p) = c {
                    assert!(p.chars().count() <= MAX_CHUNK_CHARS, "{p}");
                    assert!(p.chars().all(|c| SYMBOLS.contains(c)), "symbol outside Kokoro's set in {p:?}");
                }
            }
        }
    }

    #[test]
    fn hostile_input_never_panics() {
        assert_eq!(phonemize(&"a".repeat(MAX_INPUT + 1), &Overrides::default()), Err(NormalizeError::TooLong));
        for text in [
            "",
            "   ",
            "[pause",
            "[pause 99999s]",
            "Rp",
            "Rp.",
            "ke-",
            "ke-99999999999999999999",
            "1.",
            "1,",
            ".,.,",
            "%%%",
            "9".repeat(5000).as_str(),
            "x".repeat(100).as_str(),
            "ÉÉÉ",
        ] {
            let _ = phonemize(text, &Overrides::default()).unwrap();
        }
        assert!(word_phonemes("").is_none());
        assert!(word_phonemes("123").is_none());
    }
}
