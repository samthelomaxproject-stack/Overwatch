import hashlib
import os
from datetime import datetime, timezone
from typing import Dict, List, Optional, Tuple

import requests

from .db import get_conn

SHODAN_API_BASE = "https://api.shodan.io"

CATEGORY_QUERIES = {
    "sdr": "software:rtl-sdr OR product:\"RTL-SDR\"",
    "adsb_receiver": "port:30003 OR dump1090 OR ads-b",
    "satcom": "satcom OR inmarsat OR iridium",
    "camera": "webcam OR rtsp OR ip camera",
}


def _env_bool(name: str, default: bool) -> bool:
    return os.getenv(name, str(default).lower()).lower() in ("1", "true", "yes", "on")


def _now_iso() -> str:
    return datetime.now(timezone.utc).isoformat()


def _parse_bbox(bbox: Optional[str]) -> Optional[Tuple[float, float, float, float]]:
    if not bbox:
        return None
    try:
        min_lon, min_lat, max_lon, max_lat = [float(x.strip()) for x in bbox.split(",")]
        return min_lon, min_lat, max_lon, max_lat
    except Exception:
        return None


def _region_key(bbox_tuple: Optional[Tuple[float, float, float, float]], mode: str) -> str:
    if bbox_tuple is None:
        return f"{mode}:global"
    min_lon, min_lat, max_lon, max_lat = bbox_tuple
    if mode == "center":
        c_lat = (min_lat + max_lat) / 2.0
        c_lon = (min_lon + max_lon) / 2.0
        return f"center:{c_lat:.2f},{c_lon:.2f}"
    return f"bbox:{min_lon:.2f},{min_lat:.2f},{max_lon:.2f},{max_lat:.2f}"


def _categories_key(categories: List[str]) -> str:
    return ",".join(sorted(set(categories)))


def _shodan_key() -> str:
    return os.getenv("SHODAN_API_KEY", "").strip()


def _is_region_fresh(region_key: str, categories_key: str, ttl_sec: int) -> bool:
    with get_conn() as conn:
        row = conn.execute(
            "SELECT last_discovery_at FROM shodan_region_cache WHERE region_key=? AND categories_key=?",
            (region_key, categories_key),
        ).fetchone()
        if not row:
            return False
        try:
            last = datetime.fromisoformat(row["last_discovery_at"].replace("Z", "+00:00"))
            age = (datetime.now(timezone.utc) - last).total_seconds()
            return age < ttl_sec
        except Exception:
            return False


def _mark_region_discovery(region_key: str, categories_key: str, result_count: int):
    now = _now_iso()
    with get_conn() as conn:
        conn.execute(
            """
            INSERT INTO shodan_region_cache (region_key, categories_key, last_discovery_at, last_result_count)
            VALUES (?, ?, ?, ?)
            ON CONFLICT(region_key, categories_key) DO UPDATE SET
              last_discovery_at=excluded.last_discovery_at,
              last_result_count=excluded.last_result_count
            """,
            (region_key, categories_key, now, int(result_count)),
        )
        conn.commit()


def _build_geo_clause(bbox_tuple: Optional[Tuple[float, float, float, float]]) -> str:
    if bbox_tuple is None:
        return ""
    min_lon, min_lat, max_lon, max_lat = bbox_tuple
    c_lat = (min_lat + max_lat) / 2.0
    c_lon = (min_lon + max_lon) / 2.0
    # rough radius in km from bbox diagonal/2
    lat_span_km = abs(max_lat - min_lat) * 111.0
    lon_span_km = abs(max_lon - min_lon) * 111.0
    radius = max(10, min(400, int(max(lat_span_km, lon_span_km) / 2.0)))
    return f" geo:{c_lat:.4f},{c_lon:.4f},{radius}"


def _normalize_match(match: dict, category: str, source_query: str) -> Optional[dict]:
    loc = match.get("location") or {}
    lat = loc.get("latitude")
    lon = loc.get("longitude")
    if lat is None or lon is None:
        return None
    ip = str(match.get("ip_str") or "")
    port = int(match.get("port") or 0)
    transport = str(match.get("transport") or "")
    uid_seed = f"{ip}:{port}:{transport}:{category}"
    uid = hashlib.sha1(uid_seed.encode("utf-8")).hexdigest()
    return {
        "id": uid,
        "ip": ip,
        "port": port,
        "transport": transport,
        "org": str(match.get("org") or ""),
        "isp": str(match.get("isp") or ""),
        "asn": str(match.get("asn") or ""),
        "hostnames": ",".join(match.get("hostnames") or []),
        "product": str(match.get("product") or ""),
        "tags": ",".join(match.get("tags") or []),
        "vulns": ",".join((match.get("vulns") or {}).keys()) if isinstance(match.get("vulns"), dict) else ",".join(match.get("vulns") or []),
        "country_code": str(loc.get("country_code") or ""),
        "country_name": str(loc.get("country_name") or ""),
        "city": str(loc.get("city") or ""),
        "latitude": float(lat),
        "longitude": float(lon),
        "category": category,
        "source_query": source_query,
    }


def _upsert_findings(rows: List[dict]):
    if not rows:
        return
    now = _now_iso()
    with get_conn() as conn:
        for r in rows:
            conn.execute(
                """
                INSERT INTO shodan_findings (
                  id, ip, port, transport, org, isp, asn, hostnames, product, tags, vulns,
                  country_code, country_name, city, latitude, longitude, category, source_query,
                  last_seen_at, updated_at
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                ON CONFLICT(id) DO UPDATE SET
                  ip=excluded.ip,
                  port=excluded.port,
                  transport=excluded.transport,
                  org=excluded.org,
                  isp=excluded.isp,
                  asn=excluded.asn,
                  hostnames=excluded.hostnames,
                  product=excluded.product,
                  tags=excluded.tags,
                  vulns=excluded.vulns,
                  country_code=excluded.country_code,
                  country_name=excluded.country_name,
                  city=excluded.city,
                  latitude=excluded.latitude,
                  longitude=excluded.longitude,
                  category=excluded.category,
                  source_query=excluded.source_query,
                  last_seen_at=excluded.last_seen_at,
                  updated_at=excluded.updated_at
                """,
                (
                    r["id"], r["ip"], r["port"], r["transport"], r["org"], r["isp"], r["asn"],
                    r["hostnames"], r["product"], r["tags"], r["vulns"], r["country_code"],
                    r["country_name"], r["city"], r["latitude"], r["longitude"], r["category"],
                    r["source_query"], now, now,
                ),
            )
        conn.commit()


def discover_shodan(
    bbox: Optional[str] = None,
    categories: Optional[List[str]] = None,
    force_refresh: bool = False,
) -> Dict[str, object]:
    key = _shodan_key()
    if not key:
        return {"ok": False, "reason": "missing_shodan_api_key", "fetched": 0}

    default_categories = [c.strip() for c in os.getenv("SHODAN_DISCOVERY_CATEGORIES", "sdr,adsb_receiver,satcom,camera").split(",") if c.strip()]
    cats = categories or default_categories
    cats = [c for c in cats if c in CATEGORY_QUERIES]
    if not cats:
        return {"ok": False, "reason": "no_valid_categories", "fetched": 0}

    region_mode = os.getenv("SHODAN_DEFAULT_REGION_MODE", "bbox")
    region_ttl = int(os.getenv("SHODAN_REGION_TTL_SEC", "900"))
    max_per_query = int(os.getenv("SHODAN_MAX_RESULTS_PER_QUERY", "50"))

    bbox_tuple = _parse_bbox(bbox)
    region_key = _region_key(bbox_tuple, region_mode)
    categories_key = _categories_key(cats)

    if not force_refresh and _is_region_fresh(region_key, categories_key, region_ttl):
        return {"ok": True, "cached": True, "fetched": 0, "region_key": region_key}

    geo_clause = _build_geo_clause(bbox_tuple)
    collected: List[dict] = []

    for c in cats:
        q = CATEGORY_QUERIES[c] + geo_clause
        resp = requests.get(
            f"{SHODAN_API_BASE}/shodan/host/search",
            params={"key": key, "query": q, "page": 1, "minify": "true"},
            timeout=30,
        )
        if resp.status_code != 200:
            continue
        payload = resp.json()
        matches = payload.get("matches") or []
        for m in matches[:max_per_query]:
            norm = _normalize_match(m, c, q)
            if norm:
                collected.append(norm)

    _upsert_findings(collected)
    _mark_region_discovery(region_key, categories_key, len(collected))
    return {"ok": True, "cached": False, "fetched": len(collected), "region_key": region_key}


def get_shodan_markers(
    bbox: Optional[str] = None,
    categories: Optional[List[str]] = None,
    country: Optional[str] = None,
    limit: Optional[int] = None,
) -> List[dict]:
    max_visible = int(os.getenv("SHODAN_MAX_VISIBLE_RESULTS", "1500"))
    lim = min(int(limit or os.getenv("SHODAN_DEFAULT_LIMIT", "600")), max_visible)

    where = ["latitude IS NOT NULL", "longitude IS NOT NULL"]
    vals: List[object] = []

    bbox_tuple = _parse_bbox(bbox)
    if bbox_tuple is not None:
        min_lon, min_lat, max_lon, max_lat = bbox_tuple
        where.append("longitude BETWEEN ? AND ?")
        vals.extend([min_lon, max_lon])
        where.append("latitude BETWEEN ? AND ?")
        vals.extend([min_lat, max_lat])

    if country:
        where.append("(country_name = ? OR country_code = ?)")
        vals.extend([country, country])

    if categories:
        valid = [c for c in categories if c in CATEGORY_QUERIES]
        if valid:
            where.append("category IN ({})".format(",".join(["?"] * len(valid))))
            vals.extend(valid)

    sql = f"""
      SELECT * FROM shodan_findings
      WHERE {' AND '.join(where)}
      ORDER BY datetime(updated_at) DESC
      LIMIT ?
    """
    vals.append(lim)

    with get_conn() as conn:
        rows = conn.execute(sql, vals).fetchall()
        out = []
        for r in rows:
            out.append({
                "id": r["id"],
                "type": "shodan",
                "lat": r["latitude"],
                "lon": r["longitude"],
                "ip": r["ip"],
                "port": r["port"],
                "transport": r["transport"],
                "org": r["org"],
                "isp": r["isp"],
                "asn": r["asn"],
                "hostnames": r["hostnames"],
                "product": r["product"],
                "tags": r["tags"],
                "vulns": r["vulns"],
                "country_code": r["country_code"],
                "country_name": r["country_name"],
                "city": r["city"],
                "category": r["category"],
                "source_query": r["source_query"],
                "updated_at": r["updated_at"],
                "style": {"icon": "divIcon", "color": "#8b5cf6", "radius": 6},
                "popup": {
                    "title": f"Shodan {r['ip']}:{r['port']}",
                    "fields": {
                        "org": r["org"],
                        "isp": r["isp"],
                        "asn": r["asn"],
                        "product": r["product"],
                        "category": r["category"],
                    },
                    "sources": [{"name": "Shodan", "url": "https://www.shodan.io/"}],
                },
            })
        return out


def scheduler_enabled() -> bool:
    return _env_bool("SHODAN_ENABLE_SCHEDULER", False)


def scheduler_interval_sec() -> int:
    return int(os.getenv("SHODAN_DISCOVERY_INTERVAL_SEC", "900"))
