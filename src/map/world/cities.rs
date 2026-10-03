//! 城市坐标静态表:MockCollector 模拟连接的地理归属与地图节点定位。

/// 城市/节点:显示名经 i18n 词条(`city-<key>`)获取,这里只保留标识与坐标
pub struct City {
    /// 稳定标识(如 "tokyo"),供 [`crate::model::Connection::city`] 与词条键引用
    pub key: &'static str,
    pub lon: f32,
    pub lat: f32,
}

/// 本机节点
pub const LOCAL: City = City {
    key: "local",
    lon: 116.4,
    lat: 39.9,
};

/// 远端城市表(MockCollector 按此生成模拟连接的地理归属)
pub const CITIES: &[City] = &[
    City {
        key: "shanghai",
        lon: 121.47,
        lat: 31.23,
    },
    City {
        key: "tokyo",
        lon: 139.69,
        lat: 35.69,
    },
    City {
        key: "seoul",
        lon: 126.98,
        lat: 37.57,
    },
    City {
        key: "hongkong",
        lon: 114.17,
        lat: 22.32,
    },
    City {
        key: "macau",
        lon: 113.55,
        lat: 22.20,
    },
    City {
        key: "taipei",
        lon: 121.56,
        lat: 25.03,
    },
    City {
        key: "singapore",
        lon: 103.82,
        lat: 1.35,
    },
    City {
        key: "mumbai",
        lon: 72.88,
        lat: 19.08,
    },
    City {
        key: "moscow",
        lon: 37.62,
        lat: 55.75,
    },
    City {
        key: "frankfurt",
        lon: 8.68,
        lat: 50.11,
    },
    City {
        key: "amsterdam",
        lon: 4.90,
        lat: 52.37,
    },
    City {
        key: "london",
        lon: -0.13,
        lat: 51.51,
    },
    City {
        key: "newyork",
        lon: -74.01,
        lat: 40.71,
    },
    City {
        key: "sanjose",
        lon: -121.89,
        lat: 37.34,
    },
    City {
        key: "sydney",
        lon: 151.21,
        lat: -33.87,
    },
    City {
        key: "saopaulo",
        lon: -46.63,
        lat: -23.55,
    },
];

/// 按键查城市;`key` 必须来自 [`CITIES`](crate::map::world::CITIES)
pub fn city(key: &str) -> &City {
    CITIES
        .iter()
        .find(|c| c.key == key)
        .expect("unknown city key")
}
