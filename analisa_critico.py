"""Passo 1 do critico: o instrumento REGISTRADO em 2026-09-08, antes de rodar.

  PRIMARIO   saldo do `critico` na tabela de separacao do benchmark.
             Base: +0 em 12 de 12 sementes -- a cabeca nunca era treinada.
             Se o passo funcionar, ele sai do zero.

  GUARDA     benchmark de 150. Treinar mais uma cabeca nao pode estragar as outras.
             Base (braco `oos_*`): 113,50.

Aqui o estreito PODE ser primario: e mudanca de CODIGO, com mecanismo conhecido, e
nao mudanca de dado. A regra de "registrar a medida ampla" nasceu de duas falhas em
mudanca de DADO, onde eu nao sei prever a superficie.

LINHA DE BASE: `oos_*`, o braco de 08/09. Decidido pelo John antes de qualquer
numero, porque o binario do fora-de-escopo foi compilado ANTES do critico --
comparar contra `ct_20_*` misturaria duas mudancas.
"""
import re
import glob
import math


def por_semente(padrao):
    fora = {}
    for f in glob.glob(padrao):
        s = int(re.search(r"s(\d+)\.log$", f).group(1))
        t = open(f, encoding="utf-8", errors="replace").read()
        if "ferramenta certa:" not in t:
            continue
        bench = int(re.search(r"ferramenta certa: (\d+)/150", t).group(1))
        m = re.search(r"critico\s+melhor limiar\s+(-?[\d.]+)\s+saldo\s+([+-]?\d+)", t)
        saldo = int(m.group(2)) if m else None
        limiar = float(m.group(1)) if m else None
        mg = re.search(r"margem\s+melhor limiar\s+(-?[\d.]+)\s+saldo\s+([+-]?\d+)", t)
        margem = int(mg.group(2)) if mg else None
        # As medias da tabela do critico: o quanto ele SEPARA, que e mais
        # informativo que o saldo quando o saldo e zero dos dois lados.
        sep = re.search(
            r"CRITICO V\(s\)\s*\n\s*margem media: acertos (-?[\d.]+)\s+erros (-?[\d.]+)", t
        )
        acertos = float(sep.group(1)) if sep else None
        erros = float(sep.group(2)) if sep else None
        fora[s] = dict(
            bench=bench, saldo=saldo, limiar=limiar, margem=margem,
            acertos=acertos, erros=erros,
        )
    return fora


def pareado(a, b, pares, campo):
    d = [b[s][campo] - a[s][campo] for s in pares
         if a[s][campo] is not None and b[s][campo] is not None]
    if not d:
        return None
    n = len(d)
    m = sum(d) / n
    dp = math.sqrt(sum((x - m) ** 2 for x in d) / (n - 1)) if n > 1 else 0.0
    t = m / (dp / math.sqrt(n)) if dp else float("nan")
    return m, t, n, sum(1 for x in d if x > 0)


base = por_semente("oos_s*.log")     # sem o passo 1
trat = por_semente("cr_s*.log")      # com o passo 1
pares = sorted(set(base) & set(trat))

if len(pares) < 12:
    print(f"so {len(pares)} pares fechados de 12 — n=12 foi REGISTRADO, nao leio parcial")
    raise SystemExit

print(f"pares: {len(pares)}  sementes {pares[0]}..{pares[-1]}\n")
print("  s   saldo_base saldo_trat   sep_trat   bench_base bench_trat")
for s in pares:
    a, b = base[s], trat[s]
    sep = (b["acertos"] - b["erros"]) if None not in (b["acertos"], b["erros"]) else float("nan")
    print(f" {s:3d}   {a['saldo']:>8}   {b['saldo']:>8}   {sep:>8.3f}"
          f"   {a['bench']:>9}  {b['bench']:>9}")

r = pareado(base, trat, pares, "saldo")
if r:
    m, t, n, subiu = r
    print(f"\nPRIMARIO  saldo do critico  {m:+.2f}   t={t:+.2f}   n={n}   subiu em {subiu}/{n}")
    print(f"          base {sum(base[s]['saldo'] for s in pares)/len(pares):+.2f}"
          f"  ->  tratado {sum(trat[s]['saldo'] for s in pares)/len(pares):+.2f}")

g = pareado(base, trat, pares, "bench")
if g:
    m, t, n, _ = g
    print(f"GUARDA    benchmark de 150   {m:+.2f}   t={t:+.2f}")
    print(f"          base {sum(base[s]['bench'] for s in pares)/len(pares):.2f}"
          f"  ->  tratado {sum(trat[s]['bench'] for s in pares)/len(pares):.2f}")

# A separacao crua importa mesmo quando o saldo nao move: saldo depende do
# compromisso entre erro evitado e acerto perdido, e separacao mede o SINAL.
sa = [trat[s]["acertos"] - trat[s]["erros"] for s in pares
      if None not in (trat[s]["acertos"], trat[s]["erros"])]
sb = [base[s]["acertos"] - base[s]["erros"] for s in pares
      if None not in (base[s]["acertos"], base[s]["erros"])]
if sa:
    print(f"\nSEPARACAO do critico (acertos - erros)")
    print(f"          base {sum(sb)/len(sb):+.4f}  ->  tratado {sum(sa)/len(sa):+.4f}")
    print("          base era 0,0000 por construcao: a cabeca nao era treinada")

mg = pareado(base, trat, pares, "margem")
if mg:
    m, t, _, _ = mg
    print(f"\nde controle: saldo da MARGEM {m:+.2f} (t={t:+.2f}) — nao devia mudar muito")
