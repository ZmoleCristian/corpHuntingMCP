use crate::model::{Ats, Tenant};

pub const TENANTS: [Tenant; 12] = [
    Tenant { ats: Ats::Greenhouse, slug: "jjsnackfoods", company: "J&J Snack Foods", host: "", site: "" },
    Tenant { ats: Ats::Greenhouse, slug: "lindenlab", company: "Linden Lab", host: "", site: "" },
    Tenant { ats: Ats::Greenhouse, slug: "itranslate", company: "iTranslate", host: "", site: "" },
    Tenant { ats: Ats::Greenhouse, slug: "nintendo", company: "Nintendo", host: "", site: "" },
    Tenant { ats: Ats::Greenhouse, slug: "nordeus", company: "Nordeus", host: "", site: "" },
    Tenant { ats: Ats::Greenhouse, slug: "ondofinance", company: "Ondo Finance", host: "", site: "" },
    Tenant { ats: Ats::Greenhouse, slug: "ocadogroup", company: "Ocado Group", host: "", site: "" },
    Tenant { ats: Ats::Greenhouse, slug: "ubiquiti", company: "Ubiquiti", host: "", site: "" },
    Tenant { ats: Ats::Greenhouse, slug: "oneoncology", company: "OneOncology", host: "", site: "" },
    Tenant { ats: Ats::Greenhouse, slug: "uberfreight", company: "Uber Freight", host: "", site: "" },
    Tenant { ats: Ats::Greenhouse, slug: "oribe", company: "Oribe Hair Care", host: "", site: "" },
    Tenant { ats: Ats::Greenhouse, slug: "upstack", company: "UPSTACK", host: "", site: "" },
];
