//! The shipped market list the discovery search draws candidates from.
//!
//! Derived from a Euronext Oslo equities export of 2025-10-05, filtered to the main list and
//! ranked by that day's turnover, plus the Stockholm and Copenhagen large caps by hand because
//! Euronext does not carry either exchange.
//!
//! ponytail: a static array, not a downloaded index. A list of a hundred and some symbols is
//! four kilobytes of source that cannot fail at runtime, and the alternative is a scraper that
//! breaks silently. It goes stale instead, which is visible: a delisted symbol has no history,
//! so it is reported as unusable rather than quietly skipped.
//!
//! One day's turnover is a thin liquidity proxy. It is the one the export carries, and the point
//! of the filter is only to keep the search out of names whose stale closes would understate
//! their real volatility.

pub const OSLO: &str = "Oslo Børs";
pub const STOCKHOLM: &str = "Stockholm";
pub const COPENHAGEN: &str = "Copenhagen";

pub struct Listing {
    pub symbol: &'static str,
    pub name: &'static str,
    pub market: &'static str,
}

const fn l(symbol: &'static str, name: &'static str, market: &'static str) -> Listing {
    Listing {
        symbol,
        name,
        market,
    }
}

pub const LISTINGS: &[Listing] = &[
    l("KOG.OL", "Kongsberg Gruppen", OSLO),
    l("EQNR.OL", "Equinor", OSLO),
    l("DNB.OL", "Dnb Bank", OSLO),
    l("FRO.OL", "Frontline", OSLO),
    l("AKRBP.OL", "Aker Bp", OSLO),
    l("NHY.OL", "Norsk Hydro", OSLO),
    l("TOM.OL", "Tomra Systems", OSLO),
    l("TEL.OL", "Telenor", OSLO),
    l("MOWI.OL", "Mowi", OSLO),
    l("YAR.OL", "Yara International", OSLO),
    l("VAR.OL", "Vår Energi", OSLO),
    l("VENDB.OL", "Vend Ser. B", OSLO),
    l("ORK.OL", "Orkla", OSLO),
    l("SALM.OL", "Salmar", OSLO),
    l("NOD.OL", "Nordic Semiconduc", OSLO),
    l("BWLPG.OL", "Bw Lpg", OSLO),
    l("VENDA.OL", "Vend Ser. A", OSLO),
    l("STB.OL", "Storebrand", OSLO),
    l("HAFNI.OL", "Hafnia Limited", OSLO),
    l("GJF.OL", "Gjensidige Forsikr", OSLO),
    l("BAKKA.OL", "Bakkafrost", OSLO),
    l("SUBC.OL", "Subsea 7", OSLO),
    l("NAS.OL", "Norwegian Air Shut", OSLO),
    l("HAUTO.OL", "Höegh Autoliners", OSLO),
    l("HEX.OL", "Hexagon Composites", OSLO),
    l("DOFG.OL", "Dof Group", OSLO),
    l("ELK.OL", "Elkem", OSLO),
    l("MPCC.OL", "Mpc Container Ship", OSLO),
    l("SNI.OL", "Stolt-Nielsen", OSLO),
    l("DNO.OL", "Dno", OSLO),
    l("CMBTO.OL", "Cmb.Tech", OSLO),
    l("LINK.OL", "Link Mobility Grp", OSLO),
    l("WAWI.OL", "Wallenius Wilhelms", OSLO),
    l("SB1NO.OL", "Sparebank 1 Sør-N", OSLO),
    l("AKSO.OL", "Aker Solutions", OSLO),
    l("PROT.OL", "Protector Forsikrg", OSLO),
    l("AUTO.OL", "Autostore Holdings", OSLO),
    l("KIT.OL", "Kitron", OSLO),
    l("REACH.OL", "Reach Subsea", OSLO),
    l("AKER.OL", "Aker", OSLO),
    l("GSF.OL", "Grieg Seafood", OSLO),
    l("NEL.OL", "Nel", OSLO),
    l("2020.OL", "2020 Bulkers", OSLO),
    l("EPR.OL", "Europris", OSLO),
    l("LSG.OL", "Lerøy Seafood Gp", OSLO),
    l("ODL.OL", "Odfjell Drilling", OSLO),
    l("MING.OL", "Sparebank 1 Smn", OSLO),
    l("SATS.OL", "Sats", OSLO),
    l("NORBT.OL", "Norbit", OSLO),
    l("CADLR.OL", "Cadeler", OSLO),
    l("OTEC.OL", "Otello Corporation", OSLO),
    l("SCATC.OL", "Scatec", OSLO),
    l("SBNOR.OL", "Sparebanken Norge", OSLO),
    l("OET.OL", "Okeanis Eco Tanker", OSLO),
    l("TGS.OL", "Tgs", OSLO),
    l("HSHP.OL", "Himalaya Shipping", OSLO),
    l("TRMED.OL", "Thor Medical", OSLO),
    l("AUSS.OL", "Austevoll Seafood", OSLO),
    l("ZAP.OL", "Zaptec", OSLO),
    l("NRC.OL", "Nrc Group", OSLO),
    l("VEI.OL", "Veidekke", OSLO),
    l("ZAL.OL", "Zalaris", OSLO),
    l("NONG.OL", "Spbk1 Nord-Norge", OSLO),
    l("PUBLI.OL", "Public Property In", OSLO),
    l("ENDUR.OL", "Endúr", OSLO),
    l("PEXIP.OL", "Pexip Holding", OSLO),
    l("BRG.OL", "Borregaard", OSLO),
    l("ENVIP.OL", "Envipco Holding", OSLO),
    l("NORCO.OL", "Norconsult", OSLO),
    l("ELMRA.OL", "Elmera Group", OSLO),
    l("ABB.ST", "ABB", STOCKHOLM),
    l("ALFA.ST", "Alfa Laval", STOCKHOLM),
    l("ASSA-B.ST", "Assa Abloy B", STOCKHOLM),
    l("ATCO-A.ST", "Atlas Copco A", STOCKHOLM),
    l("ATCO-B.ST", "Atlas Copco B", STOCKHOLM),
    l("AZN.ST", "AstraZeneca", STOCKHOLM),
    l("BOL.ST", "Boliden", STOCKHOLM),
    l("ELUX-B.ST", "Electrolux B", STOCKHOLM),
    l("EPI-A.ST", "Epiroc A", STOCKHOLM),
    l("ERIC-B.ST", "Ericsson B", STOCKHOLM),
    l("ESSITY-B.ST", "Essity B", STOCKHOLM),
    l("EVO.ST", "Evolution", STOCKHOLM),
    l("GETI-B.ST", "Getinge B", STOCKHOLM),
    l("HEXA-B.ST", "Hexagon B", STOCKHOLM),
    l("HM-B.ST", "Hennes & Mauritz B", STOCKHOLM),
    l("INVE-B.ST", "Investor B", STOCKHOLM),
    l("KINV-B.ST", "Kinnevik B", STOCKHOLM),
    l("NDA-SE.ST", "Nordea Bank", STOCKHOLM),
    l("NIBE-B.ST", "Nibe Industrier B", STOCKHOLM),
    l("SAAB-B.ST", "Saab B", STOCKHOLM),
    l("SAND.ST", "Sandvik", STOCKHOLM),
    l("SCA-B.ST", "Svenska Cellulosa B", STOCKHOLM),
    l("SEB-A.ST", "SEB A", STOCKHOLM),
    l("SHB-A.ST", "Handelsbanken A", STOCKHOLM),
    l("SINCH.ST", "Sinch", STOCKHOLM),
    l("SKF-B.ST", "SKF B", STOCKHOLM),
    l("SWED-A.ST", "Swedbank A", STOCKHOLM),
    l("TEL2-B.ST", "Tele2 B", STOCKHOLM),
    l("TELIA.ST", "Telia", STOCKHOLM),
    l("VOLV-B.ST", "Volvo B", STOCKHOLM),
    l("AMBU-B.CO", "Ambu B", COPENHAGEN),
    l("BAVA.CO", "Bavarian Nordic", COPENHAGEN),
    l("CARL-B.CO", "Carlsberg B", COPENHAGEN),
    l("COLO-B.CO", "Coloplast B", COPENHAGEN),
    l("DANSKE.CO", "Danske Bank", COPENHAGEN),
    l("DEMANT.CO", "Demant", COPENHAGEN),
    l("DSV.CO", "DSV", COPENHAGEN),
    l("FLS.CO", "FLSmidth", COPENHAGEN),
    l("GMAB.CO", "Genmab", COPENHAGEN),
    l("GN.CO", "GN Store Nord", COPENHAGEN),
    l("ISS.CO", "ISS", COPENHAGEN),
    l("MAERSK-B.CO", "A.P. Moller Maersk B", COPENHAGEN),
    l("NKT.CO", "NKT", COPENHAGEN),
    l("NOVO-B.CO", "Novo Nordisk B", COPENHAGEN),
    l("NSIS-B.CO", "Novonesis B", COPENHAGEN),
    l("ORSTED.CO", "Orsted", COPENHAGEN),
    l("PNDORA.CO", "Pandora", COPENHAGEN),
    l("RBREW.CO", "Royal Unibrew", COPENHAGEN),
    l("ROCK-B.CO", "Rockwool B", COPENHAGEN),
    // Yahoo carries the Copenhagen line as ALSYDB.CO, not SYDB.CO, which returns Not Found.
    l("ALSYDB.CO", "Sydbank", COPENHAGEN),
    l("TRYG.CO", "Tryg", COPENHAGEN),
    l("VWS.CO", "Vestas Wind Systems", COPENHAGEN),
    l("ZEAL.CO", "Zealand Pharma", COPENHAGEN),
];

/// The markets a search can be scoped to, in the order the screen offers them.
pub const MARKETS: &[&str] = &[OSLO, STOCKHOLM, COPENHAGEN];

/// Every symbol in a market, or in all of them when `market` names none.
pub fn in_market(market: &str) -> Vec<String> {
    LISTINGS
        .iter()
        .filter(|l| market.is_empty() || l.market == market)
        .map(|l| l.symbol.to_string())
        .collect()
}

/// The listed name for a symbol, for a screen that would otherwise show only a ticker.
pub fn name_of(symbol: &str) -> Option<&'static str> {
    LISTINGS.iter().find(|l| l.symbol == symbol).map(|l| l.name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_listing_is_unique_and_carries_a_yahoo_suffix() {
        // A duplicate would be fetched twice and could enter one basket as two holdings; a
        // suffix-less symbol is a Yahoo lookup for something on an American exchange.
        let mut seen: Vec<&str> = LISTINGS.iter().map(|l| l.symbol).collect();
        let before = seen.len();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), before, "duplicate symbols in the list");
        for l in LISTINGS {
            assert!(
                l.symbol.ends_with(".OL") || l.symbol.ends_with(".ST") || l.symbol.ends_with(".CO"),
                "{} has no exchange suffix",
                l.symbol
            );
            assert!(!l.name.is_empty(), "{} has no name", l.symbol);
        }
    }

    #[test]
    fn a_market_filter_returns_only_that_market_and_no_filter_returns_all() {
        let oslo = in_market(OSLO);
        assert!(oslo.iter().all(|s| s.ends_with(".OL")));
        assert_eq!(in_market("").len(), LISTINGS.len());
        let counts: usize = MARKETS.iter().map(|m| in_market(m).len()).sum();
        assert_eq!(
            counts,
            LISTINGS.len(),
            "every listing is in exactly one market"
        );
    }
}
