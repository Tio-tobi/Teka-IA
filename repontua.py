# -*- coding: utf-8 -*-
"""Repontua um braco JA RODADO sob os rotulos de HOJE, sem treinar de novo.

Rerrotular 10 frases mudaria o significado de todo numero medido sobre as 329 --
216,58 e 217,17 passariam a nao ser comparaveis com o que vier depois. Seriam mais
16 horas de CPU para recuperar o que ja esta no disco.

Mas o log de cada semente guarda a DECISAO frase a frase. O modelo nao muda quando o
rotulo muda; so a correcao muda. Entao da para repontuar.

O QUE ISTO NAO CONSERTA: se as frases mudarem de TEXTO, ou se o registro mudar, o
modelo teria decidido outra coisa e repontuar seria mentira. Vale so para mudanca de
ROTULO sobre o mesmo texto e o mesmo registro.
"""
import re, glob, io, sys, math, collections

def rotulos():
    d = {}
    for l in io.open("dados/frases_teste.txt", encoding="utf-8"):
        l = l.strip()
        if not l or l.startswith("#") or "|" not in l:
            continue
        p = [x.strip() for x in l.split("|")]
        if len(p) >= 2:
            d[p[1]] = p[0]
    return d

def pontuar(padrao, rot):
    fora = {}
    for f in glob.glob(padrao):
        s = int(re.search(r"s(\d+)\.log$", f).group(1))
        if not 19 <= s <= 30:
            continue
        t = io.open(f, encoding="utf-8", errors="replace").read()
        if "ferramenta certa:" not in t:
            continue
        ok = n = 0
        vistas = set()
        # CASA POR PREFIXO, e nao por regex frouxo.
        #
        # O log formata com `{:<48}`: frase de 48 caracteres ou mais NAO recebe
        # preenchimento, e sobra UM espaco antes do JSON em vez de dois. Meu regex
        # pedia `\s{2,}` e perdia 31 frases -- justamente as mais longas, que sao as
        # do John. Pontuava 298 de 329 e inflava o percentual.
        for linha in t.splitlines():
            l = linha.strip()
            if l.startswith("ok "):
                resto = l[3:].lstrip()
            elif l.startswith("ERR "):
                resto = l[4:].lstrip()
            else:
                continue
            # A frase mais LONGA que casa o inicio: "lista {0}" e prefixo de
            # "lista os arquivos de src", e pegar a curta daria o rotulo errado.
            frase = None
            for f in rot:
                if resto.startswith(f) and (frase is None or len(f) > len(frase)):
                    frase = f
            if frase is None or frase in vistas:
                continue
            vistas.add(frase)
            n += 1
            cauda = resto[len(frase):]
            m2 = re.search(r'\{"acao":"([a-z_]+)"', cauda)
            if m2 and rot[frase] == m2.group(1):
                ok += 1
        fora[s] = (ok, n)
    return fora

def media_sd(v):
    n = len(v); m = sum(v) / n
    sd = math.sqrt(sum((x - m) ** 2 for x in v) / (n - 1)) if n > 1 else 0.0
    return m, sd

rot = rotulos()
print(f"  {len(rot)} frases rotuladas hoje\n")
for nome, pad in [a.split("=") for a in sys.argv[1:]]:
    d = pontuar(pad, rot)
    if not d:
        print(f"  {nome:<14} (nenhum log)"); continue
    vals = [ok for ok, _ in d.values()]
    tot = max(n for _, n in d.values())
    m, sd = media_sd(vals)
    print(f"  {nome:<14} {m:7.2f}/{tot}  = {100*m/tot:5.1f}%   sd {sd:5.2f}   n={len(vals)}")
