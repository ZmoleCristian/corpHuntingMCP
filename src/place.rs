use crate::model::Country;

const REMOTE_MARKERS: [&str; 9] = [
    "remote",
    "work from home",
    "wfh",
    "anywhere",
    "distributed",
    "telecommute",
    "home based",
    "home-based",
    "fully virtual",
];

const NAMES: [(&str, &str); 96] = [
    ("united states", "US"),
    ("united kingdom", "GB"),
    ("great britain", "GB"),
    ("czech republic", "CZ"),
    ("czechia", "CZ"),
    ("south korea", "KR"),
    ("north macedonia", "MK"),
    ("south africa", "ZA"),
    ("new zealand", "NZ"),
    ("saudi arabia", "SA"),
    ("united arab emirates", "AE"),
    ("hong kong", "HK"),
    ("costa rica", "CR"),
    ("puerto rico", "PR"),
    ("dominican republic", "DO"),
    ("el salvador", "SV"),
    ("bosnia", "BA"),
    ("sri lanka", "LK"),
    ("afghanistan", "AF"),
    ("argentina", "AR"),
    ("australia", "AU"),
    ("austria", "AT"),
    ("bangladesh", "BD"),
    ("belarus", "BY"),
    ("belgium", "BE"),
    ("bolivia", "BO"),
    ("brazil", "BR"),
    ("bulgaria", "BG"),
    ("cambodia", "KH"),
    ("cameroon", "CM"),
    ("canada", "CA"),
    ("chile", "CL"),
    ("china", "CN"),
    ("colombia", "CO"),
    ("croatia", "HR"),
    ("cyprus", "CY"),
    ("denmark", "DK"),
    ("ecuador", "EC"),
    ("egypt", "EG"),
    ("estonia", "EE"),
    ("ethiopia", "ET"),
    ("finland", "FI"),
    ("france", "FR"),
    ("georgia", "GE"),
    ("germany", "DE"),
    ("ghana", "GH"),
    ("greece", "GR"),
    ("guatemala", "GT"),
    ("hungary", "HU"),
    ("iceland", "IS"),
    ("india", "IN"),
    ("indonesia", "ID"),
    ("ireland", "IE"),
    ("israel", "IL"),
    ("italy", "IT"),
    ("japan", "JP"),
    ("jordan", "JO"),
    ("kazakhstan", "KZ"),
    ("kenya", "KE"),
    ("kuwait", "KW"),
    ("latvia", "LV"),
    ("lebanon", "LB"),
    ("lithuania", "LT"),
    ("luxembourg", "LU"),
    ("malaysia", "MY"),
    ("malta", "MT"),
    ("mexico", "MX"),
    ("moldova", "MD"),
    ("morocco", "MA"),
    ("netherlands", "NL"),
    ("nigeria", "NG"),
    ("norway", "NO"),
    ("pakistan", "PK"),
    ("panama", "PA"),
    ("paraguay", "PY"),
    ("peru", "PE"),
    ("philippines", "PH"),
    ("poland", "PL"),
    ("portugal", "PT"),
    ("qatar", "QA"),
    ("romania", "RO"),
    ("russia", "RU"),
    ("serbia", "RS"),
    ("singapore", "SG"),
    ("slovakia", "SK"),
    ("slovenia", "SI"),
    ("spain", "ES"),
    ("sweden", "SE"),
    ("switzerland", "CH"),
    ("taiwan", "TW"),
    ("thailand", "TH"),
    ("tunisia", "TN"),
    ("turkey", "TR"),
    ("ukraine", "UA"),
    ("uruguay", "UY"),
    ("vietnam", "VN"),
];

const CITIES: [(&str, &str); 44] = [
    ("bucharest", "RO"),
    ("bucuresti", "RO"),
    ("cluj", "RO"),
    ("timisoara", "RO"),
    ("iasi", "RO"),
    ("brasov", "RO"),
    ("sibiu", "RO"),
    ("constanta", "RO"),
    ("craiova", "RO"),
    ("oradea", "RO"),
    ("london", "GB"),
    ("manchester", "GB"),
    ("edinburgh", "GB"),
    ("dublin", "IE"),
    ("berlin", "DE"),
    ("munich", "DE"),
    ("hamburg", "DE"),
    ("frankfurt", "DE"),
    ("paris", "FR"),
    ("lyon", "FR"),
    ("madrid", "ES"),
    ("barcelona", "ES"),
    ("lisbon", "PT"),
    ("porto", "PT"),
    ("amsterdam", "NL"),
    ("rotterdam", "NL"),
    ("brussels", "BE"),
    ("zurich", "CH"),
    ("geneva", "CH"),
    ("vienna", "AT"),
    ("prague", "CZ"),
    ("warsaw", "PL"),
    ("krakow", "PL"),
    ("budapest", "HU"),
    ("sofia", "BG"),
    ("belgrade", "RS"),
    ("athens", "GR"),
    ("stockholm", "SE"),
    ("copenhagen", "DK"),
    ("oslo", "NO"),
    ("helsinki", "FI"),
    ("tallinn", "EE"),
    ("vilnius", "LT"),
    ("riga", "LV"),
];

pub fn country_of(text: &str) -> Country {
    let lowered = text.to_lowercase();
    for entry in NAMES {
        if lowered.contains(entry.0) {
            return Country::Known(entry.1.to_string());
        }
    }
    for entry in CITIES {
        if lowered.contains(entry.0) {
            return Country::Known(entry.1.to_string());
        }
    }
    for entry in NAMES {
        if token_present(&lowered, entry.1) {
            return Country::Known(entry.1.to_string());
        }
    }
    Country::Unknown
}

pub fn is_remote(text: &str) -> bool {
    let lowered = text.to_lowercase();
    for marker in REMOTE_MARKERS {
        if lowered.contains(marker) {
            return true;
        }
    }
    false
}

fn token_present(lowered: &str, code: &str) -> bool {
    let needle = code.to_lowercase();
    for chunk in lowered.split(|c: char| !c.is_ascii_alphanumeric()) {
        if chunk == needle {
            return true;
        }
    }
    false
}
