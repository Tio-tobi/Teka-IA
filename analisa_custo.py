"""Le os dois bracos e faz o pareado por semente.

Braco de 19: cf_vb_s*.log (ja no disco, 41ab886)
Braco de 20: ct_20_s*.log (HEAD)

Sai com os dois numeros registrados: as 150 cruas (primario, penaliza o braco
novo por causa da linha 117 envelhecida) e as 149 sem ela (secundario).
"""
import re, glob, os, math

MINA = "quero ouvir uma playlist relaxante"

def ler(caminho):
    txt = open(caminho, encoding="utf-8", errors="replace").read()
    m = re.search(r"ferramenta certa:\s*(\d+)/(\d+)", txt)
    if not m:
        return None
    certas, total = int(m.group(1)), int(m.group(2))
    # A linha 117: se ela ficou ERR, tirar do total e do acerto muda o placar.
    errou_mina = bool(re.search(r"^\s*ERR\s+" + re.escape(MINA), txt, re.M))
    tempo = re.findall(r"^\s*\d+\s+[\d.]+\s+[\d.]+%\s+[\d.]+%\s+[\d.]+%\s+(\d+)s", txt, re.M)
    return {
        "certas": certas, "total": total,
        "sem_mina": (certas, total - 1) if errou_mina else (certas - 1, total - 1),
        "errou_mina": errou_mina,
        "seg": int(tempo[-1]) if tempo else None,
    }

def sementes(padrao):
    fora = {}
    for c in glob.glob(padrao):
        s = int(re.search(r"s(\d+)\.log$", c).group(1))
        r = ler(c)
        if r:
            fora[s] = r
    return fora

a = sementes("logs/cf_vb_s*.log")   # 19 ferramentas
b = sementes("logs/ct_20_s*.log")   # 20 ferramentas
pares = sorted(set(a) & set(b))

if not pares:
    print("braco de 20 ainda vazio — nada a parear")
    raise SystemExit

def pareado(f):
    d = [f(b[s]) - f(a[s]) for s in pares]
    n = len(d); m = sum(d) / n
    if n < 2:
        return m, float("nan"), n, 0
    dp = math.sqrt(sum((x - m) ** 2 for x in d) / (n - 1))
    t = m / (dp / math.sqrt(n)) if dp else float("inf")
    return m, t, n, sum(1 for x in d if x < 0)

print(f"pares: {len(pares)}  sementes {pares[0]}..{pares[-1]}\n")
print("  s    19f    20f   dif")
for s in pares:
    print(f" {s:3d}  {a[s]['certas']:5d}  {b[s]['certas']:5d}  {b[s]['certas']-a[s]['certas']:+4d}")

m, t, n, pior = pareado(lambda r: r["certas"])
print(f"\nPRIMARIO  150 cruas      {m:+.2f}  t={t:+.2f}  n={n}  pior em {pior}/{n}")
m2, t2, _, pior2 = pareado(lambda r: r["sem_mina"][0])
print(f"SECUNDARIO 149 sem a 117  {m2:+.2f}  t={t2:+.2f}  n={n}  pior em {pior2}/{n}")

acertou = sum(1 for s in pares if not b[s]["errou_mina"])
print(f"\na linha 117: o braco de 20 respondeu `perguntar` em {acertou}/{len(pares)}")

# TEMPO NAO E COMPARAVEL ENTRE OS BRACOS, e isto apareceu na epoca 1.
#
# O braco de 19 rodou de madrugada com a maquina vazia: epoca 1 em 155s. O de 20
# comecou com MIR4, REPO, Spotify e Discord no ar, CPU a 83%: epoca 1 em 516s.
# 3,3x -- grande demais para ser uma ferramenta, e explicado inteiro pela carga.
#
# Os bracos rodaram em DIAS diferentes, sob cargas diferentes. Qualquer diferenca
# de tempo aqui e da maquina, nao do modelo. O desenho alternado do `exp_confirma`
# existia exatamente para isto, e aqui nao da para aplicar: metade da corrida ja
# estava paga, e o preco de aproveita-la e perder o eixo do tempo.
#
# A ACURACIA continua valendo: e determinista dada a semente, nao a carga.
#
# Para medir custo EM TEMPO seria preciso alternar os dois binarios na mesma
# sessao, sob a mesma carga -- corrida separada, nao esta.
sa = [a[s]["seg"] for s in pares if a[s]["seg"]]
sb = [b[s]["seg"] for s in pares if b[s]["seg"]]
if sa and sb:
    print()
    print(f"tempo    19f {sum(sa)/len(sa):6.0f}s   20f {sum(sb)/len(sb):6.0f}s"
          "   -- NAO COMPARAVEL, cargas de maquina diferentes")
