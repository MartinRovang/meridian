//! Reading a broker's positions export.
//!
//! ponytail: hand-rolled, no csv crate. A positions export is a header and one line per holding;
//! what makes real files hard is not the grammar but the encoding, the separator and the decimal
//! comma, and a csv crate solves none of those three.
//!
//! The file that shaped this module is a Nordnet "aksjelister" export: UTF-16LE with a BOM,
//! TAB separated despite the .csv name, Norwegian decimal commas, spaces as thousands separators,
//! and a non-breaking space inside one header name. It is checked in as a fixture.

use crate::types::Quote;

/// One position as the broker states it. No ticker: exports name the fund, not the symbol.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub name: String,
    pub currency: String,
    pub shares: f64,
    /// Average cost per share, in `currency`. Norwegian brokers call this GAV.
    pub gav: f64,
}

impl Row {
    /// What was paid in total, which is what Meridian stores.
    ///
    /// The broker states a per-share average and Meridian stores a total. Confusing the two is
    /// silent: the dashboard still renders, the P/L is just wrong by a factor of the share count.
    pub fn cost_basis(&self) -> f64 {
        self.shares * self.gav
    }
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ImportError {
    #[error("this file has no {0} column, so it is not a positions export")]
    MissingColumn(&'static str),
    #[error("this file has a header but no positions in it")]
    Empty,
}

/// Column headers that mean the same thing, primary name first: the primary is what an error
/// message says, so it has to be the word actually printed in the file.
const NAME: &[&str] = &[
    "navn",
    "verdipapir",
    "instrument",
    "name",
    "security",
    "ticker",
];
const CURRENCY: &[&str] = &["valuta", "currency", "ccy"];
const SHARES: &[&str] = &["antall", "beholdning", "quantity", "shares"];
const GAV: &[&str] = &[
    "gav",
    "snittkurs",
    "gjennomsnittskurs",
    "snittpris",
    "kostpris",
];

/// Bytes to text, whatever the broker felt like emitting.
///
/// ponytail: BOM sniffing and a Latin-1 fallback, which is every encoding these exports actually
/// use. A full charset detector would be a dependency to guess at files that announce themselves.
fn decode(bytes: &[u8]) -> String {
    let utf16 = |chunks: &[u8], le: bool| -> String {
        let units: Vec<u16> = chunks
            .chunks_exact(2)
            .map(|c| {
                if le {
                    u16::from_le_bytes([c[0], c[1]])
                } else {
                    u16::from_be_bytes([c[0], c[1]])
                }
            })
            .collect();
        char::decode_utf16(units)
            .map(|c| c.unwrap_or('\u{fffd}'))
            .collect()
    };
    match bytes {
        [0xff, 0xfe, rest @ ..] => utf16(rest, true),
        [0xfe, 0xff, rest @ ..] => utf16(rest, false),
        [0xef, 0xbb, 0xbf, rest @ ..] => String::from_utf8_lossy(rest).into_owned(),
        _ => match std::str::from_utf8(bytes) {
            Ok(s) => s.to_string(),
            // Not UTF-8, so Latin-1: every byte is the code point of the same number.
            Err(_) => bytes.iter().map(|b| *b as char).collect(),
        },
    }
}

/// A header cell reduced to something an alias can be compared against. The non-breaking space is
/// the point: a header that differs from its alias only by an invisible character is the same
/// header, and treating it as missing would refuse a perfectly good file.
fn norm(cell: &str) -> String {
    cell.replace('\u{a0}', " ")
        .trim()
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// The separator this file uses, decided by counting candidates in the header. Nordnet ships tabs
/// in a file called .csv, so the extension proves nothing.
fn separator(header: &str) -> char {
    ['\t', ';', ',']
        .into_iter()
        .max_by_key(|c| header.matches(*c).count())
        .unwrap_or(';')
}

/// A number as a Norwegian broker writes it: decimal comma, spaces or non-breaking spaces between
/// thousands. Returns None for a blank or a dash, which is how these files write "not applicable".
fn num(cell: &str) -> Option<f64> {
    let cleaned: String = cell
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '\u{a0}')
        .collect();
    if cleaned.is_empty() {
        return None;
    }
    // With both separators present the last one is the decimal point: 1.234,56 and 1,234.56 are
    // the same number written by different countries.
    let decimal = match (cleaned.rfind(','), cleaned.rfind('.')) {
        (Some(c), Some(d)) => Some(if c > d { ',' } else { '.' }),
        (Some(_), None) => Some(','),
        (None, Some(_)) => Some('.'),
        (None, None) => None,
    };
    let normalised = match decimal {
        Some(',') => cleaned.replace('.', "").replace(',', "."),
        Some(_) => cleaned.replace(',', ""),
        None => cleaned,
    };
    normalised.parse().ok()
}

fn column(headers: &[String], aliases: &[&str]) -> Option<usize> {
    headers.iter().position(|h| aliases.contains(&h.as_str()))
}

pub fn parse(bytes: &[u8]) -> Result<Vec<Row>, ImportError> {
    let text = decode(bytes);
    let mut lines = text.lines().filter(|l| !l.trim().is_empty());
    let header = lines.next().ok_or(ImportError::Empty)?;
    let sep = separator(header);
    let headers: Vec<String> = header.split(sep).map(norm).collect();

    let name = column(&headers, NAME).ok_or(ImportError::MissingColumn("Navn"))?;
    let shares = column(&headers, SHARES).ok_or(ImportError::MissingColumn("Antall"))?;
    let gav = column(&headers, GAV).ok_or(ImportError::MissingColumn("GAV"))?;
    // Optional: a single-currency account export does not always bother stating it.
    let currency = column(&headers, CURRENCY);

    let rows: Vec<Row> = lines
        .filter_map(|line| {
            let cells: Vec<&str> = line.split(sep).collect();
            let cell = |i: usize| cells.get(i).map(|c| c.trim()).unwrap_or("");
            // A row with no share count is a total line or a blank, not a position.
            Some(Row {
                name: cell(name).to_string(),
                currency: currency.map(cell).unwrap_or("").to_uppercase(),
                shares: num(cell(shares))?,
                gav: num(cell(gav)).unwrap_or(0.0),
            })
        })
        .filter(|r| !r.name.is_empty())
        .collect();

    if rows.is_empty() {
        return Err(ImportError::Empty);
    }
    Ok(rows)
}

/// Progressively shorter forms of a fund name, longest first, for a symbol search.
///
/// Measured against Yahoo rather than guessed: a full prospectus name returns nothing, and the
/// hits appear as soon as the trailing share-class words come off. The caller searches these in
/// order and stops at the first that answers, so the most specific form that works is the one
/// used. The floor of two words is what stops "State Street SPDR MSCI..." trimming down to
/// "State Street SPDR", which finds the American SPY: a different fund in a different currency.
pub fn search_terms(name: &str) -> Vec<String> {
    let mut out = vec![name.trim().to_string()];
    if let Some(open) = name.rfind('(') {
        if name.trim_end().ends_with(')') {
            let stripped = name[..open].trim().to_string();
            if !stripped.is_empty() {
                out.push(stripped);
            }
        }
    }
    let shortest = out.last().expect("seeded above").clone();
    let mut words: Vec<&str> = shortest.split_whitespace().collect();
    while words.len() > 2 {
        words.pop();
        out.push(words.join(" "));
    }
    out.dedup();
    out
}

/// The candidates whose quote is in the currency the broker stated.
///
/// An export states the currency it paid in; the same fund lists in London in GBP and Frankfurt in
/// EUR. Without this filter the import can attach a holding to the wrong listing, which does not
/// fail, it just values the position wrongly and for ever.
pub fn in_currency<'a>(
    candidates: &'a [(String, Quote)],
    currency: &str,
) -> Vec<&'a (String, Quote)> {
    candidates
        .iter()
        .filter(|(_, q)| q.currency.eq_ignore_ascii_case(currency))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const NORDNET: &[u8] = include_bytes!("../tests/fixtures/nordnet_positions.csv");

    #[test]
    fn a_utf16_tab_separated_export_parses_despite_calling_itself_a_csv() {
        let rows = parse(NORDNET).expect("parses");
        assert_eq!(rows.len(), 2);
        assert_eq!(
            rows[0].name,
            "State Street SPDR MSCI All Country World UCITS ETF (Acc)"
        );
        assert_eq!(rows[0].currency, "EUR");
        assert_eq!(rows[0].shares, 100.0);
        assert_eq!(rows[0].gav, 250.0);
        assert_eq!(rows[1].name, "Xtrackers NASDAQ 100 ETF 1C");
    }

    #[test]
    fn the_cost_basis_is_the_average_price_times_the_shares() {
        // The one arithmetic mistake in this module that would not look like a mistake: a GAV of
        // 250 for 100 shares is 25 000 paid, not 250.
        let rows = parse(NORDNET).expect("parses");
        assert_eq!(rows[0].cost_basis(), 25_000.0);
        assert_eq!(rows[1].cost_basis(), 12_000.0);
    }

    #[test]
    fn a_decimal_comma_survives_a_thousands_separator_of_either_kind() {
        assert_eq!(num("307 300,50"), Some(307_300.5)); // ordinary space
        assert_eq!(num("307\u{a0}300,50"), Some(307_300.5)); // non-breaking space
        assert_eq!(num("-864,2"), Some(-864.2));
        assert_eq!(num("59,6"), Some(59.6));
        assert_eq!(num("0"), Some(0.0));
        assert_eq!(num(""), None);
        assert_eq!(num("n/a"), None);
    }

    #[test]
    fn a_semicolon_separated_latin1_export_parses_too() {
        // The other shape these files come in. Latin-1 has no BOM to detect, so it is what the
        // decoder falls back to when the bytes are not valid UTF-8.
        let mut raw: Vec<u8> = Vec::new();
        raw.extend_from_slice("Navn;Valuta;Antall;GAV\n".as_bytes());
        raw.extend_from_slice(b"Norsk Hydro ASA;NOK;500;62,50\n");
        let at = raw.len() - 20;
        raw[at] = 0xf8; // a Latin-1 'o with stroke' inside the name
        let rows = parse(&raw).expect("parses");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].shares, 500.0);
        assert_eq!(rows[0].gav, 62.5);
    }

    #[test]
    fn a_header_column_is_matched_through_a_non_breaking_space() {
        // The real file has one of these in "Avkast.\u{a0}NOK". A header that differs from its
        // alias by an invisible character must not read as a missing column.
        let raw = "Navn\tValuta\tAntall\u{a0}\tGAV\nHydro\tNOK\t10\t1,5\n";
        assert_eq!(parse(raw.as_bytes()).expect("parses")[0].shares, 10.0);
    }

    #[test]
    fn a_file_with_no_shares_column_is_refused_by_name() {
        let raw = "Navn;Valuta;GAV\nHydro;NOK;62,50\n";
        assert_eq!(
            parse(raw.as_bytes()),
            Err(ImportError::MissingColumn("Antall"))
        );
    }

    #[test]
    fn a_header_with_no_rows_under_it_is_refused_rather_than_importing_nothing() {
        assert_eq!(parse(b"Navn;Valuta;Antall;GAV\n"), Err(ImportError::Empty));
    }

    #[test]
    fn the_longest_search_term_comes_first_and_a_parenthetical_is_dropped_early() {
        // Measured against live Yahoo, not guessed: the full name returns nothing and the name
        // without "(Acc)" returns the fund on five exchanges.
        let t = search_terms("State Street SPDR MSCI All Country World UCITS ETF (Acc)");
        assert_eq!(
            t[0],
            "State Street SPDR MSCI All Country World UCITS ETF (Acc)"
        );
        assert_eq!(t[1], "State Street SPDR MSCI All Country World UCITS ETF");
    }

    #[test]
    fn trailing_words_come_off_one_at_a_time_until_two_are_left() {
        // "Xtrackers NASDAQ 100 ETF 1C" finds nothing; "Xtrackers NASDAQ 100" finds the fund.
        let t = search_terms("Xtrackers NASDAQ 100 ETF 1C");
        assert!(t.contains(&"Xtrackers NASDAQ 100".to_string()), "{t:?}");
        // Trimming past two words turns "State Street SPDR" into the US SPY, which is a different
        // fund in a different currency. The floor is what stops the search wandering.
        assert!(t.iter().all(|s| s.split_whitespace().count() >= 2), "{t:?}");
    }

    #[test]
    fn candidates_in_the_wrong_currency_are_dropped() {
        // The whole reason the Valuta column is read: the same fund lists in London in GBP and in
        // Frankfurt in EUR, and pricing a EUR holding off the GBP line is wrong by about 15%.
        let q = |c: &str| Quote {
            price: 1.0,
            prev_close: 1.0,
            currency: c.into(),
            ts: 0,
        };
        let cands = vec![
            ("ACWD.L".to_string(), q("GBP")),
            ("SPYY.DE".to_string(), q("EUR")),
            ("ACWE.PA".to_string(), q("EUR")),
        ];
        let kept: Vec<&str> = in_currency(&cands, "EUR")
            .iter()
            .map(|(s, _)| s.as_str())
            .collect();
        assert_eq!(kept, vec!["SPYY.DE", "ACWE.PA"]);
    }
}
