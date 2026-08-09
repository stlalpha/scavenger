use log::warn;

/// Craigslist subdomain with its geographic coordinates.
pub struct CityCoord {
    pub subdomain: &'static str,
    pub lat: f64,
    pub lon: f64,
}

/// Known Craigslist city subdomains with lat/lon.
pub static CL_CITIES: &[CityCoord] = &[
    CityCoord { subdomain: "newyork",      lat: 40.7128,  lon: -74.0060 },
    CityCoord { subdomain: "losangeles",   lat: 34.0522,  lon: -118.2437 },
    CityCoord { subdomain: "chicago",      lat: 41.8781,  lon: -87.6298 },
    CityCoord { subdomain: "houston",      lat: 29.7604,  lon: -95.3698 },
    CityCoord { subdomain: "phoenix",      lat: 33.4484,  lon: -112.0740 },
    CityCoord { subdomain: "philadelphia", lat: 39.9526,  lon: -75.1652 },
    CityCoord { subdomain: "sanantonio",   lat: 29.4241,  lon: -98.4936 },
    CityCoord { subdomain: "sandiego",     lat: 32.7157,  lon: -117.1611 },
    CityCoord { subdomain: "dallas",       lat: 32.7767,  lon: -96.7970 },
    CityCoord { subdomain: "sfbay",        lat: 37.7749,  lon: -122.4194 },
    CityCoord { subdomain: "seattle",      lat: 47.6062,  lon: -122.3321 },
    CityCoord { subdomain: "denver",       lat: 39.7392,  lon: -104.9903 },
    CityCoord { subdomain: "boston",        lat: 42.3601,  lon: -71.0589 },
    CityCoord { subdomain: "detroit",      lat: 42.3314,  lon: -83.0458 },
    CityCoord { subdomain: "minneapolis",  lat: 44.9778,  lon: -93.2650 },
    CityCoord { subdomain: "stlouis",      lat: 38.6270,  lon: -90.1994 },
    CityCoord { subdomain: "baltimore",    lat: 39.2904,  lon: -76.6122 },
    CityCoord { subdomain: "washingtondc", lat: 38.9072,  lon: -77.0369 },
    CityCoord { subdomain: "nashville",    lat: 36.1627,  lon: -86.7816 },
    CityCoord { subdomain: "louisville",   lat: 38.2527,  lon: -85.7585 },
    CityCoord { subdomain: "portland",     lat: 45.5051,  lon: -122.6750 },
    CityCoord { subdomain: "oklahomacity", lat: 35.4676,  lon: -97.5164 },
    CityCoord { subdomain: "lasvegas",     lat: 36.1699,  lon: -115.1398 },
    CityCoord { subdomain: "memphis",      lat: 35.1495,  lon: -90.0490 },
    CityCoord { subdomain: "atlanta",      lat: 33.7490,  lon: -84.3880 },
    CityCoord { subdomain: "miami",        lat: 25.7617,  lon: -80.1918 },
    CityCoord { subdomain: "orlando",      lat: 28.5383,  lon: -81.3792 },
    CityCoord { subdomain: "tampa",        lat: 27.9506,  lon: -82.4572 },
    CityCoord { subdomain: "charlotte",    lat: 35.2271,  lon: -80.8431 },
    CityCoord { subdomain: "raleigh",      lat: 35.7796,  lon: -78.6382 },
    CityCoord { subdomain: "richmond",     lat: 37.5407,  lon: -77.4360 },
    CityCoord { subdomain: "pittsburgh",   lat: 40.4406,  lon: -79.9959 },
    CityCoord { subdomain: "cleveland",    lat: 41.4993,  lon: -81.6944 },
    CityCoord { subdomain: "columbus",     lat: 39.9612,  lon: -82.9988 },
    CityCoord { subdomain: "cincinnati",   lat: 39.1031,  lon: -84.5120 },
    CityCoord { subdomain: "indianapolis", lat: 39.7684,  lon: -86.1581 },
    CityCoord { subdomain: "milwaukee",    lat: 43.0389,  lon: -87.9065 },
    CityCoord { subdomain: "kansascity",   lat: 39.0997,  lon: -94.5786 },
    CityCoord { subdomain: "omaha",        lat: 41.2565,  lon: -95.9345 },
    CityCoord { subdomain: "saltlakecity", lat: 40.7608,  lon: -111.8910 },
    CityCoord { subdomain: "albuquerque",  lat: 35.0844,  lon: -106.6504 },
    CityCoord { subdomain: "tucson",       lat: 32.2226,  lon: -110.9747 },
    CityCoord { subdomain: "fresno",       lat: 36.7378,  lon: -119.7871 },
    CityCoord { subdomain: "sacramento",   lat: 38.5816,  lon: -121.4944 },
    CityCoord { subdomain: "longisland",   lat: 40.7891,  lon: -73.1350 },
    CityCoord { subdomain: "newjersey",    lat: 40.0583,  lon: -74.4057 },
    CityCoord { subdomain: "connecticut",  lat: 41.6032,  lon: -73.0877 },
    CityCoord { subdomain: "austin",       lat: 30.2672,  lon: -97.7431 },
    CityCoord { subdomain: "fortworth",    lat: 32.7555,  lon: -97.3308 },
    CityCoord { subdomain: "elpaso",       lat: 31.7619,  lon: -106.4850 },
    CityCoord { subdomain: "jacksonville", lat: 30.3322,  lon: -81.6557 },
    CityCoord { subdomain: "anchorage",    lat: 61.2181,  lon: -149.9003 },
    CityCoord { subdomain: "honolulu",     lat: 21.3069,  lon: -157.8583 },
];

pub const NATIONAL_METROS: &[&str] = &[
    "newyork",
    "losangeles",
    "chicago",
    "seattle",
    "dallas",
    "sfbay",
];

/// Haversine distance in miles between two lat/lon points.
pub fn haversine(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    const EARTH_RADIUS_MI: f64 = 3958.8;

    let lat1 = lat1.to_radians();
    let lon1 = lon1.to_radians();
    let lat2 = lat2.to_radians();
    let lon2 = lon2.to_radians();

    let dlat = lat2 - lat1;
    let dlon = lon2 - lon1;

    let a = (dlat / 2.0).sin().powi(2) + lat1.cos() * lat2.cos() * (dlon / 2.0).sin().powi(2);
    EARTH_RADIUS_MI * 2.0 * a.sqrt().asin()
}

/// Response from the Zippopotam.us API.
#[derive(serde::Deserialize)]
struct ZipResponse {
    places: Vec<ZipPlace>,
}

#[derive(serde::Deserialize)]
struct ZipPlace {
    latitude: String,
    longitude: String,
}

/// Geocode a US ZIP code to (lat, lon) via api.zippopotam.us.
async fn geocode_zip(zip: &str) -> Result<(f64, f64), Box<dyn std::error::Error + Send + Sync>> {
    let url = format!("https://api.zippopotam.us/us/{}", zip);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()?;
    let resp: ZipResponse = client.get(&url).send().await?.json().await?;
    let place = resp.places.first().ok_or("no places in ZIP response")?;
    let lat: f64 = place.latitude.parse()?;
    let lon: f64 = place.longitude.parse()?;
    Ok((lat, lon))
}

/// Return Craigslist cities sorted by relevance to the given ZIP code.
///
/// Nearest city first, then fill remaining slots with national metros
/// for coverage of rare items that may be listed anywhere.
/// Falls back to NATIONAL_METROS on any geocoding error.
pub async fn cities_for_zip(zip: &str, max_cities: usize) -> Vec<String> {
    let (lat, lon) = match geocode_zip(zip).await {
        Ok(coords) => coords,
        Err(e) => {
            warn!("ZIP geocode failed for {}: {}. Falling back to national metros.", zip, e);
            return NATIONAL_METROS
                .iter()
                .take(max_cities)
                .map(|s| s.to_string())
                .collect();
        }
    };

    // Sort all cities by distance
    let mut ranked: Vec<(&str, f64)> = CL_CITIES
        .iter()
        .map(|c| (c.subdomain, haversine(lat, lon, c.lat, c.lon)))
        .collect();
    ranked.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());

    let mut cities = vec![ranked[0].0.to_string()];

    for metro in NATIONAL_METROS {
        if !cities.iter().any(|c| c == metro) {
            cities.push(metro.to_string());
        }
        if cities.len() >= max_cities {
            break;
        }
    }

    cities
}

/// Return cities for a ZIP with the default max of 7.
pub async fn cities_for_zip_default(zip: &str) -> Vec<String> {
    cities_for_zip(zip, 7).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn haversine_known_distance() {
        // New York to Los Angeles: ~2,451 miles
        let dist = haversine(40.7128, -74.0060, 34.0522, -118.2437);
        assert!((dist - 2451.0).abs() < 20.0, "NY-LA distance was {}", dist);
    }

    #[test]
    fn haversine_same_point() {
        let dist = haversine(40.0, -90.0, 40.0, -90.0);
        assert!(dist.abs() < 0.01);
    }

    #[test]
    fn haversine_chicago_to_stlouis() {
        // ~262 miles
        let dist = haversine(41.8781, -87.6298, 38.6270, -90.1994);
        assert!((dist - 262.0).abs() < 15.0, "CHI-STL distance was {}", dist);
    }

    #[test]
    fn national_metros_count() {
        assert_eq!(NATIONAL_METROS.len(), 6);
    }
}
