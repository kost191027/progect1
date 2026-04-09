#[derive(Clone, Copy)]
pub struct RemoteRuleSet {
    pub tag: &'static str,
    pub url: &'static str,
}

pub const ADS_RULE_SET_TAG: &str = "geosite-category-ads-all";

pub const DIRECT_ROUTE_RULE_SET_TAGS: &[&str] = &[
    "geoip-ru",
    "geosite-category-gov-ru",
    "geosite-yandex",
    "geosite-vk",
];

pub const CURATED_RU_DOMAIN_SUFFIXES: &[&str] = &[
    "2gis.ru",
    "alfabank.ru",
    "avito.ru",
    "cdek.ru",
    "gosuslugi.ru",
    "kinopoisk.ru",
    "mail.ru",
    "mos.ru",
    "nalog.gov.ru",
    "ok.ru",
    "ozon.ru",
    "pochta.ru",
    "rambler.ru",
    "sberbank.ru",
    "tbank.ru",
    "tinkoff.ru",
    "vtb.ru",
    "wildberries.ru",
];

pub const PROXY_PRIORITY_DOMAIN_SUFFIXES: &[&str] = &[
    "youtube.com",
    "youtu.be",
    "youtubei.googleapis.com",
    "ytimg.com",
    "googlevideo.com",
    "ggpht.com",
    "google.com",
    "gstatic.com",
    "googleapis.com",
    "googleusercontent.com",
    "withgoogle.com",
    "gemini.google.com",
    "ai.google.dev",
];

pub const REMOTE_RULE_SETS: &[RemoteRuleSet] = &[
    RemoteRuleSet {
        tag: "geoip-ru",
        url: "https://raw.githubusercontent.com/SagerNet/sing-geoip/rule-set/geoip-ru.srs",
    },
    RemoteRuleSet {
        tag: "geosite-category-gov-ru",
        url: "https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/geosite-category-gov-ru.srs",
    },
    RemoteRuleSet {
        tag: "geosite-yandex",
        url: "https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/geosite-yandex.srs",
    },
    RemoteRuleSet {
        tag: "geosite-vk",
        url: "https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/geosite-vk.srs",
    },
    RemoteRuleSet {
        tag: ADS_RULE_SET_TAG,
        url: "https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/geosite-category-ads-all.srs",
    },
];
