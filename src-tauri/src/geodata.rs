#[derive(Clone, Copy)]
pub struct RemoteRuleSet {
    pub tag: &'static str,
    pub url: &'static str,
}

pub const ADS_RULE_SET_TAG: &str = "geosite-category-ads-all";

pub const DNS_DIRECT_RULE_SET_TAGS: &[&str] =
    &["geosite-category-gov-ru", "geosite-yandex", "geosite-vk"];

pub const DIRECT_ROUTE_RULE_SET_TAGS: &[&str] = &[
    "geoip-ru",
    "geosite-category-gov-ru",
    "geosite-yandex",
    "geosite-vk",
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
