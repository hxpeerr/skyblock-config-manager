import argparse
import concurrent.futures
import datetime as dt
import difflib
import json
import os
import re
import shutil
import subprocess
import sys
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path

import extract

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
WORK = ROOT / ".modcheck"
SNAPSHOTS = HERE / "snapshots"
UA = "skyblock-config-manager-modcheck"
SPARSE = ["*.kt", "*.java", "en_us.json", "en_US.json", "defaults.json"]
MODRINTH = "https://api.modrinth.com/v2"
CFWIDGET = "https://api.cfwidget.com"
CURSEFORGE = "https://api.curseforge.com/v1"
GITHUB = "https://api.github.com"
LIST_LIMIT = 60
BODY_LIMIT = 60000


class Report:
    def __init__(self):
        self.actions = []
        self.notes = []
        self.problems = []
        self.counts = {}

    def act(self, kind, title, lines):
        if lines:
            self.actions.append((title, lines))
            self.counts[kind] = self.counts.get(kind, 0) + len(lines)


def today():
    return dt.datetime.now(dt.timezone.utc).date()


def vt(v):
    return tuple(int(x) for x in re.findall(r"\d+", v or ""))


def is_release(v):
    return re.fullmatch(r"\d+\.\d+(?:\.\d+)?", v or "") is not None


def version_in(version, spec):
    lo, _, hi = spec.partition("-")
    return vt(lo) <= vt(version) <= vt(hi or lo)


def picked(spec, version):
    if isinstance(spec, dict):
        for k, v in spec.items():
            if k != "other" and version_in(version, k):
                return v
        return spec.get("other")
    return spec


def as_list(v):
    return [] if v is None else (list(v) if isinstance(v, list) else [v])


def matches(mapped, keys):
    if mapped.startswith("*"):
        tail = mapped[1:]
        return [k for k in keys if k == tail or k.endswith("." + tail)]
    return [mapped] if mapped in keys else []


def norm_repo(url):
    url = re.sub(r"/(?:tree|blob)/.*$", "", (url or "").strip().lower())
    url = url.removesuffix("/").removesuffix(".git")
    return re.sub(r"^https?://(www\.)?", "", url)


def norm_name(name):
    return re.sub(r"[^a-z0-9]", "", (name or "").lower())


def code(s):
    return "`" + s.replace("`", "'") + "`"


def load_jsonc(path):
    return json.loads(extract.Source(path).clean)


def load_json(path, default):
    try:
        return json.loads(Path(path).read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return default


def run(cmd, timeout=600):
    env = dict(os.environ, GIT_TERMINAL_PROMPT="0")
    return subprocess.run(cmd, check=True, capture_output=True, text=True, timeout=timeout, env=env).stdout.strip()


def http(url, headers=None, data=None, timeout=30):
    body = json.dumps(data).encode() if data is not None else None
    req = urllib.request.Request(url, data=body, headers={"User-Agent": UA, "Accept": "application/json", **(headers or {})})
    if body is not None:
        req.add_header("Content-Type", "application/json")
    with urllib.request.urlopen(req, timeout=timeout) as r:
        return json.loads(r.read().decode("utf-8"))


def github_headers():
    token = os.environ.get("GH_TOKEN") or os.environ.get("GITHUB_TOKEN")
    return {"Authorization": f"Bearer {token}", "X-GitHub-Api-Version": "2022-11-28"} if token else {}


def curseforge_key():
    return os.environ.get("CURSEFORGE_API_KEY", "").strip()


def describe(e):
    if isinstance(e, urllib.error.HTTPError):
        return f"HTTP {e.code}"
    if isinstance(e, subprocess.CalledProcessError):
        return (e.stderr or e.stdout or str(e)).strip().splitlines()[-1][:200] if (e.stderr or e.stdout) else str(e)
    return str(e)[:200]


MC_SUFFIX = re.compile(r"[-_](?:mc-?)?(?:1\.\d{2}|\d{2}\.\d+)(?:\.(?:\d+|x))*$")


def version_core(v):
    return MC_SUFFIX.sub("", (v or "").strip().lower().removeprefix("v").split("+")[0])


def tags_of(repo):
    tags = [line.split("refs/tags/")[-1] for line in run(["git", "ls-remote", "--tags", "--refs", repo], 120).splitlines()]
    return [t for t in tags if re.search(r"\d+\.\d+", t)]


def released_from(repo, versions, commit):
    if not repo or not versions:
        return False
    try:
        lines = run(["git", "ls-remote", "--tags", repo], 120).splitlines()
    except (subprocess.SubprocessError, OSError):
        return False
    wanted = {version_core(v) for v in versions}
    for line in lines:
        sha, _, ref = line.partition("\t")
        tag = ref.removeprefix("refs/tags/").removesuffix("^{}")
        if sha == commit and version_core(tag) in wanted:
            return True
    return False


def release_ref(spec, slug):
    tags = tags_of(spec["repo"])
    if not tags:
        return None
    if slug:
        try:
            latest = modrinth_latest(slug)
        except (urllib.error.URLError, OSError, ValueError, KeyError):
            latest = None
        if latest:
            want = version_core(latest["version"])
            same = [t for t in tags if version_core(t) == want]
            if same:
                return same[0]
    return max(tags, key=vt)


def clone(name, spec, slug=None):
    dest = WORK / "src" / name
    last = None
    for _ in range(2):
        try:
            if dest.exists():
                shutil.rmtree(dest)
            dest.parent.mkdir(parents=True, exist_ok=True)
            cmd = ["git", "clone", "--quiet", "--depth", "1", "--filter=blob:none", "--sparse"]
            ref = spec.get("branch")
            if ref == "release":
                ref = release_ref(spec, slug)
            if ref:
                cmd += ["--branch", ref]
            run(cmd + [spec["repo"], str(dest)])
            run(["git", "-C", str(dest), "sparse-checkout", "set", "--no-cone", *SPARSE])
            return source_info(dest)
        except (subprocess.SubprocessError, OSError) as e:
            last = e
    raise RuntimeError(f"couldn't download the source: {describe(last)}")


def source_info(dest):
    sha, date = run(["git", "-C", str(dest), "log", "-1", "--format=%H %cI"]).split()
    branch = run(["git", "-C", str(dest), "rev-parse", "--abbrev-ref", "HEAD"])
    if branch == "HEAD":
        branch = run(["git", "-C", str(dest), "describe", "--tags", "--always"])
    return {"commit": sha, "date": date[:10], "branch": branch}


def read_settings(name, spec, fetch, slug=None):
    dest = WORK / "src" / name
    info = clone(name, spec, slug) if fetch else (source_info(dest) if dest.is_dir() else None)
    if info is None:
        raise RuntimeError("no downloaded source (run without --no-fetch)")
    try:
        rows = extract.extract(spec["kind"], dest)
    except Exception as e:
        raise LookupError(f"couldn't read its settings ({e}), so tools/modcheck/extract.py needs updating") from e
    if not rows and spec["kind"] != "none":
        raise LookupError("no settings found in its source, so tools/modcheck/extract.py needs updating")
    catalog = WORK / "catalog"
    catalog.mkdir(parents=True, exist_ok=True)
    with open(catalog / f"{name}.tsv", "w", encoding="utf-8", newline="\n") as f:
        for row in rows:
            f.write("\t".join(row) + "\n")
    return info, rows


def read_snapshot(name):
    path = SNAPSHOTS / f"{name}.tsv"
    if not path.exists():
        return None
    out = {}
    for line in path.read_text(encoding="utf-8").splitlines():
        if line.strip():
            key, _, typ = line.partition("\t")
            out[key] = typ
    return out


def write_snapshot(name, rows):
    SNAPSHOTS.mkdir(parents=True, exist_ok=True)
    with open(SNAPSHOTS / f"{name}.tsv", "w", encoding="utf-8", newline="\n") as f:
        for key, typ, _ in rows:
            f.write(f"{key}\t{typ}\n")


def mapped_keys(features, newest):
    out = {}
    for f in features["features"]:
        for mod, spec in f["mods"].items():
            for key in as_list(picked(spec, newest)):
                out.setdefault(mod, {}).setdefault(key, f"{f['area']} / {f['name']}")
    return out


def check_mapped(report, features, settings, old_names, newest):
    missing, not_switch, stale_old, back_old = [], [], [], []
    wanted = mapped_keys(features, newest)
    every = {}
    for f in features["features"]:
        for mod, spec in f["mods"].items():
            for variant in (spec.values() if isinstance(spec, dict) else [spec]):
                every.setdefault(mod, set()).update(as_list(variant))
    for mod, keys in wanted.items():
        if mod not in settings:
            continue
        types = settings[mod]
        olds = set(old_names.get(mod, []))
        for key, feature in keys.items():
            hits = matches(key, types)
            if not hits:
                if key not in olds:
                    missing.append(f"{mod}: {code(key)} ({feature})")
            elif not any(types[h] == extract.BOOL for h in hits):
                not_switch.append(f"{mod}: {code(key)} is now {', '.join(sorted({types[h] for h in hits}))}, not on/off ({feature})")
    for mod, keys in old_names.items():
        for key in keys:
            if key not in every.get(mod, set()):
                stale_old.append(f"{mod}: {code(key)}")
            elif mod in settings and matches(key, settings[mod]):
                back_old.append(f"{mod}: {code(key)}")
    report.act("missing", "Mapped settings that are no longer in the mod's source. Add the new name next to the old one in features.jsonc, then add the old name to tools/modcheck/old_names.json", missing)
    report.act("not_switch", "Mapped settings that are no longer on/off settings", not_switch)
    if stale_old:
        report.notes.append(("Entries in old_names.json that features.jsonc no longer uses", stale_old))
    if back_old:
        report.notes.append(("Entries in old_names.json that are back in the mod's source", back_old))


def check_changes(report, settings, names, snapshots):
    blocks = []
    for mod, types in settings.items():
        before = snapshots.get(mod)
        if before is None:
            blocks.append(f"<details><summary>{mod}: no snapshot yet</summary>\n\nRun <code>python tools/modcheck/check.py --update</code> to save one.\n</details>")
            continue
        added = [k for k in types if k not in before]
        removed = [k for k in before if k not in types]
        flipped = [k for k in types if k in before and (types[k] == extract.BOOL) != (before[k] == extract.BOOL)]
        if not (added or removed or flipped):
            continue
        lines = []
        on_off = [k for k in added if types[k] == extract.BOOL]
        other = [k for k in added if types[k] != extract.BOOL]
        if on_off:
            lines.append(f"New on/off settings ({len(on_off)}):")
            lines += [f"- {code(k)} {names[mod].get(k, '')}".rstrip() for k in on_off[:LIST_LIMIT]]
            if len(on_off) > LIST_LIMIT:
                lines.append(f"- and {len(on_off) - LIST_LIMIT} more")
        if other:
            shown = ", ".join(code(k) for k in other[:LIST_LIMIT])
            lines.append(f"\nOther new settings ({len(other)}): {shown}{' and more' if len(other) > LIST_LIMIT else ''}")
        if removed:
            shown = ", ".join(code(k) for k in removed[:LIST_LIMIT])
            lines.append(f"\nRemoved ({len(removed)}): {shown}{' and more' if len(removed) > LIST_LIMIT else ''}")
        if flipped:
            lines.append("\nChanged between on/off and another type: " + ", ".join(code(k) for k in flipped[:LIST_LIMIT]))
        renames = []
        for r in removed:
            same = [a for a in added if a.split(".")[-1] == r.split(".")[-1]]
            if same:
                best = max(same, key=lambda a: difflib.SequenceMatcher(None, r, a).ratio())
                renames.append(f"{code(r)} → {code(best)}")
        if renames:
            lines.append("\nPossible renames: " + ", ".join(renames[:LIST_LIMIT]))
        summary = f"{mod}: {len(added)} new, {len(removed)} removed" + (f", {len(flipped)} changed type" if flipped else "")
        blocks.append(f"<details><summary>{summary}</summary>\n\n" + "\n".join(lines) + "\n</details>")
    if blocks:
        report.actions.append(("Settings that changed since the last update. Map new on/off settings that do the same thing as another mod's, then run `python tools/modcheck/check.py --update`", blocks))
        report.counts["changed"] = len(blocks)


def modrinth_latest(slug):
    q = urllib.parse.urlencode({"loaders": json.dumps(["fabric"]), "include_changelog": "false"})
    versions = http(f"{MODRINTH}/project/{urllib.parse.quote(slug)}/version?{q}")
    if not versions:
        return None
    v = versions[0]
    return {"version": v["version_number"], "date": v["date_published"][:10], "game_versions": v.get("game_versions", []),
            "url": f"https://modrinth.com/mod/{slug}/version/{v['id']}"}


CF_NAME = re.compile(r"[-_ ]v?(\d+(?:\.\d+)+\S*)$")
CF_DISPLAY = re.compile(r"(\d+(?:\.\d+)+[\w.+-]*)")


def cf_version(file_name, display):
    m = CF_NAME.search(re.sub(r"(?i)\.jar$", "", file_name or ""))
    if m:
        return m.group(1)
    m = CF_DISPLAY.search(display or "")
    return m.group(1) if m else (display or file_name or "?")


def curseforge_project(pid):
    key = curseforge_key()
    if key:
        d = http(f"{CURSEFORGE}/mods/{pid}", {"x-api-key": key})["data"]
        files = [{"name": f.get("fileName"), "display": f.get("displayName"), "date": f.get("fileDate", ""), "versions": f.get("gameVersions", [])}
                 for f in d.get("latestFiles", [])]
        links = d.get("links") or {}
        return {"id": d["id"], "title": d.get("name", ""), "url": links.get("websiteUrl", ""), "source": links.get("sourceUrl") or "",
                "files": files, "downloads": d.get("downloadCount", 0)}
    d = http(f"{CFWIDGET}/{pid}")
    return cfwidget_project(d)


def cfwidget_project(d):
    files = [{"name": f.get("name"), "display": f.get("display"), "date": f.get("uploaded_at", ""), "versions": f.get("versions", [])}
             for f in d.get("files", [])]
    urls = d.get("urls") or {}
    return {"id": d.get("id"), "title": d.get("title", ""), "url": urls.get("curseforge") or urls.get("project", ""), "source": "",
            "files": files, "downloads": (d.get("downloads") or {}).get("total", 0)}


def curseforge_latest(project):
    files = [f for f in project["files"] if "Fabric" in f["versions"]]
    if not files:
        return None
    f = max(files, key=lambda f: f["date"] or "")
    return {"version": cf_version(f["name"], f["display"]), "date": (f["date"] or "")[:10],
            "game_versions": [v for v in f["versions"] if re.match(r"\d", v)], "url": project["url"]}


def curseforge_lookup(name, mod, repo):
    tried = []
    for slug in dict.fromkeys(filter(None, [mod.get("slug"), mod.get("id"), re.sub(r"[^a-z0-9]+", "-", name.lower()).strip("-")])):
        key = curseforge_key()
        try:
            if key:
                found = http(f"{CURSEFORGE}/mods/search?" + urllib.parse.urlencode({"gameId": 432, "classId": 6, "slug": slug}), {"x-api-key": key})["data"]
                projects = [curseforge_project(p["id"]) for p in found]
            else:
                projects = [cfwidget_project(http(f"{CFWIDGET}/minecraft/mc-mods/{slug}"))]
        except (urllib.error.URLError, OSError, ValueError, KeyError):
            tried.append(slug)
            continue
        for p in projects:
            same_repo = repo and norm_repo(p["source"]) == norm_repo(repo)
            ident = norm_name(mod.get("id") or name)
            named = any("Fabric" in f["versions"] and ident and ident in norm_name(f["name"]) for f in p["files"])
            if same_repo or (named and norm_name(name) in norm_name(p["title"])):
                return p
    return None


def repo_parts(url):
    m = re.match(r"https?://(github\.com|codeberg\.org)/([^/]+)/([^/]+?)(?:\.git)?(?:/(?:tree|blob)/.*)?/?$", url or "")
    return m.groups() if m else (None, None, None)


def forge_latest(url):
    host, owner, name = repo_parts(url)
    try:
        if host == "github.com":
            data = http(f"{GITHUB}/repos/{owner}/{name}/releases?per_page=10", github_headers())
        elif host == "codeberg.org":
            data = http(f"https://codeberg.org/api/v1/repos/{owner}/{name}/releases?limit=10")
        else:
            data = None
        if isinstance(data, list):
            rel = next((r for r in data if not r.get("draft")), None)
            if rel is None:
                return None
            return {"version": rel["tag_name"], "date": (rel.get("published_at") or rel.get("created_at") or "")[:10],
                    "url": rel.get("html_url", ""), "prerelease": bool(rel.get("prerelease"))}
    except (urllib.error.URLError, OSError, ValueError, KeyError):
        pass
    tags = tags_of(url)
    if not tags:
        return None
    tag = max(tags, key=vt)
    return {"version": tag, "date": "", "url": f"{url.removesuffix('/')}/releases/tag/{tag}", "prerelease": False}


def check_releases(report, features, mods, infos):
    found = {}
    slugs = {n: m.get("slug") for n, m in features["mods"].items() if m.get("slug")}
    projects = {}
    try:
        for p in http(f"{MODRINTH}/projects?ids=" + urllib.parse.quote(json.dumps(list(slugs.values())))):
            projects[p["slug"]] = p
            projects[p["id"]] = p
    except (urllib.error.URLError, OSError, ValueError) as e:
        report.problems.append(f"Modrinth: {describe(e)}")

    def one(name):
        mod = features["mods"][name]
        spec = mods.get(name, {})
        out, errors = {}, []
        if mod.get("slug"):
            try:
                out["modrinth"] = modrinth_latest(mod["slug"])
            except (urllib.error.URLError, OSError, ValueError, KeyError) as e:
                errors.append(f"Modrinth: {describe(e)}")
        if mod.get("curseforge"):
            try:
                out["curseforge"] = curseforge_latest(curseforge_project(mod["curseforge"]))
            except (urllib.error.URLError, OSError, ValueError, KeyError) as e:
                errors.append(f"CurseForge: {describe(e)}")
        else:
            try:
                p = curseforge_lookup(name, mod, spec.get("repo"))
                if p:
                    out["curseforge_found"] = p
                    out["curseforge"] = curseforge_latest(p)
            except (urllib.error.URLError, OSError, ValueError, KeyError) as e:
                errors.append(f"CurseForge: {describe(e)}")
        if spec.get("repo"):
            try:
                out["github"] = forge_latest(spec["repo"])
            except (urllib.error.URLError, OSError, ValueError, KeyError, subprocess.SubprocessError) as e:
                errors.append(f"releases: {describe(e)}")
        return name, out, errors

    with concurrent.futures.ThreadPoolExecutor(8) as pool:
        for name, out, errors in pool.map(one, features["mods"]):
            found[name] = out
            report.problems += [f"{name} {e}" for e in errors]

    newest_app = max(features["game_versions"], key=vt)
    new_mc, moved, cf_found, behind = {}, [], [], []
    for name, out in found.items():
        mod = features["mods"][name]
        project = projects.get(mod.get("slug") or "")
        versions = set(project.get("game_versions", []) if project else [])
        for platform in ("modrinth", "curseforge"):
            if out.get(platform):
                versions.update(out[platform]["game_versions"])
        for v in versions:
            if is_release(v) and vt(v) > vt(newest_app):
                new_mc.setdefault(v, []).append(name)
        repo = mods.get(name, {}).get("repo")
        if project and project.get("source_url") and repo and norm_repo(project["source_url"]) != norm_repo(repo):
            moved.append(f"{name}: Modrinth links the source to {project['source_url']}, but tools/modcheck/mods.json has {repo}")
        if out.get("curseforge_found"):
            p = out["curseforge_found"]
            cf_found.append(f"{name}: {p['url']} (project {p['id']}). Add `\"curseforge\": {p['id']}` to its entry in features.jsonc so the app checks CurseForge for its updates too")
        info = infos.get(name)
        dates = [out[p]["date"] for p in ("modrinth", "curseforge", "github") if out.get(p) and out[p].get("date")]
        newest = [out[p]["version"] for p in ("modrinth", "curseforge", "github") if out.get(p)]
        if (info and dates and max(dates) > info["date"] and (dt.date.fromisoformat(max(dates)) - dt.date.fromisoformat(info["date"])).days > 3
                and not released_from(repo, newest, info["commit"])):
            behind.append(f"{name}: its newest release is from {max(dates)}, but the newest commit on {code(info['branch'])} is from {info['date']}. Its releases may come from another branch or repo. Set `branch` (a branch name, or `release` for its newest release) or `repo` in tools/modcheck/mods.json")
    lines = [f"Minecraft {v}: {', '.join(sorted(n))}" for v, n in sorted(new_mc.items(), key=lambda x: vt(x[0]))]
    report.act("minecraft", "Minecraft versions that mods support but the app doesn't list yet. Add them to `game_versions` in features.jsonc", lines)
    report.act("moved", "Source repos that moved", moved)
    report.act("curseforge", "Mods that are also on CurseForge", cf_found)
    report.act("behind", "Sources that look older than the newest release", behind)
    return found


def release_table(features, mods, found, infos, previous):
    rows = ["| Mod | Modrinth | CurseForge | GitHub | Source |", "|---|---|---|---|---|"]
    for name in features["mods"]:
        out = found.get(name, {})
        before = previous.get(name, {})
        cells = []
        for platform in ("modrinth", "curseforge", "github"):
            r = out.get(platform)
            if not r:
                cells.append("–")
                continue
            text = r["version"] + (f" ({r['date']})" if r.get("date") else "")
            if r.get("url"):
                text = f"[{text}]({r['url']})"
            if r.get("date") and before.get(platform) and before[platform] != r["version"]:
                text += " **new**"
            cells.append(text)
        info = infos.get(name)
        src = f"{code(info['branch'])} {info['commit'][:7]} ({info['date']})" if info else "–"
        repo = mods.get(name, {}).get("repo", "")
        cells.append(f"[{src}]({repo})" if repo and info else src)
        rows.append(f"| {name} | " + " | ".join(cells) + " |")
    return rows


def discover(report, features, mods, config):
    min_downloads = config.get("min_downloads", 100000)
    min_stars = config.get("min_stars", 50)
    since = today() - dt.timedelta(days=config.get("max_age_days", 180))
    versions = set(features["game_versions"])
    ignore = mods.get("ignore", {})
    known_slugs = {m.get("slug") for m in features["mods"].values()} | {m.get("id") for m in features["mods"].values()}
    known_cf = {m.get("curseforge") for m in features["mods"].values() if m.get("curseforge")}
    known_repos = {norm_repo(m.get("repo")) for m in mods.get("mods", {}).values() if m.get("repo")}
    known_names = {norm_name(n) for n in features["mods"]} | {norm_name(m.get("id")) for m in features["mods"].values()}
    cands = {}

    def text_ok(*parts):
        t = " ".join(p or "" for p in parts).lower()
        return "skyblock" in t or "sky block" in t

    try:
        hits = {}
        for query in ("skyblock", "hypixel skyblock"):
            for offset in (0, 100):
                q = urllib.parse.urlencode({"query": query, "facets": json.dumps([["project_type:mod"], ["categories:fabric"]]),
                                            "index": "downloads", "limit": 100, "offset": offset})
                for h in http(f"{MODRINTH}/search?{q}")["hits"]:
                    hits.setdefault(h["slug"], h)
        picks = [h for h in hits.values()
                 if h["slug"] not in known_slugs and f"modrinth:{h['slug']}" not in ignore
                 and h.get("server_side") == "unsupported" and text_ok(h["title"], h["description"])
                 and h["downloads"] >= min_downloads and h["date_modified"][:10] >= since.isoformat()
                 and versions & set(h.get("versions", []))]
        sources = {}
        if picks:
            for p in http(f"{MODRINTH}/projects?ids=" + urllib.parse.quote(json.dumps([h["project_id"] for h in picks]))):
                sources[p["id"]] = p.get("source_url") or ""
        for h in picks:
            src = sources.get(h["project_id"], "")
            if norm_repo(src) in known_repos:
                continue
            cands[h["slug"]] = {"name": h["title"], "modrinth": f"https://modrinth.com/mod/{h['slug']}", "downloads": h["downloads"],
                                "updated": h["date_modified"][:10], "github": src if repo_parts(src)[0] else "", "curseforge": "", "ignore": f"modrinth:{h['slug']}"}
    except (urllib.error.URLError, OSError, ValueError, KeyError) as e:
        report.problems.append(f"Modrinth search: {describe(e)}")

    key = curseforge_key()
    if key:
        try:
            for query in ("skyblock", "hypixel"):
                q = urllib.parse.urlencode({"gameId": 432, "classId": 6, "searchFilter": query, "modLoaderType": 4, "sortField": 6, "sortOrder": "desc", "pageSize": 50})
                for m in http(f"{CURSEFORGE}/mods/search?{q}", {"x-api-key": key})["data"]:
                    links = m.get("links") or {}
                    files = m.get("latestFilesIndexes") or []
                    if (m["id"] in known_cf or f"curseforge:{m['id']}" in ignore or norm_name(m["name"]) in known_names
                            or norm_repo(links.get("sourceUrl")) in known_repos or m.get("downloadCount", 0) < min_downloads
                            or (m.get("dateModified") or "")[:10] < since.isoformat() or not text_ok(m.get("name"), m.get("summary"))
                            or not any(f.get("modLoader") == 4 and f.get("gameVersion") in versions for f in files)):
                        continue
                    match = next((c for c in cands.values() if norm_name(c["name"]) == norm_name(m["name"])
                                  or (c["github"] and norm_repo(c["github"]) == norm_repo(links.get("sourceUrl")))), None)
                    if match:
                        match["curseforge"] = links.get("websiteUrl", "")
                        match["downloads"] += m.get("downloadCount", 0)
                    else:
                        cands[f"cf{m['id']}"] = {"name": m["name"], "modrinth": "", "curseforge": links.get("websiteUrl", ""), "downloads": m.get("downloadCount", 0),
                                                 "updated": (m.get("dateModified") or "")[:10], "github": links.get("sourceUrl") or "", "ignore": f"curseforge:{m['id']}"}
        except (urllib.error.URLError, OSError, ValueError, KeyError) as e:
            report.problems.append(f"CurseForge search: {describe(e)}")
    else:
        report.notes.append(("CurseForge", ["The CurseForge search was skipped because there's no `CURSEFORGE_API_KEY`. Add one as a repository secret to search CurseForge too. Mods with a `curseforge` id in features.jsonc are still checked for releases."]))

    try:
        repos = {}
        headers = github_headers()
        for query in (f"topic:hypixel-skyblock fork:false archived:false pushed:>{since}", f"hypixel skyblock in:name,description,topics fork:false archived:false pushed:>{since}"):
            q = urllib.parse.urlencode({"q": query, "sort": "stars", "order": "desc", "per_page": 50})
            for r in http(f"{GITHUB}/search/repositories?{q}", headers)["items"]:
                repos.setdefault(r["full_name"].lower(), r)
        linked = {norm_repo(c["github"]) for c in cands.values() if c["github"]}
        picks = [r for r in repos.values() if r["stargazers_count"] >= min_stars and r.get("language") in ("Java", "Kotlin")
                 and norm_repo(r["html_url"]) not in known_repos and f"github:{r['full_name'].lower()}" not in ignore]
        for r in sorted(picks, key=lambda r: -r["stargazers_count"])[:25]:
            if norm_repo(r["html_url"]) in linked:
                for c in cands.values():
                    if norm_repo(c["github"]) == norm_repo(r["html_url"]):
                        c["stars"] = r["stargazers_count"]
                continue
            if norm_name(r["name"]) in known_names:
                continue
            try:
                tree = http(f"{GITHUB}/repos/{r['full_name']}/git/trees/{urllib.parse.quote(r['default_branch'])}?recursive=1", headers)
            except (urllib.error.URLError, OSError, ValueError):
                continue
            if not any(t["path"].endswith("fabric.mod.json") for t in tree.get("tree", [])):
                continue
            cands[f"gh:{r['full_name'].lower()}"] = {"name": r["name"], "modrinth": "", "curseforge": "", "github": r["html_url"], "stars": r["stargazers_count"],
                                                     "downloads": 0, "updated": r["pushed_at"][:10], "ignore": f"github:{r['full_name'].lower()}"}
    except (urllib.error.URLError, OSError, ValueError, KeyError) as e:
        report.problems.append(f"GitHub search: {describe(e)}")

    rows = []
    for c in sorted(cands.values(), key=lambda c: (-c["downloads"], -c.get("stars", 0))):
        links = " · ".join(f"[{label}]({url})" for label, url in (("Modrinth", c["modrinth"]), ("CurseForge", c["curseforge"]), ("GitHub", c["github"])) if url)
        size = f"{c['downloads']:,} downloads" if c["downloads"] else f"{c.get('stars', 0)} stars"
        rows.append(f"{c['name']}: {links}, {size}, updated {c['updated']}. To skip it, add `\"{c['ignore']}\": \"<reason>\"` to `ignore` in tools/modcheck/mods.json")
    report.act("new_mods", "Hypixel SkyBlock mods the app doesn't support yet. Add the ones that share on/off features with supported mods", rows)


def build(report, features, checked, settings_total, release_rows):
    lines = [f"Checked on {today()} against mod list revision {features['revision']}: {checked} mod{'s' * (checked != 1)}, {settings_total:,} settings read from their source."]
    if report.actions:
        lines.append("\n## Needs attention")
        for title, items in report.actions:
            lines.append(f"\n### {title}\n")
            lines += [i if i.startswith("<details>") else f"- {i}" for i in items]
    else:
        lines.append("\nNothing needs attention.")
    if report.problems:
        lines.append("\n## Couldn't check\n")
        lines += [f"- {p}" for p in report.problems]
    for title, items in report.notes:
        lines.append(f"\n## {title}\n")
        lines += [f"- {i}" for i in items]
    if release_rows:
        lines.append("\n## Latest releases\n")
        lines += release_rows
    lines.append("\n<sub>Made by tools/modcheck/check.py. Run it locally with `python tools/modcheck/check.py`.</sub>")
    body = "\n".join(lines) + "\n"
    if len(body) > BODY_LIMIT:
        body = body[:BODY_LIMIT].rsplit("\n", 1)[0] + "\n\n…cut short. Run the check locally for the full report.\n"
    return body


def title_of(report):
    parts = []
    c = report.counts
    if c.get("missing"):
        parts.append(f"{c['missing']} mapped setting{'s' * (c['missing'] != 1)} gone")
    if c.get("not_switch"):
        parts.append(f"{c['not_switch']} no longer on/off")
    if c.get("changed"):
        parts.append(f"{c['changed']} mod{'s' * (c['changed'] != 1)} changed settings")
    if c.get("minecraft"):
        parts.append("new Minecraft version")
    if c.get("new_mods"):
        parts.append(f"{c['new_mods']} unsupported mod{'s' * (c['new_mods'] != 1)}")
    if c.get("broken"):
        parts.append(f"{c['broken']} mod{'s' * (c['broken'] != 1)} couldn't be read")
    rest = sum(v for k, v in c.items() if k in ("moved", "curseforge", "behind"))
    if rest:
        parts.append(f"{rest} other")
    return "Mod check: " + (", ".join(parts) if parts else "nothing to do")


def search(pattern):
    rx = re.compile(pattern, re.I)
    catalog = WORK / "catalog"
    if not catalog.is_dir():
        sys.exit("Run the check first so .modcheck/catalog exists.")
    for path in sorted(catalog.glob("*.tsv")):
        for line in path.read_text(encoding="utf-8").splitlines():
            key, typ, name = (line.split("\t") + ["", ""])[:3]
            if rx.search(key) or rx.search(name):
                print(f"{path.stem}\t{key}\t{typ}\t{name}")


def main():
    ap = argparse.ArgumentParser(description="Check the mod list against every supported mod's source and releases.")
    ap.add_argument("--update", action="store_true", help="save the settings and releases seen now as the new snapshots")
    ap.add_argument("--no-fetch", action="store_true", help="reuse the sources in .modcheck/src and skip release and mod searches")
    ap.add_argument("--mod", action="append", default=[], help="only check these mods")
    ap.add_argument("--report", type=Path, help="write the Markdown report here instead of printing it")
    ap.add_argument("--json", type=Path, help="write a short JSON summary here")
    ap.add_argument("--search", metavar="REGEX", help="search the settings read by the last run")
    ap.add_argument("--strict", action="store_true", help="exit with an error when a mapped setting is missing or a mod couldn't be read")
    args = ap.parse_args()
    if args.search:
        search(args.search)
        return

    features = load_jsonc(ROOT / "features.jsonc")
    mods = load_json(HERE / "mods.json", {})
    old_names = load_json(HERE / "old_names.json", {})
    previous = load_json(SNAPSHOTS / "releases.json", {})
    names = [n for n in features["mods"] if not args.mod or n in args.mod]
    report = Report()
    newest = max(features["game_versions"], key=vt)

    unknown = [n for n in names if n not in mods.get("mods", {})]
    report.act("broken", "Mods in features.jsonc without a source in tools/modcheck/mods.json", [f"{n}: add its repo and settings reader" for n in unknown])

    settings, labels, infos, rows_by_mod, broken = {}, {}, {}, {}, []
    todo = [n for n in names if n in mods.get("mods", {})]
    with concurrent.futures.ThreadPoolExecutor(6) as pool:
        jobs = {pool.submit(read_settings, n, mods["mods"][n], not args.no_fetch, features["mods"][n].get("slug")): n for n in todo}
        for job in concurrent.futures.as_completed(jobs):
            n = jobs[job]
            try:
                info, rows = job.result()
            except LookupError as e:
                broken.append(f"{n}: {e}")
                continue
            except (RuntimeError, OSError, subprocess.SubprocessError) as e:
                report.problems.append(f"{n}: {describe(e)}")
                continue
            infos[n], rows_by_mod[n] = info, rows
            settings[n] = {k: t for k, t, _ in rows}
            labels[n] = {k: label for k, _, label in rows}
    report.act("broken", "Mods whose settings couldn't be read", sorted(broken))
    settings = {n: settings[n] for n in names if n in settings}

    check_mapped(report, features, settings, old_names, newest)
    check_changes(report, settings, labels, {n: read_snapshot(n) for n in settings})

    found, release_rows = {}, []
    if not args.no_fetch:
        found = check_releases(report, {**features, "mods": {n: features["mods"][n] for n in names}}, mods.get("mods", {}), infos)
        release_rows = release_table({**features, "mods": {n: features["mods"][n] for n in names}}, mods.get("mods", {}), found, infos, previous)
        if not args.mod:
            discover(report, features, mods, mods.get("discovery", {}))

    body = build(report, features, len(names), sum(len(v) for v in settings.values()), release_rows)
    title = title_of(report)
    if args.report:
        args.report.write_text(f"{body}", encoding="utf-8")
    else:
        print(f"# {title}\n\n{body}")
    if args.json:
        args.json.write_text(json.dumps({"actionable": bool(report.actions), "title": title, "counts": report.counts}, indent=2) + "\n", encoding="utf-8")

    if args.update:
        for n, rows in rows_by_mod.items():
            write_snapshot(n, rows)
        if found:
            saved = dict(previous)
            for n, out in found.items():
                kept = previous.get(n, {})
                versions = {p: out[p]["version"] if out[p].get("date") else kept.get(p) for p in ("modrinth", "curseforge", "github") if out.get(p)}
                saved[n] = {p: v for p, v in versions.items() if v}
            (SNAPSHOTS / "releases.json").write_text(json.dumps(dict(sorted(saved.items())), indent=2) + "\n", encoding="utf-8")
        print(f"Saved snapshots for {len(rows_by_mod)} mods.", file=sys.stderr)
    if args.strict and any(report.counts.get(k) for k in ("missing", "not_switch", "broken")):
        sys.exit("The mod list doesn't match the mods' sources. See the report.")


if __name__ == "__main__":
    main()
