import json
import re
import sys
from pathlib import Path

BOOL = "bool"
JUMP = re.compile(r"[\"'/]")
CHAR = re.compile(r"'(?:\\u[0-9a-fA-F]{4}|\\.|[^'\\\n])'")
LITERAL = re.compile(r'"((?:[^"\\$]|\\.)*)"')
DECLARES = re.compile(r"\s*(?:@|(?:(?:private|internal|public|protected|data|enum|sealed|abstract|open|inner|override|const|lateinit)\s+)*(?:class|object|fun|val|var|interface|init)\b)")
DSL_BUILDERS = {"boolean", "int", "long", "float", "double", "string", "strings", "enum", "enums", "color", "transparency",
                "select", "obj", "draggable", "keybind", "ints", "longs", "floats", "doubles", "range"}


def read(path):
    return Path(path).read_text(encoding="utf-8", errors="replace")


def mask(text):
    out = list(text)
    n = len(text)

    def blank(a, b, ch):
        for k in range(a, min(b, n)):
            if out[k] != "\n":
                out[k] = ch

    i = 0
    while True:
        m = JUMP.search(text, i)
        if not m:
            break
        i = m.start()
        c = text[i]
        if c == "/":
            if text.startswith("//", i):
                j = text.find("\n", i)
                j = n if j < 0 else j
                blank(i, j, " ")
                i = j
            elif text.startswith("/*", i):
                j = text.find("*/", i + 2)
                j = n if j < 0 else j + 2
                blank(i, j, " ")
                i = j
            else:
                i += 1
        elif c == '"':
            if text.startswith('"""', i):
                j = text.find('"""', i + 3)
                j = n if j < 0 else j + 3
                blank(i + 3, j - 3, "_")
                i = j
            else:
                j = i + 1
                while j < n and text[j] not in '"\n':
                    j += 2 if text[j] == "\\" else 1
                blank(i + 1, j, "_")
                i = j + 1
        else:
            m = CHAR.match(text, i)
            if m:
                blank(i + 1, m.end() - 1, "_")
                i = m.end()
            else:
                i += 1
    return "".join(out)


class Source:
    def __init__(self, path):
        self.path = Path(path)
        self.text = read(path)
        self.code = mask(self.text)
        self.clean = "".join(t if c != " " or t in " \t\r\n" else " " for t, c in zip(self.text, self.code))

    def close(self, i):
        depth = 0
        code = self.code
        for j in range(i, len(code)):
            c = code[j]
            if c in "({[":
                depth += 1
            elif c in ")}]":
                depth -= 1
                if depth == 0:
                    return j
        return len(code) - 1

    def args(self, i):
        j = self.close(i)
        out, depth, start = [], 0, i + 1
        for k in range(i + 1, j):
            c = self.code[k]
            if c in "({[":
                depth += 1
            elif c in ")}]":
                depth -= 1
            elif c == "," and depth == 0:
                out.append((start, k))
                start = k + 1
        if self.code[start:j].strip():
            out.append((start, j))
        return [self.clean[a:b].strip() for a, b in out], [(a, b) for a, b in out], j

    def body(self, i):
        j, n = i, len(self.code)
        while j < n:
            c = self.code[j]
            if c in "([":
                j = self.close(j) + 1
                continue
            if c == "{":
                return j, self.close(j)
            if c in "};=":
                return None
            if c == "\n" and DECLARES.match(self.code, j + 1):
                return None
            j += 1
        return None

    def after(self, j):
        k = j + 1
        while k < len(self.code) and self.code[k] in " \t\r\n":
            k += 1
        if k < len(self.code) and self.code[k] == "{":
            return k, self.close(k)
        return None

    def own(self, a, b):
        out, depth = [], 0
        for k in range(a + 1, b):
            c = self.code[k]
            if c == "{":
                depth += 1
                out.append(" ")
            elif c == "}":
                depth -= 1
                out.append(" ")
            elif depth == 0:
                out.append(self.clean[k])
            else:
                out.append("\n" if c == "\n" else " ")
        return "".join(out)

    def depths(self, a, b):
        out, depth = [], 0
        for k in range(a, b):
            c = self.code[k]
            if c == "{":
                depth += 1
            elif c == "}":
                depth -= 1
            out.append(depth)
        return out


def literal(arg):
    if arg is None:
        return None
    m = LITERAL.fullmatch(arg.strip())
    return unescape(m.group(1)) if m else None


def unescape(s):
    return re.sub(r"\\(.)", lambda m: {"n": " ", "t": " "}.get(m.group(1), m.group(1)), s)


def named(args, name):
    for a in args:
        m = re.match(rf"{name}\s*=\s*(.*)$", a, re.S)
        if m:
            return m.group(1).strip()
    return None


def positional(args):
    return [a for a in args if not re.match(r"\w+\s*=[^=]", a)]


def tidy(s):
    s = re.sub(r"§.", "", s or "")
    return re.sub(r"\s+", " ", s).strip()


def humanize(name):
    words = re.sub(r"(?<=[a-z0-9])(?=[A-Z])|[_-]+", " ", name).split()
    return " ".join(words).capitalize() if words else name


def sources(root, pattern="*.kt"):
    return [p for p in sorted(Path(root).rglob(pattern)) if not ({"test", "build", "vendor"} & set(p.relative_to(root).parts))]


def find(root, name):
    hits = sources(root, name)
    return min(hits, key=lambda p: len(p.parts)) if hits else None


def lang_file(root, *names):
    for name in names:
        hits = [p for p in sorted(Path(root).rglob(name)) if "build" not in p.parts]
        if hits:
            return min(hits, key=lambda p: len(p.parts))
    return None


def load_json(path):
    if path is None:
        return {}
    try:
        return json.loads(read(path))
    except ValueError:
        return {}


def flatten(d, prefix=""):
    out = {}
    if isinstance(d, dict):
        for k, v in d.items():
            out.update(flatten(v, f"{prefix}.{k}" if prefix else k))
    elif isinstance(d, str):
        out[prefix] = d
    return out


def skyhanni(root):
    main = find(root, "SkyHanniConfig.kt")
    if main is None:
        raise LookupError("SkyHanniConfig.kt not found")
    classes = {}
    for path in sources(main.parent):
        src = Source(path)
        pkg = re.search(r"^package\s+([\w.]+)", src.clean, re.M)
        imports = {i.split(".")[-1]: i.rsplit(".", 1)[0] for i in re.findall(r"^import\s+([\w.]+)", src.clean, re.M)}
        for m in re.finditer(r"\b(?:class|object)\s+(\w+)", src.code):
            span = src.body(m.end())
            if span:
                classes.setdefault(m.group(1), []).append({"pkg": pkg.group(1) if pkg else "", "imports": imports, "body": src.own(*span)})

    def resolve(name, ctx):
        found = classes.get(name, [])
        if not found:
            return None
        want = ctx["imports"].get(name) if ctx else None
        for c in found:
            if want and c["pkg"] == want:
                return c
        for c in found:
            if ctx and c["pkg"] == ctx["pkg"]:
                return c
        return found[0]

    decl = re.compile(r"((?:@[\w.]+(?:\((?:[^()\"]|\"(?:[^\"\\]|\\.)*\"|\([^()]*\))*\))?\s*)*)\b(?:override\s+)?(?:lateinit\s+)?va[rl]\s+(\w+)\s*(?::\s*([\w.<>?, ]+?)\s*)?(?:=\s*([^\n]*)|\n|$)", re.S)
    rows = []

    def walk(cls, prefix, depth):
        if cls is None or depth > 16:
            return
        for m in decl.finditer(cls["body"]):
            ann, field, typ, init = m.group(1), m.group(2), (m.group(3) or "").strip(), (m.group(4) or "").strip()
            if "@Expose" not in ann:
                continue
            opt = re.search(r"@ConfigOption\(\s*name\s*=\s*\"((?:[^\"\\]|\\.)*)\"", ann)
            base = re.sub(r"<.*", "", typ or init.split("(")[0]).rstrip("?").split(".")[-1]
            sub = None if base in ("Position", "Boolean", "Int", "String", "Float", "Double", "Long", "Property") else resolve(base, cls)
            if sub is not None:
                walk(sub, prefix + field + ".", depth + 1)
            elif opt:
                is_bool = re.fullmatch(r"(?:Property<)?Boolean>?\??", typ) or (not typ and re.match(r"(?:true|false)\b|Property\.of\((?:true|false)\)", init))
                rows.append((prefix + field, BOOL if is_bool else (typ or base or "other"), unescape(opt.group(1))))

    walk(resolve("SkyHanniConfig", None), "", 0)
    return rows


def yacl(root):
    cats = [p for p in sources(root, "*.java") if p.parent.name == "categories" and "config" in p.parts]
    if not cats:
        raise LookupError("config/categories/*.java not found")
    bools = set()
    for path in sources(root, "*.java"):
        if "config" in path.parts and path.parent.name != "categories":
            bools.update(re.findall(r"\bboolean\s+(\w+)\s*[=;]", Source(path).clean))
    lang = {}
    for p in sorted(Path(root).rglob("en_us.json")):
        if "lang" in p.parts and "build" not in p.parts:
            data = load_json(p)
            if isinstance(data, dict):
                lang.update({k: v for k, v in data.items() if isinstance(v, str)})
    rows = []
    for path in cats:
        src = Source(path)
        for m in re.finditer(r"\.binding\s*\(", src.code):
            args, _, _ = src.args(m.end() - 1)
            b = re.fullmatch(r"(?:defaults|config)\.([\w.]+)", args[0]) if args else None
            if not b:
                continue
            start = src.code.rfind("createBuilder", 0, m.start())
            head = src.clean[max(0, start - 80):m.start()] if start >= 0 else ""
            t = re.search(r"Option\s*\.\s*<\s*([\w.]+)\s*>\s*createBuilder", head) or re.search(r"createBuilder\(\s*([\w.]+)\.class", head)
            typ = t.group(1).split(".")[-1] if t else None
            leaf = b.group(1).split(".")[-1]
            is_bool = typ == "Boolean" or (typ is None and leaf in bools)
            n = re.search(r"\.name\(\s*Component\.(translatable|literal)\(\s*\"((?:[^\"\\]|\\.)*)\"", head)
            name = (lang.get(n.group(2), n.group(2)) if n.group(1) == "translatable" else unescape(n.group(2))) if n else humanize(leaf)
            rows.append((b.group(1), BOOL if is_bool else (typ or "other").lower(), name))
    return rows


def nofrills(root):
    files = sources(root, "*.java")
    feature_of, settings = {}, {}
    rows = []
    for path in files:
        m = re.search(r"\bFeature\s+instance\s*=\s*new\s+Feature\(\s*\"([^\"]+)\"", Source(path).clean)
        if m:
            feature_of[path.stem] = m.group(1)
    gui_modules, gui_settings = {}, {}
    for path in files:
        src = Source(path)
        for m in re.finditer(r"new\s+Module\s*\(", src.code):
            args, _, _ = src.args(m.end() - 1)
            if len(args) >= 2 and literal(args[0]):
                gui_modules[args[1].split(".")[0]] = literal(args[0])
        for m in re.finditer(r"new\s+Settings\s*\.\s*\w+\s*\(", src.code):
            args, _, _ = src.args(m.end() - 1)
            if len(args) >= 2 and literal(args[0]) and re.fullmatch(r"\w+\.\w+", args[1]):
                gui_settings[args[1]] = literal(args[0])
    for stem, key in feature_of.items():
        rows.append((f"{key}.enabled", BOOL, gui_modules.get(stem, humanize(key))))
    for path in files:
        src = Source(path)
        for m in re.finditer(r"(?:(\w+)\s*=\s*)?new\s+(Setting\w+)\s*(?:<[^>]*>)?\s*\(", src.code):
            args, _, _ = src.args(m.end() - 1)
            if len(args) < 2 or literal(args[-2]) is None:
                continue
            parent = args[-1].replace(" ", "")
            if re.fullmatch(r"(?:this\.)?instance(?:\.key\(\))?", parent):
                owner = feature_of.get(path.stem)
            elif re.fullmatch(r"\w+\.instance(?:\.key\(\))?", parent):
                owner = feature_of.get(parent.split(".")[0])
            else:
                owner = literal(args[-1])
            if not owner:
                continue
            typ = m.group(2)
            key = f"{owner}.{literal(args[-2])}"
            label = gui_settings.get(f"{path.stem}.{m.group(1)}") if m.group(1) else None
            settings.setdefault(key, (BOOL if typ == "SettingBool" else typ.removeprefix("Setting").lower(), label or humanize(literal(args[-2]))))
    rows.extend((k, t, n) for k, (t, n) in settings.items())
    return rows


def firmament(root):
    base = Path(root) / "src" / "main" / "kotlin"
    files = sources(base if base.is_dir() else root)
    lang = load_json(lang_file(root, "translations/en_us.json", "en_us.json"))
    rows = []
    for path in files:
        src = Source(path)
        for m in re.finditer(r"\bobject\s+(\w+)\s*:\s*ManagedConfig\s*\(", src.code):
            args, _, end = src.args(m.end() - 1)
            if not args:
                continue
            group = literal(args[0])
            if group is None:
                g = re.search(rf"\bval\s+{re.escape(args[0])}\s*(?::\s*String)?\s*(?:get\(\)\s*)?=\s*\"([^\"]+)\"", src.clean)
                group = g.group(1) if g else None
            span = src.body(end + 1)
            if not group or not span:
                continue
            a, b = span
            for d in re.finditer(r"\bva[rl]\s+(\w+)\s+by\s+(\w+)\s*\(", src.code[a:b]):
                dargs, _, _ = src.args(a + d.end() - 1)
                ident = literal(dargs[0]) if dargs else None
                if not ident:
                    continue
                key = f"{group}.{ident}"
                rows.append((key, BOOL if d.group(2) == "toggle" else d.group(2).lower(), lang.get(f"firmament.config.{key}", humanize(d.group(1)))))
    return rows


def odin(root):
    base = Path(root) / "src" / "main" / "kotlin"
    rows = []
    for path in sources(base if base.is_dir() else root):
        src = Source(path)
        for m in re.finditer(r"\bobject\s+(\w+)\s*:\s*Module\s*\(", src.code):
            args, _, end = src.args(m.end() - 1)
            mod = literal(named(args, "name") or (positional(args) or [None])[0])
            if not mod:
                continue
            rows.append((f"{mod}.enabled", BOOL, mod))
            span = src.body(end + 1)
            if not span:
                continue
            a, b = span
            for s in re.finditer(r"\bby\s+(\w+Setting|HUD)\s*\(", src.code[a:b]):
                if s.group(1) in ("ActionSetting", "DropdownSetting"):
                    continue
                sargs, _, _ = src.args(a + s.end() - 1)
                name = literal(named(sargs, "name") or (positional(sargs) or [None])[0])
                if name:
                    typ = BOOL if s.group(1) == "BooleanSetting" else s.group(1).removesuffix("Setting").lower()
                    rows.append((f"{mod}.settings.{name}", typ, f"{mod} > {name}"))
    return rows


def dsl(root):
    base = Path(root) / "src" / "main" / "kotlin"
    files = sources(base if base.is_dir() else root)
    lang = {}
    for p in sorted(Path(root).rglob("en_us.json")):
        if "lang" in p.parts and not {"build", "test"} & set(p.parts):
            flat = flatten(load_json(p))
            lang.update({k.removesuffix(".@value"): v for k, v in flat.items() if not k.endswith(".desc")})
    cats, edges = {}, []
    for path in files:
        src = Source(path)
        for m in re.finditer(r"\bobject\s+(\w+)\s*:\s*(CategoryKt|ConfigKt|Category|Config)\s*\(", src.code):
            args, _, end = src.args(m.end() - 1)
            ident = literal(args[0]) if args else None
            if ident is None and args and m.group(2) in ("ConfigKt", "Config"):
                ident = args[0]
            span = src.body(end + 1)
            if ident is None or not span:
                continue
            cats[m.group(1)] = {"id": ident, "root": m.group(2) in ("ConfigKt", "Config"), "fields": dsl_fields(src, span, lang)}
            dsl_children(src, span[0] + 1, span[1], m.group(1), edges)
    parent = {}
    for p, c in edges:
        if c in cats and c != p:
            parent.setdefault(c, p)

    def path_of(name, seen=()):
        cat = cats[name]
        if cat["root"]:
            return ""
        up = parent.get(name)
        if up is None or up not in cats or up in seen:
            return cat["id"]
        head = path_of(up, seen + (name,))
        return f"{head}.{cat['id']}" if head else cat["id"]

    rows = []
    for name, cat in cats.items():
        prefix = path_of(name)
        for key, typ, label in cat["fields"]:
            rows.append((f"{prefix}.{key}" if prefix else key, typ, label))
    return rows


def dsl_fields(src, span, lang):
    a, b = span
    depth = src.depths(a, b)
    out = []
    for d in re.finditer(r"\bva[rl]\s+(\w+)(?:\s*:\s*[\w.<>?, ]+?)?\s+by\s+(\w+)\s*\(", src.code[a:b]):
        if depth[d.start()] != 1:
            continue
        args, spans, end = src.args(a + d.end() - 1)
        builder, bargs, bend = d.group(2), args, end
        if builder not in DSL_BUILDERS:
            builder = None
            for inner in re.finditer(r"\b(\w+)\s*\(", src.code[a + d.end():end]):
                if inner.group(1) in DSL_BUILDERS:
                    builder = inner.group(1)
                    bargs, _, bend = src.args(a + d.end() + inner.end() - 1)
                    break
            if builder is None:
                continue
        pos = positional(bargs)
        ident = None
        if pos and literal(pos[0]) is not None and builder not in ("strings",):
            if builder not in ("string", "keybind") or (len(pos) >= 2 and literal(pos[1]) is not None):
                ident = literal(pos[0])
        tail = src.after(bend)
        text = src.clean[tail[0]:tail[1]] if tail else ""
        label = None
        t = re.search(r"translation\s*=\s*\"([^\"]+)\"", text)
        n = re.search(r"\bname\s*=\s*(?:Literal|Translated)\(\s*\"((?:[^\"\\]|\\.)*)\"", text)
        if n:
            label = lang.get(n.group(1), unescape(n.group(1)))
        elif t:
            label = lang.get(t.group(1))
        out.append((ident or d.group(1), BOOL if builder == "boolean" else builder, tidy(label) if label else humanize(d.group(1))))
    return out


def dsl_children(src, a, b, owner, edges):
    i = a
    pattern = re.compile(r"\bcategor(y|ies)\s*\(")
    while True:
        m = pattern.search(src.code, i, b)
        if not m:
            return
        args, _, end = src.args(m.end() - 1)
        names = [re.sub(r"\(\)$", "", x.strip()) for x in args]
        names = [x for x in names if re.fullmatch(r"\w+", x)]
        if m.group(1) == "y" and names:
            edges.append((owner, names[0]))
            block = src.after(end)
            if block:
                dsl_children(src, block[0] + 1, block[1], names[0], edges)
                i = block[1] + 1
                continue
        else:
            edges.extend((owner, x) for x in names)
        i = end + 1


def noba(root):
    main = find(root, "NobaConfig.kt")
    if main is None:
        raise LookupError("NobaConfig.kt not found")
    classes = {}
    for path in sources(main.parent):
        src = Source(path)
        for m in re.finditer(r"\b(?:class|object)\s+(\w+)", src.code):
            span = src.body(m.end())
            if span:
                classes.setdefault(m.group(1), []).append((path, src.own(*span)))
    rows = []

    def walk(name, prefix, near, depth):
        found = classes.get(name, [])
        if not found or depth > 12:
            return
        path, body = next((c for c in found if c[0] == near), found[0])
        for m in re.finditer(r"\bva[rl]\s+(\w+)\s+by\s+(option|category)\s*(?:<[^>]*>)?\s*\(([^\n]*)", body):
            field, kind, rest = m.groups()
            if kind == "category":
                c = re.match(r"\s*(\w+)\s*\(", rest)
                if c:
                    walk(c.group(1), prefix + [field], path, depth + 1)
            else:
                default = rest.split(")")[0].strip()
                is_bool = re.fullmatch(r"(?:defaultValue\s*=\s*)?(?:true|false)", default) or "<Boolean>" in m.group(0)
                label = humanize(prefix[-1] if field == "enabled" and prefix else field)
                rows.append((".".join(prefix + [field]), BOOL if is_bool else "other", label))

    walk("NobaConfig", [], main, 0)
    return rows


def sba(root):
    defaults = find(root, "defaults.json")
    feature_java = find(root, "Feature.java")
    if defaults is None:
        raise LookupError("defaults.json not found")
    lang = flatten(load_json(lang_file(root, "en_US.json", "en_us.json")))
    names = {}
    for java in (feature_java, find(root, "FeatureSetting.java")):
        if java:
            for m in re.finditer(r"^\s+([A-Z][A-Z0-9_]+)\(\s*(?:-?\d+\s*,\s*)?\"([^\"]+)\"", Source(java).clean, re.M):
                names.setdefault(m.group(1), lang.get(m.group(2), humanize(m.group(1).lower())))
    rows = []
    for feature, data in load_json(defaults).items():
        if not isinstance(data, dict):
            continue
        label = names.get(feature, humanize(feature.lower()))
        if "value" in data:
            rows.append((f"{feature}.value", BOOL if isinstance(data["value"], bool) else "other", label))
        for setting, value in (data.get("settings") or {}).items():
            rows.append((f"{feature}.settings.{setting}", BOOL if isinstance(value, bool) else "other", f"{label} > {names.get(setting, humanize(setting.lower()))}"))
    return rows


def midnight(root):
    rows, lang = [], {}
    for p in sorted(Path(root).rglob("en_us.json")):
        if "lang" in p.parts and not {"build", "test"} & set(p.parts):
            data = load_json(p)
            if isinstance(data, dict):
                lang.update({k: v for k, v in data.items() if isinstance(v, str)})
    files = sources(root, "*.java") + sources(root, "*.kt")
    for path in files:
        src = Source(path)
        if not re.search(r"\b(?:extends|:)\s*MidnightConfig\b", src.code):
            continue
        for m in re.finditer(r"@Entry\b(?:\s*\([^)]*\))?\s*(?:@\w+(?:\([^)]*\))?\s*)*(?:public\s+)?(?:static\s+)?(?:@JvmField\s+)?(?:var\s+(\w+)\s*(?::\s*([\w.<>?]+))?\s*=\s*([^\n;]+)|([\w.<>\[\], ]+?)\s+(\w+)\s*=\s*([^;]+);)", src.clean):
            name = m.group(1) or m.group(5)
            typ = (m.group(2) or m.group(4) or "").strip()
            init = (m.group(3) or m.group(6) or "").strip()
            is_bool = typ in ("boolean", "Boolean") or (not typ and init in ("true", "false"))
            label = next((v for k, v in lang.items() if k.endswith(f".midnightconfig.{name}")), None)
            rows.append((name, BOOL if is_bool else (typ.split("<")[0].lower() or "other"), label or humanize(name)))
    return rows


def serialentry(root):
    classes, start = {}, None
    for path in sources(root, "*.java") + sources(root, "*.kt"):
        src = Source(path)
        m = re.search(r"ConfigClassHandler\s*(?:\.\s*<[^>]*>\s*)?\.\s*createBuilder\(\s*(\w+)(?:\.class|::class\.java)", src.clean)
        if m and start is None:
            start = m.group(1)
        for c in re.finditer(r"\b(?:class|record|object)\s+(\w+)", src.code):
            span = src.body(c.end())
            if span:
                classes.setdefault(c.group(1), []).append(src.own(*span))
    if start is None:
        raise LookupError("ConfigClassHandler.createBuilder(...) not found")
    java = re.compile(r"((?:@[\w.]+(?:\([^)]*\))?\s*)*)((?:(?:public|private|protected|final|transient|static)\s+)*)([\w.]+(?:<[\w.<>, ?]*>)?(?:\[\])?)\s+(\w+)\s*(?:=\s*([^;]+))?;")
    kotlin = re.compile(r"((?:@[\w.:]+(?:\([^)]*\))?\s*)*)((?:(?:public|private|internal|lateinit|override|open)\s+)*)va[rl]\s+(\w+)\s*(?::\s*([\w.<>?, ]+?))?\s*=\s*([^\n]+)")
    rows = []

    def walk(name, prefix, depth, top):
        if depth > 12 or name not in classes:
            return
        body = classes[name][0]
        fields = [(m.group(1), m.group(2), m.group(3), m.group(4), m.group(5)) for m in java.finditer(body)]
        fields += [(m.group(1), m.group(2), m.group(4) or "", m.group(3), m.group(5)) for m in kotlin.finditer(body)]
        for ann, mods, typ, field, init in fields:
            if top and "@SerialEntry" not in ann:
                continue
            if not top and ("static" in mods or "transient" in mods or "@Transient" in ann or "@JvmStatic" in ann):
                continue
            renamed = re.search(r"@SerialEntry\(\s*(?:value\s*=\s*)?\"([^\"]+)\"", ann)
            key = prefix + (renamed.group(1) if renamed else field)
            typ = (typ or "").strip()
            base = re.sub(r"<.*", "", typ or (init or "").replace("new ", "").split("(")[0]).strip().split(".")[-1].rstrip("?")
            if typ in ("boolean", "Boolean") or (not typ and (init or "").strip() in ("true", "false")):
                rows.append((key, BOOL, humanize(field)))
            elif base in classes and base != name:
                walk(base, key + ".", depth + 1, False)
            else:
                rows.append((key, (base or "other").lower(), humanize(field)))

    walk(start, "", 0, True)
    return rows


def stella(root):
    rows = []
    elements = re.compile(r"\b(toggle|slider|stepslider|dropdown|colorpicker|keybind|textinput|multicheckbox|switch)\s*\{")
    for path in sources(root):
        src = Source(path)
        for c in re.finditer(r"\bcategory\s*\(", src.code):
            cargs, _, cend = src.args(c.end() - 1)
            cat = literal(cargs[0]) if cargs else None
            block = src.after(cend)
            if not cat or not block:
                continue
            for sc in re.finditer(r"\bsubcategory\s*\(", src.code[block[0]:block[1]]):
                sargs, _, send = src.args(block[0] + sc.end() - 1)
                sub = literal(sargs[0]) if sargs else None
                sblock = src.after(send)
                if not sub or not sblock:
                    continue
                pos = positional(sargs)
                own = literal(named(sargs, "configName") or (pos[1] if len(pos) > 1 else None))
                if own:
                    rows.append((f"{cat}.{sub}.{own}", BOOL, sub))
                for e in elements.finditer(src.code, sblock[0], sblock[1]):
                    eblock = (e.end() - 1, src.close(e.end() - 1))
                    text = src.clean[eblock[0]:eblock[1]]
                    ident = re.search(r"\bconfigName\s*=\s*\"([^\"]+)\"", text)
                    label = re.search(r"\bname\s*=\s*\"((?:[^\"\\]|\\.)*)\"", text)
                    if ident:
                        rows.append((f"{cat}.{sub}.{ident.group(1)}", BOOL if e.group(1) in ("toggle", "switch") else e.group(1),
                                     unescape(label.group(1)) if label else humanize(ident.group(1))))
    return rows


def devonian(root):
    rows = []
    adders = {"addSwitch": BOOL, "addSlider": "slider", "addDecimalSlider": "slider", "addColorPicker": "color", "addSelection": "selection",
              "addKeybind": "keybind", "addTextInput": "text"}
    for path in sources(root):
        src = Source(path)
        for m in re.finditer(r"\b(?:object|class)\s+\w+\s*:\s*(\w*Feature)\s*\(", src.code):
            args, _, end = src.args(m.end() - 1)
            pos = positional(args)
            feature = literal(named(args, "configName") or (pos[0] if pos else None))
            if not feature:
                continue
            label = literal(named(args, "displayName"))
            rows.append((f"config.{feature}", BOOL, label or humanize(feature)))
            span = src.body(end + 1)
            if not span:
                continue
            for a in re.finditer(r"\b(addSwitch|addSlider|addDecimalSlider|addColorPicker|addSelection|addKeybind|addTextInput)\s*\(", src.code[span[0]:span[1]]):
                aargs, _, _ = src.args(span[0] + a.end() - 1)
                apos = positional(aargs)
                sub = literal(named(aargs, "configName") or (apos[0] if apos else None))
                if not sub:
                    continue
                name = literal(named(aargs, "displayName") or (apos[3] if len(apos) > 3 else None))
                rows.append((f"config.{feature}${sub}", adders[a.group(1)], name or humanize(sub)))
    return rows


def scathapro(root):
    classes, start = {}, None
    for path in sources(root, "*.java"):
        src = Source(path)
        if re.search(r"\bclass\s+Config\s+extends\s+ObjectRootJsonFile", src.code):
            start = "Config"
        for c in re.finditer(r"\bclass\s+(\w+)", src.code):
            span = src.body(c.end())
            if span:
                classes.setdefault(c.group(1), src.own(*span))
    if start is None:
        raise LookupError("class Config extends ObjectRootJsonFile not found")
    rows = []

    def walk(name, prefix, depth):
        if depth > 12 or name not in classes:
            return
        for m in re.finditer(r"\b(?:root\s*\.\s*)?(addValue|addBoolean|addPrimitive\w*|addList\w*|addMap\w*)\s*\(\s*\"([^\"]+)\"\s*,\s*(?:new\s+(\w+))?", classes[name]):
            kind, key, cls = m.groups()
            if kind == "addValue" and cls:
                walk(cls, f"{prefix}{key}.", depth + 1)
            else:
                rows.append((prefix + key, BOOL if kind == "addBoolean" else "other", humanize(key.split(".")[-1])))

    walk(start, "", 0)
    return rows


KINDS = {"skyhanni": skyhanni, "midnight": midnight, "serialentry": serialentry, "stella": stella, "devonian": devonian, "scathapro": scathapro, "yacl": yacl, "nofrills": nofrills, "firmament": firmament, "odin": odin,
         "dsl": dsl, "noba": noba, "sba": sba, "none": lambda root: []}


def extract(kind, root):
    seen, rows = set(), []
    for key, typ, name in KINDS[kind](Path(root)):
        if key not in seen:
            seen.add(key)
            rows.append((key, typ, tidy(name)))
    return sorted(rows)


if __name__ == "__main__":
    if len(sys.argv) != 3 or sys.argv[1] not in KINDS:
        sys.exit(f"usage: extract.py {{{','.join(KINDS)}}} <source folder>")
    for row in extract(sys.argv[1], sys.argv[2]):
        print("\t".join(row))
