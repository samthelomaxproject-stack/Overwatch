import hashlib
import os
from datetime import datetime, timezone
from typing import Dict, List, Optional

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


def _daily_budget() -> int:
    return int(os.getenv("SHODAN_MAX_QUERY_CREDITS_PER_DAY", "3"))


def _monthly_budget() -> int:
    return int(os.getenv("SHODAN_MAX_QUERY_CREDITS_PER_MONTH", "90"))


def _query_cost_estimate() -> int:
    return int(os.getenv("SHODAN_QUERY_COST_ESTIMATE", "1"))


def _default_region_key() -> str:
    return os.getenv("SHODAN_DEFAULT_REGION_KEY", "US-TX-NORTH").strip() or "US-TX-NORTH"


def _default_region_query_clause() -> str:
    # Coarse static region for low-cost discovery; intentionally not viewport-driven.
    return os.getenv("SHODAN_DEFAULT_REGION_QUERY", "geo:33.1500,-96.9000,220").strip()


def _query_limit() -> int:
    default_limit = int(os.getenv("SHODAN_DEFAULT_LIMIT", "100"))
    default_limit = max(1, min(100, default_limit))
    hard_cap = int(os.getenv("SHODAN_MAX_RESULTS_PER_QUERY", "200"))
    hard_cap = max(1, min(200, hard_cap))
    return min(default_limit, hard_cap)


def _cache_ttl_sec() -> int:
    return int(os.getenv("SHODAN_REGION_TTL_SEC", "43200"))


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


def _get_usage_counts() -> Dict[str, int]:
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


def _reserve_credit_or_block() -> Dict[str, object]:
    if not _budget_enforced():
        return {"ok": True, "reason": "budget_disabled"}

    daily_limit = _daily_budget()
    monthly_limit = _monthly_budget()
    cost = _query_cost_estimate()
    usage = _get_usage_counts()

    if usage["today"] + cost > daily_limit:
        return {
            "ok": False,
            "status": "budget_exceeded",
            "message": "Shodan query skipped due to credit limits",
            "scope": "daily",
            "today": usage["today"],
            "daily_limit": daily_limit,
        }
    if usage["month"] + cost > monthly_limit:
        return {
            "ok": False,
            "status": "budget_exceeded",
            "message": "Shodan query skipped due to credit limits",
            "scope": "monthly",
            "month": usage["month"],
            "monthly_limit": monthly_limit,
        }

    with get_conn() as conn:
        conn.execute(
            """
            INSERT INTO shodan_credit_usage(date, queries_used)
            VALUES (?, ?)
            ON CONFLICT(date) DO UPDATE SET queries_used=queries_used + ?
            """,
            (_today_key(), cost, cost),
        )
        conn.commit()
    return {"ok": True, "reason": "reserved"}


def _record_query_run(query: str, region_key: str, category: str, limit_requested: int, result_count: int, started_at: str, status: str, error: str = ""):
    with get_conn() as conn:
        conn.execute(
            "INSERT INTO shodan_query_runs (query, bbox, country, region_key, category, limit_requested, result_count, started_at, finished_at, status, error) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            (query, "", "", region_key, category, limit_requested, result_count, started_at, _now_iso(), status, error),
        )
        conn.commit()


def discover_shodan(bbox: Optional[str] = None, categories: Optional[List[str]] = None, force_refresh: bool = False) -> Dict[str, object]:
    key = _shodan_key()
    if not key:
        return {"ok": False, "reason": "not_configured", "fetched": 0}

    ttl_sec = _cache_ttl_sec()
    limit_per_query = _query_limit()  # hard-capped at <= 200 and defaults to 100
    region_key = _default_region_key()
    region_clause = _default_region_query_clause()

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
    blocked = None

    for c in cats:
        if (not force_refresh) and (not _is_stale(region_key, c, ttl_sec)):
            continue

        reserve = _reserve_credit_or_block()
        if not reserve.get("ok"):
            blocked = reserve
            _record_region_state(region_key, c, ttl_sec, 0, "budget_exceeded", reserve.get("message", "budget limit"))
            break

        q = f"{CATEGORY_QUERIES[c]} {region_clause}".strip()
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
        "cache_only": False,
    }
    if blocked:
        result["status"] = "budget_exceeded"
        result["message"] = "Shodan query skipped due to credit limits"
        result["budget"] = blocked
    return result


def get_shodan_events(
    bbox: Optional[str] = None,
    categories: Optional[List[str]] = None,
    since: Optional[str] = None,
    limit: Optional[int] = None,
    country: Optional[str] = None,
) -> List[dict]:
    # cache-first retrieval only; no Shodan API call from this function.
    max_visible = int(os.getenv("SHODAN_MAX_VISIBLE_RESULTS", "2000"))
    default_limit = int(os.getenv("SHODAN_DEFAULT_LIMIT", "100"))
    lim = min(max(1, int(limit or default_limit)), max_visible)

    where = ["region_key = ?"]
    vals: List[object] = [_default_region_key()]

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
            "source": "Shodan (Hub Cache)",
            "style": {"icon": "divIcon", "color": "#8b5cf6", "radius": 6},
        })
    return items


def _fetch_shodan_api_info() -> Optional[Dict[str, object]]:
    """Fetch live credit info from Shodan /api-info. Returns None on failure. Does NOT consume credits."""
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
    usage = _get_usage_counts()
    with get_conn() as conn:
        total_geo = conn.execute("SELECT COUNT(*) FROM shodan_findings WHERE lat IS NOT NULL AND lon IS NOT NULL AND region_key=?", (_default_region_key(),)).fetchone()[0]
        by_cat_rows = conn.execute("SELECT category, COUNT(*) as c FROM shodan_findings WHERE region_key=? GROUP BY category", (_default_region_key(),)).fetchall()
        last = conn.execute("SELECT MAX(updated_at) FROM shodan_findings WHERE region_key=?", (_default_region_key(),)).fetchone()[0]
        state_rows = conn.execute("SELECT region_key, category, last_discovery_at, ttl_sec, last_result_count, last_status, last_error FROM shodan_region_cache_state WHERE region_key=? ORDER BY last_discovery_at DESC LIMIT 50", (_default_region_key(),)).fetchall()

    # Internal search budget (counts our discovery calls, not necessarily Shodan-deducted credits).
    # Shodan only deducts query credits for filtered searches or page > 1.
    # Our geo: queries may or may not consume credits depending on Shodan's filter classification.
    search_budget = {
        "note": "Internal app search budget — counts discovery API calls made, not actual Shodan query credits deducted",
        "enforcement_enabled": _budget_enforced(),
        "daily_search_limit": _daily_budget(),
        "monthly_search_limit": _monthly_budget(),
        "estimated_credit_cost_per_search": _query_cost_estimate(),
        "searches_used_today": usage["today"],
        "searches_used_month": usage["month"],
        "searches_remaining_today": max(0, _daily_budget() - usage["today"]),
        "searches_remaining_month": max(0, _monthly_budget() - usage["month"]),
    }

    # Live Shodan account credit info (cached for meta, does not consume credits).
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
        "default_region_key": _default_region_key(),
        "last_discovery_at": last,
        "total_geolocated_findings": int(total_geo or 0),
        "counts_by_category": {r[0]: int(r[1]) for r in by_cat_rows},
        "scheduler_enabled": scheduler_enabled(),
        "cache_state": [dict(r) for r in state_rows],
        "internal_search_budget": search_budget,
        "shodan_account": shodan_account,
    }


def get_categories() -> List[str]:
    return list(CATEGORY_QUERIES.keys())


def seed_mock_findings() -> Dict[str, object]:
    now = _now_iso()
    region_key = _default_region_key()
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
                    f"https://www.shodan.io/host/{m['ip']}", "mock seed", "mock_shodan", region_key, now, now, 0,
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
