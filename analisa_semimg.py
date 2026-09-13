"""O custo das duas ferramentas da ponte — o instrumento REGISTRADO em `exp_22.sh`.

  PRIMARIO   `ferramenta certa` de 150, pareado por semente.
             Base: braco `tr22_*` (poco quebrado), 113,42. Pareavel -- MEDIDO pelo
             controle positivo: 117/150 identico nos dois binarios.
             `frases_teste.txt` NAO mudou -- a regua e a mesma dos dois lados.

  SECUNDARIO onde o erro caiu, por ferramenta ESPERADA. Separa "custo difuso" de
             "uma ferramenta especifica foi roubada". A suspeita foi nomeada antes:
             a familia de CAMINHO (`escrever_arquivo`, `listar_pasta`,
             `ler_arquivo`, `procurar_arquivo`). Se subir FORA dela, o mecanismo
             escrito em `exp_33.sh` esta errado.

  n = 12, FIXADO. Nao ler parcial.

RESSALVA APRENDIDA EM 10/09, e ela vale mais que o numero: esta regua mede ACURACIA.
O custo das mesmas duas ferramentas apareceu na GEOMETRIA DA ASSINATURA -- cosseno do
episodio certo caindo de 0,382 para 0,158 -- com a acuracia praticamente identica
(53,9% contra 54,0%). Um primario verde aqui NAO quer dizer "nao custou nada"; quer
dizer "nao custou em acuracia". Ver a entrada de 2026-09-10 no ROTEIRO.
"""
import re, glob, math, collections

N_REGISTRADO = 12

def por_semente(padrao):
    fora = {}
    for f in glob.glob(padrao):
        s = int(re.search(r"s(\d+)\.log$", f).group(1))
        t = open(f, encoding="utf-8", errors="replace").read()
        if "ferramenta certa:" not in t:
            continue
        m = re.search(r"ferramenta certa:\s*(\d+)/(\d+)", t)
        arg = re.search(r"argumento quando a ferramenta saiu certa:\s*([\d.]+)%", t)
        fora[s] = {
            "certas": int(m.group(1)),
            "total": int(m.group(2)),
            "arg": float(arg.group(1)) if arg else None,
            "txt": t,
        }
    return fora

def pareado(a, b, pares, campo):
    d = [b[s][campo] - a[s][campo] for s in pares
         if a[s].get(campo) is not None and b[s].get(campo) is not None]
    if not d:
        return None
    n = len(d)
    m = sum(d) / n
    dp = math.sqrt(sum((x - m) ** 2 for x in d) / (n - 1)) if n > 1 else 0.0
    t = m / (dp / math.sqrt(n)) if dp else float("nan")
    return m, t, n, sum(1 for x in d if x > 0), sum(1 for x in d if x < 0)

esperado = {}
for l in open("dados/frases_teste.txt", encoding="utf-8"):
    l = l.strip()
    if not l or l.startswith("#") or "|" not in l:
        continue
    p = [x.strip() for x in l.split("|")]
    if len(p) >= 2:
        esperado[p[1]] = p[0]

def por_ferramenta(dados, pares):
    c = collections.Counter()
    for s in pares:
        for m in re.finditer(r'^\s*ERR\s+(.+?)\s{2,}\{"acao":"([a-z_]+)"', dados[s]["txt"], re.M):
            esp = esperado.get(m.group(1).strip())
            if esp:
                c[esp] += 1
    return c

base = por_semente("logs/pasta_s*.log")    # com ler_imagem
trat = por_semente("logs/semimg_s*.log")   # poco consertado
pares = sorted(set(base) & set(trat))

print(f"base `tr22_*` (poco quebrado): {len(base)} | tratado `tr33_*` (consertado): {len(trat)} | pares: {len(pares)}")
if len(pares) < N_REGISTRADO:
    print(f"\nso {len(pares)} pares de {N_REGISTRADO} — n foi REGISTRADO, nao leio parcial.")
    raise SystemExit

print(f"sementes {pares[0]}..{pares[-1]}\n")
print("   s   20f   22f   delta")
for s in pares:
    d = trat[s]["certas"] - base[s]["certas"]
    print(f" {s:3d}  {base[s]['certas']:4d}  {trat[s]['certas']:4d}   {d:+3d}")

r = pareado(base, trat, pares, "certas")
m, t, n, subiu, caiu = r
mb = sum(base[s]["certas"] for s in pares) / n
mt = sum(trat[s]["certas"] for s in pares) / n
print(f"\nPRIMARIO  ferramenta certa de 329")
print(f"          {mb:.2f}  ->  {mt:.2f}   delta {m:+.2f}   t={t:+.2f}   n={n}")
print(f"          subiu em {subiu}, caiu em {caiu}, empatou em {n - subiu - caiu}")
print(f"          chao de ruido do benchmark: 1,2  (ver [[teka-chao-de-ruido]])")

ra = pareado(base, trat, pares, "arg")
if ra:
    m2, t2, _, _, _ = ra
    print(f"\n          argumento condicional  {m2:+.2f} pontos  (t={t2:+.2f})")

eb, et = por_ferramenta(base, pares), por_ferramenta(trat, pares)
print(f"\nSECUNDARIO  onde o erro caiu, por ferramenta ESPERADA (soma das {n} sementes)")
print(f"  {'ferramenta':<22} {'20f':>5} {'22f':>5} {'delta':>7}")
for k in sorted(set(eb) | set(et), key=lambda k: et[k] - eb[k], reverse=True):
    d = et[k] - eb[k]
    if d == 0 and eb[k] == 0:
        continue
    marca = "  <- familia de caminho, a esperada" if k in ("copiar_arquivo", "mover_arquivo", "listar_pasta", "criar_pasta") else ""
    print(f"  {k:<22} {eb[k]:>5} {et[k]:>5} {d:>+7}{marca}")

print("\nRESSALVA: isto mede ACURACIA. O custo destas mesmas duas ferramentas apareceu")
print("na geometria da assinatura (cos 0,382 -> 0,158) com acuracia IGUAL. Verde aqui")
print("nao e 'nao custou'; e 'nao custou em acuracia'. Ver ROTEIRO, 2026-09-10.")
