import re, glob, collections
esperado = {}
for l in open('dados/frases_teste.txt', encoding='utf-8'):
    l = l.strip()
    if not l or l.startswith('#') or '|' not in l: continue
    p = [x.strip() for x in l.split('|')]
    if len(p) >= 2: esperado[p[1]] = p[0]

def erros(padrao):
    c = collections.Counter(); n = 0
    for f in glob.glob(padrao):
        txt = open(f, encoding='utf-8', errors='replace').read()
        if 'ferramenta certa:' not in txt: continue
        n += 1
        for m in re.finditer(r'^\s*ERR\s+(.+?)\s{2,}\{"acao":"([a-z_]+)"', txt, re.M):
            frase, obtido = m.group(1).strip(), m.group(2)
            esp = esperado.get(frase)
            if esp: c[(esp, obtido)] += 1
    return c, n

a, na = erros('logs/cf_vb_s*.log')
b, nb = erros('logs/ct_20_s*.log')
print(f"corridas lidas: 19f={na}  20f={nb}\n")
chaves = set(a) | set(b)
piora = sorted(chaves, key=lambda k: (b[k]-a[k]), reverse=True)
print("  ONDE O BRACO DE 20 PIOROU  (erros somados nas 12 sementes)")
print(f"  {'esperado':<18} {'virou':<18} {'19f':>4} {'20f':>4} {'dif':>5}")
for k in piora[:10]:
    d = b[k]-a[k]
    if d <= 0: break
    print(f"  {k[0]:<18} {k[1]:<18} {a[k]:>4} {b[k]:>4} {d:>+5}")
print("\n  ONDE MELHOROU")
for k in sorted(chaves, key=lambda k: (b[k]-a[k]))[:6]:
    d = b[k]-a[k]
    if d >= 0: break
    print(f"  {k[0]:<18} {k[1]:<18} {a[k]:>4} {b[k]:>4} {d:>+5}")
print("\n  virou `atalho` indevidamente:", sum(v for k,v in b.items() if k[1]=='atalho'),
      " (19f nao tinha a ferramenta)")
