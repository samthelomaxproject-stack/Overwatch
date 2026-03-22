import hashlib
import os
import time
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

CATEGORY_PRIORITY = ["sdr", "adsb_receiver", "satcom", "camera"]

# Queries that use geo: filters are credit-consuming on Shodan.
# Plain keyword-only queries on page 1 may be free.
CREDIT_CONSUMING_PREFIXES = ("geo:", "country:", "city:", "net:", "org:", "port:")


def _env_bool(name: str, default: bool) -> bool:
    return os.getenv(name, str(default).lower()).lower() in ("1", "true", "yes", "on")


def _now_iso() -> str:
    return datetime.now(timezone.utc).isoformat()


def _today_key() -> str:
    return datetime.now(timezone.utc).date().isoformat()


def _month_prefix() -> str:
    return datetime.now(timezone.utc).strftime("%Y-%m")


def _shodan_key() -> str:
    return os.getenv("SHODAN_API_KEY", "").strip()


def scheduler_enabled() -> bool:
    return _env_bool("SHODAN_ENABLE_SCHEDULER", False)


def scheduler_interval_sec() -> int:
    return int(os.getenv("SHODAN_DISCOVERY_INTERVAL_SEC", "43200"))


def _budget_enforced() -> bool:
    return _env_bool("SHODAN_ENABLE_BUDGET_ENFORCEMENT", True)


# Credit event limits — only credit-consuming queries count against these.
def _daily_credit_limit() -> int:
    return int(os.getenv("SHODAN_MAX_CREDIT_EVENTS_PER_DAY", "3"))


def _monthly_credit_limit() -> int:
    return int(os.getenv("SHODAN_MAX_CREDIT_EVENTS_PER_MONTH", "90"))


# Safety throttle — prevents runaway discovery regardless of credit budget.
def _min_seconds_between_runs() -> int:
    return int(os.getenv("SHODAN_MIN_SECONDS_BETWEEN_DISCOVERY_RUNS", "300"))


def _max_runs_per_hour() -> int:
    return int(os.getenv("SHODAN_MAX_DISCOVERY_RUNS_PER_HOUR", "6"))


def _query_limit() -> int:
    default_limit = int(os.getenv("SHODAN_DEFAULT_LIMIT", "100"))
    default_limit = max(1, min(100, default_limit))
    hard_cap = int(os.getenv("SHODAN_MAX_RESULTS_PER_QUERY", "200"))
    hard_cap = max(1, min(200, hard_cap))
    return min(default_limit, hard_cap)


def _cache_ttl_sec() -> int:
    return int(os.getenv("SHODAN_REGION_TTL_SEC", "43200"))


def _query_uses_credits(query: str) -> bool:
    """Estimate whether this query will consume Shodan query credits."""
    q = query.lower()
    for prefix in CREDIT_CONSUMING_PREFIXES:
        if prefix in q:
            return True
    return False


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


def _get_credit_usage_counts() -> Dict[str, int]:
    """Count credit-consuming discovery calls (not raw searches)."""
    today = _today_key()
    month = _month_prefix()
    with get_conn() as conn:
        today_used = conn.execute(
            "SELECT queries_used FROM shodan_credit_usage WHERE date=?",
            (today,),
        ).fetchone()
        month_used = conn.execute(
            "SELECT COALESCE(SUM(queries_used), 0) FROM shodan_credit_usage WHERE date LIKE ?",
            (f"{month}%",),
        ).fetchone()[0]
    return {
        "today": int(today_used[0]) if today_used else 0,
        "month": int(month_used or 0),
    }


def _get_recent_run_count(window_secs: int = 3600) -> int:
    """Count discovery runs in recent window for throttle enforcement."""
    cutoff = _now_iso()
    try:
        cutoff_dt = datetime.now(timezone.utc).timestamp() - window_secs
        cutoff_iso = datetime.fromtimestamp(cutoff_dt, tz=timezone.utc).isoformat()
    except Exception:
        return 0
    with get_conn() as conn:
        row = conn.execute(
            "SELECT COUNT(*) FROM shodan_query_runs WHERE started_at > ?",
            (cutoff_iso,),
        ).fetchone()
    return int(row[0]) if row else 0


def _get_last_run_time() -> Optional[float]:
    """Return epoch seconds of most recent discovery run, or None."""
    with get_conn() as conn:
        row = conn.execute(
            "SELECT MAX(started_at) FROM shodan_query_runs",
        ).fetchone()
    if not row or not row[0]:
        return None
    try:
        dt = datetime.fromisoformat(str(row[0]).replace("Z", "+00:00"))
        return dt.timestamp()
    except Exception:
        return None


def _check_safety_throttle() -> Dict[str, object]:
    """Check per-run throttle limits (independent of credit budget)."""
    min_gap = _min_seconds_between_runs()
    max_per_hour = _max_runs_per_hour()

    last_run = _get_last_run_time()
    if last_run is not None:
        elapsed = time.time() - last_run
        if elapsed < min_gap:
            return {
                "ok": False,
                "reason": "throttle_min_gap",
                "message": f"Discovery throttled: {int(min_gap - elapsed)}s until next run allowed",
            }

    recent_runs = _get_recent_run_count(window_secs=3600)
    if recent_runs >= max_per_hour:
        return {
            "ok": False,
            "reason": "throttle_hourly_limit",
            "message": f"Discovery throttled: {recent_runs}/{max_per_hour} runs in last hour",
        }

    return {"ok": True}


def _reserve_credit_event_or_block(query: str) -> Dict[str, object]:
    """Reserve a credit event slot if this query is credit-consuming. Always allows non-credit queries."""
    if not _query_uses_credits(query):
        return {"ok": True, "reason": "non_credit_query", "credit_event": False}

    if not _budget_enforced():
        return {"ok": True, "reason": "budget_disabled", "credit_event": True}

    daily_limit = _daily_credit_limit()
    monthly_limit = _monthly_credit_limit()
    usage = _get_credit_usage_counts()

    if usage["today"] + 1 > daily_limit:
        return {
            "ok": False,
            "status": "credit_budget_exceeded",
            "message": "Discovery skipped: daily credit event limit reached",
            "scope": "daily",
            "credit_events_used_today": usage["today"],
            "daily_credit_event_limit": daily_limit,
        }
    if usage["month"] + 1 > monthly_limit:
        return {
            "ok": False,
            "status": "credit_budget_exceeded",
            "message": "Discovery skipped: monthly credit event limit reached",
            "scope": "monthly",
            "credit_events_used_month": usage["month"],
            "monthly_credit_event_limit": monthly_limit,
        }

    with get_conn() as conn:
        conn.execute(
            """
            INSERT INTO shodan_credit_usage(date, queries_used)
            VALUES (?, 1)
            ON CONFLICT(date) DO UPDATE SET queries_used=queries_used + 1
            """,
            (_today_key(),),
        )
        conn.commit()
    return {"ok": True, "reason": "reserved", "credit_event": True}


def _record_query_run(query: str, region_key: str, category: str, limit_requested: int, result_count: int, started_at: str, status: str, error: str = ""):
    with get_conn() as conn:
        conn.execute(
            "INSERT INTO shodan_query_runs (query, bbox, country, region_key, category, limit_requested, result_count, started_at, finished_at, status, error) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            (query, "", "", region_key, category, limit_requested, result_count, started_at, _now_iso(), status, error),
        )
        conn.commit()


def _region_key_from_bbox(bbox: Optional[str]) -> str:
    """Generate a coarse region key from bbox for cache state tracking."""
    if not bbox:
        return "scheduler"
    try:
        parts = [float(x.strip()) for x in bbox.split(",")]
        if len(parts) == 4:
            min_lon, min_lat, max_lon, max_lat = parts
            c_lat = (min_lat + max_lat) / 2.0
            c_lon = (min_lon + max_lon) / 2.0
            lat_bucket = int((c_lat + 90) // 5)
            lon_bucket = int((c_lon + 180) // 5)
            return f"tile5:{lat_bucket}:{lon_bucket}"
    except Exception:
        pass
    return "bbox_custom"


def _geo_clause_from_bbox(bbox: Optional[str]) -> str:
    """Build a Shodan geo: clause from a bbox. Falls back to default region."""
    if not bbox:
        return os.getenv("SHODAN_DEFAULT_REGION_QUERY", "geo:33.1500,-96.9000,220").strip()
    try:
        parts = [float(x.strip()) for x in bbox.split(",")]
        if len(parts) == 4:
            min_lon, min_lat, max_lon, max_lat = parts
            c_lat = (min_lat + max_lat) / 2.0
            c_lon = (min_lon + max_lon) / 2.0
            lat_span_km = abs(max_lat - min_lat) * 111.0
            lon_span_km = abs(max_lon - min_lon) * 111.0 * abs(c_lat * 3.14159 / 180)
            radius = max(30, min(400, int(max(lat_span_km, lon_span_km) / 2.0)))
            return f"geo:{c_lat:.4f},{c_lon:.4f},{radius}"
    except Exception:
        pass
    return os.getenv("SHODAN_DEFAULT_REGION_QUERY", "geo:33.1500,-96.9000,220").strip()


def discover_shodan(bbox: Optional[str] = None, categories: Optional[List[str]] = None, force_refresh: bool = False) -> Dict[str, object]:
    key = _shodan_key()
    if not key:
        return {"ok": False, "reason": "not_configured", "fetched": 0}

    ttl_sec = _cache_ttl_sec()
    limit_per_query = _query_limit()

    region_key = _region_key_from_bbox(bbox)
    geo_clause = _geo_clause_from_bbox(bbox)

    requested = categories or [c.strip() for c in os.getenv("SHODAN_DISCOVERY_CATEGORIES", "sdr,adsb_receiver,satcom,camera").split(",") if c.strip()]
    requested_valid = [c for c in requested if c in CATEGORY_QUERIES]

    cats: List[str] = []
    for p in CATEGORY_PRIORITY:
        if p in requested_valid:
            cats.append(p)

    if not cats:
        return {"ok": False, "reason": "no_valid_categories", "fetched": 0}

    total = 0
    queried_categories: List[str] = []
    credit_events_used = 0
    blocked = None

    for c in cats:
        if (not force_refresh) and (not _is_stale(region_key, c, ttl_sec)):
            continue

        q = f"{CATEGORY_QUERIES[c]} {geo_clause}".strip()
        uses_credits = _query_uses_credits(q)

        # Credit budget gate — only for credit-consuming queries.
        if uses_credits:
            reserve = _reserve_credit_event_or_block(q)
            if not reserve.get("ok"):
                blocked = reserve
                _record_region_state(region_key, c, ttl_sec, 0, "credit_budget_exceeded", reserve.get("message", "credit budget"))
                break
            credit_events_used += 1

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
            for m in (payload.get("matches") or [])[:limit_per_query]:
                collected.append(_normalize(m, c, region_key, q))
            _upsert(collected)
            _record_region_state(region_key, c, ttl_sec, len(collected), "ok", "")
            total += len(collected)
            queried_categories.append(c)
        except Exception as e:
            status = "error"
            err = str(e)
            _record_region_state(region_key, c, ttl_sec, 0, status, err)

        _record_query_run(q, region_key, c, limit_per_query, len(collected), started, status, err)

    result: Dict[str, object] = {
        "ok": True,
        "fetched": total,
        "region_key": region_key,
        "categories": cats,
        "queried_categories": queried_categories,
        "credit_events_used": credit_events_used,
        "cache_only": False,
    }
    if blocked:
        result["status"] = "credit_budget_exceeded"
        result["message"] = "Discovery skipped: credit event budget reached"
        result["budget_detail"] = blocked
    return result


def get_shodan_events(
    bbox: Optional[str] = None,
    categories: Optional[List[str]] = None,
    since: Optional[str] = None,
    limit: Optional[int] = None,
    country: Optional[str] = None,
) -> List[dict]:
    # Cache-first; no Shodan API call. No region lock — returns all cached findings matching filters.
    max_visible = int(os.getenv("SHODAN_MAX_VISIBLE_RESULTS", "2000"))
    default_limit = int(os.getenv("SHODAN_DEFAULT_LIMIT", "500"))
    lim = min(max(1, int(limit or default_limit)), max_visible)

    where = ["lat IS NOT NULL", "lon IS NOT NULL"]
    vals: List[object] = []

    # bbox filter — spatial query on cached findings (no region_key lock).
    if bbox:
        try:
            parts = [float(x.strip()) for x in bbox.split(",")]
            if len(parts) == 4:
                min_lon, min_lat, max_lon, max_lat = parts
                where.append("lon BETWEEN ? AND ?")
                vals.extend([min_lon, max_lon])
                where.append("lat BETWEEN ? AND ?")
                vals.extend([min_lat, max_lat])
        except Exception:
            pass

    if since:
        where.append("datetime(updated_at) > datetime(?)")
        vals.append(since)

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
            "source": "Shodan (Hub Cache)",
            "style": {"icon": "divIcon", "color": "#8b5cf6", "radius": 6},
        })
    return items


def _fetch_shodan_api_info() -> Optional[Dict[str, object]]:
    """Fetch live credit info from /api-info. Does NOT consume credits."""
    key = _shodan_key()
    if not key:
        return None
    try:
        resp = requests.get(f"{SHODAN_API_BASE}/api-info", params={"key": key}, timeout=10)
        if resp.status_code == 200:
            return resp.json()
    except Exception:
        pass
    return None


def get_shodan_meta() -> Dict[str, object]:
    configured = bool(_shodan_key())
    usage = _get_credit_usage_counts()

    with get_conn() as conn:
        total_cached = conn.execute("SELECT COUNT(*) FROM shodan_findings").fetchone()[0]
        total_geo = conn.execute("SELECT COUNT(*) FROM shodan_findings WHERE lat IS NOT NULL AND lon IS NOT NULL").fetchone()[0]
        by_cat_rows = conn.execute("SELECT category, COUNT(*) as c FROM shodan_findings GROUP BY category").fetchall()
        last = conn.execute("SELECT MAX(updated_at) FROM shodan_findings").fetchone()[0]
        state_rows = conn.execute("SELECT region_key, category, last_discovery_at, ttl_sec, last_result_count, last_status, last_error FROM shodan_region_cache_state ORDER BY last_discovery_at DESC LIMIT 50").fetchall()

    credit_budget = {
        "note": "Tracks credit-consuming discovery calls (queries with geo:/filter). Non-credit queries are throttle-gated only.",
        "enforcement_enabled": _budget_enforced(),
        "daily_credit_event_limit": _daily_credit_limit(),
        "monthly_credit_event_limit": _monthly_credit_limit(),
        "credit_events_used_today": usage["today"],
        "credit_events_used_month": usage["month"],
        "credit_events_remaining_today": max(0, _daily_credit_limit() - usage["today"]),
        "credit_events_remaining_month": max(0, _monthly_credit_limit() - usage["month"]),
        "safety_throttle": {
            "min_seconds_between_runs": _min_seconds_between_runs(),
            "max_runs_per_hour": _max_runs_per_hour(),
        },
    }

    shodan_account: Dict[str, object] = {"available": False}
    api_info = _fetch_shodan_api_info()
    if api_info:
        shodan_account = {
            "available": True,
            "plan": api_info.get("plan"),
            "shodan_query_credits_remaining": api_info.get("query_credits"),
            "shodan_scan_credits_remaining": api_info.get("scan_credits"),
            "unlocked_left": api_info.get("unlocked_left"),
        }

    return {
        "configured": configured,
        "last_discovery_time": last,
        "last_discovery_at": last,  # alias for backward compatibility
        "total_cached_findings": int(total_cached or 0),
        "total_geolocated_findings": int(total_geo or 0),
        "counts_by_category": {r[0]: int(r[1]) for r in by_cat_rows},
        "scheduler_enabled": scheduler_enabled(),
        "cache_state": [dict(r) for r in state_rows],
        "internal_credit_budget": credit_budget,
        "shodan_account": shodan_account,
        "zoom_thresholds": {
            "min_zoom_display": int(os.getenv("SHODAN_MIN_ZOOM_DISPLAY", "11")),
            "min_zoom_discovery": int(os.getenv("SHODAN_MIN_ZOOM_DISCOVERY", "12")),
            "note": "Frontend enforces these thresholds. Below min_zoom_display: markers cleared. Below min_zoom_discovery: cache not refreshed.",
        },
        "region_lock": False,
        "cache_only_serving": True,
    }


def get_categories() -> List[str]:
    return list(CATEGORY_QUERIES.keys())


def seed_mock_findings() -> Dict[str, object]:
    now = _now_iso()
    base = [
        {"id": "mock-sdr-1", "category": "sdr", "ip": "198.51.100.10", "port": 8073, "product": "OpenWebRX", "lat": 33.1819, "lon": -96.8877, "city": "Frisco", "region_code": "TX"},
        {"id": "mock-sdr-2", "category": "sdr", "ip": "198.51.100.11", "port": 8073, "product": "KiwiSDR", "lat": 33.1750, "lon": -96.9000, "city": "Frisco", "region_code": "TX"},
        {"id": "mock-adsb-1", "category": "adsb_receiver", "ip": "198.51.100.20", "port": 30003, "product": "dump1090", "lat": 33.1702, "lon": -96.8801, "city": "Plano", "region_code": "TX"},
        {"id": "mock-sat-1", "category": "satcom", "ip": "198.51.100.30", "port": 443, "product": "iDirect NMS", "lat": 33.1900, "lon": -96.8600, "city": "McKinney", "region_code": "TX"},
        {"id": "mock-cam-1", "category": "camera", "ip": "198.51.100.40", "port": 554, "product": "Hikvision Camera", "lat": 33.1550, "lon": -96.9050, "city": "Frisco", "region_code": "TX"},
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
                    f"https://www.shodan.io/host/{m['ip']}", "mock seed", "mock_shodan", "mock", now, now, 0,
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
