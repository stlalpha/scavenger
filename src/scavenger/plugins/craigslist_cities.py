"""Dynamic Craigslist city selection based on home ZIP code.

Uses api.zippopotam.us (free, no key) to geocode the ZIP, then ranks
known Craigslist US cities by distance. Returns nearest city first,
then fills with major metros for national coverage.
"""
import math
import logging
import httpx

logger = logging.getLogger(__name__)

# Craigslist subdomain → (lat, lon)
CL_CITIES: dict[str, tuple[float, float]] = {
    "newyork":       (40.7128,  -74.0060),
    "losangeles":    (34.0522, -118.2437),
    "chicago":       (41.8781,  -87.6298),
    "houston":       (29.7604,  -95.3698),
    "phoenix":       (33.4484, -112.0740),
    "philadelphia":  (39.9526,  -75.1652),
    "sanantonio":    (29.4241,  -98.4936),
    "sandiego":      (32.7157, -117.1611),
    "dallas":        (32.7767,  -96.7970),
    "sfbay":         (37.7749, -122.4194),
    "seattle":       (47.6062, -122.3321),
    "denver":        (39.7392, -104.9903),
    "boston":        (42.3601,  -71.0589),
    "detroit":       (42.3314,  -83.0458),
    "minneapolis":   (44.9778,  -93.2650),
    "stlouis":       (38.6270,  -90.1994),
    "baltimore":     (39.2904,  -76.6122),
    "washingtondc":  (38.9072,  -77.0369),
    "nashville":     (36.1627,  -86.7816),
    "louisville":    (38.2527,  -85.7585),
    "portland":      (45.5051, -122.6750),
    "oklahomacity":  (35.4676,  -97.5164),
    "lasvegas":      (36.1699, -115.1398),
    "memphis":       (35.1495,  -90.0490),
    "atlanta":       (33.7490,  -84.3880),
    "miami":         (25.7617,  -80.1918),
    "orlando":       (28.5383,  -81.3792),
    "tampa":         (27.9506,  -82.4572),
    "charlotte":     (35.2271,  -80.8431),
    "raleigh":       (35.7796,  -78.6382),
    "richmond":      (37.5407,  -77.4360),
    "pittsburgh":    (40.4406,  -79.9959),
    "cleveland":     (41.4993,  -81.6944),
    "columbus":      (39.9612,  -82.9988),
    "cincinnati":    (39.1031,  -84.5120),
    "indianapolis":  (39.7684,  -86.1581),
    "milwaukee":     (43.0389,  -87.9065),
    "kansascity":    (39.0997,  -94.5786),
    "omaha":         (41.2565,  -95.9345),
    "saltlakecity":  (40.7608, -111.8910),
    "albuquerque":   (35.0844, -106.6504),
    "tucson":        (32.2226, -110.9747),
    "fresno":        (36.7378, -119.7871),
    "sacramento":    (38.5816, -121.4944),
    "longisland":    (40.7891,  -73.1350),
    "newjersey":     (40.0583,  -74.4057),
    "connecticut":   (41.6032,  -73.0877),
    "austin":        (30.2672,  -97.7431),
    "fortworth":     (32.7555,  -97.3308),
    "elpaso":        (31.7619, -106.4850),
    "jacksonville":  (30.3322,  -81.6557),
    "anchorage":     (61.2181, -149.9003),
    "honolulu":      (21.3069, -157.8583),
}

NATIONAL_METROS = ["newyork", "losangeles", "chicago", "seattle", "dallas", "sfbay"]


def _haversine(lat1: float, lon1: float, lat2: float, lon2: float) -> float:
    """Distance in miles between two lat/lon points."""
    r = 3958.8  # Earth radius in miles
    lat1, lon1, lat2, lon2 = map(math.radians, [lat1, lon1, lat2, lon2])
    dlat = lat2 - lat1
    dlon = lon2 - lon1
    a = math.sin(dlat / 2) ** 2 + math.cos(lat1) * math.cos(lat2) * math.sin(dlon / 2) ** 2
    return r * 2 * math.asin(math.sqrt(a))


async def _geocode_zip(zip_code: str) -> tuple[float, float] | None:
    """Return (lat, lon) for a US ZIP code using api.zippopotam.us."""
    try:
        async with httpx.AsyncClient(timeout=5.0) as client:
            resp = await client.get(f"https://api.zippopotam.us/us/{zip_code}")
            resp.raise_for_status()
            data = resp.json()
            place = data["places"][0]
            return float(place["latitude"]), float(place["longitude"])
    except Exception as e:
        logger.warning("ZIP geocode failed for %s: %s", zip_code, e)
        return None


async def cities_for_zip(home_zip: str, max_cities: int = 7) -> list[str]:
    """Return Craigslist cities sorted by distance from home ZIP.

    Nearest city is first. Fills remaining slots with major metros
    for national coverage (rare items surface anywhere).
    """
    coords = await _geocode_zip(home_zip)
    if coords is None:
        logger.warning("Falling back to national metros (could not geocode %s)", home_zip)
        return NATIONAL_METROS[:max_cities]

    lat, lon = coords
    ranked = sorted(
        CL_CITIES.items(),
        key=lambda kv: _haversine(lat, lon, kv[1][0], kv[1][1]),
    )

    # Always include local nearest city, then fill with majors for national reach
    cities = [ranked[0][0]]  # nearest
    for metro in NATIONAL_METROS:
        if metro not in cities:
            cities.append(metro)
        if len(cities) >= max_cities:
            break

    logger.info(
        "Craigslist cities for ZIP %s (%s): %s",
        home_zip, ranked[0][0], cities,
    )
    return cities
