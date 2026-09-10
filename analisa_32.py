# -*- coding: utf-8 -*-
"""As tres familias que a guarda apontou: QUE FRASES caem, e para onde.

O metodo do 3.2 e achar a FORMA que falta, nao os itens. Entao o que interessa aqui
nao e "quantos", e sim o par (frase, escolhida) repetido em muitas sementes: erro que
aparece em 11 de 12 sementes e forma que ela nao aprendeu, e erro em 2 de 12 e ruido
de treino.
"""
import re, glob, collections, io, sys

ALVO = sys.argv[1:] or ["escrever_arquivo", "listar_pasta", "memoria"]

esperado = {}
for l in io.open("dados/frases_teste.txt", encoding="utf-8"):
    l = l.strip()
    if not l or l.startswith("#") or "|" not in l:
        continue
    p = [x.strip() for x in l.split("|")]
    if len(p) >= 2:
        esperado[p[1]] = p[0]

def colher(padrao):
    # (frase, esperada) -> Counter de ferramenta escolhida
    fora = collections.defaultdict(collections.Counter)
    n = 0
    for f in glob.glob(padrao):
        t = io.open(f, encoding="utf-8", errors="replace").read()
        if "ferramenta certa:" not in t:
            continue
        n += 1
        for m in re.finditer(r'^\s*ERR\s+(.+?)\s{2,}\{"acao":"([a-z_]+)"', t, re.M):
            frase = m.group(1).strip()
            esp = esperado.get(frase)
            if esp:
                fora[(frase, esp)][m.group(2)] += 1
    return fora, n

base, nb = colher("cr_s*.log")     # 20 ferramentas
trat, nt = colher("tr22_s*.log")   # 22 ferramentas
print(f"sementes: 20f={nb}  22f={nt}\n")

for fam in ALVO:
    itens = [(fr, c) for (fr, e), c in trat.items() if e == fam]
    if not itens:
        continue
    itens.sort(key=lambda x: -sum(x[1].values()))
    total = sum(sum(c.values()) for _, c in itens)
    antes = sum(sum(c.values()) for (fr, e), c in base.items() if e == fam)
    print(f"=== {fam}   {antes} erros com 20f  ->  {total} com 22f   ({len(itens)} frases)")
    for fr, c in itens:
        n = sum(c.values())
        if n < 3:            # menos de 3 de 12 sementes e ruido, nao forma
            continue
        antes_n = sum(base[(fr, fam)].values())
        vai = ", ".join(f"{k}x{v}" for k, v in c.most_common(3))
        print(f"  {n:2d}/{nt}  (era {antes_n:2d})  {fr[:52]:<52} -> {vai}")
    print()
