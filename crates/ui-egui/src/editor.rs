//! In-cell / formula-bar editing state: point mode, reference colouring, function
//! autocomplete and argument hints.

use egui::Color32;
use gridcraft_engine::core::{CellRef, RangeRef};

/// Colours for references in a formula being edited (our own palette).
pub const REF_COLORS: [Color32; 8] = [
    Color32::from_rgb(0x2E, 0x6F, 0xD8),
    Color32::from_rgb(0xD0, 0x3C, 0x3C),
    Color32::from_rgb(0x7A, 0x4F, 0xC9),
    Color32::from_rgb(0x2B, 0x9A, 0x4A),
    Color32::from_rgb(0xC2, 0x5E, 0x1E),
    Color32::from_rgb(0x1D, 0x92, 0xA6),
    Color32::from_rgb(0xB8, 0x3B, 0x8E),
    Color32::from_rgb(0x8A, 0x7A, 0x1F),
];

#[derive(Clone, Debug)]
pub struct EditState {
    pub sheet: usize,
    /// Syntax of this edit, captured when it starts (prevents a language switch reinterpreting it).
    pub locale: gridcraft_engine::formula::FormulaLocale,
    pub cell: CellRef,
    pub text: String,
    /// Started by typing (Enter mode: arrows commit) rather than F2 (Edit mode: arrows move the caret).
    pub enter_mode: bool,
    pub from_formula_bar: bool,
    /// Caret position (char index), mirrored from the text widget.
    pub caret: usize,
    /// The reference most recently inserted by point mode (byte range) — arrows/clicks replace it.
    pub point: Option<(usize, usize)>,
    /// Where point mode's keyboard cursor is.
    pub point_cell: Option<CellRef>,
    pub autocomplete: Vec<String>,
    /// Excel-style value list for the argument under the caret: (value, label, tip). Shares
    /// `ac_index` with `autocomplete`; only one of them is non-empty.
    pub arg_choices: Vec<(String, String, String)>,
    pub ac_index: usize,
    pub request_focus: bool,
    /// The text changed programmatically: push `caret` into the widget on the next frame.
    pub sync_caret: bool,
    /// Column AutoComplete: the full entry the typed text completes to.
    pub completion: Option<String>,
    /// Move the caret to the end on the next frame.
    pub caret_to_end: bool,
}

impl EditState {
    pub fn new(sheet: usize, cell: CellRef, text: String, enter_mode: bool, from_formula_bar: bool) -> EditState {
        let caret = text.chars().count();
        EditState {
            sheet,
            locale: Default::default(),
            cell,
            text,
            enter_mode,
            from_formula_bar,
            caret,
            point: None,
            point_cell: None,
            autocomplete: vec![],
            arg_choices: vec![],
            ac_index: 0,
            request_focus: true,
            sync_caret: true,
            caret_to_end: true,
            completion: None,
        }
    }

    pub fn is_formula(&self) -> bool {
        self.text.starts_with('=')
            || (self.text.starts_with('+') && self.text.len() > 1)
            || (self.text.starts_with('-') && self.text.len() > 1 && !self.text[1..].starts_with(|c: char| c.is_ascii_digit()))
    }

    fn byte_caret(&self) -> usize {
        self.text.char_indices().nth(self.caret).map(|(i, _)| i).unwrap_or(self.text.len())
    }

    /// Point mode: the caret follows an operator, `(`, `,` or `=` in a formula, so clicking a
    /// cell or pressing an arrow inserts a reference.
    pub fn can_point(&self) -> bool {
        if !self.is_formula() {
            return false;
        }
        if let Some((_, e)) = self.point
            && e == self.byte_caret()
        {
            return true;
        }
        let before = self.text.get(..self.byte_caret()).unwrap_or("");
        let last = before.trim_end().chars().last();
        matches!(last, Some('=' | '(' | ',' | '+' | '-' | '*' | '/' | '^' | '&' | '<' | '>' | ':' | ';' | '{'))
    }

    /// Inserts (or replaces the last pointed) reference text at the caret.
    pub fn insert_ref(&mut self, r: &str) {
        let caret_b = self.byte_caret();
        let (start, end) = match self.point {
            Some((s, e)) if e == caret_b => (s, e),
            _ => (caret_b, caret_b),
        };
        let (Some(a), Some(b)) = (self.text.get(..start), self.text.get(end..)) else { return };
        self.text = format!("{a}{r}{b}");
        let new_end = start + r.len();
        self.point = Some((start, new_end));
        self.caret = self.text.get(..new_end).map(|s| s.chars().count()).unwrap_or(0);
        self.caret_to_end = false;
        self.sync_caret = true;
    }

    /// Updates function autocomplete for the word before the caret.
    pub fn update_autocomplete(&mut self, names: &[String]) {
        self.autocomplete.clear();
        if !self.is_formula() {
            return;
        }
        let before = self.text.get(..self.byte_caret()).unwrap_or("");
        let word: String =
            before.chars().rev().take_while(|c| c.is_alphanumeric() || *c == '.' || *c == '_').collect::<String>().chars().rev().collect();
        if word.is_empty() || word.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            return;
        }
        let u = word.to_uppercase();
        // Not after a column letter of a reference like A1 (digits follow) – fine either way.
        // Alphabetical by the shown (possibly localized) name, like Excel.
        let mut matches: Vec<String> = names.iter().filter(|n| n.starts_with(&u)).cloned().collect();
        let fold = |s: &str| -> String {
            s.chars()
                .map(|c| match c {
                    'Á' | 'À' | 'Â' | 'Ã' => 'A',
                    'É' | 'Ê' => 'E',
                    'Í' => 'I',
                    'Ó' | 'Ô' | 'Õ' => 'O',
                    'Ú' | 'Ü' => 'U',
                    'Ç' => 'C',
                    c => c,
                })
                .collect()
        };
        matches.sort_by_cached_key(|n| (fold(n), n.clone()));
        matches.dedup();
        matches.truncate(12);
        self.autocomplete = matches;
        if self.autocomplete.len() == 1 && self.autocomplete[0] == u {
            self.autocomplete.clear();
        }
        self.ac_index = self.ac_index.min(self.autocomplete.len().saturating_sub(1));
    }

    /// Accepts the highlighted autocomplete entry: replaces the word and adds `(`.
    pub fn accept_autocomplete(&mut self) -> bool {
        let Some(name) = self.autocomplete.get(self.ac_index).cloned() else { return false };
        let caret_b = self.byte_caret();
        let before = self.text.get(..caret_b).unwrap_or("");
        let wlen: usize = before.chars().rev().take_while(|c| c.is_alphanumeric() || *c == '.' || *c == '_').map(char::len_utf8).sum();
        let start = caret_b - wlen;
        let (Some(a), Some(b)) = (self.text.get(..start), self.text.get(caret_b..)) else { return false };
        self.text = format!("{a}{name}({b}");
        self.caret = self.text.get(..start + name.len() + 1).map(|s| s.chars().count()).unwrap_or(0);
        self.autocomplete.clear();
        self.caret_to_end = false;
        self.sync_caret = true;
        true
    }

    /// Offers the fixed values of the argument under the caret (XLOOKUP's match mode, ...),
    /// filtered by what is already typed in that argument.
    pub fn update_arg_choices(&mut self) {
        let previous = std::mem::take(&mut self.arg_choices);
        if !self.autocomplete.is_empty() || self.locale != gridcraft_engine::formula::FormulaLocale::PtBr {
            return;
        }
        let Some((name, arg)) = self.current_function() else { return };
        let list = crate::formula_arg_choices::choices(&self.locale.canonical_name(&name).to_ascii_uppercase(), arg);
        let Some(start) = self.current_arg_start() else { return };
        let typed = self.text.get(start..self.byte_caret()).unwrap_or("").trim().to_uppercase();
        if list.iter().any(|(v, ..)| v.eq_ignore_ascii_case(&typed)) && !typed.is_empty() {
            return;
        }
        self.arg_choices = list
            .iter()
            .filter(|(v, ..)| v.to_uppercase().starts_with(&typed))
            .map(|(v, l, t)| (v.to_string(), l.to_string(), t.to_string()))
            .collect();
        if self.arg_choices != previous {
            self.ac_index = 0;
        }
    }

    /// Replaces the argument under the caret with the highlighted value from `arg_choices`.
    pub fn accept_arg_choice(&mut self) -> bool {
        let Some((value, ..)) = self.arg_choices.get(self.ac_index).cloned() else { return false };
        let Some(start) = self.current_arg_start() else { return false };
        let caret_b = self.byte_caret();
        let (Some(a), Some(b)) = (self.text.get(..start), self.text.get(caret_b..)) else { return false };
        self.text = format!("{a}{value}{b}");
        self.caret = self.text.get(..start + value.len()).map(|s| s.chars().count()).unwrap_or(0);
        self.arg_choices.clear();
        self.caret_to_end = false;
        self.sync_caret = true;
        true
    }

    /// Byte offset where the argument under the caret starts: after its `(` or separator.
    fn current_arg_start(&self) -> Option<usize> {
        let before = self.text.get(..self.byte_caret())?;
        let mut depth = 0i32;
        let mut in_str = false;
        for (i, c) in before.char_indices().rev() {
            if c == '"' {
                in_str = !in_str;
                continue;
            }
            if in_str {
                continue;
            }
            match c {
                ')' | '}' | ']' => depth += 1,
                '{' | '[' => depth -= 1,
                '(' if depth == 0 => return Some(i + 1),
                '(' => depth -= 1,
                c if c == self.locale.list_separator() && depth == 0 => return Some(i + c.len_utf8()),
                _ => {}
            }
        }
        None
    }

    /// The function call the caret is inside and the argument index (for the hint tooltip).
    pub fn current_function(&self) -> Option<(String, usize)> {
        if !self.is_formula() {
            return None;
        }
        let before = self.text.get(..self.byte_caret())?;
        let mut depth = 0i32;
        let mut arg = 0usize;
        let chars: Vec<char> = before.chars().collect();
        let mut in_str = false;
        let mut i = chars.len();
        while i > 0 {
            i -= 1;
            let c = chars[i];
            if c == '"' {
                in_str = !in_str;
                continue;
            }
            if in_str {
                continue;
            }
            match c {
                ')' | '}' | ']' => depth += 1,
                '{' | '[' => depth -= 1,
                '(' => {
                    if depth == 0 {
                        let name: String = chars[..i]
                            .iter()
                            .rev()
                            .take_while(|c| c.is_alphanumeric() || **c == '.' || **c == '_')
                            .collect::<String>()
                            .chars()
                            .rev()
                            .collect();
                        if name.is_empty() {
                            return None;
                        }
                        return Some((name.to_uppercase(), arg));
                    }
                    depth -= 1;
                }
                c if c == self.locale.list_separator() && depth == 0 => arg += 1,
                _ => {}
            }
        }
        None
    }
}

/// References in formula text with their byte spans and colour index (same reference text ⇒
/// same colour), for colouring the editor and outlining ranges on the grid.
pub fn formula_refs(text: &str) -> Vec<(usize, usize, RangeRef, Option<String>, usize)> {
    let body = text.strip_prefix('=').or_else(|| text.strip_prefix('+')).unwrap_or(text);
    let offset = text.len() - body.len();
    let Ok(toks) = gridcraft_engine::formula::lexer::tokenize(body) else { return vec![] };
    let mut out: Vec<(usize, usize, RangeRef, Option<String>, usize)> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    let mut i = 0;
    while i < toks.len() {
        use gridcraft_engine::formula::lexer::Tok;
        let (sheet, start, j) = match &toks[i].tok {
            Tok::Sheet(a, _) => (Some(a.clone()), toks[i].start, i + 1),
            _ => (None, toks[i].start, i),
        };
        // Whole columns (`A:B`, `$A:$C`) and whole rows (`1:3`) lex as two words or numbers
        // around a colon.
        if let (Some(Tok::Word(_) | Tok::Number(_)), Some(Tok::Colon), Some(Tok::Word(_) | Tok::Number(_))) =
            (toks.get(j).map(|t| &t.tok), toks.get(j + 1).map(|t| &t.tok), toks.get(j + 2).map(|t| &t.tok))
            && let Some(range) = body.get(toks[j].start..toks[j + 2].end).and_then(RangeRef::parse)
            && (range.is_full_cols() || range.is_full_rows())
        {
            let end = toks[j + 2].end;
            let key = body.get(start..end).unwrap_or("").replace('$', "").to_ascii_uppercase();
            let idx = match seen.iter().position(|k| *k == key) {
                Some(k) => k,
                None => {
                    seen.push(key);
                    seen.len() - 1
                }
            };
            out.push((start + offset, end + offset, range, sheet, idx % REF_COLORS.len()));
            i = j + 3;
            continue;
        }
        if let Some(Tok::Word(w)) = toks.get(j).map(|t| &t.tok)
            && let Some(c) = CellRef::parse(w)
        {
            let mut end = toks[j].end;
            let mut range = RangeRef::cell(c);
            if let (Some(Tok::Colon | Tok::Trim(_)), Some(Tok::Word(w2))) = (toks.get(j + 1).map(|t| &t.tok), toks.get(j + 2).map(|t| &t.tok))
                && let Some(c2) = CellRef::parse(w2)
            {
                range = RangeRef::new(c, c2);
                end = toks[j + 2].end;
                i = j + 3;
            } else {
                i = j + 1;
            }
            let key = body.get(start..end).unwrap_or("").replace('$', "").to_ascii_uppercase();
            let idx = match seen.iter().position(|k| *k == key) {
                Some(k) => k,
                None => {
                    seen.push(key);
                    seen.len() - 1
                }
            };
            out.push((start + offset, end + offset, range, sheet, idx % REF_COLORS.len()));
            continue;
        }
        i += 1;
    }
    out
}

/// Excel-style AutoComplete: the one distinct text entry in the same column (contiguous data
/// above and below) that starts with `typed` (case-insensitive). `None` when ambiguous.
pub fn column_completion(sheet: &gridcraft_engine::model::Sheet, at: CellRef, typed: &str) -> Option<String> {
    if typed.is_empty() || typed.starts_with('=') || typed.chars().all(|c| c.is_ascii_digit() || c == '.' || c == '-') {
        return None;
    }
    let lower = typed.to_lowercase();
    let mut found: Option<String> = None;
    let mut ambiguous = false;
    let mut scan = |row: u32| -> bool {
        match sheet.value(CellRef::new(row, at.col)) {
            gridcraft_engine::core::Value::Empty => false,
            gridcraft_engine::core::Value::Text(t) => {
                let folded = t.to_lowercase();
                if folded.starts_with(&lower) && folded.len() > lower.len() {
                    match &found {
                        Some(f) if f.to_lowercase() != folded => ambiguous = true,
                        None => found = Some(t.to_string()),
                        _ => {}
                    }
                }
                true
            }
            _ => true,
        }
    };
    let mut r = at.row;
    let mut n = 0;
    while r > 0 && n < 5000 {
        r -= 1;
        n += 1;
        if !scan(r) {
            break;
        }
    }
    let mut r = at.row + 1;
    n = 0;
    while n < 5000 && scan(r) {
        r += 1;
        n += 1;
    }
    if ambiguous { None } else { found }
}

/// The part of `full` after the `typed` prefix, counted in characters: case folding can change
/// a character's byte length ("ß" is two bytes, "ẞ" three).
pub fn completion_rest<'a>(full: &'a str, typed: &str) -> Option<&'a str> {
    let start = full.char_indices().nth(typed.chars().count())?.0;
    full.get(start..)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn point_mode() {
        let mut e = EditState::new(0, CellRef::new(0, 0), "=SUM(".into(), true, false);
        assert!(e.can_point());
        e.insert_ref("A1");
        assert_eq!(e.text, "=SUM(A1");
        e.insert_ref("A2");
        assert_eq!(e.text, "=SUM(A2");
        e.text.push(':');
        e.caret += 1;
        e.point = None;
        assert!(e.can_point());
        let e2 = EditState::new(0, CellRef::new(0, 0), "hello".into(), true, false);
        assert!(!e2.can_point());
    }

    #[test]
    fn autocomplete_and_hints() {
        let names: Vec<String> = ["SUM", "SUMIF", "SUMPRODUCT", "AVERAGE"].iter().map(|s| s.to_string()).collect();
        let mut e = EditState::new(0, CellRef::new(0, 0), "=su".into(), true, false);
        e.update_autocomplete(&names);
        assert_eq!(e.autocomplete.len(), 3);
        assert!(e.accept_autocomplete());
        assert_eq!(e.text, "=SUM(");
        let e = EditState::new(0, CellRef::new(0, 0), "=IF(A1>0,SUM(B1,".into(), true, false);
        assert_eq!(e.current_function(), Some(("SUM".into(), 1)));
    }

    #[test]
    fn autocomplete_and_hints_accept_accented_names() {
        let names: Vec<String> = ["MÉDIA", "MÉDIASE", "ENDEREÇO"].iter().map(|s| s.to_string()).collect();
        let mut e = EditState::new(0, CellRef::new(0, 0), "=mé".into(), true, false);
        e.update_autocomplete(&names);
        assert_eq!(e.autocomplete.len(), 2);
        assert!(e.accept_autocomplete());
        assert_eq!(e.text, "=MÉDIA(");
        let names: Vec<String> = ["PROPRIEDADEMEMBROCUBO", "PROCURAR", "PROCV", "PROB", "PROJ.LIN"].iter().map(|s| s.to_string()).collect();
        let mut e = EditState::new(0, CellRef::new(0, 0), "=pro".into(), true, false);
        e.update_autocomplete(&names);
        assert_eq!(e.autocomplete, ["PROB", "PROCURAR", "PROCV", "PROJ.LIN", "PROPRIEDADEMEMBROCUBO"]);
        let mut e = EditState::new(0, CellRef::new(0, 0), "=ENDEREÇO(1;".into(), true, false);
        e.locale = gridcraft_engine::formula::FormulaLocale::PtBr;
        assert_eq!(e.current_function(), Some(("ENDEREÇO".into(), 1)));
    }

    #[test]
    fn argument_value_lists_like_excel() {
        let pt = gridcraft_engine::formula::FormulaLocale::PtBr;
        let mut e = EditState::new(0, CellRef::new(0, 0), "=PROCX(D2;A:A;B:B;0;".into(), true, false);
        e.locale = pt;
        e.update_arg_choices();
        let values: Vec<&str> = e.arg_choices.iter().map(|c| c.0.as_str()).collect();
        assert_eq!(values, ["0", "-1", "1", "2", "3"]);
        assert_eq!(e.arg_choices[0].1, "Correspondência exata");
        e.ac_index = 1;
        assert!(e.accept_arg_choice());
        assert_eq!(e.text, "=PROCX(D2;A:A;B:B;0;-1");
        // Typing filters; a complete value closes the list.
        let mut e = EditState::new(0, CellRef::new(0, 0), "=PROCV(D2;A:B;2;f".into(), true, false);
        e.locale = pt;
        e.update_arg_choices();
        assert_eq!(e.arg_choices.len(), 1);
        assert!(e.accept_arg_choice());
        assert_eq!(e.text, "=PROCV(D2;A:B;2;FALSO");
        e.update_arg_choices();
        assert!(e.arg_choices.is_empty());
        // Nested calls count their own arguments.
        let mut e = EditState::new(0, CellRef::new(0, 0), "=PROCX(SOMA(1;2);A:A;B:B;".into(), true, false);
        e.locale = pt;
        e.update_arg_choices();
        assert!(e.arg_choices.is_empty());
    }

    #[test]
    fn whole_column_and_row_refs_are_coloured() {
        let r = formula_refs("=PROCV(D2;A:B;2;FALSO)+SOMA($C:$C;1:3)");
        let texts: Vec<String> = r.iter().map(|x| x.2.a1()).collect();
        assert_eq!(texts, ["D2", "A:B", "C:C", "1:3"]);
        assert!(r[1].2.is_full_cols() && r[3].2.is_full_rows());
        assert_ne!(r[0].4, r[1].4);
    }

    #[test]
    fn ref_spans() {
        let r = formula_refs("=A1+B2:C3*a1+Sheet2!D4");
        assert_eq!(r.len(), 4);
        assert_eq!(r[0].4, r[2].4); // A1 and a1 share a colour
        assert_eq!(r[1].2.a1(), "B2:C3");
        assert_eq!(r[3].3.as_deref(), Some("Sheet2"));
    }
}
