from rpython.rlib.rfile import create_stdio
from rpython.rlib.objectmodel import specialize, r_dict, compute_hash
from rpython.rlib.rarithmetic import intmask
from rpython.rlib.unroll import unrolling_iterable
from rpython.rlib.listsort import make_timsort_class
class ParseError(Exception):
    def __init__(self, reason): self.reason = reason
class NoResults(Exception):
    def __init__(self, message, handle): self.message = message; self.handle = handle
class ZaddyError(Exception):
    def __init__(self, message): self.message = message
class Result(object):
    pass
class Failed(Result):
    pass
failed = Failed()
def cached(f, cacheCount=[0]):
    attr = "t" + str(cacheCount[0]); cacheCount[0] += 1
    name = f.__name__
    uname = unicode(name)
    class CacheResult(Result):
        def __init__(self, i, rv): setattr(self, attr, (i, rv))
    cache = {}
    def deco(self, i):
        key = i
        if key in cache and cache[key] is failed: raise ParseError("no reason")
        elif key in cache: return getattr(cache[key], attr)
        cache[key] = failed
        i, rv = f(self, i)
        cache[key] = CacheResult(i, rv)
        self.lastMatch.append((uname, key, i))
        return i, rv
    deco.__name__ = name
    return deco
letNames = []
def let(f):
    letNames.append(f)
    return f
allRels = []
ruleList = []
FREE = 0
COMM = 1
ASSOC = 2
AC = 3
NSTR = 0
NLIST = 1
NSTRUCT = 2
PRODUCTCAP = 64
FLATCAP = 8
EXTCAP = 8
INF = 1 << 60
class ENode(object):
    def __init__(self, kind, ns, con, ckind, s, args):
        self.kind = kind; self.ns = ns; self.con = con; self.ckind = ckind
        self.s = s; self.args = args; self.cls = 0
        self.subsumed = False; self.dead = False
    def live(self): return not self.dead and not self.subsumed
    def tag(self): return self.ns + u"." + self.con
def nodeEq(l, r):
    if l.kind != r.kind or l.ns != r.ns or l.con != r.con or l.s != r.s: return False
    if len(l.args) != len(r.args): return False
    for i in range(len(l.args)):
        if l.args[i] != r.args[i]: return False
    return True
def nodeHash(n):
    h = n.kind * 1000003 + compute_hash(n.ns)
    h = intmask(h * 1000003 + compute_hash(n.con))
    h = intmask(h * 1000003 + compute_hash(n.s))
    for a in n.args: h = intmask(h * 1000003 + a)
    return h
def sortInts(xs):
    for i in range(1, len(xs)):
        x = xs[i]; j = i - 1
        while j >= 0 and xs[j] > x:
            xs[j + 1] = xs[j]; j -= 1
        xs[j + 1] = x
def shortlexLess(l, r):
    if len(l) != len(r): return len(l) < len(r)
    return l < r
def productInts(choices):
    rv = [[]]
    for cs in choices:
        nxt = []
        for prefix in rv:
            for c in cs:
                if len(nxt) < PRODUCTCAP: nxt.append(prefix + c)
        rv = nxt
    return rv
class UF(object):
    def __init__(self):
        self.parent = []
        self.members = []
        self.nodes = []
        self.memo = r_dict(nodeEq, nodeHash)
        self.strs = {}
        self.byTag = {}
        self.lists = []
        self.changes = 0
        self.unions = 0
    def make(self):
        rv = len(self.parent)
        self.parent.append(rv)
        self.members.append([])
        return rv
    def find(self, i):
        j = i
        while self.parent[j] != j: j = self.parent[j]
        while self.parent[i] != j:
            k = self.parent[i]; self.parent[i] = j; i = k
        return j
    def union(self, i, j):
        i = self.find(i); j = self.find(j)
        if i == j: return False
        if len(self.members[i]) < len(self.members[j]): i, j = j, i
        self.parent[j] = i
        self.members[i].extend(self.members[j])
        self.members[j] = []
        self.unions += 1
        return True
    def tagged(self, ns, con):
        k = ns + u"." + con
        if k in self.byTag: return self.byTag[k]
        rv = []
        self.byTag[k] = rv
        return rv
    def canonArgs(self, ckind, args):
        rv = [self.find(a) for a in args]
        if ckind == COMM or ckind == AC: sortInts(rv)
        return rv
    def intern(self, n):
        if n in self.memo: return self.find(self.memo[n].cls)
        c = self.make()
        n.cls = c
        self.memo[n] = n
        self.nodes.append(n)
        self.members[c].append(n)
        if n.kind == NLIST: self.lists.append(n)
        elif n.kind == NSTRUCT: self.tagged(n.ns, n.con).append(n)
        self.changes += 1
        return c
    def makeStr(self, s):
        if s in self.strs: return self.find(self.strs[s].cls)
        n = ENode(NSTR, u"", u"", FREE, s, [])
        self.strs[s] = n
        return self.intern(n)
    def makeList(self, l):
        return self.intern(ENode(NLIST, u"", u"", FREE, u"", [self.find(a) for a in l]))
    def flatArgs(self, ns, con, args, own):
        rv = []
        for a in args:
            sub = self.nodeIn(a, ns, con, own)
            if sub is None: rv.append(a)
            else: rv.extend(sub.args)
        return rv
    def nodeIn(self, c, ns, con, own):
        c = self.find(c)
        if c == own: return None
        best = None
        for n in self.members[c]:
            if n.kind != NSTRUCT or not n.live() or n.ns != ns or n.con != con: continue
            if best is not None and len(n.args) >= len(best.args): continue
            cyclic = False
            for a in n.args:
                a = self.find(a)
                if a == c or a == own: cyclic = True
            if not cyclic: best = n
        return best
    def struct(self, ns, con, ckind, args):
        if ckind == ASSOC or ckind == AC:
            flat = self.flatArgs(ns, con, args, -1)
            if len(flat) <= FLATCAP: args = flat
        return self.intern(ENode(NSTRUCT, ns, con, ckind, u"", self.canonArgs(ckind, args)))
    def subsume(self, n):
        if not n.subsumed:
            n.subsumed = True
            self.changes += 1
    def listsOf(self, c):
        rv = []
        for n in self.members[self.find(c)]:
            if n.kind == NLIST and n.live(): rv.append(n.args)
        return rv
    def strsOf(self, c):
        rv = []
        for n in self.members[self.find(c)]:
            if n.kind == NSTR and n.live(): rv.append(n.s)
        return rv
    def findList(self, c):
        for n in self.members[self.find(c)]:
            if n.kind == NLIST and n.live(): return n.args
        raise NoResults("findList", c)
    def minStr(self, c):
        rv = None
        for n in self.members[self.find(c)]:
            if n.kind == NSTR and n.live():
                if rv is None or shortlexLess(n.s, rv): rv = n.s
        return rv
    def findStr(self, c):
        for n in self.members[self.find(c)]:
            if n.kind == NSTR and n.live(): return n.s
        raise NoResults("findStr", c)
    def rebuild(self):
        while True:
            changed = False
            memo = r_dict(nodeEq, nodeHash)
            for n in self.nodes:
                if n.dead: continue
                n.args = self.canonArgs(n.ckind, n.args)
                if n in memo:
                    other = memo[n]
                    if self.union(other.cls, n.cls): changed = True
                    if n.subsumed: other.subsumed = True
                    n.dead = True
                else: memo[n] = n
            self.memo = memo
            for n in self.nodes[:]:
                if n.dead or n.subsumed or n.kind != NSTRUCT: continue
                if n.ckind != ASSOC and n.ckind != AC: continue
                own = self.find(n.cls)
                flat = self.flatArgs(n.ns, n.con, n.args, own)
                if len(flat) == len(n.args): continue
                probe = ENode(NSTRUCT, n.ns, n.con, n.ckind, u"", self.canonArgs(n.ckind, flat))
                if probe in self.memo and self.union(own, self.memo[probe].cls): changed = True
            if not changed: break
        for c in range(len(self.members)): self.members[c] = []
        alive = []
        for n in self.nodes:
            if n.dead: continue
            alive.append(n)
            self.members[self.find(n.cls)].append(n)
        self.nodes = alive
        self.lists = [n for n in self.lists if not n.dead]
        for k in self.byTag.keys():
            self.byTag[k] = [n for n in self.byTag[k] if not n.dead]
class Env(object):
    def __init__(self, name, cls, elems, next):
        self.name = name; self.cls = cls; self.elems = elems; self.next = next
def envLookup(env, name):
    while env is not None:
        if env.name == name: return env
        env = env.next
    return None
def sameSeq(uf, l, r):
    if len(l) != len(r): return False
    for i in range(len(l)):
        if uf.find(l[i]) != uf.find(r[i]): return False
    return True
class Bindings(object):
    def __init__(self, uf, env, rule):
        self.uf = uf; self.env = env; self.rule = rule
    def lookup(self, name):
        b = envLookup(self.env, name)
        if b is None: raise ZaddyError("rule " + self.rule + ": unbound variable " + name.encode("utf-8"))
        return b
    def get(self, name):
        b = self.lookup(name)
        if b.cls >= 0: return b.cls
        return self.uf.makeList(b.elems)
    def elems(self, name):
        b = self.lookup(name)
        if b.cls >= 0: return self.uf.findList(b.cls)
        return b.elems
class CapEnv(object):
    def __init__(self, uf):
        self.uf = uf; self.d = {}
    def set(self, name, rv): self.d[name] = rv
    def get(self, name): return self.d[name]
    def elems(self, name): return self.uf.findList(self.d[name])
class Patt(object):
    def match(self, uf, c, env, out): raise NotImplementedError
    def matchNode(self, uf, n, env, out): pass
    def groupable(self, ns, con): return False
    def isSplice(self): return False
    def candidates(self, uf): raise ZaddyError("rule patterns must be constructors, lists, or strings")
    def extend(self, uf, rule, n, matches): pass
class PIgnore(Patt):
    def match(self, uf, c, env, out): out.append(env)
    def groupable(self, ns, con): return True
class PVar(Patt):
    def __init__(self, name): self.name = name
    def match(self, uf, c, env, out):
        b = envLookup(env, self.name)
        if b is None: out.append(Env(self.name, uf.find(c), None, env))
        elif b.cls >= 0:
            if uf.find(b.cls) == uf.find(c): out.append(env)
        else:
            for l in uf.listsOf(c):
                if sameSeq(uf, l, b.elems):
                    out.append(env)
                    return
    def groupable(self, ns, con): return True
class PSplice(Patt):
    def __init__(self, name): self.name = name
    def isSplice(self): return True
    def match(self, uf, c, env, out): raise ZaddyError("splice pattern outside of a list")
class PStr(Patt):
    def __init__(self, s): self.s = s
    def match(self, uf, c, env, out):
        if self.s in uf.strs:
            n = uf.strs[self.s]
            if not n.subsumed and uf.find(n.cls) == uf.find(c): out.append(env)
    def candidates(self, uf):
        if self.s in uf.strs: return [uf.strs[self.s]]
        return []
    def matchNode(self, uf, n, env, out):
        if n.kind == NSTR and n.s == self.s: out.append(env)
class PStruct(Patt):
    def __init__(self, ns, con, args):
        self.ns = ns; self.con = con; self.args = args
    def groupable(self, ns, con): return self.ns == ns and self.con == con
    def candidates(self, uf): return uf.tagged(self.ns, self.con)[:]
    def match(self, uf, c, env, out):
        for n in uf.members[uf.find(c)][:]:
            if n.kind == NSTRUCT and n.live(): self.matchNode(uf, n, env, out)
    def matchNode(self, uf, n, env, out):
        if n.ns != self.ns or n.con != self.con: return
        k = len(self.args)
        if n.ckind == FREE:
            if len(n.args) != k: return
            envs = [env]
            for i in range(k):
                nxt = []
                for e in envs: self.args[i].match(uf, n.args[i], e, nxt)
                envs = nxt
            out.extend(envs)
        elif n.ckind == COMM:
            if len(n.args) != k: return
            self.matchBag(uf, n, 0, n.args[:], env, out)
        elif n.ckind == ASSOC:
            if len(n.args) < k: return
            self.matchSeq(uf, n, 0, 0, env, out)
        else:
            if len(n.args) < k: return
            self.matchBag(uf, n, 0, n.args[:], env, out)
    def extend(self, uf, rule, n, matches):
        if n.kind != NSTRUCT or n.ns != self.ns or n.con != self.con: return
        if n.ckind != ASSOC and n.ckind != AC: return
        k = len(self.args); size = len(n.args)
        if size <= k or size > EXTCAP: return
        seen = {}
        if n.ckind == AC:
            for mask in range(1, 1 << size):
                sub = []; rest = []
                for b in range(size):
                    if mask & (1 << b): sub.append(n.args[b])
                    else: rest.append(n.args[b])
                if len(sub) < k or len(rest) == 0: continue
                self.extendWith(uf, rule, n, sub, rest, [], seen, matches)
        else:
            for start in range(size):
                for stop in range(start + k, size + 1):
                    if stop - start == size: continue
                    self.extendWith(uf, rule, n, n.args[start:stop], n.args[:start], n.args[stop:], seen, matches)
    def extendWith(self, uf, rule, n, sub, before, after, seen, matches):
        key = u",".join([str(uf.find(a)).decode("utf-8") for a in before]) + u"|" + u",".join([str(uf.find(a)).decode("utf-8") for a in sub])
        if key in seen: return
        seen[key] = None
        tmp = ENode(NSTRUCT, n.ns, n.con, n.ckind, u"", uf.canonArgs(n.ckind, sub))
        envs = []
        self.matchNode(uf, tmp, None, envs)
        if not envs: return
        c = uf.struct(n.ns, n.con, n.ckind, sub)
        node = uf.memo[tmp] if tmp in uf.memo else None
        if node is None: return
        link = ENode(NSTRUCT, n.ns, n.con, n.ckind, u"", uf.canonArgs(n.ckind, before + [c] + after))
        uf.union(n.cls, uf.intern(link))
        for env in envs: matches.append(Match(rule, node, env))
    def matchGroup(self, uf, n, p, group, env, out):
        if len(group) == 1: p.match(uf, group[0], env, out)
        elif p.groupable(self.ns, self.con):
            c = uf.struct(self.ns, self.con, n.ckind, group)
            p.match(uf, c, env, out)
    def matchSeq(self, uf, n, pi, j, env, out):
        k = len(self.args)
        p = self.args[pi]
        if pi == k - 1:
            self.matchGroup(uf, n, p, n.args[j:], env, out)
            return
        last = len(n.args) - (k - 1 - pi)
        stop = j + 1
        if p.groupable(self.ns, self.con): stop = last
        for e in range(j + 1, stop + 1):
            envs = []
            self.matchGroup(uf, n, p, n.args[j:e], env, envs)
            for env2 in envs: self.matchSeq(uf, n, pi + 1, e, env2, out)
    def matchBag(self, uf, n, pi, rest, env, out):
        k = len(self.args)
        p = self.args[pi]
        if pi == k - 1:
            if len(rest) == 0: return
            if len(rest) > 1 and n.ckind == COMM: return
            self.matchGroup(uf, n, p, rest, env, out)
            return
        spare = len(rest) - (k - 1 - pi)
        if spare < 1: return
        if p.groupable(self.ns, self.con) and n.ckind == AC and len(rest) <= 12:
            total = 1 << len(rest)
            for mask in range(1, total):
                group = []; remain = []
                for b in range(len(rest)):
                    if mask & (1 << b): group.append(rest[b])
                    else: remain.append(rest[b])
                if len(group) > spare: continue
                envs = []
                self.matchGroup(uf, n, p, group, env, envs)
                for env2 in envs: self.matchBag(uf, n, pi + 1, remain, env2, out)
        else:
            for b in range(len(rest)):
                if b > 0 and uf.find(rest[b]) == uf.find(rest[b - 1]): continue
                remain = rest[:b] + rest[b + 1:]
                envs = []
                p.match(uf, rest[b], env, envs)
                for env2 in envs: self.matchBag(uf, n, pi + 1, remain, env2, out)
class PList(Patt):
    def __init__(self, items):
        self.items = items
        self.need = [0] * (len(items) + 1)
        for i in range(len(items) - 1, -1, -1):
            self.need[i] = self.need[i + 1]
            if not items[i].isSplice(): self.need[i] += 1
    def candidates(self, uf): return uf.lists[:]
    def match(self, uf, c, env, out):
        for n in uf.members[uf.find(c)][:]:
            if n.kind == NLIST and n.live(): self.matchNode(uf, n, env, out)
    def matchNode(self, uf, n, env, out):
        if n.kind != NLIST: return
        if len(n.args) < self.need[0]: return
        self.seq(uf, n.args, 0, 0, env, out)
    def seq(self, uf, args, i, j, env, out):
        if i == len(self.items):
            if j == len(args): out.append(env)
            return
        it = self.items[i]
        if isinstance(it, PSplice):
            b = envLookup(env, it.name)
            if b is None:
                for e in range(j, len(args) - self.need[i + 1] + 1):
                    self.seq(uf, args, i + 1, e, Env(it.name, -1, args[j:e], env), out)
            else:
                cands = [b.elems]
                if b.cls >= 0: cands = uf.listsOf(b.cls)
                for l in cands:
                    e = j + len(l)
                    if e <= len(args) and sameSeq(uf, args[j:e], l): self.seq(uf, args, i + 1, e, env, out)
        elif j < len(args):
            envs = []
            it.match(uf, args[j], env, envs)
            for env2 in envs: self.seq(uf, args, i + 1, j + 1, env2, out)
class Rule(object):
    def __init__(self, name, patt, rhs, simplify):
        self.name = name; self.patt = patt; self.rhs = rhs; self.simplify = simplify
        self.fired = 0
def rewrite(name, patt, simplify):
    def deco(f):
        ruleList.append(Rule(name, patt, f, simplify))
        return f
    return deco
class Match(object):
    def __init__(self, rule, node, env):
        self.rule = rule; self.node = node; self.env = env
class builtinRels(object):
    ns = u"builtin"
    def __init__(self, uf): self.uf = uf
    def makeLine(self, s): return self.uf.struct(self.ns, u"Line", FREE, [s])
    def makeBlock(self, ls): return self.uf.struct(self.ns, u"Block", FREE, [ls])
    def makeFlatten(self, ls): return self.uf.struct(self.ns, u"Flatten", FREE, [ls])
    def makeLength(self, s): return self.uf.struct(self.ns, u"Length", FREE, [s])
    def makeJoin(self, s, ps): return self.uf.struct(self.ns, u"Join", FREE, [s, ps])
    def makeConcat(self, p, ps): return self.uf.struct(self.ns, u"Concat", FREE, [p, ps])
allRels.append(("builtin", builtinRels))
def evalBuiltins(uf, out):
    for n in uf.tagged(u"builtin", u"Flatten")[:]:
        if not n.live(): continue
        for ls in uf.listsOf(n.args[0]):
            if len(ls) == 1:
                out.append((n.cls, ls[0]))
                continue
            choices = [uf.listsOf(x) for x in ls]
            for l in productInts(choices): out.append((n.cls, uf.makeList(l)))
    for n in uf.tagged(u"builtin", u"Length")[:]:
        if not n.live(): continue
        s = uf.minStr(n.args[0])
        if s is not None: out.append((n.cls, uf.makeStr(str(len(s)).decode("utf-8"))))
    for n in uf.tagged(u"builtin", u"Join")[:]:
        if not n.live(): continue
        sep = uf.minStr(n.args[0])
        if sep is None: continue
        for ls in uf.listsOf(n.args[1]):
            parts = minStrs(uf, ls)
            if parts is not None: out.append((n.cls, uf.makeStr(sep.join(parts))))
    for n in uf.tagged(u"builtin", u"Concat")[:]:
        if not n.live(): continue
        for ls in uf.listsOf(n.args[1]):
            if len(ls) == 0:
                out.append((n.cls, n.args[0]))
                continue
            parts = minStrs(uf, [n.args[0]] + ls)
            if parts is not None: out.append((n.cls, uf.makeStr(u"".join(parts))))
def minStrs(uf, cs):
    rv = []
    for c in cs:
        s = uf.minStr(c)
        if s is None: return None
        rv.append(s)
    return rv
def saturate(uf, parser, limit, nodeLimit, verbose, stderr):
    iteration = 0
    while iteration < limit:
        uf.rebuild()
        before = uf.changes + uf.unions
        matches = []
        for rule in ruleList:
            for n in rule.patt.candidates(uf):
                if not n.live(): continue
                envs = []
                rule.patt.matchNode(uf, n, None, envs)
                for env in envs: matches.append(Match(rule, n, env))
                rule.patt.extend(uf, rule, n, matches)
        pending = []
        evalBuiltins(uf, pending)
        for m in matches:
            try: rv = m.rule.rhs(uf, parser, Bindings(uf, m.env, m.rule.name))
            except NoResults: continue
            for c in rv: uf.union(m.node.cls, c)
            if m.rule.simplify: uf.subsume(m.node)
            m.rule.fired += 1
        for (a, b) in pending: uf.union(a, b)
        iteration += 1
        if verbose: stderr.write("Iteration %d: %d matches, %d classes, %d nodes\n" % (iteration, len(matches), len(uf.parent), len(uf.nodes)))
        if uf.changes + uf.unions == before: break
        if len(uf.nodes) > nodeLimit:
            stderr.write("Warning: stopped after %d iterations at the limit of %d e-nodes\n" % (iteration, nodeLimit))
            break
    uf.rebuild()
    return iteration
TSTR = 0
TLINE = 1
TLIST = 2
class Extractor(object):
    def __init__(self, uf):
        self.uf = uf
        size = len(uf.parent)
        self.costS = [INF] * size
        self.bestS = [u""] * size
        self.costB = [INF] * size
        self.bestB = [None] * size
        self.costL = [INF] * size
        self.bestL = [None] * size
    def run(self):
        uf = self.uf
        changed = True
        while changed:
            changed = False
            for n in uf.nodes:
                if not n.live(): continue
                c = uf.find(n.cls)
                if n.kind == NSTR:
                    if self.costS[c] > 1 or (self.costS[c] == 1 and shortlexLess(n.s, self.bestS[c])):
                        self.costS[c] = 1; self.bestS[c] = n.s; changed = True
                elif n.kind == NLIST:
                    cost = 1
                    for a in n.args:
                        cost += self.costB[uf.find(a)]
                        if cost >= INF: break
                    if cost < self.costL[c]:
                        self.costL[c] = cost; self.bestL[c] = n; changed = True
                elif n.ns == u"builtin" and n.con == u"Line":
                    cost = 1 + self.costS[uf.find(n.args[0])]
                    if cost < self.costB[c]:
                        self.costB[c] = cost; self.bestB[c] = n; changed = True
                elif n.ns == u"builtin" and n.con == u"Block":
                    cost = 1 + self.costL[uf.find(n.args[0])]
                    if cost < self.costB[c]:
                        self.costB[c] = cost; self.bestB[c] = n; changed = True
    def emitList(self, c, depth, buf):
        n = self.bestL[self.uf.find(c)]
        assert n is not None
        for a in n.args: self.emitLine(a, depth, buf)
    def emitLine(self, c, depth, buf):
        n = self.bestB[self.uf.find(c)]
        assert n is not None
        if n.con == u"Line": buf.append(u" " * (depth * 4) + self.bestS[self.uf.find(n.args[0])])
        else: self.emitList(n.args[0], depth + 1, buf)
    def diagnose(self, root):
        uf = self.uf
        seen = {}
        stack = [uf.find(root)]
        stuck = []
        while stack:
            c = stack.pop()
            if c in seen: continue
            seen[c] = None
            ok = self.costS[c] < INF or self.costB[c] < INF or self.costL[c] < INF
            for n in uf.members[c]:
                if not n.live(): continue
                for a in n.args: stack.append(uf.find(a))
                if ok or n.kind != NSTRUCT or n.ns == u"builtin": continue
                stuck.append(n.tag())
        return stuck
def describeStuck(stuck):
    counts = {}
    order = []
    for t in stuck:
        if t not in counts:
            counts[t] = 0
            order.append(t)
        counts[t] += 1
    return [t + u" (" + str(counts[t]).decode("utf-8") + u")" for t in order]
def lineNumber(s, i):
    assert i >= 0
    return s.count(unichr(10), 0, i)
SortTraces = make_timsort_class(lt=lambda l, r: l[2] > r[2])
USAGE = "usage: %s [-v] [-i ITERATIONS] [-n NODES] < input > output\n"
def main(argv):
    stdin, stdout, stderr = create_stdio()
    verbose = False
    limit = 1000
    nodeLimit = 1000000
    i = 1
    while i < len(argv):
        if argv[i] == "-v": verbose = True
        elif argv[i] == "-i" and i + 1 < len(argv):
            i += 1
            limit = int(argv[i])
        elif argv[i] == "-n" and i + 1 < len(argv):
            i += 1
            nodeLimit = int(argv[i])
        else:
            stderr.write(USAGE % argv[0])
            return 2
        i += 1
    uf = UF()
    parser = MainParser(stdin.read().decode("utf-8"), uf)
    try:
        i, l = parser.parse()
        if i != len(parser.s): raise ParseError("Failed to consume all input")
    except ParseError as pe:
        stderr.write("Parse error: %s\n" % pe.reason)
        SortTraces(parser.lastMatch).sort()
        newlines = [0]
        for line in parser.s.split(u"\n"):
            newlines.append(newlines[-1] + len(line) + 1)
        for k, start, stop in parser.lastMatch[:10]:
            startLine = lineNumber(parser.s, start)
            startCol = start - newlines[startLine]
            stopLine = lineNumber(parser.s, stop)
            stopCol = stop - newlines[stopLine]
            t = k.encode("utf-8"), startLine + 1, startCol, stopLine + 1, stopCol
            stderr.write(("Trail: %s (%d:%d - %d:%d)" % t) + chr(10))
        return 1
    root = parser.builtin.makeFlatten(l)
    try: iterations = saturate(uf, parser, limit, nodeLimit, verbose, stderr)
    except ZaddyError as ze:
        stderr.write("Error: %s\n" % ze.message)
        return 3
    if verbose:
        for rule in ruleList:
            if rule.fired: stderr.write("Rule %s: %d applications\n" % (rule.name, rule.fired))
    ex = Extractor(uf)
    ex.run()
    root = uf.find(root)
    if ex.costL[root] >= INF:
        stderr.write("Error: no output could be extracted after %d iterations\n" % iterations)
        stderr.write("Constructors which were never rewritten:\n")
        for t in describeStuck(ex.diagnose(root))[:20]: stderr.write("  %s\n" % t.encode("utf-8"))
        return 3
    buf = []
    ex.emitList(root, 0, buf)
    stdout.write(u"\n".join(buf).encode("utf-8"))
    stdout.write("\n")
    if verbose: stderr.write("Wrote %d lines after %d iterations\n" % (len(buf), iterations))
    return 0
class pyRels(object):
    ns = u"py"
    def __init__(self, uf): self.uf = uf
    def makeStatement(self, line): return self.uf.struct(self.ns, u"Statement", 0, [line])
    def makeImp(self, module, names): return self.uf.struct(self.ns, u"Imp", 0, [module, names])
    def makeRet(self, expr): return self.uf.struct(self.ns, u"Ret", 0, [expr])
    def makeRaiseIf(self, test): return self.uf.struct(self.ns, u"RaiseIf", 0, [test])
    def makeContIf(self, test): return self.uf.struct(self.ns, u"ContIf", 0, [test])
    def makeCompound(self, head, block): return self.uf.struct(self.ns, u"Compound", 0, [head, block])
    def makeConditional(self, test, block): return self.uf.struct(self.ns, u"Conditional", 0, [test, block])
    def makeHandler(self, block, handler): return self.uf.struct(self.ns, u"Handler", 0, [block, handler])
allRels.append(("py", pyRels))
class pegRels(object):
    ns = u"peg"
    def __init__(self, uf): self.uf = uf
    def makeNamePatt(self, name): return self.uf.struct(self.ns, u"NamePatt", 0, [name])
    def makeNull(self, ): return self.uf.struct(self.ns, u"Null", 0, [])
    def makeToken(self, s): return self.uf.struct(self.ns, u"Token", 0, [s])
    def makeCall(self, s): return self.uf.struct(self.ns, u"Call", 0, [s])
    def makeSequence(self, exprs): return self.uf.struct(self.ns, u"Sequence", 0, [exprs])
    def makeChoice(self, this, that): return self.uf.struct(self.ns, u"Choice", 0, [this, that])
    def makeAny(self, expr): return self.uf.struct(self.ns, u"Any", 0, [expr])
    def makeSome(self, expr): return self.uf.struct(self.ns, u"Some", 0, [expr])
    def makeMaybe(self, expr): return self.uf.struct(self.ns, u"Maybe", 0, [expr])
    def makePositive(self, expr): return self.uf.struct(self.ns, u"Positive", 0, [expr])
    def makeNegative(self, expr): return self.uf.struct(self.ns, u"Negative", 0, [expr])
    def makeCapture(self, expr, patt): return self.uf.struct(self.ns, u"Capture", 0, [expr, patt])
    def makeProduction(self, expr, prod): return self.uf.struct(self.ns, u"Production", 0, [expr, prod])
allRels.append(("peg", pegRels))
class zephyrRels(object):
    ns = u"zephyr"
    def __init__(self, uf): self.uf = uf
    def makeSignature(self, name, tys): return self.uf.struct(self.ns, u"Signature", 0, [name, tys])
    def makeProduct(self, name, fs): return self.uf.struct(self.ns, u"Product", 0, [name, fs])
    def makeSum(self, name, attrs, con, cons): return self.uf.struct(self.ns, u"Sum", 0, [name, attrs, con, cons])
    def makeCon(self, tag, args, kind): return self.uf.struct(self.ns, u"Con", 0, [tag, args, kind])
    def makeId(self, ty, name): return self.uf.struct(self.ns, u"Id", 0, [ty, name])
    def makeOption(self, ty, name): return self.uf.struct(self.ns, u"Option", 0, [ty, name])
    def makeSequence(self, ty, name): return self.uf.struct(self.ns, u"Sequence", 0, [ty, name])
allRels.append(("zephyr", zephyrRels))
class rulesRels(object):
    ns = u"rules"
    def __init__(self, uf): self.uf = uf
    def makeIgnorePatt(self, ): return self.uf.struct(self.ns, u"IgnorePatt", 0, [])
    def makeVarPatt(self, n): return self.uf.struct(self.ns, u"VarPatt", 0, [n])
    def makeSplicePatt(self, n): return self.uf.struct(self.ns, u"SplicePatt", 0, [n])
    def makeStrPatt(self, s): return self.uf.struct(self.ns, u"StrPatt", 0, [s])
    def makeStructPatt(self, ns, func, vars): return self.uf.struct(self.ns, u"StructPatt", 0, [ns, func, vars])
    def makeListPatt(self, ps): return self.uf.struct(self.ns, u"ListPatt", 0, [ps])
    def makeVarProd(self, name): return self.uf.struct(self.ns, u"VarProd", 0, [name])
    def makeConstProd(self, name): return self.uf.struct(self.ns, u"ConstProd", 0, [name])
    def makeStrProd(self, s): return self.uf.struct(self.ns, u"StrProd", 0, [s])
    def makeCharProd(self, n): return self.uf.struct(self.ns, u"CharProd", 0, [n])
    def makeStructProd(self, ns, func, ps): return self.uf.struct(self.ns, u"StructProd", 0, [ns, func, ps])
    def makeListProd(self, ps): return self.uf.struct(self.ns, u"ListProd", 0, [ps])
    def makeSpliceListProd(self, items): return self.uf.struct(self.ns, u"SpliceListProd", 0, [items])
    def makeItemProd(self, p): return self.uf.struct(self.ns, u"ItemProd", 0, [p])
    def makeSpliceProd(self, name): return self.uf.struct(self.ns, u"SpliceProd", 0, [name])
    def makeFlattenOp(self, p): return self.uf.struct(self.ns, u"FlattenOp", 0, [p])
    def makeLengthOp(self, p): return self.uf.struct(self.ns, u"LengthOp", 0, [p])
    def makeJoinOp(self, s, p): return self.uf.struct(self.ns, u"JoinOp", 0, [s, p])
    def makeConcatOp(self, p, ps): return self.uf.struct(self.ns, u"ConcatOp", 0, [p, ps])
    def makeGenSym(self, s): return self.uf.struct(self.ns, u"GenSym", 0, [s])
    def makeRewrite(self, name, root, prods, simplify): return self.uf.struct(self.ns, u"Rewrite", 0, [name, root, prods, simplify])
    def makeLet(self, name, p): return self.uf.struct(self.ns, u"Let", 0, [name, p])
allRels.append(("rules", rulesRels))
class charRels(object):
    ns = u"char"
    def __init__(self, uf): self.uf = uf
    def makeAny(self, ): return self.uf.struct(self.ns, u"Any", 0, [])
    def makeExactly(self, c): return self.uf.struct(self.ns, u"Exactly", 0, [c])
    def makeRange(self, l, u): return self.uf.struct(self.ns, u"Range", 0, [l, u])
    def makeCall(self, n): return self.uf.struct(self.ns, u"Call", 0, [n])
    def makeComplement(self, s): return self.uf.struct(self.ns, u"Complement", 0, [s])
    def makeEither(self, l, r): return self.uf.struct(self.ns, u"Either", 0, [l, r])
allRels.append(("char", charRels))
def rhspyImp(uf, rel, env):
    return [rel.py.makeStatement(rel.builtin.makeConcat(uf.makeStr(u'from '), uf.makeList([env.get(u"M"), uf.makeStr(u' import '), env.get(u"Ns")])))]
ruleList.append(Rule("pyImp", PStruct(u"py", u"Imp", [PVar(u"M"), PVar(u"Ns")]), rhspyImp, 0))
def rhspyRet(uf, rel, env):
    return [rel.py.makeStatement(rel.builtin.makeConcat(uf.makeStr(u'return '), uf.makeList([env.get(u"E")])))]
ruleList.append(Rule("pyRet", PStruct(u"py", u"Ret", [PVar(u"E")]), rhspyRet, 0))
def rhspyRaiseIf(uf, rel, env):
    return [rel.py.makeStatement(rel.builtin.makeConcat(uf.makeStr(u'if '), uf.makeList([env.get(u"T"), uf.makeStr(u': raise ParseError("no reason")')])))]
ruleList.append(Rule("pyRaiseIf", PStruct(u"py", u"RaiseIf", [PVar(u"T")]), rhspyRaiseIf, 0))
def rhspyContIf(uf, rel, env):
    return [rel.py.makeStatement(rel.builtin.makeConcat(uf.makeStr(u'if '), uf.makeList([env.get(u"T"), uf.makeStr(u': continue')])))]
ruleList.append(Rule("pyContIf", PStruct(u"py", u"ContIf", [PVar(u"T")]), rhspyContIf, 0))
def rhspyStatement(uf, rel, env):
    return [uf.makeList([rel.builtin.makeLine(env.get(u"L"))])]
ruleList.append(Rule("pyStatement", PStruct(u"py", u"Statement", [PVar(u"L")]), rhspyStatement, 0))
def rhspyCompPass(uf, rel, env):
    return [uf.makeList([rel.builtin.makeLine(rel.builtin.makeConcat(env.get(u"H"), uf.makeList([uf.makeStr(u': pass')])))])]
ruleList.append(Rule("pyCompPass", PStruct(u"py", u"Compound", [PVar(u"H"), PList([])]), rhspyCompPass, 0))
def rhspyCompBlock(uf, rel, env):
    return [uf.makeList([rel.builtin.makeLine(rel.builtin.makeConcat(env.get(u"H"), uf.makeList([uf.makeStr(u':')]))), rel.builtin.makeBlock(rel.builtin.makeFlatten(uf.makeList([] + [env.get(u"S")] + env.elems(u"Ss"))))])]
ruleList.append(Rule("pyCompBlock", PStruct(u"py", u"Compound", [PVar(u"H"), PList([PVar(u"S"), PSplice(u"Ss")])]), rhspyCompBlock, 0))
def rhspyCondEmpty(uf, rel, env):
    return [uf.makeList([])]
ruleList.append(Rule("pyCondEmpty", PStruct(u"py", u"Conditional", [PVar(u"T"), PList([])]), rhspyCondEmpty, 0))
def rhspyCondBlock(uf, rel, env):
    return [uf.makeList([rel.builtin.makeLine(rel.builtin.makeConcat(uf.makeStr(u'if '), uf.makeList([env.get(u"T"), uf.makeStr(u':')]))), rel.builtin.makeBlock(rel.builtin.makeFlatten(uf.makeList([] + [env.get(u"S")] + env.elems(u"Ss"))))])]
ruleList.append(Rule("pyCondBlock", PStruct(u"py", u"Conditional", [PVar(u"T"), PList([PVar(u"S"), PSplice(u"Ss")])]), rhspyCondBlock, 0))
def rhspyHandEmpty(uf, rel, env):
    return [uf.makeList([])]
ruleList.append(Rule("pyHandEmpty", PStruct(u"py", u"Handler", [PVar(u"B"), PList([])]), rhspyHandEmpty, 0))
def rhspyHandBlock(uf, rel, env):
    return [uf.makeList([rel.builtin.makeLine(uf.makeStr(u'try:')), rel.builtin.makeBlock(rel.builtin.makeFlatten(env.get(u"B"))), rel.builtin.makeLine(uf.makeStr(u'except ParseError as pe:')), rel.builtin.makeBlock(rel.builtin.makeFlatten(uf.makeList([] + [env.get(u"S")] + env.elems(u"Ss"))))])]
ruleList.append(Rule("pyHandBlock", PStruct(u"py", u"Handler", [PVar(u"B"), PList([PVar(u"S"), PSplice(u"Ss")])]), rhspyHandBlock, 0))
def rhspegNamePatt(uf, rel, env):
    return [rel.builtin.makeConcat(uf.makeStr(u'env.set(u"'), uf.makeList([env.get(u"Name"), uf.makeStr(u'", rv)')]))]
ruleList.append(Rule("pegNamePatt", PStruct(u"peg", u"NamePatt", [PVar(u"Name")]), rhspegNamePatt, 0))
@let
def letsave(uf, rel): rel.constsave = rel.py.makeStatement(uf.makeStr(u'st.append(i)'))
@let
def letbackup(uf, rel): rel.constbackup = rel.py.makeStatement(uf.makeStr(u'i = st.pop()'))
@let
def letboundcheck(uf, rel): rel.constboundcheck = rel.py.makeRaiseIf(uf.makeStr(u'i >= len(self.s)'))
@let
def letskipws(uf, rel): rel.constskipws = rel.py.makeStatement(uf.makeStr(u'while i < len(self.s) and ord(self.s[i]) in [9, 10, 13, 32]: i += 1'))
def rhspegSeq(uf, rel, env):
    return [rel.builtin.makeFlatten(env.get(u"Es"))]
ruleList.append(Rule("pegSeq", PStruct(u"peg", u"Sequence", [PVar(u"Es")]), rhspegSeq, 0))
def rhspegCap(uf, rel, env):
    return [uf.makeList([] + env.elems(u"E") + [rel.py.makeStatement(env.get(u"P"))])]
ruleList.append(Rule("pegCap", PStruct(u"peg", u"Capture", [PVar(u"E"), PVar(u"P")]), rhspegCap, 0))
def rhspegProd(uf, rel, env):
    return [uf.makeList([] + env.elems(u"E") + [rel.py.makeStatement(rel.builtin.makeConcat(uf.makeStr(u'rv = '), uf.makeList([env.get(u"P")])))])]
ruleList.append(Rule("pegProd", PStruct(u"peg", u"Production", [PVar(u"E"), PVar(u"P")]), rhspegProd, 0))
def rhspegCall(uf, rel, env):
    return [uf.makeList([rel.py.makeStatement(rel.builtin.makeConcat(uf.makeStr(u'i, rv = self.parse'), uf.makeList([env.get(u"S"), uf.makeStr(u'(i)')])))])]
ruleList.append(Rule("pegCall", PStruct(u"peg", u"Call", [PVar(u"S")]), rhspegCall, 0))
def rhspegChoice(uf, rel, env):
    return [uf.makeList([rel.constsave, rel.py.makeHandler(env.get(u"L"), uf.makeList([] + [rel.constbackup] + env.elems(u"R")))])]
ruleList.append(Rule("pegChoice", PStruct(u"peg", u"Choice", [PVar(u"L"), PVar(u"R")]), rhspegChoice, 0))
def rhspegToken(uf, rel, env):
    return [uf.makeList([rel.constskipws, rel.constboundcheck, rel.py.makeStatement(uf.makeStr(u'assert i >= 0')), rel.py.makeRaiseIf(rel.builtin.makeConcat(uf.makeStr(u'self.s[i:i + '), uf.makeList([rel.builtin.makeLength(env.get(u"S")), uf.makeStr(u'] != u"'), env.get(u"S"), uf.makeStr(u'"')]))), rel.py.makeStatement(rel.builtin.makeConcat(uf.makeStr(u'self.lastMatch.append((u"TOKEN '), uf.makeList([env.get(u"S"), uf.makeStr(u'", i, i + '), rel.builtin.makeLength(env.get(u"S")), uf.makeStr(u'))')]))), rel.py.makeStatement(rel.builtin.makeConcat(uf.makeStr(u'rv = self.uf.makeStr(u"'), uf.makeList([env.get(u"S"), uf.makeStr(u'"); i += '), rel.builtin.makeLength(env.get(u"S"))])))])]
ruleList.append(Rule("pegToken", PStruct(u"peg", u"Token", [PVar(u"S")]), rhspegToken, 0))
def rhspegAny(uf, rel, env):
    return [uf.makeList([rel.py.makeStatement(uf.makeStr(u'rvs = []')), rel.py.makeCompound(uf.makeStr(u'while True'), uf.makeList([rel.constsave, rel.py.makeHandler(uf.makeList([] + env.elems(u"E") + [rel.py.makeStatement(uf.makeStr(u'rvs.append(rv)'))]), uf.makeList([rel.constbackup, rel.py.makeStatement(uf.makeStr(u'break'))]))])), rel.py.makeStatement(uf.makeStr(u'rv = rel.uf.makeList(rvs)'))])]
ruleList.append(Rule("pegAny", PStruct(u"peg", u"Any", [PVar(u"E")]), rhspegAny, 0))
def rhspegSome(uf, rel, env):
    return [rel.builtin.makeFlatten(uf.makeList([rel.peg.makeAny(env.get(u"E")), uf.makeList([rel.py.makeRaiseIf(uf.makeStr(u'not len(rvs)'))])]))]
ruleList.append(Rule("pegSome", PStruct(u"peg", u"Some", [PVar(u"E")]), rhspegSome, 0))
def rhspegMaybe(uf, rel, env):
    return [uf.makeList([rel.constsave, rel.py.makeHandler(env.get(u"E"), uf.makeList([rel.py.makeStatement(uf.makeStr(u'rv = 0')), rel.constbackup]))])]
ruleList.append(Rule("pegMaybe", PStruct(u"peg", u"Maybe", [PVar(u"E")]), rhspegMaybe, 0))
def rhspegPos(uf, rel, env):
    return [uf.makeList([rel.constsave, rel.py.makeHandler(uf.makeList([] + env.elems(u"E") + [rel.py.makeStatement(uf.makeStr(u'rv = True'))]), uf.makeList([rel.py.makeStatement(uf.makeStr(u'rv = False'))])), rel.constbackup, rel.py.makeRaiseIf(uf.makeStr(u'not rv'))])]
ruleList.append(Rule("pegPos", PStruct(u"peg", u"Positive", [PVar(u"E")]), rhspegPos, 0))
def rhspegNeg(uf, rel, env):
    return [uf.makeList([rel.constsave, rel.py.makeHandler(uf.makeList([] + env.elems(u"E") + [rel.py.makeStatement(uf.makeStr(u'rv = True'))]), uf.makeList([rel.py.makeStatement(uf.makeStr(u'rv = False'))])), rel.constbackup, rel.py.makeRaiseIf(uf.makeStr(u'rv'))])]
ruleList.append(Rule("pegNeg", PStruct(u"peg", u"Negative", [PVar(u"E")]), rhspegNeg, 0))
def rhszSig(uf, rel, env):
    return [uf.makeList([rel.py.makeCompound(rel.builtin.makeConcat(uf.makeStr(u'class '), uf.makeList([env.get(u"N"), uf.makeStr(u'Rels(object)')])), rel.builtin.makeFlatten(uf.makeList([uf.makeList([rel.py.makeStatement(rel.builtin.makeConcat(uf.makeStr(u'ns = u"'), uf.makeList([env.get(u"N"), uf.makeStr(u'"')]))), rel.py.makeStatement(uf.makeStr(u'def __init__(self, uf): self.uf = uf'))]), rel.builtin.makeFlatten(env.get(u"T"))]))), rel.py.makeStatement(rel.builtin.makeConcat(uf.makeStr(u'allRels.append(("'), uf.makeList([env.get(u"N"), uf.makeStr(u'", '), env.get(u"N"), uf.makeStr(u'Rels))')])))])]
ruleList.append(Rule("zSig", PStruct(u"zephyr", u"Signature", [PVar(u"N"), PVar(u"T")]), rhszSig, 0))
def rhszProd(uf, rel, env):
    return [uf.makeList([rel.py.makeStatement(rel.builtin.makeConcat(uf.makeStr(u'# product '), uf.makeList([env.get(u"N")])))])]
ruleList.append(Rule("zProd", PStruct(u"zephyr", u"Product", [PVar(u"N"), PVar(u"Fs")]), rhszProd, 0))
def rhszSum(uf, rel, env):
    return [rel.builtin.makeFlatten(uf.makeList([env.get(u"C"), rel.builtin.makeFlatten(env.get(u"Cs"))]))]
ruleList.append(Rule("zSum", PStruct(u"zephyr", u"Sum", [PVar(u"N"), PVar(u"A"), PVar(u"C"), PVar(u"Cs")]), rhszSum, 0))
def rhszConF(uf, rel, env):
    return [uf.makeList([rel.py.makeStatement(rel.builtin.makeConcat(uf.makeStr(u'def make'), uf.makeList([env.get(u"T"), uf.makeStr(u'(self, '), rel.builtin.makeJoin(uf.makeStr(u", "), env.get(u"A")), uf.makeStr(u'): return self.uf.struct(self.ns, u"'), env.get(u"T"), uf.makeStr(u'", '), env.get(u"K"), uf.makeStr(u', ['), rel.builtin.makeJoin(uf.makeStr(u", "), env.get(u"A")), uf.makeStr(u'])')])))])]
ruleList.append(Rule("zConF", PStruct(u"zephyr", u"Con", [PVar(u"T"), PVar(u"A"), PVar(u"K")]), rhszConF, 0))
def rhszOpt(uf, rel, env):
    return [env.get(u"N")]
ruleList.append(Rule("zOpt", PStruct(u"zephyr", u"Option", [PVar(u"T"), PVar(u"N")]), rhszOpt, 0))
def rhszSeq(uf, rel, env):
    return [env.get(u"N")]
ruleList.append(Rule("zSeq", PStruct(u"zephyr", u"Sequence", [PVar(u"T"), PVar(u"N")]), rhszSeq, 0))
def rhszId(uf, rel, env):
    return [env.get(u"N")]
ruleList.append(Rule("zId", PStruct(u"zephyr", u"Id", [PVar(u"T"), PVar(u"N")]), rhszId, 0))
def rhscharAny(uf, rel, env):
    return [uf.makeStr(u'True')]
ruleList.append(Rule("charAny", PStruct(u"char", u"Any", []), rhscharAny, 0))
def rhscharExactly(uf, rel, env):
    return [rel.builtin.makeConcat(uf.makeStr(u'c == '), uf.makeList([env.get(u"C")]))]
ruleList.append(Rule("charExactly", PStruct(u"char", u"Exactly", [PVar(u"C")]), rhscharExactly, 0))
def rhscharRange(uf, rel, env):
    return [rel.builtin.makeConcat(env.get(u"L"), uf.makeList([uf.makeStr(u' <= c <= '), env.get(u"U")]))]
ruleList.append(Rule("charRange", PStruct(u"char", u"Range", [PVar(u"L"), PVar(u"U")]), rhscharRange, 0))
def rhscharCall(uf, rel, env):
    return [rel.builtin.makeConcat(uf.makeStr(u'self.cls'), uf.makeList([env.get(u"N"), uf.makeStr(u'(c)')]))]
ruleList.append(Rule("charCall", PStruct(u"char", u"Call", [PVar(u"N")]), rhscharCall, 0))
def rhscharComp(uf, rel, env):
    return [rel.builtin.makeConcat(uf.makeStr(u'not ('), uf.makeList([env.get(u"S"), uf.makeStr(u')')]))]
ruleList.append(Rule("charComp", PStruct(u"char", u"Complement", [PVar(u"S")]), rhscharComp, 0))
def rhscharEither(uf, rel, env):
    return [rel.builtin.makeConcat(uf.makeStr(u'('), uf.makeList([env.get(u"L"), uf.makeStr(u') or ('), env.get(u"R"), uf.makeStr(u')')]))]
ruleList.append(Rule("charEither", PStruct(u"char", u"Either", [PVar(u"L"), PVar(u"R")]), rhscharEither, 0))
def rhsrPattIg(uf, rel, env):
    return [uf.makeStr(u'PIgnore()')]
ruleList.append(Rule("rPattIg", PStruct(u"rules", u"IgnorePatt", []), rhsrPattIg, 0))
def rhsrPattV(uf, rel, env):
    return [rel.builtin.makeConcat(uf.makeStr(u'PVar(u"'), uf.makeList([env.get(u"N"), uf.makeStr(u'")')]))]
ruleList.append(Rule("rPattV", PStruct(u"rules", u"VarPatt", [PVar(u"N")]), rhsrPattV, 0))
def rhsrPattSp(uf, rel, env):
    return [rel.builtin.makeConcat(uf.makeStr(u'PSplice(u"'), uf.makeList([env.get(u"N"), uf.makeStr(u'")')]))]
ruleList.append(Rule("rPattSp", PStruct(u"rules", u"SplicePatt", [PVar(u"N")]), rhsrPattSp, 0))
def rhsrPattS(uf, rel, env):
    return [rel.builtin.makeConcat(uf.makeStr(u'PStr(u'), uf.makeList([uf.makeStr(unichr(39)), env.get(u"S"), uf.makeStr(unichr(39)), uf.makeStr(u')')]))]
ruleList.append(Rule("rPattS", PStruct(u"rules", u"StrPatt", [PVar(u"S")]), rhsrPattS, 0))
def rhsrPattSt(uf, rel, env):
    return [rel.builtin.makeConcat(uf.makeStr(u'PStruct(u"'), uf.makeList([env.get(u"Ns"), uf.makeStr(u'", u"'), env.get(u"F"), uf.makeStr(u'", ['), rel.builtin.makeJoin(uf.makeStr(u", "), env.get(u"Ps")), uf.makeStr(u'])')]))]
ruleList.append(Rule("rPattSt", PStruct(u"rules", u"StructPatt", [PVar(u"Ns"), PVar(u"F"), PVar(u"Ps")]), rhsrPattSt, 0))
def rhsrPattL(uf, rel, env):
    return [rel.builtin.makeConcat(uf.makeStr(u'PList(['), uf.makeList([rel.builtin.makeJoin(uf.makeStr(u", "), env.get(u"Ps")), uf.makeStr(u'])')]))]
ruleList.append(Rule("rPattL", PStruct(u"rules", u"ListPatt", [PVar(u"Ps")]), rhsrPattL, 0))
def rhsrProdV(uf, rel, env):
    return [rel.builtin.makeConcat(uf.makeStr(u'env.get(u"'), uf.makeList([env.get(u"Name"), uf.makeStr(u'")')]))]
ruleList.append(Rule("rProdV", PStruct(u"rules", u"VarProd", [PVar(u"Name")]), rhsrProdV, 0))
def rhsrProdC(uf, rel, env):
    return [rel.builtin.makeConcat(uf.makeStr(u'rel.const'), uf.makeList([env.get(u"Name")]))]
ruleList.append(Rule("rProdC", PStruct(u"rules", u"ConstProd", [PVar(u"Name")]), rhsrProdC, 0))
def rhsrProdS(uf, rel, env):
    return [rel.builtin.makeConcat(uf.makeStr(u'uf.makeStr(u'), uf.makeList([uf.makeStr(unichr(39)), env.get(u"S"), uf.makeStr(unichr(39)), uf.makeStr(u')')]))]
ruleList.append(Rule("rProdS", PStruct(u"rules", u"StrProd", [PVar(u"S")]), rhsrProdS, 0))
def rhsrProdCh(uf, rel, env):
    return [rel.builtin.makeConcat(uf.makeStr(u'uf.makeStr(unichr('), uf.makeList([env.get(u"N"), uf.makeStr(u'))')]))]
ruleList.append(Rule("rProdCh", PStruct(u"rules", u"CharProd", [PVar(u"N")]), rhsrProdCh, 0))
def rhsrProdSt(uf, rel, env):
    return [rel.builtin.makeConcat(uf.makeStr(u'rel.'), uf.makeList([env.get(u"Ns"), uf.makeStr(u'.make'), env.get(u"F"), uf.makeStr(u'('), rel.builtin.makeJoin(uf.makeStr(u", "), env.get(u"Ps")), uf.makeStr(u')')]))]
ruleList.append(Rule("rProdSt", PStruct(u"rules", u"StructProd", [PVar(u"Ns"), PVar(u"F"), PVar(u"Ps")]), rhsrProdSt, 0))
def rhsrProdL(uf, rel, env):
    return [rel.builtin.makeConcat(uf.makeStr(u'uf.makeList(['), uf.makeList([rel.builtin.makeJoin(uf.makeStr(u", "), env.get(u"Ps")), uf.makeStr(u'])')]))]
ruleList.append(Rule("rProdL", PStruct(u"rules", u"ListProd", [PVar(u"Ps")]), rhsrProdL, 0))
def rhsrProdSL(uf, rel, env):
    return [rel.builtin.makeConcat(uf.makeStr(u'uf.makeList([]'), uf.makeList([rel.builtin.makeJoin(uf.makeStr(u""), env.get(u"Is")), uf.makeStr(u')')]))]
ruleList.append(Rule("rProdSL", PStruct(u"rules", u"SpliceListProd", [PVar(u"Is")]), rhsrProdSL, 0))
def rhsrProdI(uf, rel, env):
    return [rel.builtin.makeConcat(uf.makeStr(u' + ['), uf.makeList([env.get(u"P"), uf.makeStr(u']')]))]
ruleList.append(Rule("rProdI", PStruct(u"rules", u"ItemProd", [PVar(u"P")]), rhsrProdI, 0))
def rhsrProdSp(uf, rel, env):
    return [rel.builtin.makeConcat(uf.makeStr(u' + env.elems(u"'), uf.makeList([env.get(u"N"), uf.makeStr(u'")')]))]
ruleList.append(Rule("rProdSp", PStruct(u"rules", u"SpliceProd", [PVar(u"N")]), rhsrProdSp, 0))
def rhsrProdOF(uf, rel, env):
    return [rel.builtin.makeConcat(uf.makeStr(u'rel.builtin.makeFlatten('), uf.makeList([env.get(u"P"), uf.makeStr(u')')]))]
ruleList.append(Rule("rProdOF", PStruct(u"rules", u"FlattenOp", [PVar(u"P")]), rhsrProdOF, 0))
def rhsrProdOL(uf, rel, env):
    return [rel.builtin.makeConcat(uf.makeStr(u'rel.builtin.makeLength('), uf.makeList([env.get(u"P"), uf.makeStr(u')')]))]
ruleList.append(Rule("rProdOL", PStruct(u"rules", u"LengthOp", [PVar(u"P")]), rhsrProdOL, 0))
def rhsrProdOJ(uf, rel, env):
    return [rel.builtin.makeConcat(uf.makeStr(u'rel.builtin.makeJoin(uf.makeStr(u"'), uf.makeList([env.get(u"S"), uf.makeStr(u'"), '), env.get(u"P"), uf.makeStr(u')')]))]
ruleList.append(Rule("rProdOJ", PStruct(u"rules", u"JoinOp", [PVar(u"S"), PVar(u"P")]), rhsrProdOJ, 0))
def rhsrProdOC(uf, rel, env):
    return [rel.builtin.makeConcat(uf.makeStr(u'rel.builtin.makeConcat('), uf.makeList([env.get(u"P"), uf.makeStr(u', uf.makeList(['), rel.builtin.makeJoin(uf.makeStr(u", "), env.get(u"Ps")), uf.makeStr(u']))')]))]
ruleList.append(Rule("rProdOC", PStruct(u"rules", u"ConcatOp", [PVar(u"P"), PVar(u"Ps")]), rhsrProdOC, 0))
def rhsrGenSym(uf, rel, env):
    return [rel.builtin.makeConcat(uf.makeStr(u'uf.makeStr(rel.gensym(u"'), uf.makeList([env.get(u"S"), uf.makeStr(u'"))')]))]
ruleList.append(Rule("rGenSym", PStruct(u"rules", u"GenSym", [PVar(u"S")]), rhsrGenSym, 0))
def rhsrRule(uf, rel, env):
    return [uf.makeList([rel.py.makeCompound(rel.builtin.makeConcat(uf.makeStr(u'def rhs'), uf.makeList([env.get(u"Name"), uf.makeStr(u'(uf, rel, env)')])), uf.makeList([rel.py.makeRet(rel.builtin.makeConcat(uf.makeStr(u'['), uf.makeList([rel.builtin.makeJoin(uf.makeStr(u", "), env.get(u"Ps")), uf.makeStr(u']')])))])), rel.py.makeStatement(rel.builtin.makeConcat(uf.makeStr(u'ruleList.append(Rule("'), uf.makeList([env.get(u"Name"), uf.makeStr(u'", '), env.get(u"Pat"), uf.makeStr(u', rhs'), env.get(u"Name"), uf.makeStr(u', '), env.get(u"K"), uf.makeStr(u'))')])))])]
ruleList.append(Rule("rRule", PStruct(u"rules", u"Rewrite", [PVar(u"Name"), PVar(u"Pat"), PVar(u"Ps"), PVar(u"K")]), rhsrRule, 0))
def rhsrLet(uf, rel, env):
    return [uf.makeList([rel.py.makeStatement(uf.makeStr(u'@let')), rel.py.makeStatement(rel.builtin.makeConcat(uf.makeStr(u'def let'), uf.makeList([env.get(u"Name"), uf.makeStr(u'(uf, rel): rel.const'), env.get(u"Name"), uf.makeStr(u' = '), env.get(u"P")])))])]
ruleList.append(Rule("rLet", PStruct(u"rules", u"Let", [PVar(u"Name"), PVar(u"P")]), rhsrLet, 0))
def target(driver, *args):
    driver.exe_name = "ZADDY".lower() + "c"
    return main, None
class ZADDYParser(object):
    def __init__(self, s, uf):
        self.s = s; self.uf = uf
        self.lastMatch = []
        for name in unrolledRels: setattr(self, name, frozenRels[name](uf))
        for l in unrolledLets: l(uf, self)
    gs = 0
    def gensym(self, s):
        self.gs += 1
        return s + str(self.gs).decode("utf-8")
    def parse(self):
        return self.parseZADDY(0)
    def clsWhitespace(self, c):
        return (c == 9) or ((c == 10) or ((c == 13) or (c == 32)))
    def clsDigit(self, c):
        return 48 <= c <= 57
    def clsUpper(self, c):
        return 65 <= c <= 90
    def clsLower(self, c):
        return 97 <= c <= 122
    def clsAlpha(self, c):
        return (self.clsUpper(c)) or (self.clsLower(c))
    def clsAlphanumeric(self, c):
        return (self.clsAlpha(c)) or (self.clsDigit(c))
    def clsQuote(self, c):
        return c == 39
    def clsQuoted(self, c):
        return not ((c == 10) or ((c == 13) or (c == 39)))
    def clsEllipsis(self, c):
        return c == 8230
    def clsNotNewline(self, c):
        return not (c == 10)
    def parseWS(self, i):
        if i >= len(self.s): raise ParseError("no reason")
        start = i; assert start >= 0
        while i < len(self.s) and self.clsWhitespace(ord(self.s[i])): i += 1
        rv = self.uf.makeStr(self.s[start:i])
        self.lastMatch.append((u"TOKEN WS", start, i))
        return i, rv
    def parseNumber(self, i):
        if i >= len(self.s): raise ParseError("no reason")
        start = i; assert start >= 0
        if i >= len(self.s) or not self.clsDigit(ord(self.s[i])): raise ParseError("no reason")
        while i < len(self.s) and self.clsDigit(ord(self.s[i])): i += 1
        rv = self.uf.makeStr(self.s[start:i])
        self.lastMatch.append((u"TOKEN Number", start, i))
        return i, rv
    def parseId(self, i):
        if i >= len(self.s): raise ParseError("no reason")
        start = i; assert start >= 0
        if i >= len(self.s) or not self.clsAlpha(ord(self.s[i])): raise ParseError("no reason")
        i += 1
        while i < len(self.s) and self.clsAlphanumeric(ord(self.s[i])): i += 1
        rv = self.uf.makeStr(self.s[start:i])
        self.lastMatch.append((u"TOKEN Id", start, i))
        return i, rv
    def parsePVar(self, i):
        if i >= len(self.s): raise ParseError("no reason")
        start = i; assert start >= 0
        if i >= len(self.s) or not self.clsUpper(ord(self.s[i])): raise ParseError("no reason")
        i += 1
        while i < len(self.s) and self.clsAlphanumeric(ord(self.s[i])): i += 1
        rv = self.uf.makeStr(self.s[start:i])
        self.lastMatch.append((u"TOKEN PVar", start, i))
        return i, rv
    def parsePConst(self, i):
        if i >= len(self.s): raise ParseError("no reason")
        start = i; assert start >= 0
        if i >= len(self.s) or not self.clsLower(ord(self.s[i])): raise ParseError("no reason")
        while i < len(self.s) and self.clsLower(ord(self.s[i])): i += 1
        rv = self.uf.makeStr(self.s[start:i])
        self.lastMatch.append((u"TOKEN PConst", start, i))
        return i, rv
    def parseZTId(self, i):
        if i >= len(self.s): raise ParseError("no reason")
        start = i; assert start >= 0
        if i >= len(self.s) or not self.clsLower(ord(self.s[i])): raise ParseError("no reason")
        i += 1
        while i < len(self.s) and self.clsAlphanumeric(ord(self.s[i])): i += 1
        rv = self.uf.makeStr(self.s[start:i])
        self.lastMatch.append((u"TOKEN ZTId", start, i))
        return i, rv
    def parseZCId(self, i):
        if i >= len(self.s): raise ParseError("no reason")
        start = i; assert start >= 0
        if i >= len(self.s) or not self.clsUpper(ord(self.s[i])): raise ParseError("no reason")
        i += 1
        while i < len(self.s) and self.clsAlphanumeric(ord(self.s[i])): i += 1
        rv = self.uf.makeStr(self.s[start:i])
        self.lastMatch.append((u"TOKEN ZCId", start, i))
        return i, rv
    def parseQuote(self, i):
        if i >= len(self.s): raise ParseError("no reason")
        start = i; assert start >= 0
        if i >= len(self.s) or not self.clsQuote(ord(self.s[i])): raise ParseError("no reason")
        i += 1
        rv = self.uf.makeStr(self.s[start:i])
        self.lastMatch.append((u"TOKEN Quote", start, i))
        return i, rv
    def parseQuoted(self, i):
        if i >= len(self.s): raise ParseError("no reason")
        start = i; assert start >= 0
        while i < len(self.s) and self.clsQuoted(ord(self.s[i])): i += 1
        rv = self.uf.makeStr(self.s[start:i])
        self.lastMatch.append((u"TOKEN Quoted", start, i))
        return i, rv
    def parseEllipsis(self, i):
        if i >= len(self.s): raise ParseError("no reason")
        start = i; assert start >= 0
        if i >= len(self.s) or not self.clsEllipsis(ord(self.s[i])): raise ParseError("no reason")
        i += 1
        rv = self.uf.makeStr(self.s[start:i])
        self.lastMatch.append((u"TOKEN Ellipsis", start, i))
        return i, rv
    def parseCommentText(self, i):
        if i >= len(self.s): raise ParseError("no reason")
        start = i; assert start >= 0
        while i < len(self.s) and self.clsNotNewline(ord(self.s[i])): i += 1
        rv = self.uf.makeStr(self.s[start:i])
        self.lastMatch.append((u"TOKEN CommentText", start, i))
        return i, rv
    @cached
    def parseSTRING(self, i):
        rel = self; uf = self.uf; st = []; env = CapEnv(uf)
        i, rv = self.parseWS(i)
        i, rv = self.parseQuote(i)
        i, rv = self.parseQuoted(i)
        env.set(u"Q", rv)
        i, rv = self.parseQuote(i)
        rv = env.get(u"Q")
        return i, rv
    @cached
    def parseID(self, i):
        rel = self; uf = self.uf; st = []; env = CapEnv(uf)
        i, rv = self.parseWS(i)
        i, rv = self.parseId(i)
        return i, rv
    @cached
    def parseDOTS(self, i):
        rel = self; uf = self.uf; st = []; env = CapEnv(uf)
        st.append(i)
        try:
            i, rv = self.parseEllipsis(i)
        except ParseError as pe:
            i = st.pop()
            while i < len(self.s) and ord(self.s[i]) in [9, 10, 13, 32]: i += 1
            if i >= len(self.s): raise ParseError("no reason")
            assert i >= 0
            if self.s[i:i + 3] != u"...": raise ParseError("no reason")
            self.lastMatch.append((u"TOKEN ...", i, i + 3))
            rv = self.uf.makeStr(u"..."); i += 3
        return i, rv
    @cached
    def parseCOMMENT(self, i):
        rel = self; uf = self.uf; st = []; env = CapEnv(uf)
        while i < len(self.s) and ord(self.s[i]) in [9, 10, 13, 32]: i += 1
        if i >= len(self.s): raise ParseError("no reason")
        assert i >= 0
        if self.s[i:i + 2] != u"--": raise ParseError("no reason")
        self.lastMatch.append((u"TOKEN --", i, i + 2))
        rv = self.uf.makeStr(u"--"); i += 2
        i, rv = self.parseCommentText(i)
        rv = uf.makeList([])
        return i, rv
    @cached
    def parseSIGNATURE(self, i):
        rel = self; uf = self.uf; st = []; env = CapEnv(uf)
        while i < len(self.s) and ord(self.s[i]) in [9, 10, 13, 32]: i += 1
        if i >= len(self.s): raise ParseError("no reason")
        assert i >= 0
        if self.s[i:i + 10] != u".signature": raise ParseError("no reason")
        self.lastMatch.append((u"TOKEN .signature", i, i + 10))
        rv = self.uf.makeStr(u".signature"); i += 10
        i, rv = self.parseID(i)
        env.set(u"Name", rv)
        rvs = []
        while True:
            st.append(i)
            try:
                st.append(i)
                try:
                    i, rv = self.parseZTY(i)
                except ParseError as pe:
                    i = st.pop()
                    i, rv = self.parseCOMMENT(i)
                rvs.append(rv)
            except ParseError as pe:
                i = st.pop()
                break
        rv = rel.uf.makeList(rvs)
        env.set(u"Tys", rv)
        rv = rel.zephyr.makeSignature(env.get(u"Name"), rel.builtin.makeFlatten(env.get(u"Tys")))
        return i, rv
    @cached
    def parseZTY(self, i):
        rel = self; uf = self.uf; st = []; env = CapEnv(uf)
        i, rv = self.parseID(i)
        env.set(u"Name", rv)
        while i < len(self.s) and ord(self.s[i]) in [9, 10, 13, 32]: i += 1
        if i >= len(self.s): raise ParseError("no reason")
        assert i >= 0
        if self.s[i:i + 1] != u"=": raise ParseError("no reason")
        self.lastMatch.append((u"TOKEN =", i, i + 1))
        rv = self.uf.makeStr(u"="); i += 1
        st.append(i)
        try:
            i, rv = self.parseFIELDS(i)
            env.set(u"Fs", rv)
            rv = uf.makeList([rel.zephyr.makeProduct(env.get(u"Name"), env.get(u"Fs"))])
        except ParseError as pe:
            i = st.pop()
            i, rv = self.parseZCON(i)
            env.set(u"Con", rv)
            rvs = []
            while True:
                st.append(i)
                try:
                    while i < len(self.s) and ord(self.s[i]) in [9, 10, 13, 32]: i += 1
                    if i >= len(self.s): raise ParseError("no reason")
                    assert i >= 0
                    if self.s[i:i + 1] != u"|": raise ParseError("no reason")
                    self.lastMatch.append((u"TOKEN |", i, i + 1))
                    rv = self.uf.makeStr(u"|"); i += 1
                    i, rv = self.parseZCON(i)
                    rvs.append(rv)
                except ParseError as pe:
                    i = st.pop()
                    break
            rv = rel.uf.makeList(rvs)
            env.set(u"Cons", rv)
            st.append(i)
            try:
                while i < len(self.s) and ord(self.s[i]) in [9, 10, 13, 32]: i += 1
                if i >= len(self.s): raise ParseError("no reason")
                assert i >= 0
                if self.s[i:i + 10] != u"attributes": raise ParseError("no reason")
                self.lastMatch.append((u"TOKEN attributes", i, i + 10))
                rv = self.uf.makeStr(u"attributes"); i += 10
                i, rv = self.parseFIELDS(i)
                env.set(u"Attrs", rv)
                rv = uf.makeList([rel.zephyr.makeSum(env.get(u"Name"), env.get(u"Attrs"), env.get(u"Con"), env.get(u"Cons"))])
            except ParseError as pe:
                i = st.pop()
                rv = uf.makeList([rel.zephyr.makeSum(env.get(u"Name"), uf.makeList([]), env.get(u"Con"), env.get(u"Cons"))])
        return i, rv
    @cached
    def parseZCON(self, i):
        rel = self; uf = self.uf; st = []; env = CapEnv(uf)
        i, rv = self.parseWS(i)
        st.append(i)
        try:
            i, rv = self.parseZCId(i)
            env.set(u"Tag", rv)
            i, rv = self.parseFIELDS(i)
            env.set(u"Args", rv)
            i, rv = self.parseZKIND(i)
            env.set(u"K", rv)
            rv = rel.zephyr.makeCon(env.get(u"Tag"), env.get(u"Args"), env.get(u"K"))
        except ParseError as pe:
            i = st.pop()
            i, rv = self.parseZCId(i)
            env.set(u"Tag", rv)
            i, rv = self.parseZKIND(i)
            env.set(u"K", rv)
            rv = rel.zephyr.makeCon(env.get(u"Tag"), uf.makeList([]), env.get(u"K"))
        return i, rv
    @cached
    def parseZKIND(self, i):
        rel = self; uf = self.uf; st = []; env = CapEnv(uf)
        st.append(i)
        try:
            while i < len(self.s) and ord(self.s[i]) in [9, 10, 13, 32]: i += 1
            if i >= len(self.s): raise ParseError("no reason")
            assert i >= 0
            if self.s[i:i + 5] != u".comm": raise ParseError("no reason")
            self.lastMatch.append((u"TOKEN .comm", i, i + 5))
            rv = self.uf.makeStr(u".comm"); i += 5
            rv = uf.makeStr(u'1')
        except ParseError as pe:
            i = st.pop()
            st.append(i)
            try:
                while i < len(self.s) and ord(self.s[i]) in [9, 10, 13, 32]: i += 1
                if i >= len(self.s): raise ParseError("no reason")
                assert i >= 0
                if self.s[i:i + 6] != u".assoc": raise ParseError("no reason")
                self.lastMatch.append((u"TOKEN .assoc", i, i + 6))
                rv = self.uf.makeStr(u".assoc"); i += 6
                rv = uf.makeStr(u'2')
            except ParseError as pe:
                i = st.pop()
                st.append(i)
                try:
                    while i < len(self.s) and ord(self.s[i]) in [9, 10, 13, 32]: i += 1
                    if i >= len(self.s): raise ParseError("no reason")
                    assert i >= 0
                    if self.s[i:i + 3] != u".ac": raise ParseError("no reason")
                    self.lastMatch.append((u"TOKEN .ac", i, i + 3))
                    rv = self.uf.makeStr(u".ac"); i += 3
                    rv = uf.makeStr(u'3')
                except ParseError as pe:
                    i = st.pop()
                    rv = uf.makeStr(u'0')
        return i, rv
    @cached
    def parseFIELDS(self, i):
        rel = self; uf = self.uf; st = []; env = CapEnv(uf)
        while i < len(self.s) and ord(self.s[i]) in [9, 10, 13, 32]: i += 1
        if i >= len(self.s): raise ParseError("no reason")
        assert i >= 0
        if self.s[i:i + 1] != u"(": raise ParseError("no reason")
        self.lastMatch.append((u"TOKEN (", i, i + 1))
        rv = self.uf.makeStr(u"("); i += 1
        i, rv = self.parseFIELD(i)
        env.set(u"F", rv)
        rvs = []
        while True:
            st.append(i)
            try:
                while i < len(self.s) and ord(self.s[i]) in [9, 10, 13, 32]: i += 1
                if i >= len(self.s): raise ParseError("no reason")
                assert i >= 0
                if self.s[i:i + 1] != u",": raise ParseError("no reason")
                self.lastMatch.append((u"TOKEN ,", i, i + 1))
                rv = self.uf.makeStr(u","); i += 1
                i, rv = self.parseFIELD(i)
                rvs.append(rv)
            except ParseError as pe:
                i = st.pop()
                break
        rv = rel.uf.makeList(rvs)
        env.set(u"Fs", rv)
        while i < len(self.s) and ord(self.s[i]) in [9, 10, 13, 32]: i += 1
        if i >= len(self.s): raise ParseError("no reason")
        assert i >= 0
        if self.s[i:i + 1] != u")": raise ParseError("no reason")
        self.lastMatch.append((u"TOKEN )", i, i + 1))
        rv = self.uf.makeStr(u")"); i += 1
        rv = rel.builtin.makeFlatten(uf.makeList([uf.makeList([env.get(u"F")]), env.get(u"Fs")]))
        return i, rv
    @cached
    def parseFIELD(self, i):
        rel = self; uf = self.uf; st = []; env = CapEnv(uf)
        i, rv = self.parseWS(i)
        st.append(i)
        try:
            i, rv = self.parseZTId(i)
            env.set(u"Ty", rv)
            while i < len(self.s) and ord(self.s[i]) in [9, 10, 13, 32]: i += 1
            if i >= len(self.s): raise ParseError("no reason")
            assert i >= 0
            if self.s[i:i + 1] != u"?": raise ParseError("no reason")
            self.lastMatch.append((u"TOKEN ?", i, i + 1))
            rv = self.uf.makeStr(u"?"); i += 1
            i, rv = self.parseID(i)
            env.set(u"Name", rv)
            rv = rel.zephyr.makeOption(env.get(u"Ty"), env.get(u"Name"))
        except ParseError as pe:
            i = st.pop()
            st.append(i)
            try:
                i, rv = self.parseZTId(i)
                env.set(u"Ty", rv)
                while i < len(self.s) and ord(self.s[i]) in [9, 10, 13, 32]: i += 1
                if i >= len(self.s): raise ParseError("no reason")
                assert i >= 0
                if self.s[i:i + 1] != u"*": raise ParseError("no reason")
                self.lastMatch.append((u"TOKEN *", i, i + 1))
                rv = self.uf.makeStr(u"*"); i += 1
                i, rv = self.parseID(i)
                env.set(u"Name", rv)
                rv = rel.zephyr.makeSequence(env.get(u"Ty"), env.get(u"Name"))
            except ParseError as pe:
                i = st.pop()
                i, rv = self.parseZTId(i)
                env.set(u"Ty", rv)
                i, rv = self.parseID(i)
                env.set(u"Name", rv)
                rv = rel.zephyr.makeId(env.get(u"Ty"), env.get(u"Name"))
        return i, rv
    @cached
    def parseRULES(self, i):
        rel = self; uf = self.uf; st = []; env = CapEnv(uf)
        while i < len(self.s) and ord(self.s[i]) in [9, 10, 13, 32]: i += 1
        if i >= len(self.s): raise ParseError("no reason")
        assert i >= 0
        if self.s[i:i + 6] != u".rules": raise ParseError("no reason")
        self.lastMatch.append((u"TOKEN .rules", i, i + 6))
        rv = self.uf.makeStr(u".rules"); i += 6
        rvs = []
        while True:
            st.append(i)
            try:
                st.append(i)
                try:
                    i, rv = self.parseCHRULE(i)
                except ParseError as pe:
                    i = st.pop()
                    st.append(i)
                    try:
                        i, rv = self.parseCHLET(i)
                    except ParseError as pe:
                        i = st.pop()
                        i, rv = self.parseCOMMENT(i)
                rvs.append(rv)
            except ParseError as pe:
                i = st.pop()
                break
        rv = rel.uf.makeList(rvs)
        env.set(u"Hs", rv)
        rv = rel.builtin.makeFlatten(env.get(u"Hs"))
        return i, rv
    @cached
    def parseCHRULE(self, i):
        rel = self; uf = self.uf; st = []; env = CapEnv(uf)
        i, rv = self.parseID(i)
        env.set(u"Name", rv)
        while i < len(self.s) and ord(self.s[i]) in [9, 10, 13, 32]: i += 1
        if i >= len(self.s): raise ParseError("no reason")
        assert i >= 0
        if self.s[i:i + 1] != u"@": raise ParseError("no reason")
        self.lastMatch.append((u"TOKEN @", i, i + 1))
        rv = self.uf.makeStr(u"@"); i += 1
        i, rv = self.parseWS(i)
        i, rv = self.parseCPATT(i)
        env.set(u"Root", rv)
        st.append(i)
        try:
            while i < len(self.s) and ord(self.s[i]) in [9, 10, 13, 32]: i += 1
            if i >= len(self.s): raise ParseError("no reason")
            assert i >= 0
            if self.s[i:i + 3] != u"==>": raise ParseError("no reason")
            self.lastMatch.append((u"TOKEN ==>", i, i + 3))
            rv = self.uf.makeStr(u"==>"); i += 3
            i, rv = self.parseCPRODS(i)
            env.set(u"Prods", rv)
            while i < len(self.s) and ord(self.s[i]) in [9, 10, 13, 32]: i += 1
            if i >= len(self.s): raise ParseError("no reason")
            assert i >= 0
            if self.s[i:i + 1] != u";": raise ParseError("no reason")
            self.lastMatch.append((u"TOKEN ;", i, i + 1))
            rv = self.uf.makeStr(u";"); i += 1
            rv = rel.rules.makeRewrite(env.get(u"Name"), env.get(u"Root"), env.get(u"Prods"), uf.makeStr(u'0'))
        except ParseError as pe:
            i = st.pop()
            while i < len(self.s) and ord(self.s[i]) in [9, 10, 13, 32]: i += 1
            if i >= len(self.s): raise ParseError("no reason")
            assert i >= 0
            if self.s[i:i + 3] != u"<=>": raise ParseError("no reason")
            self.lastMatch.append((u"TOKEN <=>", i, i + 3))
            rv = self.uf.makeStr(u"<=>"); i += 3
            i, rv = self.parseCPRODS(i)
            env.set(u"Prods", rv)
            while i < len(self.s) and ord(self.s[i]) in [9, 10, 13, 32]: i += 1
            if i >= len(self.s): raise ParseError("no reason")
            assert i >= 0
            if self.s[i:i + 1] != u";": raise ParseError("no reason")
            self.lastMatch.append((u"TOKEN ;", i, i + 1))
            rv = self.uf.makeStr(u";"); i += 1
            rv = rel.rules.makeRewrite(env.get(u"Name"), env.get(u"Root"), env.get(u"Prods"), uf.makeStr(u'1'))
        return i, rv
    @cached
    def parseCHLET(self, i):
        rel = self; uf = self.uf; st = []; env = CapEnv(uf)
        while i < len(self.s) and ord(self.s[i]) in [9, 10, 13, 32]: i += 1
        if i >= len(self.s): raise ParseError("no reason")
        assert i >= 0
        if self.s[i:i + 3] != u"let": raise ParseError("no reason")
        self.lastMatch.append((u"TOKEN let", i, i + 3))
        rv = self.uf.makeStr(u"let"); i += 3
        i, rv = self.parseID(i)
        env.set(u"Name", rv)
        while i < len(self.s) and ord(self.s[i]) in [9, 10, 13, 32]: i += 1
        if i >= len(self.s): raise ParseError("no reason")
        assert i >= 0
        if self.s[i:i + 2] != u":=": raise ParseError("no reason")
        self.lastMatch.append((u"TOKEN :=", i, i + 2))
        rv = self.uf.makeStr(u":="); i += 2
        i, rv = self.parseCPROD1(i)
        env.set(u"P", rv)
        while i < len(self.s) and ord(self.s[i]) in [9, 10, 13, 32]: i += 1
        if i >= len(self.s): raise ParseError("no reason")
        assert i >= 0
        if self.s[i:i + 1] != u";": raise ParseError("no reason")
        self.lastMatch.append((u"TOKEN ;", i, i + 1))
        rv = self.uf.makeStr(u";"); i += 1
        rv = rel.rules.makeLet(env.get(u"Name"), env.get(u"P"))
        return i, rv
    @cached
    def parseCPATTS(self, i):
        rel = self; uf = self.uf; st = []; env = CapEnv(uf)
        i, rv = self.parseWS(i)
        i, rv = self.parseCPATT(i)
        env.set(u"P", rv)
        rvs = []
        while True:
            st.append(i)
            try:
                while i < len(self.s) and ord(self.s[i]) in [9, 10, 13, 32]: i += 1
                if i >= len(self.s): raise ParseError("no reason")
                assert i >= 0
                if self.s[i:i + 1] != u",": raise ParseError("no reason")
                self.lastMatch.append((u"TOKEN ,", i, i + 1))
                rv = self.uf.makeStr(u","); i += 1
                i, rv = self.parseWS(i)
                i, rv = self.parseCPATT(i)
                rvs.append(rv)
            except ParseError as pe:
                i = st.pop()
                break
        rv = rel.uf.makeList(rvs)
        env.set(u"Ps", rv)
        rv = rel.builtin.makeFlatten(uf.makeList([uf.makeList([env.get(u"P")]), env.get(u"Ps")]))
        return i, rv
    @cached
    def parsePITEM(self, i):
        rel = self; uf = self.uf; st = []; env = CapEnv(uf)
        st.append(i)
        try:
            i, rv = self.parseWS(i)
            i, rv = self.parsePVar(i)
            env.set(u"N", rv)
            i, rv = self.parseDOTS(i)
            rv = rel.rules.makeSplicePatt(env.get(u"N"))
        except ParseError as pe:
            i = st.pop()
            i, rv = self.parseWS(i)
            i, rv = self.parseCPATT(i)
        return i, rv
    @cached
    def parsePITEMS(self, i):
        rel = self; uf = self.uf; st = []; env = CapEnv(uf)
        i, rv = self.parsePITEM(i)
        env.set(u"P", rv)
        rvs = []
        while True:
            st.append(i)
            try:
                while i < len(self.s) and ord(self.s[i]) in [9, 10, 13, 32]: i += 1
                if i >= len(self.s): raise ParseError("no reason")
                assert i >= 0
                if self.s[i:i + 1] != u",": raise ParseError("no reason")
                self.lastMatch.append((u"TOKEN ,", i, i + 1))
                rv = self.uf.makeStr(u","); i += 1
                i, rv = self.parsePITEM(i)
                rvs.append(rv)
            except ParseError as pe:
                i = st.pop()
                break
        rv = rel.uf.makeList(rvs)
        env.set(u"Ps", rv)
        rv = rel.builtin.makeFlatten(uf.makeList([uf.makeList([env.get(u"P")]), env.get(u"Ps")]))
        return i, rv
    @cached
    def parseCPATT(self, i):
        rel = self; uf = self.uf; st = []; env = CapEnv(uf)
        st.append(i)
        try:
            while i < len(self.s) and ord(self.s[i]) in [9, 10, 13, 32]: i += 1
            if i >= len(self.s): raise ParseError("no reason")
            assert i >= 0
            if self.s[i:i + 1] != u"_": raise ParseError("no reason")
            self.lastMatch.append((u"TOKEN _", i, i + 1))
            rv = self.uf.makeStr(u"_"); i += 1
            rv = rel.rules.makeIgnorePatt()
        except ParseError as pe:
            i = st.pop()
            st.append(i)
            try:
                while i < len(self.s) and ord(self.s[i]) in [9, 10, 13, 32]: i += 1
                if i >= len(self.s): raise ParseError("no reason")
                assert i >= 0
                if self.s[i:i + 2] != u"[]": raise ParseError("no reason")
                self.lastMatch.append((u"TOKEN []", i, i + 2))
                rv = self.uf.makeStr(u"[]"); i += 2
                rv = rel.rules.makeListPatt(uf.makeList([]))
            except ParseError as pe:
                i = st.pop()
                st.append(i)
                try:
                    while i < len(self.s) and ord(self.s[i]) in [9, 10, 13, 32]: i += 1
                    if i >= len(self.s): raise ParseError("no reason")
                    assert i >= 0
                    if self.s[i:i + 1] != u"[": raise ParseError("no reason")
                    self.lastMatch.append((u"TOKEN [", i, i + 1))
                    rv = self.uf.makeStr(u"["); i += 1
                    i, rv = self.parsePITEMS(i)
                    env.set(u"Ps", rv)
                    while i < len(self.s) and ord(self.s[i]) in [9, 10, 13, 32]: i += 1
                    if i >= len(self.s): raise ParseError("no reason")
                    assert i >= 0
                    if self.s[i:i + 1] != u"]": raise ParseError("no reason")
                    self.lastMatch.append((u"TOKEN ]", i, i + 1))
                    rv = self.uf.makeStr(u"]"); i += 1
                    rv = rel.rules.makeListPatt(env.get(u"Ps"))
                except ParseError as pe:
                    i = st.pop()
                    st.append(i)
                    try:
                        i, rv = self.parseSTRING(i)
                        env.set(u"S", rv)
                        rv = rel.rules.makeStrPatt(env.get(u"S"))
                    except ParseError as pe:
                        i = st.pop()
                        st.append(i)
                        try:
                            i, rv = self.parseID(i)
                            env.set(u"Ns", rv)
                            while i < len(self.s) and ord(self.s[i]) in [9, 10, 13, 32]: i += 1
                            if i >= len(self.s): raise ParseError("no reason")
                            assert i >= 0
                            if self.s[i:i + 1] != u".": raise ParseError("no reason")
                            self.lastMatch.append((u"TOKEN .", i, i + 1))
                            rv = self.uf.makeStr(u"."); i += 1
                            i, rv = self.parseID(i)
                            env.set(u"Func", rv)
                            while i < len(self.s) and ord(self.s[i]) in [9, 10, 13, 32]: i += 1
                            if i >= len(self.s): raise ParseError("no reason")
                            assert i >= 0
                            if self.s[i:i + 1] != u"(": raise ParseError("no reason")
                            self.lastMatch.append((u"TOKEN (", i, i + 1))
                            rv = self.uf.makeStr(u"("); i += 1
                            i, rv = self.parseCPATTS(i)
                            env.set(u"Ps", rv)
                            while i < len(self.s) and ord(self.s[i]) in [9, 10, 13, 32]: i += 1
                            if i >= len(self.s): raise ParseError("no reason")
                            assert i >= 0
                            if self.s[i:i + 1] != u")": raise ParseError("no reason")
                            self.lastMatch.append((u"TOKEN )", i, i + 1))
                            rv = self.uf.makeStr(u")"); i += 1
                            rv = rel.rules.makeStructPatt(env.get(u"Ns"), env.get(u"Func"), env.get(u"Ps"))
                        except ParseError as pe:
                            i = st.pop()
                            st.append(i)
                            try:
                                i, rv = self.parseID(i)
                                env.set(u"Ns", rv)
                                while i < len(self.s) and ord(self.s[i]) in [9, 10, 13, 32]: i += 1
                                if i >= len(self.s): raise ParseError("no reason")
                                assert i >= 0
                                if self.s[i:i + 1] != u".": raise ParseError("no reason")
                                self.lastMatch.append((u"TOKEN .", i, i + 1))
                                rv = self.uf.makeStr(u"."); i += 1
                                i, rv = self.parseID(i)
                                env.set(u"Func", rv)
                                rv = rel.rules.makeStructPatt(env.get(u"Ns"), env.get(u"Func"), uf.makeList([]))
                            except ParseError as pe:
                                i = st.pop()
                                i, rv = self.parseWS(i)
                                i, rv = self.parsePVar(i)
                                env.set(u"Name", rv)
                                st.append(i)
                                try:
                                    i, rv = self.parseDOTS(i)
                                    rv = True
                                except ParseError as pe:
                                    rv = False
                                i = st.pop()
                                if rv: raise ParseError("no reason")
                                rv = rel.rules.makeVarPatt(env.get(u"Name"))
        return i, rv
    @cached
    def parseCPRODS(self, i):
        rel = self; uf = self.uf; st = []; env = CapEnv(uf)
        i, rv = self.parseCPROD1(i)
        env.set(u"P", rv)
        rvs = []
        while True:
            st.append(i)
            try:
                while i < len(self.s) and ord(self.s[i]) in [9, 10, 13, 32]: i += 1
                if i >= len(self.s): raise ParseError("no reason")
                assert i >= 0
                if self.s[i:i + 1] != u",": raise ParseError("no reason")
                self.lastMatch.append((u"TOKEN ,", i, i + 1))
                rv = self.uf.makeStr(u","); i += 1
                i, rv = self.parseCPROD1(i)
                rvs.append(rv)
            except ParseError as pe:
                i = st.pop()
                break
        rv = rel.uf.makeList(rvs)
        env.set(u"Ps", rv)
        rv = rel.builtin.makeFlatten(uf.makeList([uf.makeList([env.get(u"P")]), env.get(u"Ps")]))
        return i, rv
    @cached
    def parseCPROD1(self, i):
        rel = self; uf = self.uf; st = []; env = CapEnv(uf)
        st.append(i)
        try:
            i, rv = self.parseWS(i)
            i, rv = self.parseCPROD2(i)
            env.set(u"P", rv)
            rvs = []
            while True:
                st.append(i)
                try:
                    i, rv = self.parseWS(i)
                    i, rv = self.parseCPROD2(i)
                    rvs.append(rv)
                except ParseError as pe:
                    i = st.pop()
                    break
            rv = rel.uf.makeList(rvs)
            if not len(rvs): raise ParseError("no reason")
            env.set(u"Ps", rv)
            rv = rel.rules.makeConcatOp(env.get(u"P"), env.get(u"Ps"))
        except ParseError as pe:
            i = st.pop()
            i, rv = self.parseWS(i)
            i, rv = self.parseCPROD2(i)
        return i, rv
    @cached
    def parseQITEM(self, i):
        rel = self; uf = self.uf; st = []; env = CapEnv(uf)
        st.append(i)
        try:
            i, rv = self.parseWS(i)
            i, rv = self.parsePVar(i)
            env.set(u"N", rv)
            i, rv = self.parseDOTS(i)
            rv = rel.rules.makeSpliceProd(env.get(u"N"))
        except ParseError as pe:
            i = st.pop()
            i, rv = self.parseCPROD1(i)
            env.set(u"P", rv)
            rv = rel.rules.makeItemProd(env.get(u"P"))
        return i, rv
    @cached
    def parseQITEMS(self, i):
        rel = self; uf = self.uf; st = []; env = CapEnv(uf)
        i, rv = self.parseQITEM(i)
        env.set(u"P", rv)
        rvs = []
        while True:
            st.append(i)
            try:
                while i < len(self.s) and ord(self.s[i]) in [9, 10, 13, 32]: i += 1
                if i >= len(self.s): raise ParseError("no reason")
                assert i >= 0
                if self.s[i:i + 1] != u",": raise ParseError("no reason")
                self.lastMatch.append((u"TOKEN ,", i, i + 1))
                rv = self.uf.makeStr(u","); i += 1
                i, rv = self.parseQITEM(i)
                rvs.append(rv)
            except ParseError as pe:
                i = st.pop()
                break
        rv = rel.uf.makeList(rvs)
        env.set(u"Ps", rv)
        rv = rel.builtin.makeFlatten(uf.makeList([uf.makeList([env.get(u"P")]), env.get(u"Ps")]))
        return i, rv
    @cached
    def parseCPROD2(self, i):
        rel = self; uf = self.uf; st = []; env = CapEnv(uf)
        st.append(i)
        try:
            i, rv = self.parseSTRING(i)
            env.set(u"S", rv)
            rv = rel.rules.makeStrProd(env.get(u"S"))
        except ParseError as pe:
            i = st.pop()
            st.append(i)
            try:
                i, rv = self.parseNumber(i)
                env.set(u"N", rv)
                rv = rel.rules.makeCharProd(env.get(u"N"))
            except ParseError as pe:
                i = st.pop()
                st.append(i)
                try:
                    while i < len(self.s) and ord(self.s[i]) in [9, 10, 13, 32]: i += 1
                    if i >= len(self.s): raise ParseError("no reason")
                    assert i >= 0
                    if self.s[i:i + 8] != u".gensym(": raise ParseError("no reason")
                    self.lastMatch.append((u"TOKEN .gensym(", i, i + 8))
                    rv = self.uf.makeStr(u".gensym("); i += 8
                    i, rv = self.parseSTRING(i)
                    env.set(u"S", rv)
                    while i < len(self.s) and ord(self.s[i]) in [9, 10, 13, 32]: i += 1
                    if i >= len(self.s): raise ParseError("no reason")
                    assert i >= 0
                    if self.s[i:i + 1] != u")": raise ParseError("no reason")
                    self.lastMatch.append((u"TOKEN )", i, i + 1))
                    rv = self.uf.makeStr(u")"); i += 1
                    rv = rel.rules.makeGenSym(env.get(u"S"))
                except ParseError as pe:
                    i = st.pop()
                    st.append(i)
                    try:
                        while i < len(self.s) and ord(self.s[i]) in [9, 10, 13, 32]: i += 1
                        if i >= len(self.s): raise ParseError("no reason")
                        assert i >= 0
                        if self.s[i:i + 7] != u".gensym": raise ParseError("no reason")
                        self.lastMatch.append((u"TOKEN .gensym", i, i + 7))
                        rv = self.uf.makeStr(u".gensym"); i += 7
                        rv = rel.rules.makeGenSym(uf.makeStr(u'gs'))
                    except ParseError as pe:
                        i = st.pop()
                        st.append(i)
                        try:
                            while i < len(self.s) and ord(self.s[i]) in [9, 10, 13, 32]: i += 1
                            if i >= len(self.s): raise ParseError("no reason")
                            assert i >= 0
                            if self.s[i:i + 1] != u"#": raise ParseError("no reason")
                            self.lastMatch.append((u"TOKEN #", i, i + 1))
                            rv = self.uf.makeStr(u"#"); i += 1
                            i, rv = self.parseCPROD2(i)
                            env.set(u"P", rv)
                            rv = rel.rules.makeLengthOp(env.get(u"P"))
                        except ParseError as pe:
                            i = st.pop()
                            st.append(i)
                            try:
                                while i < len(self.s) and ord(self.s[i]) in [9, 10, 13, 32]: i += 1
                                if i >= len(self.s): raise ParseError("no reason")
                                assert i >= 0
                                if self.s[i:i + 1] != u"*": raise ParseError("no reason")
                                self.lastMatch.append((u"TOKEN *", i, i + 1))
                                rv = self.uf.makeStr(u"*"); i += 1
                                i, rv = self.parseCPROD2(i)
                                env.set(u"P", rv)
                                rv = rel.rules.makeFlattenOp(env.get(u"P"))
                            except ParseError as pe:
                                i = st.pop()
                                st.append(i)
                                try:
                                    while i < len(self.s) and ord(self.s[i]) in [9, 10, 13, 32]: i += 1
                                    if i >= len(self.s): raise ParseError("no reason")
                                    assert i >= 0
                                    if self.s[i:i + 2] != u"*(": raise ParseError("no reason")
                                    self.lastMatch.append((u"TOKEN *(", i, i + 2))
                                    rv = self.uf.makeStr(u"*("); i += 2
                                    i, rv = self.parseCPROD1(i)
                                    env.set(u"P", rv)
                                    while i < len(self.s) and ord(self.s[i]) in [9, 10, 13, 32]: i += 1
                                    if i >= len(self.s): raise ParseError("no reason")
                                    assert i >= 0
                                    if self.s[i:i + 1] != u")": raise ParseError("no reason")
                                    self.lastMatch.append((u"TOKEN )", i, i + 1))
                                    rv = self.uf.makeStr(u")"); i += 1
                                    rv = rel.rules.makeFlattenOp(env.get(u"P"))
                                except ParseError as pe:
                                    i = st.pop()
                                    st.append(i)
                                    try:
                                        while i < len(self.s) and ord(self.s[i]) in [9, 10, 13, 32]: i += 1
                                        if i >= len(self.s): raise ParseError("no reason")
                                        assert i >= 0
                                        if self.s[i:i + 6] != u".line(": raise ParseError("no reason")
                                        self.lastMatch.append((u"TOKEN .line(", i, i + 6))
                                        rv = self.uf.makeStr(u".line("); i += 6
                                        i, rv = self.parseCPROD1(i)
                                        env.set(u"P", rv)
                                        while i < len(self.s) and ord(self.s[i]) in [9, 10, 13, 32]: i += 1
                                        if i >= len(self.s): raise ParseError("no reason")
                                        assert i >= 0
                                        if self.s[i:i + 1] != u")": raise ParseError("no reason")
                                        self.lastMatch.append((u"TOKEN )", i, i + 1))
                                        rv = self.uf.makeStr(u")"); i += 1
                                        rv = rel.rules.makeStructProd(uf.makeStr(u'builtin'), uf.makeStr(u'Line'), uf.makeList([env.get(u"P")]))
                                    except ParseError as pe:
                                        i = st.pop()
                                        st.append(i)
                                        try:
                                            while i < len(self.s) and ord(self.s[i]) in [9, 10, 13, 32]: i += 1
                                            if i >= len(self.s): raise ParseError("no reason")
                                            assert i >= 0
                                            if self.s[i:i + 7] != u".block(": raise ParseError("no reason")
                                            self.lastMatch.append((u"TOKEN .block(", i, i + 7))
                                            rv = self.uf.makeStr(u".block("); i += 7
                                            i, rv = self.parseCPROD1(i)
                                            env.set(u"P", rv)
                                            while i < len(self.s) and ord(self.s[i]) in [9, 10, 13, 32]: i += 1
                                            if i >= len(self.s): raise ParseError("no reason")
                                            assert i >= 0
                                            if self.s[i:i + 1] != u")": raise ParseError("no reason")
                                            self.lastMatch.append((u"TOKEN )", i, i + 1))
                                            rv = self.uf.makeStr(u")"); i += 1
                                            rv = rel.rules.makeStructProd(uf.makeStr(u'builtin'), uf.makeStr(u'Block'), uf.makeList([env.get(u"P")]))
                                        except ParseError as pe:
                                            i = st.pop()
                                            st.append(i)
                                            try:
                                                while i < len(self.s) and ord(self.s[i]) in [9, 10, 13, 32]: i += 1
                                                if i >= len(self.s): raise ParseError("no reason")
                                                assert i >= 0
                                                if self.s[i:i + 6] != u".join(": raise ParseError("no reason")
                                                self.lastMatch.append((u"TOKEN .join(", i, i + 6))
                                                rv = self.uf.makeStr(u".join("); i += 6
                                                i, rv = self.parseCPROD1(i)
                                                env.set(u"P", rv)
                                                while i < len(self.s) and ord(self.s[i]) in [9, 10, 13, 32]: i += 1
                                                if i >= len(self.s): raise ParseError("no reason")
                                                assert i >= 0
                                                if self.s[i:i + 1] != u",": raise ParseError("no reason")
                                                self.lastMatch.append((u"TOKEN ,", i, i + 1))
                                                rv = self.uf.makeStr(u","); i += 1
                                                i, rv = self.parseSTRING(i)
                                                env.set(u"S", rv)
                                                while i < len(self.s) and ord(self.s[i]) in [9, 10, 13, 32]: i += 1
                                                if i >= len(self.s): raise ParseError("no reason")
                                                assert i >= 0
                                                if self.s[i:i + 1] != u")": raise ParseError("no reason")
                                                self.lastMatch.append((u"TOKEN )", i, i + 1))
                                                rv = self.uf.makeStr(u")"); i += 1
                                                rv = rel.rules.makeJoinOp(env.get(u"S"), env.get(u"P"))
                                            except ParseError as pe:
                                                i = st.pop()
                                                st.append(i)
                                                try:
                                                    while i < len(self.s) and ord(self.s[i]) in [9, 10, 13, 32]: i += 1
                                                    if i >= len(self.s): raise ParseError("no reason")
                                                    assert i >= 0
                                                    if self.s[i:i + 2] != u"[]": raise ParseError("no reason")
                                                    self.lastMatch.append((u"TOKEN []", i, i + 2))
                                                    rv = self.uf.makeStr(u"[]"); i += 2
                                                    rv = rel.rules.makeListProd(uf.makeList([]))
                                                except ParseError as pe:
                                                    i = st.pop()
                                                    st.append(i)
                                                    try:
                                                        while i < len(self.s) and ord(self.s[i]) in [9, 10, 13, 32]: i += 1
                                                        if i >= len(self.s): raise ParseError("no reason")
                                                        assert i >= 0
                                                        if self.s[i:i + 1] != u"[": raise ParseError("no reason")
                                                        self.lastMatch.append((u"TOKEN [", i, i + 1))
                                                        rv = self.uf.makeStr(u"["); i += 1
                                                        i, rv = self.parseCPRODS(i)
                                                        env.set(u"Ps", rv)
                                                        while i < len(self.s) and ord(self.s[i]) in [9, 10, 13, 32]: i += 1
                                                        if i >= len(self.s): raise ParseError("no reason")
                                                        assert i >= 0
                                                        if self.s[i:i + 1] != u"]": raise ParseError("no reason")
                                                        self.lastMatch.append((u"TOKEN ]", i, i + 1))
                                                        rv = self.uf.makeStr(u"]"); i += 1
                                                        rv = rel.rules.makeListProd(env.get(u"Ps"))
                                                    except ParseError as pe:
                                                        i = st.pop()
                                                        st.append(i)
                                                        try:
                                                            while i < len(self.s) and ord(self.s[i]) in [9, 10, 13, 32]: i += 1
                                                            if i >= len(self.s): raise ParseError("no reason")
                                                            assert i >= 0
                                                            if self.s[i:i + 1] != u"[": raise ParseError("no reason")
                                                            self.lastMatch.append((u"TOKEN [", i, i + 1))
                                                            rv = self.uf.makeStr(u"["); i += 1
                                                            i, rv = self.parseQITEMS(i)
                                                            env.set(u"Is", rv)
                                                            while i < len(self.s) and ord(self.s[i]) in [9, 10, 13, 32]: i += 1
                                                            if i >= len(self.s): raise ParseError("no reason")
                                                            assert i >= 0
                                                            if self.s[i:i + 1] != u"]": raise ParseError("no reason")
                                                            self.lastMatch.append((u"TOKEN ]", i, i + 1))
                                                            rv = self.uf.makeStr(u"]"); i += 1
                                                            rv = rel.rules.makeSpliceListProd(env.get(u"Is"))
                                                        except ParseError as pe:
                                                            i = st.pop()
                                                            st.append(i)
                                                            try:
                                                                i, rv = self.parsePVar(i)
                                                                env.set(u"Name", rv)
                                                                st.append(i)
                                                                try:
                                                                    i, rv = self.parseDOTS(i)
                                                                    rv = True
                                                                except ParseError as pe:
                                                                    rv = False
                                                                i = st.pop()
                                                                if rv: raise ParseError("no reason")
                                                                rv = rel.rules.makeVarProd(env.get(u"Name"))
                                                            except ParseError as pe:
                                                                i = st.pop()
                                                                st.append(i)
                                                                try:
                                                                    i, rv = self.parsePConst(i)
                                                                    env.set(u"Name", rv)
                                                                    st.append(i)
                                                                    try:
                                                                        i, rv = self.parseDOTS(i)
                                                                        rv = True
                                                                    except ParseError as pe:
                                                                        rv = False
                                                                    i = st.pop()
                                                                    if rv: raise ParseError("no reason")
                                                                    st.append(i)
                                                                    try:
                                                                        while i < len(self.s) and ord(self.s[i]) in [9, 10, 13, 32]: i += 1
                                                                        if i >= len(self.s): raise ParseError("no reason")
                                                                        assert i >= 0
                                                                        if self.s[i:i + 1] != u".": raise ParseError("no reason")
                                                                        self.lastMatch.append((u"TOKEN .", i, i + 1))
                                                                        rv = self.uf.makeStr(u"."); i += 1
                                                                        rv = True
                                                                    except ParseError as pe:
                                                                        rv = False
                                                                    i = st.pop()
                                                                    if rv: raise ParseError("no reason")
                                                                    rv = rel.rules.makeConstProd(env.get(u"Name"))
                                                                except ParseError as pe:
                                                                    i = st.pop()
                                                                    st.append(i)
                                                                    try:
                                                                        i, rv = self.parseID(i)
                                                                        env.set(u"Ns", rv)
                                                                        while i < len(self.s) and ord(self.s[i]) in [9, 10, 13, 32]: i += 1
                                                                        if i >= len(self.s): raise ParseError("no reason")
                                                                        assert i >= 0
                                                                        if self.s[i:i + 1] != u".": raise ParseError("no reason")
                                                                        self.lastMatch.append((u"TOKEN .", i, i + 1))
                                                                        rv = self.uf.makeStr(u"."); i += 1
                                                                        i, rv = self.parseID(i)
                                                                        env.set(u"Func", rv)
                                                                        while i < len(self.s) and ord(self.s[i]) in [9, 10, 13, 32]: i += 1
                                                                        if i >= len(self.s): raise ParseError("no reason")
                                                                        assert i >= 0
                                                                        if self.s[i:i + 1] != u"(": raise ParseError("no reason")
                                                                        self.lastMatch.append((u"TOKEN (", i, i + 1))
                                                                        rv = self.uf.makeStr(u"("); i += 1
                                                                        i, rv = self.parseCPRODS(i)
                                                                        env.set(u"Ps", rv)
                                                                        while i < len(self.s) and ord(self.s[i]) in [9, 10, 13, 32]: i += 1
                                                                        if i >= len(self.s): raise ParseError("no reason")
                                                                        assert i >= 0
                                                                        if self.s[i:i + 1] != u")": raise ParseError("no reason")
                                                                        self.lastMatch.append((u"TOKEN )", i, i + 1))
                                                                        rv = self.uf.makeStr(u")"); i += 1
                                                                        rv = rel.rules.makeStructProd(env.get(u"Ns"), env.get(u"Func"), env.get(u"Ps"))
                                                                    except ParseError as pe:
                                                                        i = st.pop()
                                                                        i, rv = self.parseID(i)
                                                                        env.set(u"Ns", rv)
                                                                        while i < len(self.s) and ord(self.s[i]) in [9, 10, 13, 32]: i += 1
                                                                        if i >= len(self.s): raise ParseError("no reason")
                                                                        assert i >= 0
                                                                        if self.s[i:i + 1] != u".": raise ParseError("no reason")
                                                                        self.lastMatch.append((u"TOKEN .", i, i + 1))
                                                                        rv = self.uf.makeStr(u"."); i += 1
                                                                        i, rv = self.parseID(i)
                                                                        env.set(u"Func", rv)
                                                                        rv = rel.rules.makeStructProd(env.get(u"Ns"), env.get(u"Func"), uf.makeList([]))
        return i, rv
    @cached
    def parseGRAMMAR(self, i):
        rel = self; uf = self.uf; st = []; env = CapEnv(uf)
        while i < len(self.s) and ord(self.s[i]) in [9, 10, 13, 32]: i += 1
        if i >= len(self.s): raise ParseError("no reason")
        assert i >= 0
        if self.s[i:i + 8] != u".grammar": raise ParseError("no reason")
        self.lastMatch.append((u"TOKEN .grammar", i, i + 8))
        rv = self.uf.makeStr(u".grammar"); i += 8
        i, rv = self.parseID(i)
        env.set(u"Name", rv)
        rvs = []
        while True:
            st.append(i)
            try:
                st.append(i)
                try:
                    i, rv = self.parsePCLASS(i)
                except ParseError as pe:
                    i = st.pop()
                    st.append(i)
                    try:
                        i, rv = self.parsePTOKEN(i)
                    except ParseError as pe:
                        i = st.pop()
                        st.append(i)
                        try:
                            i, rv = self.parsePRULE(i)
                        except ParseError as pe:
                            i = st.pop()
                            i, rv = self.parseCOMMENT(i)
                rvs.append(rv)
            except ParseError as pe:
                i = st.pop()
                break
        rv = rel.uf.makeList(rvs)
        env.set(u"Rules", rv)
        rv = uf.makeList([rel.py.makeCompound(uf.makeStr(u'def target(driver, *args)'), uf.makeList([rel.py.makeStatement(rel.builtin.makeConcat(uf.makeStr(u'driver.exe_name = "'), uf.makeList([env.get(u"Name"), uf.makeStr(u'".lower() + "c"')]))), rel.py.makeRet(uf.makeStr(u'main, None'))])), rel.py.makeCompound(rel.builtin.makeConcat(uf.makeStr(u'class '), uf.makeList([env.get(u"Name"), uf.makeStr(u'Parser(object)')])), rel.builtin.makeFlatten(uf.makeList([uf.makeList([rel.py.makeCompound(uf.makeStr(u'def __init__(self, s, uf)'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'self.s = s; self.uf = uf')), rel.py.makeStatement(uf.makeStr(u'self.lastMatch = []')), rel.py.makeStatement(uf.makeStr(u'for name in unrolledRels: setattr(self, name, frozenRels[name](uf))')), rel.py.makeStatement(uf.makeStr(u'for l in unrolledLets: l(uf, self)'))])), rel.py.makeStatement(uf.makeStr(u'gs = 0')), rel.py.makeCompound(uf.makeStr(u'def gensym(self, s)'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'self.gs += 1')), rel.py.makeRet(uf.makeStr(u's + str(self.gs).decode("utf-8")'))])), rel.py.makeCompound(uf.makeStr(u'def parse(self)'), uf.makeList([rel.py.makeRet(rel.builtin.makeConcat(uf.makeStr(u'self.parse'), uf.makeList([env.get(u"Name"), uf.makeStr(u'(0)')])))]))]), rel.builtin.makeFlatten(env.get(u"Rules"))]))), rel.py.makeStatement(rel.builtin.makeConcat(uf.makeStr(u'MainParser = '), uf.makeList([env.get(u"Name"), uf.makeStr(u'Parser')])))])
        return i, rv
    @cached
    def parsePCLASS(self, i):
        rel = self; uf = self.uf; st = []; env = CapEnv(uf)
        while i < len(self.s) and ord(self.s[i]) in [9, 10, 13, 32]: i += 1
        if i >= len(self.s): raise ParseError("no reason")
        assert i >= 0
        if self.s[i:i + 5] != u"class": raise ParseError("no reason")
        self.lastMatch.append((u"TOKEN class", i, i + 5))
        rv = self.uf.makeStr(u"class"); i += 5
        i, rv = self.parseID(i)
        env.set(u"Name", rv)
        while i < len(self.s) and ord(self.s[i]) in [9, 10, 13, 32]: i += 1
        if i >= len(self.s): raise ParseError("no reason")
        assert i >= 0
        if self.s[i:i + 1] != u"=": raise ParseError("no reason")
        self.lastMatch.append((u"TOKEN =", i, i + 1))
        rv = self.uf.makeStr(u"="); i += 1
        i, rv = self.parseCLASSEXPR1(i)
        env.set(u"C", rv)
        while i < len(self.s) and ord(self.s[i]) in [9, 10, 13, 32]: i += 1
        if i >= len(self.s): raise ParseError("no reason")
        assert i >= 0
        if self.s[i:i + 1] != u";": raise ParseError("no reason")
        self.lastMatch.append((u"TOKEN ;", i, i + 1))
        rv = self.uf.makeStr(u";"); i += 1
        rv = uf.makeList([rel.py.makeCompound(rel.builtin.makeConcat(uf.makeStr(u'def cls'), uf.makeList([env.get(u"Name"), uf.makeStr(u'(self, c)')])), uf.makeList([rel.py.makeRet(env.get(u"C"))]))])
        return i, rv
    @cached
    def parseCLASSEXPR1(self, i):
        rel = self; uf = self.uf; st = []; env = CapEnv(uf)
        st.append(i)
        try:
            i, rv = self.parseCLASSEXPR2(i)
            env.set(u"L", rv)
            while i < len(self.s) and ord(self.s[i]) in [9, 10, 13, 32]: i += 1
            if i >= len(self.s): raise ParseError("no reason")
            assert i >= 0
            if self.s[i:i + 1] != u"|": raise ParseError("no reason")
            self.lastMatch.append((u"TOKEN |", i, i + 1))
            rv = self.uf.makeStr(u"|"); i += 1
            i, rv = self.parseCLASSEXPR1(i)
            env.set(u"R", rv)
            rv = rel.char.makeEither(env.get(u"L"), env.get(u"R"))
        except ParseError as pe:
            i = st.pop()
            i, rv = self.parseCLASSEXPR2(i)
        return i, rv
    @cached
    def parseCLASSEXPR2(self, i):
        rel = self; uf = self.uf; st = []; env = CapEnv(uf)
        st.append(i)
        try:
            while i < len(self.s) and ord(self.s[i]) in [9, 10, 13, 32]: i += 1
            if i >= len(self.s): raise ParseError("no reason")
            assert i >= 0
            if self.s[i:i + 4] != u".any": raise ParseError("no reason")
            self.lastMatch.append((u"TOKEN .any", i, i + 4))
            rv = self.uf.makeStr(u".any"); i += 4
            rv = rel.char.makeAny()
        except ParseError as pe:
            i = st.pop()
            st.append(i)
            try:
                while i < len(self.s) and ord(self.s[i]) in [9, 10, 13, 32]: i += 1
                if i >= len(self.s): raise ParseError("no reason")
                assert i >= 0
                if self.s[i:i + 7] != u".range(": raise ParseError("no reason")
                self.lastMatch.append((u"TOKEN .range(", i, i + 7))
                rv = self.uf.makeStr(u".range("); i += 7
                i, rv = self.parseWS(i)
                i, rv = self.parseNumber(i)
                env.set(u"L", rv)
                while i < len(self.s) and ord(self.s[i]) in [9, 10, 13, 32]: i += 1
                if i >= len(self.s): raise ParseError("no reason")
                assert i >= 0
                if self.s[i:i + 1] != u":": raise ParseError("no reason")
                self.lastMatch.append((u"TOKEN :", i, i + 1))
                rv = self.uf.makeStr(u":"); i += 1
                i, rv = self.parseWS(i)
                i, rv = self.parseNumber(i)
                env.set(u"U", rv)
                while i < len(self.s) and ord(self.s[i]) in [9, 10, 13, 32]: i += 1
                if i >= len(self.s): raise ParseError("no reason")
                assert i >= 0
                if self.s[i:i + 1] != u")": raise ParseError("no reason")
                self.lastMatch.append((u"TOKEN )", i, i + 1))
                rv = self.uf.makeStr(u")"); i += 1
                rv = rel.char.makeRange(env.get(u"L"), env.get(u"U"))
            except ParseError as pe:
                i = st.pop()
                st.append(i)
                try:
                    while i < len(self.s) and ord(self.s[i]) in [9, 10, 13, 32]: i += 1
                    if i >= len(self.s): raise ParseError("no reason")
                    assert i >= 0
                    if self.s[i:i + 1] != u"~": raise ParseError("no reason")
                    self.lastMatch.append((u"TOKEN ~", i, i + 1))
                    rv = self.uf.makeStr(u"~"); i += 1
                    i, rv = self.parseCLASSEXPR2(i)
                    env.set(u"S", rv)
                    rv = rel.char.makeComplement(env.get(u"S"))
                except ParseError as pe:
                    i = st.pop()
                    st.append(i)
                    try:
                        while i < len(self.s) and ord(self.s[i]) in [9, 10, 13, 32]: i += 1
                        if i >= len(self.s): raise ParseError("no reason")
                        assert i >= 0
                        if self.s[i:i + 1] != u"(": raise ParseError("no reason")
                        self.lastMatch.append((u"TOKEN (", i, i + 1))
                        rv = self.uf.makeStr(u"("); i += 1
                        i, rv = self.parseCLASSEXPR1(i)
                        env.set(u"S", rv)
                        while i < len(self.s) and ord(self.s[i]) in [9, 10, 13, 32]: i += 1
                        if i >= len(self.s): raise ParseError("no reason")
                        assert i >= 0
                        if self.s[i:i + 1] != u")": raise ParseError("no reason")
                        self.lastMatch.append((u"TOKEN )", i, i + 1))
                        rv = self.uf.makeStr(u")"); i += 1
                        rv = env.get(u"S")
                    except ParseError as pe:
                        i = st.pop()
                        st.append(i)
                        try:
                            i, rv = self.parseWS(i)
                            i, rv = self.parseNumber(i)
                            env.set(u"C", rv)
                            rv = rel.char.makeExactly(env.get(u"C"))
                        except ParseError as pe:
                            i = st.pop()
                            i, rv = self.parseID(i)
                            env.set(u"N", rv)
                            rv = rel.char.makeCall(env.get(u"N"))
        return i, rv
    @cached
    def parsePTOKEN(self, i):
        rel = self; uf = self.uf; st = []; env = CapEnv(uf)
        while i < len(self.s) and ord(self.s[i]) in [9, 10, 13, 32]: i += 1
        if i >= len(self.s): raise ParseError("no reason")
        assert i >= 0
        if self.s[i:i + 5] != u"token": raise ParseError("no reason")
        self.lastMatch.append((u"TOKEN token", i, i + 5))
        rv = self.uf.makeStr(u"token"); i += 5
        i, rv = self.parseID(i)
        env.set(u"Name", rv)
        while i < len(self.s) and ord(self.s[i]) in [9, 10, 13, 32]: i += 1
        if i >= len(self.s): raise ParseError("no reason")
        assert i >= 0
        if self.s[i:i + 1] != u"=": raise ParseError("no reason")
        self.lastMatch.append((u"TOKEN =", i, i + 1))
        rv = self.uf.makeStr(u"="); i += 1
        rvs = []
        while True:
            st.append(i)
            try:
                i, rv = self.parsePSCAN(i)
                rvs.append(rv)
            except ParseError as pe:
                i = st.pop()
                break
        rv = rel.uf.makeList(rvs)
        if not len(rvs): raise ParseError("no reason")
        env.set(u"Scans", rv)
        while i < len(self.s) and ord(self.s[i]) in [9, 10, 13, 32]: i += 1
        if i >= len(self.s): raise ParseError("no reason")
        assert i >= 0
        if self.s[i:i + 1] != u";": raise ParseError("no reason")
        self.lastMatch.append((u"TOKEN ;", i, i + 1))
        rv = self.uf.makeStr(u";"); i += 1
        rv = uf.makeList([rel.py.makeCompound(rel.builtin.makeConcat(uf.makeStr(u'def parse'), uf.makeList([env.get(u"Name"), uf.makeStr(u'(self, i)')])), rel.builtin.makeFlatten(uf.makeList([uf.makeList([rel.constboundcheck, rel.py.makeStatement(uf.makeStr(u'start = i; assert start >= 0'))]), rel.builtin.makeFlatten(env.get(u"Scans")), uf.makeList([rel.py.makeStatement(uf.makeStr(u'rv = self.uf.makeStr(self.s[start:i])')), rel.py.makeStatement(rel.builtin.makeConcat(uf.makeStr(u'self.lastMatch.append((u"TOKEN '), uf.makeList([env.get(u"Name"), uf.makeStr(u'", start, i))')]))), rel.py.makeRet(uf.makeStr(u'i, rv'))])])))])
        return i, rv
    @cached
    def parsePSCAN(self, i):
        rel = self; uf = self.uf; st = []; env = CapEnv(uf)
        st.append(i)
        try:
            i, rv = self.parseID(i)
            env.set(u"Cls", rv)
            while i < len(self.s) and ord(self.s[i]) in [9, 10, 13, 32]: i += 1
            if i >= len(self.s): raise ParseError("no reason")
            assert i >= 0
            if self.s[i:i + 1] != u"*": raise ParseError("no reason")
            self.lastMatch.append((u"TOKEN *", i, i + 1))
            rv = self.uf.makeStr(u"*"); i += 1
            rv = uf.makeList([rel.py.makeStatement(rel.builtin.makeConcat(uf.makeStr(u'while i < len(self.s) and self.cls'), uf.makeList([env.get(u"Cls"), uf.makeStr(u'(ord(self.s[i])): i += 1')])))])
        except ParseError as pe:
            i = st.pop()
            st.append(i)
            try:
                i, rv = self.parseID(i)
                env.set(u"Cls", rv)
                while i < len(self.s) and ord(self.s[i]) in [9, 10, 13, 32]: i += 1
                if i >= len(self.s): raise ParseError("no reason")
                assert i >= 0
                if self.s[i:i + 1] != u"+": raise ParseError("no reason")
                self.lastMatch.append((u"TOKEN +", i, i + 1))
                rv = self.uf.makeStr(u"+"); i += 1
                rv = uf.makeList([rel.py.makeRaiseIf(rel.builtin.makeConcat(uf.makeStr(u'i >= len(self.s) or not self.cls'), uf.makeList([env.get(u"Cls"), uf.makeStr(u'(ord(self.s[i]))')]))), rel.py.makeStatement(rel.builtin.makeConcat(uf.makeStr(u'while i < len(self.s) and self.cls'), uf.makeList([env.get(u"Cls"), uf.makeStr(u'(ord(self.s[i])): i += 1')])))])
            except ParseError as pe:
                i = st.pop()
                i, rv = self.parseID(i)
                env.set(u"Cls", rv)
                rv = uf.makeList([rel.py.makeRaiseIf(rel.builtin.makeConcat(uf.makeStr(u'i >= len(self.s) or not self.cls'), uf.makeList([env.get(u"Cls"), uf.makeStr(u'(ord(self.s[i]))')]))), rel.py.makeStatement(uf.makeStr(u'i += 1'))])
        return i, rv
    @cached
    def parsePRULE(self, i):
        rel = self; uf = self.uf; st = []; env = CapEnv(uf)
        i, rv = self.parseID(i)
        env.set(u"Name", rv)
        while i < len(self.s) and ord(self.s[i]) in [9, 10, 13, 32]: i += 1
        if i >= len(self.s): raise ParseError("no reason")
        assert i >= 0
        if self.s[i:i + 2] != u":=": raise ParseError("no reason")
        self.lastMatch.append((u"TOKEN :=", i, i + 2))
        rv = self.uf.makeStr(u":="); i += 2
        i, rv = self.parsePEXPR1(i)
        env.set(u"Expr", rv)
        while i < len(self.s) and ord(self.s[i]) in [9, 10, 13, 32]: i += 1
        if i >= len(self.s): raise ParseError("no reason")
        assert i >= 0
        if self.s[i:i + 1] != u";": raise ParseError("no reason")
        self.lastMatch.append((u"TOKEN ;", i, i + 1))
        rv = self.uf.makeStr(u";"); i += 1
        rv = uf.makeList([rel.py.makeStatement(uf.makeStr(u'@cached')), rel.py.makeCompound(rel.builtin.makeConcat(uf.makeStr(u'def parse'), uf.makeList([env.get(u"Name"), uf.makeStr(u'(self, i)')])), rel.builtin.makeFlatten(uf.makeList([uf.makeList([rel.py.makeStatement(uf.makeStr(u'rel = self; uf = self.uf; st = []; env = CapEnv(uf)'))]), env.get(u"Expr"), uf.makeList([rel.py.makeRet(uf.makeStr(u'i, rv'))])])))])
        return i, rv
    @cached
    def parsePEXPR1(self, i):
        rel = self; uf = self.uf; st = []; env = CapEnv(uf)
        st.append(i)
        try:
            i, rv = self.parsePEXPR2(i)
            env.set(u"This", rv)
            while i < len(self.s) and ord(self.s[i]) in [9, 10, 13, 32]: i += 1
            if i >= len(self.s): raise ParseError("no reason")
            assert i >= 0
            if self.s[i:i + 1] != u"/": raise ParseError("no reason")
            self.lastMatch.append((u"TOKEN /", i, i + 1))
            rv = self.uf.makeStr(u"/"); i += 1
            i, rv = self.parsePEXPR1(i)
            env.set(u"That", rv)
            rv = rel.peg.makeChoice(env.get(u"This"), env.get(u"That"))
        except ParseError as pe:
            i = st.pop()
            i, rv = self.parsePEXPR2(i)
        return i, rv
    @cached
    def parsePEXPR2(self, i):
        rel = self; uf = self.uf; st = []; env = CapEnv(uf)
        st.append(i)
        try:
            i, rv = self.parsePEXPR3(i)
            env.set(u"Exprs", rv)
            while i < len(self.s) and ord(self.s[i]) in [9, 10, 13, 32]: i += 1
            if i >= len(self.s): raise ParseError("no reason")
            assert i >= 0
            if self.s[i:i + 2] != u"->": raise ParseError("no reason")
            self.lastMatch.append((u"TOKEN ->", i, i + 2))
            rv = self.uf.makeStr(u"->"); i += 2
            i, rv = self.parseCPROD1(i)
            env.set(u"Prod", rv)
            rv = rel.peg.makeProduction(env.get(u"Exprs"), env.get(u"Prod"))
        except ParseError as pe:
            i = st.pop()
            i, rv = self.parsePEXPR3(i)
        return i, rv
    @cached
    def parsePEXPR3(self, i):
        rel = self; uf = self.uf; st = []; env = CapEnv(uf)
        rvs = []
        while True:
            st.append(i)
            try:
                i, rv = self.parsePEXPR4(i)
                rvs.append(rv)
            except ParseError as pe:
                i = st.pop()
                break
        rv = rel.uf.makeList(rvs)
        env.set(u"Exprs", rv)
        rv = rel.peg.makeSequence(env.get(u"Exprs"))
        return i, rv
    @cached
    def parsePEXPR4(self, i):
        rel = self; uf = self.uf; st = []; env = CapEnv(uf)
        st.append(i)
        try:
            i, rv = self.parsePEXPR5(i)
            env.set(u"Expr", rv)
            while i < len(self.s) and ord(self.s[i]) in [9, 10, 13, 32]: i += 1
            if i >= len(self.s): raise ParseError("no reason")
            assert i >= 0
            if self.s[i:i + 1] != u":": raise ParseError("no reason")
            self.lastMatch.append((u"TOKEN :", i, i + 1))
            rv = self.uf.makeStr(u":"); i += 1
            i, rv = self.parsePPATT(i)
            env.set(u"P", rv)
            rv = rel.peg.makeCapture(env.get(u"Expr"), env.get(u"P"))
        except ParseError as pe:
            i = st.pop()
            i, rv = self.parsePEXPR5(i)
        return i, rv
    @cached
    def parsePPATT(self, i):
        rel = self; uf = self.uf; st = []; env = CapEnv(uf)
        i, rv = self.parseID(i)
        env.set(u"Name", rv)
        rv = rel.peg.makeNamePatt(env.get(u"Name"))
        return i, rv
    @cached
    def parsePEXPR5(self, i):
        rel = self; uf = self.uf; st = []; env = CapEnv(uf)
        st.append(i)
        try:
            while i < len(self.s) and ord(self.s[i]) in [9, 10, 13, 32]: i += 1
            if i >= len(self.s): raise ParseError("no reason")
            assert i >= 0
            if self.s[i:i + 1] != u"&": raise ParseError("no reason")
            self.lastMatch.append((u"TOKEN &", i, i + 1))
            rv = self.uf.makeStr(u"&"); i += 1
            i, rv = self.parsePEXPR6(i)
            env.set(u"Expr", rv)
            rv = rel.peg.makePositive(env.get(u"Expr"))
        except ParseError as pe:
            i = st.pop()
            st.append(i)
            try:
                while i < len(self.s) and ord(self.s[i]) in [9, 10, 13, 32]: i += 1
                if i >= len(self.s): raise ParseError("no reason")
                assert i >= 0
                if self.s[i:i + 1] != u"!": raise ParseError("no reason")
                self.lastMatch.append((u"TOKEN !", i, i + 1))
                rv = self.uf.makeStr(u"!"); i += 1
                i, rv = self.parsePEXPR6(i)
                env.set(u"Expr", rv)
                rv = rel.peg.makeNegative(env.get(u"Expr"))
            except ParseError as pe:
                i = st.pop()
                i, rv = self.parsePEXPR6(i)
        return i, rv
    @cached
    def parsePEXPR6(self, i):
        rel = self; uf = self.uf; st = []; env = CapEnv(uf)
        st.append(i)
        try:
            i, rv = self.parsePEXPR7(i)
            env.set(u"Expr", rv)
            while i < len(self.s) and ord(self.s[i]) in [9, 10, 13, 32]: i += 1
            if i >= len(self.s): raise ParseError("no reason")
            assert i >= 0
            if self.s[i:i + 1] != u"*": raise ParseError("no reason")
            self.lastMatch.append((u"TOKEN *", i, i + 1))
            rv = self.uf.makeStr(u"*"); i += 1
            rv = rel.peg.makeAny(env.get(u"Expr"))
        except ParseError as pe:
            i = st.pop()
            st.append(i)
            try:
                i, rv = self.parsePEXPR7(i)
                env.set(u"Expr", rv)
                while i < len(self.s) and ord(self.s[i]) in [9, 10, 13, 32]: i += 1
                if i >= len(self.s): raise ParseError("no reason")
                assert i >= 0
                if self.s[i:i + 1] != u"+": raise ParseError("no reason")
                self.lastMatch.append((u"TOKEN +", i, i + 1))
                rv = self.uf.makeStr(u"+"); i += 1
                rv = rel.peg.makeSome(env.get(u"Expr"))
            except ParseError as pe:
                i = st.pop()
                st.append(i)
                try:
                    i, rv = self.parsePEXPR7(i)
                    env.set(u"Expr", rv)
                    while i < len(self.s) and ord(self.s[i]) in [9, 10, 13, 32]: i += 1
                    if i >= len(self.s): raise ParseError("no reason")
                    assert i >= 0
                    if self.s[i:i + 1] != u"?": raise ParseError("no reason")
                    self.lastMatch.append((u"TOKEN ?", i, i + 1))
                    rv = self.uf.makeStr(u"?"); i += 1
                    rv = rel.peg.makeMaybe(env.get(u"Expr"))
                except ParseError as pe:
                    i = st.pop()
                    i, rv = self.parsePEXPR7(i)
        return i, rv
    @cached
    def parsePEXPR7(self, i):
        rel = self; uf = self.uf; st = []; env = CapEnv(uf)
        st.append(i)
        try:
            while i < len(self.s) and ord(self.s[i]) in [9, 10, 13, 32]: i += 1
            if i >= len(self.s): raise ParseError("no reason")
            assert i >= 0
            if self.s[i:i + 1] != u"(": raise ParseError("no reason")
            self.lastMatch.append((u"TOKEN (", i, i + 1))
            rv = self.uf.makeStr(u"("); i += 1
            i, rv = self.parsePEXPR1(i)
            env.set(u"Expr", rv)
            while i < len(self.s) and ord(self.s[i]) in [9, 10, 13, 32]: i += 1
            if i >= len(self.s): raise ParseError("no reason")
            assert i >= 0
            if self.s[i:i + 1] != u")": raise ParseError("no reason")
            self.lastMatch.append((u"TOKEN )", i, i + 1))
            rv = self.uf.makeStr(u")"); i += 1
            rv = env.get(u"Expr")
        except ParseError as pe:
            i = st.pop()
            st.append(i)
            try:
                i, rv = self.parseSTRING(i)
                env.set(u"S", rv)
                rv = rel.peg.makeToken(env.get(u"S"))
            except ParseError as pe:
                i = st.pop()
                i, rv = self.parseID(i)
                env.set(u"Name", rv)
                rv = rel.peg.makeCall(env.get(u"Name"))
        return i, rv
    @cached
    def parseZADDY(self, i):
        rel = self; uf = self.uf; st = []; env = CapEnv(uf)
        i, rv = self.parseWS(i)
        rvs = []
        while True:
            st.append(i)
            try:
                st.append(i)
                try:
                    i, rv = self.parseRULES(i)
                except ParseError as pe:
                    i = st.pop()
                    st.append(i)
                    try:
                        i, rv = self.parseSIGNATURE(i)
                    except ParseError as pe:
                        i = st.pop()
                        st.append(i)
                        try:
                            i, rv = self.parseGRAMMAR(i)
                        except ParseError as pe:
                            i = st.pop()
                            i, rv = self.parseCOMMENT(i)
                rvs.append(rv)
            except ParseError as pe:
                i = st.pop()
                break
        rv = rel.uf.makeList(rvs)
        env.set(u"Clss", rv)
        i, rv = self.parseWS(i)
        rv = rel.builtin.makeFlatten(uf.makeList([uf.makeList([rel.py.makeStatement(uf.makeStr(u'from rpython.rlib.rfile import create_stdio')), rel.py.makeStatement(uf.makeStr(u'from rpython.rlib.objectmodel import specialize, r_dict, compute_hash')), rel.py.makeStatement(uf.makeStr(u'from rpython.rlib.rarithmetic import intmask')), rel.py.makeStatement(uf.makeStr(u'from rpython.rlib.unroll import unrolling_iterable')), rel.py.makeStatement(uf.makeStr(u'from rpython.rlib.listsort import make_timsort_class')), rel.py.makeCompound(uf.makeStr(u'class ParseError(Exception)'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'def __init__(self, reason): self.reason = reason'))])), rel.py.makeCompound(uf.makeStr(u'class NoResults(Exception)'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'def __init__(self, message, handle): self.message = message; self.handle = handle'))])), rel.py.makeCompound(uf.makeStr(u'class ZaddyError(Exception)'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'def __init__(self, message): self.message = message'))])), rel.py.makeCompound(uf.makeStr(u'class Result(object)'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'pass'))])), rel.py.makeCompound(uf.makeStr(u'class Failed(Result)'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'pass'))])), rel.py.makeStatement(uf.makeStr(u'failed = Failed()')), rel.py.makeCompound(uf.makeStr(u'def cached(f, cacheCount=[0])'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'attr = "t" + str(cacheCount[0]); cacheCount[0] += 1')), rel.py.makeStatement(uf.makeStr(u'name = f.__name__')), rel.py.makeStatement(uf.makeStr(u'uname = unicode(name)')), rel.py.makeCompound(uf.makeStr(u'class CacheResult(Result)'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'def __init__(self, i, rv): setattr(self, attr, (i, rv))'))])), rel.py.makeStatement(uf.makeStr(u'cache = {}')), rel.py.makeCompound(uf.makeStr(u'def deco(self, i)'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'key = i')), rel.py.makeStatement(uf.makeStr(u'if key in cache and cache[key] is failed: raise ParseError("no reason")')), rel.py.makeStatement(uf.makeStr(u'elif key in cache: return getattr(cache[key], attr)')), rel.py.makeStatement(uf.makeStr(u'cache[key] = failed')), rel.py.makeStatement(uf.makeStr(u'i, rv = f(self, i)')), rel.py.makeStatement(uf.makeStr(u'cache[key] = CacheResult(i, rv)')), rel.py.makeStatement(uf.makeStr(u'self.lastMatch.append((uname, key, i))')), rel.py.makeStatement(uf.makeStr(u'return i, rv'))])), rel.py.makeStatement(uf.makeStr(u'deco.__name__ = name')), rel.py.makeStatement(uf.makeStr(u'return deco'))])), rel.py.makeStatement(uf.makeStr(u'letNames = []')), rel.py.makeCompound(uf.makeStr(u'def let(f)'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'letNames.append(f)')), rel.py.makeStatement(uf.makeStr(u'return f'))])), rel.py.makeStatement(uf.makeStr(u'allRels = []')), rel.py.makeStatement(uf.makeStr(u'ruleList = []')), rel.py.makeStatement(uf.makeStr(u'FREE = 0')), rel.py.makeStatement(uf.makeStr(u'COMM = 1')), rel.py.makeStatement(uf.makeStr(u'ASSOC = 2')), rel.py.makeStatement(uf.makeStr(u'AC = 3')), rel.py.makeStatement(uf.makeStr(u'NSTR = 0')), rel.py.makeStatement(uf.makeStr(u'NLIST = 1')), rel.py.makeStatement(uf.makeStr(u'NSTRUCT = 2')), rel.py.makeStatement(uf.makeStr(u'PRODUCTCAP = 64')), rel.py.makeStatement(uf.makeStr(u'FLATCAP = 8')), rel.py.makeStatement(uf.makeStr(u'EXTCAP = 8')), rel.py.makeStatement(uf.makeStr(u'INF = 1 << 60')), rel.py.makeCompound(uf.makeStr(u'class ENode(object)'), uf.makeList([rel.py.makeCompound(uf.makeStr(u'def __init__(self, kind, ns, con, ckind, s, args)'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'self.kind = kind; self.ns = ns; self.con = con; self.ckind = ckind')), rel.py.makeStatement(uf.makeStr(u'self.s = s; self.args = args; self.cls = 0')), rel.py.makeStatement(uf.makeStr(u'self.subsumed = False; self.dead = False'))])), rel.py.makeStatement(uf.makeStr(u'def live(self): return not self.dead and not self.subsumed')), rel.py.makeStatement(uf.makeStr(u'def tag(self): return self.ns + u"." + self.con'))])), rel.py.makeCompound(uf.makeStr(u'def nodeEq(l, r)'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'if l.kind != r.kind or l.ns != r.ns or l.con != r.con or l.s != r.s: return False')), rel.py.makeStatement(uf.makeStr(u'if len(l.args) != len(r.args): return False')), rel.py.makeCompound(uf.makeStr(u'for i in range(len(l.args))'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'if l.args[i] != r.args[i]: return False'))])), rel.py.makeStatement(uf.makeStr(u'return True'))])), rel.py.makeCompound(uf.makeStr(u'def nodeHash(n)'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'h = n.kind * 1000003 + compute_hash(n.ns)')), rel.py.makeStatement(uf.makeStr(u'h = intmask(h * 1000003 + compute_hash(n.con))')), rel.py.makeStatement(uf.makeStr(u'h = intmask(h * 1000003 + compute_hash(n.s))')), rel.py.makeStatement(uf.makeStr(u'for a in n.args: h = intmask(h * 1000003 + a)')), rel.py.makeStatement(uf.makeStr(u'return h'))])), rel.py.makeCompound(uf.makeStr(u'def sortInts(xs)'), uf.makeList([rel.py.makeCompound(uf.makeStr(u'for i in range(1, len(xs))'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'x = xs[i]; j = i - 1')), rel.py.makeCompound(uf.makeStr(u'while j >= 0 and xs[j] > x'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'xs[j + 1] = xs[j]; j -= 1'))])), rel.py.makeStatement(uf.makeStr(u'xs[j + 1] = x'))]))])), rel.py.makeCompound(uf.makeStr(u'def shortlexLess(l, r)'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'if len(l) != len(r): return len(l) < len(r)')), rel.py.makeStatement(uf.makeStr(u'return l < r'))])), rel.py.makeCompound(uf.makeStr(u'def productInts(choices)'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'rv = [[]]')), rel.py.makeCompound(uf.makeStr(u'for cs in choices'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'nxt = []')), rel.py.makeCompound(uf.makeStr(u'for prefix in rv'), uf.makeList([rel.py.makeCompound(uf.makeStr(u'for c in cs'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'if len(nxt) < PRODUCTCAP: nxt.append(prefix + c)'))]))])), rel.py.makeStatement(uf.makeStr(u'rv = nxt'))])), rel.py.makeStatement(uf.makeStr(u'return rv'))])), rel.py.makeCompound(uf.makeStr(u'class UF(object)'), uf.makeList([rel.py.makeCompound(uf.makeStr(u'def __init__(self)'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'self.parent = []')), rel.py.makeStatement(uf.makeStr(u'self.members = []')), rel.py.makeStatement(uf.makeStr(u'self.nodes = []')), rel.py.makeStatement(uf.makeStr(u'self.memo = r_dict(nodeEq, nodeHash)')), rel.py.makeStatement(uf.makeStr(u'self.strs = {}')), rel.py.makeStatement(uf.makeStr(u'self.byTag = {}')), rel.py.makeStatement(uf.makeStr(u'self.lists = []')), rel.py.makeStatement(uf.makeStr(u'self.changes = 0')), rel.py.makeStatement(uf.makeStr(u'self.unions = 0'))])), rel.py.makeCompound(uf.makeStr(u'def make(self)'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'rv = len(self.parent)')), rel.py.makeStatement(uf.makeStr(u'self.parent.append(rv)')), rel.py.makeStatement(uf.makeStr(u'self.members.append([])')), rel.py.makeStatement(uf.makeStr(u'return rv'))])), rel.py.makeCompound(uf.makeStr(u'def find(self, i)'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'j = i')), rel.py.makeStatement(uf.makeStr(u'while self.parent[j] != j: j = self.parent[j]')), rel.py.makeCompound(uf.makeStr(u'while self.parent[i] != j'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'k = self.parent[i]; self.parent[i] = j; i = k'))])), rel.py.makeStatement(uf.makeStr(u'return j'))])), rel.py.makeCompound(uf.makeStr(u'def union(self, i, j)'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'i = self.find(i); j = self.find(j)')), rel.py.makeStatement(uf.makeStr(u'if i == j: return False')), rel.py.makeStatement(uf.makeStr(u'if len(self.members[i]) < len(self.members[j]): i, j = j, i')), rel.py.makeStatement(uf.makeStr(u'self.parent[j] = i')), rel.py.makeStatement(uf.makeStr(u'self.members[i].extend(self.members[j])')), rel.py.makeStatement(uf.makeStr(u'self.members[j] = []')), rel.py.makeStatement(uf.makeStr(u'self.unions += 1')), rel.py.makeStatement(uf.makeStr(u'return True'))])), rel.py.makeCompound(uf.makeStr(u'def tagged(self, ns, con)'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'k = ns + u"." + con')), rel.py.makeStatement(uf.makeStr(u'if k in self.byTag: return self.byTag[k]')), rel.py.makeStatement(uf.makeStr(u'rv = []')), rel.py.makeStatement(uf.makeStr(u'self.byTag[k] = rv')), rel.py.makeStatement(uf.makeStr(u'return rv'))])), rel.py.makeCompound(uf.makeStr(u'def canonArgs(self, ckind, args)'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'rv = [self.find(a) for a in args]')), rel.py.makeStatement(uf.makeStr(u'if ckind == COMM or ckind == AC: sortInts(rv)')), rel.py.makeStatement(uf.makeStr(u'return rv'))])), rel.py.makeCompound(uf.makeStr(u'def intern(self, n)'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'if n in self.memo: return self.find(self.memo[n].cls)')), rel.py.makeStatement(uf.makeStr(u'c = self.make()')), rel.py.makeStatement(uf.makeStr(u'n.cls = c')), rel.py.makeStatement(uf.makeStr(u'self.memo[n] = n')), rel.py.makeStatement(uf.makeStr(u'self.nodes.append(n)')), rel.py.makeStatement(uf.makeStr(u'self.members[c].append(n)')), rel.py.makeStatement(uf.makeStr(u'if n.kind == NLIST: self.lists.append(n)')), rel.py.makeStatement(uf.makeStr(u'elif n.kind == NSTRUCT: self.tagged(n.ns, n.con).append(n)')), rel.py.makeStatement(uf.makeStr(u'self.changes += 1')), rel.py.makeStatement(uf.makeStr(u'return c'))])), rel.py.makeCompound(uf.makeStr(u'def makeStr(self, s)'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'if s in self.strs: return self.find(self.strs[s].cls)')), rel.py.makeStatement(uf.makeStr(u'n = ENode(NSTR, u"", u"", FREE, s, [])')), rel.py.makeStatement(uf.makeStr(u'self.strs[s] = n')), rel.py.makeStatement(uf.makeStr(u'return self.intern(n)'))])), rel.py.makeCompound(uf.makeStr(u'def makeList(self, l)'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'return self.intern(ENode(NLIST, u"", u"", FREE, u"", [self.find(a) for a in l]))'))])), rel.py.makeCompound(uf.makeStr(u'def flatArgs(self, ns, con, args, own)'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'rv = []')), rel.py.makeCompound(uf.makeStr(u'for a in args'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'sub = self.nodeIn(a, ns, con, own)')), rel.py.makeStatement(uf.makeStr(u'if sub is None: rv.append(a)')), rel.py.makeStatement(uf.makeStr(u'else: rv.extend(sub.args)'))])), rel.py.makeStatement(uf.makeStr(u'return rv'))])), rel.py.makeCompound(uf.makeStr(u'def nodeIn(self, c, ns, con, own)'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'c = self.find(c)')), rel.py.makeStatement(uf.makeStr(u'if c == own: return None')), rel.py.makeStatement(uf.makeStr(u'best = None')), rel.py.makeCompound(uf.makeStr(u'for n in self.members[c]'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'if n.kind != NSTRUCT or not n.live() or n.ns != ns or n.con != con: continue')), rel.py.makeStatement(uf.makeStr(u'if best is not None and len(n.args) >= len(best.args): continue')), rel.py.makeStatement(uf.makeStr(u'cyclic = False')), rel.py.makeCompound(uf.makeStr(u'for a in n.args'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'a = self.find(a)')), rel.py.makeStatement(uf.makeStr(u'if a == c or a == own: cyclic = True'))])), rel.py.makeStatement(uf.makeStr(u'if not cyclic: best = n'))])), rel.py.makeStatement(uf.makeStr(u'return best'))])), rel.py.makeCompound(uf.makeStr(u'def struct(self, ns, con, ckind, args)'), uf.makeList([rel.py.makeCompound(uf.makeStr(u'if ckind == ASSOC or ckind == AC'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'flat = self.flatArgs(ns, con, args, -1)')), rel.py.makeStatement(uf.makeStr(u'if len(flat) <= FLATCAP: args = flat'))])), rel.py.makeStatement(uf.makeStr(u'return self.intern(ENode(NSTRUCT, ns, con, ckind, u"", self.canonArgs(ckind, args)))'))])), rel.py.makeCompound(uf.makeStr(u'def subsume(self, n)'), uf.makeList([rel.py.makeCompound(uf.makeStr(u'if not n.subsumed'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'n.subsumed = True')), rel.py.makeStatement(uf.makeStr(u'self.changes += 1'))]))])), rel.py.makeCompound(uf.makeStr(u'def listsOf(self, c)'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'rv = []')), rel.py.makeCompound(uf.makeStr(u'for n in self.members[self.find(c)]'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'if n.kind == NLIST and n.live(): rv.append(n.args)'))])), rel.py.makeStatement(uf.makeStr(u'return rv'))])), rel.py.makeCompound(uf.makeStr(u'def strsOf(self, c)'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'rv = []')), rel.py.makeCompound(uf.makeStr(u'for n in self.members[self.find(c)]'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'if n.kind == NSTR and n.live(): rv.append(n.s)'))])), rel.py.makeStatement(uf.makeStr(u'return rv'))])), rel.py.makeCompound(uf.makeStr(u'def findList(self, c)'), uf.makeList([rel.py.makeCompound(uf.makeStr(u'for n in self.members[self.find(c)]'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'if n.kind == NLIST and n.live(): return n.args'))])), rel.py.makeStatement(uf.makeStr(u'raise NoResults("findList", c)'))])), rel.py.makeCompound(uf.makeStr(u'def minStr(self, c)'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'rv = None')), rel.py.makeCompound(uf.makeStr(u'for n in self.members[self.find(c)]'), uf.makeList([rel.py.makeCompound(uf.makeStr(u'if n.kind == NSTR and n.live()'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'if rv is None or shortlexLess(n.s, rv): rv = n.s'))]))])), rel.py.makeStatement(uf.makeStr(u'return rv'))])), rel.py.makeCompound(uf.makeStr(u'def findStr(self, c)'), uf.makeList([rel.py.makeCompound(uf.makeStr(u'for n in self.members[self.find(c)]'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'if n.kind == NSTR and n.live(): return n.s'))])), rel.py.makeStatement(uf.makeStr(u'raise NoResults("findStr", c)'))])), rel.py.makeCompound(uf.makeStr(u'def rebuild(self)'), uf.makeList([rel.py.makeCompound(uf.makeStr(u'while True'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'changed = False')), rel.py.makeStatement(uf.makeStr(u'memo = r_dict(nodeEq, nodeHash)')), rel.py.makeCompound(uf.makeStr(u'for n in self.nodes'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'if n.dead: continue')), rel.py.makeStatement(uf.makeStr(u'n.args = self.canonArgs(n.ckind, n.args)')), rel.py.makeCompound(uf.makeStr(u'if n in memo'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'other = memo[n]')), rel.py.makeStatement(uf.makeStr(u'if self.union(other.cls, n.cls): changed = True')), rel.py.makeStatement(uf.makeStr(u'if n.subsumed: other.subsumed = True')), rel.py.makeStatement(uf.makeStr(u'n.dead = True'))])), rel.py.makeStatement(uf.makeStr(u'else: memo[n] = n'))])), rel.py.makeStatement(uf.makeStr(u'self.memo = memo')), rel.py.makeCompound(uf.makeStr(u'for n in self.nodes[:]'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'if n.dead or n.subsumed or n.kind != NSTRUCT: continue')), rel.py.makeStatement(uf.makeStr(u'if n.ckind != ASSOC and n.ckind != AC: continue')), rel.py.makeStatement(uf.makeStr(u'own = self.find(n.cls)')), rel.py.makeStatement(uf.makeStr(u'flat = self.flatArgs(n.ns, n.con, n.args, own)')), rel.py.makeStatement(uf.makeStr(u'if len(flat) == len(n.args): continue')), rel.py.makeStatement(uf.makeStr(u'probe = ENode(NSTRUCT, n.ns, n.con, n.ckind, u"", self.canonArgs(n.ckind, flat))')), rel.py.makeStatement(uf.makeStr(u'if probe in self.memo and self.union(own, self.memo[probe].cls): changed = True'))])), rel.py.makeStatement(uf.makeStr(u'if not changed: break'))])), rel.py.makeStatement(uf.makeStr(u'for c in range(len(self.members)): self.members[c] = []')), rel.py.makeStatement(uf.makeStr(u'alive = []')), rel.py.makeCompound(uf.makeStr(u'for n in self.nodes'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'if n.dead: continue')), rel.py.makeStatement(uf.makeStr(u'alive.append(n)')), rel.py.makeStatement(uf.makeStr(u'self.members[self.find(n.cls)].append(n)'))])), rel.py.makeStatement(uf.makeStr(u'self.nodes = alive')), rel.py.makeStatement(uf.makeStr(u'self.lists = [n for n in self.lists if not n.dead]')), rel.py.makeCompound(uf.makeStr(u'for k in self.byTag.keys()'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'self.byTag[k] = [n for n in self.byTag[k] if not n.dead]'))]))]))])), rel.py.makeCompound(uf.makeStr(u'class Env(object)'), uf.makeList([rel.py.makeCompound(uf.makeStr(u'def __init__(self, name, cls, elems, next)'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'self.name = name; self.cls = cls; self.elems = elems; self.next = next'))]))])), rel.py.makeCompound(uf.makeStr(u'def envLookup(env, name)'), uf.makeList([rel.py.makeCompound(uf.makeStr(u'while env is not None'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'if env.name == name: return env')), rel.py.makeStatement(uf.makeStr(u'env = env.next'))])), rel.py.makeStatement(uf.makeStr(u'return None'))])), rel.py.makeCompound(uf.makeStr(u'def sameSeq(uf, l, r)'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'if len(l) != len(r): return False')), rel.py.makeCompound(uf.makeStr(u'for i in range(len(l))'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'if uf.find(l[i]) != uf.find(r[i]): return False'))])), rel.py.makeStatement(uf.makeStr(u'return True'))])), rel.py.makeCompound(uf.makeStr(u'class Bindings(object)'), uf.makeList([rel.py.makeCompound(uf.makeStr(u'def __init__(self, uf, env, rule)'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'self.uf = uf; self.env = env; self.rule = rule'))])), rel.py.makeCompound(uf.makeStr(u'def lookup(self, name)'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'b = envLookup(self.env, name)')), rel.py.makeStatement(uf.makeStr(u'if b is None: raise ZaddyError("rule " + self.rule + ": unbound variable " + name.encode("utf-8"))')), rel.py.makeStatement(uf.makeStr(u'return b'))])), rel.py.makeCompound(uf.makeStr(u'def get(self, name)'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'b = self.lookup(name)')), rel.py.makeStatement(uf.makeStr(u'if b.cls >= 0: return b.cls')), rel.py.makeStatement(uf.makeStr(u'return self.uf.makeList(b.elems)'))])), rel.py.makeCompound(uf.makeStr(u'def elems(self, name)'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'b = self.lookup(name)')), rel.py.makeStatement(uf.makeStr(u'if b.cls >= 0: return self.uf.findList(b.cls)')), rel.py.makeStatement(uf.makeStr(u'return b.elems'))]))])), rel.py.makeCompound(uf.makeStr(u'class CapEnv(object)'), uf.makeList([rel.py.makeCompound(uf.makeStr(u'def __init__(self, uf)'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'self.uf = uf; self.d = {}'))])), rel.py.makeStatement(uf.makeStr(u'def set(self, name, rv): self.d[name] = rv')), rel.py.makeStatement(uf.makeStr(u'def get(self, name): return self.d[name]')), rel.py.makeStatement(uf.makeStr(u'def elems(self, name): return self.uf.findList(self.d[name])'))])), rel.py.makeCompound(uf.makeStr(u'class Patt(object)'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'def match(self, uf, c, env, out): raise NotImplementedError')), rel.py.makeStatement(uf.makeStr(u'def matchNode(self, uf, n, env, out): pass')), rel.py.makeStatement(uf.makeStr(u'def groupable(self, ns, con): return False')), rel.py.makeStatement(uf.makeStr(u'def isSplice(self): return False')), rel.py.makeStatement(uf.makeStr(u'def candidates(self, uf): raise ZaddyError("rule patterns must be constructors, lists, or strings")')), rel.py.makeStatement(uf.makeStr(u'def extend(self, uf, rule, n, matches): pass'))])), rel.py.makeCompound(uf.makeStr(u'class PIgnore(Patt)'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'def match(self, uf, c, env, out): out.append(env)')), rel.py.makeStatement(uf.makeStr(u'def groupable(self, ns, con): return True'))])), rel.py.makeCompound(uf.makeStr(u'class PVar(Patt)'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'def __init__(self, name): self.name = name')), rel.py.makeCompound(uf.makeStr(u'def match(self, uf, c, env, out)'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'b = envLookup(env, self.name)')), rel.py.makeStatement(uf.makeStr(u'if b is None: out.append(Env(self.name, uf.find(c), None, env))')), rel.py.makeCompound(uf.makeStr(u'elif b.cls >= 0'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'if uf.find(b.cls) == uf.find(c): out.append(env)'))])), rel.py.makeCompound(uf.makeStr(u'else'), uf.makeList([rel.py.makeCompound(uf.makeStr(u'for l in uf.listsOf(c)'), uf.makeList([rel.py.makeCompound(uf.makeStr(u'if sameSeq(uf, l, b.elems)'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'out.append(env)')), rel.py.makeStatement(uf.makeStr(u'return'))]))]))]))])), rel.py.makeStatement(uf.makeStr(u'def groupable(self, ns, con): return True'))])), rel.py.makeCompound(uf.makeStr(u'class PSplice(Patt)'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'def __init__(self, name): self.name = name')), rel.py.makeStatement(uf.makeStr(u'def isSplice(self): return True')), rel.py.makeStatement(uf.makeStr(u'def match(self, uf, c, env, out): raise ZaddyError("splice pattern outside of a list")'))])), rel.py.makeCompound(uf.makeStr(u'class PStr(Patt)'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'def __init__(self, s): self.s = s')), rel.py.makeCompound(uf.makeStr(u'def match(self, uf, c, env, out)'), uf.makeList([rel.py.makeCompound(uf.makeStr(u'if self.s in uf.strs'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'n = uf.strs[self.s]')), rel.py.makeStatement(uf.makeStr(u'if not n.subsumed and uf.find(n.cls) == uf.find(c): out.append(env)'))]))])), rel.py.makeCompound(uf.makeStr(u'def candidates(self, uf)'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'if self.s in uf.strs: return [uf.strs[self.s]]')), rel.py.makeStatement(uf.makeStr(u'return []'))])), rel.py.makeCompound(uf.makeStr(u'def matchNode(self, uf, n, env, out)'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'if n.kind == NSTR and n.s == self.s: out.append(env)'))]))])), rel.py.makeCompound(uf.makeStr(u'class PStruct(Patt)'), uf.makeList([rel.py.makeCompound(uf.makeStr(u'def __init__(self, ns, con, args)'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'self.ns = ns; self.con = con; self.args = args'))])), rel.py.makeStatement(uf.makeStr(u'def groupable(self, ns, con): return self.ns == ns and self.con == con')), rel.py.makeStatement(uf.makeStr(u'def candidates(self, uf): return uf.tagged(self.ns, self.con)[:]')), rel.py.makeCompound(uf.makeStr(u'def match(self, uf, c, env, out)'), uf.makeList([rel.py.makeCompound(uf.makeStr(u'for n in uf.members[uf.find(c)][:]'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'if n.kind == NSTRUCT and n.live(): self.matchNode(uf, n, env, out)'))]))])), rel.py.makeCompound(uf.makeStr(u'def matchNode(self, uf, n, env, out)'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'if n.ns != self.ns or n.con != self.con: return')), rel.py.makeStatement(uf.makeStr(u'k = len(self.args)')), rel.py.makeCompound(uf.makeStr(u'if n.ckind == FREE'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'if len(n.args) != k: return')), rel.py.makeStatement(uf.makeStr(u'envs = [env]')), rel.py.makeCompound(uf.makeStr(u'for i in range(k)'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'nxt = []')), rel.py.makeStatement(uf.makeStr(u'for e in envs: self.args[i].match(uf, n.args[i], e, nxt)')), rel.py.makeStatement(uf.makeStr(u'envs = nxt'))])), rel.py.makeStatement(uf.makeStr(u'out.extend(envs)'))])), rel.py.makeCompound(uf.makeStr(u'elif n.ckind == COMM'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'if len(n.args) != k: return')), rel.py.makeStatement(uf.makeStr(u'self.matchBag(uf, n, 0, n.args[:], env, out)'))])), rel.py.makeCompound(uf.makeStr(u'elif n.ckind == ASSOC'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'if len(n.args) < k: return')), rel.py.makeStatement(uf.makeStr(u'self.matchSeq(uf, n, 0, 0, env, out)'))])), rel.py.makeCompound(uf.makeStr(u'else'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'if len(n.args) < k: return')), rel.py.makeStatement(uf.makeStr(u'self.matchBag(uf, n, 0, n.args[:], env, out)'))]))])), rel.py.makeCompound(uf.makeStr(u'def extend(self, uf, rule, n, matches)'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'if n.kind != NSTRUCT or n.ns != self.ns or n.con != self.con: return')), rel.py.makeStatement(uf.makeStr(u'if n.ckind != ASSOC and n.ckind != AC: return')), rel.py.makeStatement(uf.makeStr(u'k = len(self.args); size = len(n.args)')), rel.py.makeStatement(uf.makeStr(u'if size <= k or size > EXTCAP: return')), rel.py.makeStatement(uf.makeStr(u'seen = {}')), rel.py.makeCompound(uf.makeStr(u'if n.ckind == AC'), uf.makeList([rel.py.makeCompound(uf.makeStr(u'for mask in range(1, 1 << size)'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'sub = []; rest = []')), rel.py.makeCompound(uf.makeStr(u'for b in range(size)'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'if mask & (1 << b): sub.append(n.args[b])')), rel.py.makeStatement(uf.makeStr(u'else: rest.append(n.args[b])'))])), rel.py.makeStatement(uf.makeStr(u'if len(sub) < k or len(rest) == 0: continue')), rel.py.makeStatement(uf.makeStr(u'self.extendWith(uf, rule, n, sub, rest, [], seen, matches)'))]))])), rel.py.makeCompound(uf.makeStr(u'else'), uf.makeList([rel.py.makeCompound(uf.makeStr(u'for start in range(size)'), uf.makeList([rel.py.makeCompound(uf.makeStr(u'for stop in range(start + k, size + 1)'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'if stop - start == size: continue')), rel.py.makeStatement(uf.makeStr(u'self.extendWith(uf, rule, n, n.args[start:stop], n.args[:start], n.args[stop:], seen, matches)'))]))]))]))])), rel.py.makeCompound(uf.makeStr(u'def extendWith(self, uf, rule, n, sub, before, after, seen, matches)'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'key = u",".join([str(uf.find(a)).decode("utf-8") for a in before]) + u"|" + u",".join([str(uf.find(a)).decode("utf-8") for a in sub])')), rel.py.makeStatement(uf.makeStr(u'if key in seen: return')), rel.py.makeStatement(uf.makeStr(u'seen[key] = None')), rel.py.makeStatement(uf.makeStr(u'tmp = ENode(NSTRUCT, n.ns, n.con, n.ckind, u"", uf.canonArgs(n.ckind, sub))')), rel.py.makeStatement(uf.makeStr(u'envs = []')), rel.py.makeStatement(uf.makeStr(u'self.matchNode(uf, tmp, None, envs)')), rel.py.makeStatement(uf.makeStr(u'if not envs: return')), rel.py.makeStatement(uf.makeStr(u'c = uf.struct(n.ns, n.con, n.ckind, sub)')), rel.py.makeStatement(uf.makeStr(u'node = uf.memo[tmp] if tmp in uf.memo else None')), rel.py.makeStatement(uf.makeStr(u'if node is None: return')), rel.py.makeStatement(uf.makeStr(u'link = ENode(NSTRUCT, n.ns, n.con, n.ckind, u"", uf.canonArgs(n.ckind, before + [c] + after))')), rel.py.makeStatement(uf.makeStr(u'uf.union(n.cls, uf.intern(link))')), rel.py.makeStatement(uf.makeStr(u'for env in envs: matches.append(Match(rule, node, env))'))])), rel.py.makeCompound(uf.makeStr(u'def matchGroup(self, uf, n, p, group, env, out)'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'if len(group) == 1: p.match(uf, group[0], env, out)')), rel.py.makeCompound(uf.makeStr(u'elif p.groupable(self.ns, self.con)'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'c = uf.struct(self.ns, self.con, n.ckind, group)')), rel.py.makeStatement(uf.makeStr(u'p.match(uf, c, env, out)'))]))])), rel.py.makeCompound(uf.makeStr(u'def matchSeq(self, uf, n, pi, j, env, out)'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'k = len(self.args)')), rel.py.makeStatement(uf.makeStr(u'p = self.args[pi]')), rel.py.makeCompound(uf.makeStr(u'if pi == k - 1'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'self.matchGroup(uf, n, p, n.args[j:], env, out)')), rel.py.makeStatement(uf.makeStr(u'return'))])), rel.py.makeStatement(uf.makeStr(u'last = len(n.args) - (k - 1 - pi)')), rel.py.makeStatement(uf.makeStr(u'stop = j + 1')), rel.py.makeStatement(uf.makeStr(u'if p.groupable(self.ns, self.con): stop = last')), rel.py.makeCompound(uf.makeStr(u'for e in range(j + 1, stop + 1)'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'envs = []')), rel.py.makeStatement(uf.makeStr(u'self.matchGroup(uf, n, p, n.args[j:e], env, envs)')), rel.py.makeStatement(uf.makeStr(u'for env2 in envs: self.matchSeq(uf, n, pi + 1, e, env2, out)'))]))])), rel.py.makeCompound(uf.makeStr(u'def matchBag(self, uf, n, pi, rest, env, out)'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'k = len(self.args)')), rel.py.makeStatement(uf.makeStr(u'p = self.args[pi]')), rel.py.makeCompound(uf.makeStr(u'if pi == k - 1'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'if len(rest) == 0: return')), rel.py.makeStatement(uf.makeStr(u'if len(rest) > 1 and n.ckind == COMM: return')), rel.py.makeStatement(uf.makeStr(u'self.matchGroup(uf, n, p, rest, env, out)')), rel.py.makeStatement(uf.makeStr(u'return'))])), rel.py.makeStatement(uf.makeStr(u'spare = len(rest) - (k - 1 - pi)')), rel.py.makeStatement(uf.makeStr(u'if spare < 1: return')), rel.py.makeCompound(uf.makeStr(u'if p.groupable(self.ns, self.con) and n.ckind == AC and len(rest) <= 12'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'total = 1 << len(rest)')), rel.py.makeCompound(uf.makeStr(u'for mask in range(1, total)'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'group = []; remain = []')), rel.py.makeCompound(uf.makeStr(u'for b in range(len(rest))'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'if mask & (1 << b): group.append(rest[b])')), rel.py.makeStatement(uf.makeStr(u'else: remain.append(rest[b])'))])), rel.py.makeStatement(uf.makeStr(u'if len(group) > spare: continue')), rel.py.makeStatement(uf.makeStr(u'envs = []')), rel.py.makeStatement(uf.makeStr(u'self.matchGroup(uf, n, p, group, env, envs)')), rel.py.makeStatement(uf.makeStr(u'for env2 in envs: self.matchBag(uf, n, pi + 1, remain, env2, out)'))]))])), rel.py.makeCompound(uf.makeStr(u'else'), uf.makeList([rel.py.makeCompound(uf.makeStr(u'for b in range(len(rest))'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'if b > 0 and uf.find(rest[b]) == uf.find(rest[b - 1]): continue')), rel.py.makeStatement(uf.makeStr(u'remain = rest[:b] + rest[b + 1:]')), rel.py.makeStatement(uf.makeStr(u'envs = []')), rel.py.makeStatement(uf.makeStr(u'p.match(uf, rest[b], env, envs)')), rel.py.makeStatement(uf.makeStr(u'for env2 in envs: self.matchBag(uf, n, pi + 1, remain, env2, out)'))]))]))]))])), rel.py.makeCompound(uf.makeStr(u'class PList(Patt)'), uf.makeList([rel.py.makeCompound(uf.makeStr(u'def __init__(self, items)'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'self.items = items')), rel.py.makeStatement(uf.makeStr(u'self.need = [0] * (len(items) + 1)')), rel.py.makeCompound(uf.makeStr(u'for i in range(len(items) - 1, -1, -1)'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'self.need[i] = self.need[i + 1]')), rel.py.makeStatement(uf.makeStr(u'if not items[i].isSplice(): self.need[i] += 1'))]))])), rel.py.makeStatement(uf.makeStr(u'def candidates(self, uf): return uf.lists[:]')), rel.py.makeCompound(uf.makeStr(u'def match(self, uf, c, env, out)'), uf.makeList([rel.py.makeCompound(uf.makeStr(u'for n in uf.members[uf.find(c)][:]'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'if n.kind == NLIST and n.live(): self.matchNode(uf, n, env, out)'))]))])), rel.py.makeCompound(uf.makeStr(u'def matchNode(self, uf, n, env, out)'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'if n.kind != NLIST: return')), rel.py.makeStatement(uf.makeStr(u'if len(n.args) < self.need[0]: return')), rel.py.makeStatement(uf.makeStr(u'self.seq(uf, n.args, 0, 0, env, out)'))])), rel.py.makeCompound(uf.makeStr(u'def seq(self, uf, args, i, j, env, out)'), uf.makeList([rel.py.makeCompound(uf.makeStr(u'if i == len(self.items)'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'if j == len(args): out.append(env)')), rel.py.makeStatement(uf.makeStr(u'return'))])), rel.py.makeStatement(uf.makeStr(u'it = self.items[i]')), rel.py.makeCompound(uf.makeStr(u'if isinstance(it, PSplice)'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'b = envLookup(env, it.name)')), rel.py.makeCompound(uf.makeStr(u'if b is None'), uf.makeList([rel.py.makeCompound(uf.makeStr(u'for e in range(j, len(args) - self.need[i + 1] + 1)'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'self.seq(uf, args, i + 1, e, Env(it.name, -1, args[j:e], env), out)'))]))])), rel.py.makeCompound(uf.makeStr(u'else'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'cands = [b.elems]')), rel.py.makeStatement(uf.makeStr(u'if b.cls >= 0: cands = uf.listsOf(b.cls)')), rel.py.makeCompound(uf.makeStr(u'for l in cands'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'e = j + len(l)')), rel.py.makeStatement(uf.makeStr(u'if e <= len(args) and sameSeq(uf, args[j:e], l): self.seq(uf, args, i + 1, e, env, out)'))]))]))])), rel.py.makeCompound(uf.makeStr(u'elif j < len(args)'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'envs = []')), rel.py.makeStatement(uf.makeStr(u'it.match(uf, args[j], env, envs)')), rel.py.makeStatement(uf.makeStr(u'for env2 in envs: self.seq(uf, args, i + 1, j + 1, env2, out)'))]))]))])), rel.py.makeCompound(uf.makeStr(u'class Rule(object)'), uf.makeList([rel.py.makeCompound(uf.makeStr(u'def __init__(self, name, patt, rhs, simplify)'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'self.name = name; self.patt = patt; self.rhs = rhs; self.simplify = simplify')), rel.py.makeStatement(uf.makeStr(u'self.fired = 0'))]))])), rel.py.makeCompound(uf.makeStr(u'def rewrite(name, patt, simplify)'), uf.makeList([rel.py.makeCompound(uf.makeStr(u'def deco(f)'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'ruleList.append(Rule(name, patt, f, simplify))')), rel.py.makeStatement(uf.makeStr(u'return f'))])), rel.py.makeStatement(uf.makeStr(u'return deco'))])), rel.py.makeCompound(uf.makeStr(u'class Match(object)'), uf.makeList([rel.py.makeCompound(uf.makeStr(u'def __init__(self, rule, node, env)'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'self.rule = rule; self.node = node; self.env = env'))]))])), rel.py.makeCompound(uf.makeStr(u'class builtinRels(object)'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'ns = u"builtin"')), rel.py.makeStatement(uf.makeStr(u'def __init__(self, uf): self.uf = uf')), rel.py.makeStatement(uf.makeStr(u'def makeLine(self, s): return self.uf.struct(self.ns, u"Line", FREE, [s])')), rel.py.makeStatement(uf.makeStr(u'def makeBlock(self, ls): return self.uf.struct(self.ns, u"Block", FREE, [ls])')), rel.py.makeStatement(uf.makeStr(u'def makeFlatten(self, ls): return self.uf.struct(self.ns, u"Flatten", FREE, [ls])')), rel.py.makeStatement(uf.makeStr(u'def makeLength(self, s): return self.uf.struct(self.ns, u"Length", FREE, [s])')), rel.py.makeStatement(uf.makeStr(u'def makeJoin(self, s, ps): return self.uf.struct(self.ns, u"Join", FREE, [s, ps])')), rel.py.makeStatement(uf.makeStr(u'def makeConcat(self, p, ps): return self.uf.struct(self.ns, u"Concat", FREE, [p, ps])'))])), rel.py.makeStatement(uf.makeStr(u'allRels.append(("builtin", builtinRels))')), rel.py.makeCompound(uf.makeStr(u'def evalBuiltins(uf, out)'), uf.makeList([rel.py.makeCompound(uf.makeStr(u'for n in uf.tagged(u"builtin", u"Flatten")[:]'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'if not n.live(): continue')), rel.py.makeCompound(uf.makeStr(u'for ls in uf.listsOf(n.args[0])'), uf.makeList([rel.py.makeCompound(uf.makeStr(u'if len(ls) == 1'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'out.append((n.cls, ls[0]))')), rel.py.makeStatement(uf.makeStr(u'continue'))])), rel.py.makeStatement(uf.makeStr(u'choices = [uf.listsOf(x) for x in ls]')), rel.py.makeStatement(uf.makeStr(u'for l in productInts(choices): out.append((n.cls, uf.makeList(l)))'))]))])), rel.py.makeCompound(uf.makeStr(u'for n in uf.tagged(u"builtin", u"Length")[:]'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'if not n.live(): continue')), rel.py.makeStatement(uf.makeStr(u's = uf.minStr(n.args[0])')), rel.py.makeStatement(uf.makeStr(u'if s is not None: out.append((n.cls, uf.makeStr(str(len(s)).decode("utf-8"))))'))])), rel.py.makeCompound(uf.makeStr(u'for n in uf.tagged(u"builtin", u"Join")[:]'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'if not n.live(): continue')), rel.py.makeStatement(uf.makeStr(u'sep = uf.minStr(n.args[0])')), rel.py.makeStatement(uf.makeStr(u'if sep is None: continue')), rel.py.makeCompound(uf.makeStr(u'for ls in uf.listsOf(n.args[1])'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'parts = minStrs(uf, ls)')), rel.py.makeStatement(uf.makeStr(u'if parts is not None: out.append((n.cls, uf.makeStr(sep.join(parts))))'))]))])), rel.py.makeCompound(uf.makeStr(u'for n in uf.tagged(u"builtin", u"Concat")[:]'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'if not n.live(): continue')), rel.py.makeCompound(uf.makeStr(u'for ls in uf.listsOf(n.args[1])'), uf.makeList([rel.py.makeCompound(uf.makeStr(u'if len(ls) == 0'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'out.append((n.cls, n.args[0]))')), rel.py.makeStatement(uf.makeStr(u'continue'))])), rel.py.makeStatement(uf.makeStr(u'parts = minStrs(uf, [n.args[0]] + ls)')), rel.py.makeStatement(uf.makeStr(u'if parts is not None: out.append((n.cls, uf.makeStr(u"".join(parts))))'))]))]))])), rel.py.makeCompound(uf.makeStr(u'def minStrs(uf, cs)'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'rv = []')), rel.py.makeCompound(uf.makeStr(u'for c in cs'), uf.makeList([rel.py.makeStatement(uf.makeStr(u's = uf.minStr(c)')), rel.py.makeStatement(uf.makeStr(u'if s is None: return None')), rel.py.makeStatement(uf.makeStr(u'rv.append(s)'))])), rel.py.makeStatement(uf.makeStr(u'return rv'))])), rel.py.makeCompound(uf.makeStr(u'def saturate(uf, parser, limit, nodeLimit, verbose, stderr)'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'iteration = 0')), rel.py.makeCompound(uf.makeStr(u'while iteration < limit'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'uf.rebuild()')), rel.py.makeStatement(uf.makeStr(u'before = uf.changes + uf.unions')), rel.py.makeStatement(uf.makeStr(u'matches = []')), rel.py.makeCompound(uf.makeStr(u'for rule in ruleList'), uf.makeList([rel.py.makeCompound(uf.makeStr(u'for n in rule.patt.candidates(uf)'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'if not n.live(): continue')), rel.py.makeStatement(uf.makeStr(u'envs = []')), rel.py.makeStatement(uf.makeStr(u'rule.patt.matchNode(uf, n, None, envs)')), rel.py.makeStatement(uf.makeStr(u'for env in envs: matches.append(Match(rule, n, env))')), rel.py.makeStatement(uf.makeStr(u'rule.patt.extend(uf, rule, n, matches)'))]))])), rel.py.makeStatement(uf.makeStr(u'pending = []')), rel.py.makeStatement(uf.makeStr(u'evalBuiltins(uf, pending)')), rel.py.makeCompound(uf.makeStr(u'for m in matches'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'try: rv = m.rule.rhs(uf, parser, Bindings(uf, m.env, m.rule.name))')), rel.py.makeStatement(uf.makeStr(u'except NoResults: continue')), rel.py.makeStatement(uf.makeStr(u'for c in rv: uf.union(m.node.cls, c)')), rel.py.makeStatement(uf.makeStr(u'if m.rule.simplify: uf.subsume(m.node)')), rel.py.makeStatement(uf.makeStr(u'm.rule.fired += 1'))])), rel.py.makeStatement(uf.makeStr(u'for (a, b) in pending: uf.union(a, b)')), rel.py.makeStatement(uf.makeStr(u'iteration += 1')), rel.py.makeStatement(uf.makeStr(u'if verbose: stderr.write("Iteration %d: %d matches, %d classes, %d nodes\\n" % (iteration, len(matches), len(uf.parent), len(uf.nodes)))')), rel.py.makeStatement(uf.makeStr(u'if uf.changes + uf.unions == before: break')), rel.py.makeCompound(uf.makeStr(u'if len(uf.nodes) > nodeLimit'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'stderr.write("Warning: stopped after %d iterations at the limit of %d e-nodes\\n" % (iteration, nodeLimit))')), rel.py.makeStatement(uf.makeStr(u'break'))]))])), rel.py.makeStatement(uf.makeStr(u'uf.rebuild()')), rel.py.makeStatement(uf.makeStr(u'return iteration'))])), rel.py.makeStatement(uf.makeStr(u'TSTR = 0')), rel.py.makeStatement(uf.makeStr(u'TLINE = 1')), rel.py.makeStatement(uf.makeStr(u'TLIST = 2')), rel.py.makeCompound(uf.makeStr(u'class Extractor(object)'), uf.makeList([rel.py.makeCompound(uf.makeStr(u'def __init__(self, uf)'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'self.uf = uf')), rel.py.makeStatement(uf.makeStr(u'size = len(uf.parent)')), rel.py.makeStatement(uf.makeStr(u'self.costS = [INF] * size')), rel.py.makeStatement(uf.makeStr(u'self.bestS = [u""] * size')), rel.py.makeStatement(uf.makeStr(u'self.costB = [INF] * size')), rel.py.makeStatement(uf.makeStr(u'self.bestB = [None] * size')), rel.py.makeStatement(uf.makeStr(u'self.costL = [INF] * size')), rel.py.makeStatement(uf.makeStr(u'self.bestL = [None] * size'))])), rel.py.makeCompound(uf.makeStr(u'def run(self)'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'uf = self.uf')), rel.py.makeStatement(uf.makeStr(u'changed = True')), rel.py.makeCompound(uf.makeStr(u'while changed'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'changed = False')), rel.py.makeCompound(uf.makeStr(u'for n in uf.nodes'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'if not n.live(): continue')), rel.py.makeStatement(uf.makeStr(u'c = uf.find(n.cls)')), rel.py.makeCompound(uf.makeStr(u'if n.kind == NSTR'), uf.makeList([rel.py.makeCompound(uf.makeStr(u'if self.costS[c] > 1 or (self.costS[c] == 1 and shortlexLess(n.s, self.bestS[c]))'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'self.costS[c] = 1; self.bestS[c] = n.s; changed = True'))]))])), rel.py.makeCompound(uf.makeStr(u'elif n.kind == NLIST'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'cost = 1')), rel.py.makeCompound(uf.makeStr(u'for a in n.args'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'cost += self.costB[uf.find(a)]')), rel.py.makeStatement(uf.makeStr(u'if cost >= INF: break'))])), rel.py.makeCompound(uf.makeStr(u'if cost < self.costL[c]'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'self.costL[c] = cost; self.bestL[c] = n; changed = True'))]))])), rel.py.makeCompound(uf.makeStr(u'elif n.ns == u"builtin" and n.con == u"Line"'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'cost = 1 + self.costS[uf.find(n.args[0])]')), rel.py.makeCompound(uf.makeStr(u'if cost < self.costB[c]'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'self.costB[c] = cost; self.bestB[c] = n; changed = True'))]))])), rel.py.makeCompound(uf.makeStr(u'elif n.ns == u"builtin" and n.con == u"Block"'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'cost = 1 + self.costL[uf.find(n.args[0])]')), rel.py.makeCompound(uf.makeStr(u'if cost < self.costB[c]'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'self.costB[c] = cost; self.bestB[c] = n; changed = True'))]))]))]))]))])), rel.py.makeCompound(uf.makeStr(u'def emitList(self, c, depth, buf)'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'n = self.bestL[self.uf.find(c)]')), rel.py.makeStatement(uf.makeStr(u'assert n is not None')), rel.py.makeStatement(uf.makeStr(u'for a in n.args: self.emitLine(a, depth, buf)'))])), rel.py.makeCompound(uf.makeStr(u'def emitLine(self, c, depth, buf)'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'n = self.bestB[self.uf.find(c)]')), rel.py.makeStatement(uf.makeStr(u'assert n is not None')), rel.py.makeStatement(uf.makeStr(u'if n.con == u"Line": buf.append(u" " * (depth * 4) + self.bestS[self.uf.find(n.args[0])])')), rel.py.makeStatement(uf.makeStr(u'else: self.emitList(n.args[0], depth + 1, buf)'))])), rel.py.makeCompound(uf.makeStr(u'def diagnose(self, root)'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'uf = self.uf')), rel.py.makeStatement(uf.makeStr(u'seen = {}')), rel.py.makeStatement(uf.makeStr(u'stack = [uf.find(root)]')), rel.py.makeStatement(uf.makeStr(u'stuck = []')), rel.py.makeCompound(uf.makeStr(u'while stack'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'c = stack.pop()')), rel.py.makeStatement(uf.makeStr(u'if c in seen: continue')), rel.py.makeStatement(uf.makeStr(u'seen[c] = None')), rel.py.makeStatement(uf.makeStr(u'ok = self.costS[c] < INF or self.costB[c] < INF or self.costL[c] < INF')), rel.py.makeCompound(uf.makeStr(u'for n in uf.members[c]'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'if not n.live(): continue')), rel.py.makeStatement(uf.makeStr(u'for a in n.args: stack.append(uf.find(a))')), rel.py.makeStatement(uf.makeStr(u'if ok or n.kind != NSTRUCT or n.ns == u"builtin": continue')), rel.py.makeStatement(uf.makeStr(u'stuck.append(n.tag())'))]))])), rel.py.makeStatement(uf.makeStr(u'return stuck'))]))])), rel.py.makeCompound(uf.makeStr(u'def describeStuck(stuck)'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'counts = {}')), rel.py.makeStatement(uf.makeStr(u'order = []')), rel.py.makeCompound(uf.makeStr(u'for t in stuck'), uf.makeList([rel.py.makeCompound(uf.makeStr(u'if t not in counts'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'counts[t] = 0')), rel.py.makeStatement(uf.makeStr(u'order.append(t)'))])), rel.py.makeStatement(uf.makeStr(u'counts[t] += 1'))])), rel.py.makeStatement(uf.makeStr(u'return [t + u" (" + str(counts[t]).decode("utf-8") + u")" for t in order]'))])), rel.py.makeCompound(uf.makeStr(u'def lineNumber(s, i)'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'assert i >= 0')), rel.py.makeStatement(uf.makeStr(u'return s.count(unichr(10), 0, i)'))])), rel.py.makeStatement(uf.makeStr(u'SortTraces = make_timsort_class(lt=lambda l, r: l[2] > r[2])')), rel.py.makeStatement(uf.makeStr(u'USAGE = "usage: %s [-v] [-i ITERATIONS] [-n NODES] < input > output\\n"')), rel.py.makeCompound(uf.makeStr(u'def main(argv)'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'stdin, stdout, stderr = create_stdio()')), rel.py.makeStatement(uf.makeStr(u'verbose = False')), rel.py.makeStatement(uf.makeStr(u'limit = 1000')), rel.py.makeStatement(uf.makeStr(u'nodeLimit = 1000000')), rel.py.makeStatement(uf.makeStr(u'i = 1')), rel.py.makeCompound(uf.makeStr(u'while i < len(argv)'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'if argv[i] == "-v": verbose = True')), rel.py.makeCompound(uf.makeStr(u'elif argv[i] == "-i" and i + 1 < len(argv)'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'i += 1')), rel.py.makeStatement(uf.makeStr(u'limit = int(argv[i])'))])), rel.py.makeCompound(uf.makeStr(u'elif argv[i] == "-n" and i + 1 < len(argv)'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'i += 1')), rel.py.makeStatement(uf.makeStr(u'nodeLimit = int(argv[i])'))])), rel.py.makeCompound(uf.makeStr(u'else'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'stderr.write(USAGE % argv[0])')), rel.py.makeStatement(uf.makeStr(u'return 2'))])), rel.py.makeStatement(uf.makeStr(u'i += 1'))])), rel.py.makeStatement(uf.makeStr(u'uf = UF()')), rel.py.makeStatement(uf.makeStr(u'parser = MainParser(stdin.read().decode("utf-8"), uf)')), rel.py.makeCompound(uf.makeStr(u'try'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'i, l = parser.parse()')), rel.py.makeStatement(uf.makeStr(u'if i != len(parser.s): raise ParseError("Failed to consume all input")'))])), rel.py.makeCompound(uf.makeStr(u'except ParseError as pe'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'stderr.write("Parse error: %s\\n" % pe.reason)')), rel.py.makeStatement(uf.makeStr(u'SortTraces(parser.lastMatch).sort()')), rel.py.makeStatement(uf.makeStr(u'newlines = [0]')), rel.py.makeCompound(uf.makeStr(u'for line in parser.s.split(u"\\n")'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'newlines.append(newlines[-1] + len(line) + 1)'))])), rel.py.makeCompound(uf.makeStr(u'for k, start, stop in parser.lastMatch[:10]'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'startLine = lineNumber(parser.s, start)')), rel.py.makeStatement(uf.makeStr(u'startCol = start - newlines[startLine]')), rel.py.makeStatement(uf.makeStr(u'stopLine = lineNumber(parser.s, stop)')), rel.py.makeStatement(uf.makeStr(u'stopCol = stop - newlines[stopLine]')), rel.py.makeStatement(uf.makeStr(u't = k.encode("utf-8"), startLine + 1, startCol, stopLine + 1, stopCol')), rel.py.makeStatement(uf.makeStr(u'stderr.write(("Trail: %s (%d:%d - %d:%d)" % t) + chr(10))'))])), rel.py.makeStatement(uf.makeStr(u'return 1'))])), rel.py.makeStatement(uf.makeStr(u'root = parser.builtin.makeFlatten(l)')), rel.py.makeStatement(uf.makeStr(u'try: iterations = saturate(uf, parser, limit, nodeLimit, verbose, stderr)')), rel.py.makeCompound(uf.makeStr(u'except ZaddyError as ze'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'stderr.write("Error: %s\\n" % ze.message)')), rel.py.makeStatement(uf.makeStr(u'return 3'))])), rel.py.makeCompound(uf.makeStr(u'if verbose'), uf.makeList([rel.py.makeCompound(uf.makeStr(u'for rule in ruleList'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'if rule.fired: stderr.write("Rule %s: %d applications\\n" % (rule.name, rule.fired))'))]))])), rel.py.makeStatement(uf.makeStr(u'ex = Extractor(uf)')), rel.py.makeStatement(uf.makeStr(u'ex.run()')), rel.py.makeStatement(uf.makeStr(u'root = uf.find(root)')), rel.py.makeCompound(uf.makeStr(u'if ex.costL[root] >= INF'), uf.makeList([rel.py.makeStatement(uf.makeStr(u'stderr.write("Error: no output could be extracted after %d iterations\\n" % iterations)')), rel.py.makeStatement(uf.makeStr(u'stderr.write("Constructors which were never rewritten:\\n")')), rel.py.makeStatement(uf.makeStr(u'for t in describeStuck(ex.diagnose(root))[:20]: stderr.write("  %s\\n" % t.encode("utf-8"))')), rel.py.makeStatement(uf.makeStr(u'return 3'))])), rel.py.makeStatement(uf.makeStr(u'buf = []')), rel.py.makeStatement(uf.makeStr(u'ex.emitList(root, 0, buf)')), rel.py.makeStatement(uf.makeStr(u'stdout.write(u"\\n".join(buf).encode("utf-8"))')), rel.py.makeStatement(uf.makeStr(u'stdout.write("\\n")')), rel.py.makeStatement(uf.makeStr(u'if verbose: stderr.write("Wrote %d lines after %d iterations\\n" % (len(buf), iterations))')), rel.py.makeStatement(uf.makeStr(u'return 0'))]))]), rel.builtin.makeFlatten(env.get(u"Clss")), uf.makeList([rel.py.makeStatement(uf.makeStr(u'frozenRels = dict(allRels)')), rel.py.makeStatement(uf.makeStr(u'unrolledRels = unrolling_iterable(frozenRels)')), rel.py.makeStatement(uf.makeStr(u'frozenLets = letNames[:]')), rel.py.makeStatement(uf.makeStr(u'unrolledLets = unrolling_iterable(frozenLets)'))])]))
        return i, rv
MainParser = ZADDYParser
frozenRels = dict(allRels)
unrolledRels = unrolling_iterable(frozenRels)
frozenLets = letNames[:]
unrolledLets = unrolling_iterable(frozenLets)
