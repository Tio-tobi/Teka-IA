# -*- coding: utf-8 -*-
"""Roda as 500 frases do John contra um .bin e reporta por secao.

A REGUA INDEPENDENTE. Existe porque quem escreve o treino da Teka e quem escreve
`dados/frases_teste.txt` e a mesma cabeca, e isso ja escondeu um defeito grande:
treino e benchmark tinham 0% de maiuscula, 0% de acento e 0% de "?", enquanto o
texto que uma pessoa digita de verdade tem 100%, 60% e 21%.

Tres grupos, medidos separados porque medem coisas diferentes:

  SEM ARGUMENTO   hora, memoria, disco             teste puro de intencao
  COM ALVO        as frases que nomeiam o objeto   intencao + ponteiro
  SEM ALVO        "le esse arquivo" (sem qual)     abstencao: verbo certo, objeto ausente

Do terceiro eu previ que iria mal, porque o treino nao tem exemplo dessa forma
(a familia "vago" que existe nao tem verbo de ferramenta: "faz aquilo la"). Fez
245/265 = 92,5%. A cabeca de presenca resolve sozinha: sem trecho para o ponteiro
copiar, ela abstem. Previsao errada, mecanismo certo.

Cuidado ao ler esse numero: maiuscula tambem empurra para `perguntar` — foi assim
que "Quanta RAM ta usando?" quebrou. Como a resposta certa do grupo "sem alvo" E
`perguntar`, ele pode acertar pelo motivo errado. Por isso rode nos dois modos e
compare: se `--normalizado` derrubar o grupo, o acerto era artefato.

  python medir_john.py <modelo.bin> [--cru | --normalizado]
"""
import io
import os
import re
import shutil
import subprocess
import sys
import tempfile
import unicodedata

EXE = os.path.join(".", "target", "release", "teka.exe")
FONTE = os.path.join("dados", "frases_john_bruto.txt")

SEM_ARGUMENTO = {"hora", "memoria", "disco", "perguntar"}
CMDS = ["ipconfig", "tasklist", "dir", "hostname", "echo", "ver", "date",
        "whoami", "ping", "cls"]
PASTAS_CONHECIDAS = ["downloads", "documentos", "projetos", "dados", "src",
                     "temp", "desktop", "windows", "system32"]


def sem_acento(t):
    t = unicodedata.normalize("NFD", t.lower())
    return "".join(c for c in t if unicodedata.category(c) != "Mn")


def normalizar(t):
    return re.sub(r"[?!.,;:]", "", sem_acento(t)).strip()


def carregar():
    secoes, atual = {}, None
    for ln in io.open(FONTE, encoding="utf-8"):
        ln = ln.rstrip("\n")
        if ln.startswith("## "):
            atual = ln[3:].strip()
            secoes[atual] = []
        elif ln.strip() and not ln.startswith("#") and atual:
            secoes[atual].append(ln.strip())
    return secoes


def tem_alvo(sec, frase):
    n = sem_acento(frase)
    if sec == "escrever_arquivo":
        # Precisa de DOIS argumentos: caminho e texto. As frases do John dizem o
        # arquivo ("salva isso em dados.txt") mas nunca o texto literal — "isso",
        # "esse texto", "essa mensagem". Sem o texto nao ha o que o ponteiro copie,
        # entao o pedido esta incompleto e a resposta certa e `perguntar`.
        #
        # A versao anterior olhava so o ".txt" e classificava 5 frases como
        # completas. O modelo respondia `perguntar` — corretamente — e eu contava
        # 0/5 como erro dele. Era erro do medidor.
        tem_arquivo = bool(re.search(r"[\w-]+\.(txt|json|md|csv|log|ini)", n))
        tem_texto = '"' in frase or "'" in frase
        return tem_arquivo and tem_texto
    if sec == "calcular":
        return bool(re.search(r"\d", n))
    if sec == "executar_comando":
        return any(c in n for c in CMDS)
    if re.search(r"\b[\w-]+\.(txt|json|md|csv|log|rs|toml|pdf|ini|py)\b", n):
        return True
    return bool(re.search(r"\b(" + "|".join(PASTAS_CONHECIDAS) + r")\b", n))


VIES = "0.0"


def acao(exe, modelo, frase):
    r = subprocess.run([exe, "agente", "--carregar", modelo, "--pedido", frase,
                        "--vies-abster", VIES],
                       capture_output=True, text=True, encoding="utf-8",
                       errors="replace")
    m = re.search(r'"acao"\s*:\s*"([a-z_]+)"', r.stdout or "")
    return m.group(1) if m else "?"


def main():
    if len(sys.argv) < 2:
        print(__doc__)
        return 1
    modelo = sys.argv[1]
    modo = sys.argv[2] if len(sys.argv) > 2 else "--cru"
    # Terceiro argumento: vies de abstencao. Negativo faz duvidar menos.
    global VIES
    if len(sys.argv) > 3:
        VIES = sys.argv[3]
    prep = normalizar if modo == "--normalizado" else (lambda t: t)

    # O modo --pedido salva de volta no .bin carregado. Trabalha numa copia, senao
    # medir altera o que esta sendo medido.
    tmp = os.path.join(tempfile.gettempdir(), "teka_medindo.bin")
    shutil.copy(modelo, tmp)

    secoes = carregar()
    grupos = {"sem argumento": [0, 0], "com alvo": [0, 0], "sem alvo": [0, 0]}
    print("modelo: %s   modo: %s\n" % (modelo, modo))
    print("%-18s %-14s %-14s %s" % ("secao", "sem argumento", "com alvo", "sem alvo"))
    print("-" * 62)

    for sec, frases in secoes.items():
        linha = {"sem argumento": [0, 0], "com alvo": [0, 0], "sem alvo": [0, 0]}
        for f in frases:
            # Sem contexto anterior, pedido sem objeto so pode virar `perguntar`.
            if sec in SEM_ARGUMENTO:
                g, esperado = "sem argumento", sec
            elif tem_alvo(sec, f):
                g, esperado = "com alvo", sec
            else:
                g, esperado = "sem alvo", "perguntar"
            got = acao(EXE, tmp, prep(f))
            linha[g][1] += 1
            linha[g][0] += (got == esperado)

        def cel(g):
            a, b = linha[g]
            return "%d/%d" % (a, b) if b else "-"
        print("%-18s %-14s %-14s %s" % (sec, cel("sem argumento"),
                                        cel("com alvo"), cel("sem alvo")))
        for g in grupos:
            grupos[g][0] += linha[g][0]
            grupos[g][1] += linha[g][1]

    print("-" * 62)
    total_ok = total_n = 0
    for g, (a, b) in grupos.items():
        if b:
            print("  %-16s %3d/%-4d  %5.1f%%" % (g, a, b, 100.0 * a / b))
        total_ok += a
        total_n += b
    print("  %-16s %3d/%-4d  %5.1f%%" % ("TOTAL", total_ok, total_n,
                                         100.0 * total_ok / total_n))
    os.remove(tmp)
    return 0


if __name__ == "__main__":
    sys.exit(main())
