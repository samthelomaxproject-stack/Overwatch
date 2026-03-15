import hashlib
import os
from datetime import datetime, timezone
from typing import Dict, List, Optional, Tuple

import requests

from .db import get_conn

SHODAN_API_BASE = "https://api.shodan.io"

CATEGORY_QUERIES = {
    "sdr": "OpenWebRX OR KiwiSDR OR WebSDR",
    "adsb_receiver": "dump1090 OR tar1090 OR \"port:30003\"",
    "satcom": "VSAT OR iDirect OR Hughes OR Inmarsat OR Viasat OR BGAN",
    "camera": "Hikvision OR Dahua OR Axis OR \"ip camera\"",
}


def _env_bool(name: str, default: bool) -> bool:
    return os.getenv(name, str(default).lower()).lower() in ("1", "true", "yes", "on")


def _now_iso() -> str:
    return datetime.now(timezone.utc).isoformat()


def _shodan_key() -> str:
    return os.getenv("SHODAN_API_KEY", "").strip()


def scheduler_enabled() -> bool:
    return _env_bool("SHODAN_ENABLE_SCHEDULER", False)


def scheduler_interval_sec() -> int:
    return int(os.getenv("SHODAN_DISCOVERY_INTERVAL_SEC", "43200"))


def _parse_bbox(bbox: Optional[str]) -> Optional[Tuple[float, float, float, float]]:
    if not bbox:
        return None
    try:
        min_lon, min_lat, max_lon, max_lat = [float(x.strip()) for x in bbox.split(",")]
        return min_lon, min_lat, max_lon, max_lat
    except Exception:
        return None


def _region_key_from_bbox(bbox: Optional[str]) -> str:
    t = _parse_bbox(bbox)
    if t is None:
        return "global"
    min_lon, min_lat, max_lon, max_lat = t
    # coarse fixed tile key (~10-degree buckets) to minimize API churn
    c_lon = (min_lon + max_lon) / 2.0
    c_lat = (min_lat + max_lat) / 2.0
    lon_bucket = int((c_lon + 180) // 10)
    lat_bucket = int((c_lat + 90) // 10)
    return f"tile10:{lat_bucket}:{lon_bucket}"


def _build_geo_clause(bbox: Optional[str]) -> str:
    t = _parse_bbox(bbox)
    if t is None:
        return ""
    min_lon, min_lat, max_lon, max_lat = t
    c_lat = (min_lat + max_lat) / 2.0
    c_lon = (min_lon + max_lon) / 2.0
    lat_span_km = abs(max_lat - min_lat) * 111.0
    lon_span_km = abs(max_lon - min_lon) * 111.0
    radius = max(20, min(600, int(max(lat_span_km, lon_span_km) / 2.0)))
    return f" geo:{c_lat:.4f},{c_lon:.4f},{radius}"


def _normalize(match: dict, category: str, region_key: str, query: str) -> dict:
    loc = match.get("location") or {}
    ip = str(match.get("ip_str") or "")
    port = int(match.get("port") or 0)
    transport = str(match.get("transport") or "")
    uid = hashlib.sha1(f"{ip}:{port}:{transport}:{category}".encode("utf-8")).hexdigest()
    now = _now_iso()
    return {
        "id": uid,
        "ip": ip,
        "port": port,
        "transport": transport,
        "org": str(match.get("org") or ""),
        "isp": str(match.get("isp") or ""),
        "asn": str(match.get("asn") or ""),
        "hostnames": ",".join(match.get("hostnames") or []),
        "domains": ",".join(match.get("domains") or []),
        "product": str(match.get("product") or ""),
        "version": str(match.get("version") or ""),
        "os": str(match.get("os") or ""),
        "tags": ",".join(match.get("tags") or []),
        "vulns": ",".join((match.get("vulns") or {}).keys()) if isinstance(match.get("vulns"), dict) else ",".join(match.get("vulns") or []),
        "category": category,
        "lat": loc.get("latitude"),
        "lon": loc.get("longitude"),
        "country_code": str(loc.get("country_code") or ""),
        "country_name": str(loc.get("country_name") or ""),
        "city": str(loc.get("city") or ""),
        "region_code": str(loc.get("region_code") or ""),
        "timestamp": str(match.get("timestamp") or now),
        "last_seen": now,
        "shodan_url": f"https://www.shodan.io/host/{ip}" if ip else "https://www.shodan.io/",
        "query": query,
        "source": "shodan",
        "region_key": region_key,
        "inserted_at": now,
        "updated_at": now,
        "stale_score": 0,
    }


def _upsert(rows: List[dict]):
    if not rows:
        return
    with get_conn() as conn:
        for r in rows:
            conn.execute(
                """
                INSERT INTO shodan_findings (
                  id, ip, port, transport, org, isp, asn, hostnames, domains, product, version, os, tags, vulns,
                  category, lat, lon, country_code, country_name, city, region_code, timestamp, last_seen,
                  shodan_url, query, source, region_key, inserted_at, updated_at, stale_score
                ) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)
                ON CONFLICT(id) DO UPDATE SET
                  ip=excluded.ip, port=excluded.port, transport=excluded.transport,
                  org=excluded.org, isp=excluded.isp, asn=excluded.asn,
                  hostnames=excluded.hostnames, domains=excluded.domains,
                  product=excluded.product, version=excluded.version, os=excluded.os,
                  tags=excluded.tags, vulns=excluded.vulns, category=excluded.category,
                  lat=excluded.lat, lon=excluded.lon,
                  country_code=excluded.country_code, country_name=excluded.country_name,
                  city=excluded.city, region_code=excluded.region_code,
                  timestamp=excluded.timestamp, last_seen=excluded.last_seen,
                  shodan_url=excluded.shodan_url, query=excluded.query, source=excluded.source,
                  region_key=excluded.region_key, updated_at=excluded.updated_at,
                  stale_score=excluded.stale_score
                """,
                (
                    r["id"], r["ip"], r["port"], r["transport"], r["org"], r["isp"], r["asn"], r["hostnames"],
                    r["domains"], r["product"], r["version"], r["os"], r["tags"], r["vulns"], r["category"],
                    r["lat"], r["lon"], r["country_code"], r["country_name"], r["city"], r["region_code"],
                    r["timestamp"], r["last_seen"], r["shodan_url"], r["query"], r["source"], r["region_key"],
                    r["inserted_at"], r["updated_at"], r["stale_score"],
                ),
            )
        conn.commit()


def _record_region_state(region_key: str, category: str, ttl_sec: int, result_count: int, status: str, error: str = ""):
    with get_conn() as conn:
        conn.execute(
            """
            INSERT INTO shodan_region_cache_state (region_key, category, last_discovery_at, ttl_sec, last_result_count, last_status, last_error)
            VALUES (?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(region_key, category) DO UPDATE SET
              last_discovery_at=excluded.last_discovery_at,
              ttl_sec=excluded.ttl_sec,
              last_result_count=excluded.last_result_count,
              last_status=excluded.last_status,
              last_error=excluded.last_error
            """,
            (region_key, category, _now_iso(), int(ttl_sec), int(result_count), status, error),
        )
        conn.commit()


def _is_stale(region_key: str, category: str, ttl_sec: int) -> bool:
    with get_conn() as conn:
        row = conn.execute(
            "SELECT last_discovery_at FROM shodan_region_cache_state WHERE region_key=? AND category=?",
            (region_key, category),
        ).fetchone()
    if not row:
        return True
    try:
        last = datetime.fromisoformat(str(row["last_discovery_at"]).replace("Z", "+00:00"))
        return (datetime.now(timezone.utc) - last).total_seconds() > ttl_sec
    except Exception:
        return True


def discover_shodan(bbox: Optional[str] = None, categories: Optional[List[str]] = None, force_refresh: bool = False) -> Dict[str, object]:
    key = _shodan_key()
    if not key:
        return {"ok": False, "reason": "not_configured", "fetched": 0}

    max_per_query = int(os.getenv("SHODAN_MAX_RESULTS_PER_QUERY", "100"))
    ttl_sec = int(os.getenv("SHODAN_REGION_TTL_SEC", "43200"))

    cats = categories or [c.strip() for c in os.getenv("SHODAN_DISCOVERY_CATEGORIES", "sdr,adsb_receiver,satcom,camera").split(",") if c.strip()]
    cats = [c for c in cats if c in CATEGORY_QUERIES]
    if not cats:
        return {"ok": False, "reason": "no_valid_categories", "fetched": 0}

    region_key = _region_key_from_bbox(bbox)
    geo_clause = _build_geo_clause(bbox)

    total = 0
    for c in cats:
        if (not force_refresh) and (not _is_stale(region_key, c, ttl_sec)):
            continue

        q = CATEGORY_QUERIES[c] + geo_clause
        started = _now_iso()
        status = "ok"
        err = ""
        collected: List[dict] = []
        try:
            resp = requests.get(
                f"{SHODAN_API_BASE}/shodan/host/search",
                params={"key": key, "query": q, "page": 1, "minify": "true"},
                timeout=30,
            )
            if resp.status_code != 200:
                raise RuntimeError(f"shodan_http_{resp.status_code}")
            payload = resp.json()
            for m in (payload.get("matches") or [])[:max_per_query]:
                collected.append(_normalize(m, c, region_key, q))
            _upsert(collected)
            total += len(collected)
            _record_region_state(region_key, c, ttl_sec, len(collected), "ok", "")
        except Exception as e:
            status = "error"
            err = str(e)
            _record_region_state(region_key, c, ttl_sec, 0, status, err)

        with get_conn() as conn:
            conn.execute(
                "INSERT INTO shodan_query_runs (query, bbox, country, region_key, category, limit_requested, result_count, started_at, finished_at, status, error) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                (q, bbox or "", "", region_key, c, max_per_query, len(collected), started, _now_iso(), status, err),
            )
            conn.commit()

    return {"ok": True, "fetched": total, "region_key": region_key, "categories": cats}


def get_shodan_events(
    bbox: Optional[str] = None,
    categories: Optional[List[str]] = None,
    since: Optional[str] = None,
    limit: Optional[int] = None,
    country: Optional[str] = None,
) -> List[dict]:
    max_visible = int(os.getenv("SHODAN_MAX_VISIBLE_RESULTS", "2000"))
    lim = min(int(limit or os.getenv("SHODAN_DEFAULT_LIMIT", "500")), max_visible)

    where = ["1=1"]
    vals: List[object] = []

    if since:
        where.append("datetime(updated_at) > datetime(?)")
        vals.append(since)

    t = _parse_bbox(bbox)
    if t is not None:
        min_lon, min_lat, max_lon, max_lat = t
        where.append("lon BETWEEN ? AND ?")
        vals.extend([min_lon, max_lon])
        where.append("lat BETWEEN ? AND ?")
        vals.extend([min_lat, max_lat])

    if country:
        where.append("(country_name=? OR country_code=?)")
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

    items = []
    for r in rows:
        if r["lat"] is None or r["lon"] is None:
            continue
        items.append({
            "id": r["id"],
            "type": "shodan",
            "lat": r["lat"],
            "lon": r["lon"],
            "category": r["category"],
            "ip": r["ip"],
            "port": r["port"],
            "org": r["org"],
            "isp": r["isp"],
            "asn": r["asn"],
            "product": r["product"],
            "version": r["version"],
            "os": r["os"],
            "tags": r["tags"],
            "vulns": r["vulns"],
            "city": r["city"],
            "region_code": r["region_code"],
            "country_code": r["country_code"],
            "country_name": r["country_name"],
            "timestamp": r["timestamp"],
            "last_seen": r["last_seen"],
            "shodan_url": r["shodan_url"],
            "query": r["query"],
            "region_key": r["region_key"],
            "source": r["source"],
            "style": {"icon": "divIcon", "color": "#8b5cf6", "radius": 6},
            "popup": {
                "title": (r["product"] or "Shodan") + f" • {r['ip']}:{r['port']}",
                "fields": {
                    "category": r["category"],
                    "org": r["org"],
                    "isp": r["isp"],
                    "last_seen": r["last_seen"],
                    "location": f"{r['city'] or '-'}, {r['region_code'] or '-'}, {r['country_name'] or r['country_code'] or '-'}",
                },
                "sources": [{"name": "Shodan", "url": r["shodan_url"] or "https://www.shodan.io/"}],
            },
        })
    return items


def get_shodan_meta() -> Dict[str, object]:
    configured = bool(_shodan_key())
    with get_conn() as conn:
        total_geo = conn.execute("SELECT COUNT(*) FROM shodan_findings WHERE lat IS NOT NULL AND lon IS NOT NULL").fetchone()[0]
        by_cat_rows = conn.execute("SELECT category, COUNT(*) as c FROM shodan_findings GROUP BY category").fetchall()
        last = conn.execute("SELECT MAX(updated_at) FROM shodan_findings").fetchone()[0]
        state_rows = conn.execute("SELECT region_key, category, last_discovery_at, ttl_sec, last_result_count, last_status, last_error FROM shodan_region_cache_state ORDER BY last_discovery_at DESC LIMIT 50").fetchall()
    return {
        "configured": configured,
        "last_discovery_at": last,
        "total_geolocated_findings": int(total_geo or 0),
        "counts_by_category": {r[0]: int(r[1]) for r in by_cat_rows},
        "scheduler_enabled": scheduler_enabled(),
        "cache_state": [dict(r) for r in state_rows],
    }


def get_categories() -> List[str]:
    return list(CATEGORY_QUERIES.keys())


def seed_mock_findings() -> Dict[str, object]:
    now = _now_iso()
    base = [
        {"id": "mock-sdr-1", "category": "sdr", "ip": "198.51.100.10", "port": 8073, "product": "OpenWebRX", "lat": 33.1819, "lon": -96.8877, "city": "Frisco", "region_code": "TX"},
        {"id": "mock-sdr-2", "category": "sdr", "ip": "198.51.100.11", "port": 8073, "product": "KiwiSDR", "lat": 33.1750, "lon": -96.9000, "city": "Frisco", "region_code": "TX"},
        {"id": "mock-adsb-1", "category": "adsb_receiver", "ip": "198.51.100.20", "port": 30003, "product": "dump1090", "lat": 33.1702, "lon": -96.8801, "city": "Plano", "region_code": "TX"},
        {"id": "mock-adsb-2", "category": "adsb_receiver", "ip": "198.51.100.21", "port": 80, "product": "tar1090", "lat": 33.1600, "lon": -96.8700, "city": "Plano", "region_code": "TX"},
        {"id": "mock-sat-1", "category": "satcom", "ip": "198.51.100.30", "port": 443, "product": "iDirect NMS", "lat": 33.1900, "lon": -96.8600, "city": "McKinney", "region_code": "TX"},
        {"id": "mock-sat-2", "category": "satcom", "ip": "198.51.100.31", "port": 443, "product": "Hughes Gateway", "lat": 33.2000, "lon": -96.8450, "city": "McKinney", "region_code": "TX"},
        {"id": "mock-cam-1", "category": "camera", "ip": "198.51.100.40", "port": 554, "product": "Hikvision Camera", "lat": 33.1550, "lon": -96.9050, "city": "Frisco", "region_code": "TX"},
        {"id": "mock-cam-2", "category": "camera", "ip": "198.51.100.41", "port": 80, "product": "Axis Cam UI", "lat": 33.1450, "lon": -96.9150, "city": "Frisco", "region_code": "TX"},
    ]

    with get_conn() as conn:
        for m in base:
            conn.execute(
                """
                INSERT INTO shodan_findings (
                  id, ip, port, transport, org, isp, asn, hostnames, domains, product, version, os, tags, vulns,
                  category, lat, lon, country_code, country_name, city, region_code, timestamp, last_seen,
                  shodan_url, query, source, region_key, inserted_at, updated_at, stale_score
                ) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)
                ON CONFLICT(id) DO UPDATE SET
                  lat=excluded.lat, lon=excluded.lon, category=excluded.category, product=excluded.product,
                  city=excluded.city, region_code=excluded.region_code, source=excluded.source,
                  updated_at=excluded.updated_at, last_seen=excluded.last_seen
                """,
                (
                    m["id"], m["ip"], m["port"], "tcp", "Mock Org", "Mock ISP", "AS65000", "", "",
                    m["product"], "", "", "mock,verification", "", m["category"], m["lat"], m["lon"],
                    "US", "United States", m["city"], m["region_code"], now, now,
                    f"https://www.shodan.io/host/{m['ip']}", "mock seed", "mock_shodan", "tile10:12:8", now, now, 0,
                ),
            )
        conn.commit()
    return {"ok": True, "inserted": len(base), "source": "mock_shodan"}


def clear_mock_findings() -> Dict[str, object]:
    with get_conn() as conn:
        cur = conn.execute("DELETE FROM shodan_findings WHERE source='mock_shodan'")
        deleted = cur.rowcount if cur.rowcount is not None else 0
        conn.commit()
    return {"ok": True, "deleted": int(deleted), "source": "mock_shodan"}


def get_detail(item_id: str) -> Optional[dict]:
    with get_conn() as conn:
        r = conn.execute("SELECT * FROM shodan_findings WHERE id=?", (item_id,)).fetchone()
    return dict(r) if r else None
