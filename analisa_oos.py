"""Instrumento REGISTRADO em 07/09 e mantido em 08/09.

Frases de `perguntar` e `hora` que viram `atalho`, por semente, EXCLUINDO a linha
117 -- naquela o modelo esta certo e o gabarito e que envelheceu.

Base do braco `ct_20_*`: 20 erros em 12 sementes = 1,67 por semente.
"""
import re, glob, math, collections

MINA = "quero ouvir uma playlist relaxante"
esperado = {}
for l in open('dados/frases_teste.txt', encoding='utf-8'):
    l = l.strip()
    if not l or l.startswith('#') or '|' not in l: continue
    p = [x.strip() for x in l.split('|')]
    if len(p) >= 2: esperado[p[1]] = p[0]

def por_semente(padrao):
    fora = {}
    for f in glob.glob(padrao):
        s = int(re.search(r's(\d+)\.log$', f).group(1))
        t = open(f, encoding='utf-8', errors='replace').read()
        if 'ferramenta certa:' not in t: continue
        n = 0
        quais = collections.Counter()
        for m in re.finditer(r'^\s*ERR\s+(.+?)\s{2,}\{"acao":"atalho"', t, re.M):
            frase = m.group(1).strip()
            if frase == MINA: continue
            if esperado.get(frase) in ('perguntar', 'hora'):
                n += 1; quais[frase] += 1
        bench = int(re.search(r'ferramenta certa: (\d+)/150', t).group(1))
        fora[s] = (n, bench, quais)
    return fora

a = por_semente('ct_20_s*.log')   # base: com a contradicao
b = por_semente('oos_s*.log')     # tratado: sem a contradicao
pares = sorted(set(a) & set(b))

def pareado(f):
    d = [f(b[s]) - f(a[s]) for s in pares]
    n = len(d); m = sum(d)/n
    dp = math.sqrt(sum((x-m)**2 for x in d)/(n-1)) if n > 1 else 0.0
    t = m/(dp/math.sqrt(n)) if dp else float('nan')
    return m, t, sum(1 for x in d if x < 0)

print(f"pares: {len(pares)}  sementes {pares[0]}..{pares[-1]}\n")
print("  s   base  trat   dif      bench_base  bench_trat")
for s in pares:
    print(f" {s:3d}  {a[s][0]:4d}  {b[s][0]:4d}  {b[s][0]-a[s][0]:+4d}"
          f"        {a[s][1]:4d}        {b[s][1]:4d}")

m, t, melhor = pareado(lambda r: r[0])
print(f"\nINSTRUMENTO  falsos `atalho`   {m:+.2f} por semente   t={t:+.2f}   melhorou em {melhor}/{len(pares)}")
print(f"             base {sum(a[s][0] for s in pares)/len(pares):.2f}"
      f"  ->  tratado {sum(b[s][0] for s in pares)/len(pares):.2f}")
mb, tb, _ = pareado(lambda r: r[1])
print(f"GUARDA       benchmark de 150  {mb:+.2f}   t={tb:+.2f}")
print(f"             base {sum(a[s][1] for s in pares)/len(pares):.2f}"
      f"  ->  tratado {sum(b[s][1] for s in pares)/len(pares):.2f}")
ca = collections.Counter(); cb = collections.Counter()
for s in pares:
    ca.update(a[s][2]); cb.update(b[s][2])
print("\n  quais frases, base -> tratado")
for f in sorted(set(ca) | set(cb), key=lambda k: -(ca[k]+cb[k]))[:8]:
    print(f"    {ca[f]:2d} -> {cb[f]:2d}   [{esperado.get(f,'?')}] {f}")
