# -*- coding: utf-8 -*-
"""O recuo do `atalho` impossivel, aplicado aos logs que ja existem.

A checagem e MECANICA -- nao depende do modelo, so da tabela de gatilhos. Entao da
para aplica-la em cima das decisoes ja registradas e ver o efeito sem treinar de
novo. O modelo e o mesmo dos dois lados; so a regra de aceitacao muda.
"""
import re, glob, io, math, unicodedata

def sem(s):
    s = unicodedata.normalize('NFD', s.lower())
    return ''.join(c for c in s if unicodedata.category(c) != 'Mn')

rot = {}
for l in io.open("dados/frases_teste.txt", encoding="utf-8"):
    l = l.strip()
    if not l or l.startswith("#") or "|" not in l:
        continue
    p = [x.strip() for x in l.split("|")]
    if len(p) >= 2:
        rot[p[1]] = p[0]

gat, canon = set(), set()
for l in io.open("dados/gatilhos.txt", encoding="utf-8"):
    if "|" not in l or l.strip().startswith("#"):
        continue
    n, fs = l.split("|", 1)
    canon.add(n.strip().rstrip("*"))
    for g in fs.split(","):
        gat.add(sem(g.strip()))

def pontuar(pad, recuo):
    fora = {}
    for f in glob.glob(pad):
        s = int(re.search(r"s(\d+)\.log$", f).group(1))
        if not 19 <= s <= 30:
            continue
        t = io.open(f, encoding="utf-8", errors="replace").read()
        if "ferramenta certa:" not in t:
            continue
        ok = n = 0
        vistas = set()
        for linha in t.splitlines():
            l = linha.strip()
            if l.startswith("ok "):
                resto = l[3:].lstrip()
            elif l.startswith("ERR "):
                resto = l[4:].lstrip()
            else:
                continue
            fr = None
            for x in rot:
                if resto.startswith(x) and (fr is None or len(x) > len(fr)):
                    fr = x
            if fr is None or fr in vistas:
                continue
            vistas.add(fr)
            n += 1
            m = re.search(r'\{"acao":"([a-z_]+)"([^}]*)\}', resto[len(fr):])
            obt = m.group(1) if m else None
            if recuo and obt == "atalho":
                mn = re.search(r'"nome":"([^"]*)"', m.group(2))
                cru = mn.group(1) if mn else ""
                if not (any(g in sem(cru) for g in gat) or cru in canon):
                    obt = "perguntar"
            if obt and rot[fr] == obt:
                ok += 1
        fora[s] = (ok, n)
    return fora

a = pontuar("logs/tres_s*.log", False)
b = pontuar("logs/tres_s*.log", True)
p = sorted(a)
d = [b[s][0] - a[s][0] for s in p]
m = sum(d) / len(d)
sd = math.sqrt(sum((x - m) ** 2 for x in d) / (len(d) - 1))
t = m / (sd / math.sqrt(len(d))) if sd else float("inf")
print(f"\n  MESMOS MODELOS, so a regra de aceitacao muda ({len(p)} sementes)\n")
print(f"  sem recuo   {sum(a[s][0] for s in p)/len(p):7.2f}/329")
print(f"  com recuo   {sum(b[s][0] for s in p)/len(p):7.2f}/329   delta {m:+.2f}  t={t:+.2f}  sd {sd:.2f}")
print(f"  subiu em {sum(1 for x in d if x>0)}, caiu em {sum(1 for x in d if x<0)}, de {len(d)}")
